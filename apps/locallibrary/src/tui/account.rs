//! The Atmosphere account in the Profile view: the bar at its top says who's signed in, and `L`
//! signs in (typing the handle) or, pressed twice, out. Signing in and out run on threads, which
//! report back as [`AccountEvent`]s.

use std::path::Path;
use std::time::{Duration, Instant};

use locallibrary::api::{self, AccountInfo, Client};
use locallibrary::atmosphere;
use locallibrary::error::Reason;
use locallibrary::local_time;
use locallibrary::progress::{CancelToken, Ctx, NoProgress};
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::text::{Line, Span};

use super::app::{App, Level, Msg, typed_tail};
use super::theme::{self, bold, dim, fg};

/// News about the Atmosphere account, from the threads that sign in and out.
pub enum AccountEvent {
    /// The browser was sent to the account's server.
    /// `local_app`: signing in as the development client (locallibrary's document isn't online).
    Started { server: String, url: String, opened: bool, local_app: bool },
    /// Signing in or out finished (or stopped).
    Done { message: String, error: bool },
}

/// Signing in or out, while it happens.
enum SignIn {
    /// Typing the handle.
    Typing(String),
    /// Finding the account's server.
    Starting(CancelToken),
    /// Waiting for the browser.
    Waiting {
        server: String,
        url: String,
        opened: bool,
        local_app: bool,
        cancel: CancelToken,
    },
    SigningOut,
}

/// Who's signed in, and signing in or out while it happens.
#[derive(Default)]
pub struct Account {
    /// The signed-in Atmosphere account, as saved.
    pub info: Option<AccountInfo>,
    last_handle: Option<String>,
    sign_in: Option<SignIn>,
    /// When `L` was pressed once: a second press within a few seconds signs out.
    logout_armed: Option<Instant>,
}

impl Account {
    /// Read who's signed in from the store folder (no network).
    pub fn load(&mut self, store_root: &Path) {
        self.info = atmosphere::status(store_root).ok().flatten();
        self.last_handle = atmosphere::last_handle(store_root);
    }

    /// Whether signing in or out is under way (the header spins).
    pub fn busy(&self) -> bool {
        matches!(self.sign_in, Some(SignIn::Starting(_) | SignIn::Waiting { .. } | SignIn::SigningOut))
    }

    /// Whether a handle is being typed.
    pub fn typing(&self) -> bool {
        matches!(self.sign_in, Some(SignIn::Typing(_)))
    }

    /// Put away the handle being typed, as esc does (signing in already under way goes on).
    pub fn stop_typing(&mut self) {
        if self.typing() {
            self.sign_in = None;
        }
    }

    /// The bar at the top of the view.
    pub fn line(&self, width: u16) -> Line<'static> {
        let key = |k: &str| Span::styled(format!("  {k} "), theme::key());
        match (&self.sign_in, &self.info) {
            (Some(SignIn::Typing(text)), _) => Line::from(vec![
                Span::styled(" Atmosphere handle or server: ", bold(theme::CYAN)),
                Span::styled(typed_tail(text, (width as usize).saturating_sub(31)).to_string(), bold(theme::FG)),
                Span::styled("▏", fg(theme::PINK)),
                key("enter"),
                Span::styled("sign in", dim()),
                key("esc"),
                Span::styled("cancel", dim()),
            ]),
            (Some(SignIn::Starting(_)), _) => Line::from(vec![
                Span::styled(" ⟳ Finding your account's server…", fg(theme::CYAN)),
                key("esc"),
                Span::styled("stop", dim()),
            ]),
            (Some(SignIn::Waiting { server, url, opened, local_app, .. }), _) => {
                let mut spans = vec![Span::styled(format!(" ⟳ Approve in your browser at {server}"), fg(theme::CYAN))];
                if *local_app {
                    spans.push(Span::styled(
                        " as a local development app (locallibrary's page isn't online yet)",
                        dim(),
                    ));
                }
                if !opened {
                    spans.push(Span::styled(format!(" (the browser didn't open: {url})"), dim()));
                }
                spans.extend([key("esc"), Span::styled("stop", dim())]);
                Line::from(spans)
            }
            (Some(SignIn::SigningOut), _) => Line::styled(" ⟳ Signing out…", fg(theme::CYAN)),
            (None, Some(a)) => {
                let again = if a.expired() {
                    Span::styled(" · the session has likely ended; L signs in again", fg(theme::AMBER))
                } else {
                    Span::styled(
                        format!(
                            " · sign in again by {} ({} days)",
                            local_time(a.sign_in_again_by, "%Y-%m-%d"),
                            a.days_left()
                        ),
                        dim(),
                    )
                };
                let mut spans = vec![
                    Span::styled(" ● ", fg(theme::GREEN)),
                    Span::styled("Atmosphere account ", dim()),
                    Span::styled(a.name(), bold(theme::FG)),
                ];
                if a.client == Client::Loopback {
                    spans.push(Span::styled(" as the development client", dim()));
                }
                spans.extend([again, key("L"), Span::styled("sign out", dim())]);
                Line::from(spans)
            }
            (None, None) => Line::from(vec![
                Span::styled(" ○ Not signed in", dim()),
                key("L"),
                Span::styled(
                    "sign in with your Atmosphere account (the one for Bluesky and other AT Protocol apps; alpha)",
                    dim(),
                ),
            ]),
        }
    }
}

