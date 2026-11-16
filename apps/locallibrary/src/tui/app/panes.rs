//! Moving between panes: by the numbers in their titles, and through the Drives view's.

use locallibrary::Playlist;

use crate::tui::browse::{Category, NodeId};

use super::*;

impl App {
    /// A digit: the pane with that number in its title. A digit with none does nothing.
    pub(super) fn jump_to_pane(&mut self, n: u32) {
        match self.view {
            View::Library => match n {
                1 => self.focus = Focus::Drives,
                2 => self.focus = Focus::Playlists,
                3 => self.focus = Focus::Tracks,
                4 if self.show_crate => self.focus = Focus::Crate,
                5 if self.show_details => self.focus = Focus::Details,
                _ => {}
            },
            View::Prepare => {}
            View::Drives => match n {
                1 => self.show_drive_pane(DrivePane::List),
                2 => self.show_drive_pane(DrivePane::Info),
                3 => self.show_drive_pane(DrivePane::Snapshots),
                4 => self.show_drive_pane(DrivePane::History),
                _ => {}
            },
            View::Mixes => crate::tui::mix::jump_to_pane(self, n),
            View::Profile => crate::tui::profile::jump_to_pane(self, n),
        }
    }

    /// Give `pane` focus, reading the snapshots the first time they're shown. The list keeps what
    /// was beside it.
    pub(in crate::tui) fn show_drive_pane(&mut self, pane: DrivePane) {
        self.drive_pane = pane;
        if pane == DrivePane::List {
            return;
        }
        self.drive_tab = pane;
        if pane == DrivePane::Snapshots && self.history.is_none() {
            self.refresh_history();
        }
    }

    /// Tab in the Drives view.
    pub(super) fn cycle_drive_pane(&mut self, back: bool) {
        const ORDER: [DrivePane; 4] = [DrivePane::List, DrivePane::Info, DrivePane::Snapshots, DrivePane::History];
        let i = ORDER.iter().position(|p| *p == self.drive_pane).unwrap_or(0);
        self.show_drive_pane(ORDER[if back { (i + 3) % 4 } else { (i + 1) % 4 }]);
    }

    /// What players recorded on the drive open in the library, in its order.
    pub fn played_lists(&self) -> Vec<&Playlist> {
        let Some(l) = &self.loaded else { return Vec::new() };
        l.library.history.iter().filter(|h| !h.is_folder).collect()
    }

    /// Enter on what players recorded: that list, in the library.
    pub(super) fn open_played(&mut self) {
        let lists = self.played_lists();
        let Some(id) = self.played.selected().and_then(|i| lists.get(i)).map(|h| h.id) else { return };
        self.set_view(View::Library);
        self.show_list(NodeId::Group(Category::History, id.to_string()));
    }
}
