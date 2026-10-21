//! Plugins' events and services in the player: what they tell the user is a message, and what
//! they suggest waits in the palette to be reviewed (planned, then confirmed as any plan) or
//! dismissed, kept in the store until it is, with what the `watch` agent kept for it. The
//! services the user turned on run while the player does.

use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::Path;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex, PoisonError};

use locallibrary::plugin::{Cli, Notices, PluginNotice, forget_suggested, keep_suggested, suggested};
use locallibrary::{Result, Store};
use loclib_plugin::Context;

use super::Run;
use crate::tui::app::{App, Level, Msg};
use crate::tui::keymap::Act;
use crate::tui::palette::Item;

/// The plugin's name, as its manifest has it (its id, if it's gone).
fn name(plugin: &str) -> String {
    crate::plugins::host().and_then(|p| p.manifest(plugin)).map_or_else(|_| plugin.into(), |m| m.name.clone())
}

/// How the palette names a suggestion: the same for the same one, whatever else is kept.
fn mark(notice: &PluginNotice) -> u64 {
    let mut h = DefaultHasher::new();
    serde_json::to_string(notice).unwrap_or_default().hash(&mut h);
    h.finish()
}

/// Held while the player changes the suggestions kept and says what they are now: what it's
/// told last is what's kept.
static CHANGING: Mutex<()> = Mutex::new(());

/// `change`s the suggestions kept in the store at `store_root`, and tells the player what they
/// are then (or what went wrong).
fn change_kept(tx: &Sender<Msg>, store_root: &Path, change: impl FnOnce(&Path) -> Result<Vec<PluginNotice>>) {
    let _one = CHANGING.lock().unwrap_or_else(PoisonError::into_inner);
    let _ = tx.send(match change(store_root) {
        Ok(kept) => Msg::Suggested(kept),
        Err(e) => Msg::Note(format!("suggestions: {e}"), Level::Error),
    });
}

impl App {
    /// Where what plugins tell the user goes while the player runs: its messages, and what they
    /// suggest kept in the store (on the thread reading the plugin, so in the order it's told).
    pub(in crate::tui) fn plugin_notices(&self) -> Notices {
        let (tx, store_root) = (self.sender(), self.store_root.clone());
        Arc::new(move |notice: PluginNotice| {
            if notice.operation.is_some() {
                change_kept(&tx, &store_root, |root| keep_suggested(&Store::open_or_init(root)?, &notice));
            }
            let _ = tx.send(Msg::Notice(notice));
        })
    }

    /// [`change_kept`], in the background.
    fn change_kept(&self, change: impl FnOnce(&Path) -> Result<Vec<PluginNotice>> + Send + 'static) {
        let (tx, store_root) = (self.sender(), self.store_root.clone());
        std::thread::spawn(move || change_kept(&tx, &store_root, change));
    }

    /// Offers what plugins suggested before (in the player or the `watch` agent) and runs the
    /// services the user turned on, in the background.
    pub(in crate::tui) fn start_plugins(&self) {
        self.change_kept(|root| Store::open(root).map_or_else(|_| Ok(Vec::new()), |s| suggested(&s)));
        let store_root = self.store_root.clone();
        std::thread::spawn(move || {
            if let Ok(plugins) = crate::plugins::host() {
                plugins.run_services(Store::open(&store_root).ok().as_ref());
            }
        });
    }

    /// What a plugin tells the user, as a message, saying when it suggests something to review.
    pub(in crate::tui) fn told(&mut self, notice: PluginNotice) {
        let by = name(&notice.plugin);
        let review = if notice.operation.is_some() { " (: to review)" } else { "" };
        self.toast(format!("{by}: {}{review}", notice.message), Level::Info);
    }

    /// What plugins suggested, as the palette lists them, newest first: to review, or dismiss.
    pub(in crate::tui) fn suggested_items(&self) -> Vec<(Act, Item)> {
        let items = |n: &PluginNotice| {
            let by = name(&n.plugin);
            [
                (Act::Review(mark(n)), Item::new(format!("review: {}", n.message), &by)),
                (Act::Dismiss(mark(n)), Item::new(format!("dismiss: {}", n.message), &by)),
            ]
        };
        self.suggested.iter().rev().flat_map(items).collect()
    }

    /// The suggestion marked `marked`, no longer kept: none when it went since.
    fn forget(&mut self, marked: u64) -> Option<PluginNotice> {
        let i = self.suggested.iter().position(|n| mark(n) == marked)?;
        let notice = self.suggested.remove(i);
        let forgotten = notice.clone();
        self.change_kept(move |root| forget_suggested(&Store::open(root)?, &forgotten));
        Some(notice)
    }

    /// Plans what a plugin suggested (the one marked `marked`, see [`Act::Review`]), on the drive
    /// actions work on: the plan is confirmed as any is, or it asks first. One gone since is not.
    pub(in crate::tui) fn review(&mut self, marked: u64) {
        let Some(notice) = self.forget(marked) else { return };
        let Some(suggested) = notice.operation else { return };
        let Ok(m) = crate::plugins::host().and_then(|p| p.manifest(&notice.plugin)) else { return };
        let Some(op) = m.contributes.operations.iter().find(|o| o.name == suggested.name) else { return };
        let root = self.action_drive();
        let context = Context { drive: root.as_deref().and_then(|r| self.plugin_drive(r)), ..Context::default() };
        let run = Run { plugin: &m.id, hook: Cli::Operation(op), root, context };
        self.run_plugin(run, suggested.params);
    }

    /// No longer offers what a plugin suggested (the one marked `marked`).
    pub(in crate::tui) fn dismiss(&mut self, marked: u64) {
        if self.forget(marked).is_some() {
            self.toast("suggestion dismissed", Level::Info);
        }
    }
}
