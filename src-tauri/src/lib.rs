//! AI Balance: track AI subscriptions and usage limits.
//! Copyright (c) 2026 Karim Seyam. MIT License.

mod http;
mod login;
mod model;
mod providers;
mod store;

use std::collections::{HashMap, HashSet};
use std::sync::Mutex;
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::sync::mpsc;
use tokio::task::JoinSet;

use login::LoginPrompt;
use model::{now_ms, Account, Credential, MetricLine, ProviderError, ProviderKind, Settings, Snapshot};
use store::Store;

const STATE_EVENT: &str = "state-changed";
const LOGIN_EVENT: &str = "login-finished";
/// How long a failed account waits before the loop reads it again, by consecutive failures.
const BACKOFF_SECS: [i64; 5] = [60, 120, 300, 900, 1800];

struct LoginSession {
    pasted: mpsc::Sender<String>,
    task: tauri::async_runtime::JoinHandle<()>,
}

#[derive(Default)]
struct Failure {
    count: usize,
    retry_at: i64,
}

struct AppState {
    store: Store,
    http: reqwest::Client,
    snapshots: Mutex<HashMap<String, Snapshot>>,
    failures: Mutex<HashMap<String, Failure>>,
    /// Limits already notified about, so each one notifies once per window.
    notified: Mutex<HashSet<String>>,
    logins: Mutex<HashMap<String, LoginSession>>,
    /// Held for a whole refresh pass so two passes never overlap.
    refreshing: tokio::sync::Mutex<()>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct ViewState {
    accounts: Vec<Account>,
    snapshots: HashMap<String, Snapshot>,
    settings: Settings,
    mobile: bool,
    version: &'static str,
}

fn view(state: &AppState) -> ViewState {
    ViewState {
        accounts: state.store.accounts(),
        snapshots: state.snapshots.lock().unwrap().clone(),
        settings: state.store.settings(),
        mobile: cfg!(mobile),
        version: env!("CARGO_PKG_VERSION"),
    }
}

fn emit_state(app: &AppHandle) {
    let state = app.state::<AppState>();
    let _ = app.emit(STATE_EVENT, view(&state));
    update_tray(app, &state);
}

/// Reads one account and returns its new snapshot. A failure keeps the last usage that was read.
async fn refresh_account(state: &AppState, account: &Account) -> Snapshot {
    let previous = state.snapshots.lock().unwrap().get(&account.id).cloned();
    let failed = |error: ProviderError| Snapshot {
        account_id: account.id.clone(),
        plan: previous.as_ref().and_then(|snapshot| snapshot.plan.clone()),
        lines: previous.as_ref().map(|snapshot| snapshot.lines.clone()).unwrap_or_default(),
        fetched_at: previous.as_ref().and_then(|snapshot| snapshot.fetched_at),
        needs_login: error.needs_login(),
        error: Some(error.to_string()),
    };
    let Some(mut credential) = state.store.credential(&account.id) else {
        return failed(ProviderError::SignInExpired);
    };
    let before = credential.clone();
    let result = providers::fetch(&state.http, account.provider, &mut credential).await;
    if credential != before {
        state.store.set_credential(&account.id, &credential);
    }
    let mut failures = state.failures.lock().unwrap();
    match result {
        Ok(usage) => {
            failures.remove(&account.id);
            Snapshot {
                account_id: account.id.clone(),
                plan: usage.plan,
                lines: usage.lines,
                fetched_at: Some(now_ms()),
                error: None,
                needs_login: false,
            }
        }
        Err(error) => {
            let failure = failures.entry(account.id.clone()).or_default();
            let backoff = BACKOFF_SECS[failure.count.min(BACKOFF_SECS.len() - 1)];
            let wait = match &error {
                ProviderError::RateLimited { retry_after_secs: Some(seconds) } => backoff.max(*seconds as i64),
                _ => backoff,
            };
            failure.count += 1;
            failure.retry_at = now_ms() + wait * 1000;
            failed(error)
        }
    }
}

/// Reads every account that is due. `force` reads them all, whatever their age or backoff.
async fn refresh_all(app: &AppHandle, force: bool) {
    let state = app.state::<AppState>();
    let _pass = state.refreshing.lock().await;
    let interval_ms = state.store.settings().refresh_minutes as i64 * 60_000;
    let now = now_ms();
    let due: Vec<Account> = state
        .store
        .accounts()
        .into_iter()
        .filter(|account| {
            if force {
                return true;
            }
            if state.failures.lock().unwrap().get(&account.id).is_some_and(|failure| failure.retry_at > now) {
                return false;
            }
            let snapshots = state.snapshots.lock().unwrap();
            match snapshots.get(&account.id) {
                // A snapshot holding an error was not a read; its `fetched_at` is the last good one.
                Some(snapshot) if snapshot.error.is_none() => snapshot.fetched_at.map_or(true, |at| now - at >= interval_ms),
                _ => true,
            }
        })
        .collect();
    if due.is_empty() {
        return;
    }
    let mut reads = JoinSet::new();
    for account in due {
        let app = app.clone();
        reads.spawn(async move {
            let state = app.state::<AppState>();
            refresh_account(&state, &account).await
        });
    }
    while let Some(Ok(snapshot)) = reads.join_next().await {
        // The account may have been removed while it was being read.
        if state.store.account(&snapshot.account_id).is_some() {
            state.snapshots.lock().unwrap().insert(snapshot.account_id.clone(), snapshot);
        }
    }
    let snapshots = state.snapshots.lock().unwrap().clone();
    state.store.save_snapshots(&snapshots);
    notify_limits(app, &state);
    emit_state(app);
}

/// The fullest limits across every account, fullest first.
fn top_limits(state: &AppState) -> Vec<(String, f64)> {
    let snapshots = state.snapshots.lock().unwrap();
    let mut limits: Vec<(String, f64)> = state
        .store
        .accounts()
        .iter()
        .filter_map(|account| snapshots.get(&account.id).map(|snapshot| (account, snapshot)))
        .flat_map(|(account, snapshot)| {
            snapshot.lines.iter().filter_map(move |line| {
                line.used_percent().map(|percent| (format!("{} {}", account.provider.display_name(), line.label()), percent))
            })
        })
        .collect();
    limits.sort_by(|a, b| b.1.total_cmp(&a.1));
    limits
}

fn notify_limits(app: &AppHandle, state: &AppState) {
    use tauri_plugin_notification::NotificationExt;
    let settings = state.store.settings();
    let threshold = settings.notify_threshold as f64;
    let accounts = state.store.accounts();
    let snapshots = state.snapshots.lock().unwrap().clone();
    let mut notified = state.notified.lock().unwrap();
    let mut current = HashSet::new();
    for account in &accounts {
        let Some(snapshot) = snapshots.get(&account.id) else { continue };
        for line in &snapshot.lines {
            let MetricLine::Progress { label, resets_at, .. } = line else { continue };
            let Some(percent) = line.used_percent().filter(|percent| *percent >= threshold) else { continue };
            let key = format!("{}/{}/{}", account.id, label, resets_at.unwrap_or(0));
            if settings.notifications && !notified.contains(&key) {
                let title = format!("{} · {}", account.provider.display_name(), label);
                let body = format!("{:.0}% used ({})", percent, account.label);
                let _ = app.notification().builder().title(title).body(body).show();
            }
            current.insert(key);
        }
    }
    *notified = current;
}

// ----- commands -----

#[tauri::command]
fn get_state(state: State<'_, AppState>) -> ViewState {
    view(&state)
}

#[tauri::command]
async fn refresh(app: AppHandle) -> Result<(), String> {
    refresh_all(&app, true).await;
    Ok(())
}

fn save_account(app: &AppHandle, provider: ProviderKind, label: String, credential: Credential) -> Result<Account, String> {
    let state = app.state::<AppState>();
    let account = state.store.upsert_account(Account { id: login::new_id(), provider, label, created_at: now_ms() });
    if !state.store.set_credential(&account.id, &credential) {
        state.store.remove_account(&account.id);
        return Err("Could not save the credential on this device.".into());
    }
    state.failures.lock().unwrap().remove(&account.id);
    Ok(account)
}

/// Adds an API-key account after one read proves the key works.
#[tauri::command]
async fn add_api_key_account(app: AppHandle, provider: ProviderKind, key: String, label: Option<String>) -> Result<Account, String> {
    let key = key.trim().to_string();
    if key.is_empty() || !provider.uses_api_key() {
        return Err("Enter an API key.".into());
    }
    let state = app.state::<AppState>();
    let mut credential = Credential::ApiKey { key: key.clone() };
    let usage = providers::fetch(&state.http, provider, &mut credential).await.map_err(|error| error.to_string())?;
    let mut name = label.map(|label| label.trim().to_string()).filter(|label| !label.is_empty());
    if name.is_none() && provider == ProviderKind::Ollama {
        name = providers::ollama::account(&state.http, &key).await.and_then(|body| providers::ollama::field(&body, "Email"));
    }
    let name = name.unwrap_or_else(|| {
        let taken = state.store.accounts().iter().filter(|account| account.provider == provider).count();
        if taken == 0 {
            provider.display_name().to_string()
        } else {
            format!("{} {}", provider.display_name(), taken + 1)
        }
    });
    let account = save_account(&app, provider, name, credential)?;
    let snapshot = Snapshot {
        account_id: account.id.clone(),
        plan: usage.plan,
        lines: usage.lines,
        fetched_at: Some(now_ms()),
        error: None,
        needs_login: false,
    };
    state.snapshots.lock().unwrap().insert(account.id.clone(), snapshot);
    state.store.save_snapshots(&state.snapshots.lock().unwrap().clone());
    emit_state(&app);
    Ok(account)
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct LoginFinished {
    session_id: String,
    account: Option<Account>,
    error: Option<String>,
}

#[tauri::command]
async fn begin_login(app: AppHandle, provider: ProviderKind) -> Result<LoginPrompt, String> {
    let state = app.state::<AppState>();
    let (prompt, pending) = login::start(&state.http, provider, cfg!(mobile)).await?;
    let (pasted, pasted_codes) = mpsc::channel(4);
    let session_id = prompt.session_id.clone();
    let task_app = app.clone();
    let task = tauri::async_runtime::spawn(async move {
        let state = task_app.state::<AppState>();
        let outcome = login::finish(&state.http, pending, pasted_codes)
            .await
            .and_then(|(label, credential)| save_account(&task_app, provider, label, credential));
        state.logins.lock().unwrap().remove(&session_id);
        let (account, error) = match outcome {
            Ok(account) => (Some(account), None),
            Err(error) => (None, Some(error)),
        };
        let _ = task_app.emit(LOGIN_EVENT, LoginFinished { session_id, account, error });
        emit_state(&task_app);
        refresh_all(&task_app, false).await;
    });
    state.logins.lock().unwrap().insert(prompt.session_id.clone(), LoginSession { pasted, task });
    Ok(prompt)
}

#[tauri::command]
async fn submit_login_code(state: State<'_, AppState>, session_id: String, code: String) -> Result<(), String> {
    let pasted = state.logins.lock().unwrap().get(&session_id).map(|session| session.pasted.clone());
    match pasted {
        Some(pasted) => pasted.send(code).await.map_err(|_| "This sign-in has ended. Start again.".to_string()),
        None => Err("This sign-in has ended. Start again.".into()),
    }
}

#[tauri::command]
fn cancel_login(state: State<'_, AppState>, session_id: String) {
    if let Some(session) = state.logins.lock().unwrap().remove(&session_id) {
        session.task.abort();
    }
}

#[tauri::command]
fn remove_account(app: AppHandle, state: State<'_, AppState>, id: String) {
    state.store.remove_account(&id);
    state.snapshots.lock().unwrap().remove(&id);
    state.failures.lock().unwrap().remove(&id);
    state.store.save_snapshots(&state.snapshots.lock().unwrap().clone());
    emit_state(&app);
}

#[tauri::command]
fn rename_account(app: AppHandle, state: State<'_, AppState>, id: String, label: String) -> Result<(), String> {
    let label = label.trim();
    if label.is_empty() {
        return Err("Enter a name.".into());
    }
    state.store.rename_account(&id, label);
    emit_state(&app);
    Ok(())
}

#[tauri::command]
fn move_account(app: AppHandle, state: State<'_, AppState>, id: String, offset: i32) {
    state.store.move_account(&id, offset);
    emit_state(&app);
}

#[tauri::command]
fn set_settings(app: AppHandle, state: State<'_, AppState>, settings: Settings) -> Settings {
    let saved = state.store.set_settings(settings);
    emit_state(&app);
    saved
}

// ----- tray (desktop) -----

#[cfg(desktop)]
fn show_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

#[cfg(desktop)]
fn build_tray(app: &AppHandle) -> tauri::Result<()> {
    use tauri::menu::{Menu, MenuItem};
    use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};

    let show = MenuItem::with_id(app, "show", "Show AI Balance", true, None::<&str>)?;
    let refresh = MenuItem::with_id(app, "refresh", "Refresh", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &refresh, &quit])?;
    let mut tray = TrayIconBuilder::with_id("main")
        .tooltip("AI Balance")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => show_window(app),
            "refresh" => {
                let app = app.clone();
                tauri::async_runtime::spawn(async move { refresh_all(&app, true).await });
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                show_window(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.build(app)?;
    Ok(())
}

/// Puts the three fullest limits in the tray icon's tooltip.
#[cfg(desktop)]
fn update_tray(app: &AppHandle, state: &AppState) {
    let Some(tray) = app.tray_by_id("main") else { return };
    let limits = top_limits(state);
    let tooltip = if limits.is_empty() {
        "AI Balance".to_string()
    } else {
        limits.iter().take(3).map(|(name, percent)| format!("{name}: {percent:.0}%")).collect::<Vec<_>>().join("\n")
    };
    let _ = tray.set_tooltip(Some(tooltip));
}

#[cfg(mobile)]
fn update_tray(_: &AppHandle, state: &AppState) {
    let _ = top_limits(state);
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let mut builder = tauri::Builder::default();
    #[cfg(desktop)]
    {
        // A second launch shows the running app's window. The first plugin registered must be this one.
        builder = builder
            .plugin(tauri_plugin_single_instance::init(|app, _, _| show_window(app)))
            .plugin(tauri_plugin_autostart::init(tauri_plugin_autostart::MacosLauncher::LaunchAgent, Some(vec!["--hidden"])));
    }
    builder
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
        .setup(|app| {
            let dir = app.path().app_data_dir()?;
            let store = Store::open(dir);
            let snapshots = store.load_snapshots();
            app.manage(AppState {
                store,
                http: http::client(),
                snapshots: Mutex::new(snapshots),
                failures: Mutex::default(),
                notified: Mutex::default(),
                logins: Mutex::default(),
                refreshing: tokio::sync::Mutex::new(()),
            });
            #[cfg(desktop)]
            {
                if let Err(error) = build_tray(app.handle()) {
                    eprintln!("aibalance: no tray icon: {error}");
                }
                // Started at login: stay in the tray until asked for.
                if std::env::args().any(|argument| argument == "--hidden") && app.tray_by_id("main").is_some() {
                    if let Some(window) = app.get_webview_window("main") {
                        let _ = window.hide();
                    }
                }
                update_tray(app.handle(), &app.state::<AppState>());
            }
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                loop {
                    refresh_all(&handle, false).await;
                    tokio::time::sleep(Duration::from_secs(30)).await;
                }
            });
            Ok(())
        })
        .on_window_event(|window, event| {
            // Closing the window keeps the app in the tray, when there is a tray to bring it back from.
            #[cfg(desktop)]
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                if window.app_handle().tray_by_id("main").is_some() {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
            #[cfg(mobile)]
            let _ = (window, event);
        })
        .invoke_handler(tauri::generate_handler![
            get_state,
            refresh,
            add_api_key_account,
            begin_login,
            submit_login_code,
            cancel_login,
            remove_account,
            rename_account,
            move_account,
            set_settings
        ])
        .run(tauri::generate_context!())
        .expect("error while running AI Balance");
}
