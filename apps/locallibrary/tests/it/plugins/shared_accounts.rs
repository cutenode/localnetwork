//! Accounts kept once per store, in the sidecar (`accounts.*`): signed in to through an account
//! plugin at a mock OAuth service on this computer (`LOCALLIBRARY_TEST_LOOPBACK`, the test playing
//! the browser), for the echo plugin or not, signed out of, and the echo plugin's access to them
//! made its default or taken away; and nothing answered holds a token.

use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};

use hegel::TestCase;
use hegel::generators as gs;
use locallibrary::plugin::Restoring;
use serde_json::{Value, json};

use crate::mock_oauth::{Server, Service, WITH, account_plugin, holds_token, keys_account_plugin, service};
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

    /// [`Mock::new`], its account plugin's `mock` accounts signed in to with an access and a secret
    /// key that the user names (see [`keys_account_plugin`]).
    pub(super) fn keyed() -> Mock {
        let state = Arc::new(Mutex::new(Service::default()));
        let server = service(state.clone());
        let mut env = Env::new();
        env.plugins(&[&asking(), &keys_account_plugin(&server.url, None)]);
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
        holds_token(text, self.state.lock().unwrap().issued)
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
/// lists it with that access; signs out of it, which removes the access and remembers it, the
/// account listed as signed out; and resets what's remembered, after which neither is listed.
/// Nothing answered holds a token.
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
    assert_eq!((&listed["result"]["accesses"], &listed["result"]["remembered"]), (&json!([]), &json!([access])));
    assert_eq!(s.call(9, "accounts.access.list", json!({ "plugin": "echo" }))["result"], json!([]));
    let again = s.call(10, "accounts.sign_out", json!({ "account": account("a") }));
    assert_eq!(again["error"]["data"]["kind"], "not_found", "{again}");

    let reset = s.call(11, "accounts.access.restore", json!({ "account": account("a"), "plugins": [] }));
    assert_eq!(reset["result"], json!([]), "{reset}");
    assert_eq!(s.call(12, "accounts.list", json!({}))["result"], json!({ "accounts": [], "accesses": [] }));
    let answers = [begun.to_string(), listed.to_string(), out.to_string(), reset.to_string()].concat();
    assert!(!m.leaked(&answers), "an answer holds a token");
}

