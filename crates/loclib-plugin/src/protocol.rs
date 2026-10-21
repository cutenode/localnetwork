//! The messages host and plugin exchange: the handshake, `describe`, the hooks, progress, partial
//! results, fetching from the web and cancelling. Params and results are these types as JSON
//! objects.

use std::path::PathBuf;

use indexmap::IndexMap;
use schemars::JsonSchema;
use semver::Version;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{Choice, FieldValue, ProviderKind, RpcError};

/// The protocol's version, versioned apart from locallibrary's. A new kind of hook doesn't change
/// it: it's an optional feature (see [`crate::FEATURES`]).
pub const PROTOCOL_VERSION: &str = "0.2.0";

/// The methods' names.
pub mod method {
    /// Host to plugin, first: [`super::Hello`] -> [`super::HelloReply`].
    pub const HELLO: &str = "hello";
    /// Host to plugin: no params -> the plugin's [`crate::Manifest`].
    pub const DESCRIBE: &str = "describe";
    /// Host to plugin: [`super::RunCommand`] -> any JSON (a [`crate::View`] for a view command).
    pub const RUN_COMMAND: &str = "command/run";
    /// Host to plugin: [`super::AnalyzeTrack`] -> [`super::Analysis`].
    pub const ANALYZE_TRACK: &str = "track/analyze";
    /// Host to plugin: [`super::WriteExport`] -> [`super::Exported`].
    pub const WRITE_EXPORT: &str = "export/write";
    /// Host to plugin: [`super::PlanOp`] -> [`super::OpAnswer`].
    pub const PLAN_OP: &str = "op/plan";
    /// Host to plugin (feature `providers`): [`super::ProvideCandidates`] -> [`super::Candidates`].
    pub const PROVIDE: &str = "provide/candidates";
    /// Notification, plugin to host: [`super::ProgressParams`].
    pub const PROGRESS: &str = "progress";
    /// Notification, plugin to host (feature `blobs`): [`super::Partial`].
    pub const PARTIAL: &str = "partial";
    /// Plugin to host (feature `net`): [`super::Fetch`] -> [`super::Fetched`].
    pub const NET_FETCH: &str = "net/fetch";
    /// Notification, either way: [`super::Cancel`].
    pub const CANCEL: &str = "$/cancel";
}

/// Whether the other side's protocol version is one this side speaks: semver's caret rule both
/// ways, so the major version must match, and before 1.0 the minor version too.
pub fn check_protocol(theirs: &str) -> Result<(), RpcError> {
    let ours = Version::parse(PROTOCOL_VERSION).expect("PROTOCOL_VERSION is a version");
    let compatible =
        Version::parse(theirs).is_ok_and(|v| v.major == ours.major && (ours.major > 0 || v.minor == ours.minor));
    if compatible {
        return Ok(());
    }
    Err(RpcError::failed("incompatible", format!("protocol {theirs:?} isn't compatible with {PROTOCOL_VERSION}")))
}

/// `hello`: the host's first message.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Hello {
    /// The protocol version the host speaks.
    pub protocol: String,
    pub host: HostInfo,
    /// The optional features the host has (see [`crate::FEATURES`]). Names a side doesn't know
    /// are ignored.
    #[serde(default)]
    pub features: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct HostInfo {
    /// `locallibrary`.
    pub name: String,
    pub version: String,
}

/// What a plugin answers `hello` with.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct HelloReply {
    pub protocol: String,
    pub plugin: PluginInfo,
    /// The optional features the plugin implements: the host calls only those it has too.
    #[serde(default)]
    pub features: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PluginInfo {
    pub id: String,
    pub version: String,
}

/// Where the user was when they ran a command or an operation (for an analyzer or an exporter,
/// the drive), and the plugin's settings. Everything is optional.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Context {
    /// The drive's id or name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub drive: Option<String>,
    /// The playlist open, by path (`Folder/Name`) or unique name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub playlist: Option<String>,
    /// The tracks selected, by id.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tracks: Vec<u32>,
    /// The plugin's settings, by key: each one the user set, else its default. Never a secret:
    /// those only go into `net/fetch` requests, filled in by the host.
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    pub settings: IndexMap<String, Value>,
    /// A folder for this request alone (feature `blobs`), absolute and empty: files the answer
    /// refers to go here (see [`Blob`]). The host removes it once it has read them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scratch: Option<PathBuf>,
}

