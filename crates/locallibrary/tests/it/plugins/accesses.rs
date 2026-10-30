//! Accesses: the scopes of an account of a type a plugin is given, at a sign-in for it or as the
//! user says, its default for the type answering `{account:<type>}`. Its token goes into a request
//! only where one of the access's scopes covers it and the account was granted what that scope
//! asks, and only while the access has every scope the plugin needs; anything else fails before a
//! request is sent, and an error the plugin passes on is resolved as its manifest says.

use hegel::TestCase;
use hegel::generators as gs;
use locallibrary::error::{Action, Reason};
use locallibrary::plugin::AccountId;

use super::accounts::{form, has};
use super::net::Got;
use super::shared_accounts::{Mock, WITH, asking, mock_for, who};
use super::*;

/// The mock type's scopes (see `shared_accounts::account_plugin`).
const SCOPES: [&str; 3] = ["read", "write", "peek"];

/// What the mock type's scope `scope` asks the service for.
fn asks(scope: &str) -> &'static [&'static str] {
    match scope {
        "read" => &["read"],
        "write" => &["write", "read"],
        _ => &[],
    }
}

/// Whether the mock type's scope `scope` covers a `method` request to `path`.
fn covers(scope: &str, method: &str, path: &str) -> bool {
    match scope {
        "read" => method == "GET" && path.starts_with("/r/"),
        "write" => method == "POST" && path.starts_with("/w/"),
        _ => method == "GET" && path.starts_with("/p/"),
    }
}

fn account(subject: &str) -> AccountId {
    AccountId { plugin: WITH.into(), subject: subject.into() }
}

/// Some of [`SCOPES`], as the test draws them.
fn some(tc: &TestCase) -> Vec<&'static str> {
    SCOPES.into_iter().filter(|_| tc.draw(gs::booleans())).collect()
}

impl Mock {
    /// What the echo plugin's `method` request to `path` answered, naming an account as `named`
    /// in the query parameter `query`, or in the `Authorization` header.
    pub(super) fn fetch_as(&self, method: &str, path: &str, named: &str, query: Option<&str>) -> Value {
        let url = format!("{}{path}", self.server.url);
        self.f.fetch(match query {
            Some(name) => json!({ "url": format!("{url}?{name}={named}"), "method": method }),
            None => json!({ "url": url, "method": method, "headers": { "Authorization": named } }),
        })
    }

    /// Whether the last request the service got had `token` where `query` says (the
    /// `Authorization` header after `Bearer` without one). Never prints a token.
    fn last_had(&self, token: &str, query: Option<&str>) -> bool {
        let got = self.server.got().last().cloned().unwrap_or_default();
        match query {
            Some(name) => {
                got.target.ends_with(&format!("?{name}={token}")) && !got.headers.contains_key("authorization")
            }
            None => got.headers.get("authorization").is_some_and(|a| *a == format!("Bearer {token}")),
        }
    }

    /// What the echo plugin failing as `kind`, with `reason` and `fields`, is to the host.
    fn passed_on(&self, kind: &str, reason: Option<&str>, fields: &Value) -> Error {
        let failing = json!({ "fail": kind, "message": "it can't", "reason": reason, "fields": fields });
        self.f.echo_params(failing).unwrap_err()
    }
}

/// How a request names the account.
#[derive(Debug, Clone, Copy)]
enum Naming {
    /// `{account:mock}`.
    Default,
    /// `{account:mock/<id>}`, the access's id.
    Its,
    /// `{account:mock/<id>}`, an id no access of the plugin's has.
    Another,
}

