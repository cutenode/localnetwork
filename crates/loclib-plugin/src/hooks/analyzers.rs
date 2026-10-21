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
    /// A beat grid (feature `analysis`, see [`crate::Output`]).
    #[serde(rename = "beats")]
    Beats,
    /// A song structure: the mood and the phrases (feature `analysis`).
    #[serde(rename = "phrases")]
    Phrases,
    /// A waveform in three bands, in a blob (feature `analysis`).
    #[serde(rename = "waveform")]
    Waveform,
}

impl FieldType {
    /// Whether `value` is one of these (an integer is a number too), or null.
    pub fn accepts(self, value: &FieldValue) -> bool {
        if self.is_structured() {
            return crate::Output::of(self, value).is_ok();
        }
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

    /// A beat grid, phrases or a waveform: kept apart from the columns (see [`crate::Output`]).
    pub fn is_structured(self) -> bool {
        matches!(self, FieldType::Beats | FieldType::Phrases | FieldType::Waveform)
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
    /// A beat grid, phrases or a waveform (feature `analysis`), as JSON: what [`crate::Output`]
    /// reads. Made with [`FieldValue::beats`], [`FieldValue::phrases`] and
    /// [`FieldValue::waveform`]; never a column.
    #[schemars(skip)]
    Structured(serde_json::Value),
}

/// An analyzer's version, which names a folder: letters, digits, `.`, `_`, `+` and `-`, not
/// starting with a dot, at most 64 bytes (`3`, `1.2.0`).
fn is_version(s: &str) -> bool {
    (1..=64).contains(&s.len())
        && !s.starts_with('.')
        && s.chars().all(|c| c.is_ascii_alphanumeric() || "._+-".contains(c))
}

/// `[[contributes.analyzers]]`: names, titles, versions, fields (no two analyzers with one) and
/// roles of their fields' types.
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
    }
    Ok(())
}
