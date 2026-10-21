use std::io::pipe;
use std::path::Path;
use std::sync::mpsc::Receiver;
use std::thread::JoinHandle;
use std::time::Duration;

use hegel::TestCase;
use hegel::generators::{self as gs, Generator};
use loclib_progress::{Event, Phase, Steps};
use serde_json::json;

use super::*;
use crate::{
    Analysis, AnalyzeTrack, Candidate, Candidates, DestinationKind, Exported, ExportedFile, FieldValue, HasBlobs,
    Missing, OpAnswer, OpPlan, Pending, PlanOp, ProvideCandidates, PublishSet, Published, RunCommand, View,
    WriteExport, code,
};

#[path = "../../examples/echo/plugin.rs"]
mod echo;
mod events;

/// The host's end of a plugin served on a thread, over pipes.
struct Session {
    host: Peer,
    incoming: Receiver<Incoming>,
    served: JoinHandle<()>,
}

fn start(hooks: Hooks) -> Session {
    start_with(echo::MANIFEST.into(), hooks)
}

/// Serving `manifest` (TOML).
fn start_with(manifest: String, hooks: Hooks) -> Session {
    let (plugin_in, to_plugin) = pipe().unwrap();
    let (from_plugin, plugin_out) = pipe().unwrap();
    let served = std::thread::spawn(move || serve(&manifest, hooks, plugin_in, plugin_out));
    let (host, incoming) = Peer::new(from_plugin, to_plugin);
    Session { host, incoming, served }
}

fn hello(protocol: &str, features: &[&str]) -> Value {
    json!({ "protocol": protocol, "host": { "name": "locallibrary", "version": "0.1.0" }, "features": features })
}

/// Started and past `hello`.
fn greeted(hooks: Hooks) -> Session {
    let s = start(hooks);
    s.host.request::<_, HelloReply>(method::HELLO, &hello(PROTOCOL_VERSION, &[])).unwrap();
    s
}

impl Session {
    fn call(&self, method: &str, params: Value) -> Result<Value, RpcError> {
        self.host.request(method, &params)
    }

    fn next(&self) -> Incoming {
        self.incoming.recv_timeout(Duration::from_secs(10)).expect("a message from the plugin")
    }

    /// Answers the plugin's next callback, which must be `tracks.query`, with a track titled
    /// after the drive asked about.
    fn answer_tracks_query(&self) {
        let Incoming::Request(req) = self.next() else { panic!("not a callback") };
        assert_eq!(req.method, "tracks.query");
        let drive = req.params["drive"].as_str().unwrap().to_string();
        self.host.respond(&req.id, Ok(page(&drive, &format!("/Contents/{drive}.mp3"))));
    }

    /// Closes the plugin's input: it stops.
    fn stop(self) {
        self.host.close();
        self.served.join().unwrap();
    }
}

/// `tracks.query`'s answer: one track, titled after `drive`.
fn page(drive: &str, file_path: &str) -> Value {
    let track =
        loclib_model::Track { title: format!("{drive} track"), file_path: file_path.into(), ..Default::default() };
    json!({ "total": 1, "total_duration_secs": 0, "tracks": [track] })
}

fn run(command: &str, params: Value, drive: Option<&str>) -> Value {
    json!({ "command": command, "params": params, "context": { "drive": drive } })
}

fn echo_manifest() -> Manifest {
    Manifest::parse(echo::MANIFEST).unwrap()
}

/// `hello` says who the plugin is and the features of the hooks it has, whatever the host's.
#[test]
fn hello_answers_who_the_plugin_is_and_ignores_unknown_features() {
    let s = start(echo::hooks());
    let reply: HelloReply = s.host.request(method::HELLO, &hello("0.2.7", &["telepathy"])).unwrap();
    let plugin = PluginInfo { id: "echo".into(), version: "0.1.0".into() };
    // And the SDK's own.
    let features = [
        "accounts",
        "commands",
        "destinations",
        "operations",
        "analyzers",
        "exporters",
        "providers",
        "blobs",
        "net",
        "choose",
    ];
    let features = features.map(String::from).to_vec();
    assert_eq!(reply, HelloReply { protocol: PROTOCOL_VERSION.into(), plugin, features });
    let again = s.call(method::HELLO, hello(PROTOCOL_VERSION, &[])).unwrap_err();
    assert_eq!((again.code, again.message.as_str()), (code::INVALID_REQUEST, "hello was sent already"));
    s.stop();
}

#[test]
fn an_incompatible_protocol_is_refused() {
    let s = start(echo::hooks());
    for theirs in ["0.1.0", "1.2.0", "one"] {
        let e = s.call(method::HELLO, hello(theirs, &[])).unwrap_err();
        assert_eq!((e.code, e.kind()), (code::FAILED, Some("incompatible")), "{theirs}");
        assert_eq!(e.message, format!("protocol {theirs:?} isn't compatible with 0.2.0"));
    }
    // Refused, it's still waiting for a hello.
    assert_eq!(s.call(method::DESCRIBE, json!({})).unwrap_err().message, "hello first");
    let e = s.call(method::HELLO, json!({ "protocol": "0.2.0" })).unwrap_err();
    assert_eq!((e.kind(), e.message.as_str()), (Some("invalid_params"), "invalid params: missing field `host`"));
    s.call(method::HELLO, hello("0.2.0", &[])).unwrap();
    s.stop();
}

#[test]
fn nothing_before_hello_is_answered_but_with_a_refusal() {
    let s = start(echo::hooks());
    s.host.notify("progress", &json!({}));
    for (method, params) in [
        (method::DESCRIBE, json!({})),
        (method::RUN_COMMAND, run("echo", json!({}), None)),
        ("tracks.query", json!({})),
    ] {
        let e = s.call(method, params).unwrap_err();
        assert_eq!((e.code, e.message.as_str()), (code::INVALID_REQUEST, "hello first"), "{method}");
    }
    s.stop();
}

