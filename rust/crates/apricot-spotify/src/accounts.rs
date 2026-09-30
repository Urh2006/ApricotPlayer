//! Saved Spotify accounts: `spotify_accounts.json` in the Apricot data folder.
//!
//! Reusable `LibreSpot` credentials are stored only as a DPAPI blob of this
//! Windows user (never plain text, never in `settings.json`). Each account has
//! its own data folder `spotify/<key>` for its cache and settings.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const FILE_NAME: &str = "spotify_accounts.json";
const DATA_DIR: &str = "spotify";
const ENTROPY: &[u8] = b"ApricotPlayer Spotify credentials v1";

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct SpotifyAccount {
    /// Local key, derived from the Spotify user name; used for folders.
    pub key: String,
    pub display_name: String,
    /// `premium`, `free`, ... from the session attributes; empty when unknown.
    #[serde(default)]
    pub product: String,
    #[serde(default)]
    pub country: String,
    /// DPAPI-protected reusable credentials; empty after logout.
    #[serde(default)]
    pub credentials: String,
}

impl SpotifyAccount {
    pub const fn is_logged_in(&self) -> bool {
        !self.credentials.is_empty()
    }

    pub fn is_premium(&self) -> bool {
        self.product == "premium"
    }
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct SpotifyAccounts {
    #[serde(default = "version")]
    pub version: u32,
    /// Connect device ID of this installation, created once.
    #[serde(default)]
    pub device_id: String,
    #[serde(default)]
    pub active: Option<String>,
    #[serde(default)]
    pub accounts: Vec<SpotifyAccount>,
}

const fn version() -> u32 {
    1
}

/// Stable local key: the first 16 hex digits of SHA-256 of the user name.
pub fn account_key(username: &str) -> String {
    hex(&Sha256::digest(username.as_bytes())[..8])
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    bytes
        .iter()
        .fold(String::with_capacity(bytes.len() * 2), |mut out, byte| {
            let _ = write!(out, "{byte:02x}");
            out
        })
}

/// # Errors
///
/// Returns the DPAPI error text.
pub fn protect_credentials(json: &[u8]) -> Result<String, String> {
    apricot_platform::audiovault_credentials::protect_data(json, ENTROPY, "ApricotPlayer Spotify")
}

pub fn unprotect_credentials(value: &str) -> Option<Vec<u8>> {
    apricot_platform::audiovault_credentials::unprotect_data(value, ENTROPY)
}

/// Loads, changes and saves the account file.
#[derive(Clone, Debug)]
pub struct AccountStore {
    root: PathBuf,
}

impl AccountStore {
    pub fn new(app_data: &Path) -> Self {
        Self {
            root: app_data.to_path_buf(),
        }
    }

    pub fn file(&self) -> PathBuf {
        self.root.join(FILE_NAME)
    }

    pub fn account_dir(&self, key: &str) -> PathBuf {
        self.root.join(DATA_DIR).join(key)
    }

    /// A missing file is an empty store. An unreadable one is kept aside as
    /// `spotify_accounts.json.corrupt` so nothing is silently overwritten.
    pub fn load(&self) -> SpotifyAccounts {
        let path = self.file();
        if !path.exists() {
            return SpotifyAccounts::default();
        }
        if let Ok(accounts) = apricot_storage::json_file::read_json::<SpotifyAccounts>(&path) {
            accounts
        } else {
            let _ = std::fs::rename(&path, path.with_extension("json.corrupt"));
            SpotifyAccounts::default()
        }
    }

    /// # Errors
    ///
    /// Returns the storage error text when the file cannot be written.
    pub fn save(&self, accounts: &SpotifyAccounts) -> Result<(), String> {
        apricot_storage::json_file::write_json_atomic(&self.file(), accounts)
            .map_err(|error| error.to_string())
    }

    /// The installation device ID, created and saved on first use.
    ///
    /// # Errors
    ///
    /// Returns the storage error text when the file cannot be written.
    pub fn device_id(&self) -> Result<String, String> {
        let mut accounts = self.load();
        if accounts.device_id.is_empty() {
            let mut bytes = [0_u8; 20];
            rand::RngCore::fill_bytes(&mut rand::rng(), &mut bytes);
            accounts.device_id = hex(&bytes);
            accounts.version = version();
            self.save(&accounts)?;
        }
        Ok(accounts.device_id)
    }