/// The sidecar signs in to an account with keys for the echo plugin: `sign_in.begin` answers where
/// to get them, which to enter and that the user names it, and `sign_in.finish` with them and a
/// name keeps it, giving echo access. A key the host refuses ends that sign-in with what it says.
/// No key is answered.
#[test]
fn the_sidecar_signs_in_with_keys_for_a_plugin() {
    let m = Mock::keyed();
    let mut s = Rpc::start(m.command());
    let (access, secret) = ("ak-0123456789", "sk-9876543210");
    let begin = json!({ "type": "mock", "for": "echo" });
    let begun = s.call(1, "accounts.sign_in.begin", begin.clone());
    let id = &begun["result"]["id"];
    let keys = json!({ "access": "Access key", "secret": "Secret key" });
    let url = format!("{}/keys", m.server.url);
    let expected =
        json!({ "id": id, "type": "mock", "title": "Mock", "plugin": WITH, "url": url, "keys": keys, "named": true });
    assert_eq!(begun["result"], expected, "{begun}");
    let short = json!({ "id": id, "keys": { "access": access, "secret": "sk-1234" }, "name": "me" });
    let refused = s.call(2, "accounts.sign_in.finish", short);
    let said = "the Mock account's Secret key (secret) is 7 characters: a key is 8 at least";
    assert_eq!(refused["error"]["message"], said, "{refused}");
    let again = s.call(3, "accounts.sign_in.finish", json!({ "id": id, "keys": {}, "name": "me" }));
    assert_eq!(again["error"]["data"]["kind"], "not_found", "the sign-in isn't over: {again}");

    let begun = s.call(4, "accounts.sign_in.begin", begin);
    let finish = json!({ "id": begun["result"]["id"], "keys": { "access": access, "secret": secret }, "name": "me" });
    let described = contract()["methods"].as_array().unwrap().iter().find(|m| m["name"] == "accounts.sign_in.finish");
    assert_eq!(violation(&finish, &format!("{}/params", at(described.unwrap()))), None, "the contract takes keys");
    let done = s.call(5, "accounts.sign_in.finish", finish);
    let r = &done["result"];
    let got = (&r["subject"], &r["name"], &r["refreshes"], &r["access"]["plugin"], &r["access"]["default"]);
    assert_eq!(got, (&json!("me"), &json!("me"), &json!(false), &json!("echo"), &json!(true)), "{done}");
    let listed = s.call(6, "accounts.list", json!({}));
    assert_eq!(listed["result"]["accesses"], json!([r["access"]]), "{listed}");
    assert!(m.server.got().is_empty(), "the service was sent something");
    let answers = [begun, refused, again, done, listed].map(|a| a.to_string()).concat();
    assert!(![access, secret, "sk-1234"].iter().any(|k| answers.contains(k)), "an answer holds a key: {answers}");
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
/// has, with parameters its contract takes, and calling it does what its label says. Given more
/// scopes, then signed out, echo has no access: its error offers to sign in for it, and signing
/// in again as its account asks for what echo had and gives it back.
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
    let more = json!({ "plugin": "echo", "account": account("a"), "scopes": ["write"] });
    let given = s.call(21, "accounts.access.give", more);
    assert_eq!(given["result"]["scopes"], json!(["read", "write"]), "{given}");

    s.call(22, "accounts.sign_out", json!({ "account": account("a") }));
    let failing =
        json!({ "fail": "access", "message": "it can't", "reason": "signed_out", "fields": { "type": "mock" } });
    let e = s.call(23, "plugins.run", json!({ "plugin": "echo", "command": "echo", "params": failing }));
    let again = &e["error"]["data"]["actions"][0];
    let label = "Sign in to a mock account";
    assert_eq!((&again["method"], &again["label"]), (&json!("accounts.sign_in.begin"), &json!(label)), "{e}");
    assert_eq!(again["params"], json!({ "type": "mock", "for": "echo" }), "{e}");
    let described = contract()["methods"].as_array().unwrap().iter().find(|m| m["name"] == again["method"]).unwrap();
    assert_eq!(violation(&again["params"], &format!("{}/params", at(described))), None, "{again}");
    // Signed in to again by its subject, it's asked for what echo had, and echo gets that back.
    let begin = json!({ "type": "mock", "for": "echo", "account": "a" });
    assert_eq!(violation(&begin, &format!("{}/params", at(described))), None, "{begin}");
    let begun = s.call(24, "accounts.sign_in.begin", begin);
    let (_, query) = begun["result"]["url"].as_str().unwrap().split_once('?').unwrap();
    let query: Vec<(String, String)> = form_urlencoded::parse(query.as_bytes()).into_owned().collect();
    let at = |key: &str| query.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str());
    assert_eq!((at("login_hint"), at("scope")), (Some("a"), Some("openid read write")), "{begun}");
    let url = m.approved(&begun["result"], &who("a"));
    let done = s.call(25, "accounts.sign_in.paste", json!({ "id": begun["result"]["id"], "url": url }));
    let back = &done["result"]["restored"][0];
    assert_eq!((&back["scopes"], &back["default"]), (&json!(["read", "write"]), &json!(true)), "{done}");
    assert_eq!(done["result"]["access"]["scopes"], json!(["read", "write"]), "{done}");
}

