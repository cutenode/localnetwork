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

use super::accounts::{Service, back, come_back_to, form, has, opened_at, service};
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
    mock_among(expires_in, |server| vec![account_plugin(server)], echo, options)
}

/// [`mock_with`], with the manifests of the account plugin and any others besides it what
/// `manifests` makes of the service's address.
pub(super) fn mock_among(
    expires_in: u64,
    manifests: impl FnOnce(&str) -> Vec<String>,
    echo: &str,
    options: Options,
) -> Mock {
    let state = Arc::new(Mutex::new(Service { expires_in: Some(expires_in), ..Service::default() }));
    let server = service(state.clone());
    let manifests = [vec![echo.to_string()], manifests(&server.url)].concat();
    let f = Fixture::new(move |dir| manifests.iter().map(|m| spec(dir, m, &[])).collect(), options);
    Mock { state, server, f }
}

/// An account plugin `id` at `server` providing the account type `kind`, signed in to with keys
/// (see `keys_account_plugin`), saying nowhere who they're for.
pub(super) fn keys_plugin(server: &str, id: &str, kind: &str) -> String {
    let manifest = crate::mock_oauth::keys_account_plugin(server, None);
    manifest.replace(&format!("id = \"{WITH}\""), &format!("id = \"{id}\"")).replace("\"mock\"", &format!("{kind:?}"))
}

