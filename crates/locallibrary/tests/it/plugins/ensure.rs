//! What more of an account a plugin may use (`accounts/ensure`): what its access has and the
//! service granted, not what the user declined before, and otherwise what the user says when the
//! front end can ask them, signing in again when the service hasn't granted it; with nobody to
//! ask, an error saying how to give it. Never a token.

use std::net::TcpListener;
use std::sync::OnceLock;
use std::thread::JoinHandle;

use hegel::TestCase;
use hegel::generators as gs;
use indexmap::IndexMap;
use locallibrary::error::Reason;
use locallibrary::plugin::{AccessQuestion, AskAccess};

use super::accesses::{SCOPES, account, asks, some};
use super::accounts::{come_back_to, opened_at};
use super::key_accounts::entered;
use super::shared_accounts::{Losing, Mock, WITH, asking, edit_json, mock_with, options, who};
use super::*;
use crate::mock_oauth::{Service, account_plugin, keys_account_plugin};

/// A front end asking as the test says: answering `answer` to every question (`None`: it can't
/// ask), and signing in again in the browser with the mock service granting `grants`.
#[derive(Default)]
struct Script {
    answer: Mutex<Option<bool>>,
    grants: Mutex<Vec<String>>,
    asked: Mutex<Vec<AccessQuestion>>,
    service: OnceLock<Arc<Mutex<Service>>>,
    browser: Mutex<Vec<JoinHandle<String>>>,
}

impl AskAccess for Script {
    fn allow(&self, question: &AccessQuestion) -> Option<bool> {
        self.asked.lock().unwrap().push(question.clone());
        *self.answer.lock().unwrap()
    }

    fn sign_in(&self, url: &str) {
        let query = opened_at(url);
        {
            let mut s = self.service.get().unwrap().lock().unwrap();
            (s.challenge, s.signing_in_as) = (query["code_challenge"].clone(), who(json!("a"), None));
            s.grants = Some(self.grants.lock().unwrap().clone());
        }
        let back = come_back_to(&query["redirect_uri"], format!("code=c0de&state={}", query["state"]));
        self.browser.lock().unwrap().push(back);
    }
}

/// Echo's access to the account `a`, as the test models it: its scopes, those declined, and what
/// the service granted the account.
#[derive(Debug, Default)]
struct Model {
    scopes: Vec<&'static str>,
    declined: Vec<&'static str>,
    granted: Vec<&'static str>,
}

impl Model {
    /// Whether the service granted what `scope` asks.
    fn serviced(&self, scope: &str) -> bool {
        asks(scope).iter().all(|a| self.granted.contains(a))
    }

    /// Whether echo may use `scope`: its access has it, and the service granted what it asks.
    fn usable(&self, scope: &str) -> bool {
        self.scopes.contains(&scope) && self.serviced(scope)
    }

    /// A sign-in for echo, giving it `given` (the scopes it needs and those asked for), its
    /// service granting `grants`: it has those it needs, and the others it was granted what they
    /// ask, and remembers those it wasn't.
    fn signed_in(&mut self, needs: &[&'static str], given: &[&'static str], grants: Vec<&'static str>) {
        self.granted = grants;
        let mut given = given.to_vec();
        given.sort_unstable();
        given.dedup();
        let declined: Vec<&str> = given.iter().copied().filter(|s| !self.serviced(s)).collect();
        let giving = given.iter().copied().filter(|s| needs.contains(s) || !declined.contains(s));
        self.declined.retain(|s| !given.contains(s));
        for s in giving {
            if !self.scopes.contains(&s) {
                self.scopes.push(s);
            }
        }
        self.declined.extend(declined.into_iter().filter(|s| !self.declined.contains(s)).collect::<Vec<_>>());
    }
}

/// How the front end answers in [`ensure_answers_granted_or_declined_never_a_token`].
#[derive(Debug, Clone, Copy, PartialEq)]
enum Answer {
    /// There's no front end to ask.
    Nobody,
    /// The front end can't ask here.
    Cannot,
    Allow,
    Decline,
}

/// `scopes`, sorted, to compare as a set.
fn sorted(scopes: &[&str]) -> Vec<String> {
    let mut all: Vec<String> = scopes.iter().map(|s| s.to_string()).collect();
    all.sort_unstable();
    all
}

