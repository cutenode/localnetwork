//! Accesses: the scopes of an account of a type a plugin is given, at a sign-in for it or as the
//! user says, its default for the type answering `{account:<type>}`. Its token goes into a request
//! only where one of the access's scopes covers it and the account was granted what that scope
//! asks, and only while the access has every scope the plugin needs; anything else fails before a
//! request is sent, and an error the plugin passes on is resolved as its manifest says.

use hegel::TestCase;
use hegel::generators as gs;
use locallibrary::error::{Action, Reason};
use locallibrary::plugin::AccountId;

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
    fn fetch_as(&self, method: &str, path: &str, named: &str, query: Option<&str>) -> Value {
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
