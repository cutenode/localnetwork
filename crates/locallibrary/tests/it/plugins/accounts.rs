//! Plugins' accounts: signing in through a mock OAuth server on this computer (the test plays the
//! browser), the tokens kept with the plugin's secrets and filled into the requests that name the
//! account, refreshed before they expire, never sent to another origin, never seen by the plugin,
//! its log or what the host answers; and signing out.

use std::time::Duration;

use hegel::TestCase;
use hegel::generators as gs;
use indexmap::IndexMap;
use locallibrary::plugin::{AccountStatus, SignIn};
use oauth2::{PkceCodeChallenge, PkceCodeVerifier};

use super::net::{Got, Reply, Server, fixture, ok};
use super::*;

/// What the mock service hands out: access tokens `at-<n>` and refresh tokens `rt-<n>`, each
/// valid `expires_in` seconds, and the code challenge the browser was sent with.
#[derive(Default)]
struct Service {
    expires_in: Option<u64>,
    /// Whether a refresh answers a new refresh token.
    rotates: bool,
    /// An error the token endpoint answers instead (`invalid_grant`), with its description.
    refuses: Option<(String, String)>,
    /// The token endpoint answers what can't be read as tokens instead.
    garbles: bool,
    challenge: String,
    issued: usize,
}

/// The form a request sent, by name.
fn form(got: &Got) -> IndexMap<String, String> {
    form_urlencoded::parse(got.body.as_bytes()).into_owned().collect()
}

fn json_reply(status: u16, body: Value) -> Reply {
    Reply { status, headers: vec![("Content-Type", "application/json".into())], body: body.to_string().into_bytes() }
}

/// A mock OAuth service: `/token` trades a code (checked against the PKCE challenge) or a refresh
/// token for tokens, `/revoke` takes one back, and anything else answers what it was sent.
fn service(state: Arc<Mutex<Service>>) -> Server {
    Server::new(move |base, got| {
        let mut s = state.lock().unwrap();
        match got.target.as_str() {
            "/token" => {
                let f = form(got);
                if let Some((error, description)) = &s.refuses {
                    return json_reply(400, json!({ "error": error, "error_description": description }));
                }
                if s.garbles {
                    return json_reply(200, json!({ "token_type": "bearer" }));
                }
                let pkce_holds = |v: &String| {
                    PkceCodeChallenge::from_code_verifier_sha256(&PkceCodeVerifier::new(v.clone())).as_str()
                        == s.challenge
                };
                let fits = match f.get("grant_type").map(String::as_str) {
                    Some("authorization_code") => f.get("code_verifier").is_some_and(pkce_holds),
                    Some("refresh_token") => f.contains_key("refresh_token"),
                    _ => false,
                };
                if !fits || f.get("client_id").map(String::as_str) != Some("locallibrary-test") {
                    return json_reply(400, json!({ "error": "invalid_request" }));
                }
                s.issued += 1;
                let n = s.issued;
                let mut answer =
                    json!({ "access_token": format!("at-{n}"), "token_type": "bearer", "scope": "read write" });
                if f["grant_type"] == "authorization_code" || s.rotates {
                    answer["refresh_token"] = format!("rt-{n}").into();
                }
                if let Some(secs) = s.expires_in {
                    answer["expires_in"] = secs.into();
                }
                json_reply(200, answer)
            }
            "/revoke" => ok(""),
            // Elsewhere in its own origin.
            "/home" => Reply { status: 307, headers: vec![("Location", format!("{base}/here"))], body: Vec::new() },
            _ => ok(format!("{} {}", got.target, got.headers.get("authorization").map_or("-", String::as_str))),
        }
    })
}

/// The echo plugin talking to this computer, with an account `main` at `server`'s endpoints.
fn with_account(server: &str) -> String {
    let account = format!(
        r#"
[[contributes.accounts]]
id = "main"
title = "Mock"
authorize_url = "{server}/authorize?prompt=consent"
token_url = "{server}/token"
scopes = ["read", "write"]
client_id = "locallibrary-test"
revoke_url = "{server}/revoke"
"#
    );
    format!("{}{account}", granting(&["net:127.0.0.1"]))
}

/// The query of the address a sign-in opens in the browser, by name.
fn opened(start: &SignIn) -> IndexMap<String, String> {
    let url = reqwest::Url::parse(&start.url).unwrap();
    url.query_pairs().into_owned().collect()
}