/// A file in the request's [`Context::scratch`] folder standing for a value too big for a
/// message (an image, a file to import): `{"blob": "cover.jpg"}`, by its name there. The host
/// reads it itself, and refuses one outside the folder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Blob {
    pub blob: String,
}

/// `command/run`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RunCommand {
    /// One of the manifest's commands.
    pub command: String,
    /// What the command's `params` schema describes.
    #[serde(default)]
    pub params: Value,
    #[serde(default)]
    pub context: Context,
}

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

/// What to export. An export always reads the drive's whole library as locallibrary combines it
/// (`export.pdb` with `exportExt.pdb`, or OneLibrary); `playlists` and `history` only narrow which
/// lists go in, and with neither, every list does.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ExportSource {
    /// The drive's id or name.
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

/// `op/plan`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PlanOp {
    /// One of the manifest's operations.
    pub operation: String,
    /// What the operation's `params` schema describes.
    #[serde(default)]
    pub params: Value,
    #[serde(default)]
    pub context: Context,
}

/// What `op/plan` answers: a plan, or first a question for the user (`{"choose": ...}`, feature
/// `choose`), once answered planned again with the param set (see [`Choice`]).
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum OpAnswer {
    Choose { choose: Choice },
    Plan(OpPlan),
}

impl From<OpPlan> for OpAnswer {
    fn from(plan: OpPlan) -> OpAnswer {
        OpAnswer::Plan(plan)
    }
}

impl From<Choice> for OpAnswer {
    fn from(choose: Choice) -> OpAnswer {
        OpAnswer::Choose { choose }
    }
}

/// Read by its `choose` key rather than by trying each kind: what's wrong with a plan is said as
/// a plan's error.
impl<'de> Deserialize<'de> for OpAnswer {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<OpAnswer, D::Error> {
        let answer = Value::deserialize(d)?;
        let read = match answer.get("choose") {
            Some(choose) => Choice::deserialize(choose).map(OpAnswer::from),
            None => OpPlan::deserialize(answer).map(OpAnswer::Plan),
        };
        read.map_err(serde::de::Error::custom)
    }
}

/// An operation's plan: edits for the host to plan as one `edit.plan` (one confirmation, one
/// backup, applied together), confirm and apply itself. A plugin never writes to a drive.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct OpPlan {
    /// One line on what it would do, for the confirmation.
    pub summary: String,
    #[serde(default)]
    pub warnings: Vec<String>,
    /// Each an `Edit` as the sidecar's `edit.plan` takes them (`{"op": "create_playlist", ...}`),
    /// which the host checks against its own schema. Whether they're destructive is the host's
    /// to work out.
    pub edits: Vec<Value>,
    /// Files on this computer (absolute) the plan was worked out from, a tracklist say: applying
    /// it is refused once one of them changes.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub inputs: Vec<PathBuf>,
}

/// `provide/candidates` (feature `providers`): what the provider has for these tracks, of its
/// kind. The host asks every provider of a kind, offers the user what they answer, and makes the
/// edits chosen itself: a provider never plans one.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ProvideCandidates {
    /// One of the manifest's providers.
    pub provider: String,
    /// Its kind, as the manifest declares it.
    pub kind: ProviderKind,
    pub tracks: Vec<TrackTags>,
    #[serde(default)]
    pub context: Context,
}

/// A track as a provider looks it up: by its tags, each left out when it has none.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TrackTags {
    pub id: u32,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub artist: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub title: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub album: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub album_artist: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub isrc: String,
    /// In seconds.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub duration: u32,
}

fn is_zero(n: &u32) -> bool {
    *n == 0
}

