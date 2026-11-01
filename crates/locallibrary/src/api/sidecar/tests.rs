use super::*;
use crate::progress::{Event, Phase, Report, Scope, Step};

#[test]
fn progress_reads_as_the_event_with_its_scope() {
    let scope = Scope { target: Some("/Volumes/A".into()), step: Some(Step { index: 1, total: 2, id: "copy" }) };
    let event = Event::Item { phase: Phase::Copy, path: "x".into(), bytes: 1 };
    let item = ProgressEvent::new(&scope, Report::Event { event, items: Some(3) });
    assert_eq!(
        serde_json::to_value(&item).unwrap(),
        serde_json::json!({
            "event": "item", "phase": "copy", "path": "x", "bytes": 1, "items": 3,
            "target": "/Volumes/A", "step": { "index": 1, "total": 2, "id": "copy" },
        })
    );
    let step = ProgressEvent::new(&scope, Report::Step);
    assert_eq!(serde_json::to_value(&step).unwrap()["event"], "step");
    let finished = JobEvent::Finished { id: 4.into(), method: "sync.apply".into(), ok: false, kind: None };
    assert_eq!(
        serde_json::to_value(&finished).unwrap(),
        serde_json::json!({ "event": "job.finished", "id": 4, "method": "sync.apply", "ok": false })
    );
}

/// The bytes of a `progress` notification's params, which front ends and plugins read: one
/// event of each shape, in or out of a step and on a drive or not.
#[test]
fn progress_params_keep_their_bytes() {
    let scope = Scope { target: Some("/Volumes/A".into()), step: Some(Step { index: 1, total: 2, id: "copy" }) };
    let event = |event: Event, items: Option<u64>| ProgressEvent::new(&scope, Report::Event { event, items });
    let item = event(Event::Item { phase: Phase::Copy, path: "x".into(), bytes: 1 }, Some(3));
    let start = event(Event::PhaseStart { phase: Phase::Copy, items: 2, bytes: 9 }, None);
    let step = ProgressEvent::new(&scope, Report::Step);
    let bare = ProgressEvent::new(
        &Scope::default(),
        Report::Event { event: Event::Warning { message: "odd".into() }, items: None },
    );
    let bytes = |event: ProgressEvent| serde_json::to_string(&ProgressParams { id: 7.into(), event }).unwrap();
    assert_eq!(
        bytes(item),
        r#"{"id":7,"event":{"event":"item","phase":"copy","path":"x","bytes":1,"target":"/Volumes/A","step":{"index":1,"total":2,"id":"copy"},"items":3}}"#
    );
    assert_eq!(
        bytes(start),
        r#"{"id":7,"event":{"event":"phase_start","phase":"copy","items":2,"bytes":9,"target":"/Volumes/A","step":{"index":1,"total":2,"id":"copy"}}}"#
    );
    assert_eq!(
        bytes(step),
        r#"{"id":7,"event":{"event":"step","target":"/Volumes/A","step":{"index":1,"total":2,"id":"copy"}}}"#
    );
    assert_eq!(bytes(bare), r#"{"id":7,"event":{"event":"warning","message":"odd"}}"#);
}

use hegel::TestCase;
use hegel::generators as gs;

/// Parameter names a request and an action share now and then.
const KEYS: &[&str] = &["drive", "name", "force", "snapshot"];

#[hegel::test_helper]
fn params(tc: &TestCase) -> Map<String, Value> {
    let keys: Vec<&str> = tc.draw(gs::vecs(gs::sampled_from(KEYS.to_vec())).max_size(3));
    keys.into_iter().map(|k| (k.to_string(), Value::from(tc.draw(gs::integers::<i64>())))).collect()
}

#[hegel::test_helper]
fn action(tc: &TestCase) -> Action {
    Action {
        label: tc.draw(gs::sampled_from(vec!["Pull it", "Try again", "Choose a name"])).to_string(),
        method: tc.draw(gs::optional(gs::sampled_from(vec!["pull", "sync.plan"]))).map(String::from),
        params: params(tc),
        elevate: tc.draw(gs::booleans()),
    }
}

/// An error answering a request: the actions that send the request again (no method) become
/// calls to its method with its parameters and their own, theirs winning where both name one
/// (a request whose parameters aren't an object adds none), and the others stay as they were.
/// For a plan applied by id, which can't be sent again, those actions are dropped instead and the
/// others kept, in order.
#[hegel::test(test_cases = 200)]
fn resending_actions_name_the_request_or_go_with_a_used_plan(tc: TestCase) {
    let n = tc.draw(gs::integers::<usize>().max_value(4));
    let actions: Vec<Action> = (0..n).map(|_| action(&tc)).collect();
    let method = tc.draw(gs::sampled_from(vec!["push.apply", "drive.rename"]));
    let asked = match tc.draw(gs::booleans()) {
        true => Value::Object(params(&tc)),
        false => Value::from(vec![1, 2]),
    };
    let failure = || {
        let mut f = RpcFailure::from(Error::invalid("refused"));
        f.detail.actions = actions.clone();
        f
    };

    let resolved = failure().resolve(method, &asked).data().actions;
    let want: Vec<Action> = actions
        .iter()
        .map(|a| match &a.method {
            Some(_) => a.clone(),
            None => {
                let mut params = asked.as_object().cloned().unwrap_or_default();
                params.extend(a.params.clone());
                Action { method: Some(method.to_string()), params, ..a.clone() }
            }
        })
        .collect();
    assert_eq!(resolved, want);

    let used = failure().plan_used();
    let kept: Vec<Action> = actions.iter().filter(|a| a.method.is_some()).cloned().collect();
    assert_eq!(used.data().actions, kept);
    assert_eq!((used.code, used.kind, used.message.as_str()), (code::FAILED, "invalid", "refused"));
}
