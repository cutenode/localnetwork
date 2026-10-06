//! `settings publish`, `settings import --from-handle` and the publish RPC methods, against a
//! pretend data server on 127.0.0.1. The session is written straight into the store (as files,
//! never the keychain); nothing signs in to a real account or touches the network.

#![cfg(feature = "account")]

use crate::mock_pds;

use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

use mock_pds::{MockPds, Writes};
use serde_json::{Value, json};

const DID: &str = "did:plc:testdj";
const SETTINGS: &str = "computer.localnetwork.locallibrary.settings";

/// A P-256 key made for these tests (it signs nothing but proofs sent to the pretend server).
fn test_key() -> Value {
    json!({
        "kty": "EC",
        "crv": "P-256",
        "x": "laxgzfuTILMca4EtW61jYuY5U6rrU0SriQbuzl5PbVk",
        "y": "_cGaHm7PKttheS1e6rIuy21TxDGGGqkgc5L02W1VewM",
        "d": "vSy5sRAucJv2w6BzKciV3Y57kx-EgAFnM2QTVlvdyrY",
    })
}

fn locallibrary(store: &Path) -> Command {
    crate::locallibrary(store, &[])
}

/// The exit status too: 3 is not applied (not confirmed, or declined).
fn status(store: &Path, args: &[&str]) -> (String, String, Option<i32>) {
    let out = locallibrary(store).args(args).output().unwrap();
    (String::from_utf8(out.stdout).unwrap(), String::from_utf8(out.stderr).unwrap(), out.status.code())
}

fn run(store: &Path, args: &[&str]) -> (String, String, bool) {
    let out = locallibrary(store).args(args).output().unwrap();
    (String::from_utf8(out.stdout).unwrap(), String::from_utf8(out.stderr).unwrap(), out.status.success())
}

/// Sign `DID` in, as the session files a sign-in with `--session-file` leaves, for `pds`.
fn sign_in(store: &Path, pds: &str, scopes: &[&str]) {
    let scope = scopes.join(" ");
    let session = json!({
        "account_did": DID,
        "session_id": "s1",
        "host_url": pds,
        "authserver_url": pds,
        "authserver_token_endpoint": format!("{pds}/oauth/token"),
        "authserver_revocation_endpoint": format!("{pds}/oauth/revoke"),
        "scopes": scope,
        "dpop_authserver_nonce": "",
        "dpop_host_nonce": "",
        "iss": pds,
        "sub": DID,
        "aud": pds,
        "scope": scope,
        "access_token": "access-token",
        "token_type": "DPoP",
        "expires_at": "2099-01-01T00:00:00.000Z",
    });
    let account = json!({
        "account": {
            "info": {
                "did": DID,
                "handle": "dj.example.com",
                "pds": pds,
                "auth_server": pds,
                "scopes": scopes,
                "signed_in_at": "2026-10-05T00:00:00Z",
                "sign_in_again_by": "2099-01-01T00:00:00Z",
                "access_expires_at": null,
                "client": { "kind": "loopback" },
                "storage": "file",
            },
            "session_id": "s1",
            "redirect_uri": "http://127.0.0.1:1/callback",
            "requested_scopes": scopes,
            "session": session,
        }
    });
    std::fs::write(store.join("atmosphere.json"), account.to_string()).unwrap();
    let secrets = json!({ "did": DID, "dpop_key": test_key(), "refresh_token": "refresh-1" });
    std::fs::write(store.join("atmosphere-session.json"), secrets.to_string()).unwrap();
}

const PUBLISHING: [&str; 4] = [
    "atproto",
    "repo:computer.localnetwork.locallibrary.settings",
    "repo:computer.localnetwork.locallibrary.profile",
    "repo:computer.localnetwork.locallibrary.set",
];

