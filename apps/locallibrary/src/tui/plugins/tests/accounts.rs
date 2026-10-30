//! Accounts in the player: signing in to an account of a type and out of one the store keeps,
//! giving a plugin access to one, and signing in to a plugin's own accounts and out, from the
//! palette, in the background.

use hegel::TestCase;
use hegel::generators as gs;
use ratatui::crossterm::event::KeyCode;

use super::super::super::app::testing::{
    blank, demo_drive, echo_account, echo_plugin, key, last_message, plugins_once, press, wait,
};
use super::super::super::app::{App, View as Screen};
use super::super::super::ui::screen;
use super::{entry, picker};

/// The store at `app`'s keeping the mock accounts `subjects` (signed in through mock-accounts,
/// named `<subject>@mock.example`), as a sign-in leaves them, without their tokens.
fn kept(app: &App, subjects: &[&str]) {
    locallibrary::Store::open_or_init(&app.store_root).unwrap();
    let account = |s: &&str| {
        let name = format!("{s}@mock.example");
        serde_json::json!({ "type": "mock", "plugin": "mock-accounts", "subject": s, "name": name, "scopes": ["read"], "refreshes": true })
    };
    let book = serde_json::json!({ "accounts": subjects.iter().map(account).collect::<Vec<_>>(), "accesses": [] });
    std::fs::write(app.store_root.join("accounts.json"), book.to_string()).unwrap();
}

/// The accounts the store keeps, by subject.
fn subjects(app: &App) -> Vec<String> {
    let store = locallibrary::Store::open_or_init(&app.store_root).unwrap();
    let kept = crate::plugins::host().unwrap().kept_accounts(&store).unwrap();
    kept.into_iter().map(|a| a.subject).collect()
}

/// The account echo uses for mock accounts, by subject.
fn echos_default(app: &App) -> Option<String> {
    let store = locallibrary::Store::open_or_init(&app.store_root).unwrap();
    let accesses = crate::plugins::host().unwrap().accesses(&store, Some("echo"), None).unwrap();
    accesses.into_iter().find(|a| a.default).map(|a| a.account.subject)
}

/// The palette signs in to an account of each type an account plugin provides (in the
/// background, failing as it begins when its page is on this computer), signs out of each the
/// store keeps, and opens echo's access.
#[test]
fn accounts_are_signed_in_to_and_out_of_from_the_palette() {
    plugins_once();
    let (mut app, _store) = blank(Screen::Library);
    kept(&app, &["a", "b"]);
    press(&mut app, ":");
    for (label, detail) in [
        ("sign in to a Mock account…", "Mock accounts"),
        ("access for Echo…", "Echo"),
        ("sign out of a@mock.example", "Mock account"),
        ("sign out of b@mock.example", "Mock account"),
    ] {
        assert_eq!(entry(&app, label).0.detail, detail, "{label}");
    }
    press(&mut app, "\u{1b}:sign in to a mock account\n");
    assert_eq!(app.job.as_ref().map(|j| j.label.as_str()), Some("Signing in"));
    wait(&mut app, |a| a.job.is_none());
    let said = last_message(&app);
    assert!(said.starts_with("signing in: ") && said.contains("can't be opened"), "{said}");

    press(&mut app, ":sign out of a@mock.example\n");
    assert_eq!(app.job.as_ref().map(|j| j.label.as_str()), Some("Signing out"));
    wait(&mut app, |a| a.job.is_none());
    assert_eq!(last_message(&app), "signed out of a@mock.example on this computer");
    assert_eq!(subjects(&app), ["b"]);
    press(&mut app, ":");
    assert!(picker(&app).items.iter().all(|i| i.label != "sign out of a@mock.example"), "gone from the palette");
}

