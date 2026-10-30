//! `account`, `account TYPE sign-in`, `account TYPE:NAME status|sign-out` and `plugin access` at
//! the mock service, and what a plugin's destination does without access to an account it needs.

use hegel::TestCase;
use hegel::generators as gs;
use serde_json::{Value, json};

use crate::plugins::shared_accounts::{Mock, Model, PEOPLE, shape};
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
