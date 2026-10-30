use std::io;
use std::path::{Path, PathBuf};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// Everything that can go wrong in locallibrary.
///
/// Every variant maps to a stable machine-readable [`Error::code`] so front ends (the CLI's
/// `--json` mode, the JSON-RPC sidecar, an Electron app) can react without parsing messages.
/// [`Error::detail`] says more: a [`Reason`] (which decision a `conflict` needs, say), the values
/// the error is about, and the [`Action`]s that would resolve it. Messages say what's wrong
/// without assuming a front end: how to do something about it in the CLI (a command, a flag) is
/// the CLI's to say, from the reason and actions.
///
/// Detail wraps the error it's about ([`Error::Detailed`]), so match on [`Error::inner`]
/// (`matches!(e.inner(), Error::Conflict(_))`), or on [`Error::code`] and [`Error::reason`].
///
/// New kinds of error may come (as may new [`Reason`]s, and the variants of the other enums a
/// front end reacts to: progress [`Event`](crate::progress::Event)s and phases, sidecar
/// notices, watch events, doctor's problems...): a `match` needs a `_` arm.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    #[error("{}: {source}", path.display())]
    Io { path: PathBuf, source: io::Error },

    #[error("{} is not a rekordbox export (no PIONEER/rekordbox/export.pdb or exportLibrary.db)", .0.display())]
    NotAnExport(PathBuf),

    #[error("could not parse {}: {message}", path.display())]
    Parse { path: PathBuf, message: String },

    #[error("{0}")]
    Invalid(String),

    #[error("not found: {0}")]
    NotFound(String),

    /// A decision is needed first: [`Error::detail`]'s reason says which.
    #[error("{0}")]
    Conflict(String),

    /// A plan no longer matches what it would change (the drive was written, or rekordbox
    /// exported to it, since it was made): plan again.
    #[error("{0}")]
    StalePlan(String),

    #[error("{} is corrupt: expected {expected}, found {actual}", path.display())]
    Corrupt { path: PathBuf, expected: String, actual: String },

    #[error("not enough free space on {}: need {needed} bytes, {available} available", path.display())]
    NoSpace { path: PathBuf, needed: u64, available: u64 },

    #[error("operation cancelled")]
    Cancelled,

    #[error("{} is in use by another locallibrary operation", .0.display())]
    Locked(PathBuf),

    #[error("export.pdb: {0}")]
    Pdb(String),

    /// `exportExt.pdb` (My Tags) can't be edited. Its code is `pdb`, as for export.pdb.
    #[error("exportExt.pdb: {0}")]
    ExportExt(String),

    /// The OneLibrary database (`exportLibrary.db`) can't be read, edited or written.
    #[error("exportLibrary.db: {0}")]
    OneLibrary(String),

    #[error("image: {0}")]
    Image(String),

    #[error("tags: {0}")]
    Tags(String),

    /// A system tool (diskutil, mkfs, PowerShell...) failed.
    #[error("{command} failed: {message}")]
    External { command: String, message: String },

    /// The operation needs administrator rights (root, an elevated prompt).
    #[error("{0}")]
    Permission(String),

    /// Signing in to an Atmosphere account, or using its session, failed: not signed in, the
    /// session ended, the sign-in was declined, the server couldn't be reached...
    #[error("{0}")]
    Account(String),

    /// Reaching another computer's store over the network failed: it couldn't be reached, it
    /// refused the share code, or it stopped serving part way.
    #[error("{0}")]
    Peer(String),

    /// Another error, with what a front end needs to act on it (see [`Error::detail`]). Its
    /// code and message are the other error's.
    #[error("{error}")]
    Detailed { error: Box<Error>, detail: Box<Detail> },
}

