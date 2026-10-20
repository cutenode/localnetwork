//! `locallibrary-plugin.toml`: who a plugin is, what it may call back for and what it adds to
//! locallibrary. Hosts draw the contributions; plugins never ship UI code.

use std::borrow::Cow;
use std::fmt;
use std::path::Path;
use std::str::FromStr;

use indexmap::IndexMap;
use rustc_hash::FxHashSet;
use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use semver::{Version, VersionReq};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{Context, PROTOCOL_VERSION, RpcError};

/// Read tolerantly: a key this side doesn't know is left out, and [`Manifest::validate`] refuses
/// it only in a plugin locallibrary ships (see [`Manifest::unknown`]).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Manifest {
    /// Lowercase letters, digits, `-` and `_`, starting with a letter.
    pub id: String,
    pub name: String,
    /// The plugin's version (semver).
    pub version: String,
    /// The protocol versions it speaks, as a semver requirement (`^0.2`).
    pub protocol: String,
    /// What it may call back for (see [`Capability`]).
    #[serde(default)]
    pub capabilities: Vec<Capability>,
    #[serde(default)]
    pub contributes: Contributions,
    /// How often the host may send `net/fetch` requests to some of the hosts it talks to.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub net: Vec<NetLimit>,
    /// The keys [`Manifest::parse`] left out, where they were (`contributes.commands[0].colour`).
    #[serde(skip)]
    #[schemars(skip)]
    unknown: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Contributions {
    #[serde(default)]
    pub commands: Vec<Command>,
    #[serde(default)]
    pub operations: Vec<Operation>,
    #[serde(default)]
    pub analyzers: Vec<Analyzer>,
    #[serde(default)]
    pub exporters: Vec<Exporter>,
    #[serde(default)]
    pub settings: Vec<Setting>,
}

/// Run with `command/run`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Command {
    pub name: String,
    pub title: String,
    pub mode: Mode,
    /// Where hosts offer it (see [`Where`]).
    #[serde(default = "on_a_drive", rename = "where")]
    pub offered: Vec<Where>,
    /// A JSON Schema for its params (an object).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
    /// Its own place on the command line (see [`Operation::cli`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cli: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    /// Answers any JSON, shown as it is (`--json`) or not at all.
    Headless,
    /// Answers a [`crate::View`].
    View,
}

/// Where a host offers a command or an operation: in the menu of the tracks selected, of a
/// playlist, or of a drive. A host's palette has every one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Where {
    Track,
    Playlist,
    Drive,
}

impl Where {
    /// As a manifest names it (`track`).
    pub fn name(self) -> &'static str {
        match self {
            Where::Track => "track",
            Where::Playlist => "playlist",
            Where::Drive => "drive",
        }
    }
}

/// Where a command or an operation is offered when its manifest doesn't say: a drive's menu.
fn on_a_drive() -> Vec<Where> {
    vec![Where::Drive]
}

/// Planned with `op/plan`, applied by the host, which works out from the edits whether it's
/// destructive.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Operation {
    pub name: String,
    pub title: String,
    /// Where hosts offer it (see [`Where`]).
    #[serde(default = "on_a_drive", rename = "where")]
    pub offered: Vec<Where>,
    /// Planning takes a while: the host shows progress.
    #[serde(default)]
    pub long_running: bool,
    /// A JSON Schema for its params (an object).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
    /// Its own place on the command line, rather than `locallibrary <plugin> <name>`: a
    /// top-level command (`import`), or one in a group of the host's (`playlist import`).
    /// First-party plugins only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cli: Option<String>,
}

/// What a param's (or option's) JSON Schema says about giving it on the command line, under its
/// `x-cli` keyword: `{ flag = "from", one_based = true }`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct CliHints {
    /// Its flag, rather than the one its name makes (`--min-bpm` for `min_bpm`): a name.
    pub flag: Option<String>,
    /// An object whose keys are positions (`--choose 1=42`), counted from 1 on the command line
    /// and from 0 in the params.
    pub one_based: bool,
    /// Given without a flag, after the drive (`playlist import DRIVE FILE`): a string, and one
    /// param at most.
    pub positional: bool,
}