#[test]
fn describe_answers_the_manifest() {
    let s = greeted(echo::hooks());
    let described: Manifest = s.host.request(method::DESCRIBE, &json!({})).unwrap();
    assert_eq!(described, echo_manifest());
    let e = s.call("shutdown", json!({})).unwrap_err();
    assert_eq!((e.code, e.message.as_str()), (code::METHOD_NOT_FOUND, "unknown method \"shutdown\""));
    s.stop();
}

#[test]
fn a_headless_command_answers_any_json() {
    let s = greeted(echo::hooks());
    let params = json!({ "anything": [1, "two"] });
    assert_eq!(s.call(method::RUN_COMMAND, run("echo", params.clone(), None)).unwrap(), params);
    s.stop();
}

/// A view command calls back for the drive's tracks while the host waits for its answer.
#[test]
fn a_view_command_calls_back_to_the_host() {
    let s = greeted(echo::hooks());
    let pending = s.host.send_request(method::RUN_COMMAND, &run("titles", json!({}), Some("USB"))).unwrap();
    s.answer_tracks_query();
    let view: View = pending.wait().unwrap();
    let rows = vec![vec![json!("USB track")]];
    assert_eq!(view, View::Table { title: Some("USB".into()), columns: vec!["Title".into()], rows });
    s.stop();
}

#[test]
fn an_analyzer_answers_its_fields() {
    let dir = tempfile::tempdir().unwrap();
    let audio = dir.path().join("a.mp3");
    std::fs::write(&audio, b"12345").unwrap();
    let s = greeted(echo::hooks());
    let track = json!({ "id": 3, "file_path": "/Contents/a.mp3", "audio_path": audio });
    let analysis: Analysis =
        s.host.request(method::ANALYZE_TRACK, &json!({ "analyzer": "size", "track": track })).unwrap();
    assert_eq!(analysis.fields["bytes"], FieldValue::Integer(5));

    let track = json!({ "id": 3, "file_path": "/Contents/b.mp3", "audio_path": dir.path().join("b.mp3") });
    let e = s.call(method::ANALYZE_TRACK, json!({ "analyzer": "size", "track": track })).unwrap_err();
    assert_eq!(e.kind(), Some("io"));
    // Where in the params a bad one is.
    let e = s.call(method::ANALYZE_TRACK, json!({ "analyzer": "size", "track": { "id": "x" } })).unwrap_err();
    assert_eq!(e.data.unwrap().fields["path"], "track.id");
    s.stop();
}

#[test]
fn an_exporter_writes_its_files() {
    let dir = tempfile::tempdir().unwrap();
    let output = dir.path().join("USB.m3u");
    let s = greeted(echo::hooks());
    let req = json!({ "exporter": "m3u", "source": { "drive": "USB" }, "output": output, "options": {} });
    let pending = s.host.send_request(method::WRITE_EXPORT, &req).unwrap();
    s.answer_tracks_query();
    let exported: Exported = pending.wait().unwrap();
    let text = "#EXTM3U\n/Contents/USB.mp3\n";
    let file = ExportedFile { path: output.clone(), what: "playlist".into(), bytes: text.len() as u64 };
    let counts = [("tracks".to_string(), 1)].into_iter().collect();
    assert_eq!(exported, Exported { files: vec![file], counts, ..Exported::default() });
    assert_eq!(std::fs::read_to_string(&output).unwrap(), text);
    s.stop();
}

/// A dry run says what it would write, so the host can ask before overwriting, and writes
/// nothing.
#[test]
fn an_exporters_dry_run_writes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let output = dir.path().join("USB.m3u");
    let s = greeted(echo::hooks());
    let source = json!({ "drive": "USB", "playlists": ["Sets/Friday"], "history": ["HISTORY 001"] });
    let req = json!({ "exporter": "m3u", "source": source, "output": output, "dry_run": true });
    let pending = s.host.send_request(method::WRITE_EXPORT, &req).unwrap();
    s.answer_tracks_query();
    let exported: Exported = pending.wait().unwrap();
    let paths: Vec<&Path> = exported.files.iter().map(|f| f.path.as_path()).collect();
    assert_eq!((paths, exported.files[0].bytes), (vec![output.as_path()], 26));
    assert!(!output.exists());
    s.stop();
}

#[test]
fn an_operation_plans_core_edits() {
    let s = greeted(echo::hooks());
    let req = json!({ "operation": "playlist", "params": { "name": "Warm up" }, "context": { "drive": "USB" } });
    let plan: OpPlan = s.host.request(method::PLAN_OP, &req).unwrap();
    assert_eq!(plan.summary, "make the playlist Warm up");
    assert_eq!(plan.edits, [json!({ "op": "create_playlist", "path": "Warm up" })]);
    let e = s.call(method::PLAN_OP, json!({ "operation": "playlist", "params": {} })).unwrap_err();
    assert_eq!(e.message, "invalid params: missing field `name`");
    s.stop();
}

/// An operation may ask the user first: its answer is `{"choose": ...}`, read back as one.
#[test]
fn an_operation_may_ask_first() {
    let ask = |req: PlanOp, host: &Host| -> Result<OpAnswer, RpcError> {
        let item = crate::ChoiceItem { label: "Warm up".into(), detail: None, value: json!("Warm up") };
        let choice = crate::Choice { title: "Which?".into(), param: "name".into(), multi: false, items: vec![item] };
        match req.params.get("name") {
            Some(name) => echo::plan_op(PlanOp { params: json!({ "name": name }), ..req }, host).map(OpAnswer::from),
            None => Ok(choice.into()),
        }
    };
    let s = greeted(Hooks::new().on(ask));
    let asked = s.call(method::PLAN_OP, json!({ "operation": "playlist", "params": {} })).unwrap();
    assert_eq!(
        asked,
        json!({ "choose": { "title": "Which?", "param": "name", "items": [{ "label": "Warm up", "value": "Warm up" }] } })
    );
    let OpAnswer::Choose { choose } = serde_json::from_value(asked).unwrap() else { panic!("a question") };
    let answered = choose.answered(json!({}), vec![choose.items[0].value.clone()]);
    let planned: OpAnswer =
        s.host.request(method::PLAN_OP, &json!({ "operation": "playlist", "params": answered })).unwrap();
    let OpAnswer::Plan(plan) = planned else { panic!("a plan") };
    assert_eq!(plan.summary, "make the playlist Warm up");
    s.stop();
}

