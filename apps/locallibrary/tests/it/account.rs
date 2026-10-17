//! `locallibrary account` and the `account.*` RPC methods. Signing in needs a browser and a real
//! account, so a signed-in session is written as the files a sign-in with `--session-file` leaves
//! (never the keychain), for a pretend data server on 127.0.0.1.

#![cfg(feature = "account")]

use serde_json::{Value, json};

use crate::mock_pds::MockPds;
use crate::support::account::{DID, PUBLISHING, sign_in};
use crate::support::{Env, Rpc, path};

#[test]
fn signed_out_status_and_sign_out() {
    let env = Env::new();
    let out = env.ok(&["account"]);
    assert!(out.contains("Not signed in to an Atmosphere account"), "{out}");
    let v = env.json(&["account", "status"]);
    assert!(v["account"].is_null() && v["last_handle"].is_null(), "{v}");
    let out = env.ok(&["account", "logout"]);
    assert!(out.contains("Not signed in"), "{out}");
    let err = env.fails(1, &["account", "status", "--check"]);
    assert!(err.contains("not signed in to an Atmosphere account"), "{err}");
    assert!(!env.store.exists(), "nothing is written while signed out");

    // No handle, nothing remembered, and no terminal to ask in.
    let r = env.run(&["--json", "account", "login"]);
    assert!(r.code != 0 && (r.out + &r.err).contains("which Atmosphere account?"));
}

/// The client metadata document for a client of one's own, printed or written to a file (never
/// over one that's there unless asked), and where to host it.
#[test]
fn client_metadata_for_a_hosted_client() {
    let env = Env::new();
    let id = "https://locallibrary.example/oauth-client-metadata.json";
    let out = env.ok(&["account", "client-metadata", "--client-id", id, "--scope", "repo:app.example.thing"]);
    let v: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["client_id"], id);
    assert_eq!(v["application_type"], "native");
    // Every document allows publishing locallibrary's records and filing bug reports, so sign-in
    // can ask for them.
    let scope = "atproto include:computer.localnetwork.locallibraryPublishing repo:app.example.thing \
                 repo:app.userinput.discussion?action=create";
    assert_eq!(v["scope"], scope);
    let file = env.outside("meta.json");
    let write = |more: &[&str]| {
        env.run(&[&["account", "client-metadata", "--client-id", id, "-o", path(&file)], more].concat())
    };
    let out = write(&[]).out;
    assert!(file.exists() && out.contains(&format!("wrote {}", file.display())), "{out}");
    assert!(out.contains(&format!("Host it at {id}, then sign in with --client-id {id}")), "{out}");
    std::fs::write(&file, "mine").unwrap();
    let r = write(&[]);
    assert!(r.code != 0 && r.err.contains("already there"), "{}", r.err);
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "mine");
    assert_eq!(write(&["--replace"]).code, 0);
    assert!(std::fs::read_to_string(&file).unwrap().contains(id));
    // Without --client-id: locallibrary's own (Local Network's web app serves it).
    let published: Value = serde_json::from_str(&env.ok(&["account", "client-metadata"])).unwrap();
    assert_eq!(published["client_id"], "https://locallibrary.localnetwork.computer/oauth-client-metadata.json");
    let out = env.ok(&["account", "client-metadata", "-o", path(&file), "--replace"]);
    let host = "Host it at https://locallibrary.localnetwork.computer/oauth-client-metadata.json; sign-in uses it once it's online";
    assert!(out.contains(host), "{out}");
    let r = env.run(&["account", "client-metadata", "--client-id", "http://insecure.example/c.json"]);
    assert!(r.code != 0 && r.err.contains("https://"), "{}", r.err);
}

#[test]
fn rpc_account_methods() {
    let env = Env::new();
    let mut rpc = Rpc::start(env.command());
    let version = rpc.call(1, "version", Value::Null);
    let methods = version["result"]["methods"].as_array().unwrap();
    for m in ["account.status", "account.login.begin", "account.login.finish", "account.logout"] {
        assert!(methods.contains(&Value::from(m)), "{m}");
    }
    assert!(rpc.call(2, "account.status", Value::Null)["result"]["account"].is_null());
    assert!(rpc.call(3, "account.logout", Value::Null)["result"]["account"].is_null());
    assert_eq!(rpc.call(4, "account.login.finish", json!({ "id": "nope" }))["error"]["data"]["kind"], "not_found");
    assert_eq!(rpc.call(5, "account.login.cancel", json!({ "id": "nope" }))["result"]["cancelled"], false);
    let metadata = rpc.call(6, "account.client_metadata", json!({ "client_id": "https://x.example/c.json" }));
    assert_eq!(metadata["result"]["redirect_uris"][0], "http://127.0.0.1/callback");
    assert_eq!(rpc.call(7, "account.check", Value::Null)["error"]["data"]["kind"], "account");
}

