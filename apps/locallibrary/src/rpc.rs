//! `locallibrary rpc`: JSON-RPC 2.0 over stdio, one JSON message per line.
//!
//! Intended as a sidecar for GUI front ends (an Electron app spawns `locallibrary rpc` and talks to it
//! over pipes). Every method maps onto [`locallibrary::api`]; parameters are that module's
//! request structs as JSON objects. The methods are listed once, in [`methods`], with their
//! schemas: `rpc.describe` answers them and `locallibrary rpc --emit-schema DIR` writes them as
//! `schema.json` and TypeScript types (see [`contract`]).
//!
//! Only a trusted parent process may spawn and drive the sidecar: it acts with the user's rights.
//! Requests name files and folders it reads and writes (`output` for exports, `plugins.export.*`
//! and `mix.export`, `to` for `recordings.collect.*`, `path` for `store.copy`, images and audio for
//! `art.*`, `file` for `firmware.*`, a plugin's path params, `db` for `collection.*`, any folder as
//! a drive) and servers it fetches from (`pds` for `settings.import_published.*`, `client_id` and
//! `scopes` for `account.login.begin`, which the user still approves in the browser; no token is
//! ever sent back over the pipe). Never expose it on a socket or to another user, and never pass
//! web content (a page's or a renderer's input) into its parameters. It refuses relative paths to
//! write to (its working folder is whatever its parent's was) and a data server that isn't https
//! (or on this machine); those checks only catch mistakes.
//!
//! * Requests run concurrently, so a long `sync.apply` doesn't block `drives.list`. An id (a
//!   string or a number) that a running request already has is refused (-32600). Notifications
//!   (no id) get no response and can't be cancelled.
//! * Long operations send `progress` notifications: `{"method":"progress","params":{"id":<request
//!   id>,"event":{...}}}`. Byte counts and per-file `item` events are coalesced to about ten a
//!   second (`items` says how many files an `item` stands for). Events about one drive of several
//!   carry `target` (its root); events during a step carry `step` (`{index, total, id}`), and a
//!   `step` event announces each step.
//! * `$/cancel` with `{"id": <request id>}` stops a running request at the next safe point; it
//!   then fails with error kind `cancelled`. `jobs.list` lists the running requests.
//! * `shutdown`, or closing stdin, stops reading requests; running ones finish, then the process
//!   exits. `shutdown` with `{"cancel": true}` cancels them first.
//! * SIGINT or SIGTERM (Ctrl-C on Windows) cancels every running request and refuses new ones;
//!   the process exits (128 + the signal's number) once they have answered. A second signal
//!   exits once no database swap is under way.
//! * A line that isn't JSON (or isn't UTF-8), or is longer than 16 MiB, gets a parse error
//!   (-32700); requests carry on.
//! * Errors use code -32000 with `data.kind` set to a stable identifier (`not_found`, `conflict`,
//!   `no_space`, `cancelled`, ...).
//! * `initialize` returns the protocol version, the sidecar's version, its capabilities and its
//!   methods; `version` the last two. `rpc.describe` (`{"method": ...}` for one) describes them:
//!   kind, paired plan or apply, aliases, and the schemas of their parameters and results.
//! * `events.subscribe` (`{"kinds": [...]}`, default every kind) sends `event` notifications,
//!   `{"method":"event","params":{"event":"drive.attached", ...}}`, for drives attached, detached
//!   and changed and the store changing (polled cheaply, see [`locallibrary::watch`]), and for
//!   requests starting and finishing (`job.started`, `job.finished`). `events.unsubscribe` stops
//!   them.
//! * `watch.start` pulls drives into the store as they're plugged in (`locallibrary watch`, see
//!   [`locallibrary::api::AutoPull`]) until `watch.stop`, sending a `watch.*` event for each
//!   thing it does.
//! * Plugins listening for them are told of these events while the sidecar runs (drives are
//!   polled for them whether or not the client subscribed), and of plans applied; what plugins
//!   tell the user is a `plugin.notice` event. Plugins' services the user turned on run while it
//!   does.
//! * Every `*.plan` answers with a `plan_id` (see [`locallibrary::op`]) and keeps the plan, so
//!   `*.apply {"plan_id": ...}` applies exactly what the user was shown, or fails with
//!   `stale_plan` if anything it touches changed since. A plan is applied once; the newest
//!   [`MAX_PLANS`] are kept for [`PLAN_TTL`], shared by every worker. `*.apply` with the request
//!   instead plans and applies in one go, bound to no confirmation. `overwrite_archive: true` in
//!   any request's params lets it write to archive and retired drives.

mod contract;
mod methods;
mod typescript;

use indexmap::IndexMap;
use std::collections::VecDeque;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use locallibrary::api;
use locallibrary::api::RpcFailure as RpcError;
use locallibrary::op::AnyPlanned;
use locallibrary::progress::{CancelToken, Coalesced, Ctx, Event, Flusher, Progress, Report, Scope};
use locallibrary::watch::{DriveChange, VolumeWatcher, WatchEvent};
use loclib_plugin::EventName;
use loclib_plugin::code::{FAILED, INTERNAL, INVALID_PARAMS, INVALID_REQUEST, PARSE_ERROR};
use loclib_plugin::framing::{Line, MAX_LINE, read_line};
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use self::methods::{Call, Kind, registry};

