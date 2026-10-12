use super::*;
use crate::progress::{Phase, Step};

#[test]
fn progress_reads_as_the_event_with_its_scope() {
    let scope = Scope { target: Some("/Volumes/A".into()), step: Some(Step { index: 1, total: 2, id: "copy" }) };
    let item = ProgressEvent {
        event: Progressed::Event(Event::Item { phase: Phase::Copy, path: "x".into(), bytes: 1 }),
        scope: scope.clone(),
        items: Some(3),
    };
    assert_eq!(
        serde_json::to_value(&item).unwrap(),
        serde_json::json!({
            "event": "item", "phase": "copy", "path": "x", "bytes": 1, "items": 3,
            "target": "/Volumes/A", "step": { "index": 1, "total": 2, "id": "copy" },
        })
    );
    let step = ProgressEvent { event: Progressed::Step(StepStarted::new()), scope, items: None };
    assert_eq!(serde_json::to_value(&step).unwrap()["event"], "step");
    let finished = JobEvent::Finished { id: 4.into(), method: "sync.apply".into(), ok: false, kind: None };
    assert_eq!(
        serde_json::to_value(&finished).unwrap(),
        serde_json::json!({ "event": "job.finished", "id": 4, "method": "sync.apply", "ok": false })
    );
}
