//! Feature `analyzers`: values a plugin works out for each track from its audio, kept by the host
//! as columns, and read by role for what locallibrary itself needs to know of a track.

use std::path::PathBuf;
use std::str::FromStr;

use indexmap::IndexMap;
use rustc_hash::FxHashSet;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::manifest::{NAME_RULE, invalid, is_name, shown, unique_names};
use crate::sdk::declared;
use crate::{Context, Contributions, Feature, Hook, Manifest, ManifestError, RpcError};

/// Host to plugin: [`AnalyzeTrack`] -> [`Analysis`].
pub const ANALYZE_TRACK: &str = "track/analyze";

pub(crate) const FEATURE: Feature = Feature {
    name: "analyzers",
    methods: &[ANALYZE_TRACK],
    sends: &[],
    contributed: |m| !m.contributes.analyzers.is_empty(),
};

/// `track/analyze`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct AnalyzeTrack {
    /// One of the manifest's analyzers.
    pub analyzer: String,
    pub track: Track,
    #[serde(default)]
    pub context: Context,
}

/// The track an analyzer looks at.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Track {
    pub id: u32,
    /// Its path on the drive, from the drive's root (`/Contents/...`).
    pub file_path: String,
    /// Its audio file, absolute. Read it; never write to it.
    pub audio_path: PathBuf,
    /// Its analysis file (`ANLZ0000.DAT`), absolute, when it has one. Read only, too.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub analyze_path: Option<PathBuf>,
}

/// What `track/analyze` answers: a value for fields the analyzer declares, of the type it
/// declares (see [`crate::FieldType`]).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Analysis {
    pub fields: IndexMap<String, FieldValue>,
}

impl Hook for AnalyzeTrack {
    const METHOD: &'static str = ANALYZE_TRACK;
    type Answer = Analysis;

    fn declared(&self, c: &Contributions) -> Result<(), RpcError> {
        declared("analyzer", &self.analyzer, c.analyzers.iter().map(|x| &x.name))
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
    /// How hosts show some of its fields, by field (`display.lufs = { unit = "LUFS" }`): in the
    /// player's columns, `track show` and the mix report.
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    pub display: IndexMap<String, FieldDisplay>,
}

/// How hosts show a field's values, each part optional. Without them a number shows to two
/// places at most (trailing zeros left out), a list joined with commas, a boolean as yes or no.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct FieldDisplay {
    /// Said after a value (`LUFS`); a `db` format's is `dB` unless it says.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
    /// A number's decimal places, 0 to 6.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub precision: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format: Option<FieldFormat>,
    /// How wide its column is in the player, in cells (1 to 40); for a `meter`, how long it is.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<u16>,
}

/// What a field's values are, for hosts to show them as such.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FieldFormat {
    /// A key (a string), as the host shows keys.
    Key,
    /// Seconds (a number), as minutes and seconds (`3:07`, `1:02:03`).
    Duration,
    /// Decibels (a number), signed (`+0.4 dB`, `-8.2 dB`).
    Db,
    /// A fraction (a number, 0 to 1) as a percentage (`85%`; no decimal places unless it says).
    Percent,
    /// A fraction (a number, 0 to 1) as a bar, `width` cells long (8 unless it says).
    Meter,
}

impl FieldFormat {
    /// Whether a field of type `kind` can be shown so.
    fn fits(self, kind: FieldType) -> bool {
        match self {
            FieldFormat::Key => kind == FieldType::String,
            _ => matches!(kind, FieldType::Number | FieldType::Integer),
        }
    }

    /// As a manifest names it.
    fn name(self) -> &'static str {
        match self {
            FieldFormat::Key => "key",
            FieldFormat::Duration => "duration",
            FieldFormat::Db => "db",
            FieldFormat::Percent => "percent",
            FieldFormat::Meter => "meter",
        }
    }
}

impl FieldDisplay {
    /// The most decimal places.
    pub const MAX_PRECISION: u8 = 6;
    /// The widest a column is.
    pub const MAX_WIDTH: u16 = 40;

