//! The `locallibrary rpc` sidecar's lifecycle: bad input, request ids, the handshake, shutdown,
//! signals and progress. Long-running requests are the test-only `$/test.wait`.

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, ExitStatus, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use assert_cmd::cargo::CommandCargoExt;
use locallibrary::Store;
use locallibrary::device::dunce_canonicalize;
use locallibrary::fields::{Analyzer, FieldStore, FieldValue, Fields};
use serde_json::{Value, json};

use crate::support::{blank_drive, demo_drive};

/// Generous: the first run of a freshly built binary can be slow to start.
const WAIT: Duration = Duration::from_secs(60);

struct Sidecar {
    dir: tempfile::TempDir,
    child: Child,
    stdin: Option<ChildStdin>,
    messages: Receiver<Value>,
    /// Messages read while looking for another one.
    unclaimed: Vec<Value>,
}

impl Sidecar {
    fn start() -> Sidecar {
        Sidecar::start_with(&[])
    }

    /// With these arguments after `rpc`.
    fn start_with(args: &[&str]) -> Sidecar {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("volumes")).unwrap();
        let mut child = crate::locallibrary(&dir.path().join("store"), &[&dir.path().join("volumes")])
            .arg("rpc")
            .args(args)
            .current_dir(dir.path())
            .env("LOCALLIBRARY_RPC_TEST", "1")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let stdout = child.stdout.take().unwrap();
        let (tx, messages) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                let msg: Value = serde_json::from_str(&line).unwrap_or_else(|e| panic!("{e}: {line}"));
                if tx.send(msg).is_err() {
                    break;
                }
            }
        });
        let stdin = child.stdin.take();
        Sidecar { dir, child, stdin, messages, unclaimed: Vec::new() }
    }

    fn root(&self) -> &Path {
        self.dir.path()
    }

    fn write(&mut self, bytes: &[u8]) {
        let stdin = self.stdin.as_mut().unwrap();
        stdin.write_all(bytes).unwrap();
        stdin.flush().unwrap();
    }

    fn send(&mut self, id: impl Into<Value>, method: &str, params: Value) {
        let msg = json!({ "jsonrpc": "2.0", "id": id.into(), "method": method, "params": params });
        self.write(format!("{msg}\n").as_bytes());
    }

    fn notify(&mut self, method: &str, params: Value) {
        let msg = json!({ "jsonrpc": "2.0", "method": method, "params": params });
        self.write(format!("{msg}\n").as_bytes());
    }

    /// The next message, progress notifications included.
    fn next(&mut self) -> Value {
        self.messages.recv_timeout(WAIT).expect("the sidecar stopped answering")
    }

    /// The response with this id (`null` for errors about unreadable lines). Progress is skipped;
    /// other responses are kept for later.
    fn reply_to(&mut self, id: impl Into<Value>) -> Value {
        let id = id.into();
        if let Some(i) = self.unclaimed.iter().position(|m| m["id"] == id) {
            return self.unclaimed.remove(i);
        }
        loop {
            let msg = self.next();
            if msg["method"] == "progress" {
                continue;
            }
            if msg["id"] == id {
                return msg;
            }
            self.unclaimed.push(msg);
        }
    }

    fn call(&mut self, id: impl Into<Value>, method: &str, params: Value) -> Value {
        let id = id.into();
        self.send(id.clone(), method, params);
        self.reply_to(id)
    }

    /// The next `event` notification of this kind (`drive.attached`) whose fields include
    /// `with`'s. Other messages are kept for later.
    fn event(&mut self, kind: &str, with: Value) -> Value {
        let matches = |m: &Value| {
            let e = &m["params"];
            m["method"] == "event" && e["event"] == kind && with.as_object().unwrap().iter().all(|(k, v)| &e[k] == v)
        };
        if let Some(i) = self.unclaimed.iter().position(matches) {
            return self.unclaimed.remove(i)["params"].take();
        }
        loop {
            let mut msg = self.next();
            if matches(&msg) {
                return msg["params"].take();
            }
            if msg["method"] != "progress" {
                self.unclaimed.push(msg);
            }
        }
    }

    /// The next `serve` notification about the request `id`: its event. Other messages are kept
    /// for later.
    #[cfg(feature = "peers")]
    fn served(&mut self, id: i64) -> Value {
        let matches = |m: &Value| m["method"] == "serve" && m["params"]["id"] == id;
        if let Some(i) = self.unclaimed.iter().position(matches) {
            return self.unclaimed.remove(i)["params"]["event"].take();
        }
        loop {
            let mut msg = self.next();
            if matches(&msg) {
                return msg["params"]["event"].take();
            }
            if msg["method"] != "progress" {
                self.unclaimed.push(msg);
            }
        }
    }

    /// Forgets the events not asked for.
    fn drop_events(&mut self) {
        self.unclaimed.retain(|m| m["method"] != "event");
    }

    /// Closes stdin and waits for the process to exit.
    fn exit(mut self) -> ExitStatus {
        drop(self.stdin.take());
        let deadline = Instant::now() + WAIT;
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                assert!(self.unclaimed.is_empty(), "unexpected responses: {:?}", self.unclaimed);
                return status;
            }
            if Instant::now() > deadline {
                let _ = self.child.kill();
                panic!("the sidecar didn't exit");
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

fn kind(response: &Value) -> &Value {
    &response["error"]["data"]["kind"]
}

#[test]
fn a_line_that_isnt_utf8_gets_a_parse_error_and_requests_carry_on() {
    let mut s = Sidecar::start();
    s.send(1, "$/test.wait", json!({ "ms": 600_000 }));
    s.write(b"\xff\xfe\n");
    s.write(b"{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"vers\xffion\"}\n");
    for _ in 0..2 {
        let v = s.reply_to(Value::Null);
        assert_eq!(v["error"]["code"], -32700, "{v}");
        assert_eq!(kind(&v), "parse_error");
    }
    // The request in flight is still running, and the server still answers.
    let jobs = s.call(3, "jobs.list", json!({}));
    assert_eq!(jobs["result"][0]["id"], 1, "{jobs}");
    assert_eq!(s.call(4, "$/cancel", json!({ "id": 1 }))["result"]["cancelled"], true);
    assert_eq!(kind(&s.reply_to(1)), "cancelled");
    assert!(s.exit().success());
}

/// A line without an end can't make the sidecar keep it all: past 16 MiB it gets a parse error,
/// even a well-formed request, and is skipped to its end.
#[test]
fn a_line_too_long_gets_a_parse_error_and_requests_carry_on() {
    let mut s = Sidecar::start();
    s.send(1, "$/test.wait", json!({ "ms": 600_000 }));
    let pad = "x".repeat(16 << 20);
    s.send(2, "version", json!({ "pad": pad }));
    let v = s.reply_to(Value::Null);
    assert_eq!((&v["error"]["code"], kind(&v)), (&json!(-32700), &json!("parse_error")), "{v}");
    assert!(v["error"]["message"].as_str().unwrap().contains("longer than 16 MiB"), "{v}");
    assert!(s.call(3, "version", json!({}))["result"]["version"].is_string());
    assert_eq!(s.call(4, "jobs.list", json!({}))["result"][0]["id"], 1);
    assert_eq!(s.call(5, "$/cancel", json!({ "id": 1 }))["result"]["cancelled"], true);
    assert_eq!(kind(&s.reply_to(1)), "cancelled");
    // Nothing answered request 2.
    assert!(s.exit().success());
}

#[test]
fn an_id_in_flight_is_refused_until_its_response() {
    let mut s = Sidecar::start();
    s.send(1, "$/test.wait", json!({ "ms": 600_000 }));
    let v = s.call(1, "version", json!({}));
    assert_eq!(v["error"]["code"], -32600, "{v}");
    assert_eq!(kind(&v), "invalid_request");
    assert!(v["error"]["message"].as_str().unwrap().contains("already in use"), "{v}");
    // The string "1" is a different id.
    assert!(s.call("1", "version", json!({}))["result"]["version"].is_string());
    s.write(b"{\"jsonrpc\":\"2.0\",\"id\":[1],\"method\":\"version\"}\n");
    assert_eq!(s.reply_to(Value::Null)["error"]["code"], -32600);

    // Notifications run, but `$/cancel` can't name them.
    s.notify("$/test.wait", json!({ "ms": 600_000 }));
    s.notify("$/test.wait", json!({ "ms": 600_000 }));
    let jobs = s.call(2, "jobs.list", json!({}))["result"].clone();
    let ids: Vec<&Value> = jobs.as_array().unwrap().iter().map(|j| &j["id"]).collect();
    assert_eq!(ids, [&json!(1), &Value::Null, &Value::Null], "{jobs}");
    assert_eq!(jobs[0]["method"], "$/test.wait");
    assert!(jobs[0]["started"].as_str().is_some_and(|t| t.contains('T')), "{jobs}");
    assert_eq!(s.call(3, "$/cancel", json!({ "id": null }))["result"]["cancelled"], false);

    // Once request 1 has answered, its id is free again.
    assert_eq!(s.call(4, "$/cancel", json!({ "id": 1 }))["result"]["cancelled"], true);
    assert_eq!(kind(&s.reply_to(1)), "cancelled");
    assert!(s.call(1, "version", json!({}))["result"]["version"].is_string());
    // The notifications are still running; shutting down cancels them.
    assert!(s.call(5, "shutdown", json!({ "cancel": true }))["result"].is_null());
    assert!(s.exit().success());
}

#[test]
fn initialize_describes_the_sidecar() {
    let mut s = Sidecar::start();
    let v = s.call(1, "initialize", json!({ "client": "test" }))["result"].clone();
    assert_eq!(v["protocol"], 1, "{v}");
    assert_eq!(v["version"], env!("CARGO_PKG_VERSION"));
    let has = |list: &Value, x: &str| list.as_array().unwrap().contains(&json!(x));
    for c in ["progress", "progress.scope", "cancel", "jobs.list", "shutdown.cancel"] {
        assert!(has(&v["capabilities"], c), "{c}");
    }
    for m in ["initialize", "version", "jobs.list", "$/cancel", "shutdown", "sync.apply", "rpc.describe"] {
        assert!(has(&v["methods"], m), "{m}");
    }
    // Older names are listed too: they still work.
    for m in ["sync_many.apply", "drive.adopt", "store.gc", "drive.restore.preview", "drive.sets"] {
        assert!(has(&v["methods"], m), "{m}");
    }
    assert!(has(&v["capabilities"], "describe"));
    assert!(!has(&v["methods"], "$/test.wait"));
    // `version` still answers as it did.
    let version = s.call(2, "version", json!({}))["result"].clone();
    assert_eq!((&version["version"], &version["methods"]), (&v["version"], &v["methods"]));
    let unknown = s.call(3, "no.such.method", json!({}));
    assert_eq!((&unknown["error"]["code"], kind(&unknown)), (&json!(-32601), &json!("method_not_found")));
    assert!(s.exit().success());
}

#[test]
fn initialize_says_whether_the_account_is_there() {
    let mut s = Sidecar::start();
    let v = s.call(1, "initialize", json!({}))["result"].clone();
    let has = |list: &Value, x: &str| list.as_array().unwrap().contains(&json!(x));
    let built_with = cfg!(feature = "account");
    assert_eq!(has(&v["capabilities"], "account"), built_with, "{v}");
    for m in ["store.serve", "store.fetch"] {
        assert_eq!(has(&v["methods"], m), cfg!(feature = "peers"), "{m}");
    }
    assert_eq!(has(&v["capabilities"], "peers"), cfg!(feature = "peers"), "{v}");
    for m in ["account.status", "account.login.begin", "profile.publish.plan", "settings.import_published.apply"] {
        assert_eq!(has(&v["methods"], m), built_with, "{m}");
    }
    // The profile is built and its document written either way; only publishing needs the account.
    assert!(has(&v["methods"], "profile.document"));
    if !built_with {
        let status = s.call(2, "account.status", json!({}));
        assert_eq!(kind(&status), &json!("method_not_found"));
    }
    assert!(s.exit().success());
}

/// The sidecar's cheap checks on what a peer asks for: a relative path to write to is refused (its
/// working folder is its parent's business), and a repairing fsck is described as destructive.
#[test]
fn paths_to_write_to_are_absolute_and_fsck_says_it_can_destroy() {
    let mut s = Sidecar::start();
    for (id, method, params) in [
        (1, "export", json!({ "drive": "A", "output": "list.m3u8" })),
        (2, "recordings.collect.plan", json!({ "drive": "A", "to": "recordings" })),
        (3, "store.copy", json!({ "path": "copy" })),
    ] {
        let v = s.call(id, method, params);
        assert_eq!(kind(&v), "invalid_params", "{v}");
        assert!(v["error"]["message"].as_str().unwrap().contains("must be an absolute path"), "{v}");
    }
    assert!(!s.root().join("copy").exists(), "nothing where its working folder is");
    let fsck = s.call(4, "rpc.describe", json!({ "method": "drive.fsck" }))["result"]["methods"][0].clone();
    assert_eq!((&fsck["kind"], &fsck["destructive"]), (&json!("immediate"), &json!(true)), "{fsck}");
    assert!(s.exit().success());
}

/// An apply by id refused because the store won't open keeps its plan, as every refusal before
/// anything is written does.
#[test]
fn a_plan_stays_when_the_store_wont_open() {
    let mut s = Sidecar::start();
    let plan = s.call(1, "players.set.plan", json!({ "models": ["CDJ-3000"] }));
    let id = plan["result"]["plan_id"].clone();
    assert!(id.is_string(), "{plan}");
    let (meta, away) = (s.root().join("store/store.json"), s.root().join("store.json"));
    std::fs::rename(&meta, &away).unwrap();
    let v = s.call(2, "players.set.apply", json!({ "plan_id": id }));
    assert!(!v["error"].is_null(), "{v}");
    std::fs::rename(&away, &meta).unwrap();
    let v = s.call(3, "players.set.apply", json!({ "plan_id": id }));
    assert!(v["error"].is_null(), "{v}");
    assert!(s.exit().success());
}

/// `art.embed` rewrites the files it's given: each of them must be an absolute path too.
#[test]
fn art_embed_takes_absolute_paths_only() {
    let mut s = Sidecar::start();
    let request = json!({ "files": [s.root().join("a.mp3"), "b.mp3"], "image": s.root().join("c.jpg") });
    for (id, method) in [(1, "art.embed.plan"), (2, "art.embed.apply")] {
        let v = s.call(id, method, request.clone());
        assert_eq!(kind(&v), "invalid_params", "{v}");
        let message = v["error"]["message"].as_str().unwrap();
        assert!(message.contains("`files` must be an absolute path, not \"b.mp3\""), "{v}");
    }
    assert!(s.exit().success());
}

#[test]
fn describe_says_what_each_method_takes_and_returns() {
    let mut s = Sidecar::start();
    let all = s.call(1, "rpc.describe", json!({}))["result"].clone();
    assert_eq!((&all["protocol"], &all["version"]), (&json!(1), &json!(env!("CARGO_PKG_VERSION"))));
    let methods = all["methods"].as_array().unwrap();
    let listed = s.call(2, "initialize", json!({}))["result"]["methods"].as_array().unwrap().len();
    let names = methods.iter().map(|m| 1 + m["aliases"].as_array().unwrap().len()).sum::<usize>();
    assert_eq!(names, listed, "initialize lists every method and alias rpc.describe does");
    let defs = all["$defs"].as_object().unwrap();
    assert!(defs.contains_key("SyncRequest") && defs.contains_key("PlanFields"));
    assert!(all["notifications"]["progress"].is_object() && all["notifications"]["event"].is_object());

    // One method, by an old name: its schemas, and only the definitions they use.
    let one = s.call(3, "rpc.describe", json!({ "method": "sync_set.plan" }))["result"].clone();
    let m = &one["methods"][0];
    assert_eq!(
        (&m["name"], &m["kind"], &m["pair"]),
        (&json!("sync.group.plan"), &json!("plan"), &json!("sync.group.apply"))
    );
    assert_eq!((&m["long_running"], &m["destructive"]), (&json!(true), &json!(true)));
    assert!(one["$defs"].as_object().unwrap().len() < defs.len());
    let unknown = s.call(4, "rpc.describe", json!({ "method": "no.such" }));
    assert_eq!(kind(&unknown), "invalid_params", "{unknown}");

    // Old and new names answer alike.
    let new = s.call(5, "drive.groups", json!({}))["result"].clone();
    assert_eq!(s.call(6, "drive.sets", json!({}))["result"], new);
    let stats = s.call(7, "store.gc.plan", json!({}))["result"].clone();
    assert_eq!(stats["operation"], "store.gc");
    let gc = s.call(8, "store.gc", json!({ "plan_id": stats["plan_id"] }));
    assert!(gc["error"].is_null(), "{gc}");
    assert!(s.exit().success());
}

/// The contract in docs/rpc is what `rpc --emit-schema` writes. When a method or a type changes,
/// regenerate it: `cargo run -p locallibrary-cli -- rpc --emit-schema apps/locallibrary/docs/rpc`
/// (or run this test with `LOCALLIBRARY_BLESS=1`).
#[test]
#[cfg_attr(not(feature = "account"), ignore = "docs/rpc describes the default build, which has the account methods")]
fn the_contract_in_docs_is_current() {
    let dir = tempfile::tempdir().unwrap();
    let status = Command::cargo_bin("locallibrary")
        .unwrap()
        .args(["rpc", "--emit-schema"])
        .arg(dir.path())
        .stdout(Stdio::null())
        .status()
        .unwrap();
    assert!(status.success());
    let docs = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("docs/rpc");
    let bless = std::env::var_os("LOCALLIBRARY_BLESS").is_some_and(|v| v == "1");
    let mut stale = Vec::new();
    for name in ["schema.json", "locallibrary.d.ts"] {
        let made = std::fs::read_to_string(dir.path().join(name)).unwrap();
        let kept = std::fs::read_to_string(docs.join(name)).unwrap_or_default();
        if made != kept {
            if bless {
                std::fs::create_dir_all(&docs).unwrap();
                std::fs::write(docs.join(name), made).unwrap();
            } else {
                stale.push(name);
            }
        }
    }
    assert!(
        stale.is_empty(),
        "docs/rpc/{stale:?} is out of date: the RPC contract changed. Regenerate it with \
         `cargo run -p locallibrary-cli -- rpc --emit-schema apps/locallibrary/docs/rpc` \
         (or LOCALLIBRARY_BLESS=1 cargo test)"
    );
}

#[test]
fn shutdown_waits_for_running_requests_or_cancels_them() {
    // Plain shutdown: the running request finishes first.
    let mut s = Sidecar::start();
    s.send(1, "$/test.wait", json!({ "ms": 200 }));
    assert!(s.call(2, "shutdown", json!({}))["result"].is_null());
    assert_eq!(s.reply_to(1)["result"]["waited_ms"], 200);
    assert!(s.exit().success());

    // With `cancel`: they stop at once.
    let mut s = Sidecar::start();
    s.send(1, "$/test.wait", json!({ "ms": 600_000 }));
    s.send(2, "$/test.wait", json!({ "ms": 600_000 }));
    let v = s.call(3, "shutdown", json!({ "cancel": "yes" }));
    assert_eq!(v["error"]["code"], -32602, "bad params don't shut down: {v}");
    let started = Instant::now();
    assert!(s.call(4, "shutdown", json!({ "cancel": true }))["result"].is_null());
    for id in [1, 2] {
        assert_eq!(kind(&s.reply_to(id)), "cancelled");
    }
    assert!(s.exit().success());
    assert!(started.elapsed() < Duration::from_secs(30));
}

#[cfg(unix)]
#[test]
fn sigterm_cancels_running_requests_then_exits() {
    let mut s = Sidecar::start();
    s.send(1, "$/test.wait", json!({ "ms": 600_000 }));
    // An answer means the signal handler is in place and request 1 is running.
    assert_eq!(s.call(2, "jobs.list", json!({}))["result"][0]["id"], 1);
    let kill = Command::new("kill").args(["-TERM", &s.child.id().to_string()]).status().unwrap();
    assert!(kill.success());
    assert_eq!(kind(&s.reply_to(1)), "cancelled");
    assert_eq!(s.exit().code(), Some(128 + 15));
}

#[test]
fn item_progress_is_coalesced_and_counted() {
    let mut s = Sidecar::start();
    s.send(1, "$/test.wait", json!({ "items": 5000 }));
    let (mut lines, mut items) = (0, 0);
    let reply = loop {
        let msg = s.next();
        if msg["method"] != "progress" {
            break msg;
        }
        assert_eq!(msg["params"]["id"], 1);
        lines += 1;
        let event = &msg["params"]["event"];
        if event["event"] == "item" {
            items += event["items"].as_u64().unwrap_or(1);
        }
    };
    assert_eq!(reply["id"], 1, "{reply}");
    assert_eq!(items, 5000, "every item is counted, and all before the response");
    assert!(lines < 100, "{lines} progress lines");
    assert!(s.exit().success());
}

/// A request's progress events and its response.
fn run(s: &mut Sidecar, id: i64, method: &str, params: Value) -> (Vec<Value>, Value) {
    s.send(id, method, params);
    let mut events = Vec::new();
    loop {
        let mut msg = s.next();
        if msg["method"] != "progress" {
            assert_eq!(msg["id"], id, "{msg}");
            return (events, msg);
        }
        events.push(msg["params"]["event"].take());
    }
}

#[test]
fn progress_says_which_drive_and_step() {
    let mut s = Sidecar::start();
    let root = s.root().to_path_buf();
    let a = demo_drive(&root, "A");

    // Several drives at once: each target's events carry its root, as in the results.
    let (b, c) = (blank_drive(&root, "B"), blank_drive(&root, "C"));
    let (events, reply) = run(&mut s, 1, "sync_many.apply", json!({ "drives": [a, b, c] }));
    let results = reply["result"].as_array().unwrap_or_else(|| panic!("{reply}"));
    let mut targets: Vec<&Value> = results.iter().map(|r| &r["target"]).collect();
    targets.sort_by_key(|t| t.to_string());
    let mut seen: Vec<&Value> = events.iter().map(|e| &e["target"]).filter(|t| !t.is_null()).collect();
    seen.sort_by_key(|t| t.to_string());
    seen.dedup();
    assert_eq!(seen, targets, "{events:#?}");

    // Provisioning: each step it runs is announced, and its events carry it.
    let new = blank_drive(&root, "New");
    let (events, reply) = run(&mut s, 2, "provision.apply", json!({ "target": new, "like": a, "name": "Spare" }));
    assert!(reply["result"]["stopped"].is_null(), "{reply}");
    let steps: Vec<&Value> = events.iter().filter(|e| e["event"] == "step").map(|e| &e["step"]).collect();
    assert_eq!(
        steps,
        [
            &json!({ "index": 1, "total": 3, "id": "name" }),
            &json!({ "index": 2, "total": 3, "id": "library" }),
            &json!({ "index": 3, "total": 3, "id": "verify" }),
        ]
    );
    let first = events.iter().position(|e| e["event"] == "step").unwrap();
    assert!(events[first..].iter().all(|e| e["step"].is_object()), "{events:#?}");
    for id in ["library", "verify"] {
        assert!(events.iter().any(|e| e["event"] != "step" && e["step"]["id"] == id), "no events in {id}");
    }
    assert!(s.exit().success());
}

#[test]
fn events_say_what_drives_the_store_and_requests_do() {
    let mut s = Sidecar::start();
    let root = s.root().to_path_buf();
    let volumes = root.join("volumes");
    let v = s.call(1, "events.subscribe", json!({ "interval_ms": 100 }));
    assert_eq!(v["result"], json!({ "kinds": [], "drives": [] }), "{v}");

    // A drive plugged in (made beside the volumes, then moved in at once).
    std::fs::rename(demo_drive(&root, "A"), volumes.join("A")).unwrap();
    let a = dunce_canonicalize(&volumes.join("A"));
    let drive = json!({ "root": a, "id": null, "name": "A", "has_export": true });
    let mut attached = drive.clone();
    attached["event"] = "drive.attached".into();
    assert_eq!(s.event("drive.attached", json!({})), attached);
    assert_eq!(s.call(2, "drives.mounted", json!({}))["result"], json!([drive]));

    // Requests starting and finishing.
    s.call(3, "version", json!({}));
    assert_eq!(s.event("job.started", json!({ "id": 3 }))["method"], "version");
    assert_eq!(s.event("job.finished", json!({ "id": 3 }))["ok"], true);
    assert!(s.call(4, "drive.info", json!({ "drive": "nowhere" }))["error"].is_object());
    let finished = s.event("job.finished", json!({ "id": 4 }));
    assert_eq!((&finished["ok"], &finished["kind"]), (&json!(false), &json!("not_found")));

    // Pulled: the drive is given an identity, and the store a library and a snapshot.
    let v = s.call(5, "pull", json!({ "drive": a, "library": "Main", "new_library": true }));
    assert!(v["result"]["snapshot"].is_object(), "{v}");
    let changed = s.event("drive.changed", json!({ "root": a }));
    assert!(changed["id"].is_string() && changed["changed"].as_array().unwrap().contains(&json!("state")), "{changed}");
    let mut store_parts = Vec::new();
    while !["libraries", "snapshots"].iter().all(|p| store_parts.contains(&json!(p))) {
        let e = s.event("store.changed", json!({}));
        assert_eq!(e["root"], json!(root.join("store")));
        store_parts.extend(e["changed"].as_array().unwrap().iter().cloned());
    }

    // Its database rewritten, then the drive unplugged.
    let pdb = a.join("PIONEER/rekordbox/export.pdb");
    let mut bytes = std::fs::read(&pdb).unwrap();
    bytes.extend_from_slice(&[0; 4096]);
    std::fs::write(&pdb, bytes).unwrap();
    // (The pull's own writes may still be reported first.)
    while !s.event("drive.changed", json!({ "root": a }))["changed"].as_array().unwrap().contains(&json!("databases")) {
    }
    std::fs::remove_dir_all(volumes.join("A")).unwrap();
    assert_eq!(s.event("drive.detached", json!({}))["root"], json!(a));

    // Only some kinds, then none.
    let v = s.call(6, "events.subscribe", json!({ "kinds": ["job.finished"] }));
    assert_eq!(v["result"]["kinds"], json!(["job.finished"]));
    s.drop_events();
    s.call(7, "version", json!({}));
    assert_eq!(s.event("job.finished", json!({ "id": 7 }))["method"], "version");
    assert!(!s.unclaimed.iter().any(|m| m["params"]["event"] == "job.started" && m["params"]["id"] == 7));
    assert_eq!(s.call(8, "events.unsubscribe", json!({}))["result"]["unsubscribed"], true);
    s.call(9, "version", json!({}));
    assert!(!s.unclaimed.iter().any(|m| m["params"]["id"] == 9), "{:?}", s.unclaimed);
    assert_eq!(s.call(10, "events.unsubscribe", json!({}))["result"]["unsubscribed"], false);
    s.drop_events();
    assert!(s.exit().success());
}

/// `store.serve` gives its code in a `serve` notification and serves until it's cancelled, then
/// says how much went; another computer's `store.fetch` copies the snapshots its store lacks, as
/// `store serve` and `store fetch` do.
#[cfg(feature = "peers")]
#[test]
fn a_store_is_served_and_fetched_through_the_sidecar() {
    let mut laptop = Sidecar::start();
    let main = demo_drive(laptop.root(), "Main");
    let pulled = laptop.call(1, "pull", json!({ "drive": main, "library": "Main", "new_library": true }));
    assert!(pulled["result"]["snapshot"].is_object(), "{pulled}");
    laptop.send(2, "store.serve", json!({ "local_only": true }));
    let ready = laptop.served(2);
    assert_eq!(ready["event"], "ready", "{ready}");
    let code = ready["code"].clone();

    let mut studio = Sidecar::start();
    let dry = studio.call(1, "store.fetch", json!({ "code": code, "local_only": true, "dry_run": true }));
    assert_eq!((&dry["result"]["snapshots"], &dry["result"]["dry_run"]), (&json!(1), &json!(true)), "{dry}");
    assert!(studio.call(2, "library.list", json!({}))["result"].as_array().unwrap().is_empty());
    let fetched = studio.call(3, "store.fetch", json!({ "code": code, "local_only": true }));
    let library = &fetched["result"]["libraries"][0];
    assert_eq!((&library["name"], &library["outcome"]), (&json!("Main"), &json!("added")), "{fetched}");
    assert_eq!(studio.call(4, "library.list", json!({}))["result"][0]["name"], "Main");
    let wrong = studio.call(5, "store.fetch", json!({ "code": "not-a-code" }));
    assert_eq!(kind(&wrong), "invalid", "{wrong}");

    // Each fetch connected, and was done once it had what it came for.
    let events: Vec<Value> = (0..4).map(|_| laptop.served(2)["event"].clone()).collect();
    let count = |e: &str| events.iter().filter(|x| *x == e).count();
    assert_eq!((count("connected"), count("done")), (2, 2), "{events:?}");
    assert_eq!(laptop.call(3, "$/cancel", json!({ "id": 2 }))["result"]["cancelled"], true);
    let report = laptop.reply_to(2);
    assert_eq!(report["result"]["peers"], 2, "{report}");
    assert!(report["result"]["files"].as_u64().unwrap() > 0, "{report}");
    // Serving stops when the front end goes away, where the sidecar would wait for it forever.
    laptop.send(4, "store.serve", json!({ "local_only": true }));
    assert_eq!(laptop.served(4)["event"], "ready");
    assert!(laptop.exit().success());
    assert!(studio.exit().success());
}

#[test]
fn watching_pulls_drives_as_they_are_plugged_in() {
    let mut s = Sidecar::start();
    let root = s.root().to_path_buf();
    let main = demo_drive(&root, "Main");
    let v = s.call(1, "pull", json!({ "drive": main, "library": "Main", "new_library": true }));
    assert!(v["result"]["snapshot"].is_object(), "{v}");
    // A player records a history list.
    let pdb = Path::new(&main).join("PIONEER/rekordbox/export.pdb");
    let first = rekordbox_pdb::Database::from_file(&pdb).unwrap().tracks[0].id;
    let mut ed = rekordbox_pdb::PdbEditor::from_file(&pdb).unwrap();
    let list = ed.create_history_playlist("HISTORY 001").unwrap();
    ed.add_to_history(list, first).unwrap();
    ed.save(&pdb).unwrap();

    let v = s.call(2, "watch.start", json!({ "pull": true, "settle_ms": 0, "interval_ms": 100 }));
    assert_eq!(v["result"]["watching"], true, "{v}");
    assert_eq!(kind(&s.call(3, "watch.start", json!({}))), "conflict");
    let jobs = s.call(4, "jobs.list", json!({}));
    assert!(jobs["result"].as_array().unwrap().iter().any(|j| j["method"] == "watch" && j["id"].is_null()), "{jobs}");

    // Plugged in: pulled, and said without subscribing to anything.
    std::fs::rename(&main, root.join("volumes/Main")).unwrap();
    let pulled = s.event("watch.pulled", json!({ "name": "Main" }));
    assert_eq!((&pulled["history_lists"], &pulled["beside"]), (&json!(["HISTORY 001"]), &json!(false)));

    assert_eq!(s.call(5, "watch.stop", json!({}))["result"]["stopped"], true);
    let deadline = Instant::now() + WAIT;
    while s.call(6, "jobs.list", json!({}))["result"].as_array().unwrap().iter().any(|j| j["method"] == "watch") {
        assert!(Instant::now() < deadline, "still watching");
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(s.call(7, "watch.stop", json!({}))["result"]["stopped"], false);
    // Watching doesn't keep the sidecar running once the client goes away.
    assert_eq!(s.call(8, "watch.start", json!({}))["result"]["watching"], true);
    s.drop_events();
    assert!(s.exit().success());
}

#[test]
fn requests_wait_for_a_worker_and_reads_have_their_own() {
    let mut s = Sidecar::start_with(&["--workers", "1"]);
    s.send(1, "$/test.wait", json!({ "ms": 600_000 }));
    s.send(2, "$/test.wait", json!({ "ms": 600_000 }));
    // A read isn't held up by them.
    assert!(s.call(3, "version", json!({}))["result"]["version"].is_string());
    let deadline = Instant::now() + WAIT;
    let jobs = loop {
        let jobs = s.call(4, "jobs.list", json!({}))["result"].clone();
        if jobs[0]["queued"].is_null() {
            break jobs;
        }
        assert!(Instant::now() < deadline, "{jobs}");
    };
    assert_eq!((&jobs[0]["id"], &jobs[1]["id"], &jobs[1]["queued"]), (&json!(1), &json!(2), &json!(true)), "{jobs}");

    // Cancelled while it waits: it answers at once.
    assert_eq!(s.call(5, "$/cancel", json!({ "id": 2 }))["result"]["cancelled"], true);
    assert_eq!(kind(&s.reply_to(2)), "cancelled");
    assert_eq!(s.call(6, "jobs.list", json!({}))["result"].as_array().unwrap().len(), 1);
    // The next one runs once the worker is free.
    s.send(7, "$/test.wait", json!({ "ms": 10 }));
    assert_eq!(s.call(8, "$/cancel", json!({ "id": 1 }))["result"]["cancelled"], true);
    assert_eq!(kind(&s.reply_to(1)), "cancelled");
    assert_eq!(s.reply_to(7)["result"]["waited_ms"], 10);
    assert!(s.exit().success());
}

#[test]
fn reads_keep_the_library_until_it_changes_and_come_in_pieces() {
    let mut s = Sidecar::start();
    let root = s.root().to_path_buf();
    let a = demo_drive(&root, "A");
    let total =
        |s: &mut Sidecar, id| s.call(id, "tracks.query", json!({ "drive": a, "limit": 1 }))["result"]["total"].clone();
    assert_eq!(total(&mut s, 1), 2);

    // rekordbox exports another track: the next read has it.
    let pdb = Path::new(&a).join("PIONEER/rekordbox/export.pdb");
    let mut ed = rekordbox_pdb::PdbEditor::from_file(&pdb).unwrap();
    let new = ed.add_track(&rekordbox_pdb::NewTrack::new("New", "/Contents/New/new.mp3")).unwrap();
    ed.save(&pdb).unwrap();
    std::fs::create_dir_all(Path::new(&a).join("Contents/New")).unwrap();
    std::fs::write(Path::new(&a).join("Contents/New/new.mp3"), "new audio").unwrap();
    assert_eq!(total(&mut s, 2), 3);
    let page = s.call(3, "drive.library", json!({ "drive": a, "offset": 2, "limit": 5 }))["result"].clone();
    assert_eq!(page["page"], json!({ "offset": 2, "count": 1, "total": 3 }), "{page}");
    assert_eq!(page["library"]["tracks"][0]["id"], new);

    // So does a change made through the sidecar.
    let v = s.call(4, "edit.apply", json!({ "drive": a, "edits": [{ "op": "create_playlist", "path": "Fresh" }] }));
    assert!(v["error"].is_null(), "{v}");
    let tree = s.call(5, "playlists.tree", json!({ "drive": a }))["result"].clone();
    let names: Vec<&str> = tree["playlists"].as_array().unwrap().iter().map(|p| p["name"].as_str().unwrap()).collect();
    assert!(names.contains(&"Fresh"), "{tree}");

    let got = s.call(6, "tracks.get", json!({ "drive": a, "ids": [new, 404] }))["result"].clone();
    assert_eq!((&got["tracks"][0]["title"], &got["missing"]), (&json!("New"), &json!([404])));
    let one = s.call(7, "track.analysis", json!({ "drive": a, "track": 1, "waveform": "none" }))["result"].clone();
    assert!(one["preview"].is_null() && one["detail"].is_null() && one["beats"].is_array(), "{one}");
    let packed =
        s.call(8, "track.analysis", json!({ "drive": a, "track": 1, "waveform": "detail", "encoding": "base64" }));
    assert_eq!(packed["result"]["detail"]["encoding"], "base64", "{packed}");
    assert_eq!(
        s.call(9, "art.path", json!({ "drive": a, "tracks": [1, 2] }))["result"],
        json!([{ "track": 1, "image": null }, { "track": 2, "image": null }])
    );
    assert_eq!(s.call(10, "art.get", json!({ "drive": a, "track": 1 }))["result"], Value::Null);
    assert!(s.exit().success());
}

/// Several tracks' analyses come in one request, without scrolling waveforms, up to a cap; the
/// fields analyzer plugins kept are read without making a store when there's none, and only
/// those of plugins locallibrary has.
#[test]
fn analyses_of_several_tracks_and_plugin_fields() {
    let mut s = Sidecar::start();
    let root = s.root().to_path_buf();
    let a = demo_drive(&root, "A");
    let many = s.call(1, "tracks.analysis", json!({ "drive": a, "ids": [2, 404, 1] }))["result"].clone();
    let tracks: Vec<&Value> = many["tracks"].as_array().unwrap().iter().map(|t| &t["track"]).collect();
    assert_eq!((tracks, &many["missing"]), (vec![&json!(2), &json!(1)], &json!([404])), "{many}");
    let one = s.call(2, "track.analysis", json!({ "drive": a, "track": 1, "waveform": "none" }))["result"].clone();
    assert_eq!(many["tracks"][1]["analysis"], one);
    let too_many = s.call(3, "tracks.analysis", json!({ "drive": a, "ids": vec![1; 101] }));
    assert_eq!(kind(&too_many), "invalid", "{too_many}");
    let scrolling = s.call(8, "tracks.analysis", json!({ "drive": a, "ids": [1], "waveform": "detail" }));
    assert_eq!(kind(&scrolling), "invalid", "{scrolling}");

    let store = root.join("store");
    let nothing = json!({ "tracks": [], "missing": [] });
    assert_eq!(s.call(4, "track.fields", json!({ "drive": a }))["result"], nothing);
    assert!(!store.exists(), "a read made the store");
    let file = s.call(5, "tracks.get", json!({ "drive": a, "ids": [1] }))["result"]["tracks"][0]["file_path"].clone();
    let audio = a.join(file.as_str().unwrap().trim_start_matches('/'));
    let fields = FieldStore::in_store(&Store::open_or_init(&store).unwrap());
    let bpm: Fields = [("bpm".to_string(), FieldValue::Number(128.0))].into_iter().collect();
    let tempo = Analyzer { plugin: "analysis".into(), version: "0.1.0".into(), analyzer: "tempo".into() };
    fields.put(&audio, &tempo, &bpm).unwrap();
    // Only the fields of plugins locallibrary has are read, and it has none here (tests/it/plugins.rs
    // gives it the test plugin).
    assert_eq!(s.call(6, "track.fields", json!({ "drive": a }))["result"], nothing);
    let other = s.call(7, "track.fields", json!({ "drive": a, "tracks": [2, 404, 1], "plugin": "other" }));
    assert_eq!(other["result"], json!({ "tracks": [], "missing": [404] }));
    assert!(s.exit().success());
}

#[test]
fn errors_say_why_and_what_would_resolve_them() {
    let mut s = Sidecar::start();
    let root = s.root().to_path_buf();
    let (a, b) = (demo_drive(&root, "A"), demo_drive(&root, "B"));
    let data = |v: &Value| v["error"]["data"].clone();

    // Bad parameters say where they are.
    let v = s.call(1, "edit.plan", json!({ "drive": a, "edits": [{ "op": "create_playlist" }] }));
    assert_eq!(kind(&v), "invalid_params", "{v}");
    assert!(data(&v)["fields"]["path"].as_str().is_some_and(|p| p.starts_with("edits[0]")), "{v}");
    assert!(v["error"]["message"].as_str().unwrap().contains("edits[0]"), "{v}");

    // A library the store doesn't have: start it, by sending the same request with one more
    // parameter.
    let v = s.call(2, "pull", json!({ "drive": a, "library": "Main" }));
    let d = data(&v);
    assert_eq!(
        (&d["kind"], &d["reason"], &d["fields"]["library"]),
        (&json!("not_found"), &json!("unknown_library"), &json!("Main"))
    );
    let start = &d["actions"][0];
    assert_eq!(start["method"], "pull");
    assert_eq!(start["params"], json!({ "drive": a, "library": "Main", "new_library": true }));
    assert!(start["label"].as_str().is_some_and(|l| !l.is_empty()));
    assert!(s.call(3, "pull", start["params"].clone())["error"].is_null());
    assert!(s.call(4, "pull", json!({ "drive": b, "library": "Main" }))["error"].is_null());

    // A retired drive: bring it back, or write to it anyway.
    assert!(s.call(5, "drive.retire", json!({ "drive": b }))["error"].is_null());
    s.call(6, "edit.apply", json!({ "drive": a, "edits": [{ "op": "create_playlist", "path": "New" }] }));
    let sync = json!({ "left": a, "right": b, "direction": "left_to_right" });
    let v = s.call(7, "sync.plan", sync.clone());
    let d = data(&v);
    assert_eq!((&d["kind"], &d["reason"]), (&json!("conflict"), &json!("retired")), "{v}");
    let methods: Vec<&Value> = d["actions"].as_array().unwrap().iter().map(|a| &a["method"]).collect();
    assert_eq!(methods, [&json!("drive.retire"), &json!("sync.plan")], "{d}");
    assert_eq!(d["actions"][0]["params"]["undo"], true);
    let anyway = &d["actions"][1]["params"];
    assert_eq!((&anyway["overwrite_archive"], &anyway["left"]), (&json!(true), &json!(a)));
    let plan = s.call(8, "sync.plan", anyway.clone());
    assert!(plan["result"]["plan_id"].is_string(), "{plan}");
    assert!(s.exit().success());
}

/// The mix methods that keep fingerprints, reports or exports in the store say they write: a
/// front end treats reads as free to call, and they never make a store.
#[test]
fn mix_methods_that_keep_files_say_they_write() {
    let mut s = Sidecar::start();
    for name in ["mix.index", "mix.identify", "mix.export", "mix.reports.html"] {
        let m = s.call(1, "rpc.describe", json!({ "method": name }))["result"]["methods"][0].clone();
        assert_eq!(m["kind"], "immediate", "{name}: {m}");
    }
    assert!(s.exit().success());
}

/// `mix.export` doesn't replace a file at its `output` unless asked to, as no writer on this
/// computer does: refused before the mix is identified, with the request plus `replace` as the
/// way on.
#[test]
fn mix_export_keeps_a_file_at_its_output() {
    let mut s = Sidecar::start();
    let output = s.root().join("set.mix.json");
    std::fs::write(&output, "mine").unwrap();
    let request = json!({ "mix": s.root().join("set.wav"), "output": output });
    let v = s.call(1, "mix.export", request.clone());
    let d = &v["error"]["data"];
    assert_eq!((&d["kind"], &d["reason"]), (&json!("conflict"), &json!("exists")), "{v}");
    let mut replace = request;
    replace["replace"] = json!(true);
    assert_eq!(d["actions"][0]["params"], replace, "{d}");
    assert_eq!(std::fs::read_to_string(&output).unwrap(), "mine");
    assert!(s.exit().success());
}

#[test]
fn mix_reports_kept_in_the_store_are_listed_read_and_rendered() {
    let mut s = Sidecar::start();
    let root = s.root().to_path_buf();
    assert_eq!(s.call(1, "mix.reports.list", json!({}))["result"], json!([]));

    // A report as `mix.identify {save: true}` keeps it, of a recording next to its audio.
    let mixes = root.join("mixes");
    std::fs::create_dir_all(&mixes).unwrap();
    std::fs::write(mixes.join("warehouse.aup3"), "project").unwrap();
    std::fs::write(mixes.join("warehouse export.mp3"), "audio").unwrap();
    let report = json!({
        "mix": {
            "path": mixes.join("warehouse.aup3"), "kind": "audacity", "title": "Live at the Warehouse",
            "artist": "Example DJ", "genre": null, "date": null, "recorder": null, "duration_secs": 3600.0,
            "sample_rate": 44100, "channels": 2, "labels": [], "cover": null,
        },
        "drive": "DNA",
        "library": { "tracks": 0, "indexed": 0, "fingerprinted": 0, "missing": [], "failed": [], "hashes": 0 },
        "plays": [], "transitions": [], "unidentified": [{ "start": 0.0, "end": 3600.0 }], "coverage": 0.0,
        "waveform": { "columns_per_sec": 4.0, "peak": [], "low": [], "mid": [], "high": [] },
        "tempo_range": 0.16, "analysed_at": "2026-10-05T21:00:00Z", "elapsed_secs": 9.5,
    });
    let reports = root.join("store/reports");
    std::fs::create_dir_all(&reports).unwrap();
    std::fs::write(reports.join("live-at-the-warehouse.json"), report.to_string()).unwrap();

    let list = s.call(2, "mix.reports.list", json!({}))["result"].clone();
    assert_eq!(list[0]["id"], "live-at-the-warehouse", "{list}");
    assert_eq!(
        (&list[0]["title"], &list[0]["drive"], &list[0]["html"]),
        (&json!("Live at the Warehouse"), &json!("DNA"), &Value::Null)
    );

    let got = s.call(3, "mix.reports.get", json!({ "id": "live-at-the-warehouse" }))["result"].clone();
    assert_eq!((&got["mix"]["kind"], &got["saved"]), (&json!("audacity"), &json!("live-at-the-warehouse")), "{got}");
    assert_eq!(got["audio"], json!(mixes.join("warehouse export.mp3")), "the export next to the project plays");

    // The page is written beside it (no drive mounted: no tracks' waveforms) and listed.
    let page =
        s.call(4, "mix.reports.html", json!({ "id": "live-at-the-warehouse", "inline": true }))["result"].clone();
    let path = PathBuf::from(page["path"].as_str().unwrap_or_else(|| panic!("{page}")));
    assert_eq!(path, reports.join("live-at-the-warehouse.html"));
    let html = std::fs::read_to_string(&path).unwrap();
    assert!(html.contains("Live at the Warehouse") && html.contains("warehouse%20export.mp3"), "the audio is linked");
    assert_eq!(page["html"].as_str(), Some(html.as_str()));
    let list = s.call(5, "mix.reports.list", json!({}))["result"].clone();
    assert_eq!(list[0]["html"], json!(path));

    let missing = s.call(6, "mix.reports.get", json!({ "id": "nope" }));
    assert_eq!(kind(&missing), "not_found", "{missing}");
    assert_eq!(kind(&s.call(7, "mix.reports.html", json!({ "id": "../x" }))), "invalid");
    assert!(s.exit().success());
}

/// Regression: an apply by id refused before anything was written (the drive was made an archive
/// drive after planning) used the plan up, so retrying with `overwrite_archive`, as the error
/// offers, answered that the plan was applied already.
#[test]
fn an_apply_refused_before_writing_keeps_its_plan() {
    let mut s = Sidecar::start();
    let root = s.root().to_path_buf();
    let a = demo_drive(&root, "A");
    assert!(s.call(1, "pull", json!({ "drive": a, "library": "Main", "new_library": true }))["error"].is_null());
    let edit = json!({ "drive": a, "edits": [{ "op": "create_playlist", "path": "Friday" }] });
    let id = s.call(2, "edit.plan", edit)["result"]["plan_id"].clone();
    assert!(s.call(3, "drive.role", json!({ "drive": a, "role": "archive" }))["error"].is_null());
    let refused = s.call(4, "edit.apply", json!({ "plan_id": id }));
    assert_eq!(kind(&refused), "conflict", "{refused}");
    let actions = refused["error"]["data"]["actions"].as_array().unwrap().clone();
    let retry = actions.iter().find(|a| a["method"] == "edit.apply").unwrap_or_else(|| panic!("{refused}"));
    assert_eq!(retry["params"], json!({ "plan_id": id, "overwrite_archive": true }));
    let done = s.call(5, "edit.apply", retry["params"].clone());
    assert!(done["error"].is_null(), "{done}");
    let again = s.call(6, "edit.apply", retry["params"].clone());
    assert_eq!(kind(&again), "stale_plan", "applied once: {again}");
    assert!(s.exit().success());
}

#[test]
fn a_plan_is_applied_by_its_id_once_and_not_once_stale() {
    let mut s = Sidecar::start();
    let root = s.root().to_path_buf();
    let a = demo_drive(&root, "A");
    let edit = |name: &str| json!({ "drive": a, "edits": [{ "op": "create_playlist", "path": name }] });

    let plan = s.call(1, "edit.plan", edit("Friday"));
    let id = plan["result"]["plan_id"].as_str().unwrap_or_else(|| panic!("{plan}")).to_string();
    assert_eq!(plan["result"]["operation"], "edit");
    assert_eq!(plan["result"]["playlists_created"], json!(["Friday"]), "the plan's own fields stay");
    assert!(plan["result"]["summary"].as_array().is_some_and(|s| !s.is_empty()), "{plan}");

    // Applied by id, exactly once; and only by the method of its kind.
    let wrong = s.call(2, "convert.apply", json!({ "plan_id": id }));
    assert_eq!(kind(&wrong), "invalid_params", "{wrong}");
    let done = s.call(3, "edit.apply", json!({ "plan_id": id }));
    assert!(done["result"]["changes"].as_u64().is_some(), "{done}");
    let again = s.call(4, "edit.apply", json!({ "plan_id": id }));
    assert_eq!(kind(&again), "stale_plan", "{again}");

    // A plan made before another write to the drive is refused.
    let stale = s.call(5, "edit.plan", edit("Saturday"))["result"]["plan_id"].clone();
    s.call(6, "edit.apply", edit("Sunday"));
    let refused = s.call(7, "edit.apply", json!({ "plan_id": stale }));
    assert_eq!(kind(&refused), "stale_plan", "{refused}");
    let playlists = s.call(8, "drive.library", json!({ "drive": a }))["result"]["library"]["playlists"].clone();
    let names: Vec<&str> = playlists.as_array().unwrap().iter().filter_map(|p| p["name"].as_str()).collect();
    assert!(names.contains(&"Sunday") && !names.contains(&"Saturday"), "{names:?}");

    // Sent twice at once, to two workers: applied once.
    let id = s.call(9, "edit.plan", edit("Monday"))["result"]["plan_id"].clone();
    s.send(10, "edit.apply", json!({ "plan_id": id }));
    s.send(11, "edit.apply", json!({ "plan_id": id }));
    let (first, second) = (s.reply_to(10), s.reply_to(11));
    let mut kinds = [kind(&first).as_str().unwrap_or("ok"), kind(&second).as_str().unwrap_or("ok")];
    kinds.sort();
    assert_eq!(kinds, ["ok", "stale_plan"], "{first} {second}");

    // Undone by plan id: the next read of the (cached) library doesn't have it.
    let tree = |s: &mut Sidecar, id: i64| -> Vec<String> {
        let tree = s.call(id, "playlists.tree", json!({ "drive": a }))["result"]["playlists"].clone();
        tree.as_array().unwrap().iter().map(|p| p["name"].as_str().unwrap().to_string()).collect()
    };
    assert!(tree(&mut s, 12).contains(&"Monday".to_string()));
    let undo = s.call(13, "undo.plan", json!({ "drive": a }))["result"].clone();
    assert_eq!(undo["operation"], "undo", "{undo}");
    let undone = s.call(14, "undo.apply", json!({ "plan_id": undo["plan_id"] }));
    assert!(undone["error"].is_null(), "{undone}");
    assert!(!tree(&mut s, 15).contains(&"Monday".to_string()));
    assert!(s.exit().success());
}

/// Regression: giving a drive a new identity had a plan in the library (its refusal, its
/// warning), but only the immediate `drive.reidentify` over RPC, so a GUI couldn't show either
/// before the change.
#[test]
fn reidentify_is_planned_then_applied() {
    let mut s = Sidecar::start();
    let root = s.root().to_path_buf();
    let a = demo_drive(&root, "A");
    assert!(s.call(1, "drive.adopt", json!({ "drive": a, "name": "A" }))["error"].is_null());
    let refused = s.call(2, "drive.reidentify.plan", json!({ "drive": a }));
    assert_eq!(kind(&refused), "conflict", "nothing shows it's a copy: {refused}");
    let plan = s.call(3, "drive.reidentify.plan", json!({ "drive": a, "force": true }))["result"].clone();
    assert_eq!(plan["operation"], "drive.reidentify", "{plan}");
    let done = s.call(4, "drive.reidentify.apply", json!({ "plan_id": plan["plan_id"] }))["result"].clone();
    assert!(done["id"].is_string() && done["id"] != plan["id"], "a new identity: {done}");
    assert!(s.exit().success());
}

#[test]
fn drive_groups_answer_under_their_old_names_too() {
    let mut s = Sidecar::start();
    let methods = s.call(1, "version", json!({}))["result"]["methods"].clone();
    let listed = |m: &str| methods.as_array().unwrap().iter().any(|v| v == m);
    assert!(listed("drive.groups") && listed("sync.group.plan"), "{methods}");
    // The other spellings are one method's aliases.
    let described = s.call(5, "rpc.describe", json!({ "method": "sync_group.plan" }))["result"]["methods"][0].clone();
    assert_eq!(described["name"], "sync.group.plan");
    assert_eq!(described["aliases"], json!(["sync_group.plan", "sync_set.plan"]));
    assert_eq!(s.call(2, "drive.groups", json!({}))["result"], json!([]));
    assert_eq!(s.call(3, "drive.sets", json!({}))["result"], json!([]));
    // The old name plans under the new one.
    let v = s.call(4, "sync_set.plan", json!({}));
    assert_eq!(kind(&v), "not_found", "{v}");
    assert!(s.exit().success());
}
