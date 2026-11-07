//! Signing in to an account of a type through a mock OAuth service on this computer (the test
//! plays the browser): the authorize page it opens, with PKCE and the way back to this computer
//! (at the port the type names, when it names one); the code and refresh tokens traded at the
//! type's token endpoint or at Local Network's token exchange; sign-ins declined, pasted, from
//! another sign-in, cancelled or refused, which keep what was there; and only at public https
//! addresses outside tests. And a token in a plugin's request: only where its type says it goes,
//! never in an error, a log or on to another origin. Keeping the accounts is
//! [`super::shared_accounts`]'s, and plugins' accesses to them [`super::accesses`]'.

use std::collections::VecDeque;
use std::net::TcpListener;
use std::sync::OnceLock;
use std::time::Duration;

use hegel::TestCase;
use hegel::generators as gs;
use indexmap::IndexMap;
use locallibrary::keychain::Storage;
use locallibrary::plugin::{AccountSignIn, AccountSignedIn};

use super::net::{Reply, Server, ok};
use super::shared_accounts::{
    Damage, Mock, Snapshot, WITH, asking, edit_json, keys_plugin, mock_among, mock_for, mock_with, options, who,
};
use super::*;
use crate::mock_oauth::{EXCHANGE, account_plugin, fields, json_reply};
pub(super) use crate::mock_oauth::{Service, form, service};

/// The query of the address a sign-in opens in the browser (`url`), by name.
pub(super) fn opened_at(url: &str) -> IndexMap<String, String> {
    reqwest::Url::parse(url).unwrap().query_pairs().into_owned().collect()
}

/// Where the browser comes back to sign-in `start`.
pub(super) fn back(start: &AccountSignIn) -> &str {
    start.redirect_uri.as_deref().expect("a sign-in in the browser is come back to")
}

/// Comes back from the browser to `redirect_uri` with `query`, once it's waited for.
pub(super) fn come_back_to(redirect_uri: &str, query: String) -> std::thread::JoinHandle<String> {
    let url = format!("{redirect_uri}?{query}");
    std::thread::spawn(move || reqwest::blocking::get(url).unwrap().text().unwrap())
}

impl Fixture {
    /// Everything here that might hold a token: the plugins' logs, their answers so far, and every
    /// file in the store but its secrets.
    pub(super) fn seen(&self, answers: &[&Value]) -> String {
        let logs: Vec<String> = self.logs.lock().unwrap().iter().map(|(_, l)| l.clone()).collect();
        let files = walkdir::WalkDir::new(self.store.root()).into_iter().flatten().filter(|e| e.file_type().is_file());
        let secrets = self.store.root().join("secrets");
        let files = files.filter(|e| !e.path().starts_with(&secrets)).map(|e| std::fs::read(e.path()).unwrap());
        let files: Vec<String> = files.map(|b| String::from_utf8_lossy(&b).into_owned()).collect();
        let answers: Vec<String> = answers.iter().map(|a| a.to_string()).collect();
        [logs, files, answers].concat().join("\n")
    }
}

/// Whether a request's form has `value` as `key` (a token, never printed).
pub(super) fn has(form: &IndexMap<String, String>, key: &str, value: &str) -> bool {
    form.get(key).is_some_and(|v| v == value)
}

impl Mock {
    /// Starts a sign-in to a mock account, for no plugin.
    fn begin(&self) -> AccountSignIn {
        self.f.plugins.begin_account_sign_in("mock", WITH, None, None, &[], &self.f.store).unwrap()
    }

    /// The service will check the code challenge `start` sent the browser with, and say `who`
    /// signed in.
    fn expect(&self, start: &AccountSignIn, who: Value) {
        let mut s = self.state.lock().unwrap();
        (s.challenge, s.signing_in_as) = (opened_at(&start.url)["code_challenge"].clone(), who);
    }

    /// The address the browser ends up on after `start`, pasted, as `a`.
    fn paste(&self, start: &AccountSignIn) -> locallibrary::Result<AccountSignedIn> {
        self.expect(start, who(json!("a"), None));
        let pasted = format!("{}?code=c0de&state={}", back(start), opened_at(&start.url)["state"]);
        self.f.plugins.finish_account_sign_in_with_url(&start.id, &pasted)
    }

    /// The browser coming back to `start` declined: what finishing it says, and what the browser
    /// was told.
    fn decline(&self, start: &AccountSignIn) -> (Error, String) {
        let declined = format!("error=access_denied&state={}", opened_at(&start.url)["state"]);
        let browser = come_back_to(back(start), declined);
        let e = self.f.plugins.finish_account_sign_in(&start.id, Duration::from_secs(10), &Ctx::silent()).unwrap_err();
        (e, browser.join().unwrap())
    }
}

/// The account plugin `manifest` of the service at `server`, its tokens traded at Local Network's
/// token exchange (the service's own, at `EXCHANGE`) in place of its token endpoint.
fn exchanging(server: &str, manifest: String) -> String {
    manifest.replace(&format!("token_url = \"{server}/token\""), &format!("token_exchange = \"{server}{EXCHANGE}\""))
}

/// Where the service says `a` signed in, whatever it's asked with: for an account type whose
/// token goes where the mock's `/userinfo` doesn't look.
fn anyone() -> String {
    Server::new(|_, _| json_reply(200, json!({ "sub": "a" }))).url
}

