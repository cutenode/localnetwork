//! `locallibrary account` and the `account.*` RPC methods, without signing in (that needs a
//! browser and a real account).

#![cfg(feature = "account")]

use std::io::Write;
use std::process::{Command, Stdio};

use serde_json::Value;

fn locallibrary(store: &std::path::Path) -> Command {
    crate::locallibrary(store, &[])
}

fn run(store: &std::path::Path, args: &[&str]) -> (String, String, bool) {
    let out = locallibrary(store).args(args).output().unwrap();
    (String::from_utf8(out.stdout).unwrap(), String::from_utf8(out.stderr).unwrap(), out.status.success())
}

#[test]
fn signed_out_status_and_sign_out() {
    let dir = tempfile::tempdir().unwrap();
    let store = dir.path().join("store");
    let (out, _, ok) = run(&store, &["account"]);
    assert!(ok && out.contains("Not signed in to an Atmosphere account"), "{out}");
    let (out, _, ok) = run(&store, &["--json", "account", "status"]);
    let v: Value = serde_json::from_str(&out).unwrap();
    assert!(ok && v["account"].is_null() && v["last_handle"].is_null());
    let (out, _, ok) = run(&store, &["account", "logout"]);
    assert!(ok && out.contains("Not signed in"), "{out}");
    let (_, err, ok) = run(&store, &["account", "status", "--check"]);
    assert!(!ok && err.contains("not signed in to an Atmosphere account"), "{err}");
    assert!(!store.exists(), "nothing is written while signed out");

    // No handle, nothing remembered, and no terminal to ask in.
    let (out, err, ok) = run(&store, &["--json", "account", "login"]);
    assert!(!ok && (out + &err).contains("which Atmosphere account?"));
}

#[test]
fn client_metadata_for_a_hosted_client() {
    let dir = tempfile::tempdir().unwrap();
    let store = dir.path().join("store");
    let id = "https://locallibrary.example/oauth-client-metadata.json";
    let (out, _, ok) =
        run(&store, &["account", "client-metadata", "--client-id", id, "--scope", "repo:app.example.thing"]);
    assert!(ok, "{out}");
    let v: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["client_id"], id);
    assert_eq!(v["application_type"], "native");
    // Every document allows publishing locallibrary's records, so sign-in can ask for it.
    assert_eq!(
        v["scope"],
        "atproto repo:app.example.thing \
         repo:computer.localnetwork.locallibrary.profile?action=create&action=update \
         repo:computer.localnetwork.locallibrary.set?action=create&action=update \
         repo:computer.localnetwork.locallibrary.settings?action=create&action=update"
    );
    let file = dir.path().join("meta.json");
    let (_, _, ok) = run(&store, &["account", "client-metadata", "--client-id", id, "-o", file.to_str().unwrap()]);
    assert!(ok && file.exists());
    let write = |more: &[&str]| {
        run(&store, &[&["account", "client-metadata", "--client-id", id, "-o", file.to_str().unwrap()], more].concat())
    };
    std::fs::write(&file, "mine").unwrap();
    let (_, err, ok) = write(&[]);
    assert!(!ok && err.contains("already there"), "{err}");
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "mine");
    assert!(write(&["--replace"]).2);
    assert!(std::fs::read_to_string(&file).unwrap().contains(id));
    // Without --client-id: locallibrary's own (Local Network's web app serves it).
    let (out, _, ok) = run(&store, &["account", "client-metadata"]);
    let published: Value = serde_json::from_str(&out).unwrap();
    assert!(ok);
    assert_eq!(published["client_id"], "https://locallibrary.localnetwork.computer/oauth-client-metadata.json");
    let (_, err, ok) = run(&store, &["account", "client-metadata", "--client-id", "http://insecure.example/c.json"]);
    assert!(!ok && err.contains("https://"), "{err}");
}

#[test]
fn rpc_account_methods() {
    let dir = tempfile::tempdir().unwrap();
    let mut child = locallibrary(&dir.path().join("store"))
        .arg("rpc")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let requests = [
        r#"{"jsonrpc":"2.0","id":1,"method":"version"}"#,
        r#"{"jsonrpc":"2.0","id":2,"method":"account.status"}"#,
        r#"{"jsonrpc":"2.0","id":3,"method":"account.logout"}"#,
        r#"{"jsonrpc":"2.0","id":4,"method":"account.login.finish","params":{"id":"nope"}}"#,
        r#"{"jsonrpc":"2.0","id":5,"method":"account.login.cancel","params":{"id":"nope"}}"#,
        r#"{"jsonrpc":"2.0","id":6,"method":"account.client_metadata","params":{"client_id":"https://x.example/c.json"}}"#,
        r#"{"jsonrpc":"2.0","id":7,"method":"account.check"}"#,
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
    for m in ["account.status", "account.login.begin", "account.login.finish", "account.logout"] {
        assert!(methods.contains(&Value::from(m)), "{m}");
    }
    assert!(reply(2)["result"]["account"].is_null());
    assert!(reply(3)["result"]["account"].is_null());
    assert_eq!(reply(4)["error"]["data"]["kind"], "not_found");
    assert_eq!(reply(5)["result"]["cancelled"], false);
    assert_eq!(reply(6)["result"]["redirect_uris"][0], "http://127.0.0.1/callback");
    assert_eq!(reply(7)["error"]["data"]["kind"], "account");
}