/// Changes the JSON in `file` as `change` does.
pub(super) fn edit_json(file: &Path, change: impl FnOnce(&mut Value)) {
    let mut kept: Value = serde_json::from_str(&std::fs::read_to_string(file).unwrap()).unwrap();
    change(&mut kept);
    std::fs::write(file, kept.to_string()).unwrap();
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
        let browser = come_back_to(back(&start), format!("code=c0de&state={}", query["state"]));
        let done = self.f.plugins.finish_account_sign_in(&start.id, Duration::from_secs(10), &Ctx::silent());
        browser.join().unwrap();
        (done, query)
    }

    /// Changes the store's `file` (`accounts.json`, or `secrets/accounts.json`) as `change` does.
    pub(super) fn edit(&self, file: &str, change: impl FnOnce(&mut Value)) {
        edit_json(&self.f.store.root().join(file), change);
    }

    pub(super) fn listed(&self) -> Vec<Account> {
        self.f.plugins.store_accounts(&self.f.store).unwrap()
    }

    pub(super) fn token(&self, subject: &str) -> locallibrary::Result<String> {
        locallibrary::testing::account_token(&self.f.plugins, &self.f.store, WITH, subject)
    }

    /// How many times the service was asked to refresh a token.
    pub(super) fn refreshes(&self) -> usize {
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
/// again as an account kept or one signed out that the plugin it's for still has access to (the
/// browser maybe signing in as another), out, and refreshing: the store keeps exactly the
/// accounts signed in since they were last signed out, once each, in the order they were first
/// signed in then, each changed in place; the browser is asked to sign in as the account signed in
/// again, or to another when the account plugin has signed in any; signing in again as another
/// says so and gives the plugin it was for no access to it; and no token gets into
/// `accounts.json`, an error, a listing or a log.
#[hegel::test(test_cases = 8, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn an_account_is_kept_once_whoever_signs_in(tc: TestCase) {
    let m = mock(30);
    let people = 0..WHO.len();
    let each = people.clone().flat_map(|p| [Step::SignIn(p), Step::SignOut(p), Step::Refresh(p)]);
    let again = people.clone().flat_map(|of| people.clone().map(move |p| Step::Again(of, p)));
    let steps = tc.draw(gs::vecs(gs::sampled_from(each.chain(again).collect::<Vec<_>>())).max_size(6));
    // The accounts kept: subject to name; and those echo has access to, which outlast signing out.
    let mut kept: IndexMap<String, String> = IndexMap::new();
    let mut accessed: Vec<String> = Vec::new();
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
                match again.as_ref().filter(|a| !kept.contains_key(*a) && !accessed.contains(a)) {
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
                        // The plugin it was for is given access only to the account it was for.
                        let given = done.access.as_ref().map(|a| a.plugin.as_str());
                        let expected = instead.is_none().then_some("echo");
                        assert_eq!((done.instead_of.clone(), given), (instead, expected), "step {i}");
                        assert_eq!((&done.account.subject, &done.account.name), (&subject, &name), "step {i}");
                        answers.push(serde_json::to_value(&done).unwrap());
                        if given.is_some() && !accessed.contains(&subject) {
                            accessed.push(subject.clone());
                        }
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
/// account plugin has when there are any, and to nothing in particular before; the type's own
/// params go with every sign-in; and the plugin a sign-in again is for isn't given access to
/// another account the browser signs in to.
#[test]
fn the_browser_is_asked_for_one_account_or_another() {
    let m = mock_at(3600, "/userinfo", "params = { access_type = \"offline\" }");
    let (_, first) = m.sign_in(who(json!("a"), None), None, &[]);
    assert!(!first.contains_key("prompt") && !first.contains_key("login_hint"));
    let (_, another) = m.sign_in(who(json!("b"), None), None, &[]);
    assert_eq!((another.get("prompt").map(String::as_str), another.get("login_hint")), (Some("select_account"), None));
    let (done, again) = m.sign_in(who(json!("b"), None), Some("a"), &[]);
    assert_eq!((again.get("prompt"), again["login_hint"].as_str()), (None, "a"));
    assert!([first, another, again].iter().all(|q| has(q, "access_type", "offline")), "the type's params");
    let done = done.unwrap();
    assert_eq!(done.instead_of.as_deref(), Some("a"), "signed in as another than asked");
    assert_eq!(done.access, None, "echo was given access to another account than it was for");
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

/// Where [`whoever_whoami_says_signed_in_is_kept_or_nobody_is`] asks who signed in.
#[derive(Debug, Clone, Copy)]
enum Whoami {
    /// The service's `/userinfo`.
    Service,
    /// `/userinfo/moved`, which sends there.
    Moved,
    /// Elsewhere at the service, which answers what isn't JSON.
    NotJson,
    /// Another origin, which sends to the service's `/userinfo`.
    Away,
    /// Another origin, which sends back to itself.
    Around,
    /// Another origin, which answers 500.
    Broken,
    /// Another origin, which refuses the token with a 401.
    Refusing,
}

/// For any whoami endpoint (the service's, one sending there, one answering what isn't JSON, or
/// one at another origin sending there, around in circles, failing or refusing the token), its
/// token given in the `Authorization` header or the query, and anyone it says signed in (a string
/// or integer subject, or none): the account kept is the one it says, named by its email or
/// subject, or the sign-in fails saying why and nothing is kept.
#[hegel::test(test_cases = 16, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn whoever_whoami_says_signed_in_is_kept_or_nobody_is(tc: TestCase) {
    use Whoami::*;
    let at = tc.draw(gs::sampled_from(vec![Service, Moved, NotJson, Away, Around, Broken, Refusing]));
    let token = tc.draw(gs::sampled_from(vec!["", "token = { query = \"access_token\" }"]));
    let subject = tc.draw(gs::sampled_from(vec![json!("a"), json!(42), json!(1.5), Value::Null]));
    let email = tc.draw(gs::sampled_from(vec![None, Some("a@mock.example")]));
    let manifest = move |server: &str| {
        let to = format!("{server}/userinfo");
        let elsewhere = |reply: fn(&str, &str) -> Reply| {
            let to = to.clone();
            Server::new(move |base, _| reply(base, &to)).url + "/who"
        };
        fn moved(location: String) -> Reply {
            Reply { status: 307, headers: vec![("Location", location)], body: Vec::new() }
        }
        let whoami = match at {
            Service => to.clone(),
            Moved => format!("{server}/userinfo/moved"),
            NotJson => format!("{server}/nobody"),
            Away => elsewhere(|_, to| moved(to.into())),
            Around => elsewhere(|base, _| moved(format!("{base}/who"))),
            Broken => elsewhere(|_, _| Reply { status: 500, headers: Vec::new(), body: Vec::new() }),
            Refusing => elsewhere(|_, _| Reply { status: 401, headers: Vec::new(), body: Vec::new() }),
        };
        account_plugin(server, &whoami, token)
    };
    let m = mock_with(3600, manifest, &asking(&["read"], &["write"]), options());
    let mut said = json!({ "email": email });
    if !subject.is_null() {
        said["sub"] = subject.clone();
    }
    let done = m.sign_in(said, None, &[]).0;
    // A string subject as it is, an integer as its digits.
    let kept = match &subject {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) if n.is_u64() => Some(n.to_string()),
        _ => None,
    };
    let why = match (at, kept) {
        (Away, _) => "it sent the token on to 127.0.0.1, which isn't where it was asked",
        (Around, _) => "too many redirects (it follows 5)",
        (Broken, _) => "it answered 500 Internal Server Error",
        (Refusing, _) => "it answered 401 Unauthorized",
        (NotJson, _) => "its answer isn't JSON",
        (_, None) => "its answer has no subject at /sub",
        (_, Some(kept)) => {
            let done = done.unwrap();
            assert_eq!(done.account.subject, kept);
            assert_eq!(done.account.name, email.map_or(kept, String::from));
            assert_eq!(m.listed().len(), 1);
            return;
        }
    };
    assert_eq!(done.unwrap_err().to_string(), format!("asking Mock who signed in: {why}"));
    assert!(m.listed().is_empty());
}

/// Where [`prop_a_whoami_out_of_reach_or_sending_nowhere_signs_nobody_in`] asks who signed in.
#[derive(Debug, Clone, Copy)]
enum Unasked {
    /// A port nothing listens on.
    Closed,
    /// A server elsewhere, in a test kept offline.
    Offline,
    /// A server sending to an address that can't be read.
    Nowhere,
    /// A server sending back to itself.
    Around,
}

/// For any whoever the service says signed in, and a whoami endpoint that can't be reached (a
/// port nothing listens on, a server elsewhere in a test kept offline) or sends to an address
/// that can't be read, or back to itself: the sign-in fails saying why, and nothing is kept.
#[hegel::test(test_cases = 4, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn prop_a_whoami_out_of_reach_or_sending_nowhere_signs_nobody_in(tc: TestCase) {
    let subject = tc.draw(gs::sampled_from(vec![json!("a"), json!(42)]));
    for at in [Unasked::Closed, Unasked::Offline, Unasked::Nowhere, Unasked::Around] {
        let closed = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap();
        let moved = |to: String| Reply { status: 302, headers: vec![("Location", to)], body: Vec::new() };
        let nowhere = Server::new(move |base, _| match at {
            Unasked::Around => moved(format!("{base}/who")),
            _ => moved("http://[oops".into()),
        });
        let whoami = match at {
            Unasked::Closed => format!("http://{closed}/who"),
            Unasked::Offline => "https://example.com/who".into(),
            Unasked::Nowhere | Unasked::Around => format!("{}/who", nowhere.url),
        };
        let offline = Options { test_offline: true, ..options() };
        let m = mock_with(3600, |server| account_plugin(server, &whoami, ""), &asking(&["read"], &[]), offline);
        let e = m.sign_in(who(subject.clone(), None), None, &[]).0.unwrap_err().to_string();
        let why = match at {
            Unasked::Closed => "error sending request",
            Unasked::Offline => "example.com isn't on this computer, and tests stay offline",
            Unasked::Nowhere => "invalid IPv6 address",
            Unasked::Around => "too many redirects (it follows 5)",
        };
        assert!(e.starts_with(&format!("asking Mock who signed in: {why}")), "{at:?}: {e}");
        assert!(m.listed().is_empty(), "{at:?}");
    }
}

