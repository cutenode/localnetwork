//! A plugin's process: started on first use, shaking hands, answering its callbacks, stopped
//! when idle, started again after a crash, and killed when it doesn't stop as asked.

use std::cell::Cell;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock, PoisonError, Weak};
use std::time::{Duration, Instant};

use indexmap::IndexMap;
use loclib_plugin::framing::{Line, read_line};
use loclib_plugin::{
    Cancel, Context, FEATURES, Hello, HelloReply, HostInfo, Manifest, PROTOCOL_VERSION, Partial, Peer, ProgressReport,
    Progressed, RpcError, check_protocol, code, method,
};
use rustc_hash::FxHashMap;
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use super::blobs::{Scratch, Scratches};
use super::events::Queue;
use super::services::Running;
use super::{Launch, Options, Reads, callbacks, net};
use crate::device::Device;
use crate::error::IoContext;
use crate::progress::{Ctx, Event};
use crate::store::Store;
use crate::{Error, Result, api, lock};

/// How often a caller waiting for a plugin passes its progress on and looks for being cancelled.
const TICK: Duration = Duration::from_millis(50);

/// The most items one `progress` event may stand for: one event is passed on that many times.
const MAX_ITEMS: u64 = 10_000;

thread_local! {
    /// Whether this thread counts as elevated (`None`: as the process is), for tests.
    pub(crate) static THREAD_ELEVATED: Cell<Option<bool>> = const { Cell::new(None) };
}

/// What every plugin is hosted with.
pub(crate) struct Host {
    pub(crate) reads: Arc<dyn Reads>,
    pub(crate) options: Options,
    /// When each host a `[[net]]` limit names lets a request go next, whichever plugin sends it.
    pub(crate) turns: Mutex<FxHashMap<String, Instant>>,
}

impl Host {
    /// Logs `line` as the plugin `id`'s.
    pub(crate) fn log(&self, id: &str, line: &str) {
        (self.options.log)(id, line);
    }
}

/// A plugin, whichever of its processes runs: what its callbacks hold, which keeps none of
/// them running.
pub(crate) struct Plugin {
    pub(crate) manifest: Manifest,
    pub(crate) host: Arc<Host>,
    /// Its `net/fetch` requests' client and secrets, whichever process makes them.
    pub(crate) net: Arc<net::State>,
}

/// One plugin, and its process while it runs. Only the host and the hooks under way (an event
/// being sent and a service running too) hold it; a plan kept, the plugin's callbacks and the
/// idle reaper never do: once the host lets go of it, a plan kept is stale.
pub(crate) struct Slot {
    pub(crate) plugin: Arc<Plugin>,
    launch: Launch,
    /// What the idle reaper holds instead, while it stops the process.
    hosting: Arc<Hosting>,
    /// Its events waiting to be sent.
    pub(crate) events: Arc<Queue>,
    /// Its services running, by name.
    pub(crate) services: Mutex<IndexMap<String, Running>>,
}

/// A slot's process, and its starting and stopping.
#[derive(Default)]
struct Hosting {
    /// The process while it runs. Only ever locked for a moment.
    process: Mutex<Option<Arc<Process>>>,
    /// Held while a process starts (and shakes hands) or stops, so one does at a time: whoever
    /// stops it next waits for a stop under way, as a host exiting must.
    changing: Mutex<()>,
}

/// A slot is its plugin, hosted: what's the plugin's is read through it.
impl std::ops::Deref for Slot {
    type Target = Plugin;

    fn deref(&self) -> &Plugin {
        &self.plugin
    }
}

/// What a process's callbacks need besides its plugin.
pub(crate) struct Live {
    /// The features it and the host both have, once it has shaken hands.
    features: OnceLock<Vec<String>>,
    /// The store it was started in, for its settings and secrets.
    pub(crate) store: Option<Store>,
    /// Its requests' scratch folders.
    pub(crate) scratches: Arc<Scratches>,
}

impl Live {
    /// A process's, started in `store`, before it has shaken hands.
    pub(crate) fn new(store: Option<Store>) -> Live {
        Live { features: OnceLock::new(), store, scratches: Arc::default() }
    }