/// A provider answers what it has for each track, and only for the kind its manifest declares.
#[test]
fn a_provider_answers_candidates_of_its_kind() {
    let s = greeted(echo::hooks());
    let tracks = json!([{ "id": 4, "artist": "Burial", "title": "Archangel" }, { "id": 5 }]);
    let req = json!({ "provider": "tags", "kind": "metadata", "tracks": tracks });
    let found: Candidates = s.host.request(method::PROVIDE, &req).unwrap();
    let metadata: Vec<_> = found.candidates.iter().map(|c| (c.track, c.metadata.clone().unwrap())).collect();
    let archangel = crate::TrackMetadata {
        title: Some("Archangel".into()),
        artist: Some("Burial".into()),
        ..crate::TrackMetadata::default()
    };
    assert_eq!(metadata, [(4, archangel), (5, crate::TrackMetadata::default())]);
    let e = s.call(method::PROVIDE, json!({ "provider": "tags", "kind": "artwork", "tracks": [] })).unwrap_err();
    assert_eq!((e.code, e.message.as_str()), (code::INVALID_PARAMS, "invalid params: tags provides no artwork"));
    s.stop();
}

/// A destination answers for its kind alone: echo's store has nothing yet, and its set
/// destination says what it would publish, then where it did.
#[test]
fn a_destination_answers_for_its_kind() {
    let s = greeted(echo::hooks());
    let hash = "ab".repeat(32);
    let missing: Missing =
        s.host.request(method::BLOBS_HAS, &json!({ "destination": "folder", "hashes": [hash] })).unwrap();
    assert_eq!(missing.missing, std::slice::from_ref(&hash));
    let recording = if cfg!(windows) { "C:\\set.wav" } else { "/set.wav" };
    let set = json!({ "id": "3m4n", "date": "2026-10-03" });
    let publish = |dry_run: bool| {
        let req = json!({ "destination": "nowhere", "set": set, "recording": recording, "tracklist": [{}, {}], "dry_run": dry_run });
        s.host.request::<_, Published>(method::SET_PUBLISH, &req)
    };
    assert_eq!(publish(true).unwrap(), Published { url: None, warnings: vec!["2 tracks would be published".into()] });
    assert_eq!(publish(false).unwrap().url.as_deref(), Some("https://sets.example/3m4n"));
    let e = s.call(method::BLOBS_HAS, json!({ "destination": "nowhere", "hashes": [] })).unwrap_err();
    assert_eq!(
        (e.code, e.message.as_str()),
        (code::INVALID_PARAMS, "invalid params: nowhere isn't a store destination")
    );
    let req = json!({ "destination": "folder", "set": set, "recording": recording, "tracklist": [] });
    let e = s.call(method::SET_PUBLISH, req).unwrap_err();
    assert_eq!((e.code, e.message.as_str()), (code::INVALID_PARAMS, "invalid params: folder isn't a set destination"));
    s.stop();
}

#[test]
fn hooks_run_only_for_what_the_manifest_declares() {
    let s = greeted(echo::hooks());
    let track = json!({ "id": 1, "file_path": "/a.mp3", "audio_path": "/a.mp3" });
    for (method, params, said) in [
        (method::RUN_COMMAND, run("nope", json!({}), None), "command \"nope\""),
        (method::ANALYZE_TRACK, json!({ "analyzer": "nope", "track": track }), "analyzer \"nope\""),
        (
            method::WRITE_EXPORT,
            json!({ "exporter": "nope", "source": { "drive": "A" }, "output": "/x" }),
            "exporter \"nope\"",
        ),
        (method::PLAN_OP, json!({ "operation": "nope" }), "operation \"nope\""),
        (method::PROVIDE, json!({ "provider": "nope", "kind": "metadata", "tracks": [] }), "provider \"nope\""),
    ] {
        let e = s.call(method, params).unwrap_err();
        assert_eq!(e.code, code::INVALID_PARAMS);
        assert_eq!(e.message, format!("invalid params: the manifest declares no {said}"));
    }
    s.stop();
}

/// A plugin has the features of the hooks it implements (and the SDK's own), and a hook it
/// doesn't implement isn't found.
#[test]
fn a_hook_the_plugin_doesnt_implement_isnt_found() {
    let s = start(Hooks::new().on(echo::analyze_track));
    let reply: HelloReply = s.host.request(method::HELLO, &hello(PROTOCOL_VERSION, &[])).unwrap();
    assert_eq!(reply.features, ["accounts", "analyzers", "blobs", "net", "choose"]);
    let track = json!({ "id": 1, "file_path": "/a.mp3", "audio_path": "/a.mp3" });
    for (method, params) in [
        (method::RUN_COMMAND, run("echo", json!({}), None)),
        (method::WRITE_EXPORT, json!({ "exporter": "m3u", "source": { "drive": "A" }, "output": "/x" })),
        (method::PLAN_OP, json!({ "operation": "playlist" })),
        (method::PROVIDE, json!({ "provider": "tags", "kind": "metadata", "tracks": [] })),
    ] {
        let e = s.call(method, params).unwrap_err();
        assert_eq!((e.code, e.message), (code::METHOD_NOT_FOUND, format!("unknown method {method:?}")));
    }
    let e = s.call(method::ANALYZE_TRACK, json!({ "analyzer": "size", "track": track })).unwrap_err();
    assert_eq!(e.kind(), Some("io"), "the one it has runs");
    let none = start(Hooks::new());
    let reply: HelloReply = none.host.request(method::HELLO, &hello(PROTOCOL_VERSION, &[])).unwrap();
    assert_eq!(reply.features, ["accounts", "blobs", "net", "choose"], "only the SDK's own");
    none.stop();
    s.stop();
}

