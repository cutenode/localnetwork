//! Feature `commands`: what a plugin adds to run, answering any JSON, or a view to draw. A command
//! with `plan = true` is asked for its plan first (`dry_run`), which the user confirms before it
//! runs.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::manifest::{object_schema, offered_in, on_a_drive, shown, unique_names};
use crate::sdk::declared;
use crate::{Choice, Context, Contributions, ExportedFile, Feature, Hook, Manifest, ManifestError, RpcError, Where};
// Named in a doc comment the contract shows as it's written.
#[cfg(doc)]
use crate::Operation;

/// Host to plugin: [`RunCommand`] -> any JSON (a [`crate::View`] for a view command), or with
/// `dry_run` a [`CommandPlanAnswer`].
pub const RUN_COMMAND: &str = "command/run";

pub(crate) const FEATURE: Feature = Feature {
    name: "commands",
    methods: &[RUN_COMMAND],
    sends: &[],
    contributed: |m| !m.contributes.commands.is_empty(),
};

/// The SDK's own feature a host has when it plans a command with `plan = true` before running it:
/// a host without it would run one unasked, so the SDK refuses to there.
pub(crate) const PLANS: &str = "plans";

/// `command/run`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RunCommand {
    /// One of the manifest's commands.
    pub command: String,
    /// What the command's `params` schema describes.
    #[serde(default)]
    pub params: Value,
    /// Answer what it would do (a [`CommandPlanAnswer`]), doing nothing: only for a command with
    /// `plan = true`, which the host then runs with the same request but this, once the user
    /// confirms.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub dry_run: bool,
    #[serde(default)]
    pub context: Context,
}

/// Any JSON for a headless command, a [`crate::View`] for a view command (or a question first,
/// [`crate::View::Choose`]); for a dry run, a [`CommandPlanAnswer`].
impl Hook for RunCommand {
    const METHOD: &'static str = RUN_COMMAND;
    type Answer = Value;

    fn declared(&self, c: &Contributions) -> Result<(), RpcError> {
        declared("command", &self.command, c.commands.iter().map(|x| &x.name))
    }

    /// A dry run only of a command planned first, and that command run only by a host that plans
    /// it first: an older one would run it without asking the user.
    fn allowed(&self, c: &Contributions, host_has: &dyn Fn(&str) -> bool) -> Result<(), RpcError> {
        let planned = c.commands.iter().any(|x| x.name == self.command && x.plan);
        if self.dry_run && !planned {
            return Err(RpcError::invalid_params(format!("{} isn't planned first (plan = true)", self.command)));
        }
        if planned && !host_has(PLANS) {
            let message = format!("{} is planned and confirmed first, which this host doesn't do", self.command);
            return Err(RpcError::failed("incompatible", message));
        }
        Ok(())
    }
}

/// What a command with `plan = true` answers a dry run: its plan, or first a question for the
/// user (`{"choose": ...}`, feature `choose`), after which the host asks for the plan again with
/// the param set (see [`Choice`]).
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum CommandPlanAnswer {
    Choose { choose: Choice },
    Plan(CommandPlan),
}

impl From<CommandPlan> for CommandPlanAnswer {
    fn from(plan: CommandPlan) -> CommandPlanAnswer {
        CommandPlanAnswer::Plan(plan)
    }
}

impl From<Choice> for CommandPlanAnswer {
    fn from(choose: Choice) -> CommandPlanAnswer {
        CommandPlanAnswer::Choose { choose }
    }
}

/// What a hook answers: a command's hook answers any JSON.
impl From<CommandPlanAnswer> for Value {
    fn from(answer: CommandPlanAnswer) -> Value {
        serde_json::to_value(answer).expect("a plan is JSON")
    }
}

impl From<CommandPlan> for Value {
    fn from(plan: CommandPlan) -> Value {
        CommandPlanAnswer::Plan(plan).into()
    }
}

/// Read by its `choose` key rather than by trying each kind: what's wrong with a plan is said as
/// a plan's error.
impl<'de> Deserialize<'de> for CommandPlanAnswer {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<CommandPlanAnswer, D::Error> {
        crate::choice::or_plan(d, CommandPlanAnswer::from, CommandPlanAnswer::Plan)
    }
}

/// A command's plan, from its dry run: what the host shows the user before it runs the command.
/// A plugin does its own writing, so the host works out from `files` whether it's destructive
/// (it overwrites one there already) and refuses to run it once one of them changed.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CommandPlan {
    /// One line on what it would do, for the confirmation.
    pub summary: String,
    #[serde(default)]
    pub warnings: Vec<String>,
    /// The files it would write, as an exporter says them: each absolute, with what it holds and
    /// its size as far as it's known.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<ExportedFile>,
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
    /// Planned first: the host asks for a dry run, shows its plan ([`CommandPlan`]) and runs it
    /// only once the user confirms.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub plan: bool,
    /// Running it takes a while: hosts show its progress and offer to stop it, and give it longer
    /// to stop once asked before they kill the plugin.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub long_running: bool,
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
