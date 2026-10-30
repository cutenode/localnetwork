//! `account`, `account TYPE sign-in`, `account TYPE:NAME status|sign-out` and `plugin access` at
//! the mock service, and what a plugin's destination does without access to an account it needs.

use hegel::TestCase;
use hegel::generators as gs;
use serde_json::{Value, json};

use crate::mock_oauth::{WITH, account_plugin};
use crate::plugins::shared_accounts::{Mock, Model, PEOPLE, asking, shape};
use crate::support::Rpc;

/// Who the service says signed in, by subject.
fn who(subject: &str) -> Value {
    json!({ "sub": subject, "email": format!("{subject}@mock.example") })
}

/// `store copy` to echo's store destination.
const COPY: [&str; 4] = ["store", "copy", "--to", "echo:folder"];

impl Mock {
    /// What `args` exits with, and prints to stdout and stderr.
    pub(super) fn run(&self, args: &[&str]) -> (i32, String, String) {
        let out = self.command().args(args).output().unwrap();
        let said = |b: Vec<u8>| String::from_utf8(b).unwrap();
        (out.status.code().unwrap(), said(out.stdout), said(out.stderr))
    }

    /// `account mock sign-in` with `more`, approved in the browser as `subject`.
    fn sign_in(&self, subject: &str, more: &[&str]) -> (i32, String, String) {
        let mut c = self.command();
        c.args([&["account", "mock", "sign-in", "--no-browser", "--timeout", "10"][..], more].concat());
        self.approving(c, &who(subject))
    }

    /// `args` run in a terminal on which `typed` was typed ahead, a sign-in it begins approved as
    /// `subject`.
    #[cfg(unix)]
    fn typed(&self, args: &[&str], typed: &str, subject: &str) -> (i32, String, String) {
        use std::io::Write;
        let (mut keyboard, stdin) = crate::support::terminal();
        keyboard.write_all(typed.as_bytes()).unwrap();
        let mut c = self.command();
        c.args(args).stdin(std::process::Stdio::from(stdin));
        self.approving(c, &who(subject))
    }

    /// `account mock sign-in --paste` with `more` in a terminal, on which the address the browser
    /// ended up on, approving as `subject`, is pasted once it's asked for: with another sign-in's
    /// state when `elsewhere`.
    #[cfg(unix)]
    fn pasting(&self, more: &[&str], subject: &str, elsewhere: bool) -> (i32, String, String) {
        use std::io::{BufRead, BufReader, Read, Write};
        use std::process::Stdio;
        let (mut keyboard, stdin) = crate::support::terminal();
        let mut c = self.command();
        c.args([&["account", "mock", "sign-in", "--paste"][..], more].concat());
        let mut child = c.stdin(Stdio::from(stdin)).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
        let mut err = String::new();
        for line in BufReader::new(child.stderr.take().unwrap()).lines() {
            let line = line.unwrap();
            err.push_str(&format!("{line}\n"));
            let link = line.trim();
            if link.starts_with("http") && link.contains("/authorize?") {
                let back = self.approved_link(link, &who(subject));
                let back = match elsewhere {
                    true => format!("{}&state=elsewhere", back.split("&state=").next().unwrap()),
                    false => back,
                };
                writeln!(keyboard, "{back}").unwrap();
            }
        }
        let mut out = String::new();
        child.stdout.take().unwrap().read_to_string(&mut out).unwrap();
        (child.wait().unwrap().code().unwrap(), out, err)
    }
}