/// Comes back from the browser to a sign-in with `query` (or the code and state it was sent with),
/// once it's waited for.
fn come_back(start: &SignIn, query: Option<String>) -> std::thread::JoinHandle<String> {
    let query = query.unwrap_or_else(|| format!("code=c0de&state={}", opened(start)["state"]));
    let url = format!("{}?{query}", start.redirect_uri);
    std::thread::spawn(move || reqwest::blocking::get(url).unwrap().text().unwrap())
}

impl Fixture {
    /// Signs in to `main`, the service checking the code challenge the browser was sent.
    fn sign_in(&self, state: &Mutex<Service>) -> locallibrary::Result<AccountStatus> {
        let start = self.plugins.begin_sign_in("echo", "main", &self.store)?;
        state.lock().unwrap().challenge = opened(&start)["code_challenge"].clone();
        let browser = come_back(&start, None);
        let signed_in = self.plugins.finish_sign_in(&start.id, Duration::from_secs(10), &Ctx::silent());
        browser.join().unwrap();
        signed_in
    }

    /// What the server answered a fetch of `path` with the account named as `authorization`.
    fn as_account(&self, server: &Server, path: &str, authorization: &str) -> Value {
        self.fetch(json!({ "url": format!("{}{path}", server.url), "headers": { "Authorization": authorization } }))
    }

    /// Everything here that might hold a token: the plugin's logs, its answers so far, and every
    /// file in the store but its secrets.
    fn seen(&self, answers: &[&Value]) -> String {
        let logs: Vec<String> = self.logs.lock().unwrap().iter().map(|(_, l)| l.clone()).collect();
        let files = walkdir::WalkDir::new(self.store.root()).into_iter().flatten().filter(|e| e.file_type().is_file());
        let secrets = self.store.root().join("secrets");
        let files = files.filter(|e| !e.path().starts_with(&secrets)).map(|e| std::fs::read(e.path()).unwrap());
        let files: Vec<String> = files.map(|b| String::from_utf8_lossy(&b).into_owned()).collect();
        let answers: Vec<String> = answers.iter().map(|a| a.to_string()).collect();
        [logs, files, answers].concat().join("\n")
    }
}

/// Whether the server answered `answer`'s fetch of `path` as sent `Bearer` and the access token
/// `at-<n>`. Asserts never print a token, even a test's.
fn sent_token(answer: &Value, path: &str, n: usize) -> bool {
    answer["result"]["body"] == format!("{path} Bearer at-{n}")
}

/// Whether a request's form has `value` as `key` (a token, never printed).
fn has(form: &IndexMap<String, String>, key: &str, value: &str) -> bool {
    form.get(key).is_some_and(|v| v == value)
}

/// What a status says, never a token.
fn shown(status: &AccountStatus) -> Value {
    serde_json::to_value(status).unwrap()
}

