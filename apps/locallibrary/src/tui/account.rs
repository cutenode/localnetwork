//! The Atmosphere account in the Profile view: the bar at its top says who's signed in, and
//! signing in (by name in `:` or the view's `e` menu) asks for the handle; signed in, the same
//! asks whether to sign out. Signing in and out run on threads, which report back as
//! [`AccountEvent`]s. Signing in asks to publish the player's keys too, and loads the account's.

use std::path::Path;

use locallibrary::api::{self, AccountInfo, Client};
use locallibrary::atmosphere;
use locallibrary::error::Reason;
use locallibrary::local_time;
use locallibrary::progress::{CancelToken, Ctx, NoProgress};
use ratatui::crossterm::event::KeyEvent;
use ratatui::text::{Line, Span};

use super::app::{App, Level, Msg, edit_line, typed_tail};
use super::edits::Modal;
use super::icons;
use super::keymap::{self, Act, Scope, dialogs};
use super::profile::PROFILE;
use super::theme::{self, bold, dim, fg};
use super::ui::key_says;

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
}

impl Account {
    /// Read who's signed in from the store folder (no network).
    pub fn load(&mut self, store_root: &Path) {
        let status = api::account_status(store_root).ok();
        (self.info, self.last_handle) = status.map(|s| (s.account, s.last_handle)).unwrap_or_default();
    }

    /// Whether an account is signed in.
    pub fn signed_in(&self) -> bool {
        self.info.is_some()
    }

    /// Who's signed in, as their server reports them.
    pub fn handle(&self) -> Option<&str> {
        self.info.as_ref()?.handle.as_deref()
    }

    /// Whether signing in or out is under way (the status line spins).
    pub fn busy(&self) -> bool {
        matches!(self.sign_in, Some(SignIn::Starting(_) | SignIn::Waiting { .. } | SignIn::SigningOut))
    }

    /// Stop signing in, if that's what's under way: signing out just finishes. Returns whether
    /// it was.
    pub fn stop(&mut self) -> bool {
        match &self.sign_in {
            Some(SignIn::Starting(cancel) | SignIn::Waiting { cancel, .. }) => {
                cancel.cancel();
                true
            }
            _ => false,
        }
    }

    /// What signing in or out is called now: what the actions menu offers.
    pub fn action(&self) -> String {
        match &self.info {
            Some(a) => format!("sign out of {}", a.name()),
            None => "sign in…".into(),
        }
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
        // Each key set off by two spaces, then what it does there.
        let key =
            |scopes: &[Scope], act, says: &str| [vec![Span::raw("  ")], key_says(scopes, act, says, dim())].concat();
        // Signing in or out is in the view's actions menu, or on a key of its own.
        let account =
            keymap::key_name(&PROFILE, Act::ProfileAccount).or_else(|| keymap::key_name(&PROFILE, Act::PaneActions));
        let account_key = |says: &str| match &account {
            Some(k) => {
                vec![Span::raw("  "), Span::styled(k.clone(), theme::key()), Span::styled(format!(" {says}"), dim())]
            }
            None => vec![Span::styled(format!("  {says} in :"), dim())],
        };
        match (&self.sign_in, &self.info) {
            (Some(SignIn::Typing(text)), _) => Line::from(
                [
                    vec![
                        Span::styled(" Atmosphere handle or server: ", bold(theme::current().info)),
                        Span::styled(
                            typed_tail(text, (width as usize).saturating_sub(31)).to_string(),
                            bold(theme::current().text),
                        ),
                        Span::styled(icons::CURSOR.get(), fg(theme::current().accent)),
                    ],
                    key(&dialogs::PROMPT, Act::Activate, " sign in"),
                    key(&dialogs::PROMPT, Act::Escape, " cancel"),
                ]
                .concat(),
            ),
            (Some(SignIn::Starting(_)), _) => Line::from(
                [
                    vec![Span::styled(
                        format!(" {} Finding your account's server…", icons::BUSY.get()),
                        fg(theme::current().info),
                    )],
                    key(&PROFILE, Act::Escape, " stop"),
                ]
                .concat(),
            ),
            (Some(SignIn::Waiting { server, url, opened, local_app, .. }), _) => {
                let mut spans = vec![Span::styled(
                    format!(" {} Approve in your browser at {server}", icons::BUSY.get()),
                    fg(theme::current().info),
                )];
                if *local_app {
                    spans.push(Span::styled(
                        " as a local development app (locallibrary's page isn't online yet)",
                        dim(),
                    ));
                }
                if !opened {
                    spans.push(Span::styled(format!(" (the browser didn't open: {url})"), dim()));
                }
                spans.extend(key(&PROFILE, Act::Escape, " stop"));
                Line::from(spans)
            }
            (Some(SignIn::SigningOut), _) => {
                Line::styled(format!(" {} Signing out…", icons::BUSY.get()), fg(theme::current().info))
            }
            (None, Some(a)) => {
                // When to sign in again, only once it's close.
                let again = if a.expired() {
                    let again = account.as_ref().map_or_else(|| " in :".to_string(), |k| format!(" from {k}"));
                    Some(format!(" · the session has likely ended; sign in again{again}"))
                } else if a.due_soon() {
                    let by = local_time(a.sign_in_again_by, "%Y-%m-%d");
                    Some(format!(" · sign in again by {by} ({} days)", a.days_left()))
                } else {
                    None
                };
                let mut spans = vec![
                    Span::styled(icons::DOT.padded(), fg(theme::current().good)),
                    Span::styled("Atmosphere account ", dim()),
                    Span::styled(a.name(), bold(theme::current().text)),
                ];
                if a.client == Client::Loopback {
                    spans.push(Span::styled(" as the development client", dim()));
                }
                if let Some(again) = again {
                    spans.push(Span::styled(again, fg(theme::current().warning)));
                }
                spans.extend(account_key("sign out"));
                Line::from(spans)
            }
            (None, None) => Line::from(
                [
                    vec![Span::styled(format!(" {} Not signed in", icons::RING.get()), dim())],
                    account_key(
                        "sign in with your Atmosphere account (the one for Bluesky and other AT Protocol apps; alpha)",
                    ),
                ]
                .concat(),
            ),
        }
    }
}

