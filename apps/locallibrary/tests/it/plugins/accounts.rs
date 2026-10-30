//! Accounts on the command line: `account` listing them, signing in to one through an account
//! plugin at the mock service (see [`super::shared_accounts`]) and out, `plugin access` giving the
//! echo plugin access, and a plugin's destination asking which account it may use only where it
//! can ask (see [`cli`]). Here, `plugin account` and `plugins.account*`: a plugin's own
//! accounts (their tokens in a file, `LOCALLIBRARY_SESSION_STORE=file`), which reach no server on
//! this computer.

mod cli;

use std::process::Command;

use serde_json::{Value, json};

use super::shared_accounts::browser_back;
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

/// How the browser ends a sign-in the sidecar began, in
/// [`an_account_is_as_its_kept_sign_in_says_wherever_its_asked`].
#[derive(Debug, Clone, Copy)]
enum Ending {
    /// Cancelled before the browser comes back.
    Cancelled,
    /// The address of another sign-in, pasted.
    Foreign,
    /// Declined at the service, pasted.
    Declined,
    /// Stopped by the service, saying why, pasted.
    Stopped,
    /// Approved, coming back to this computer (or pasted): the code is traded at a token endpoint
    /// on this computer, which a release never reaches.
    Approved { pasted: bool },
    /// Never back, waited for no time at all.
    Waited,
}

/// A step of [`an_account_is_as_its_kept_sign_in_says_wherever_its_asked`].
#[derive(Debug, Clone, Copy)]
enum Step {
    /// A sign-in kept as the host keeps one: refreshed or not, with this many of the scopes, and
    /// a time it expires or not.
    Keep { refreshes: bool, scopes: usize, expires: bool },
    /// `plugin account echo main` (`status` named or not).
    Status { json: bool, named: bool },
    /// `plugin info echo`.
    Info { json: bool },
    /// `plugin account echo main sign-out`.
    SignOut { json: bool },
    /// `sign-in --paste`, with no terminal to paste into.
    Paste,
    /// `sign-in --no-browser --timeout 0`.
    GaveUp,
    /// `plugins.accounts` in the sidecar.
    Listed,
    /// `plugins.account.sign_out` in the sidecar.
    SidecarSignOut,
    /// `plugins.account.sign_in.begin` in the sidecar, then the browser.
    Begun(Ending),
}

/// A sign-in kept, as the model has it.
#[derive(Debug, Clone, Copy)]
struct Kept {
    refreshes: bool,
    scopes: usize,
    expires: bool,
}

/// When a kept sign-in expires, when it does.
const EXPIRES: &str = "2030-01-01T00:00:00Z";

impl Kept {
    fn scopes(&self) -> Vec<&'static str> {
        ["read", "write"][..self.scopes].to_vec()
    }

    /// As the host writes it among the plugin's secrets.
    fn written(&self) -> String {
        let mut kept = json!({ "access_token": "at-kept-1", "scopes": self.scopes() });
        if self.refreshes {
            kept["refresh_token"] = json!("rt-kept-1");
        }
        if self.expires {
            kept["expires_at"] = json!(EXPIRES);
        }
        kept.to_string()
    }
}

/// The account's status as `--json` and the sidecar answer it.
fn status_json(kept: Option<Kept>) -> Value {
    let mut status = json!({
        "plugin": "echo",
        "account": "main",
        "title": "Main",
        "signed_in": kept.is_some(),
        "refreshes": kept.is_some_and(|k| k.refreshes),
        "scopes": kept.map(|k| k.scopes()).unwrap_or_default(),
    });
    if kept.is_some_and(|k| k.expires) {
        status["expires"] = json!(EXPIRES);
    }
    status
}

/// The account's status as text.
fn status_text(kept: Option<Kept>) -> String {
    let Some(k) = kept else { return "Main: not signed in\n".into() };
    let mut text = String::from("Main: signed in\n");
    if k.scopes > 0 {
        text.push_str(&format!("  allowed: {}\n", k.scopes().join(", ")));
    }
    if !k.refreshes {
        text.push_str("  sign in again once it expires\n");
    }
    text
}

/// `v` without the `schema` `--json` adds.
fn unschema(mut v: Value) -> Value {
    v.as_object_mut().unwrap().shift_remove("schema");
    v
}

/// The query of a sign-in's address, by name, as it was sent (each value one this test reads
/// without decoding: a state, a challenge).
fn query_of(url: &str) -> Vec<(String, String)> {
    let (_, query) = url.split_once('?').unwrap();
    query.split('&').filter_map(|p| p.split_once('=')).map(|(k, v)| (k.into(), v.into())).collect()
}

