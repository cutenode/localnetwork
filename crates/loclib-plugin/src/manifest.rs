//! `locallibrary-plugin.toml`: who a plugin is, what it may call back for and what it adds to
//! locallibrary. Hosts draw the contributions; plugins never ship UI code.

use std::borrow::Cow;
use std::fmt;
use std::str::FromStr;

use indexmap::IndexMap;
use rustc_hash::FxHashSet;
use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use semver::{Version, VersionReq};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{PROTOCOL_VERSION, RpcError};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    /// Lowercase letters, digits, `-` and `_`, starting with a letter.
    pub id: String,
    pub name: String,
    /// The plugin's version (semver).
    pub version: String,
    /// The protocol versions it speaks, as a semver requirement (`^0.1`).
    pub protocol: String,
    /// What it may call back for (see [`Capability`]).
    #[serde(default)]
    pub capabilities: Vec<Capability>,
    #[serde(default)]
    pub contributes: Contributions,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
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
#[serde(deny_unknown_fields)]
pub struct Command {
    pub name: String,
    pub title: String,
    pub mode: Mode,
    /// A JSON Schema for its params (an object).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
    /// A top-level CLI command (`locallibrary import`). First-party plugins only.
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

/// Planned with `op/plan`, applied by the host, which works out from the edits whether it's
/// destructive.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Operation {
    pub name: String,
    pub title: String,
    /// Planning takes a while: the host shows progress.
    #[serde(default)]
    pub long_running: bool,
    /// A JSON Schema for its params (an object).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
}

/// Run with `track/analyze`; its fields become columns.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Analyzer {
    pub name: String,
    pub title: String,
    pub fields: IndexMap<String, FieldType>,
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
    /// Whether `value` is one of these (an integer is a number too).
    pub fn accepts(self, value: &FieldValue) -> bool {
        matches!(
            (self, value),
            (FieldType::Number, FieldValue::Number(_) | FieldValue::Integer(_))
                | (FieldType::Integer, FieldValue::Integer(_))
                | (FieldType::String, FieldValue::String(_))
                | (FieldType::Boolean, FieldValue::Boolean(_))
                | (FieldType::Strings, FieldValue::Strings(_))
        )
    }
}

/// A field's value, as an analyzer answers it and the host keeps it: of the type the manifest
/// declares (numbers are finite, as JSON's are).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum FieldValue {
    Integer(i64),
    Number(f64),
    Boolean(bool),
    String(String),
    Strings(Vec<String>),
}

/// Run with `export/write`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
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
#[serde(deny_unknown_fields)]
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
    /// Kept in the keychain; it has no default.
    Secret,
}

impl Setting {
    /// Whether `value` can be this setting's: of its type (for an enum, one of its choices). A
    /// secret takes none here: it's kept in the keychain.
    pub fn accepts(&self, value: &Value) -> bool {
        match self.kind {
            SettingType::String | SettingType::File | SettingType::Directory => value.is_string(),
            SettingType::Number => value.is_number(),
            SettingType::Boolean => value.is_boolean(),
            SettingType::Enum => value.as_str().is_some_and(|v| self.choices.iter().any(|c| c == v)),
            SettingType::Secret => false,
        }
    }
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
    /// `net:<host>`: it talks to this server. Shown to the user; a native plugin can't be held
    /// to it.
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

impl fmt::Display for Capability {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Capability::Net(host) => write!(f, "net:{host}"),
            named => f.write_str(named.name().unwrap_or_default()),
        }
    }
}

/// A host name: no scheme, path, port or spaces.
fn is_host(host: &str) -> bool {
    !host.is_empty() && host.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
}

impl FromStr for Capability {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, String> {
        if let Some(host) = s.strip_prefix("net:") {
            return match is_host(host) {
                true => Ok(Capability::Net(host.into())),
                false => Err(format!("{s:?} names no host: net:<host>, like net:musicbrainz.org")),
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
            "anyOf": [{ "enum": named }, { "pattern": "^net:[A-Za-z0-9.-]+$" }],
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

/// `schema` (at `at`), when there is one, a valid JSON Schema for an object.
fn object_schema(at: String, schema: Option<&Value>) -> Result<(), ManifestError> {
    let Some(schema) = schema else { return Ok(()) };
    if schema.get("type") != Some(&Value::from("object")) {
        return Err(invalid(at, "must be a JSON Schema for an object (type = \"object\")"));
    }
    jsonschema::validator_for(schema).map_err(|e| invalid(at, format!("isn't a valid JSON Schema: {e}")))?;
    Ok(())
}

/// Whether `params` fit `schema`, a contribution's `params` or `options` (without one, any object
/// does): `invalid_params` says where they don't (`fields.path`, a JSON pointer like `/name`).
pub fn check_params(schema: Option<&Value>, params: &Value) -> Result<(), RpcError> {
    let Some(schema) = schema else {
        return match params.is_object() {
            true => Ok(()),
            false => Err(RpcError::invalid_params("expected an object")),
        };
    };
    let validator = jsonschema::validator_for(schema).map_err(|e| RpcError::internal(e.to_string()))?;
    let Err(e) = validator.validate(params) else { return Ok(()) };
    let path = e.instance_path().to_string();
    let mut error = RpcError::invalid_params(match path.is_empty() {
        true => e.to_string(),
        false => format!("{path}: {e}"),
    });
    if let Some(data) = error.data.as_mut().filter(|_| !path.is_empty()) {
        data.fields.insert("path".into(), path.into());
    }
    Err(error)
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
    /// Reads `locallibrary-plugin.toml`. See [`Manifest::validate`] for what else is checked.
    pub fn parse(toml: &str) -> Result<Manifest, ManifestError> {
        Ok(toml::from_str(toml)?)
    }

    /// Checks what parsing doesn't: names, titles, versions, capabilities, schemas and settings.
    /// `first_party`: the host ships this plugin itself, so its commands may be top-level CLI
    /// commands (`cli`).
    pub fn validate(&self, first_party: bool) -> Result<(), ManifestError> {
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
        let at = "contributes.commands";
        unique_names(at, "name", c.commands.iter().map(|x| x.name.as_str()).enumerate())?;
        let cli = c.commands.iter().enumerate().filter_map(|(i, x)| Some((i, x.cli.as_deref()?)));
        if let Some((i, _)) = cli.clone().next()
            && !first_party
        {
            return Err(invalid(format!("{at}[{i}].cli"), "only plugins locallibrary ships add CLI commands"));
        }
        unique_names(at, "cli", cli)?;
        for (i, x) in c.commands.iter().enumerate() {
            shown(format!("{at}[{i}].title"), &x.title)?;
            object_schema(format!("{at}[{i}].params"), x.params.as_ref())?;
        }
        let at = "contributes.operations";
        unique_names(at, "name", c.operations.iter().map(|x| x.name.as_str()).enumerate())?;
        for (i, x) in c.operations.iter().enumerate() {
            shown(format!("{at}[{i}].title"), &x.title)?;
            object_schema(format!("{at}[{i}].params"), x.params.as_ref())?;
        }
        let at = "contributes.analyzers";
        unique_names(at, "name", c.analyzers.iter().map(|x| x.name.as_str()).enumerate())?;
        // Fields are columns: one name is one column, whichever analyzer fills it.
        let mut fields = FxHashSet::default();
        for (i, x) in c.analyzers.iter().enumerate() {
            shown(format!("{at}[{i}].title"), &x.title)?;
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
            if x.default.as_ref().is_some_and(|d| !x.accepts(d)) {
                return Err(invalid(
                    format!("{at}[{i}].default"),
                    "doesn't fit the setting's type (and a secret has none)",
                ));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
