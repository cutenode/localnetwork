//! Feature `checks`: what a plugin finds on a drive, which the host adds to its check before a set
//! and doctor, and what it says about a plan before it's applied (a guard).

use loclib_model::finding::Finding;
use rustc_hash::FxHashSet;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::manifest::{invalid, shown, unique_names};
use crate::sdk::declared;
use crate::{Context, Contributions, Feature, Hook, Manifest, ManifestError, RpcError};

/// Host to plugin (feature `checks`): [`RunCheck`] -> [`Findings`].
pub const RUN_CHECK: &str = "check/run";
/// Host to plugin (feature `checks`): [`PlanCheck`] -> [`Findings`].
pub const PLAN_CHECK: &str = "plan/check";

pub(crate) const FEATURE: Feature = Feature {
    name: "checks",
    methods: &[RUN_CHECK, PLAN_CHECK],
    sends: &[],
    contributed: |m| !m.contributes.checks.is_empty(),
};

/// `check/run` (feature `checks`): what the check finds on the drive `context.drive`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RunCheck {
    /// One of the manifest's checks run `when = "check"`.
    pub check: String,
    #[serde(default)]
    pub context: Context,
}

/// `plan/check` (feature `checks`): what the check says about a plan before it's applied, on the
/// drive `context.drive`. Its findings are shown with the plan's warnings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PlanCheck {
    /// One of the manifest's checks run `when = "apply"`.
    pub check: String,
    /// The plan's (`edit`, a plugin's `<plugin>.<operation>`).
    pub operation: String,
    /// Each an `Edit` as the sidecar's `edit.plan` takes them, tracks named by file, as the drive
    /// records it.
    pub edits: Vec<Value>,
    #[serde(default)]
    pub context: Context,
}

/// What `check/run` and `plan/check` answer: what the check found (nothing, when all is well).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Findings {
    pub findings: Vec<CheckFinding>,
}

/// A finding (its `check` is the host's to name: `<plugin>.<check>`), and what fixes it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CheckFinding {
    #[serde(flatten)]
    pub finding: Finding,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fix: Option<Fix>,
}

/// One of the plugin's operations, with its params: run to fix a finding, it's planned and
/// confirmed as any.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Fix {
    pub operation: String,
    #[serde(default)]
    pub params: Value,
}

impl Hook for RunCheck {
    const METHOD: &'static str = RUN_CHECK;
    type Answer = Findings;

    fn declared(&self, c: &Contributions) -> Result<(), RpcError> {
        declared("check", &self.check, c.checks.iter().filter(|x| x.when.contains(&When::Check)).map(|x| &x.name))
    }
}

/// For a guard: a check run `when = "apply"`.
impl Hook for PlanCheck {
    const METHOD: &'static str = PLAN_CHECK;
    type Answer = Findings;

    fn declared(&self, c: &Contributions) -> Result<(), RpcError> {
        declared("guard", &self.check, c.checks.iter().filter(|x| x.when.contains(&When::Apply)).map(|x| &x.name))
    }
}

/// Run with `check/run` on a drive along with locallibrary's own checks, and with `plan/check`
/// before a plan is applied: as its `when` says.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Check {
    pub name: String,
    pub title: String,
    /// It runs unless the user turns it off; otherwise, once they turn it on.
    #[serde(default = "on")]
    pub default_on: bool,
    /// When it runs: with the check before a set and doctor (`check`, the default), and before a
    /// plan is applied (`apply`: a guard).
    #[serde(default = "with_the_check")]
    pub when: Vec<When>,
}

fn on() -> bool {
    true
}

fn with_the_check() -> Vec<When> {
    vec![When::Check]
}

/// When a check runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum When {
    /// With locallibrary's check before a set and doctor (`check/run`).
    Check,
    /// Before a plan is applied (`plan/check`): a guard.
    Apply,
}

impl When {
    /// As a manifest names it (`apply`).
    pub fn name(self) -> &'static str {
        match self {
            When::Check => "check",
            When::Apply => "apply",
        }
    }
}

/// `[[contributes.checks]]`: names, titles, and when each runs.
pub(crate) fn validate(m: &Manifest) -> Result<(), ManifestError> {
    let (at, c) = ("contributes.checks", &m.contributes);
    unique_names(at, "name", c.checks.iter().map(|x| x.name.as_str()).enumerate())?;
    for (i, x) in c.checks.iter().enumerate() {
        shown(format!("{at}[{i}].title"), &x.title)?;
        if x.when.is_empty() {
            return Err(invalid(format!("{at}[{i}].when"), "names no time: check, apply or both"));
        }
        let mut seen = FxHashSet::default();
        if let Some(w) = x.when.iter().find(|w| !seen.insert(**w)) {
            return Err(invalid(format!("{at}[{i}].when"), format!("{} is there twice", w.name())));
        }
    }
    Ok(())
}
