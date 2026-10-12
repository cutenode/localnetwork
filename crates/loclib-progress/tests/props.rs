//! Properties of progress events, scopes, cancellation and the warning limiter.

use std::sync::Mutex;

use hegel::generators as gs;
use hegel::{Generator, TestCase};
use loclib_progress::{
    CancelToken, Cancelled, Ctx, Event, NoProgress, Phase, Progress, Scope, Step, Steps, WarningLimiter,
};

const PHASES: [Phase; 12] = [
    Phase::Scan,
    Phase::Hash,
    Phase::Copy,
    Phase::Write,
    Phase::Delete,
    Phase::Verify,
    Phase::Format,
    Phase::Fingerprint,
    Phase::Index,
    Phase::Listen,
    Phase::Match,
    Phase::Analyze,
];

/// Step names: `Steps` takes `&'static str`s, so generated steps pick from these.
const STEP_IDS: [&str; 5] = ["format", "library", "verify", "settings", "copy"];

/// What reached a front end: each event's scope (`None`: plain `on_event`), and each step
/// announced. Events are kept as their `Debug` text, since `Event` has no `PartialEq`.
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

#[hegel::test_helper]
fn phase(tc: &TestCase) -> Phase {
    tc.draw(gs::sampled_from(PHASES.to_vec()).print_as_debug())
}

#[hegel::test_helper]
fn event(tc: &TestCase) -> Event {
    let bytes = tc.draw(gs::integers::<u64>());
    match tc.draw(gs::integers::<u8>().max_value(4)) {
        0 => Event::PhaseStart { phase: phase(tc), items: tc.draw(gs::integers::<u64>()), bytes },
        1 => Event::Item { phase: phase(tc), path: tc.draw(gs::text().max_size(8)), bytes },
        2 => Event::Bytes { phase: phase(tc), bytes },
        3 => Event::PhaseEnd { phase: phase(tc) },
        _ => Event::Warning { message: tc.draw(gs::text().max_size(8)) },
    }
}

/// Sends `event` through whichever of `ctx`'s helpers makes it.
fn send(ctx: &Ctx<'_>, event: &Event) {
    match event {
        Event::PhaseStart { phase, items, bytes } => ctx.phase_start(*phase, *items, *bytes),
        Event::PhaseEnd { phase } => ctx.phase_end(*phase),
        Event::Warning { message } => ctx.warn(message.clone()),
        other => ctx.emit(other.clone()),
    }
}

