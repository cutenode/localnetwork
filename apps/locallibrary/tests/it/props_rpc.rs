//! Property tests of the `locallibrary rpc` sidecar against the contract it publishes
//! (docs/rpc/schema.json): every line it's sent, garbage included, gets the answer the protocol
//! promises, in the shapes the contract gives; a read asked twice answers the same; and changes
//! to the store made through it agree with a model.

use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::Stdio;
use std::sync::OnceLock;

use hegel::TestCase;
use hegel::generators as gs;
use jsonschema::Registry;
use serde_json::{Map, Value, json};

use crate::schema;
use crate::support::{Env, Rpc, path};

/// The contract the sidecar publishes.
pub(crate) fn contract() -> &'static Value {
    static CONTRACT: OnceLock<Value> = OnceLock::new();
    CONTRACT.get_or_init(|| {
        let file = Path::new(env!("CARGO_MANIFEST_DIR")).join("docs/rpc/schema.json");
        serde_json::from_slice(&std::fs::read(file).unwrap()).unwrap()
    })
}

/// A method of the contract by its name or one of its old names.
fn method(name: &str) -> Option<&'static Value> {
    contract()["methods"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["name"] == name || m["aliases"].as_array().is_some_and(|a| a.iter().any(|n| n == name)))
}

/// The name the contract goes by among the validators' documents.
const CONTRACT: &str = "urn:locallibrary:rpc";

/// The contract, as the validators read its `$ref`s.
fn registry() -> &'static Registry<'static> {
    static REGISTRY: OnceLock<Registry<'static>> = OnceLock::new();
    REGISTRY.get_or_init(|| Registry::new().add(CONTRACT, contract()).unwrap().prepare().unwrap())
}

/// Where `v` breaks what the contract gives at `pointer` (`/error`, `/methods/3/result`), or
/// `None` where it doesn't.
pub(crate) fn violation(v: &Value, pointer: &str) -> Option<String> {
    let schema = json!({ "$ref": format!("{CONTRACT}#{pointer}") });
    schema::violations(&schema::options().with_registry(registry()).build(&schema).unwrap(), v)
}

/// Where the contract has method `m`.
pub(crate) fn at(m: &Value) -> String {
    let i = contract()["methods"].as_array().unwrap().iter().position(|o| std::ptr::eq(o, m)).unwrap();
    format!("/methods/{i}")
}

#[test]
fn the_schema_checker_tells_right_from_wrong() {
    let data = json!({ "kind": "not_found", "reason": null, "fields": {}, "actions": [] });
    assert_eq!(violation(&data, "/error"), None);
    assert!(violation(&json!({ "kind": 7 }), "/error").is_some());
    assert!(violation(&json!({ "fields": {} }), "/error").is_some(), "kind is required");
    let page = format!("{}/result", at(method("tracks.query").unwrap()));
    assert!(violation(&json!({ "total": -1, "total_duration_secs": 0, "tracks": [] }), &page).is_some());
    assert_eq!(violation(&json!({ "total": 0, "total_duration_secs": 0, "tracks": [] }), &page), None);
}

/// Methods left out of random calls: they reach servers on the internet (the account,
/// publishing, firmware, the providers locallibrary ships), touch this machine's disks
/// (formatting, ejecting, fsck), take minutes (mix identification, the analysis benchmark), are
/// only for tests, or end the session.
pub(crate) fn excluded(m: &str) -> bool {
    let prefixes = ["account.", "firmware.", "format.", "provision.", "prep.", "drive.eject", "mix.", "$/test."];
    prefixes.iter().any(|p| m.starts_with(p))
        || m.contains("publish")
        || ["drive.fsck", "drive.repair", "disks.list", "shutdown", "analysis.bench"].contains(&m)
        || m == "plugins.provide"
}

