//! Every method the sidecar answers, in one registry: its name and old names, what kind of
//! method it is, the types it takes and returns, and the code that runs it. Dispatch,
//! `initialize`, `version` and `rpc.describe` all read this list, so they can't disagree.
//!
//! Names are `noun.verb` (`drive.info`, `tracks.query`). A change is two methods: `<op>.plan`
//! answers a plan with a `plan_id`, and `<op>.apply` applies it (`{"plan_id": ...}`), or plans
//! and applies a request in one go. Older names stay as aliases (`drive.adopt` for
//! `drive.adopt.apply`, `sync_many.plan` for `sync.many.plan`).

use rustc_hash::FxHashMap;
use std::path::Path;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use locallibrary::edit::EditPlan;
use locallibrary::op::{AnyPlanned, Operation, Planned};
use locallibrary::plugin::OpPlanned;
use locallibrary::progress::{Ctx, Event, Phase};
use locallibrary::{Store, api};
use schemars::{JsonSchema, Schema, SchemaGenerator};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

use super::{LIBRARIES, Plans, RpcError, invalid_params, ok, params};

/// What a method does, for a front end deciding how to call it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// Only reads: call it whenever.
    Read,
    /// Works out a change and answers it with a `plan_id`, changing nothing: show it, ask, then
    /// call the paired `.apply`.
    Plan,
    /// Makes a change: the plan `plan_id` names, or a request planned and applied in one go.
    Apply,
    /// Makes a change at once, with no plan step (small changes to the store, signing in).
    Immediate,
    /// About the sidecar itself: the handshake, cancelling, jobs, events, shutting down.
    Control,
}

/// Where applying a plan by id finds the store.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlanStore {
    /// It doesn't use one.
    None,
    /// The store if there is one; none is made.
    Existing,
    /// The store, made if there's none yet; without it if that fails.
    Optional,
    /// The store, made if there's none yet.
    Required,
}

type SchemaFn = fn(&mut SchemaGenerator) -> Schema;
type Run = Box<dyn Fn(&Call<'_>, Value) -> Result<Value, RpcError> + Send + Sync>;

fn schema<T: JsonSchema>(g: &mut SchemaGenerator) -> Schema {
    g.subschema_for::<T>()
}

/// What a request runs with.
pub struct Call<'a> {
    pub root: &'a Path,
    pub plans: &'a Plans,
    pub ctx: &'a Ctx<'a>,
}

impl Call<'_> {
    /// The store, made if there's none yet.
    fn store(&self) -> Result<Store, RpcError> {
        Ok(Store::open_or_init(self.root)?)
    }

    /// The store, made if there's none yet; `None` if that fails.
    fn store_opt(&self) -> Option<Store> {
        Store::open_or_init(self.root).ok()
    }

    /// The store if there is one (read-only methods never make one).
    fn existing(&self) -> Result<Option<Store>, RpcError> {
        Ok(Store::open_existing(self.root)?)
    }

    /// What `read` finds in the store, or nothing (its empty value) when there's none.
    fn or_empty<T: Default>(&self, read: impl FnOnce(&Store) -> locallibrary::Result<T>) -> Result<T, RpcError> {
        Ok(self.existing()?.as_ref().map(read).transpose()?.unwrap_or_default())
    }

    fn plan_store(&self, which: PlanStore) -> Result<Option<Store>, RpcError> {
        Ok(match which {
            PlanStore::None => None,
            PlanStore::Existing => self.existing()?,
            PlanStore::Optional => self.store_opt(),
            PlanStore::Required => Some(self.store()?),
        })
    }
}

/// One method.
pub struct Method {
    pub name: &'static str,
    pub kind: Kind,
    /// Older names that still work.
    pub aliases: Vec<&'static str>,
    /// The `.plan` of an `.apply` and the other way round.
    pub pair: Option<&'static str>,
    /// For a plan, the plans it makes; for an apply, the plans it applies (a plan's `operation`).
    pub operation: Option<&'static str>,
    /// It can take a while (it reads every file, copies, decodes audio, or waits on the network)
    /// and sends `progress`: show it, and offer `$/cancel`.
    pub long_running: bool,
    /// It can delete or overwrite something: a front end should say so when it asks.
    pub destructive: bool,
    /// Kept for older clients: use this method instead.
    pub deprecated: Option<&'static str>,
    /// A quick read, run on its own pool of workers.
    pub quick: bool,
    /// Changes no drive even though it isn't a read (the library cache is kept).
    pub keeps_cache: bool,
    /// Not listed (the tests' `$/test.wait`).
    pub hidden: bool,
    /// The parameter naming a file or folder on this computer that it writes (or a list of
    /// them): refused unless each is an absolute path (the sidecar's working folder is whatever
    /// its parent's was).
    writes_to: Option<&'static str>,
    params: SchemaFn,
    result: SchemaFn,
    /// A plan's: what applying it returns.
    applies: Option<SchemaFn>,
    /// `None`: answered by the server itself (control methods).
    run: Option<Run>,
}

impl Method {
    fn new(name: &'static str, kind: Kind, params: SchemaFn, result: SchemaFn, run: Option<Run>) -> Method {
        Method {
            name,
            kind,
            aliases: Vec::new(),
            pair: None,
            operation: None,
            long_running: false,
            destructive: false,
            deprecated: None,
            quick: false,
            keeps_cache: false,
            hidden: false,
            writes_to: None,
            params,
            result,
            applies: None,
            run,
        }
    }

    fn alias(&mut self, name: &'static str) -> &mut Self {
        self.aliases.push(name);
        self
    }

    fn long(&mut self) -> &mut Self {
        self.long_running = true;
        self
    }

    fn destructive(&mut self) -> &mut Self {
        self.destructive = true;
        self
    }

    fn quick(&mut self) -> &mut Self {
        self.quick = true;
        self
    }

    fn keeps_cache(&mut self) -> &mut Self {
        self.keeps_cache = true;
        self
    }

    fn operation(&mut self, operation: &'static str) -> &mut Self {
        self.operation = Some(operation);
        self
    }

