//! `plugin account` on the command line, and `plugins.account*` in the sidecar, with the test
//! plugin given an account (its tokens in a file, `LOCALLIBRARY_SESSION_STORE=file`). Signing in
//! all the way isn't reachable here: the host reaches no server on this computer outside the
//! plugin host's own tests, which go through it with a server of their own.

use std::process::Command;

use serde_json::{Value, json};

use crate::support::{Env, Rpc, echo_plugin};

/// What the host keeps of a sign-in (as `plugin::accounts` writes it), standing for one.
const KEPT: &str = r#"{"access_token":"at-kept-1","refresh_token":"rt-kept-1","scopes":["read"]}"#;

/// The echo manifest with an account `main` on a service, and a server to talk to with it.
fn with_account() -> String {
    let account = "\n[[contributes.accounts]]\nid = \"main\"\ntitle = \"Main\"\n\
                   authorize_url = \"https://auth.example/authorize\"\ntoken_url = \"https://auth.example/token\"\n\
                   scopes = [\"read\"]\nclient_id = \"echo\"\n";
    echo_plugin().replace(r#""library:plan"]"#, r#""library:plan", "net:api.example"]"#) + account
}

fn env() -> Env {
    let mut env = Env::new();
    env.plugins(&[&with_account()]);
    env
}

/// The command, its tokens kept in a file.
fn command(env: &Env) -> Command {
    let mut c = env.command();
    c.env("LOCALLIBRARY_SESSION_STORE", "file");
    c
}

/// What a run of `args` exits with, and prints to stdout and stderr.
fn run(env: &Env, args: &[&str]) -> (i32, String, String) {
    let out = command(env).args(args).output().unwrap();
    let said = |b: Vec<u8>| String::from_utf8(b).unwrap();
    (out.status.code().unwrap(), said(out.stdout), said(out.stderr))
}

/// Signed in, as far as the host knows: its tokens kept in the store's secrets file.
fn signed_in(env: &Env) {
    locallibrary::Store::open_or_init(&env.store).unwrap();
    let dir = env.store.join("secrets/plugins");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("echo.json"), json!({ "account:main": KEPT }).to_string()).unwrap();
}

/// Whether `text` holds one of the tokens kept.
fn leaks(text: &str) -> bool {
    text.contains("at-kept-1") || text.contains("rt-kept-1")
}

/// An account is shown as signed in or not, with what it was allowed, and never its tokens; signing
/// out forgets them (here alone: the service has no revocation endpoint), and says so once more.
#[test]
fn an_account_is_signed_in_or_not_and_its_tokens_are_never_shown() {
    let env = env();
    locallibrary::Store::open_or_init(&env.store).unwrap();
    let (code, out, _) = run(&env, &["plugin", "account", "echo", "main"]);
    assert_eq!((code, out.as_str()), (0, "Main: not signed in\n"));
    signed_in(&env);
    for args in [&["plugin", "account", "echo", "main", "status"][..], &["plugin", "info", "echo"][..]] {
        let (code, out, err) = run(&env, args);
        assert_eq!(code, 0, "{err}");
        assert!(!leaks(&out) && !leaks(&err), "the output holds a token");
        assert!(out.contains("Main: signed in"), "{out}");
    }
    let (_, out, _) = run(&env, &["plugin", "account", "echo", "main"]);
    assert!(out.contains("allowed: read"), "{out}");
    let (_, json, _) = run(&env, &["--json", "plugin", "account", "echo", "main"]);
    assert!(!leaks(&json), "the JSON holds a token");
    let mut json: Value = serde_json::from_str(&json).unwrap();
    json.as_object_mut().unwrap().shift_remove("schema");
    let expected = json!({ "plugin": "echo", "account": "main", "title": "Main", "signed_in": true, "refreshes": true, "scopes": ["read"] });
    assert_eq!(json, expected);
    let (_, described, _) = run(&env, &["--json", "plugin", "info", "echo"]);
    let described: Value = serde_json::from_str(&described).unwrap();
    assert_eq!(described["accounts"][0]["signed_in"], true);

    let (code, out, _) = run(&env, &["plugin", "account", "echo", "main", "sign-out"]);
    assert_eq!((code, out.as_str()), (0, "✓ Signed out of Main on this computer\n"));
    let kept = std::fs::read_to_string(env.store.join("secrets/plugins/echo.json")).unwrap_or_default();
    assert!(!leaks(&kept), "a token is still kept");
    let (_, out, _) = run(&env, &["plugin", "account", "echo", "main", "sign-out"]);
    assert_eq!(out, "Not signed in to Main.\n");
    let (_, json, _) = run(&env, &["--json", "plugin", "account", "echo", "main", "sign-out"]);
    let json: Value = serde_json::from_str(&json).unwrap();
    assert_eq!(
        (&json["report"]["was_signed_in"], &json["report"]["revoked"]),
        (&json!(false), &json!(false)),
        "{json}"
    );
}