/// The sidecar gives echo back the access signing out took as it's asked, with params its
/// contract takes: `accounts.access.restore` is refused while the account is signed out
/// (`not_signed_in`); Automatically Restore Access on for the account restores it all as it's
/// signed in again, as it was, and off leaves it answered as `remembered`; restoring echo with no
/// scopes chosen gives it what it needs; and signing in with `restore: "reset"` forgets it.
#[test]
fn the_sidecar_restores_access_as_asked() {
    let m = Mock::new();
    let mut s = Rpc::start(m.command());
    let mut id = 0;
    let mut call = |method: &str, params: Value| {
        let described = contract()["methods"].as_array().unwrap().iter().find(|m| m["name"] == method);
        let described = described.unwrap_or_else(|| panic!("{method} isn't in the contract"));
        assert_eq!(violation(&params, &format!("{}/params", at(described))), None, "{method}: {params}");
        id += 1;
        s.call(id, method, params)
    };
    let sign_in = |call: &mut dyn FnMut(&str, Value) -> Value, params: Value| {
        let begun = call("accounts.sign_in.begin", params);
        let url = m.approved(&begun["result"], &who("a"));
        call("accounts.sign_in.paste", json!({ "id": begun["result"]["id"], "url": url }))["result"].clone()
    };
    let had = sign_in(&mut call, json!({ "type": "mock", "for": "echo", "scopes": ["write"] }))["access"].clone();
    assert_eq!((&had["scopes"], &had["default"]), (&json!(["read", "write"]), &json!(true)), "{had}");
    let a = json!({ "account": account("a") });
    call("accounts.sign_out", a.clone());

    let refused = call("accounts.access.restore", a.clone());
    let why = "the mock account a@mock.example is signed out: sign in to it first";
    assert_eq!(
        (&refused["error"]["message"], &refused["error"]["data"]["reason"]),
        (&json!(why), &json!("not_signed_in"))
    );
    let on = json!({ "account": account("a"), "on": true });
    assert_eq!(call("accounts.restore_automatically", on.clone())["result"], on);
    assert_eq!(call("accounts.list", json!({}))["result"]["restore_automatically"], json!([on]));
    let again = json!({ "type": "mock", "account": "a" });
    let done = sign_in(&mut call, again.clone());
    assert_eq!((&done["restored"], &done["remembered"]), (&json!([had]), &Value::Null), "{done}");

    let off = json!({ "account": account("a"), "on": false });
    call("accounts.restore_automatically", off);
    call("accounts.sign_out", a.clone());
    let done = sign_in(&mut call, again);
    assert_eq!((&done["restored"], &done["remembered"]), (&Value::Null, &json!([had])), "{done}");
    let chosen = json!({ "account": account("a"), "plugins": [{ "plugin": "echo", "scopes": [] }] });
    let back = call("accounts.access.restore", chosen)["result"].clone();
    assert_eq!((&back[0]["id"], &back[0]["scopes"]), (&had["id"], &json!(["read"])), "{back}");

    call("accounts.sign_out", a);
    let done = sign_in(&mut call, json!({ "type": "mock", "account": "a", "restore": "reset" }));
    assert_eq!((&done["restored"], &done["remembered"]), (&Value::Null, &Value::Null), "{done}");
    let listed = call("accounts.list", json!({}))["result"].clone();
    assert_eq!((&listed["accesses"], &listed["remembered"]), (&json!([]), &Value::Null), "{listed}");
}

/// Who the steps of the property tests here are about, by subject.
pub(super) const PEOPLE: [&str; 2] = ["a", "b"];