/// An account is signed in to for a plugin, listed with the access it gave (by `account`, `account
/// TYPE:NAME status` and `plugin access`), signed out of, listed for that access, and the access
/// taken away; and a command that can't name an account says why.
#[test]
fn an_account_is_signed_in_listed_and_signed_out_on_the_command_line() {
    let m = Mock::new();
    let (code, out, err) = m.run(&["--json", "account"]);
    assert_eq!(code, 0, "{err}");
    let listed: Value = serde_json::from_str(&out).unwrap();
    assert_eq!((&listed["accounts"], &listed["accesses"]), (&json!([]), &json!([])), "{listed}");
    let (code, out, err) = m.sign_in("a", &["--for", "echo"]);
    assert_eq!(code, 0, "{err}");
    assert_eq!(out, "✓ Signed in to the Mock account a@mock.example\n  echo has access to it, its default\n");
    assert!(err.contains("Sign in: open this link\n  http://127.0.0.1:"), "{err}");

    let (_, out, _) = m.run(&["account"]);
    let shown = "Mock account a@mock.example\n  signed in, refreshed on its own · allowed: read write\n  access: echo (default)\n";
    assert!(out.ends_with(shown), "{out}");
    for named in ["mock:a", "mock:a@mock.example"] {
        let (_, out, _) = m.run(&["--json", "account", named, "status"]);
        let one: Value = serde_json::from_str(&out).unwrap();
        assert_eq!((&one["accounts"][0]["subject"], &one["accesses"][0]["plugin"]), (&json!("a"), &json!("echo")));
    }
    let (_, out, _) = m.run(&["plugin", "access", "echo"]);
    assert_eq!(out, "mock: Tests\n  a@mock.example (default) · read\n");

    let (code, out, _) = m.run(&["account", "mock:a", "sign-out"]);
    assert_eq!((code, out.as_str()), (0, "✓ Signed out of a@mock.example\n"));
    let (_, out, _) = m.run(&["account"]);
    let signed_out = "  signed out: `locallibrary account mock sign-in` signs in again\n  access: echo (default)\n";
    assert!(out.contains(signed_out), "{out}");
    // Signed out, it's still named for signing in to again: echo has access to it.
    let mut c = m.command();
    c.args(["account", "mock:a@mock.example", "sign-in", "--no-browser", "--timeout", "10"]);
    let (code, out, err) = m.approving(c, &who("a"));
    assert_eq!((code, out.as_str()), (0, "✓ Signed in to the Mock account a@mock.example\n"), "{err}");
    assert_eq!(m.run(&["account", "mock:a", "sign-out"]).0, 0);
    let (code, out, _) = m.run(&["plugin", "access", "echo", "--take-away", "a"]);
    assert_eq!((code, out.as_str()), (0, "✓ Took echo's access to the Mock account a@mock.example away\n"));
    let (_, out, _) = m.run(&["plugin", "access", "echo"]);
    assert_eq!(out, "mock: Tests\n  no access: `locallibrary account mock sign-in --for echo` gives it one\n");

    let (code, _, err) = m.run(&["account", "mock", "status"]);
    assert!(code == 2 && err.contains("name an account: mock:NAME"), "{err}");
    let (code, _, err) = m.run(&["account", "mock:nope"]);
    assert!(code == 1 && err.contains("not found: the mock account \"nope\""), "{err}");
    let (code, _, err) = m.run(&["account", "mock:a", "sign-out", "--for", "echo"]);
    assert!(code == 2 && err.contains("--with, --for and --scope go with sign-in"), "{err}");
    let (code, _, err) = m.run(&["account", "nope", "sign-in"]);
    assert!(code == 1 && err.contains("not found: an account plugin providing nope accounts"), "{err}");
}

/// A step of [`an_account_reads_back_the_same_wherever_its_asked`], naming people by their place
/// in [`PEOPLE`].
#[derive(Debug, Clone, Copy)]
enum Step {
    /// `account mock sign-in`, for echo or not.
    SignIn { who: usize, for_echo: bool },
    /// `accounts.sign_in.*` in the sidecar, for echo.
    SidecarSignIn(usize),
    /// `account mock:SUBJECT sign-out`.
    SignOut(usize),
    /// `plugin access echo --default SUBJECT`.
    Default(usize),
    /// `plugin access echo --take-away SUBJECT`.
    TakeAway(usize),
}

