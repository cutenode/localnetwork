//! The Atmosphere account in the Profile view: the bar at its top says who's signed in, and `L`
//! signs in (typing the handle) or, pressed twice, out. Signing in and out run on threads, which
//! report back as [`AccountEvent`]s.

use std::path::Path;
use std::time::{Duration, Instant};

use locallibrary::api::{self, AccountInfo, Client};
use locallibrary::atmosphere;
use locallibrary::error::Reason;
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
                            a.sign_in_again_by.format("%Y-%m-%d"),
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
                    "sign in with your Atmosphere account (the one for Bluesky and other AT Protocol apps)",
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
