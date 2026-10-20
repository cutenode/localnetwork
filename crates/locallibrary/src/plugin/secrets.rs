//! Plugins' secret settings: in the system's keychain, an item for each plugin, key and store
//! (service `locallibrary-plugin-<id>`, account `<key>@<store id>`), or with [`Storage::File`] in
//! `<store>/secrets/plugins/<id>.json`, readable only by the user. Never in `settings.json`, a
//! hook's context, `plugins.describe`, what `plugin settings` shows or a log: the host only fills
//! them into the plugin's `net/fetch` requests. A plugin runs as the user and isn't sandboxed, so
//! this is what the host does with them, not a wall the system keeps: their file is kept apart
//! from the plugin's own folder (where it runs, and its requests' scratch folders are) and from
//! everything the host sends it, but nothing stops the plugin reading it as the user could.

use std::path::{Path, PathBuf};

use indexmap::IndexMap;
use loclib_plugin::SettingType;

use super::Plugins;
use super::process::Slot;
use crate::error::IoContext;
use crate::keychain::{self, Storage};
use crate::store::Store;
use crate::{Error, Result, fsutil};

/// The file of a plugin's secrets, with [`Storage::File`].
fn file(store: &Store, plugin: &str) -> PathBuf {
    store.root().join("secrets").join("plugins").join(format!("{plugin}.json"))
}

/// `dir`, and the folders above it it needs, made for the user alone.
fn private_dir(dir: &Path) -> Result<()> {
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
    builder.create(dir).at(dir)
}

/// The keychain failing, and the way round it.
fn keychain_error(e: keyring::Error) -> Error {
    Error::External {
        command: "the system keychain".into(),
        message: format!(
            "{e}; start locallibrary with LOCALLIBRARY_SESSION_STORE=file to keep secrets in files only you can read"
        ),
    }
}

impl Slot {
    fn entry(&self, store: &Store, key: &str) -> Result<keyring_core::Entry> {
        let service = format!("locallibrary-plugin-{}", self.manifest.id);
        keychain::entry(&service, &format!("{key}@{}", store.id()?)).map_err(keychain_error)
    }

    /// The secrets file's, by key. One where the first builds with secrets kept it, in the
    /// plugin's own folder, is moved out of it first.
    fn secrets_file(&self, store: &Store) -> Result<IndexMap<String, String>> {
        let path = file(store, &self.manifest.id);
        let old = store.root().join("plugins").join(&self.manifest.id).join("secrets.json");
        if old.is_file() && !path.exists() {
            private_dir(path.parent().expect("in a folder"))?;
            std::fs::rename(&old, &path).at(&old)?;
        }
        Ok(fsutil::read_json_opt(&path)?.unwrap_or_default())
    }

    /// The plugin's secret setting `key`, if it's set.
    pub(crate) fn secret(&self, store: &Store, key: &str) -> Result<Option<String>> {
        match self.host.options.secrets {
            Storage::Keychain => match self.entry(store, key)?.get_password() {
                Ok(secret) => Ok(Some(secret)),
                Err(keyring::Error::NoEntry) => Ok(None),
                Err(e) => Err(keychain_error(e)),
            },
            Storage::File => Ok(self.secrets_file(store)?.shift_remove(key)),
        }
    }

    /// Keeps `secret` as the plugin's secret setting `key`, or with `None` forgets it.
    pub(crate) fn set_secret(&self, store: &Store, key: &str, secret: Option<&str>) -> Result<()> {
        if let Storage::Keychain = self.host.options.secrets {
            let entry = self.entry(store, key)?;
            return match secret {
                Some(secret) => entry.set_password(secret),
                None => keychain::delete(&entry),
            }
            .map_err(keychain_error);
        }
        // One that can't be read has nothing usable in it: it's written anew.
        let mut all = self.secrets_file(store).unwrap_or_default();
        match secret {
            Some(secret) => all.insert(key.into(), secret.into()),
            None => all.shift_remove(key),
        };
        let path = file(store, &self.manifest.id);
        if all.is_empty() {
            return fsutil::remove_if_exists(&path).map(drop);
        }
        let mut bytes = serde_json::to_vec_pretty(&all).map_err(|e| Error::invalid(e.to_string()))?;
        bytes.push(b'\n');
        private_dir(path.parent().expect("in a folder"))?;
        fsutil::write_private(&path, &bytes)
    }
}

impl Plugins {
    /// Whether each of a plugin's secret settings is set (never what it is). Without a store,
    /// none is.
    pub fn secrets(&self, plugin: &str, store: Option<&Store>) -> Result<IndexMap<String, bool>> {
        let slot = self.slot(plugin)?;
        let secrets = slot.manifest.contributes.settings.iter().filter(|s| s.kind == SettingType::Secret);
        secrets
            .map(|s| {
                let set = store.map(|store| slot.secret(store, &s.key)).transpose()?.flatten().is_some();
                Ok((s.key.clone(), set))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests;
