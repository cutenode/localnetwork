//! Field roles: what locallibrary reads an analyzer's field as (a track's key, its tempo, its
//! loudness...), whichever plugin works it out (see [`Role`]). Each role is read from one analyzer:
//! the one chosen for it in the store's settings (`analysis.roles.<role>`), else the only one with
//! it, else the first in the order the app has its plugins.
//!
//! The app says which analyzers' fields have roles ([`set_offered`]) as its plugins load: this
//! crate never depends on a plugin, and analyses nothing for a role. Readers ([`by_role`]) read
//! what those analyzers kept in the [`FieldStore`].

use std::cell::RefCell;
use std::sync::OnceLock;

use indexmap::IndexMap;
use rustc_hash::FxHashMap;
use schemars::JsonSchema;
use serde::Serialize;
use serde_json::Value;

pub use loclib_plugin::Role;

use super::{Analyzer, FieldStore, FieldValue};
use crate::device::Device;
use crate::model::Track;
use crate::plugin::{host_settings, set_host_setting};
use crate::store::Store;
use crate::{Error, Result, paths};

/// An analyzer's field with a role.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoleField {
    pub analyzer: Analyzer,
    pub field: String,
}

impl RoleField {
    /// Its analyzer, as the setting names it: `<plugin>.<analyzer>`.
    pub fn name(&self) -> String {
        format!("{}.{}", self.analyzer.plugin, self.analyzer.analyzer)
    }
}

/// The analyzers' fields with each role, in the order the app has its plugins.
pub type Offered = IndexMap<Role, Vec<RoleField>>;

/// A track's role values (a value may be null: worked out, with nothing to show).
pub type RoleValues = IndexMap<Role, FieldValue>;

static OFFERED: OnceLock<Offered> = OnceLock::new();

thread_local! {
    /// [`set_offered`] for one thread: tests sharing a process each offer their own
    /// (`testing::roles_on_this_thread`).
    pub(crate) static THREAD_OFFERED: RefCell<Option<Offered>> = const { RefCell::new(None) };
}

/// Tell the readers which analyzers' fields have roles. Set once, by the app, as its plugins
/// load; before then no analyzer has any.
pub fn set_offered(offered: Offered) {
    let _ = OFFERED.set(offered);
}

/// What [`set_offered`] said (this thread's, in tests).
fn offered() -> Offered {
    THREAD_OFFERED.with(|o| o.borrow().clone()).or_else(|| OFFERED.get().cloned()).unwrap_or_default()
}

/// The setting naming the analyzer chosen for `role`.
fn setting(role: Role) -> String {
    format!("analysis.roles.{}", role.name())
}

/// The analyzer `settings` choose for `role` among those `offered` it, if that one still is.
fn set_in<'o>(settings: &IndexMap<String, Value>, offered: &'o Offered, role: Role) -> Option<&'o RoleField> {
    let name = settings.get(&setting(role))?.as_str()?;
    offered.get(&role)?.iter().find(|f| f.name() == name)
}

/// The field `role` is read from: the one `settings` choose, else the first offered.
fn chosen_in<'o>(settings: &IndexMap<String, Value>, offered: &'o Offered, role: Role) -> Option<&'o RoleField> {
    set_in(settings, offered, role).or_else(|| offered.get(&role)?.first())
}

/// The analyzer's field `role` is read from now, with the settings in `store` (none: the default).
pub fn chosen(store: Option<&Store>, role: Role) -> Option<RoleField> {
    chosen_in(&host_settings(store), &offered(), role).cloned()
}

/// A role, and the analyzers answering it.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct RoleChoice {
    pub role: Role,
    /// The analyzer it's read from now, `<plugin>.<analyzer>` (none: no analyzer has it).
    pub chosen: Option<String>,
    /// Chosen in the settings, rather than by default.
    pub set: bool,
    /// Every analyzer with it, `<plugin>.<analyzer>`, in the order the app has its plugins.
    pub offered: Vec<String>,
}

/// Every role, and the analyzers answering it, with the settings in `store`.
pub fn choices(store: Option<&Store>) -> Vec<RoleChoice> {
    let (settings, offered) = (host_settings(store), offered());
    let choice = |role| RoleChoice {
        role,
        chosen: chosen_in(&settings, &offered, role).map(RoleField::name),
        set: set_in(&settings, &offered, role).is_some(),
        offered: offered.get(&role).into_iter().flatten().map(RoleField::name).collect(),
    };
    Role::ALL.into_iter().map(choice).collect()
}

/// Read `role` from `analyzer` (`<plugin>.<analyzer>`, one that has it), or with `None` from the
/// default again. Answers every role then. Settings that can't be read are written anew.
pub fn choose(store: &Store, role: Role, analyzer: Option<&str>) -> Result<Vec<RoleChoice>> {
    let offered = offered();
    let names: Vec<String> = offered.get(&role).into_iter().flatten().map(RoleField::name).collect();
    if let Some(a) = analyzer.filter(|a| !names.iter().any(|n| n == a)) {
        return Err(Error::invalid(match names.is_empty() {
            true => format!("no analyzer works out {}", role.name()),
            false => format!("{a} doesn't work out {}: {} does", role.name(), names.join(", ")),
        }));
    }
    set_host_setting(store, &setting(role), analyzer.map(Value::from))?;
    Ok(choices(Some(store)))
}

/// What the analyzers chosen for `roles` worked out for `tracks` (of `device`), kept in `store`:
/// by track id, then role. A track with none is left out, as is a role no analyzer has.
pub fn by_role(store: &Store, roles: &[Role], device: &Device, tracks: &[&Track]) -> FxHashMap<u32, RoleValues> {
    let (settings, offered) = (host_settings(Some(store)), offered());
    let chosen: Vec<(Role, &RoleField)> =
        roles.iter().filter_map(|&r| Some((r, chosen_in(&settings, &offered, r)?))).collect();
    let mut analyzers: Vec<Analyzer> = Vec::new();
    for (_, f) in &chosen {
        if !analyzers.contains(&f.analyzer) {
            analyzers.push(f.analyzer.clone());
        }
    }
    if analyzers.is_empty() {
        return FxHashMap::default();
    }
    let fields = FieldStore::in_store(store);
    let values = |t: &&Track| {
        let audio = device.checked_path(&paths::key(&t.file_path)).ok()?;
        let kept = fields.get(&audio, &analyzers);
        let field = |f: &RoleField| kept.get(&f.analyzer.plugin)?.get(&f.analyzer.analyzer)?.get(&f.field).cloned();
        let values: RoleValues = chosen.iter().filter_map(|(r, f)| Some((*r, field(f)?))).collect();
        (!values.is_empty()).then_some((t.id, values))
    };
    tracks.iter().filter_map(values).collect()
}

#[cfg(test)]
mod tests;
