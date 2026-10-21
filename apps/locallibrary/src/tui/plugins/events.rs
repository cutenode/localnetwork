//! Plugins' events and services in the player: what they tell the user is a message, and what
//! they suggest waits in the palette to be reviewed (planned, then confirmed as any plan, or left
//! alone), with what the `watch` agent kept for it. The services the user turned on run while the
//! player does.

use std::sync::Arc;

use locallibrary::Store;
use locallibrary::plugin::{Cli, MAX_SUGGESTED, Notices, PluginNotice, take_suggested};
use loclib_plugin::Context;

use super::Run;
use crate::tui::app::{App, Level, Msg};
use crate::tui::keymap::Act;
use crate::tui::palette::Item;

/// The plugin's name, as its manifest has it (its id, if it's gone).
fn name(plugin: &str) -> String {
    crate::plugins::host().and_then(|p| p.manifest(plugin)).map_or_else(|_| plugin.into(), |m| m.name.clone())
}

impl App {
    /// Where what plugins tell the user goes while the player runs: its messages.
    pub(in crate::tui) fn plugin_notices(&self) -> Notices {
        let tx = self.sender();
        Arc::new(move |notice| {
            let _ = tx.send(Msg::Notice(vec![notice]));
        })
    }

    /// Runs the services the user turned on, and offers what the `watch` agent kept, in the
    /// background.
    pub(in crate::tui) fn start_plugins(&self) {
        let store_root = self.store_root.clone();
        let job = move || {
            let store = Store::open(&store_root).ok();
            if let Ok(plugins) = crate::plugins::host() {
                plugins.run_services(store.as_ref());
            }
            Msg::Notice(store.and_then(|s| take_suggested(&s).ok()).unwrap_or_default())
        };
        self.spawn_or(job, |e| Msg::Note(format!("plugins: {e}"), Level::Error));
    }

    /// What plugins tell the user, as messages: each suggestion kept to review (the newest
    /// [`MAX_SUGGESTED`]), saying so.
    pub(in crate::tui) fn told(&mut self, notices: Vec<PluginNotice>) {
        for notice in notices {
            let by = name(&notice.plugin);
            let review = if notice.operation.is_some() { " (: to review)" } else { "" };
            self.toast(format!("{by}: {}{review}", notice.message), Level::Info);
            if notice.operation.is_some() {
                if self.suggested.len() >= MAX_SUGGESTED {
                    self.suggested.pop_front();
                    self.suggested_gone += 1;
                }
                self.suggested.push_back(notice);
            }
        }
    }

    /// What plugins suggested, as the palette lists them: newest first.
    pub(in crate::tui) fn suggested_items(&self) -> Vec<(Act, Item)> {
        let all = self.suggested.iter().enumerate().rev();
        let item = |(i, n): (usize, &PluginNotice)| {
            (Act::Review(self.suggested_gone + i), Item::new(format!("review: {}", n.message), name(&n.plugin)))
        };
        all.map(item).collect()
    }

    /// Plans what a plugin suggested (the one at `place`, see [`Act::Review`]), on the drive
    /// actions work on: the plan is confirmed as any is, or it asks first. One gone since is not.
    pub(in crate::tui) fn review(&mut self, place: usize) {
        let Some(i) = place.checked_sub(self.suggested_gone) else { return };
        let Some(notice) = self.suggested.remove(i) else { return };
        let Some(suggested) = notice.operation else { return };
        let Ok(m) = crate::plugins::host().and_then(|p| p.manifest(&notice.plugin)) else { return };
        let Some(op) = m.contributes.operations.iter().find(|o| o.name == suggested.name) else { return };
        let root = self.action_drive();
        let context = Context { drive: root.as_deref().and_then(|r| self.plugin_drive(r)), ..Context::default() };
        let run = Run { plugin: &m.id, hook: Cli::Operation(op), root, context };
        self.run_plugin(run, suggested.params);
    }
}