/// For an echo plugin needing any of the mock type's scopes (the rest optional), an account
/// granted what any of them ask, given to it or not (with any optional scopes, and maybe missing
/// one it needs now), signed out or not, and a request of any method to any address, naming the
/// account by type, by the access or by another: the token goes, where the type says, exactly
/// when the plugin has the access named, the account is signed in, the access has every scope the
/// plugin needs, one of its scopes covers the request and the account was granted what that one
/// asks; otherwise it fails as `access` (`signed_out` when it's signed out) or `scope`, naming
/// the type and the account plugin, and nothing is sent. No token gets out.
#[hegel::test(test_cases = 16, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn a_token_goes_only_where_an_access_scope_covers(tc: TestCase) {
    let query = tc.draw(gs::sampled_from(vec![None, Some("access_token")]));
    let place = query.map_or(String::new(), |name| format!("token = {{ query = \"{name}\" }}"));
    let mut needs = some(&tc);
    if needs.is_empty() {
        needs.push(tc.draw(gs::sampled_from(SCOPES.to_vec())));
    }
    let optional: Vec<&str> = SCOPES.into_iter().filter(|s| !needs.contains(s)).collect();
    let m = mock_for(3600, "/userinfo", &place, &asking(&needs, &optional));
    m.state.lock().unwrap().unscoped = true;
    let asked = some(&tc);
    m.sign_in_for(None, who(json!("a"), Some("a@mock.example")), None, &asked).0.unwrap();
    let granted: Vec<&str> = asked.iter().flat_map(|s| asks(s)).copied().collect();

    let given = tc.draw(gs::booleans());
    let (mut scopes, mut id) = (Vec::new(), "ffffffff".to_string());
    if given {
        let extra: Vec<String> = optional.iter().filter(|_| tc.draw(gs::booleans())).map(|s| s.to_string()).collect();
        let access = m.f.plugins.give_access("echo", &account("a"), &extra, &m.f.store).unwrap();
        assert!(access.default && access.name == "a@mock.example", "the first access is the default");
        (scopes, id) = (access.scopes, access.id);
    }
    // Given before the plugin needed one of the scopes it needs now.
    let dropped = given && tc.draw(gs::booleans());
    if dropped {
        let file = m.f.store.root().join("accounts.json");
        let mut kept: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
        kept["accesses"][0]["scopes"] = json!(scopes.iter().filter(|s| *s != needs[0]).collect::<Vec<_>>());
        std::fs::write(&file, kept.to_string()).unwrap();
    }
    let signed_out = tc.draw(gs::booleans());
    if signed_out {
        m.f.plugins.sign_out_account(WITH, "a", &m.f.store).unwrap();
    }
    let naming = tc.draw(gs::sampled_from(vec![Naming::Default, Naming::Its, Naming::Another]));
    let named = match naming {
        Naming::Default => "{account:mock}".to_string(),
        Naming::Its => format!("{{account:mock/{id}}}"),
        Naming::Another => "{account:mock/0a1b2c3d}".to_string(),
    };
    let method = tc.draw(gs::sampled_from(vec!["GET", "POST"]));
    let path = tc.draw(gs::sampled_from(vec!["/r/x", "/w/x", "/p/x", "/o/x"]));
    let sent = m.server.got().len();
    let answer = m.fetch_as(method, path, &named, query);

    let covering: Vec<&String> = scopes.iter().filter(|s| covers(s, method, path)).collect();
    let expected = match () {
        _ if !given || matches!(naming, Naming::Another) => Err(("access", None)),
        _ if signed_out => Err(("access", Some("signed_out"))),
        _ if dropped => Err(("scope", None)),
        _ if covering.is_empty() => Err(("scope", None)),
        _ if !covering.iter().any(|s| asks(s).iter().all(|a| granted.contains(a))) => Err(("scope", None)),
        _ => Ok(()),
    };
    match expected {
        Ok(()) => {
            assert_eq!(answer["result"]["status"], 200, "{answer}");
            assert!(m.last_had("at-1", query), "not sent the token where the type says");
        }
        Err((kind, reason)) => {
            let e = &answer["error"]["data"];
            assert_eq!((e["kind"].as_str(), e["reason"].as_str()), (Some(kind), reason), "{answer}");
            assert_eq!((&e["fields"]["type"], &e["fields"]["account_plugins"]), (&json!("mock"), &json!([WITH])));
            assert_eq!(m.server.got().len(), sent, "something was sent");
        }
    }
    // What the service answered is the service's: it echoes the `Authorization` header it got.
    assert!(!m.leaked(&[answer["error"].clone()]), "a token is out where it may not be");
}

/// A step of [`the_default_access_answers_the_type_alone`], naming accounts by their place in
/// [`SUBJECTS`].
#[derive(Debug, Clone, Copy)]
enum Step {
    Give(usize),
    SetDefault(usize),
    TakeAway(usize),
}

const SUBJECTS: [&str; 3] = ["a", "b", "c"];

