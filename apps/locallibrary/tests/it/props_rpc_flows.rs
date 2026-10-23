//! Property tests of the `locallibrary rpc` sidecar's own state, driven by random sequences of
//! requests and checked against a model of it: plans kept until applied once (by the method of
//! their kind), subscriptions to events and the events they let through, watching drives, what
//! `initialize` and `rpc.describe` say, errors whose actions name the request to send again,
//! lines too long to read, and a shutdown that answers what's running.

use std::io::{BufRead, BufReader, Write};
use std::process::Stdio;
use std::sync::mpsc::Receiver;
use std::time::Duration;

use hegel::generators as gs;
use hegel::{HealthCheck, TestCase};
use rustc_hash::FxHashMap;
use serde_json::{Value, json};

use crate::support::{Env, path};

/// The sidecar, its messages read on a thread of their own so one can be waited for with a
/// timeout, those read while waiting for another kept.
struct Sidecar {
    child: std::process::Child,
    stdin: Option<std::process::ChildStdin>,
    messages: Receiver<Value>,
    kept: Vec<Value>,
    next: i64,
}

impl Sidecar {
    fn start(fx: &Env) -> Sidecar {
        Sidecar::start_with(fx, &[])
    }

    /// With `args` after `rpc` (`--workers 1`).
    fn start_with(fx: &Env, args: &[&str]) -> Sidecar {
        let mut child = fx
            .command()
            .arg("rpc")
            .args(args)
            .env("LOCALLIBRARY_RPC_TEST", "1")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let stdout = child.stdout.take().unwrap();
        let (tx, messages) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if tx.send(serde_json::from_str::<Value>(&line).unwrap()).is_err() {
                    break;
                }
            }
        });
        let stdin = child.stdin.take();
        Sidecar { child, stdin, messages, kept: Vec::new(), next: 0 }
    }

    fn write(&mut self, line: &[u8]) {
        let stdin = self.stdin.as_mut().unwrap();
        stdin.write_all(line).unwrap();
        stdin.write_all(b"\n").unwrap();
    }

    /// A request with a fresh id, and its response.
    fn call(&mut self, method: &str, params: Value) -> Value {
        self.next += 1;
        let id = json!(self.next);
        let msg = json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params });
        self.write(msg.to_string().as_bytes());
        self.answer(&id)
    }

    /// The response to `id` (a null id: the first response that has none).
    fn answer(&mut self, id: &Value) -> Value {
        let is_it = |m: &Value| m.get("method").is_none() && m["id"] == *id;
        if let Some(i) = self.kept.iter().position(is_it) {
            return self.kept.remove(i);
        }
        loop {
            let m = self.messages.recv_timeout(Duration::from_secs(60)).expect("an answer");
            if is_it(&m) {
                return m;
            }
            self.kept.push(m);
        }
    }

    /// The events sent so far, taken.
    fn events(&mut self) -> Vec<Value> {
        self.kept.extend(self.messages.try_iter());
        let (events, rest) = std::mem::take(&mut self.kept).into_iter().partition(|m| m["method"] == "event");
        self.kept = rest;
        events.into_iter().map(|e: Value| e["params"].clone()).collect()
    }
}

/// A sidecar a test case left running when it failed (a request waiting minutes, say) is
/// stopped, so the cases shrinking runs don't pile up. One shut down as it should be has exited.
impl Drop for Sidecar {
    fn drop(&mut self) {
        // Its input closed, it exits on its own; one still running after that is stuck.
        drop(self.stdin.take());
        crate::support::end(&mut self.child, std::time::Duration::from_secs(10));
    }
}

/// What the model knows of the sidecar.
#[derive(Debug, Default)]
struct Model {
    /// Plans kept, by id: their operation.
    plans: FxHashMap<String, &'static str>,
    /// Plans applied (or used up): applying them again is stale.
    used: Vec<String>,
    /// The kinds of event subscribed to (empty: all), when subscribed.
    subscribed: Option<Vec<String>>,
    watch: Watch,
}

/// Watching drives, as far as the model knows.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
enum Watch {
    #[default]
    Off,
    On,
    /// Told to stop: it ends once it's done what it's doing, so for a while it may still be on.
    Stopping,
}

