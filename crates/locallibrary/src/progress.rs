//! Progress reporting and cancellation shared by every long-running operation.
//!
//! Events are plain serde types so they can be forwarded verbatim to a UI process (see the
//! `locallibrary rpc` sidecar), rendered as progress bars by the CLI, or ignored.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError, RwLock, RwLockReadGuard, RwLockWriteGuard, TryLockError};

use schemars::JsonSchema;
use serde::Serialize;

use crate::{Error, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum Phase {
    Scan,
    Hash,
    Copy,
    Write,
    Delete,
    Verify,
    Format,
    /// Decoding and fingerprinting library tracks.
    Fingerprint,
    /// Building the fingerprint index.
    Index,
    /// Decoding and fingerprinting a mix.
    Listen,
    /// Looking for library tracks in a mix.
    Match,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(tag = "event", rename_all = "snake_case")]
#[non_exhaustive]
pub enum Event {
    /// A phase begins. `items`/`bytes` are totals when known up front (0 otherwise).
    PhaseStart {
        phase: Phase,
        items: u64,
        bytes: u64,
    },
    /// Work on one item (usually a file) begins.
    Item {
        phase: Phase,
        path: String,
        bytes: u64,
    },
    /// `bytes` more bytes were processed in the current phase.
    Bytes {
        phase: Phase,
        bytes: u64,
    },
    PhaseEnd {
        phase: Phase,
    },
    Warning {
        message: String,
    },
}

/// Which part of a larger operation an event belongs to: the drive, when several are written at
/// once ([`Ctx::for_target`]), and the step, in an operation made of steps ([`Steps`]).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, JsonSchema)]
pub struct Scope {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub step: Option<Step>,
}

impl Scope {
    /// `self` inside `outer`: what `self` leaves unset comes from `outer`.
    pub fn within(&self, outer: &Scope) -> Scope {
        Scope { target: self.target.clone().or_else(|| outer.target.clone()), step: self.step.or(outer.step) }
    }
}

/// Step `index` of `total`, counting from 1. `id` names it (`format`, `library`, `verify`, ...).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
pub struct Step {
    pub index: u32,
    pub total: u32,
    pub id: &'static str,
}

pub trait Progress: Send + Sync {
    fn on_event(&self, event: &Event);

    /// An event from inside a [`Scope`]. Unless overridden, the scope is dropped and the event
    /// goes to `on_event`.
    fn on_scoped_event(&self, _scope: &Scope, event: &Event) {
        self.on_event(event);
    }

    /// A step begins (`scope.step` says which). Ignored unless overridden.
    fn on_step(&self, _scope: &Scope) {}
}

/// Marks events with a scope on their way to `inner`.
struct Scoped<'a> {
    inner: &'a dyn Progress,
    scope: Scope,
}

impl Progress for Scoped<'_> {
    fn on_event(&self, event: &Event) {
        self.inner.on_scoped_event(&self.scope, event);
    }

    fn on_scoped_event(&self, scope: &Scope, event: &Event) {
        self.inner.on_scoped_event(&scope.within(&self.scope), event);
    }

    fn on_step(&self, scope: &Scope) {
        self.inner.on_step(&scope.within(&self.scope));
    }
}

/// The steps of an operation, for progress: events sent through [`Steps::ctx`] carry the step
/// that's running, and [`Steps::begin`] announces each one (so a step with no events of its own
/// still shows).
pub struct Steps<'a> {
    inner: &'a dyn Progress,
    cancel: CancelToken,
    ids: Vec<&'static str>,
    current: Mutex<Option<Step>>,
}

impl<'a> Steps<'a> {
    /// `ids`: the steps that will run, in order.
    pub fn new(ctx: &Ctx<'a>, ids: Vec<&'static str>) -> Self {
        Steps { inner: ctx.progress, cancel: ctx.cancel.clone(), ids, current: Mutex::new(None) }
    }