/// For any order of giving the echo plugin access to three accounts, making one its default and
/// taking them away: its first access is its default, setting one makes it the default (giving
/// it first when it isn't), taking the default away leaves the one left the default when one is
/// left and none otherwise, and taking away one it hasn't is refused; `{account:mock}` gets the
/// default's token, or fails as `access` when there's none.
#[hegel::test(test_cases = 8, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn the_default_access_answers_the_type_alone(tc: TestCase) {
    let m = mock_for(3600, "/userinfo", "", &asking(&["read"], &[]));
    for subject in SUBJECTS {
        // Each gets the next token: `at-1` for the first.
        m.sign_in_for(None, who(json!(subject), None), None, &["read"]).0.unwrap();
    }
    let each = (0..SUBJECTS.len()).flat_map(|i| [Step::Give(i), Step::SetDefault(i), Step::TakeAway(i)]);
    let steps = tc.draw(gs::vecs(gs::sampled_from(each.collect::<Vec<_>>())).max_size(6));
    let (mut has, mut default) = ([false; 3], None);
    for (n, step) in steps.into_iter().enumerate() {
        let plugins = &m.f.plugins;
        match step {
            Step::Give(i) => {
                plugins.give_access("echo", &account(SUBJECTS[i]), &[], &m.f.store).unwrap();
                if !has.iter().any(|h| *h) {
                    default = Some(i);
                }
                has[i] = true;
            }
            Step::SetDefault(i) => {
                assert!(plugins.set_default_access("echo", &account(SUBJECTS[i]), &m.f.store).unwrap().default);
                (has[i], default) = (true, Some(i));
            }
            Step::TakeAway(i) => {
                let taken = plugins.take_away_access("echo", &account(SUBJECTS[i]), &m.f.store);
                match has[i] {
                    true => {
                        assert_eq!(taken.unwrap().account, account(SUBJECTS[i]), "step {n}");
                        has[i] = false;
                        if default == Some(i) {
                            let left: Vec<usize> = (0..SUBJECTS.len()).filter(|j| has[*j]).collect();
                            default = (left.len() == 1).then(|| left[0]);
                        }
                    }
                    false => assert_eq!(taken.unwrap_err().code(), "not_found", "step {n}"),
                }
            }
        }
        let listed = m.f.plugins.accesses(&m.f.store, Some("echo"), None).unwrap();
        let mut said: Vec<(&str, bool)> = listed.iter().map(|a| (a.account.subject.as_str(), a.default)).collect();
        said.sort();
        let expected: Vec<(&str, bool)> =
            (0..SUBJECTS.len()).filter(|i| has[*i]).map(|i| (SUBJECTS[i], default == Some(i))).collect();
        assert_eq!(said, expected, "step {n}");
        let answer = m.fetch_as("GET", "/r/x", "{account:mock}", None);
        match default {
            Some(i) => assert!(m.last_had(&format!("at-{}", i + 1), None), "step {n}: not the default's token"),
            None => assert_eq!(answer["error"]["data"]["kind"], "access", "step {n}: {answer}"),
        }
    }
}

/// A redirect within the origin to where no scope of the access covers loses the token; one to
/// where a scope covers keeps it.
#[test]
fn a_redirect_outside_the_scopes_drops_the_token() {
    let m = mock_for(3600, "/userinfo", "", &asking(&["read"], &[]));
    m.sign_in(who(json!("a"), None), None, &["read"]).0.unwrap();
    let kept = m.fetch_as("GET", "/r/go/r/there", "{account:mock}", None);
    assert_eq!(kept["result"]["body"], "/r/there Bearer at-1", "the token didn't go along");
    let dropped = m.fetch_as("GET", "/r/go/o/there", "{account:mock}", None);
    assert_eq!(dropped["result"]["body"], "/o/there -", "the token went where no scope covers");
}

/// The actions an `account` error a plugin passes on offers: signing in to an account of the
/// type for it, and making each one of the type the store keeps its default.
fn resolving(accounts: &[&str]) -> Vec<Action> {
    let sign_in = json!({ "type": "mock", "for": "echo" });
    let mut actions = vec![Action::call("accounts.sign_in.begin", sign_in, "Sign in to a mock account")];
    for name in accounts {
        let params = json!({ "plugin": "echo", "account": account(name) });
        actions.push(Action::call("accounts.access.set_default", params, format!("Use {name}")));
    }
    actions
}

