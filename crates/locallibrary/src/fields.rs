//! What analyzer plugins work out for tracks (their `track/analyze` fields), kept on the
//! computer for the hosts to show.
//!
//! Kept like [`crate::audio_cache`]'s results, a small file per audio file keyed by the file's
//! name, size and modification time, so the same file on another drive, or exported again, keeps
//! its values, and a changed file's are never read. Each version of each analyzer (its own, not
//! its plugin's) has a folder, `<store>/cache/fields/<plugin>/<analyzer>/<version>/`. Readers say
//! which versions they read: the plugin host's, those installed. A field may be null: worked out,
//! with nothing to show (a field left out wasn't worked out).
//!
//! An analyzer's beat grid, phrases and waveform (structured fields) aren't columns: they're kept
//! apart, in the [`overlay`] store.
//!
//! Filters and sorts name a field by its plugin's id and its own (`sound.bpm`): a plugin's
//! analyzers never share a field name. [`TrackValues`] has a library's, for them. What
//! locallibrary itself reads (a track's key, its tempo, its level) it reads by role, whichever
//! analyzer has it ([`by_role`]).

pub mod overlay;
pub mod roles;

use std::path::{Path, PathBuf};

use indexmap::IndexMap;
use rustc_hash::FxHashMap;

use crate::audio_cache::{CACHE_DIR, Cache, Kind};
use crate::device::Device;
use crate::model::Library;
use crate::store::Store;
use crate::{Error, Result, paths};

/// One field's value, and the types a plugin's manifest declares.
pub use loclib_plugin::{FieldType, FieldValue};
pub use overlay::{Overlay, OverlayStore};
pub use roles::{Role, by_role};

/// An analyzer's fields for one track, by name.
pub type Fields = IndexMap<String, FieldValue>;

/// The values one version of an analyzer worked out for one audio file.
struct Values;

impl Kind for Values {
    const DIR: &'static str = "fields";
    const EXT: &'static str = "json";
    const WHAT: &'static str = "a plugin's fields";
    /// The files' own format; the analyzer's version is in their folder.
    const VERSION: u32 = 1;
    type Value = Fields;

    fn encode(fields: &Fields) -> Vec<u8> {
        serde_json::to_vec(fields).expect("fields serialise")
    }

    fn decode(bytes: &[u8]) -> Option<Fields> {
        serde_json::from_slice(bytes).ok()
    }
}

/// One analyzer of a plugin, at its version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Analyzer {
    pub plugin: String,
    pub analyzer: String,
    pub version: String,
}

/// A track's fields by plugin, then analyzer.
pub type PluginsFields = IndexMap<String, IndexMap<String, Fields>>;

/// The fields every analyzer plugin worked out, `<store>/cache/fields`.
pub struct FieldStore {
    root: PathBuf,
}

/// Whether `s` can name a folder: plugin ids, analyzers and their versions are short names
/// (`sound`, `tempo-key-loudness`, `3`).
pub(crate) fn is_name(s: &str) -> bool {
    !s.is_empty() && !s.starts_with('.') && s.chars().all(|c| c.is_ascii_alphanumeric() || "._+-".contains(c))
}

impl FieldStore {
    pub fn in_store(store: &Store) -> FieldStore {
        FieldStore { root: store.root().join(CACHE_DIR).join(Values::DIR) }
    }

    fn values(&self, a: &Analyzer) -> Cache<Values> {
        Cache::new(self.root.join(&a.plugin).join(&a.analyzer).join(&a.version))
    }

    /// Folders nothing reads any more, for `store gc`: values kept by their plugin's version
    /// (`<plugin>/<plugin version>/<analyzer>/`) before each analyzer had a version of its own. A
    /// plugin's version starts with a digit; what's in its place now, an analyzer's name, never
    /// does.
    pub(crate) fn unread(&self) -> Vec<PathBuf> {
        let folders = |dir: &Path| {
            let entries = std::fs::read_dir(dir).into_iter().flatten().flatten();
            entries.filter(|e| e.file_type().is_ok_and(|t| t.is_dir())).map(|e| e.path())
        };
        let by_version =
            |p: &PathBuf| p.file_name().is_some_and(|n| n.to_string_lossy().starts_with(|c: char| c.is_ascii_digit()));
        let mut unread: Vec<PathBuf> =
            folders(&self.root).flat_map(|plugin| folders(&plugin)).filter(by_version).collect();
        unread.sort();
        unread
    }

