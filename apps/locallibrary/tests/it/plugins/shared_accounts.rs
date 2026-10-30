//! Accounts kept once per store, in the sidecar (`accounts.*`): signed in to through an account
//! plugin at a mock OAuth service on this computer (`LOCALLIBRARY_TEST_LOOPBACK`, the test playing
//! the browser), for the echo plugin or not, signed out of, and the echo plugin's access to them
//! made its default or taken away; and nothing answered holds a token.

use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};

use hegel::TestCase;
use hegel::generators as gs;
use serde_json::{Value, json};

use crate::mock_oauth::{Server, Service, WITH, account_plugin, service};
use crate::props_rpc::{at, contract, violation};
use crate::support::{Env, Rpc, echo_plugin};

/// The echo plugin, talking to this computer, needing `read` of a `mock` account and maybe `write`.
pub(super) fn asking() -> String {
    let accounts = "\n[[accounts]]\ntype = \"mock\"\nscopes = [\"read\"]\nwhy = \"Tests\"\n\
                    optional = [{ scopes = [\"write\"], why = \"More\" }]\n";
    echo_plugin().replace(r#""library:plan"]"#, r#""library:plan", "net:127.0.0.1"]"#) + accounts
}

/// The mock service, and the echo plugin and an account plugin for it.
pub(super) struct Mock {
    pub(super) state: Arc<Mutex<Service>>,
    pub(super) server: Server,
    pub(super) env: Env,
}

impl Mock {
    pub(super) fn new() -> Mock {
        let state = Arc::new(Mutex::new(Service { expires_in: Some(3600), ..Service::default() }));
        let server = service(state.clone());
        let mut env = Env::new();
        env.plugins(&[&asking(), &account_plugin(&server.url, &format!("{}/userinfo", server.url), "")]);
        Mock { state, server, env }
    }

    /// The command, reaching the mock service, its tokens kept in a file.
    pub(super) fn command(&self) -> Command {
        let mut c = self.env.command();
        c.env("LOCALLIBRARY_TEST_LOOPBACK", "1").env("LOCALLIBRARY_SESSION_STORE", "file");
        c
    }

    /// The address the browser ends up on once it approves the sign-in `begun` (what its
    /// `begin` answered) as `who`.
    pub(super) fn approved(&self, begun: &Value, who: &Value) -> String {
        let (url, redirect) = (begun["url"].as_str().unwrap(), begun["redirect_uri"].as_str().unwrap());
        let (_, query) = url.split_once('?').unwrap();
        let query: Vec<(String, String)> = form_urlencoded::parse(query.as_bytes()).into_owned().collect();
        let at = |key: &str| query.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone()).unwrap();
        {
            let mut s = self.state.lock().unwrap();
            (s.challenge, s.signing_in_as) = (at("code_challenge"), who.clone());
        }
        format!("{redirect}?code=c0de&state={}", at("state"))
    }

    /// [`Mock::approved`], for the sign-in whose link (to the service's `/authorize`) is `link`.
    pub(super) fn approved_link(&self, link: &str, who: &Value) -> String {
        let (_, query) = link.split_once('?').unwrap();
        let query: Vec<(String, String)> = form_urlencoded::parse(query.as_bytes()).into_owned().collect();
        let redirect = query.iter().find(|(k, _)| k == "redirect_uri").map(|(_, v)| v.clone()).unwrap();
        self.approved(&json!({ "url": link, "redirect_uri": redirect }), who)
    }

    /// Runs `command`, approving as `who` in the browser the sign-in it prints a link to (on
    /// stderr) if it does: how it exited, and what it printed to stdout and stderr.
    pub(super) fn approving(&self, mut command: Command, who: &Value) -> (i32, String, String) {
        use std::io::{BufRead, BufReader, Read};
        let mut child = command.stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
        let mut stdout = child.stdout.take().unwrap();
        let out = std::thread::spawn(move || {
            let mut out = String::new();
            stdout.read_to_string(&mut out).unwrap();
            out
        });
        let (mut err, mut browser) = (String::new(), None);
        for line in BufReader::new(child.stderr.take().unwrap()).lines() {
            let line = line.unwrap();
            err.push_str(&format!("{line}\n"));
            let link = line.trim();
            if browser.is_none() && link.starts_with("http") && link.contains("/authorize?") {
                let back = self.approved_link(link, who);
                let (to, query) = back.split_once('?').unwrap();
                browser = Some(browser_back(to, query.into()));
            }
        }
        let code = child.wait().unwrap().code().unwrap();
        if let Some(browser) = browser {
            browser.join().unwrap();
        }
        (code, out.join().unwrap(), err)
    }

    /// Whether any token the service gave is in `text`.
    pub(super) fn leaked(&self, text: &str) -> bool {
        let issued = self.state.lock().unwrap().issued;
        (1..=issued).any(|n| text.contains(&format!("at-{n}")) || text.contains(&format!("rt-{n}")))
    }
}

