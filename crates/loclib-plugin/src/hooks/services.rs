//! Feature `services`: something a plugin keeps running while a host is open (watching a
//! folder, a timer), started only once the user turns it on.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::hooks::events::HOST_NOTIFY;
use crate::manifest::{shown, unique_names};
use crate::sdk::declared;
use crate::{Context, Contributions, Feature, Hook, Manifest, ManifestError, RpcError};

/// Host to plugin (feature `services`): [`StartService`] -> `null`, once it's cancelled.
pub const SERVICE_START: &str = "service/start";

pub(crate) const FEATURE: Feature = Feature {
    name: "services",
    methods: &[SERVICE_START],
    sends: &[HOST_NOTIFY],
    contributed: |m| !m.contributes.services.is_empty(),
};

/// `service/start`: run the service until the host cancels the request (`$/cancel`), then
/// answer (as cancelled). While it runs it may send `host/notify` and `progress` about it. One
/// that answers or fails sooner is started again, a while later, a few times.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct StartService {
    /// One of the manifest's services.
    pub service: String,
    #[serde(default)]
    pub context: Context,
}

impl Hook for StartService {
    const METHOD: &'static str = SERVICE_START;
    type Answer = ();

    fn declared(&self, c: &Contributions) -> Result<(), RpcError> {
        declared("service", &self.service, c.services.iter().map(|x| &x.name))
    }
}

/// Started with `service/start` by a host that stays open (the player, the sidecar, the `watch`
/// agent), once the user has turned it on (its plugin's setting `services.<name>`), never by a
/// command run once.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Service {
    pub name: String,
    pub title: String,
}

/// `[[contributes.services]]`: names and titles.
pub(crate) fn validate(m: &Manifest) -> Result<(), ManifestError> {
    let (at, c) = ("contributes.services", &m.contributes);
    unique_names(at, "name", c.services.iter().map(|x| x.name.as_str()).enumerate())?;
    for (i, x) in c.services.iter().enumerate() {
        shown(format!("{at}[{i}].title"), &x.title)?;
    }
    Ok(())
}