/// One level of an operation: a drive of several, or a set of steps, one of which may have
/// begun.
#[derive(Debug, Clone)]
enum Layer {
    Target(String),
    Steps { ids: Vec<&'static str>, begun: Option<usize> },
}

#[hegel::test_helper]
fn layer(tc: &TestCase) -> Layer {
    if tc.draw(gs::booleans()) {
        return Layer::Target(
            tc.draw(hegel::one_of!(gs::sampled_from(vec!["Backup", "USB"]).map(String::from), gs::text().max_size(4))),
        );
    }
    let ids = tc.draw(gs::vecs(gs::sampled_from(STEP_IDS.to_vec())).unique(true).min_size(1).max_size(4));
    let begun = tc.draw(gs::optional(gs::integers::<usize>().max_value(ids.len() - 1)));
    Layer::Steps { ids, begun }
}

/// Runs `layers`, outermost first, and sends `events` from inside the innermost.
fn nest(ctx: &Ctx<'_>, layers: &[Layer], events: &[Event]) {
    match layers.split_first() {
        None => events.iter().for_each(|e| send(ctx, e)),
        Some((Layer::Target(target), rest)) => ctx.for_target(target.clone(), |c| nest(c, rest, events)),
        Some((Layer::Steps { ids, begun }, rest)) => {
            let steps = Steps::new(ctx, ids.clone());
            if let Some(i) = begun {
                steps.begin(ids[*i]);
            }
            nest(&steps.ctx(), rest, events);
        }
    }
}

/// What `layers` say an event from inside them belongs to: the innermost drive and the innermost
/// step that has begun.
fn scope_of(layers: &[Layer]) -> Scope {
    let mut scope = Scope::default();
    for l in layers {
        match l {
            Layer::Target(t) => scope.target = Some(t.clone()),
            Layer::Steps { ids, begun: Some(i) } => {
                scope.step = Some(Step { index: *i as u32 + 1, total: ids.len() as u32, id: ids[*i] })
            }
            Layer::Steps { begun: None, .. } => {}
        }
    }
    scope
}

/// However drives and steps nest, every event arrives once, in order, carrying the innermost
/// drive and begun step; each step that begins is announced with the drive around it. A front
/// end that only implements `on_event` gets the same events with the scopes dropped.
#[hegel::test(test_cases = 300)]
fn events_carry_the_innermost_drive_and_step(tc: TestCase) {
    let layers: Vec<Layer> = (0..tc.draw(gs::integers::<usize>().max_value(4))).map(|_| layer(&tc)).collect();
    let events: Vec<Event> = (0..tc.draw(gs::integers::<usize>().max_value(5))).map(|_| event(&tc)).collect();
    let texts: Vec<String> = events.iter().map(|e| format!("{e:?}")).collect();

    let recorder = Recorder::default();
    nest(&Ctx::new(&recorder, CancelToken::new()), &layers, &events);
    let expected_scope = (!layers.is_empty()).then(|| scope_of(&layers));
    let got = recorder.events.into_inner().unwrap();
    assert_eq!(got, texts.iter().map(|t| (expected_scope.clone(), t.clone())).collect::<Vec<_>>());
    let announced: Vec<Scope> = (0..layers.len())
        .filter(|&k| matches!(layers[k], Layer::Steps { begun: Some(_), .. }))
        .map(|k| Scope { target: scope_of(&layers[..k]).target, step: scope_of(&layers[..=k]).step })
        .collect();
    assert_eq!(recorder.steps.into_inner().unwrap(), announced);

    let plain = Mutex::new(Vec::new());
    let front_end = |e: &Event| plain.lock().unwrap().push(format!("{e:?}"));
    nest(&Ctx::new(&front_end, CancelToken::new()), &layers, &events);
    assert_eq!(plain.into_inner().unwrap(), texts);
    // Discarding events is fine wherever they come from.
    nest(&Ctx::new(&NoProgress, CancelToken::new()), &layers, &events);
}

#[hegel::test_helper]
fn scope(tc: &TestCase) -> Scope {
    let target = tc.draw(gs::optional(gs::sampled_from(vec!["A", "B"]).map(String::from)));
    let step = tc.draw(gs::optional(gs::tuples!(
        gs::integers::<u32>().max_value(3),
        gs::integers::<u32>().max_value(3),
        gs::sampled_from(STEP_IDS.to_vec())
    )));
    Scope { target, step: step.map(|(index, total, id)| Step { index, total, id }) }
}

/// `within` fills only what the inner scope leaves unset, the empty scope changes nothing on
/// either side, and nesting three scopes gives the same however it's grouped.
#[hegel::test(test_cases = 200)]
fn within_fills_only_what_is_unset(tc: TestCase) {
    let (a, b, c) = (scope(&tc), scope(&tc), scope(&tc));
    let ab = a.within(&b);
    assert_eq!(ab.target, a.target.clone().or(b.target.clone()));
    assert_eq!(ab.step, a.step.or(b.step));
    assert_eq!(a.within(&Scope::default()), a);
    assert_eq!(Scope::default().within(&a), a);
    assert_eq!(a.within(&a), a);
    assert_eq!(ab.within(&c), a.within(&b.within(&c)));
}

/// How a test reaches a cancellation flag.
#[derive(Debug, Clone, Copy)]
enum Op {
    Clone(usize),
    /// A context for work that must finish: a flag of its own.
    Uncancellable(usize),
    /// The flag inside a drive's scope.
    ForTarget(usize),
    /// The flag inside a set of steps.
    Steps(usize),
    Cancel(usize),
}

/// Cancelling any handle fails every check sharing its flag (clones, a drive's or a step's
/// context) and stays cancelled, while an uncancellable context's flag is its own.
#[hegel::test(test_cases = 300)]
fn cancellation_reaches_every_context_sharing_the_flag(tc: TestCase) {
    let mut handles: Vec<(usize, CancelToken)> = vec![(0, CancelToken::new())];
    let mut cancelled = vec![false];
    for _ in 0..tc.draw(gs::integers::<usize>().max_value(12)) {
        let i = tc.draw(gs::integers::<usize>().max_value(handles.len() - 1));
        let op = match tc.draw(gs::integers::<u8>().max_value(4)) {
            0 => Op::Clone(i),
            1 => Op::Uncancellable(i),
            2 => Op::ForTarget(i),
            3 => Op::Steps(i),
            _ => Op::Cancel(i),
        };
        let ctx_of = |i: usize| Ctx::new(&NoProgress, handles[i].1.clone());
        match op {
            Op::Clone(i) => handles.push((handles[i].0, handles[i].1.clone())),
            Op::Uncancellable(i) => {
                let token = ctx_of(i).uncancellable().cancel;
                cancelled.push(false);
                handles.push((cancelled.len() - 1, token));
            }
            Op::ForTarget(i) => {
                let token = ctx_of(i).for_target("USB", |c| c.cancel.clone());
                handles.push((handles[i].0, token));
            }
            Op::Steps(i) => {
                let token = Steps::new(&ctx_of(i), vec!["copy"]).ctx().cancel;
                handles.push((handles[i].0, token));
            }
            Op::Cancel(i) => {
                handles[i].1.cancel();
                cancelled[handles[i].0] = true;
            }
        }
        for (flag, token) in &handles {
            let expected = if cancelled[*flag] { Err(Cancelled) } else { Ok(()) };
            assert_eq!(Ctx::new(&NoProgress, token.clone()).check(), expected, "after {op:?}");
            assert_eq!(token.is_cancelled(), cancelled[*flag]);
        }
    }
    assert_eq!(Ctx::silent().check(), Ok(()));
    assert_eq!(Cancelled.to_string(), "operation cancelled");
}

/// Of each kind of warning the first `limit` pass through as they come, in order, and `finish`
/// adds one summary per kind that went over, counting the rest and the total.
#[hegel::test(test_cases = 300)]
fn warnings_past_the_limit_are_summed_up(tc: TestCase) {
    let limit = tc.draw(gs::integers::<usize>().max_value(4));
    let kinds = tc.draw(gs::vecs(gs::sampled_from(vec!["unreadable files", "missing files", "bad tags"])).max_size(20));
    let seen = Mutex::new(Vec::new());
    let front_end = |e: &Event| match e {
        Event::Warning { message } => seen.lock().unwrap().push(message.clone()),
        other => panic!("not a warning: {other:?}"),
    };
    let ctx = Ctx::new(&front_end, CancelToken::new());
    let mut limiter = WarningLimiter::new(&ctx, limit);
    for (i, kind) in kinds.iter().enumerate() {
        limiter.warn(kind, format!("{kind} #{i}"));
    }
    limiter.finish();

    let mut counts: Vec<(&str, usize)> = Vec::new();
    let mut expected = Vec::new();
    for (i, kind) in kinds.iter().enumerate() {
        let n = match counts.iter_mut().find(|(k, _)| k == kind) {
            Some((_, n)) => n,
            None => {
                counts.push((kind, 0));
                &mut counts.last_mut().unwrap().1
            }
        };
        *n += 1;
        if *n <= limit {
            expected.push(format!("{kind} #{i}"));
        }
    }
    for (kind, n) in counts.iter().filter(|(_, n)| *n > limit) {
        expected.push(format!("... and {} more like that ({n} {kind} in total)", n - limit));
    }
    assert_eq!(seen.into_inner().unwrap(), expected);
}