/// A hook reads the manifest `serve` was given.
#[test]
fn a_hook_reads_the_manifest() {
    let s = greeted(Hooks::new().on(|_: RunCommand, host: &Host| Ok(Value::from(host.manifest().name.clone()))));
    assert_eq!(s.call(method::RUN_COMMAND, run("echo", json!({}), None)).unwrap(), "Echo");
    s.stop();
}

#[test]
fn a_hook_that_panics_still_answers() {
    let s = greeted(Hooks::new().on(|_: RunCommand, _: &Host| -> Result<Value, RpcError> { panic!("a bug") }));
    let e = s.call(method::RUN_COMMAND, run("echo", json!({}), None)).unwrap_err();
    assert_eq!((e.code, e.message.as_str()), (code::INTERNAL, "internal error while handling \"command/run\""));
    // The plugin carries on.
    s.call(method::DESCRIBE, json!({})).unwrap();
    s.stop();
}

/// The next notification, as JSON.
fn notification(s: &Session) -> Value {
    let Incoming::Notification { method, params } = s.next() else { panic!("not a notification") };
    assert_eq!(method, method::PROGRESS);
    params
}

#[test]
fn a_hook_sends_progress_and_stops_when_cancelled() {
    let s = greeted(echo::hooks());
    let pending = s.host.send_request(method::RUN_COMMAND, &run("wait", json!({}), None)).unwrap();
    let event = json!({ "event": "phase_start", "phase": "scan", "items": 0, "bytes": 0 });
    assert_eq!(notification(&s), json!({ "id": pending.id(), "event": event }));
    let item = json!({ "event": "item", "phase": "scan", "path": "a.mp3", "bytes": 0 });
    assert_eq!(notification(&s)["event"], item, "held back, then sent while the hook runs on");
    pending.cancel();
    assert_eq!(pending.wait::<Value>().unwrap_err(), RpcError::cancelled());
    s.stop();
}

/// Progress carries its step and drive, and items are coalesced, as the sidecar sends them.
#[test]
fn progress_carries_its_scope_and_items_are_coalesced() {
    let s = greeted(echo::hooks());
    let pending = s.host.send_request(method::RUN_COMMAND, &run("steps", json!({}), None)).unwrap();
    let step = json!({ "index": 1, "total": 1, "id": "scan" });
    assert_eq!(notification(&s)["event"], json!({ "event": "step", "step": step }));
    let item = json!({ "event": "item", "phase": "scan", "path": "c.mp3", "bytes": 0, "target": "USB", "step": step, "items": 3 });
    assert_eq!(notification(&s)["event"], item, "the held items go out before the answer");
    pending.wait::<Value>().unwrap();
    s.stop();
}

/// Cancelling a hook cancels the callback it's waiting for, which then fails at once.
#[test]
fn cancelling_a_hook_cancels_its_callback() {
    let s = greeted(echo::hooks());
    let pending = s.host.send_request(method::RUN_COMMAND, &run("titles", json!({}), Some("USB"))).unwrap();
    let Incoming::Request(callback) = s.next() else { panic!("not a callback") };
    pending.cancel();
    assert_eq!(pending.wait::<Value>().unwrap_err(), RpcError::cancelled());
    assert!(callback.cancel.is_cancelled(), "the plugin sent $/cancel for the callback first");
    // Answered late, it's dropped.
    s.host.respond(&callback.id, Ok(json!({ "tracks": [] })));
    s.call(method::DESCRIBE, json!({})).unwrap();
    s.stop();
}

/// Hooks run at once, each calling back to the host, which answers them out of order.
#[test]
fn concurrent_hooks_each_get_their_own_callbacks() {
    let s = greeted(echo::hooks());
    let drives = ["A", "B", "C", "D", "E", "F"];
    let pending: Vec<_> = drives
        .iter()
        .map(|d| s.host.send_request(method::RUN_COMMAND, &run("titles", json!({}), Some(d))).unwrap())
        .collect();
    let mut callbacks: Vec<Request> = drives
        .iter()
        .map(|_| match s.next() {
            Incoming::Request(r) => r,
            other => panic!("{other:?}"),
        })
        .collect();
    callbacks.reverse();
    for req in callbacks {
        let drive = req.params["drive"].as_str().unwrap().to_string();
        s.host.respond(&req.id, Ok(page(&drive, "/x")));
    }
    for (drive, p) in drives.iter().zip(pending) {
        let View::Table { rows, .. } = p.wait().unwrap() else { panic!("not a table") };
        assert_eq!(rows, [[json!(format!("{drive} track"))]]);
    }
    s.stop();
}

/// Closing the plugin's input cancels what's running, which answers before `serve` returns.
#[test]
fn closing_the_input_cancels_running_hooks() {
    let s = greeted(echo::hooks());
    let pending = s.host.send_request(method::RUN_COMMAND, &run("wait", json!({}), None)).unwrap();
    // Running: it has sent progress.
    s.next();
    s.host.close();
    assert_eq!(pending.wait::<Value>().unwrap_err(), RpcError::cancelled());
    s.served.join().unwrap();
}

// Properties.