/// For any steps signing in (on the command line or in the sidecar), out, making an account echo's
/// default and taking its access away: `account --json` lists the accounts and accesses the
/// sidecar's `accounts.list` does, which are those the model keeps, `account` shows each and
/// `plugin info echo` has echo's accesses; a step about what isn't there fails `not_found`; and
/// nothing printed or answered holds a token.
#[hegel::test(test_cases = 8, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn an_account_reads_back_the_same_wherever_its_asked(tc: TestCase) {
    let m = Mock::new();
    let mut s = Rpc::start(m.command());
    let mut model = Model::default();
    let mut said = String::new();
    let n = tc.draw(gs::integers::<usize>().min_value(2).max_value(5));
    for i in 0..n {
        let person = tc.draw(gs::integers::<usize>().max_value(1));
        let step = match tc.draw(gs::integers::<u8>().max_value(5)) {
            0 | 1 => Step::SignIn { who: person, for_echo: tc.draw(gs::booleans()) },
            2 => Step::SidecarSignIn(person),
            3 => Step::SignOut(person),
            4 => Step::Default(person),
            _ => Step::TakeAway(person),
        };
        let (Step::SignIn { who: p, .. }
        | Step::SidecarSignIn(p)
        | Step::SignOut(p)
        | Step::Default(p)
        | Step::TakeAway(p)) = step;
        let subject = PEOPLE[p];
        let id = i as i64 * 10;
        let named = format!("mock:{subject}");
        let (code, out, err) = match step {
            Step::SignIn { for_echo, .. } => {
                let more: &[&str] = if for_echo { &["--for", "echo"] } else { &[] };
                model.signed_in(subject, for_echo);
                m.sign_in(subject, more)
            }
            Step::SidecarSignIn(_) => {
                let begun = s.call(id, "accounts.sign_in.begin", json!({ "type": "mock", "for": "echo" }));
                let url = m.approved(&begun["result"], &who(subject));
                let done = s.call(id + 1, "accounts.sign_in.paste", json!({ "id": begun["result"]["id"], "url": url }));
                model.signed_in(subject, true);
                (if done["error"].is_null() { 0 } else { 1 }, done.to_string(), String::new())
            }
            Step::SignOut(_) => {
                let r = m.run(&["account", &named, "sign-out"]);
                assert_eq!(r.0 == 0, model.signed_out(subject), "step {i}: {step:?} {}", r.2);
                r
            }
            Step::Default(_) => {
                let r = m.run(&["plugin", "access", "echo", "--default", subject]);
                assert_eq!(r.0 == 0, model.made_default(subject), "step {i}: {step:?} {}", r.2);
                r
            }
            Step::TakeAway(_) => {
                let r = m.run(&["plugin", "access", "echo", "--take-away", subject]);
                assert_eq!(r.0 == 0, model.taken_away(subject), "step {i}: {step:?} {}", r.2);
                r
            }
        };
        if code != 0 {
            assert!(err.contains("not found"), "step {i}: {step:?} {err}");
        }
        said.push_str(&format!("{out}{err}"));
        let (_, listed, _) = m.run(&["--json", "account"]);
        let listed: Value = serde_json::from_str(&listed).unwrap();
        let answered = s.call(id + 5, "accounts.list", json!({}));
        assert_eq!(shape(&listed), shape(&answered["result"]), "step {i}: {step:?}");
        assert_eq!(shape(&listed), model.listed(), "step {i}: {step:?}");
        let (_, shown, _) = m.run(&["account"]);
        for a in listed["accounts"].as_array().unwrap() {
            let heading = format!("Mock account {}\n", a["name"].as_str().unwrap());
            assert!(shown.contains(&heading), "step {i}: {shown}");
        }
        // `plugin info` has echo's accesses too, by name beside why it asks.
        let (_, info, _) = m.run(&["--json", "plugin", "info", "echo"]);
        let info: Value = serde_json::from_str(&info).unwrap();
        let accesses = model.listed()["accesses"].clone();
        assert_eq!(shape(&json!({ "accesses": info["accesses"] }))["accesses"], accesses, "step {i}: {step:?}");
        let names: Vec<String> =
            accesses.as_array().unwrap().iter().map(|a| format!("{}@mock.example", a[0].as_str().unwrap())).collect();
        let access = match names.is_empty() {
            true => "no access".to_string(),
            false => format!("access to {}", names.join(", ")),
        };
        let (_, described, _) = m.run(&["plugin", "info", "echo"]);
        assert!(described.contains(&format!("    mock  Tests (read; {access})\n")), "step {i}: {described}");
        said.push_str(&format!("{listed}{answered}{shown}{info}{described}"));
    }
    assert!(!m.leaked(&said), "a token was printed or answered");
}

/// Where [`a_destination_without_access_asks_only_where_it_can`] uses echo's destination.
#[derive(Debug, Clone, Copy)]
enum Using {
    /// [`COPY`] with `--json`.
    Json,
    /// [`COPY`] with no terminal to ask in.
    NoTerminal,
    /// [`COPY`] with `--yes`, in a terminal.
    #[cfg(unix)]
    Yes,
    /// `plugins.store.copy` in the sidecar.
    Sidecar,
}