/// For an echo plugin needing any of the mock type's scopes (the rest optional), given an account
/// signed in for it (the service granting any of what they ask), maybe signed out or its session
/// ended since, and asking about any scopes in turn, the user answering each time as drawn and the
/// service granting any of what they ask when they sign in again: a scope its access has and the
/// account was granted what it asks is granted; one declined before is declined unasked; any other
/// is asked about, and allowed it's given (signing in again first unless the account was granted
/// what it asks), declined it's remembered, and with nobody to ask the plugin fails as `scope`
/// saying how to give them. Once the account is lost, echo has no access to ask about: it fails as
/// `no_access`, and nobody is asked. One its manifest doesn't ask for is refused. No token gets
/// out.
#[hegel::test(test_cases = 16, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn ensure_answers_granted_or_declined_never_a_token(tc: TestCase) {
    let mut needs = some(&tc);
    if needs.is_empty() {
        needs.push(tc.draw(gs::sampled_from(SCOPES.to_vec())));
    }
    let optional: Vec<&str> = SCOPES.into_iter().filter(|s| !needs.contains(s)).collect();
    let script = Arc::new(Script::default());
    // Mostly a front end that asks.
    let nobody = tc.draw(gs::sampled_from(vec![false, false, true]));
    let ask = (!nobody).then(|| script.clone() as Arc<dyn AskAccess>);
    let manifest = |server: &str| account_plugin(server, &format!("{server}/userinfo"), "");
    let m: Mock = mock_with(3600, manifest, &asking(&needs, &optional), Options { ask, ..options() });
    assert!(script.service.set(m.state.clone()).is_ok());
    let grants = || ["read", "write"].into_iter().filter(|_| tc.draw(gs::booleans())).collect::<Vec<&str>>();

    let mut model = Model::default();
    let first = grants();
    m.state.lock().unwrap().grants = Some(first.iter().map(|s| s.to_string()).collect());
    let asked: Vec<&str> = optional.iter().copied().filter(|_| tc.draw(gs::booleans())).collect();
    m.sign_in(who(json!("a"), None), None, &asked).0.unwrap();
    model.signed_in(&needs, &[needs.clone(), asked].concat(), first);
    let lost = tc.draw(gs::sampled_from(vec![None, Some(Losing::SignOut), Some(Losing::SessionEnds)]));
    if let Some(how) = lost {
        m.lose("a", how);
    }

    for step in 0..tc.draw(gs::integers::<usize>().min_value(1).max_value(4)) {
        let scopes = some(&tc);
        let unasked = tc.draw(gs::booleans()) && step == 0;
        let answer = match nobody {
            true => Answer::Nobody,
            false => tc.draw(gs::sampled_from(vec![Answer::Cannot, Answer::Allow, Answer::Decline])),
        };
        *script.answer.lock().unwrap() = match answer {
            Answer::Allow => Some(true),
            Answer::Decline => Some(false),
            _ => None,
        };
        let again = grants();
        *script.grants.lock().unwrap() = again.iter().map(|s| s.to_string()).collect();
        let named: Vec<&str> = scopes.iter().copied().chain(unasked.then_some("nope")).collect();
        let before = script.asked.lock().unwrap().len();
        let done = m.f.command("ensure", json!({ "type": "mock", "scopes": named }), &Ctx::silent());
        script.browser.lock().unwrap().drain(..).for_each(|b| drop(b.join().unwrap()));
        let said = format!("step {step}, {answer:?} {scopes:?} with {model:?}");
        if unasked {
            assert_eq!(done.unwrap_err().code(), "invalid", "{said}");
            continue;
        }
        if lost.is_some() {
            let e = done.unwrap_err();
            assert_eq!((e.code(), e.reason()), ("account", Some(Reason::NoAccess)), "{said} lost: {e}");
            assert_eq!(script.asked.lock().unwrap().len(), before, "{said} lost: asked");
            assert!(!m.leaked(&[json!(e.to_string())]), "a token is out: {said}");
            continue;
        }
        let open: Vec<&str> =
            scopes.iter().copied().filter(|s| !model.usable(s) && !model.declined.contains(s)).collect();
        let sign_in = !open.iter().all(|s| model.serviced(s));
        let questions = script.asked.lock().unwrap()[before..].to_vec();
        match (open.is_empty(), answer) {
            (true, _) | (_, Answer::Nobody) => assert!(questions.is_empty(), "{said}: asked {questions:?}"),
            _ => {
                let [q] = questions.as_slice() else { panic!("{said}: asked {questions:?}") };
                assert_eq!((&q.plugin, &q.access.name, q.sign_in), (&"Echo".to_string(), &"a".to_string(), sign_in));
                let open: Vec<String> = open.iter().map(|s| s.to_string()).collect();
                assert_eq!((&q.scopes, &q.titles), (&open, &open), "{said}");
            }
        }
        match answer {
            _ if open.is_empty() => {}
            Answer::Nobody | Answer::Cannot => {
                let e = done.as_ref().unwrap_err();
                assert_eq!((e.code(), e.reason()), ("account", Some(Reason::MissingScope)), "{said}: {e}");
                let actions = e.detail().actions;
                let offered: Vec<(&str, Vec<String>)> = actions
                    .iter()
                    .filter(|a| a.params.get("account").is_some_and(|x| x == "a" || x == &json!(account("a"))))
                    .map(|a| (a.method.as_deref().unwrap_or_default(), scopes_of(&a.params)))
                    .collect();
                assert!(!offered.is_empty(), "{said}: nothing offered for a: {e:?}");
                let covered: Vec<String> = offered.iter().flat_map(|(_, s)| s.clone()).collect();
                assert!(open.iter().all(|s| covered.contains(&s.to_string())), "{said}: {offered:?}");
                assert!(!m.leaked(&[json!(e.to_string())]), "a token is out: {said}");
                continue;
            }
            Answer::Decline => model.declined.extend(open.iter().copied()),
            Answer::Allow if sign_in => model.signed_in(&needs, &[needs.clone(), open.clone()].concat(), again),
            Answer::Allow => {
                model.scopes.extend(open.iter().copied());
                model.declined.retain(|s| !open.contains(s));
            }
        }
        let answered = done.unwrap();
        let CommandOutput::Headless(answered) = answered else { panic!("{said}: {answered:?}") };
        let (granted, declined): (Vec<&str>, Vec<&str>) = scopes.iter().copied().partition(|s| model.usable(s));
        let listed = |key: &str| answered[key].as_array().unwrap().iter().map(|s| s.as_str().unwrap()).collect();
        let (said_granted, said_declined): (Vec<&str>, Vec<&str>) = (listed("granted"), listed("declined"));
        assert_eq!((sorted(&said_granted), sorted(&said_declined)), (sorted(&granted), sorted(&declined)), "{said}");
        assert!(!m.leaked(&[answered]), "a token is out: {said}");
        let access = &m.f.plugins.accesses(&m.f.store, Some("echo"), None).unwrap()[0];
        let have: Vec<&str> = access.scopes.iter().map(String::as_str).collect();
        let left_out: Vec<&str> = access.declined.iter().map(String::as_str).collect();
        assert_eq!((sorted(&have), sorted(&left_out)), (sorted(&model.scopes), sorted(&model.declined)), "{said}");
    }
}