#[test]
fn signing_in_keeps_the_tokens_and_fills_them_into_requests_naming_the_account() {
    let state = Arc::new(Mutex::new(Service { expires_in: Some(3600), ..Service::default() }));
    let server = service(state.clone());
    let f = fixture(&with_account(&server.url), &[], Options::default());
    let before = f.plugins.account("echo", "main", Some(&f.store)).unwrap();
    assert_eq!((before.signed_in, before.refreshes, before.expires), (false, false, None));

    let start = f.plugins.begin_sign_in("echo", "main", &f.store).unwrap();
    let query = opened(&start);
    assert!(start.url.starts_with(&format!("{}/authorize?prompt=consent&", server.url)), "{}", start.url);
    for (key, value) in [
        ("response_type", "code"),
        ("client_id", "locallibrary-test"),
        ("scope", "read write"),
        ("code_challenge_method", "S256"),
        ("redirect_uri", start.redirect_uri.as_str()),
    ] {
        assert_eq!(query.get(key).map(String::as_str), Some(value), "{key}");
    }
    assert!(start.redirect_uri.starts_with("http://127.0.0.1:"), "{}", start.redirect_uri);
    state.lock().unwrap().challenge = query["code_challenge"].clone();
    let browser = come_back(&start, None);
    let status = f.plugins.finish_sign_in(&start.id, Duration::from_secs(10), &Ctx::silent()).unwrap();
    assert!(browser.join().unwrap().contains("Mock is signed in."));
    assert_eq!(
        (status.signed_in, status.refreshes, status.scopes.clone()),
        (true, true, vec!["read".into(), "write".into()])
    );
    assert!(status.expires.is_some_and(|at| at > chrono::Utc::now() + chrono::Duration::minutes(59)));
    let traded = form(&server.got()[0]);
    assert!(has(&traded, "grant_type", "authorization_code") && has(&traded, "code", "c0de"), "the code is traded");
    assert_eq!(traded["redirect_uri"], start.redirect_uri);
    // A sign-in is finished once.
    let again = f.plugins.finish_sign_in(&start.id, Duration::from_secs(1), &Ctx::silent()).unwrap_err();
    assert_eq!(again.code(), "not_found");

    let answer = f.as_account(&server, "/me", "{account:main}");
    assert!(sent_token(&answer, "/me", 1), "the server wasn't sent the token");
    let listed = shown(&f.plugins.accounts("echo", Some(&f.store)).unwrap()[0]);
    assert_eq!(listed, shown(&status));
    // The server's answer is the server's: it echoed what it was sent.
    let seen = f.seen(&[&listed]);
    assert!(!seen.contains("at-1") && !seen.contains("rt-1"), "a token is out where it may not be");
    assert_eq!(state.lock().unwrap().issued, 1, "a token valid for an hour isn't refreshed");
}

#[test]
fn a_token_about_to_expire_is_refreshed_before_its_used() {
    let state = Arc::new(Mutex::new(Service { expires_in: Some(30), ..Service::default() }));
    let server = service(state.clone());
    let f = fixture(&with_account(&server.url), &[], Options::default());
    f.sign_in(&state).unwrap();
    // Within the minute before it expires: refreshed with the refresh token, which the service
    // doesn't replace, so it's kept.
    assert!(sent_token(&f.as_account(&server, "/a", "{account:main}"), "/a", 2), "not refreshed");
    let refreshed = form(&server.got().iter().rev().nth(1).unwrap().clone());
    assert!(has(&refreshed, "grant_type", "refresh_token") && has(&refreshed, "refresh_token", "rt-1"), "refreshed");
    assert!(f.plugins.account("echo", "main", Some(&f.store)).unwrap().refreshes, "the refresh token is kept");
    // Refreshed for an hour: used as it is.
    state.lock().unwrap().expires_in = Some(3600);
    assert!(sent_token(&f.as_account(&server, "/b", "{account:main}"), "/b", 3), "not refreshed");
    assert!(sent_token(&f.as_account(&server, "/c", "{account:main}"), "/c", 3), "refreshed when it needn't be");
    assert_eq!(state.lock().unwrap().issued, 3);

    // Signed in again, for seconds, with a service that can't refresh it now: what it said,
    // without the refresh token, and still signed in.
    f.plugins.sign_out("echo", "main", &f.store).unwrap();
    state.lock().unwrap().expires_in = Some(5);
    f.sign_in(&state).unwrap();
    state.lock().unwrap().refuses = Some(("temporarily_unavailable".into(), "rt-4 can't be used now".into()));
    let e = f.as_account(&server, "/d", "{account:main}");
    let said = e["error"]["message"].as_str().unwrap_or_default().to_string();
    assert!(said.starts_with("signing in to Mock: temporarily_unavailable: "), "not what the service said");
    assert!(!said.contains("rt-4"), "the refresh token is left out of what the plugin is told");
    assert!(f.plugins.account("echo", "main", Some(&f.store)).unwrap().signed_in);
    // One that won't refresh it any more: it's signed out, and signing in again will do.
    state.lock().unwrap().refuses = Some(("invalid_grant".into(), "spent".into()));
    let e = &f.as_account(&server, "/e", "{account:main}")["error"];
    assert_eq!(e["message"], "its Mock account isn't signed in (locallibrary plugin account echo main sign-in)");
    assert!(!f.plugins.account("echo", "main", Some(&f.store)).unwrap().signed_in);
}