/// For any accounts signed in (none, one or two, not for echo), echo's destination, whose plugin
/// needs a mock account and has no access, is never asked about with `--json`, `--yes`, no
/// terminal or in the sidecar: it's refused `account` (`no_access`), with an action to sign in
/// for echo and one making each account its default, whose hints are commands that run; once one
/// has, the destination is used.
#[hegel::test(test_cases = 6, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn a_destination_without_access_asks_only_where_it_can(tc: TestCase) {
    let m = Mock::new();
    locallibrary::Store::open_or_init(&m.env.store).unwrap();
    let signed_in = tc.draw(gs::integers::<usize>().max_value(2));
    for subject in &PEOPLE[..signed_in] {
        assert_eq!(m.sign_in(subject, &[]).0, 0);
    }
    let all = [
        Using::Json,
        Using::NoTerminal,
        #[cfg(unix)]
        Using::Yes,
        Using::Sidecar,
    ];
    let using = tc.draw(gs::sampled_from(&all[..]));
    let mut hints = vec!["Sign in to a mock account: locallibrary account mock sign-in --for echo".to_string()];
    hints.extend(
        PEOPLE[..signed_in]
            .iter()
            .map(|s| format!("Use {s}@mock.example: locallibrary plugin access echo --default {s}")),
    );
    let (error, said) = match using {
        Using::Sidecar => {
            let mut s = Rpc::start(m.command());
            let e = s.call(1, "plugins.store.copy", json!({ "destination": "echo:folder" }));
            (e["error"]["data"].clone(), e.to_string())
        }
        Using::Json => {
            let (code, out, err) = m.run(&[&["--json"][..], &COPY].concat());
            assert_eq!(code, 1, "{err}");
            (serde_json::from_str::<Value>(&out).unwrap()["error"].clone(), format!("{out}{err}"))
        }
        _ => {
            #[cfg(unix)]
            let (code, _, err) = match using {
                Using::Yes => m.typed(&[&["--yes"][..], &COPY].concat(), "", "a"),
                _ => m.run(&COPY),
            };
            #[cfg(not(unix))]
            let (code, _, err) = m.run(&COPY);
            assert_eq!(code, 1, "{err}");
            for h in &hints {
                assert!(err.contains(&format!("hint: {h}\n")), "{h}: {err}");
            }
            (json!({ "kind": "account", "reason": "no_access", "actions": hints }), err)
        }
    };
    assert!(!said.contains("Which Mock account"), "{using:?} asked: {said}");
    assert_eq!((&error["kind"], &error["reason"]), (&json!("account"), &json!("no_access")), "{error}");
    assert_eq!(error["actions"].as_array().unwrap().len(), hints.len(), "{error}");
    // The last, run as its hint says.
    let (_, command) = hints.last().unwrap().split_once(": ").unwrap();
    let words = shlex::split(command).unwrap();
    let words: Vec<&str> = words[1..].iter().map(String::as_str).collect();
    let (code, _, err) = match signed_in {
        0 => {
            let mut c = m.command();
            c.args(&words).args(["--no-browser", "--timeout", "10"]);
            m.approving(c, &who("a"))
        }
        _ => m.run(&words),
    };
    assert_eq!(code, 0, "{command}: {err}");
    let (code, _, err) = m.run(&COPY);
    assert_eq!(code, 0, "{err}");
}

/// In a terminal, echo's destination without access asks which mock account echo may use and
/// what for, offering to sign in to another: the account chosen, or the one signed in to, is made
/// its default, and the destination is used.
#[cfg(unix)]
#[test]
fn a_destination_without_access_asks_which_account_in_a_terminal() {
    let m = Mock::new();
    locallibrary::Store::open_or_init(&m.env.store).unwrap();
    let (code, out, err) = m.typed(&COPY, "1\n", "a");
    assert_eq!(code, 0, "{err}");
    assert!(err.contains("? Which Mock account may Echo use, to read? Tests\n    1. Sign in\n"), "{err}");
    assert!(out.starts_with("✓ Echo uses the Mock account a@mock.example\n"), "{out}");

    let m = Mock::new();
    locallibrary::Store::open_or_init(&m.env.store).unwrap();
    for subject in PEOPLE {
        assert_eq!(m.sign_in(subject, &[]).0, 0);
    }
    let (code, out, err) = m.typed(&COPY, "2\n", "a");
    assert_eq!(code, 0, "{err}");
    assert!(err.contains("    1. a@mock.example\n    2. b@mock.example\n    3. Sign in to another\n"), "{err}");
    assert!(out.starts_with("✓ Echo uses the Mock account b@mock.example\n"), "{out}");
    let (_, out, _) = m.run(&["plugin", "access", "echo"]);
    assert_eq!(out, "mock: Tests\n  b@mock.example (default) · read\n");
}

/// A second account plugin's id, providing the `mock` type too.
const TOO: &str = "mock-accounts-too";

