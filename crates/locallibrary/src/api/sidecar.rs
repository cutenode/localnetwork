//! The RPC sidecar's own requests, results and notifications: the handshake, cancelling, jobs,
//! events, watching and shutting down. They live here with the rest of the API's types so the
//! sidecar's contract (`rpc.describe`) describes them the same way.

use chrono::{DateTime, Utc};
use loclib_plugin::code;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use super::WatchAction;
use crate::Error;
use crate::error::{Action, Detail, Reason};
use crate::progress::{Event, Scope};
use crate::watch::{MountedDrive, WatchEvent};

/// A JSON-RPC request's id: a string or a number. Only describes ids in the schema: they're
/// passed through as they came.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum RequestId {
    Number(serde_json::Number),
    String(String),
}

/// No parameters (any object is accepted).
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct NoParams {}

/// `*.apply` by the id of the plan `*.plan` returned: applies exactly that plan, once.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ApplyPlan {
    pub plan_id: String,
}

/// Accepted by every method that writes (with its other parameters).
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct WriteOptions {
    /// Allow writing to archive and retired drives, which are refused (`conflict`) otherwise.
    #[serde(default)]
    pub overwrite_archive: bool,
}

/// What `initialize` answers.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Initialized {
    /// Changes only when a change would break clients written for the previous one.
    pub protocol: u32,
    /// The sidecar's version.
    pub version: String,
    /// Features a client can check for: `progress`, `progress.scope`, `cancel`, `jobs.list`,
    /// `shutdown.cancel`, `events`, `plan_id`, `describe`, `error.detail`, and `account` when the
    /// sidecar was built with the Atmosphere account (the `account.*` and `*.publish.*` methods,
    /// and `settings.import_published.*`).
    pub capabilities: Vec<String>,
    /// Every method's name, aliases included (`rpc.describe` says more about each).
    pub methods: Vec<String>,
}

/// What `version` answers.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct VersionInfo {
    pub version: String,
    pub methods: Vec<String>,
}

/// `$/cancel`: stop a running request at its next safe point.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct CancelRequest {
    /// The request's id (a notification, which has none, can't be cancelled).
    #[serde(default)]
    #[schemars(with = "Option<RequestId>")]
    pub id: Value,
}

/// Whether there was something to cancel.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Cancelled {
    pub cancelled: bool,
}

/// A running request, as `jobs.list` lists it.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct JobInfo {
    /// `null` for a notification (and for `watch`).
    #[schemars(with = "Option<RequestId>")]
    pub id: Value,
    pub method: String,
    pub started: DateTime<Utc>,
    /// Waiting for a worker.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub queued: bool,
}

/// `events.subscribe`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct Subscribe {
    /// Event kinds (`drive.attached`) or groups (`drive`, `store`, `job`, `watch`); empty: all.
    #[serde(default)]
    pub kinds: Vec<String>,
    /// How often drives and the store are looked at, in milliseconds (at least 100; default
    /// 1000).
    #[serde(default)]
    pub interval_ms: Option<u64>,
}

/// What `events.subscribe` answers: from here on, events say what changes.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Subscribed {
    pub kinds: Vec<String>,
    /// The drives mounted now.
    pub drives: Vec<MountedDrive>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Unsubscribed {
    /// False when there was no subscription.
    pub unsubscribed: bool,
}

/// `watch.start`'s parameters (see [`super::WatchOptions`]).
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct StartWatch {
    /// Pull drives that hold changes the store lacks.
    #[serde(default)]
    pub pull: bool,
    /// Then sync the rest of the drive's group from it (`sync_set`, the old name, works too).
    #[serde(default, alias = "sync_set")]
    pub sync_group: bool,
    /// How long a drive must go unchanged before it's looked at, in milliseconds.
    #[serde(default)]
    pub settle_ms: Option<u64>,
    /// How often drives are polled, in milliseconds (at least 100).
    #[serde(default)]
    pub interval_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct WatchStarted {
    pub watching: bool,
}

/// `watch.stop`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct StopWatch {
    /// Stop what it's doing too, instead of letting it finish.
    #[serde(default)]
    pub cancel: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct WatchStopped {
    /// False when it wasn't watching.
    pub stopped: bool,
}

/// `shutdown`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct Shutdown {
    /// Cancel running requests instead of waiting for them.
    #[serde(default)]
    pub cancel: bool,
}

/// `store.gc` with a request (rather than a `plan_id`).
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct GcRequest {
    /// Say what would go, without deleting it.
    #[serde(default)]
    pub dry_run: bool,
}

/// `rpc.describe`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct DescribeRequest {
    /// One method (or an alias of one); default every method.
    #[serde(default)]
    pub method: Option<String>,
}

/// A request that failed, as the sidecar answers it: `error`'s code and message, and its `data`
/// (see [`RpcFailure::data`]). The plugin host reads the reads it makes for plugins as these too.
#[derive(Debug, Clone)]
pub struct RpcFailure {
    pub code: i64,
    pub message: String,
    pub kind: &'static str,
    /// Its reason, the values it's about and what would resolve it (see [`Error::detail`]).
    // Boxed: it's most of the failure's size, which every `Result` carrying one would be.
    pub detail: Box<Detail>,
}

