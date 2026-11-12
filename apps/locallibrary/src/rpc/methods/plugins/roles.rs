//! `plugins.roles*`: which analyzer each field role, and each part of the analysis, is read from.

use locallibrary::fields::roles::{self, RoleOrPart};
use schemars::JsonSchema;
use serde::Deserialize;

use crate::rpc::methods::Registry;

#[derive(Deserialize, JsonSchema)]
struct ChooseRole {
    /// The role, or the part of the analysis (`grid`, `phrases`, `waveform`).
    role: RoleOrPart,
    /// The analyzer to read it from, `<plugin>.<analyzer>` (one with the role); `null` (or left
    /// out) puts it back to its default.
    #[serde(default)]
    analyzer: Option<String>,
    /// Read it from no analyzer instead (`recording`: `sets publish` looks up no MusicBrainz IDs).
    #[serde(default)]
    none: bool,
}

pub(super) fn register(r: &mut Registry) {
    r.read("plugins.roles", |c, _: locallibrary::api::NoParams| Ok(roles::choices(c.existing()?.as_ref()))).quick();
    r.immediate("plugins.roles.set", |c, req: ChooseRole| {
        let store = c.store()?;
        Ok(match req.none {
            true => roles::turn_off(&store, req.role)?,
            false => roles::choose(&store, req.role, req.analyzer.as_deref())?,
        })
    })
    .keeps_cache();
}