impl CliHints {
    /// `property`'s hints (none, without `x-cli`).
    pub fn of(property: &Value) -> Result<CliHints, String> {
        let Some(hints) = property.get("x-cli") else { return Ok(CliHints::default()) };
        let hints = CliHints::deserialize(hints).map_err(|e| e.to_string())?;
        if let Some(flag) = hints.flag.as_deref().filter(|f| !is_name(f)) {
            return Err(format!("its flag {flag:?} isn't a name: {NAME_RULE}"));
        }
        if hints.positional && property["type"] != "string" {
            return Err("only a string is positional".into());
        }
        if hints.one_based && property["type"] != "object" {
            return Err("only an object's keys are one_based".into());
        }
        Ok(hints)
    }
}

/// Whether `property` (a param's or option's JSON Schema) is a file or folder on the computer
/// (`x-path = true`), which must be given as an absolute path: a plugin runs in a folder of its
/// own (`<store>/plugins/<id>/work/`), and hosts in others.
pub fn is_path(property: &Value) -> bool {
    property.get("x-path") == Some(&Value::Bool(true))
}

/// What a host fills a param in with when it isn't given (its `x-from`): what the user has
/// selected where they run it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Selection {
    /// `selection.tracks`: the tracks selected, by id (a list of integers).
    #[serde(rename = "selection.tracks")]
    Tracks,
    /// `selection.playlist`: the playlist open, by path (a string).
    #[serde(rename = "selection.playlist")]
    Playlist,
    /// `selection.drive`: the drive, by id or name (a string).
    #[serde(rename = "selection.drive")]
    Drive,
}

impl Selection {
    /// `property`'s, if it has one: of a type that holds it.
    pub fn of(property: &Value) -> Result<Option<Selection>, String> {
        let Some(from) = property.get("x-from") else { return Ok(None) };
        let named = Selection::deserialize(from)
            .map_err(|_| "is selection.tracks, selection.playlist or selection.drive".to_string())?;
        let (fits, holds) = match named {
            Selection::Tracks => (property["type"] == "array" && property["items"]["type"] == "integer", "integers"),
            Selection::Playlist | Selection::Drive => (property["type"] == "string", "a string"),
        };
        match fits {
            true => Ok(Some(named)),
            false => Err(format!("{from} is {holds}, which the param isn't")),
        }
    }

    /// What it is in `context`, if there's something selected.
    pub fn value(self, context: &Context) -> Option<Value> {
        match self {
            Selection::Tracks if context.tracks.is_empty() => None,
            Selection::Tracks => Some(context.tracks.clone().into()),
            Selection::Playlist => context.playlist.clone().map(Value::from),
            Selection::Drive => context.drive.clone().map(Value::from),
        }
    }
}

/// Run with `track/analyze`; its fields become columns.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Analyzer {
    pub name: String,
    pub title: String,
    /// Its own version: the host keeps its values by it, so a new one works them out again. Bump
    /// it whenever the values would change.
    pub version: String,
    pub fields: IndexMap<String, FieldType>,
    /// Which of its fields has each role it answers (`bpm = "bpm"`): what the host reads for
    /// that role (see [`Role`]).
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    pub roles: IndexMap<Role, String>,
}

/// What a field is to locallibrary, whichever plugin works it out: the host reads a role's value
/// (doctor's tempos, keys and levels, the check before a set, ordering a set, writing keys and
/// BPMs to tracks) from the analyzer chosen for it. A closed list, each with one type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[schemars(rename = "FieldRole")]
#[serde(rename_all = "snake_case")]
pub enum Role {
    /// A Camelot code (`8A`): a string.
    Key,
    /// Beats per minute: a number.
    Bpm,
    /// How sure the analyzer is of its key, 0 to 1: a number.
    KeyConfidence,
    /// How sure it is of its tempo, 0 to 1: a number.
    BpmConfidence,
    /// Integrated loudness in LUFS: a number.
    Loudness,
    /// True peak in dBTP: a number.
    TruePeak,
    /// Runs of samples at full scale: an integer.
    ClippedRuns,
    /// Equal values are the same recording: a string.
    Identity,
}