/// What the store keeps, as the model has it.
#[derive(Default)]
pub(super) struct Model {
    /// Subjects signed in, in the order they were first signed in since they were last signed
    /// out of.
    kept: Vec<&'static str>,
    /// The echo plugin's accesses, by subject, in the order they were given or restored, and
    /// whether each is its default. Only to accounts kept: signing out removes it.
    accesses: Vec<(&'static str, bool)>,
    /// Echo's accesses to accounts signed out, as they were, in the order they were signed out
    /// of, until they're restored or reset.
    remembered: Vec<(&'static str, bool)>,
    /// Automatically Restore Access: for one subject, or for all (none), on or off.
    restores: Vec<(Option<&'static str>, bool)>,
}

impl Model {
    pub(super) fn kept(&self, subject: &str) -> bool {
        self.kept.contains(&subject)
    }

    pub(super) fn has_access(&self, subject: &str) -> bool {
        self.accesses.iter().any(|(a, _)| *a == subject)
    }

    /// Whether echo's access to `subject` is remembered, waiting to be restored.
    pub(super) fn remembers(&self, subject: &str) -> bool {
        self.remembered.iter().any(|(a, _)| *a == subject)
    }

    /// Whether `subject` is listed: kept, or signed out with echo's access remembered.
    pub(super) fn known(&self, subject: &str) -> bool {
        self.kept(subject) || self.remembers(subject)
    }

    /// Why echo's access to `subject` can't be given or restored, as the sidecar's error `kind`
    /// (`account` with reason `not_signed_in`) says: signed out with its access remembered, else
    /// not there at all.
    pub(super) fn refusal(&self, subject: &str) -> &'static str {
        if self.remembers(subject) { "not_signed_in" } else { "not_found" }
    }

    /// `subject` signed in, for the echo plugin or not, restoring nothing it isn't set to (see
    /// [`Model::signed_in_restoring`]).
    pub(super) fn signed_in(&mut self, subject: &'static str, for_echo: bool) -> bool {
        self.signed_in_restoring(subject, for_echo, None)
    }

    /// `subject` signed in, for the echo plugin or not: echo's remembered access to it restored
    /// as `restoring` says, else when Automatically Restore Access is on for it, and always when
    /// it's for echo (unless reset); then given access when it's for echo and it has none, its
    /// default when it's its first. Whether echo's access was restored.
    pub(super) fn signed_in_restoring(
        &mut self,
        subject: &'static str,
        for_echo: bool,
        restoring: Option<Restoring>,
    ) -> bool {
        if !self.kept(subject) {
            self.kept.push(subject);
        }
        let restores = match restoring {
            Some(Restoring::Reset) => {
                self.remembered.retain(|(a, _)| *a != subject);
                false
            }
            Some(Restoring::Restore) => true,
            None => for_echo || self.restores_automatically(subject),
        };
        let restored = restores && self.restore(subject);
        if for_echo {
            self.given(subject);
        }
        restored
    }

    /// Whether `subject`'s access is restored as it's signed in: its own setting, else the one
    /// for all, else not.
    fn restores_automatically(&self, subject: &str) -> bool {
        let setting = |of: Option<&str>| self.restores.iter().find(|(s, _)| *s == of).map(|(_, on)| *on);
        setting(Some(subject)).or_else(|| setting(None)).unwrap_or(false)
    }

    /// Automatically Restore Access turned on or off for `subject`, or for all.
    pub(super) fn restore_automatically(&mut self, subject: Option<&'static str>, on: bool) {
        self.restores.retain(|(s, _)| *s != subject);
        self.restores.push((subject, on));
    }

    /// Echo's remembered access to `subject` put back, if there is one: joined to the access it
    /// has to it if it has one, else last, its default when it was and echo has no default
    /// since, or when it's its only access. Whether there was one.
    fn restore(&mut self, subject: &str) -> bool {
        let Some(at) = self.remembered.iter().position(|(a, _)| *a == subject) else { return false };
        let (subject, was_default) = self.remembered.remove(at);
        if !self.has_access(subject) {
            let default = !self.accesses.iter().any(|(_, d)| *d) && (was_default || self.accesses.is_empty());
            self.accesses.push((subject, default));
        }
        true
    }

    /// `restore` (`account TYPE:NAME restore`, `accounts.access.restore`) for `subject`: echo's
    /// remembered access put back when `keeps`, else forgotten. Whether it could be: putting back
    /// needs the account signed in, forgetting doesn't.
    pub(super) fn restored(&mut self, subject: &str, keeps: bool) -> bool {
        match keeps {
            true if !self.kept(subject) => return false,
            true => drop(self.restore(subject)),
            false => self.remembered.retain(|(a, _)| *a != subject),
        }
        true
    }

    /// Echo given access to `subject`, its default when it's its first: whether it could be (it's
    /// kept: one signed out gets its access back by signing in again, not by being given it).
    pub(super) fn given(&mut self, subject: &'static str) -> bool {
        if !self.kept(subject) {
            return false;
        }
        if !self.has_access(subject) {
            let first = self.accesses.is_empty();
            self.accesses.push((subject, first));
        }
        true
    }

    /// `subject` signed out of: whether it was kept. Echo's access to it is removed and
    /// remembered, and when it was its default, no other access becomes it.
    pub(super) fn signed_out(&mut self, subject: &'static str) -> bool {
        let was = self.kept(subject);
        self.kept.retain(|k| *k != subject);
        if let Some(at) = self.accesses.iter().position(|(a, _)| *a == subject) {
            let gone = self.accesses.remove(at);
            self.remembered.retain(|(a, _)| *a != subject);
            self.remembered.push(gone);
        }
        was
    }

    /// `subject` made echo's default, given access first: whether it could be (it's kept).
    pub(super) fn made_default(&mut self, subject: &'static str) -> bool {
        if !self.kept(subject) {
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

    /// The accounts listed (by subject, and whether each is signed in), the accesses and those
    /// remembered (by subject, and whether each is the default), as `accounts.list` answers them.
    pub(super) fn listed(&self) -> Value {
        let signed_out = self.remembered.iter().map(|(a, _)| *a).filter(|a| !self.kept(a));
        let accounts: Vec<Value> =
            self.kept.iter().map(|k| json!([k, true])).chain(signed_out.map(|a| json!([a, false]))).collect();
        let pairs = |of: &[(&str, bool)]| -> Vec<Value> { of.iter().map(|(a, d)| json!([a, d])).collect() };
        json!({ "accounts": accounts, "accesses": pairs(&self.accesses), "remembered": pairs(&self.remembered) })
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
        "remembered": pairs("remembered", "/account/subject", "/default"),
    })
}

/// Asserts the sidecar's `answer` failed as `why` ([`Model::refusal`]) says.
fn refused(answer: &Value, why: &str, step: usize) {
    let data = &answer["error"]["data"];
    match why {
        "not_signed_in" => {
            assert_eq!((&data["kind"], &data["reason"]), (&json!("account"), &json!(why)), "step {step}: {answer}")
        }
        _ => assert_eq!(data["kind"], why, "step {step}: {answer}"),
    }
}

/// How [`Step::Restore`] restores what's remembered of an account signed out.
#[derive(Debug, Clone, Copy)]
enum Restore {
    /// Every plugin's access, as it was.
    All,
    /// Echo's, with `write` chosen besides what it needs or not.
    Echo { write: bool },
    /// None: what's remembered is forgotten.
    Reset,
}

/// A step of [`accounts_and_accesses_are_as_the_calls_made_them`], naming people by their place
/// in [`PEOPLE`].
#[derive(Debug, Clone, Copy)]
enum Step {
    /// Signing in, for the echo plugin or not, again as the account when it's listed, the browser
    /// coming back to this computer or the address it ended up on pasted, restoring or resetting
    /// what echo had of it or neither.
    SignIn {
        who: usize,
        for_echo: bool,
        pasted: bool,
        restoring: Option<Restoring>,
    },
    /// Begun, then cancelled.
    Cancelled,
    SignOut(usize),
    SetDefault(usize),
    TakeAway(usize),
    /// `accounts.access.restore`.
    Restore {
        who: usize,
        how: Restore,
    },
    /// Automatically Restore Access for one person, or (none) for all.
    RestoreAutomatically {
        who: Option<usize>,
        on: bool,
    },
}

/// For any sequence of sidecar calls signing in (for the echo plugin or not, coming back or
/// pasted, restoring echo's access, resetting it or neither), cancelling, signing out, making an
/// access the default, taking it away, restoring what's remembered (all of it, echo's with the
/// scopes chosen, or none) and turning Automatically Restore Access on or off: the accounts,
/// accesses and remembered accesses `accounts.list` answers are those the model keeps, a sign-in
/// answers what it restored and what's still remembered, a call about what isn't there is
/// `not_found` and one giving or restoring access to an account signed out is `not_signed_in`,
/// and no answer holds a token.
#[hegel::test(test_cases = 8, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn accounts_and_accesses_are_as_the_calls_made_them(tc: TestCase) {
    let m = Mock::new();
    let mut s = Rpc::start(m.command());
    let mut model = Model::default();
    let mut answers = String::new();
    // Half the time, echo's access to a starts out remembered: a signed in to for it, then out of.
    let remembered = [Step::SignIn { who: 0, for_echo: true, pasted: true, restoring: None }, Step::SignOut(0)];
    let start = if tc.draw(gs::booleans()) { &remembered[..] } else { &[] };
    let n = start.len() + tc.draw(gs::integers::<usize>().min_value(2).max_value(10));
    for i in 0..n {
        let step = match start.get(i) {
            Some(step) => *step,
            None => {
                let person = tc.draw(gs::integers::<usize>().max_value(1));
                match tc.draw(gs::integers::<u8>().max_value(7)) {
                    0 | 1 => Step::SignIn {
                        who: person,
                        for_echo: tc.draw(gs::booleans()),
                        pasted: tc.draw(gs::booleans()),
                        restoring: tc.draw(gs::sampled_from(vec![
                            None,
                            Some(Restoring::Restore),
                            Some(Restoring::Reset),
                        ])),
                    },
                    2 => Step::Cancelled,
                    3 => Step::SignOut(person),
                    4 => Step::SetDefault(person),
                    5 => Step::TakeAway(person),
                    6 => {
                        let how = match tc.draw(gs::integers::<u8>().max_value(2)) {
                            0 => Restore::All,
                            1 => Restore::Echo { write: tc.draw(gs::booleans()) },
                            _ => Restore::Reset,
                        };
                        Step::Restore { who: person, how }
                    }
                    _ => Step::RestoreAutomatically {
                        who: tc.draw(gs::booleans()).then_some(person),
                        on: tc.draw(gs::booleans()),
                    },
                }
            }
        };
        let id = i as i64 * 10;
        let subject = match step {
            Step::SignIn { who, .. }
            | Step::SignOut(who)
            | Step::SetDefault(who)
            | Step::TakeAway(who)
            | Step::Restore { who, .. }
            | Step::RestoreAutomatically { who: Some(who), .. } => PEOPLE[who],
            Step::Cancelled | Step::RestoreAutomatically { who: None, .. } => "",
        };
        let not_found = |answer: &Value| assert_eq!(answer["error"]["data"]["kind"], "not_found", "step {i}: {answer}");
        let answer = match step {
            Step::SignIn { for_echo, pasted, restoring, .. } => {
                let mut params = json!({ "type": "mock" });
                if for_echo {
                    params["for"] = json!("echo");
                }
                if model.known(subject) {
                    params["account"] = json!(subject);
                }
                if let Some(restoring) = restoring {
                    params["restore"] = json!(restoring);
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
                let restored = model.signed_in_restoring(subject, for_echo, restoring);
                let count = |key: &str| r[key].as_array().map_or(0, Vec::len);
                assert_eq!(count("restored"), usize::from(restored), "step {i}: {done}");
                assert_eq!(count("remembered"), usize::from(model.remembers(subject)), "step {i}: {done}");
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
                let why = model.refusal(subject);
                let set =
                    s.call(id, "accounts.access.set_default", json!({ "plugin": "echo", "account": account(subject) }));
                match model.made_default(subject) {
                    true => assert_eq!(set["result"]["default"], true, "step {i}: {set}"),
                    false => refused(&set, why, i),
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
            Step::Restore { how, .. } => {
                let mut params = json!({ "account": account(subject) });
                match how {
                    Restore::All => {}
                    Restore::Echo { write } => {
                        let scopes = if write { json!(["write"]) } else { json!([]) };
                        params["plugins"] = json!([{ "plugin": "echo", "scopes": scopes }]);
                    }
                    Restore::Reset => params["plugins"] = json!([]),
                }
                let (had, why) = (model.remembers(subject), model.refusal(subject));
                let keeps = !matches!(how, Restore::Reset);
                let r = s.call(id, "accounts.access.restore", params);
                match model.restored(subject, keeps) {
                    true => {
                        let back = r["result"].as_array().unwrap();
                        assert_eq!(back.len(), usize::from(had && keeps), "step {i}: {r}");
                        if let (Restore::Echo { write }, [back]) = (how, back.as_slice()) {
                            let scopes = if write { json!(["read", "write"]) } else { json!(["read"]) };
                            assert_eq!(back["scopes"], scopes, "step {i}: what it needs, and what's chosen: {r}");
                        }
                    }
                    false => refused(&r, why, i),
                }
                r
            }
            Step::RestoreAutomatically { on, .. } => {
                let named = (!subject.is_empty()).then_some(subject);
                let mut params = json!({ "on": on });
                if let Some(subject) = named {
                    params["account"] = account(subject);
                }
                let set = s.call(id, "accounts.restore_automatically", params.clone());
                assert_eq!(set["result"], params, "step {i}: {set}");
                model.restore_automatically(named, on);
                set
            }
        };
        answers.push_str(&answer.to_string());
        let listed = s.call(id + 5, "accounts.list", json!({}));
        assert_eq!(shape(&listed["result"]), model.listed(), "step {i}: {step:?}");
        answers.push_str(&listed.to_string());
    }
    assert!(!m.leaked(&answers), "an answer holds a token");
}