/// A string for a parameter, by its name: what such a parameter names in the fixture, or
/// doesn't.
#[hegel::test_helper]
fn string_for(tc: &TestCase, fx: &Env, key: &str, format: Option<&str>) -> String {
    let a = path(&fx.volumes.join("A")).to_string();
    let club = path(&fx.volumes.join("Club")).to_string();
    let outside = |name: &str| path(&fx.home(name)).to_string();
    let pick = |options: Vec<String>| tc.draw(gs::sampled_from(options));
    match format {
        Some("date") => return pick(vec!["2026-10-03".into(), "2099-01-01".into(), "not a day".into()]),
        Some("date-time") => return pick(vec!["2026-10-03T21:00:00Z".into(), "yesterday".into()]),
        Some("uuid") => return pick(vec![uuid::Uuid::nil().to_string(), "not-a-uuid".into()]),
        _ => {}
    }
    let s = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<String>>();
    match key {
        "drive" | "source" | "target" | "left" | "right" | "from" | "drives" => {
            pick(vec![a.clone(), club, "A".into(), "Main".into(), "/definitely/not/here".into(), String::new()])
        }
        "library" => pick(s(&["Main", "main", "Nope", ""])),
        "playlist" | "playlists" | "only" | "parent" | "folder" | "list" => {
            pick(s(&["Set", "HISTORY 001", "Nope", "Sets/Friday", ""]))
        }
        "filter" | "query" | "search" => pick(s(&["bpm:>=0", "key:8A~", "demo", "bpm:", "rating:>=4 -x", "a|b"])),
        "sort" => pick(s(&["bpm", "-rating", "key,bpm", "nonsense"])),
        "plan_id" => pick(s(&["", "0", "no-such-plan"])),
        "method" => pick(s(&["tracks.query", "drive.adopt", "nope"])),
        "path" | "output" | "file" | "to" | "db" | "image" | "dir" | "files" | "look_in" => {
            pick(vec![outside("out.txt"), outside("missing.mp3"), a, "relative.txt".into(), String::new()])
        }
        "name" | "title" | "label" | "message" | "headline" | "venue" | "city" => {
            pick(s(&["Friday", "Ünïcödé 日本", "", " padded ", "Main"]))
        }
        "set" | "snapshot" | "id" | "session" | "backup" => pick(s(&["last", "0000", "deadbeef", "HISTORY 001", ""])),
        "kind" | "kinds" => pick(s(&["club", "drive.attached", "job.finished", "nope"])),
        "models" | "players" | "model" => pick(s(&["CDJ-3000", "cdj-2000nxs2", "XDJ-AZ", "Nope"])),
        "keys" | "key" => pick(s(&["quantize", "jog_mode", "nope"])),
        _ => pick(s(&["", "x", "Main", "1"])),
    }
}

/// Parameters `schema` allows (or, now and then, ones it doesn't quite), their strings what
/// [`string_for`] gives.
fn value_for(tc: &TestCase, fx: &Env, schema: &Value) -> Value {
    let strings = |tc: &TestCase, key: &str, s: &Map<String, Value>| {
        string_for(tc, fx, key, s.get("format").and_then(Value::as_str))
    };
    schema::draw(tc, contract(), schema, "", 0, &strings)
}

/// Parameters for `m`: drawn from its schema mostly, else something no method takes.
#[hegel::test_helper]
pub(crate) fn params_for(tc: &TestCase, fx: &Env, m: &Value) -> (Value, bool) {
    if tc.draw(gs::weighted_booleans(0.85)) {
        return (value_for(tc, fx, &m["params"]), true);
    }
    let wrong = [json!(7), json!("drive"), json!(true)];
    (tc.draw(gs::sampled_from(&wrong[..])), false)
}

/// The sidecar's output for `lines`, sent at once before its input closes, and whether it
/// exited cleanly.
pub(crate) fn run_batch(fx: &Env, lines: &[Vec<u8>]) -> (Vec<Value>, bool) {
    let mut child =
        fx.command().arg("rpc").stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let input: Vec<u8> = lines.iter().flat_map(|l| l.iter().copied().chain([b'\n'])).collect();
    // Written on its own thread: the answers can fill the pipe before every request is in.
    let writer = std::thread::spawn(move || stdin.write_all(&input));
    let out: Vec<Value> = BufReader::new(child.stdout.take().unwrap())
        .lines()
        .map(|l| {
            let l = l.unwrap();
            serde_json::from_str(&l).unwrap_or_else(|e| panic!("not JSON ({e}): {l}"))
        })
        .collect();
    writer.join().unwrap().unwrap();
    (out, child.wait().unwrap().success())
}