/// A day on the calendar, `hours` from now.
fn from_now(hours: i64) -> String {
    (chrono::Utc::now() + chrono::Duration::hours(hours)).to_rfc3339()
}

#[test]
fn the_status_says_what_the_session_allows_and_until_when() {
    let env = Env::ascii();
    let pds = "http://127.0.0.1:9";
    // The status reads the account's details only: a session kept in the keychain isn't opened.
    let client = json!({ "kind": "hosted", "client_id": "https://dj.example.com/c.json" });
    sign_in(&env.store, pds, &["atproto", "transition:generic"], json!({ "storage": "keychain", "client": client }));
    let out = env.ok(&["account"]);
    assert!(out.starts_with("Atmosphere account @dj.example.com\n"), "{out}");
    assert!(out.contains("  did:plc:testdj - data on 127.0.0.1:9 - signed in at 127.0.0.1:9\n"), "{out}");
    // The account's details hold UTC; the status shows this computer's calendar.
    let day =
        |t: &str| locallibrary::local_time(chrono::DateTime::parse_from_rfc3339(t).unwrap(), "%Y-%m-%d").to_string();
    let (signed_in, again) = (day("2026-10-05T00:00:00Z"), day("2099-01-01T00:00:00Z"));
    assert!(out.contains(&format!("  signed in {signed_in} - sign in again by {again} (")), "{out}");
    assert!(out.contains("allowed: atproto transition:generic - session kept "), "{out}");
    if cfg!(target_os = "macos") {
        assert!(out.contains("session kept in the macOS Keychain"), "{out}");
    }
    assert!(out.contains("  client https://dj.example.com/c.json\n"), "{out}");

    // Signed in to publish: the server granted the permission set's scopes in its place.
    sign_in(&env.store, pds, &PUBLISHING, json!({ "sign_in_again_by": from_now(2) }));
    let out = env.ok(&["account", "status"]);
    assert!(out.contains(" (today)\n"), "{out}");
    assert!(
        out.contains("can publish your My Settings, profile and sets; can't post or change anything else"),
        "{out}"
    );
    assert!(out.contains("session kept in atmosphere-session.json, readable only by you"), "{out}");
    sign_in(&env.store, pds, &["atproto"], json!({ "sign_in_again_by": from_now(-2) }));
    let out = env.ok(&["account", "status"]);
    assert!(out.contains(" - the session has likely ended; sign in again\n"), "{out}");
}

#[test]
fn the_session_is_checked_with_the_server_and_signed_out() {
    let env = Env::ascii();
    let pds = MockPds::start();
    pds.set_account(DID, "dj.example.com");
    sign_in(&env.store, &pds.url, &["atproto"], json!({}));
    let out = env.ok(&["account", "status", "--check"]);
    assert!(out.starts_with("Atmosphere account @dj.example.com\n"), "{out}");
    assert!(out.contains(&format!("v {} answered for this session", pds.url)), "{out}");

    let out = env.ok(&["account", "logout"]);
    assert_eq!(out, "v Signed out of @dj.example.com\n");
    // The handle is remembered, for signing in again.
    let out = env.ok(&["account"]);
    assert!(out.contains("`locallibrary account login dj.example.com` signs in with the account"), "{out}");

    // The server can't be told: the session is gone from this computer all the same.
    pds.set_revocation(500);
    sign_in(&env.store, &pds.url, &["atproto"], json!({}));
    let out = env.ok(&["account", "logout"]);
    assert!(out.contains("v Signed out of @dj.example.com on this computer\n"), "{out}");
    assert!(out.contains("the server couldn't be told; the session ends on its own within two weeks"), "{out}");
    assert!(env.ok(&["account"]).contains("Not signed in"));

    // Its secrets can't be removed (a folder where the file is): signing out says so.
    sign_in(&env.store, &pds.url, &["atproto"], json!({}));
    let secrets = env.store.join("atmosphere-session.json");
    std::fs::remove_file(&secrets).unwrap();
    std::fs::create_dir(&secrets).unwrap();
    let out = env.ok(&["account", "logout"]);
    assert!(out.contains("its secrets are still on this computer ("), "{out}");
    assert!(out.contains("`locallibrary account logout` tries again"), "{out}");
}
