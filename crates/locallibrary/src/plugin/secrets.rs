//! Plugins' secret settings: in the system's keychain, an item for each plugin, key and store
//! (service `locallibrary-plugin-<id>`, account `<key>@<store id>`), or with [`Storage::File`] in
//! `<store>/secrets/plugins/<id>.json`, readable only by the user. Never in `settings.json`, a
//! hook's context, `plugins.describe`, what `plugin settings` shows or a log: the host only fills
//! them into the plugin's `net/fetch` requests. A plugin runs as the user and isn't sandboxed, so
//! this is what the host does with them, not a wall the system keeps: their file is kept apart
//! from the plugin's own folder (where it runs, and its requests' scratch folders are) and from
//! everything the host sends it, but nothing stops the plugin reading it as the user could.
//!
//! Where a secret is kept is a [`Place`], so that other secrets a store keeps can be kept the same
//! way.

use std::path::{Path, PathBuf};

use indexmap::IndexMap;
use loclib_plugin::SettingType;

use super::Plugins;
use super::process::Plugin;
use crate::error::IoContext;
use crate::keychain::{self, Storage};
use crate::store::Store;
use crate::{Error, Result, fsutil};

/// Where a secret is kept in `store`: the keychain item `<key>@<store id>` of `service`, or with
/// [`Storage::File`] `key` in the JSON object in `file`, readable only by the user.
pub(super) struct Place<'a> {
    pub store: &'a Store,
    pub service: String,
    pub file: PathBuf,
    pub key: String,
    /// Where `file` was kept before, moved to `file` the first time it's read.
    pub moved_from: Option<PathBuf>,
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

/// `place`'s keychain item.
fn entry(place: &Place) -> Result<keyring_core::Entry> {
    keychain::entry(&place.service, &format!("{}@{}", place.key, place.store.id()?)).map_err(keychain_error)
}

/// The secrets in `place`'s file, by key.
fn secrets_file(place: &Place) -> Result<IndexMap<String, String>> {
    if let Some(old) = place.moved_from.as_deref().filter(|old| old.is_file() && !place.file.exists()) {
        private_dir(place.file.parent().expect("in a folder"))?;
        std::fs::rename(old, &place.file).at(old)?;
    }
    Ok(fsutil::read_json_opt(&place.file)?.unwrap_or_default())
}

/// The secret kept at `place`, if there's one.
pub(super) fn secret(storage: Storage, place: &Place) -> Result<Option<String>> {
    match storage {
        Storage::Keychain => match entry(place)?.get_password() {
            Ok(secret) => Ok(Some(secret)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(keychain_error(e)),
        },
        Storage::File => Ok(secrets_file(place)?.shift_remove(&place.key)),
    }
}

/// Keeps `secret` at `place`, or with `None` forgets it.
pub(super) fn set_secret(storage: Storage, place: &Place, secret: Option<&str>) -> Result<()> {
    if let Storage::Keychain = storage {
        let entry = entry(place)?;
        return match secret {
            Some(secret) => entry.set_password(secret),
            None => keychain::delete(&entry),
        }
        .map_err(keychain_error);
    }
    // One that can't be read has nothing usable in it: it's written anew.
    let mut all = secrets_file(place).unwrap_or_default();
    match secret {
        Some(secret) => all.insert(place.key.clone(), secret.into()),
        None => all.shift_remove(&place.key),
    };
    if all.is_empty() {
        return fsutil::remove_if_exists(&place.file).map(drop);
    }
    let mut bytes = serde_json::to_vec_pretty(&all).map_err(|e| Error::invalid(e.to_string()))?;
    bytes.push(b'\n');
    private_dir(place.file.parent().expect("in a folder"))?;
    fsutil::write_private(&place.file, &bytes)
}

impl Plugin {
    /// Where its secret setting `key` is kept in `store`. Its secrets file was in the plugin's
    /// own folder in the first builds with secrets.
    fn place<'a>(&self, store: &'a Store, key: &str) -> Place<'a> {
        let id = &self.manifest.id;
        Place {
            store,
            service: format!("locallibrary-plugin-{id}"),
            file: store.root().join("secrets").join("plugins").join(format!("{id}.json")),
            key: key.into(),
            moved_from: Some(store.root().join("plugins").join(id).join("secrets.json")),
        }
    }

    /// The plugin's secret setting `key`, if it's set.
    pub(crate) fn secret(&self, store: &Store, key: &str) -> Result<Option<String>> {
        secret(self.host.options.secrets, &self.place(store, key))
    }

    /// Keeps `secret` as the plugin's secret setting `key`, or with `None` forgets it.
    pub(crate) fn set_secret(&self, store: &Store, key: &str, secret: Option<&str>) -> Result<()> {
        set_secret(self.host.options.secrets, &self.place(store, key), secret)
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