#[test]
fn settings_publish_and_import_from_the_command_line() {
    let pds = MockPds::start();
    let dir = tempfile::tempdir().unwrap();
    let store = dir.path().join("store");
    let (_, err, ok) = run(&store, &["--yes", "settings", "set", "quantize=off", "jog_mode=cdj"]);
    assert!(ok, "{err}");

    // Signed out, it can be looked at but not published.
    let (out, _, ok) = run(&store, &["settings", "publish", "--dry-run"]);
    assert!(ok && out.contains("as it would be published") && out.contains("Jog Mode"), "{out}");
    let (_, err, ok) = run(&store, &["--yes", "settings", "publish"]);
    assert!(!ok && err.contains("account login <handle> --publish"), "{err}");

    sign_in(&store, &pds.url, &PUBLISHING);
    // Without a terminal or --yes, it shows what it would publish and stops (exit status 3).
    let (out, err, code) = status(&store, &["settings", "publish"]);
    assert_eq!(code, Some(3), "{err}");
    assert!(out.contains("Publish your My Settings as @dj.example.com"), "{out}");
    assert!(out.contains(&format!("at://{DID}/{SETTINGS}/self · not published yet")), "{out}");
    assert!(err.contains("not confirmed"), "{err}");
    assert!(pds.calls("com.atproto.repo.putRecord").is_empty());

    let (out, err, ok) = run(&store, &["--json", "--yes", "settings", "publish"]);
    assert!(ok, "{err}");
    let v: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["report"]["written"], true);
    assert_eq!((&v["applied"], &v["outcome"]), (&json!(true), &json!("applied")));
    assert_eq!(v["plan"]["operation"], "settings.publish");
    assert_eq!(v["report"]["uri"], format!("at://{DID}/{SETTINGS}/self"));
    assert_eq!(v["plan"]["record"]["player"], json!({ "quantize": "off", "jogMode": "cdj" }));
    let (record, _) = pds.record(DID, SETTINGS, "self").unwrap();
    assert_eq!(record["player"]["quantize"], "off");
    let write = pds.calls("com.atproto.repo.putRecord").pop().unwrap();
    assert_eq!(write.header("authorization"), Some("DPoP access-token"));
    assert_eq!(write.dpop().unwrap().0["jwk"]["x"], test_key()["x"]);

    let (out, _, ok) = run(&store, &["settings", "publish", "--yes"]);
    assert!(ok && out.contains("already published, unchanged"), "{out}");

    // Another computer imports them, signed out, naming the account and its server.
    let other = dir.path().join("other");
    let from = ["settings", "import", "--from-handle", DID, "--pds", &pds.url];
    let (out, _, code) = status(&other, &[&["--json"][..], &from[..]].concat());
    let v: Value = serde_json::from_str(&out).unwrap();
    assert_eq!((code, &v["outcome"]), (Some(3), &json!("not_confirmed")), "{out}");
    assert!(v["applied"] == false && v["plan"]["changes"].as_array().unwrap().len() == 2, "{out}");
    let (out, _, code) = status(&other, &[&["--json"][..], &from[..], &["--dry-run"][..]].concat());
    assert_eq!(code, Some(0), "{out}");
    let (out, err, ok) = run(&other, &[&["--yes"][..], &from[..]].concat());
    assert!(ok && out.contains("imported 2 settings from did:plc:testdj"), "{out}{err}");
    let (out, _, _) = run(&other, &["--json", "settings", "profile"]);
    let profile: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(profile["base"], json!({ "jog_mode": "cdj", "quantize": "off" }));
    let (_, err, ok) = run(&other, &["settings", "import", "--pds", &pds.url]);
    assert!(!ok && err.contains("--from-handle or --from-account"), "{err}");

    // The server refuses: say what to do.
    pds.set_writes(Writes::RefuseScope);
    run(&store, &["--yes", "settings", "set", "quantize=on"]);
    let (_, err, ok) = run(&store, &["--yes", "settings", "publish"]);
    // The library says what's wrong; the CLI says what to type.
    assert!(!ok && err.contains("sign in again, allowing publishing"), "{err}");
    assert!(err.contains("hint: Sign in again, allowing publishing: locallibrary account login --publish"), "{err}");

    // A session signed in without permission to publish can't sign in again without a terminal.
    pds.set_writes(Writes::Accept);
    sign_in(&store, &pds.url, &["atproto"]);
    let (out, err, ok) = run(&store, &["--yes", "settings", "publish"]);
    assert!(!ok, "{out}");
    assert!(err.contains("@dj.example.com's session doesn't allow publishing yet"), "{err}");
    // The plan's warning is part of the plan, on stdout with it.
    assert!(out.contains("this session can't publish yet"), "{out}");
}