/// For an account signed in to with either of two pairs of keys and signed out since, a plugin
/// wanting more of it has no access to it any more: it fails as `no_access`, nobody asked. Signed
/// in to again with other keys for the plugin, its access is back, and the user allowing more gives
/// it the scope without signing in again (its scopes ask the service for nothing); a request
/// through it gets the new keys.
#[hegel::test(test_cases = 2, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn prop_ensure_signs_in_again_with_the_keys_entered(tc: TestCase) {
    let script = Arc::new(Script { answer: Mutex::new(Some(true)), ..Script::default() });
    let ask = Some(script.clone() as Arc<dyn AskAccess>);
    let manifest = |server: &str| keys_account_plugin(server, None);
    let m: Mock = mock_with(3600, manifest, &asking(&["read"], &["write"]), Options { ask, ..options() });
    let first = tc.draw(gs::sampled_from(vec![("ak-0123456789", "sk-9876543210"), ("ak-3333333333", "sk-4444444444")]));
    let (old, new) = (entered(first.0, first.1), entered("ak-1111111111", "sk-2222222222"));
    let sign_in = |keys: &IndexMap<String, String>, again: Option<&str>| {
        let start = m.f.plugins.begin_account_sign_in("mock", WITH, Some("echo"), again, &[], None, &m.f.store);
        m.f.plugins.finish_account_sign_in_with_keys(&start.unwrap().id, keys, Some("me")).unwrap()
    };
    let had = sign_in(&old, None).access.unwrap();
    m.f.plugins.sign_out_account(WITH, "me", &m.f.store).unwrap();
    let ensure = || m.f.command("ensure", json!({ "type": "mock", "scopes": ["write"] }), &Ctx::silent());

    let e = ensure().unwrap_err();
    assert_eq!((e.code(), e.reason()), ("account", Some(Reason::NoAccess)), "{e}");
    assert!(script.asked.lock().unwrap().is_empty(), "asked");
    assert!(m.listed().is_empty(), "signed in without keys");

    assert_eq!(sign_in(&new, Some("me")).restored, [had], "not given back");
    let CommandOutput::Headless(answered) = ensure().unwrap() else { panic!("not an answer") };
    assert_eq!((&answered["granted"], &answered["declined"]), (&json!(["write"]), &json!([])));
    let sent = m.fetch_as("POST", "/w/x", "{account:mock}", None);
    assert_eq!(sent["result"]["body"], "/w/x LOW ak-1111111111:sk-2222222222", "{sent}");
}

