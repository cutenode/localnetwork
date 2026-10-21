use loclib_progress::{Event, Phase};
use serde_json::json;

use super::*;
use crate::OpPlan;

fn report(event: Value) -> ProgressReport {
    serde_json::from_value::<ProgressParams>(json!({ "id": 1, "event": event })).unwrap().event
}

/// The sidecar's shape, read back: the event, its scope, and how many items it stands for.
#[test]
fn progress_reads_as_the_sidecar_sends_it() {
    let step = json!({ "index": 1, "total": 2, "id": "scan" });
    let item =
        json!({ "event": "item", "phase": "scan", "path": "a", "bytes": 0, "target": "USB", "step": step, "items": 3 });
    let ProgressReport::Known(e) = report(item.clone()) else { panic!("known") };
    assert!(matches!(e.event, Progressed::Event(Event::Item { phase: Phase::Scan, .. })));
    assert_eq!((e.target.as_deref(), e.items), (Some("USB"), Some(3)));
    assert_eq!(e.step, Some(StepInfo { index: 1, total: 2, id: "scan".into() }));
    assert_eq!(serde_json::to_value(&e).unwrap(), item);

    let ProgressReport::Known(e) = report(json!({ "event": "step", "step": step })) else { panic!("known") };
    assert!(matches!(e.event, Progressed::Step(_)));
}

/// A newer plugin's event or phase doesn't stop an older host reading progress: it reads as
/// unknown, to skip. A field it doesn't know is just ignored.
#[test]
fn progress_from_a_newer_plugin_reads_as_unknown() {
    for newer in [
        json!({ "event": "phase_start", "phase": "teleport", "items": 1, "bytes": 0 }),
        json!({ "event": "rewind", "seconds": 3 }),
        json!("eta 3s"),
    ] {
        assert!(matches!(report(newer.clone()), ProgressReport::Unknown(v) if v == newer));
    }
    let extra = json!({ "event": "warning", "message": "slow", "eta_secs": 3 });
    assert!(matches!(
        report(extra),
        ProgressReport::Known(ProgressEvent { event: Progressed::Event(Event::Warning { .. }), .. })
    ));
}

/// A plan worked out from no files says nothing of them, as plugins before `inputs` did.
#[test]
fn an_op_plan_without_inputs_leaves_them_out() {
    let plan = OpPlan {
        summary: "make it".into(),
        warnings: Vec::new(),
        edits: vec![json!({ "op": "create_playlist", "path": "x" })],
        inputs: Vec::new(),
    };
    let sent = serde_json::to_value(&plan).unwrap();
    assert_eq!(
        sent,
        json!({ "summary": "make it", "warnings": [], "edits": [{ "op": "create_playlist", "path": "x" }] })
    );
    assert_eq!(serde_json::from_value::<OpPlan>(sent).unwrap(), plan);
    let read = OpPlan { inputs: vec!["/music/set.txt".into()], ..plan };
    let sent = serde_json::to_value(&read).unwrap();
    assert_eq!(sent["inputs"], json!(["/music/set.txt"]));
    assert_eq!(serde_json::from_value::<OpPlan>(sent).unwrap(), read);
}

#[test]
fn protocol_versions_are_compatible_by_semvers_caret() {
    for (theirs, compatible) in
        [("0.2.0", true), ("0.2.99", true), ("0.1.0", false), ("0.3.0", false), ("1.2.0", false)]
    {
        assert_eq!(check_protocol(theirs).is_ok(), compatible, "{theirs}");
    }
}
