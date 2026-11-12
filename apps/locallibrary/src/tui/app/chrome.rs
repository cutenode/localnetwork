//! What the player says: toasts, the log, and whether anything's moving on screen.

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use super::*;

/// How long `g` waits for what follows before showing what can.
pub const CHORD_HINT: Duration = Duration::from_millis(400);

/// How many messages the log (`!`) keeps.
pub(super) const LOG: usize = 200;

/// What a message about a change `u` takes back ends with.
pub const UNDO_HINT: &str = " · u undo";

/// How long an info message stays: five seconds, longer for long ones.
pub fn reading_time(msg: &str) -> Duration {
    Duration::from_secs(5 + (msg.chars().count() as u64).saturating_sub(100) / 20)
}

impl App {
    /// Say something on the status line (and in the log).
    pub fn toast(&mut self, msg: impl Into<String>, level: Level) {
        let msg = msg.into();
        self.remember(msg.clone(), level);
        // An error that wasn't dismissed stays on screen, above what came after it.
        if let Some((m, Level::Error, _)) = self.toast.take()
            && m != msg
        {
            self.held = Some(m);
        }
        self.toast = Some((msg, level, Instant::now()));
    }

    /// Keep a message in the log (`!`).
    pub(in crate::tui) fn remember(&mut self, msg: String, level: Level) {
        if self.log.len() >= LOG {
            self.log.pop_front();
        }
        self.log.push_back((msg, level, Instant::now()));
    }

    /// Where plugins' logs go while the player runs: each line, with the plugin's id, to the log.
    pub(in crate::tui) fn plugin_log(&self) -> locallibrary::plugin::Log {
        let tx = self.sender();
        Arc::new(move |id, line| {
            let _ = tx.send(Msg::Log(format!("{id}: {line}")));
        })
    }

    /// Esc with an error on screen: put it away. Returns whether there was one.
    pub(super) fn dismiss_errors(&mut self) -> bool {
        let error = self.toast.as_ref().is_some_and(|(_, level, _)| *level == Level::Error);
        if error {
            self.toast = None;
        }
        self.held.take().is_some() || error
    }

    /// A drive's name as shown: its adopted name, else its volume's.
    pub fn drive_label(&self, root: &Path) -> String {
        self.drives
            .iter()
            .find(|d| d.root == root)
            .map(|d| d.name.clone().unwrap_or_else(|| d.volume_name.clone()))
            .or_else(|| self.loaded.as_ref().filter(|l| l.root() == root).map(|l| l.device.name()))
            .unwrap_or_else(|| root.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default())
    }

    /// Whether the screen needs redrawing at animation rate. The first drive scan counts: the loop
    /// only wakes for input or its timer, so its answer would otherwise wait out the slow timer.
    pub fn animating(&self) -> bool {
        self.scanning()
            || self.loading.is_some()
            || self.search_due.is_some()
            || self.status_loading
            || self.profile.loading
            || self.profile.account.busy()
            || self.mix.is_loading()
            || self.job.is_some()
            || self.details_asked.is_some()
            || (self.show_details && self.view == View::Library && self.panels.waiting(self.selected_key()))
            || (self.show_details
                && self.view == View::Library
                && self.selected_track().is_some()
                && !self.details_current()
                && !self.details_lookup_failed())
            || self.now.as_ref().is_some_and(|n| n.loading)
            || self.loaded.as_ref().is_some_and(Loaded::reading_set)
            || self.deck.outputs.as_ref().is_some_and(|o| o.listing.is_some())
            || self.deck.opening.is_some()
            || !self.analyzer.is_idle()
            || self.is_playing()
            || self.flash().is_some()
            || self.covers.busy()
            || self.transitions.active()
            || self.chord.is_some_and(|at| at.elapsed() < CHORD_HINT)
    }
}
