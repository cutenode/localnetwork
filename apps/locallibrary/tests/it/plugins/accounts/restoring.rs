//! Restoring access on the command line: signing out of an account takes the echo plugin's access
//! to it away and remembers it, and `account TYPE:NAME sign-in --restore|--reset`, `account
//! TYPE:NAME restore`, Automatically Restore Access (`account [TYPE:NAME] --restore-automatically
//! on|off`) and, in a terminal, the question signing in again asks give it back or forget it.

use hegel::TestCase;
use hegel::generators as gs;
use serde_json::{Value, json};

use super::cli::{report, who};
use crate::plugins::shared_accounts::Mock;

/// `account mock:a sign-in`, waiting for the browser (which the test plays) for 10 seconds.
const AGAIN: [&str; 6] = ["account", "mock:a", "sign-in", "--no-browser", "--timeout", "10"];

/// What's said when signing in to a again leaves echo's access to it to restore.
const HINT: &str = "echo had access to it: `locallibrary account mock:a restore` gives it back\n";

/// A signed in to for echo, which is given `write` too when `write`, then signed out of: echo's
/// access to it remembered, its default.
fn signed_out_of_a(write: bool) -> Mock {
    let m = Mock::new();
    let more: &[&str] = if write { &["--for", "echo", "--scope", "write"] } else { &["--for", "echo"] };
    assert_eq!(m.sign_in("a", more).0, 0);
    assert_eq!(m.run(&["account", "mock:a", "sign-out"]).0, 0);
    m
}

/// How many accesses `key` of `account --json` lists.
fn listed(m: &Mock, key: &str) -> usize {
    let (_, out, _) = m.run(&["--json", "account"]);
    serde_json::from_str::<Value>(&out).unwrap()[key].as_array().map_or(0, Vec::len)
}

/// Automatically Restore Access as it's set before signing in again in
/// [`prop_signing_in_again_restores_as_asked_or_set`].
#[derive(Debug, Clone, Copy)]
enum Auto {
    Unset,
    /// On for every account.
    AllOn,
    /// On for a alone.
    OwnOn,
    /// On for every account but off for a.
    OwnOff,
}

/// For echo's access to a signed-out account (with `write` or not), Automatically Restore Access
/// set any way [`Auto`] has, and signing in again with `--restore`, `--reset` or neither, for echo
/// or not, with `--json` or not: echo gets its access back as it was when `--restore` says, or
/// it's on for the account, or the sign-in is for echo, never with `--reset`. What's restored is
/// said in a line; what isn't, nor reset, is said with the command that restores it (nothing's
/// asked: there's no terminal) and listed under the account as access to restore. Setting
/// Automatically Restore Access says so in a line.
#[hegel::test(test_cases = 6)]
#[hegel::explicit_test_case(write = false, auto = Auto::AllOn, flag = None::<&str>, for_echo = false, json = false)]
#[hegel::explicit_test_case(write = true, auto = Auto::OwnOff, flag = None::<&str>, for_echo = false, json = true)]
#[hegel::explicit_test_case(write = true, auto = Auto::AllOn, flag = Some("--reset"), for_echo = false, json = false)]
#[hegel::explicit_test_case(write = true, auto = Auto::Unset, flag = Some("--restore"), for_echo = false, json = false)]
fn prop_signing_in_again_restores_as_asked_or_set(tc: TestCase) {
    let write = tc.draw(gs::booleans());
    let auto = tc.draw(gs::sampled_from(vec![Auto::Unset, Auto::AllOn, Auto::OwnOn, Auto::OwnOff]));
    let flag = tc.draw(gs::sampled_from(vec![None, Some("--restore"), Some("--reset")]));
    let for_echo = tc.draw(gs::booleans());
    let json = tc.draw(gs::booleans());
    let m = signed_out_of_a(write);
    let setting = |target: &[&str], on: &str, which: &str| {
        let (code, out, err) = m.run(&[&["account"][..], target, &["--restore-automatically", on]].concat());
        assert_eq!((code, out), (0, format!("✓ Automatically Restore Access is {on} for {which}\n")), "{err}");
    };
    let a = "the Mock account a@mock.example";
    match auto {
        Auto::Unset => {}
        Auto::AllOn => setting(&[], "on", "every account"),
        Auto::OwnOn => setting(&["mock:a"], "on", a),
        Auto::OwnOff => {
            setting(&[], "on", "every account");
            setting(&["mock:a@mock.example"], "off", a);
        }
    }

    let mut args = if json { vec!["--json"] } else { Vec::new() };
    args.extend(AGAIN);
    args.extend(flag);
    if for_echo {
        args.extend(["--for", "echo"]);
    }
    let mut c = m.command();
    c.args(&args);
    let (code, out, err) = m.approving(c, &who("a"));
    let said = format!("{auto:?} {flag:?} for echo {for_echo}: exit {code}\n{out}{err}");
    assert_eq!(code, 0, "{said}");
    let reset = flag == Some("--reset");
    let restored = !reset && (flag == Some("--restore") || for_echo || matches!(auto, Auto::AllOn | Auto::OwnOn));
    let left = !restored && !reset;
    let scopes = if write { "read, write" } else { "read" };
    match json {
        true => {
            let done = report(&out);
            let count = |key: &str| done[key].as_array().map_or(0, Vec::len);
            assert_eq!((count("restored"), count("remembered")), (usize::from(restored), usize::from(left)), "{said}");
        }
        false => {
            let back = format!("✓ Restored access: echo (default) · {scopes}\n");
            assert_eq!(out.contains(&back), restored, "{said}");
            assert_eq!(out.contains(HINT), left, "{said}");
            assert!(!err.contains("Give it back?"), "asked without a terminal: {said}");
        }
    }
    // Echo has what it had back, or (for echo, reset) what it needs, as its default.
    let has = match (restored, for_echo) {
        (true, _) => format!("  a@mock.example (default) · {scopes}\n"),
        (false, true) => "  a@mock.example (default) · read\n".to_string(),
        (false, false) => "  no access: `locallibrary account mock sign-in --for echo` gives it one\n".to_string(),
    };
    assert_eq!(m.run(&["plugin", "access", "echo"]).1, format!("mock: Tests\n{has}"), "{said}");
    let (_, shown, _) = m.run(&["account"]);
    assert_eq!(shown.contains("  access to restore: echo (default)\n"), left, "{said}\n{shown}");
    assert_eq!(listed(&m, "remembered"), usize::from(left), "{said}");
}