/// [`Mock`], the account type's token going where `token` says, with a scope `there` besides,
/// asking the service for nothing and covering GETs to each of the servers `there`; the echo
/// plugin needs `read` and `there`. The service says `a` signed in.
fn reaching(token: &str, there: &[&str]) -> Mock {
    let requests: Vec<String> = there.iter().map(|url| format!("{{ method = \"GET\", url = \"{url}/*\" }}")).collect();
    let scope =
        format!("[contributes.account_types.scopes.there]\ntitle = \"there\"\nrequests = [{}]\n", requests.join(", "));
    let whoami = anyone();
    let manifest = move |server: &str| format!("{}\n{scope}", account_plugin(server, &whoami, token));
    mock_with(3600, manifest, &asking(&["read", "there"], &[]), options())
}

/// A sign-in opens the type's authorize page asking for a code with PKCE, the browser coming back
/// to this computer; the code is traded at the token endpoint, the browser told who signed in, and
/// a sign-in is finished once.
#[test]
fn a_sign_in_trades_the_code_the_browser_comes_back_with_once() {
    let m = mock_for(3600, "/userinfo", "", &asking(&["read"], &[]));
    let start = m.begin();
    let query = opened_at(&start.url);
    assert!(start.url.starts_with(&format!("{}/authorize?", m.server.url)), "{}", start.url);
    for (key, value) in [
        ("response_type", "code"),
        ("client_id", "locallibrary-test"),
        ("code_challenge_method", "S256"),
        ("redirect_uri", back(&start)),
    ] {
        assert_eq!(query.get(key).map(String::as_str), Some(value), "{key}");
    }
    assert!(back(&start).starts_with("http://127.0.0.1:"), "{}", back(&start));
    m.expect(&start, who(json!("a"), Some("a@mock.example")));
    let browser = come_back_to(back(&start), format!("code=c0de&state={}", query["state"]));
    let done = m.f.plugins.finish_account_sign_in(&start.id, Duration::from_secs(10), &Ctx::silent()).unwrap();
    assert!(browser.join().unwrap().contains("a@mock.example is signed in."));
    assert_eq!((done.account.signed_in, done.account.refreshes), (true, true));
    assert!(done.account.expires.is_some_and(|at| at > chrono::Utc::now() + chrono::Duration::minutes(59)));
    let traded = form(&m.server.got()[0]);
    assert!(has(&traded, "grant_type", "authorization_code") && has(&traded, "code", "c0de"), "the code is traded");
    assert_eq!(traded["redirect_uri"], back(&start));
    let again = m.f.plugins.finish_account_sign_in(&start.id, Duration::from_secs(1), &Ctx::silent()).unwrap_err();
    assert_eq!(again.code(), "not_found");
}

/// An account type naming Local Network's token exchange trades its code and refresh tokens there,
/// as JSON (a form posted without an `Origin` is refused there), never at a token endpoint; and
/// what the exchange answers when it can't is what the plugin is told, the account still signed
/// in.
#[test]
fn an_account_type_naming_the_token_exchange_signs_in_and_refreshes_there() {
    let manifest = |server: &str| exchanging(server, account_plugin(server, &format!("{server}/userinfo"), ""));
    let m = mock_with(30, manifest, &asking(&["read"], &[]), options());
    assert!(m.sign_in(who(json!("a"), None), None, &[]).0.unwrap().account.refreshes);
    let traded = m.server.got()[0].clone();
    assert_eq!((traded.method.as_str(), traded.target.as_str()), ("POST", EXCHANGE));
    let sent = fields(&traded);
    assert!(has(&sent, "grant_type", "authorization_code") && has(&sent, "code", "c0de"), "the code isn't traded");
    assert!(sent["redirect_uri"].starts_with("http://127.0.0.1:") && sent.contains_key("code_verifier"));

    let answer = m.fetch_as("GET", "/r/a", "{account:mock}", None);
    assert_eq!(answer["result"]["body"], "/r/a Bearer at-2", "not refreshed");
    let refreshed = fields(&m.server.got().iter().rev().nth(1).unwrap().clone());
    assert!(has(&refreshed, "grant_type", "refresh_token") && has(&refreshed, "refresh_token", "rt-1"), "refreshed");
    assert!(m.server.got().iter().all(|g| g.target != "/token"), "the service's token endpoint was asked");

    let unset = "Local Network isn't set up to sign in to Mock yet";
    m.state.lock().unwrap().refuses = Some(("temporarily_unavailable".into(), unset.into()));
    let e = m.fetch_as("GET", "/r/b", "{account:mock}", None);
    assert_eq!(e["error"]["message"], format!("signing in to Mock: temporarily_unavailable: {unset}"));
    assert!(m.listed()[0].signed_in, "signed out for the exchange");
}

/// A declined sign-in trades no code and keeps nothing; an address pasted from another computer
/// signs in only when it's this sign-in's; a sign-in waiting can be cancelled; and the service
/// refusing the code says why.
#[test]
fn a_declined_or_foreign_sign_in_keeps_nothing() {
    let m = mock_for(3600, "/userinfo", "", &asking(&["read"], &[]));
    let (e, told) = m.decline(&m.begin());
    assert_eq!(e.to_string(), "the sign-in was declined, so locallibrary wasn't given access");
    assert!(told.contains("locallibrary couldn't sign you in"));
    assert!(!m.server.got().iter().any(|g| g.target == "/token"), "a declined sign-in trades no code");
    assert!(m.listed().is_empty());

    let start = m.begin();
    let foreign = format!("{}?code=c0de&state=guess", back(&start));
    let e = m.f.plugins.finish_account_sign_in_with_url(&start.id, &foreign).unwrap_err();
    assert_eq!(e.to_string(), "that address is from another sign-in: sign in again");
    assert!(m.paste(&m.begin()).unwrap().account.signed_in);
    assert!(m.f.plugins.cancel_sign_in(&m.begin().id));
    assert!(!m.f.plugins.cancel_sign_in("nope"));

    m.state.lock().unwrap().refuses = Some(("invalid_grant".into(), "the code\nexpired".into()));
    let e = m.sign_in(who(json!("b"), None), None, &[]).0.unwrap_err();
    assert_eq!(e.to_string(), "signing in to Mock: invalid_grant: the code expired");
    assert_eq!(m.listed().len(), 1);
}