/// Signing in prints the service's sign-in page (with PKCE) when it isn't opened, and gives up once
/// the browser hasn't come back in time; pasting needs a terminal. An account or plugin that isn't
/// one is refused naming it.
#[test]
fn signing_in_sends_the_browser_to_the_service_and_waits_for_it() {
    let env = env();
    let (code, out, err) =
        run(&env, &["plugin", "account", "echo", "main", "sign-in", "--no-browser", "--timeout", "1"]);
    assert_eq!((code, out.as_str()), (1, ""), "{err}");
    let link = err.lines().find(|l| l.trim_start().starts_with("https://auth.example/authorize?")).unwrap_or_default();
    for part in [
        "response_type=code",
        "client_id=echo",
        "code_challenge_method=S256",
        "scope=read",
        "redirect_uri=http%3A%2F%2F127.0.0.1%3A",
    ] {
        assert!(link.contains(part), "{part}: {err}");
    }
    assert!(err.contains("no answer from the browser within 1 second; sign in again"), "{err}");
    let (code, _, err) = run(&env, &["plugin", "account", "echo", "main", "sign-in", "--paste"]);
    assert_eq!(code, 1, "{err}");
    assert!(err.contains("--paste needs a terminal to paste into"), "{err}");
    let (code, _, err) = run(&env, &["plugin", "account", "echo", "nope"]);
    assert!(code == 1 && err.contains("not found: the echo plugin's account \"nope\""), "{err}");
    let (code, _, err) = run(&env, &["plugin", "account", "nope", "main"]);
    assert!(code == 1 && err.contains("not found: plugin \"nope\""), "{err}");
}

/// The sidecar lists a plugin's accounts, starts a sign-in (answering the page to open), cancels
/// it, and signs out; finishing one that isn't waiting is refused. Nothing it answers holds a
/// token.
#[test]
fn the_sidecar_signs_in_and_out_of_a_plugins_account() {
    let env = env();
    signed_in(&env);
    let mut s = Rpc::start(command(&env));
    let listed = s.call(1, "plugins.accounts", json!({ "plugin": "echo" }));
    assert!(!leaks(&listed.to_string()), "the answer holds a token");
    assert_eq!(listed["result"][0]["signed_in"], true, "{listed}");
    let begun = s.call(2, "plugins.account.sign_in.begin", json!({ "plugin": "echo", "account": "main" }));
    let url = begun["result"]["url"].as_str().unwrap_or_default();
    assert!(url.starts_with("https://auth.example/authorize?") && url.contains("code_challenge="), "{begun}");
    let id = begun["result"]["id"].clone();
    let pasted = s.call(
        3,
        "plugins.account.sign_in.paste",
        json!({ "id": id, "url": "http://127.0.0.1/callback?code=c&state=other" }),
    );
    assert_eq!(pasted["error"]["data"]["kind"], "account", "{pasted}");
    let cancelled = s.call(4, "plugins.account.sign_in.cancel", json!({ "id": id }));
    assert_eq!(cancelled["result"], json!({ "cancelled": false }), "a paste ends the sign-in either way");
    let begun = s.call(5, "plugins.account.sign_in.begin", json!({ "plugin": "echo", "account": "main" }));
    let id = begun["result"]["id"].clone();
    assert_eq!(
        s.call(6, "plugins.account.sign_in.cancel", json!({ "id": id }))["result"],
        json!({ "cancelled": true })
    );
    let finished = s.call(7, "plugins.account.sign_in.finish", json!({ "id": id, "timeout_secs": 1 }));
    assert_eq!(finished["error"]["data"]["kind"], "not_found", "{finished}");
    let out = s.call(8, "plugins.account.sign_out", json!({ "plugin": "echo", "account": "main" }));
    assert!(!leaks(&out.to_string()), "the answer holds a token");
    assert_eq!((&out["result"]["was_signed_in"], &out["result"]["signed_in"]), (&json!(true), &json!(false)), "{out}");
}