/// Whether an event of `kind` is let through by a subscription to `kinds`: a kind is wanted by
/// its own name or its group's (`job` for `job.finished`).
fn wants(kinds: &[String], kind: &str) -> bool {
    kinds.is_empty() || kinds.iter().any(|k| kind == k || kind.starts_with(&format!("{k}.")))
}

/// The events received so far are each one the subscription `m` has wants (`watch.*` events are
/// sent while watching whether subscribed or not).
fn events_are_wanted(s: &mut Sidecar, m: &Model) {
    for e in s.events() {
        let kind = e["event"].as_str().unwrap_or_default();
        if kind.starts_with("watch.") {
            continue;
        }
        let kinds = m.subscribed.as_ref().unwrap_or_else(|| panic!("an event while unsubscribed: {e}"));
        assert!(wants(kinds, kind), "{kind} let through by {kinds:?}: {e}");
    }
}

/// Requests in any order (and a drive plugged in or pulled out), checked against the model after
/// each.
#[hegel::test(test_cases = 10, suppress_health_check = [HealthCheck::TooSlow])]
fn the_sidecar_keeps_plans_subscriptions_and_watches_as_modelled(tc: TestCase) {
    let fx = Env::from_template();
    let mut s = Sidecar::start(&fx);
    let mut m = Model::default();
    let a = path(&fx.volumes.join("A")).to_string();
    let contract: Value = serde_json::from_slice(
        &std::fs::read(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("docs/rpc/schema.json")).unwrap(),
    )
    .unwrap();
    let methods: Vec<&str> =
        contract["methods"].as_array().unwrap().iter().map(|m| m["name"].as_str().unwrap()).collect();
    let mut names = 0;
    for _ in 0..tc.draw(gs::integers::<usize>().min_value(2).max_value(16)) {
        match tc.draw(gs::integers::<u8>().max_value(12)) {
            0 => {
                let v = s.call("initialize", json!({}));
                let r = &v["result"];
                assert_eq!(
                    r["capabilities"].as_array().unwrap().contains(&json!("account")),
                    cfg!(feature = "account"),
                    "{v}"
                );
                let listed: Vec<&str> = r["methods"].as_array().unwrap().iter().map(|m| m.as_str().unwrap()).collect();
                assert!(methods.iter().all(|m| listed.contains(m)), "{v}");
            }
            1 => {
                let method = match tc.draw(gs::weighted_booleans(0.8)) {
                    true => tc.draw(gs::sampled_from(&methods[..])).to_string(),
                    false => "no.such.method".to_string(),
                };
                let v = s.call("rpc.describe", json!({ "method": method }));
                match methods.contains(&method.as_str()) {
                    true => assert_eq!(v["result"]["methods"][0]["name"], method.as_str(), "{v}"),
                    false => assert_eq!(v["error"]["data"]["kind"], "invalid_params", "{v}"),
                }
                // A parameter of the wrong type says which.
                let v = s.call("rpc.describe", json!({ "method": 7 }));
                assert_eq!(v["error"]["data"]["fields"]["path"], "method", "{v}");
            }
            2 => {
                let kinds: Vec<String> = tc
                    .draw(gs::subsequences(vec!["job", "drive", "store", "job.finished"]))
                    .into_iter()
                    .map(String::from)
                    .collect();
                let v = s.call("events.subscribe", json!({ "kinds": kinds, "interval_ms": 100 }));
                assert_eq!(v["result"]["kinds"], json!(kinds), "{v}");
                // What came before the answer came under the subscription before, or this one.
                let before = m.subscribed.replace(kinds.clone());
                for e in s.events() {
                    let kind = e["event"].as_str().unwrap_or_default();
                    let by = |k: &Option<Vec<String>>| k.as_ref().is_some_and(|k| wants(k, kind));
                    assert!(kind.starts_with("watch.") || by(&before) || by(&m.subscribed), "{e}");
                }
            }
            3 => {
                let v = s.call("events.unsubscribe", json!({}));
                assert_eq!(v["result"]["unsubscribed"], m.subscribed.is_some(), "{v}");
                events_are_wanted(&mut s, &m);
                m.subscribed = None;
                // The watcher polling for the subscription ends at its next look.
                std::thread::sleep(Duration::from_millis(150));
            }
            4 => {
                let v = s.call("watch.start", json!({ "settle_ms": 0, "interval_ms": 100 }));
                let started = v["result"]["watching"] == true;
                if !started {
                    assert_eq!(v["error"]["data"]["kind"], "conflict", "{v}");
                }
                match (m.watch, started) {
                    (Watch::Off, _) => assert!(started, "{v}"),
                    (Watch::On, _) => assert!(!started, "started twice: {v}"),
                    (Watch::Stopping, _) => {}
                }
                if started {
                    m.watch = Watch::On;
                }
                if m.watch == Watch::On {
                    let jobs = s.call("jobs.list", json!({}));
                    assert!(jobs["result"].as_array().unwrap().iter().any(|j| j["method"] == "watch"), "{jobs}");
                }
            }
            5 => {
                let cancel = tc.draw(gs::booleans());
                let v = s.call("watch.stop", json!({ "cancel": cancel }));
                let stopped = v["result"]["stopped"].as_bool().unwrap_or_else(|| panic!("{v}"));
                match m.watch {
                    Watch::Off => assert!(!stopped, "{v}"),
                    Watch::On => assert!(stopped, "{v}"),
                    Watch::Stopping => {}
                }
                if m.watch != Watch::Off {
                    m.watch = Watch::Stopping;
                }
            }
            6 | 7 => {
                names += 1;
                let edit =
                    json!({ "drive": a, "edits": [{ "op": "create_playlist", "path": format!("Friday {names}") }] });
                let v = s.call("edit.plan", edit);
                let id = v["result"]["plan_id"].as_str().unwrap_or_else(|| panic!("{v}")).to_string();
                m.plans.insert(id, "edit");
                if tc.draw(gs::booleans()) {
                    let v =
                        s.call("tags.plan", json!({ "drive": a, "action": "add", "tag": "Genre/Deep", "ids": [1] }));
                    if let Some(id) = v["result"]["plan_id"].as_str() {
                        m.plans.insert(id.to_string(), "tags");
                    }
                }
            }
            8 | 9 => {
                // A kept plan, one applied already, one of the other kind, or none at all.
                let kept: Vec<(String, &str)> = m.plans.iter().map(|(k, v)| (k.clone(), *v)).collect();
                let (id, op) = match tc.draw(gs::integers::<u8>().max_value(2)) {
                    0 if !kept.is_empty() => tc.draw(gs::sampled_from(&kept[..])),
                    1 if !m.used.is_empty() => (tc.draw(gs::sampled_from(&m.used[..])), "edit"),
                    _ => ("no-such-plan".to_string(), "edit"),
                };
                let apply_as =
                    if tc.draw(gs::weighted_booleans(0.3)) { if op == "edit" { "tags" } else { "edit" } } else { op };
                let v = s.call(&format!("{apply_as}.apply"), json!({ "plan_id": id }));
                let said = format!("{apply_as}.apply {id} ({op}): {v}\n{m:?}");
                match m.plans.get(&id) {
                    Some(made) if *made != apply_as => {
                        assert_eq!(v["error"]["data"]["kind"], "invalid_params", "{said}");
                        assert!(
                            v["error"]["message"]
                                .as_str()
                                .unwrap()
                                .contains(&format!("is a {made} plan, not {apply_as}")),
                            "{said}"
                        );
                    }
                    Some(_) => {
                        // Applied, or refused as stale since the drive changed under it.
                        let kind = &v["error"]["data"]["kind"];
                        assert!(v["error"].is_null() || kind == "stale_plan", "{said}");
                        m.plans.remove(&id);
                        m.used.push(id);
                    }
                    None => assert_eq!(v["error"]["data"]["kind"], "stale_plan", "{said}"),
                }
            }
            10 => {
                // Nothing to cancel: an id never sent, or none.
                let id = if tc.draw(gs::booleans()) { json!("never-sent") } else { Value::Null };
                let v = s.call("$/cancel", json!({ "id": id }));
                assert_eq!(v["result"]["cancelled"], false, "{v}");
            }
            11 => {
                // An error that offers sending the request again with more names the method and
                // what was asked: undoing an edit on the club drive, whose audio isn't there.
                // By its path: a drive plugged in as a copy of it has its name too.
                let club = path(&fx.volumes.join("Club")).to_string();
                let edit =
                    json!({ "drive": club, "edits": [{ "op": "create_playlist", "path": format!("Spare {names}") }] });
                names += 1;
                let v = s.call("edit.apply", edit);
                assert!(v["error"].is_null(), "{v}");
                let v = s.call("undo.plan", json!({ "drive": club }));
                let actions = v["error"]["data"]["actions"].as_array().unwrap_or_else(|| panic!("{v}"));
                let again = actions.iter().find(|a| a["method"] == "undo.plan").unwrap_or_else(|| panic!("{v}"));
                assert_eq!((&again["params"]["drive"], &again["params"]["force"]), (&json!(club), &json!(true)), "{v}");
            }
            _ => {
                // A drive plugged in, or pulled out again: the watcher sees it at its next look.
                let spare = fx.volumes.join("Spare");
                match spare.exists() {
                    true => std::fs::remove_dir_all(&spare).unwrap(),
                    false => crate::support::copy_dir(&fx.volumes.join("Club"), &spare),
                }
                std::thread::sleep(Duration::from_millis(250));
            }
        }
        events_are_wanted(&mut s, &m);
    }
    // A line longer than anything a request needs is skipped, and the sidecar carries on.
    if tc.draw(gs::weighted_booleans(0.3)) {
        let line = vec![b'x'; (16 << 20) + 10];
        s.write(&line);
        let e = s.answer(&Value::Null);
        assert_eq!(e["error"]["data"]["kind"], "parse_error", "{e}");
        assert!(e["error"]["message"].as_str().unwrap().contains("longer than 16 MiB"), "{e}");
    }
    // Blank lines are skipped; a shutdown it can't read is refused, and the sidecar carries on.
    s.write(b"");
    let v = s.call("shutdown", json!({ "cancel": "now" }));
    assert_eq!(v["error"]["data"]["kind"], "invalid_params", "{v}");
    let v = s.call("version", json!({}));
    assert!(v["result"].is_object(), "{v}");
    let v = s.call("shutdown", json!({ "cancel": true }));
    assert_eq!(v["result"], Value::Null, "{v}");
    drop(s.stdin.take());
    assert!(s.child.wait().unwrap().success());
}

