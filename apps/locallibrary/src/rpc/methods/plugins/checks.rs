//! `plugins.checks*`: the plugins' checks and guards, whether each runs, and how long one has to
//! answer. What they find is in `check` and `doctor`, a fix planned with `plugins.op.plan`.

use locallibrary::plugin::{CheckChange, set_check_timeout};
use schemars::JsonSchema;
use serde::Deserialize;

use crate::plugins::checks::Checks;
use crate::plugins::host;
use crate::rpc::methods::Registry;

#[derive(Deserialize, JsonSchema)]
struct SetCheck {
    plugin: String,
    /// The check's name.
    check: String,
    #[serde(flatten)]
    change: CheckChange,
}

#[derive(Deserialize, JsonSchema)]
struct SetTimeout {
    /// How long a check has to answer, in seconds, before it's stopped (a warning says so);
    /// `null` (or left out) puts it back to its default.
    #[serde(default)]
    timeout: Option<f64>,
}

pub(super) fn register(r: &mut Registry) {
    r.read("plugins.checks", |c, _: locallibrary::api::NoParams| Ok(Checks::of(host()?, c.existing()?.as_ref())))
        .quick();
    r.immediate("plugins.checks.set", |c, req: SetCheck| {
        let store = c.store()?;
        host()?.set_check(&store, &req.plugin, &req.check, req.change)?;
        Ok(Checks::of(host()?, Some(&store)))
    })
    .keeps_cache();
    r.immediate("plugins.checks.timeout.set", |c, req: SetTimeout| {
        let store = c.store()?;
        set_check_timeout(&store, req.timeout)?;
        Ok(Checks::of(host()?, Some(&store)))
    })
    .keeps_cache();
}