/// Outside tests, an account type's service is reached only at public https addresses; and an
/// account plugin signs in only to the type it provides.
#[test]
fn an_account_type_is_only_reached_at_a_public_https_address_outside_tests() {
    let manifest = |server: &str| account_plugin(server, &format!("{server}/userinfo"), "");
    let outside = Options { secrets: Storage::File, ..Options::default() };
    let m = mock_with(3600, manifest, &asking(&["read"], &[]), outside);
    let begin = |kind: &str| m.f.plugins.begin_account_sign_in(kind, WITH, None, None, &[], &m.f.store);
    let e = begin("mock").unwrap_err().to_string();
    assert!(e.starts_with("the Mock accounts plugin failed: its Mock account's sign-in page can't be opened: "), "{e}");
    assert!(e.ends_with("isn't an https address"), "{e}");
    assert_eq!(begin("nope").unwrap_err().to_string(), "not found: the mock-accounts plugin's account type \"nope\"");
}

/// For an account named by its email or its subject, a kept sign-in that can't be read says to
/// sign in again, naming it, and both signing in again and signing out (with nothing to revoke)
/// put it right.
#[hegel::test(test_cases = 2, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn prop_a_sign_in_that_cant_be_read_is_signed_in_again_or_out(tc: TestCase) {
    let email = tc.draw(gs::sampled_from(vec![Some("a@mock.example"), None]));
    let m = mock_for(3600, "/userinfo", "", &asking(&["read"], &[]));
    let garble = || {
        let file = m.f.store.root().join("secrets/accounts.json");
        std::fs::write(&file, format!("{{\"account:{WITH}:a\": \"not json\"}}")).unwrap();
    };
    let a = || who(json!("a"), email);
    m.sign_in(a(), None, &[]).0.unwrap();
    garble();
    let e = m.fetch_as("GET", "/r/x", "{account:mock}", None);
    let name = email.unwrap_or("a");
    assert_eq!(e["error"]["message"], format!("the {name} account's sign-in can't be read: sign in again"));
    m.sign_in(a(), Some("a"), &[]).0.unwrap();
    assert_eq!(m.fetch_as("GET", "/r/x", "{account:mock}", None)["result"]["body"], "/r/x Bearer at-2");
    garble();
    let out = m.f.plugins.sign_out_account(WITH, "a", &m.f.store).unwrap();
    assert_eq!((out.revoked, out.account.signed_in), (false, true));
    assert!(m.listed().is_empty());
}

