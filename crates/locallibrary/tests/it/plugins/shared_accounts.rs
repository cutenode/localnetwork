//! Accounts of a type, kept once per store: signed in to through an account plugin (a manifest
//! the host never runs) at a mock OAuth service on this computer, which says who signed in at its
//! `/userinfo`; kept once for each account plugin and subject, whoever signs in; refreshed one at
//! a time, whichever host on the store asks; signed out; and no token anywhere but the secrets.

use std::sync::Barrier;

use hegel::TestCase;
use hegel::generators as gs;
use indexmap::IndexMap;
use locallibrary::keychain::Storage;
use locallibrary::plugin::{Account, AccountSignedIn};

use super::accounts::{Service, come_back_to, form, has, opened_at, service};
use super::net::{Got, Reply, Server};
use super::*;
pub(super) use crate::mock_oauth::WITH;
use crate::mock_oauth::account_plugin;

/// The echo plugin, talking to this computer, asking for `mock` accounts: for the scopes
/// `needs`, and `optional` besides when there are any.
pub(super) fn asking(needs: &[&str], optional: &[&str]) -> String {
    let optional = match optional {
        [] => String::new(),
        scopes => format!("optional = [{{ scopes = {scopes:?}, why = \"More\" }}]\n"),
    };
    let accounts = format!("\n[[accounts]]\ntype = \"mock\"\nscopes = {needs:?}\nwhy = \"Tests\"\n{optional}");
    format!("{}{accounts}", granting(&["net:127.0.0.1"]))
}

/// How the host runs the plugins here: reaching this computer, secrets in files.
pub(super) fn options() -> Options {
    Options { test_loopback: true, secrets: Storage::File, ..Options::default() }
}

/// The mock service, and the echo plugin and an account plugin for it in a store.
pub(super) struct Mock {
    pub(super) state: Arc<Mutex<Service>>,
    pub(super) server: Server,
    pub(super) f: Fixture,
}

/// [`Mock`], its tokens lasting `expires_in` seconds, the account plugin asking `whoami` (a path
/// at the service, or an address) who signed in, its token going where `token` says, the echo
/// plugin needing `read` of a mock account and maybe `write`.
fn mock_at(expires_in: u64, whoami: &str, token: &str) -> Mock {
    mock_for(expires_in, whoami, token, &asking(&["read"], &["write"]))
}

/// [`mock_at`], the echo plugin's manifest `echo`.
pub(super) fn mock_for(expires_in: u64, whoami: &str, token: &str, echo: &str) -> Mock {
    let manifest = |server: &str| {
        let whoami = if whoami.starts_with('/') { format!("{server}{whoami}") } else { whoami.into() };
        account_plugin(server, &whoami, token)
    };
    mock_with(expires_in, manifest, echo, options())
}

/// [`Mock`], its tokens lasting `expires_in` seconds, the account plugin's manifest what
/// `account_plugin` makes of the service's address, the echo plugin's `echo`, run as `options` say.
pub(super) fn mock_with(
    expires_in: u64,
    account_plugin: impl FnOnce(&str) -> String,
    echo: &str,
    options: Options,
) -> Mock {
    let state = Arc::new(Mutex::new(Service { expires_in: Some(expires_in), ..Service::default() }));
    let server = service(state.clone());
    let manifest = account_plugin(&server.url);
    let echo = echo.to_string();
    let f = Fixture::new(move |dir| vec![spec(dir, &echo, &[]), spec(dir, &manifest, &[])], options);
    Mock { state, server, f }
}

/// [`mock_at`], asking `/userinfo` with `Bearer`.
fn mock(expires_in: u64) -> Mock {
    mock_at(expires_in, "/userinfo", "")
}

/// Whom the service says signed in, by subject (a string or an integer) and email.
pub(super) fn who(subject: Value, email: Option<&str>) -> Value {
    match email {
        Some(email) => json!({ "sub": subject, "email": email }),
        None => json!({ "sub": subject }),
    }
}