/// For a token in the header or the query, a whoami answer that isn't JSON, or has a subject that
/// is neither a string nor an integer, or none, signs nobody in, saying so.
#[hegel::test(test_cases = 2, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn prop_a_whoami_answer_without_a_subject_signs_nobody_in(tc: TestCase) {
    let token = tc.draw(gs::sampled_from(vec!["", "token = { query = \"access_token\" }"]));
    for (whoami, said, why) in [
        ("/nobody", json!({ "sub": "a" }), "its answer isn't JSON"),
        ("/userinfo", json!({ "sub": 1.5 }), "its answer has no subject at /sub"),
        ("/userinfo", json!({ "sub": "" }), "its answer has no subject at /sub"),
        ("/userinfo", json!({ "email": "a@mock.example" }), "its answer has no subject at /sub"),
    ] {
        let m = mock_at(3600, whoami, token);
        let e = m.sign_in(said.clone(), None, &[]).0.unwrap_err();
        assert_eq!(e.to_string(), format!("asking Mock who signed in: {why}"), "{said}");
        assert!(m.listed().is_empty(), "{said}");
    }
}

/// For an account the service gave no refresh token, its token in the header or the query: once
/// its token is about to expire it says to sign in again, and a request its token is refused for
/// is answered as the service answered, sent once, the account kept.
#[hegel::test(test_cases = 2, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn prop_an_account_without_a_refresh_token_isnt_refreshed(tc: TestCase) {
    let token = tc.draw(gs::sampled_from(vec!["", "token = { query = \"access_token\" }"]));
    let m = mock_at(3600, "/userinfo", token);
    m.state.lock().unwrap().refreshless = true;
    m.sign_in(who(json!("a"), None), None, &["read"]).0.unwrap();
    m.state.lock().unwrap().script.push_back(Reply { status: 401, headers: Vec::new(), body: Vec::new() });
    let query = (!token.is_empty()).then_some("access_token");
    let sent = m.server.got().len();
    let answer = m.fetch_as("GET", "/r/x", "{account:mock}", query);
    assert_eq!(answer["result"]["status"], 401, "{answer}");
    assert_eq!(m.server.got().len(), sent + 1, "sent again");
    m.edit("accounts.json", |book| book["accounts"][0]["expires_at"] = json!(chrono::Utc::now()));
    assert_eq!(m.token("a").unwrap_err().to_string(), "the Mock account a is signed out: sign in again");
    assert_eq!(m.listed().len(), 1, "forgotten");
}