impl From<Error> for RpcFailure {
    fn from(e: Error) -> Self {
        RpcFailure { code: code::FAILED, kind: e.code(), message: e.to_string(), detail: Box::new(e.detail()) }
    }
}

impl RpcFailure {
    pub fn new(code: i64, kind: &'static str, message: impl Into<String>) -> RpcFailure {
        RpcFailure { code, kind, message: message.into(), detail: Box::default() }
    }

    pub fn method_not_found(method: &str) -> RpcFailure {
        RpcFailure::new(code::METHOD_NOT_FOUND, "method_not_found", format!("unknown method {method:?}"))
    }

    /// Makes the actions that send the request again (`method` absent) calls to `method` with
    /// `params` and theirs.
    pub fn resolve(mut self, method: &str, params: &Value) -> RpcFailure {
        for a in self.detail.actions.iter_mut().filter(|a| a.method.is_none()) {
            let mut all = params.as_object().cloned().unwrap_or_default();
            all.extend(std::mem::take(&mut a.params));
            a.method = Some(method.to_string());
            a.params = all;
        }
        self
    }

    /// The error of applying a plan by id that's used up: sending the request again can't help,
    /// so actions that would are dropped.
    pub fn plan_used(mut self) -> RpcFailure {
        self.detail.actions.retain(|a| a.method.is_some());
        self
    }

    /// `error.data`.
    pub fn data(&self) -> ErrorData {
        ErrorData {
            kind: self.kind.to_string(),
            reason: self.detail.reason,
            fields: self.detail.fields.clone(),
            actions: self.detail.actions.clone(),
        }
    }
}

/// An error response's `error.data`: what a front end can act on without reading the message.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ErrorData {
    /// Stable: `not_found`, `conflict`, `stale_plan`, `invalid_params`... (see [`crate::Error::code`]).
    pub kind: String,
    /// Which decision a `conflict` needs, or what else would resolve the error.
    pub reason: Option<Reason>,
    /// The values it's about: a `path` (for `invalid_params`, where in the request), the space
    /// `needed` and `available`, the `drive`, `left` and `right`...
    pub fields: Map<String, Value>,
    /// What a front end can offer: call `method` with `params` (the request that failed, with
    /// some parameters added, is one), with administrator rights when `elevate` is set.
    pub actions: Vec<Action>,
}

/// A `progress` notification's parameters.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct ProgressParams {
    /// The request it's about.
    #[schemars(with = "Option<RequestId>")]
    pub id: Value,
    pub event: ProgressEvent,
}

/// What happened, which drive and step it's about, and (for a coalesced `item`) how many files
/// it stands for.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct ProgressEvent {
    #[serde(flatten)]
    pub event: Progressed,
    #[serde(flatten)]
    pub scope: Scope,
    /// How many files an `item` stands for, when more than one (count `items ?? 1`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub items: Option<u64>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum Progressed {
    Event(Event),
    /// A step begins (`step` says which).
    Step(StepStarted),
}

/// `{"event": "step"}`.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct StepStarted {
    pub event: StepTag,
}

impl StepStarted {
    pub fn new() -> StepStarted {
        StepStarted { event: StepTag::Step }
    }
}

impl Default for StepStarted {
    fn default() -> Self {
        StepStarted::new()
    }
}

#[derive(Debug, Clone, Copy, Serialize, JsonSchema)]
pub enum StepTag {
    #[serde(rename = "step")]
    Step,
}

/// An `event` notification's parameters: drives and the store (after `events.subscribe`),
/// requests starting and finishing, what `watch.start` does, and what plugins tell the user.
#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(untagged)]
#[non_exhaustive]
pub enum Notice {
    Watch(WatchEvent),
    Job(JobEvent),
    Watched(WatchAction),
    Plugin(PluginEvent),
}

/// What a plugin did.
#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(tag = "event")]
#[non_exhaustive]
pub enum PluginEvent {
    /// It tells the user something (`host/notify`), and may suggest one of its operations, which
    /// a front end offers to plan (`plugins.op.plan`): never planned or applied unless the user
    /// chooses to.
    #[serde(rename = "plugin.notice")]
    Notice(crate::plugin::PluginNotice),
}

/// A request starting or finishing.
#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(tag = "event")]
#[non_exhaustive]
pub enum JobEvent {
    #[serde(rename = "job.started")]
    Started {
        #[schemars(with = "Option<RequestId>")]
        id: Value,
        method: String,
        started: DateTime<Utc>,
    },
    #[serde(rename = "job.finished")]
    Finished {
        #[schemars(with = "Option<RequestId>")]
        id: Value,
        method: String,
        ok: bool,
        /// The error's `kind`, when it failed.
        #[serde(skip_serializing_if = "Option::is_none")]
        kind: Option<String>,
    },
}

#[cfg(test)]
mod tests;
