//! Feature `events`: what the host tells a plugin as it happens (a drive plugged in, a plan
//! applied), and what a plugin tells the user back, a suggestion they may review.

use rustc_hash::FxHashSet;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::manifest::invalid;
use crate::{Capability, Feature, Manifest, ManifestError};

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

/// `host/notify` (feature `events` or `services`): a line for the user, and an operation of the
/// plugin's they may review, which the host plans (and they confirm) only if they choose to.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct HostNotify {
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operation: Option<Suggested>,
}

/// One of the plugin's operations, with its params, suggested to the user.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Suggested {
    pub name: String,
    #[serde(default)]
    pub params: Value,
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
