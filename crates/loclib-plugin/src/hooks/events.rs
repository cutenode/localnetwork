//! Feature `events`: what the host tells a plugin as it happens (a drive plugged in, a plan
//! applied), and what a plugin tells the user back, a suggestion they may review.

use rustc_hash::FxHashSet;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::manifest::invalid;
use crate::{Capability, ExportFrom, Feature, Manifest, ManifestError, ProviderKind, check_params};

/// Notification, host to plugin (feature `events`): [`EventNotify`].
pub const EVENT_NOTIFY: &str = "event/notify";
/// Notification, plugin to host (feature `events` or `services`): [`HostNotify`].
pub const HOST_NOTIFY: &str = "host/notify";

pub(crate) const FEATURE: Feature = Feature {
    name: "events",
    methods: &[EVENT_NOTIFY],
    sends: &[HOST_NOTIFY],
    contributed: |m| !m.contributes.events.is_empty(),
};

/// What a host tells plugins of: a closed list, each needing a capability (see
/// [`EventName::capability`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
pub enum EventName {
    /// A drive was plugged in.
    #[serde(rename = "drive.mounted")]
    DriveMounted,
    /// A drive was unplugged.
    #[serde(rename = "drive.unmounted")]
    DriveUnmounted,
    /// A drive's databases or its locallibrary state changed (`payload.changed` says which).
    #[serde(rename = "drive.changed")]
    DriveChanged,
    /// A drive was saved into the store as a new snapshot (`payload.history_lists`: the
    /// history lists the store lacked).
    #[serde(rename = "drive.pulled")]
    DrivePulled,
    /// A drive back from a set was saved beside its library's newest snapshot.
    #[serde(rename = "set.returned")]
    SetReturned,
    /// A request to the sidecar finished (`payload.method`, `payload.ok`).
    #[serde(rename = "job.finished")]
    JobFinished,
    /// A plan was applied (`payload.operation`, `payload.summary`).
    #[serde(rename = "plan.applied")]
    PlanApplied,
}

impl EventName {
    pub const ALL: [EventName; 7] = [
        EventName::DriveMounted,
        EventName::DriveUnmounted,
        EventName::DriveChanged,
        EventName::DrivePulled,
        EventName::SetReturned,
        EventName::JobFinished,
        EventName::PlanApplied,
    ];

    /// As a manifest names it (`drive.mounted`).
    pub fn name(self) -> &'static str {
        match self {
            EventName::DriveMounted => "drive.mounted",
            EventName::DriveUnmounted => "drive.unmounted",
            EventName::DriveChanged => "drive.changed",
            EventName::DrivePulled => "drive.pulled",
            EventName::SetReturned => "set.returned",
            EventName::JobFinished => "job.finished",
            EventName::PlanApplied => "plan.applied",
        }
    }

    /// What a plugin must be granted to be told of it.
    pub fn capability(self) -> Capability {
        match self {
            EventName::DriveMounted
            | EventName::DriveUnmounted
            | EventName::DriveChanged
            | EventName::DrivePulled
            | EventName::SetReturned => Capability::EventsDrives,
            EventName::JobFinished => Capability::EventsJobs,
            EventName::PlanApplied => Capability::EventsPlans,
        }
    }
}

/// `event/notify` (feature `events`): something happened that the plugin listens for. A
/// notification: nothing answers it, and a host drops the oldest it hasn't sent yet once too
/// many wait.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct EventNotify {
    pub event: EventName,
    /// The drive it's about, by id or name (never a path).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub drive: Option<String>,
    /// What the host says of it, as the sidecar's `event` notification does, without where
    /// anything is on the computer: paths are said from their drive, `<store>` or `~`.
    #[serde(default)]
    pub payload: Value,
}

/// `host/notify` (feature `events` or `services`): a line for the user, and something they may
/// review, which the host plans (and they confirm) only if they choose to: one of the plugin's
/// operations, or one of the host's actions ([`HostAction`]), never both. An older host, which doesn't
/// know `action`, leaves it out and tells the line alone.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct HostNotify {
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operation: Option<Suggested>,
    /// A [`HostAction`], as `{name, params}`: kept as it's sent until the host checks it, so one it
    /// refuses still tells the line.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action: Option<Suggested>,
}

/// The longest params a suggestion may have, as JSON: the host keeps them to plan it.
pub const MAX_SUGGESTED_PARAMS: usize = 64 << 10;

