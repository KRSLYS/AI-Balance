//! Sign-in flows. `start` opens a flow and says what to show the user; `finish` waits for the
//! provider and returns the account's name and credential.

use std::time::Duration;

use base64::Engine;
use rand::RngCore;
use reqwest::Client;
use serde::Serialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use tokio::time::{sleep, timeout, Instant};

use crate::http::{self, Resp};
use crate::model::{Credential, ProviderKind};
use crate::providers::{amp, claude, codex, copilot, cursor, devin, grok, kimi, muse, opencode};

const BROWSER_SIGN_IN_TIMEOUT: Duration = Duration::from_secs(10 * 60);

pub type LoginResult<T> = std::result::Result<T, String>;

/// What the sign-in sheet shows.
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct LoginPrompt {
    pub session_id: String,
    /// The page to open in the browser.
    pub url: String,
    /// A device code to type on that page.
    pub user_code: Option<String>,
    /// The page may show a code to paste back into the app.
    pub accepts_paste: bool,
}

pub enum Pending {
    AuthCode {
        kind: ProviderKind,
        verifier: String,
        state: String,
        redirect_uri: String,
        listener: Option<TcpListener>,
    },
    GitHubDevice {
        device_code: String,
        interval: u64,
        expires_in: u64,
    },
    KimiDevice {
        device_code: String,
        device_id: String,
        interval: u64,
        expires_in: u64,
    },
    CodexDevice {
        device_auth_id: String,
        user_code: String,
        interval: u64,
    },
    CursorPoll {
        uuid: String,
        verifier: String,
    },
    /// A standard device sign-in (RFC 8628), for the providers that need nothing special.
    Device {
        kind: ProviderKind,
        device_code: String,
        interval: u64,
        expires_in: u64,
    },
    /// The user pastes a one-time token the provider's page shows.
    PastedToken {
        kind: ProviderKind,
    },
}

/// Where a standard device sign-in starts and ends.
struct DeviceFlow {
    start_url: &'static str,
    token_url: &'static str,
    client_id: &'static str,
    scopes: Option<&'static str>,
    start_fields: &'static [(&'static str, &'static str)],
    /// A page that takes the code in its address, when the provider has one.
    verification_page: Option<&'static str>,
}

fn device_flow(kind: ProviderKind) -> Option<DeviceFlow> {
    let flow = |start_url, token_url, client_id| DeviceFlow {
        start_url,
        token_url,
        client_id,
        scopes: None,
        start_fields: &[],
        verification_page: None,
    };
    Some(match kind {
        ProviderKind::Grok => DeviceFlow { scopes: Some(grok::SCOPES), ..flow(grok::DEVICE_CODE_URL, grok::TOKEN_URL, grok::CLIENT_ID) },
        ProviderKind::Amp => {
            DeviceFlow { verification_page: Some(amp::VERIFICATION_PAGE), ..flow(amp::DEVICE_CODE_URL, amp::TOKEN_URL, amp::CLIENT_ID) }
        }
        ProviderKind::OpenCode => DeviceFlow {
            start_fields: &[("supports_org_scope", "true")],
            ..flow(opencode::DEVICE_CODE_URL, opencode::TOKEN_URL, opencode::CLIENT_ID)
        },
        ProviderKind::Muse => flow(muse::DEVICE_CODE_URL, muse::TOKEN_URL, muse::CLIENT_ID),
        _ => return None,
    })
}

/// The page to open for a device sign-in, and the code to type there when the page does not
/// already carry it. Only an https page is ever opened.
fn device_page(flow: &DeviceFlow, device: &Value) -> LoginResult<(String, Option<String>)> {
    let code = required(device, "user_code")?;
    let with_code = match flow.verification_page {
        Some(page) => url::Url::parse_with_params(page, [("user_code", code.as_str())]).ok().map(String::from),
        None => http::text(&device["verification_uri_complete"]),
    };
    let https = |address: &String| url::Url::parse(address).map(|url| url.scheme() == "https" && url.has_host()).unwrap_or(false);
    match with_code.filter(https) {
        Some(url) => Ok((url, None)),
        None => {
            let url = http::text(&device["verification_uri"]).filter(https).ok_or("The sign-in service returned an invalid response.")?;
            Ok((url, Some(code)))
        }
    }
}

fn random_token(bytes: usize) -> String {
    let mut buffer = vec![0u8; bytes];
    rand::thread_rng().fill_bytes(&mut buffer);
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(buffer)
}

