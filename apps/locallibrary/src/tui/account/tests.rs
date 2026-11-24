use ratatui::crossterm::event::{KeyCode, KeyModifiers};

use super::super::app::View;
use super::super::app::testing::{blank, key, last_message, press as keys, wait};
use super::super::edits::Modal;
use super::*;

/// The bar as text.
fn shown(t: &Account, width: u16) -> String {
    t.line(width).spans.iter().map(|s| s.content.as_ref()).collect()
}

/// An account signed in as the hosted client, whose session lasts until `until`.
fn signed_in(until: &str) -> AccountInfo {
    serde_json::from_value(serde_json::json!({
        "did": "did:plc:abc",
        "handle": "dj.example.com",
        "pds": "https://pds.example",
        "auth_server": "https://pds.example",
        "scopes": ["atproto"],
        "signed_in_at": "2026-01-05T12:00:00Z",
        "sign_in_again_by": until,
        "access_expires_at": null,
        "client": { "kind": "hosted", "client_id": "https://example.com/client.json" },
        "storage": "file"
    }))
    .unwrap()
}

/// A key, as the account takes it before the view: whether it took it.
fn input(app: &mut App, code: KeyCode, modifiers: KeyModifiers) -> bool {
    on_input(app, KeyEvent::new(code, modifiers))
}

fn starting(app: &mut App) -> CancelToken {
    let cancel = CancelToken::new();
    app.profile.account.sign_in = Some(SignIn::Starting(cancel.clone()));
    cancel
}

#[test]
fn the_bar_says_how_signing_in_is_going() {
    let mut t = Account::default();
    assert!(!t.busy());
    t.sign_in = Some(SignIn::Starting(CancelToken::new()));
    assert!(shown(&t, 120).contains("Finding your account's server…  esc stop"));
    assert!(t.busy() && !t.typing());

    // The browser didn't open, so the address to open by hand is shown.
    t.sign_in = Some(SignIn::Waiting {
        server: "https://pds.example".into(),
        url: "https://pds.example/authorize".into(),
        opened: false,
        local_app: true,
        cancel: CancelToken::new(),
    });
    let bar = shown(&t, 200);
    assert!(bar.contains("Approve in your browser at https://pds.example"), "{bar}");
    assert!(bar.contains("as a local development app"), "{bar}");
    assert!(bar.contains("(the browser didn't open: https://pds.example/authorize)"), "{bar}");

    t.sign_in = Some(SignIn::Waiting {
        server: "https://pds.example".into(),
        url: "https://pds.example/authorize".into(),
        opened: true,
        local_app: false,
        cancel: CancelToken::new(),
    });
    let bar = shown(&t, 200);
    assert!(!bar.contains("didn't open") && !bar.contains("development app"), "{bar}");

    t.sign_in = Some(SignIn::SigningOut);
    assert!(shown(&t, 80).contains("Signing out…"));
    assert!(t.busy());
}

#[test]
fn a_session_past_its_end_asks_to_sign_in_again() {
    let mut t = Account { info: Some(signed_in("2020-01-01T00:00:00Z")), ..Default::default() };
    let bar = shown(&t, 200);
    assert!(bar.contains("Atmosphere account @dj.example.com"), "{bar}");
    assert!(bar.contains("the session has likely ended; sign in again from e"), "{bar}");
    assert!(!bar.contains("development client"), "a hosted client isn't called that: {bar}");

    // Far off, it isn't said when.
    t.info = Some(signed_in("2099-01-01T00:00:00Z"));
    let bar = shown(&t, 200);
    assert!(!bar.contains("sign in again") && !bar.contains("likely ended"), "{bar}");
}

/// A tab clicked while a handle is typed shows its view and puts the handle away, as esc
/// does, so the keys go to the view shown.
#[test]
fn a_tab_clicked_while_typing_a_handle_puts_it_away() {
    use super::super::app::testing::{LEFT, point, press};
    use super::super::ui::screen;
    let (mut app, _dir) = blank(View::Profile);
    super::press(&mut app);
    assert!(screen(&mut app, 120, 30).contains("Atmosphere handle or server:"));
    point(&mut app, LEFT, "Library");
    assert_eq!(app.view, View::Library);
    press(&mut app, "gn");
    assert!(!screen(&mut app, 120, 30).contains("Atmosphere handle or server:"));
    for _ in 0..4 {
        key(&mut app, KeyCode::Esc);
    }
    assert!(app.quit, "esc twice quits, not typed");
}