pub use self::contract::emit as emit_schema;

/// Bumped when a change would break a client written for the previous one.
const PROTOCOL: u32 = 1;

/// Features a client can check for in `initialize`'s result. Besides these, `account` when the
/// sidecar was built with the Atmosphere account (see [`capabilities`]).
const CAPABILITIES: &[&str] = &[
    "progress",
    "progress.scope",
    "cancel",
    "jobs.list",
    "shutdown.cancel",
    "events",
    "plan_id",
    "describe",
    "error.detail",
];

/// What `initialize` says the sidecar can do: [`CAPABILITIES`], and `account` in a build with the
/// Atmosphere account, which has the `account.*` and `*.publish.*` methods (`settings.import_published.*`
/// too). A sidecar built without it answers those with `method_not_found`.
fn capabilities() -> Vec<String> {
    let mut all: Vec<String> = CAPABILITIES.iter().map(|c| c.to_string()).collect();
    if cfg!(feature = "account") {
        all.push("account".into());
    }
    all
}

/// How long a plan waits to be applied.
const PLAN_TTL: Duration = Duration::from_secs(30 * 60);

/// Most plans kept at once; the oldest goes first.
const MAX_PLANS: usize = 64;

/// How often a request's coalesced progress is sent, at most.
const PROGRESS_EVERY: Duration = Duration::from_millis(100);

/// Requests run at once in each pool (reads, and everything else), unless `--workers` says.
pub const WORKERS: usize = 4;

/// Sends the progress requests hold back once it has waited long enough (see [`RpcProgress`]).
static PROGRESS: Flusher = Flusher::new();

/// Parsed libraries kept between requests (see [`api::LibraryCache`]): the last few drives read.
static LIBRARIES: api::LibraryCache = api::LibraryCache::new(4);

/// Plans `*.plan` made, until they're applied (once), expire or make room for newer ones. One
/// lock for each of its caches, shared by every worker: a plan is taken out under it, so two
/// `*.apply` with the same `plan_id` can't both get it. An apply refused before anything was
/// written puts it back. Plugins' plans (the `plugins.*` methods') are kept apart, so planning
/// with plugins never makes one of locallibrary's own go before it's applied.
#[derive(Default)]
struct Plans {
    own: Kept,
    plugins: Kept,
}

type Kept = Mutex<VecDeque<(Instant, Box<dyn AnyPlanned>)>>;

impl Plans {
    const fn new() -> Plans {
        Plans { own: Mutex::new(VecDeque::new()), plugins: Mutex::new(VecDeque::new()) }
    }

    /// The cache the plans the method `method` makes or applies are kept in.
    fn cache(&self, method: &str) -> &Kept {
        if method.starts_with(PLUGINS) { &self.plugins } else { &self.own }
    }

    fn lock(kept: &Kept) -> MutexGuard<'_, VecDeque<(Instant, Box<dyn AnyPlanned>)>> {
        let mut kept = kept.lock().unwrap_or_else(PoisonError::into_inner);
        kept.retain(|(made, _)| made.elapsed() < PLAN_TTL);
        kept
    }

    /// Keep a plan: the answer of `method` (a `*.plan`).
    fn keep(&self, method: &str, plan: impl AnyPlanned + 'static) -> Result<Value, RpcError> {
        let json = plan.to_json();
        let mut kept = Plans::lock(self.cache(method));
        while kept.len() >= MAX_PLANS {
            kept.pop_front();
        }
        kept.push_back((Instant::now(), Box::new(plan)));
        Ok(json)
    }

    /// Take out the plan `id` for `method` (a `*.apply` of plans made by `operation.plan`) to
    /// apply it, with when it was made.
    fn take(&self, method: &str, id: &str, operation: &str) -> Result<(Instant, Box<dyn AnyPlanned>), RpcError> {
        let cache = self.cache(method);
        let mut kept = Plans::lock(cache);
        let Some(i) = kept.iter().position(|(_, p)| p.id() == id) else {
            drop(kept);
            // One the other cache has is for other applies.
            let other = if std::ptr::eq(cache, &self.own) { &self.plugins } else { &self.own };
            if let Some((_, plan)) = Plans::lock(other).iter().find(|(_, p)| p.id() == id) {
                return Err(invalid_params(format!("plan {id} is a {} plan, not {operation}", plan.operation())));
            }
            return Err(RpcError::new(
                FAILED,
                "stale_plan",
                format!(
                    "there's no plan {id}: it was applied already (or failed while being applied), it expired, or \
                     the sidecar restarted; plan again"
                ),
            ));
        };
        let made = kept[i].1.operation();
        if applied_as(method, made) != operation {
            return Err(invalid_params(format!("plan {id} is a {made} plan, not {operation}")));
        }
        Ok(kept.remove(i).expect("found above"))
    }

    /// Keep a plan [`Plans::take`] took out for `method` again, as when it was made.
    fn put_back(&self, method: &str, made: Instant, plan: Box<dyn AnyPlanned>) {
        Plans::lock(self.cache(method)).push_back((made, plan));
    }
}