    /// Adds or replaces an account and makes it the active one.
    ///
    /// # Errors
    ///
    /// Returns the storage error text when the file cannot be written.
    pub fn upsert_active(&self, account: SpotifyAccount) -> Result<SpotifyAccounts, String> {
        let mut accounts = self.load();
        accounts.version = version();
        accounts.active = Some(account.key.clone());
        if let Some(existing) = accounts.accounts.iter_mut().find(|a| a.key == account.key) {
            *existing = account;
        } else {
            accounts.accounts.push(account);
        }
        self.save(&accounts)?;
        Ok(accounts)
    }

    /// # Errors
    ///
    /// Returns the storage error text when the file cannot be written.
    pub fn set_active(&self, key: &str) -> Result<SpotifyAccounts, String> {
        let mut accounts = self.load();
        if accounts.accounts.iter().any(|account| account.key == key) {
            accounts.active = Some(key.to_owned());
            self.save(&accounts)?;
        }
        Ok(accounts)
    }

    /// Logout: forgets the credentials, keeps the account and its data.
    ///
    /// # Errors
    ///
    /// Returns the storage error text when the file cannot be written.
    pub fn logout(&self, key: &str) -> Result<SpotifyAccounts, String> {
        let mut accounts = self.load();
        for account in &mut accounts.accounts {
            if account.key == key {
                account.credentials.clear();
            }
        }
        self.save(&accounts)?;
        Ok(accounts)
    }

    /// Removes the account, its credentials and its data folder.
    ///
    /// # Errors
    ///
    /// Returns the storage error text when the file cannot be written.
    pub fn remove(&self, key: &str) -> Result<SpotifyAccounts, String> {
        let mut accounts = self.load();
        accounts.accounts.retain(|account| account.key != key);
        if accounts.active.as_deref() == Some(key) {
            accounts.active = accounts.accounts.first().map(|account| account.key.clone());
        }
        self.save(&accounts)?;
        let dir = self.account_dir(key);
        if dir.starts_with(self.root.join(DATA_DIR)) && key.len() == 16 {
            let _ = std::fs::remove_dir_all(dir);
        }
        Ok(accounts)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn account(key: &str) -> SpotifyAccount {
        SpotifyAccount {
            key: key.into(),
            display_name: format!("Name {key}"),
            product: "premium".into(),
            country: "SI".into(),
            credentials: "blob".into(),
        }
    }

    #[test]
    fn keys_are_stable_short_and_folder_safe() {
        let key = account_key("user.name");
        assert_eq!(key.len(), 16);
        assert_eq!(key, account_key("user.name"));
        assert_ne!(key, account_key("other"));
        assert!(key.bytes().all(|b| b.is_ascii_hexdigit()));
    }

    #[test]
    fn add_switch_logout_and_remove_keep_other_accounts() {
        let dir = tempfile::tempdir().unwrap();
        let store = AccountStore::new(dir.path());
        let a = account_key("a");
        let b = account_key("b");
        store.upsert_active(account(&a)).unwrap();
        store.upsert_active(account(&b)).unwrap();
        assert_eq!(store.load().active.as_deref(), Some(b.as_str()));
        store.set_active(&a).unwrap();
        assert_eq!(store.load().active.as_deref(), Some(a.as_str()));
        std::fs::create_dir_all(store.account_dir(&a)).unwrap();
        let after_logout = store.logout(&a).unwrap();
        assert!(!after_logout.accounts[0].is_logged_in());
        assert!(after_logout.accounts[1].is_logged_in());
        assert!(store.account_dir(&a).exists());
        let after_remove = store.remove(&a).unwrap();
        assert_eq!(after_remove.accounts.len(), 1);
        assert_eq!(after_remove.active.as_deref(), Some(b.as_str()));
        assert!(!store.account_dir(&a).exists());
    }

    #[test]
    fn device_id_is_created_once() {
        let dir = tempfile::tempdir().unwrap();
        let store = AccountStore::new(dir.path());
        let first = store.device_id().unwrap();
        assert_eq!(first.len(), 40);
        assert_eq!(store.device_id().unwrap(), first);
    }

    #[test]
    fn corrupt_file_is_kept_aside_not_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        let store = AccountStore::new(dir.path());
        std::fs::write(store.file(), b"{not json").unwrap();
        assert_eq!(store.load(), SpotifyAccounts::default());
        assert!(dir.path().join("spotify_accounts.json.corrupt").exists());
    }

    #[cfg(windows)]
    #[test]
    fn credentials_round_trip_through_dpapi() {
        let blob = protect_credentials(b"{\"k\":1}").unwrap();
        assert_eq!(
            unprotect_credentials(&blob).as_deref(),
            Some(&b"{\"k\":1}"[..])
        );
        assert_eq!(unprotect_credentials("not-base64"), None);
    }
}