impl Mock {
    /// Signs in for the echo plugin as `who`, again as the account `again` when it's given, asking
    /// for the type's `scopes`: what it did, and the query of the address the browser opened.
    pub(super) fn sign_in(
        &self,
        who: Value,
        again: Option<&str>,
        scopes: &[&str],
    ) -> (locallibrary::Result<AccountSignedIn>, IndexMap<String, String>) {
        self.sign_in_for(Some("echo"), who, again, scopes)
    }

    /// [`Mock::sign_in`], for `plugin` when it's given.
    pub(super) fn sign_in_for(
        &self,
        plugin: Option<&str>,
        who: Value,
        again: Option<&str>,
        scopes: &[&str],
    ) -> (locallibrary::Result<AccountSignedIn>, IndexMap<String, String>) {
        let scopes: Vec<String> = scopes.iter().map(|s| s.to_string()).collect();
        let start = self.f.plugins.begin_account_sign_in("mock", WITH, plugin, again, &scopes, &self.f.store);
        let start = match start {
            Ok(start) => start,
            Err(e) => return (Err(e), IndexMap::new()),
        };
        let query = opened_at(&start.url);
        {
            let mut s = self.state.lock().unwrap();
            (s.challenge, s.signing_in_as) = (query["code_challenge"].clone(), who);
        }
        let browser = come_back_to(&start.redirect_uri, format!("code=c0de&state={}", query["state"]));
        let done = self.f.plugins.finish_account_sign_in(&start.id, Duration::from_secs(10), &Ctx::silent());
        browser.join().unwrap();
        (done, query)
    }

    pub(super) fn listed(&self) -> Vec<Account> {
        self.f.plugins.store_accounts(&self.f.store).unwrap()
    }

    pub(super) fn token(&self, subject: &str) -> locallibrary::Result<String> {
        locallibrary::testing::account_token(&self.f.plugins, &self.f.store, WITH, subject)
    }

    /// How many times the service was asked to refresh a token.
    fn refreshes(&self) -> usize {
        let refreshing = |g: &Got| g.target == "/token" && has(&form(g), "grant_type", "refresh_token");
        self.server.got().iter().filter(|g| refreshing(g)).count()
    }

    /// Whether any token the service gave is out in the plugins' logs, the store but its secrets,
    /// or `answers`.
    pub(super) fn leaked(&self, answers: &[Value]) -> bool {
        let issued = self.state.lock().unwrap().issued;
        let seen = self.f.seen(&answers.iter().collect::<Vec<_>>());
        (1..=issued).any(|n| seen.contains(&format!("at-{n}")) || seen.contains(&format!("rt-{n}")))
    }
}

/// Who a step of [`an_account_is_kept_once_whoever_signs_in`] signs in as: by a string subject
/// and an email, or by an integer subject alone.
const WHO: [(&str, Option<&str>); 2] = [("a", Some("a@mock.example")), ("42", None)];

/// The subject [`WHO`]'s `i` is kept as, what the service says it is, and its name.
fn person(i: usize) -> (String, Value, String) {
    let (subject, email) = WHO[i];
    let said = subject.parse::<u64>().map_or_else(|_| json!(subject), |n| json!(n));
    (subject.into(), who(said, email), email.unwrap_or(subject).into())
}

/// A step of [`an_account_is_kept_once_whoever_signs_in`], naming people by their place in
/// [`WHO`].
#[derive(Debug, Clone, Copy)]
enum Step {
    SignIn(usize),
    /// Signing in again as the first, the browser signing in as the second.
    Again(usize, usize),
    SignOut(usize),
    Refresh(usize),
}