/// What a line sent is, and what the sidecar owes it.
#[derive(Debug)]
enum Sent {
    /// Not a request at all: a parse error or an invalid request, with no id.
    Unanswerable,
    /// A request with this id, for this method (`None`: one it doesn't have), with parameters
    /// that should be refused.
    Request { id: Value, method: Option<&'static Value>, refused: bool },
    /// With an id but no method.
    NoMethod { id: Value },
    /// A notification: never answered.
    Notification,
}

/// Every line the sidecar is sent, garbage included, is answered as JSON-RPC 2.0 says: a
/// request once, by its id, with a result in the shape the contract gives its method or an
/// error with a stable kind (never an internal one: a panic); a line that isn't a request,
/// with an error and no id; a notification never. Progress is about a request it was sent, and
/// it exits cleanly once its input closes.
#[hegel::test(test_cases = 8)]
fn every_line_gets_the_answer_the_contract_promises(tc: TestCase) {
    let fx = Env::from_template();
    let methods: Vec<&'static Value> =
        contract()["methods"].as_array().unwrap().iter().filter(|m| !excluded(m["name"].as_str().unwrap())).collect();
    let n = tc.draw(gs::integers::<usize>().min_value(1).max_value(24));
    let mut lines = Vec::new();
    let mut sent = Vec::new();
    for i in 0..n {
        let id = match tc.draw(gs::booleans()) {
            true => json!(i),
            false => json!(format!("r{i}")),
        };
        let kind = tc.draw(gs::integers::<u8>().max_value(9));
        let (line, what) = match kind {
            0 => {
                let junk = tc.draw(gs::sampled_from(
                    &[&b"{"[..], b"[]", b"null", b"42", b"\"x\"", b"\xff\xfe", b"{\"jsonrpc\":\"2.0\",\"id\":"][..],
                ));
                (junk.to_vec(), Sent::Unanswerable)
            }
            1 => {
                let bad_id = tc.draw(gs::sampled_from(&[json!({}), json!([1]), json!(true)][..]));
                (
                    json!({"jsonrpc": "2.0", "id": bad_id, "method": "version"}).to_string().into_bytes(),
                    Sent::Unanswerable,
                )
            }
            2 => {
                let m = tc.draw(gs::sampled_from(&[Value::Null, json!(3), json!(["version"])][..]));
                let line = json!({"jsonrpc": "2.0", "id": id, "method": m}).to_string().into_bytes();
                (line, Sent::NoMethod { id })
            }
            3 => {
                let line = json!({"jsonrpc": "2.0", "method": "version"}).to_string().into_bytes();
                (line, Sent::Notification)
            }
            4 => {
                let name = tc.draw(gs::sampled_from(&["nope", "tracks", "drive.info.plan", ""][..]));
                let line = json!({"jsonrpc": "2.0", "id": id, "method": name}).to_string().into_bytes();
                (line, Sent::Request { id, method: None, refused: true })
            }
            _ => {
                let m = tc.draw(gs::sampled_from(&methods[..]));
                let aliases: Vec<&str> = m["aliases"].as_array().unwrap().iter().filter_map(Value::as_str).collect();
                let name = match aliases.is_empty() || tc.draw(gs::booleans()) {
                    true => m["name"].as_str().unwrap(),
                    false => tc.draw(gs::sampled_from(&aliases[..])),
                };
                let (params, fits) = params_for(&tc, &fx, m);
                let line = json!({"jsonrpc": "2.0", "id": id, "method": name, "params": params});
                // Control methods without parameters don't read them.
                let refused = !fits && m["kind"] != "control";
                (line.to_string().into_bytes(), Sent::Request { id, method: Some(m), refused })
            }
        };
        lines.push(line);
        sent.push(what);
    }
    let (out, clean) = run_batch(&fx, &lines);
    assert!(clean, "the sidecar didn't exit cleanly: {out:?}");

    let (answers, notes): (Vec<&Value>, Vec<&Value>) = out.iter().partition(|m| m.get("method").is_none());
    let ids: Vec<&Value> = sent
        .iter()
        .filter_map(|s| match s {
            Sent::Request { id, .. } | Sent::NoMethod { id } => Some(id),
            _ => None,
        })
        .collect();
    for a in &answers {
        assert_eq!(a["jsonrpc"], "2.0", "{a}");
        assert!(a.get("result").is_some() != a.get("error").is_some(), "a result or an error: {a}");
        if let Some(e) = a.get("error") {
            assert!(e["code"].is_i64() && e["message"].as_str().is_some_and(|m| !m.is_empty()), "{a}");
            if let Some(v) = violation(&e["data"], "/error") {
                panic!("{v}\n{a}");
            }
            assert_ne!(e["data"]["kind"], "internal", "a request panicked: {a}");
        }
    }
    for s in &sent {
        match s {
            Sent::Request { id, method, refused } => {
                let got: Vec<&&Value> = answers.iter().filter(|a| a["id"] == *id).collect();
                assert_eq!(got.len(), 1, "{id} answered {} times: {out:?}", got.len());
                let a = got[0];
                match (method, a.get("result")) {
                    (None, _) => assert_eq!(a["error"]["code"], -32601, "{a}"),
                    (Some(_), Some(_)) if *refused => panic!("parameters no method takes, accepted: {a}"),
                    (Some(m), Some(r)) => {
                        if let Some(v) = violation(r, &format!("{}/result", at(m))) {
                            panic!("{}.result: {v}\n{a}", m["name"]);
                        }
                    }
                    (Some(_), None) if *refused => assert_eq!(a["error"]["data"]["kind"], "invalid_params", "{a}"),
                    (Some(_), None) => {}
                }
            }
            Sent::NoMethod { id } => {
                let a = answers.iter().find(|a| a["id"] == *id).unwrap_or_else(|| panic!("{id}: no answer"));
                assert_eq!((&a["error"]["code"], &a["error"]["message"]), (&json!(-32600), &json!("missing method")));
            }
            Sent::Unanswerable | Sent::Notification => {}
        }
    }
    let unanswerable = sent.iter().filter(|s| matches!(s, Sent::Unanswerable)).count();
    let anonymous: Vec<&&Value> = answers.iter().filter(|a| a["id"].is_null()).collect();
    assert_eq!(anonymous.len(), unanswerable, "errors without an id: {anonymous:?}");
    assert!(anonymous.iter().all(|a| [-32700, -32600].contains(&a["error"]["code"].as_i64().unwrap())));
    assert_eq!(answers.len(), ids.len() + unanswerable, "{out:?}");
    for note in notes {
        let params = &note["params"];
        let name = note["method"].as_str().unwrap();
        assert!(contract()["notifications"][name].is_object(), "an unknown notification: {note}");
        if let Some(v) = violation(params, &format!("/notifications/{name}")) {
            panic!("{v}\n{note}");
        }
        if note["method"] == "progress" {
            assert!(ids.contains(&&params["id"]), "progress about a request never sent: {note}");
        }
    }
}

/// Reads that change nothing, whose answer depends only on what's on the drives and in the
/// store.
const STEADY_READS: &[&str] = &[
    "drive.library",
    "playlists.tree",
    "tracks.get",
    "tracks.query",
    "track.analysis",
    "tracks.analysis",
    "track.fields",
    "track.show",
    "track.matches",
    "tags.list",
    "cues.list",
    "plays.history",
    "plays.stats",
    "undo.list",
    "drive.backups",
    "edits.list",
    "art.missing",
    "art.get",
    "settings.schema",
    "settings.show",
    "players.models",
    "players.show",
    "library.list",
    "store.history",
    "smart.list",
    "sets.list",
    "sets.get",
    "profile.prefs",
    "rpc.describe",
    "diff",
    "drive.log",
    "drives.known",
    "drive.groups",
    "version",
];

/// The same read, asked twice of the same drives and store (at once, on different workers),
/// gets the same answer, and changes no file.
#[hegel::test(test_cases = 6)]
fn the_same_read_gets_the_same_answer(tc: TestCase) {
    let fx = Env::from_template();
    let before = fx.files();
    let n = tc.draw(gs::integers::<usize>().min_value(1).max_value(6));
    let mut lines = Vec::new();
    let mut asked = Vec::new();
    for i in 0..n {
        let name = tc.draw(gs::sampled_from(STEADY_READS));
        let m = method(name).unwrap();
        let params = value_for(&tc, &fx, &m["params"]);
        for copy in 0..2 {
            let req = json!({"jsonrpc": "2.0", "id": i * 2 + copy, "method": name, "params": params});
            lines.push(req.to_string().into_bytes());
        }
        asked.push((name, params));
    }
    let (out, clean) = run_batch(&fx, &lines);
    assert!(clean);
    for (i, (name, params)) in asked.iter().enumerate() {
        let answer = |id: usize| {
            let a = out.iter().find(|a| a["id"] == id && a.get("method").is_none()).expect("an answer");
            let mut a = a.clone();
            a.as_object_mut().unwrap().remove("id");
            a
        };
        assert_eq!(answer(i * 2), answer(i * 2 + 1), "{name} {params}");
    }
    assert!(fx.files() == before, "a read changed a file");
}

/// What the model expects of the store's rule-based playlists, My Settings and players.
#[derive(Default)]
struct StoreModel {
    smart: Vec<(String, String)>,
    settings: Vec<(String, String)>,
    players: Vec<String>,
    library: String,
}

/// The store's rule-based playlists, My Settings, default players and library name, changed
/// through the sidecar in any order (and plans applied by id, once), agree with a model after
/// every step.
#[hegel::test(test_cases = 6)]
fn store_edits_agree_with_a_model(tc: TestCase) {
    let fx = Env::from_template();
    let mut rpc = Rpc::start(fx.command());
    let mut model = StoreModel { library: "A".into(), ..Default::default() };
    let steps = tc.draw(gs::integers::<usize>().min_value(1).max_value(10));
    for step in 0..steps as i64 {
        let id = step * 10;
        match tc.draw(gs::integers::<u8>().max_value(6)) {
            0 => {
                let path = tc.draw(gs::sampled_from(&["Smart/Deep", "Smart/Peak time", "Top"][..]));
                let filter = tc.draw(gs::sampled_from(&["bpm:>=120", "key:8A~", "rating:>=4"][..]));
                let r = rpc.call(id, "smart.set", json!({"path": path, "filter": filter}));
                assert!(r.get("result").is_some(), "{r}");
                match model.smart.iter_mut().find(|(p, _)| p == path) {
                    Some(rule) => rule.1 = filter.into(),
                    None => model.smart.push((path.into(), filter.into())),
                }
            }
            1 => {
                let path = tc.draw(gs::sampled_from(&["Smart/Deep", "Top", "Never made"][..]));
                let r = rpc.call(id, "smart.remove", json!({"path": path}));
                let had = model.smart.iter().any(|(p, _)| p == path);
                assert_eq!(r.get("result").is_some(), had, "{r}");
                if !had {
                    assert_eq!(r["error"]["data"]["kind"], "not_found", "{r}");
                }
                model.smart.retain(|(p, _)| p != path);
            }
            2 => {
                let (key, value) = tc.draw(gs::sampled_from(
                    &[("quantize", "on"), ("quantize", "off"), ("jog_mode", "cdj"), ("jog_mode", "vinyl")][..],
                ));
                let r = rpc.call(id, "settings.set", json!({"values": {key: value}}));
                assert_eq!(r["result"]["base"][key], value, "{r}");
                model.settings.retain(|(k, _)| k != key);
                model.settings.push((key.into(), value.into()));
            }
            3 => {
                let key = tc.draw(gs::sampled_from(&["quantize", "jog_mode"][..]));
                let r = rpc.call(id, "settings.unset", json!({"keys": [key]}));
                assert!(r["result"]["base"].get(key).is_none(), "{r}");
                model.settings.retain(|(k, _)| k != key);
            }
            4 => {
                let models: Vec<&str> =
                    tc.draw(gs::vecs(gs::sampled_from(&["CDJ-3000", "XDJ-AZ", "CDJ-2000NXS2"][..])).max_size(2));
                let by_plan = tc.draw(gs::booleans());
                let r = if by_plan && models.is_empty() {
                    rpc.call(id, "players.set.plan", json!({"models": models}))
                } else if by_plan {
                    let plan = rpc.call(id, "players.set.plan", json!({"models": models}));
                    let plan_id = plan["result"]["plan_id"].as_str().unwrap_or_else(|| panic!("{plan}")).to_string();
                    let r = rpc.call(id + 1, "players.set.apply", json!({"plan_id": plan_id}));
                    let again = rpc.call(id + 2, "players.set.apply", json!({"plan_id": plan_id}));
                    assert_eq!(again["error"]["data"]["kind"], "stale_plan", "a plan applies once: {again}");
                    r
                } else {
                    rpc.call(id, "players.set.apply", json!({"models": models}))
                };
                if models.is_empty() {
                    assert!(r.get("error").is_some(), "no models: {r}");
                } else {
                    assert!(r.get("result").is_some(), "{r}");
                    // Each model once, in the order first given.
                    model.players.clear();
                    for m in &models {
                        if !model.players.iter().any(|p| p == m) {
                            model.players.push(m.to_string());
                        }
                    }
                }
            }
            5 => {
                let r = rpc.call(id, "players.clear.apply", json!({}));
                assert!(r.get("result").is_some(), "{r}");
                model.players.clear();
            }
            _ => {
                let name = tc.draw(gs::sampled_from(&["A", "Club", "Home"][..]));
                let r = rpc.call(id, "library.rename", json!({"library": model.library, "name": name}));
                assert_eq!(r["result"]["name"], name, "{r}");
                model.library = name.into();
            }
        }
        let smart = rpc.call(id + 5, "smart.list", Value::Null);
        let mut got: Vec<(String, String)> = smart["result"]["playlists"]
            .as_array()
            .unwrap_or_else(|| panic!("{smart}"))
            .iter()
            .map(|p| (p["path"].as_str().unwrap().into(), p["filter"].as_str().unwrap().into()))
            .collect();
        let mut want = model.smart.clone();
        got.sort();
        want.sort();
        assert_eq!(got, want);
        let profile = rpc.call(id + 6, "settings.profile", Value::Null);
        let base = profile["result"]["base"].as_object().unwrap_or_else(|| panic!("{profile}"));
        let mut got: Vec<(String, String)> =
            base.iter().map(|(k, v)| (k.clone(), v.as_str().unwrap().into())).collect();
        let mut want = model.settings.clone();
        got.sort();
        want.sort();
        assert_eq!(got, want);
        let players = rpc.call(id + 7, "players.show", Value::Null);
        assert_eq!(players["result"]["default"], json!(model.players), "{players}");
        let libraries = rpc.call(id + 8, "library.list", Value::Null);
        let names: Vec<&Value> = libraries["result"].as_array().unwrap().iter().map(|l| &l["name"]).collect();
        assert_eq!(names, [&json!(model.library)], "{libraries}");
    }
}

/// A sidecar with its test-only `$/test.wait` (a request that runs until cancelled), read on
/// its own thread so a message can be waited for with a timeout, and messages read while
/// waiting for another kept.
struct Session {
    child: std::process::Child,
    stdin: Option<std::process::ChildStdin>,
    messages: std::sync::mpsc::Receiver<Value>,
    kept: Vec<Value>,
}

/// A session a failed test case left open is ended, so its sidecar doesn't outlive the test.
impl Drop for Session {
    fn drop(&mut self) {
        drop(self.stdin.take());
        crate::support::end(&mut self.child, std::time::Duration::from_secs(10));
    }
}

impl Session {
    fn start(fx: &Env) -> Session {
        let mut child = fx
            .command()
            .arg("rpc")
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
        Session { child, stdin, messages, kept: Vec::new() }
    }