/// The operation (see [`methods::Method::operation`]) of the `.apply` that applies a plan of
/// `operation`, for `method`: the plan's own (the host's `plugins.*`, too), but a plugin's
/// operation (`<plugin>.<operation>`) is `plugins.op`'s, its export (`<plugin>.export.<format>`)
/// `plugins.export`'s, its import (`<plugin>.import.<format>`) `plugins.import`'s and a drive
/// built from its library source (`<plugin>.source.<source>`) `plugins.source`'s.
fn applied_as<'a>(method: &str, operation: &'a str) -> &'a str {
    if !method.starts_with(PLUGINS) {
        return operation;
    }
    match operation.split('.').collect::<Vec<_>>()[..] {
        ["plugins", _] => operation,
        [_, "export", _] => "plugins.export",
        [_, "import", _] => "plugins.import",
        [_, "source", _] => "plugins.source",
        _ => "plugins.op",
    }
}

type Task = Box<dyn FnOnce() + Send>;

/// A fixed number of threads running requests in the order they came.
struct Pool {
    queue: Mutex<std::sync::mpsc::Sender<Task>>,
}

impl Pool {
    fn new(threads: usize) -> Pool {
        let (tx, rx) = std::sync::mpsc::channel::<Task>();
        let rx = Arc::new(Mutex::new(rx));
        for _ in 0..threads.max(1) {
            let rx = rx.clone();
            std::thread::spawn(move || {
                loop {
                    let task = rx.lock().unwrap_or_else(PoisonError::into_inner).recv();
                    let Ok(task) = task else { return };
                    // A task answers its request even when it panics; this keeps the thread.
                    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(task));
                }
            });
        }
        Pool { queue: Mutex::new(tx) }
    }

    fn run(&self, task: Task) {
        let _ = self.queue.lock().unwrap_or_else(PoisonError::into_inner).send(task);
    }
}

type Out = Arc<Mutex<Box<dyn Write + Send>>>;

fn send(out: &Out, msg: &Value) {
    let mut o = out.lock().unwrap_or_else(PoisonError::into_inner);
    let _ = writeln!(o, "{msg}");
    let _ = o.flush();
}

fn invalid_request(message: impl Into<String>) -> RpcError {
    RpcError::new(INVALID_REQUEST, "invalid_request", message)
}

fn invalid_params(e: impl std::fmt::Display) -> RpcError {
    RpcError::new(INVALID_PARAMS, "invalid_params", format!("invalid params: {e}"))
}

/// A request's parameters as `T`; when they don't fit, `invalid_params` says where
/// (`fields.path`, `edits[0].op`).
fn params<T: DeserializeOwned>(v: Value) -> Result<T, RpcError> {
    loclib_plugin::params(v).map_err(|e| {
        let mut err = RpcError::new(INVALID_PARAMS, "invalid_params", e.message);
        err.detail.fields = e.data.map(|d| d.fields).unwrap_or_default();
        err
    })
}

fn ok<T: serde::Serialize>(v: T) -> Result<Value, RpcError> {
    serde_json::to_value(v).map_err(|e| RpcError::new(INTERNAL, "internal", e.to_string()))
}

/// Sends a request's progress notifications, coalesced (see [`Coalesced`]): a 100 GB copy or a
/// 10,000-track export sends about ten lines a second, not one per chunk or file.
struct RpcProgress(Arc<Coalesced>);

impl RpcProgress {
    fn new(id: Value, out: Out) -> Self {
        let emit = move |scope: &Scope, report: Report| {
            let (event, items) = match report {
                Report::Event { event, items } => (api::Progressed::Event(event), items),
                Report::Step => (api::Progressed::Step(api::StepStarted::new()), None),
            };
            let params = api::ProgressParams {
                id: id.clone(),
                event: api::ProgressEvent { event, scope: scope.clone(), items },
            };
            send(&out, &json!({ "jsonrpc": "2.0", "method": "progress", "params": params }));
        };
        RpcProgress(Arc::new(Coalesced::new(PROGRESS_EVERY, emit)))
    }

    /// Sends everything held back (before the response).
    fn finish(&self) {
        self.0.finish();
    }
}

impl Progress for RpcProgress {
    fn on_event(&self, event: &Event) {
        self.0.on_event(event);
    }

    fn on_scoped_event(&self, scope: &Scope, event: &Event) {
        self.0.on_scoped_event(scope, event);
    }

    fn on_step(&self, scope: &Scope) {
        self.0.on_step(scope);
    }
}

/// Requests in flight, so `$/cancel`, `jobs.list`, signals and shutdown can reach them.
#[derive(Default)]
struct Jobs {
    state: Mutex<JobState>,
    /// Notified whenever a job starts or ends.
    changed: Condvar,
}