/// Echo's access is a form naming what echo needs a mock account for and why, offering the
/// accounts the store keeps and signing in to another: the one chosen becomes echo's default,
/// and the form starts on it next time; signing in to another fails as it begins here.
#[test]
fn the_access_form_says_what_for_and_makes_the_account_chosen_the_default() {
    plugins_once();
    let (mut app, _store) = blank(Screen::Library);
    kept(&app, &["a", "b"]);
    press(&mut app, ":access for echo\n");
    let shown = screen(&mut app, 120, 30);
    for text in ["Access for Echo", "mock  ‹ a@mock.example ›", "Echo needs it to read things: Tests", "enter save"]
    {
        assert!(shown.contains(text), "{text:?} isn't shown:\n{shown}");
    }
    key(&mut app, KeyCode::Right);
    press(&mut app, "\n");
    assert_eq!(app.job.as_ref().map(|j| j.label.as_str()), Some("Giving access"));
    wait(&mut app, |a| a.job.is_none());
    assert_eq!(last_message(&app), "Echo uses b@mock.example");
    assert_eq!(echos_default(&app).as_deref(), Some("b"));

    press(&mut app, ":access for echo\n");
    assert!(screen(&mut app, 120, 30).contains("mock  ‹ b@mock.example ›"), "the default first");
    key(&mut app, KeyCode::Right);
    assert!(screen(&mut app, 120, 30).contains("mock  ‹ sign in to another ›"));
    press(&mut app, "\n");
    wait(&mut app, |a| a.job.is_none());
    let said = last_message(&app);
    assert!(said.starts_with("giving access: ") && said.contains("can't be opened"), "{said}");
    assert_eq!(echos_default(&app).as_deref(), Some("b"), "as it was");
}

/// For any accounts the store keeps and any one chosen in echo's access form (from the one it
/// starts on, echo's default or the first), that one becomes echo's default.
#[hegel::test(test_cases = 10, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn whichever_account_is_chosen_becomes_echos_default(tc: TestCase) {
    plugins_once();
    let (mut app, _store) = blank(Screen::Library);
    let all = ["a", "b", "c"];
    let n = tc.draw(gs::integers::<usize>().min_value(1).max_value(3));
    kept(&app, &all[..n]);
    for _ in 0..tc.draw(gs::integers::<usize>().min_value(1).max_value(3)) {
        let before = echos_default(&app);
        let at = before.as_deref().and_then(|b| all.iter().position(|s| *s == b)).unwrap_or(0);
        let moves = tc.draw(gs::integers::<usize>().max_value(n - 1 - at));
        press(&mut app, ":access for echo\n");
        for _ in 0..moves {
            key(&mut app, KeyCode::Right);
        }
        press(&mut app, "\n");
        wait(&mut app, |a| a.job.is_none());
        let chosen = all[at + moves];
        assert_eq!(echos_default(&app).as_deref(), Some(chosen), "{before:?}, {moves} on");
    }
}

/// The echo plugin's account is in the palette by the plugin's name, with or without a drive,
/// signing in and out; and in no drive's menu.
#[test]
fn a_plugins_account_is_signed_in_to_and_out_of_from_the_palette() {
    let (dir, _root) = demo_drive();
    echo_plugin(dir.path());
    echo_account(dir.path());
    let (mut app, _store) = blank(Screen::Library);
    press(&mut app, ":");
    let labels: Vec<&str> = picker(&app).items.iter().map(|i| i.label.as_str()).collect();
    let settings = labels.iter().position(|l| *l == "Echo plugin settings…").unwrap();
    assert_eq!(
        labels[settings + 1..settings + 3],
        ["sign in to Echo Cloud…", "sign out of Echo Cloud"],
        "beside its settings"
    );
    for label in ["sign in to Echo Cloud…", "sign out of Echo Cloud"] {
        assert_eq!((entry(&app, label).0.detail.as_str(), entry(&app, label).1), ("Echo", None));
    }
    press(&mut app, "\u{1b}");

    // Signed out already: said so, and nothing's asked.
    press(&mut app, ":sign out of echo cloud\n");
    assert_eq!(app.job.as_ref().map(|j| j.label.as_str()), Some("Signing out"));
    wait(&mut app, |a| a.job.is_none());
    assert_eq!(last_message(&app), "not signed in to Echo Cloud");
}

