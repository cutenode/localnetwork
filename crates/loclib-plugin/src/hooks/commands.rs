//! Feature `commands`: what a plugin adds to run, answering any JSON, or a view to draw.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::manifest::{object_schema, offered_in, on_a_drive, shown, unique_names};
use crate::sdk::declared;
use crate::{Context, Contributions, Feature, Hook, Manifest, ManifestError, RpcError, Where};
// Named in a doc comment the contract shows as it's written.
#[cfg(doc)]
use crate::Operation;

/// Host to plugin: [`RunCommand`] -> any JSON (a [`crate::View`] for a view command).
pub const RUN_COMMAND: &str = "command/run";

pub(crate) const FEATURE: Feature = Feature {
    name: "commands",
    methods: &[RUN_COMMAND],
    sends: &[],
    contributed: |m| !m.contributes.commands.is_empty(),
};

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

/// Any JSON for a headless command, a [`crate::View`] for a view command (or a question first,
/// [`crate::View::Choose`]).
impl Hook for RunCommand {
    const METHOD: &'static str = RUN_COMMAND;
    type Answer = Value;

    fn declared(&self, c: &Contributions) -> Result<(), RpcError> {
        declared("command", &self.command, c.commands.iter().map(|x| &x.name))
    }
}

/// Run with `command/run`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Command {
    pub name: String,
    pub title: String,
    pub mode: Mode,
    /// Where hosts offer it (see [`Where`]).
    #[serde(default = "on_a_drive", rename = "where")]
    pub offered: Vec<Where>,
    /// A JSON Schema for its params (an object).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
    /// Its own place on the command line (see [`Operation::cli`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cli: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    /// Answers any JSON, shown as it is (`--json`) or not at all.
    Headless,
    /// Answers a [`crate::View`].
    View,
}

/// `[[contributes.commands]]`: names, titles, places and params.
pub(crate) fn validate(m: &Manifest) -> Result<(), ManifestError> {
    let (at, c) = ("contributes.commands", &m.contributes);
    unique_names(at, "name", c.commands.iter().map(|x| x.name.as_str()).enumerate())?;
    for (i, x) in c.commands.iter().enumerate() {
        shown(format!("{at}[{i}].title"), &x.title)?;
        offered_in(format!("{at}[{i}].where"), &x.offered)?;
        object_schema(format!("{at}[{i}].params"), x.params.as_ref())?;
    }
    Ok(())
}
