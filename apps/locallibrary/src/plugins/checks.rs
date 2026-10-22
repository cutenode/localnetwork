//! The checks the plugins have, as `plugin checks` and the sidecar list them.

use locallibrary::plugin::{CheckSettings, Plugins, check_timeout};
use loclib_plugin::When;
use schemars::JsonSchema;
use serde::Serialize;

/// The plugins' checks, whether each runs, and how long one has to answer, as `plugin checks` and
/// the sidecar's `plugins.checks` show them.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Checks {
    /// In the order the plugins are.
    pub checks: Vec<ListedCheck>,
    /// How long a check has to answer, in seconds, before it's stopped (a warning says so): the
    /// `checks.timeout` setting.
    pub timeout: f64,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct ListedCheck {
    pub plugin: String,
    pub name: String,
    pub title: String,
    /// With the check before a set and doctor (`check`), before a plan is applied (`apply`: a
    /// guard), or both.
    pub when: Vec<When>,
    /// Whether it runs; and for a guard, whether a warning or a problem it finds keeps the plan
    /// from being applied.
    #[serde(flatten)]
    pub settings: CheckSettings,
}

impl Checks {
    pub fn of(plugins: &Plugins, store: Option<&locallibrary::Store>) -> Checks {
        let checks = plugins.checks(store).into_iter().map(|(plugin, c, settings)| {
            let (name, title, when) = (c.name.clone(), c.title.clone(), c.when.clone());
            ListedCheck { plugin: plugin.into(), name, title, when, settings }
        });
        Checks { checks: checks.collect(), timeout: check_timeout(store).as_secs_f64() }
    }
}
