//! Feature `importers`: what another program kept about tracks (keys, tempos, cues, tags) and the
//! plays it logged, read from its file, which the host finds on a drive and makes the edits of
//! itself.

use std::path::PathBuf;

use rustc_hash::FxHashSet;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::hooks::exporters::is_extension;
use crate::manifest::{invalid, object_schema, shown, unique_names};
use crate::sdk::declared;
use crate::{Blob, Context, Contributions, Feature, Hook, Manifest, ManifestError, RpcError};

/// Host to plugin: [`ReadImport`] -> [`ImportAnswer`].
pub const READ_IMPORT: &str = "import/read";

pub(crate) const FEATURE: Feature = Feature {
    name: "importers",
    methods: &[READ_IMPORT],
    sends: &[],
    contributed: |m| !m.contributes.importers.is_empty(),
};

/// `import/read`: what the file says about tracks, and the plays it logged. The host finds the
/// tracks on the drive and plans the edits itself, adding to what tracks have: a plugin never
/// plans one.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ReadImport {
    /// One of the manifest's importers, by format.
    pub importer: String,
    /// The file to read, absolute, on this computer. Read it; never write to it.
    pub file: PathBuf,
    /// What the importer's `options` schema describes.
    #[serde(default)]
    pub options: Value,
    #[serde(default)]
    pub context: Context,
}

/// What `import/read` answers: what it read, or (feature `blobs`) a file in the request's scratch
/// folder holding it as JSON, for an answer too big for a message.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum ImportAnswer {
    Blob(Blob),
    Read(Imported),
}

impl From<Imported> for ImportAnswer {
    fn from(read: Imported) -> ImportAnswer {
        ImportAnswer::Read(read)
    }
}

impl From<Blob> for ImportAnswer {
    fn from(blob: Blob) -> ImportAnswer {
        ImportAnswer::Blob(blob)
    }
}

/// Read by its `blob` key rather than by trying each kind: what's wrong with what it read is said
/// as its own error.
impl<'de> Deserialize<'de> for ImportAnswer {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<ImportAnswer, D::Error> {
        let answer = Value::deserialize(d)?;
        let read = match answer.get("blob") {
            Some(_) => Blob::deserialize(answer).map(ImportAnswer::Blob),
            None => Imported::deserialize(answer).map(ImportAnswer::Read),
        };
        read.map_err(serde::de::Error::custom)
    }
}

/// What an importer read: only what its manifest says it `provides`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Imported {
    #[serde(default)]
    pub tracks: Vec<ImportedTrack>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sessions: Vec<ImportedLog>,
    /// What it couldn't read, in a line each.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

/// A track the file has something about, and what.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ImportedTrack {
    /// How the host finds it on the drive.
    #[serde(rename = "match")]
    pub find: TrackMatch,
    /// In any notation rekordbox shows: Camelot (`8A`), Open Key (`1m`) or a note name (`Am`).
    /// `fields`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    /// `fields`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bpm: Option<f64>,
    /// `cues`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cues: Vec<ImportedCue>,
    /// My Tags, `Category/Tag`. `tags`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub my_tags: Vec<String>,
}

/// A track as the file names it, each left out when it doesn't say: by its audio file, its ISRC,
/// then its artist, title and length, as a tracklist's entries are found.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct TrackMatch {
    /// The audio file, as the file has it: absolute on the computer it was made on, or below a
    /// drive's root (`/Contents/...`). Its last parts are what count.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub isrc: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artist: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// In seconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration: Option<u32>,
}

/// A cue point: a hot cue, or a memory cue, maybe a loop.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ImportedCue {
    /// `A` to `H`; a memory cue when left out.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hot_cue: Option<String>,
    /// Where it is, in milliseconds from the start.
    pub start_ms: u32,
    /// A loop this many beats long (a whole number, or 0.5, 0.25...).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub loop_beats: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// `#rrggbb`, or a colour's name (`red`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
}

/// A log of what played: one session, a night say. `sessions`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ImportedLog {
    /// What it's called [default: the file's name].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// When it started (RFC 3339, `2026-10-03T23:14:05+02:00`): a play's time when it has none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started: Option<String>,
    /// In the order they played.
    pub plays: Vec<ImportedPlay>,
}

/// A track that played, as the log names it: found on the drive as a track is.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ImportedPlay {
    #[serde(default)]
    pub artist: String,
    #[serde(default)]
    pub title: String,
    /// When it started playing (RFC 3339) [default: the session's start].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at: Option<String>,
    /// Its audio file, as the log has it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

impl Hook for ReadImport {
    const METHOD: &'static str = READ_IMPORT;
    type Answer = ImportAnswer;

    fn declared(&self, c: &Contributions) -> Result<(), RpcError> {
        declared("importer", &self.importer, c.importers.iter().map(|x| &x.format))
    }
}

/// Run with `import/read` on a file the user names: the host finds what it read on a drive and
/// plans the edits itself, so it needs no capability to plan one.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Importer {
    pub format: String,
    pub title: String,
    /// The file it reads usually ends in this: lowercase letters and digits, without the dot
    /// (`nml`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extension: Option<String>,
    /// A JSON Schema for its options (an object).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub options: Option<Value>,
    /// What it reads: what of a track's (and the plays) the host takes from it.
    pub provides: Vec<ImportKind>,
}

/// What an importer reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ImportKind {
    /// A track's key and BPM.
    Fields,
    Cues,
    /// My Tags.
    Tags,
    /// What played, as sessions.
    Sessions,
}

impl ImportKind {
    /// As a manifest names it (`fields`).
    pub fn name(self) -> &'static str {
        match self {
            ImportKind::Fields => "fields",
            ImportKind::Cues => "cues",
            ImportKind::Tags => "tags",
            ImportKind::Sessions => "sessions",
        }
    }
}

/// `[[contributes.importers]]`: formats, titles, a file's extension, options, and what each
/// provides, each once.
pub(crate) fn validate(m: &Manifest) -> Result<(), ManifestError> {
    let (at, c) = ("contributes.importers", &m.contributes);
    unique_names(at, "format", c.importers.iter().map(|x| x.format.as_str()).enumerate())?;
    for (i, x) in c.importers.iter().enumerate() {
        shown(format!("{at}[{i}].title"), &x.title)?;
        if let Some(ext) = x.extension.as_deref().filter(|ext| !is_extension(ext)) {
            let message = format!("{ext:?} isn't an extension: lowercase letters and digits, without the dot");
            return Err(invalid(format!("{at}[{i}].extension"), message));
        }
        object_schema(format!("{at}[{i}].options"), x.options.as_ref())?;
        if x.provides.is_empty() {
            return Err(invalid(format!("{at}[{i}].provides"), "names nothing: fields, cues, tags or sessions"));
        }
        let mut seen = FxHashSet::default();
        if let Some(kind) = x.provides.iter().find(|k| !seen.insert(**k)) {
            return Err(invalid(format!("{at}[{i}].provides"), format!("{} is there twice", kind.name())));
        }
    }
    Ok(())
}
