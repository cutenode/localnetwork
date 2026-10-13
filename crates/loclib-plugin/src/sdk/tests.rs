use std::io::pipe;
use std::sync::mpsc::Receiver;
use std::thread::JoinHandle;
use std::time::Duration;

use serde_json::json;

use super::*;
use crate::{View, code};

#[path = "../../examples/echo/plugin.rs"]
mod echo;

/// The host's end of a plugin served on a thread, over pipes.
struct Session {
    host: Peer,
    incoming: Receiver<Incoming>,
    served: JoinHandle<()>,
}

fn start(plugin: impl Plugin) -> Session {
    let (plugin_in, to_plugin) = pipe().unwrap();
    let (from_plugin, plugin_out) = pipe().unwrap();
    let served = std::thread::spawn(move || serve(&echo::manifest(), plugin, plugin_in, plugin_out));
    let (host, incoming) = Peer::new(from_plugin, to_plugin);
    Session { host, incoming, served }
}

fn hello(protocol: &str, features: &[&str]) -> Value {
    json!({ "protocol": protocol, "host": { "name": "locallibrary", "version": "0.1.0" }, "features": features })
}

/// Started and past `hello`.
fn greeted(plugin: impl Plugin) -> Session {
    let s = start(plugin);
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
        let track = json!({ "title": format!("{drive} track"), "file_path": format!("/Contents/{drive}.mp3") });
        self.host.respond(&req.id, Ok(json!({ "total": 1, "total_duration_secs": 0, "tracks": [track] })));
    }

    /// Closes the plugin's input: it stops.
    fn stop(self) {
        self.host.close();
        self.served.join().unwrap();
    }
}

fn run(command: &str, params: Value, drive: Option<&str>) -> Value {
    json!({ "command": command, "params": params, "context": { "drive": drive } })
}

#[test]
fn hello_answers_who_the_plugin_is_and_ignores_unknown_features() {
    let s = start(echo::Echo);
    let reply: HelloReply = s.host.request(method::HELLO, &hello("0.1.7", &["telepathy"])).unwrap();
    let plugin = PluginInfo { id: "echo".into(), version: "0.1.0".into() };
    assert_eq!(reply, HelloReply { protocol: PROTOCOL_VERSION.into(), plugin, features: vec![] });
    let again = s.call(method::HELLO, hello(PROTOCOL_VERSION, &[])).unwrap_err();
    assert_eq!((again.code, again.message.as_str()), (code::INVALID_REQUEST, "hello was sent already"));
    s.stop();
}

#[test]
fn an_incompatible_protocol_is_refused() {
    let s = start(echo::Echo);
    for theirs in ["0.2.0", "1.1.0", "one"] {
        let e = s.call(method::HELLO, hello(theirs, &[])).unwrap_err();
        assert_eq!((e.code, e.kind()), (code::FAILED, Some("incompatible")), "{theirs}");
        assert_eq!(e.message, format!("protocol {theirs:?} isn't compatible with 0.1.0"));
    }
    // Refused, it's still waiting for a hello.
    assert_eq!(s.call(method::DESCRIBE, json!({})).unwrap_err().message, "hello first");
    let e = s.call(method::HELLO, json!({ "protocol": "0.1.0" })).unwrap_err();
    assert_eq!((e.kind(), e.message.as_str()), (Some("invalid_params"), "invalid params: missing field `host`"));
    s.call(method::HELLO, hello("0.1.0", &[])).unwrap();
    s.stop();
}

#[test]
fn nothing_before_hello_is_answered_but_with_a_refusal() {
    let s = start(echo::Echo);
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
    let s = greeted(echo::Echo);
    let described: Manifest = s.host.request(method::DESCRIBE, &json!({})).unwrap();
    assert_eq!(described, echo::manifest());
    let e = s.call("shutdown", json!({})).unwrap_err();
    assert_eq!((e.code, e.message.as_str()), (code::METHOD_NOT_FOUND, "unknown method \"shutdown\""));
    s.stop();
}

#[test]
fn a_headless_command_answers_any_json() {
    let s = greeted(echo::Echo);
    let params = json!({ "anything": [1, "two"] });
    assert_eq!(s.call(method::RUN_COMMAND, run("echo", params.clone(), None)).unwrap(), params);
    s.stop();
}

/// A view command calls back for the drive's tracks while the host waits for its answer.
#[test]
fn a_view_command_calls_back_to_the_host() {
    let s = greeted(echo::Echo);
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
    let s = greeted(echo::Echo);
    let track = json!({ "id": 3, "file_path": "/Contents/a.mp3", "audio_path": audio });
    let analysis: Analysis =
        s.host.request(method::ANALYZE_TRACK, &json!({ "analyzer": "size", "track": track })).unwrap();
    assert_eq!(analysis.fields["bytes"], json!(5));

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
    let s = greeted(echo::Echo);
    let req = json!({ "exporter": "m3u", "source": { "drive": "USB" }, "output": output, "options": {} });
    let pending = s.host.send_request(method::WRITE_EXPORT, &req).unwrap();
    s.answer_tracks_query();
    let exported: Exported = pending.wait().unwrap();
    assert_eq!(exported, Exported { files: vec![output.clone()], warnings: vec![] });
    assert_eq!(std::fs::read_to_string(&output).unwrap(), "#EXTM3U\n/Contents/USB.mp3\n");
    s.stop();
}