/// The hooks a request names, and what each declares its contributions by.
const HOOKS: [(&str, &str); 7] = [
    (method::RUN_COMMAND, "command"),
    (method::ANALYZE_TRACK, "analyzer"),
    (method::WRITE_EXPORT, "exporter"),
    (method::PLAN_OP, "operation"),
    (method::PROVIDE, "provider"),
    (method::BLOBS_HAS, "destination"),
    (method::SET_PUBLISH, "destination"),
];

/// A version the host might speak: mostly 0.x and 1.x, sometimes not a version at all.
#[hegel::test_helper]
fn protocol(tc: &TestCase) -> String {
    match tc.draw(gs::integers::<u8>().max_value(5)) {
        0 => tc.draw(gs::sampled_from(vec!["", "one", "0.2", "0.2.0.0", "^0.2.0", "v0.2.0"])).to_string(),
        _ => {
            let [major, minor] = tc.draw(gs::arrays(gs::integers::<u8>().max_value(2)));
            format!("{major}.{minor}.{}", tc.draw(gs::integers::<u16>()))
        }
    }
}

/// Until the host's `hello` names a protocol the plugin speaks, every request is refused (an
/// incompatible one saying so, a malformed one saying where) and notifications are ignored. Once
/// it has, the plugin answers who it is, refuses another `hello`, describes itself with exactly
/// the manifest it serves, and doesn't know any other method.
#[hegel::test(test_cases = 25)]
fn hello_comes_first_and_then_the_plugin_describes_its_manifest(tc: TestCase) {
    let mut manifest = echo_manifest();
    manifest.id = tc.draw(gs::from_regex("[a-z][a-z0-9_-]{0,11}"));
    manifest.name = tc.draw(gs::text().min_size(1).max_size(12));
    manifest.version = format!("{}.{}.{}", tc.draw(gs::integers::<u8>()), tc.draw(gs::integers::<u8>()), 0);
    // JSON's strings are TOML's, but for DEL, which TOML has escaped.
    let quoted = |s: &str| json!(s).to_string().replace('\u{7f}', "\\u007f");
    let toml = echo::MANIFEST
        .replacen("id = \"echo\"", &format!("id = {}", quoted(&manifest.id)), 1)
        .replacen("name = \"Echo\"", &format!("name = {}", quoted(&manifest.name)), 1)
        .replacen("version = \"0.1.0\"", &format!("version = {}", quoted(&manifest.version)), 1);
    let s = start_with(toml, echo::hooks());
    let others =
        vec!["describe", "command/run", "track/analyze", "export/write", "op/plan", "tracks.query", "shutdown"];
    let plugin = PluginInfo { id: manifest.id.clone(), version: manifest.version.clone() };
    // And the SDK's own.
    let features = [
        "accounts",
        "commands",
        "destinations",
        "operations",
        "analyzers",
        "exporters",
        "providers",
        "blobs",
        "net",
        "choose",
    ];
    let features = features.map(String::from).to_vec();
    let greeting = HelloReply { protocol: PROTOCOL_VERSION.into(), plugin, features };
    let mut greeted = false;
    for _ in 0..tc.draw(gs::integers::<usize>().max_value(6)) {
        match tc.draw(gs::integers::<u8>().max_value(3)) {
            0 => s.host.notify(tc.draw(gs::sampled_from(vec!["progress", "$/cancel", "ping"])), &json!({})),
            1 => {
                let e = s.call(tc.draw(gs::sampled_from(others.clone())), json!({})).unwrap_err();
                assert_eq!((e.code, e.message.as_str()), (code::INVALID_REQUEST, "hello first"));
            }
            2 => {
                let e = s.call(method::HELLO, json!({ "protocol": PROTOCOL_VERSION })).unwrap_err();
                let said = (e.kind(), e.message.as_str());
                assert_eq!(said, (Some("invalid_params"), "invalid params: missing field `host`"));
            }
            _ => {
                let theirs = protocol(&tc);
                let features: Vec<String> = tc.draw(gs::vecs(gs::from_regex("[a-z]{1,8}")).max_size(2));
                let features: Vec<&str> = features.iter().map(String::as_str).collect();
                let speaks = semver::Version::parse(&theirs).is_ok_and(|v| (v.major, v.minor) == (0, 2));
                match s.host.request::<_, HelloReply>(method::HELLO, &hello(&theirs, &features)) {
                    Ok(reply) => {
                        assert!(speaks, "{theirs:?}");
                        assert_eq!(reply, greeting);
                        greeted = true;
                        break;
                    }
                    Err(e) => {
                        assert!(!speaks, "{theirs:?}");
                        assert_eq!((e.code, e.kind()), (code::FAILED, Some("incompatible")));
                        assert_eq!(e.message, format!("protocol {theirs:?} isn't compatible with {PROTOCOL_VERSION}"));
                    }
                }
            }
        }
    }
    if !greeted {
        let ours = format!("0.2.{}", tc.draw(gs::integers::<u16>()));
        assert_eq!(s.host.request::<_, HelloReply>(method::HELLO, &hello(&ours, &[])), Ok(greeting));
    }
    let again = s.call(method::HELLO, hello(PROTOCOL_VERSION, &[])).unwrap_err();
    assert_eq!((again.code, again.message.as_str()), (code::INVALID_REQUEST, "hello was sent already"));
    let described: Manifest = s.host.request(method::DESCRIBE, &json!({})).unwrap();
    assert_eq!(described, manifest);
    let unknown = tc.draw(
        gs::text()
            .max_size(10)
            .filter(|m: &String| !["hello", "describe"].contains(&m.as_str()) && !HOOKS.iter().any(|(h, _)| h == m)),
    );
    let e = s.call(&unknown, json!({})).unwrap_err();
    assert_eq!((e.code, e.message), (code::METHOD_NOT_FOUND, format!("unknown method {unknown:?}")));
    s.stop();
}