    /// Keep what `a` worked out for the audio file at `audio`, beside the plugin's other
    /// analyzers' values.
    pub fn put(&self, audio: &Path, a: &Analyzer, fields: &Fields) -> Result<()> {
        if let Some(bad) = [&a.plugin, &a.analyzer, &a.version].into_iter().find(|s| !is_name(s)) {
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

/// The fields the analyzers installed declare, by name as filters and sorts name them
/// (`sound.bpm`), with their types, in the order the plugins declare them.
pub type Declared = IndexMap<String, FieldType>;

/// Where what analyzers worked out is read for filters and sorts naming their fields: the store
/// (none: nothing was worked out), and the analyzers installed with the fields they declare.
#[derive(Clone, Copy)]
pub struct Source<'a> {
    pub store: Option<&'a Store>,
    pub analyzers: &'a [Analyzer],
    pub declared: &'a Declared,
}

/// The analyzers installed and the fields they declare, and the store what they worked out is
/// kept in: what filters and sorts naming their fields (`sound.bpm:>120`) read. Plugins that were
/// refused have none.
pub struct Installed {
    store: Option<Store>,
    analyzers: Vec<Analyzer>,
    declared: Declared,
}

impl Installed {
    /// The analyzers of `plugins` (none without), what they worked out kept in `store`.
    pub fn new(store: Option<Store>, plugins: Option<&crate::plugin::Plugins>) -> Installed {
        Installed {
            store,
            analyzers: plugins.map(crate::plugin::Plugins::analyzers).unwrap_or_default(),
            declared: plugins.map(crate::plugin::Plugins::fields).unwrap_or_default(),
        }
    }

    /// [`Installed::new`] for the plugins the app hosts ([`crate::plugin::set_hosted`]), in the
    /// store drive lookups consult ([`crate::api::store_root`]): what edits' selectors and sorts
    /// and smart playlists' rules read.
    pub(crate) fn hosted() -> Installed {
        Installed::new(crate::api::registry(), crate::plugin::hosted())
    }

    pub fn source(&self) -> Source<'_> {
        Source { store: self.store.as_ref(), analyzers: &self.analyzers, declared: &self.declared }
    }
}

/// What the analyzers installed worked out for a library's tracks, as filters and sorts read it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TrackValues {
    pub declared: Declared,
    /// Each track's values by its id, then field (`sound.bpm`); a track with none is left out.
    pub tracks: FxHashMap<u32, Fields>,
}

impl TrackValues {
    /// What `source` has for the tracks of `library`, the library of `device`.
    pub fn read(source: Source<'_>, device: &Device, library: &Library) -> TrackValues {
        let store = source.store.map(FieldStore::in_store).filter(|_| !source.analyzers.is_empty());
        let tracks = store.map_or_else(FxHashMap::default, |fields| {
            let values = |t: &crate::model::Track| {
                let audio = device.checked_path(&paths::key(&t.file_path)).ok()?;
                let named: Fields = (fields.get(&audio, source.analyzers).into_iter())
                    .flat_map(|(plugin, analyzers)| {
                        analyzers.into_values().flatten().map(move |(name, v)| (format!("{plugin}.{name}"), v))
                    })
                    .collect();
                (!named.is_empty()).then_some((t.id, named))
            };
            library.tracks.iter().filter_map(values).collect()
        });
        TrackValues { declared: source.declared.clone(), tracks }
    }

    /// Track `id`'s value of the field `name`, if it has one (null, when it was worked out with
    /// nothing to show).
    pub fn get(&self, id: u32, name: &str) -> Option<&FieldValue> {
        self.tracks.get(&id)?.get(name)
    }

    /// Track `id`'s value of the field `name`, if it has one to show (not null): what sorts
    /// compare, the rest going last.
    pub fn shown(&self, id: u32, name: &str) -> Option<&FieldValue> {
        self.get(id, name).filter(|v| **v != FieldValue::Null)
    }
}

#[cfg(test)]
mod tests;