#[test]
fn typing_a_handle_edits_it_and_enter_wants_one() {
    let (mut app, _dir) = blank(View::Profile);
    app.profile.account.last_handle = Some("old.example.com".into());
    super::press(&mut app);
    assert!(app.profile.account.typing());
    assert!(shown(&app.profile.account, 120).contains("old.example.com"), "the last handle to begin with");

    // Ctrl-U clears it; enter with nothing typed says what to type.
    assert!(input(&mut app, KeyCode::Char('u'), KeyModifiers::CONTROL));
    assert!(input(&mut app, KeyCode::Char(' '), KeyModifiers::NONE));
    assert!(input(&mut app, KeyCode::Enter, KeyModifiers::NONE));
    assert!(app.profile.account.typing(), "still typing");
    assert!(last_message(&app).starts_with("type your handle"), "{}", last_message(&app));
    // Other control keys type nothing.
    assert!(input(&mut app, KeyCode::Char('x'), KeyModifiers::CONTROL));
    assert!(input(&mut app, KeyCode::Left, KeyModifiers::NONE));
    assert_eq!(shown(&app.profile.account, 120).matches('x').count(), 0);
}

#[test]
fn esc_stops_a_sign_in_and_l_waits_for_it() {
    let (mut app, _dir) = blank(View::Profile);
    assert!(!input(&mut app, KeyCode::Esc, KeyModifiers::NONE), "nothing to stop");

    let cancel = starting(&mut app);
    press(&mut app);
    assert!(matches!(app.profile.account.sign_in, Some(SignIn::Starting(_))), "already signing in");
    assert!(!input(&mut app, KeyCode::Char('x'), KeyModifiers::NONE));
    assert!(input(&mut app, KeyCode::Esc, KeyModifiers::NONE));
    assert!(cancel.is_cancelled());

    // The browser was sent on: esc still stops it.
    let cancel = starting(&mut app);
    let started = AccountEvent::Started {
        server: "https://pds.example".into(),
        url: "https://pds.example/authorize".into(),
        opened: true,
        local_app: false,
    };
    on_event(&mut app, started);
    assert!(shown(&app.profile.account, 120).contains("Approve in your browser at https://pds.example"));
    assert!(input(&mut app, KeyCode::Esc, KeyModifiers::NONE));
    assert!(cancel.is_cancelled());
}

/// News of a sign-in that was stopped meanwhile changes nothing.
#[test]
fn a_late_start_is_ignored() {
    let (mut app, _dir) = blank(View::Profile);
    let started = AccountEvent::Started { server: "s".into(), url: "u".into(), opened: true, local_app: false };
    on_event(&mut app, started);
    assert!(!app.profile.account.busy());
}

#[test]
fn a_failed_sign_in_says_so_as_an_error() {
    let (mut app, _dir) = blank(View::Profile);
    starting(&mut app);
    on_event(&mut app, AccountEvent::Done { message: "the server said no".into(), error: true });
    assert!(!app.profile.account.busy());
    assert!(matches!(&app.toast, Some((m, Level::Error, _)) if m == "the server said no"), "{:?}", app.toast);
}

/// Signed in, the actions menu offers to sign out of the account, and signing out asks first:
/// no keeps it signed in.
#[test]
fn signing_out_asks_first() {
    let (mut app, _dir) = blank(View::Profile);
    assert_eq!(app.profile.account.action(), "sign in…");
    app.profile.account.info = Some(signed_in("2099-01-01T00:00:00Z"));
    assert_eq!(app.profile.account.action(), "sign out of @dj.example.com");
    super::press(&mut app);
    assert!(matches!(&app.modal, Some(Modal::Ask(q, _)) if q == "Sign out of @dj.example.com?"));
    keys(&mut app, "n");
    assert!(!app.profile.account.busy() && app.profile.account.info.is_some());
}

