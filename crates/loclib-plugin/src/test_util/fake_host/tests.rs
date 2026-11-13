use std::io::{Read, Write};
use std::time::Duration;

use hegel::TestCase;
use hegel::generators as gs;
use indexmap::IndexMap;
use loclib_progress::{Event, Phase};
use serde_json::Map;

use super::*;
use crate::peer::tests::json_value;
use crate::{EventName, Hooks, Host, Progressed, RunCommand};

/// A plugin that makes the call its params name, leaves a file in its scratch folder, or starts
/// a phase, reads `drive.info` and waits to be cancelled; and tells the user of each event.
const MANIFEST: &str = r#"
id = "probe"
name = "Probe"
version = "0.1.0"
protocol = "^0.2"
capabilities = ["library:read", "net:example.com"]

[[contributes.commands]]
name = "call"
title = "Make the call its params name"
mode = "headless"

[[contributes.commands]]
name = "blob"
title = "Leave a file in the scratch folder"
mode = "headless"

[[contributes.commands]]
name = "wait"
title = "Wait until cancelled"
mode = "headless"
"#;

fn run(req: RunCommand, host: &Host) -> Result<Value, RpcError> {
    match req.command.as_str() {
        "call" => host.call(req.params["method"].as_str().unwrap_or_default(), &req.params["params"]),
        "blob" => {
            let scratch = req.context.scratch.ok_or_else(|| RpcError::invalid_params("no scratch folder"))?;
            std::fs::write(scratch.join("out.txt"), "kept").map_err(|e| RpcError::internal(e.to_string()))?;
            Ok(json!({ "blob": "out.txt", "scratch": scratch }))
        }
        _ => {
            let ctx = host.ctx();
            ctx.phase_start(Phase::Scan, 0, 0);
            host.call::<_, Value>("drive.info", &json!({}))?;
            loop {
                ctx.check()?;
                std::thread::sleep(Duration::from_millis(5));
            }
        }
    }
}

fn serve(input: impl Read + Send + 'static, output: impl Write + Send + 'static) {
    let hooks = Hooks::new().on(run).on_event(|e: EventNotify, host: &Host| {
        let message = format!("{} {}", e.event.name(), e.drive.unwrap_or_default());
        host.notify(&HostNotify { message, ..HostNotify::default() });
        Ok(())
    });
    crate::serve(MANIFEST, hooks, input, output);
}

fn call(method: &str, params: Value) -> Value {
    json!({ "command": "call", "params": { "method": method, "params": params } })
}

/// Greeted, it has the plugin's `hello`.
#[test]
fn it_greets_the_plugin() {
    let host = FakeHost::start(MANIFEST, serve);
    assert_eq!(host.hello().plugin.id, "probe");
    assert!(host.hello().features.iter().any(|f| f == "blobs"), "{:?}", host.hello().features);
    host.stop();
}

/// A plugin that describes itself as another manifest than its own is refused.
#[test]
#[should_panic(expected = "describe answers the plugin's manifest")]
fn a_plugin_describing_another_manifest_is_refused() {
    let other = MANIFEST.replace("name = \"Probe\"", "name = \"Other\"");
    let _ = FakeHost::start(&other, serve);
}

/// A read is answered by what the params make of it, as often as it's called; a read with no
/// answer fails with `method_not_found`, and one whose answer panics with `internal`. Every call
/// is kept.
#[test]
fn reads_are_answered_by_method_and_kept() {
    let host = FakeHost::start(MANIFEST, serve);
    host.on_read("drive.info", |p| match p["drive"].as_str() {
        Some("USB") => Ok(json!({ "name": "USB" })),
        _ => Err(RpcError::failed("not_found", "not found: drive")),
    });
    host.on_read("tracks.query", |_| panic!("not this one"));
    assert_eq!(
        host.request::<Value>(method::RUN_COMMAND, &call("drive.info", json!({ "drive": "USB" }))).unwrap(),
        json!({ "name": "USB" })
    );
    let e = host.request::<Value>(method::RUN_COMMAND, &call("drive.info", json!({ "drive": "SD" }))).unwrap_err();
    assert_eq!((e.kind(), e.message.as_str()), (Some("not_found"), "not found: drive"));
    let e = host.request::<Value>(method::RUN_COMMAND, &call("sets.get", json!({}))).unwrap_err();
    assert_eq!(e.code, crate::code::METHOD_NOT_FOUND, "{e:?}");
    let e = host.request::<Value>(method::RUN_COMMAND, &call("tracks.query", json!({}))).unwrap_err();
    assert_eq!(e.message, "the fake host's answer to tracks.query panicked");
    let methods: Vec<String> = host.calls().into_iter().map(|(m, _)| m).collect();
    assert_eq!(methods, ["drive.info", "drive.info", "sets.get", "tracks.query"]);
    host.stop();
}