/// Comes back to `redirect_uri` (`http://127.0.0.1:<port>/...`) with `query`, as the browser
/// does, once it's waited for: what this computer answered it.
pub(super) fn browser_back(redirect_uri: &str, query: String) -> std::thread::JoinHandle<String> {
    let rest = redirect_uri.strip_prefix("http://").unwrap().to_string();
    std::thread::spawn(move || {
        use std::io::{Read, Write};
        let (host, path) = rest.split_once('/').unwrap();
        let mut stream = std::net::TcpStream::connect(host).unwrap();
        write!(stream, "GET /{path}?{query} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n").unwrap();
        let mut answer = String::new();
        stream.read_to_string(&mut answer).unwrap();
        answer
    })
}

/// Who the service says signed in, by subject and email.
fn who(subject: &str) -> Value {
    json!({ "sub": subject, "email": format!("{subject}@mock.example") })
}

/// An account as `accounts.list` names it.
fn account(subject: &str) -> Value {
    json!({ "plugin": WITH, "subject": subject })
}

/// The sidecar signs in to an account for the echo plugin (giving it access, its default) and
/// lists it with that access; signs out of it, which leaves the access, the account listed as
/// signed out; and takes the access away, after which neither is listed. Nothing answered holds a
/// token.
#[test]
fn the_sidecar_signs_in_for_a_plugin_and_lists_its_access() {
    let m = Mock::new();
    let mut s = Rpc::start(m.command());
    assert_eq!(s.call(1, "accounts.list", json!({}))["result"], json!({ "accounts": [], "accesses": [] }));
    let begun = s.call(2, "accounts.sign_in.begin", json!({ "type": "mock", "for": "echo" }));
    let r = &begun["result"];
    assert_eq!((&r["type"], &r["plugin"]), (&json!("mock"), &json!(WITH)), "{begun}");
    let url = m.approved(r, &who("a"));
    let done = s.call(3, "accounts.sign_in.paste", json!({ "id": r["id"], "url": url }));
    let done = &done["result"];
    assert_eq!(
        (&done["subject"], &done["name"], &done["signed_in"]),
        (&json!("a"), &json!("a@mock.example"), &json!(true))
    );
    let access = &done["access"];
    assert_eq!(
        (&access["plugin"], &access["account"], &access["scopes"]),
        (&json!("echo"), &account("a"), &json!(["read"]))
    );
    assert_eq!(access["default"], true, "its first access of the type");

    let listed = s.call(4, "accounts.list", json!({}));
    assert_eq!(listed["result"]["accesses"], json!([access]));
    assert_eq!(listed["result"]["accounts"][0]["subject"], "a");
    assert_eq!(s.call(5, "accounts.access.list", json!({ "plugin": "echo" }))["result"], json!([access]));
    assert_eq!(s.call(6, "accounts.access.list", json!({ "plugin": "other" }))["result"], json!([]));

    let out = s.call(7, "accounts.sign_out", json!({ "account": account("a") }));
    assert_eq!((&out["result"]["revoked"], &out["result"]["signed_in"]), (&json!(true), &json!(true)), "{out}");
    assert!(m.server.got().iter().any(|g| g.target == "/revoke"), "the service was told");
    let listed = s.call(8, "accounts.list", json!({}));
    let expected = json!({ "type": "mock", "plugin": WITH, "subject": "a", "name": "a@mock.example", "scopes": [], "refreshes": false, "signed_in": false });
    assert_eq!(listed["result"]["accounts"], json!([expected]), "signed out, listed for its access");
    let again = s.call(9, "accounts.sign_out", json!({ "account": account("a") }));
    assert_eq!(again["error"]["data"]["kind"], "not_found", "{again}");

    let taken = s.call(10, "accounts.access.take_away", json!({ "plugin": "echo", "account": account("a") }));
    assert_eq!(taken["result"], *access);
    assert_eq!(s.call(11, "accounts.list", json!({}))["result"], json!({ "accounts": [], "accesses": [] }));
    let answers = [begun.to_string(), listed.to_string(), out.to_string(), taken.to_string()].concat();
    assert!(!m.leaked(&answers), "an answer holds a token");
}