/// For a whoami endpoint asked with the token in its query, whatever else its query has: what it
/// has is kept, but a parameter of the token's name, which the token takes the place of.
#[hegel::test(test_cases = 4, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn prop_whoami_keeps_its_query_but_where_the_token_goes(tc: TestCase) {
    let kept = tc.draw(gs::sampled_from(vec!["", "via=test&", "via=test&n=2&"]));
    let replaced = tc.draw(gs::booleans());
    let query = format!("{kept}{}", if replaced { "access_token=old" } else { "" });
    let m = mock_at(3600, &format!("/userinfo?{query}"), "token = { query = \"access_token\" }");
    m.sign_in(who(json!("a"), None), None, &[]).0.unwrap();
    let asked = m.server.got().into_iter().find(|g| g.target.starts_with("/userinfo")).unwrap();
    assert_eq!(asked.target, format!("/userinfo?{kept}access_token=at-1"), "{query}");
}

/// A way the files a store keeps its accounts in can be damaged.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum Damage {
    /// `accounts.json` isn't JSON.
    Book,
    /// `accounts.lock`, held while `accounts.json` changes, is a folder.
    Lock,
    /// `accounts/`, where each account's tokens are held, is a file.
    Holds,
    /// The folder of the secrets file is a file.
    Locker,
    /// The secrets file isn't JSON.
    Secrets,
}

impl Damage {
    pub(super) const ALL: [Damage; 5] = [Damage::Book, Damage::Lock, Damage::Holds, Damage::Locker, Damage::Secrets];

