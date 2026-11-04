use std::sync::{Arc, Mutex};

use super::*;
use crate::{CancelToken, Ctx, Steps};

/// What a [`Coalesced`] passed on, as `(target, step id, report)`.
type Passed = Arc<Mutex<Vec<(Option<String>, Option<&'static str>, String)>>>;

fn coalesced(passed: &Passed, every: Duration) -> Coalesced {
    let passed = passed.clone();
    Coalesced::new(every, move |scope: &Scope, report: Report| {
        let report = match report {
            Report::Event { event, items } => format!("{event:?} {items:?}"),
            Report::Step => "step".into(),
        };
        passed.lock().unwrap().push((scope.target.clone(), scope.step.map(|s| s.id), report));
    })
}

#[test]
fn items_and_bytes_are_summed_until_another_event() {
    let passed = Passed::default();
    let p = coalesced(&passed, Duration::from_secs(3600));
    let ctx = Ctx::new(&p, CancelToken::new());
    ctx.phase_start(Phase::Copy, 3, 30);
    for i in 0..3 {
        ctx.emit(Event::Item { phase: Phase::Copy, path: format!("t{i}"), bytes: 10 });
        ctx.emit(Event::Bytes { phase: Phase::Copy, bytes: 10 });
    }
    // Bytes of another phase pass on the copy's first.
    ctx.emit(Event::Bytes { phase: Phase::Verify, bytes: 5 });
    ctx.phase_end(Phase::Copy);
    p.flush_due();
    let before_finish = passed.lock().unwrap().len();
    p.finish();
    assert_eq!(passed.lock().unwrap().len(), before_finish, "the other events passed on what was held");
    let reports: Vec<String> = passed.lock().unwrap().iter().map(|(_, _, r)| r.clone()).collect();
    assert_eq!(
        reports,
        [
            "PhaseStart { phase: Copy, items: 3, bytes: 30 } None",
            "Item { phase: Copy, path: \"t2\", bytes: 10 } Some(3)",
            "Bytes { phase: Copy, bytes: 30 } None",
            "Bytes { phase: Verify, bytes: 5 } None",
            "PhaseEnd { phase: Copy } None",
        ]
    );
}

#[test]
fn held_events_go_out_once_due() {
    let passed = Passed::default();
    let p = coalesced(&passed, Duration::ZERO);
    let ctx = Ctx::new(&p, CancelToken::new());
    ctx.emit(Event::Item { phase: Phase::Copy, path: "a".into(), bytes: 1 });
    assert_eq!(passed.lock().unwrap().len(), 1, "due at once");
    p.flush_due();
    assert_eq!(passed.lock().unwrap().len(), 1, "nothing held");

    // Not due: held.
    let passed = Passed::default();
    let p = coalesced(&passed, Duration::from_secs(3600));
    Ctx::new(&p, CancelToken::new()).emit(Event::Bytes { phase: Phase::Copy, bytes: 1 });
    p.flush_due();
    assert!(passed.lock().unwrap().is_empty());

    // Due by the time it's asked (and maybe already when the event came).
    let passed = Passed::default();
    let p = coalesced(&passed, Duration::from_millis(20));
    Ctx::new(&p, CancelToken::new()).emit(Event::Bytes { phase: Phase::Copy, bytes: 1 });
    std::thread::sleep(Duration::from_millis(25));
    p.flush_due();
    assert_eq!(passed.lock().unwrap().len(), 1);
}

/// A flusher sends what a reporter holds back once it's due, with no event to prompt it, and
/// stops when told, even while it waits for a reporter.
#[test]
fn a_flusher_sends_what_is_held_back() {
    let passed = Passed::default();
    let p = Arc::new(coalesced(&passed, Duration::from_millis(20)));
    let flusher = Arc::new(Flusher::new());
    let running = flusher.clone();
    let thread = std::thread::spawn(move || running.run(Duration::from_millis(1)));
    flusher.watch(&p);
    Ctx::new(&*p, CancelToken::new()).emit(Event::Bytes { phase: Phase::Copy, bytes: 1 });
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while passed.lock().unwrap().is_empty() {
        assert!(std::time::Instant::now() < deadline, "never flushed");
        std::thread::sleep(Duration::from_millis(1));
    }
    // Once the reporter is gone, the flusher waits for another; stopping ends it.
    drop(p);
    flusher.stop();
    thread.join().unwrap();
}

use hegel::TestCase;
use hegel::generators as gs;

/// A scope's target, its latest item (as its `Debug` text) with how many it stands for, and its
/// bytes of one phase summed.
type HeldBy = (Option<String>, Option<(String, u64)>, Option<(Phase, u64)>);

/// What a reporter passes on, by a model of it: per scope (in the order scopes first held
/// something), the latest item with how many it stands for, then the bytes of one phase summed.
#[derive(Default)]
struct Model {
    held: Vec<HeldBy>,
    passed: Vec<(Option<String>, String)>,
}

impl Model {
    fn flush(&mut self) {
        for (target, item, bytes) in self.held.drain(..) {
            if let Some((event, n)) = item {
                self.passed.push((target.clone(), format!("{event} {:?}", Some(n).filter(|n| *n > 1))));
            }
            if let Some((phase, bytes)) = bytes {
                self.passed.push((target, format!("{:?} None", Event::Bytes { phase, bytes })));
            }
        }
    }

    fn scope(&mut self, target: &Option<String>) -> usize {
        match self.held.iter().position(|h| h.0 == *target) {
            Some(i) => i,
            None => {
                self.held.push((target.clone(), None, None));
                self.held.len() - 1
            }
        }
    }
}

/// Items and bytes are held back and passed on summed, per scope (each target its own): until
/// another kind of event comes, or bytes of another phase, or the reporter's finished; or at
/// once when they're due at once. Nothing's lost, and nothing passes twice.
#[hegel::test(test_cases = 200)]
fn prop_held_events_pass_on_summed_per_scope(tc: TestCase) {
    let at_once = tc.draw(gs::booleans());
    let every = if at_once { Duration::ZERO } else { Duration::from_secs(3600) };
    let events: Vec<(Option<&str>, u8, Phase, u64)> = tc.draw(
        gs::vecs(gs::tuples!(
            gs::optional(gs::sampled_from(vec!["A", "B"])),
            gs::integers::<u8>().max_value(2),
            gs::sampled_from(vec![Phase::Copy, Phase::Verify]),
            gs::integers::<u64>().max_value(1000)
        ))
        .max_size(12),
    );
    let passed = Passed::default();
    let p = coalesced(&passed, every);
    let ctx = Ctx::new(&p, CancelToken::new());
    let mut model = Model::default();
    for (i, (target, kind, phase, n)) in events.into_iter().enumerate() {
        let event = match kind {
            0 => Event::Item { phase, path: format!("t{i}"), bytes: n },
            1 => Event::Bytes { phase, bytes: n },
            _ => Event::PhaseEnd { phase },
        };
        match target {
            Some(t) => ctx.for_target(t, |ctx| ctx.emit(event.clone())),
            None => ctx.emit(event.clone()),
        }
        let target = target.map(String::from);
        match event {
            Event::Item { .. } => {
                let s = model.scope(&target);
                let n = model.held[s].1.as_ref().map_or(0, |h| h.1);
                model.held[s].1 = Some((format!("{event:?}"), n + 1));
            }
            Event::Bytes { phase, bytes } => {
                let s = model.scope(&target);
                if model.held[s].2.is_some_and(|(p, _)| p != phase) {
                    model.flush();
                }
                let s = model.scope(&target);
                let sum = model.held[s].2.map_or(0, |h| h.1);
                model.held[s].2 = Some((phase, sum + bytes));
            }
            other => {
                model.flush();
                model.passed.push((target, format!("{other:?} None")));
                continue;
            }
        }
        if at_once {
            model.flush();
        }
    }
    p.finish();
    model.flush();
    let got: Vec<(Option<String>, String)> =
        passed.lock().unwrap().iter().map(|(t, _, r)| (t.clone(), r.clone())).collect();
    assert_eq!(got, model.passed);
}

/// A step beginning passes on what's held first, summed, then the step, in the step's scope.
#[hegel::test(test_cases = 50)]
fn prop_a_step_passes_on_what_is_held_before_it(tc: TestCase) {
    let items = tc.draw(gs::integers::<u64>().max_value(4));
    let passed = Passed::default();
    let p = coalesced(&passed, Duration::from_secs(3600));
    let ctx = Ctx::new(&p, CancelToken::new());
    let steps = Steps::new(&ctx, vec!["copy"]);
    for i in 0..items {
        steps.ctx().emit(Event::Item { phase: Phase::Copy, path: format!("t{i}"), bytes: 1 });
    }
    steps.begin("copy");
    let mut want = Vec::new();
    if items > 0 {
        let last = Event::Item { phase: Phase::Copy, path: format!("t{}", items - 1), bytes: 1 };
        want.push((None, None, format!("{last:?} {:?}", Some(items).filter(|n| *n > 1))));
    }
    want.push((None, Some("copy"), "step".to_string()));
    assert_eq!(*passed.lock().unwrap(), want);
}

/// A flusher whose reporters are all gone waits for another, and stops when told while it waits;
/// one with a live reporter passes on what it holds once that's due, with no event to prompt it,
/// and stops when told. Any flusher starts so, as a default one does.
#[hegel::test(test_cases = 5)]
fn prop_a_flusher_waits_for_a_reporter_and_stops_when_told(tc: TestCase) {
    let live = tc.draw(gs::booleans());
    let flusher = Arc::new(Flusher::default());
    let passed = Passed::default();
    // Long enough for the gone reporter's one event never to come due, however late this runs.
    let p = Arc::new(coalesced(&passed, Duration::from_secs(if live { 0 } else { 3600 }) + Duration::from_millis(5)));
    flusher.watch(&p);
    Ctx::new(&*p, CancelToken::new()).emit(Event::Bytes { phase: Phase::Copy, bytes: 1 });
    if !live {
        drop(p);
    }
    let running = flusher.clone();
    let thread = std::thread::spawn(move || running.run(Duration::from_millis(1)));
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    if live {
        while passed.lock().unwrap().is_empty() {
            assert!(std::time::Instant::now() < deadline, "never flushed");
            std::thread::yield_now();
        }
    } else {
        // The flusher drops the gone reporter and waits in one hold of its lock: once the list is
        // empty here, it's waiting.
        while !flusher.lock().watched.is_empty() {
            assert!(std::time::Instant::now() < deadline, "never looked at its reporters");
            std::thread::yield_now();
        }
    }
    flusher.stop();
    thread.join().unwrap();
    // A reporter gone before it was due had nothing passed on for it.
    assert_eq!(passed.lock().unwrap().is_empty(), !live);
}

#[test]
fn each_scope_is_coalesced_on_its_own_and_steps_are_announced() {
    let passed = Passed::default();
    let p = coalesced(&passed, Duration::from_secs(3600));
    let ctx = Ctx::new(&p, CancelToken::new());
    let steps = Steps::new(&ctx, vec!["copy"]);
    let in_step = steps.ctx();
    in_step.for_target("A", |ctx| ctx.emit(Event::Item { phase: Phase::Copy, path: "a".into(), bytes: 1 }));
    steps.begin("copy");
    in_step.for_target("B", |ctx| ctx.emit(Event::Item { phase: Phase::Copy, path: "b".into(), bytes: 1 }));
    in_step.for_target("B", |ctx| ctx.emit(Event::Item { phase: Phase::Copy, path: "c".into(), bytes: 1 }));
    p.finish();
    let passed = passed.lock().unwrap().clone();
    assert_eq!(passed[0], (Some("A".into()), None, "Item { phase: Copy, path: \"a\", bytes: 1 } None".into()));
    assert_eq!(passed[1], (None, Some("copy"), "step".into()));
    assert_eq!(
        passed[2],
        (Some("B".into()), Some("copy"), "Item { phase: Copy, path: \"c\", bytes: 1 } Some(2)".into())
    );
    assert_eq!(passed.len(), 3);
}