/// Signing in names the account plugin when the type has one; a type none provides, or a plugin
/// that doesn't ask for it, is refused before the browser is sent anywhere.
#[test]
fn a_sign_in_the_sidecar_cant_begin_says_why() {
    let m = Mock::new();
    let mut s = Rpc::start(m.command());
    let none = s.call(1, "accounts.sign_in.begin", json!({ "type": "nope" }));
    assert_eq!(none["error"]["data"]["kind"], "not_found", "{none}");
    assert!(none["error"]["message"].as_str().unwrap().contains("an account plugin providing nope accounts"), "{none}");
    let other = s.call(2, "accounts.sign_in.begin", json!({ "type": "mock", "for": "echo", "scopes": ["peek"] }));
    assert!(other["error"]["message"].as_str().unwrap().contains("doesn't ask for the mock scope \"peek\""), "{other}");
    let named = s.call(3, "accounts.sign_in.begin", json!({ "type": "mock", "with": WITH }));
    assert_eq!(named["result"]["plugin"], WITH, "{named}");
    let finished = s.call(4, "accounts.sign_in.finish", json!({ "id": "never", "timeout_secs": 0 }));
    assert_eq!(finished["error"]["data"]["kind"], "not_found", "{finished}");
}

/// An access error the echo plugin passes on carries actions: each names a method the sidecar
/// has, with parameters its contract takes, and calling it does what its label says.
#[test]
fn the_actions_of_an_access_error_are_methods_the_sidecar_has() {
    let m = Mock::new();
    let mut s = Rpc::start(m.command());
    let begun = s.call(1, "accounts.sign_in.begin", json!({ "type": "mock" }));
    let url = m.approved(&begun["result"], &who("a"));
    s.call(2, "accounts.sign_in.paste", json!({ "id": begun["result"]["id"], "url": url }));
    let failing = json!({ "fail": "access", "message": "it can't", "fields": { "type": "mock" } });
    let e = s.call(3, "plugins.run", json!({ "plugin": "echo", "command": "echo", "params": failing }));
    assert_eq!((&e["error"]["data"]["kind"], &e["error"]["data"]["reason"]), (&json!("account"), &json!("no_access")));
    let actions = e["error"]["data"]["actions"].as_array().unwrap();
    let methods: Vec<&str> = actions.iter().map(|a| a["method"].as_str().unwrap()).collect();
    assert_eq!(methods, ["accounts.sign_in.begin", "accounts.access.set_default"], "{e}");
    for (i, a) in actions.iter().enumerate() {
        let name = a["method"].as_str().unwrap();
        let described = contract()["methods"].as_array().unwrap().iter().find(|m| m["name"] == name).unwrap();
        assert_eq!(violation(&a["params"], &format!("{}/params", at(described))), None, "{name}");
        let answer = s.call(10 + i as i64, name, a["params"].clone());
        assert!(answer["error"].is_null(), "{name}: {answer}");
    }
    let accesses = s.call(20, "accounts.access.list", json!({ "plugin": "echo" }))["result"].clone();
    assert_eq!((&accesses[0]["account"], &accesses[0]["default"]), (&account("a"), &json!(true)), "{accesses}");
}

/// Who the steps of the property tests here are about, by subject.
pub(super) const PEOPLE: [&str; 2] = ["a", "b"];