/// The mock service, the echo plugin and one account plugin for it or two ([`WITH`], [`TOO`]),
/// whose revocation the service refuses unless `revokes`.
fn providing(two: bool, revokes: bool) -> (Mock, Vec<&'static str>) {
    let mut m = Mock::new();
    let mut with = account_plugin(&m.server.url, &format!("{}/userinfo", m.server.url), "");
    if !revokes {
        // Somewhere on the service that refuses a request without a token.
        with = with.replace("/revoke\"", "/userinfo\"");
    }
    let too = with.replacen(&format!("id = \"{WITH}\""), &format!("id = \"{TOO}\""), 1);
    match two {
        true => m.env.plugins(&[&asking(), &with, &too]),
        false => m.env.plugins(&[&asking(), &with]),
    }
    (m, if two { vec![WITH, TOO] } else { vec![WITH] })
}

/// How [`every_account_command_says_in_words_what_it_answers_as_json`] names an account.
#[derive(Debug, Clone, Copy)]
enum Naming {
    Subject,
    /// By what it's shown as.
    Name,
    /// As TYPE:SUBJECT.
    Typed,
}

/// How long the mock service's tokens last in
/// [`every_account_command_says_in_words_what_it_answers_as_json`].
#[derive(Debug, Clone, Copy)]
enum Lasting {
    /// An hour, then they're refreshed.
    Refreshed,
    /// An hour, with nothing to refresh them with.
    Hour,
    /// Until they're revoked, with nothing to refresh them with.
    Ever,
}

/// An account signed in to in [`every_account_command_says_in_words_what_it_answers_as_json`]:
/// its subject, what it's shown as and the account plugin it was signed in through.
#[derive(Debug, Clone)]
struct Kept {
    subject: &'static str,
    name: String,
    with: &'static str,
}

/// `args`, after `--json` when `json`.
fn maybe_json<'a>(json: bool, args: &[&'a str]) -> Vec<&'a str> {
    [if json { &["--json"][..] } else { &[] }, args].concat()
}

/// What a command that changes something reported, as it prints it with `--json`.
fn report(out: &str) -> Value {
    serde_json::from_str::<Value>(out).unwrap()["report"].clone()
}

/// `values`' strings, joined with `sep`.
fn joined(values: &Value, sep: &str) -> String {
    values.as_array().unwrap().iter().map(|v| v.as_str().unwrap()).collect::<Vec<_>>().join(sep)
}

/// Echo's accesses as `plugin access echo --json` answers them, checking `plugin access echo`
/// says each in words: its name, whether it's the default, the account plugin when `two` provide
/// the type, and its scopes.
fn echo_accesses(m: &Mock, two: bool) -> Value {
    let (_, answered, _) = m.run(&["--json", "plugin", "access", "echo"]);
    let answered: Value = serde_json::from_str(&answered).unwrap();
    let lines: Vec<String> = answered
        .as_array()
        .unwrap()
        .iter()
        .map(|x| {
            let default = if x["default"] == json!(true) { " (default)" } else { "" };
            let through = match two {
                true => format!(" through {}", x["account"]["plugin"].as_str().unwrap()),
                false => String::new(),
            };
            format!("  {}{default}{through} · {}\n", x["name"].as_str().unwrap(), joined(&x["scopes"], ", "))
        })
        .collect();
    let (_, words, _) = m.run(&["plugin", "access", "echo"]);
    assert_eq!(words, format!("mock: Tests\n{}", lines.concat()), "{answered}");
    answered
}

