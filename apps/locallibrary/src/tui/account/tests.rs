use super::super::app::View;
use super::super::app::testing::{blank, last_message, wait};
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
    assert!(bar.contains("the session has likely ended; L signs in again"), "{bar}");
    assert!(!bar.contains("development client"), "a hosted client isn't called that: {bar}");

    t.info = Some(signed_in("2099-01-01T00:00:00Z"));
    let bar = shown(&t, 200);
    let by = local_time(t.info.as_ref().unwrap().sign_in_again_by, "%Y-%m-%d");
    assert!(bar.contains(&format!("sign in again by {by}")), "{bar}");
    assert!(!bar.contains("likely ended"), "{bar}");
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
    point(&mut app, LEFT, "1 Library");
    assert_eq!(app.view, View::Library);
    press(&mut app, "4");
    assert!(!screen(&mut app, 120, 30).contains("Atmosphere handle or server:"));
    press(&mut app, "q");
    assert!(app.quit, "q quits, not typed");
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
    assert!(!input(&mut app, KeyCode::Char('L'), KeyModifiers::NONE), "L goes to the view");

    let cancel = starting(&mut app);
    assert!(input(&mut app, KeyCode::Char('L'), KeyModifiers::NONE), "already signing in");
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

/// The first press of `L` signed in only arms signing out for a few seconds.
#[test]
fn a_second_press_long_after_the_first_asks_again() {
    let (mut app, _dir) = blank(View::Profile);
    app.profile.account.info = Some(signed_in("2099-01-01T00:00:00Z"));
    app.profile.account.logout_armed = Instant::now().checked_sub(Duration::from_secs(10));
    super::press(&mut app);
    assert!(!app.profile.account.busy(), "too long ago to count");
    assert_eq!(last_message(&app), "signed in as @dj.example.com: press L again to sign out");
}

/// Signing out runs on a thread, which says how it went; here nobody was signed in.
#[test]
fn signing_out_reports_back() {
    let (mut app, _dir) = blank(View::Profile);
    app.profile.account.info = Some(signed_in("2099-01-01T00:00:00Z"));
    super::press(&mut app);
    super::press(&mut app);
    assert!(app.profile.account.busy());
    wait(&mut app, |a| !a.profile.account.busy());
    assert!(matches!(&app.toast, Some((m, Level::Info, _)) if m == "not signed in"), "{:?}", app.toast);
    assert!(app.profile.account.info.is_none(), "read again from the store");
}

/// Typing a handle (L, then letters, backspace, ctrl-u) shows what's typed in the bar, as
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
            0 => {
                app.on_key(KeyEvent::new(KeyCode::Char('L'), KeyModifiers::SHIFT));
                match &mut typed {
                    Some(t) => t.push('L'),
                    None => typed = Some(String::new()),
                }
            }
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

/// Signed in, the bar names the account (as the development client, when it is) and says when
/// to sign in again, or that the session has likely ended; L once says how to sign out and L
/// again soon after signs out (only once: a second L while it does waits), L long after asks
/// again; a tab clicked while a handle is typed puts it away.
#[hegel::test(test_cases = 100)]
fn the_bar_says_whos_signed_in_and_l_twice_signs_out(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let (mut app, _dir) = blank(View::Profile);
    let days = tc.draw(gs::sampled_from(&[-400i64, -1, 3, 200][..]));
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
        assert!(bar.contains("the session has likely ended; L signs in again"), "{bar}");
    } else {
        let by = local_time(info.sign_in_again_by, "%Y-%m-%d");
        assert!(bar.contains(&format!("sign in again by {by} ({} days)", info.days_left())), "{bar}");
    }
    // A handle being typed is put away when another view is shown.
    if tc.draw(gs::booleans()) {
        app.profile.account.sign_in = Some(SignIn::Typing("dj".into()));
        app.profile.account.stop_typing();
        assert!(!app.profile.account.typing());
    }
    let long_ago = tc.draw(gs::booleans());
    super::press(&mut app);
    assert_eq!(last_message(&app), format!("signed in as {}: press L again to sign out", info.name()));
    if long_ago {
        app.profile.account.logout_armed = Some(Instant::now() - Duration::from_secs(5));
        super::press(&mut app);
        assert!(!app.profile.account.busy(), "asked again rather than signed out");
        assert!(app.profile.account.logout_armed.is_some());
        return;
    }
    super::press(&mut app);
    assert!(app.profile.account.busy() && shown(&app.profile.account, width).contains("Signing out…"));
    assert!(input(&mut app, KeyCode::Char('L'), KeyModifiers::SHIFT), "L waits for it");
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