/// What the store keeps, as the model has it.
#[derive(Default)]
pub(super) struct Model {
    /// Subjects signed in, in the order they were first signed in since they were last signed
    /// out of.
    kept: Vec<&'static str>,
    /// The echo plugin's accesses, by subject, in the order they were given, and whether each is
    /// its default.
    accesses: Vec<(&'static str, bool)>,
}

impl Model {
    pub(super) fn kept(&self, subject: &str) -> bool {
        self.kept.contains(&subject)
    }

    pub(super) fn has_access(&self, subject: &str) -> bool {
        self.accesses.iter().any(|(a, _)| *a == subject)
    }

    /// `subject` signed in, for the echo plugin or not: given access when it's for echo and it
    /// has none, its default when it's its first.
    pub(super) fn signed_in(&mut self, subject: &'static str, for_echo: bool) {
        if !self.kept(subject) {
            self.kept.push(subject);
        }
        if for_echo && !self.has_access(subject) {
            let first = self.accesses.is_empty();
            self.accesses.push((subject, first));
        }
    }

    /// `subject` signed out of: whether it was kept.
    pub(super) fn signed_out(&mut self, subject: &str) -> bool {
        let was = self.kept(subject);
        self.kept.retain(|k| *k != subject);
        was
    }

    /// `subject` made echo's default, given access first: whether it could be (it's kept, or
    /// echo has access to it).
    pub(super) fn made_default(&mut self, subject: &'static str) -> bool {
        if !self.has_access(subject) && !self.kept(subject) {
            return false;
        }
        if !self.has_access(subject) {
            self.accesses.push((subject, false));
        }
        self.accesses.iter_mut().for_each(|(a, default)| *default = *a == subject);
        true
    }

    /// Echo's access to `subject` taken away: whether it had one. When it was its default and one
    /// is left, that one is.
    pub(super) fn taken_away(&mut self, subject: &str) -> bool {
        let Some(at) = self.accesses.iter().position(|(a, _)| *a == subject) else { return false };
        let (_, was_default) = self.accesses.remove(at);
        if let [only] = self.accesses.as_mut_slice() {
            only.1 |= was_default;
        }
        true
    }