pub fn new_id() -> String {
    random_token(12)
}

fn pkce() -> (String, String) {
    let verifier = random_token(48);
    let challenge = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    (verifier, challenge)
}

fn network(_: impl std::fmt::Debug) -> String {
    "Could not reach the sign-in service. Check your connection.".to_string()
}

fn decode(response: &Resp) -> LoginResult<Value> {
    if !response.is_ok() {
        return Err(format!("Sign-in failed with HTTP {}.", response.status));
    }
    response.json().ok_or_else(|| "The sign-in service returned an invalid response.".to_string())
}

fn required(body: &Value, field: &str) -> LoginResult<String> {
    http::text(&body[field]).ok_or_else(|| "The sign-in service returned an invalid response.".to_string())
}

pub async fn start(http: &Client, kind: ProviderKind, mobile: bool) -> LoginResult<(LoginPrompt, Pending)> {
    let session_id = new_id();
    let prompt = |url: String, user_code: Option<String>, accepts_paste: bool| LoginPrompt { session_id, url, user_code, accepts_paste };
    match kind {
        ProviderKind::Claude => {
            // A phone cannot keep a callback server alive behind the browser, so there Anthropic
            // shows the code to paste. On desktop the browser returns to a loopback port.
            let listener = if mobile { None } else { TcpListener::bind(("127.0.0.1", 0)).await.ok() };
            let redirect_uri = match listener.as_ref().and_then(|listener| listener.local_addr().ok()) {
                Some(address) => format!("http://localhost:{}/callback", address.port()),
                None => claude::MANUAL_REDIRECT_URI.to_string(),
            };
            let (url, verifier, state) =
                authorize_url(claude::AUTHORIZE_URL, claude::CLIENT_ID, claude::SCOPES, &redirect_uri, &[("code", "true")]);
            Ok((prompt(url, None, true), Pending::AuthCode { kind, verifier, state, redirect_uri, listener }))
        }
        ProviderKind::Codex => {
            // OpenAI only accepts this exact callback. Without the port, or on a phone, use a device code.
            let listener = if mobile { None } else { TcpListener::bind(("127.0.0.1", 1455)).await.ok() };
            let Some(listener) = listener else {
                let body = json!({"client_id": codex::CLIENT_ID});
                let request = http.post("https://auth.openai.com/api/accounts/deviceauth/usercode").json(&body);
                let device = decode(&http::send(request).await.map_err(network)?)?;
                let user_code = required(&device, "user_code")?;
                let pending = Pending::CodexDevice {
                    device_auth_id: required(&device, "device_auth_id")?,
                    user_code: user_code.clone(),
                    interval: http::num(&device["interval"]).unwrap_or(5.0) as u64,
                };
                return Ok((prompt("https://auth.openai.com/codex/device".into(), Some(user_code), false), pending));
            };
            let redirect_uri = "http://localhost:1455/auth/callback".to_string();
            let extra = [("id_token_add_organizations", "true"), ("codex_cli_simplified_flow", "true")];
            let (url, verifier, state) = authorize_url(codex::AUTHORIZE_URL, codex::CLIENT_ID, codex::SCOPES, &redirect_uri, &extra);
            Ok((prompt(url, None, true), Pending::AuthCode { kind, verifier, state, redirect_uri, listener: Some(listener) }))
        }
        ProviderKind::Copilot => {
            let form = [("client_id", copilot::CLIENT_ID), ("scope", copilot::SCOPES)];
            let request = http.post(copilot::DEVICE_CODE_URL).header("Accept", "application/json").form(&form);
            let device = decode(&http::send(request).await.map_err(network)?)?;
            let url = http::text(&device["verification_uri"]).unwrap_or_else(|| "https://github.com/login/device".into());
            let pending = Pending::GitHubDevice {
                device_code: required(&device, "device_code")?,
                interval: http::num(&device["interval"]).unwrap_or(5.0) as u64,
                expires_in: http::num(&device["expires_in"]).unwrap_or(900.0) as u64,
            };
            Ok((prompt(url, Some(required(&device, "user_code")?), false), pending))
        }
        ProviderKind::Kimi => {
            let device_id = uuid_v4().replace('-', "");
            let request = kimi::auth_request(http, kimi::DEVICE_CODE_URL, &device_id).form(&[("client_id", kimi::CLIENT_ID)]);
            let device = decode(&http::send(request).await.map_err(network)?)?;
            // The complete link carries the code, so there is nothing to type.
            let (url, user_code) = match http::text(&device["verification_uri_complete"]) {
                Some(url) => (url, None),
                None => (required(&device, "verification_uri")?, Some(required(&device, "user_code")?)),
            };
            let pending = Pending::KimiDevice {
                device_code: required(&device, "device_code")?,
                device_id,
                interval: http::num(&device["interval"]).unwrap_or(5.0) as u64,
                expires_in: http::num(&device["expires_in"]).filter(|seconds| *seconds > 0.0).unwrap_or(900.0) as u64,
            };
            Ok((prompt(url, user_code, false), pending))
        }
        ProviderKind::Cursor => {
            let (verifier, challenge) = pkce();
            let uuid = uuid_v4();
            let mut url = url::Url::parse(cursor::LOGIN_URL).expect("static URL");
            url.query_pairs_mut()
                .append_pair("challenge", &challenge)
                .append_pair("uuid", &uuid)
                .append_pair("mode", "login")
                .append_pair("redirectTarget", "cli");
            Ok((prompt(url.into(), None, false), Pending::CursorPoll { uuid, verifier }))
        }
        ProviderKind::Devin => {
            let mut url = url::Url::parse(devin::TOKEN_PAGE_URL).expect("static URL");
            url.query_pairs_mut()
                .append_pair("response_type", "token")
                .append_pair("redirect_uri", "show-auth-token")
                .append_pair("state", &uuid_v4())
                .append_pair("scope", "openid profile email")
                .append_pair("redirect_parameters_type", "query");
            Ok((prompt(url.into(), None, true), Pending::PastedToken { kind }))
        }
        _ => {
            let Some(flow) = device_flow(kind) else { return Err(format!("{} uses an API key.", kind.display_name())) };
            let mut form: Vec<(&str, &str)> = flow.start_fields.to_vec();
            form.push(("client_id", flow.client_id));
            form.extend(flow.scopes.map(|scopes| ("scope", scopes)));
            let request = http.post(flow.start_url).header("Accept", "application/json").form(&form);
            let device = decode(&http::send(request).await.map_err(network)?)?;
            let (url, user_code) = device_page(&flow, &device)?;
            let pending = Pending::Device {
                kind,
                device_code: required(&device, "device_code")?,
                interval: http::num(&device["interval"]).unwrap_or(5.0) as u64,
                expires_in: http::num(&device["expires_in"]).filter(|seconds| *seconds > 0.0).unwrap_or(900.0) as u64,
            };
            Ok((prompt(url, user_code, false), pending))
        }
    }
}