/// A fetch is answered for its URL, else by an answer for `net/fetch`, else fails as a server
/// that couldn't be reached.
#[test]
fn fetches_are_answered_by_url() {
    let host = FakeHost::start(MANIFEST, serve);
    let page = Fetched { status: 200, headers: IndexMap::new(), body: Some("hi".into()), blob: None };
    host.fetch("https://example.com/a", Ok(page.clone()));
    host.fetch("https://example.com/b", Err(RpcError::failed("denied", "not that one")));
    let fetch =
        |url: &str| host.request::<Fetched>(method::RUN_COMMAND, &call(method::NET_FETCH, json!(Fetch::get(url))));
    assert_eq!(fetch("https://example.com/a").unwrap(), page);
    assert_eq!(fetch("https://example.com/b").unwrap_err().kind(), Some("denied"));
    let e = fetch("https://example.com/c").unwrap_err();
    assert_eq!(
        (e.kind(), e.message.as_str()),
        (Some("external"), "the fake host has no answer for https://example.com/c")
    );
    host.on_read(method::NET_FETCH, |p| Ok(json!({ "status": 404, "body": p["url"] })));
    assert_eq!(fetch("https://example.com/c").unwrap().status, 404);
    assert_eq!(fetch("https://example.com/a").unwrap(), page);
    host.stop();
}

/// Each hook request has a scratch folder of its own, which holds what the plugin left there
/// until the request is dropped.
#[test]
fn each_request_has_a_scratch_folder_until_its_dropped() {
    let host = FakeHost::start(MANIFEST, serve);
    let blob = json!({ "command": "blob" });
    let (first, second) = (host.send(method::RUN_COMMAND, &blob), host.send(method::RUN_COMMAND, &blob));
    let (a, b): (Value, Value) = (first.wait().unwrap(), second.wait().unwrap());
    let scratch = first.scratch().unwrap().to_path_buf();
    assert_eq!(a["scratch"], json!(scratch));
    assert_ne!(a["scratch"], b["scratch"]);
    assert_eq!(std::fs::read_to_string(scratch.join("out.txt")).unwrap(), "kept");
    drop(first);
    assert!(!scratch.exists());
    assert!(second.scratch().unwrap().join("out.txt").exists());
    host.stop();
}

/// A request cancelled stops with `cancelled`, having sent its progress.
#[test]
fn a_request_cancelled_stops_with_its_progress() {
    let host = FakeHost::start(MANIFEST, serve);
    let (read, started) = std::sync::mpsc::channel();
    host.on_read("drive.info", move |_| Ok(json!(read.send(()).is_ok())));
    let waiting = host.send(method::RUN_COMMAND, &json!({ "command": "wait" }));
    started.recv().unwrap();
    waiting.cancel();
    assert_eq!(waiting.wait::<Value>().unwrap_err().kind(), Some("cancelled"));
    let events: Vec<String> = waiting
        .progress()
        .into_iter()
        .map(|p| match p {
            ProgressReport::Known(e) => match e.event {
                Progressed::Event(Event::PhaseStart { phase, .. }) => format!("start {phase:?}"),
                other => format!("{other:?}"),
            },
            ProgressReport::Unknown(v) => v.to_string(),
        })
        .collect();
    assert_eq!(events, ["start Scan"]);
    host.stop();
}

/// `cancel_on` cancels the request the plugin is calling for while it waits on that call, once:
/// the next request goes on.
#[test]
fn cancel_on_cancels_the_request_under_way_once() {
    let host = FakeHost::start(MANIFEST, serve);
    host.read("drive.info", json!({ "name": "USB" })).cancel_on("drive.info");
    let info = call("drive.info", json!({ "drive": "USB" }));
    assert_eq!(host.request::<Value>(method::RUN_COMMAND, &info).unwrap_err().kind(), Some("cancelled"));
    assert_eq!(host.request::<Value>(method::RUN_COMMAND, &info).unwrap(), json!({ "name": "USB" }));
    host.stop();
}

/// What the plugin tells the user of each event is kept, in order, by the time it has stopped.
#[test]
fn notices_are_kept_in_order() {
    let host = FakeHost::start(MANIFEST, serve);
    for drive in ["A", "B"] {
        host.event(&EventNotify { event: EventName::DriveMounted, drive: Some(drive.into()), payload: json!({}) });
    }
    host.stop();
    let messages: Vec<String> = host.notices().into_iter().map(|n| n.message).collect();
    assert_eq!(messages, ["drive.mounted A", "drive.mounted B"]);
}

/// Any answer to any read reaches the plugin as it is, and the call is kept with its params as
/// the plugin gave them.
#[hegel::test(test_cases = 25)]
fn prop_canned_reads_reach_the_plugin_unchanged(tc: TestCase) {
    let read: String = tc.draw(gs::from_regex("[a-z]{1,8}\\.[a-z]{1,8}"));
    let names = tc.draw(gs::vecs(gs::sampled_from(vec!["drive", "ids", "id", "method"])).max_size(3).unique(true));
    let given: Map<String, Value> = names.into_iter().map(|k| (k.to_string(), json_value(&tc, 2))).collect();
    let answer = json_value(&tc, 3);
    let host = FakeHost::start(MANIFEST, serve);
    host.read(&read, &answer);
    let got: Value = host.request(method::RUN_COMMAND, &call(&read, Value::Object(given.clone()))).unwrap();
    assert_eq!(got, answer);
    assert_eq!(host.calls(), [(read, Value::Object(given))]);
    host.stop();
}