/// `account TYPE:NAME restore` gives echo back its access only once the account is signed in
/// again: signed out, it's refused saying to sign in first, and so are giving echo access to it
/// and making it echo's default. Restored, there's nothing more to give back.
#[test]
fn restoring_waits_for_the_account_to_be_signed_in() {
    let m = signed_out_of_a(false);
    let first = "error: the mock account a@mock.example is signed out: sign in to it first";
    for args in [
        &["account", "mock:a", "restore"][..],
        &["plugin", "access", "echo", "--give", "a"],
        &["plugin", "access", "echo", "--give", "a@mock.example", "--scope", "write"],
        &["plugin", "access", "echo", "--default", "a"],
    ] {
        let (code, out, err) = m.run(args);
        assert!(code == 1 && out.is_empty() && err.contains(first), "{args:?}: {err}");
    }
    assert_eq!(listed(&m, "remembered"), 1, "a refused step forgot it");

    let mut c = m.command();
    c.args(AGAIN);
    let (code, out, err) = m.approving(c, &who("a"));
    assert_eq!((code, out), (0, format!("✓ Signed in to the Mock account a@mock.example\n{HINT}")), "{err}");
    let (code, out, err) = m.run(&["account", "mock:a", "restore"]);
    assert_eq!((code, out.as_str()), (0, "✓ Restored access: echo (default) · read\n"), "{err}");
    let (code, out, err) = m.run(&["account", "mock:a", "restore"]);
    assert_eq!((code, out.as_str()), (0, "No plugin had access to it to give back.\n"), "{err}");
    let (code, out, err) = m.run(&["--json", "account", "mock:a", "restore"]);
    assert_eq!((code, report(&out)), (0, json!([])), "{err}");
    assert_eq!((listed(&m, "accesses"), listed(&m, "remembered")), (1, 0));
}