fn uuid_v4() -> String {
    let mut b = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut b);
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    let hex: String = b.iter().map(|byte| format!("{byte:02x}")).collect();
    format!("{}-{}-{}-{}-{}", &hex[0..8], &hex[8..12], &hex[12..16], &hex[16..20], &hex[20..32])
}

fn authorize_url(base: &str, client_id: &str, scopes: &str, redirect_uri: &str, extra: &[(&str, &str)]) -> (String, String, String) {
    let (verifier, challenge) = pkce();
    let state = random_token(24);
    let mut url = url::Url::parse(base).expect("static URL");
    {
        let mut query = url.query_pairs_mut();
        query
            .append_pair("client_id", client_id)
            .append_pair("response_type", "code")
            .append_pair("redirect_uri", redirect_uri)
            .append_pair("scope", scopes)
            .append_pair("code_challenge", &challenge)
            .append_pair("code_challenge_method", "S256")
            .append_pair("state", &state);
        for (name, value) in extra {
            query.append_pair(name, value);
        }
    }
    (url.into(), verifier, state)
}

pub async fn finish(http: &Client, pending: Pending, mut pasted: mpsc::Receiver<String>) -> LoginResult<(String, Credential)> {
    match pending {
        Pending::AuthCode { kind, verifier, state, redirect_uri, listener } => {
            let path = url::Url::parse(&redirect_uri).map(|url| url.path().to_string()).unwrap_or_default();
            let wait = async {
                tokio::select! {
                    code = wait_for_callback(listener, &path, &state) => code,
                    Some(value) = pasted.recv() => pasted_code(&value, &state),
                }
            };
            let code = timeout(BROWSER_SIGN_IN_TIMEOUT, wait).await.map_err(|_| "Sign-in timed out.".to_string())??;
            match kind {
                ProviderKind::Claude => claude_exchange(http, &code, &state, &redirect_uri, &verifier).await,
                _ => codex_exchange(http, &code, &verifier, &redirect_uri).await,
            }
        }
        Pending::GitHubDevice { device_code, interval, expires_in } => {
            let token = poll(interval, expires_in, || async {
                let form = [
                    ("client_id", copilot::CLIENT_ID),
                    ("device_code", device_code.as_str()),
                    ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
                ];
                let request = http.post(copilot::DEVICE_TOKEN_URL).header("Accept", "application/json").form(&form);
                let body = http::send(request).await.ok().and_then(|response| response.json()).unwrap_or(Value::Null);
                if let Some(token) = http::text(&body["access_token"]) {
                    return Poll::Done(token);
                }
                match body["error"].as_str() {
                    Some("authorization_pending") | None => Poll::Pending,
                    Some("slow_down") => Poll::SlowDown,
                    Some("expired_token") => Poll::Failed("The GitHub device code expired.".into()),
                    Some("access_denied") => Poll::Failed("GitHub sign-in was denied.".into()),
                    Some(_) => Poll::Failed("GitHub sign-in failed.".into()),
                }
            })
            .await?;
            let request = http.get(copilot::USER_URL).bearer_auth(&token).header("Accept", "application/vnd.github+json");
            let user = decode(&http::send(request).await.map_err(network)?)?;
            Ok((required(&user, "login")?, Credential::oauth(token, None, None)))
        }
        Pending::KimiDevice { device_code, device_id, interval, expires_in } => {
            let token = poll(interval, expires_in, || async {
                let form = [
                    ("client_id", kimi::CLIENT_ID),
                    ("device_code", device_code.as_str()),
                    ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
                ];
                let request = kimi::auth_request(http, kimi::TOKEN_URL, &device_id).form(&form);
                let body = http::send(request).await.ok().and_then(|response| response.json()).unwrap_or(Value::Null);
                if http::text(&body["access_token"]).is_some() {
                    return Poll::Done(body);
                }
                match body["error"].as_str() {
                    Some("authorization_pending") | None => Poll::Pending,
                    Some("slow_down") => Poll::SlowDown,
                    Some("expired_token") => Poll::Failed("The Kimi sign-in link expired.".into()),
                    Some("access_denied") => Poll::Failed("Kimi sign-in was denied.".into()),
                    Some(_) => Poll::Failed("Kimi sign-in failed.".into()),
                }
            })
            .await?;
            let access = required(&token, "access_token")?;
            let expires_at = http::num(&token["expires_in"])
                .map(|seconds| crate::model::now_ms() + (seconds * 1000.0) as i64)
                .or_else(|| http::jwt_expiry_ms(&access));
            // Kimi has no profile read; name the account from the token when it carries a name.
            let claims = http::jwt_payload(&access).unwrap_or(Value::Null);
            let label = ["email", "name", "phone"]
                .iter()
                .find_map(|key| http::text(&claims[*key]))
                .or_else(|| {
                    http::text(&claims["sub"]).map(|id| {
                        let tail: String = id.chars().rev().take(6).collect::<Vec<_>>().into_iter().rev().collect();
                        format!("Kimi {tail}")
                    })
                })
                .unwrap_or_else(|| "Kimi Account".into());
            let credential =
                Credential::oauth(access, http::text(&token["refresh_token"]), expires_at).with_extra(kimi::DEVICE_ID_KEY, Some(device_id));
            Ok((label, credential))
        }
        Pending::Device { kind, device_code, interval, expires_in } => {
            let flow = device_flow(kind).ok_or("This provider has no device sign-in.")?;
            let name = kind.display_name();
            let token = poll(interval, expires_in, || async {
                let form = [
                    ("client_id", flow.client_id),
                    ("device_code", device_code.as_str()),
                    ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
                ];
                let request = http.post(flow.token_url).header("Accept", "application/json").form(&form);
                let body = http::send(request).await.ok().and_then(|response| response.json()).unwrap_or(Value::Null);
                if http::text(&body["access_token"]).is_some() {
                    return Poll::Done(body);
                }
                match body["error"].as_str() {
                    Some("authorization_pending") | None => Poll::Pending,
                    Some("slow_down") => Poll::SlowDown,
                    Some("expired_token") => Poll::Failed(format!("The {name} sign-in expired.")),
                    Some("access_denied") => Poll::Failed(format!("{name} sign-in was denied.")),
                    Some(_) => Poll::Failed(format!("{name} sign-in failed.")),
                }
            })
            .await?;
            device_account(http, kind, &token).await
        }
        Pending::PastedToken { kind } => {
            let pasted =
                timeout(BROWSER_SIGN_IN_TIMEOUT, pasted.recv()).await.map_err(|_| "Sign-in timed out.".to_string())?.unwrap_or_default();
            let pasted = pasted.trim();
            if pasted.is_empty() {
                return Err("That token is not valid.".into());
            }
            debug_assert_eq!(kind, ProviderKind::Devin);
            devin_account(http, pasted).await
        }
        Pending::CodexDevice { device_auth_id, user_code, interval } => {
            let grant = poll(interval, 900, || async {
                let body = json!({"device_auth_id": device_auth_id, "user_code": user_code});
                let request = http.post("https://auth.openai.com/api/accounts/deviceauth/token").json(&body);
                let Ok(response) = http::send(request).await else { return Poll::Pending };
                let body = response.json().unwrap_or(Value::Null);
                if response.is_ok() {
                    return match (http::text(&body["authorization_code"]), http::text(&body["code_verifier"])) {
                        (Some(code), Some(verifier)) => Poll::Done((code, verifier)),
                        _ => Poll::Failed("OpenAI returned an invalid response.".into()),
                    };
                }
                if matches!(response.status, 403 | 404) {
                    return Poll::Pending;
                }
                match body["error"].as_str() {
                    Some("deviceauth_authorization_pending") => Poll::Pending,
                    Some("slow_down") => Poll::SlowDown,
                    Some("expired_token") => Poll::Failed("The device code expired.".into()),
                    _ => Poll::Failed("Device sign-in failed.".into()),
                }
            })
            .await?;
            codex_exchange(http, &grant.0, &grant.1, "https://auth.openai.com/deviceauth/callback").await
        }
        Pending::CursorPoll { uuid, verifier } => {
            let (access, refresh) = poll(2, 300, || async {
                let request = http.get(cursor::POLL_URL).query(&[("uuid", uuid.as_str()), ("verifier", verifier.as_str())]);
                let Ok(response) = http::send(request).await else { return Poll::Pending };
                if response.is_ok() {
                    let body = response.json().unwrap_or(Value::Null);
                    if let (Some(access), Some(refresh)) = (http::text(&body["accessToken"]), http::text(&body["refreshToken"])) {
                        return Poll::Done((access, refresh));
                    }
                }
                match response.status {
                    202 | 404 => Poll::Pending,
                    _ => Poll::Failed("Cursor sign-in failed.".into()),
                }
            })
            .await?;
            let label = cursor::profile_name(http, &access).await.unwrap_or_else(|| "Cursor Account".into());
            let expires_at = http::jwt_expiry_ms(&access);
            Ok((label, Credential::oauth(access, Some(refresh), expires_at)))
        }
    }
}

