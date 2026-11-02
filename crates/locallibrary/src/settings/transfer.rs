//! Everything set here in one file, to keep a copy or take it to another computer: the My Settings
//! profile, the players `check` remembers, the profile's preferences, the player's preferences
//! (never where it was: the drive, the list, the queue, its panels), the host's settings for
//! plugins and each plugin's own (its services' switches and the player's keys for its commands
//! included). A plugin's settings go as the JSON object the store keeps for it, installed here or
//! not. Secrets never do: they're kept apart (see [`crate::plugin`]).
//!
//! Importing one ([`merge`]) adds what's missing here and leaves what's the same. Where a value
//! differs the one here stays, unless `replace` says to take the file's. Lists of names (tracks
//! and artists the profile hides, players) get the names they lack; with `replace`, the file's list.

use std::path::{Path, PathBuf};

use indexmap::IndexMap;
use schemars::JsonSchema;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use super::SettingsProfile;
use crate::equipment::Remembered;
use crate::profile::{PREFS_FILE, ProfilePrefs};
use crate::query::SortKey;
use crate::store::{PLAYER_FILE, Store, player_fallback};
use crate::{Error, Result, fsutil, plugin};

/// The file's format.
pub const VERSION: u32 = 1;

/// What [`export`] writes and [`merge`] reads. Each section is as the store keeps it, with empty
/// maps and lists left out.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SettingsExport {
    /// The file's format, [`VERSION`].
    pub version: u32,
    /// The My Settings profile: `base`, and per-drive `drives` overrides by drive id.
    #[serde(default)]
    pub my_settings: Map<String, Value>,
    /// The player models `check` uses: `default`, and `libraries`' own by library id.
    #[serde(default)]
    pub players: Map<String, Value>,
    /// The profile's preferences: its sections, precision, pinned and hidden tracks, hidden
    /// artists, genre spellings and headline.
    #[serde(default)]
    pub profile: Map<String, Value>,
    /// The player's preferences ([`PlayerPrefs`]): its theme, columns, sort, visualiser and
    /// playback modes.
    #[serde(default)]
    pub player: Map<String, Value>,
    /// The host's settings for plugins: which analyzer each field role is read from, when a
    /// provider's candidate is picked, and whether each check runs.
    #[serde(default)]
    pub plugin_host: Map<String, Value>,
    /// Each plugin's settings by its id, as it keeps them (`services.<name>` and `keys.<name>`
    /// among them).
    #[serde(default)]
    pub plugins: Map<String, Value>,
}

impl Default for SettingsExport {
    fn default() -> Self {
        SettingsExport {
            version: VERSION,
            my_settings: Map::new(),
            players: Map::new(),
            profile: Map::new(),
            player: Map::new(),
            plugin_host: Map::new(),
            plugins: Map::new(),
        }
    }
}

/// The choices the player remembers in [`PLAYER_FILE`] that are the user's preferences, as it
/// keeps them there (the CLI's `tui::state::Saved`, whose other fields are where it was: the drive
/// and list open, the view, the volume, its panels, the output, the crate and the queue). Each is
/// left out when it isn't set.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct PlayerPrefs {
    /// A plugin's theme, `<plugin>.<name>`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub theme: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sort: Option<SortKey>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub search_all: Option<bool>,
    /// The visualiser's name (`spectrum`, `deck`...).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vis: Option<String>,
    /// The track list's rating column.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ratings: Option<bool>,
    /// The track list's My Tags column.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags_column: Option<bool>,
    /// The track list's columns of plugins' fields (`sound.bpm`), in order.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub field_columns: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shuffle: Option<bool>,
    /// `off`, `all` or `one`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repeat: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quantize: Option<bool>,
    /// Single play: stop after each track.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub single: Option<bool>,
    /// Auto cue: start tracks at their first memory cue or beat.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auto_cue: Option<bool>,
}

/// The file the player reads what it remembers from: the store's, else the one beside it (see
/// [`player_fallback`]).
fn player_file(store: &Store) -> Option<PathBuf> {
    let file = store.root().join(PLAYER_FILE);
    match file.is_file() {
        true => Some(file),
        false => player_fallback(store.root()).map(|d| d.join(PLAYER_FILE)),
    }
}

/// What the player remembers, as a JSON object (none when it's missing or can't be read: the
/// player starts afresh then too).
fn player_state(store: &Store) -> Map<String, Value> {
    let read = player_file(store).and_then(|f| fsutil::read_json_opt::<Map<String, Value>>(&f).ok().flatten());
    read.unwrap_or_default()
}