/// Hooks that answer each from its request alone, and with a script for the `steps` command
/// report progress: `warn:<message>`, `start`, `end`, `item:<path>` (all in the scan phase) or
/// `step:<one, two or three>`.
fn answers() -> Hooks {
    let run_command = |req: RunCommand, host: &Host| match req.command.as_str() {
        "steps" => {
            let ctx = host.ctx();
            let steps = Steps::new(&ctx, vec!["one", "two", "three"]);
            let ctx = steps.ctx();
            let script: Vec<String> = params(req.params)?;
            for entry in &script {
                match entry.split_once(':').unwrap_or((entry, "")) {
                    ("warn", message) => ctx.warn(message),
                    ("start", _) => ctx.phase_start(Phase::Scan, 0, 0),
                    ("end", _) => ctx.phase_end(Phase::Scan),
                    ("item", path) => ctx.emit(Event::Item { phase: Phase::Scan, path: path.into(), bytes: 0 }),
                    (_, step) => steps.begin(["one", "two", "three"].into_iter().find(|s| *s == step).unwrap()),
                }
            }
            Ok(Value::Null)
        }
        // Calls back for the drive named, and answers what the host does.
        "titles" => host.call("tracks.query", &json!({ "drive": req.context.drive })),
        _ => Ok(json!({ "command": req.command, "params": req.params })),
    };
    let analyze_track = |req: AnalyzeTrack, _: &Host| {
        Ok(Analysis { fields: [("bytes".to_string(), FieldValue::Integer(req.track.id.into()))].into_iter().collect() })
    };
    let write_export = |req: WriteExport, _: &Host| {
        let file = ExportedFile { path: req.output, what: req.exporter, bytes: 0 };
        Ok(Exported { files: vec![file], ..Exported::default() })
    };
    let plan_op = |req: PlanOp, _: &Host| {
        Ok(OpPlan { summary: req.operation, warnings: Vec::new(), edits: vec![req.params], inputs: Vec::new() })
    };
    let provide = |req: ProvideCandidates, _: &Host| {
        let found = req.tracks.iter().map(|t| Candidate {
            track: t.id,
            score: 1.0,
            source: req.provider.clone(),
            label: String::new(),
            artwork: None,
            metadata: None,
        });
        Ok(Candidates { candidates: found.collect(), warnings: Vec::new() })
    };
    let has_blobs = |req: HasBlobs, _: &Host| Ok(Missing { missing: req.hashes });
    let publish_set = |req: PublishSet, _: &Host| Ok(Published { url: Some(req.set.id), warnings: Vec::new() });
    let hooks = Hooks::new().on(run_command).on(analyze_track).on(write_export).on(plan_op).on(provide);
    hooks.on(has_blobs).on(publish_set)
}

/// Hooks that panic, every one.
fn panics_everywhere() -> Hooks {
    fn bug<H, A>(_: H, _: &Host) -> Result<A, RpcError> {
        panic!("a bug");
    }
    let hooks = Hooks::new().on(bug::<RunCommand, Value>).on(bug::<AnalyzeTrack, Analysis>);
    let hooks = hooks.on(bug::<WriteExport, Exported>).on(bug::<PlanOp, OpAnswer>);
    hooks.on(bug::<ProvideCandidates, Candidates>).on(bug::<HasBlobs, Missing>).on(bug::<PublishSet, Published>)
}