/// Names the account a device sign-in produced and builds its credential.
async fn device_account(http: &Client, kind: ProviderKind, token: &Value) -> LoginResult<(String, Credential)> {
    let access = required(token, "access_token")?;
    let refresh = http::text(&token["refresh_token"]);
    let expires_at = http::num(&token["expires_in"])
        .map(|seconds| crate::model::now_ms() + (seconds * 1000.0) as i64)
        .or_else(|| http::jwt_expiry_ms(&access));
    let fallback = format!("{} Account", kind.display_name());
    match kind {
        ProviderKind::Grok => {
            let email = http::jwt_payload(&access).and_then(|claims| http::text(&claims["email"]));
            Ok((email.unwrap_or(fallback), Credential::oauth(access, refresh, expires_at)))
        }
        ProviderKind::Amp => {
            // The sign-in proves it can read usage, and the usage text names the account.
            let balance = amp::balance(http, &access).await.map_err(network)?;
            let text = amp::display_text(&balance).map_err(|error| error.to_string())?;
            let expires_at = http::jwt_expiry_ms(&access).or(expires_at);
            Ok((amp::email(&text).unwrap_or(fallback), Credential::oauth(access, refresh, expires_at)))
        }
        ProviderKind::OpenCode => {
            let org = required(token, opencode::ORG_KEY)?;
            let request = http.get(opencode::USER_URL).bearer_auth(&access).header("x-org-id", &org);
            let user = decode(&http::send(request).await.map_err(network)?)?;
            let credential = Credential::oauth(access, refresh, expires_at).with_extra(opencode::ORG_KEY, Some(org));
            Ok((http::text(&user["email"]).unwrap_or(fallback), credential))
        }
        ProviderKind::Muse => {
            let account = muse::account(http, &access).await.map_err(|error| error.to_string())?;
            Ok((http::text(&account["user_email"]).unwrap_or(fallback), Credential::oauth(access, None, None)))
        }
        _ => Err("This provider has no device sign-in.".into()),
    }
}