/// A front end that allows whatever it's asked, after forgetting every access the store at
/// `forgets` keeps when it's given. It never signs in in the browser.
#[derive(Default)]
struct Allowing {
    forgets: OnceLock<PathBuf>,
}

impl AskAccess for Allowing {
    fn allow(&self, _: &AccessQuestion) -> Option<bool> {
        if let Some(root) = self.forgets.get() {
            edit_json(&root.join("accounts.json"), |kept| kept["accesses"] = json!([]));
        }
        Some(true)
    }

    fn sign_in(&self, url: &str) {
        panic!("asked to sign in at {url}");
    }
}

/// How `accounts/ensure` goes in [`prop_ensure_answers_what_it_can_and_says_why_not`].
#[derive(Debug, Clone, Copy, PartialEq)]
enum Ensuring {
    /// Asked what isn't an `accounts/ensure`.
    Unreadable,
    /// The plugin has no access to an account of the type.
    NoAccess,
    /// It runs without a store.
    NoStore,
    /// Its access is taken away while the user is asked.
    TakenAway,
    /// The account was granted what the scope asks: allowed, the access is given it.
    Granted,
    /// The user declines: the access remembers it.
    Declined,
    /// The account wasn't granted what the scope asks: allowed, it's signed in again in the
    /// browser.
    Browser,
    /// The account wasn't granted what the scope asks, and its type's port is held.
    PortHeld,
    /// The account, signed in to with keys, is signed out, which took the access with it.
    SignedOut,
    /// The account's session ended, which took the access with it.
    SessionEnded,
}