    fn send(&mut self, id: &Value, method: &str, params: Value) {
        let msg = json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params });
        writeln!(self.stdin.as_mut().unwrap(), "{msg}").unwrap();
    }

    /// The response to `id`.
    fn answer(&mut self, id: &Value) -> Value {
        if let Some(i) = self.kept.iter().position(|m| m.get("method").is_none() && m["id"] == *id) {
            return self.kept.remove(i);
        }
        loop {
            let m = self.messages.recv_timeout(std::time::Duration::from_secs(60)).expect("an answer");
            if m.get("method").is_none() && m["id"] == *id {
                return m;
            }
            self.kept.push(m);
        }
    }

    fn call(&mut self, id: &Value, method: &str, params: Value) -> Value {
        self.send(id, method, params);
        self.answer(id)
    }
}

/// Requests in flight (`$/test.wait`), cancelled, refused for reusing a running request's id,
/// and listed, agree with a model of what's running: `jobs.list` lists exactly those, `$/cancel`
/// says whether there was one to cancel and that one answers `cancelled`, and `shutdown` with
/// `cancel` answers every one left before the sidecar exits cleanly.
#[hegel::test(test_cases = 6)]
fn requests_in_flight_agree_with_a_model(tc: TestCase) {
    let fx = Env::from_template();
    let mut s = Session::start(&fx);
    let mut running: Vec<Value> = Vec::new();
    let mut started: Vec<Value> = Vec::new();
    let mut next = 0;
    let mut fresh = |tc: &TestCase| {
        next += 1;
        match tc.draw(gs::booleans()) {
            true => json!(next),
            false => json!(format!("r{next}")),
        }
    };
    for _ in 0..tc.draw(gs::integers::<usize>().min_value(1).max_value(12)) {
        match tc.draw(gs::integers::<u8>().max_value(4)) {
            0 | 1 => {
                let id = fresh(&tc);
                let items = tc.draw(gs::integers::<u64>().max_value(3));
                s.send(&id, "$/test.wait", json!({ "ms": 600_000, "items": items }));
                running.push(id.clone());
                started.push(id);
            }
            2 => {
                let id = match running.is_empty() || tc.draw(gs::weighted_booleans(0.2)) {
                    true => fresh(&tc),
                    false => tc.draw(gs::sampled_from(&running[..])),
                };
                let cancel_id = fresh(&tc);
                let r = s.call(&cancel_id, "$/cancel", json!({ "id": id }));
                let was = running.contains(&id);
                assert_eq!(r["result"]["cancelled"], was, "{id}: {r}");
                if was {
                    let a = s.answer(&id);
                    assert_eq!(a["error"]["data"]["kind"], "cancelled", "{a}");
                    running.retain(|r| *r != id);
                }
            }
            3 if !running.is_empty() => {
                let id = tc.draw(gs::sampled_from(&running[..]));
                let r = s.call(&id, "version", json!({}));
                assert_eq!(r["error"]["code"], -32600, "a running request's id, reused: {r}");
            }
            _ => {
                let id = fresh(&tc);
                let r = s.call(&id, "jobs.list", json!({}));
                let mut listed: Vec<String> =
                    r["result"].as_array().unwrap().iter().map(|j| j["id"].to_string()).collect();
                let mut want: Vec<String> = running.iter().map(Value::to_string).collect();
                listed.sort();
                want.sort();
                assert_eq!(listed, want, "{r}");
                for j in r["result"].as_array().unwrap() {
                    assert_eq!(j["method"], "$/test.wait", "{j}");
                }
            }
        }
    }
    let id = fresh(&tc);
    let r = s.call(&id, "shutdown", json!({ "cancel": true }));
    assert_eq!(r["result"], Value::Null, "{r}");
    for id in &running {
        assert_eq!(s.answer(id)["error"]["data"]["kind"], "cancelled", "{id}");
    }
    drop(s.stdin.take());
    assert!(s.child.wait().unwrap().success());
    for m in s.kept.iter().chain(s.messages.try_iter().collect::<Vec<_>>().iter()) {
        if m["method"] == "progress" {
            assert!(started.contains(&m["params"]["id"]), "progress about a request never sent: {m}");
        }
    }
}