/// Why an error needs a decision, or what would resolve it: a stable code a front end can switch
/// on (`data.reason` over RPC), next to the error's [`Error::code`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum Reason {
    // `conflict`: a decision is needed.
    /// Which of two drives is newer can't be told with confidence: choose a direction.
    Uncertain,
    /// Both drives changed since they were last in sync: merge them, or choose a direction (the
    /// other side's changes are lost).
    Diverged,
    /// The drives hold different libraries: merge them, or choose a direction to overwrite one.
    Unrelated,
    /// A drive came back from a set with changes players made: sync it as a pair first, which
    /// keeps them in the store.
    BackFromSet,
    /// What would be overwritten (a library state, history lists) isn't saved in the store:
    /// pull the drive first, or force it.
    NotSaved,
    /// The drive holds an older state of the library than the store: one its latest went
    /// through, or an earlier snapshot's (`fields.snapshot`, when a snapshot holds it). Push to
    /// update it, or force the pull to record it as the newest.
    OlderThanStore,
    /// The drive's library has diverged from the store's: force the pull, pull into another
    /// library, or merge.
    DivergedFromStore,
    /// The drive is retired (`fields.drive`): bring it back, or force the write.
    Retired,
    /// The drive is an archive drive, kept as it is: force the write.
    Archive,
    /// Something by that name, or at that path, is there already (`fields.name` or
    /// `fields.path`).
    Exists,
    /// Files of the drive's own are where the library would go (`fields.paths`): move them.
    InTheWay,
    /// A backup lists audio that's no longer on the drive (`fields.missing`): force it.
    MissingAudio,
    /// The drive changed since the change a backup takes back (`fields.changed`), or the backup
    /// doesn't record what its change left, so that can't be told (`fields.changed` empty):
    /// putting it back would take that back too. Force it.
    ChangedSince,
    /// The drive already has its own identity: re-identify it to replace it.
    HasIdentity,
    /// Another mounted drive holds that identity.
    IdentityInUse,
    /// An interrupted operation must be finished first.
    Interrupted,
    /// The copy holds a different store.
    DifferentStore,
    /// The copy has snapshots this store doesn't, or changed since it was last copied to: force
    /// it to replace them.
    CopyAhead,
    /// The label is on another snapshot: take it off first.
    LabelTaken,
    /// The snapshot is a library's latest or labelled, and stays.
    SnapshotKept,
    /// The update file has problems (`fields.problems`): force it.
    FirmwareProblems,
    /// The drive is fake: it holds less than it claims.
    FakeCapacity,
    /// What it works on changed while it worked: run it again.
    Changed,
    /// Replaying recorded edits stopped part way: undo takes back what it made.
    ReplayStopped,
    /// Files didn't read back correctly.
    VerifyFailed,
    /// A plugin's guard the user made blocking (`fields.check`, `<plugin>.<check>`) found
    /// something about the plan: deal with what it found, or stop the guard blocking.
    Blocked,

    // `invalid` and `not_found`: something to give or choose.
    /// Several libraries could be meant: name one (`fields.libraries`).
    ChooseLibrary,
    /// Several drives could be meant: name one (`fields.drives`).
    ChooseDrive,
    /// No library has that name (`fields.library`): start one, or name another.
    UnknownLibrary,
    /// It needs the store, and there's none (or it isn't given).
    NoStore,
    /// A parameter it needs is missing (`fields.param`).
    MissingParam,
    /// The drive has no OneLibrary database (`exportLibrary.db`): convert to add one.
    NoOneLibrary,
    /// The drive has no rekordbox export.
    NoExport,
    /// rekordbox's collection can't be decrypted with the key it has (`fields.env` names the
    /// variable that gives another).
    WrongKey,
    /// No setting has that key.
    UnknownSetting,
    /// The settings profile is empty: import or set some first.
    EmptyProfile,

    // `permission`.
    /// It needs administrator rights: run it again elevated.
    NeedsAdmin,

    // `account`.
    /// Not signed in to an Atmosphere account, or the account a plugin's access is to is signed
    /// out: sign in.
    NotSignedIn,
    /// The session ended or was revoked: sign in again.
    SessionEnded,
    /// The session may not publish, or a plugin's access doesn't reach what it asked: sign in
    /// again allowing it.
    MissingScope,
    /// A plugin has no access to an account of the type it asked for (`fields.type`): sign in to
    /// one for it, or make one its default.
    NoAccess,
    /// The system keychain isn't available: keep the session in a file instead.
    NoKeychain,
}