/// Signing out runs on a thread, which says how it went; here nobody was signed in.
#[test]
fn signing_out_reports_back() {
    let (mut app, _dir) = blank(View::Profile);
    app.profile.account.info = Some(signed_in("2099-01-01T00:00:00Z"));
    super::press(&mut app);
    keys(&mut app, "y");
    assert!(app.profile.account.busy());
    wait(&mut app, |a| !a.profile.account.busy());
    assert!(matches!(&app.toast, Some((m, Level::Info, _)) if m == "not signed in"), "{:?}", app.toast);
    assert!(app.profile.account.info.is_none(), "read again from the store");
}

/// Typing a handle (signing in, then letters, backspace, ctrl-u) shows what's typed in the bar, as
/// much of its end as fits; esc puts it away; a sign-in under way goes from finding the
/// server to the browser and stops at esc or when it's done, saying how it went.
#[hegel::test(test_cases = 100)]
fn the_bar_follows_typing_and_signing_in(tc: hegel::TestCase) {
    use hegel::generators as gs;
    use unicode_width::UnicodeWidthStr;
    let (mut app, _dir) = blank(View::Profile);
    let mut typed: Option<String> = None;
    for _ in 0..tc.draw(gs::integers::<usize>().max_value(20)) {
        let width = tc.draw(gs::integers::<u16>().max_value(160));
        match tc.draw(gs::integers::<u8>().max_value(9)) {
            // Signing in from `:` (or the actions menu); while typing, L is a letter of the handle.
            0 => match &mut typed {
                Some(t) => {
                    app.on_key(KeyEvent::new(KeyCode::Char('L'), KeyModifiers::SHIFT));
                    t.push('L');
                }
                None => {
                    app.perform(Act::ProfileAccount, None);
                    typed = Some(String::new());
                }
            },
            1..=4 if typed.is_some() => {
                let c = tc.draw(gs::sampled_from(&['a', 'b', '.', '日', '😹', ' '][..]));
                assert!(input(&mut app, KeyCode::Char(c), KeyModifiers::NONE));
                typed.as_mut().unwrap().push(c);
            }
            5 if typed.is_some() => {
                assert!(input(&mut app, KeyCode::Backspace, KeyModifiers::NONE));
                typed.as_mut().unwrap().pop();
            }
            6 if typed.is_some() => {
                assert!(input(&mut app, KeyCode::Char('u'), KeyModifiers::CONTROL));
                typed.as_mut().unwrap().clear();
            }
            // Enter on nothing typed only says what to type (with something, it would sign in).
            7 if typed.as_ref().is_some_and(|t| t.trim().is_empty()) => {
                assert!(input(&mut app, KeyCode::Enter, KeyModifiers::NONE));
                assert!(last_message(&app).starts_with("type your handle"));
            }
            8 => {
                let taken = input(&mut app, KeyCode::Esc, KeyModifiers::NONE);
                assert_eq!(taken, typed.is_some(), "esc is the typing's");
                typed = None;
            }
            9 if typed.is_none() => {
                // A sign-in under way, as the thread reports it.
                let cancel = starting(&mut app);
                assert!(app.profile.account.busy() && shown(&app.profile.account, width).contains("server"));
                if tc.draw(gs::booleans()) {
                    let opened = tc.draw(gs::booleans());
                    let (server, url) = ("https://pds.example".to_string(), "https://pds.example/a".to_string());
                    on_event(&mut app, AccountEvent::Started { server, url, opened, local_app: false });
                    let bar = shown(&app.profile.account, 200);
                    assert!(bar.contains("Approve in your browser") && bar.contains("didn't open") != opened, "{bar}");
                }
                if tc.draw(gs::booleans()) {
                    assert!(input(&mut app, KeyCode::Esc, KeyModifiers::NONE));
                    assert!(cancel.is_cancelled());
                }
                let error = tc.draw(gs::booleans());
                on_event(&mut app, AccountEvent::Done { message: "sign-in stopped".into(), error });
                assert!(!app.profile.account.busy());
                assert_eq!(
                    app.toast.as_ref().map(|t| (t.0.as_str(), t.1 == Level::Error)),
                    Some(("sign-in stopped", error))
                );
            }
            _ => continue,
        }
        assert_eq!(app.profile.account.typing(), typed.is_some());
        let bar = shown(&app.profile.account, width);
        match &typed {
            Some(t) => {
                let tail = typed_tail(t, (width as usize).saturating_sub(31));
                assert!(t.ends_with(tail) && bar.contains(&format!("server: {tail}▏")), "{bar:?} for {t:?}");
                assert!(tail.width() <= (width as usize).saturating_sub(31));
            }
            None => assert!(bar.contains("Not signed in"), "{bar:?}"),
        }
    }
}