/// A token goes only where its account type says (the `Authorization` header, after `Bearer`), and
/// never on to another origin a redirect leads to, even one a scope of the access covers; a request
/// naming it anywhere else, or naming a type the plugin doesn't ask for, is refused before anything
/// is sent, and a server it isn't granted gets nothing.
#[test]
fn an_account_goes_only_where_its_type_says_and_never_to_another_origin() {
    let elsewhere = Server::new(|_, got| ok(got.headers.get("authorization").cloned().unwrap_or_default()));
    let to = elsewhere.url.clone();
    let redirecting = Server::new(move |_, _| Reply {
        status: 302,
        headers: vec![("Location", format!("{to}/there"))],
        body: Vec::new(),
    });
    let m = reaching("", &[&redirecting.url, &elsewhere.url]);
    m.sign_in(who(json!("a"), None), None, &[]).0.unwrap();
    let url = format!("{}/r/x", m.server.url);
    let sent = m.server.got().len();
    let refused = "an account goes only in the Authorization header: {account:<type>}";
    for fetch in [
        json!({ "url": url, "headers": { "X-Token": "{account:mock}" } }),
        json!({ "url": url, "headers": { "Authorization": "Bearer {account:mock}" } }),
        json!({ "url": format!("{url}?t={{account:mock}}") }),
        json!({ "url": url, "method": "POST", "body": "t={account:mock}" }),
    ] {
        assert_eq!(m.f.fetch(fetch.clone())["error"]["message"], refused, "{fetch}");
    }
    let e = m.fetch_as("GET", "/r/x", "{account:other}", None);
    assert_eq!(e["error"]["message"], "\"other\" isn't an account type it asks for");
    assert_eq!(m.server.got().len(), sent, "something was sent");
    let e = m.f.fetch(json!({ "url": "https://example.com/", "headers": { "Authorization": "{account:mock}" } }));
    assert_eq!(e["error"]["data"]["kind"], "denied", "{e}");
    let away = format!("{}/away", redirecting.url);
    let answer = m.f.fetch(json!({ "url": away, "headers": { "Authorization": "{account:mock}" } }));
    assert_eq!(answer["result"]["status"], 200, "{answer}");
    assert!(redirecting.got()[0].headers.contains_key("authorization"), "the first origin got no token");
    assert!(elsewhere.got().iter().all(|g| !g.headers.contains_key("authorization")), "the other origin got one");
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
/// refused or misanswered by the service, cancelled) and sign-outs, of an account whose type trades
/// its tokens at a token endpoint or at Local Network's token exchange, and can revoke them or not:
/// the account is kept exactly when a sign-in finished since it was last signed out, each failed
/// one saying why and keeping what was there; signing out says whether the service was told
/// (exactly when it can be), and is refused once it's signed out; and a sign-in is finished or
/// cancelled once. No token gets into the plugins' logs, the answers or the store
/// but its secrets.
#[hegel::test(test_cases = 8, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn an_account_is_signed_in_exactly_when_its_last_sign_in_finished(tc: TestCase) {
    let (revokes, exchanges) = (tc.draw(gs::booleans()), tc.draw(gs::booleans()));
    let manifest = |server: &str| {
        let mut manifest = account_plugin(server, &format!("{server}/userinfo"), "");
        if !revokes {
            manifest = manifest.replace(&format!("revoke_url = \"{server}/revoke\"\n"), "");
        }
        if exchanges {
            manifest = exchanging(server, manifest);
        }
        manifest
    };
    let m = mock_with(3600, manifest, &asking(&["read"], &[]), options());
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
                answers.push(serde_json::to_value(m.sign_in(who(json!("a"), None), None, &[]).0.unwrap()).unwrap());
                signed_in = true;
                None
            }
            Signing::Pasted => {
                let start = m.begin();
                answers.push(serde_json::to_value(m.paste(&start).unwrap()).unwrap());
                assert!(!m.f.plugins.cancel_sign_in(&start.id), "step {i}: a sign-in finished is over");
                signed_in = true;
                None
            }
            Signing::Foreign => {
                let start = m.begin();
                let foreign = format!("{}?code=c0de&state=guess", back(&start));
                let e = m.f.plugins.finish_account_sign_in_with_url(&start.id, &foreign).unwrap_err();
                assert!(!m.f.plugins.cancel_sign_in(&start.id), "step {i}: a sign-in pasted to is over");
                Some((e, "that address is from another sign-in: sign in again"))
            }
            Signing::Declined => {
                let (e, told) = m.decline(&m.begin());
                assert!(told.contains("locallibrary couldn't sign you in"), "step {i}");
                Some((e, "the sign-in was declined, so locallibrary wasn't given access"))
            }
            Signing::Refused | Signing::Garbled => {
                let refused = matches!(step, Signing::Refused);
                {
                    let mut s = m.state.lock().unwrap();
                    s.refuses = refused.then(|| ("invalid_grant".into(), "the code\nexpired".into()));
                    s.garbles = !refused;
                }
                let e = m.sign_in(who(json!("a"), None), None, &[]).0.unwrap_err();
                {
                    let mut s = m.state.lock().unwrap();
                    (s.refuses, s.garbles) = (None, false);
                }
                match refused {
                    true => Some((e, "signing in to Mock: invalid_grant: the code expired")),
                    false => Some((e, "signing in to Mock: its answer couldn't be read")),
                }
            }
            Signing::Cancelled => {
                let start = m.begin();
                assert!(m.f.plugins.cancel_sign_in(&start.id), "step {i}: not waiting");
                let e = m.f.plugins.finish_account_sign_in(&start.id, Duration::from_secs(1), &Ctx::silent());
                assert_eq!(e.unwrap_err().code(), "not_found", "step {i}");
                None
            }
            Signing::SignOut => {
                let out = m.f.plugins.sign_out_account(WITH, "a", &m.f.store);
                match signed_in {
                    true => {
                        let out = out.unwrap();
                        assert_eq!((out.revoked, out.account.signed_in), (revokes, true), "step {i}");
                        answers.push(serde_json::to_value(&out).unwrap());
                    }
                    false => assert_eq!(out.unwrap_err().code(), "not_found", "step {i}"),
                }
                signed_in = false;
                None
            }
        };
        if let Some((e, said)) = failed {
            assert_eq!(e.to_string(), said, "step {i}");
        }
        let listed = m.listed();
        let said: Vec<(&str, bool, bool)> =
            listed.iter().map(|a| (a.subject.as_str(), a.signed_in, a.refreshes)).collect();
        assert_eq!(said, if signed_in { vec![("a", true, true)] } else { Vec::new() }, "step {i}");
        answers.push(serde_json::to_value(&listed).unwrap());
    }
    assert!(!m.leaked(&answers), "what the service gave is out where it may not be");
}