/// Any hooks, run at once: one the plugin doesn't implement isn't found, whatever its params. The
/// others answer only for a contribution the manifest declares (a provider or a destination, of
/// its kind), with
/// params that read: params that don't are refused saying where, an undeclared name saying what
/// isn't declared, and one that panics still answers. The rest answer what the plugin does.
#[hegel::test(test_cases = 25)]
fn hooks_answer_only_for_what_the_manifest_declares(tc: TestCase) {
    let plugin = tc.draw(gs::sampled_from(vec!["answers", "bare", "panics"]));
    let s = match plugin {
        "answers" => greeted(answers()),
        "bare" => greeted(Hooks::new()),
        _ => greeted(panics_everywhere()),
    };
    let manifest = echo_manifest();
    let c = &manifest.contributes;
    let destinations: Vec<String> = c.destinations.iter().map(|x| x.name.clone()).collect();
    let of_kind = |kind| c.destinations.iter().find(|x| x.kind == kind).map(|x| x.name.clone()).unwrap();
    let (store, set) = (of_kind(DestinationKind::Store), of_kind(DestinationKind::Set));
    let declared: [Vec<String>; 7] = [
        c.commands.iter().filter(|x| !["steps", "titles"].contains(&x.name.as_str())).map(|x| x.name.clone()).collect(),
        c.analyzers.iter().map(|x| x.name.clone()).collect(),
        c.exporters.iter().map(|x| x.format.clone()).collect(),
        c.operations.iter().map(|x| x.name.clone()).collect(),
        c.providers.iter().map(|x| x.name.clone()).collect(),
        destinations.clone(),
        destinations,
    ];
    let mut sent = Vec::new();
    for _ in 0..tc.draw(gs::integers::<usize>().min_value(1).max_value(6)) {
        let hook = tc.draw(gs::integers::<usize>().max_value(6));
        let (method, what) = HOOKS[hook];
        let name = match tc.draw(gs::booleans()) {
            true => tc.draw(gs::sampled_from(declared[hook].clone())),
            false => {
                tc.draw(gs::from_regex("[a-z]{1,6}").filter(|n: &String| !declared.iter().flatten().any(|d| d == n)))
            }
        };
        let id = tc.draw(gs::integers::<u32>());
        let echoed = json!({ "n": tc.draw(gs::integers::<i32>()) });
        let output = if cfg!(windows) { "C:\\out" } else { "/out" };
        let (params, answer) = match hook {
            0 => (run(&name, echoed.clone(), None), json!({ "command": name, "params": echoed })),
            1 => {
                let track = json!({ "id": id, "file_path": "/Contents/a.mp3", "audio_path": "/a.mp3" });
                (json!({ "analyzer": name, "track": track }), json!({ "fields": { "bytes": id } }))
            }
            2 => {
                let req = json!({ "exporter": name, "source": { "drive": "USB" }, "output": output });
                (req, json!({ "files": [{ "path": output, "what": name, "bytes": 0 }], "warnings": [] }))
            }
            3 => {
                let plan = json!({ "summary": name, "warnings": [], "edits": [echoed] });
                (json!({ "operation": name, "params": echoed }), plan)
            }
            4 => {
                // Echo's provider gives tags.
                let kind = tc.draw(gs::sampled_from(vec!["metadata", "artwork"]));
                let found = json!({ "candidates": [{ "track": id, "score": 1.0, "source": name, "label": "" }] });
                (json!({ "provider": name, "kind": kind, "tracks": [{ "id": id }] }), found)
            }
            5 => {
                let hash = format!("{id:064x}");
                (json!({ "destination": name, "hashes": [hash] }), json!({ "missing": [hash] }))
            }
            _ => {
                let set = json!({ "id": format!("set{id}"), "date": "2026-10-03" });
                let req = json!({ "destination": name, "set": set, "recording": output, "tracklist": [{}] });
                (req, json!({ "url": format!("set{id}") }))
            }
        };
        // Params that don't read, and where `invalid_params` says they're wrong.
        let (params, path) = match tc.draw(gs::weighted_booleans(0.25)) {
            false => (params, None),
            true => match hook {
                0 => (json!({ "command": 5 }), Some("command")),
                1 => (json!({ "analyzer": name, "track": { "id": "x" } }), Some("track.id")),
                2 => (json!({ "exporter": name, "source": { "drive": "USB" } }), None),
                3 => (json!({ "operation": [] }), Some("operation")),
                4 => (json!({ "provider": name, "kind": "lyrics", "tracks": [] }), Some("kind")),
                5 => (json!({ "destination": name, "hashes": "all" }), Some("hashes")),
                _ => (json!({ "destination": name, "set": { "id": 5 }, "recording": output }), Some("set.id")),
            },
        };
        let pending = s.host.send_request(method, &params).unwrap();
        let is_declared = declared[hook].contains(&name);
        // A destination of the other kind.
        let other_kind = match hook {
            5 => (name == set).then_some("store"),
            6 => (name == store).then_some("set"),
            _ => None,
        };
        sent.push((method, what, name, is_declared, other_kind, params, path, answer, pending));
    }
    for (method, what, name, declared, other_kind, params, path, answer, pending) in sent {
        let got = pending.wait::<Value>();
        let malformed = path.is_some() || params.get("output").is_none() && method == method::WRITE_EXPORT;
        match got {
            Err(e) if plugin == "bare" => {
                assert_eq!((e.code, e.message), (code::METHOD_NOT_FOUND, format!("unknown method {method:?}")))
            }
            Err(e) if malformed => {
                assert_eq!(e.kind(), Some("invalid_params"), "{e}");
                let at = e.data.unwrap().fields.get("path").and_then(Value::as_str).map(str::to_string);
                assert_eq!(at.as_deref(), path);
            }
            Err(e) if !declared => {
                assert_eq!(
                    (e.code, e.message),
                    (code::INVALID_PARAMS, format!("invalid params: the manifest declares no {what} {name:?}"))
                );
            }
            Err(e) if params["kind"] == "artwork" => {
                assert_eq!(
                    (e.code, e.message),
                    (code::INVALID_PARAMS, format!("invalid params: {name} provides no artwork"))
                );
            }
            Err(e) if let Some(kind) = other_kind => {
                let said = format!("invalid params: {name} isn't a {kind} destination");
                assert_eq!((e.code, e.message), (code::INVALID_PARAMS, said));
            }
            Err(e) if plugin == "panics" => {
                assert_eq!((e.code, e.message), (code::INTERNAL, format!("internal error while handling {method:?}")));
            }
            got => assert_eq!(got, Ok(answer), "{method} {params}"),
        }
    }
    s.stop();
}

/// A step, as progress names it.
fn step(id: &str) -> Value {
    let index = ["one", "two", "three"].iter().position(|s| *s == id).unwrap() + 1;
    json!({ "index": index, "total": 3, "id": id })
}

/// What a hook reports reaches the host as it happened, in order and before the hook's answer,
/// each event with the step it's in: everything but items at once, and items coalesced, so a run
/// of them comes as one or more reports that add up to it and end with its last.
#[hegel::test(test_cases = 25)]
fn a_hooks_progress_reaches_the_host_in_order_before_its_answer(tc: TestCase) {
    let s = greeted(answers());
    let entry = gs::one_of(vec![
        gs::from_regex("warn:[a-z ]{0,6}").boxed(),
        gs::sampled_from(vec!["start".to_string(), "end".to_string()]).boxed(),
        gs::from_regex("item:[a-z]{1,4}\\.mp3").boxed(),
        gs::sampled_from(vec!["step:one".to_string(), "step:two".to_string(), "step:three".to_string()]).boxed(),
    ]);
    let script: Vec<String> = tc.draw(gs::vecs(entry).max_size(12));
    // What the host should see, items merged.
    let mut expected: Vec<Value> = Vec::new();
    let mut current: Option<Value> = None;
    for entry in &script {
        let (kind, rest) = entry.split_once(':').unwrap_or((entry, ""));
        let mut event = match kind {
            "warn" => json!({ "event": "warning", "message": rest }),
            "start" => json!({ "event": "phase_start", "phase": "scan", "items": 0, "bytes": 0 }),
            "end" => json!({ "event": "phase_end", "phase": "scan" }),
            "item" => json!({ "event": "item", "phase": "scan", "path": rest, "bytes": 0 }),
            _ => {
                current = Some(step(rest));
                expected.push(json!({ "event": "step", "step": step(rest) }));
                continue;
            }
        };
        if let Some(step) = &current {
            event["step"] = step.clone();
        }
        expected.push(event);
    }
    let pending = s.host.send_request_reporting(method::RUN_COMMAND, &run("steps", json!(script), None)).unwrap();
    let answer = pending.wait_timeout::<Value>(Duration::from_secs(60)).expect("answered");
    assert_eq!(answer, Ok(Value::Null));
    let got: Vec<Value> = pending.progress().map(|r| serde_json::to_value(r).unwrap()).collect();
    assert_eq!(merged(got), merged(expected), "{script:?}");
    s.stop();
}