#[derive(Default)]
struct JobState {
    next: u64,
    /// In the order they started.
    running: IndexMap<u64, Job>,
    /// Shutting down: new requests are refused.
    stopping: bool,
    /// The signal that stopped the sidecar: it sets the exit status.
    signal: Option<i32>,
}

struct Job {
    /// `null` for a notification, which `$/cancel` can't name.
    id: Value,
    method: String,
    started: DateTime<Utc>,
    cancel: CancelToken,
    /// Waiting for a worker (see [`Pool`]).
    queued: bool,
}

impl Jobs {
    fn lock(&self) -> MutexGuard<'_, JobState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Registers a request. Its id must not be one a running request has: `$/cancel` and the
    /// response would be ambiguous.
    fn start(&self, job: Job) -> Result<u64, RpcError> {
        let mut s = self.lock();
        if s.stopping {
            return Err(RpcError::new(FAILED, "cancelled", "locallibrary is shutting down"));
        }
        if !job.id.is_null()
            && let Some(other) = s.running.values().find(|j| j.id == job.id)
        {
            return Err(invalid_request(format!(
                "request id {} is already in use by a running {:?} request",
                job.id, other.method
            )));
        }
        let key = s.next;
        s.next += 1;
        s.running.insert(key, job);
        self.changed.notify_all();
        Ok(key)
    }

    /// Sends a finished job's response, if it has one, and forgets the job. Both happen under
    /// the lock, so shutting down never exits between the two, and the id is free again by the
    /// time the client has the response.
    fn finish(&self, key: u64, out: &Out, response: Option<&Value>) {
        let mut s = self.lock();
        if let Some(r) = response {
            send(out, r);
        }
        s.running.shift_remove(&key);
        self.changed.notify_all();
    }

    /// A worker takes up a queued job. False when it was cancelled while it waited (and has
    /// answered).
    fn begin(&self, key: u64) -> bool {
        let mut s = self.lock();
        s.running.get_mut(&key).map(|j| j.queued = false).is_some()
    }

    /// Cancels the running request with this id; false when there's none. One still waiting
    /// for a worker answers at once (after `dropped`, under the lock as [`Jobs::finish`] does).
    fn cancel(&self, id: &Value, out: &Out, dropped: impl FnOnce(&Job)) -> bool {
        if id.is_null() {
            return false;
        }
        let mut s = self.lock();
        let Some((&key, job)) = s.running.iter().find(|(_, j)| j.id == *id) else { return false };
        job.cancel.cancel();
        if job.queued {
            dropped(job);
            let e = RpcError::from(locallibrary::Error::Cancelled);
            send(out, &response(id, Err(e)));
            s.running.shift_remove(&key);
            self.changed.notify_all();
        }
        true
    }

    /// Refuses new requests from now on and, with `cancel`, cancels the running ones.
    fn stop(&self, cancel: bool) {
        let mut s = self.lock();
        s.stopping = true;
        if cancel {
            for job in s.running.values() {
                job.cancel.cancel();
            }
        }
    }

    /// A signal arrived: refuse new requests and cancel the running ones.
    fn interrupt(&self, signal: i32) {
        self.lock().signal = Some(signal);
        self.stop(true);
    }

    /// Whether new requests are being refused (shutting down).
    fn is_stopping(&self) -> bool {
        self.lock().stopping
    }

    fn list(&self) -> Vec<api::JobInfo> {
        let s = self.lock();
        s.running
            .values()
            .map(|j| api::JobInfo { id: j.id.clone(), method: j.method.clone(), started: j.started, queued: j.queued })
            .collect()
    }

    /// Blocks until no request is running.
    fn wait_idle(&self) {
        let mut s = self.lock();
        while !s.running.is_empty() {
            s = self.changed.wait(s).unwrap_or_else(PoisonError::into_inner);
        }
    }
}

/// How often drives and the store are polled for events, unless the subscriber asks otherwise.
const EVENTS_EVERY: Duration = Duration::from_secs(1);

/// `events.subscribe`: what the client asked for, and the watcher polling drives for it.
#[derive(Default)]
struct Events {
    state: Mutex<EventState>,
}

#[derive(Default)]
struct EventState {
    /// Subscribed: the kinds wanted (empty: every kind). `None`: not subscribed.
    kinds: Option<Vec<String>>,
    every: Duration,
    /// Plugins listen for drives: they're polled as long as the sidecar runs.
    plugins: bool,
    /// Shared with the thread polling it, which ends (and clears this) once nobody is subscribed
    /// and no plugin listens.
    watcher: Option<Arc<Mutex<VolumeWatcher>>>,
}

impl EventState {
    /// Whether an event of this kind (`drive.attached`) is wanted: subscribed to it, to its
    /// group (`drive`), or to everything.
    fn wants(&self, kind: &str) -> bool {
        self.kinds.as_ref().is_some_and(|kinds| {
            kinds.is_empty()
                || kinds
                    .iter()
                    .any(|k| kind.strip_prefix(k.as_str()).is_some_and(|rest| rest.is_empty() || rest.starts_with('.')))
        })
    }
}