    /// A context for the steps' work.
    pub fn ctx(&self) -> Ctx<'_> {
        Ctx::new(self, self.cancel.clone())
    }

    /// Step `id` (one of the `ids`) begins.
    pub fn begin(&self, id: &'static str) {
        let index = self.ids.iter().position(|s| *s == id);
        debug_assert!(index.is_some(), "step {id:?} isn't one of {:?}", self.ids);
        let step = Step { index: index.map_or(0, |i| i as u32 + 1), total: self.ids.len() as u32, id };
        *self.current.lock().unwrap_or_else(PoisonError::into_inner) = Some(step);
        self.inner.on_step(&Scope { target: None, step: Some(step) });
    }

    fn scope(&self) -> Scope {
        Scope { target: None, step: *self.current.lock().unwrap_or_else(PoisonError::into_inner) }
    }
}

impl Progress for Steps<'_> {
    fn on_event(&self, event: &Event) {
        self.inner.on_scoped_event(&self.scope(), event);
    }

    fn on_scoped_event(&self, scope: &Scope, event: &Event) {
        self.inner.on_scoped_event(&scope.within(&self.scope()), event);
    }

    fn on_step(&self, scope: &Scope) {
        self.inner.on_step(&scope.within(&self.scope()));
    }
}

/// Discards all events.
pub struct NoProgress;

impl Progress for NoProgress {
    fn on_event(&self, _: &Event) {}
}

impl<F: Fn(&Event) + Send + Sync> Progress for F {
    fn on_event(&self, event: &Event) {
        self(event)
    }
}

/// Steps under way, in this process, that must finish together (swapping in a drive's
/// databases): each [`Critical`] holds it to read, and a front end exiting at once (a second
/// Ctrl-C or signal) takes it to write first ([`wait_for_critical`]), which waits for them.
static CRITICAL: RwLock<()> = RwLock::new(());

/// How many [`Critical`] steps are under way: the lock can be held by another exit instead.
static SWAPS: AtomicUsize = AtomicUsize::new(0);

/// While one lives, a front end exiting at once waits (see [`wait_for_critical`]).
pub struct Critical {
    _held: RwLockReadGuard<'static, ()>,
}

impl Critical {
    pub fn enter() -> Critical {
        let held = CRITICAL.read().unwrap_or_else(PoisonError::into_inner);
        SWAPS.fetch_add(1, Ordering::SeqCst);
        Critical { _held: held }
    }
}

impl Drop for Critical {
    fn drop(&mut self) {
        // Before the lock is let go: an exit that gets it next sees none under way.
        SWAPS.fetch_sub(1, Ordering::SeqCst);
    }
}

/// For a front end about to exit at once: waits for the [`Critical`] steps under way to finish
/// (saying so on stderr when there are any), and keeps new ones from starting while the guard it
/// returns lives.
pub fn wait_for_critical() -> RwLockWriteGuard<'static, ()> {
    exclusive(|swapping| {
        if swapping {
            eprintln!("finishing the database swap under way first...");
        }
    })
}

/// [`wait_for_critical`], telling `waiting` whether a swap is under way when it has to wait (or
/// another exit holds the lock).
fn exclusive(waiting: impl FnOnce(bool)) -> RwLockWriteGuard<'static, ()> {
    match CRITICAL.try_write() {
        Ok(held) => held,
        Err(TryLockError::Poisoned(e)) => e.into_inner(),
        Err(TryLockError::WouldBlock) => {
            waiting(SWAPS.load(Ordering::SeqCst) > 0);
            CRITICAL.write().unwrap_or_else(PoisonError::into_inner)
        }
    }
}

/// Cooperative cancellation flag. Clones share the same flag.
#[derive(Debug, Clone, Default)]
pub struct CancelToken(Arc<AtomicBool>);

impl CancelToken {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

/// Execution context threaded through long-running operations.
#[derive(Clone)]
pub struct Ctx<'a> {
    pub progress: &'a dyn Progress,
    pub cancel: CancelToken,
}