    fn deprecated(&mut self, instead: &'static str) -> &mut Self {
        self.deprecated = Some(instead);
        self
    }

    fn writes_to(&mut self, param: &'static str) -> &mut Self {
        self.writes_to = Some(param);
        self
    }

    /// Runs the method (`None` for control methods, which the server answers).
    pub fn run(&self, call: &Call<'_>, params: Value) -> Option<Result<Value, RpcError>> {
        if let Some(param) = self.writes_to {
            let paths: Vec<&Value> = match params.get(param) {
                Some(Value::Array(list)) => list.iter().collect(),
                one => one.into_iter().collect(),
            };
            if let Some(path) = paths.into_iter().filter_map(Value::as_str).find(|p| !Path::new(p).is_absolute()) {
                return Some(Err(invalid_params(format!("`{param}` must be an absolute path, not {path:?}"))));
            }
        }
        self.run.as_ref().map(|run| run(call, params))
    }

    /// Whether the library cache stays valid after it: reads and plans change no drive.
    pub fn changes_no_drive(&self) -> bool {
        matches!(self.kind, Kind::Read | Kind::Plan | Kind::Control) || self.keeps_cache
    }

    /// Its parameters' schema. Methods that write also take [`api::WriteOptions`]; an apply
    /// takes a plan's id instead of a request.
    pub fn params_schema(&self, g: &mut SchemaGenerator) -> Schema {
        let request = (self.params)(g);
        let request = match self.kind {
            Kind::Plan | Kind::Apply | Kind::Immediate => {
                let options = g.subschema_for::<api::WriteOptions>();
                schemars::json_schema!({ "allOf": [request, options] })
            }
            Kind::Read | Kind::Control => request,
        };
        match self.kind {
            Kind::Apply => {
                let by_id = g.subschema_for::<api::ApplyPlan>();
                schemars::json_schema!({ "anyOf": [by_id, request] })
            }
            _ => request,
        }
    }

    /// Its result's schema. An apply returns what applying its plan returns, or (with a
    /// request) what its own code returns, when those differ.
    pub fn result_schema(&self, g: &mut SchemaGenerator, registry: &Registry) -> Schema {
        let own = (self.result)(g);
        if self.kind != Kind::Apply {
            return own;
        }
        let planned = self.pair.and_then(|p| registry.get(p)).and_then(|p| p.applies).map(|applies| applies(g));
        match planned {
            Some(planned) if planned != own => schemars::json_schema!({ "anyOf": [planned, own] }),
            _ => own,
        }
    }
}

/// What a `*.plan` answers: a plan, kept for its `.apply`, and what applying it returns.
trait PlanAnswer: JsonSchema + 'static {
    type Report: JsonSchema;
    /// The answer, kept as `method`'s plan when it is one.
    fn keep(self, plans: &Plans, method: &str) -> Result<Value, RpcError>;
}

impl<P: Operation> PlanAnswer for Planned<P, P::Request>
where
    P::Report: JsonSchema,
    Planned<P, P::Request>: AnyPlanned + JsonSchema + 'static,
{
    type Report = P::Report;

    fn keep(self, plans: &Plans, method: &str) -> Result<Value, RpcError> {
        plans.keep(method, self)
    }
}

/// A plugin's operation may ask a question first (`{"choose": ...}`), which keeps nothing: the
/// caller plans it again with the param set.
impl PlanAnswer for OpPlanned {
    type Report = <Planned<EditPlan> as PlanAnswer>::Report;

    fn keep(self, plans: &Plans, method: &str) -> Result<Value, RpcError> {
        match self {
            OpPlanned::Plan(plan) => plan.keep(plans, method),
            asked @ OpPlanned::Choose { .. } => ok(asked),
        }
    }
}

/// Every method, by name and alias.
pub struct Registry {
    methods: Vec<Method>,
    by_name: FxHashMap<&'static str, usize>,
}

impl Registry {
    fn add(&mut self, m: Method) -> &mut Method {
        self.methods.push(m);
        self.methods.last_mut().expect("just pushed")
    }