impl Events {
    fn lock(&self) -> MutexGuard<'_, EventState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Sends `event` if it's wanted, and tells the plugins listening for it.
    fn send(&self, out: &Out, event: api::Notice) {
        crate::plugins::events::deliver(&event);
        let event = json!(event);
        let s = self.lock();
        if event["event"].as_str().is_some_and(|kind| s.wants(kind)) {
            send(out, &json!({ "jsonrpc": "2.0", "method": "event", "params": event }));
        }
    }

    /// Sends a `watch.*` event: always while watching, unless a subscription leaves it out. And
    /// tells the plugins listening for it.
    fn send_watch(&self, out: &Out, event: api::Notice) {
        crate::plugins::events::deliver(&event);
        let event = json!(event);
        let s = self.lock();
        if s.kinds.is_none() || event["event"].as_str().is_some_and(|kind| s.wants(kind)) {
            send(out, &json!({ "jsonrpc": "2.0", "method": "event", "params": event }));
        }
    }

    /// Subscribes (again, replacing the kinds), starting the watcher if it isn't running.
    /// Returns the kinds and the drives mounted now: events say what changes from there.
    fn subscribe(self: &Arc<Self>, req: api::Subscribe, store_root: &Path, out: &Out) -> api::Subscribed {
        let mut s = self.lock();
        s.kinds = Some(req.kinds.clone());
        s.every = req.interval_ms.map_or(EVENTS_EVERY, |ms| Duration::from_millis(ms.max(100)));
        let watcher = self.watcher(&mut s, store_root, out);
        let drives = watcher.lock().unwrap_or_else(PoisonError::into_inner).drives();
        api::Subscribed { kinds: req.kinds, drives }
    }

    /// Polls drives for the plugins listening for them as long as the sidecar runs, whoever
    /// subscribes, when any is.
    fn poll_for_plugins(self: &Arc<Self>, store_root: &Path, out: &Out) {
        let Ok(plugins) = crate::plugins::host() else { return };
        let drives = [EventName::DriveMounted, EventName::DriveUnmounted, EventName::DriveChanged];
        if !drives.into_iter().any(|e| plugins.listens(e)) {
            return;
        }
        let mut s = self.lock();
        s.plugins = true;
        s.every = EVENTS_EVERY;
        self.watcher(&mut s, store_root, out);
    }

    /// The watcher, started polling if it isn't.
    fn watcher(self: &Arc<Self>, s: &mut EventState, store_root: &Path, out: &Out) -> Arc<Mutex<VolumeWatcher>> {
        if let Some(w) = &s.watcher {
            return w.clone();
        }
        let w = Arc::new(Mutex::new(VolumeWatcher::new().with_store(store_root)));
        s.watcher = Some(w.clone());
        let (events, out, polled) = (self.clone(), out.clone(), w.clone());
        std::thread::spawn(move || events.poll(&polled, &out));
        w
    }

    /// Polls drives and the store while anyone is subscribed, or plugins listen.
    fn poll(&self, watcher: &Mutex<VolumeWatcher>, out: &Out) {
        loop {
            let every = self.lock().every;
            std::thread::sleep(every);
            let found = watcher.lock().unwrap_or_else(PoisonError::into_inner).poll();
            for e in &found {
                match e {
                    WatchEvent::Changed { drive, changed } if changed.contains(&DriveChange::Databases) => {
                        LIBRARIES.invalidate(&drive.root);
                    }
                    WatchEvent::Detached(drive) => LIBRARIES.invalidate(&drive.root),
                    _ => {}
                }
                crate::plugins::events::deliver(&api::Notice::Watch(e.clone()));
            }
            let mut s = self.lock();
            if s.kinds.is_none() && !s.plugins {
                s.watcher = None;
                return;
            }
            for e in found.into_iter().filter(|e| s.wants(e.kind())) {
                send(out, &json!({ "jsonrpc": "2.0", "method": "event", "params": api::Notice::Watch(e) }));
            }
        }
    }

    /// False when there was no subscription.
    fn unsubscribe(&self) -> bool {
        self.lock().kinds.take().is_some()
    }
}

/// A running `watch.start`: set `stop` to end it after what it's doing; its job's cancel token
/// stops that too.
struct Watch {
    stop: Arc<AtomicBool>,
    cancel: CancelToken,
}

struct Server {
    store_root: PathBuf,
    out: Out,
    jobs: Arc<Jobs>,
    events: Arc<Events>,
    watch: Arc<Mutex<Option<Watch>>>,
    /// Runs quick reads (see [`methods::Method::quick`]).
    reads: Pool,
    /// Runs everything else.
    work: Pool,
    /// Plans waiting for `*.apply {plan_id}`.
    plans: Arc<Plans>,
}