    /// Whether it has the feature `name` (and the host does).
    pub(crate) fn has(&self, name: &str) -> bool {
        self.features.get().is_some_and(|f| f.iter().any(|f| f == name))
    }

    /// Whether it may send the host `method`: a feature it has lets it
    /// ([`loclib_plugin::Feature::sends`]).
    pub(crate) fn sends(&self, method: &str) -> bool {
        FEATURES.iter().any(|f| f.sends.contains(&method) && self.has(f.name))
    }
}

/// A plugin's process, once it has shaken hands. Dropping the last of it stops it. Only its slot
/// and the hooks using it hold it, never a thread of the host's (the idle reaper reads its
/// [`Activity`], and whether it runs is asked through the slot): whoever takes it out of the slot
/// is the last to let go of it, unless a hook under way is, so waits for it to stop.
pub(crate) struct Process {
    child: Mutex<Child>,
    peer: Peer,
    activity: Arc<Activity>,
    /// It closed its output (it exited or crashed), or was killed.
    dead: Arc<AtomicBool>,
    grace: Duration,
    live: Arc<Live>,
    /// Where its requests' scratch folders go.
    scratch: PathBuf,
    /// Its folder when there's no store, removed once it has stopped.
    _temp: Option<tempfile::TempDir>,
}

/// How busy a process is, as the idle reaper reads it.
struct Activity {
    /// Hooks under way.
    busy: AtomicUsize,
    last_used: Mutex<Instant>,
}

impl Process {
    /// Whether it has stopped: it closed its output, was killed, or exited.
    fn stopped(&self) -> bool {
        self.dead.load(Ordering::SeqCst) || lock(&self.child).try_wait().is_ok_and(|exited| exited.is_some())
    }
}

impl Drop for Process {
    /// Closes its input, which a plugin stops at once it has answered what it was doing, and
    /// kills it if it hasn't within its grace. Closing waits for a write under way, which a
    /// plugin that stopped reading holds up until it's killed: it's done on a thread of its own.
    fn drop(&mut self) {
        let peer = self.peer.clone();
        std::thread::spawn(move || peer.close());
        let child = self.child.get_mut().unwrap_or_else(PoisonError::into_inner);
        let until = Instant::now() + self.grace;
        while matches!(child.try_wait(), Ok(None)) && Instant::now() < until {
            std::thread::sleep(Duration::from_millis(10));
        }
        let _ = child.kill();
        let _ = child.wait();
    }
}

/// A hook under way, keeping its process from being stopped as idle.
struct Busy<'a>(&'a Activity);

impl<'a> Busy<'a> {
    fn new(p: &'a Process) -> Busy<'a> {
        p.activity.busy.fetch_add(1, Ordering::SeqCst);
        Busy(&p.activity)
    }
}

impl Drop for Busy<'_> {
    fn drop(&mut self) {
        *lock(&self.0.last_used) = Instant::now();
        self.0.busy.fetch_sub(1, Ordering::SeqCst);
    }
}

/// A debug build with this set counts as not elevated, for CI's Windows runners, which run as an
/// administrator; a release never reads it.
#[cfg(debug_assertions)]
const TEST_NOT_ELEVATED: &str = "LOCALLIBRARY_TEST_NOT_ELEVATED";

/// Whether this process runs elevated (as root, or as an administrator with Windows' elevation).
fn elevated() -> bool {
    THREAD_ELEVATED.with(Cell::get).unwrap_or_else(|| {
        #[cfg(debug_assertions)]
        if std::env::var_os(TEST_NOT_ELEVATED).is_some() {
            return false;
        }
        process_elevated()
    })
}

#[cfg(unix)]
fn process_elevated() -> bool {
    // SAFETY: geteuid has no preconditions and can't fail.
    unsafe { libc::geteuid() == 0 }
}