/// A pack copied from drive A onto the blank drive over the sidecar: its progress is the request's
/// own, comes before its response, and counts no more bytes copied than the plan copies.
#[hegel::test(test_cases = 3, suppress_health_check = [HealthCheck::TooSlow])]
fn progress_counts_what_the_request_copies(tc: TestCase) {
    let fx = Env::from_template();
    let mut s = Sidecar::start(&fx);
    let (a, blank) = (path(&fx.volumes.join("A")).to_string(), path(&fx.volumes.join("Blank")).to_string());
    let playlists: Vec<&str> = tc.draw(gs::sampled_from(&[vec!["Set"], vec![]][..]));
    let plan = s.call("pack.plan", json!({ "source": a, "target": blank, "playlists": playlists }));
    let Some(id) = plan["result"]["plan_id"].as_str().map(String::from) else {
        assert!(plan["error"].is_object(), "{plan}");
        return;
    };
    let copy = plan["result"]["copy_bytes"].as_u64().unwrap();
    s.next += 1;
    let rid = json!(s.next);
    s.write(
        json!({ "jsonrpc": "2.0", "id": rid, "method": "pack.apply", "params": { "plan_id": id } })
            .to_string()
            .as_bytes(),
    );
    let mut bytes = 0;
    let reply = loop {
        let m = s.messages.recv_timeout(Duration::from_secs(60)).unwrap();
        if m["method"] == "progress" {
            assert_eq!(m["params"]["id"], rid, "{m}");
            let e = &m["params"]["event"];
            // The copying's own: hashing and verifying read what's copied again.
            if e["event"] == "bytes" && e["phase"] == "copy" {
                bytes += e["bytes"].as_u64().unwrap();
            }
            continue;
        }
        if m["id"] == rid {
            break m;
        }
    };
    assert!(reply["error"].is_null(), "{reply}");
    assert!(bytes <= copy, "{bytes} bytes said for a copy of {copy}");
    assert_eq!(bytes > 0, copy > 0, "{bytes} bytes said for a copy of {copy}");
    drop(s.stdin.take());
    assert!(s.child.wait().unwrap().success());
}