/// Serves requests until input closes or `shutdown`; `workers` requests of each kind (quick
/// reads, and everything else) run at once.
pub fn serve(store_root: PathBuf, workers: usize) -> anyhow::Result<()> {
    let server = Server {
        store_root,
        out: Arc::new(Mutex::new(Box::new(std::io::stdout()))),
        jobs: Arc::default(),
        events: Arc::default(),
        watch: Arc::default(),
        reads: Pool::new(workers),
        work: Pool::new(workers),
        plans: Arc::default(),
    };
    watch_signals(&server)?;
    std::thread::spawn(|| PROGRESS.run(PROGRESS_EVERY));
    let (events, out) = (server.events.clone(), server.out.clone());
    let told = move |n| events.send(&out, api::Notice::Plugin(api::PluginEvent::Notice(n)));
    crate::plugins::events::notices_to(Some(Arc::new(told)));
    server.events.poll_for_plugins(&server.store_root, &server.out);
    if let Ok(plugins) = crate::plugins::host() {
        plugins.run_services(locallibrary::Store::open(&server.store_root).ok().as_ref());
    }

    let mut input = std::io::stdin().lock();
    let mut buf = Vec::new();
    let mut failed = None;
    loop {
        match read_line(&mut input, &mut buf) {
            Ok(Line::End) => break,
            Ok(Line::Read) => {}
            Ok(Line::TooLong) => {
                let e = format!("parse error: a line is longer than {} MiB", MAX_LINE >> 20);
                send(&server.out, &response(&Value::Null, Err(RpcError::new(PARSE_ERROR, "parse_error", e))));
                continue;
            }
            Err(e) => {
                // Treated like closed input, so running requests still finish.
                failed = Some(e);
                break;
            }
        }
        if !server.handle(buf.trim_ascii()) {
            break;
        }
    }
    // Input closed or shutdown: let running requests finish (a sync interrupted only because the
    // parent went away gains nothing), then exit. Use `$/cancel` to stop them early. Watching
    // stops after what it's doing.
    server.stop_watch(false);
    server.jobs.wait_idle();
    // A signal sets the exit status, even when input closed while it was being handled.
    if let Some(signal) = server.jobs.lock().signal {
        stop_plugins();
        exit_after_signal(&server.out, signal);
    }
    match failed {
        Some(e) => Err(anyhow::Error::new(e).context("reading requests")),
        None => Ok(()),
    }
}

/// Stops the plugins running, each given its grace to exit: before exiting on a signal (`main`
/// stops them otherwise). Not on a second signal, which exits at once. A signal and closed input
/// can bring two threads here (or one here and `main`) at once: each returns only once the
/// plugins have stopped, whichever stopped them.
fn stop_plugins() {
    if let Ok(plugins) = crate::plugins::host() {
        plugins.shutdown();
    }
}

/// Exits with 128 + `signal` once no database swap is under way, holding the output so no line
/// is cut short.
fn exit_after_signal(out: &Out, signal: i32) -> ! {
    let _swaps = locallibrary::progress::wait_for_critical();
    let _out = out.lock();
    std::process::exit(128 + signal);
}

/// SIGINT and SIGTERM (Ctrl-C on Windows) cancel every running request at its next safe point
/// and refuse new ones; the process exits once the running ones have answered. A second signal
/// exits once no database swap is under way.
fn watch_signals(server: &Server) -> anyhow::Result<()> {
    let (jobs, out) = (server.jobs.clone(), server.out.clone());
    let signalled = AtomicBool::new(false);
    let on_signal = move |signal: i32| {
        if signalled.swap(true, Ordering::SeqCst) {
            exit_after_signal(&out, signal);
        }
        eprintln!("locallibrary rpc: stopping running requests... (signal again to force)");
        // On its own thread, so a second signal is still heard while this waits.
        let (jobs, out) = (jobs.clone(), out.clone());
        std::thread::spawn(move || {
            jobs.interrupt(signal);
            jobs.wait_idle();
            stop_plugins();
            exit_after_signal(&out, signal);
        });
    };
    #[cfg(unix)]
    {
        use signal_hook::consts::{SIGINT, SIGTERM};
        let mut signals = signal_hook::iterator::Signals::new([SIGINT, SIGTERM])?;
        std::thread::spawn(move || {
            for signal in signals.forever() {
                on_signal(signal);
            }
        });
    }
    #[cfg(not(unix))]
    ctrlc::set_handler(move || on_signal(2))?;
    Ok(())
}