/// Keys the account takes before the Profile view's own: typing a handle takes every key until
/// enter or esc, and esc stops a sign-in under way. Returns whether the key was used.
pub fn on_input(app: &mut App, key: KeyEvent) -> bool {
    let t = &mut app.profile.account;
    if let Some(SignIn::Typing(text)) = &mut t.sign_in {
        match keymap::dialog_act(&dialogs::PROMPT, &key) {
            Some(Act::Escape) => t.sign_in = None,
            Some(Act::Activate) => {
                let input = text.trim().to_string();
                if input.is_empty() {
                    app.toast("type your handle (alice.bsky.social) or your server (https://…)", Level::Info);
                } else {
                    start_sign_in(app, input);
                }
            }
            _ => {
                edit_line(text, key);
            }
        }
        return true;
    }
    match (keymap::dialog_act(&dialogs::PROMPT, &key), &t.sign_in) {
        (Some(Act::Escape), Some(SignIn::Starting(cancel) | SignIn::Waiting { cancel, .. })) => {
            cancel.cancel();
            true
        }
        _ => false,
    }
}

/// Sign in/out: sign in (type the handle); signed in, asks whether to sign out.
pub fn press(app: &mut App) {
    let t = &mut app.profile.account;
    // Signing in or out already.
    if t.sign_in.is_some() {
        return;
    }
    let Some(a) = t.info.clone() else {
        t.sign_in = Some(SignIn::Typing(t.last_handle.clone().unwrap_or_default()));
        return;
    };
    let question = format!("Sign out of {}?", a.name());
    app.modal = Some(Modal::Ask(question, Box::new(start_sign_out)));
}

fn start_sign_in(app: &mut App, input: String) {
    let root = app.store_root.clone();
    let cancel = CancelToken::new();
    app.profile.account.sign_in = Some(SignIn::Starting(cancel.clone()));
    let started = app.profile.tx.clone();
    // Typing a handle only starts signed out (`press`), so there's no session whose scopes to keep.
    let scopes = atmosphere::player_scopes();
    let job = move || {
        let req = api::AccountLoginRequest { handle: Some(input), scopes, ..Default::default() };
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
            let signing_in = matches!(t.sign_in, Some(SignIn::Waiting { .. } | SignIn::Starting(_)));
            t.sign_in = None;
            t.load(&app.store_root);
            app.toast(message, if error { Level::Error } else { Level::Info });
            if signing_in && !error && app.profile.account.info.is_some() {
                app.keys_after_sign_in();
            }
        }
    }
}

#[cfg(test)]
mod tests;