/// For any order of signing in as one person or another (by a string or an integer subject), in
/// again as an account kept (the browser maybe signing in as another), out, and refreshing: the
/// store keeps exactly the accounts signed in since they were last signed out, once each, in the
/// order they were first signed in then, each changed in place; the browser is asked to sign in
/// as the account signed in again, or to another when the account plugin has signed in any;
/// signing in again as another says so; and no token gets into `accounts.json`, an error, a
/// listing or a log.
#[hegel::test(test_cases = 8, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn an_account_is_kept_once_whoever_signs_in(tc: TestCase) {
    let m = mock(30);
    let people = 0..WHO.len();
    let each = people.clone().flat_map(|p| [Step::SignIn(p), Step::SignOut(p), Step::Refresh(p)]);
    let again = people.clone().flat_map(|of| people.clone().map(move |p| Step::Again(of, p)));
    let steps = tc.draw(gs::vecs(gs::sampled_from(each.chain(again).collect::<Vec<_>>())).max_size(6));
    // The accounts kept: subject to name.
    let mut kept: IndexMap<String, String> = IndexMap::new();
    let mut answers = Vec::new();
    for (i, step) in steps.into_iter().enumerate() {
        match step {
            Step::SignIn(p) | Step::Again(_, p) => {
                let again = match step {
                    Step::Again(of, _) => Some(person(of).0),
                    _ => None,
                };
                let (subject, said, name) = person(p);
                let (done, opened) = m.sign_in(said, again.as_deref(), &["read"]);
                match again.as_ref().filter(|a| !kept.contains_key(*a)) {
                    Some(_) => {
                        let e = done.unwrap_err();
                        assert_eq!(e.code(), "not_found", "step {i}: {e}");
                        answers.push(json!(e.to_string()));
                    }
                    None => {
                        let done = done.unwrap();
                        assert_eq!(opened.get("login_hint").map(String::as_str), again.as_deref(), "step {i}");
                        let select = (again.is_none() && !kept.is_empty()).then_some("select_account");
                        assert_eq!(opened.get("prompt").map(String::as_str), select, "step {i}");
                        let instead = again.clone().filter(|a| *a != subject);
                        let given = done.access.as_ref().map(|a| a.plugin.as_str());
                        assert_eq!((done.instead_of.clone(), given), (instead, Some("echo")));
                        assert_eq!((&done.account.subject, &done.account.name), (&subject, &name), "step {i}");
                        answers.push(serde_json::to_value(&done).unwrap());
                        kept.insert(subject, name);
                    }
                }
            }
            Step::SignOut(p) => {
                let subject = person(p).0;
                match kept.shift_remove(&subject) {
                    Some(_) => {
                        let out = m.f.plugins.sign_out_account(WITH, &subject, &m.f.store).unwrap();
                        assert!(out.revoked && out.account.signed_in, "step {i}");
                        answers.push(serde_json::to_value(&out).unwrap());
                    }
                    None => {
                        let e = m.f.plugins.sign_out_account(WITH, &subject, &m.f.store).unwrap_err();
                        assert_eq!(e.code(), "not_found", "step {i}");
                    }
                }
            }
            Step::Refresh(p) => {
                let subject = person(p).0;
                let token = m.token(&subject);
                match kept.contains_key(&subject) {
                    // Tokens last half a minute: each is refreshed before it's used.
                    true => {
                        let newest = format!("at-{}", m.state.lock().unwrap().issued);
                        assert!(token.is_ok_and(|t| t == newest), "step {i}: not the newest token");
                    }
                    false => assert_eq!(token.unwrap_err().code(), "not_found", "step {i}"),
                }
            }
        }
        let listed = m.listed();
        let said: Vec<(String, String)> = listed.iter().map(|a| (a.subject.clone(), a.name.clone())).collect();
        let expected: Vec<(String, String)> = kept.clone().into_iter().collect();
        assert_eq!(said, expected, "step {i}");
        assert!(listed.iter().all(|a| a.kind == "mock" && a.plugin == WITH && a.signed_in && a.refreshes));
        answers.push(serde_json::to_value(&listed).unwrap());
    }
    assert!(!m.leaked(&answers), "a token is out where it may not be");
}