/// Something a front end can offer to resolve an error: call a method, or send the request that
/// failed again with some parameters added (`method` absent), or again with administrator rights.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Action {
    /// What a button offering it says.
    pub label: String,
    /// The RPC method to call (`pull`, `sync.plan`). Absent: the request that failed, again,
    /// with `params` added to its own.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub method: Option<String>,
    /// The parameters: the method's, or the ones to add to the request that failed. `null`
    /// means a value the user has to give (a name, a path).
    #[serde(default)]
    pub params: Map<String, Value>,
    /// Run it with administrator rights (restart the sidecar elevated first).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub elevate: bool,
}

impl Action {
    /// Call `method` with `params` (an object).
    pub fn call(method: &str, params: Value, label: impl Into<String>) -> Action {
        Action { label: label.into(), method: Some(method.into()), params: object(params), elevate: false }
    }

    /// The request that failed, again, with `params` (an object) added.
    pub fn retry(params: Value, label: impl Into<String>) -> Action {
        Action { label: label.into(), method: None, params: object(params), elevate: false }
    }

    /// The request that failed, again, with administrator rights.
    pub fn elevate(label: impl Into<String>) -> Action {
        Action { label: label.into(), method: None, params: Map::new(), elevate: true }
    }
}

fn object(v: Value) -> Map<String, Value> {
    match v {
        Value::Object(map) => map,
        _ => Map::new(),
    }
}

/// What a front end needs to act on an error: see [`Error::detail`].
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Detail {
    pub reason: Option<Reason>,
    /// The values the error is about: a `path`, the space `needed` and `available`, the
    /// `drive`, `left` and `right`...
    pub fields: Map<String, Value>,
    pub actions: Vec<Action>,
}