/// What importing does with one of the file's values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
#[schemars(rename = "SettingsFileAction")]
#[serde(rename_all = "snake_case")]
pub enum Action {
    /// It's missing here, or adds names to a list: it's added.
    Add,
    /// It differs, and the one here stays.
    Keep,
    /// It differs, and the file's takes its place (`replace`).
    Replace,
}

/// One of the file's values that isn't as it is here.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
#[schemars(rename = "SettingsFileChange")]
pub struct Change {
    /// The section, then each key down to the value: `["plugins", "sound", "keys.analyze"]`.
    pub path: Vec<String>,
    pub action: Action,
    /// The value here (`None`: not set).
    pub current: Option<Value>,
    /// The file's value; for names added to a list, the names it adds.
    pub imported: Value,
}

/// What importing a file would do: its values that aren't as they are here, and what each
/// section becomes.
#[derive(Debug, Clone)]
pub struct Merged {
    pub changes: Vec<Change>,
    /// The file's values that are as they are here already.
    pub unchanged: usize,
    /// The sections (and plugins, `plugins.<id>`) importing writes, as it writes them.
    writes: Vec<(Target, Map<String, Value>)>,
}

impl Merged {
    /// Write what importing changes into `store`.
    pub fn write(&self, store: &Store) -> Result<()> {
        for (target, value) in &self.writes {
            target.write(store, value)?;
        }
        Ok(())
    }
}

/// Where a section lives in the store.
#[derive(Debug, Clone, PartialEq)]
enum Target {
    MySettings,
    Players,
    Profile,
    Player,
    PluginHost,
    Plugin(String),
}

impl Target {
    fn file(&self) -> String {
        match self {
            Target::MySettings => "settings.json".into(),
            Target::Players => "players.json".into(),
            Target::Profile => PREFS_FILE.into(),
            Target::Player => PLAYER_FILE.into(),
            Target::PluginHost => "plugins/settings.json".into(),
            Target::Plugin(id) => format!("plugins/{id}/settings.json"),
        }
    }

    fn write(&self, store: &Store, value: &Map<String, Value>) -> Result<()> {
        match self {
            Target::MySettings => {
                // Kept in order, as `settings set` keeps them.
                let mut profile: SettingsProfile = typed(value)?;
                profile.base.sort_unstable_keys();
                profile.drives.values_mut().for_each(|o| o.values.sort_unstable_keys());
                store.save_settings_profile(&profile)
            }
            Target::Players => store.save_player_models(&typed(value)?),
            Target::Profile => typed::<ProfilePrefs>(value)?.save(store),
            Target::Player => {
                // Into what the player remembers, where it was kept as it was.
                let mut state = player_state(store);
                state.extend(untyped(&typed::<PlayerPrefs>(value)?));
                fsutil::write_json_atomic(&store.root().join(PLAYER_FILE), &state)
            }
            Target::PluginHost => plugin::save_host_settings(store, &typed(value)?),
            Target::Plugin(id) => plugin::save_settings(store, id, &typed(value)?),
        }
    }
}

/// The sections, by their names in the file.
const SECTIONS: [&str; 6] = ["my_settings", "players", "profile", "player", "plugin_host", "plugins"];

impl SettingsExport {
    /// The store files its sections are kept in, below the store's folder: what importing it
    /// reads and may write.
    pub fn files(&self) -> Vec<String> {
        let sections = [
            (Target::MySettings, &self.my_settings),
            (Target::Players, &self.players),
            (Target::Profile, &self.profile),
            (Target::Player, &self.player),
            (Target::PluginHost, &self.plugin_host),
        ];
        let sections = sections.into_iter().filter(|(_, s)| !s.is_empty()).map(|(t, _)| t.file());
        sections.chain(self.plugins.keys().map(|id| Target::Plugin(id.clone()).file())).collect()
    }

    fn section(&self, name: &str) -> &Map<String, Value> {
        match name {
            "my_settings" => &self.my_settings,
            "players" => &self.players,
            "profile" => &self.profile,
            "player" => &self.player,
            "plugin_host" => &self.plugin_host,
            _ => &self.plugins,
        }
    }