/// Trades Windsurf's one-time token for the account's API key, the key its editor plugins use.
async fn devin_account(http: &Client, pasted: &str) -> LoginResult<(String, Credential)> {
    let request = http.post(devin::REGISTER_URL).json(&json!({"firebase_id_token": pasted}));
    let response = http::send(request).await.map_err(network)?;
    let expired = "Windsurf rejected that token. A token works for five minutes, so open the page again for a new one.";
    if matches!(response.status, 401 | 403) {
        return Err(expired.into());
    }
    let registered = decode(&response)?;
    let server = http::text(&registered["api_server_url"]).and_then(|address| devin::trusted_server(&address));
    let credential = Credential::oauth(required(&registered, "api_key")?, None, None).with_extra(devin::SERVER_KEY, server);
    let status = devin::user_status(http, &credential).await.map_err(network)?;
    if matches!(status.status, 401 | 403) {
        return Err(expired.into());
    }
    let user = decode(&status)?;
    Ok((http::text(&user["userStatus"]["email"]).unwrap_or_else(|| "Devin Account".into()), credential))
}

async fn claude_exchange(http: &Client, code: &str, state: &str, redirect_uri: &str, verifier: &str) -> LoginResult<(String, Credential)> {
    let body = json!({
        "grant_type": "authorization_code",
        "client_id": claude::CLIENT_ID,
        "code": code,
        "state": state,
        "redirect_uri": redirect_uri,
        "code_verifier": verifier,
    });
    let token = decode(&http::send(http.post(claude::TOKEN_URL).json(&body)).await.map_err(network)?)?;
    let access = required(&token, "access_token")?;
    let expires_at = http::num(&token["expires_in"]).map(|seconds| crate::model::now_ms() + (seconds * 1000.0) as i64);
    let profile = claude::profile(http, &access).await.map_err(|_| "Claude did not return your profile. Try again.".to_string())?;
    let label =
        http::text(&profile["account"]["email"]).or_else(|| http::text(&profile["email"])).unwrap_or_else(|| "Claude Account".into());
    let credential =
        Credential::oauth(access, http::text(&token["refresh_token"]), expires_at).with_extra("plan", claude::plan_name(&profile));
    Ok((label, credential))
}

