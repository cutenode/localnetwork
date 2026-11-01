//! Where locallibrary keeps secrets (the Atmosphere account's session, plugins' secret
//! settings): the system's keychain, or with [`Storage::File`] files in the store folder only the
//! user can read.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Where secrets are kept.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Storage {
    /// macOS Keychain, Windows Credential Manager, or the Secret Service on Linux.
    #[default]
    Keychain,
    /// Files in the store folder, readable only by you (`atmosphere-session.json`,
    /// `secrets/plugins/<id>.json`): for computers without a keychain (a Linux server without a
    /// desktop session).
    File,
}

impl Storage {
    /// `LOCALLIBRARY_SESSION_STORE=file` chooses the files.
    pub fn from_env() -> Storage {
        match std::env::var("LOCALLIBRARY_SESSION_STORE").as_deref() {
            Ok("file") => Storage::File,
            _ => Storage::Keychain,
        }
    }
}

/// The keychain item `account` of `service`. `keyring::Entry::new` refuses whenever the system's
/// keychain couldn't be opened, even when another store is the default (the tests' in-memory one,
/// on a Linux without Secret Service), so the entry comes from the default store, once `keyring`
/// has tried to set the system's.
pub(crate) fn entry(service: &str, account: &str) -> keyring::Result<keyring_core::Entry> {
    let _ = keyring::Entry::store_status();
    keyring_core::Entry::new(service, account)
}

/// Removes `entry`'s secret, if it has one.
pub(crate) fn delete(entry: &keyring_core::Entry) -> keyring::Result<()> {
    match entry.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(e),
    }
}

#[cfg(test)]
pub(crate) mod tests;