#[cfg(windows)]
fn process_elevated() -> bool {
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::Security::{GetTokenInformation, TOKEN_ELEVATION, TOKEN_QUERY, TokenElevation};
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
    let mut token: HANDLE = std::ptr::null_mut();
    // SAFETY: the token is this process's own, opened to query and closed once queried; the
    // elevation is read into a value of the size given.
    unsafe {
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return false;
        }
        let mut elevation = TOKEN_ELEVATION { TokenIsElevated: 0 };
        let mut size = 0;
        let size_of = std::mem::size_of::<TOKEN_ELEVATION>() as u32;
        let read = GetTokenInformation(token, TokenElevation, (&raw mut elevation).cast(), size_of, &mut size);
        CloseHandle(token);
        read != 0 && elevation.TokenIsElevated != 0
    }
}

/// `line` without terminal escape sequences or other control characters (but tabs), which would
/// draw on a terminal UI's screen.
pub(crate) fn printable(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        match c {
            '\u{1b}' => match chars.next() {
                // A control sequence: parameters, then a final character.
                Some('[') => {
                    let _ = chars.by_ref().find(|c| ('@'..='~').contains(c));
                }
                // A string (an OSC title, a hyperlink...), up to BEL or ESC \. The backslash is
                // part of the end only after ESC: after BEL it's text.
                Some(']' | 'P' | 'X' | '^' | '_') => {
                    let end = chars.by_ref().find(|c| matches!(c, '\u{7}' | '\u{1b}'));
                    if end == Some('\u{1b}') && chars.as_str().starts_with('\\') {
                        chars.next();
                    }
                }
                // Two characters.
                _ => {}
            },
            '\t' => out.push(c),
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
    out
}

impl Plugin {
    /// How errors name it: "the Echo plugin".
    fn label(&self) -> String {
        format!("the {} plugin", self.manifest.name)
    }

    /// An error for something the plugin did wrong.
    pub(crate) fn misbehaved(&self, message: impl Into<String>) -> Error {
        Error::External { command: self.label(), message: message.into() }
    }
}

impl Slot {
    pub(crate) fn new(manifest: Manifest, launch: Launch, host: Arc<Host>) -> Slot {
        let plugin = Arc::new(Plugin { manifest, host, net: Arc::default() });
        Slot { plugin, launch, hosting: Arc::default(), events: Arc::default(), services: Mutex::default() }
    }

    /// The plugin's error as locallibrary's, of the kind it says (`not_found`, `invalid`,
    /// `conflict`) with the fields it gives, naming the plugin: a front end handles it as it
    /// handles locallibrary's own. `io` and `parse` are about the file `fields.path` names.
    /// Another kind (or one of those without a file) is the plugin failing, `permission` (a
    /// plugin never runs elevated, so running locallibrary so can't help) and `stale_plan` (only
    /// the host's plans go stale) too. Its connection closing means it stopped. The secrets the
    /// host filled in for it are left out.
    fn error(&self, p: &Process, mut e: RpcError) -> Error {
        e.message = self.net.scrub(&e.message);
        let said = format!("{} ({})", e.message, self.label());
        let mut fields = e.data.as_ref().map(|d| d.fields.clone()).unwrap_or_default();
        fields.values_mut().for_each(|v| self.net.scrub_value(v));
        let file = fields.get("path").and_then(|p| p.as_str()).map(PathBuf::from);
        let error = match (e.kind(), file) {
            (Some("cancelled"), _) => return Error::Cancelled,
            (Some("closed"), _) => {
                p.dead.store(true, Ordering::SeqCst);
                return self.misbehaved("it stopped unexpectedly");
            }
            _ if e.code == code::INVALID_PARAMS => Error::Invalid(said),
            (Some("invalid"), _) => Error::Invalid(said),
            (Some("not_found"), _) => Error::NotFound(said),
            (Some("conflict"), _) => Error::Conflict(said),
            (Some("io"), Some(path)) => Error::Io { path, source: std::io::Error::other(said) },
            (Some("parse"), Some(path)) => Error::Parse { path, message: said },
            _ => return self.misbehaved(e.message),
        };
        fields.into_iter().fold(error, |error, (key, value)| error.with(&key, value))
    }

    /// The process, if it's running. Never waits.
    fn running(&self) -> Option<Arc<Process>> {
        lock(&self.hosting.process).clone().filter(|p| !p.stopped())
    }

    /// Whether it's running, without holding the process: a caller that did could be left the
    /// last to let go of it, after whoever stopped it was told it had.
    pub(crate) fn is_running(&self) -> bool {
        lock(&self.hosting.process).as_ref().is_some_and(|p| !p.stopped())
    }

    /// The process, started (and shaken hands with) if it isn't running: in its folder in
    /// `store` (see [`Slot::spawn`]).
    pub(crate) fn start(self: &Arc<Self>, store: Option<&Store>) -> Result<Arc<Process>> {
        if let Some(p) = self.running() {
            return Ok(p);
        }
        let _one = lock(&self.hosting.changing);
        // Another caller may have started it meanwhile.
        if let Some(p) = self.running() {
            return Ok(p);
        }
        let p = Arc::new(self.spawn(store)?);
        let features = self.handshake(&p)?;
        let _ = p.live.features.set(features);
        let (hosting, process, idle) = (Arc::downgrade(&self.hosting), Arc::downgrade(&p), self.host.options.idle);
        let (activity, dead) = (p.activity.clone(), p.dead.clone());
        std::thread::spawn(move || reap(&hosting, &process, &activity, &dead, idle));
        // One that stopped is waited for as it's dropped, once the lock is let go.
        let stopped = lock(&self.hosting.process).replace(p.clone());
        drop(stopped);
        Ok(p)
    }

    /// Stops the process, if it's running (see [`Process`]'s drop), or waits for it to stop if
    /// that's under way.
    pub(crate) fn stop(&self) {
        let _one = lock(&self.hosting.changing);
        let stopped = lock(&self.hosting.process).take();
        drop(stopped);
    }

    /// Starts the process in a folder of its own, `work/` in the plugin's folder (made if it isn't
    /// there): `<store>/plugins/<id>/`, or without a store a new one in the temporary folder,
    /// removed once it stops. The plugin's own files go there, and its relative paths are never
    /// another program's, nor the host's files about it (beside that folder, as its requests'
    /// scratch folders are, in `scratch/`).
    fn spawn(self: &Arc<Self>, store: Option<&Store>) -> Result<Process> {
        if elevated() {
            return Err(Error::invalid(format!(
                "{} isn't started from a process running as an administrator (or root): run locallibrary as yourself",
                self.label()
            )));
        }
        let (base, temp) = match store {
            Some(store) => (store.root().join("plugins").join(&self.manifest.id), None),
            None => {
                let temp = std::env::temp_dir();
                let made = tempfile::Builder::new()
                    .prefix(&format!("locallibrary-plugin-{}-", self.manifest.id))
                    .tempdir_in(&temp)
                    .at(&temp)?;
                (made.path().to_path_buf(), Some(made))
            }
        };
        let dir = base.join("work");
        std::fs::create_dir_all(&dir).at(&dir)?;
        let mut command = Command::new(&self.launch.program);
        command.current_dir(&dir);
        for var in &self.launch.env_remove {
            command.env_remove(var);
        }
        let mut child = command
            .args(&self.launch.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| self.misbehaved(format!("it couldn't be started: {e}")))?;
        let (Some(input), Some(output), Some(stderr)) = (child.stdin.take(), child.stdout.take(), child.stderr.take())
        else {
            unreachable!("the child's stdio are pipes");
        };
        let (host, id, net) = (self.host.clone(), self.manifest.id.clone(), self.net.clone());
        std::thread::spawn(move || {
            let (mut stderr, mut buf) = (BufReader::new(stderr), Vec::new());
            while let Ok(Line::Read | Line::TooLong) = read_line(&mut stderr, &mut buf) {
                host.log(&id, net.scrub(&printable(&String::from_utf8_lossy(&buf))).trim_end());
            }
        });
        let (host, id, net) = (self.host.clone(), self.manifest.id.clone(), self.net.clone());
        // What it sent that couldn't be read can quote it.
        let (peer, incoming) = Peer::with_log(output, input, move |line| host.log(&id, &net.scrub(line)));
        let dead = Arc::new(AtomicBool::new(false));
        let live = Arc::new(Live::new(store.cloned()));
        let (answering, ended, slot, answered) = (peer.clone(), dead.clone(), Arc::downgrade(self), live.clone());
        let plugin = self.plugin.clone();
        std::thread::spawn(move || {
            callbacks::answer(&slot, &plugin, &answered, &answering, incoming);
            ended.store(true, Ordering::SeqCst);
        });
        let grace = self.host.options.grace;
        let activity = Arc::new(Activity { busy: AtomicUsize::new(0), last_used: Mutex::new(Instant::now()) });
        let child = Mutex::new(child);
        let scratch = base.join("scratch");
        Ok(Process { child, peer, activity, dead, grace, live, scratch, _temp: temp })
    }

    /// `hello`, then `describe`: it must speak the protocol, be the plugin its manifest says, and
    /// have every feature what it contributes needs. Answers the features both sides have.
    fn handshake(&self, p: &Process) -> Result<Vec<String>> {
        let m = &self.manifest;
        let hello = Hello {
            protocol: PROTOCOL_VERSION.into(),
            host: HostInfo { name: "locallibrary".into(), version: env!("CARGO_PKG_VERSION").into() },
            features: FEATURES.iter().map(|f| f.name.into()).collect(),
        };
        let reply: HelloReply = self.answered(p, method::HELLO, &hello)?;
        check_protocol(&reply.protocol).map_err(|e| self.misbehaved(e.message))?;
        if (reply.plugin.id.as_str(), reply.plugin.version.as_str()) != (m.id.as_str(), m.version.as_str()) {
            return Err(self.misbehaved(format!(
                "it says it's {} {}, not {} {}",
                reply.plugin.id, reply.plugin.version, m.id, m.version
            )));
        }
        let described: Manifest = self.answered(p, method::DESCRIBE, &json!({}))?;
        if described != *m {
            return Err(self.misbehaved("it describes itself otherwise than the manifest locallibrary has for it"));
        }
        // Of those both sides have: a feature only the plugin names is one this host never calls.
        let lacks = FEATURES.iter().find(|f| (f.contributed)(m) && !reply.features.iter().any(|n| n == f.name));
        if let Some(f) = lacks {
            return Err(
                self.misbehaved(format!("it says it doesn't have the {} feature, which its manifest needs", f.name))
            );
        }
        let both = FEATURES.iter().filter(|f| reply.features.iter().any(|n| n == f.name));
        Ok(both.map(|f| f.name.to_string()).collect())
    }

    /// A handshake request's answer, within [`Options::handshake`].
    fn answered<R: DeserializeOwned>(&self, p: &Process, method: &str, params: &impl Serialize) -> Result<R> {
        let pending = p.peer.send_request(method, params).map_err(|e| self.error(p, e))?;
        match pending.wait_timeout(self.host.options.handshake) {
            Some(answer) => answer.map_err(|e| self.error(p, e)),
            None => Err(self.misbehaved(format!("it didn't answer {method} in time"))),
        }
    }

    /// Calls the hook `method` with `params`, starting the plugin (in `store`) if it isn't running.
    /// Its progress goes to `ctx`; cancelling `ctx` asks it to stop, and kills it if it hasn't
    /// answered within its grace.
    pub(crate) fn call<R: DeserializeOwned>(
        self: &Arc<Self>,
        method: &str,
        params: &impl Serialize,
        store: Option<&Store>,
        ctx: &Ctx<'_>,
    ) -> Result<R> {
        self.call_lending(method, params, &[], store, ctx)
    }

    /// [`Slot::call`], lending the hook the files `lent` (absolute, as its params name them, to
    /// read): its `net/fetch` may send them until it answers, the bytes sent its progress.
    pub(crate) fn call_lending<R: DeserializeOwned>(
        self: &Arc<Self>,
        method: &str,
        params: &impl Serialize,
        lent: &[&Path],
        store: Option<&Store>,
        ctx: &Ctx<'_>,
    ) -> Result<R> {
        self.call_giving(method, params, &[], lent, store, ctx, &|_| {}, |answer, _, _| Ok(answer))
    }

    /// [`Slot::call`], the hook's `params.context` given a scratch folder of its own (with the
    /// `blobs` feature), the view's rows it sends in parts shown with `shown` as they come, and
    /// its answer read by `read` with the files it left in that folder (see [`Scratch::blob`]),
    /// which is removed after, and what the process has (its features). A folder holding more
    /// than [`Options::max_blobs`] fails the call.
    pub(crate) fn call_with<R: DeserializeOwned, T>(
        self: &Arc<Self>,
        method: &str,
        params: &impl Serialize,
        store: Option<&Store>,
        ctx: &Ctx<'_>,
        shown: &dyn Fn(Partial),
        read: impl FnOnce(R, &Scratch, &Live) -> Result<T>,
    ) -> Result<T> {
        self.call_giving(method, params, &[], &[], store, ctx, shown, read)
    }

    /// [`Slot::call_with`], the files `given` (by name, their bytes) written to the scratch
    /// folder first, for the request to name: it fails without one. And the files `lent` (see
    /// [`Slot::call_lending`]).
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn call_giving<R: DeserializeOwned, T>(
        self: &Arc<Self>,
        method: &str,
        params: &impl Serialize,
        given: &[(&str, &[u8])],
        lent: &[&Path],
        store: Option<&Store>,
        ctx: &Ctx<'_>,
        shown: &dyn Fn(Partial),
        read: impl FnOnce(R, &Scratch, &Live) -> Result<T>,
    ) -> Result<T> {
        ctx.check()?;
        let p = self.start(store)?;
        let _busy = Busy::new(&p);
        let scratch = match p.live.has("blobs") {
            true => p.live.scratches.make(&p.scratch, lent)?,
            false => p.live.scratches.none(lent),
        };
        for (name, bytes) in given {
            let dir = scratch
                .dir()
                .ok_or_else(|| self.misbehaved("it has no scratch folder for files (the blobs feature)"))?;
            let path = dir.join(name);
            std::fs::write(&path, bytes).at(&path)?;
        }
        let mut params = serde_json::to_value(params).map_err(|e| Error::invalid(e.to_string()))?;
        if let Some(dir) = scratch.dir() {
            params["context"]["scratch"] = Value::from(dir.to_string_lossy().into_owned());
        }
        let pending = p.peer.send_request_reporting(method, &params).map_err(|e| self.error(&p, e))?;
        if let Some(reporter) = pending.reporter() {
            scratch.report_to(reporter);
        }
        let mut cancelled: Option<Instant> = None;
        loop {
            let answer = pending.wait_timeout(TICK);
            pending.progress().for_each(|report| pass_on(report, ctx));
            pending.partials().filter(|_| p.live.sends(method::PARTIAL)).for_each(shown);
            if let Some(answer) = answer {
                let answer = answer.map_err(|e| self.error(&p, e))?;
                let (size, most) = (scratch.size(), self.host.options.max_blobs);
                if size > most {
                    return Err(self.misbehaved(format!(
                        "it left {} in its scratch folder, more than the {} a request may",
                        crate::op::bytes(size),
                        crate::op::bytes(most)
                    )));
                }
                return read(answer, &scratch, &p.live);
            }
            if cancelled.is_none() && ctx.cancel.is_cancelled() {
                // From now: a plugin that isn't reading holds up the `$/cancel`, on a thread
                // of its own, but not its grace.
                cancelled = Some(Instant::now());
                let (peer, id) = (p.peer.clone(), pending.id());
                std::thread::spawn(move || peer.notify(method::CANCEL, &Cancel { id: id.into() }));
            }
            if cancelled.is_some_and(|at| at.elapsed() >= self.host.options.grace) {
                self.kill(&p);
                return Err(Error::Cancelled);
            }
        }
    }

    /// Sends the plugin the notification `method`, if it has the feature `feature`, starting it
    /// (in `store`) if it isn't running: it's been used then, for the idle reaper.
    pub(crate) fn notify(
        self: &Arc<Self>,
        feature: &str,
        method: &str,
        params: &impl Serialize,
        store: Option<&Store>,
    ) -> Result<()> {
        let p = self.start(store)?;
        if p.live.has(feature) {
            let _busy = Busy::new(&p);
            p.peer.notify(method, params);
        }
        Ok(())
    }

    /// Kills the process: a request it was asked to cancel wasn't answered within its grace.
    fn kill(&self, p: &Arc<Process>) {
        let grace = self.host.options.grace;
        self.host.log(&self.manifest.id, &format!("killed: it didn't stop within {grace:?} of being asked to"));
        p.dead.store(true, Ordering::SeqCst);
        let _ = lock(&p.child).kill();
        // Its `net/fetch` requests too, now: its output may close only later.
        p.peer.cancel_running();
        let mut running = lock(&self.hosting.process);
        let stopped = running.take_if(|r| Arc::ptr_eq(r, p));
        drop(running);
        drop(stopped);
    }

    /// `context` as the plugin gets it: its drive (by id, path or name) named by id, else name,
    /// never a path, and the plugin's settings. With the drive.
    pub(crate) fn context(&self, mut context: Context, store: Option<&Store>) -> Result<(Context, Option<Device>)> {
        let device = context.drive.as_deref().map(api::resolve_drive).transpose()?;
        context.drive = device.as_ref().map(|d| d.id().map_or_else(|| d.name(), |id| id.to_string()));
        context.settings = self.settings(store);
        Ok((context, device))
    }
}

/// Passes a plugin's progress on: events a newer plugin has and step announcements (whose names
/// are the plugin's own) are skipped, and an item standing for several goes on as that many.
fn pass_on(report: ProgressReport, ctx: &Ctx<'_>) {
    let ProgressReport::Known(p) = report else { return };
    let Progressed::Event(event) = p.event else { return };
    let times = match event {
        Event::Item { .. } => p.items.unwrap_or(1).min(MAX_ITEMS),
        _ => 1,
    };
    let emit = |ctx: &Ctx<'_>| (0..times).for_each(|_| ctx.emit(event.clone()));
    match p.target {
        Some(target) => ctx.for_target(target, emit),
        None => emit(ctx),
    }
}

/// Stops the process once it has been idle for `idle`, unless it has stopped already or the
/// plugin has started another since. It reads the process's `activity` and whether it's `dead`,
/// but never holds it: holding it as another caller stopped it would leave this thread to stop
/// it, after that caller was told it had. Nor does it hold the slot, only its [`Hosting`]: a host
/// letting go of the plugin as it stops the process lets go of the slot.
fn reap(hosting: &Weak<Hosting>, process: &Weak<Process>, activity: &Activity, dead: &AtomicBool, idle: Duration) {
    let idle_for = || match activity.busy.load(Ordering::SeqCst) {
        0 => idle.saturating_sub(lock(&activity.last_used).elapsed()),
        _ => idle,
    };
    loop {
        if dead.load(Ordering::SeqCst) || process.strong_count() == 0 {
            return;
        }
        let wait = idle_for();
        if !wait.is_zero() {
            std::thread::sleep(wait);
            continue;
        }
        let Some(hosting) = hosting.upgrade() else { return };
        let one = lock(&hosting.changing);
        let mut running = lock(&hosting.process);
        if !running.as_ref().is_some_and(|r| std::ptr::eq(Arc::as_ptr(r), process.as_ptr())) {
            return;
        }
        // Checked again, now that nothing else can take it out.
        if idle_for().is_zero() {
            // Stopped once nothing else holds it (a hook that has just started still can), while
            // `changing` is held.
            let stopped = running.take();
            drop(running);
            drop(stopped);
            drop(one);
            return;
        }
    }
}

#[cfg(test)]
mod tests;