impl HostNotify {
    /// A line for the user suggesting one of the host's actions.
    pub fn action(message: impl Into<String>, action: &HostAction) -> HostNotify {
        let action = serde_json::to_value(action).and_then(serde_json::from_value);
        let action = action.expect("an action serializes as {name, params}");
        HostNotify { message: message.into(), operation: None, action: Some(action) }
    }

    /// What it suggests, if the plugin whose manifest is `m` may suggest it: one of its own
    /// operations, or an action on its own contributions (see [`HostAction`]), its params an object of
    /// at most [`MAX_SUGGESTED_PARAMS`] as JSON, and for an action ones it takes. Refused, why
    /// (`it suggests ...`).
    pub fn suggestion(&self, m: &Manifest) -> Result<Option<Suggestion>, String> {
        let s = match (&self.operation, &self.action) {
            (None, None) => return Ok(None),
            (Some(_), Some(_)) => return Err("it suggests both an operation and an action".into()),
            (Some(s), None) | (None, Some(s)) => s,
        };
        let what = match self.action {
            Some(_) => format!("the action {:?}", s.name),
            None => format!("{:?}", s.name),
        };
        if !(s.params.is_null() || s.params.is_object()) {
            return Err(format!("it suggests {what} with params that aren't an object"));
        }
        if s.params.to_string().len() > MAX_SUGGESTED_PARAMS {
            return Err(format!("it suggests {what} with params longer than {MAX_SUGGESTED_PARAMS} bytes"));
        }
        if self.action.is_none() {
            return match m.contributes.operations.iter().any(|o| o.name == s.name) {
                true => Ok(Some(Suggestion::Operation(s.clone()))),
                false => Err(format!("it suggests {what}, which isn't one of its operations")),
            };
        }
        let action: HostAction = serde_json::from_value(json!({ "name": s.name, "params": s.params }))
            .map_err(|e| format!("it suggests {what}, which it can't: {e}"))?;
        action.check(m).map_err(|why| format!("it suggests {what}, which it can't: {why}"))?;
        Ok(Some(Suggestion::Action(action)))
    }
}

/// What a plugin suggests (`host/notify`): an operation's or an action's name, and its params.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Suggested {
    pub name: String,
    #[serde(default)]
    pub params: Value,
}

/// What a plugin suggests the user review, checked ([`HostNotify::suggestion`]): one of its
/// operations, or one of the host's actions.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Suggestion {
    Operation(Suggested),
    Action(HostAction),
}

/// One of the host's own flows a plugin may suggest (`host/notify`'s `action`): the host starts it
/// with these params filled in, and it's planned and confirmed as when the user starts it. Each is
/// on the plugin's own contributions alone, so it never starts another plugin's work.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "name", content = "params")]
pub enum HostAction {
    /// Fetch cover art from its artwork providers (`locallibrary art fetch`).
    #[serde(rename = "art.fetch")]
    ArtFetch(FetchAction),
    /// Fetch tags from its metadata providers (`locallibrary track fetch`).
    #[serde(rename = "track.fetch")]
    TrackFetch(FetchAction),
    /// Export in one of its formats, to where the user picks.
    #[serde(rename = "export")]
    Export(ExportAction),
    /// Run one of its analyzers on a drive's tracks (`locallibrary analyze`).
    #[serde(rename = "analyze")]
    Analyze(AnalyzeAction),
}

/// `art.fetch`'s and `track.fetch`'s params.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FetchAction {
    /// The drive, by id or name (as events name it).
    pub drive: String,
    /// Only these tracks, by id [default: every one].
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tracks: Vec<u32>,
    /// Only this provider of the plugin's [default: each of its providers of the kind].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
}

/// `export`'s params: what its exporter writes out (a drive's library, a set or a mix, as
/// `export/write`'s source names them) and its options. Where to is the user's to pick.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ExportAction {
    /// One of the plugin's exporters, by format.
    pub format: String,
    /// The drive, by id or name: a drive exporter's library, else whose tracks to give it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub drive: Option<String>,
    /// A set exporter's set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub set: Option<String>,
    /// A mix exporter's mix.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mix: Option<String>,
    /// As the exporter's `options` schema says.
    #[serde(default, skip_serializing_if = "Map::is_empty")]
    pub options: Map<String, Value>,
}

/// `analyze`'s params.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AnalyzeAction {
    /// One of the plugin's analyzers.
    pub analyzer: String,
    /// The drive, by id or name.
    pub drive: String,
    /// Only these tracks, by id [default: every one].
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tracks: Vec<u32>,
}

