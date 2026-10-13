//! A plugin's process: started on first use, shaking hands, answering its callbacks, stopped
//! when idle, started again after a crash, and killed when it doesn't stop as asked.

use std::cell::Cell;
use std::io::BufReader;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError, Weak};
use std::time::{Duration, Instant};

use loclib_plugin::framing::{Line, read_line};
use loclib_plugin::{
    Cancel, Context, Hello, HelloReply, HostInfo, Manifest, PROTOCOL_VERSION, Peer, ProgressReport, Progressed,
    RpcError, check_protocol, code, method,
};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::json;

use super::{Launch, Options, Reads, callbacks};
use crate::device::Device;
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
}

impl Host {
    /// Logs `line` as the plugin `id`'s.
    pub(crate) fn log(&self, id: &str, line: &str) {
        (self.options.log)(id, line);
    }
}

/// One plugin, and its process while it runs.
pub(crate) struct Slot {
    pub(crate) manifest: Manifest,
    launch: Launch,
    pub(crate) host: Arc<Host>,
    /// The process while it runs. Only ever locked for a moment.
    process: Mutex<Option<Arc<Process>>>,
    /// Held while a process starts and shakes hands, so one starts at a time.
    starting: Mutex<()>,
}

/// A plugin's process, once it has shaken hands. Dropping the last of it stops it.
pub(crate) struct Process {
    child: Mutex<Child>,
    peer: Peer,
    /// Hooks under way.
    busy: AtomicUsize,
    last_used: Mutex<Instant>,
    /// It closed its output (it exited or crashed), or was killed.
    dead: Arc<AtomicBool>,
    grace: Duration,
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
struct Busy<'a>(&'a Process);

impl<'a> Busy<'a> {
    fn new(p: &'a Process) -> Busy<'a> {
        p.busy.fetch_add(1, Ordering::SeqCst);
        Busy(p)
    }
}

impl Drop for Busy<'_> {
    fn drop(&mut self) {
        *lock(&self.0.last_used) = Instant::now();
        self.0.busy.fetch_sub(1, Ordering::SeqCst);
    }
}