impl Role {
    pub const ALL: [Role; 8] = [
        Role::Key,
        Role::Bpm,
        Role::KeyConfidence,
        Role::BpmConfidence,
        Role::Loudness,
        Role::TruePeak,
        Role::ClippedRuns,
        Role::Identity,
    ];

    /// The type of a field with it.
    pub fn kind(self) -> FieldType {
        match self {
            Role::Key | Role::Identity => FieldType::String,
            Role::ClippedRuns => FieldType::Integer,
            _ => FieldType::Number,
        }
    }

    /// As a manifest names it (`key_confidence`).
    pub fn name(self) -> &'static str {
        match self {
            Role::Key => "key",
            Role::Bpm => "bpm",
            Role::KeyConfidence => "key_confidence",
            Role::BpmConfidence => "bpm_confidence",
            Role::Loudness => "loudness",
            Role::TruePeak => "true_peak",
            Role::ClippedRuns => "clipped_runs",
            Role::Identity => "identity",
        }
    }
}

impl FromStr for Role {
    type Err = String;

    fn from_str(s: &str) -> Result<Role, String> {
        Role::ALL.into_iter().find(|r| r.name() == s).ok_or_else(|| {
            let all: Vec<&str> = Role::ALL.iter().map(|r| r.name()).collect();
            format!("{s:?} isn't a role: one of {}", all.join(", "))
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum FieldType {
    #[serde(rename = "number")]
    Number,
    #[serde(rename = "integer")]
    Integer,
    #[serde(rename = "string")]
    String,
    #[serde(rename = "boolean")]
    Boolean,
    #[serde(rename = "string[]")]
    Strings,
}

impl FieldType {
    /// Whether `value` is one of these (an integer is a number too), or null.
    pub fn accepts(self, value: &FieldValue) -> bool {
        matches!(
            (self, value),
            (_, FieldValue::Null)
                | (FieldType::Number, FieldValue::Number(_) | FieldValue::Integer(_))
                | (FieldType::Integer, FieldValue::Integer(_))
                | (FieldType::String, FieldValue::String(_))
                | (FieldType::Boolean, FieldValue::Boolean(_))
                | (FieldType::Strings, FieldValue::Strings(_))
        )
    }
}

/// A field's value, as an analyzer answers it and the host keeps it: of the type the manifest
/// declares (numbers are finite, as JSON's are), or null for one worked out with nothing to show
/// (a track left out of an answer has no value: it wasn't worked out).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum FieldValue {
    Integer(i64),
    Number(f64),
    Boolean(bool),
    String(String),
    Strings(Vec<String>),
    Null,
}

/// Run with `export/write`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Exporter {
    pub format: String,
    pub title: String,
    /// The file it writes ends in this: lowercase letters and digits, without the dot (`nml`).
    /// Either this or `folder`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extension: Option<String>,
    /// It writes a folder.
    #[serde(default)]
    pub folder: bool,
    /// A JSON Schema for its options (an object).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub options: Option<Value>,
}

/// A setting the host stores and shows in its settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Setting {
    pub key: String,
    pub title: String,
    #[serde(rename = "type")]
    pub kind: SettingType,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<Value>,
    /// What an `enum` setting can be.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub choices: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SettingType {
    String,
    Number,
    Boolean,
    /// One of `choices`.
    Enum,
    File,
    Directory,
    /// A password or a key: kept in the system's keychain, never shown, and given to the plugin
    /// only as the host fills it into its `net/fetch` requests. It has no default.
    Secret,
}

impl Setting {
    /// Whether `value` can be this setting's: of its type (for an enum, one of its choices; for
    /// a secret, a string that isn't empty).
    pub fn accepts(&self, value: &Value) -> bool {
        match self.kind {
            SettingType::String | SettingType::File | SettingType::Directory => value.is_string(),
            SettingType::Number => value.is_number(),
            SettingType::Boolean => value.is_boolean(),
            SettingType::Enum => value.as_str().is_some_and(|v| self.choices.iter().any(|c| c == v)),
            SettingType::Secret => value.as_str().is_some_and(|v| !v.is_empty()),
        }
    }
}

/// `[[net]]`: at most `per_second` `net/fetch` requests a second to `host`, one of the plugin's
/// `net:` capabilities as it's written (`musicbrainz.org`, `*.archive.org`), however many hooks
/// are under way: the limit the server asks its clients to keep.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct NetLimit {
    pub host: String,
    pub per_second: f64,
}