/// An account signed out keeps its accesses, which still list, and a request through one fails as
/// `access` (`signed_out`), which passed on says to sign in; signed in again, it has its token.
#[test]
fn a_signed_out_accounts_access_lists_and_says_to_sign_in() {
    let m = mock_for(3600, "/userinfo", "", &asking(&["read"], &[]));
    m.sign_in(who(json!("a"), Some("a@mock.example")), None, &[]).0.unwrap();
    m.f.plugins.sign_out_account(WITH, "a", &m.f.store).unwrap();
    let listed = m.f.plugins.accesses(&m.f.store, None, Some(&account("a"))).unwrap();
    let said: Vec<(&str, &str, bool, &[String])> =
        listed.iter().map(|a| (a.plugin.as_str(), a.name.as_str(), a.default, a.scopes.as_slice())).collect();
    assert_eq!(said, [("echo", "a@mock.example", true, ["read".to_string()].as_slice())]);
    let answer = m.fetch_as("GET", "/r/x", "{account:mock}", None);
    let failed = &answer["error"]["data"];
    assert_eq!((&failed["kind"], &failed["reason"]), (&json!("access"), &json!("signed_out")), "{answer}");
    let e = m.passed_on("access", Some("signed_out"), &failed["fields"]);
    assert_eq!((e.code(), e.reason()), ("account", Some(Reason::NotSignedIn)), "{e}");
    assert_eq!(e.detail().actions, resolving(&[]));

    m.sign_in(who(json!("a"), Some("a@mock.example")), None, &[]).0.unwrap();
    assert_eq!(m.fetch_as("GET", "/r/x", "{account:mock}", None)["result"]["body"], "/r/x Bearer at-2");
    assert_eq!(m.f.plugins.accesses(&m.f.store, Some("echo"), None).unwrap(), listed, "one access, as it was");
}

/// For an account granted what any of the mock type's scopes ask, echo's access to it and another
/// plugin's (each with any scopes, or none), signed out or not, and signed in to again (for echo or
/// not) asking for any scopes besides, the service answering a new refresh token or none: the
/// browser is asked to sign in as the account, for what the type always asks, what the account
/// was granted, what every access's scopes ask and what's asked for besides; the refresh token it
/// had is kept when the service answers none; and one signed out that no plugin has access to
/// can't be signed in to again.
#[hegel::test(test_cases = 12, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn signing_in_again_asks_for_what_every_access_needs(tc: TestCase) {
    let m = mock_for(30, "/userinfo", "", &asking(&["read"], &["write", "peek"]));
    m.state.lock().unwrap().unscoped = true;
    let first = some(&tc);
    m.sign_in_for(None, who(json!("a"), None), None, &first).0.unwrap();
    // The scopes whose asks the sign-in again asks for.
    let mut named: Vec<String> = Vec::new();
    let echo = tc.draw(gs::booleans()).then(|| some(&tc));
    if let Some(extra) = &echo {
        let extra: Vec<String> = extra.iter().map(|s| s.to_string()).collect();
        named.extend(m.f.plugins.give_access("echo", &account("a"), &extra, &m.f.store).unwrap().scopes);
    }
    let other = tc.draw(gs::booleans()).then(|| some(&tc));
    if let Some(scopes) = &other {
        let file = m.f.store.root().join("accounts.json");
        let mut kept: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
        let access = json!({ "id": "0a1b2c3d", "plugin": "other", "type": "mock", "account": account("a"),
                             "name": "a", "scopes": scopes, "default": true });
        kept["accesses"].as_array_mut().unwrap().push(access);
        std::fs::write(&file, kept.to_string()).unwrap();
        named.extend(scopes.iter().map(|s| s.to_string()));
    }
    let signed_out = tc.draw(gs::booleans());
    match signed_out {
        true => drop(m.f.plugins.sign_out_account(WITH, "a", &m.f.store).unwrap()),
        false => named.extend(first.iter().map(|s| s.to_string())),
    }
    let refreshless = tc.draw(gs::booleans());
    m.state.lock().unwrap().refreshless = refreshless;
    let for_echo = tc.draw(gs::booleans());
    let added = some(&tc);
    let (done, opened) = m.sign_in_for(for_echo.then_some("echo"), who(json!("a"), None), Some("a"), &added);
    if signed_out && echo.is_none() && other.is_none() {
        assert_eq!(done.unwrap_err().code(), "not_found");
        return;
    }
    let done = done.unwrap();
    named.extend(added.iter().map(|s| s.to_string()));
    if for_echo {
        named.push("read".into());
    }
    let mut expected: Vec<&str> = named.iter().flat_map(|s| asks(s)).copied().chain(["openid"]).collect();
    expected.sort_unstable();
    expected.dedup();
    let mut asked: Vec<&str> = opened["scope"].split(' ').collect();
    asked.sort_unstable();
    assert_eq!((asked, opened["login_hint"].as_str()), (expected, "a"));
    // Refreshed with the refresh token it had, when the service answered none.
    let refreshes = !(refreshless && signed_out);
    assert_eq!(done.account.refreshes, refreshes);
    let token = m.token("a");
    let tokens: Vec<Got> = m.server.got().into_iter().filter(|g| g.target == "/token").collect();
    match refreshes {
        true => {
            assert!(token.is_ok_and(|t| t == "at-3"), "not refreshed");
            let spent = if refreshless { "rt-1" } else { "rt-2" };
            assert!(has(&form(tokens.last().unwrap()), "refresh_token", spent), "not the refresh token it had");
        }
        false => assert_eq!(token.unwrap_err().code(), "account"),
    }
}