impl Server {
    /// Handles one line of input. False once `shutdown` has been answered.
    fn handle(&self, line: &[u8]) -> bool {
        if line.is_empty() {
            return true;
        }
        // Raw bytes: a line that isn't UTF-8 gets a parse error like any other bad line, instead
        // of ending the loop (and with it a sync in progress).
        let msg: Value = match serde_json::from_slice(line) {
            Ok(v) => v,
            Err(e) => {
                let e = RpcError::new(PARSE_ERROR, "parse_error", format!("parse error: {e}"));
                send(&self.out, &response(&Value::Null, Err(e)));
                return true;
            }
        };
        let id = msg.get("id").cloned().unwrap_or(Value::Null);
        if !(id.is_null() || id.is_string() || id.is_number()) {
            send(&self.out, &response(&Value::Null, Err(invalid_request("id must be a string or a number"))));
            return true;
        }
        let Some(method) = msg.get("method").and_then(Value::as_str).map(str::to_string) else {
            send(&self.out, &response(&id, Err(invalid_request("missing method"))));
            return true;
        };
        let args = msg.get("params").cloned().unwrap_or_else(|| json!({}));

        let Some(m) = registry().get(&method) else {
            self.reply(&id, Err(RpcError::method_not_found(&method)));
            return true;
        };
        if m.kind != Kind::Control {
            self.spawn(id, method, args);
            return true;
        }
        // Methods about the server itself are answered here, at once; the rest run on a worker.
        let result = match m.name {
            "initialize" => ok(api::Initialized {
                protocol: PROTOCOL,
                version: env!("CARGO_PKG_VERSION").into(),
                capabilities: capabilities(),
                methods: registry().names().into_iter().map(String::from).collect(),
            }),
            "rpc.describe" => params::<api::DescribeRequest>(args).and_then(|req| {
                contract::describe(req.method.as_deref())
                    .ok_or_else(|| invalid_params(format!("there's no method {:?}", req.method.unwrap_or_default())))
            }),
            "$/cancel" => params::<api::CancelRequest>(args).and_then(|req| {
                let cancelled = self.jobs.cancel(&req.id, &self.out, |job| {
                    let finished = api::JobEvent::Finished {
                        id: job.id.clone(),
                        method: job.method.clone(),
                        ok: false,
                        kind: Some("cancelled".into()),
                    };
                    self.events.send(&self.out, api::Notice::Job(finished));
                });
                ok(api::Cancelled { cancelled })
            }),
            "jobs.list" => ok(self.jobs.list()),
            "events.subscribe" => params::<api::Subscribe>(args)
                .and_then(|req| ok(self.events.subscribe(req, &self.store_root, &self.out))),
            "events.unsubscribe" => ok(api::Unsubscribed { unsubscribed: self.events.unsubscribe() }),
            "watch.start" => params::<api::StartWatch>(args).and_then(|req| self.start_watch(req)),
            "watch.stop" => params::<api::StopWatch>(args)
                .and_then(|req| ok(api::WatchStopped { stopped: self.stop_watch(req.cancel) })),
            "shutdown" => match params::<api::Shutdown>(args) {
                Ok(req) => {
                    self.jobs.stop(req.cancel);
                    self.reply(&id, Ok(Value::Null));
                    return false;
                }
                Err(e) => Err(e),
            },
            other => Err(RpcError::method_not_found(other)),
        };
        self.reply(&id, result);
        true
    }