/// Emits the first few warnings of each kind and a summary for the rest, so one systemic
/// problem (say, a thousand unreadable analysis files) doesn't drown everything else.
pub struct WarningLimiter<'c, 'a> {
    ctx: &'c Ctx<'a>,
    counts: Vec<(&'static str, usize)>,
    limit: usize,
}

impl<'c, 'a> WarningLimiter<'c, 'a> {
    pub fn new(ctx: &'c Ctx<'a>, limit: usize) -> Self {
        WarningLimiter { ctx, counts: Vec::new(), limit }
    }

    pub fn warn(&mut self, kind: &'static str, message: impl Into<String>) {
        let n = match self.counts.iter_mut().find(|(k, _)| *k == kind) {
            Some((_, n)) => {
                *n += 1;
                *n
            }
            None => {
                self.counts.push((kind, 1));
                1
            }
        };
        if n <= self.limit {
            self.ctx.warn(message);
        }
    }

    pub fn finish(self) {
        for (kind, n) in &self.counts {
            if *n > self.limit {
                self.ctx.warn(format!("... and {} more like that ({n} {kind} in total)", n - self.limit));
            }
        }
    }
}

static NO_PROGRESS: NoProgress = NoProgress;

impl<'a> Ctx<'a> {
    pub fn new(progress: &'a dyn Progress, cancel: CancelToken) -> Self {
        Self { progress, cancel }
    }

    /// No progress output, never cancelled.
    pub fn silent() -> Ctx<'static> {
        Ctx { progress: &NO_PROGRESS, cancel: CancelToken::new() }
    }

    /// The same progress reporting, never cancelled: for what a write records once it has
    /// changed the drive, which must finish then.
    pub fn uncancellable(&self) -> Ctx<'a> {
        Ctx { progress: self.progress, cancel: CancelToken::new() }
    }

    /// Runs `f` with its events marked as being about `target` (one drive of several).
    pub fn for_target<R>(&self, target: impl Into<String>, f: impl FnOnce(&Ctx<'_>) -> R) -> R {
        let scoped = Scoped { inner: self.progress, scope: Scope { target: Some(target.into()), step: None } };
        f(&Ctx::new(&scoped, self.cancel.clone()))
    }

    pub fn check(&self) -> Result<()> {
        if self.cancel.is_cancelled() { Err(Error::Cancelled) } else { Ok(()) }
    }

    pub fn emit(&self, event: Event) {
        self.progress.on_event(&event);
    }

    pub fn warn(&self, message: impl Into<String>) {
        self.emit(Event::Warning { message: message.into() });
    }

    pub fn phase_start(&self, phase: Phase, items: u64, bytes: u64) {
        self.emit(Event::PhaseStart { phase, items, bytes });
    }

    pub fn phase_end(&self, phase: Phase) {
        self.emit(Event::PhaseEnd { phase });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::Duration;

    /// An exit at once waits for the swap under way, saying so, and no swap starts while it's
    /// exiting. A second exit waits for the first without saying it's finishing a swap.
    #[test]
    fn exiting_at_once_waits_for_the_swap_and_holds_off_the_next() {
        let swapping = Critical::enter();
        let (said, heard) = mpsc::channel();
        let (held, release) = (mpsc::channel(), mpsc::channel::<()>());
        let exiting = std::thread::spawn(move || {
            let _exit = exclusive(|swap| said.send(swap).unwrap());
            held.0.send(()).unwrap();
            release.1.recv().unwrap();
        });
        assert!(heard.recv().unwrap(), "a swap is under way");
        assert!(held.1.recv_timeout(Duration::from_millis(100)).is_err(), "exited during the swap");
        drop(swapping);
        held.1.recv().unwrap();

        let (said, heard) = mpsc::channel();
        let again = std::thread::spawn(move || drop(exclusive(|swap| said.send(swap).unwrap())));
        assert!(!heard.recv().unwrap(), "no swap is under way, only the other exit");
        let started = mpsc::channel();
        let next = std::thread::spawn(move || {
            let _swapping = Critical::enter();
            started.0.send(()).unwrap();
        });
        assert!(started.1.recv_timeout(Duration::from_millis(100)).is_err(), "a swap started while exiting");
        release.0.send(()).unwrap();
        started.1.recv().unwrap();
        for t in [exiting, again, next] {
            t.join().unwrap();
        }
    }
}