/// Where a step of [`a_token_lands_exactly_where_its_account_type_says`] names the account, as
/// the whole value.
#[derive(Debug, Clone, Copy)]
enum Named {
    Authorization,
    /// Another header.
    Header,
    /// The query parameter.
    Query(&'static str),
    Body,
}

/// For an account type whose token goes in the `Authorization` header (after `Bearer` when it
/// doesn't say, or the scheme it names) or as a query parameter, and any requests naming the
/// account anywhere: one naming it where the type says reaches the server with the token there and
/// nowhere else in the request (its address, its other headers, its body), and a token sent in the
/// query is answered to the plugin as the placeholder that named it; one naming it anywhere else is
/// refused, saying where it goes, before anything is sent; and no token gets into the plugins' logs
/// or the store but its secrets.
#[hegel::test(test_cases = 16, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn a_token_lands_exactly_where_its_account_type_says(tc: TestCase) {
    let token = tc.draw(gs::sampled_from(vec![
        "",
        "token = { scheme = \"Bearer\" }",
        "token = { scheme = \"OAuth\" }",
        "token = { query = \"access_token\" }",
        "token = { query = \"oauth_token\" }",
    ]));
    let whoami = anyone();
    let m = mock_with(3600, |server| account_plugin(server, &whoami, token), &asking(&["read"], &[]), options());
    m.sign_in(who(json!("a"), None), None, &[]).0.unwrap();
    let scheme = match token {
        "" => Some("Bearer"),
        t => t.strip_prefix("token = { scheme = \"").and_then(|t| t.strip_suffix("\" }")),
    };
    let query = token.strip_prefix("token = { query = \"").and_then(|t| t.strip_suffix("\" }"));
    let named = [Named::Authorization, Named::Header, Named::Query("access_token"), Named::Query("oauth_token")];
    let steps = tc.draw(gs::vecs(gs::sampled_from([named.to_vec(), vec![Named::Body]].concat())).max_size(4));
    let mut answers = Vec::new();
    for (i, step) in steps.into_iter().enumerate() {
        let (path, sent) = (format!("/r/s{i}?page=2"), m.server.got().len());
        let url = format!("{}{path}", m.server.url);
        let answer = m.f.fetch(match step {
            Named::Authorization => {
                json!({ "url": url, "headers": { "Authorization": "{account:mock}", "X-Other": "x" } })
            }
            Named::Header => json!({ "url": url, "headers": { "X-Token": "{account:mock}" } }),
            Named::Query(name) => json!({ "url": format!("{url}&{name}={{account:mock}}") }),
            Named::Body => json!({ "url": url, "method": "POST", "body": "{account:mock}" }),
        });
        let lands = match step {
            Named::Authorization => scheme.is_some(),
            Named::Query(name) => query == Some(name),
            _ => false,
        };
        if !lands {
            let said = match query {
                Some(name) => format!("this account goes only in the query: {name}={{account:<type>}}"),
                None => "an account goes only in the Authorization header: {account:<type>}".into(),
            };
            assert_eq!(answer["error"]["message"], said, "step {i}: not refused, or not saying where it goes");
            assert_eq!(m.server.got().len(), sent, "step {i}: something was sent");
            answers.push(answer);
            continue;
        }
        let got = m.server.got().last().cloned().unwrap_or_default();
        let (target, authorization) = match (step, scheme) {
            (Named::Query(name), _) => (format!("{path}&{name}=at-1"), None),
            (_, scheme) => (path.clone(), scheme.map(|s| format!("{s} at-1"))),
        };
        assert!(got.target == target, "step {i}: the address isn't what it was named in");
        assert!(got.headers.get("authorization") == authorization.as_ref(), "step {i}: the header isn't");
        let elsewhere =
            got.headers.iter().filter(|(name, _)| *name != "authorization").any(|(_, v)| v.contains("at-1"));
        assert!(!elsewhere && !got.body.contains("at-1"), "step {i}: the token is elsewhere in the request");
        if let Named::Query(name) = step {
            // The server says the address it was asked at back: as the plugin named it.
            assert_eq!(answer["result"]["body"], format!("{path}&{name}={{account:mock}} -"), "step {i}");
            answers.push(answer);
        }
    }
    assert!(!m.leaked(&answers), "a token is out where it may not be");
}

/// A token sent in the query stays out of the errors the plugin is told and its log (a failed
/// request's error names its address), is never carried to another origin a redirect leads to,
/// and comes back in what the server answers as the placeholder that named it, which asks for it
/// again.
#[test]
fn a_token_in_the_query_stays_out_of_errors_logs_and_other_origins() {
    let closed = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let closed = format!("http://127.0.0.1:{closed}");
    let elsewhere = Server::new(|_, _| ok("there"));
    let to = elsewhere.url.clone();
    let echoing = Server::new(move |_, got| {
        let query = got.target.split_once('?').map(|(_, q)| q).unwrap_or_default().to_string();
        Reply { status: 302, headers: vec![("Location", format!("{to}/there?{query}"))], body: Vec::new() }
    });
    let paging = Server::new(|base, got| {
        let next = format!("{base}{}", got.target.replace("page=1", "page=2"));
        let headers = vec![("Link", format!("<{next}>; rel=\"next\""))];
        Reply { status: 200, headers, body: json!({ "next": next }).to_string().into_bytes() }
    });
    let m = reaching("token = { query = \"access_token\" }", &[&closed, &echoing.url, &paging.url]);
    m.sign_in(who(json!("a"), None), None, &[]).0.unwrap();
    let mut answers = Vec::new();

    let e = m.f.fetch(json!({ "url": format!("{closed}/x?access_token={{account:mock}}") }));
    let said = e["error"]["message"].as_str().unwrap_or_default().to_string();
    assert!(said.contains("access_token=<secret>") && !said.contains("at-1"), "the address isn't scrubbed: {e}");
    answers.push(e);

    let e = m.f.fetch(json!({ "url": format!("{}/away?access_token={{account:mock}}", echoing.url) }));
    let said = e["error"]["message"].as_str().unwrap_or_default().to_string();
    assert!(said.ends_with("/there?access_token=<secret>, which a secret in its address isn't for"), "not refused");
    assert!(elsewhere.got().is_empty(), "the other origin was sent the token");
    answers.push(e);

    let first = m.f.fetch(json!({ "url": format!("{}/p?page=1&access_token={{account:mock}}", paging.url) }));
    let next = format!("{}/p?page=2&access_token={{account:mock}}", paging.url);
    assert_eq!(first["result"]["headers"]["link"], format!("<{next}>; rel=\"next\""));
    let body: Value = serde_json::from_str(first["result"]["body"].as_str().unwrap_or_default()).unwrap();
    assert_eq!(body["next"], next);
    let second = m.f.fetch(json!({ "url": next }));
    assert_eq!(second["result"]["status"], 200, "the link given back doesn't ask again");
    assert!(paging.got().iter().all(|g| g.target.ends_with("&access_token=at-1")), "a page went without the token");
    answers.extend([first, second]);
    assert!(!m.leaked(&answers), "a token is out where it may not be");
}