/// What resolves an `access` or `scope` error a plugin passes on comes from its manifest and the
/// accounts the store keeps, whatever type, plugin or account plugins its fields name.
#[test]
fn actions_come_from_the_manifest_whatever_the_plugin_says() {
    let m = mock_for(3600, "/userinfo", "", &asking(&["read"], &[]));
    m.sign_in_for(None, who(json!("a"), None), None, &[]).0.unwrap();
    let lying = json!({ "type": "evil", "for": "other", "account_plugins": ["evil-accounts"] });
    for (kind, reason) in [("access", Reason::NoAccess), ("scope", Reason::MissingScope)] {
        let e = m.passed_on(kind, None, &lying);
        assert_eq!((e.code(), e.reason()), ("account", Some(reason)), "{e}");
        assert_eq!(e.detail().actions, resolving(&["a"]), "{kind}");
        assert_eq!(e.to_string(), "it can't (the Echo plugin)");
    }
}

/// How a request echo makes through its access to the account `a` fails, in
/// [`a_request_an_access_cant_serve_fails_as_the_host_passes_it_on`].
#[derive(Debug, Clone, Copy)]
enum Failing {
    /// Echo has no access to it.
    NoAccess,
    /// It's signed out.
    SignedOut,
    /// Its token expired, and the service won't refresh it any more.
    Refused,
    /// Its token expired, and the service can't refresh it now.
    Unavailable,
    /// The account plugin it was signed in through isn't installed any more.
    Uninstalled,
    /// The access lacks the scope echo needs.
    Missing,
    /// None of the access's scopes covers the request.
    Uncovered,
    /// The account wasn't granted what the scope covering the request asks.
    Ungranted,
    /// The request names a type echo doesn't ask for.
    Unasked,
}

/// An account plugin that isn't installed.
const GONE: &str = "gone-accounts";

