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

use locallibrary::op::{AnyPlanned, Operation, Planned};
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
    fn existing(&self) -> Option<Store> {
        Store::open(self.root).ok()
    }

    fn plan_store(&self, which: PlanStore) -> Result<Option<Store>, RpcError> {
        Ok(match which {
            PlanStore::None => None,
            PlanStore::Existing => self.existing(),
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

/// The plan an `.apply` applies, and what applying it returns.
trait PlanOf {
    type Report: JsonSchema;
}

impl<P: Operation> PlanOf for Planned<P, P::Request>
where
    P::Report: JsonSchema,
{
    type Report = P::Report;
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

    /// `<op>.plan`: the plan is kept for `<op>.apply {plan_id}`.
    fn plan<P, Pl, F>(&mut self, name: &'static str, f: F) -> &mut Method
    where
        P: DeserializeOwned + JsonSchema,
        Pl: AnyPlanned + PlanOf + JsonSchema + 'static,
        F: Fn(&Call<'_>, P) -> Result<Pl, RpcError> + Send + Sync + 'static,
    {
        let run: Run = Box::new(move |c, v| c.plans.keep(f(c, params(v)?)?));
        let m = self.add(Method::new(name, Kind::Plan, schema::<P>, schema::<Pl>, Some(run)));
        m.applies = Some(schema::<<Pl as PlanOf>::Report>);
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
            let (made, plan) = c.plans.take(id, operation)?;
            // Refused before anything was written (a changed or archive drive, or one in use), the
            // plan stays, to be applied again (with `overwrite_archive`, say).
            if let Err(e) = plan.check(store.as_ref()) {
                c.plans.put_back(made, plan);
                return Err(e.into());
            }
            match plan.apply_json(store.as_ref(), c.ctx) {
                Ok(report) => ok(report),
                Err(e @ locallibrary::Error::Locked(_)) if plan.check(store.as_ref()).is_ok() => {
                    c.plans.put_back(made, plan);
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
    r.read("status", |c, _: NoParams| Ok(api::status(&c.store()?, c.ctx)?)).long();
    r.read("drives.list", |_, req| Ok(api::list_drives(&req))).quick();
    r.read("drives.mounted", |_, _: NoParams| Ok(api::drives_mounted())).quick();
    r.read("drive.info", |_, req| Ok(api::drive_info(&req)?)).quick();
    r.plan("drive.adopt.plan", |_, req| Ok(api::adopt_plan(&req)?));
    r.apply("drive.adopt.apply", Optional, |c, req| Ok(api::adopt_drive(c.store_opt().as_ref(), &req)?))
        .alias("drive.adopt");
    r.read("drive.library", |_, req| Ok(api::drive_library_cached(&LIBRARIES, &req)?)).quick();
    r.read("playlists.tree", |_, req| Ok(api::playlists_tree(&LIBRARIES, &req)?)).quick();
    r.read("tracks.get", |_, req| Ok(api::tracks_get(&LIBRARIES, &req)?)).quick();
    r.read("tracks.query", |_, req| Ok(api::tracks_query_cached(&LIBRARIES, &req)?)).quick();
    r.read("track.analysis", |_, req| Ok(api::track_analysis_cached(&LIBRARIES, &req)?)).quick();
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
    r.read("undo.list", |c, req| Ok(api::undo_list(c.existing().as_ref(), &req)?)).quick();
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
    r.plan("clean.plan", |c, req| Ok(api::clean_plan(&req, c.ctx)?)).long().destructive();
    r.apply("clean.apply", PlanStore::None, |c, req| Ok(api::clean_apply(&req, c.ctx)?)).long().destructive();
    r.plan("convert.plan", |c, req| Ok(api::convert_plan(&req, c.ctx)?)).long().destructive();
    r.apply("convert.apply", PlanStore::None, |c, req| Ok(api::convert_apply(&req, c.ctx)?)).long().destructive();

    // The store.
    r.read("store.info", |c, _: NoParams| Ok(api::store_info(&c.store()?)?)).quick();
    r.read("store.history", |c, req| Ok(api::history(&c.store()?, &req)?)).quick();
    r.plan("store.gc.plan", |c, _: NoParams| Ok(api::store_gc_plan(&c.store()?)?)).long().destructive();
    r.apply("store.gc.apply", Required, |c, req: api::GcRequest| {
        let s = c.store()?;
        let _lock = s.lock()?;
        Ok(s.gc(req.dry_run)?)
    })
    .alias("store.gc")
    .long()
    .destructive();
    r.read("store.verify", |c, _: NoParams| Ok(c.store()?.verify(c.ctx)?)).long();
    r.plan("store.prune.plan", |c, req| Ok(api::store_prune_plan(&c.store()?, &req, c.ctx)?)).long().destructive();
    r.apply("store.prune.apply", Required, |c, req| Ok(api::store_prune(&c.store()?, &req, c.ctx)?))
        .alias("store.prune")
        .long()
        .destructive();
    r.immediate("store.copy", |c, req| Ok(api::store_copy(&c.store()?, &req, c.ctx)?))
        .long()
        .destructive()
        .writes_to("path");
    r.read("store.copies", |c, _: NoParams| Ok(api::store_copies(&c.store()?)?)).quick();
    r.immediate("snapshot.label", |c, req| Ok(api::snapshot_label(&c.store()?, &req)?));
    r.read("library.list", |c, _: NoParams| Ok(api::libraries_list(&c.store()?)?)).quick();
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
    r.read("smart.list", |c, _: NoParams| Ok(api::smart_list(&c.store()?)?)).quick();
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
    r.plan("history.delete.plan", |c, req| Ok(api::delete_history_plan(c.existing().as_ref(), &req, c.ctx)?))
        .destructive();
    r.apply("history.delete.apply", Existing, |c, req| {
        Ok(api::delete_history_apply(c.existing().as_ref(), &req, c.ctx)?)
    })
    .destructive();
    r.plan("import.plan", |c, req| Ok(api::import_plan(&req, c.ctx)?));
    r.apply("import.apply", PlanStore::None, |c, req| Ok(api::import_apply(&req, c.ctx)?));
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
    r.plan("playlist.restore.plan", |c, req| Ok(api::playlist_restore_plan(c.existing().as_ref(), &req, c.ctx)?))
        .destructive();
    r.apply("playlist.restore.apply", Existing, |c, req| {
        Ok(api::playlist_restore_apply(c.existing().as_ref(), &req, c.ctx)?)
    })
    .alias("playlist.restore")
    .destructive();

    // My Settings.
    r.read("settings.schema", |_, _: NoParams| Ok(locallibrary::settings::SCHEMA)).quick();
    r.read("settings.profile", |c, _: NoParams| Ok(c.store()?.settings_profile()?)).quick();
    r.immediate("settings.set", |c, req| Ok(api::settings_set(&c.store()?, &req)?));
    r.immediate("settings.unset", |c, req| Ok(api::settings_unset(&c.store()?, &req)?));
    r.read("settings.show", |c, req| Ok(api::settings_show(&c.store()?, &req)?)).quick();
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
    r.plan("art.set.plan", |_, req| Ok(api::artwork_set_plan(&req)?)).destructive();
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
    r.immediate("xml.export", |c, req| Ok(api::export_xml(&req, c.ctx)?)).long().keeps_cache().writes_to("output");
    r.immediate("export", |c, req| Ok(api::export_playlist(c.existing().as_ref(), &req, c.ctx)?))
        .long()
        .keeps_cache()
        .writes_to("output");
    r.read("diff", |c, req| Ok(api::diff(c.existing().as_ref(), &req)?));
    r.read("doctor", |c, req| Ok(api::doctor(&req, c.ctx)?)).long();
    r.plan("doctor.orphans.plan", |c, req| Ok(api::doctor_orphans_plan(&req, c.ctx)?)).long().destructive();
    r.apply("doctor.orphans.apply", PlanStore::None, |c, req| Ok(api::doctor_orphans_apply(&req, c.ctx)?))
        .long()
        .destructive();
    r.read("check", |c, req| Ok(api::check(c.existing().as_ref(), &req, c.ctx)?)).long();
    r.read("analysis.bench", |c, req| Ok(api::analysis_bench(&req, c.ctx)?)).long();
    r.read("plays.history", |_, req| Ok(api::drive_history(&req)?)).quick();
    r.read("plays.archive", |c, req| Ok(api::history_archive(&Store::open(c.root)?, &req, c.ctx)?)).long();
    r.read("plays.stats", |c, req| Ok(api::play_stats(c.existing().as_ref(), &req)?));
    r.read("players.models", |_, _: NoParams| Ok(api::player_models()));
    r.read("players.show", |c, _: NoParams| Ok(api::players_show(&c.store()?)?));
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
    r.read("profile.build", |c, req| Ok(api::profile(c.existing().as_ref(), &req, c.ctx)?)).long();
    r.read("profile.document", |c, req| Ok(api::profile_document(c.existing().as_ref(), &req, c.ctx)?)).long();
    r.read("profile.prefs", |c, _: NoParams| {
        Ok(c.existing().map(|s| api::profile_prefs(&s)).transpose()?.unwrap_or_default())
    })
    .quick();
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
    r.read("sets.list", |c, req| Ok(c.existing().map(|s| api::sets_list(&s, &req)).transpose()?.unwrap_or_default()))
        .quick();
    r.read("sets.get", |c, req| Ok(api::set_get(&Store::open(c.root)?, &req)?)).quick();
    r.immediate("sets.new", |c, req| Ok(api::set_new(&c.store()?, &req, c.ctx)?)).long().keeps_cache();
    r.immediate("sets.edit", |c, req| Ok(api::set_edit(&c.store()?, &req, c.ctx)?)).long().keeps_cache();
    r.immediate("sets.delete", |c, req| Ok(api::set_delete(&c.store()?, &req)?)).destructive().keeps_cache();
    r.immediate("sets.export", |c, req| Ok(api::set_export(&c.store()?, &req)?)).keeps_cache().writes_to("output");
    r.read("sets.played", |c, req| {
        Ok(c.existing().map(|s| api::track_plays(&s, &req)).transpose()?.unwrap_or_default())
    })
    .quick();
    r.plan("sets.playlist.plan", |c, req| Ok(api::set_playlist_plan(&c.store()?, &req, c.ctx)?));
    r.apply("sets.playlist.apply", PlanStore::None, |c, req| Ok(api::set_playlist_apply(&c.store()?, &req, c.ctx)?));
    r.read("sets.suggest", |c, req| Ok(api::sets_suggest(&c.store()?, &req, c.ctx)?)).long();
    r.read("sessions.list", |c, req| Ok(api::sessions(c.existing().as_ref(), &req, c.ctx)?)).long();
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
    r.immediate("mix.reports.html", |c, req| Ok(api::mix_report_html(&c.store()?, &req)?)).keeps_cache();

    // Every drive you own.
    r.read("fleet.status", |c, req| Ok(api::fleet_status(&c.store()?, &req, c.ctx)?)).long();
    r.read("drives.known", |c, req| Ok(api::drives_known(&c.store()?, &req)?)).quick();
    r.plan("drive.rename.plan", |_, req| Ok(api::drive_rename_plan(&req)?));
    r.apply("drive.rename.apply", Optional, |c, req| Ok(api::drive_rename(c.store_opt().as_ref(), &req)?))
        .alias("drive.rename");
    r.immediate("drive.forget", |c, req| Ok(api::drive_forget(&c.store()?, &req)?)).destructive();
    r.immediate("drive.role", |c, req| Ok(api::drive_role(&c.store()?, &req)?));
    r.immediate("drive.retire", |c, req| Ok(api::drive_retire(&c.store()?, &req)?));
    r.read("drive.log", |c, req| Ok(api::drive_log(&c.store()?, &req)?)).quick();
    r.read("drive.recognise", |c, req| Ok(api::drive_recognise(&c.store()?, &req, c.ctx)?)).long();
    r.plan("drive.adopt_as.plan", |c, req| Ok(api::drive_adopt_as_plan(&c.store()?, &req)?)).destructive();
    r.apply("drive.adopt_as.apply", Required, |c, req| Ok(api::drive_adopt_as(&c.store()?, &req, c.ctx)?))
        .alias("drive.adopt_as")
        .destructive();
    r.plan("drive.reidentify.plan", |c, req| Ok(api::drive_reidentify_plan(c.store_opt().as_ref(), &req)?))
        .destructive();
    r.apply("drive.reidentify.apply", Optional, |c, req| Ok(api::drive_reidentify(c.store_opt().as_ref(), &req)?))
        .alias("drive.reidentify")
        .destructive();
    r.read("drive.groups", |c, _: NoParams| Ok(api::drive_sets(&c.store()?)?)).alias("drive.sets").quick();
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
    r.read("drive.space", |c, req| Ok(api::drive_space(c.store_opt().as_ref(), &req, c.ctx)?)).long();
    r.plan("drive.contact.plan", |_, req| Ok(api::drive_contact_plan(&req)?));
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
    r.plan("interop.plan", |c, req| Ok(api::interop_plan(&req, c.ctx)?)).long().destructive().writes_to("output");
    r.apply("interop.apply", PlanStore::None, |c, req| Ok(api::interop_apply(&req, c.ctx)?))
        .long()
        .destructive()
        .writes_to("output");

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

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn every_plan_has_its_apply_and_every_name_one_method() {
        let r = registry();
        for m in r.methods() {
            match m.kind {
                Kind::Plan | Kind::Apply => {
                    let pair = m.pair.unwrap_or_else(|| panic!("{} has no pair", m.name));
                    assert_eq!(r.get(pair).unwrap().pair, Some(m.name), "{}", m.name);
                    assert!(m.operation.is_some(), "{}", m.name);
                }
                _ => assert!(m.pair.is_none(), "{}", m.name),
            }
            assert_eq!(m.run.is_none(), m.kind == Kind::Control, "{}", m.name);
            if let Some(instead) = m.deprecated {
                assert!(r.get(instead).is_some(), "{} points to {instead}", m.name);
            }
            for name in std::iter::once(m.name).chain(m.aliases.iter().copied()) {
                assert!(!name.is_empty() && name.chars().all(|c| c.is_ascii_lowercase() || "._$/".contains(c)));
            }
        }
        // Applies and plans of the same kind of plan agree on it.
        for m in r.methods().filter(|m| m.kind == Kind::Apply) {
            assert_eq!(r.get(m.pair.unwrap()).unwrap().operation, m.operation, "{}", m.name);
        }
        assert!(r.get("$/test.wait").is_some() && !r.names().contains(&"$/test.wait"));
    }

    /// Methods run as a worker runs them, with a store and drives in a temporary folder. Drive
    /// lookups on this thread see only those drives, never the machine's.
    struct Sidecar {
        dir: tempfile::TempDir,
        plans: Plans,
    }

    impl Sidecar {
        fn new() -> Sidecar {
            let dir = tempfile::tempdir().unwrap();
            locallibrary::testing::volumes_on_this_thread(Some(vec![dir.path().to_path_buf()]));
            locallibrary::testing::store_root_on_this_thread(Some(dir.path().join("store")));
            Sidecar { dir, plans: Plans::default() }
        }

        fn store(&self) -> std::path::PathBuf {
            self.dir.path().join("store")
        }

        /// The demo export as the drive `name`, with placeholder audio and a history list a player
        /// recorded.
        fn demo(&self, name: &str) -> String {
            fn copy(src: &Path, dst: &Path) {
                std::fs::create_dir_all(dst).unwrap();
                for e in std::fs::read_dir(src).unwrap().map(Result::unwrap) {
                    match e.file_type().unwrap().is_dir() {
                        true => copy(&e.path(), &dst.join(e.file_name())),
                        false => drop(std::fs::copy(e.path(), dst.join(e.file_name())).unwrap()),
                    }
                }
            }
            let root = self.dir.path().join(name);
            let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../crates/locallibrary/tests/fixtures/rekordcrate/demo_tracks");
            copy(&fixture, &root);
            let pdb = root.join("PIONEER/rekordbox/export.pdb");
            for t in &rekordbox_pdb::Database::from_file(&pdb).unwrap().tracks {
                let audio = root.join(t.file_path().trim_start_matches('/'));
                std::fs::create_dir_all(audio.parent().unwrap()).unwrap();
                std::fs::write(audio, t.title()).unwrap();
            }
            let mut ed = rekordbox_pdb::PdbEditor::from_file(&pdb).unwrap();
            let list = ed.create_history_playlist("HISTORY 001").unwrap();
            ed.add_to_history(list, 1).unwrap();
            ed.save(&pdb).unwrap();
            root.display().to_string()
        }

        fn call(&self, method: &str, params: Value) -> Result<Value, RpcError> {
            let (root, ctx) = (self.store(), Ctx::silent());
            let call = Call { root: &root, plans: &self.plans, ctx: &ctx };
            registry().get(method).unwrap().run(&call, params).expect("a worker runs it")
        }

        fn ok(&self, method: &str, params: Value) -> Value {
            self.call(method, params).unwrap_or_else(|e| panic!("{method}: {}", e.message))
        }
    }

    /// A drive's library, read from its database rather than the sidecar's cache.
    fn library(drive: &str) -> locallibrary::Library {
        locallibrary::pdb::read_library(&Path::new(drive).join("PIONEER/rekordbox/export.pdb")).unwrap()
    }

    fn playlists(drive: &str) -> Vec<String> {
        library(drive).playlists.into_iter().map(|p| p.name).collect()
    }

    #[test]
    fn reads_without_a_store_answer_empty_and_make_none() {
        let s = Sidecar::new();
        assert!(s.ok("profile.prefs", json!({})).is_object());
        assert_eq!(s.ok("sets.played", json!({ "track": "Demo Track 1" })), json!([]));
        assert!(!s.store().exists());
    }

    /// `$/test.wait` answers only in the sidecar's own tests (`LOCALLIBRARY_RPC_TEST=1`).
    #[test]
    fn the_test_method_is_not_there_outside_the_tests() {
        let s = Sidecar::new();
        assert_eq!(s.call("$/test.wait", json!({})).err().map(|e| e.kind), Some("method_not_found"));
    }

    /// An apply given a request instead of a plan's id plans and applies it at once.
    #[test]
    fn an_apply_given_a_request_plans_and_applies_it() {
        let s = Sidecar::new();
        assert_eq!(s.ok("store.gc.apply", json!({ "dry_run": true }))["freed_bytes"], 0);
        let unembed = s.call("art.unembed.apply", json!({})).err().map(|e| (e.kind, e.message));
        assert_eq!(unembed, Some(("not_found", "not found: no embedded art to take back".into())));
    }

    /// Deleting history lists by a plan's id or by request comes to the same, with or without a
    /// store.
    #[test]
    fn history_lists_are_deleted_by_plan_or_by_request() {
        let s = Sidecar::new();
        let (a, b) = (s.demo("A"), s.demo("B"));
        assert_eq!(s.ok("plays.history", json!({ "drive": a }))["lists"].as_array().unwrap().len(), 1);
        let plan = s.ok("history.delete.plan", json!({ "drive": a, "force": true }));
        s.ok("history.delete.apply", json!({ "plan_id": plan["plan_id"] }));
        s.ok("history.delete.apply", json!({ "drive": b, "force": true }));
        assert!(library(&a).history.is_empty() && library(&b).history.is_empty());
    }

    /// Refused because the store is busy, a plan stays, to be applied once it isn't.
    #[test]
    fn a_plan_refused_while_the_store_is_locked_stays() {
        let s = Sidecar::new();
        let id = s.ok("store.gc.plan", json!({}))["plan_id"].clone();
        let lock = Store::open(s.store()).unwrap().lock().unwrap();
        let busy = s.call("store.gc.apply", json!({ "plan_id": id }));
        assert_eq!(busy.err().map(|e| e.kind), Some("locked"));
        drop(lock);
        assert!(s.ok("store.gc.apply", json!({ "plan_id": id }))["freed_bytes"].is_number());
    }

    /// Restoring a backup is planned as an undo, which it is, under its own name too.
    #[test]
    fn a_backup_is_restored_by_plan() {
        let s = Sidecar::new();
        let a = s.demo("A");
        s.ok("edit.apply", json!({ "drive": a, "edits": [{ "op": "create_playlist", "path": "Friday" }] }));
        assert_eq!(playlists(&a), ["Friday"]);
        let plan = s.ok("drive.restore.plan", json!({ "drive": a }));
        assert_eq!(plan["operation"], "undo", "{plan}");
        s.ok("drive.restore.apply", json!({ "plan_id": plan["plan_id"] }));
        assert!(playlists(&a).is_empty());
    }

    /// A playlist deleted from a drive comes back from the store's snapshot of it, and the drive's
    /// My Tags are listed.
    #[test]
    fn a_playlist_comes_back_from_the_store() {
        let s = Sidecar::new();
        let a = s.demo("A");
        let friday = json!({ "op": "add_tracks", "playlist": "Friday", "tracks": { "ids": [1] } });
        s.ok("edit.apply", json!({ "drive": a, "edits": [{ "op": "create_playlist", "path": "Friday" }, friday] }));
        s.ok("pull.apply", json!({ "drive": a, "library": "Main", "new_library": true }));
        s.ok("edit.apply", json!({ "drive": a, "edits": [{ "op": "delete_playlist", "playlist": "Friday" }] }));
        assert!(playlists(&a).is_empty());
        s.ok("playlist.restore.apply", json!({ "drive": a, "playlist": "Friday" }));
        assert_eq!(playlists(&a), ["Friday"]);

        s.ok("tags.apply", json!({ "drive": a, "action": "add", "tag": "Genre/Deep", "ids": [1] }));
        let tags = s.ok("tags.list", json!({ "drive": a }));
        assert_eq!(tags["categories"][0]["name"], "Genre", "{tags}");
    }
}