    /// Damages the store at `root` so.
    pub(super) fn to(self, root: &Path) {
        let [book, lock, holds, secrets] = files(root);
        match self {
            Damage::Book => std::fs::write(book, "not json").unwrap(),
            Damage::Lock => {
                let _ = std::fs::remove_file(&lock);
                std::fs::create_dir(lock).unwrap();
            }
            Damage::Holds => {
                let _ = std::fs::remove_dir_all(&holds);
                std::fs::write(holds, "").unwrap();
            }
            Damage::Locker => {
                let folder = secrets.parent().unwrap();
                let _ = std::fs::remove_dir_all(folder);
                std::fs::write(folder, "").unwrap();
            }
            Damage::Secrets => {
                std::fs::create_dir_all(secrets.parent().unwrap()).unwrap();
                std::fs::write(secrets, "not json").unwrap();
            }
        }
    }

    /// What an error it causes is: what isn't JSON can't be parsed, and a folder or a file in
    /// another's place can't be opened.
    pub(super) fn code(self) -> &'static str {
        match self {
            Damage::Book | Damage::Secrets => "parse",
            Damage::Lock | Damage::Holds | Damage::Locker => "io",
        }
    }
}

/// `accounts.json`, `accounts.lock`, `accounts/` and the secrets file of the store at `root`.
fn files(root: &Path) -> [PathBuf; 4] {
    let secrets = root.join("secrets").join("accounts.json");
    [root.join("accounts.json"), root.join("accounts.lock"), root.join("accounts"), secrets]
}

/// `accounts.json` and the secrets file of a store as they were, to put back.
pub(super) struct Snapshot {
    root: PathBuf,
    book: Option<Vec<u8>>,
    secrets: Option<Vec<u8>>,
}

impl Snapshot {
    pub(super) fn of(root: &Path) -> Snapshot {
        let [book, _, _, secrets] = files(root);
        Snapshot { root: root.into(), book: std::fs::read(book).ok(), secrets: std::fs::read(secrets).ok() }
    }

    /// Puts back what `damage` damaged, as it was.
    pub(super) fn undo(&self, damage: Damage) {
        let [book, lock, holds, secrets] = files(&self.root);
        let put = |file: PathBuf, was: &Option<Vec<u8>>| match was {
            Some(bytes) => {
                std::fs::create_dir_all(file.parent().unwrap()).unwrap();
                std::fs::write(file, bytes).unwrap();
            }
            None => drop(std::fs::remove_file(file)),
        };
        match damage {
            Damage::Book => put(book, &self.book),
            Damage::Secrets => put(secrets, &self.secrets),
            Damage::Locker => {
                if secrets.parent().unwrap().is_file() {
                    std::fs::remove_file(secrets.parent().unwrap()).unwrap();
                }
                put(secrets, &self.secrets);
            }
            Damage::Lock if lock.is_dir() => std::fs::remove_dir(lock).unwrap(),
            Damage::Holds if holds.is_file() => std::fs::remove_file(holds).unwrap(),
            Damage::Lock | Damage::Holds => {}
        }
    }

    /// Whether `accounts.json` and the secrets file are as they were.
    pub(super) fn unchanged(&self) -> bool {
        let now = Snapshot::of(&self.root);
        (&self.book, &self.secrets) == (&now.book, &now.secrets)
    }

    /// Puts everything back as it was.
    pub(super) fn restore(&self) {
        Damage::ALL.into_iter().for_each(|d| self.undo(d));
    }
}

/// What [`prop_a_damaged_store_fails_what_reads_it_and_changes_nothing`] does with the accounts.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Use {
    /// Lists the accounts, asking where their tokens are kept.
    List,
    /// Lists them without asking.
    Kept,
    /// Lists them and the accesses.
    Listing,
    Accesses,
    Give,
    SetDefault,
    TakeAway,
    Token,
    SignOut,
    /// Begins a sign-in to another account.
    Begin,
    /// Begins a sign-in again as the account.
    Again,
    /// Finishes a sign-in again as the account, begun before the damage.
    SignIn,
    Unready,
    /// Starts the echo plugin's store destination, checking its accounts first.
    Destination,
    /// The echo plugin's request with the account's token.
    Fetch,
}

impl Use {
    const ALL: [Use; 15] = [
        Use::List,
        Use::Kept,
        Use::Listing,
        Use::Accesses,
        Use::Give,
        Use::SetDefault,
        Use::TakeAway,
        Use::Token,
        Use::SignOut,
        Use::Begin,
        Use::Again,
        Use::SignIn,
        Use::Unready,
        Use::Destination,
        Use::Fetch,
    ];