async fn codex_exchange(http: &Client, code: &str, verifier: &str, redirect_uri: &str) -> LoginResult<(String, Credential)> {
    let form = [
        ("grant_type", "authorization_code"),
        ("client_id", codex::CLIENT_ID),
        ("code", code),
        ("code_verifier", verifier),
        ("redirect_uri", redirect_uri),
    ];
    let token = decode(&http::send(http.post(codex::TOKEN_URL).form(&form)).await.map_err(network)?)?;
    let access = required(&token, "access_token")?;
    let account_id = http::jwt_payload(&access).and_then(|claims| http::text(&claims["https://api.openai.com/auth"]["chatgpt_account_id"]));
    let email = token["id_token"].as_str().and_then(http::jwt_payload).and_then(|claims| http::text(&claims["email"]));
    let expires_at = http::jwt_expiry_ms(&access);
    let credential =
        Credential::oauth(access, http::text(&token["refresh_token"]), expires_at).with_extra(codex::ACCOUNT_ID_KEY, account_id);
    Ok((email.unwrap_or_else(|| "ChatGPT Account".into()), credential))
}

enum Poll<T> {
    Pending,
    SlowDown,
    Done(T),
    Failed(String),
}

async fn poll<T, F, Fut>(interval_secs: u64, expires_in_secs: u64, mut request: F) -> LoginResult<T>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Poll<T>>,
{
    let deadline = Instant::now() + Duration::from_secs(expires_in_secs);
    let mut delay = interval_secs.max(1);
    while Instant::now() < deadline {
        sleep(Duration::from_secs(delay)).await;
        match request().await {
            Poll::Pending => {}
            Poll::SlowDown => delay += 5,
            Poll::Done(value) => return Ok(value),
            Poll::Failed(message) => return Err(message),
        }
    }
    Err("Sign-in expired. Try again.".into())
}

