#![doc = include_str!("../README.md")]

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
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

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
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
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
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

/// What [`Ctx::check`] fails with once the operation has been cancelled.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("operation cancelled")]
pub struct Cancelled;

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

    /// [`Cancelled`] once the operation has been cancelled: `ctx.check()?` between steps.
    pub fn check(&self) -> Result<(), Cancelled> {
        if self.cancel.is_cancelled() { Err(Cancelled) } else { Ok(()) }
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

    /// What reached a front end: each event's scope (`None`: plain `on_event`), and each step
    /// announced.
    #[derive(Default)]
    struct Recorder {
        events: Mutex<Vec<(Option<Scope>, String)>>,
        steps: Mutex<Vec<Scope>>,
    }

    impl Progress for Recorder {
        fn on_event(&self, event: &Event) {
            self.events.lock().unwrap().push((None, format!("{event:?}")));
        }
        fn on_scoped_event(&self, scope: &Scope, event: &Event) {
            self.events.lock().unwrap().push((Some(scope.clone()), format!("{event:?}")));
        }
        fn on_step(&self, scope: &Scope) {
            self.steps.lock().unwrap().push(scope.clone());
        }
    }

    /// An event knows both the drive and the step it belongs to, whichever was entered first.
    #[test]
    fn events_carry_their_drive_and_step_however_scopes_nest() {
        let recorder = Recorder::default();
        let ctx = Ctx::new(&recorder, CancelToken::new());
        let library = Step { index: 2, total: 2, id: "library" };
        let both = Scope { target: Some("Backup".into()), step: Some(library) };

        // A step, then a drive inside it.
        let steps = Steps::new(&ctx, vec!["format", "library"]);
        steps.begin("library");
        steps.ctx().for_target("Backup", |c| c.phase_start(Phase::Copy, 1, 10));
        // A drive, then steps inside it.
        ctx.for_target("Backup", |c| {
            let steps = Steps::new(c, vec!["format", "library"]);
            steps.begin("library");
            steps.ctx().phase_end(Phase::Copy);
            // Steps inside steps: the inner step is the one that's running.
            let inner = Steps::new(&steps.ctx(), vec!["verify"]);
            inner.begin("verify");
        });

        let events = recorder.events.into_inner().unwrap();
        assert_eq!(events.len(), 2);
        assert!(events.iter().all(|(scope, _)| scope.as_ref() == Some(&both)), "{events:?}");
        let steps = recorder.steps.into_inner().unwrap();
        let backup = Some("Backup".to_string());
        assert_eq!(steps[0], Scope { target: None, step: Some(library) });
        assert_eq!(steps[1], both);
        assert_eq!(steps[2], Scope { target: backup, step: Some(Step { index: 1, total: 1, id: "verify" }) });
    }

    /// Cancelling any clone of the token fails the next check, and the error reads as
    /// locallibrary's own cancelled error does.
    #[test]
    fn cancelling_a_clone_fails_the_next_check() {
        let ctx = Ctx::silent();
        assert_eq!(ctx.check(), Ok(()));
        ctx.cancel.clone().cancel();
        assert_eq!(ctx.check(), Err(Cancelled));
        assert_eq!(Cancelled.to_string(), "operation cancelled");
        assert_eq!(ctx.uncancellable().check(), Ok(()));
    }

    #[test]
    fn repeated_warnings_are_summed_up() {
        let recorder = Recorder::default();
        let ctx = Ctx::new(&recorder, CancelToken::new());
        let mut limiter = WarningLimiter::new(&ctx, 2);
        for n in 1..=4 {
            limiter.warn("unreadable files", format!("ANLZ{n}.DAT is unreadable"));
        }
        limiter.warn("missing files", "a.mp3 is missing");
        limiter.finish();
        let warnings: Vec<String> = recorder.events.into_inner().unwrap().into_iter().map(|(_, e)| e).collect();
        assert_eq!(warnings.len(), 4, "{warnings:?}");
        assert!(warnings[1].contains("ANLZ2.DAT") && warnings[2].contains("a.mp3"), "{warnings:?}");
        assert!(warnings[3].contains("... and 2 more like that (4 unreadable files in total)"), "{warnings:?}");
    }
}
