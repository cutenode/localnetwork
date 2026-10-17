//! Plugins' settings, kept in the store (`<store>/plugins/<id>/settings.json`) and handed to
//! each hook in its context.

use std::path::PathBuf;

use indexmap::IndexMap;
use serde_json::Value;

use super::Plugins;
use super::process::Slot;
use crate::error::IoContext;
use crate::store::Store;
use crate::{Error, Result, fsutil};

fn file(store: &Store, plugin: &str) -> PathBuf {
    store.root().join("plugins").join(plugin).join("settings.json")
}

impl Slot {
    /// What's kept for the plugin, as it was written.
    fn saved(&self, store: &Store) -> Result<IndexMap<String, Value>> {
        Ok(fsutil::read_json_opt(&file(store, &self.manifest.id))?.unwrap_or_default())
    }

    /// The plugin's settings, by key: each one set (when what's kept still fits it), else its
    /// default; one with neither is left out. What's kept that can't be read is logged, and the
    /// defaults are used.
    pub(crate) fn settings(&self, store: Option<&Store>) -> IndexMap<String, Value> {
        let saved = store.map(|s| self.saved(s)).transpose().unwrap_or_else(|e| {
            self.host.log(&self.manifest.id, &format!("its settings are its defaults: {e}"));
            None
        });
        let saved = saved.unwrap_or_default();
        let settings = self.manifest.contributes.settings.iter().filter_map(|s| {
            let value = saved.get(&s.key).filter(|v| s.accepts(v)).or(s.default.as_ref())?;
            Some((s.key.clone(), value.clone()))
        });
        settings.collect()
    }
}

impl Plugins {
    /// A plugin's settings, as its hooks get them: each one set (when what's kept still fits
    /// it), else its default. Without a store, or one whose file for them can't be read, the
    /// defaults.
    pub fn settings(&self, plugin: &str, store: Option<&Store>) -> Result<IndexMap<String, Value>> {
        Ok(self.slot(plugin)?.settings(store))
    }

    /// Sets one of a plugin's settings to `value`, which must be of its type, or with `None`
    /// puts it back to its default. Answers its settings then. A file for them that can't be
    /// read is written anew.
    pub fn set_setting(
        &self,
        plugin: &str,
        store: &Store,
        key: &str,
        value: Option<Value>,
    ) -> Result<IndexMap<String, Value>> {
        let slot = self.slot(plugin)?;
        let manifest = &slot.manifest;
        let setting = manifest.contributes.settings.iter().find(|s| s.key == key);
        let setting =
            setting.ok_or_else(|| Error::NotFound(format!("the {} plugin's setting {key:?}", manifest.id)))?;
        let mut saved = slot.saved(store).unwrap_or_default();
        match value {
            Some(v) if !setting.accepts(&v) => {
                let kind = serde_json::to_value(setting.kind).unwrap_or_default();
                return Err(Error::invalid(match setting.choices.is_empty() {
                    true => format!("{key} is a {}, not {v}", kind.as_str().unwrap_or("?")),
                    false => format!("{key} is one of {}, not {v}", setting.choices.join(", ")),
                }));
            }
            Some(v) => saved.insert(key.into(), v),
            None => saved.shift_remove(key),
        };
        let path = file(store, &manifest.id);
        let dir = path.parent().expect("in a folder");
        std::fs::create_dir_all(dir).at(dir)?;
        fsutil::write_json_atomic(&path, &saved)?;
        Ok(slot.settings(Some(store)))
    }
}