/// Any number of hosts on one store (threads let go together, each with its own) asking for an
/// account's expiring token get the newest one, refreshed one at a time: once, when the refreshed
/// token lasts, or once each, when it expires again within the minute. The service's refresh
/// tokens refresh once, so a token refreshed twice would sign the account out.
#[hegel::test(test_cases = 10, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn refreshes_of_an_account_happen_one_at_a_time(tc: TestCase) {
    let hosts = tc.draw(gs::integers::<usize>().min_value(2).max_value(6));
    let lasting = tc.draw(gs::booleans());
    let m = mock(30);
    {
        let mut s = m.state.lock().unwrap();
        (s.rotates, s.single_use) = (true, true);
    }
    m.sign_in(who(json!("a"), None), None, &["read"]).0.unwrap();
    m.state.lock().unwrap().expires_in = Some(if lasting { 3600 } else { 30 });
    let dir = tempfile::tempdir().unwrap();
    let manifest = account_plugin(&m.server.url, &format!("{}/userinfo", m.server.url), "");
    let specs = vec![spec(dir.path(), &manifest, &[])];
    let start = Barrier::new(hosts);
    let mut got: Vec<String> = std::thread::scope(|s| {
        let asking: Vec<_> = (0..hosts)
            .map(|_| {
                s.spawn(|| {
                    let plugins = Plugins::new(specs.clone(), Arc::new(NoReads), options()).unwrap();
                    let store = Store::open(m.f.store.root()).unwrap();
                    start.wait();
                    locallibrary::testing::account_token(&plugins, &store, WITH, "a")
                })
            })
            .collect();
        asking.into_iter().map(|h| h.join().unwrap().map_err(|e| e.to_string())).collect::<Result<_, _>>()
    })
    .unwrap();
    got.sort_by_key(|t| t.trim_start_matches("at-").parse::<usize>().unwrap_or_default());
    let expected: Vec<String> = match lasting {
        true => vec!["at-2".into(); hosts],
        false => (2..=hosts + 1).map(|n| format!("at-{n}")).collect(),
    };
    assert!(got == expected, "not the newest tokens, one refresh at a time");
    assert_eq!(m.refreshes(), if lasting { 1 } else { hosts });
    assert!(m.listed()[0].signed_in);
}

/// The whoami endpoint's redirects are followed within its origin, the token given again where
/// the account type says it goes; one to another origin is refused, and nothing is kept.
#[test]
fn whoami_follows_redirects_within_its_origin_only() {
    for token in ["", "token = { query = \"access_token\" }"] {
        let m = mock_at(3600, "/userinfo/moved", token);
        let done = m.sign_in(who(json!("a"), None), None, &[]).0.unwrap();
        assert_eq!(done.account.subject, "a", "{token}");
        let asked: Vec<Got> = m.server.got().into_iter().filter(|g| g.target.starts_with("/userinfo")).collect();
        let sent = |g: &Got| match token {
            "" => g.headers.get("authorization").is_some_and(|a| a == "Bearer at-1"),
            _ => g.target.ends_with("?access_token=at-1"),
        };
        assert!(asked.len() == 2 && asked.iter().all(sent), "{token}: not followed with the token");
    }

    let away = |server: &str| {
        let to = format!("{server}/userinfo");
        let away =
            Server::new(move |_, _| Reply { status: 302, headers: vec![("Location", to.clone())], body: Vec::new() });
        account_plugin(server, &format!("{}/who", away.url), "")
    };
    let m = mock_with(3600, away, &asking(&["read"], &["write"]), options());
    let e = m.sign_in(who(json!("a"), None), None, &[]).0.unwrap_err();
    assert_eq!(
        e.to_string(),
        "asking Mock who signed in: it sent the token on to 127.0.0.1, which isn't where it was asked"
    );
    assert!(m.server.got().iter().all(|g| !g.target.starts_with("/userinfo")), "the other origin was asked");
    assert!(m.listed().is_empty());
}

/// An integer subject is kept as its digits, and named by them when the service gives no name.
#[test]
fn an_integer_subject_is_kept_as_its_digits() {
    let m = mock(3600);
    let done = m.sign_in(who(json!(1234567890123_u64), None), None, &[]).0.unwrap();
    assert_eq!((done.account.subject.as_str(), done.account.name.as_str()), ("1234567890123", "1234567890123"));
    let e = m.sign_in(json!({ "sub": 1.5 }), None, &[]).0.unwrap_err();
    assert_eq!(e.to_string(), "asking Mock who signed in: its answer has no subject at /sub");
    assert_eq!(m.listed().len(), 1);
}

