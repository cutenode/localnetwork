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

    // A phase's start has items of its own: how many there are, not how many it stands for.
    let start = json!({ "event": "phase_start", "phase": "copy", "items": 2, "bytes": 9, "target": "USB" });
    let ProgressReport::Known(e) = report(start.clone()) else { panic!("known") };
    assert!(matches!(e.event, Progressed::Event(Event::PhaseStart { phase: Phase::Copy, items: 2, bytes: 9 })));
    assert_eq!((e.target.as_deref(), e.items), (Some("USB"), None));
    assert_eq!(serde_json::to_value(&e).unwrap(), start);
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

/// A fetch of any URL is a plain `GET` of it, as one that names only its URL reads.
#[hegel::test(test_cases = 50)]
fn prop_a_fetch_naming_only_its_url_is_a_get(tc: hegel::TestCase) {
    let url = tc.draw(hegel::generators::text().max_size(16));
    let fetch = Fetch::get(url.clone());
    assert_eq!(serde_json::to_value(&fetch).unwrap(), json!({ "url": url, "method": "GET" }));
    assert_eq!(serde_json::from_value::<Fetch>(json!({ "url": url })).unwrap(), fetch);
}

/// Progress read from text is the event it says, with its scope and how many items it stands
/// for; text that isn't JSON, whose scope isn't one (a target that isn't a string), or whose
/// event isn't one, isn't read as a progress event.
#[hegel::test(test_cases = 50)]
fn prop_progress_that_isnt_json_or_has_no_scope_isnt_an_event(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let items = tc.draw(gs::integers::<u64>().min_value(2).max_value(9));
    let item = json!({ "event": "item", "phase": "scan", "path": "a", "bytes": 0, "items": items }).to_string();
    let read = serde_json::from_str::<ProgressEvent>(&item).unwrap();
    assert!(matches!(read.event, Progressed::Event(Event::Item { .. })) && read.items == Some(items));
    let start = json!({ "event": "phase_start", "phase": "copy", "items": items, "bytes": 0 }).to_string();
    assert_eq!(serde_json::from_str::<ProgressEvent>(&start).unwrap().items, None);
    assert!(serde_json::from_str::<ProgressEvent>(r#"{ "event": "rewind" }"#).is_err());
    let cut = tc.draw(gs::integers::<usize>().min_value(1).max_value(20));
    let cut = &item[..item.len() - cut];
    assert!(serde_json::from_str::<ProgressEvent>(cut).is_err(), "{cut}");
    let target = tc.draw(gs::integers::<i64>());
    let e =
        serde_json::from_str::<ProgressEvent>(&json!({ "event": "step", "target": target }).to_string()).unwrap_err();
    assert!(e.to_string().starts_with("invalid type: integer"), "{e}");
}
