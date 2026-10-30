//! The messages host and plugin exchange: the handshake, `describe`, what every hook is given,
//! progress, partial results, fetching from the web and cancelling. The hooks' own are their
//! families' (see `crate::hooks`). Params and results are these types as JSON objects.

use std::path::PathBuf;

use indexmap::IndexMap;
use schemars::JsonSchema;
use semver::Version;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::RpcError;

/// The protocol's version, versioned apart from locallibrary's. A new kind of hook doesn't change
/// it: it's an optional feature (see [`crate::FEATURES`]).
pub const PROTOCOL_VERSION: &str = "0.2.0";

/// The methods' names.
pub mod method {
    pub use crate::hooks::accounts::ACCOUNTS_ENSURE;
    pub use crate::hooks::analyzers::ANALYZE_TRACK;
    pub use crate::hooks::checks::{PLAN_CHECK, RUN_CHECK};
    pub use crate::hooks::commands::RUN_COMMAND;
    pub use crate::hooks::destinations::{BLOBS_GET, BLOBS_HAS, BLOBS_PUT, MANIFEST_GET, MANIFEST_PUT, SET_PUBLISH};
    pub use crate::hooks::events::{EVENT_NOTIFY, HOST_NOTIFY};
    pub use crate::hooks::exporters::WRITE_EXPORT;
    pub use crate::hooks::importers::READ_IMPORT;
    pub use crate::hooks::operations::PLAN_OP;
    pub use crate::hooks::panels::RENDER_PANEL;
    pub use crate::hooks::providers::PROVIDE;
    pub use crate::hooks::services::SERVICE_START;
    pub use crate::hooks::sources::READ_LIBRARY;

    /// Host to plugin, first: [`super::Hello`] -> [`super::HelloReply`].
    pub const HELLO: &str = "hello";
    /// Host to plugin: no params -> the plugin's [`crate::Manifest`].
    pub const DESCRIBE: &str = "describe";
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

/// Where the user was when they ran a command or an operation (for an analyzer, an exporter or an
/// importer, the drive), and the plugin's settings. Everything is optional.
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
    /// The set open, by id (`sets.get` reads it).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub set: Option<String>,
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
#[derive(Debug, Clone, Serialize, JsonSchema)]
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

/// Read by hand: a phase's start has `items` of its own (how many there are), which a derived
/// reader would take for the event's count and leave the start without.
impl<'de> Deserialize<'de> for ProgressEvent {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Scope {
            target: Option<String>,
            step: Option<StepInfo>,
            items: Option<u64>,
        }
        let v = Value::deserialize(d)?;
        let Scope { target, step, items } = Scope::deserialize(&v).map_err(serde::de::Error::custom)?;
        let event = Progressed::deserialize(&v).map_err(serde::de::Error::custom)?;
        let items = items.filter(|_| matches!(event, Progressed::Event(loclib_progress::Event::Item { .. })));
        Ok(ProgressEvent { event, target, step, items })
    }
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
/// `{secret:<key>}` in a header's value, a query value, `body` or a `multipart` part's value
/// stands for the plugin's secret setting `key`, and `{account:<type>}` as the whole value of the
/// `Authorization` header, or of the query parameter the account type names, for the account of
/// that type it has access to (feature `accounts`): the host fills them in after the checks (an
/// account's access token after `Bearer`, or the scheme the type names, in the header), and the
/// plugin never sees either.
/// The host says how long the body is (`Content-Length`), and for `multipart`, its type.
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
    /// A file to send instead (absolute): one in the request's scratch folder, or one a hook
    /// under way was given to send (`set/publish`'s `recording`, but for a dry run). The host
    /// reads it as it sends it, again for a redirect that asks for the body again, and reports
    /// the bytes sent as the hook's progress (`bytes` events of phase `upload`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body_blob: Option<PathBuf>,
    /// Only this part of `body_blob`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body_range: Option<ByteRange>,
    /// A `multipart/form-data` form to send instead, its parts in this order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub multipart: Vec<FormPart>,
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
        Fetch {
            url: url.into(),
            method: get(),
            headers: IndexMap::new(),
            body: None,
            body_blob: None,
            body_range: None,
            multipart: Vec::new(),
            into_blob: None,
        }
    }
}

/// `length` bytes of a file from `offset` (to its end, when it doesn't say): an upload's part. A
/// range past the file's end is refused.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ByteRange {
    #[serde(default)]
    pub offset: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub length: Option<u64>,
}

/// A part of a `multipart/form-data` form: text (`value`), or a file (`file`, as `body_blob` names
/// one, perhaps only its `range`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct FormPart {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub range: Option<ByteRange>,
    /// The name the server is told the file has.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filename: Option<String>,
    /// `audio/mpeg`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_type: Option<String>,
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
