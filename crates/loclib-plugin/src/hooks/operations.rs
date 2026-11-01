//! Feature `operations`: changes a plugin plans as edits, which the host confirms and applies.

use std::path::PathBuf;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::manifest::{object_schema, offered_in, on_a_drive, shown, unique_names};
use crate::sdk::declared;
use crate::{Choice, Context, Contributions, Feature, Hook, Manifest, ManifestError, RpcError, Where};

/// Host to plugin: [`PlanOp`] -> [`OpAnswer`].
pub const PLAN_OP: &str = "op/plan";

pub(crate) const FEATURE: Feature = Feature {
    name: "operations",
    methods: &[PLAN_OP],
    sends: &[],
    contributed: |m| !m.contributes.operations.is_empty(),
};

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

impl Hook for PlanOp {
    const METHOD: &'static str = PLAN_OP;
    type Answer = OpAnswer;

    fn declared(&self, c: &Contributions) -> Result<(), RpcError> {
        declared("operation", &self.operation, c.operations.iter().map(|x| &x.name))
    }
}

/// Planned with `op/plan`, applied by the host, which works out from the edits whether it's
/// destructive.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Operation {
    pub name: String,
    pub title: String,
    /// Where hosts offer it (see [`Where`]).
    #[serde(default = "on_a_drive", rename = "where")]
    pub offered: Vec<Where>,
    /// Planning takes a while: the host shows progress.
    #[serde(default)]
    pub long_running: bool,
    /// A JSON Schema for its params (an object).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
    /// Its own place on the command line, rather than `locallibrary <plugin> <name>`: a
    /// top-level command (`import`), or one in a group of the host's (`playlist import`).
    /// Built-in plugins only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cli: Option<String>,
}

/// `[[contributes.operations]]`: names, titles, places and params.
pub(crate) fn validate(m: &Manifest) -> Result<(), ManifestError> {
    let (at, c) = ("contributes.operations", &m.contributes);
    unique_names(at, "name", c.operations.iter().map(|x| x.name.as_str()).enumerate())?;
    for (i, x) in c.operations.iter().enumerate() {
        shown(format!("{at}[{i}].title"), &x.title)?;
        offered_in(format!("{at}[{i}].where"), &x.offered)?;
        object_schema(format!("{at}[{i}].params"), x.params.as_ref())?;
    }
    Ok(())
}
