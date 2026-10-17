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