    /// Whether it fails with `damage` done: it reads or changes what's damaged.
    fn fails(self, damage: Damage) -> bool {
        use Damage::*;
        match self {
            Use::Kept | Use::Accesses | Use::Begin => damage == Book,
            Use::List | Use::Listing | Use::Again | Use::Unready | Use::Destination => {
                matches!(damage, Book | Locker | Secrets)
            }
            Use::Give | Use::SetDefault | Use::TakeAway => matches!(damage, Book | Lock),
            Use::Token | Use::Fetch => matches!(damage, Book | Holds | Locker | Secrets),
            // A secrets file that can't be read is written anew.
            Use::SignIn => damage != Secrets,
            Use::SignOut => true,
        }
    }
}

impl Mock {
    /// Does `u` with the echo plugin's access to the account `a`, the store damaged as `damage`
    /// says once anything begun beforehand is: the error's code (a request's, its kind) and
    /// message when it fails.
    fn use_damaged(&self, u: Use, damage: Damage, query: Option<&str>) -> Result<(), (String, String)> {
        let (plugins, store) = (&self.f.plugins, &self.f.store);
        let a = super::accesses::account("a");
        let begun = (u == Use::SignIn).then(|| {
            let start = plugins.begin_account_sign_in("mock", WITH, Some("echo"), Some("a"), &[], store).unwrap();
            let query = opened_at(&start.url);
            let mut s = self.state.lock().unwrap();
            (s.challenge, s.signing_in_as) = (query["code_challenge"].clone(), who(json!("a"), None));
            (start, query)
        });
        damage.to(store.root());
        let begin = |again: Option<&str>| {
            let start = plugins.begin_account_sign_in("mock", WITH, None, again, &[], store);
            start.map(|s| assert!(plugins.cancel_sign_in(&s.id), "not waiting"))
        };
        let done = match u {
            Use::List => plugins.store_accounts(store).map(drop),
            Use::Kept => plugins.kept_accounts(store).map(drop),
            Use::Listing => plugins.accounts_and_accesses(store).map(drop),
            Use::Accesses => plugins.accesses(store, Some("echo"), None).map(drop),
            Use::Give => plugins.give_access("echo", &a, &[], store).map(drop),
            Use::SetDefault => plugins.set_default_access("echo", &a, store).map(drop),
            Use::TakeAway => plugins.take_away_access("echo", &a, store).map(drop),
            Use::Token => self.token("a").map(drop),
            Use::SignOut => plugins.sign_out_account(WITH, "a", store).map(drop),
            Use::Begin => begin(None),
            Use::Again => begin(Some("a")),
            Use::SignIn => {
                let (start, query) = begun.unwrap();
                let browser = come_back_to(back(&start), format!("code=c0de&state={}", query["state"]));
                let done = plugins.finish_account_sign_in(&start.id, Duration::from_secs(10), &Ctx::silent());
                browser.join().unwrap();
                done.map(drop)
            }
            Use::Unready => plugins.unready("echo", store).map(drop),
            Use::Destination => plugins.store_destination("echo:folder", Some(store)).map(drop),
            Use::Fetch => {
                let answer = self.fetch_as("GET", "/r/x", "{account:mock}", query);
                let e = &answer["error"];
                return match e.is_null() {
                    true => Ok(()),
                    false => Err((e["data"]["kind"].as_str().unwrap_or_default().into(), e["message"].to_string())),
                };
            }
        };
        done.map_err(|e| (e.code().to_string(), e.to_string()))
    }
}