/// Keys the account takes before the Profile view's own: typing a handle takes every key until
/// enter or esc, and esc stops a sign-in under way. Returns whether the key was used.
pub fn on_input(app: &mut App, key: KeyEvent) -> bool {
    let t = &mut app.profile.account;
    if let Some(SignIn::Typing(text)) = &mut t.sign_in {
        match key.code {
            KeyCode::Esc => t.sign_in = None,
            KeyCode::Enter => {
                let input = text.trim().to_string();
                if input.is_empty() {
                    app.toast("type your handle (alice.bsky.social) or your server (https://…)", Level::Info);
                } else {
                    start_sign_in(app, input);
                }
            }
            KeyCode::Backspace => {
                text.pop();
            }
            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => text.clear(),
            KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => text.push(c),
            _ => {}
        }
        return true;
    }
    match (key.code, &t.sign_in) {
        (KeyCode::Esc, Some(SignIn::Starting(cancel) | SignIn::Waiting { cancel, .. })) => {
            cancel.cancel();
            true
        }
        // Signing in or out already.
        (KeyCode::Char('L'), Some(_)) => true,
        _ => false,
    }
}

/// `L`: sign in (type the handle); signed in, press twice to sign out.
pub fn press(app: &mut App) {
    let t = &mut app.profile.account;
    let Some(a) = t.info.clone() else {
        t.sign_in = Some(SignIn::Typing(t.last_handle.clone().unwrap_or_default()));
        return;
    };
    if t.logout_armed.take().is_some_and(|at| at.elapsed() < Duration::from_secs(4)) {
        start_sign_out(app);
    } else {
        t.logout_armed = Some(Instant::now());
        app.toast(format!("signed in as {}: press L again to sign out", a.name()), Level::Info);
    }
}

fn start_sign_in(app: &mut App, input: String) {
    let root = app.store_root.clone();
    let cancel = CancelToken::new();
    app.profile.account.sign_in = Some(SignIn::Starting(cancel.clone()));
    let started = app.profile.tx.clone();
    let job = move || {
        let req = api::AccountLoginRequest { handle: Some(input), ..Default::default() };
        let start = match api::account_login_begin(&root, &req) {
            Ok(s) => s,
            // The player takes no `--session-file`: the setting does it.
            Err(e) if e.reason() == Some(Reason::NoKeychain) => {
                return done(format!("{e}: start locallibrary with LOCALLIBRARY_SESSION_STORE=file"), true);
            }
            Err(e) => return done(e.to_string(), true),
        };
        if cancel.is_cancelled() {
            api::account_login_cancel(&api::AccountLoginCancel { id: start.id });
            return done("sign-in stopped".into(), false);
        }
        let opened = locallibrary::opener::open_url(&start.url).is_ok();
        let local_app = start.note.is_some();
        let _ = started.send(Msg::Account(AccountEvent::Started {
            server: start.server,
            url: start.url,
            opened,
            local_app,
        }));
        let progress = NoProgress;
        let ctx = Ctx::new(&progress, cancel);
        let finish = api::AccountLoginFinish { id: start.id, timeout_secs: None };
        match api::account_login_finish(&finish, &ctx) {
            Ok(info) => done(format!("signed in with your Atmosphere account as {}", info.name()), false),
            Err(locallibrary::Error::Cancelled) => done("sign-in stopped".into(), false),
            Err(e) => done(e.to_string(), true),
        }
    };
    super::app::worker(app.profile.tx.clone(), job, |e| done(e, true));
}

/// What a sign-in or sign-out came to.
fn done(message: String, error: bool) -> Msg {
    Msg::Account(AccountEvent::Done { message, error })
}

fn start_sign_out(app: &mut App) {
    let root = app.store_root.clone();
    app.profile.account.sign_in = Some(SignIn::SigningOut);
    let job = move || {
        let (message, error) = match api::account_logout(&root) {
            Ok(r) => {
                let said = match (r.account, r.revoked) {
                    (Some(a), true) => format!("signed out of {}", a.name()),
                    (Some(a), false) => {
                        format!("signed out of {} on this computer; the server couldn't be told", a.name())
                    }
                    (None, _) => "not signed in".into(),
                };
                match r.left_behind {
                    Some(why) => (
                        format!(
                            "{said}; its secrets are still here ({why}): `locallibrary account logout` tries again"
                        ),
                        true,
                    ),
                    None => (said, false),
                }
            }
            Err(e) => (e.to_string(), true),
        };
        done(message, error)
    };
    super::app::worker(app.profile.tx.clone(), job, |e| done(e, true));
}

/// A message from a sign-in or sign-out thread.
pub fn on_event(app: &mut App, e: AccountEvent) {
    let t = &mut app.profile.account;
    match e {
        AccountEvent::Started { server, url, opened, local_app } => {
            if let Some(SignIn::Starting(cancel)) = t.sign_in.take() {
                t.sign_in = Some(SignIn::Waiting { server, url, opened, local_app, cancel });
            }
        }
        AccountEvent::Done { message, error } => {
            t.sign_in = None;
            t.load(&app.store_root);
            app.toast(message, if error { Level::Error } else { Level::Info });
        }
    }
}

#[cfg(test)]
mod tests {
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
                        assert!(
                            bar.contains("Approve in your browser") && bar.contains("didn't open") != opened,
                            "{bar}"
                        );
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
}