/// Signing out of the account echo uses by default leaves echo with no default, its access to
/// another account staying as it was; signing in again with `--restore` makes it the default
/// again unless echo was given another default since.
#[hegel::test(test_cases = 4)]
fn prop_signing_out_of_the_default_leaves_no_default(tc: TestCase) {
    let made_default = tc.draw(gs::booleans());
    let m = Mock::new();
    assert_eq!(m.sign_in("a", &["--for", "echo"]).0, 0);
    assert_eq!(m.sign_in("b", &[]).0, 0);
    assert_eq!(m.run(&["plugin", "access", "echo", "--give", "b"]).0, 0);
    let (_, out, _) = m.run(&["plugin", "access", "echo"]);
    assert_eq!(out, "mock: Tests\n  a@mock.example (default) · read\n  b@mock.example · read\n");

    assert_eq!(m.run(&["account", "mock:a", "sign-out"]).0, 0);
    let (_, out, _) = m.run(&["plugin", "access", "echo"]);
    assert_eq!(out, "mock: Tests\n  b@mock.example · read\n", "b isn't made the default");
    if made_default {
        assert_eq!(m.run(&["plugin", "access", "echo", "--default", "b"]).0, 0);
    }
    let mut c = m.command();
    c.args(AGAIN).arg("--restore");
    let (code, out, err) = m.approving(c, &who("a"));
    assert_eq!(code, 0, "{err}");
    let (a, b) = if made_default { ("", " (default)") } else { (" (default)", "") };
    assert!(out.ends_with(&format!("✓ Restored access: echo{a} · read\n")), "{out}");
    let (_, out, _) = m.run(&["plugin", "access", "echo"]);
    assert_eq!(out, format!("mock: Tests\n  b@mock.example{b} · read\n  a@mock.example{a} · read\n"));
}

/// How [`prop_in_a_terminal_signing_in_again_asks_what_to_give_back`] answers "Give it back?".
#[cfg(unix)]
#[derive(Debug, Clone, Copy)]
enum Answer {
    Restore,
    /// Choosing echo or not, and when it's chosen, `write` besides what it needs or not.
    Choose {
        echo: bool,
        write: bool,
    },
    Reset,
}

/// In a terminal, signing in again to an account echo had access to (with `write`) asks what to
/// give back, listing it: everything (Restore), the plugins chosen with the scopes chosen
/// besides what each needs (Choose), or nothing, forgetting it (Reset). Echo has as the answer
/// says, nothing's left to restore, and nothing printed holds a token the service gave.
#[cfg(unix)]
#[hegel::test(test_cases = 5)]
#[hegel::explicit_test_case(answer = Answer::Restore)]
#[hegel::explicit_test_case(answer = Answer::Choose { echo: true, write: false })]
#[hegel::explicit_test_case(answer = Answer::Reset)]
fn prop_in_a_terminal_signing_in_again_asks_what_to_give_back(tc: TestCase) {
    let answer = tc.draw(gs::sampled_from(vec![
        Answer::Restore,
        Answer::Choose { echo: true, write: true },
        Answer::Choose { echo: true, write: false },
        Answer::Choose { echo: false, write: false },
        Answer::Reset,
    ]));
    let m = signed_out_of_a(true);
    let typed = match answer {
        Answer::Restore => "1\n",
        Answer::Choose { echo: true, write: true } => "2\n1\n1\n",
        Answer::Choose { echo: true, write: false } => "2\n1\n\n",
        Answer::Choose { echo: false, .. } => "2\n\n",
        Answer::Reset => "3\n",
    };
    let (code, out, err) = m.typed(&AGAIN, typed, "a");
    let said = format!("{answer:?}: exit {code}\n{out}{err}");
    assert_eq!(code, 0, "{said}");
    let listing = "Plugins had access to the Mock account a@mock.example before it was signed out:\n  \
                   echo (default) · read, write\n";
    assert!(out.contains(listing), "{said}");
    assert!(err.contains("? Give it back?\n    1. Restore\n    2. Choose\n    3. Reset\n"), "{said}");
    let choosing = matches!(answer, Answer::Choose { .. });
    assert_eq!(err.contains("? Which plugins?\n    1. echo\n"), choosing, "{said}");
    let scoping = matches!(answer, Answer::Choose { echo: true, .. });
    assert_eq!(err.contains("? Which may echo do besides what it needs?\n    1. write  had it\n"), scoping, "{said}");
    assert!(!m.leaked(&said), "a token the service gave was printed: {said}");
    let has = match answer {
        Answer::Restore | Answer::Choose { echo: true, write: true } => Some("read, write"),
        Answer::Choose { echo: true, write: false } => Some("read"),
        _ => None,
    };
    let (back, access) = match has {
        Some(scopes) => (
            format!("✓ Restored access: echo (default) · {scopes}\n"),
            format!("  a@mock.example (default) · {scopes}\n"),
        ),
        None => (
            "✓ Reset: no plugin has access to it\n".to_string(),
            "  no access: `locallibrary account mock sign-in --for echo` gives it one\n".to_string(),
        ),
    };
    assert!(out.ends_with(&back), "{said}");
    assert_eq!(m.run(&["plugin", "access", "echo"]).1, format!("mock: Tests\n{access}"), "{said}");
    assert_eq!(listed(&m, "remembered"), 0, "{said}");
}
