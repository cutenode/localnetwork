//! Feature `exporters`: a drive's library, a set or a mix written in another program's format, on
//! this computer.

use std::path::PathBuf;

use indexmap::IndexMap;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::manifest::{invalid, object_schema, shown, unique_names};
use crate::sdk::declared;
use crate::{Capability, Context, Contributions, Feature, Hook, Manifest, ManifestError, RpcError};

/// Host to plugin: [`WriteExport`] -> [`Exported`].
pub const WRITE_EXPORT: &str = "export/write";

pub(crate) const FEATURE: Feature = Feature {
    name: "exporters",
    methods: &[WRITE_EXPORT],
    sends: &[],
    contributed: |m| !m.contributes.exporters.is_empty(),
};

/// `export/write`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct WriteExport {
    /// One of the manifest's exporters, by format.
    pub exporter: String,
    pub source: ExportSource,
    /// Where to write, absolute: a file, or a folder for an exporter that writes one.
    pub output: PathBuf,
    /// What the exporter's `options` schema describes.
    #[serde(default)]
    pub options: Value,
    /// Answer the files it would write, writing nothing: the host asks before overwriting any.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub dry_run: bool,
    #[serde(default)]
    pub context: Context,
}

/// What to export, as the exporter's `source` says: a drive's library, a set or a mix. A drive's
/// export always reads its whole library as locallibrary combines it (`export.pdb` with
/// `exportExt.pdb`, or OneLibrary); `playlists` and `history` only narrow which lists go in, and
/// with neither, every list does. A set's or a mix's names the drive its tracks are on, when
/// there's one.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ExportSource {
    /// The drive's id or name: always a drive exporter's.
    #[serde(default)]
    pub drive: String,
    /// Where it's mounted, absolute, as the host sets it: its tracks' audio files are below it,
    /// at their `file_path`, and the files an export writes may name them there. Read them; never
    /// write to the drive.
    #[serde(default)]
    pub root: PathBuf,
    /// Playlists, by path (`Folder/Name`) or unique name.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub playlists: Vec<String>,
    /// History lists the players recorded, by name, as `playlists.tree` lists them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub history: Vec<String>,
    /// A set exporter's set, by id: `sets.get` reads it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub set: Option<String>,
    /// A mix exporter's mix, the report kept of it, by id: `mix.reports.get` reads it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mix: Option<String>,
}

/// What `export/write` answers.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Exported {
    /// The files written (or, for a dry run, that would be).
    pub files: Vec<ExportedFile>,
    /// What went in, as labelled numbers in the order to show them (`{"tracks": 12, "cues":
    /// 40}`): the labels are the plugin's own words.
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    pub counts: IndexMap<String, u64>,
    /// What the format can't hold, and what to do with the files: the same for any library.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
    /// Problems with this library: missing audio, cues the format has no room for...
    #[serde(default)]
    pub warnings: Vec<String>,
}

/// A file an export writes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
// Named apart from locallibrary's list exports' `ExportedFile` in the sidecar's contract.
#[schemars(rename = "PluginExportedFile")]
pub struct ExportedFile {
    /// Absolute, below the export's `output`.
    pub path: PathBuf,
    /// What it holds, in a few words (`Traktor collection`).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub what: String,
    /// Its size, as far as it's known before it's written.
    #[serde(default)]
    pub bytes: u64,
}

impl Hook for WriteExport {
    const METHOD: &'static str = WRITE_EXPORT;
    type Answer = Exported;

    fn declared(&self, c: &Contributions) -> Result<(), RpcError> {
        declared("exporter", &self.exporter, c.exporters.iter().map(|x| &x.format))
    }
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
    /// What it writes out [default: a drive's library]: one source (`"set"`), or a list of them
    /// (`["drive", "set", "mix"]`) for an exporter that writes each out the same way.
    #[serde(default, skip_serializing_if = "ExportSources::is_default")]
    pub source: ExportSources,
}

impl Exporter {
    /// What it writes out, each once.
    pub fn sources(&self) -> &[ExportFrom] {
        match &self.source {
            ExportSources::One(from) => std::slice::from_ref(from),
            ExportSources::Several(list) => list,
        }
    }

    /// Whether it writes out `from`.
    pub fn takes(&self, from: ExportFrom) -> bool {
        self.sources().contains(&from)
    }
}

/// What an exporter writes out, as its manifest says it: one source, or a list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum ExportSources {
    One(ExportFrom),
    Several(Vec<ExportFrom>),
}

impl Default for ExportSources {
    fn default() -> Self {
        ExportSources::One(ExportFrom::Drive)
    }
}

impl ExportSources {
    fn is_default(&self) -> bool {
        *self == ExportSources::default()
    }
}

/// What an exporter writes out.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExportFrom {
    /// A drive's library.
    #[default]
    Drive,
    /// A set the DJ declared (`locallibrary sets`).
    Set,
    /// A recorded mix's tracklist, as `locallibrary mix` found it.
    Mix,
}

impl ExportFrom {
    /// As a manifest names it (`set`).
    pub fn name(self) -> &'static str {
        match self {
            ExportFrom::Drive => "drive",
            ExportFrom::Set => "set",
            ExportFrom::Mix => "mix",
        }
    }
}

/// An exporter's (or an importer's) file extension: no dot, just lowercase letters and digits.
pub(crate) fn is_extension(s: &str) -> bool {
    (1..=16).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
}

/// `[[contributes.exporters]]`: formats (one per exporter, whatever it writes out), titles, a
/// file's extension or a folder, options, and sources, each once.
pub(crate) fn validate(m: &Manifest) -> Result<(), ManifestError> {
    let (at, c) = ("contributes.exporters", &m.contributes);
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
        let sources = x.sources();
        if sources.is_empty() {
            return Err(invalid(format!("{at}[{i}].source"), "an exporter writes out a drive, a set or a mix"));
        }
        if let Some(twice) = sources.iter().enumerate().find_map(|(j, s)| sources[..j].contains(s).then_some(s)) {
            return Err(invalid(format!("{at}[{i}].source"), format!("{} is listed twice", twice.name())));
        }
        let read = sources.iter().find(|s| **s != ExportFrom::Drive);
        if let Some(from) = read.filter(|_| !m.capabilities.contains(&Capability::SetsRead)) {
            let message = format!("reads {}s, which needs the {} capability", from.name(), Capability::SetsRead);
            return Err(invalid(format!("{at}[{i}].source"), message));
        }
    }
    Ok(())
}