/// What `provide/candidates` answers: what it found for the tracks, any number for each (or
/// none), and why it found nothing for some, in a line each.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Candidates {
    pub candidates: Vec<Candidate>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

/// What a provider found for a track: an image (an artwork provider's), or tags (a metadata
/// provider's).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Candidate {
    /// The track's id, as it was asked for.
    pub track: u32,
    /// How sure the provider is that it's the track's, 0 to 1: the host picks one alone at the
    /// top for the user, when they let it.
    pub score: f64,
    /// Where it's from, in a few words (`Cover Art Archive`).
    pub source: String,
    /// What it is, in a line (`Discovery, album`).
    pub label: String,
    /// The image, a file in the request's scratch folder.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artwork: Option<Blob>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<TrackMetadata>,
}

/// The tags a metadata provider has for a track: each it doesn't have left out.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
pub struct TrackMetadata {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artist: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub album: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub year: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub genre: Option<String>,
    /// The record label.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub isrc: Option<String>,
}

/// A `progress` notification's params.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ProgressParams {
    /// The host's request it's about.
    pub id: Value,
    pub event: ProgressReport,
}

/// A progress event as a host reads it.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum ProgressReport {
    Known(ProgressEvent),
    /// From a newer plugin: an event or a phase this side has no name for, which it can skip.
    Unknown(Value),
}

/// The RPC sidecar's progress event: what happened, the drive (`target`) and step it's about, and
/// for a coalesced `item`, how many items it stands for (count `items ?? 1`).
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ProgressEvent {
    #[serde(flatten)]
    pub event: Progressed,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub step: Option<StepInfo>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub items: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum Progressed {
    Event(loclib_progress::Event),
    /// A step begins (`step` says which).
    Step(StepStarted),
}

/// `{"event": "step"}`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct StepStarted {
    pub event: StepTag,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, JsonSchema)]
pub enum StepTag {
    #[default]
    #[serde(rename = "step")]
    Step,
}

/// Step `index` of `total`, counting from 1, and its name (`id`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct StepInfo {
    pub index: u32,
    pub total: u32,
    pub id: String,
}

/// A `partial` notification's params: the next rows of a view command's table, or items of its
/// list, for the host to show while the rest is worked out. The view answered at the end still
/// has every row (or item), these first, in the order they were sent: a host that shows none
/// loses nothing.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Partial {
    /// The host's request it's about.
    pub id: Value,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rows: Vec<Vec<Value>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub items: Vec<crate::ListItem>,
}

/// `net/fetch`: an HTTP request the host makes for the plugin, to a host its `net:<host>`
/// capabilities grant (https only, redirects checked hop by hop), as `locallibrary/<version>`.
/// `{secret:<key>}` in a header's value, a query value or `body` stands for the plugin's secret
/// setting `key`, which the host fills in after the checks: the plugin never sees it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Fetch {
    pub url: String,
    /// `GET` unless it says.
    #[serde(default = "get")]
    pub method: String,
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    pub headers: IndexMap<String, String>,
    /// Text to send.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
    /// A file in the request's scratch folder to send instead (absolute).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body_blob: Option<PathBuf>,
    /// Write what it answers to this file in the request's scratch folder (absolute) rather than
    /// answering it: for an image, or more than the host answers in a message.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub into_blob: Option<PathBuf>,
}

fn get() -> String {
    "GET".into()
}

impl Fetch {
    /// A `GET` of `url`.
    pub fn get(url: impl Into<String>) -> Fetch {
        Fetch { url: url.into(), method: get(), headers: IndexMap::new(), body: None, body_blob: None, into_blob: None }
    }
}

/// What `net/fetch` answers: the status and headers (names in lowercase) of the last answer
/// redirects led to, and its body, as text or in the file `into_blob` named.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Fetched {
    pub status: u16,
    #[serde(default)]
    pub headers: IndexMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
    /// The file it's in, with `into_blob`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blob: Option<PathBuf>,
}

/// `$/cancel`: stop the request `id` at its next safe point. It then fails with kind `cancelled`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Cancel {
    pub id: Value,
}

#[cfg(test)]
mod tests;
