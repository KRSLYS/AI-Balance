//! What the app keeps on disk: accounts, settings, the last snapshots, and credentials.
//!
//! Credentials go to the operating system's keychain on desktop (Keychain, Credential Manager,
//! Secret Service). On Android and iOS, and on a desktop without a keychain, they go to a file
//! inside the app's private data directory.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{de::DeserializeOwned, Deserialize, Serialize};

use crate::model::{Account, Credential, Settings, Snapshot};

#[derive(Serialize, Deserialize, Default, Clone)]
struct Saved {
    #[serde(default)]
    accounts: Vec<Account>,
    #[serde(default)]
    settings: Settings,
}

pub struct Store {
    dir: PathBuf,
    saved: Mutex<Saved>,
    /// Serializes reads and writes of the credentials file.
    vault: Mutex<()>,
}

fn read_json<T: DeserializeOwned + Default>(path: &Path) -> T {
    fs::read(path).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or_default()
}

/// Writes through a temporary file so a crash never leaves half a file behind.
fn write_json<T: Serialize>(path: &Path, value: &T, private: bool) -> std::io::Result<()> {
    let temporary = path.with_extension("tmp");
    fs::write(&temporary, serde_json::to_vec_pretty(value)?)?;
    #[cfg(unix)]
    if private {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&temporary, fs::Permissions::from_mode(0o600))?;
    }
    #[cfg(not(unix))]
    let _ = private;
    fs::rename(&temporary, path)
}

impl Store {
    pub fn open(dir: PathBuf) -> Self {
        let _ = fs::create_dir_all(&dir);
        let mut saved: Saved = read_json(&dir.join("state.json"));
        saved.settings = saved.settings.sanitized();
        Self { dir, saved: Mutex::new(saved), vault: Mutex::new(()) }
    }

    fn update<T>(&self, edit: impl FnOnce(&mut Saved) -> T) -> T {
        let mut saved = self.saved.lock().unwrap();
        let result = edit(&mut saved);
        if let Err(error) = write_json(&self.dir.join("state.json"), &*saved, false) {
            eprintln!("aibalance: could not save state: {error}");
        }
        result
    }

    pub fn accounts(&self) -> Vec<Account> {
        self.saved.lock().unwrap().accounts.clone()
    }

    pub fn account(&self, id: &str) -> Option<Account> {
        self.saved.lock().unwrap().accounts.iter().find(|account| account.id == id).cloned()
    }

    pub fn settings(&self) -> Settings {
        self.saved.lock().unwrap().settings.clone()
    }

    pub fn set_settings(&self, settings: Settings) -> Settings {
        let settings = settings.sanitized();
        self.update(|saved| saved.settings = settings.clone());
        settings
    }

    /// Adds an account, or replaces the one already there for the same provider and label
    /// (signing in again). Returns the stored account.
    pub fn upsert_account(&self, account: Account) -> Account {
        self.update(|saved| {
            let existing = saved.accounts.iter().find(|a| a.provider == account.provider && a.label.eq_ignore_ascii_case(&account.label));
            match existing {
                Some(existing) => existing.clone(),
                None => {
                    saved.accounts.push(account.clone());
                    account
                }
            }
        })
    }

    pub fn rename_account(&self, id: &str, label: &str) {
        self.update(|saved| {
            if let Some(account) = saved.accounts.iter_mut().find(|account| account.id == id) {
                account.label = label.to_string();
            }
        });
    }

    pub fn move_account(&self, id: &str, offset: i32) {
        self.update(|saved| {
            let Some(from) = saved.accounts.iter().position(|account| account.id == id) else { return };
            let to = (from as i64 + offset as i64).clamp(0, saved.accounts.len() as i64 - 1) as usize;
            let account = saved.accounts.remove(from);
            saved.accounts.insert(to, account);
        });
    }

    pub fn remove_account(&self, id: &str) {
        self.update(|saved| saved.accounts.retain(|account| account.id != id));
        self.delete_credential(id);
    }

    pub fn load_snapshots(&self) -> HashMap<String, Snapshot> {
        read_json(&self.dir.join("snapshots.json"))
    }

    pub fn save_snapshots(&self, snapshots: &HashMap<String, Snapshot>) {
        let _ = write_json(&self.dir.join("snapshots.json"), snapshots, false);
    }

    // ----- credentials -----

    fn vault_path(&self) -> PathBuf {
        self.dir.join("credentials.json")
    }