#[test]
fn an_unchanged_record_needs_no_permission_to_leave_as_it_is() {
    let pds = MockPds::start();
    let dir = tempfile::tempdir().unwrap();
    let store = dir.path().join("store");
    run(&store, &["--yes", "settings", "set", "quantize=off"]);
    sign_in(&store, &pds.url, &PUBLISHING);
    let (_, err, ok) = run(&store, &["--yes", "settings", "publish"]);
    assert!(ok, "{err}");
    // Signed in again, only to say who you are: there's nothing to publish, so nothing to allow.
    sign_in(&store, &pds.url, &["atproto"]);
    let (out, _, _) = run(&store, &["account"]);
    assert!(out.contains("signed in as the development client"), "{out}");
    let (out, err, ok) = run(&store, &["--yes", "settings", "publish"]);
    assert!(ok && out.contains("already published, unchanged"), "{out}{err}");
    // Signing in again for publishing takes the sign-in's browser options.
    for flag in ["--no-browser", "--paste", "--timeout=60"] {
        let (_, err, ok) = run(&store, &["settings", "publish", "--dry-run", flag]);
        assert!(ok, "{flag}: {err}");
    }
}

#[test]
fn rpc_publish_methods() {
    let pds = MockPds::start();
    let dir = tempfile::tempdir().unwrap();
    let store = dir.path().join("store");
    run(&store, &["--yes", "settings", "set", "quantize=off"]);
    sign_in(&store, &pds.url, &["atproto"]);
    let mut child = locallibrary(&store).arg("rpc").stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().unwrap();
    let requests = [
        r#"{"jsonrpc":"2.0","id":1,"method":"version"}"#,
        r#"{"jsonrpc":"2.0","id":2,"method":"settings.publish.plan"}"#,
        r#"{"jsonrpc":"2.0","id":3,"method":"settings.publish.apply"}"#,
        r#"{"jsonrpc":"2.0","id":4,"method":"history.publish.plan","params":{"drive":"/nowhere","list":"HISTORY 001"}}"#,
        r#"{"jsonrpc":"2.0","id":5,"method":"settings.import_published.plan"}"#,
    ];
    {
        let mut stdin = child.stdin.take().unwrap();
        for r in requests {
            writeln!(stdin, "{r}").unwrap();
        }
    }
    let out = child.wait_with_output().unwrap();
    let replies: Vec<Value> =
        String::from_utf8(out.stdout).unwrap().lines().map(|l| serde_json::from_str(l).unwrap()).collect();
    let reply = |id: i64| replies.iter().find(|r| r["id"] == id).unwrap_or_else(|| panic!("no reply {id}"));
    let methods = reply(1)["result"]["methods"].as_array().unwrap();
    for m in [
        "settings.publish.plan",
        "settings.publish.apply",
        "settings.import_published.plan",
        "settings.import_published.apply",
        "profile.publish.plan",
        "profile.publish.apply",
        "history.publish.plan",
        "history.publish.apply",
    ] {
        assert!(methods.contains(&Value::from(m)), "{m}");
    }
    let plan = &reply(2)["result"];
    assert_eq!(plan["missing_scope"], format!("repo:{SETTINGS}?action=create&action=update"));
    assert_eq!(plan["record"]["player"]["quantize"], "off");
    assert_eq!(reply(3)["error"]["data"]["kind"], "account");
    assert!(reply(4)["error"]["data"]["kind"].is_string());
    // Nothing published yet.
    assert_eq!(reply(5)["error"]["data"]["kind"], "not_found");
}

/// A sidecar to talk to one request at a time.
struct Sidecar {
    child: std::process::Child,
    stdin: std::process::ChildStdin,
    stdout: std::io::Lines<std::io::BufReader<std::process::ChildStdout>>,
}

impl Sidecar {
    fn start(store: &Path) -> Sidecar {
        use std::io::BufRead;
        let mut child = locallibrary(store).arg("rpc").stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().unwrap();
        let stdin = child.stdin.take().unwrap();
        let stdout = std::io::BufReader::new(child.stdout.take().unwrap()).lines();
        Sidecar { child, stdin, stdout }
    }