/// A dry run says what it would write, so the host can ask before overwriting, and writes
/// nothing.
#[test]
fn an_exporters_dry_run_writes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let output = dir.path().join("USB.m3u");
    let s = greeted(echo::Echo);
    let source = json!({ "drive": "USB", "playlists": ["Sets/Friday"], "history": ["HISTORY 001"] });
    let req = json!({ "exporter": "m3u", "source": source, "output": output, "dry_run": true });
    let exported: Exported = s.host.request(method::WRITE_EXPORT, &req).unwrap();
    assert_eq!(exported.files, std::slice::from_ref(&output));
    assert!(!output.exists());
    s.stop();
}

#[test]
fn an_operation_plans_core_edits() {
    let s = greeted(echo::Echo);
    let req = json!({ "operation": "playlist", "params": { "name": "Warm up" }, "context": { "drive": "USB" } });
    let plan: OpPlan = s.host.request(method::PLAN_OP, &req).unwrap();
    assert_eq!(plan.summary, "make the playlist Warm up");
    assert_eq!(plan.edits, [json!({ "op": "create_playlist", "path": "Warm up" })]);
    let e = s.call(method::PLAN_OP, json!({ "operation": "playlist", "params": {} })).unwrap_err();
    assert_eq!(e.message, "invalid params: missing field `name`");
    s.stop();
}

#[test]
fn hooks_run_only_for_what_the_manifest_declares() {
    let s = greeted(echo::Echo);
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
    ] {
        let e = s.call(method, params).unwrap_err();
        assert_eq!(e.code, code::INVALID_PARAMS);
        assert_eq!(e.message, format!("invalid params: the manifest declares no {said}"));
    }
    s.stop();
}

/// A plugin that implements no hooks.
struct Bare;

impl Plugin for Bare {}

#[test]
fn a_hook_the_plugin_doesnt_implement_isnt_found() {
    let s = greeted(Bare);
    let track = json!({ "id": 1, "file_path": "/a.mp3", "audio_path": "/a.mp3" });
    for (method, params) in [
        (method::RUN_COMMAND, run("echo", json!({}), None)),
        (method::ANALYZE_TRACK, json!({ "analyzer": "size", "track": track })),
        (method::WRITE_EXPORT, json!({ "exporter": "m3u", "source": { "drive": "A" }, "output": "/x" })),
        (method::PLAN_OP, json!({ "operation": "playlist" })),
    ] {
        let e = s.call(method, params).unwrap_err();
        assert_eq!((e.code, e.message), (code::METHOD_NOT_FOUND, format!("unknown method {method:?}")));
    }
    s.stop();
}

struct Panics;

impl Plugin for Panics {
    fn run_command(&self, _: RunCommand, _: &Host) -> Result<Value, RpcError> {
        panic!("a bug");
    }
}

#[test]
fn a_hook_that_panics_still_answers() {
    let s = greeted(Panics);
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
    let s = greeted(echo::Echo);
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
    let s = greeted(echo::Echo);
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
    let s = greeted(echo::Echo);
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
    let s = greeted(echo::Echo);
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
        let tracks = json!({ "tracks": [{ "title": format!("{drive} track"), "file_path": "/x" }] });
        s.host.respond(&req.id, Ok(tracks));
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
    let s = greeted(echo::Echo);
    let pending = s.host.send_request(method::RUN_COMMAND, &run("wait", json!({}), None)).unwrap();
    // Running: it has sent progress.
    s.next();
    s.host.close();
    assert_eq!(pending.wait::<Value>().unwrap_err(), RpcError::cancelled());
    s.served.join().unwrap();
}

/// The example plugin, as its own process over real stdio.
#[test]
fn the_example_plugin_runs_as_a_child_process() {
    let deps = std::env::current_exe().unwrap();
    let exe =
        deps.parent().unwrap().parent().unwrap().join("examples").join(format!("echo{}", std::env::consts::EXE_SUFFIX));
    assert!(exe.exists(), "cargo builds the examples with the tests: {}", exe.display());
    let mut child = std::process::Command::new(exe)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let (host, incoming) = Peer::new(child.stdout.take().unwrap(), child.stdin.take().unwrap());
    let reply: HelloReply = host.request(method::HELLO, &hello(PROTOCOL_VERSION, &[])).unwrap();
    assert_eq!(reply.plugin.id, "echo");
    let described: Manifest = host.request(method::DESCRIBE, &json!({})).unwrap();
    assert_eq!(described, echo::manifest());
    let answer: Value = host.request(method::RUN_COMMAND, &run("echo", json!({ "n": 1 }), None)).unwrap();
    assert_eq!(answer, json!({ "n": 1 }));
    // Closing its stdin while a hook runs: the hook is cancelled and answers before it exits.
    let waiting = host.send_request(method::RUN_COMMAND, &run("wait", json!({}), None)).unwrap();
    assert!(
        matches!(incoming.recv_timeout(Duration::from_secs(10)), Ok(Incoming::Notification { .. })),
        "it has started"
    );
    host.close();
    assert_eq!(waiting.wait::<Value>().unwrap_err(), RpcError::cancelled());
    assert!(child.wait().unwrap().success());
}
