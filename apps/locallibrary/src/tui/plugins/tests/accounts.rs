//! Plugins' accounts in the player: signing in to each and out of it from the palette, in the
//! background.

use hegel::TestCase;
use hegel::generators as gs;

use super::super::super::app::View as Screen;
use super::super::super::app::testing::{
    blank, demo_drive, echo_account, echo_plugin, last_message, plugins_once, press, wait,
};
use super::{entry, picker};

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