/// Signed in, the bar names the account (as the development client, when it is) and, as a
/// warning in the last two days, says when to sign in again, or that the session has likely ended;
/// signing out asks first, and yes signs out (only once: asked again while it does, it waits); a
/// handle being typed is put away when another view is shown.
#[hegel::test(test_cases = 100)]
fn the_bar_says_who_is_signed_in_and_signing_out_asks(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let (mut app, _dir) = blank(View::Profile);
    let days = tc.draw(gs::sampled_from(&[-400i64, -1, 1, 3, 200][..]));
    let until = (chrono::Utc::now() + chrono::Duration::days(days) + chrono::Duration::hours(12)).to_rfc3339();
    let mut info = signed_in(&until);
    let loopback = tc.draw(gs::booleans());
    if loopback {
        info.client = Client::Loopback;
    }
    let width = tc.draw(gs::integers::<u16>().min_value(40).max_value(220));
    app.profile.account.info = Some(info.clone());
    let bar = shown(&app.profile.account, width);
    assert!(bar.contains(&format!("Atmosphere account {}", info.name())), "{bar}");
    assert_eq!(bar.contains("as the development client"), loopback, "{bar}");
    if info.expired() {
        assert!(bar.contains("the session has likely ended; sign in again from e"), "{bar}");
    } else if days == 1 {
        let by = local_time(info.sign_in_again_by, "%Y-%m-%d");
        assert!(bar.contains(&format!("sign in again by {by} ({} days)", info.days_left())), "{bar}");
        // In the colour the terminal draws a warning with.
        let line = app.profile.account.line(width);
        let again = line.spans.iter().find(|s| s.content.contains("sign in again by")).unwrap();
        let warning = crate::tui::theme::fg(crate::tui::theme::current().warning);
        assert_eq!(again.style.fg, warning.fg, "{bar}");
    } else {
        assert!(!bar.contains("sign in again"), "{bar}");
    }
    // A handle being typed is put away when another view is shown.
    if tc.draw(gs::booleans()) {
        app.profile.account.sign_in = Some(SignIn::Typing("dj".into()));
        app.profile.account.stop_typing();
        assert!(!app.profile.account.typing());
    }
    let no = tc.draw(gs::booleans());
    super::press(&mut app);
    assert!(matches!(&app.modal, Some(Modal::Ask(q, _)) if *q == format!("Sign out of {}?", info.name())));
    if no {
        keys(&mut app, "n");
        assert!(!app.profile.account.busy(), "kept signed in");
        return;
    }
    keys(&mut app, "y");
    assert!(app.profile.account.busy() && shown(&app.profile.account, width).contains("Signing out…"));
    super::press(&mut app);
    assert!(matches!(app.profile.account.sign_in, Some(SignIn::SigningOut)), "a second sign out waits for it");
    wait(&mut app, |a| !a.profile.account.busy());
    assert_eq!(last_message(&app), "not signed in");
}

