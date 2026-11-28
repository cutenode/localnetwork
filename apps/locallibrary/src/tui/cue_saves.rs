//! Hot cues set and deleted from the pads, written to the drive in the background one after
//! another. Each is drawn at once (see `deck::set_hot_cue`); one that can't be written is taken
//! back off the deck.

use std::path::PathBuf;

use locallibrary::anlz::Cue;
use locallibrary::api::{self, CueOp, EditCues};
use locallibrary::query::TrackSelector;

use super::app::{App, Level, UNDO_HINT};

/// A change to a track's hot cues, waiting to be written or being written.
#[derive(Debug, Clone)]
pub struct CueSave {
    /// The drive, and the track by its audio file there.
    pub root: PathBuf,
    pub file: String,
    pub op: CueOp,
    /// `hot cue B`
    pub what: String,
    /// What it did to the deck's cues, to take back if it isn't written.
    pub undo: Undo,
}

#[derive(Debug, Clone)]
pub enum Undo {
    /// The cue it added.
    Added(Cue),
    /// The cue it took away.
    Removed(Cue),
}

impl App {
    /// Write `save` once the ones before it are written.
    pub(super) fn save_cue(&mut self, save: CueSave) {
        self.cue_saves.push_back(save);
        self.next_cue_save();
    }

    /// Start writing the next cue change, if nothing else is being planned or written and no
    /// dialog is up (its answer would be lost under the plan's). The tick tries again.
    pub(super) fn next_cue_save(&mut self) {
        if self.job.is_some() || self.cue_saving.is_some() || self.modal.is_some() {
            return;
        }
        let Some(save) = self.cue_saves.pop_front() else { return };
        let req = EditCues {
            drive: save.root.display().to_string(),
            tracks: TrackSelector::paths([save.file.clone()]),
            edit: save.op.clone(),
        };
        let root = save.root.clone();
        self.cue_saving = Some(save);
        self.plan(root, move |ctx| Ok(Box::new(api::cues_plan(&req, ctx)?)));
        if self.job.is_none() {
            // Refused before it started (the drive went, or can't be written): said already.
            self.cue_save_failed(true);
        }
    }

    /// The cue change being written was written.
    pub(super) fn cue_saved(&mut self) {
        if let Some(save) = self.cue_saving.take() {
            self.toast(format!("{}{UNDO_HINT}", save.what), Level::Info);
        }
        self.next_cue_save();
    }

    /// The cue change being written wasn't: back off the deck with it (`said`: why was shown
    /// already), and on to the next.
    pub(super) fn cue_save_failed(&mut self, said: bool) {
        let Some(save) = self.cue_saving.take() else { return };
        let playing = self.now.as_mut().filter(|n| n.root == save.root && n.track.file_path == save.file);
        if let Some(now) = playing {
            let cues = &mut now.analysis.cues;
            match save.undo {
                Undo::Added(c) => cues.retain(|x| x.hot_cue != c.hot_cue),
                Undo::Removed(c) => {
                    cues.push(c);
                    cues.sort_by_key(|c| c.start_ms);
                }
            }
        }
        if !said {
            self.toast(format!("{}: not saved", save.what), Level::Error);
        }
        self.next_cue_save();
    }
}

#[cfg(test)]
mod tests;
