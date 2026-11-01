//! The check before a set over one or more drives, and the player models it checks against.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::resolve_drive;
use crate::check::{CheckOptions, CheckReport};
use crate::device::Device;
use crate::equipment::{self, Model};
use crate::error::{Action, Reason};
use crate::op::{Operation, Planned, Probe, Touched};
use crate::progress::Ctx;
use crate::store::Store;
use crate::{Error, Result};

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct CheckRequest {
    /// The drives going to the set (a library's name: its mounted clones, but archive and retired
    /// ones); the first is the reference the others must match.
    pub drives: Vec<String>,
    #[serde(default, flatten)]
    pub options: CheckOptions,
}

pub fn check(store: Option<&Store>, req: &CheckRequest, ctx: &Ctx<'_>) -> Result<CheckReport> {
    if req.drives.is_empty() {
        return Err(Error::invalid("give at least one drive"));
    }
    let mut devices: Vec<Device> = Vec::new();
    for spec in &super::expand_libraries(&req.drives)?.0 {
        let d = resolve_drive(spec)?;
        if devices.iter().any(|x| x.root() == d.root()) {
            return Err(Error::invalid(format!("{spec:?} is listed twice")));
        }
        devices.push(d);
    }
    crate::check::check(&devices, store, &req.options, ctx)
}

/// Every model locallibrary knows, with what it reads (see [`crate::equipment`]).
pub fn player_models() -> &'static [Model] {
    equipment::MODELS
}

/// The player models `check` uses when none are given.
#[derive(Debug, Clone, Default, Serialize, JsonSchema)]
pub struct RememberedPlayers {
    /// For drives of a library without models of its own.
    pub default: Vec<String>,
    pub libraries: Vec<LibraryPlayers>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct LibraryPlayers {
    pub id: Uuid,
    /// `None` when the library is no longer in the store.
    pub name: Option<String>,
    pub models: Vec<String>,
}

fn remembered(store: &Store) -> Result<RememberedPlayers> {
    let r = store.player_models()?;
    let mut libraries = Vec::new();
    for (id, models) in r.libraries {
        let name = store.library_by_id(id)?.map(|l| l.name);
        libraries.push(LibraryPlayers { id, name, models });
    }
    Ok(RememberedPlayers { default: r.default, libraries })
}

pub fn players_show(store: &Store) -> Result<RememberedPlayers> {
    remembered(store)
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct SetPlayers {
    /// Model names (`CDJ-3000X`, `cdj-2000nxs2`...); stored as the table spells them.
    pub models: Vec<String>,
    /// The library (name or id) they're for; without one, every library without models of its own.
    #[serde(default)]
    pub library: Option<String>,
}

fn library_id(store: &Store, library: &str) -> Result<Uuid> {
    match store.find_library(library)? {
        Some(l) => Ok(l.id),
        None => Err(unknown_library(library)),
    }
}

fn unknown_library(library: &str) -> Error {
    Error::NotFound(format!("library {library:?}"))
        .because(Reason::UnknownLibrary)
        .with("library", library)
        .action(Action::call("library.list", serde_json::json!({}), "List the libraries"))
}

/// What remembering (or forgetting) player models would change.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct PlayersPlan {
    /// The library they're for; `None` for the default (every library without models of its
    /// own).
    pub library: Option<Uuid>,
    pub library_name: Option<String>,
    /// The models remembered now.
    pub from: Vec<String>,
    /// The models remembered after (empty: forgotten).
    pub to: Vec<String>,
}

/// Which models to remember for which library (`None`: the default); no models forgets them.
#[derive(Debug, Clone)]
pub struct PlayersChange {
    library: Option<Uuid>,
    models: Vec<String>,
}

/// The plan, against the models `store` remembers (with no store, none: it's made on apply).
fn players_plan(
    store: Option<&Store>,
    operation: &'static str,
    library: Option<&str>,
    models: Vec<String>,
) -> Result<Planned<PlayersPlan, PlayersChange>> {
    let (library, library_name, from) = match (library, store) {
        (Some(library), None) => return Err(unknown_library(library)),
        (Some(library), Some(store)) => {
            let r = store.player_models()?;
            let id = library_id(store, library)?;
            let name = store.library_by_id(id)?.map(|l| l.name);
            (Some(id), name, r.libraries.get(&id).cloned().unwrap_or_default())
        }
        (None, store) => (None, None, store.map(Store::player_models).transpose()?.unwrap_or_default().default),
    };
    let whose = match (library, &library_name) {
        (Some(_), Some(name)) => format!("drives of {name}"),
        (Some(id), None) => format!("drives of library {id}"),
        (None, _) => "every library without models of its own".to_string(),
    };
    let line = match (models.is_empty(), from.is_empty()) {
        (false, true) => format!("remember {} for {whose}", models.join(", ")),
        (false, false) => format!("remember {} for {whose} (instead of {})", models.join(", "), from.join(", ")),
        (true, _) => format!("forget the models remembered for {whose}: {}", from.join(", ")),
    };
    let noop = from == models;
    let change = PlayersChange { library, models: models.clone() };
    let plan = PlayersPlan { library, library_name, from, to: models };
    let planned = Planned::new(operation, plan, change).line(line).noop(noop);
    Ok(match store {
        Some(store) => {
            planned.touches(Touched::path(store.root(), true, vec![Probe::Content(vec!["players.json".into()])]))
        }
        None => planned,
    })
}

/// What remembering these models would change (`players set`).
pub fn players_set_plan(store: Option<&Store>, req: &SetPlayers) -> Result<Planned<PlayersPlan, PlayersChange>> {
    let models = equipment::canonical(&req.models)?;
    if models.is_empty() {
        return Err(Error::invalid("give at least one model (`players clear` forgets them)"));
    }
    players_plan(store, "players.set", req.library.as_deref(), models)
}

impl Operation for PlayersPlan {
    type Request = PlayersChange;
    type Report = RememberedPlayers;

    fn apply(planned: &Planned<Self, PlayersChange>, store: Option<&Store>, _: &Ctx<'_>) -> Result<RememberedPlayers> {
        let store = store.ok_or_else(|| Error::invalid("remembering player models needs the store"))?;
        let change = planned.request();
        let _lock = store.lock()?;
        let mut r = store.player_models()?;
        match (change.library, change.models.is_empty()) {
            (Some(id), false) => {
                r.libraries.insert(id, change.models.clone());
            }
            (Some(id), true) => {
                r.libraries.shift_remove(&id);
            }
            (None, _) => r.default = change.models.clone(),
        }
        store.save_player_models(&r)?;
        remembered(store)
    }
}

/// Remember the models (planned and applied in one go; see [`players_set_plan`]).
pub fn players_set(store: &Store, req: &SetPlayers) -> Result<RememberedPlayers> {
    players_set_plan(Some(store), req)?.apply(Some(store), &Ctx::silent())
}

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct ClearPlayers {
    /// The library (name or id) whose models to forget; without one, the default.
    #[serde(default)]
    pub library: Option<String>,
}

/// What forgetting the remembered models would change (`players clear`).
pub fn players_clear_plan(store: Option<&Store>, req: &ClearPlayers) -> Result<Planned<PlayersPlan, PlayersChange>> {
    players_plan(store, "players.clear", req.library.as_deref(), Vec::new())
}

/// Forget the models (planned and applied in one go; see [`players_clear_plan`]).
pub fn players_clear(store: &Store, req: &ClearPlayers) -> Result<RememberedPlayers> {
    players_clear_plan(Some(store), req)?.apply(Some(store), &Ctx::silent())
}