/// What a sign-in asks the service for: what the type always asks, what the account was granted,
/// and what the scopes asked for ask; a service that doesn't say what it granted granted that.
#[test]
fn a_service_that_doesnt_say_what_it_granted_granted_what_was_asked() {
    let m = mock(3600);
    let (done, opened) = m.sign_in(who(json!("a"), None), None, &["read"]);
    assert_eq!(opened["scope"], "openid read");
    assert_eq!(done.unwrap().account.scopes, ["read", "write"], "what the service said it granted");
    m.state.lock().unwrap().unscoped = true;
    let (done, opened) = m.sign_in(who(json!("a"), None), Some("a"), &["write"]);
    assert_eq!(opened["scope"], "openid read write");
    assert_eq!(done.unwrap().account.scopes, ["openid", "read", "write"]);
    let e = m.f.plugins.begin_account_sign_in("mock", WITH, None, None, &["nope".into()], &m.f.store).unwrap_err();
    assert_eq!(e.to_string(), "not found: the mock account type's scope \"nope\"");
}

/// The browser is asked to sign in as the account signed in again, or to another than those the
/// account plugin has when there are any, and to nothing in particular before.
#[test]
fn the_browser_is_asked_for_one_account_or_another() {
    let m = mock(3600);
    let (_, first) = m.sign_in(who(json!("a"), None), None, &[]);
    assert!(!first.contains_key("prompt") && !first.contains_key("login_hint"));
    let (_, another) = m.sign_in(who(json!("b"), None), None, &[]);
    assert_eq!((another.get("prompt").map(String::as_str), another.get("login_hint")), (Some("select_account"), None));
    let (done, again) = m.sign_in(who(json!("b"), None), Some("a"), &[]);
    assert_eq!((again.get("prompt"), again["login_hint"].as_str()), (None, "a"));
    assert_eq!(done.unwrap().instead_of.as_deref(), Some("a"), "signed in as another than asked");
    let subjects: Vec<String> = m.listed().into_iter().map(|a| a.subject).collect();
    assert_eq!(subjects, ["a", "b"]);
}

/// Signing out revokes the refresh token at the service and forgets the account and its tokens.
#[test]
fn signing_out_revokes_and_forgets_the_account() {
    let m = mock(3600);
    m.sign_in(who(json!("a"), None), None, &[]).0.unwrap();
    let out = m.f.plugins.sign_out_account(WITH, "a", &m.f.store).unwrap();
    assert!(out.revoked && out.account.signed_in);
    let revoked = form(m.server.got().last().unwrap());
    assert!(has(&revoked, "token", "rt-1") && has(&revoked, "token_type_hint", "refresh_token"), "not revoked");
    assert!(m.listed().is_empty());
    let secrets = std::fs::read_to_string(m.f.store.root().join("secrets/accounts.json")).unwrap_or_default();
    assert!(!secrets.contains("at-1") && !secrets.contains("rt-1"), "forgotten here too");
    assert_eq!(m.token("a").unwrap_err().code(), "not_found");
}

/// A refresh the service won't make any more (`invalid_grant`) signs the account out; one it
/// can't make now leaves it signed in, saying what the service said but the refresh token.
#[test]
fn a_refresh_the_service_wont_make_signs_the_account_out() {
    let m = mock(30);
    m.sign_in(who(json!("a"), None), None, &[]).0.unwrap();
    m.state.lock().unwrap().refuses = Some(("temporarily_unavailable".into(), "rt-1 can't be used now".into()));
    let e = m.token("a").unwrap_err();
    assert_eq!(e.to_string(), "signing in to Mock: temporarily_unavailable: <secret> can't be used now");
    assert!(m.listed()[0].signed_in);
    m.state.lock().unwrap().refuses = Some(("invalid_grant".into(), "spent".into()));
    let e = m.token("a").unwrap_err();
    assert_eq!(e.to_string(), "the Mock account a is signed out: sign in again");
    assert!(m.listed().is_empty());
    assert!(!m.leaked(&[json!(e.to_string())]));
}

/// A whoami endpoint that doesn't say who signed in signs nobody in.
#[test]
fn a_whoami_that_doesnt_say_who_signs_nobody_in() {
    let m = mock_at(3600, "/nobody", "");
    let e = m.sign_in(who(json!("a"), None), None, &[]).0.unwrap_err();
    assert_eq!(e.to_string(), "asking Mock who signed in: its answer isn't JSON");
    assert!(m.listed().is_empty());
}