impl HostAction {
    /// A fetch's kind of provider, and its params.
    pub fn fetch(&self) -> Option<(ProviderKind, &FetchAction)> {
        match self {
            HostAction::ArtFetch(f) => Some((ProviderKind::Artwork, f)),
            HostAction::TrackFetch(f) => Some((ProviderKind::Metadata, f)),
            HostAction::Export(_) | HostAction::Analyze(_) => None,
        }
    }

    /// The drive it's on, as the plugin names it, when it names one.
    pub fn drive(&self) -> Option<&str> {
        match self {
            HostAction::ArtFetch(f) | HostAction::TrackFetch(f) => Some(&f.drive),
            HostAction::Export(e) => e.drive.as_deref(),
            HostAction::Analyze(a) => Some(&a.drive),
        }
    }

    /// Its drive and the tracks it's for (by id; none for every one), when it has them: for a host
    /// to name them as it knows them.
    pub fn scope_mut(&mut self) -> (Option<&mut String>, Option<&mut Vec<u32>>) {
        match self {
            HostAction::ArtFetch(f) | HostAction::TrackFetch(f) => (Some(&mut f.drive), Some(&mut f.tracks)),
            HostAction::Export(e) => (e.drive.as_mut(), None),
            HostAction::Analyze(a) => (Some(&mut a.drive), Some(&mut a.tracks)),
        }
    }

    /// Whether it's on `m`'s own contributions, with what they take, and on a drive named by id
    /// or name (never a path, which a host would take for a folder): why not, if it isn't.
    fn check(&self, m: &Manifest) -> Result<(), String> {
        let c = &m.contributes;
        let path = |d: &str| d.is_empty() || d.contains(['/', '\\']) || d.starts_with(['.', '~']);
        if let Some(drive) = self.drive().filter(|d| path(d)) {
            return Err(format!("{drive:?} isn't a drive's id or name"));
        }
        if let Some((kind, f)) = self.fetch() {
            let mut ours = c.providers.iter().filter(|p| p.kind == kind);
            return match &f.provider {
                Some(name) if !ours.any(|p| p.name == *name) => Err(format!("no {} provider {name:?}", kind.name())),
                None if ours.next().is_none() => Err(format!("no {} provider", kind.name())),
                _ => Ok(()),
            };
        }
        match self {
            HostAction::Export(e) => {
                let Some(exporter) = c.exporters.iter().find(|x| x.format == e.format) else {
                    return Err(format!("no exporter {:?}", e.format));
                };
                let (takes, fits) = match exporter.source {
                    ExportFrom::Drive => ("a drive", e.drive.is_some() && e.set.is_none() && e.mix.is_none()),
                    ExportFrom::Set => ("a set (and maybe a drive)", e.set.is_some() && e.mix.is_none()),
                    ExportFrom::Mix => ("a mix (and maybe a drive)", e.mix.is_some() && e.set.is_none()),
                };
                if !fits {
                    return Err(format!("its exporter {:?} takes {takes}", e.format));
                }
                let options = Value::Object(e.options.clone());
                check_params(exporter.options.as_ref(), &options).map_err(|x| format!("options: {}", x.message))
            }
            HostAction::Analyze(a) if !c.analyzers.iter().any(|x| x.name == a.analyzer) => {
                Err(format!("no analyzer {:?}", a.analyzer))
            }
            _ => Ok(()),
        }
    }
}

/// `[[contributes.events]]`: what the plugin is told of (`event/notify`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Listener {
    pub on: Vec<EventName>,
}

/// `[[contributes.events]]`: each names an event, none twice, each granted.
pub(crate) fn validate(m: &Manifest) -> Result<(), ManifestError> {
    let at = "contributes.events";
    let mut seen = FxHashSet::default();
    for (i, x) in m.contributes.events.iter().enumerate() {
        if x.on.is_empty() {
            return Err(invalid(format!("{at}[{i}].on"), "names no event"));
        }
        for (j, event) in x.on.iter().enumerate() {
            let at = format!("{at}[{i}].on[{j}]");
            if !seen.insert(*event) {
                return Err(invalid(at, format!("{} is listened for twice", event.name())));
            }
            let needs = event.capability();
            if !m.capabilities.contains(&needs) {
                return Err(invalid(at, format!("{} needs the {needs} capability", event.name())));
            }
        }
    }
    Ok(())
}