/// For anyone signing in, an account type naming the port the browser comes back to is signed in
/// there, and a sign-in still waiting on it gives way to a new one, while one waiting for the keys
/// of another type keeps waiting; another program holding the port is said.
#[hegel::test(test_cases = 2, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn prop_an_account_type_naming_a_port_is_come_back_to_there(tc: TestCase) {
    let subject = tc.draw(gs::sampled_from(vec![json!("a"), json!(42)]));
    let port = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let manifests = |server: &str| {
        let manifest = account_plugin(server, &format!("{server}/userinfo"), &format!("redirect_port = {port}"));
        vec![manifest, keys_plugin(server, "keyed-accounts", "keyed")]
    };
    let m = mock_among(3600, manifests, &asking(&["read"], &[]), options());
    let keyed = m.f.plugins.begin_account_sign_in("keyed", "keyed-accounts", None, None, &[], &m.f.store).unwrap();
    let waiting = m.begin();
    let redirect_uri = format!("http://127.0.0.1:{port}/callback");
    assert_eq!((back(&waiting), opened_at(&waiting.url)["redirect_uri"].as_str()), (&*redirect_uri, &*redirect_uri));
    assert!(m.sign_in(who(subject, None), None, &[]).0.unwrap().account.signed_in);
    assert_eq!(form(&m.server.got()[0])["redirect_uri"], redirect_uri);
    assert!(!m.f.plugins.cancel_sign_in(&waiting.id), "the sign-in waiting didn't give way");
    assert!(m.f.plugins.cancel_sign_in(&keyed.id), "the sign-in waiting for keys gave way");

    let _held = TcpListener::bind(("127.0.0.1", port)).unwrap();
    let e = m.f.plugins.begin_account_sign_in("mock", WITH, None, None, &[], &m.f.store).unwrap_err().to_string();
    assert!(e.starts_with(&format!("can't listen for the browser on 127.0.0.1:{port}: ")), "{e}");
}

/// For a sign-in for a plugin or not: the browser coming back with neither a code nor an error
/// signs nobody in, and is told why; an address pasted with no sign-in result in it is refused;
/// and a browser that never comes back is given up on. Sign-ins waiting for the browser wait side
/// by side.
#[hegel::test(test_cases = 2, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn prop_a_browser_coming_back_without_a_code_signs_nobody_in(tc: TestCase) {
    let plugin = tc.draw(gs::sampled_from(vec![None, Some("echo")]));
    let m = mock_for(3600, "/userinfo", "", &asking(&["read"], &[]));
    let begin = || m.f.plugins.begin_account_sign_in("mock", WITH, plugin, None, &[], &m.f.store).unwrap();
    let finish = |start: &AccountSignIn, wait: u64| {
        let done = m.f.plugins.finish_account_sign_in(&start.id, Duration::from_secs(wait), &Ctx::silent());
        done.unwrap_err().to_string()
    };
    // Sign-ins waiting for the browser wait side by side.
    let (one, other) = (begin(), begin());
    assert!(m.f.plugins.cancel_sign_in(&one.id) && m.f.plugins.cancel_sign_in(&other.id), "one gave way");
    let start = begin();
    let browser = come_back_to(back(&start), format!("state={}", opened_at(&start.url)["state"]));
    assert_eq!(finish(&start, 10), "the browser came back without a code");
    assert!(browser.join().unwrap().contains("the browser came back without a code"));
    let start = begin();
    let e = m.f.plugins.finish_account_sign_in_with_url(&start.id, "http://127.0.0.1/callback").unwrap_err();
    assert_eq!(e.to_string(), "that address has no sign-in result in it (nothing after `?`)");
    assert_eq!(finish(&begin(), 1), "no answer from the browser within 1 second; sign in again");
    assert!(m.listed().is_empty() && !m.server.got().iter().any(|g| g.target == "/token"));
}

/// For anyone signing in, an address pasted from another computer signs in only when it's this
/// sign-in's, and outside tests a service is reached only at a public https address.
#[hegel::test(test_cases = 2, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn prop_a_pasted_address_signs_in_only_when_its_this_sign_ins(tc: TestCase) {
    let subject = tc.draw(gs::sampled_from(vec!["a", "b"]));
    let m = mock_for(3600, "/userinfo", "", &asking(&["read"], &[]));
    let start = m.begin();
    let foreign = format!("{}?code=c0de&state=guess", back(&start));
    let e = m.f.plugins.finish_account_sign_in_with_url(&start.id, &foreign).unwrap_err();
    assert_eq!(e.to_string(), "that address is from another sign-in: sign in again");
    let start = m.begin();
    m.expect(&start, who(json!(subject), None));
    let pasted = format!("{}?code=c0de&state={}", back(&start), opened_at(&start.url)["state"]);
    let done = m.f.plugins.finish_account_sign_in_with_url(&start.id, &pasted).unwrap();
    assert_eq!(done.account.subject, subject);

    let manifest = |server: &str| account_plugin(server, &format!("{server}/userinfo"), "");
    let outside =
        mock_with(3600, manifest, &asking(&["read"], &[]), Options { secrets: Storage::File, ..Options::default() });
    let e = outside.f.plugins.begin_account_sign_in("mock", WITH, None, None, &[], &outside.f.store).unwrap_err();
    assert!(e.to_string().ends_with("isn't an https address"), "{e}");
}

/// What a scripted server answers next: the store damaged first as `.0` does, when it does.
type Turn = (Option<Box<dyn FnOnce(&Path) + Send>>, Reply);

