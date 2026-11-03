//! Accounts in the player: signing in to an account of a type and out of one the store keeps, and
//! giving a plugin access to one, from the palette, in the background.

use hegel::TestCase;
use hegel::generators as gs;
use locallibrary::plugin::{Access, AccessQuestion, AccountId};
use ratatui::crossterm::event::KeyCode;

use super::super::super::app::testing::{blank, key, last_message, plugins_once, press, wait};
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

/// For any steps (keeping a mock account as a sign-in leaves it, signing out of it, signing in
/// to another) from the palette: signing out of it is offered exactly while it's kept, and leaves
/// it signed out; signing in fails before the browser opens (its page is on this computer),
/// keeping what was kept; and the store keeps the account exactly when it was kept since it was
/// last signed out of.
#[hegel::test(test_cases = 10, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn signing_in_and_out_from_the_palette_leaves_the_accounts_as_the_steps_say(tc: TestCase) {
    plugins_once();
    let (mut app, _store) = blank(Screen::Library);
    let mut signed_in = false;
    for _ in 0..tc.draw(gs::integers::<usize>().min_value(2).max_value(6)) {
        match tc.draw(gs::integers::<u8>().max_value(2)) {
            0 => {
                kept(&app, &["a"]);
                signed_in = true;
            }
            1 => {
                press(&mut app, ":");
                let offered = picker(&app).items.iter().any(|i| i.label == "sign out of a@mock.example");
                press(&mut app, "\u{1b}");
                assert_eq!(offered, signed_in, "signing out is offered exactly while it's kept");
                if offered {
                    press(&mut app, ":sign out of a@mock.example\n");
                    assert_eq!(app.job.as_ref().map(|j| j.label.as_str()), Some("Signing out"));
                    wait(&mut app, |a| a.job.is_none());
                    assert_eq!(last_message(&app), "signed out of a@mock.example on this computer");
                }
                signed_in = false;
            }
            _ => {
                press(&mut app, ":sign in to a mock account\n");
                assert_eq!(app.job.as_ref().map(|j| j.label.as_str()), Some("Signing in"));
                wait(&mut app, |a| a.job.is_none());
                let said = last_message(&app);
                assert!(said.starts_with("signing in: ") && said.contains("can't be opened"), "it failed as it began");
            }
        }
        assert_eq!(subjects(&app), if signed_in { vec!["a".to_string()] } else { Vec::new() });
    }
}

/// For a mock account the store keeps, refreshed or not, signing itself out in any number of
/// hours (as its access token expires, or its refresh token runs out) or never: the player warns
/// of it as it starts exactly when that's within `WARN_DAYS`, saying where to sign in again.
#[hegel::test(test_cases = 20)]
fn the_player_warns_of_an_account_signing_itself_out_soon(tc: TestCase) {
    let (mut app, _store) = blank(Screen::Library);
    plugins_once();
    let refreshes = tc.draw(gs::booleans());
    let hours = tc.draw(gs::optional(gs::integers::<i64>().min_value(1).max_value(24 * 30)));
    let at = hours.map(|h| chrono::Utc::now() + chrono::Duration::hours(h));
    let lasting = if refreshes { "refresh_expires_at" } else { "expires_at" };
    kept(&app, &["a"]);
    let file = app.store_root.join("accounts.json");
    let mut book: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    book["accounts"][0]["refreshes"] = refreshes.into();
    book["accounts"][0][lasting] = serde_json::json!(at);
    std::fs::write(&file, book.to_string()).unwrap();
    app.warn_signing_out();
    let warned = hours.is_some_and(|h| h <= 24 * locallibrary::plugin::WARN_DAYS);
    let said = last_message(&app);
    assert_eq!(said.contains("the Mock account a@mock.example signs itself out "), warned, "{hours:?}: {said}");
    assert_eq!(said.ends_with(": sign in to it again from the palette (:)"), warned, "{said}");
}

/// What a plugin asks of the mock account `a`: `write`, signing in again to it or not.
fn wanting(sign_in: bool) -> AccessQuestion {
    let account = AccountId { plugin: "mock-accounts".into(), subject: "a".into() };
    let (kind, name, scopes) = ("mock".to_string(), "a@mock.example".to_string(), vec!["read".to_string()]);
    let access = Access {
        id: "0a1b2c3d".into(),
        plugin: "echo".into(),
        kind,
        account,
        name,
        scopes,
        default: true,
        declined: vec![],
    };
    let write = vec!["write".to_string()];
    let why = vec!["More".to_string()];
    AccessQuestion {
        plugin: "Echo".into(),
        access,
        title: "Mock".into(),
        scopes: write.clone(),
        titles: write,
        why,
        sign_in,
    }
}

/// For a plugin wanting more of an account, signing in again to or not, the player's asker shows
/// a form saying what it wants and why, and waits for its answer: enter allows it (or signs in
/// again to), "don't allow" doesn't, and esc leaves it unanswered; a sign-in it takes opens the
/// browser, or says where to go when it can't.
#[hegel::test(test_cases = 8)]
fn a_plugin_wanting_more_of_an_account_asks_in_a_form(tc: TestCase) {
    let (mut app, _store) = blank(Screen::Library);
    let ask = app.plugin_asker();
    let sign_in = tc.draw(gs::booleans());
    let (keys, answered) = tc.draw(gs::sampled_from(vec![("\n", Some(true)), (" \n", Some(false)), ("\u{1b}", None)]));
    let asking = std::thread::spawn(move || ask.allow(&wanting(sign_in)));
    wait(&mut app, |app| app.modal.is_some());
    let s = screen(&mut app, 120, 30);
    assert!(s.contains("Access for Echo"), "{s}");
    assert!(s.contains("Echo wants to write with the Mock account a@mock.example (More)"), "{s}");
    assert!(s.contains(if sign_in { "sign in again to allow" } else { "allow" }), "{s}");
    press(&mut app, keys);
    assert!(app.modal.is_none(), "{}", last_message(&app));
    assert_eq!(asking.join().unwrap(), answered);

    app.plugin_asker().sign_in("http://127.0.0.1:9/authorize?state=x");
    wait(&mut app, |app| last_message(app).contains("open this in your browser to sign in"));
}