#[test]
fn an_account_goes_only_in_the_authorization_header_and_never_to_another_origin() {
    let state = Arc::new(Mutex::new(Service { expires_in: Some(3600), ..Service::default() }));
    let elsewhere = Server::new(|_, got| ok(got.headers.get("authorization").cloned().unwrap_or_default()));
    let to = elsewhere.url.clone();
    let server = service(state.clone());
    let redirecting = Server::new(move |_, _| Reply {
        status: 302,
        headers: vec![("Location", format!("{to}/there"))],
        body: Vec::new(),
    });
    let f = fixture(&with_account(&server.url), &[], Options::default());
    f.sign_in(&state).unwrap();
    let refused = "an account goes only in the Authorization header: {account:<id>}";
    for fetch in [
        json!({ "url": format!("{}/x", server.url), "headers": { "X-Token": "{account:main}" } }),
        json!({ "url": format!("{}/x", server.url), "headers": { "Authorization": "Bearer {account:main}" } }),
        json!({ "url": format!("{}/x?t={{account:main}}", server.url) }),
        json!({ "url": format!("{}/x", server.url), "method": "POST", "body": "t={account:main}" }),
    ] {
        assert_eq!(f.fetch(fetch.clone())["error"]["message"], refused, "{fetch}");
    }
    let e = f.as_account(&server, "/x", "{account:other}");
    assert_eq!(e["error"]["message"], "\"other\" isn't one of its accounts");
    // A server it isn't granted gets nothing, the token least of all.
    let e = f.fetch(json!({ "url": "https://example.com/", "headers": { "Authorization": "{account:main}" } }));
    assert_eq!(e["error"]["data"]["kind"], "denied", "{e}");
    // Redirected to another origin (another port), the token stays behind; within its origin, it
    // goes along.
    let away = f.as_account(&redirecting, "/away", "{account:main}");
    assert_eq!(away["result"]["status"], 200, "{away}");
    assert!(elsewhere.got().iter().all(|g| !g.headers.contains_key("authorization")), "the other origin got no token");
    assert!(sent_token(&f.as_account(&server, "/home", "{account:main}"), "/here", 1), "lost within its origin");
}

#[test]
fn signing_out_revokes_the_tokens_and_forgets_them() {
    let state = Arc::new(Mutex::new(Service { expires_in: Some(3600), ..Service::default() }));
    let server = service(state.clone());
    let f = fixture(&with_account(&server.url), &[], Options::default());
    f.sign_in(&state).unwrap();
    let out = f.plugins.sign_out("echo", "main", &f.store).unwrap();
    assert_eq!((out.was_signed_in, out.revoked, out.status.signed_in), (true, true, false));
    let revoked = form(server.got().last().unwrap());
    assert!(has(&revoked, "token", "rt-1") && has(&revoked, "token_type_hint", "refresh_token"), "not revoked");
    let e = f.as_account(&server, "/me", "{account:main}");
    assert_eq!(
        e["error"]["message"],
        "its Mock account isn't signed in (locallibrary plugin account echo main sign-in)"
    );
    let again = f.plugins.sign_out("echo", "main", &f.store).unwrap();
    assert_eq!((again.was_signed_in, again.revoked), (false, false));
    assert!(!f.seen(&[]).contains("rt-1"));
    let secrets = std::fs::read_to_string(f.store.root().join("secrets/plugins/echo.json")).unwrap_or_default();
    assert!(!secrets.contains("at-1"), "forgotten here too");
}

/// A kept sign-in that can't be read says to sign in again, and both signing in again and signing
/// out (with nothing to revoke) put it right.
#[test]
fn a_sign_in_that_cant_be_read_is_signed_in_again_or_out() {
    let state = Arc::new(Mutex::new(Service { expires_in: Some(3600), ..Service::default() }));
    let server = service(state.clone());
    let f = fixture(&with_account(&server.url), &[], Options::default());
    let garble = || {
        let file = f.store.root().join("secrets/plugins/echo.json");
        std::fs::write(&file, json!({ "account:main": "not json" }).to_string()).unwrap();
    };
    f.sign_in(&state).unwrap();
    garble();
    let e = f.plugins.account("echo", "main", Some(&f.store)).unwrap_err();
    assert_eq!(e.to_string(), "the main account's sign-in can't be read: sign in again");
    assert!(f.sign_in(&state).unwrap().signed_in);
    garble();
    let out = f.plugins.sign_out("echo", "main", &f.store).unwrap();
    assert_eq!((out.was_signed_in, out.revoked, out.status.signed_in), (true, false, false));
    assert!(!f.plugins.account("echo", "main", Some(&f.store)).unwrap().signed_in);
}

