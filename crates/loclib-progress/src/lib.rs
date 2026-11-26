#![doc = include_str!("../README.md")]
#![cfg_attr(docsrs, feature(doc_cfg))]
#![warn(missing_docs)]

mod coalesce;

pub use coalesce::{Coalesced, Flusher, Report};

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use serde::{Deserialize, Serialize};

/// The kind of work an [`Event`] belongs to, which a front end names in its progress display.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum Phase {
    /// Going through items to see what there is or what needs doing (files, tracks, snapshots).
    Scan,
    /// Reading files to hash their contents.
    Hash,
    /// Copying or transcoding files to their destination.
    Copy,
    /// Writing databases, tags, artwork or other files.
    Write,
    /// Deleting files.
    Delete,
    /// Reading files back to check they're intact.
    Verify,
    /// Formatting a drive.
    Format,
    /// Decoding and fingerprinting library tracks.
    Fingerprint,
    /// Building the fingerprint index.
    Index,
    /// Decoding and fingerprinting a mix.
    Listen,
    /// Looking for library tracks in a mix.
    Match,
    /// Decoding library tracks to estimate their tempo, key and loudness.
    Analyze,
    /// Sending files to a server (a set's recording, by a plugin's `net/fetch`).
    Upload,
    /// Timing writes to a drive, of scratch data that's removed after.
    Measure,
}

/// A progress event from a long-running operation. In JSON, `event` names the kind.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(tag = "event", rename_all = "snake_case")]
#[non_exhaustive]
pub enum Event {
    /// A phase begins. `items`/`bytes` are totals when known up front (0 otherwise).
    PhaseStart {
        /// The phase that begins.
        phase: Phase,
        /// How many items the phase will go through, or 0 when that isn't known.
        items: u64,
        /// How many bytes the phase will process, or 0 when that isn't known.
        bytes: u64,
    },
    /// Work on one item (usually a file) begins.
    Item {
        /// The phase the item belongs to.
        phase: Phase,
        /// The item's path, or a name for it when it isn't a file.
        path: String,
        /// The item's size in bytes, or 0 when that isn't known or doesn't apply.
        bytes: u64,
    },
    /// `bytes` more bytes were processed in the current phase.
    Bytes {
        /// The phase the bytes belong to.
        phase: Phase,
        /// How many bytes were processed since the last `bytes` event.
        bytes: u64,
    },
    /// A phase is done.
    PhaseEnd {
        /// The phase that ended.
        phase: Phase,
    },
    /// Something went wrong that the operation carries on past.
    Warning {
        /// What went wrong, for a person to read.
        message: String,
    },
}

/// Which part of a larger operation an event belongs to: the drive, when several are written at
/// once ([`Ctx::for_target`]), and the step, in an operation made of steps ([`Steps`]).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct Scope {
    /// The drive the event is about, when several are written at once. Absent otherwise.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    /// The step that's running, in an operation made of steps. Absent otherwise.
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
    /// The step's position, from 1.
    pub index: u32,
    /// How many steps the operation has.
    pub total: u32,
    /// The step's name.
    pub id: &'static str,
}

/// Receives an operation's progress events. Any `Fn(&Event) + Send + Sync` closure is one.
pub trait Progress: Send + Sync {
    /// An event, with no scope.
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
    /// A token that isn't cancelled yet.
    pub fn new() -> Self {
        Self::default()
    }

    /// Cancels the operation, for this token and every clone of it. It can't be undone.
    pub fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }

    /// Whether [`CancelToken::cancel`] has been called on this token or a clone of it.
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
    /// Where events go.
    pub progress: &'a dyn Progress,
    /// Cancels the operation when cancelled; [`Ctx::check`] reads it.
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
    /// Passes up to `limit` warnings of each kind on to `ctx`.
    pub fn new(ctx: &'c Ctx<'a>, limit: usize) -> Self {
        WarningLimiter { ctx, counts: Vec::new(), limit }
    }

    /// A warning of `kind`, passed on unless `limit` of that kind already were.
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

    /// Warns once for each kind that went over the limit, with how many more there were.
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
    /// A context that sends events to `progress` and is cancelled by `cancel`.
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

    /// Sends `event` to the progress receiver.
    pub fn emit(&self, event: Event) {
        self.progress.on_event(&event);
    }

    /// Sends an [`Event::Warning`] with `message`.
    pub fn warn(&self, message: impl Into<String>) {
        self.emit(Event::Warning { message: message.into() });
    }

    /// Sends an [`Event::PhaseStart`]. `items` and `bytes` are the phase's totals, or 0 when not known.
    pub fn phase_start(&self, phase: Phase, items: u64, bytes: u64) {
        self.emit(Event::PhaseStart { phase, items, bytes });
    }

    /// Sends an [`Event::PhaseEnd`].
    pub fn phase_end(&self, phase: Phase) {
        self.emit(Event::PhaseEnd { phase });
    }
}

#[cfg(test)]
mod tests;