    /// `value` as hosts show it, by these hints: null is `-` (worked out, nothing to show).
    pub fn show(&self, value: &FieldValue) -> String {
        let n = match value {
            FieldValue::Null => return "-".into(),
            FieldValue::Boolean(b) => return if *b { "yes" } else { "no" }.into(),
            FieldValue::String(s) => return self.with_unit(s.clone()),
            FieldValue::Strings(all) => return self.with_unit(all.join(", ")),
            FieldValue::Integer(i) if self.format.is_none() => return self.with_unit(i.to_string()),
            FieldValue::Integer(i) => *i as f64,
            FieldValue::Number(n) => *n,
        };
        match self.format {
            Some(FieldFormat::Duration) => {
                let secs = n.abs().round() as u64;
                let sign = if n <= -0.5 { "-" } else { "" };
                match secs >= 3600 {
                    true => format!("{sign}{}:{:02}:{:02}", secs / 3600, secs / 60 % 60, secs % 60),
                    false => format!("{sign}{}:{:02}", secs / 60, secs % 60),
                }
            }
            Some(FieldFormat::Db) => {
                let shown = number(n, self.precision);
                let sign = if n > 0.0 && !zero(&shown) { "+" } else { "" };
                format!("{sign}{shown} {}", self.unit.as_deref().unwrap_or("dB"))
            }
            Some(FieldFormat::Percent) => format!("{}%", number(n * 100.0, Some(self.precision.unwrap_or(0)))),
            Some(FieldFormat::Meter) => {
                let cells = usize::from(self.width.unwrap_or(8));
                let full = ((n.clamp(0.0, 1.0) * cells as f64).round() as usize).min(cells);
                format!("{}{}", "█".repeat(full), "░".repeat(cells - full))
            }
            Some(FieldFormat::Key) | None => self.with_unit(number(n, self.precision)),
        }
    }

    fn with_unit(&self, shown: String) -> String {
        match &self.unit {
            Some(unit) => format!("{shown} {unit}"),
            None => shown,
        }
    }
}

/// `n` to `precision` places, or without it to two at most (trailing zeros left out); never
/// `-0`.
fn number(n: f64, precision: Option<u8>) -> String {
    let mut shown = format!("{n:.*}", usize::from(precision.unwrap_or(2)));
    if precision.is_none() && shown.contains('.') {
        shown = shown.trim_end_matches('0').trim_end_matches('.').into();
    }
    match shown.strip_prefix('-') {
        Some(rest) if zero(rest) => rest.into(),
        _ => shown,
    }
}

/// Whether a number as shown is nothing but zeros.
fn zero(shown: &str) -> bool {
    shown.bytes().all(|b| matches!(b, b'0' | b'.' | b'-'))
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

/// An analyzer's version, which names a folder: letters, digits, `.`, `_`, `+` and `-`, not
/// starting with a dot, at most 64 bytes (`3`, `1.2.0`).
fn is_version(s: &str) -> bool {
    (1..=64).contains(&s.len())
        && !s.starts_with('.')
        && s.chars().all(|c| c.is_ascii_alphanumeric() || "._+-".contains(c))
}

/// `[[contributes.analyzers]]`: names, titles, versions, fields (no two analyzers with one),
/// roles of their fields' types, and how fields are shown.
pub(crate) fn validate(m: &Manifest) -> Result<(), ManifestError> {
    let (at, c) = ("contributes.analyzers", &m.contributes);
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
        for (field, d) in &x.display {
            let at = format!("{at}[{i}].display.{field}");
            let kind = x.fields.get(field).ok_or_else(|| invalid(&at, format!("{field:?} isn't its field")))?;
            if let Some(f) = d.format.filter(|f| !f.fits(*kind)) {
                let kind = serde_json::to_value(kind).unwrap_or_default();
                let message = format!("{field} is a {}, which {} isn't for", kind.as_str().unwrap_or("?"), f.name());
                return Err(invalid(format!("{at}.format"), message));
            }
            if d.precision.is_some_and(|p| p > FieldDisplay::MAX_PRECISION) {
                return Err(invalid(format!("{at}.precision"), "is 0 to 6 places"));
            }
            if d.width.is_some_and(|w| !(1..=FieldDisplay::MAX_WIDTH).contains(&w)) {
                return Err(invalid(format!("{at}.width"), "is 1 to 40 cells"));
            }
            if let Some(unit) = &d.unit {
                shown(format!("{at}.unit"), unit)?;
            }
        }
    }
    Ok(())
}