#[test]
fn a_declined_or_foreign_sign_in_keeps_nothing() {
    let state = Arc::new(Mutex::new(Service { expires_in: Some(3600), ..Service::default() }));
    let server = service(state.clone());
    let f = fixture(&with_account(&server.url), &[], Options::default());
    let start = f.plugins.begin_sign_in("echo", "main", &f.store).unwrap();
    let declined = format!("error=access_denied&state={}", opened(&start)["state"]);
    let browser = come_back(&start, Some(declined));
    let e = f.plugins.finish_sign_in(&start.id, Duration::from_secs(10), &Ctx::silent()).unwrap_err();
    assert_eq!(e.to_string(), "the sign-in was declined, so locallibrary wasn't given access");
    assert!(browser.join().unwrap().contains("locallibrary couldn't sign you in"));
    assert!(!server.got().iter().any(|g| g.target == "/token"), "a declined sign-in trades no code");
    assert!(!f.plugins.account("echo", "main", Some(&f.store)).unwrap().signed_in);

    // Pasted from another computer: only the address of this sign-in.
    let start = f.plugins.begin_sign_in("echo", "main", &f.store).unwrap();
    state.lock().unwrap().challenge = opened(&start)["code_challenge"].clone();
    let foreign = format!("{}?code=c0de&state=guess", start.redirect_uri);
    let e = f.plugins.finish_sign_in_with_url(&start.id, &foreign).unwrap_err();
    assert_eq!(e.to_string(), "that address is from another sign-in: sign in again");
    let start = f.plugins.begin_sign_in("echo", "main", &f.store).unwrap();
    state.lock().unwrap().challenge = opened(&start)["code_challenge"].clone();
    let pasted = format!("{}?code=c0de&state={}", start.redirect_uri, opened(&start)["state"]);
    assert!(f.plugins.finish_sign_in_with_url(&start.id, &pasted).unwrap().signed_in);
    assert!(f.plugins.cancel_sign_in(&f.plugins.begin_sign_in("echo", "main", &f.store).unwrap().id));
    assert!(!f.plugins.cancel_sign_in("nope"));
    // The service refusing the code says why.
    f.plugins.sign_out("echo", "main", &f.store).unwrap();
    state.lock().unwrap().refuses = Some(("invalid_grant".into(), "the code\nexpired".into()));
    let e = f.sign_in(&state).unwrap_err();
    assert_eq!(e.to_string(), "signing in to Mock: invalid_grant: the code expired");
}

#[test]
fn an_account_is_only_reached_at_a_public_https_address_outside_tests() {
    let state = Arc::new(Mutex::new(Service::default()));
    let server = service(state);
    let manifest = with_account(&server.url);
    let f = Fixture::new(
        move |dir| vec![spec(dir, &manifest, &[])],
        Options { secrets: locallibrary::keychain::Storage::File, ..Options::default() },
    );
    let e = f.plugins.begin_sign_in("echo", "main", &f.store).unwrap_err().to_string();
    assert!(e.starts_with("the Echo plugin failed: its Mock account's sign-in page can't be opened: "), "{e}");
    assert!(e.ends_with("isn't an https address"), "{e}");
    let e = f.plugins.begin_sign_in("echo", "other", &f.store).unwrap_err();
    assert_eq!(e.to_string(), "not found: the echo plugin's account \"other\"");
}

/// How a step of [`a_token_goes_only_where_the_account_is_named`] names the account.
#[derive(Debug, Clone, Copy)]
enum Naming {
    /// As the whole `Authorization` header.
    Header,
    /// In another header, a query or the body: refused.
    Elsewhere,
    /// An account it doesn't have: refused.
    Unknown,
}

