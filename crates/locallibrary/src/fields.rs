//! What analyzer plugins work out for tracks (their `track/analyze` fields), kept on the
//! computer for the hosts to show.
//!
//! Kept like [`crate::audio_cache`]'s results, a small file per audio file keyed by the file's
//! name, size and modification time, so the same file on another drive, or exported again, keeps
//! its values, and a changed file's are never read. Each analyzer of each plugin version has a
//! folder, `<store>/cache/fields/<plugin>/<version>/<analyzer>/`. Readers say which versions
//! they read (the plugin host, those installed); `<plugin>/.version` names the version that
//! wrote last, for readers that don't know.

use std::fs;
use std::path::{Path, PathBuf};

use indexmap::IndexMap;

use crate::audio_cache::{CACHE_DIR, Cache, Kind};
use crate::store::Store;
use crate::{Error, Result, fsutil};

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

/// The file in a plugin's folder naming the version that wrote last (never a version's folder:
/// those don't start with a dot).
const LAST_WRITTEN: &str = ".version";

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

/// The names of the folders in `dir` that [`is_name`] allows, sorted.
fn names_in(dir: &Path) -> Vec<String> {
    let mut out: Vec<String> = fs::read_dir(dir)
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok()?.file_name().into_string().ok())
        .filter(|n| is_name(n) && dir.join(n).is_dir())
        .collect();
    out.sort();
    out
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
        self.values(a).put(&key, fields)?;
        let last = self.root.join(&a.plugin).join(LAST_WRITTEN);
        if fs::read_to_string(&last).ok().as_deref() != Some(a.version.as_str()) {
            fsutil::write_deferred(&last, a.version.as_bytes())?;
        }
        Ok(())
    }

    /// The analyzers with values kept, of each plugin's version that wrote last (only `plugin`'s
    /// when given), by plugin and analyzer: for readers that don't know which versions are
    /// installed. A new version's first value hides the older version's of every other track.
    pub fn last_written(&self, plugin: Option<&str>) -> Vec<Analyzer> {
        let mut out = Vec::new();
        for p in names_in(&self.root).into_iter().filter(|p| plugin.is_none_or(|o| o == p)) {
            let Ok(version) = fs::read_to_string(self.root.join(&p).join(LAST_WRITTEN)) else { continue };
            if !is_name(&version) {
                continue;
            }
            for analyzer in names_in(&self.root.join(&p).join(&version)) {
                out.push(Analyzer { plugin: p.clone(), version: version.clone(), analyzer });
            }
        }
        out
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