/// What a plugin may call back for. A closed list: a manifest naming another is refused.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Capability {
    LibraryRead,
    AnalysisRead,
    ArtworkRead,
    DriveInfo,
    HealthRead,
    StoreRead,
    SettingsRead,
    PlaysRead,
    SetsRead,
    AccountStatus,
    CollectionRead,
    LibraryPlan,
    LibraryDelete,
    TagsPlan,
    CuesPlan,
    ArtworkPlan,
    /// `net:<host>`: it talks to this server through the host (`net/fetch`), or with
    /// `net:*.<domain>` to any server below that domain. Shown to the user; a native plugin can't
    /// be held to it.
    Net(String),
}

impl Capability {
    /// Every capability but `net:<host>`.
    pub const NAMED: [Capability; 16] = [
        Capability::LibraryRead,
        Capability::AnalysisRead,
        Capability::ArtworkRead,
        Capability::DriveInfo,
        Capability::HealthRead,
        Capability::StoreRead,
        Capability::SettingsRead,
        Capability::PlaysRead,
        Capability::SetsRead,
        Capability::AccountStatus,
        Capability::CollectionRead,
        Capability::LibraryPlan,
        Capability::LibraryDelete,
        Capability::TagsPlan,
        Capability::CuesPlan,
        Capability::ArtworkPlan,
    ];

    /// Its name, unless it's `net:<host>`.
    fn name(&self) -> Option<&'static str> {
        Some(match self {
            Capability::LibraryRead => "library:read",
            Capability::AnalysisRead => "analysis:read",
            Capability::ArtworkRead => "artwork:read",
            Capability::DriveInfo => "drive:info",
            Capability::HealthRead => "health:read",
            Capability::StoreRead => "store:read",
            Capability::SettingsRead => "settings:read",
            Capability::PlaysRead => "plays:read",
            Capability::SetsRead => "sets:read",
            Capability::AccountStatus => "account:status",
            Capability::CollectionRead => "collection:read",
            Capability::LibraryPlan => "library:plan",
            Capability::LibraryDelete => "library:delete",
            Capability::TagsPlan => "tags:plan",
            Capability::CuesPlan => "cues:plan",
            Capability::ArtworkPlan => "artwork:plan",
            Capability::Net(_) => return None,
        })
    }
}

impl Capability {
    /// Whether it's `net:` for `host` (as a URL has it, in lowercase): that host, or one below
    /// the domain of a `net:*.<domain>`.
    pub fn reaches(&self, host: &str) -> bool {
        let Capability::Net(granted) = self else { return false };
        let granted = granted.to_ascii_lowercase();
        match granted.strip_prefix("*.") {
            Some(domain) => host.strip_suffix(domain).is_some_and(|below| below.ends_with('.') && below.len() > 1),
            None => host == granted,
        }
    }
}

impl fmt::Display for Capability {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Capability::Net(host) => write!(f, "net:{host}"),
            named => f.write_str(named.name().unwrap_or_default()),
        }
    }
}

/// A host name, or `*.` and a domain: no scheme, path, port or spaces.
fn is_host(host: &str) -> bool {
    let host = host.strip_prefix("*.").unwrap_or(host);
    !host.is_empty() && host.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
}