/// For tokens of any lifetime, refreshed with a new refresh token or not, and any requests: one
/// naming the account as its whole `Authorization` header reaches the server with the access
/// token the service gave last, refreshed first exactly when it's within a minute of expiring;
/// one naming it anywhere else, or naming another account, is refused before anything is sent;
/// and no token gets into the plugin's log, its answers or the store but its secrets.
#[hegel::test(test_cases = 6, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn a_token_goes_only_where_the_account_is_named(tc: TestCase) {
    let expires_in = tc.draw(gs::sampled_from(vec![None, Some(5), Some(59), Some(61), Some(3600)]));
    let rotates = tc.draw(gs::booleans());
    let state = Arc::new(Mutex::new(Service { expires_in, rotates, ..Service::default() }));
    let server = service(state.clone());
    let f = fixture(&with_account(&server.url), &[], Options::default());
    f.sign_in(&state).unwrap();
    let expiring = expires_in.is_some_and(|s| s <= 60);
    let (mut current, mut answers) = (1, Vec::new());
    let steps =
        tc.draw(gs::vecs(gs::sampled_from(vec![Naming::Header, Naming::Elsewhere, Naming::Unknown])).max_size(5));
    for (i, step) in steps.into_iter().enumerate() {
        let path = format!("/s{i}");
        let sent = server.got().len();
        let answer = match step {
            Naming::Header => f.as_account(&server, &path, "{account:main}"),
            Naming::Unknown => f.as_account(&server, &path, "{account:nope}"),
            Naming::Elsewhere => {
                let url = format!("{}{path}", server.url);
                let fetch = match tc.draw(gs::integers::<u8>().max_value(2)) {
                    0 => json!({ "url": url, "headers": { "X-Account": "{account:main}" } }),
                    1 => json!({ "url": format!("{url}?a={{account:main}}") }),
                    _ => json!({ "url": url, "method": "PUT", "body": "{account:main}" }),
                };
                f.fetch(fetch)
            }
        };
        match step {
            Naming::Header => {
                current += usize::from(expiring);
                assert!(sent_token(&answer, &path, current), "step {i}: not the token the service gave last");
            }
            _ => {
                assert!(answer["error"]["data"]["kind"] == "invalid", "step {i}: not refused");
                assert_eq!(server.got().len(), sent, "step {i}: something was sent");
            }
        }
        answers.push(answer["error"].clone());
    }
    assert_eq!(state.lock().unwrap().issued, current, "refreshed when it needn't be, or not when it had to");
    let tokens: Vec<String> = (1..=current).flat_map(|n| [format!("at-{n}"), format!("rt-{n}")]).collect();
    let seen = f.seen(&answers.iter().collect::<Vec<_>>());
    assert!(tokens.iter().all(|t| !seen.contains(t.as_str())), "a token is out where it may not be");
}

/// A step of [`an_account_is_signed_in_exactly_when_its_last_sign_in_finished`].
#[derive(Debug, Clone, Copy)]
enum Signing {
    /// Signed in, the browser coming back to this computer.
    Browser,
    /// Signed in with the address the browser ended up on, pasted.
    Pasted,
    /// An address from another sign-in pasted.
    Foreign,
    /// The user declined in the browser.
    Declined,
    /// The service refuses the code.
    Refused,
    /// The service answers what can't be read.
    Garbled,
    /// A sign-in begun, then cancelled.
    Cancelled,
    SignOut,
}

