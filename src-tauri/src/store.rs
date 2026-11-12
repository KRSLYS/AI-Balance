//! What the app keeps on disk: accounts, settings, the last snapshots, and credentials.
//!
//! Credentials go to the operating system's keychain on desktop (Keychain, Credential Manager,
//! Secret Service). Elsewhere they go to a file inside the app's private data directory: on Android
//! that file is encrypted with a key held by the Android Keystore; on iOS, and on a desktop without
//! a keychain, it is a private file.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde::{de::DeserializeOwned, Deserialize, Serialize};

use crate::model::{Account, Credential, Settings, Snapshot};

#[derive(Serialize, Deserialize, Default, Clone)]
struct Saved {
    #[serde(default)]
    accounts: Vec<Account>,
    #[serde(default)]
    settings: Settings,
}

/// Encrypts and decrypts the credentials file with a key the device holds and never gives out.
pub trait Sealer: Send + Sync {
    fn seal(&self, plain: &str) -> Option<String>;
    fn unseal(&self, sealed: &str) -> Option<String>;
}

pub struct Store {
    dir: PathBuf,
    saved: Mutex<Saved>,
    /// Serializes reads and writes of the credentials file.
    vault: Mutex<()>,
    /// Present where the platform can encrypt the credentials file.
    sealer: Option<Arc<dyn Sealer>>,
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
        Self::open_with(dir, device::sealer())
    }

    fn open_with(dir: PathBuf, sealer: Option<Arc<dyn Sealer>>) -> Self {
        let _ = fs::create_dir_all(&dir);
        let mut saved: Saved = read_json(&dir.join("state.json"));
        saved.settings = saved.settings.sanitized();
        Self { dir, saved: Mutex::new(saved), vault: Mutex::new(()), sealer }
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

    fn sealed_path(&self) -> PathBuf {
        self.dir.join("credentials.sealed")
    }

    /// Every credential in the file. The encrypted file wins when both exist; one that cannot be
    /// decrypted (the device's key is gone) reads as empty, and those accounts sign in again.
    fn read_vault(&self) -> HashMap<String, Credential> {
        if let (Some(sealer), Ok(sealed)) = (&self.sealer, fs::read_to_string(self.sealed_path())) {
            return sealer.unseal(sealed.trim()).and_then(|plain| serde_json::from_str(&plain).ok()).unwrap_or_default();
        }
        read_json(&self.vault_path())
    }

    /// Writes the file encrypted where the device can, and removes the plain file it replaces.
    fn write_vault(&self, all: &HashMap<String, Credential>) -> bool {
        let sealed = self.sealer.as_ref().and_then(|sealer| sealer.seal(&serde_json::to_string(all).ok()?));
        match sealed {
            Some(sealed) => {
                let temporary = self.sealed_path().with_extension("tmp");
                let written = fs::write(&temporary, sealed).and_then(|()| fs::rename(&temporary, self.sealed_path())).is_ok();
                if written {
                    let _ = fs::remove_file(self.vault_path());
                }
                written
            }
            None => write_json(&self.vault_path(), all, true).is_ok(),
        }
    }

    fn edit_vault_file(&self, edit: impl FnOnce(&mut HashMap<String, Credential>)) -> bool {
        let _guard = self.vault.lock().unwrap();
        let mut all = self.read_vault();
        let before = all.clone();
        edit(&mut all);
        // A plain file left from an earlier version is rewritten encrypted even when nothing changed.
        let migrate = self.sealer.is_some() && self.vault_path().exists();
        (all == before && !migrate) || self.write_vault(&all)
    }

    pub fn credential(&self, id: &str) -> Option<Credential> {
        if let Some(credential) = keychain::get(id) {
            return Some(credential);
        }
        let _guard = self.vault.lock().unwrap();
        self.read_vault().remove(id)
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

/// The device's own key store, where the app can reach one.
pub mod device {
    use std::sync::Arc;

    use super::Sealer;

    #[cfg(target_os = "android")]
    mod keystore {
        use std::sync::OnceLock;

        use serde::{Deserialize, Serialize};
        use tauri::plugin::PluginHandle;

        static PLUGIN: OnceLock<PluginHandle<tauri::Wry>> = OnceLock::new();

        #[derive(Serialize)]
        struct Request<'a> {
            data: &'a str,
        }

        #[derive(Deserialize)]
        struct Reply {
            data: String,
        }

        /// Hands over the Kotlin plugin that talks to the Android Keystore (VaultPlugin.kt).
        pub fn register(handle: PluginHandle<tauri::Wry>) {
            let _ = PLUGIN.set(handle);
        }

        pub fn is_registered() -> bool {
            PLUGIN.get().is_some()
        }

        fn call(command: &str, data: &str) -> Option<String> {
            PLUGIN.get()?.run_mobile_plugin::<Reply>(command, Request { data }).ok().map(|reply| reply.data)
        }

        pub struct Keystore;

        impl super::Sealer for Keystore {
            fn seal(&self, plain: &str) -> Option<String> {
                call("seal", plain)
            }

            fn unseal(&self, sealed: &str) -> Option<String> {
                call("unseal", sealed)
            }
        }
    }

    #[cfg(target_os = "android")]
    pub use keystore::register;

    pub fn sealer() -> Option<Arc<dyn Sealer>> {
        #[cfg(target_os = "android")]
        if keystore::is_registered() {
            return Some(Arc::new(keystore::Keystore));
        }
        None
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

    /// Stands in for the device's key store: reversible, and recognizably not the plain text.
    struct Reversing;

    impl Sealer for Reversing {
        fn seal(&self, plain: &str) -> Option<String> {
            Some(plain.chars().rev().collect())
        }

        fn unseal(&self, sealed: &str) -> Option<String> {
            Some(sealed.chars().rev().collect())
        }
    }

    /// A device whose key was lost: nothing sealed earlier can be read.
    struct Forgetful;

    impl Sealer for Forgetful {
        fn seal(&self, plain: &str) -> Option<String> {
            Some(plain.to_string())
        }

        fn unseal(&self, _: &str) -> Option<String> {
            None
        }
    }

    fn sealed_store(name: &str, sealer: impl Sealer + 'static) -> Store {
        let dir = std::env::temp_dir().join(format!("aibalance-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        Store::open_with(dir, Some(Arc::new(sealer)))
    }

    #[test]
    fn where_the_device_can_encrypt_no_plain_file_is_left() {
        let store = sealed_store("sealed", Reversing);
        let credential = Credential::ApiKey { key: "sk-secret".into() };
        // A plain file from an earlier version of the app.
        write_json(&store.vault_path(), &HashMap::from([("old".to_string(), credential.clone())]), true).unwrap();
        assert!(store.edit_vault_file(|all| {
            all.insert("new".into(), credential.clone());
        }));
        assert!(!store.vault_path().exists());
        let on_disk = fs::read_to_string(store.sealed_path()).unwrap();
        assert!(!on_disk.contains("sk-secret"));
        let all = store.read_vault();
        assert_eq!((all.get("old"), all.get("new")), (Some(&credential), Some(&credential)));
    }

    #[test]
    fn a_plain_file_is_encrypted_the_first_time_the_vault_is_touched() {
        let store = sealed_store("migrate", Reversing);
        write_json(&store.vault_path(), &HashMap::from([("a".to_string(), Credential::ApiKey { key: "k".into() })]), true).unwrap();
        assert!(store.edit_vault_file(|_| {}));
        assert!(!store.vault_path().exists() && store.sealed_path().exists());
    }

    #[test]
    fn a_lost_device_key_reads_as_no_credentials() {
        let store = sealed_store("lost", Forgetful);
        store.edit_vault_file(|all| {
            all.insert("a".into(), Credential::ApiKey { key: "k".into() });
        });
        assert!(store.read_vault().is_empty());
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
