//! What analyzer plugins work out for tracks (their `track/analyze` fields), kept on the
//! computer for the hosts to show.
//!
//! Kept like [`crate::audio_cache`]'s results, a small file per audio file keyed by the file's
//! name, size and modification time, so the same file on another drive, or exported again, keeps
//! its values, and a changed file's are never read. Each analyzer of each plugin version has a
//! folder, `<store>/cache/fields/<plugin>/<version>/<analyzer>/`. Readers say which versions
//! they read: the plugin host's, those installed.

use std::path::{Path, PathBuf};

use indexmap::IndexMap;

use crate::audio_cache::{CACHE_DIR, Cache, Kind};
use crate::store::Store;
use crate::{Error, Result};

/// One field's value, of the types a plugin's manifest declares.
pub use loclib_plugin::FieldValue;

/// An analyzer's fields for one track, by name.
pub type Fields = IndexMap<String, FieldValue>;

/// The values one analyzer of one plugin version worked out for one audio file.
struct Values;

impl Kind for Values {
    const DIR: &'static str = "fields";
    const EXT: &'static str = "json";
    const WHAT: &'static str = "a plugin's fields";
    /// The files' own format; the plugin's version is in their folder.
    const VERSION: u32 = 1;
    type Value = Fields;

    fn encode(fields: &Fields) -> Vec<u8> {
        serde_json::to_vec(fields).expect("fields serialise")
    }

    fn decode(bytes: &[u8]) -> Option<Fields> {
        serde_json::from_slice(bytes).ok()
    }
}

/// One analyzer of a plugin, at a version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Analyzer {
    pub plugin: String,
    pub version: String,
    pub analyzer: String,
}

/// A track's fields by plugin, then analyzer.
pub type PluginsFields = IndexMap<String, IndexMap<String, Fields>>;

/// The fields every analyzer plugin worked out, `<store>/cache/fields`.
pub struct FieldStore {
    root: PathBuf,
}

/// Whether `s` can name a folder: plugin ids, versions and analyzers are short names
/// (`analysis`, `0.1.0`, `tempo`).
fn is_name(s: &str) -> bool {
    !s.is_empty() && !s.starts_with('.') && s.chars().all(|c| c.is_ascii_alphanumeric() || "._+-".contains(c))
}

impl FieldStore {
    pub fn in_store(store: &Store) -> FieldStore {
        FieldStore { root: store.root().join(CACHE_DIR).join(Values::DIR) }
    }

    fn values(&self, a: &Analyzer) -> Cache<Values> {
        Cache::new(self.root.join(&a.plugin).join(&a.version).join(&a.analyzer))
    }

    /// Keep what `a` worked out for the audio file at `audio`, beside the plugin's other
    /// analyzers' values.
    pub fn put(&self, audio: &Path, a: &Analyzer, fields: &Fields) -> Result<()> {
        if let Some(bad) = [&a.plugin, &a.version, &a.analyzer].into_iter().find(|s| !is_name(s)) {
            return Err(Error::invalid(format!("{bad:?} isn't a plugin id, version or analyzer")));
        }
        if let Some((name, _)) = fields.iter().find(|(_, v)| matches!(v, FieldValue::Number(n) if !n.is_finite())) {
            return Err(Error::invalid(format!("{}'s field {name:?} isn't a finite number", a.plugin)));
        }
        let key = Cache::<Values>::key(audio).ok_or_else(|| Error::NotFound(audio.display().to_string()))?;
        self.values(a).put(&key, fields)
    }

    /// What each of `analyzers` worked out for the audio file at `audio`, by plugin then
    /// analyzer, leaving out those with nothing for it as it is now.
    pub fn get(&self, audio: &Path, analyzers: &[Analyzer]) -> PluginsFields {
        let mut out = PluginsFields::new();
        let Some(key) = Cache::<Values>::key(audio) else { return out };
        for a in analyzers {
            if let Some(fields) = self.values(a).get(&key) {
                out.entry(a.plugin.clone()).or_default().insert(a.analyzer.clone(), fields);
            }
        }
        out
    }
}

#[cfg(test)]
mod tests;