/// For any order of sign-ins (finished in the browser or pasted, declined, from another sign-in,
/// refused or misanswered by the service, cancelled) and sign-outs, of an account whose service
/// can revoke tokens or not: the account is signed in exactly when a sign-in finished since it was
/// last signed out, each failed one saying why and keeping what was there; signing out says
/// whether it was signed in and whether the service was told (exactly when it was signed in and
/// can be), and a sign-in is finished or cancelled once. Listing the plugin's accounts says what
/// asking for the one does, and no token gets into the plugin's log, the answers or the store but
/// its secrets.
#[hegel::test(test_cases = 8, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn an_account_is_signed_in_exactly_when_its_last_sign_in_finished(tc: TestCase) {
    let state = Arc::new(Mutex::new(Service { expires_in: Some(3600), ..Service::default() }));
    let server = service(state.clone());
    let revokes = tc.draw(gs::booleans());
    let mut manifest = with_account(&server.url);
    if !revokes {
        manifest = manifest.replace(&format!("revoke_url = \"{}/revoke\"\n", server.url), "");
    }
    let f = fixture(&manifest, &[], Options::default());
    let steps = tc.draw(
        gs::vecs(gs::sampled_from(vec![
            Signing::Browser,
            Signing::Pasted,
            Signing::Foreign,
            Signing::Declined,
            Signing::Refused,
            Signing::Garbled,
            Signing::Cancelled,
            Signing::SignOut,
        ]))
        .max_size(6),
    );
    let mut signed_in = false;
    let mut answers = Vec::new();
    for (i, step) in steps.into_iter().enumerate() {
        let failed = match step {
            Signing::Browser => {
                answers.push(shown(&f.sign_in(&state).unwrap()));
                signed_in = true;
                None
            }
            Signing::Pasted => {
                let start = f.plugins.begin_sign_in("echo", "main", &f.store).unwrap();
                state.lock().unwrap().challenge = opened(&start)["code_challenge"].clone();
                let pasted = format!("{}?code=c0de&state={}", start.redirect_uri, opened(&start)["state"]);
                answers.push(shown(&f.plugins.finish_sign_in_with_url(&start.id, &pasted).unwrap()));
                assert!(!f.plugins.cancel_sign_in(&start.id), "step {i}: a sign-in finished is over");
                signed_in = true;
                None
            }
            Signing::Foreign => {
                let start = f.plugins.begin_sign_in("echo", "main", &f.store).unwrap();
                let foreign = format!("{}?code=c0de&state=guess", start.redirect_uri);
                let e = f.plugins.finish_sign_in_with_url(&start.id, &foreign).unwrap_err();
                assert!(!f.plugins.cancel_sign_in(&start.id), "step {i}: a sign-in pasted to is over");
                Some((e, "that address is from another sign-in: sign in again"))
            }
            Signing::Declined => {
                let start = f.plugins.begin_sign_in("echo", "main", &f.store).unwrap();
                let browser = come_back(&start, Some(format!("error=access_denied&state={}", opened(&start)["state"])));
                let e = f.plugins.finish_sign_in(&start.id, Duration::from_secs(10), &Ctx::silent()).unwrap_err();
                assert!(browser.join().unwrap().contains("locallibrary couldn't sign you in"), "step {i}");
                Some((e, "the sign-in was declined, so locallibrary wasn't given access"))
            }
            Signing::Refused | Signing::Garbled => {
                let refused = matches!(step, Signing::Refused);
                {
                    let mut s = state.lock().unwrap();
                    s.refuses = refused.then(|| ("invalid_grant".into(), "the code\nexpired".into()));
                    s.garbles = !refused;
                }
                let e = f.sign_in(&state).unwrap_err();
                {
                    let mut s = state.lock().unwrap();
                    (s.refuses, s.garbles) = (None, false);
                }
                match refused {
                    true => Some((e, "signing in to Mock: invalid_grant: the code expired")),
                    false => Some((e, "signing in to Mock: its answer couldn't be read")),
                }
            }
            Signing::Cancelled => {
                let start = f.plugins.begin_sign_in("echo", "main", &f.store).unwrap();
                assert!(f.plugins.cancel_sign_in(&start.id), "step {i}: not waiting");
                let e = f.plugins.finish_sign_in(&start.id, Duration::from_secs(1), &Ctx::silent()).unwrap_err();
                assert_eq!(e.code(), "not_found", "step {i}");
                None
            }
            Signing::SignOut => {
                let out = f.plugins.sign_out("echo", "main", &f.store).unwrap();
                let said = (out.was_signed_in, out.revoked, out.status.signed_in);
                assert_eq!(said, (signed_in, signed_in && revokes, false), "step {i}");
                answers.push(serde_json::to_value(&out).unwrap());
                signed_in = false;
                None
            }
        };
        if let Some((e, said)) = failed {
            assert_eq!(e.to_string(), said, "step {i}");
        }
        let status = f.plugins.account("echo", "main", Some(&f.store)).unwrap();
        assert_eq!((status.signed_in, status.refreshes), (signed_in, signed_in), "step {i}");
        assert_eq!(f.plugins.accounts("echo", Some(&f.store)).unwrap(), vec![status.clone()], "step {i}");
        answers.push(shown(&status));
    }
    let issued = state.lock().unwrap().issued;
    let given: Vec<String> = (1..=issued).flat_map(|n| [format!("at-{n}"), format!("rt-{n}")]).collect();
    let seen = f.seen(&answers.iter().collect::<Vec<_>>());
    assert!(given.iter().all(|t| !seen.contains(t.as_str())), "what the service gave is out where it may not be");
}