impl FromStr for Capability {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, String> {
        if let Some(host) = s.strip_prefix("net:") {
            return match is_host(host) {
                true => Ok(Capability::Net(host.into())),
                false => Err(format!("{s:?} names no host: net:<host> or net:*.<domain>, like net:musicbrainz.org")),
            };
        }
        Capability::NAMED.into_iter().find(|c| c.name() == Some(s)).ok_or_else(|| {
            let known: Vec<_> = Capability::NAMED.iter().filter_map(Capability::name).collect();
            format!("unknown capability {s:?}: one of {} or net:<host>", known.join(", "))
        })
    }
}

impl Serialize for Capability {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Capability {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        String::deserialize(d)?.parse().map_err(serde::de::Error::custom)
    }
}

impl JsonSchema for Capability {
    fn schema_name() -> Cow<'static, str> {
        "Capability".into()
    }

    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        let named: Vec<_> = Capability::NAMED.iter().filter_map(Capability::name).collect();
        json_schema!({
            "type": "string",
            "anyOf": [{ "enum": named }, { "pattern": "^net:(\\*\\.)?[A-Za-z0-9.-]+$" }],
        })
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ManifestError {
    #[error("{0}")]
    Parse(#[from] toml::de::Error),
    /// `at` says where (`contributes.commands[1].name`).
    #[error("{at}: {message}")]
    Invalid { at: String, message: String },
}

fn invalid(at: impl Into<String>, message: impl Into<String>) -> ManifestError {
    ManifestError::Invalid { at: at.into(), message: message.into() }
}

/// What [`is_name`] takes.
const NAME_RULE: &str = "lowercase letters, digits, - and _, starting with a letter";

/// Lowercase letters, digits, `-` and `_`, starting with a letter, at most 64 bytes.
fn is_name(s: &str) -> bool {
    s.len() <= 64
        && s.starts_with(|c: char| c.is_ascii_lowercase())
        && s.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
}

/// Each of `names` (`at[i].field`) a valid name, used once.
fn unique_names<'a>(
    at: &str,
    field: &str,
    names: impl IntoIterator<Item = (usize, &'a str)>,
) -> Result<(), ManifestError> {
    let mut seen = FxHashSet::default();
    for (i, name) in names {
        if !is_name(name) {
            return Err(invalid(format!("{at}[{i}].{field}"), format!("{name:?} isn't a name: {NAME_RULE}")));
        }
        if !seen.insert(name) {
            return Err(invalid(format!("{at}[{i}].{field}"), format!("{name:?} is used twice")));
        }
    }
    Ok(())
}

/// `schema` (at `at`), when there is one, a valid JSON Schema for an object, whose properties'
/// [`CliHints`] can be read.
fn object_schema(at: String, schema: Option<&Value>) -> Result<(), ManifestError> {
    let Some(schema) = schema else { return Ok(()) };
    if schema.get("type") != Some(&Value::from("object")) {
        return Err(invalid(at, "must be a JSON Schema for an object (type = \"object\")"));
    }
    jsonschema::validator_for(schema).map_err(|e| invalid(&at, format!("isn't a valid JSON Schema: {e}")))?;
    let mut positional = None;
    for (name, property) in schema["properties"].as_object().into_iter().flatten() {
        let at = format!("{at}.properties.{name}");
        if !is_param_name(name) {
            return Err(invalid(at, format!("{name:?} isn't a param's name: {PARAM_RULE}")));
        }
        let hints = CliHints::of(property).map_err(|e| invalid(format!("{at}.x-cli"), e))?;
        if hints.positional
            && let Some(other) = positional.replace(name)
        {
            return Err(invalid(format!("{at}.x-cli"), format!("{other:?} is positional already")));
        }
        let paths = property["type"] == "string" || property["items"]["type"] == "string";
        match property.get("x-path") {
            None => {}
            Some(Value::Bool(_)) if paths => {}
            Some(_) => return Err(invalid(format!("{at}.x-path"), "is true or false, on a string or a list of them")),
        }
        Selection::of(property).map_err(|e| invalid(format!("{at}.x-from"), e))?;
    }
    Ok(())
}

/// `offered` (at `at`) names somewhere, each place once.
fn offered_in(at: String, offered: &[Where]) -> Result<(), ManifestError> {
    if offered.is_empty() {
        return Err(invalid(at, "names no place: track, playlist or drive"));
    }
    let mut seen = FxHashSet::default();
    match offered.iter().find(|w| !seen.insert(*w)) {
        Some(w) => Err(invalid(at, format!("{} is there twice", w.name()))),
        None => Ok(()),
    }
}

/// An analyzer's version, which names a folder: letters, digits, `.`, `_`, `+` and `-`, not
/// starting with a dot, at most 64 bytes (`3`, `1.2.0`).
fn is_version(s: &str) -> bool {
    (1..=64).contains(&s.len())
        && !s.starts_with('.')
        && s.chars().all(|c| c.is_ascii_alphanumeric() || "._+-".contains(c))
}

/// Where `path` is in a manifest, as its errors say (`contributes.commands[0].colour`).
fn at(path: &serde_ignored::Path<'_>) -> String {
    use serde_ignored::Path;
    match path {
        Path::Root => String::new(),
        Path::Seq { parent, index } => format!("{}[{index}]", at(parent)),
        Path::Map { parent, key } => match at(parent) {
            parent if parent.is_empty() => key.clone(),
            parent => format!("{parent}.{key}"),
        },
        Path::Some { parent } | Path::NewtypeStruct { parent } | Path::NewtypeVariant { parent } => at(parent),
    }
}

/// What [`is_param_name`] takes.
const PARAM_RULE: &str = "lowercase letters, digits and _, starting with a letter";

/// A params or options property's name: one a host can make a flag of.
fn is_param_name(s: &str) -> bool {
    s.len() <= 64
        && s.starts_with(|c: char| c.is_ascii_lowercase())
        && s.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

/// A place on the command line: one or two names (`import`, `playlist import`).
fn is_cli_path(s: &str) -> bool {
    let words: Vec<&str> = s.split(' ').collect();
    words.len() <= 2 && words.iter().all(|w| is_name(w))
}

/// Whether `params` fit `schema`, a contribution's `params` or `options` (without one, any object
/// does), files and folders (see [`is_path`]) as absolute paths: `invalid_params` says where they
/// don't (`fields.path`, a JSON pointer like `/name`).
pub fn check_params(schema: Option<&Value>, params: &Value) -> Result<(), RpcError> {
    let Some(schema) = schema else {
        return match params.is_object() {
            true => Ok(()),
            false => Err(RpcError::invalid_params("expected an object")),
        };
    };
    let validator = jsonschema::validator_for(schema).map_err(|e| RpcError::internal(e.to_string()))?;
    if let Err(e) = validator.validate(params) {
        let path = e.instance_path().to_string();
        return Err(match path.is_empty() {
            true => RpcError::invalid_params(e.to_string()),
            false => invalid_at(&path, &e.to_string()),
        });
    }
    for (name, _) in schema["properties"].as_object().into_iter().flatten().filter(|(_, p)| is_path(p)) {
        let given: Vec<&str> = match &params[name] {
            Value::Array(list) => list.iter().filter_map(Value::as_str).collect(),
            one => one.as_str().into_iter().collect(),
        };
        if let Some(relative) = given.into_iter().find(|p| !Path::new(p).is_absolute()) {
            return Err(invalid_at(&format!("/{name}"), &format!("{relative:?} isn't an absolute path")));
        }
    }
    Ok(())
}

/// `invalid_params` about the param at `path` (a JSON pointer).
fn invalid_at(path: &str, message: &str) -> RpcError {
    let mut error = RpcError::invalid_params(format!("{path}: {message}"));
    if let Some(data) = error.data.as_mut() {
        data.fields.insert("path".into(), path.into());
    }
    error
}

/// `text` (at `at`) isn't blank.
fn shown(at: impl Into<String>, text: &str) -> Result<(), ManifestError> {
    match text.trim().is_empty() {
        true => Err(invalid(at, "is empty")),
        false => Ok(()),
    }
}

/// An exporter's file extension: no dot, just lowercase letters and digits.
fn is_extension(s: &str) -> bool {
    (1..=16).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
}

impl Manifest {
    /// Reads `locallibrary-plugin.toml`, leaving out keys it doesn't know (see
    /// [`Manifest::unknown`]). See [`Manifest::validate`] for what else is checked.
    pub fn parse(toml: &str) -> Result<Manifest, ManifestError> {
        let mut unknown = Vec::new();
        let mut manifest: Manifest =
            serde_ignored::deserialize(toml::Deserializer::parse(toml)?, |path| unknown.push(at(&path)))?;
        manifest.unknown = unknown;
        Ok(manifest)
    }

    /// The keys [`Manifest::parse`] left out, where they were (`contributes.commands[0].colour`):
    /// a newer locallibrary's, or mistakes, which [`Manifest::validate`] refuses in a plugin
    /// locallibrary ships.
    pub fn unknown(&self) -> &[String] {
        &self.unknown
    }

    /// Checks what parsing doesn't: names, titles, versions, capabilities, schemas and settings.
    /// `first_party`: the host ships this plugin itself, so its commands and operations may have
    /// places of their own on the command line (`cli`), and a key it doesn't know is a mistake,
    /// refused (another plugin's is left out, see [`Manifest::unknown`]).
    pub fn validate(&self, first_party: bool) -> Result<(), ManifestError> {
        if let Some(key) = self.unknown.first().filter(|_| first_party) {
            return Err(invalid(key, "isn't a key locallibrary knows"));
        }
        if !is_name(&self.id) {
            return Err(invalid("id", format!("{:?} isn't a name: {NAME_RULE}", self.id)));
        }
        shown("name", &self.name)?;
        Version::parse(&self.version).map_err(|e| invalid("version", e.to_string()))?;
        let protocol = VersionReq::parse(&self.protocol).map_err(|e| invalid("protocol", e.to_string()))?;
        if !protocol.matches(&Version::parse(PROTOCOL_VERSION).expect("PROTOCOL_VERSION is a version")) {
            return Err(invalid("protocol", format!("{:?} doesn't include {PROTOCOL_VERSION}", self.protocol)));
        }
        let mut granted = FxHashSet::default();
        if let Some((i, c)) = self.capabilities.iter().enumerate().find(|(_, c)| !granted.insert(*c)) {
            return Err(invalid(format!("capabilities[{i}]"), format!("{c} is there twice")));
        }
        let c = &self.contributes;
        let commands = c.commands.iter().enumerate().map(|(i, x)| ("contributes.commands", i, x.cli.as_deref()));
        let operations = c.operations.iter().enumerate().map(|(i, x)| ("contributes.operations", i, x.cli.as_deref()));
        let mut places = FxHashSet::default();
        for (at, i, cli) in commands.chain(operations).filter_map(|(at, i, cli)| Some((at, i, cli?))) {
            let at = format!("{at}[{i}].cli");
            if !first_party {
                return Err(invalid(at, "only plugins locallibrary ships add CLI commands"));
            }
            if !is_cli_path(cli) {
                return Err(invalid(
                    at,
                    format!("{cli:?} isn't one or two names ({NAME_RULE}), like \"playlist import\""),
                ));
            }
            if !places.insert(cli) {
                return Err(invalid(at, format!("{cli:?} is used twice")));
            }
        }
        let at = "contributes.commands";
        unique_names(at, "name", c.commands.iter().map(|x| x.name.as_str()).enumerate())?;
        for (i, x) in c.commands.iter().enumerate() {
            shown(format!("{at}[{i}].title"), &x.title)?;
            offered_in(format!("{at}[{i}].where"), &x.offered)?;
            object_schema(format!("{at}[{i}].params"), x.params.as_ref())?;
        }
        let at = "contributes.operations";
        unique_names(at, "name", c.operations.iter().map(|x| x.name.as_str()).enumerate())?;
        for (i, x) in c.operations.iter().enumerate() {
            shown(format!("{at}[{i}].title"), &x.title)?;
            offered_in(format!("{at}[{i}].where"), &x.offered)?;
            object_schema(format!("{at}[{i}].params"), x.params.as_ref())?;
        }
        let at = "contributes.analyzers";
        unique_names(at, "name", c.analyzers.iter().map(|x| x.name.as_str()).enumerate())?;
        // Fields are columns: one name is one column, whichever analyzer fills it.
        let mut fields = FxHashSet::default();
        for (i, x) in c.analyzers.iter().enumerate() {
            shown(format!("{at}[{i}].title"), &x.title)?;
            if !is_version(&x.version) {
                return Err(invalid(
                    format!("{at}[{i}].version"),
                    format!("{:?} isn't a version: letters, digits, ., _, + and -", x.version),
                ));
            }
            if x.fields.is_empty() {
                return Err(invalid(format!("{at}[{i}].fields"), "an analyzer needs a field"));
            }
            for name in x.fields.keys() {
                if !is_name(name) {
                    return Err(invalid(format!("{at}[{i}].fields.{name}"), format!("isn't a name: {NAME_RULE}")));
                }
                if !fields.insert(name) {
                    return Err(invalid(format!("{at}[{i}].fields.{name}"), "another analyzer has this field"));
                }
            }
            for (role, field) in &x.roles {
                let at = format!("{at}[{i}].roles.{}", role.name());
                let kind = x.fields.get(field).ok_or_else(|| invalid(&at, format!("{field:?} isn't its field")))?;
                if *kind != role.kind() {
                    let kind = serde_json::to_value(role.kind()).unwrap_or_default();
                    let message = format!("{field} isn't a {}, which the role is", kind.as_str().unwrap_or("?"));
                    return Err(invalid(at, message));
                }
            }
        }
        let at = "contributes.exporters";
        unique_names(at, "format", c.exporters.iter().map(|x| x.format.as_str()).enumerate())?;
        for (i, x) in c.exporters.iter().enumerate() {
            shown(format!("{at}[{i}].title"), &x.title)?;
            if x.extension.is_some() == x.folder {
                return Err(invalid(format!("{at}[{i}]"), "an exporter has either an extension or folder = true"));
            }
            if let Some(ext) = x.extension.as_deref().filter(|ext| !is_extension(ext)) {
                let message = format!("{ext:?} isn't an extension: lowercase letters and digits, without the dot");
                return Err(invalid(format!("{at}[{i}].extension"), message));
            }
            object_schema(format!("{at}[{i}].options"), x.options.as_ref())?;
        }
        let at = "contributes.settings";
        unique_names(at, "key", c.settings.iter().map(|x| x.key.as_str()).enumerate())?;
        for (i, x) in c.settings.iter().enumerate() {
            shown(format!("{at}[{i}].title"), &x.title)?;
            let is_enum = x.kind == SettingType::Enum;
            if is_enum == x.choices.is_empty() {
                return Err(invalid(format!("{at}[{i}].choices"), "an enum setting has choices, and only an enum"));
            }
            let mut choices = FxHashSet::default();
            if let Some(choice) = x.choices.iter().find(|c| !choices.insert(*c)) {
                return Err(invalid(format!("{at}[{i}].choices"), format!("{choice:?} is there twice")));
            }
            if x.default.as_ref().is_some_and(|d| x.kind == SettingType::Secret || !x.accepts(d)) {
                return Err(invalid(
                    format!("{at}[{i}].default"),
                    "doesn't fit the setting's type (and a secret has none)",
                ));
            }
        }
        let mut limited = FxHashSet::default();
        for (i, x) in self.net.iter().enumerate() {
            let at = format!("net[{i}]");
            if !self.capabilities.contains(&Capability::Net(x.host.clone())) {
                return Err(invalid(format!("{at}.host"), format!("net:{} isn't one of its capabilities", x.host)));
            }
            if !limited.insert(&x.host) {
                return Err(invalid(format!("{at}.host"), format!("{:?} is limited twice", x.host)));
            }
            // So the time between two requests is one the host can wait.
            if !(x.per_second.is_finite() && x.per_second * 3600.0 >= 1.0) {
                return Err(invalid(format!("{at}.per_second"), "is how many requests a second, at least 1/3600"));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