/// A server answering its turns in order, damaging the store at `root` (once it's given) first
/// when one says; once they've run out, tokens `at-9` and `rt-9` lasting half a minute.
pub(super) struct Scripted {
    pub(super) url: String,
    root: Arc<OnceLock<PathBuf>>,
    turns: Arc<Mutex<VecDeque<Turn>>>,
}

impl Scripted {
    pub(super) fn new() -> Scripted {
        let (root, turns): (Arc<OnceLock<PathBuf>>, Arc<Mutex<VecDeque<Turn>>>) = Default::default();
        let (at, script) = (root.clone(), turns.clone());
        let fresh =
            || json!({ "access_token": "at-9", "token_type": "bearer", "expires_in": 30, "refresh_token": "rt-9" });
        let server = Server::new(move |_, _| {
            let Some((damage, reply)) = script.lock().unwrap().pop_front() else { return json_reply(200, fresh()) };
            if let Some(damage) = damage {
                damage(at.get().unwrap());
            }
            reply
        });
        Scripted { url: server.url, root, turns }
    }

    /// It answers `reply`, damaging the store first as `damage` does.
    pub(super) fn then(&self, damage: Option<Damage>, reply: Reply) {
        let damage = damage.map(|d| Box::new(move |root: &Path| d.to(root)) as Box<dyn FnOnce(&Path) + Send>);
        self.turns.lock().unwrap().push_back((damage, reply));
    }

    /// It damages the store of `m` from now on.
    fn damages(&self, m: &Mock) {
        self.root.set(m.f.store.root().into()).unwrap();
    }
}

/// [`Mock`], its account type trading codes and refresh tokens at `base`'s token endpoint, or
/// at `base`'s Local Network's token exchange when it `exchanges`; whoever it's asked about signed
/// in as `a`.
pub(super) fn trading_at(base: &str, exchanges: bool) -> Mock {
    let whoami = anyone();
    let trades = match exchanges {
        true => format!("token_exchange = \"{base}{EXCHANGE}\""),
        false => format!("token_url = \"{base}/token\""),
    };
    let manifest = move |server: &str| {
        account_plugin(server, &whoami, "").replace(&format!("token_url = \"{server}/token\""), &trades)
    };
    mock_with(3600, manifest, &asking(&["read"], &[]), options())
}

/// A server that answers every request with the head of a body it then cuts short.
fn cut_short() -> String {
    let headers = vec![("Content-Type", "application/json".into()), ("Content-Length", "100".into())];
    Server::new(move |_, _| Reply { status: 200, headers: headers.clone(), body: b"{\"access".to_vec() }).url
}

/// How the token endpoint fails in [`prop_a_token_endpoint_failing_says_why_and_keeps_nothing`].
#[derive(Debug, Clone, Copy)]
enum Trade {
    /// It names an error, describing nothing.
    Said,
    /// It answers an error with nothing in it.
    Empty,
    /// Nothing listens there.
    Closed,
    /// Its answer is cut short.
    CutShort,
    /// It answers a token that can't go in a header.
    Unsendable,
    /// It answers what isn't tokens.
    Garbled,
}

/// For an account type trading its codes at a token endpoint or Local Network's token exchange,
/// one that fails (naming an error without describing it, answering an empty error, not
/// listening, cutting its answer short, answering what isn't tokens, or a token no request can
/// carry) signs nobody in, saying why.
#[hegel::test(test_cases = 2, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn prop_a_token_endpoint_failing_says_why_and_keeps_nothing(tc: TestCase) {
    let exchanges = tc.draw(gs::booleans());
    for trade in [Trade::Said, Trade::Empty, Trade::Closed, Trade::CutShort, Trade::Unsendable, Trade::Garbled] {
        let scripted = Scripted::new();
        let base = match trade {
            Trade::Closed => format!("http://{}", TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap()),
            Trade::CutShort => cut_short(),
            _ => scripted.url.clone(),
        };
        let reply = match trade {
            Trade::Said => json_reply(400, json!({ "error": "invalid_grant" })),
            Trade::Unsendable => json_reply(200, json!({ "access_token": "at\n1", "token_type": "bearer" })),
            Trade::Garbled => json_reply(200, json!({ "token_type": "bearer" })),
            _ => Reply { status: 400, headers: Vec::new(), body: Vec::new() },
        };
        scripted.then(None, reply);
        let m = trading_at(&base, exchanges);
        let e = m.sign_in(who(json!("a"), None), None, &[]).0.unwrap_err().to_string();
        let said = match trade {
            Trade::Said => "signing in to Mock: it said invalid_grant",
            Trade::Empty => "signing in to Mock: server returned empty error response",
            Trade::Closed => "signing in to Mock: error sending request",
            Trade::CutShort => "signing in to Mock: error decoding response body",
            Trade::Unsendable => "asking Mock who signed in: ",
            Trade::Garbled => "signing in to Mock: its answer couldn't be read",
        };
        assert!(e.starts_with(said), "{trade:?}: {e}");
        assert!(m.listed().is_empty(), "{trade:?}");
    }
}