/// Plans of several kinds over the sidecar (undoing an edit, a playlist brought back from the
/// store's snapshot, a history list deleted once the store has it, the store's garbage), in any
/// order: each is answered with an id and its operation, applied by that id once (or refused as
/// stale when the drive changed since), and never again.
#[hegel::test(test_cases = 4, suppress_health_check = [HealthCheck::TooSlow])]
fn plans_of_any_kind_apply_once(tc: TestCase) {
    let fx = Env::from_template();
    let mut s = Sidecar::start(&fx);
    let a = path(&fx.volumes.join("A")).to_string();
    // Something to undo.
    let edit = json!({ "drive": a, "edits": [{ "op": "rename_playlist", "playlist": "Set", "name": "Friday" }] });
    let v = s.call("edit.apply", edit);
    assert!(v["error"].is_null(), "{v}");
    let kinds = [
        ("drive.restore", "undo", json!({ "drive": a })),
        ("playlist.restore", "playlist.restore", json!({ "drive": a, "playlist": "Set" })),
        ("history.delete", "history.delete", json!({ "drive": a, "names": ["HISTORY 001"] })),
        ("store.gc", "store.gc", json!({})),
    ];
    for i in tc.draw(gs::permutations(vec![0, 1, 2, 3])) {
        let (method, operation, params) = &kinds[i];
        let plan = s.call(&format!("{method}.plan"), params.clone());
        let said = format!("{method}.plan: {plan}");
        let Some(id) = plan["result"]["plan_id"].as_str().map(String::from) else {
            // Nothing to bring back once an undo put the playlist back, say.
            assert!(plan["error"]["data"]["kind"].is_string(), "{said}");
            continue;
        };
        assert_eq!(plan["result"]["operation"], *operation, "{said}");
        let applied = s.call(&format!("{method}.apply"), json!({ "plan_id": id }));
        let kind = &applied["error"]["data"]["kind"];
        assert!(applied["error"].is_null() || kind == "stale_plan", "{said}\n{applied}");
        let again = s.call(&format!("{method}.apply"), json!({ "plan_id": id }));
        assert_eq!(again["error"]["data"]["kind"], "stale_plan", "{said}\n{again}");
    }
    // Applied with what it was planned from (no plan to name): the same change, made at once.
    for (method, params) in [
        ("history.delete.apply", json!({ "drive": a, "names": ["HISTORY 001"], "force": true })),
        ("playlist.restore.apply", json!({ "drive": a, "playlist": "Set" })),
        ("store.gc.apply", json!({})),
    ] {
        let v = s.call(method, params);
        assert!(v["error"].is_null() || v["error"]["data"]["kind"].is_string(), "{method}: {v}");
    }
    let v = s.call("sets.played", json!({ "track": "Demo" }));
    assert!(v["result"].is_array(), "{v}");
    // A drive made an archive since an edit was planned refuses it, keeping the plan to apply
    // again once allowed.
    let plan = s.call("edit.plan", json!({ "drive": a, "edits": [{ "op": "create_playlist", "path": "Archived" }] }));
    let id = plan["result"]["plan_id"].clone();
    let role = s.call("drive.role", json!({ "drive": a, "role": "archive" }));
    assert!(role["error"].is_null(), "{role}");
    let refused = s.call("edit.apply", json!({ "plan_id": id }));
    assert_eq!(refused["error"]["data"]["kind"], "conflict", "{refused}");
    let allowed = s.call("edit.apply", json!({ "plan_id": id, "overwrite_archive": true }));
    assert!(allowed["error"].is_null(), "the plan stayed: {allowed}");
    // A write to a path that isn't absolute is refused before anything runs.
    let v = s.call("art.embed.plan", json!({ "image": "cover.png", "files": ["relative.mp3"] }));
    assert_eq!(v["error"]["data"]["kind"], "invalid_params", "{v}");
    // Items a request works through are each counted in its progress, before its answer.
    // Held back while they come quickly, then sent while the request waits on.
    let items = tc.draw(gs::integers::<u64>().max_value(3000));
    s.next += 1;
    let rid = json!(s.next);
    s.write(
        json!({ "jsonrpc": "2.0", "id": rid, "method": "$/test.wait", "params": { "ms": 250, "items": items } })
            .to_string()
            .as_bytes(),
    );
    let mut counted = 0;
    loop {
        let m = s.messages.recv_timeout(Duration::from_secs(60)).unwrap();
        if m["method"] == "progress" && m["params"]["event"]["event"] == "item" {
            counted += m["params"]["event"]["items"].as_u64().unwrap_or(1);
        }
        if m["id"] == rid {
            break;
        }
    }
    assert_eq!(counted, items);
    drop(s.stdin.take());
    assert!(s.child.wait().unwrap().success());
}