/// Signing out of an account signed in forgets its tokens on this computer, and says the service
/// wasn't told when it has no way to be.
#[test]
fn signing_out_forgets_the_accounts_tokens() {
    let (dir, _root) = demo_drive();
    echo_plugin(dir.path());
    echo_account(dir.path());
    let (mut app, _store) = blank(Screen::Library);
    let store = locallibrary::Store::open_or_init(&app.store_root).unwrap();
    // As a sign-in leaves it (with secrets in files): the plugin's secrets file holds the tokens.
    let file = app.store_root.join("secrets/plugins/echo.json");
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    let kept = serde_json::json!({ "account:main": serde_json::json!({ "access_token": "a1" }).to_string() });
    std::fs::write(&file, kept.to_string()).unwrap();
    let plugins = crate::plugins::host().unwrap();
    assert!(plugins.account("echo", "main", Some(&store)).unwrap().signed_in);
    press(&mut app, ":sign out of echo cloud\n");
    wait(&mut app, |a| a.job.is_none());
    assert_eq!(last_message(&app), "signed out of Echo Cloud on this computer");
    assert!(!plugins.account("echo", "main", Some(&store)).unwrap().signed_in);
}

/// Signing in runs in the background, and an account whose sign-in page can't be opened fails
/// before the browser is sent anywhere.
#[test]
fn signing_in_is_a_task_that_fails_before_the_browser_opens_on_a_bad_page() {
    let (dir, _root) = demo_drive();
    echo_plugin(dir.path());
    echo_account(dir.path());
    let (mut app, _store) = blank(Screen::Library);
    press(&mut app, ":sign in to echo cloud\n");
    assert_eq!(app.job.as_ref().map(|j| j.label.as_str()), Some("Signing in"));
    wait(&mut app, |a| a.job.is_none());
    let said = last_message(&app);
    assert!(said.starts_with("signing in: ") && said.contains("can't be opened"), "it failed as it began");
    let store = locallibrary::Store::open_or_init(&app.store_root).unwrap();
    assert!(!crate::plugins::host().unwrap().account("echo", "main", Some(&store)).unwrap().signed_in);
}

/// For any steps (a sign-in kept as the host keeps one, with a refresh token or not; signing
/// out; signing in) from the palette: signing out says whether the account was signed in, and
/// leaves it signed out; signing in fails before the browser opens (its page is on this
/// computer), keeping what was kept; and the account is signed in exactly when a sign-in was
/// kept since it was last signed out of.
#[hegel::test(test_cases = 10, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn signing_in_and_out_from_the_palette_leaves_the_account_as_the_steps_say(tc: TestCase) {
    plugins_once();
    let (mut app, _store) = blank(Screen::Library);
    let store = locallibrary::Store::open_or_init(&app.store_root).unwrap();
    let plugins = crate::plugins::host().unwrap();
    let mut signed_in = false;
    for _ in 0..tc.draw(gs::integers::<usize>().min_value(2).max_value(6)) {
        match tc.draw(gs::integers::<u8>().max_value(2)) {
            0 => {
                let mut kept = serde_json::json!({ "access_token": "a1" });
                if tc.draw(gs::booleans()) {
                    kept["refresh_token"] = "r1".into();
                }
                let file = app.store_root.join("secrets/plugins/echo.json");
                std::fs::create_dir_all(file.parent().unwrap()).unwrap();
                std::fs::write(&file, serde_json::json!({ "account:main": kept.to_string() }).to_string()).unwrap();
                signed_in = true;
            }
            1 => {
                press(&mut app, ":sign out of echo cloud\n");
                assert_eq!(app.job.as_ref().map(|j| j.label.as_str()), Some("Signing out"));
                wait(&mut app, |a| a.job.is_none());
                let said = match signed_in {
                    true => "signed out of Echo Cloud on this computer",
                    false => "not signed in to Echo Cloud",
                };
                assert!(last_message(&app) == said, "not what signing out says");
                signed_in = false;
            }
            _ => {
                press(&mut app, ":sign in to echo cloud\n");
                assert_eq!(app.job.as_ref().map(|j| j.label.as_str()), Some("Signing in"));
                wait(&mut app, |a| a.job.is_none());
                let said = last_message(&app);
                assert!(said.starts_with("signing in: ") && said.contains("can't be opened"), "it failed as it began");
            }
        }
        assert_eq!(plugins.account("echo", "main", Some(&store)).unwrap().signed_in, signed_in);
    }
}