/// For every way a request echo makes through its access to an account can fail (no access, the
/// account signed out here or by the service, a refresh the service can't make now, its account
/// plugin gone, a scope missing, not covering or not granted, a type it doesn't ask for), its
/// token placed either way and the access named by type or by its id: the request fails before
/// it's sent, as `access` (`signed_out` when it's signed out), `scope`, `external` (the service's
/// failure) or `invalid`, an account failure naming the type and the account plugins; an account
/// the service signed out is forgotten, one it couldn't refresh kept; and echo passing an
/// `access` or `scope` failure on, whatever type it says, is an `account` error (not signed in,
/// no access, a missing scope) offering what its manifest and the store resolve it with.
#[hegel::test(test_cases = 2, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn a_request_an_access_cant_serve_fails_as_the_host_passes_it_on(tc: TestCase) {
    use Failing::*;
    for failing in [NoAccess, SignedOut, Refused, Unavailable, Uninstalled, Missing, Uncovered, Ungranted, Unasked] {
        let query = tc.draw(gs::sampled_from(vec![None, Some("access_token")]));
        let place = query.map_or(String::new(), |name| format!("token = {{ query = \"{name}\" }}"));
        let expiring = matches!(failing, Refused | Unavailable);
        let m = mock_for(if expiring { 0 } else { 3600 }, "/userinfo", &place, &asking(&["read"], &["write"]));
        m.state.lock().unwrap().unscoped = true;
        let for_echo = (!matches!(failing, NoAccess)).then_some("echo");
        m.sign_in_for(for_echo, who(json!("a"), None), None, &["read"]).0.unwrap();
        let file = m.f.store.root().join("accounts.json");
        let edit = |change: &dyn Fn(&mut Value)| {
            let mut kept: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
            change(&mut kept);
            std::fs::write(&file, kept.to_string()).unwrap();
        };
        match failing {
            SignedOut => drop(m.f.plugins.sign_out_account(WITH, "a", &m.f.store).unwrap()),
            Refused => m.state.lock().unwrap().refuses = Some(("invalid_grant".into(), "spent".into())),
            Unavailable => m.state.lock().unwrap().refuses = Some(("temporarily_unavailable".into(), "later".into())),
            Uninstalled => edit(&|kept| {
                kept["accounts"][0]["plugin"] = json!(GONE);
                kept["accesses"][0]["account"]["plugin"] = json!(GONE);
            }),
            Missing => edit(&|kept| kept["accesses"][0]["scopes"] = json!([])),
            Ungranted => drop(m.f.plugins.give_access("echo", &account("a"), &["write".into()], &m.f.store).unwrap()),
            NoAccess | Uncovered | Unasked => {}
        }
        let (method, path) = match failing {
            Uncovered => ("GET", "/o/x"),
            Ungranted => ("POST", "/w/x"),
            _ => ("GET", "/r/x"),
        };
        let id = m.f.plugins.accesses(&m.f.store, Some("echo"), None).unwrap().first().map(|a| a.id.clone());
        let by_id = tc.draw(gs::booleans());
        let named = match (by_id, id) {
            _ if matches!(failing, Unasked) => "{account:other}".to_string(),
            (true, Some(id)) => format!("{{account:mock/{id}}}"),
            _ => "{account:mock}".to_string(),
        };
        let answer = m.fetch_as(method, path, &named, query);
        let said = format!("{failing:?} {method} {path} as {named}: {answer}");
        assert!(!m.server.got().iter().any(|g| g.target.starts_with(path)), "sent: {said}");
        let e = &answer["error"]["data"];
        let (kind, reason) = match failing {
            NoAccess | Uninstalled => ("access", None),
            SignedOut | Refused => ("access", Some("signed_out")),
            Unavailable => ("external", None),
            Missing | Uncovered | Ungranted => ("scope", None),
            Unasked => ("invalid", None),
        };
        assert_eq!((e["kind"].as_str(), e["reason"].as_str()), (Some(kind), reason), "{said}");
        let kept = m.listed();
        assert_eq!(kept.is_empty(), matches!(failing, SignedOut | Refused), "{said}");
        assert!(!m.leaked(&[answer["error"].clone()]), "a token is out: {said}");
        let message = match kind {
            "external" => "signing in to Mock: temporarily_unavailable: later",
            "invalid" => "\"other\" isn't an account type it asks for",
            _ => "",
        };
        if !message.is_empty() {
            assert_eq!(answer["error"]["message"], message, "{said}");
            continue;
        }
        // Those installed when its own isn't.
        assert_eq!((&e["fields"]["type"], &e["fields"]["account_plugins"]), (&json!("mock"), &json!([WITH])), "{said}");
        let lying = tc.draw(gs::booleans());
        let fields = if lying { json!({ "type": "evil" }) } else { e["fields"].clone() };
        let passed = m.passed_on(kind, reason, &fields);
        let resolves = match reason {
            Some(_) => Reason::NotSignedIn,
            None if kind == "access" => Reason::NoAccess,
            None => Reason::MissingScope,
        };
        assert_eq!((passed.code(), passed.reason()), ("account", Some(resolves)), "{said}");
        let mut actions = resolving(&[]);
        for k in &kept {
            let params = json!({ "plugin": "echo", "account": { "plugin": k.plugin, "subject": k.subject } });
            actions.push(Action::call("accounts.access.set_default", params, format!("Use {}", k.name)));
        }
        assert_eq!(passed.detail().actions, actions, "{said}");
    }
}