/// Whether this process runs elevated (as root, or as an administrator with Windows' elevation).
fn elevated() -> bool {
    THREAD_ELEVATED.with(Cell::get).unwrap_or_else(process_elevated)
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
fn printable(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        match c {
            '\u{1b}' => match chars.next() {
                // A control sequence: parameters, then a final character.
                Some('[') => {
                    let _ = chars.by_ref().find(|c| ('@'..='~').contains(c));
                }
                // A string (an OSC title, a hyperlink...), up to BEL or ESC \.
                Some(']' | 'P' | 'X' | '^' | '_') => {
                    let _ = chars.by_ref().find(|c| matches!(c, '\u{7}' | '\u{1b}'));
                    if chars.as_str().starts_with('\\') {
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

impl Slot {
    pub(crate) fn new(manifest: Manifest, launch: Launch, host: Arc<Host>) -> Slot {
        Slot { manifest, launch, host, process: Mutex::new(None), starting: Mutex::new(()) }
    }

    /// How errors name it: "the Echo plugin".
    fn label(&self) -> String {
        format!("the {} plugin", self.manifest.name)
    }

    /// An error for something the plugin did wrong.
    pub(crate) fn misbehaved(&self, message: impl Into<String>) -> Error {
        Error::External { command: self.label(), message: message.into() }
    }

    /// The plugin's error as locallibrary's: cancelled and invalid params keep their kinds,
    /// the rest is the plugin failing. Its connection closing means it stopped.
    fn error(&self, p: &Process, e: RpcError) -> Error {
        match e.kind() {
            Some("cancelled") => Error::Cancelled,
            Some("closed") => {
                p.dead.store(true, Ordering::SeqCst);
                self.misbehaved("it stopped unexpectedly")
            }
            _ if e.code == code::INVALID_PARAMS => Error::Invalid(e.message),
            _ => self.misbehaved(e.message),
        }
    }

    /// The process, if it's running. Never waits.
    fn running(&self) -> Option<Arc<Process>> {
        lock(&self.process).clone().filter(|p| !p.stopped())
    }

    pub(crate) fn is_running(&self) -> bool {
        self.running().is_some()
    }

    /// The process, started (and shaken hands with) if it isn't running.
    pub(crate) fn start(self: &Arc<Self>) -> Result<Arc<Process>> {
        if let Some(p) = self.running() {
            return Ok(p);
        }
        let _one = lock(&self.starting);
        // Another caller may have started it meanwhile.
        if let Some(p) = self.running() {
            return Ok(p);
        }
        let p = Arc::new(self.spawn()?);
        self.handshake(&p)?;
        let (slot, process, idle) = (Arc::downgrade(self), Arc::downgrade(&p), self.host.options.idle);
        std::thread::spawn(move || reap(&slot, &process, idle));
        // One that stopped is waited for as it's dropped, once the lock is let go.
        let stopped = lock(&self.process).replace(p.clone());
        drop(stopped);
        Ok(p)
    }

    /// Stops the process, if it's running: see [`Process`]'s drop.
    pub(crate) fn stop(&self) {
        let stopped = lock(&self.process).take();
        drop(stopped);
    }

    fn spawn(&self) -> Result<Process> {
        if elevated() {
            return Err(Error::invalid(format!(
                "{} isn't started from a process running as an administrator (or root): run locallibrary as yourself",
                self.label()
            )));
        }
        let mut child = Command::new(&self.launch.program)
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
        let (host, id) = (self.host.clone(), self.manifest.id.clone());
        std::thread::spawn(move || {
            let (mut stderr, mut buf) = (BufReader::new(stderr), Vec::new());
            while let Ok(Line::Read | Line::TooLong) = read_line(&mut stderr, &mut buf) {
                host.log(&id, printable(&String::from_utf8_lossy(&buf)).trim_end());
            }
        });
        let (host, id) = (self.host.clone(), self.manifest.id.clone());
        let (peer, incoming) = Peer::with_log(output, input, move |line| host.log(&id, line));
        let dead = Arc::new(AtomicBool::new(false));
        let (answering, ended) = (peer.clone(), dead.clone());
        let (manifest, host) = (self.manifest.clone(), self.host.clone());
        std::thread::spawn(move || {
            callbacks::answer(&manifest, &host, &answering, incoming);
            ended.store(true, Ordering::SeqCst);
        });
        let grace = self.host.options.grace;
        let last_used = Mutex::new(Instant::now());
        Ok(Process { child: Mutex::new(child), peer, busy: AtomicUsize::new(0), last_used, dead, grace })
    }

    /// `hello`, then `describe`: it must speak the protocol, and be the plugin its manifest says.
    fn handshake(&self, p: &Process) -> Result<()> {
        let m = &self.manifest;
        let hello = Hello {
            protocol: PROTOCOL_VERSION.into(),
            host: HostInfo { name: "locallibrary".into(), version: env!("CARGO_PKG_VERSION").into() },
            features: Vec::new(),
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
        Ok(())
    }

    /// A handshake request's answer, within [`Options::handshake`].
    fn answered<R: DeserializeOwned>(&self, p: &Process, method: &str, params: &impl Serialize) -> Result<R> {
        let pending = p.peer.send_request(method, params).map_err(|e| self.error(p, e))?;
        match pending.wait_timeout(self.host.options.handshake) {
            Some(answer) => answer.map_err(|e| self.error(p, e)),
            None => Err(self.misbehaved(format!("it didn't answer {method} in time"))),
        }
    }

    /// Calls the hook `method` with `params`, starting the plugin if it isn't running. Its
    /// progress goes to `ctx`; cancelling `ctx` asks it to stop, and kills it if it hasn't
    /// answered within its grace.
    pub(crate) fn call<R: DeserializeOwned>(
        self: &Arc<Self>,
        method: &str,
        params: &impl Serialize,
        ctx: &Ctx<'_>,
    ) -> Result<R> {
        ctx.check()?;
        let p = self.start()?;
        let _busy = Busy::new(&p);
        let pending = p.peer.send_request_reporting(method, params).map_err(|e| self.error(&p, e))?;
        let mut cancelled: Option<Instant> = None;
        loop {
            let answer = pending.wait_timeout(TICK);
            pending.progress().for_each(|report| pass_on(report, ctx));
            if let Some(answer) = answer {
                return answer.map_err(|e| self.error(&p, e));
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

    /// Kills the process: a request it was asked to cancel wasn't answered within its grace.
    fn kill(&self, p: &Arc<Process>) {
        let grace = self.host.options.grace;
        self.host.log(&self.manifest.id, &format!("killed: it didn't stop within {grace:?} of being asked to"));
        p.dead.store(true, Ordering::SeqCst);
        let _ = lock(&p.child).kill();
        let mut running = lock(&self.process);
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
/// plugin has started another since.
fn reap(slot: &Weak<Slot>, process: &Weak<Process>, idle: Duration) {
    let idle_for = |p: &Process| match p.busy.load(Ordering::SeqCst) {
        0 => idle.saturating_sub(lock(&p.last_used).elapsed()),
        _ => idle,
    };
    loop {
        let Some(p) = process.upgrade().filter(|p| !p.dead.load(Ordering::SeqCst)) else { return };
        let wait = idle_for(&p);
        if !wait.is_zero() {
            drop(p);
            std::thread::sleep(wait);
            continue;
        }
        let Some(slot) = slot.upgrade() else { return };
        let mut running = lock(&slot.process);
        if !running.as_ref().is_some_and(|r| Arc::ptr_eq(r, &p)) {
            return;
        }
        // Checked again, now that nothing else can take it out.
        if idle_for(&p).is_zero() {
            // Stopped once nothing else holds it (a hook that has just started still can).
            let stopped = running.take();
            drop(running);
            drop(stopped);
            return;
        }
    }
}