    /// A method that only reads.
    fn read<P, R, F>(&mut self, name: &'static str, f: F) -> &mut Method
    where
        P: DeserializeOwned + JsonSchema,
        R: Serialize + JsonSchema,
        F: Fn(&Call<'_>, P) -> Result<R, RpcError> + Send + Sync + 'static,
    {
        self.add(Method::new(
            name,
            Kind::Read,
            schema::<P>,
            schema::<R>,
            Some(Box::new(move |c, v| ok(f(c, params(v)?)?))),
        ))
    }

    /// A method that writes at once, with no plan step.
    fn immediate<P, R, F>(&mut self, name: &'static str, f: F) -> &mut Method
    where
        P: DeserializeOwned + JsonSchema,
        R: Serialize + JsonSchema,
        F: Fn(&Call<'_>, P) -> Result<R, RpcError> + Send + Sync + 'static,
    {
        let run: Run = Box::new(move |c, v| ok(f(c, params(v)?)?));
        self.add(Method::new(name, Kind::Immediate, schema::<P>, schema::<R>, Some(run)))
    }

    /// `<op>.plan`: the plan is kept for `<op>.apply {plan_id}` (see [`PlanAnswer`]).
    fn plan<P, Pl, F>(&mut self, name: &'static str, f: F) -> &mut Method
    where
        P: DeserializeOwned + JsonSchema,
        Pl: PlanAnswer,
        F: Fn(&Call<'_>, P) -> Result<Pl, RpcError> + Send + Sync + 'static,
    {
        let run: Run = Box::new(move |c, v| f(c, params(v)?)?.keep(c.plans, name));
        let m = self.add(Method::new(name, Kind::Plan, schema::<P>, schema::<Pl>, Some(run)));
        m.applies = Some(schema::<<Pl as PlanAnswer>::Report>);
        m.operation = name.strip_suffix(".plan");
        m
    }

    /// `<op>.apply`: the plan `plan_id` names (found with `store`), or the request planned and
    /// applied in one go.
    fn apply<P, R, F>(&mut self, name: &'static str, store: PlanStore, f: F) -> &mut Method
    where
        P: DeserializeOwned + JsonSchema,
        R: Serialize + JsonSchema,
        F: Fn(&Call<'_>, P) -> Result<R, RpcError> + Send + Sync + 'static,
    {
        let operation = name.strip_suffix(".apply").unwrap_or(name);
        let run: Run = Box::new(move |c, v| {
            let Some(id) = v.get("plan_id") else { return ok(f(c, params(v)?)?) };
            let id = id.as_str().ok_or_else(|| invalid_params("plan_id is a string"))?;
            let operation = registry().get(name).and_then(|m| m.operation).unwrap_or(operation);
            // The store first: a plan taken out is only put back by a refusal below.
            let store = c.plan_store(store)?;
            let (made, plan) = c.plans.take(name, id, operation)?;
            // Refused before anything was written (a changed or archive drive, or one in use), the
            // plan stays, to be applied again (with `overwrite_archive`, say).
            if let Err(e) = plan.check(store.as_ref()) {
                c.plans.put_back(name, made, plan);
                return Err(e.into());
            }
            match plan.apply_json(store.as_ref(), c.ctx) {
                Ok(report) => ok(report),
                Err(e @ locallibrary::Error::Locked(_)) if plan.check(store.as_ref()).is_ok() => {
                    c.plans.put_back(name, made, plan);
                    Err(e.into())
                }
                Err(e) => Err(RpcError::from(e).plan_used()),
            }
        });
        let m = self.add(Method::new(name, Kind::Apply, schema::<P>, schema::<R>, Some(run)));
        m.operation = Some(operation);
        m
    }

    /// A method the server answers itself.
    fn control<P: JsonSchema, R: JsonSchema>(&mut self, name: &'static str) -> &mut Method {
        self.add(Method::new(name, Kind::Control, schema::<P>, schema::<R>, None))
    }

    /// The method `name` names (or an alias of it).
    pub fn get(&self, name: &str) -> Option<&Method> {
        self.by_name.get(name).map(|&i| &self.methods[i])
    }

    /// Every method, in the order they're described.
    pub fn methods(&self) -> impl Iterator<Item = &Method> {
        self.methods.iter().filter(|m| !m.hidden)
    }

    /// Every name a client may call: methods and their aliases.
    pub fn names(&self) -> Vec<&'static str> {
        self.methods().flat_map(|m| std::iter::once(m.name).chain(m.aliases.iter().copied())).collect()
    }

    /// Indexes the names and pairs each `.plan` with its `.apply`.
    fn finish(mut self) -> Registry {
        for (i, m) in self.methods.iter().enumerate() {
            for name in std::iter::once(&m.name).chain(&m.aliases) {
                let before = self.by_name.insert(name, i);
                assert!(before.is_none(), "{name} is registered twice");
            }
        }
        let pairs: Vec<(usize, &'static str)> = self
            .methods
            .iter()
            .enumerate()
            .filter_map(|(i, m)| {
                let (stem, other) = match m.kind {
                    Kind::Plan => (m.name.strip_suffix(".plan")?, ".apply"),
                    Kind::Apply => (m.name.strip_suffix(".apply")?, ".plan"),
                    _ => return None,
                };
                let pair = format!("{stem}{other}");
                let j = *self.by_name.get(pair.as_str())?;
                Some((i, self.methods[j].name))
            })
            .collect();
        for (i, pair) in pairs {
            self.methods[i].pair = Some(pair);
        }
        self
    }
}

/// Every method the sidecar answers.
pub fn registry() -> &'static Registry {
    static REGISTRY: OnceLock<Registry> = OnceLock::new();
    REGISTRY.get_or_init(build)
}

/// `Arc`'d results (the cache's), serialised as what they hold.
struct Shared<T>(Arc<T>);

impl<T: Serialize> Serialize for Shared<T> {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        self.0.serialize(s)
    }
}

impl<T: JsonSchema> JsonSchema for Shared<T> {
    fn inline_schema() -> bool {
        T::inline_schema()
    }

    fn schema_name() -> std::borrow::Cow<'static, str> {
        T::schema_name()
    }

    fn schema_id() -> std::borrow::Cow<'static, str> {
        T::schema_id()
    }

    fn json_schema(g: &mut SchemaGenerator) -> Schema {
        T::json_schema(g)
    }
}

/// `$/test.wait`'s parameters: report `items` files, then wait until cancelled or `ms`
/// milliseconds have passed.
#[derive(serde::Deserialize, JsonSchema)]
struct Wait {
    #[serde(default)]
    items: u64,
    #[serde(default)]
    ms: u64,
}

#[derive(Serialize, JsonSchema)]
struct Waited {
    waited_ms: u64,
}

fn build() -> Registry {
    use PlanStore::{Existing, Optional, Required};
    use api::NoParams;

    let mut r = Registry { methods: Vec::new(), by_name: FxHashMap::default() };

    // The sidecar itself.
    r.control::<NoParams, api::Initialized>("initialize");
    r.read("version", |_, _: NoParams| {
        Ok(api::VersionInfo {
            version: env!("CARGO_PKG_VERSION").into(),
            methods: registry().names().into_iter().map(String::from).collect(),
        })
    })
    .quick();
    r.control::<api::DescribeRequest, Value>("rpc.describe");
    r.control::<api::CancelRequest, api::Cancelled>("$/cancel");
    r.control::<NoParams, Vec<api::JobInfo>>("jobs.list");
    r.control::<api::Subscribe, api::Subscribed>("events.subscribe");
    r.control::<NoParams, api::Unsubscribed>("events.unsubscribe");
    r.control::<api::StartWatch, api::WatchStarted>("watch.start");
    r.control::<api::StopWatch, api::WatchStopped>("watch.stop");
    r.control::<api::Shutdown, ()>("shutdown");

    // Drives and their libraries.
    r.read("status", |c, _: NoParams| Ok(api::status(c.existing()?.as_ref(), c.ctx)?)).long();
    r.read("drives.list", |c, req| Ok(api::list_drives_in_libraries(c.existing()?.as_ref(), &req))).quick();
    r.read("drives.mounted", |_, _: NoParams| Ok(api::drives_mounted())).quick();
    r.read("drive.info", |_, req| Ok(api::drive_info(&req)?)).quick();
    r.plan("drive.adopt.plan", |_, req| Ok(api::adopt_plan(&req)?));
    r.apply("drive.adopt.apply", Optional, |c, req| Ok(api::adopt_drive(c.store_opt().as_ref(), &req)?))
        .alias("drive.adopt");
    r.read("drive.library", |_, req| Ok(api::drive_library_cached(&LIBRARIES, &req)?)).quick();
    r.read("playlists.tree", |_, req| Ok(api::playlists_tree(&LIBRARIES, &req)?)).quick();
    r.read("tracks.get", |_, req| Ok(api::tracks_get(&LIBRARIES, &req)?)).quick();
    r.read("tracks.query", |c, req| {
        let fields = crate::plugins::Installed::with(c.existing()?);
        Ok(api::tracks_query_with(&LIBRARIES, &req, Some(fields.source()))?)
    })
    .quick();
    r.read("track.analysis", |_, req| Ok(api::track_analysis_cached(&LIBRARIES, &req)?)).quick();
    r.read("tracks.analysis", |_, req| Ok(api::tracks_analysis(&LIBRARIES, &req)?)).quick();
    r.read("track.fields", |c, req| {
        // Plugins that were refused have nothing to show either.
        let installed = crate::plugins::host().map(|p| p.analyzers()).unwrap_or_default();
        Ok(api::track_fields_of(&LIBRARIES, c.existing()?.as_ref(), &req, &installed)?)
    });
    r.read("track.show", |_, req| Ok(api::track_details_cached(&LIBRARIES, &req)?)).quick();
    r.read("track.matches", |_, req| Ok(api::harmonic_matches_cached(&LIBRARIES, &req)?)).quick();

    // Backups and undo.
    r.read("drive.backups", |_, req| Ok(api::list_backups(&req)?)).quick();
    r.read("drive.restore.preview", |_, req| Ok(api::restore_preview(&req)?)).deprecated("drive.restore.plan");
    r.plan("drive.restore.plan", |_, req: api::RestoreBackup| {
        Ok(api::undo_plan(&api::UndoRequest { drive: Some(req.drive), backup: req.backup, force: req.force })?)
    })
    .operation("undo")
    .destructive();
    r.apply("drive.restore.apply", PlanStore::None, |_, req| Ok(api::restore_backup(&req)?))
        .operation("undo")
        .alias("drive.restore")
        .destructive();
    r.read("undo.list", |c, req| Ok(api::undo_list(c.existing()?.as_ref(), &req)?)).quick();
    r.plan("undo.plan", |_, req| Ok(api::undo_plan(&req)?)).destructive();
    r.apply("undo.apply", PlanStore::None, |c, req| Ok(api::undo_apply(&req, c.ctx)?)).destructive();

    // Comparing and syncing.
    r.read("compare", |c, req| Ok(api::compare(&req, c.ctx)?)).long();
    r.plan("sync.plan", |c, req| Ok(api::sync_plan(&req, c.ctx)?)).long().destructive();
    r.apply("sync.apply", Optional, |c, req| Ok(api::sync_apply(c.store_opt().as_ref(), &req, c.ctx)?))
        .long()
        .destructive();
    r.plan("sync.many.plan", |c, req| Ok(api::sync_many_plan(&req, c.ctx)?))
        .alias("sync_many.plan")
        .long()
        .destructive();
    r.apply("sync.many.apply", Optional, |c, req| {
        Ok(api::sync_many_plan(&req, c.ctx)?.apply(c.store_opt().as_ref(), c.ctx)?)
    })
    .alias("sync_many.apply")
    .long()
    .destructive();
    r.plan("sync.group.plan", |c, req| Ok(api::sync_set_plan(&c.store()?, &req, c.ctx)?))
        .alias("sync_group.plan")
        .alias("sync_set.plan")
        .long()
        .destructive();
    r.apply("sync.group.apply", Required, |c, req| Ok(api::sync_set_apply(&c.store()?, &req, c.ctx)?))
        .alias("sync_group.apply")
        .alias("sync_set.apply")
        .long()
        .destructive();
    r.plan("pull.plan", |c, req| Ok(api::pull_plan(c.root, &req, c.ctx)?)).long();
    r.apply("pull.apply", Optional, |c, req| Ok(api::pull(&c.store()?, &req, c.ctx)?)).alias("pull").long();
    r.plan("push.plan", |c, req| Ok(api::push_plan(&c.store()?, &req, c.ctx)?)).long().destructive();
    r.apply("push.apply", Required, |c, req| Ok(api::push_apply(&c.store()?, &req, c.ctx)?)).long().destructive();
    r.plan("merge.plan", |c, req| Ok(api::merge_plan(&req, c.ctx)?)).long();
    r.apply("merge.apply", PlanStore::None, |c, req| Ok(api::merge_apply(&req, c.ctx)?)).long();
    r.plan("convert.plan", |c, req| Ok(api::convert_plan(&req, c.ctx)?)).long().destructive();
    r.apply("convert.apply", PlanStore::None, |c, req| Ok(api::convert_apply(&req, c.ctx)?)).long().destructive();

    // The store.
    r.read("store.info", |c, _: NoParams| Ok(api::store_info(&Store::open(c.root)?)?)).quick();
    r.read("store.history", |c, req| Ok(api::history(&Store::open(c.root)?, &req)?)).quick();
    r.plan("store.gc.plan", |c, _: NoParams| Ok(api::store_gc_plan(&c.store()?)?)).long().destructive();
    r.apply("store.gc.apply", Required, |c, req: api::GcRequest| {
        let s = c.store()?;
        let _lock = s.lock()?;
        Ok(s.gc(req.dry_run)?)
    })
    .alias("store.gc")
    .long()
    .destructive();
    r.read("store.verify", |c, _: NoParams| Ok(Store::open(c.root)?.verify(c.ctx)?)).long();
    r.plan("store.prune.plan", |c, req| Ok(api::store_prune_plan(&c.store()?, &req, c.ctx)?)).long().destructive();
    r.apply("store.prune.apply", Required, |c, req| Ok(api::store_prune(&c.store()?, &req, c.ctx)?))
        .alias("store.prune")
        .long()
        .destructive();
    r.immediate("store.copy", |c, req| Ok(api::store_copy(&c.store()?, &req, c.ctx)?))
        .long()
        .destructive()
        .writes_to("path");
    r.read("store.copies", |c, _: NoParams| c.or_empty(api::store_copies)).quick();
    r.immediate("snapshot.label", |c, req| Ok(api::snapshot_label(&c.store()?, &req)?));
    r.read("library.list", |c, _: NoParams| c.or_empty(api::libraries_list)).quick();
    r.immediate("library.rename", |c, req| Ok(api::library_rename(&c.store()?, &req)?));

    // Editing a drive's library.
    r.plan("edit.plan", |c, req| Ok(api::edit_plan(&req, c.ctx)?)).destructive();
    r.apply("edit.apply", PlanStore::None, |c, req| Ok(api::edit_apply(&req, c.ctx)?)).destructive();
    r.read("edits.list", |_, req| Ok(api::edits_list(&req)?)).quick();
    r.plan("edits.replay.plan", |c, req| Ok(api::edits_replay_plan(&req, c.ctx)?));
    r.apply("edits.replay.apply", PlanStore::None, |c, req| Ok(api::edits_replay_apply(&req, c.ctx)?));
    r.plan("edits.clear.plan", |_, req| Ok(api::edits_clear_plan(&req)?)).destructive();
    r.apply("edits.clear.apply", PlanStore::None, |_, req| Ok(api::edits_clear(&req)?))
        .alias("edits.clear")
        .destructive();
    r.read("smart.list", |c, _: NoParams| c.or_empty(api::smart_list)).quick();
    r.immediate("smart.set", |c, req| Ok(api::smart_set(&c.store()?, &req)?));
    r.immediate("smart.remove", |c, req| Ok(api::smart_remove(&c.store()?, &req)?)).destructive();
    r.plan("smart.write.plan", |c, req| Ok(api::smart_apply_plan(&c.store()?, &req, c.ctx)?))
        .alias("smart.apply.plan")
        .destructive();
    r.apply("smart.write.apply", Required, |c, req| Ok(api::smart_apply(&c.store()?, &req, c.ctx)?))
        .alias("smart.apply")
        .destructive();
    r.plan("dedupe.plan", |c, req| Ok(api::dedupe_plan(&req, c.ctx)?)).destructive();
    r.apply("dedupe.apply", PlanStore::None, |c, req| Ok(api::dedupe_apply(&req, c.ctx)?)).destructive();
    r.plan("relocate.plan", |c, req| Ok(api::relocate_plan(&req, c.ctx)?)).long().destructive();
    r.apply("relocate.apply", PlanStore::None, |c, req| Ok(api::relocate_apply(&req, c.ctx)?)).long().destructive();
    r.plan("upgrade.plan", |c, req| Ok(api::upgrade_plan(&req, c.ctx)?)).long().destructive();
    r.apply("upgrade.apply", PlanStore::None, |c, req| Ok(api::upgrade_apply(&req, c.ctx)?)).long().destructive();
    r.plan("history.delete.plan", |c, req| Ok(api::delete_history_plan(c.existing()?.as_ref(), &req, c.ctx)?))
        .destructive();
    r.apply("history.delete.apply", Existing, |c, req| {
        Ok(api::delete_history_apply(c.existing()?.as_ref(), &req, c.ctx)?)
    })
    .destructive();
    r.plan("pack.plan", |c, req| Ok(api::pack_plan(&req, c.ctx)?)).long().destructive();
    r.apply("pack.apply", Optional, |c, req| Ok(api::pack_apply(c.store_opt().as_ref(), &req, c.ctx)?))
        .long()
        .destructive();
    r.read("tags.list", |_, req| Ok(Shared(api::tags_list_cached(&LIBRARIES, &req)?))).quick();
    r.plan("tags.plan", |_, req| Ok(api::tags_plan(&req)?)).destructive();
    r.apply("tags.apply", PlanStore::None, |c, req| Ok(api::tags_apply(&req, c.ctx)?)).destructive();
    r.read("cues.list", |_, req| Ok(api::cues_list_cached(&LIBRARIES, &req)?)).quick();
    r.plan("cues.plan", |c, req| Ok(api::cues_plan(&req, c.ctx)?)).destructive();
    r.apply("cues.apply", PlanStore::None, |c, req| Ok(api::cues_apply(&req, c.ctx)?)).destructive();
    r.plan("playlist.restore.plan", |c, req| Ok(api::playlist_restore_plan(c.existing()?.as_ref(), &req, c.ctx)?))
        .destructive();
    r.apply("playlist.restore.apply", Existing, |c, req| {
        Ok(api::playlist_restore_apply(c.existing()?.as_ref(), &req, c.ctx)?)
    })
    .alias("playlist.restore")
    .destructive();

    // My Settings.
    r.read("settings.schema", |_, _: NoParams| Ok(locallibrary::settings::SCHEMA)).quick();
    r.read("settings.profile", |c, _: NoParams| c.or_empty(Store::settings_profile)).quick();
    r.immediate("settings.set", |c, req| Ok(api::settings_set(&c.store()?, &req)?));
    r.immediate("settings.unset", |c, req| Ok(api::settings_unset(&c.store()?, &req)?));
    r.read("settings.show", |c, req| Ok(api::settings_show(c.existing()?.as_ref(), &req)?)).quick();
    r.plan("settings.write.plan", |c, req| Ok(api::settings_apply_plan(&c.store()?, &req)?))
        .alias("settings.apply.plan")
        .destructive();
    r.apply("settings.write.apply", Required, |c, req| Ok(api::settings_apply(&c.store()?, &req)?))
        .alias("settings.apply")
        .destructive();
    r.plan("settings.import.plan", |c, req| Ok(api::settings_import_plan(&c.store()?, &req)?)).destructive();
    r.apply("settings.import.apply", Required, |c, req| Ok(api::settings_import(&c.store()?, &req)?))
        .alias("settings.import")
        .destructive();

    // Cover art.
    r.read("art.missing", |_, req| Ok(api::artwork_missing(&req)?)).quick();
    r.read("art.get", |_, req| Ok(api::artwork_get_cached(&LIBRARIES, &req)?)).quick();
    r.read("art.path", |_, req| Ok(api::artwork_path(&LIBRARIES, &req)?)).quick();
    r.plan("art.set.plan", |c, req| Ok(api::artwork_set_plan(&req, c.ctx)?)).destructive();
    r.apply("art.set.apply", PlanStore::None, |c, req| Ok(api::artwork_set(&req, c.ctx)?))
        .alias("art.set")
        .destructive();
    r.plan("art.auto.plan", |_, req| Ok(api::artwork_auto_plan(&req)?));
    r.apply("art.auto.apply", PlanStore::None, |c, req| Ok(api::artwork_auto_apply(&req, c.ctx)?)).long();
    r.plan("art.embed.plan", |_, req| Ok(api::artwork_embed_plan(&req)?)).destructive().writes_to("files");
    r.apply("art.embed.apply", Required, |c, req| Ok(api::artwork_embed(&c.store()?, &req)?))
        .alias("art.embed")
        .destructive()
        .writes_to("files");
    r.plan("art.unembed.plan", |c, req| Ok(api::artwork_unembed_plan(&c.store()?, &req)?)).destructive();
    r.apply("art.unembed.apply", Required, |c, req| {
        let s = c.store()?;
        Ok(api::artwork_unembed_plan(&s, &req)?.apply(Some(&s), c.ctx)?)
    })
    .alias("art.unembed")
    .destructive();

    // Exports, health and history.
    r.immediate("export", |c, req| Ok(api::export_playlist(c.existing()?.as_ref(), &req, c.ctx)?))
        .long()
        .keeps_cache()
        .writes_to("output");
    r.read("diff", |c, req| Ok(api::diff(c.existing()?.as_ref(), &req)?));
    r.read("doctor", |c, req: api::DoctorRequest| {
        let store = req.options.wants(locallibrary::doctor::Section::Analysis).then(|| c.store()).transpose()?;
        Ok(crate::plugins::roles::doctor(&req, store.as_ref(), c.ctx)?)
    })
    .long();
    r.plan("doctor.orphans.plan", |c, req| Ok(api::doctor_orphans_plan(&req, c.ctx)?)).long().destructive();
    r.apply("doctor.orphans.apply", PlanStore::None, |c, req| Ok(api::doctor_orphans_apply(&req, c.ctx)?))
        .long()
        .destructive();
    r.read("check", |c, req| Ok(api::check(c.existing()?.as_ref(), &req, c.ctx)?)).long();
    r.read("analysis.bench", |c, req| Ok(api::analysis_bench(&req, c.ctx)?)).long();
    r.read("plays.history", |_, req| Ok(api::drive_history(&req)?)).quick();
    r.read("plays.archive", |c, req| Ok(api::history_archive(&Store::open(c.root)?, &req, c.ctx)?)).long();
    r.read("plays.stats", |c, req| Ok(api::play_stats(c.existing()?.as_ref(), &req)?));
    r.read("players.models", |_, _: NoParams| Ok(api::player_models()));
    r.read("players.show", |c, _: NoParams| c.or_empty(api::players_show));
    r.plan("players.set.plan", |c, req| Ok(api::players_set_plan(&c.store()?, &req)?));
    r.apply("players.set.apply", Required, |c, req| Ok(api::players_set(&c.store()?, &req)?)).alias("players.set");
    r.plan("players.clear.plan", |c, req| Ok(api::players_clear_plan(&c.store()?, &req)?));
    r.apply("players.clear.apply", Required, |c, req| Ok(api::players_clear(&c.store()?, &req)?))
        .alias("players.clear");

    // Formatting.
    r.read("disks.list", |c, _: NoParams| Ok(api::list_disks(c.root)?));
    // Only an existing store is consulted; formatting never creates one, nor erases its disk.
    r.plan("format.plan", |c, req| Ok(api::format_plan(c.root, &req, c.ctx)?)).long().destructive();
    r.apply("format.apply", Existing, |c, req| Ok(api::format_apply(c.root, &req, c.ctx)?)).long().destructive();

    // The Atmosphere account (only in a build with it: `initialize` lists the `account`
    // capability), the DJ profile and publishing.
    #[cfg(feature = "account")]
    {
        r.read("account.status", |c, _: NoParams| Ok(api::account_status(c.root)?)).quick();
        r.read("account.check", |c, _: NoParams| Ok(api::account_check(c.root, c.ctx)?)).long();
        r.immediate("account.login.begin", |c, req| Ok(api::account_login_begin(c.root, &req)?)).keeps_cache();
        r.immediate("account.login.finish", |c, req| Ok(api::account_login_finish(&req, c.ctx)?)).long().keeps_cache();
        r.immediate("account.login.paste", |_, req| Ok(api::account_login_paste(&req)?)).keeps_cache();
        r.immediate("account.login.cancel", |_, req| Ok(api::Cancelled { cancelled: api::account_login_cancel(&req) }))
            .quick()
            .keeps_cache();
        r.immediate("account.logout", |c, _: NoParams| Ok(api::account_logout(c.root)?)).keeps_cache();
        r.read("account.client_metadata", |_, req| Ok(api::account_client_metadata(&req)?)).quick();
    }
    r.read("profile.build", |c, req| Ok(api::profile(c.existing()?.as_ref(), &req, c.ctx)?)).long();
    r.read("profile.document", |c, req| Ok(api::profile_document(c.existing()?.as_ref(), &req, c.ctx)?)).long();
    r.read("profile.prefs", |c, _: NoParams| c.or_empty(api::profile_prefs)).quick();
    r.immediate("profile.prefs.set", |c, req| Ok(api::profile_prefs_update(&c.store()?, &req)?));
    #[cfg(feature = "account")]
    {
        r.plan("profile.publish.plan", |c, req| Ok(api::profile_publish_plan(c.root, &req, c.ctx)?))
            .long()
            .destructive();
        r.apply("profile.publish.apply", PlanStore::None, |c, req| {
            Ok(api::profile_publish_apply(c.root, &req, c.ctx)?)
        })
        .long()
        .destructive();
        r.immediate("sets.lookup", |c, req| Ok(crate::plugins::roles::set_lookup(c.root, &req, c.ctx)?))
            .long()
            .keeps_cache();
        r.plan("sets.publish.plan", |c, req| Ok(api::set_publish_plan(c.root, &req)?)).destructive();
        r.apply("sets.publish.apply", PlanStore::None, |c, req| Ok(api::set_publish_apply(c.root, &req)?))
            .destructive();
        r.plan("settings.publish.plan", |c, _: NoParams| Ok(api::settings_publish_plan(c.root)?)).destructive();
        r.apply("settings.publish.apply", PlanStore::None, |c, _: NoParams| Ok(api::settings_publish_apply(c.root)?))
            .destructive();
        r.plan("settings.import_published.plan", |c, req| Ok(api::settings_import_published_plan(c.root, &req)?))
            .destructive();
        r.apply("settings.import_published.apply", PlanStore::None, |c, req| {
            Ok(api::settings_import_published_apply(c.root, &req)?)
        })
        .destructive();
    }

    // Sets, in the store's `sets.json` (written at once), and the sessions they're made from.
    r.read("sets.list", |c, req| c.or_empty(|s| api::sets_list(s, &req))).quick();
    r.read("sets.get", |c, req| Ok(api::set_get(&Store::open(c.root)?, &req)?)).quick();
    r.immediate("sets.new", |c, req| Ok(api::set_new(&c.store()?, &req, c.ctx)?)).long().keeps_cache();
    r.immediate("sets.edit", |c, req| Ok(api::set_edit(&c.store()?, &req, c.ctx)?)).long().keeps_cache();
    r.immediate("sets.delete", |c, req| Ok(api::set_delete(&c.store()?, &req)?)).destructive().keeps_cache();
    r.immediate("sets.export", |c, req| Ok(api::set_export(&c.store()?, &req)?)).keeps_cache().writes_to("output");
    r.read("sets.played", |c, req| c.or_empty(|s| api::track_plays(s, &req))).quick();
    r.plan("sets.playlist.plan", |c, req| Ok(api::set_playlist_plan(&c.store()?, &req, c.ctx)?));
    r.apply("sets.playlist.apply", PlanStore::None, |c, req| Ok(api::set_playlist_apply(&c.store()?, &req, c.ctx)?));
    r.read("sets.suggest", |c, req| Ok(api::sets_suggest(c.existing()?.as_ref(), &req, c.ctx)?)).long();
    r.read("sessions.list", |c, req| Ok(api::sessions(c.existing()?.as_ref(), &req, c.ctx)?)).long();
    r.immediate("sessions.dismiss", |c, req| Ok(api::sessions_dismiss(&c.store()?, &req, c.ctx)?)).long().keeps_cache();
    r.immediate("sessions.import", |c, req| Ok(api::sessions_import(&c.store()?, &req)?)).keeps_cache();

    // Mixes.
    // These keep fingerprints (and reports, exports) in the store, so they make one.
    r.immediate("mix.index", |c, req| Ok(api::mix_index(&c.store()?, &req, c.ctx)?)).long().keeps_cache();
    r.immediate("mix.identify", |c, req| Ok(api::mix_identify_cached(&LIBRARIES, &c.store()?, &req, c.ctx)?))
        .long()
        .keeps_cache();
    r.immediate("mix.export", |c, req| Ok(api::mix_export_cached(&LIBRARIES, &c.store()?, &req, c.ctx)?))
        .long()
        .keeps_cache()
        .writes_to("output");
    r.read("mix.reports.list", |c, _: NoParams| Ok(api::mix_reports_list(c.root)?)).quick();
    r.read("mix.reports.get", |c, req| Ok(api::mix_report_get(c.root, &req)?));
    r.immediate("mix.reports.html", |c, req| {
        let store = c.store()?;
        let shown = crate::plugins::shown_fields(Some(store.clone()));
        Ok(api::mix_report_html(&store, &req, Some(&shown))?)
    })
    .keeps_cache();

    // Every drive you own.
    r.read("fleet.status", |c, req| Ok(api::fleet_status(c.existing()?.as_ref(), &req, c.ctx)?)).long();
    r.read("drives.known", |c, req| c.or_empty(|s| api::drives_known(s, &req))).quick();
    r.plan("drive.rename.plan", |_, req| Ok(api::drive_rename_plan(&req)?));
    r.apply("drive.rename.apply", Optional, |c, req| Ok(api::drive_rename(c.store_opt().as_ref(), &req)?))
        .alias("drive.rename");
    r.immediate("drive.forget", |c, req| Ok(api::drive_forget(&c.store()?, &req)?)).destructive();
    r.immediate("drive.role", |c, req| Ok(api::drive_role(&c.store()?, &req)?));
    r.immediate("drive.retire", |c, req| Ok(api::drive_retire(&c.store()?, &req)?));
    r.read("drive.log", |c, req| Ok(api::drive_log(&Store::open(c.root)?, &req)?)).quick();
    r.read("drive.recognise", |c, req| Ok(api::drive_recognise(c.existing()?.as_ref(), &req, c.ctx)?)).long();
    r.plan("drive.adopt_as.plan", |c, req| Ok(api::drive_adopt_as_plan(&c.store()?, &req)?)).destructive();
    r.apply("drive.adopt_as.apply", Required, |c, req| Ok(api::drive_adopt_as(&c.store()?, &req, c.ctx)?))
        .alias("drive.adopt_as")
        .destructive();
    r.plan("drive.reidentify.plan", |c, req| Ok(api::drive_reidentify_plan(c.store_opt().as_ref(), &req)?))
        .destructive();
    r.apply("drive.reidentify.apply", Optional, |c, req| Ok(api::drive_reidentify(c.store_opt().as_ref(), &req)?))
        .alias("drive.reidentify")
        .destructive();
    r.read("drive.groups", |c, _: NoParams| c.or_empty(api::drive_sets)).alias("drive.sets").quick();
    r.plan("drive.eject.plan", |c, req| Ok(api::drive_eject_plan(Some(c.root), &req)?)).operation("eject");
    r.apply("drive.eject.apply", Optional, |c, req| Ok(api::drive_eject(c.store_opt().as_ref(), &req)?))
        .operation("eject")
        .alias("drive.eject");
    r.read("drive.verify", |c, req| Ok(api::drive_verify(c.store_opt().as_ref(), &req, c.ctx)?)).long();
    r.immediate("drive.repair", |c, req| Ok(api::drive_repair(c.store_opt().as_ref(), &req, c.ctx)?))
        .long()
        .destructive();
    // With `repair`, the drive is unmounted and damaged files may be truncated or removed.
    r.immediate("drive.fsck", |c, req| Ok(api::drive_fs_check(c.store_opt().as_ref(), &req)?)).long().destructive();
    r.read("drive.space", |c, req| Ok(api::drive_space(c.existing()?.as_ref(), &req, c.ctx)?)).long();
    r.plan("drive.contact.plan", |c, req| Ok(api::drive_contact_plan(&req, c.ctx)?));
    r.apply("drive.contact.apply", PlanStore::None, |c, req| Ok(api::drive_contact(&req, c.ctx)?))
        .alias("drive.contact");
    r.read("player.show", |_, req| Ok(api::player_show(&req)?)).quick();
    r.plan("player.set.plan", |_, req| Ok(api::player_set_plan(&req)?));
    r.apply("player.set.apply", PlanStore::None, |_, req| Ok(api::player_set(&req)?)).alias("player.set");
    r.plan("tidy.plan", |c, req| Ok(api::tidy_plan(&req, c.ctx)?)).long().destructive();
    r.apply("tidy.apply", PlanStore::None, |c, req| Ok(api::tidy_apply(&req, c.ctx)?)).long().destructive();
    r.read("recordings.list", |_, req| Ok(api::recordings_list(&req)?)).quick();
    r.plan("recordings.collect.plan", |_, req| Ok(api::recordings_collect_plan(&req)?)).writes_to("to");
    r.apply("recordings.collect.apply", PlanStore::None, |c, req| Ok(api::recordings_collect(&req, c.ctx)?))
        .writes_to("to")
        .alias("recordings.collect")
        .long();
    r.plan("firmware.plan", |_, req| Ok(api::firmware_plan(&req)?));
    r.apply("firmware.apply", PlanStore::None, |c, req| Ok(api::firmware_apply(&req, c.ctx)?)).long();
    r.plan("firmware.remove.plan", |_, req| Ok(api::firmware_remove_plan(&req)?)).destructive();
    r.apply("firmware.remove.apply", PlanStore::None, |c, req| Ok(api::firmware_remove(&req, c.ctx)?))
        .alias("firmware.remove")
        .destructive();
    r.plan("provision.plan", |c, req| Ok(api::provision_plan(&c.store()?, &req, c.ctx)?)).long().destructive();
    r.apply("provision.apply", Required, |c, req: api::ProvisionApply| {
        Ok(api::provision_apply(&c.store()?, &req.request, req.format_identity.as_deref(), c.ctx)?)
    })
    .long()
    .destructive();

    // Jobs: several steps planned and applied as one.
    r.plan("refresh.plan", |c, req| Ok(api::refresh_plan(&c.store()?, &req, c.ctx)?)).long().destructive();
    r.apply("refresh.apply", Required, |c, req| Ok(api::refresh_apply(&c.store()?, &req, c.ctx)?)).long().destructive();
    // `prep` was `set`: its plans keep that name.
    r.plan("prep.plan", |c, req| Ok(api::set_job_plan(&c.store()?, &req, c.ctx)?))
        .operation("set")
        .alias("set.plan")
        .long()
        .destructive();
    r.apply("prep.apply", Required, |c, req| Ok(api::set_job_apply(&c.store()?, &req, c.ctx)?))
        .operation("set")
        .alias("set.apply")
        .long()
        .destructive();

    // rekordbox's collection, and other DJ software.
    r.read("collection.info", |_, req| Ok(api::collection_info(&req)?)).long();
    r.read("collection.library", |_, req| Ok(api::collection_library(&req)?)).long();
    r.read("collection.diff", |_, req| Ok(api::collection_diff(&req)?)).long();
    r.plan("collection.export.plan", |c, req| Ok(api::collection_export_plan(&req, c.ctx)?)).long().destructive();
    r.apply("collection.export.apply", Optional, |c, req| {
        Ok(api::collection_export_apply(c.store_opt().as_ref(), &req, c.ctx)?)
    })
    .long()
    .destructive();

    // Plugins.
    plugins::register(&mut r);

    // For the sidecar's own tests only (`LOCALLIBRARY_RPC_TEST=1`, not listed).
    r.read("$/test.wait", |c, req: Wait| {
        if std::env::var_os("LOCALLIBRARY_RPC_TEST").is_none_or(|v| v != "1") {
            return Err(RpcError::method_not_found("$/test.wait"));
        }
        if req.items > 0 {
            c.ctx.phase_start(Phase::Scan, req.items, 0);
            for i in 0..req.items {
                c.ctx.emit(Event::Item { phase: Phase::Scan, path: format!("/test/{i}"), bytes: 0 });
            }
            c.ctx.phase_end(Phase::Scan);
        }
        let until = Instant::now() + Duration::from_millis(req.ms);
        while Instant::now() < until {
            c.ctx.check().map_err(locallibrary::Error::from)?;
            std::thread::sleep(Duration::from_millis(10));
        }
        Ok(Waited { waited_ms: req.ms })
    })
    .hidden = true;

    r.finish()
}

mod plugins;

#[cfg(test)]
mod tests;