/// Input that can't be read at all (a folder where the pipe should be) ends the sidecar with an
/// error saying so, however many workers it has, and nothing is answered.
#[cfg(unix)]
#[hegel::test(test_cases = 3)]
fn input_that_cannot_be_read_ends_the_sidecar_with_an_error(tc: TestCase) {
    let fx = Env::from_template();
    let workers = tc.draw(gs::integers::<u32>().min_value(1).max_value(4)).to_string();
    let folder = std::fs::File::open(&fx.volumes).unwrap();
    let out = fx.command().args(["rpc", "--workers", &workers]).stdin(folder).output().unwrap();
    let err = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(1), "{err}");
    assert!(out.stdout.is_empty(), "{}", String::from_utf8_lossy(&out.stdout));
    assert!(err.contains("reading requests"), "{err}");
}

/// Requests that each need the store, sent at once to a sidecar that has none yet, all find it:
/// whichever makes it, the others open the same one.
#[hegel::test(test_cases = 10)]
fn requests_making_the_store_at_once_all_find_it(tc: TestCase) {
    let fx = Env::new();
    let n = tc.draw(gs::integers::<usize>().min_value(2).max_value(6));
    let lines: Vec<Vec<u8>> = (0..n)
        .map(|id| {
            json!({ "jsonrpc": "2.0", "id": id, "method": "store.gc.plan", "params": {} }).to_string().into_bytes()
        })
        .collect();
    let (out, clean) = run_batch(&fx, &lines);
    assert!(clean, "the sidecar didn't exit cleanly");
    for a in out.iter().filter(|a| a.get("id").is_some()) {
        assert!(a["error"].is_null(), "{a}");
    }
}