    fn section_mut(&mut self, name: &str) -> &mut Map<String, Value> {
        match name {
            "my_settings" => &mut self.my_settings,
            "players" => &mut self.players,
            "profile" => &mut self.profile,
            "player" => &mut self.player,
            "plugin_host" => &mut self.plugin_host,
            _ => &mut self.plugins,
        }
    }

    /// Each section as it's kept: values checked (My Settings in their canonical spelling, in
    /// order), empty maps and lists left out.
    fn normalized(mut self) -> Result<SettingsExport> {
        if self.version != VERSION {
            let why = if self.version > VERSION { "a newer locallibrary's" } else { "not one locallibrary wrote" };
            return Err(Error::invalid(format!("its version is {}: {why} (this one reads {VERSION})", self.version)));
        }
        let mut profile: SettingsProfile = typed(&self.my_settings).map_err(|e| in_section("my_settings", e))?;
        let canonical = |values: &mut IndexMap<String, String>| -> Result<()> {
            let mut out = IndexMap::new();
            for (k, v) in values.iter() {
                let (def, choice) = super::validate(k, v)?;
                out.insert(def.key.to_string(), choice.id.to_string());
            }
            out.sort_unstable_keys();
            *values = out;
            Ok(())
        };
        canonical(&mut profile.base).map_err(|e| in_section("my_settings", e))?;
        for o in profile.drives.values_mut() {
            canonical(&mut o.values).map_err(|e| in_section("my_settings", e))?;
        }
        profile.drives.retain(|_, o| !o.values.is_empty());
        self.my_settings = untyped(&profile);
        self.players = retyped::<Remembered>("players", &self.players)?;
        self.profile = retyped::<ProfilePrefs>("profile", &self.profile)?;
        // Only the preferences: what else a hand-made file has of the player's is left out.
        self.player = retyped::<PlayerPrefs>("player", &self.player)?;
        if let Some((id, _)) = self.plugins.iter().find(|(id, v)| !crate::fields::is_name(id) || !v.is_object()) {
            return Err(in_section("plugins", Error::invalid(format!("{id:?} isn't a plugin's id with its settings"))));
        }
        self.plugins.sort_keys();
        for name in SECTIONS {
            prune(&mut vec![name.to_string()], self.section_mut(name));
        }
        Ok(self)
    }
}

fn in_section(name: &str, e: Error) -> Error {
    Error::invalid(format!("{name}: {e}"))
}

fn typed<T: DeserializeOwned>(map: &Map<String, Value>) -> Result<T> {
    serde_json::from_value(Value::Object(map.clone())).map_err(|e| Error::invalid(e.to_string()))
}

/// `map` read as a `T` and written back (checked, in `T`'s order), or left empty: an empty section
/// is one the store doesn't keep, not one with `T`'s defaults.
fn retyped<T: DeserializeOwned + Serialize>(name: &str, map: &Map<String, Value>) -> Result<Map<String, Value>> {
    match map.is_empty() {
        true => Ok(Map::new()),
        false => Ok(untyped(&typed::<T>(map).map_err(|e| in_section(name, e))?)),
    }
}

fn untyped<T: Serialize>(value: &T) -> Map<String, Value> {
    match serde_json::to_value(value) {
        Ok(Value::Object(map)) => map,
        _ => Map::new(),
    }
}

/// Whether the value at `path` is one value, compared whole, rather than a map to go into or a
/// list of names to add to: anything but a map or a list, a plugin's setting (whatever it holds),
/// and the profile's sections (which go out, not names to add).
fn is_value(path: &[String], v: &Value) -> bool {
    let path: Vec<&str> = path.iter().map(String::as_str).collect();
    match path[..] {
        ["plugins", _, _] | ["plugin_host", _] | ["profile", "sections"] => true,
        _ => !(v.is_object() || v.is_array()),
    }
}

/// Leaves out the maps and lists in `map` (at `path`) that are empty, once theirs are left out.
fn prune(path: &mut Vec<String>, map: &mut Map<String, Value>) {
    map.retain(|k, v| {
        path.push(k.clone());
        if !is_value(path, v)
            && let Value::Object(inner) = v
        {
            prune(path, inner);
        }
        let empty =
            !is_value(path, v) && (v.as_object().is_some_and(Map::is_empty) || v.as_array().is_some_and(Vec::is_empty));
        path.pop();
        !empty
    });
}

/// What the store keeps of a section, as the file has it (empty when it keeps none).
fn kept<T: DeserializeOwned + Serialize>(store: &Store, file: &str) -> Result<Map<String, Value>> {
    Ok(fsutil::read_json_opt::<T>(&store.root().join(file))?.map(|v| untyped(&v)).unwrap_or_default())
}