/// For one account plugin providing the type or two, a service that says what it granted or not,
/// revokes or not and whose tokens are refreshed, last an hour or last, accounts named by subject,
/// name or TYPE:SUBJECT, and `--json` or not: signing in for echo and not, again as a kept account
/// (the browser maybe approving another), each account's status, echo's accesses, making one its
/// default, signing out and taking the access away say in words what they answer as JSON; a name
/// two accounts share is refused, naming their subjects; with two account plugins, signing in
/// without `--with` is refused, naming them, and so is signing in for echo with a scope it doesn't
/// ask for; an account plugin says what it provides and isn't served; and in a terminal, signing
/// in takes the address the browser ended up on, pasted, but not one from another sign-in.
#[hegel::test(test_cases = 3, suppress_health_check = [hegel::HealthCheck::TooSlow])]
#[hegel::explicit_test_case(
    two = true,
    revokes = false,
    unscoped = true,
    lasting = Lasting::Refreshed,
    naming = Naming::Name,
    json = false,
    a_with = TOO,
    b_with = WITH,
    same_name = true,
    instead = true
)]
#[hegel::explicit_test_case(
    two = false,
    revokes = true,
    unscoped = false,
    lasting = Lasting::Ever,
    naming = Naming::Typed,
    json = true,
    a_with = WITH,
    b_with = WITH,
    same_name = false,
    instead = true
)]
#[hegel::explicit_test_case(
    two = true,
    revokes = true,
    unscoped = false,
    lasting = Lasting::Hour,
    naming = Naming::Subject,
    json = false,
    a_with = WITH,
    b_with = TOO,
    same_name = true,
    instead = false
)]
fn every_account_command_says_in_words_what_it_answers_as_json(tc: TestCase) {
    let two = tc.draw(gs::booleans());
    let revokes = tc.draw(gs::booleans());
    let unscoped = tc.draw(gs::booleans());
    let lasting = tc.draw(gs::sampled_from(vec![Lasting::Refreshed, Lasting::Hour, Lasting::Ever]));
    let naming = tc.draw(gs::sampled_from(vec![Naming::Subject, Naming::Name, Naming::Typed]));
    let json = tc.draw(gs::booleans());
    let (m, providers) = providing(two, revokes);
    // a signed in to for echo and b (maybe shown as a is) not, through the account plugins drawn;
    // then a again, the browser approving it or c.
    let a_with = tc.draw(gs::sampled_from(providers.clone()));
    let b_with = tc.draw(gs::sampled_from(providers));
    let same_name = tc.draw(gs::booleans());
    let instead = tc.draw(gs::booleans());
    {
        let mut s = m.state.lock().unwrap();
        s.unscoped = unscoped;
        s.refreshless = !matches!(lasting, Lasting::Refreshed);
        s.expires_in = (!matches!(lasting, Lasting::Ever)).then_some(3600);
    }
    let called = |k: &Kept| match two {
        true => format!("Mock account {} (through {})", k.name, k.with),
        false => format!("Mock account {}", k.name),
    };

    // Before there's a store: echo has no access, and the account plugin asks for none and says
    // what it provides; an account plugin isn't served.
    let none = "mock: Tests\n  no access: `locallibrary account mock sign-in --for echo` gives it one\n";
    assert_eq!(m.run(&["plugin", "access", "echo"]).1, none);
    assert_eq!(m.run(&["--json", "plugin", "access", "echo"]).1, "[]\n");
    assert_eq!(m.run(&["plugin", "access", WITH]).1, format!("{WITH} asks for no accounts.\n"));
    assert_eq!(m.run(&["--json", "plugin", "access", WITH]).1, "[]\n");
    let (_, info, _) = m.run(&["plugin", "info", WITH]);
    let provides = "  Account type:\n    mock  Mock\n  Scopes:\n    read   read\n    write  write\n    peek   peek\n";
    assert!(info.contains(provides), "{info}");
    // A debug build serves every test plugin, so a built-in account plugin.
    let (code, _, err) = m.run(&["plugin", "serve", "google-account"]);
    let why = "the google-account plugin is an account plugin, which nothing runs";
    assert!(code == 1 && err.contains(why), "{err}");
    if two {
        let (code, _, err) = m.run(&["account", "mock", "sign-in", "--no-browser"]);
        let why = format!("several account plugins provide mock accounts ({WITH}, {TOO}): name one");
        assert!(code != 0 && err.contains(&why), "{err}");
    }
    // A scope echo doesn't ask for isn't asked for it.
    let (code, _, err) = m.run(&["account", "mock", "sign-in", "--with", a_with, "--for", "echo", "--scope", "peek"]);
    assert!(code != 0 && err.contains("the echo plugin doesn't ask for the mock scope \"peek\""), "{err}");

    let a = Kept { subject: "a", name: "a@mock.example".into(), with: a_with };
    let b_name = if same_name { a.name.clone() } else { "b@mock.example".into() };
    let b = Kept { subject: "b", name: b_name, with: b_with };
    for (k, more) in [(&a, &["--for", "echo"][..]), (&b, &[])] {
        let mut c = m.command();
        c.args(maybe_json(json, &["account", "mock", "sign-in", "--no-browser", "--timeout", "10", "--with", k.with]));
        c.args(more);
        let (code, out, err) = m.approving(c, &json!({ "sub": k.subject, "email": k.name }));
        assert_eq!(code, 0, "{err}");
        match json {
            true => {
                let done = report(&out);
                assert_eq!((&done["subject"], &done["plugin"]), (&json!(k.subject), &json!(k.with)), "{done}");
                assert_eq!(done["access"]["default"], if more.is_empty() { Value::Null } else { json!(true) });
            }
            false => assert!(out.starts_with(&format!("✓ Signed in to the {}\n", called(k))), "{out}"),
        }
    }
    let mut kept = vec![a.clone(), b.clone()];
    let named = |k: &Kept| match naming {
        Naming::Name => k.name.clone(),
        _ => k.subject.to_string(),
    };
    let shared = |k: &Kept, kept: &[Kept]| {
        matches!(naming, Naming::Name) && kept.iter().filter(|x| x.name == k.name).count() > 1
    };
    let several = |k: &Kept| format!("several mock accounts are {}: name one as mock:SUBJECT (a, b)", k.name);

    let target = format!("mock:{}", named(&a));
    let mut c = m.command();
    c.args(maybe_json(json, &["account", &target, "sign-in", "--no-browser", "--timeout", "10"]));
    let (code, out, err) = m.approving(c, &who(if instead { "c" } else { "a" }));
    if shared(&a, &kept) {
        assert!(code == 2 && format!("{out}{err}").contains(&several(&a)), "{out}{err}");
    } else {
        assert_eq!(code, 0, "{err}");
        match json {
            true => {
                let done = report(&out);
                assert_eq!(done["instead_of"], if instead { json!("a") } else { Value::Null }, "{done}");
            }
            false => {
                let warned =
                    format!("the browser signed in to another account than {}, which is still kept", a.subject);
                assert_eq!(out.contains(&warned), instead, "{out}");
            }
        }
        if instead {
            kept.push(Kept { subject: "c", name: "c@mock.example".into(), with: a.with });
        }
    }

    // Each account's status, in words and as `account --json` lists it.
    let (_, listed, _) = m.run(&["--json", "account"]);
    let listed: Value = serde_json::from_str(&listed).unwrap();
    for k in &kept {
        let target = format!("mock:{}", named(k));
        let (code, out, _) = m.run(&["--json", "account", &target, "status"]);
        if shared(k, &kept) {
            assert!(code == 2 && out.contains(&several(k)), "{out}");
            continue;
        }
        let one: Value = serde_json::from_str(&out).unwrap();
        let account = &one["accounts"][0];
        assert_eq!((&account["subject"], &account["plugin"]), (&json!(k.subject), &json!(k.with)), "{one}");
        assert!(listed["accounts"].as_array().unwrap().contains(account), "{listed}");
        assert_eq!(account["refreshes"], json!(matches!(lasting, Lasting::Refreshed)), "{one}");
        assert_eq!(account["expires"].is_null(), matches!(lasting, Lasting::Ever), "{one}");
        let state = match lasting {
            Lasting::Refreshed => "signed in, refreshed on its own".to_string(),
            Lasting::Hour => {
                let at: chrono::DateTime<chrono::Utc> = serde_json::from_value(account["expires"].clone()).unwrap();
                format!("signed in until {}", locallibrary::local_time(at, "%Y-%m-%d %H:%M"))
            }
            Lasting::Ever => "signed in".to_string(),
        };
        // What the service granted, or when it doesn't say, what was asked.
        let allowed = joined(&account["scopes"], " ");
        assert_eq!(allowed == "read write", !unscoped, "{allowed}");
        let access = match one["accesses"].as_array().unwrap().is_empty() {
            true => "",
            false => "  access: echo (default)\n",
        };
        let shown = format!("{}\n  {state} · allowed: {allowed}\n{access}", called(k));
        assert_eq!(m.run(&["account", &target, "status"]).1, shown);
    }

    // b made echo's default; a signed out, and echo's access to it taken away.
    // By name only when it's b's alone: `plugin access` says a name two accounts of a type share
    // isn't found.
    let given = match naming {
        Naming::Typed => format!("mock:{}", b.subject),
        _ if shared(&b, &kept) => b.subject.to_string(),
        _ => named(&b),
    };
    let (code, out, err) = m.run(&maybe_json(json, &["plugin", "access", "echo", "--default", &given]));
    assert_eq!(code, 0, "{err}");
    match json {
        true => assert_eq!(report(&out)["account"]["subject"], json!("b"), "{out}"),
        false => assert_eq!(out, format!("✓ echo uses the {}\n", called(&b))),
    }
    let answered = echo_accesses(&m, two);
    let defaults: Vec<(&Value, &Value)> =
        answered.as_array().unwrap().iter().map(|x| (&x["account"]["subject"], &x["default"])).collect();
    assert_eq!(defaults, [(&json!("a"), &json!(false)), (&json!("b"), &json!(true))], "{answered}");

    let target = format!("mock:{}", a.subject);
    let (code, _, err) = m.run(&["account", &target, "sign-out", "--scope", "read"]);
    assert!(code == 2 && err.contains("--with, --for and --scope go with sign-in"), "{err}");
    let (code, out, err) = m.run(&maybe_json(json, &["account", &target, "sign-out"]));
    assert_eq!(code, 0, "{err}");
    match (json, revokes) {
        (true, _) => assert_eq!(report(&out)["revoked"], json!(revokes), "{out}"),
        (false, true) => assert_eq!(out, format!("✓ Signed out of {}\n", a.name)),
        (false, false) => assert_eq!(out, format!("✓ Signed out of {} on this computer\n", a.name)),
    }
    let (_, shown, _) = m.run(&["account"]);
    let signed_out = "signed out: `locallibrary account mock sign-in` signs in again\n  access: echo\n";
    assert!(shown.contains(&format!("{}\n  {signed_out}", called(&a))), "{shown}");
    let (code, out, err) = m.run(&maybe_json(json, &["plugin", "access", "echo", "--take-away", &target]));
    assert_eq!(code, 0, "{err}");
    if !json {
        assert_eq!(out, format!("✓ Took echo's access to the {} away\n", called(&a)));
    }
    assert_eq!(echo_accesses(&m, two).as_array().unwrap().len(), 1);

    #[cfg(unix)]
    {
        let with = ["--with", b.with];
        let (code, _, err) = m.pasting(&with, "d", true);
        assert!(code == 1 && err.contains("that address is from another sign-in: sign in again"), "{err}");
        let (code, out, err) = m.pasting(&with, "d", false);
        assert_eq!(code, 0, "{err}");
        let d = Kept { subject: "d", name: "d@mock.example".into(), with: b.with };
        assert!(out.starts_with(&format!("✓ Signed in to the {}\n", called(&d))), "{out}");
        let (_, listed, _) = m.run(&["--json", "account"]);
        let listed: Value = serde_json::from_str(&listed).unwrap();
        let ds = listed["accounts"].as_array().unwrap().iter().filter(|a| a["subject"] == "d").count();
        assert_eq!(ds, 1, "{listed}");
    }
}

