//! `plugins.roles*`: which analyzer each field role is read from.

use locallibrary::fields::{Role, roles};
use schemars::JsonSchema;
use serde::Deserialize;

use crate::rpc::methods::Registry;

#[derive(Deserialize, JsonSchema)]
struct ChooseRole {
    role: Role,
    /// The analyzer to read it from, `<plugin>.<analyzer>` (one with the role); `null` (or left
    /// out) puts it back to its default.
    #[serde(default)]
    analyzer: Option<String>,
}

pub(super) fn register(r: &mut Registry) {
    r.read("plugins.roles", |c, _: locallibrary::api::NoParams| Ok(roles::choices(c.existing()?.as_ref()))).quick();
    r.immediate("plugins.roles.set", |c, req: ChooseRole| {
        Ok(roles::choose(&c.store()?, req.role, req.analyzer.as_deref())?)
    })
    .keeps_cache();
}