/// For an echo plugin needing `read` of an account (signed in to as anyone) and asking for
/// `write` besides, a front end allowing whatever it's asked: an `accounts/ensure` that can't be
/// read is refused; without an access (none given, or the account signed out or its session
/// ended), or without a store, it fails as a request would; an access taken away while the user
/// is asked fails it too; a scope the account was granted is given as the user allows, and
/// remembered as declined as they decline; one it wasn't granted is signed in again for in the
/// browser, given nothing when the browser signs in as another; and where signing in again can't
/// begin, it says why.
#[hegel::test(test_cases = 2, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn prop_ensure_answers_what_it_can_and_says_why_not(tc: TestCase) {
    use Ensuring::*;
    let subject = tc.draw(gs::sampled_from(vec!["a", "42"]));
    for ensuring in
        [Unreadable, NoAccess, NoStore, TakenAway, Granted, Declined, Browser, PortHeld, SignedOut, SessionEnded]
    {
        let allowing = Arc::new(Allowing::default());
        let script = Arc::new(Script { answer: Mutex::new(Some(ensuring == Browser)), ..Script::default() });
        *script.grants.lock().unwrap() = vec!["read".into(), "write".into()];
        let ask = match ensuring {
            Declined | Browser => Some(script.clone() as Arc<dyn AskAccess>),
            _ => Some(allowing.clone() as Arc<dyn AskAccess>),
        };
        let port = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        let manifest = |server: &str| match ensuring {
            SignedOut => keys_account_plugin(server, None),
            PortHeld => account_plugin(server, &format!("{server}/userinfo"), &format!("redirect_port = {port}")),
            _ => account_plugin(server, &format!("{server}/userinfo"), ""),
        };
        let m: Mock = mock_with(3600, manifest, &asking(&["read"], &["write"]), Options { ask, ..options() });
        assert!(script.service.set(m.state.clone()).is_ok());
        let plugin = (!matches!(ensuring, NoAccess)).then_some("echo");
        if matches!(ensuring, Browser | PortHeld) {
            m.state.lock().unwrap().grants = Some(vec!["read".into()]);
        }
        match ensuring {
            SignedOut => {
                let start =
                    m.f.plugins.begin_account_sign_in("mock", WITH, plugin, None, &[], None, &m.f.store).unwrap();
                m.f.plugins
                    .finish_account_sign_in_with_keys(
                        &start.id,
                        &entered("ak-0123456789", "sk-9876543210"),
                        Some(subject),
                    )
                    .unwrap();
            }
            _ => drop(m.sign_in_for(plugin, who(json!(subject), None), None, &[]).0.unwrap()),
        }
        match ensuring {
            SignedOut => m.lose(subject, Losing::SignOut),
            SessionEnded => m.lose(subject, Losing::SessionEnds),
            _ => {}
        }
        if let TakenAway = ensuring {
            allowing.forgets.set(m.f.store.root().into()).unwrap();
        }
        let _held = matches!(ensuring, PortHeld).then(|| TcpListener::bind(("127.0.0.1", port)).unwrap());
        let asked = json!({ "type": "mock", "scopes": ["write"] });
        let done = match ensuring {
            Unreadable => {
                let answer = m.f.echo_params(json!({ "call": "accounts/ensure", "params": { "type": 1 } })).unwrap();
                assert!(answer["result"].is_null() && answer["error"]["message"].is_string(), "{answer}");
                continue;
            }
            NoStore => m.f.plugins.run_command("echo", "ensure", asked, m.f.context(), None, &Ctx::silent(), &|_| {}),
            _ => m.f.command("ensure", asked, &Ctx::silent()),
        };
        script.browser.lock().unwrap().drain(..).for_each(|b| drop(b.join().unwrap()));
        let said = format!("{ensuring:?} as {subject}");
        if matches!(ensuring, Granted | Declined | Browser) {
            let CommandOutput::Headless(answered) = done.unwrap() else { panic!("{said}: not an answer") };
            // The browser signs in as `a`: as another account, the access is given nothing.
            let expected = match ensuring {
                Declined => (json!([]), json!(["write"])),
                Browser if subject != "a" => (json!([]), json!(["write"])),
                _ => (json!(["write"]), json!([])),
            };
            assert_eq!((answered["granted"].clone(), answered["declined"].clone()), expected, "{said}");
            continue;
        }
        let e = done.unwrap_err();
        let expected = match ensuring {
            NoAccess | NoStore | TakenAway | SignedOut | SessionEnded => ("account", Some(Reason::NoAccess)),
            // The browser can't come back: the plugin is told why, as any other failure.
            _ => ("external", None),
        };
        assert_eq!((e.code(), e.reason()), expected, "{said}: {e}");
        if let PortHeld = ensuring {
            assert!(e.to_string().contains("can't listen for the browser"), "{said}: {e}");
        }
    }
}

/// The scopes an action's `params` name.
fn scopes_of(params: &serde_json::Map<String, Value>) -> Vec<String> {
    let scopes = params.get("scopes").and_then(Value::as_array).into_iter().flatten();
    scopes.filter_map(Value::as_str).map(String::from).collect()
}