/// For one account plugin providing the type or two, and any accounts signed in through them,
/// echo's destination without access asks in a terminal which one echo may use (each with its
/// account plugin when two provide the type), offering to sign in to another through each: the
/// account picked, or the one signed in to, becomes echo's default, and the destination is used.
#[cfg(unix)]
#[hegel::test(test_cases = 6, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn a_destination_asks_for_any_account_in_a_terminal(tc: TestCase) {
    let two = tc.draw(gs::booleans());
    let (m, providers) = providing(two, true);
    locallibrary::Store::open_or_init(&m.env.store).unwrap();
    let signed_in = &PEOPLE[..tc.draw(gs::integers::<usize>().max_value(PEOPLE.len()))];
    let mut offered: Vec<(String, Option<&str>)> = Vec::new();
    for subject in signed_in {
        let with = tc.draw(gs::sampled_from(providers.clone()));
        let mut c = m.command();
        c.args(["account", "mock", "sign-in", "--no-browser", "--timeout", "10", "--with", with]);
        assert_eq!(m.approving(c, &who(subject)).0, 0);
        let detail = if two { format!("  {with}") } else { String::new() };
        offered.push((format!("{subject}@mock.example{detail}"), None));
    }
    let another = if signed_in.is_empty() { "Sign in" } else { "Sign in to another" };
    for with in &providers {
        let through = if two { format!(" through {with}") } else { String::new() };
        offered.push((format!("{another}{through}"), Some(*with)));
    }
    let picked = tc.draw(gs::sampled_from((1..=offered.len()).collect::<Vec<_>>()));
    let (code, out, err) = m.typed(&COPY, &format!("{picked}\n"), "c");
    assert_eq!(code, 0, "{err}");
    let numbered: Vec<String> =
        offered.iter().enumerate().map(|(i, (l, _))| format!("  {:>3}. {l}\n", i + 1)).collect();
    let asked = format!("? Which Mock account may Echo use, to read? Tests\n{}", numbered.concat());
    assert!(err.contains(&asked), "{asked}\n{err}");
    // The account picked, or the one signed in to through the account plugin picked.
    let (chosen, with) = match &offered[picked - 1] {
        (_, Some(with)) => ("c", *with),
        (label, None) => (&label[..1], label.split("  ").nth(1).unwrap_or(WITH)),
    };
    let called = match two {
        true => format!("Mock account {chosen}@mock.example (through {with})"),
        false => format!("Mock account {chosen}@mock.example"),
    };
    assert!(out.starts_with(&format!("✓ Echo uses the {called}\n")), "{out}");
    let answered = echo_accesses(&m, two);
    let defaults: Vec<&Value> =
        answered.as_array().unwrap().iter().filter(|x| x["default"] == json!(true)).map(|x| &x["account"]).collect();
    assert_eq!(defaults, [&json!({ "plugin": with, "subject": chosen })], "{answered}");
}