impl Error {
    /// Stable identifier for programmatic handling.
    pub fn code(&self) -> &'static str {
        match self {
            Error::Io { .. } => "io",
            Error::NotAnExport(_) => "not_an_export",
            Error::Parse { .. } => "parse",
            Error::Invalid(_) => "invalid",
            Error::NotFound(_) => "not_found",
            Error::Conflict(_) => "conflict",
            Error::StalePlan(_) => "stale_plan",
            Error::Corrupt { .. } => "corrupt",
            Error::NoSpace { .. } => "no_space",
            Error::Cancelled => "cancelled",
            Error::Locked(_) => "locked",
            Error::Pdb(_) | Error::ExportExt(_) => "pdb",
            Error::OneLibrary(_) => "onelibrary",
            Error::Image(_) => "image",
            Error::Tags(_) => "tags",
            Error::External { .. } => "external",
            Error::Permission(_) => "permission",
            Error::Account(_) => "account",
            Error::Peer(_) => "peer",
            Error::Detailed { error, .. } => error.code(),
        }
    }

    pub(crate) fn parse(path: impl Into<PathBuf>, message: impl ToString) -> Self {
        Error::Parse { path: path.into(), message: message.to_string() }
    }

    /// [`Error::StalePlan`]: `what` (a drive, a mount path) `happened` (`changed`, `is gone`)
    /// since the plan was made.
    pub(crate) fn stale(what: impl std::fmt::Display, happened: &str) -> Self {
        Error::StalePlan(format!("{what} {happened} since the plan was made; plan again"))
    }

    pub(crate) fn invalid(message: impl Into<String>) -> Self {
        Error::Invalid(message.into())
    }

    /// A track id the library read doesn't have, saying so when it's one of OneLibrary's own
    /// tracks in a combined library ([`crate::convert::ONELIBRARY_ONLY`]).
    pub(crate) fn no_track(id: u32) -> Self {
        match crate::convert::is_onelibrary_only(id) {
            true => Error::NotFound(format!(
                "track #{id} is one only OneLibrary has: reads see it with `combined`, and edits can't change it"
            )),
            false => Error::NotFound(format!("track #{id}")),
        }
    }

    /// A `conflict`: a decision is needed first.
    pub(crate) fn conflict(reason: Reason, message: impl Into<String>) -> Self {
        Error::Conflict(message.into()).because(reason)
    }

    /// The error itself, without the detail added to it.
    pub fn inner(&self) -> &Error {
        match self {
            Error::Detailed { error, .. } => error.inner(),
            e => e,
        }
    }

    /// The same error, with a reason.
    pub fn because(self, reason: Reason) -> Self {
        self.edit(|d| d.reason = Some(reason))
    }

    /// The same error, with a value it's about.
    pub fn with(self, key: &str, value: impl Serialize) -> Self {
        let value = serde_json::to_value(value).unwrap_or(Value::Null);
        self.edit(|d| {
            d.fields.insert(key.to_string(), value);
        })
    }

    /// The same error, with something that would resolve it.
    pub fn action(self, action: Action) -> Self {
        self.edit(|d| d.actions.push(action))
    }

    /// The same error, without the actions that send the request again (with some parameters
    /// added): for a request that can't be sent again so.
    pub fn without_retries(self) -> Self {
        match self {
            Error::Detailed { error, mut detail } => {
                detail.actions.retain(|a| a.method.is_some());
                Error::Detailed { error, detail }
            }
            e => e,
        }
    }

    fn edit(self, f: impl FnOnce(&mut Detail)) -> Self {
        match self {
            Error::Detailed { error, mut detail } => {
                f(&mut detail);
                Error::Detailed { error, detail }
            }
            error => {
                let mut detail = Detail::default();
                f(&mut detail);
                Error::Detailed { error: Box::new(error), detail: Box::new(detail) }
            }
        }
    }

    /// Its reason, the values it's about and what would resolve it: those it was given, and what
    /// its kind says (the path of an `io` error, the space a `no_space` needs, that a
    /// `permission` error wants administrator rights).
    pub fn detail(&self) -> Detail {
        let mut d = match self {
            Error::Detailed { detail, .. } => (**detail).clone(),
            _ => Detail::default(),
        };
        let mut field = |key: &str, value: Value| {
            d.fields.entry(key.to_string()).or_insert(value);
        };
        let path = |p: &Path| Value::String(p.display().to_string());
        match self.inner() {
            Error::Io { path: p, .. } | Error::Parse { path: p, .. } | Error::NotAnExport(p) | Error::Locked(p) => {
                field("path", path(p))
            }
            Error::Corrupt { path: p, expected, actual } => {
                field("path", path(p));
                field("expected", expected.clone().into());
                field("actual", actual.clone().into());
            }
            Error::NoSpace { path: p, needed, available } => {
                field("path", path(p));
                field("needed", (*needed).into());
                field("available", (*available).into());
            }
            Error::External { command, .. } => field("command", command.clone().into()),
            Error::Permission(_) => {
                d.reason.get_or_insert(Reason::NeedsAdmin);
                if !d.actions.iter().any(|a| a.elevate) {
                    d.actions.push(Action::elevate("Run it as an administrator"));
                }
            }
            _ => {}
        }
        d
    }

    /// Its reason, if it has one (see [`Error::detail`]).
    pub fn reason(&self) -> Option<Reason> {
        self.detail().reason
    }
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Attach a path to `io::Error`s.
pub(crate) trait IoContext<T> {
    fn at(self, path: impl AsRef<Path>) -> Result<T>;
}

impl<T> IoContext<T> for io::Result<T> {
    fn at(self, path: impl AsRef<Path>) -> Result<T> {
        self.map_err(|source| Error::Io { path: path.as_ref().to_path_buf(), source })
    }
}

