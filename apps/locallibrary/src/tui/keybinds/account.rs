//! The keys and the account, on worker threads: publishing them a moment after they change,
//! loading the account's after signing in, and reading someone's to import.

use std::time::Instant;

use locallibrary::api::{self, AfterSignIn, KeybindsFetched};
use locallibrary::error::Reason;

use super::super::app::{App, Level, Msg};
use super::super::edits::{Modal, Prompt, PromptFor};
use super::effective::{self, Effective};
use super::manifests;

/// News from the keys' worker threads.
pub enum KeysEvent {
    /// A publish finished: why not, when it didn't (and whether the session lacks the scope).
    Published(Result<(), (String, bool)>),
    /// What the account had, read after signing in.
    AfterSignIn(Result<AfterSignIn, String>),
    /// Someone's keys, read to import.
    Fetched(Result<KeybindsFetched, String>),
}

fn send(e: KeysEvent) -> Msg {
    Msg::Keys(e)
}

impl App {
    /// Publish the keys once they've settled after a change, one publish at a time: a change
    /// while one runs publishes again after it.
    pub(in crate::tui) fn publish_keys(&mut self) {
        let due = self.publishing.due.is_some_and(|at| Instant::now() >= at);
        if !due || self.publishing.running {
            return;
        }
        self.publishing.due = None;
        if !self.profile.account.signed_in() {
            return;
        }
        self.publishing.running = true;
        let root = self.store_root.clone();
        let job = move || {
            let published = api::keybinds_publish(&root);
            send(KeysEvent::Published(published.map(|_| ()).map_err(|e| {
                let missing = e.reason() == Some(Reason::MissingScope);
                (e.to_string(), missing)
            })))
        };
        self.spawn_or(job, |e| send(KeysEvent::Published(Err((e, false)))));
    }

    /// Signed in: what the account published, against the keys here (see [`AfterSignIn`]).
    pub(in crate::tui) fn keys_after_sign_in(&mut self) {
        let root = self.store_root.clone();
        let job = move || send(KeysEvent::AfterSignIn(api::keybinds_after_sign_in(&root).map_err(|e| e.to_string())));
        self.spawn_or(job, |e| send(KeysEvent::AfterSignIn(Err(e))));
    }

    /// `import someone's keys…`: asks for their handle.
    pub(in crate::tui) fn ask_keys_from(&mut self) {
        let purpose = PromptFor::KeysFrom;
        let label = "Import the keys of (a handle or DID)".to_string();
        self.modal = Some(Modal::Prompt(Prompt { label, text: String::new(), purpose, root: self.store_root.clone() }));
    }

    /// Read the keys `who` published, to show what they'd change.
    pub(in crate::tui) fn fetch_keys(&mut self, who: String) {
        self.toast(format!("reading {who}'s keys…"), Level::Info);
        let job = move || send(KeysEvent::Fetched(api::keybinds_fetch(&who).map_err(|e| e.to_string())));
        self.spawn_or(job, |e| send(KeysEvent::Fetched(Err(e))));
    }

    pub(in crate::tui) fn on_keys_event(&mut self, e: KeysEvent) {
        if let KeysEvent::AfterSignIn(Ok(found)) = &e {
            let theirs = matches!(found, AfterSignIn::Loaded(_) | AfterSignIn::Same);
            self.publishing.published = theirs || *found == AfterSignIn::NonePublished { published_here: true };
        }
        match e {
            KeysEvent::Published(result) => {
                self.publishing.running = false;
                self.publishing.published = result.is_ok();
                self.publishing.error = match result {
                    Ok(()) => None,
                    Err((_, true)) if self.publishing.scope_said => Some(SIGN_IN_AGAIN.into()),
                    Err((_, true)) => {
                        self.publishing.scope_said = true;
                        self.toast(SIGN_IN_AGAIN, Level::Info);
                        Some(SIGN_IN_AGAIN.into())
                    }
                    Err((why, false)) => {
                        self.toast(format!("your keys weren't published: {why}"), Level::Error);
                        Some(why)
                    }
                };
            }
            KeysEvent::AfterSignIn(Ok(AfterSignIn::Loaded(theirs))) => {
                effective::install(Effective::new(&theirs, &manifests()));
                self.keybinds = theirs;
                let who = self.profile.account.info.as_ref().map(|a| a.name()).unwrap_or_default();
                self.toast(format!("loaded your keys from {who}"), Level::Info);
            }
            KeysEvent::AfterSignIn(Ok(AfterSignIn::NonePublished { published_here: true })) => {
                self.toast("published your keys", Level::Info);
            }
            KeysEvent::AfterSignIn(Ok(AfterSignIn::NonePublished { published_here: false } | AfterSignIn::Same)) => {}
            KeysEvent::AfterSignIn(Ok(AfterSignIn::Differs(f))) => {
                // The account's own: credited to whoever its record credits, not to itself.
                let title = format!("Your keys from {}", f.from.name());
                let credit = f.theirs.from.clone();
                self.preview_keys(title, f.theirs, credit, f.skipped.len());
            }
            KeysEvent::AfterSignIn(Err(why)) => {
                self.toast(format!("your published keys couldn't be read: {why}"), Level::Error);
            }
            KeysEvent::Fetched(Ok(f)) => {
                let title = format!("Keys from {}", f.from.name());
                self.preview_keys(title, f.theirs, Some(f.from.did), f.skipped.len());
            }
            KeysEvent::Fetched(Err(why)) => self.toast(why, Level::Error),
        }
    }
}

/// What a session from before the player published keys has to do first.
const SIGN_IN_AGAIN: &str = "sign in again to publish your keys";