    /// The accounts listed (by subject, and whether each is signed in) and the accesses (by
    /// subject, and whether each is the default), as `accounts.list` answers them.
    pub(super) fn listed(&self) -> Value {
        let signed_out = self.accesses.iter().map(|(a, _)| *a).filter(|a| !self.kept(a));
        let accounts: Vec<Value> =
            self.kept.iter().map(|k| json!([k, true])).chain(signed_out.map(|a| json!([a, false]))).collect();
        let accesses: Vec<Value> = self.accesses.iter().map(|(a, d)| json!([a, d])).collect();
        json!({ "accounts": accounts, "accesses": accesses })
    }
}

/// `listed` (what `accounts.list` answers, or `account --json` prints) as [`Model::listed`]
/// has it.
pub(super) fn shape(listed: &Value) -> Value {
    let pairs = |key: &str, a: &str, b: &str| -> Vec<Value> {
        let all = listed[key].as_array().cloned().unwrap_or_default();
        all.iter().map(|x| json!([x.pointer(a), x.pointer(b)])).collect()
    };
    json!({
        "accounts": pairs("accounts", "/subject", "/signed_in"),
        "accesses": pairs("accesses", "/account/subject", "/default"),
    })
}

/// A step of [`accounts_and_accesses_are_as_the_calls_made_them`], naming people by their place
/// in [`PEOPLE`].
#[derive(Debug, Clone, Copy)]
enum Step {
    /// Signing in, for the echo plugin or not, again as the account when it's kept, the browser
    /// coming back to this computer or the address it ended up on pasted.
    SignIn {
        who: usize,
        for_echo: bool,
        pasted: bool,
    },
    /// Begun, then cancelled.
    Cancelled,
    SignOut(usize),
    SetDefault(usize),
    TakeAway(usize),
}

/// For any sequence of sidecar calls signing in (for the echo plugin or not, coming back or
/// pasted), cancelling, signing out, making an access the default and taking it away: the
/// accounts and accesses `accounts.list` answers are those the model keeps, the echo plugin has
/// one default while it has any access, a call about what isn't there is `not_found`, and no
/// answer holds a token.
#[hegel::test(test_cases = 8, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn accounts_and_accesses_are_as_the_calls_made_them(tc: TestCase) {
    let m = Mock::new();
    let mut s = Rpc::start(m.command());
    let mut model = Model::default();
    let mut answers = String::new();
    let n = tc.draw(gs::integers::<usize>().min_value(2).max_value(8));
    for i in 0..n {
        let person = tc.draw(gs::integers::<usize>().max_value(1));
        let step = match tc.draw(gs::integers::<u8>().max_value(5)) {
            0 | 1 => Step::SignIn { who: person, for_echo: tc.draw(gs::booleans()), pasted: tc.draw(gs::booleans()) },
            2 => Step::Cancelled,
            3 => Step::SignOut(person),
            4 => Step::SetDefault(person),
            _ => Step::TakeAway(person),
        };
        let id = i as i64 * 10;
        let subject = match step {
            Step::SignIn { who, .. } | Step::SignOut(who) | Step::SetDefault(who) | Step::TakeAway(who) => PEOPLE[who],
            Step::Cancelled => "",
        };
        let not_found = |answer: &Value| assert_eq!(answer["error"]["data"]["kind"], "not_found", "step {i}: {answer}");
        let answer = match step {
            Step::SignIn { for_echo, pasted, .. } => {
                let mut params = json!({ "type": "mock" });
                if for_echo {
                    params["for"] = json!("echo");
                }
                if model.kept(subject) {
                    params["account"] = json!(subject);
                }
                let begun = s.call(id, "accounts.sign_in.begin", params);
                let url = m.approved(&begun["result"], &who(subject));
                let sign_in = &begun["result"]["id"];
                let done = match pasted {
                    true => s.call(id + 1, "accounts.sign_in.paste", json!({ "id": sign_in, "url": url })),
                    false => {
                        let (redirect, query) = url.split_once('?').unwrap();
                        let browser = browser_back(redirect, query.into());
                        let finish = json!({ "id": sign_in, "timeout_secs": 10 });
                        let done = s.call(id + 1, "accounts.sign_in.finish", finish);
                        assert!(browser.join().unwrap().contains("is signed in"), "step {i}: told the browser");
                        done
                    }
                };
                let r = &done["result"];
                assert_eq!((&r["subject"], &r["signed_in"]), (&json!(subject), &json!(true)), "step {i}: {done}");
                assert_eq!(r["access"].is_object(), for_echo, "step {i}: {done}");
                model.signed_in(subject, for_echo);
                done
            }
            Step::Cancelled => {
                let begun = s.call(id, "accounts.sign_in.begin", json!({ "type": "mock" }));
                let cancelled = s.call(id + 1, "accounts.sign_in.cancel", json!({ "id": begun["result"]["id"] }));
                assert_eq!(cancelled["result"], json!({ "cancelled": true }), "step {i}");
                cancelled
            }
            Step::SignOut(_) => {
                let out = s.call(id, "accounts.sign_out", json!({ "account": account(subject) }));
                match model.signed_out(subject) {
                    true => assert_eq!(out["result"]["revoked"], true, "step {i}: {out}"),
                    false => not_found(&out),
                }
                out
            }
            Step::SetDefault(_) => {
                let set =
                    s.call(id, "accounts.access.set_default", json!({ "plugin": "echo", "account": account(subject) }));
                match model.made_default(subject) {
                    true => assert_eq!(set["result"]["default"], true, "step {i}: {set}"),
                    false => not_found(&set),
                }
                set
            }
            Step::TakeAway(_) => {
                let taken =
                    s.call(id, "accounts.access.take_away", json!({ "plugin": "echo", "account": account(subject) }));
                if !model.taken_away(subject) {
                    not_found(&taken);
                }
                taken
            }
        };
        answers.push_str(&answer.to_string());
        let listed = s.call(id + 5, "accounts.list", json!({}));
        assert_eq!(shape(&listed["result"]), model.listed(), "step {i}: {step:?}");
        answers.push_str(&listed.to_string());
    }
    assert!(!m.leaked(&answers), "an answer holds a token");
}