/// Serves the loopback callback until a request carries the code for this sign-in.
async fn wait_for_callback(listener: Option<TcpListener>, path: &str, state: &str) -> LoginResult<String> {
    let Some(listener) = listener else { return std::future::pending().await };
    loop {
        let Ok((mut stream, _)) = listener.accept().await else { continue };
        let mut buffer = vec![0u8; 8192];
        let read = match timeout(Duration::from_secs(5), stream.read(&mut buffer)).await {
            Ok(Ok(read)) => read,
            _ => continue,
        };
        let request = String::from_utf8_lossy(&buffer[..read]);
        let result = callback_code(&request, path, state);
        let (status, message) = match &result {
            Some(Ok(_)) => ("200 OK", "Signed in. You can close this tab and return to AI Balance."),
            Some(Err(_)) => ("400 Bad Request", "Sign-in failed. Return to AI Balance and try again."),
            None => ("404 Not Found", "Not found."),
        };
        let page = format!("<!doctype html><meta charset=utf-8><title>AI Balance</title><body style=\"font-family:system-ui;padding:3rem;text-align:center\"><h2>{message}</h2>");
        let response = format!(
            "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{page}",
            page.len()
        );
        let _ = stream.write_all(response.as_bytes()).await;
        let _ = stream.shutdown().await;
        if let Some(result) = result {
            return result;
        }
    }
}

/// `None` for a request that is not the callback, such as the browser's favicon fetch.
fn callback_code(request: &str, path: &str, state: &str) -> Option<LoginResult<String>> {
    let mut parts = request.lines().next()?.split_whitespace();
    let (method, target) = (parts.next()?, parts.next()?);
    let url = url::Url::parse(&format!("http://localhost{target}")).ok()?;
    if method != "GET" || url.path() != path {
        return None;
    }
    let param = |name: &str| url.query_pairs().find(|(key, _)| key == name).map(|(_, value)| value.into_owned());
    if let Some(error) = param("error") {
        return Some(Err(format!("Authorization failed: {error}")));
    }
    if param("state").as_deref() != Some(state) {
        return Some(Err("The sign-in response did not match this request.".into()));
    }
    Some(param("code").filter(|code| !code.is_empty()).ok_or_else(|| "The sign-in response had no code.".into()))
}

