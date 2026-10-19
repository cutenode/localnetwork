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
use crate::{ExportedFile, FieldValue, Pending, View, code};

#[path = "../../examples/echo/plugin.rs"]
mod echo;

/// The host's end of a plugin served on a thread, over pipes.
struct Session {
    host: Peer,
    incoming: Receiver<Incoming>,
    served: JoinHandle<()>,
}

fn start(plugin: impl Plugin) -> Session {
    start_with(echo::manifest(), plugin)
}

fn start_with(manifest: Manifest, plugin: impl Plugin) -> Session {
    let (plugin_in, to_plugin) = pipe().unwrap();
    let (from_plugin, plugin_out) = pipe().unwrap();
    let served = std::thread::spawn(move || serve(&manifest, plugin, plugin_in, plugin_out));
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
    let s = greeted(echo::Echo);
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
    let s = greeted(echo::Echo);
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

// Properties.

/// The hooks a request names, and what each declares its contributions by.
const HOOKS: [(&str, &str); 4] = [
    (method::RUN_COMMAND, "command"),
    (method::ANALYZE_TRACK, "analyzer"),
    (method::WRITE_EXPORT, "exporter"),
    (method::PLAN_OP, "operation"),
];

/// A version the host might speak: mostly 0.x and 1.x, sometimes not a version at all.
#[hegel::test_helper]
fn protocol(tc: &TestCase) -> String {
    match tc.draw(gs::integers::<u8>().max_value(5)) {
        0 => tc.draw(gs::sampled_from(vec!["", "one", "0.1", "0.1.0.0", "^0.1.0", "v0.1.0"])).to_string(),
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
    let mut manifest = echo::manifest();
    manifest.id = tc.draw(gs::from_regex("[a-z][a-z0-9_-]{0,11}"));
    manifest.name = tc.draw(gs::text().min_size(1).max_size(12));
    manifest.version = format!("{}.{}.{}", tc.draw(gs::integers::<u8>()), tc.draw(gs::integers::<u8>()), 0);
    let s = start_with(manifest.clone(), echo::Echo);
    let others =
        vec!["describe", "command/run", "track/analyze", "export/write", "op/plan", "tracks.query", "shutdown"];
    let plugin = PluginInfo { id: manifest.id.clone(), version: manifest.version.clone() };
    let greeting = HelloReply { protocol: PROTOCOL_VERSION.into(), plugin, features: Vec::new() };
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
                let speaks = semver::Version::parse(&theirs).is_ok_and(|v| (v.major, v.minor) == (0, 1));
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
        let ours = format!("0.1.{}", tc.draw(gs::integers::<u16>()));
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

/// Answers each hook from its request alone, and with a script for the `steps` command reports
/// progress: `warn:<message>`, `start`, `end`, `item:<path>` (all in the scan phase) or
/// `step:<one, two or three>`.
struct Answers;

impl Plugin for Answers {
    fn run_command(&self, req: RunCommand, host: &Host) -> Result<Value, RpcError> {
        match req.command.as_str() {
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
        }
    }

    fn analyze_track(&self, req: AnalyzeTrack, _: &Host) -> Result<Analysis, RpcError> {
        Ok(Analysis { fields: [("bytes".to_string(), FieldValue::Integer(req.track.id.into()))].into_iter().collect() })
    }

    fn write_export(&self, req: WriteExport, _: &Host) -> Result<Exported, RpcError> {
        let file = ExportedFile { path: req.output, what: req.exporter, bytes: 0 };
        Ok(Exported { files: vec![file], ..Exported::default() })
    }

    fn plan_op(&self, req: PlanOp, _: &Host) -> Result<OpPlan, RpcError> {
        Ok(OpPlan { summary: req.operation, warnings: Vec::new(), edits: vec![req.params], inputs: Vec::new() })
    }
}

/// Panics in every hook.
struct PanicsEverywhere;

impl Plugin for PanicsEverywhere {
    fn run_command(&self, _: RunCommand, _: &Host) -> Result<Value, RpcError> {
        panic!("a bug");
    }

    fn analyze_track(&self, _: AnalyzeTrack, _: &Host) -> Result<Analysis, RpcError> {
        panic!("a bug");
    }

    fn write_export(&self, _: WriteExport, _: &Host) -> Result<Exported, RpcError> {
        panic!("a bug");
    }

    fn plan_op(&self, _: PlanOp, _: &Host) -> Result<OpPlan, RpcError> {
        panic!("a bug");
    }
}

/// Any hooks, run at once, each answer only for a contribution the manifest declares, with params
/// that read: params that don't are refused saying where, an undeclared name saying what isn't
/// declared, a hook the plugin doesn't implement isn't found, and one that panics still answers.
/// The rest answer what the plugin does.
#[hegel::test(test_cases = 25)]
fn hooks_answer_only_for_what_the_manifest_declares(tc: TestCase) {
    let plugin = tc.draw(gs::sampled_from(vec!["answers", "bare", "panics"]));
    let s = match plugin {
        "answers" => greeted(Answers),
        "bare" => greeted(Bare),
        _ => greeted(PanicsEverywhere),
    };
    let manifest = echo::manifest();
    let c = &manifest.contributes;
    let declared: [Vec<String>; 4] = [
        c.commands.iter().filter(|x| !["steps", "titles"].contains(&x.name.as_str())).map(|x| x.name.clone()).collect(),
        c.analyzers.iter().map(|x| x.name.clone()).collect(),
        c.exporters.iter().map(|x| x.format.clone()).collect(),
        c.operations.iter().map(|x| x.name.clone()).collect(),
    ];
    let mut sent = Vec::new();
    for _ in 0..tc.draw(gs::integers::<usize>().min_value(1).max_value(6)) {
        let hook = tc.draw(gs::integers::<usize>().max_value(3));
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
            _ => {
                let plan = json!({ "summary": name, "warnings": [], "edits": [echoed] });
                (json!({ "operation": name, "params": echoed }), plan)
            }
        };
        // Params that don't read, and where `invalid_params` says they're wrong.
        let (params, path) = match tc.draw(gs::weighted_booleans(0.25)) {
            false => (params, None),
            true => match hook {
                0 => (json!({ "command": 5 }), Some("command")),
                1 => (json!({ "analyzer": name, "track": { "id": "x" } }), Some("track.id")),
                2 => (json!({ "exporter": name, "source": { "drive": "USB" } }), None),
                _ => (json!({ "operation": [] }), Some("operation")),
            },
        };
        let pending = s.host.send_request(method, &params).unwrap();
        let is_declared = declared[hook].contains(&name);
        sent.push((method, what, name, is_declared, params, path, answer, pending));
    }
    for (method, what, name, declared, params, path, answer, pending) in sent {
        let got = pending.wait::<Value>();
        let malformed = path.is_some() || params.get("output").is_none() && method == method::WRITE_EXPORT;
        match got {
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
            Err(e) if plugin == "bare" => {
                assert_eq!((e.code, e.message), (code::METHOD_NOT_FOUND, format!("unknown method {method:?}")))
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
    let s = greeted(Answers);
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
    let s = greeted(Answers);
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