    /// Starts watching drives on its own thread, pulling them (see [`api::Watching`]) and sending
    /// a `watch.*` event for each thing it does. It's listed by `jobs.list` (method `watch`), stops
    /// on `watch.stop` or shutdown, and is cancelled like a request.
    fn start_watch(&self, req: api::StartWatch) -> Result<Value, RpcError> {
        let mut slot = self.watch.lock().unwrap_or_else(PoisonError::into_inner);
        if slot.is_some() {
            return Err(RpcError::new(FAILED, "conflict", "already watching; watch.stop first"));
        }
        let options = api::WatchOptions {
            pull: req.pull,
            sync_set: req.sync_group,
            settle: req.settle_ms.map_or(api::WATCH_SETTLE, Duration::from_millis),
        };
        let mut watching = api::Watching::new(VolumeWatcher::new(), options);
        if let Some(ms) = req.interval_ms {
            watching.every = Duration::from_millis(ms.max(100));
        }
        let cancel = CancelToken::new();
        let job =
            Job { id: Value::Null, method: "watch".into(), started: Utc::now(), cancel: cancel.clone(), queued: false };
        let started = job.started;
        let key = self.jobs.start(job)?;
        let event = api::JobEvent::Started { id: Value::Null, method: "watch".into(), started };
        self.events.send(&self.out, api::Notice::Job(event));
        let stop = Arc::new(AtomicBool::new(false));
        *slot = Some(Watch { stop: stop.clone(), cancel: cancel.clone() });
        let (jobs, out, events, store_root, watch) =
            (self.jobs.clone(), self.out.clone(), self.events.clone(), self.store_root.clone(), self.watch.clone());
        std::thread::spawn(move || {
            let ctx = Ctx::new(&locallibrary::progress::NoProgress, cancel);
            let stopped = || stop.load(Ordering::SeqCst) || jobs.is_stopping();
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                watching.run(&store_root, &ctx, &stopped, &mut |w| {
                    if let api::Watched::Did(action) = w {
                        events.send_watch(&out, api::Notice::Watched(action.clone()));
                    }
                })
            }));
            *watch.lock().unwrap_or_else(PoisonError::into_inner) = None;
            let ok = matches!(result, Ok(Ok(())));
            let finished = api::JobEvent::Finished { id: Value::Null, method: "watch".into(), ok, kind: None };
            events.send(&out, api::Notice::Job(finished));
            jobs.finish(key, &out, None);
        });
        ok(api::WatchStarted { watching: true })
    }

    /// Stops watching after what it's doing (with `cancel`, part way through it). False when
    /// it wasn't watching.
    fn stop_watch(&self, cancel: bool) -> bool {
        let slot = self.watch.lock().unwrap_or_else(PoisonError::into_inner);
        let Some(w) = slot.as_ref() else { return false };
        w.stop.store(true, Ordering::SeqCst);
        if cancel {
            w.cancel.cancel();
        }
        true
    }

    /// Answers a request; a notification gets no response.
    fn reply(&self, id: &Value, result: Result<Value, RpcError>) {
        if !id.is_null() {
            send(&self.out, &response(id, result));
        }
    }

    /// Runs a request on its own thread.
    fn spawn(&self, id: Value, method: String, params: Value) {
        let cancel = CancelToken::new();
        let progress = RpcProgress::new(id.clone(), self.out.clone());
        PROGRESS.watch(&progress.0);
        let job =
            Job { id: id.clone(), method: method.clone(), started: Utc::now(), cancel: cancel.clone(), queued: true };
        let started = job.started;
        let key = match self.jobs.start(job) {
            Ok(key) => key,
            Err(e) => return self.reply(&id, Err(e)),
        };
        let event = api::JobEvent::Started { id: id.clone(), method: method.clone(), started };
        self.events.send(&self.out, api::Notice::Job(event));
        let (jobs, out, store_root, events, plans) =
            (self.jobs.clone(), self.out.clone(), self.store_root.clone(), self.events.clone(), self.plans.clone());
        let m = registry().get(&method).expect("only registered methods are spawned");
        let pool = if m.quick { &self.reads } else { &self.work };
        pool.run(Box::new(move || {
            // Cancelled while it waited: it has answered already.
            if !jobs.begin(key) {
                return;
            }
            let ctx = Ctx::new(&progress, cancel);
            // Workers are reused: this request's permission, and no other's.
            locallibrary::op::allow_archive_writes(params.get("overwrite_archive") == Some(&Value::Bool(true)));
            // What was asked, for the actions an error offers.
            let asked = params.clone();
            // A bug must not leave the client waiting forever for a response.
            let run = || {
                // Cancelled while it waited (shutting down): it doesn't start.
                ctx.check().map_err(locallibrary::Error::from)?;
                let call = Call { root: &store_root, plans: &plans, ctx: &ctx };
                m.run(&call, params).unwrap_or_else(|| Err(RpcError::method_not_found(&method)))
            };
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(run)).unwrap_or_else(|_| {
                Err(RpcError::new(INTERNAL, "internal", format!("internal error while handling {method:?}")))
            });
            let result = result.map_err(|e| e.resolve(&method, &asked));
            if !m.changes_no_drive() {
                LIBRARIES.clear();
            }
            progress.finish();
            let finished = api::JobEvent::Finished {
                id: id.clone(),
                method: method.clone(),
                ok: result.is_ok(),
                kind: result.as_ref().err().map(|e| e.kind.to_string()),
            };
            events.send(&out, api::Notice::Job(finished));
            // A notification gets no response.
            jobs.finish(key, &out, (!id.is_null()).then(|| response(&id, result)).as_ref());
        }));
    }
}

/// What the methods about plugins are named with: their plans are kept apart (see [`Plans`]).
const PLUGINS: &str = "plugins.";

/// The namespaces of the sidecar's methods: `drive` for `drive.info`.
pub fn namespaces() -> Vec<&'static str> {
    registry().names().into_iter().filter_map(|n| n.split('.').next()).collect()
}

/// Runs the read method `method` on this thread, as a worker would: plugins' callbacks (see
/// [`crate::plugins`]). Any other method is refused.
pub fn read(method: &str, params: Value, ctx: &Ctx<'_>) -> Result<Value, RpcError> {
    // Reads keep no plans.
    static NONE: Plans = Plans::new();
    let m = registry().get(method).filter(|m| m.kind == Kind::Read && !m.hidden);
    let m = m.ok_or_else(|| RpcError::method_not_found(method))?;
    // The store the host started the plugin in (see `Reads::read`), else the process's.
    let root = api::store_root().unwrap_or_else(locallibrary::Store::default_root);
    let call = Call { root: &root, plans: &NONE, ctx };
    m.run(&call, params).unwrap_or_else(|| Err(RpcError::method_not_found(method)))
}

fn response(id: &Value, result: Result<Value, RpcError>) -> Value {
    match result {
        Ok(v) => json!({ "jsonrpc": "2.0", "id": id, "result": v }),
        Err(e) => {
            json!({ "jsonrpc": "2.0", "id": id, "error": { "code": e.code, "message": e.message, "data": e.data() } })
        }
    }
}

#[cfg(test)]
mod tests;