/// Requests running when the sidecar is told to stop (SIGTERM or SIGINT, as a GUI closing or
/// Ctrl-C): each answers that it was cancelled, and the sidecar exits with 128 and the signal's
/// number.
#[cfg(unix)]
#[hegel::test(test_cases = 3, suppress_health_check = [HealthCheck::TooSlow])]
fn a_signal_cancels_what_is_running_then_exits(tc: TestCase) {
    let fx = Env::from_template();
    let mut s = Sidecar::start(&fx);
    let running: Vec<Value> = (0..tc.draw(gs::integers::<usize>().min_value(1).max_value(3)))
        .map(|_| {
            s.next += 1;
            let id = json!(s.next);
            let msg = json!({ "jsonrpc": "2.0", "id": id, "method": "$/test.wait", "params": { "ms": 600_000 } });
            s.write(msg.to_string().as_bytes());
            id
        })
        .collect();
    // Each has started once jobs.list lists it.
    loop {
        let jobs = s.call("jobs.list", json!({}));
        if jobs["result"].as_array().unwrap().len() >= running.len() {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let signal = tc.draw(gs::sampled_from(&[libc::SIGTERM, libc::SIGINT][..]));
    // SAFETY: a signal to the child this test started.
    assert_eq!(unsafe { libc::kill(s.child.id() as i32, signal) }, 0);
    for id in &running {
        let a = s.answer(id);
        assert_eq!(a["error"]["data"]["kind"], "cancelled", "{a}");
    }
    let status = s.child.wait().unwrap();
    use std::os::unix::process::ExitStatusExt;
    assert_eq!((status.code(), status.signal()), (Some(128 + signal), None), "{status:?}");
}

/// With one worker, requests wait their turn: one cancelled while it waits answers `cancelled`
/// at once (and its `job.finished` says so), while the one running carries on until it too is
/// cancelled.
#[hegel::test(test_cases = 4, suppress_health_check = [HealthCheck::TooSlow])]
fn a_waiting_request_cancelled_answers_at_once(tc: TestCase) {
    let fx = Env::from_template();
    let mut s = Sidecar::start_with(&fx, &["--workers", "1"]);
    s.call("events.subscribe", json!({ "kinds": ["job"] }));
    let n = tc.draw(gs::integers::<usize>().min_value(2).max_value(4));
    let ids: Vec<Value> = (0..n)
        .map(|_| {
            s.next += 1;
            let id = json!(s.next);
            s.write(
                json!({ "jsonrpc": "2.0", "id": id, "method": "$/test.wait", "params": { "ms": 600_000 } })
                    .to_string()
                    .as_bytes(),
            );
            id
        })
        .collect();
    loop {
        let jobs = s.call("jobs.list", json!({}));
        if jobs["result"].as_array().unwrap().len() == n {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    // The first runs; the others wait for the worker.
    let waiting = &tc.draw(gs::sampled_from(&ids[1..]));
    let v = s.call("$/cancel", json!({ "id": waiting }));
    assert_eq!(v["result"]["cancelled"], true, "{v}");
    let a = s.answer(waiting);
    assert_eq!(a["error"]["data"]["kind"], "cancelled", "{a}");
    let finished = s.events().into_iter().find(|e| e["event"] == "job.finished" && e["id"] == *waiting);
    assert_eq!(finished.map(|e| e["kind"].clone()), Some(json!("cancelled")));
    let jobs = s.call("jobs.list", json!({}));
    assert_eq!(jobs["result"].as_array().unwrap().len(), n - 1, "{jobs}");
    s.call("shutdown", json!({ "cancel": true }));
    for id in ids.iter().filter(|id| *id != waiting) {
        assert_eq!(s.answer(id)["error"]["data"]["kind"], "cancelled", "{id}");
    }
    drop(s.stdin.take());
    assert!(s.child.wait().unwrap().success());
}

/// Every method the contract has (but those that reach servers on the internet or this
/// machine's disks, or take minutes), in any order, by its name or an old one, with parameters
/// drawn from its schema (or ones no method takes): each answers once, with a result in the
/// shape its contract gives or an error the contract describes (never a panic), and parameters
/// it doesn't take are refused.
#[hegel::test(test_cases = 2, suppress_health_check = [HealthCheck::TooSlow])]
fn every_method_answers_as_its_contract_says(tc: TestCase) {
    use crate::props_rpc::{at, contract, excluded, params_for, violation};
    let fx = Env::from_template();
    let mut s = Sidecar::start(&fx);
    let methods: Vec<&'static Value> =
        contract()["methods"].as_array().unwrap().iter().filter(|m| !excluded(m["name"].as_str().unwrap())).collect();
    // By their names, so a failing order prints as names.
    let names: Vec<&str> = methods.iter().map(|m| m["name"].as_str().unwrap()).collect();
    for i in tc.draw(gs::permutations((0..names.len()).collect::<Vec<_>>())) {
        let m = methods[i];
        let aliases: Vec<&str> = m["aliases"].as_array().unwrap().iter().filter_map(Value::as_str).collect();
        let name = match aliases.is_empty() || tc.draw(gs::booleans()) {
            true => m["name"].as_str().unwrap(),
            false => tc.draw(gs::sampled_from(&aliases[..])),
        };
        // Up to three tries, for parameters the method takes (drawn ones may not quite fit).
        for _ in 0..3 {
            let (params, fits) = params_for(&tc, &fx, m);
            let a = s.call(name, params.clone());
            let said = format!("{name} {params}: {a}");
            match (a.get("result"), a.get("error")) {
                (Some(r), None) => {
                    assert!(fits || m["kind"] == "control", "parameters no method takes, accepted: {said}");
                    if let Some(v) = violation(r, &format!("{}/result", at(m))) {
                        panic!("{v}\n{said}");
                    }
                }
                (None, Some(e)) => {
                    if let Some(v) = violation(&e["data"], "/error") {
                        panic!("{v}\n{said}");
                    }
                    assert_ne!(e["data"]["kind"], "internal", "a request panicked: {said}");
                    if !fits && m["kind"] != "control" {
                        assert_eq!(e["data"]["kind"], "invalid_params", "{said}");
                    }
                }
                _ => panic!("a result or an error: {said}"),
            }
            if a["error"]["data"]["kind"] != "invalid_params" {
                break;
            }
        }
    }
    let v = s.call("shutdown", json!({ "cancel": true }));
    assert_eq!(v["result"], Value::Null, "{v}");
    drop(s.stdin.take());
    assert!(s.child.wait().unwrap().success());
}

/// The account and publishing over the sidecar, signed in to a pretend data server (or not):
/// the account's status, a check with its server, and its client document answer in their
/// contract's shapes; each record planned is applied by its plan's id (or straight from the
/// request) and is then on the server as planned, once, and the settings published import back
/// either way too; signing in to no account is refused, and a sign-in that was never begun is
/// nothing to finish or cancel; signed out, it's gone.
#[cfg(feature = "account")]
#[hegel::test(test_cases = 4, suppress_health_check = [HealthCheck::TooSlow])]
fn the_account_and_publishing_answer_over_the_sidecar(tc: TestCase) {
    use crate::mock_pds::MockPds;
    use crate::props_rpc::{at, contract, violation};
    use crate::support::account::{DID, PUBLISHING, sign_in};
    let pds = MockPds::start();
    pds.set_account(DID, "dj.example.com");
    let fx = Env::from_template();
    let signed_in = tc.draw(gs::booleans());
    if signed_in {
        sign_in(&fx.store, &pds.url, &PUBLISHING, json!({}));
    }
    fx.ok(&["--yes", "settings", "set", "quantize=off"]);
    fx.ok(&["--yes", "sets", "new", "--from", "HISTORY 001", "--title", "Friday", "--date", "2026-10-03"]);
    let mut s = Sidecar::start(&fx);
    let shaped = |name: &str, v: &Value| {
        let m = contract()["methods"].as_array().unwrap().iter().find(|m| m["name"] == name).unwrap();
        match v.get("result") {
            Some(r) => assert_eq!(violation(r, &format!("{}/result", at(m))), None, "{name}: {v}"),
            None => assert_eq!(violation(&v["error"]["data"], "/error"), None, "{name}: {v}"),
        }
    };
    let status = s.call("account.status", json!({}));
    shaped("account.status", &status);
    assert_eq!(status["result"]["account"].is_object(), signed_in, "{status}");
    let check = s.call("account.check", json!({}));
    shaped("account.check", &check);
    assert_eq!(check["result"]["did"] == DID, signed_in, "{check}");
    let doc = s.call("account.client_metadata", json!({ "scopes": ["repo:app.example.thing"] }));
    shaped("account.client_metadata", &doc);
    assert!(doc["result"]["scope"].as_str().unwrap().contains("repo:app.example.thing"), "{doc}");
    let never = s.call("account.login.finish", json!({ "id": "never-begun" }));
    assert_eq!(never["error"]["data"]["kind"], "not_found", "{never}");
    let cancelled = s.call("account.login.cancel", json!({ "id": "never-begun" }));
    assert_eq!(cancelled["result"]["cancelled"], false, "{cancelled}");
    let pasted =
        s.call("account.login.paste", json!({ "id": "never-begun", "url": "http://127.0.0.1/callback?code=x" }));
    assert!(pasted["error"].is_object(), "{pasted}");
    let begun = s.call("account.login.begin", json!({}));
    assert!(begun["error"].is_object(), "no account to sign in to: {begun}");

    let kinds = [
        ("settings.publish", json!({}), "computer.localnetwork.locallibrary.settings"),
        ("profile.publish", json!({ "drives": ["A"], "no_analysis": true }), "computer.localnetwork.dj.profile"),
        ("sets.publish", json!({ "set": "Friday" }), "computer.localnetwork.dj.set"),
    ];
    for i in tc.draw(gs::permutations(vec![0, 1, 2])) {
        let (method, params, collection) = &kinds[i];
        let plan = s.call(&format!("{method}.plan"), params.clone());
        shaped(&format!("{method}.plan"), &plan);
        let p = &plan["result"];
        if !signed_in {
            // Planned as it would be published, it has nowhere to go.
            assert!(p["account"].is_null() || plan["error"].is_object(), "{plan}");
            let refused = s.call(&format!("{method}.apply"), params.clone());
            assert!(refused["error"].is_object(), "{refused}");
            continue;
        }
        let by_id = tc.draw(gs::booleans());
        let request = if by_id { json!({ "plan_id": p["plan_id"] }) } else { params.clone() };
        let applied = s.call(&format!("{method}.apply"), request);
        shaped(&format!("{method}.apply"), &applied);
        assert!(applied["error"].is_null(), "{applied}");
        let uri = p["uri"].as_str().unwrap();
        let rkey = uri.rsplit('/').next().unwrap();
        let (record, _) = pds.record(DID, collection, rkey).unwrap_or_else(|| panic!("{uri} isn't there"));
        for (k, planned) in p["record"].as_object().unwrap() {
            if !k.ends_with("At") {
                assert_eq!(&record[k], planned, "{method} {k}");
            }
        }
        if by_id {
            let again = s.call(&format!("{method}.apply"), json!({ "plan_id": p["plan_id"] }));
            assert_eq!(again["error"]["data"]["kind"], "stale_plan", "{again}");
        }
    }
    // What's published comes back from the account.
    let import = s.call("settings.import_published.plan", json!({ "pds": pds.url, "from": DID }));
    shaped("settings.import_published.plan", &import);
    if signed_in {
        let request = match tc.draw(gs::booleans()) {
            true => json!({ "plan_id": import["result"]["plan_id"] }),
            false => json!({ "pds": pds.url, "from": DID }),
        };
        let applied = s.call("settings.import_published.apply", request);
        shaped("settings.import_published.apply", &applied);
        assert!(applied["error"].is_null(), "{applied}");
    }
    let out = s.call("account.logout", json!({}));
    shaped("account.logout", &out);
    assert!(s.call("account.status", json!({}))["result"]["account"].is_null());
    drop(s.stdin.take());
    assert!(s.child.wait().unwrap().success());
}