/// For any steps, on the command line and in the sidecar: the account reads back (as text, as
/// JSON, in `plugin info` and in the sidecar) as the sign-in kept last says, or signed out once
/// it's signed out of, which says whether it was signed in; a sign-in that's pasted without a
/// terminal, given up on, cancelled, declined, stopped, from another sign-in, or whose code the
/// service can't be reached to trade keeps what was kept, says why, and is over; and nothing
/// answered or printed holds a token.
#[hegel::test(test_cases = 12, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn an_account_is_as_its_kept_sign_in_says_wherever_its_asked(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let revokes = tc.draw(gs::booleans());
    let mut manifest = with_account().replace("https://auth.example/token", "https://127.0.0.1/token");
    if revokes {
        manifest.push_str("revoke_url = \"https://127.0.0.1/revoke\"\n");
    }
    let mut env = Env::new();
    env.plugins(&[&manifest]);
    let mut s = Rpc::start(command(&env));
    let mut kept: Option<Kept> = None;
    let n = tc.draw(gs::integers::<usize>().min_value(3).max_value(8));
    for i in 0..n {
        let json = tc.draw(gs::booleans());
        let step = match tc.draw(gs::integers::<u8>().max_value(8)) {
            0 => Step::Keep {
                refreshes: tc.draw(gs::booleans()),
                scopes: tc.draw(gs::integers::<usize>().max_value(2)),
                expires: tc.draw(gs::booleans()),
            },
            1 => Step::Status { json, named: tc.draw(gs::booleans()) },
            2 => Step::Info { json },
            3 => Step::SignOut { json },
            4 => Step::Paste,
            5 => Step::GaveUp,
            6 => Step::Listed,
            7 => Step::SidecarSignOut,
            _ => Step::Begun(match tc.draw(gs::integers::<u8>().max_value(6)) {
                0 => Ending::Cancelled,
                1 => Ending::Foreign,
                2 => Ending::Declined,
                3 => Ending::Stopped,
                4 => Ending::Approved { pasted: false },
                5 => Ending::Approved { pasted: true },
                _ => Ending::Waited,
            }),
        };
        let id = i as i64 * 10;
        match step {
            Step::Keep { refreshes, scopes, expires } => {
                let k = Kept { refreshes, scopes, expires };
                locallibrary::Store::open_or_init(&env.store).unwrap();
                let dir = env.store.join("secrets/plugins");
                std::fs::create_dir_all(&dir).unwrap();
                std::fs::write(dir.join("echo.json"), json!({ "account:main": k.written() }).to_string()).unwrap();
                kept = Some(k);
            }
            Step::Status { json, named } => {
                let mut args = vec!["plugin", "account", "echo", "main"];
                if named {
                    args.push("status");
                }
                if json {
                    args.insert(0, "--json");
                }
                let (code, out, err) = run(&env, &args);
                assert!(code == 0 && !leaks(&out) && !leaks(&err), "step {i}: {step:?}");
                match json {
                    true => assert_eq!(unschema(serde_json::from_str(&out).unwrap()), status_json(kept), "step {i}"),
                    false => assert_eq!(out, status_text(kept), "step {i}"),
                }
            }
            Step::Info { json } => {
                let args: &[&str] =
                    if json { &["--json", "plugin", "info", "echo"] } else { &["plugin", "info", "echo"] };
                let (code, out, err) = run(&env, args);
                assert!(code == 0 && !leaks(&out) && !leaks(&err), "step {i}: {step:?}");
                match json {
                    true => {
                        let info: Value = serde_json::from_str(&out).unwrap();
                        assert_eq!(info["accounts"], json!([status_json(kept)]), "step {i}");
                    }
                    false => {
                        let said = if kept.is_some() { "Main: signed in" } else { "Main: not signed in" };
                        assert!(out.contains(said), "step {i}: {out}");
                    }
                }
            }
            Step::SignOut { json } => {
                let args = ["plugin", "account", "echo", "main", "sign-out"];
                let args: Vec<&str> = if json { [&["--json"][..], &args].concat() } else { args.to_vec() };
                let (code, out, err) = run(&env, &args);
                assert!(code == 0 && !leaks(&out) && !leaks(&err), "step {i}: {step:?}");
                match json {
                    true => {
                        let report = &serde_json::from_str::<Value>(&out).unwrap()["report"];
                        let mut expected = status_json(None);
                        expected["was_signed_in"] = json!(kept.is_some());
                        expected["revoked"] = json!(false);
                        assert_eq!(report, &expected, "step {i}");
                    }
                    false => {
                        let said = match kept {
                            Some(_) => "✓ Signed out of Main on this computer\n",
                            None => "Not signed in to Main.\n",
                        };
                        assert_eq!(out, said, "step {i}");
                    }
                }
                kept = None;
            }
            Step::Paste | Step::GaveUp => {
                let how: &[&str] = match step {
                    Step::Paste => &["--paste"],
                    _ => &["--no-browser", "--timeout", "0"],
                };
                let (code, out, err) =
                    run(&env, &[&["plugin", "account", "echo", "main", "sign-in"][..], how].concat());
                assert_eq!((code, out.as_str()), (1, ""), "step {i}: {step:?}");
                assert!(err.contains("Sign in: open this link\n  https://auth.example/authorize?"), "step {i}: {err}");
                let why = match step {
                    Step::Paste => "--paste needs a terminal to paste into",
                    _ => "no answer from the browser within 0 seconds; sign in again",
                };
                assert!(err.contains(why) && !leaks(&err), "step {i}: {err}");
            }
            Step::Listed => {
                let listed = s.call(id, "plugins.accounts", json!({ "plugin": "echo" }));
                assert!(!leaks(&listed.to_string()), "step {i}: the answer holds a kept sign-in");
                assert_eq!(listed["result"], json!([status_json(kept)]), "step {i}");
            }
            Step::SidecarSignOut => {
                let out = s.call(id, "plugins.account.sign_out", json!({ "plugin": "echo", "account": "main" }));
                assert!(!leaks(&out.to_string()), "step {i}: the answer holds a kept sign-in");
                let mut expected = status_json(None);
                expected["was_signed_in"] = json!(kept.is_some());
                expected["revoked"] = json!(false);
                assert_eq!(out["result"], expected, "step {i}");
                kept = None;
            }
            Step::Begun(ending) => {
                let begun = s.call(id, "plugins.account.sign_in.begin", json!({ "plugin": "echo", "account": "main" }));
                let r = &begun["result"];
                let (url, redirect) = (r["url"].as_str().unwrap(), r["redirect_uri"].as_str().unwrap());
                assert_eq!((&r["plugin"], &r["account"]), (&json!("echo"), &json!("main")), "step {i}");
                let query = query_of(url);
                let at = |k: &str| query.iter().find(|(key, _)| key == k).map(|(_, v)| v.clone()).unwrap_or_default();
                assert_eq!(
                    (at("client_id"), at("scope"), at("code_challenge_method")),
                    ("echo".into(), "read".into(), "S256".into())
                );
                let state = at("state");
                let sign_in = r["id"].clone();
                let paste = |s: &mut Rpc, query: String| {
                    let url = format!("{redirect}?{query}");
                    s.call(id + 1, "plugins.account.sign_in.paste", json!({ "id": sign_in, "url": url }))
                };
                let (answer, why) = match ending {
                    Ending::Cancelled => {
                        let cancelled = s.call(id + 1, "plugins.account.sign_in.cancel", json!({ "id": sign_in }));
                        assert_eq!(cancelled["result"], json!({ "cancelled": true }), "step {i}");
                        (None, "")
                    }
                    Ending::Foreign => (
                        Some(paste(&mut s, "code=c0de&state=guess".into())),
                        "that address is from another sign-in: sign in again",
                    ),
                    Ending::Declined => (
                        Some(paste(&mut s, format!("error=access_denied&state={state}"))),
                        "the sign-in was declined, so locallibrary wasn't given access",
                    ),
                    Ending::Stopped => (
                        Some(paste(&mut s, format!("error=server_error&error_description=down+for+now&state={state}"))),
                        "your account's server stopped the sign-in: down for now",
                    ),
                    Ending::Approved { pasted: true } => {
                        (Some(paste(&mut s, format!("code=c0de&state={state}"))), "signing in to Main: ")
                    }
                    Ending::Approved { pasted: false } => {
                        let browser = browser_back(redirect, format!("code=c0de&state={state}"));
                        let finished = s.call(
                            id + 1,
                            "plugins.account.sign_in.finish",
                            json!({ "id": sign_in, "timeout_secs": 10 }),
                        );
                        assert!(browser.join().unwrap().contains("couldn't sign you in"), "step {i}: told the browser");
                        (Some(finished), "signing in to Main: ")
                    }
                    Ending::Waited => (
                        Some(s.call(
                            id + 1,
                            "plugins.account.sign_in.finish",
                            json!({ "id": sign_in, "timeout_secs": 0 }),
                        )),
                        "no answer from the browser within 0 seconds; sign in again",
                    ),
                };
                if let Some(answer) = answer {
                    assert_eq!(answer["error"]["data"]["kind"], "account", "step {i}: {ending:?}");
                    let said = answer["error"]["message"].as_str().unwrap_or_default();
                    assert!(said.starts_with(why), "step {i}: {ending:?} said {said}");
                }
                // Over, either way.
                let again = s.call(id + 2, "plugins.account.sign_in.cancel", json!({ "id": sign_in }));
                assert_eq!(again["result"], json!({ "cancelled": false }), "step {i}");
            }
        }
    }
    let listed = s.call(1000, "plugins.accounts", json!({ "plugin": "echo" }));
    assert_eq!(listed["result"], json!([status_json(kept)]));
}