    fn edit_vault_file(&self, edit: impl FnOnce(&mut HashMap<String, Credential>)) -> bool {
        let _guard = self.vault.lock().unwrap();
        let path = self.vault_path();
        let mut all: HashMap<String, Credential> = read_json(&path);
        let before = all.clone();
        edit(&mut all);
        all == before || write_json(&path, &all, true).is_ok()
    }

    pub fn credential(&self, id: &str) -> Option<Credential> {
        if let Some(credential) = keychain::get(id) {
            return Some(credential);
        }
        let _guard = self.vault.lock().unwrap();
        read_json::<HashMap<String, Credential>>(&self.vault_path()).remove(id)
    }

    pub fn set_credential(&self, id: &str, credential: &Credential) -> bool {
        if keychain::set(id, credential) {
            self.edit_vault_file(|all| {
                all.remove(id);
            });
            return true;
        }
        self.edit_vault_file(|all| {
            all.insert(id.to_string(), credential.clone());
        })
    }

    fn delete_credential(&self, id: &str) {
        keychain::delete(id);
        self.edit_vault_file(|all| {
            all.remove(id);
        });
    }
}

#[cfg(desktop)]
mod keychain {
    use crate::model::Credential;

    const SERVICE: &str = "com.krslys.aibalance";

    fn entry(id: &str) -> Option<keyring::Entry> {
        keyring::Entry::new(SERVICE, id).ok()
    }

    pub fn get(id: &str) -> Option<Credential> {
        serde_json::from_str(&entry(id)?.get_password().ok()?).ok()
    }

    pub fn set(id: &str, credential: &Credential) -> bool {
        let Ok(json) = serde_json::to_string(credential) else { return false };
        entry(id).map(|entry| entry.set_password(&json).is_ok()).unwrap_or(false)
    }

    pub fn delete(id: &str) {
        if let Some(entry) = entry(id) {
            let _ = entry.delete_credential();
        }
    }
}

#[cfg(not(desktop))]
mod keychain {
    use crate::model::Credential;

    pub fn get(_: &str) -> Option<Credential> {
        None
    }

    pub fn set(_: &str, _: &Credential) -> bool {
        false
    }

    pub fn delete(_: &str) {}
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ProviderKind;

    fn temp_store(name: &str) -> Store {
        let dir = std::env::temp_dir().join(format!("aibalance-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        Store::open(dir)
    }

    fn account(id: &str, label: &str) -> Account {
        Account { id: id.into(), provider: ProviderKind::Claude, label: label.into(), created_at: 0 }
    }

    #[test]
    fn accounts_survive_a_reopen_and_keep_their_order() {
        let store = temp_store("accounts");
        store.upsert_account(account("a", "one@example.com"));
        store.upsert_account(account("b", "two@example.com"));
        store.move_account("b", -1);
        store.rename_account("a", "Work");
        let reopened = Store::open(store.dir.clone());
        let labels: Vec<_> = reopened.accounts().into_iter().map(|a| a.label).collect();
        assert_eq!(labels, ["two@example.com", "Work"]);
    }

    #[test]
    fn signing_in_again_keeps_the_existing_account() {
        let store = temp_store("upsert");
        store.upsert_account(account("a", "me@example.com"));
        let stored = store.upsert_account(account("b", "ME@example.com"));
        assert_eq!(stored.id, "a");
        assert_eq!(store.accounts().len(), 1);
    }

    #[test]
    fn settings_are_clamped_to_sane_values() {
        let store = temp_store("settings");
        let saved = store.set_settings(Settings { refresh_minutes: 0, notify_threshold: 500, ..Settings::default() });
        assert_eq!((saved.refresh_minutes, saved.notify_threshold), (2, 100));
    }

    #[test]
    fn the_credentials_file_round_trips_and_forgets() {
        let store = temp_store("vault");
        let credential = Credential::oauth("token".into(), Some("refresh".into()), Some(5)).with_extra("plan", Some("Pro".into()));
        assert!(store.edit_vault_file(|all| {
            all.insert("a".into(), credential.clone());
        }));
        let all: HashMap<String, Credential> = read_json(&store.vault_path());
        assert_eq!(all.get("a"), Some(&credential));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(fs::metadata(store.vault_path()).unwrap().permissions().mode() & 0o777, 0o600);
        }
        store.edit_vault_file(|all| {
            all.remove("a");
        });
        assert!(read_json::<HashMap<String, Credential>>(&store.vault_path()).is_empty());
    }
}