/// For an account type trading its tokens at a token endpoint or Local Network's token exchange,
/// a store whose `accounts.json` can't be changed while an expiring token is refreshed fails the
/// refresh, whether the service refreshed it or won't any more, and keeps the account as it was.
#[hegel::test(test_cases = 2, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn prop_a_store_locked_while_refreshing_keeps_the_account_as_it_was(tc: TestCase) {
    let exchanges = tc.draw(gs::booleans());
    let scripted = Scripted::new();
    let m = trading_at(&scripted.url, exchanges);
    m.sign_in(who(json!("a"), None), None, &[]).0.unwrap();
    scripted.damages(&m);
    let snapshot = Snapshot::of(m.f.store.root());
    for refreshes in [true, false] {
        let reply = match refreshes {
            true => json_reply(200, json!({ "access_token": "at-10", "token_type": "bearer", "expires_in": 3600 })),
            false => json_reply(400, json!({ "error": "invalid_grant" })),
        };
        scripted.then(Some(Damage::Lock), reply);
        assert_eq!(m.token("a").unwrap_err().code(), "io", "refreshes {refreshes}");
        snapshot.undo(Damage::Lock);
        assert!(snapshot.unchanged(), "refreshes {refreshes}");
    }
    assert_eq!(m.listed().len(), 1);
}

/// How a server a plugin asks with an account's token damages the store as it answers, in
/// [`prop_a_store_damaged_as_a_service_refuses_a_token_says_so`].
#[derive(Debug, Clone, Copy)]
enum Refusing {
    /// [`Damage`]'s, answering the token refreshed that it's no good.
    Damaged(Damage),
    /// The account is forgotten, then the token refreshed is no good.
    Forgotten,
    /// Another host signs it in again, then the token refreshed is no good.
    Replaced,
    /// `accounts.json` isn't JSON, then the token lacks a scope.
    Scoped,
    /// `accounts.json` isn't JSON, then the token is refused, to be refreshed.
    Unrefreshable,
    /// Nothing's damaged, and the token refreshed is no good.
    SignedOut,
}

/// For a plugin's request with an account's token, a server refusing it (once, then the token
/// refreshed with `error="invalid_token"`, or saying it lacks a scope) as the store is damaged
/// fails as the service's failure where refreshing it, signing it out or forgetting what it was
/// granted can't be done; and the refusal is passed on, signing the account out, unless it's no
/// longer kept or another host signed it in again since.
#[hegel::test(test_cases = 2, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn prop_a_store_damaged_as_a_service_refuses_a_token_says_so(tc: TestCase) {
    let query = tc.draw(gs::sampled_from(vec!["", "token = { query = \"access_token\" }"]));
    let damaged = Damage::here().filter(|d| *d != Damage::Lock).map(Refusing::Damaged);
    let others =
        [Refusing::Forgotten, Refusing::Replaced, Refusing::Scoped, Refusing::Unrefreshable, Refusing::SignedOut];
    for refusing in damaged.chain(others) {
        let there = Scripted::new();
        let m = reaching(query, &[&there.url]);
        m.sign_in(who(json!("a"), None), None, &[]).0.unwrap();
        there.damages(&m);
        let challenge = |said: &str| vec![("WWW-Authenticate", format!("Bearer realm=\"mock\", {said}"))];
        let invalid = Reply { status: 401, headers: challenge(r#"error="invalid_token""#), body: Vec::new() };
        // The store's `file`, changed as `change` says.
        let edit = |file: &'static str, change: Box<dyn FnOnce(&mut Value) + Send>| -> Box<dyn FnOnce(&Path) + Send> {
            Box::new(move |root: &Path| edit_json(&root.join(file), change))
        };
        let first = Reply { status: 401, headers: Vec::new(), body: Vec::new() };
        let turns: Vec<Turn> = match refusing {
            Refusing::Damaged(damage) => {
                vec![(None, first), (Some(Box::new(move |root: &Path| damage.to(root))), invalid)]
            }
            Refusing::Forgotten => {
                vec![
                    (None, first),
                    (Some(edit("accounts.json", Box::new(|kept| kept["accounts"] = json!([])))), invalid),
                ]
            }
            Refusing::Replaced => {
                let other = json!({ "access_token": "at-99" }).to_string();
                let replace = Box::new(move |kept: &mut Value| kept[format!("account:{WITH}:a")] = json!(other));
                vec![(None, first), (Some(edit("secrets/accounts.json", replace)), invalid)]
            }
            Refusing::Scoped => {
                let scoped =
                    Reply { status: 403, headers: challenge(r#"error="insufficient_scope""#), body: Vec::new() };
                vec![(Some(Box::new(|root: &Path| Damage::Book.to(root))), scoped)]
            }
            Refusing::Unrefreshable => vec![(Some(Box::new(|root: &Path| Damage::Book.to(root))), first)],
            Refusing::SignedOut => vec![(None, first), (None, invalid)],
        };
        there.turns.lock().unwrap().extend(turns);
        let answer = match query {
            "" => m
                .f
                .fetch(json!({ "url": format!("{}/x", there.url), "headers": { "Authorization": "{account:mock}" } })),
            _ => m.f.fetch(json!({ "url": format!("{}/x?access_token={{account:mock}}", there.url) })),
        };
        let said = format!("{refusing:?}: {answer}");
        match refusing {
            Refusing::Damaged(_) | Refusing::Scoped | Refusing::Unrefreshable => {
                assert_eq!(answer["error"]["data"]["kind"], "external", "{said}")
            }
            Refusing::Forgotten | Refusing::Replaced | Refusing::SignedOut => {
                assert_eq!(answer["result"]["status"], 401, "{said}")
            }
        }
        if let Refusing::SignedOut = refusing {
            assert!(m.listed().is_empty(), "{said}: still signed in");
        }
        if let Refusing::Replaced = refusing {
            assert_eq!(m.token("a").unwrap(), "at-99", "{said}: signed out");
        }
        assert!(!m.leaked(&[answer["error"].clone()]), "a token is out: {said}");
    }
}