impl From<rekordbox_pdb::PdbError> for Error {
    fn from(e: rekordbox_pdb::PdbError) -> Self {
        Error::Pdb(e.to_string())
    }
}

impl From<loclib_mysettings::Error> for Error {
    fn from(e: loclib_mysettings::Error) -> Self {
        use loclib_mysettings::Error as E;
        match e {
            E::UnknownSetting(ref key) => Error::invalid(e.to_string())
                .because(Reason::UnknownSetting)
                .with("key", key)
                .action(Action::call("settings.schema", serde_json::json!({}), "List the settings")),
            E::InvalidValue { .. } => Error::invalid(e.to_string()),
            E::Parse { file, message } => Error::parse(file.file_name(), message),
            // A variant a newer loclib-mysettings adds keeps its message.
            other => Error::invalid(other.to_string()),
        }
    }
}

impl From<loclib_equipment::UnknownModels> for Error {
    fn from(e: loclib_equipment::UnknownModels) -> Self {
        let known: Vec<&str> = loclib_equipment::MODELS.iter().map(|m| m.name).collect();
        Error::invalid(format!("not a model locallibrary knows: {} (it knows {})", e.0.join(", "), known.join(", ")))
    }
}

impl From<loclib_progress::Cancelled> for Error {
    fn from(_: loclib_progress::Cancelled) -> Self {
        Error::Cancelled
    }
}

impl From<loclib_mixid::Error> for Error {
    fn from(e: loclib_mixid::Error) -> Self {
        use loclib_mixid::Error as E;
        match e {
            E::Io { path, source } => Error::Io { path, source },
            E::Parse { path, message } => Error::Parse { path, message },
            E::Invalid(message) => Error::Invalid(message),
            E::Cancelled(_) => Error::Cancelled,
            // A variant a newer loclib-mixid adds keeps its message.
            other => Error::Invalid(other.to_string()),
        }
    }
}

impl From<loclib_analysis::Error> for Error {
    fn from(e: loclib_analysis::Error) -> Self {
        use loclib_analysis::Error as E;
        match e {
            E::Audio(e) => e.into(),
            // Audio that decoded but the analyzer couldn't measure: remembered like audio that
            // doesn't decode, as it won't measure next time either.
            E::Analyzer { path, message } => Error::Parse { path, message },
            E::Annotation { line, message } => Error::invalid(format!("line {line}: {message}")),
            E::Cancelled(_) => Error::Cancelled,
            // A variant a newer loclib-analysis adds keeps its message.
            other => Error::Invalid(other.to_string()),
        }
    }
}

impl From<loclib_transcode::Error> for Error {
    fn from(e: loclib_transcode::Error) -> Self {
        use loclib_transcode::Error as E;
        match e {
            E::Decode(e) => e.into(),
            E::Io { path, source } => Error::Io { path, source },
            // Audio the container can't hold, or tags it can't take: this file's problem.
            E::Encode { path, message } | E::Tags { path, message } => Error::Parse { path, message },
            E::TooShort { path, frames, source_rate, rate } => Error::Parse {
                path,
                message: format!("too short to copy at {rate} Hz: {frames} frame(s) at {source_rate} Hz"),
            },
            E::Invalid(message) => Error::Invalid(message),
            E::Cancelled(_) => Error::Cancelled,
            // A variant a newer loclib-transcode adds keeps its message.
            other => Error::Invalid(other.to_string()),
        }
    }
}

/// What a panic said, from its payload (as `catch_unwind` or a thread's `join` hands it back).
pub fn panic_text(payload: &(dyn std::any::Any + Send)) -> String {
    match (payload.downcast_ref::<&str>(), payload.downcast_ref::<String>()) {
        (Some(s), _) => s.to_string(),
        (_, Some(s)) => s.clone(),
        _ => "a panic".into(),
    }
}

#[cfg(test)]
mod tests;