/// For an account the echo plugin has access to, its token in the header or the query, and each
/// way the files the store keeps its accounts in can be damaged (`accounts.json` or the secrets
/// file not JSON, the lock of `accounts.json`, of each account's tokens or the secrets' folder
/// taken by a file or a folder): everything that reads or changes what's damaged fails saying it
/// can't be read or opened (a request, as the service's failure) and changes nothing else, but a
/// sign-in, which writes a secrets file it can't read anew; and everything else does as it
/// would. No token gets out.
#[hegel::test(test_cases = 2, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn prop_a_damaged_store_fails_what_reads_it_and_changes_nothing(tc: TestCase) {
    let query = tc.draw(gs::sampled_from(vec![None, Some("access_token")]));
    let place = query.map_or(String::new(), |name| format!("token = {{ query = \"{name}\" }}"));
    let m = mock_for(3600, "/userinfo", &place, &asking(&["read"], &["write"]));
    m.sign_in(who(json!("a"), None), None, &["read"]).0.unwrap();
    let snapshot = Snapshot::of(m.f.store.root());
    let mut answers = Vec::new();
    for damage in Damage::ALL {
        for u in Use::ALL {
            let done = m.use_damaged(u, damage, query);
            let said = format!("{u:?} with {damage:?}: {done:?}");
            assert_eq!(done.is_err(), u.fails(damage), "{said}");
            snapshot.undo(damage);
            if let Err((code, message)) = done {
                let expected = if u == Use::Fetch { "external" } else { damage.code() };
                assert_eq!(code, expected, "{said}");
                assert!(snapshot.unchanged(), "{said}: changed what wasn't damaged");
                answers.push(json!(message));
            }
            snapshot.restore();
        }
    }
    assert!(!m.leaked(&answers), "a token is out where it may not be");
}

/// For an access given more of an account, taken away or made the default, in a store whose
/// folder can't be written to: `accounts.json` can't be changed, the change fails saying so, and
/// the accesses are as they were.
#[cfg(unix)]
#[hegel::test(test_cases = 3, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn prop_a_store_it_cant_write_to_keeps_its_accesses_as_they_were(tc: TestCase) {
    use std::os::unix::fs::PermissionsExt;
    let m = mock(3600);
    m.sign_in(who(json!("a"), None), None, &["read"]).0.unwrap();
    m.sign_in_for(None, who(json!("b"), None), None, &[]).0.unwrap();
    let (plugins, store, root) = (&m.f.plugins, &m.f.store, m.f.store.root());
    let before = plugins.accesses(store, None, None).unwrap();
    let change = tc.draw(gs::integers::<usize>().max_value(2));
    let mode = |m: u32| std::fs::set_permissions(root, std::fs::Permissions::from_mode(m)).unwrap();
    let account = super::accesses::account;
    mode(0o555);
    let done = match change {
        0 => plugins.give_access("echo", &account("a"), &["write".into()], store).map(drop),
        1 => plugins.take_away_access("echo", &account("a"), store).map(drop),
        _ => plugins.set_default_access("echo", &account("b"), store).map(drop),
    };
    mode(0o755);
    assert_eq!(done.unwrap_err().code(), "io", "change {change}");
    assert_eq!(plugins.accesses(store, None, None).unwrap(), before, "change {change}");
}

/// Whether an account type can be told to forget a token decides whether signing out tells it:
/// the account is forgotten here either way.
#[hegel::test(test_cases = 4, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn prop_signing_out_tells_the_service_only_when_its_type_can_be_told(tc: TestCase) {
    let email = tc.draw(gs::sampled_from(vec![None, Some("a@mock.example")]));
    for revokes in [false, true] {
        let manifest = |server: &str| {
            let manifest = account_plugin(server, &format!("{server}/userinfo"), "");
            match revokes {
                true => manifest,
                false => manifest.replace(&format!("revoke_url = \"{server}/revoke\"\n"), ""),
            }
        };
        let m = mock_with(3600, manifest, &asking(&["read"], &[]), options());
        m.sign_in(who(json!("a"), email), None, &[]).0.unwrap();
        let out = m.f.plugins.sign_out_account(WITH, "a", &m.f.store).unwrap();
        assert_eq!((out.revoked, out.account.signed_in), (revokes, true), "revokes {revokes}");
        assert_eq!(m.server.got().iter().any(|g| g.target == "/revoke"), revokes, "revokes {revokes}");
        assert!(m.listed().is_empty(), "revokes {revokes}");
    }
}