/// Reads a pasted callback URL, a `code#state` pair, or a bare code.
fn pasted_code(value: &str, state: &str) -> LoginResult<String> {
    let value = value.trim();
    let (code, pasted_state) = match url::Url::parse(value) {
        Ok(url) if url.has_host() => {
            let param = |name: &str| url.query_pairs().find(|(key, _)| key == name).map(|(_, value)| value.into_owned());
            (param("code"), param("state"))
        }
        _ => match value.split_once('#') {
            Some((code, pasted_state)) => (Some(code.to_string()), Some(pasted_state.to_string())),
            None => (Some(value.to_string()), None),
        },
    };
    if pasted_state.is_some_and(|pasted_state| pasted_state != state) {
        return Err("That code belongs to a different sign-in. Start again.".into());
    }
    code.filter(|code| !code.is_empty()).ok_or_else(|| "That code is not valid.".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pkce_challenge_is_the_sha256_of_the_verifier() {
        let (verifier, challenge) = pkce();
        assert!(verifier.len() >= 43);
        let expected = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
        assert_eq!(challenge, expected);
    }

    #[test]
    fn authorize_url_carries_every_parameter() {
        let (url, _, state) =
            authorize_url("https://example.com/authorize", "client", "a b", "http://localhost:1/callback", &[("code", "true")]);
        let url = url::Url::parse(&url).unwrap();
        let query: std::collections::HashMap<_, _> = url.query_pairs().into_owned().collect();
        assert_eq!(query["client_id"], "client");
        assert_eq!(query["scope"], "a b");
        assert_eq!(query["redirect_uri"], "http://localhost:1/callback");
        assert_eq!(query["code_challenge_method"], "S256");
        assert_eq!(query["state"], state);
        assert_eq!(query["code"], "true");
    }

    #[test]
    fn the_callback_accepts_only_its_own_path_and_state() {
        let request = |target: &str| format!("GET {target} HTTP/1.1\r\nHost: localhost\r\n\r\n");
        assert_eq!(callback_code(&request("/callback?code=abc&state=s1"), "/callback", "s1"), Some(Ok("abc".into())));
        assert!(matches!(callback_code(&request("/callback?code=abc&state=other"), "/callback", "s1"), Some(Err(_))));
        assert!(matches!(callback_code(&request("/callback?error=access_denied"), "/callback", "s1"), Some(Err(_))));
        assert_eq!(callback_code(&request("/favicon.ico"), "/callback", "s1"), None);
        assert_eq!(callback_code("POST /callback?code=abc&state=s1 HTTP/1.1\r\n\r\n", "/callback", "s1"), None);
    }

    #[test]
    fn pasted_codes_read_in_every_shape() {
        assert_eq!(pasted_code(" abc#s1 ", "s1"), Ok("abc".into()));
        assert_eq!(pasted_code("abc", "s1"), Ok("abc".into()));
        assert_eq!(pasted_code("http://localhost:1455/auth/callback?code=xyz&state=s1", "s1"), Ok("xyz".into()));
        assert!(pasted_code("abc#other", "s1").is_err());
        assert!(pasted_code("", "s1").is_err());
    }

    #[tokio::test]
    async fn the_loopback_server_skips_stray_requests_and_returns_the_code() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let address = listener.local_addr().unwrap();
        let browser = tokio::spawn(async move {
            let mut pages = Vec::new();
            for target in ["/favicon.ico", "/callback?code=abc&state=s1"] {
                let mut stream = tokio::net::TcpStream::connect(address).await.unwrap();
                stream.write_all(format!("GET {target} HTTP/1.1\r\nHost: localhost\r\n\r\n").as_bytes()).await.unwrap();
                let mut page = String::new();
                stream.read_to_string(&mut page).await.unwrap();
                pages.push(page);
            }
            pages
        });
        assert_eq!(wait_for_callback(Some(listener), "/callback", "s1").await, Ok("abc".into()));
        let pages = browser.await.unwrap();
        assert!(pages[0].starts_with("HTTP/1.1 404"));
        assert!(pages[1].starts_with("HTTP/1.1 200") && pages[1].contains("Signed in"));
    }

    #[test]
    fn a_device_page_carries_the_code_when_it_can() {
        let amp_flow = device_flow(ProviderKind::Amp).unwrap();
        let page = device_page(&amp_flow, &json!({"user_code": "AB CD", "verification_uri": "https://ampcode.com/x"})).unwrap();
        assert_eq!(page, ("https://ampcode.com/auth/cli/device?user_code=AB+CD".to_string(), None));

        let grok_flow = device_flow(ProviderKind::Grok).unwrap();
        let complete = json!({"user_code": "WXYZ", "verification_uri": "https://x.ai/device", "verification_uri_complete": "https://x.ai/device?code=WXYZ"});
        assert_eq!(device_page(&grok_flow, &complete).unwrap(), ("https://x.ai/device?code=WXYZ".to_string(), None));
        let plain = json!({"user_code": "WXYZ", "verification_uri": "https://x.ai/device"});
        assert_eq!(device_page(&grok_flow, &plain).unwrap(), ("https://x.ai/device".to_string(), Some("WXYZ".to_string())));
    }

    #[test]
    fn a_device_page_that_is_not_https_is_refused() {
        let flow = device_flow(ProviderKind::Grok).unwrap();
        assert!(device_page(&flow, &json!({"user_code": "A", "verification_uri": "http://x.ai/device"})).is_err());
        assert!(device_page(&flow, &json!({"user_code": "A", "verification_uri": "javascript:alert(1)"})).is_err());
        let mixed = json!({"user_code": "A", "verification_uri": "https://x.ai/device", "verification_uri_complete": "file:///etc/passwd"});
        assert_eq!(device_page(&flow, &mixed).unwrap().0, "https://x.ai/device");
    }

    #[test]
    fn uuids_are_version_four() {
        let uuid = uuid_v4();
        assert_eq!(uuid.len(), 36);
        assert_eq!(uuid.as_bytes()[14], b'4');
    }
}