/// A handle is typed as its keys say (enter on nothing typed only asks for one), and while the
/// browser is waited on the bar says where, and how to get there if it didn't open.
#[hegel::test(test_cases = 60)]
fn a_handle_is_typed_as_its_keys_say(tc: hegel::TestCase) {
    use hegel::generators as gs;

    let (mut app, _dir) = blank(View::Profile);
    app.profile.account.sign_in = Some(SignIn::Typing(String::new()));
    let mut model = String::new();
    // Every kind of key in turn, then as drawn (enter only on nothing typed: it signs in).
    let keys = tc.draw(gs::integers::<usize>().min_value(5).max_value(30));
    for k in 0..keys {
        let kind = if k < 5 { k } else { tc.draw(gs::integers::<usize>().max_value(4)) };
        match kind {
            0 => {
                let c = tc.draw(gs::sampled_from(&['a', '.', ' ', 'é', 'Z'][..]));
                assert!(input(&mut app, KeyCode::Char(c), KeyModifiers::NONE));
                model.push(c);
            }
            1 => {
                assert!(input(&mut app, KeyCode::Backspace, KeyModifiers::NONE));
                model.pop();
            }
            2 => {
                assert!(input(&mut app, KeyCode::Char('u'), KeyModifiers::CONTROL));
                model.clear();
            }
            // Keys a handle has no use for are taken all the same.
            3 => {
                let code = tc.draw(gs::sampled_from(&[KeyCode::Tab, KeyCode::Left, KeyCode::Char('x')][..]));
                let mods = if code == KeyCode::Char('x') { KeyModifiers::CONTROL } else { KeyModifiers::NONE };
                assert!(input(&mut app, code, mods));
            }
            _ if model.trim().is_empty() => {
                assert!(input(&mut app, KeyCode::Enter, KeyModifiers::NONE));
                assert_eq!(last_message(&app), "type your handle (alice.bsky.social) or your server (https://…)");
            }
            _ => {}
        }
        assert!(matches!(&app.profile.account.sign_in, Some(SignIn::Typing(t)) if *t == model), "{model:?}");
        assert!(shown(&app.profile.account, 200).contains(&model), "the typed handle shows");
    }
    assert!(input(&mut app, KeyCode::Esc, KeyModifiers::NONE));
    assert!(app.profile.account.sign_in.is_none(), "esc puts it away");

    let (opened, local_app) = (tc.draw(gs::booleans()), tc.draw(gs::booleans()));
    let url = "https://pds.example/oauth/authorize?x=1".to_string();
    let cancel = CancelToken::new();
    app.profile.account.sign_in = Some(SignIn::Waiting {
        server: "pds.example".into(),
        url: url.clone(),
        opened,
        local_app,
        cancel: cancel.clone(),
    });
    let bar = shown(&app.profile.account, 200);
    assert!(bar.contains("Approve in your browser at pds.example"), "{bar}");
    assert_eq!(bar.contains("as a local development app"), local_app, "{bar}");
    assert_eq!(bar.contains(&format!("the browser didn't open: {url}")), !opened, "{bar}");
}

/// Enter on a handle starts signing in on a thread, which says how it went: here the client's
/// document is on this computer, which signing in refuses before going online.
#[test]
fn enter_on_a_handle_starts_signing_in() {
    // SAFETY: nextest runs each test in a process of its own, and no thread is running yet.
    unsafe {
        std::env::set_var("LOCALLIBRARY_OAUTH_CLIENT_ID", "https://localhost/c.json");
        std::env::set_var("LOCALLIBRARY_SESSION_STORE", "file");
    }
    let (mut app, _dir) = blank(View::Profile);
    super::press(&mut app);
    keys(&mut app, "dj.example.com\n");
    assert!(matches!(app.profile.account.sign_in, Some(SignIn::Starting(_))), "{:?}", app.toast);
    wait(&mut app, |a| !a.profile.account.busy());
    let said = "the client document at https://localhost/c.json can't be used";
    assert!(app.log.iter().any(|(m, level, _)| m.contains(said) && *level == Level::Error), "{:?}", app.log);
}

/// Signed in, the keys the account published are looked for: here Local Network's directory
/// can't be reached, which is said.
#[test]
fn signing_in_looks_for_the_accounts_keys() {
    // SAFETY: as above.
    unsafe { std::env::set_var("LOCALLIBRARY_HAPPYVIEW_URL", "http://127.0.0.1:9") };
    let (mut app, _dir) = blank(View::Profile);
    starting(&mut app);
    // What signing in leaves in the store.
    let account = serde_json::json!({
        "account": {
            "info": signed_in("2099-01-01T00:00:00Z"),
            "session_id": "s1",
            "redirect_uri": "http://127.0.0.1/callback",
            "session": {}
        }
    });
    std::fs::create_dir_all(&app.store_root).unwrap();
    std::fs::write(app.store_root.join("atmosphere.json"), account.to_string()).unwrap();
    on_event(&mut app, AccountEvent::Done { message: "signed in".into(), error: false });
    assert!(app.profile.account.signed_in());
    let read = |a: &App| a.log.iter().any(|(m, ..)| m.starts_with("your published keys couldn't be read: "));
    wait(&mut app, read);
}