/// `reports` with each run of item reports one report, of how many items they stood for together
/// and the last one's path.
fn merged(reports: Vec<Value>) -> Vec<Value> {
    let mut out: Vec<Value> = Vec::new();
    for mut r in reports {
        let items = r.as_object_mut().unwrap().remove("items").and_then(|n| n.as_u64()).unwrap_or(1);
        if let Some(last) = out.last_mut().filter(|l| l["event"] == "item" && r["event"] == "item") {
            let before = last["items"].as_u64().unwrap();
            r["items"] = (before + items).into();
            *last = r;
            continue;
        }
        if r["event"] == "item" {
            r["items"] = items.into();
        }
        out.push(r);
    }
    out
}

/// Cancelling hooks reaches each, and the callback it's waiting for: it fails with `cancelled`
/// at once, and the plugin cancels the callback too, while the other hooks get their callbacks'
/// answers. Hooks still waiting when the host closes the plugin's input fail with `closed`, and
/// the plugin stops once they have answered.
#[hegel::test(test_cases = 25)]
fn cancelling_a_hook_reaches_it_and_its_callback(tc: TestCase) {
    let s = greeted(answers());
    let hooks = tc.draw(gs::integers::<usize>().min_value(1).max_value(4));
    let closing = tc.draw(gs::integers::<usize>().max_value(2));
    let start =
        |n: usize| s.host.send_request(method::RUN_COMMAND, &run("titles", json!({}), Some(&format!("d{n}")))).unwrap();
    let pending: Vec<Pending> = (0..hooks).map(start).collect();
    let mut callbacks: Vec<Option<Request>> = (0..hooks).map(|_| None).collect();
    for _ in 0..hooks {
        let Incoming::Request(req) = s.next() else { panic!("not a callback") };
        let n: usize = req.params["drive"].as_str().unwrap()[1..].parse().unwrap();
        callbacks[n] = Some(req);
    }
    let cancelled = tc.draw(gs::vecs(gs::integers::<usize>().max_value(hooks - 1)).unique(true).max_size(hooks));
    for (n, (p, callback)) in pending.into_iter().zip(callbacks).enumerate() {
        let callback = callback.unwrap();
        if cancelled.contains(&n) {
            p.cancel();
            assert_eq!(p.wait::<Value>(), Err(RpcError::cancelled()));
            assert!(callback.cancel.is_cancelled(), "the plugin cancelled its callback first");
            // Answered late, it's dropped.
            s.host.respond(&callback.id, Ok(json!(n)));
        } else {
            s.host.respond(&callback.id, Ok(json!(n)));
            assert_eq!(p.wait::<usize>(), Ok(n));
        }
    }
    let waiting: Vec<Pending> = (hooks..hooks + closing).map(start).collect();
    for _ in &waiting {
        assert!(matches!(s.next(), Incoming::Request(_)), "a callback");
    }
    s.host.close();
    for p in waiting {
        assert_eq!(p.wait::<Value>(), Err(RpcError::closed()));
    }
    s.served.join().unwrap();
}

/// A hook fetches through the host (`net/fetch`, its request as the host gets it), and sends a
/// view's rows and items in parts, which reach the host's request before its answer, in order.
#[test]
fn a_hook_fetches_through_the_host_and_sends_its_rows_in_parts() {
    let s = greeted(Hooks::new().on(|_: RunCommand, host: &Host| {
        host.partial_rows(vec![vec![json!("first")]]);
        host.partial_items(vec![crate::ListItem { label: "an item".into(), detail: None }]);
        let fetched =
            host.fetch(&Fetch { into_blob: Some("/scratch/a.jpg".into()), ..Fetch::get("https://a.org/x") })?;
        host.partial_rows(vec![vec![json!("second")], vec![json!(fetched.status)]]);
        Ok(serde_json::to_value(fetched).expect("JSON"))
    }));
    let pending = s.host.send_request_reporting(method::RUN_COMMAND, &run("echo", json!({}), None)).unwrap();
    let Incoming::Request(req) = s.next() else { panic!("not a callback") };
    assert_eq!(req.method, method::NET_FETCH);
    assert_eq!(req.params, json!({ "url": "https://a.org/x", "method": "GET", "into_blob": "/scratch/a.jpg" }));
    let answer = json!({ "status": 200, "headers": { "content-type": "image/jpeg" }, "blob": "/scratch/a.jpg" });
    s.host.respond(&req.id, Ok(answer.clone()));
    assert_eq!(pending.wait_timeout::<Value>(Duration::from_secs(10)).unwrap().unwrap(), answer);
    let parts: Vec<(Vec<Vec<Value>>, Vec<String>)> =
        pending.partials().map(|p| (p.rows, p.items.into_iter().map(|i| i.label).collect())).collect();
    assert_eq!(
        parts,
        [
            (vec![vec![json!("first")]], vec![]),
            (vec![], vec!["an item".to_string()]),
            (vec![vec![json!("second")], vec![json!(200)]], vec![])
        ]
    );
    s.stop();
}