/// Everything set in `store`.
pub fn export(store: &Store) -> Result<SettingsExport> {
    let plugins =
        plugin::all_saved_settings(store)?.into_iter().map(|(id, s)| (id, Value::Object(s.into_iter().collect())));
    let doc = SettingsExport {
        version: VERSION,
        my_settings: kept::<SettingsProfile>(store, &Target::MySettings.file())?,
        players: kept::<Remembered>(store, &Target::Players.file())?,
        profile: kept::<ProfilePrefs>(store, &Target::Profile.file())?,
        // A value the player can't read means it starts afresh, so there are none to export.
        player: serde_json::from_value::<PlayerPrefs>(player_state(store).into())
            .map(|p| untyped(&p))
            .unwrap_or_default(),
        plugin_host: plugin::host_settings(Some(store)).into_iter().collect(),
        plugins: plugins.collect(),
    };
    doc.normalized()
}

/// The file at `path`, checked.
pub fn read(path: &Path) -> Result<SettingsExport> {
    let doc: SettingsExport = fsutil::read_json(path)?;
    doc.normalized().map_err(|e| Error::parse(path, e))
}

/// What importing `file` into `store` would do (see the module's docs).
pub fn merge(store: &Store, file: &SettingsExport, replace: bool) -> Result<Merged> {
    let here = export(store)?;
    let mut merged = Merged { changes: Vec::new(), unchanged: 0, writes: Vec::new() };
    for name in SECTIONS {
        let mut section = here.section(name).clone();
        let before = merged.changes.len();
        let mut path = vec![name.to_string()];
        add(&mut path, &mut section, file.section(name), replace, &mut merged);
        let written = merged.changes[before..].iter().filter(|c| c.action != Action::Keep);
        let mut ids: Vec<String> = written.map(|c| c.path.get(1).cloned().unwrap_or_default()).collect();
        if ids.is_empty() {
            continue;
        }
        let target = match name {
            "my_settings" => Target::MySettings,
            "players" => Target::Players,
            "profile" => Target::Profile,
            "player" => Target::Player,
            "plugin_host" => Target::PluginHost,
            _ => {
                ids.dedup();
                for id in ids {
                    let settings = section.get(&id).and_then(Value::as_object).cloned().unwrap_or_default();
                    merged.writes.push((Target::Plugin(id), settings));
                }
                continue;
            }
        };
        merged.writes.push((target, section));
    }
    Ok(merged)
}

/// Adds `file`'s values (at `path`) to `here`, noting each that isn't as it is here.
fn add(
    path: &mut Vec<String>,
    here: &mut Map<String, Value>,
    file: &Map<String, Value>,
    replace: bool,
    m: &mut Merged,
) {
    for (k, theirs) in file {
        path.push(k.clone());
        let value = is_value(path, theirs);
        let note = |m: &mut Merged, action, current: Option<&Value>, imported: Value| {
            m.changes.push(Change { path: path.clone(), action, current: current.cloned(), imported });
        };
        match here.get_mut(k) {
            None if !value && theirs.is_object() => {
                let mut inner = Map::new();
                add(path, &mut inner, theirs.as_object().expect("a map"), replace, m);
                here.insert(k.clone(), Value::Object(inner));
            }
            None => {
                note(m, Action::Add, None, theirs.clone());
                here.insert(k.clone(), theirs.clone());
            }
            Some(Value::Object(ours)) if !value && theirs.is_object() => {
                add(path, ours, theirs.as_object().expect("a map"), replace, m);
            }
            Some(ours) if *ours == *theirs => m.unchanged += 1,
            Some(ours) if !value && !replace && ours.is_array() && theirs.is_array() => {
                let list = ours.as_array_mut().expect("a list");
                let new: Vec<Value> =
                    theirs.as_array().expect("a list").iter().filter(|v| !list.contains(v)).cloned().collect();
                if new.is_empty() {
                    m.unchanged += 1;
                } else {
                    note(m, Action::Add, Some(&Value::Array(list.clone())), Value::Array(new.clone()));
                    list.extend(new);
                }
            }
            Some(ours) if replace => {
                note(m, Action::Replace, Some(ours), theirs.clone());
                *ours = theirs.clone();
            }
            Some(ours) => note(m, Action::Keep, Some(ours), theirs.clone()),
        }
        path.pop();
    }
}
