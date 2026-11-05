//! What the player remembers between runs.

use std::path::Path;
use std::time::{Duration, Instant};

use locallibrary::Track;
use locallibrary::query::FieldName;

use crate::tui::crate_list::CrateTrack;
use crate::tui::state::{self, Saved, SavedCrateTrack, SavedQueue};

use super::*;

impl App {
    pub(super) fn apply_saved(&mut self, s: &Saved, open_last_drive: bool) {
        self.sort = s.sort;
        self.search_all = s.search_all;
        self.show_details = s.details;
        self.show_crate = s.crate_panel;
        self.show_ratings = s.ratings;
        self.show_tags_column = s.tags_column;
        self.field_columns = s.field_columns.iter().map(|f| FieldName::new(f)).collect();
        // Before the first frame: drawn in it from the start.
        crate::tui::theme::select(s.theme.as_deref());
        self.shuffle = s.shuffle;
        self.repeat = match s.repeat.as_deref() {
            Some("all") => Repeat::All,
            Some("one") => Repeat::One,
            _ => Repeat::Off,
        };
        self.mix.set_recent(&s.mixes);
        self.mix.folders = s.mix_folders.clone();
        self.crate_list.items = (s.crate_list.iter())
            .map(|t| CrateTrack { root: t.root.clone(), drive: t.drive.clone(), track: t.track.clone() })
            .collect();
        self.crate_state.select((!self.crate_list.is_empty()).then_some(0));
        self.restore_queue = s.queue.clone();
        if let Some(v) = s.vis.as_deref().and_then(Vis::from_name) {
            self.vis = v;
        }
        self.deck.quantize = s.quantize.unwrap_or(true);
        self.deck.output = s.output.clone();
        self.deck.single = s.single;
        self.deck.auto_cue = s.auto_cue;
        if let Some(v) = s.volume {
            self.set_volume(v);
        }
        if open_last_drive {
            self.preferred_root = s.root.clone();
        }
        if let (Some(root), Some(node)) = (&s.root, &s.node) {
            self.restore = Some((root.clone(), node.clone()));
        }
    }

    /// What to remember for next time: the drive, list, view, sort, volume, visualiser, panels,
    /// crate and queue.
    pub(in crate::tui) fn saved_state(&self) -> Saved {
        let l = self.loaded.as_ref();
        Saved {
            root: l.map(|l| l.root().to_path_buf()),
            node: l.map(|l| l.save_node(&l.node)),
            view: Some(self.view.title().to_string()),
            sort: self.sort,
            search_all: self.search_all,
            volume: Some(self.volume),
            vis: Some(self.vis.name().to_string()),
            details: self.show_details,
            crate_panel: self.show_crate,
            ratings: self.show_ratings,
            tags_column: self.show_tags_column,
            field_columns: self.field_columns.iter().map(|f| f.as_str().to_string()).collect(),
            theme: crate::tui::theme::chosen().map(String::from),
            shuffle: self.shuffle,
            repeat: Some(format!("{:?}", self.repeat).to_lowercase()),
            quantize: Some(self.deck.quantize),
            output: self.deck.output.clone(),
            single: self.deck.single,
            auto_cue: self.deck.auto_cue,
            mixes: self.mix.recent.clone(),
            mix_folders: self.mix.folders.clone(),
            crate_list: (self.crate_list.items.iter())
                .map(|t| SavedCrateTrack { root: t.root.clone(), drive: t.drive.clone(), track: t.track.clone() })
                .collect(),
            queue: self.saved_queue().or_else(|| self.restore_queue.clone()),
        }
    }

    /// The queue as it's remembered: in the order it plays, at most `QUEUE_KEPT` tracks around
    /// the one it's on, and how far into that one (to the last 15 seconds, so playing along
    /// doesn't count as a change every moment).
    fn saved_queue(&self) -> Option<SavedQueue> {
        let q = &self.queue;
        if q.is_empty() || q.root.as_os_str().is_empty() {
            return None;
        }
        let len = q.order.len();
        let start = q.pos.saturating_sub(state::QUEUE_KEPT / 10).min(len.saturating_sub(state::QUEUE_KEPT));
        let end = (start + state::QUEUE_KEPT).min(len);
        let current = q.order.get(q.pos).map(|&i| q.track(i).file_path.as_str());
        let at = match &self.now {
            Some(n) if n.root == q.root && Some(n.track.file_path.as_str()) == current => self.position(),
            _ => self.resume.unwrap_or_default(),
        };
        Some(SavedQueue {
            root: q.root.clone(),
            drive: q.drive.clone(),
            tracks: q.order[start..end].iter().map(|&i| q.track(i).file_path.clone()).collect(),
            pos: q.pos - start,
            at_ms: at.as_millis() as u64 / 15_000 * 15_000,
        })
    }

    /// Save what to remember, now (on the way out).
    pub fn save_state(&mut self) {
        let saved = self.saved_state();
        state::save(&self.store_root, &saved);
        self.saved = Some(saved);
    }

    /// Save what to remember when it has changed (checked every couple of seconds), so a crash
    /// or a killed terminal loses little.
    pub(super) fn autosave(&mut self) {
        if self.last_save_check.elapsed() < Duration::from_secs(2) {
            return;
        }
        self.last_save_check = Instant::now();
        let saved = self.saved_state();
        if self.saved.as_ref() != Some(&saved) {
            state::save(&self.store_root, &saved);
            self.saved = Some(saved);
        }
    }

    /// The queue the last run had, on the drive just read: ready to carry on with space.
    pub(super) fn restore_queue(&mut self, root: &Path) {
        let Some(q) = self.restore_queue.take_if(|q| q.root == root) else { return };
        let Some(l) = self.loaded.as_ref().filter(|_| self.now.is_none() && self.queue.is_empty()) else {
            return;
        };
        let mut pos = 0;
        let mut items = Vec::new();
        for (i, path) in q.tracks.iter().enumerate() {
            if i == q.pos {
                pos = items.len();
            }
            items.extend(l.index_of(path));
        }
        if items.is_empty() {
            return;
        }
        let pos = pos.min(items.len() - 1);
        let library = l.library.clone();
        self.queue = Queue { order: (0..items.len()).collect(), library, items, root: q.root, drive: q.drive, pos };
        self.resume = Some(Duration::from_millis(q.at_ms));
    }

    /// The track space would carry on with, and from where, when nothing's playing.
    pub fn resumable(&self) -> Option<(&Track, Duration)> {
        let at = self.resume.filter(|_| self.now.is_none())?;
        let &i = self.queue.order.get(self.queue.pos)?;
        Some((self.queue.track(i), at))
    }
}