/// For an account signed in, whoever it is: an account plugin, plugin, account type or scope that
/// isn't there is said not to be, as is a scope a plugin doesn't ask for; an account whose
/// tokens the keychain lost is signed out; and one of a type its account plugin no longer
/// provides can't be signed out of or used, and stays.
#[hegel::test(test_cases = 4, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn prop_what_isnt_there_is_said_not_to_be(tc: TestCase) {
    let subject = tc.draw(gs::sampled_from(vec!["a", "42"]));
    let m = mock(3600);
    m.sign_in(who(json!(subject), None), None, &["read"]).0.unwrap();
    let (plugins, store) = (&m.f.plugins, &m.f.store);
    let begin = |kind: &str, with: &str, plugin: Option<&str>, scopes: &[&str]| {
        let scopes: Vec<String> = scopes.iter().map(|s| s.to_string()).collect();
        plugins.begin_account_sign_in(kind, with, plugin, None, &scopes, store).unwrap_err()
    };
    let said = [
        (begin("mock", "nope", None, &[]), "not_found"),
        (begin("mock", WITH, Some("nope"), &[]), "not_found"),
        (begin("nope", WITH, None, &[]), "not_found"),
        (begin("mock", WITH, None, &["nope"]), "not_found"),
        (begin("mock", WITH, Some("echo"), &["peek"]), "invalid"),
        (locallibrary::testing::account_token(plugins, store, "nope", subject).unwrap_err(), "not_found"),
        (plugins.sign_out_account("nope", subject, store).unwrap_err(), "not_found"),
        (plugins.unready("nope", store).unwrap_err(), "not_found"),
    ];
    for (i, (e, code)) in said.iter().enumerate() {
        assert_eq!(e.code(), *code, "{i}: {e}");
    }
    let secrets = store.root().join("secrets/accounts.json");
    let kept = std::fs::read(&secrets).unwrap();
    std::fs::write(&secrets, "{}").unwrap();
    let e = m.token(subject).unwrap_err();
    assert_eq!(e.to_string(), format!("the Mock account {subject} is signed out: sign in again"));
    std::fs::write(&secrets, kept).unwrap();

    m.edit("accounts.json", |book| book["accounts"][0]["type"] = json!("gone"));
    let gone = "not found: the mock-accounts plugin's account type \"gone\"";
    assert_eq!(plugins.sign_out_account(WITH, subject, store).unwrap_err().to_string(), gone);
    assert_eq!(m.fetch_as("GET", "/r/x", "{account:mock}", None)["error"]["message"], gone);
    assert_eq!(m.token(subject).unwrap_err().to_string(), gone);
    assert_eq!(m.listed().len(), 1, "signed out");
}

/// An account type is signed in to through the account plugin named, else the only one providing
/// it; with several, the user is asked to name one of them, and with none, there's none.
#[hegel::test(test_cases = 4)]
fn prop_an_account_type_is_signed_in_to_through_the_plugin_named_or_the_only_one(tc: TestCase) {
    let named = tc.draw(gs::sampled_from(vec!["mock-accounts", "more-accounts", "anyone"]));
    let keyed = |id: &str, kind: &str| keys_plugin("http://127.0.0.1:9", id, kind);
    let manifests = [keyed(WITH, "mock"), keyed("more-accounts", "mock"), keyed("other-accounts", "other")];
    let f = Fixture::new(|dir| manifests.iter().map(|m| spec(dir, m, &[])).collect(), options());
    assert_eq!(f.plugins.providing("mock"), [WITH, "more-accounts"]);
    assert_eq!(f.plugins.provider("mock", Some(named)).unwrap(), named);
    assert_eq!(f.plugins.provider("other", None).unwrap(), "other-accounts");
    let e = f.plugins.provider("mock", None).unwrap_err();
    assert_eq!((e.code(), e.reason()), ("invalid", Some(locallibrary::error::Reason::MissingParam)), "{e}");
    assert_eq!(e.detail().fields["account_plugins"], json!([WITH, "more-accounts"]));
    assert_eq!(f.plugins.provider("nope", None).unwrap_err().code(), "not_found");
}