    fn call(&mut self, id: i64, method: &str, params: Value) -> Value {
        let request = json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params });
        writeln!(self.stdin, "{request}").unwrap();
        loop {
            let line = self.stdout.next().expect("a response").unwrap();
            let v: Value = serde_json::from_str(&line).unwrap();
            if v["id"] == id {
                return v;
            }
        }
    }
}

impl Drop for Sidecar {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[test]
fn a_publish_plan_is_applied_by_its_id_and_only_over_what_it_read() {
    let pds = MockPds::start();
    let dir = tempfile::tempdir().unwrap();
    let store = dir.path().join("store");
    run(&store, &["--yes", "settings", "set", "quantize=off"]);
    sign_in(&store, &pds.url, &PUBLISHING);
    let mut s = Sidecar::start(&store);

    let plan = s.call(1, "settings.publish.plan", json!({}))["result"].clone();
    assert_eq!(
        (&plan["operation"], &plan["noop"], &plan["destructive"]),
        (&json!("settings.publish"), &json!(false), &json!(false))
    );
    assert!(plan["summary"][0].as_str().unwrap().contains("publish your My Settings as @dj.example.com"), "{plan}");
    let id = plan["plan_id"].clone();
    let done = s.call(2, "settings.publish.apply", json!({ "plan_id": id }));
    assert_eq!(done["result"]["written"], true, "{done}");
    assert_eq!(pds.record(DID, SETTINGS, "self").unwrap().0["player"]["quantize"], "off");
    let again = s.call(3, "settings.publish.apply", json!({ "plan_id": id }));
    assert_eq!(again["error"]["data"]["kind"], "stale_plan", "{again}");

    // Published from another computer after the plan read it: refused, and nothing is replaced.
    run(&store, &["--yes", "settings", "set", "quantize=on"]);
    let plan = s.call(4, "settings.publish.plan", json!({}))["result"].clone();
    assert_eq!(plan["destructive"], true, "it replaces what's published: {plan}");
    let theirs = json!({ "$type": SETTINGS, "version": 1, "updatedAt": "2026-10-05T00:00:00.000Z" });
    pds.put(DID, SETTINGS, "self", theirs.clone());
    let refused = s.call(5, "settings.publish.apply", json!({ "plan_id": plan["plan_id"] }));
    assert_eq!(refused["error"]["data"]["kind"], "stale_plan", "{refused}");
    assert_eq!(pds.record(DID, SETTINGS, "self").unwrap().0, theirs);

    // The settings profile changed since: refused before anything is sent.
    let plan = s.call(6, "settings.publish.plan", json!({}))["result"].clone();
    run(&store, &["--yes", "settings", "set", "quantize=off"]);
    let writes = pds.calls("com.atproto.repo.putRecord").len();
    let refused = s.call(7, "settings.publish.apply", json!({ "plan_id": plan["plan_id"] }));
    assert_eq!(refused["error"]["data"]["kind"], "stale_plan", "{refused}");
    assert_eq!(pds.calls("com.atproto.repo.putRecord").len(), writes);

    // Importing applies what it showed, and not over a profile changed since.
    let import = s.call(8, "settings.import_published.plan", json!({}))["result"].clone();
    assert_eq!(import["operation"], "settings.import_published", "{import}");
    run(&store, &["--yes", "settings", "set", "jog_mode=cdj"]);
    let refused = s.call(9, "settings.import_published.apply", json!({ "plan_id": import["plan_id"] }));
    assert_eq!(refused["error"]["data"]["kind"], "stale_plan", "{refused}");
    // Theirs has no values: adding them changes nothing, replacing the base with them empties it.
    let import = s.call(10, "settings.import_published.plan", json!({}))["result"].clone();
    assert_eq!(import["noop"], true, "{import}");
    let import = s.call(11, "settings.import_published.plan", json!({ "replace": true }))["result"].clone();
    let done = s.call(12, "settings.import_published.apply", json!({ "plan_id": import["plan_id"] }));
    assert!(done["result"]["base"].as_object().is_some_and(|b| b.is_empty()), "theirs has no values: {done}");
}
