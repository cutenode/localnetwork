//! Fewer progress events for a front end at the other end of a pipe.

use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError, Weak};
use std::time::{Duration, Instant};

use crate::{Event, Phase, Progress, Scope};

/// What a [`Coalesced`] passes on.
#[derive(Debug, Clone)]
pub enum Report {
    /// An event; for an `item`, how many items it stands for when that's more than one.
    Event { event: Event, items: Option<u64> },
    /// A step begins (the scope's `step` says which).
    Step,
}

/// Where a [`Coalesced`] passes reports on to.
type Sink = dyn Fn(&Scope, Report) + Send + Sync;

/// Passes events on to a function, coalescing byte counts and per-file events, so a 100 GB copy
/// or a 10,000-track export reports a few times per `every`, not once per chunk or file: bytes
/// are added up, and an `item` event stands for the items since the last one passed on (with how
/// many). Other events go out at once, after anything held back. Each scope (drive, step) is
/// coalesced separately. Call [`Coalesced::flush_due`] now and then (a [`Flusher`] does), so
/// what's held back doesn't wait for the next event, and [`Coalesced::finish`] once the work is
/// done.
pub struct Coalesced {
    send: Box<Sink>,
    every: Duration,
    held: Mutex<Held>,
}

struct Held {
    /// When held events were last passed on.
    sent: Instant,
    scopes: Vec<HeldScope>,
}

struct HeldScope {
    scope: Scope,
    /// The latest item, and how many items it stands for.
    item: Option<(Event, u64)>,
    bytes: Option<(Phase, u64)>,
}

impl Held {
    fn scope(&mut self, scope: &Scope) -> &mut HeldScope {
        let i = match self.scopes.iter().position(|h| h.scope == *scope) {
            Some(i) => i,
            None => {
                self.scopes.push(HeldScope { scope: scope.clone(), item: None, bytes: None });
                self.scopes.len() - 1
            }
        };
        &mut self.scopes[i]
    }
}

impl Coalesced {
    pub fn new(every: Duration, send: impl Fn(&Scope, Report) + Send + Sync + 'static) -> Self {
        Coalesced { send: Box::new(send), every, held: Mutex::new(Held { sent: Instant::now(), scopes: Vec::new() }) }
    }

    fn lock(&self) -> MutexGuard<'_, Held> {
        self.held.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Passes on everything held back.
    fn flush(&self, held: &mut Held) {
        for h in held.scopes.drain(..) {
            if let Some((event, n)) = h.item {
                (self.send)(&h.scope, Report::Event { event, items: Some(n).filter(|n| *n > 1) });
            }
            if let Some((phase, bytes)) = h.bytes {
                (self.send)(&h.scope, Report::Event { event: Event::Bytes { phase, bytes }, items: None });
            }
        }
        held.sent = Instant::now();
    }

    /// Passes on what's held back once it has waited `every`, for when no event follows soon (a
    /// slow file after quick ones).
    pub fn flush_due(&self) {
        let mut held = self.lock();
        if !held.scopes.is_empty() && held.sent.elapsed() >= self.every {
            self.flush(&mut held);
        }
    }

    /// Passes on everything held back.
    pub fn finish(&self) {
        self.flush(&mut self.lock());
    }
}

impl Progress for Coalesced {
    fn on_event(&self, event: &Event) {
        self.on_scoped_event(&Scope::default(), event);
    }

    fn on_scoped_event(&self, scope: &Scope, event: &Event) {
        let mut held = self.lock();
        match event {
            Event::Bytes { phase, bytes } => {
                if held.scope(scope).bytes.is_some_and(|(p, _)| p != *phase) {
                    self.flush(&mut held);
                }
                let slot = &mut held.scope(scope).bytes;
                *slot = Some((*phase, slot.map_or(0, |(_, n)| n) + bytes));
            }
            Event::Item { .. } => {
                let slot = &mut held.scope(scope).item;
                let n = slot.as_ref().map_or(0, |(_, n)| *n);
                *slot = Some((event.clone(), n + 1));
            }
            other => {
                self.flush(&mut held);
                (self.send)(scope, Report::Event { event: other.clone(), items: None });
                return;
            }
        }
        if held.sent.elapsed() >= self.every {
            self.flush(&mut held);
        }
    }

    fn on_step(&self, scope: &Scope) {
        let mut held = self.lock();
        self.flush(&mut held);
        (self.send)(scope, Report::Step);
    }
}

/// Flushes what watched [`Coalesced`] reporters hold back once it's due, for as long as they
/// live, so a slow file after quick ones still shows. [`Flusher::run`] does it on the caller's
/// thread.
pub struct Flusher {
    state: Mutex<Flushing>,
    /// Notified when a reporter is watched, or the flusher stopped.
    changed: Condvar,
}

struct Flushing {
    watched: Vec<Weak<Coalesced>>,
    stopped: bool,
}

impl Default for Flusher {
    fn default() -> Self {
        Flusher::new()
    }
}

impl Flusher {
    /// Const, for a `static` one.
    pub const fn new() -> Self {
        Flusher { state: Mutex::new(Flushing { watched: Vec::new(), stopped: false }), changed: Condvar::new() }
    }

    fn lock(&self) -> MutexGuard<'_, Flushing> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Flushes `reporter` from now until it's dropped.
    pub fn watch(&self, reporter: &Arc<Coalesced>) {
        self.lock().watched.push(Arc::downgrade(reporter));
        self.changed.notify_all();
    }

    /// Flushes the watched reporters every `every` while any of them lives (and waits, without
    /// waking, while none does), until [`Flusher::stop`].
    pub fn run(&self, every: Duration) {
        loop {
            let live: Vec<Arc<Coalesced>> = {
                let mut s = self.lock();
                loop {
                    if s.stopped {
                        return;
                    }
                    s.watched.retain(|w| w.strong_count() > 0);
                    if !s.watched.is_empty() {
                        break;
                    }
                    s = self.changed.wait(s).unwrap_or_else(PoisonError::into_inner);
                }
                s.watched.iter().filter_map(Weak::upgrade).collect()
            };
            for reporter in live {
                reporter.flush_due();
            }
            std::thread::sleep(every);
        }
    }

    /// Ends [`Flusher::run`].
    pub fn stop(&self) {
        self.lock().stopped = true;
        self.changed.notify_all();
    }
}

#[cfg(test)]
mod tests;
