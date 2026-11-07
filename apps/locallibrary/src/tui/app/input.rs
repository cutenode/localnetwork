//! Keys: what each does, and the lists they move through.

use std::time::{Duration, Instant};

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::tui::browse::{self, NodeId};
use crate::tui::edits::Action;
use crate::tui::keymap::{self, Act};

use super::*;

/// Above this many tracks, the search box waits for a pause in typing before narrowing the list.
pub(super) const TYPE_AHEAD: usize = 5_000;

/// A letter typed with shift, as a terminal reporting keys by their codes sends it (CSI u, or
/// xterm's modifyOtherKeys): the lowercase letter, and shift. Taken as the capital, as other
/// terminals send it, so `Q` isn't `q` (quit).
fn capital(mut key: KeyEvent) -> KeyEvent {
    let typed = !key.modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT);
    if let KeyCode::Char(c) = key.code
        && typed
        && key.modifiers.contains(KeyModifiers::SHIFT)
    {
        key.code = KeyCode::Char(c.to_ascii_uppercase());
    }
    key
}

/// Edit a line of text with a key: typing, backspace, ctrl-u to clear, ctrl-w to delete a word.
/// Returns whether the text changed.
pub fn edit_line(text: &mut String, key: KeyEvent) -> bool {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    match key.code {
        KeyCode::Backspace => text.pop().is_some(),
        KeyCode::Char('u') if ctrl => {
            let had = !text.is_empty();
            text.clear();
            had
        }
        KeyCode::Char('w') if ctrl => {
            let had = !text.is_empty();
            let cut = text.trim_end().rfind(' ').map_or(0, |i| i + 1);
            text.truncate(cut);
            had
        }
        KeyCode::Char(c) if !ctrl => {
            text.push(c);
            true
        }
        _ => false,
    }
}

/// The end of a line being typed that fits in `columns`, so the cursor drawn after it stays in
/// view. Measured in terminal columns the way ratatui measures a span: CJK and emoji take two.
pub fn typed_tail(text: &str, columns: usize) -> &str {
    use unicode_width::UnicodeWidthStr;
    let mut start = text.len();
    for (i, _) in text.char_indices().rev() {
        if text[i..].width() > columns {
            break;
        }
        start = i;
    }
    &text[start..]
}

impl App {
    /// `q`, ctrl-c, ctrl-q, or the terminal closing: quit, except while the drive is being
    /// written. Stopping a write halfway would leave the drive half edited (and marked as
    /// interrupted), so the player waits for it and quits once it's done. Planning reads only,
    /// so it just stops.
    pub fn request_quit(&mut self) {
        match &self.job {
            Some(j) if j.writing => {
                if !self.quit_after_write {
                    self.toast("still writing the drive: quitting when it's done (esc to stay)", Level::Info);
                }
                self.quit_after_write = true;
            }
            Some(_) => {
                self.cancel_job();
                self.quit = true;
            }
            None => self.quit = true,
        }
    }

    /// Before exiting for good (the screen is gone, or the event loop stopped): let a write to
    /// the drive that's under way finish.
    pub fn finish_write(&mut self) {
        while self.job.as_ref().is_some_and(|j| j.writing) {
            if let Ok(msg) = self.rx.recv_timeout(Duration::from_millis(100)) {
                self.handle_msg(msg);
            }
        }
    }

    pub fn on_key(&mut self, key: KeyEvent) {
        if self.late_answers.swallows(&key, Instant::now()) {
            return;
        }
        let key = capital(key);
        // The player may have gone on to the next track since the last tick: keys act on the one
        // playing (a seek asked of the one that ended would be lost).
        self.follow_queue();
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        if ctrl && matches!(key.code, KeyCode::Char('c') | KeyCode::Char('q')) {
            self.request_quit();
            return;
        }
        if self.chord.take().is_some() {
            // Whatever follows `g` ends it: what it names, or nothing.
            let goes = keymap::lookup(&[keymap::Scope::Goto], &key).filter(|_| !self.picking_view(&key));
            if let Some(b) = goes {
                self.perform(b.act, Some(key));
            }
            return;
        }
        if self.help {
            match key.code {
                KeyCode::Up | KeyCode::Char('k') => self.help_scroll = self.help_scroll.saturating_sub(1),
                KeyCode::Down | KeyCode::Char('j') => self.help_scroll = self.help_scroll.saturating_add(1),
                KeyCode::PageUp => self.help_scroll = self.help_scroll.saturating_sub(10),
                KeyCode::PageDown | KeyCode::Char(' ') => self.help_scroll = self.help_scroll.saturating_add(10),
                _ => self.help = false,
            }
            return;
        }
        if self.show_log {
            match key.code {
                KeyCode::Up | KeyCode::Char('k') => self.log_scroll = self.log_scroll.saturating_sub(1),
                KeyCode::Down | KeyCode::Char('j') => self.log_scroll = self.log_scroll.saturating_add(1),
                KeyCode::PageUp => self.log_scroll = self.log_scroll.saturating_sub(10),
                KeyCode::PageDown | KeyCode::Char(' ') => self.log_scroll = self.log_scroll.saturating_add(10),
                _ => self.show_log = false,
            }
            return;
        }
        // The outputs are drawn over a dialog that came up behind them.
        if self.deck.outputs.is_some() {
            crate::tui::deck::on_outputs_key(self, key);
            return;
        }
        if self.modal.is_some() {
            self.on_modal_key(key);
            return;
        }
        if self.searching {
            match key.code {
                KeyCode::Esc => self.close_search(),
                KeyCode::Enter => {
                    self.searching = false;
                    self.flush_search();
                    if let Some(e) = self.filter_error.clone() {
                        self.toast(e, Level::Error);
                    }
                }
                KeyCode::Tab | KeyCode::BackTab => {
                    self.search_all = !self.search_all;
                    self.refresh_visible();
                }
                KeyCode::Up | KeyCode::Down | KeyCode::PageUp | KeyCode::PageDown => {
                    self.flush_search();
                    let act = match key.code {
                        KeyCode::Up => Act::Move(-1),
                        KeyCode::Down => Act::Move(1),
                        KeyCode::PageUp => Act::Page(-1),
                        _ => Act::Page(1),
                    };
                    self.perform(act, None);
                }
                _ => {
                    if edit_line(&mut self.query, key) {
                        self.search_changed();
                    }
                }
            }
            return;
        }
        if self.picking.is_some() && self.on_pick_key(key) {
            return;
        }

        if self.view == View::Profile && crate::tui::account::on_input(self, key) {
            return;
        }
        if self.view == View::Mixes && crate::tui::mix::on_input(self, key) {
            return;
        }
        let scopes = keymap::scopes(self);
        // The player's own keys first, then those bound to plugins' commands.
        if let Some(b) = keymap::lookup(&scopes, &key) {
            self.perform(b.act, Some(key));
        } else if let Some(act) = keymap::bound::lookup(&self.plugin_keys, &key) {
            self.perform(act, Some(key));
        }
    }

    /// Picking a playlist stays in the library: `g` then only reaches the first row.
    fn picking_view(&self, key: &KeyEvent) -> bool {
        self.picking.is_some() && key.code != KeyCode::Char('g')
    }

    /// Do what a key (or the command palette) asks for. `key` is the key that was pressed, for
    /// the actions that take something from it (which digit, whether shift was held).
    pub(in crate::tui) fn perform(&mut self, act: Act, key: Option<KeyEvent>) {
        use crate::tui::{deck, mix, profile};
        let shift = key.is_some_and(|k| k.modifiers.contains(KeyModifiers::SHIFT));
        let digit = key.and_then(|k| match k.code {
            KeyCode::Char(c) => c.to_digit(10).filter(|d| *d > 0),
            _ => None,
        });
        let (in_profile, in_mixes) = (self.view == View::Profile, self.view == View::Mixes);
        match act {
            Act::None => {}
            Act::Quit => self.request_quit(),
            Act::Help => {
                self.help = true;
                self.help_scroll = 0;
            }
            Act::Palette => self.open_palette(),
            Act::Messages => {
                self.show_log = true;
                self.log_scroll = 0;
            }
            Act::PaneKey => {
                if let Some(d) = digit {
                    self.jump_to_pane(d);
                }
            }
            Act::Goto => self.chord = Some(Instant::now()),
            Act::ShowView(v) => self.set_view(v),
            Act::Refresh => self.refresh(),
            Act::PlayPause => {
                if !(in_mixes && mix::play(self)) {
                    self.toggle_pause();
                }
            }
            Act::Seek(dir) => self.seek_by(dir as f64 * if shift { 30.0 } else { 5.0 }),
            Act::Next => {
                if !(in_mixes && mix::track_jump(self, true)) {
                    self.advance(false);
                }
            }
            Act::Prev => {
                if !(in_mixes && mix::track_jump(self, false)) {
                    self.previous();
                }
            }
            Act::Stop => self.stop(),
            Act::Cue(forward) => self.jump_cue(forward),
            Act::Phrase(forward) => deck::phrase_jump(self, forward),
            Act::Volume(dir) => self.change_volume(0.05 * dir as f32),
            Act::Shuffle => {
                self.drop_upcoming();
                self.shuffle = !self.shuffle;
                if let Some(&cur) = self.queue.order.get(self.queue.pos) {
                    if self.shuffle {
                        self.shuffle_order(cur);
                    } else {
                        self.queue.order = (0..self.queue.len()).collect();
                        self.queue.pos = cur;
                    }
                }
                self.toast(if self.shuffle { "shuffle on" } else { "shuffle off" }, Level::Info);
            }
            Act::Repeat => {
                self.drop_upcoming();
                self.repeat = match self.repeat {
                    Repeat::Off => Repeat::All,
                    Repeat::All => Repeat::One,
                    Repeat::One => Repeat::Off,
                };
            }
            Act::GoTo => {
                if !(in_mixes && mix::go_to(self)) {
                    self.locate_playing();
                }
            }
            Act::Pads => deck::toggle_pads(self),
            Act::HotCue => {
                if let Some(d) = digit {
                    deck::hot_cue(self, d as u8 - 1);
                }
            }
            Act::BeatJump(beats) => deck::beat_jump(self, beats as f64),
            Act::Quantize => deck::toggle_quantize(self),
            Act::Single => deck::toggle_single(self),
            Act::AutoCue => deck::toggle_auto_cue(self),
            Act::Outputs => deck::list_outputs(self),
            Act::Loop => deck::toggle_loop(self),
            Act::LoopSize(double) => deck::resize_loop(self, if double { 2.0 } else { 0.5 }),
            Act::Visualiser => self.vis = self.vis.next(),
            Act::DeckLayout => {
                self.deck_layout = !self.deck_layout;
                self.browsing = false;
            }
            Act::Browse => self.browsing = true,
            Act::NextBands => self.next_bands(),
            Act::Bands(mode) => self.use_bands(mode),
            Act::KeyNotation(n) => self.use_notation(n),
            Act::KeyPalette(k) => self.use_key_palette(k),
            Act::EndWarning(secs) => self.use_end_warning(secs),
            Act::Art(on) => self.use_art(on),
            Act::ResetPlayed => self.reset_played(),
            Act::Zoom(true) => self.deck_secs = (self.deck_secs / 1.5).max(1.5),
            Act::Zoom(false) => self.deck_secs = (self.deck_secs * 1.5).min(60.0),
            Act::Move(dir) if in_profile => profile::move_by(self, dir as isize),
            Act::Move(dir) if in_mixes => mix::move_by(self, dir as isize),
            Act::Move(dir) => self.move_selection(dir as isize),
            Act::Page(dir) if in_profile => profile::move_by(self, 10 * dir as isize),
            Act::Page(dir) if in_mixes => mix::move_by(self, 10 * dir as isize),
            Act::Page(dir) => {
                let page = match self.focus {
                    Focus::Tracks if self.view == View::Library && !self.deck_showing() => {
                        self.areas.tracks.height.saturating_sub(3).max(1) as isize
                    }
                    _ => 10,
                };
                self.move_selection(page * dir as isize);
            }
            Act::End(dir) if in_profile => profile::move_by(self, 10_000 * dir as isize),
            Act::End(dir) if in_mixes => mix::move_by(self, 100_000 * dir as isize),
            Act::End(dir) => self.move_selection(if dir < 0 { isize::MIN / 2 } else { isize::MAX / 2 }),
            Act::Activate if in_profile => profile::switch_pane(self),
            Act::Activate if in_mixes => mix::enter(self),
            Act::Activate => self.activate(),
            Act::Escape => self.escape(),
            Act::Focus(back) => match self.view {
                View::Library => self.cycle_focus(back),
                View::Profile => profile::switch_pane(self),
                View::Mixes => mix::switch_pane(self),
                View::Drives => self.cycle_drive_pane(back),
                View::Prepare => {}
            },
            Act::Search => {
                self.searching = true;
                self.focus = Focus::Tracks;
            }
            Act::Sort => {
                self.sort = browse::cycle(self.sort);
                self.refresh_visible();
            }
            Act::SortReverse => {
                if let Some(k) = &mut self.sort {
                    k.descending = !k.descending;
                    self.refresh_visible();
                }
            }
            Act::Ratings => self.show_ratings = !self.show_ratings,
            Act::TagsColumn => {
                self.show_tags_column = !self.show_tags_column;
                if self.show_tags_column {
                    self.read_my_tags();
                }
            }
            Act::Tag => self.tag_tracks(),
            Act::SmartList => self.smart_list(),
            Act::SmartApply => self.smart_apply(),
            Act::SmartNew => self.smart_from_search(),
            Act::ExportList => self.export_list(),
            Act::Plugin(plugin, name) => self.plugin_here(plugin, name),
            Act::Analyze(plugin, analyzer) => self.analyze(plugin, analyzer, self.action_drive(), None),
            Act::ApplyFields(plugin, analyzer) => self.apply_fields(plugin, analyzer, self.action_drive()),
            Act::FieldColumn(name) => self.toggle_field_column(name),
            Act::PluginSettings(plugin) => self.plugin_settings(plugin),
            Act::AccountSignIn(kind, with) => self.account_sign_in(kind, with),
            Act::AccountSignOut(mark) => self.account_sign_out(mark),
            Act::PluginAccess(plugin) => self.plugin_access(plugin),
            Act::Review(mark) => self.review(mark),
            Act::Dismiss(mark) => self.dismiss(mark),
            Act::ProviderSettings => self.provider_settings(),
            Act::CheckSettings => self.check_settings(),
            Act::Fix(i) => self.fix(i),
            Act::Theme(key) => self.use_theme(key),
            Act::Colours(tier) => self.use_colours(tier),
            Act::Icons(set) => self.use_icons(set),
            Act::Export(plugin, format, from) => self.plugin_export(plugin, format, from, self.action_drive()),
            Act::Import(plugin, format) => self.plugin_import(plugin, format, self.action_drive()),
            Act::Source(plugin, source) => self.plugin_source(plugin, source, self.action_drive()),
            Act::Fetch(kind) => {
                let target = (self.view == View::Library).then(|| self.target()).flatten();
                self.fetch_form(kind, self.action_drive(), target.as_ref());
            }
            Act::Details => self.toggle_details(),
            Act::CratePanel => {
                self.show_crate = !self.show_crate;
                if !self.show_crate && self.focus == Focus::Crate {
                    self.focus = Focus::Tracks;
                }
                if self.crate_state.selected().is_none() && !self.crate_list.is_empty() {
                    self.crate_state.select(Some(0));
                }
            }
            Act::Mark => self.toggle_mark(),
            Act::MarkAll => self.mark_all(),
            Act::CrateAdd => self.quick(Action::CrateAdd),
            Act::Actions => self.open_menu(None),
            Act::Remove if in_mixes => mix::forget(self),
            Act::Remove => match self.focus {
                Focus::Crate => self.remove_from_crate(),
                Focus::Tracks => self.quick(Action::RemoveFromList),
                Focus::Playlists => self.quick(Action::Delete),
                Focus::Drives | Focus::Details => {}
            },
            Act::CrateMove(dir) => self.shift_crate(dir as isize),
            Act::CrateSave => self.quick(Action::SaveCrate),
            Act::CrateClear => self.quick(Action::ClearCrate),
            Act::Undo => self.undo(),
            Act::DriveActions => self.drive_menu(),
            Act::Pack => self.pack(if self.view == View::Drives { self.action_drive() } else { None }),
            Act::Replay if self.replay_offer.is_some() && key.is_some() => self.take_replay_offer(),
            Act::Pull
            | Act::Check
            | Act::SetPrep
            | Act::SyncGroup
            | Act::RestorePlaylist
            | Act::Replay
            | Act::Eject => self.drive_key(act),
            Act::ProfileInclude
            | Act::ProfilePin
            | Act::ProfileHide
            | Act::ProfileHideArtist
            | Act::ProfileDocument
            | Act::ProfileWrite
            | Act::ProfileScope
            | Act::ProfileRounding
            | Act::ProfileAccount => profile::perform(self, act),
            Act::MixOpen | Act::MixZoom | Act::MixExport | Act::MixReport => mix::perform(self, act),
        }
    }

    /// `R` (or F5): read again what the view shows, and the drives.
    pub(super) fn refresh(&mut self) {
        self.refresh_drives();
        match self.view {
            View::Drives => {
                self.refresh_status();
                if self.drive_tab == DrivePane::Snapshots {
                    self.refresh_history();
                }
            }
            View::Profile => crate::tui::profile::refresh(self),
            View::Mixes => crate::tui::mix::refresh(self),
            View::Library | View::Prepare => {
                if let Some(l) = &self.loaded {
                    let root = l.root().to_path_buf();
                    self.load_library(root);
                }
            }
        }
    }

    /// Esc: the view's own (closing the document, going back a pane, stopping an identification)
    /// first; then staying when asked to quit after a write, leaving full screen, stopping the
    /// job under way, hiding errors, clearing the search, and (pressed twice) the marks.
    fn escape(&mut self) {
        if (self.view == View::Profile && crate::tui::profile::escape(self))
            || (self.view == View::Mixes && crate::tui::mix::escape(self))
        {
            return;
        }
        if self.quit_after_write {
            self.quit_after_write = false;
            self.toast("staying: not quitting after the write", Level::Info);
        } else if self.view == View::Library && self.deck_layout {
            // Browse closes first; then back to the Library layout.
            self.deck_layout = std::mem::take(&mut self.browsing);
        } else if self.job.is_some() {
            self.cancel_job();
        } else if self.dismiss_errors() {
            // The error was put away: that's all this esc does.
        } else if let Some(o) = self.replay_offer.take() {
            self.replay_dismissed.insert(o.root);
            self.toast("not replaying: : and replay does it later", Level::Info);
        } else if !self.query.is_empty() {
            self.query.clear();
            self.refresh_visible();
        } else if !self.marks.is_empty() || !self.list_marks.is_empty() {
            // Marks can take a while to make: the first esc only says what a second does.
            if self.clear_marks_armed.take().is_some_and(|at| at.elapsed() < Duration::from_secs(4)) {
                self.marks.clear();
                self.list_marks.clear();
                self.toast("marks cleared", Level::Info);
            } else {
                self.clear_marks_armed = Some(Instant::now());
                let n = self.marks.len() + self.list_marks.len();
                self.toast(format!("esc again clears the {}", plural_marks(n)), Level::Info);
            }
        }
    }

    /// `i`: show or hide the details. Where the pane only shows with focus, it takes focus, and a
    /// pane left open behind another is brought back rather than closed.
    fn toggle_details(&mut self) {
        let by_focus = !self.areas.density.shows_details();
        if by_focus && self.show_details && self.focus != Focus::Details {
            self.focus = Focus::Details;
            return;
        }
        self.show_details = !self.show_details;
        if by_focus && self.show_details {
            self.focus = Focus::Details;
        }
        if !self.show_details && self.focus == Focus::Details {
            self.focus = Focus::Tracks;
        }
    }

    /// Tab: drives, playlists, tracks, then the side panels that are open, as they're numbered.
    fn cycle_focus(&mut self, back: bool) {
        let mut order = vec![Focus::Drives, Focus::Playlists, Focus::Tracks];
        if self.show_crate {
            order.push(Focus::Crate);
        }
        if self.show_details {
            order.push(Focus::Details);
        }
        let n = order.len();
        let i = order.iter().position(|f| *f == self.focus).unwrap_or(2);
        self.focus = order[if back { (i + n - 1) % n } else { (i + 1) % n }];
    }

    /// Put the search box away and clear what was typed in it, as esc does.
    fn close_search(&mut self) {
        self.searching = false;
        self.query.clear();
        self.refresh_visible();
    }

    pub(super) fn set_view(&mut self, v: View) {
        // The search box is the library's: keys typed in another view aren't for it.
        if self.searching && v != View::Library {
            self.close_search();
        }
        // So are the lines typed in the Mixes and Profile views: left open behind another view
        // (a tab clicked while typing), they'd take its every key, esc included.
        if v != View::Mixes {
            self.mix.input = None;
        }
        if v != View::Profile {
            self.profile.account.stop_typing();
        }
        self.view = v;
        match v {
            View::Drives => {
                if self.status.is_none() {
                    self.refresh_status();
                }
                self.show_drive_pane(self.drive_pane);
            }
            View::Profile if self.profile.view.is_none() && !self.profile.loading => crate::tui::profile::refresh(self),
            _ => {}
        }
    }

    pub(super) fn activate(&mut self) {
        match (self.view, self.focus) {
            (View::Library, _) if self.deck_showing() => self.play_next(),
            (View::Library, Focus::Tracks) => {
                self.play_selected();
                // As a CDJ does: a track loaded, the deck's back.
                self.browsing = false;
            }
            (View::Library, Focus::Playlists) => self.open_playlist(),
            (View::Library, Focus::Details) => self.play_match(),
            (View::Library, Focus::Crate) => self.play_crate(),
            (View::Prepare, _) => crate::tui::prepare::run(self),
            (View::Drives, _) if self.drive_pane == DrivePane::Snapshots => self.restore_from_selected_snapshot(),
            (View::Drives, _) if self.drive_pane == DrivePane::History => self.open_played(),
            (View::Drives, _) if self.drive_header.is_some() => self.activate_header(),
            (View::Library, Focus::Drives) | (View::Drives, _) => {
                if let Some(d) = self.drive_state.selected().and_then(|i| self.drives.get(i)) {
                    if !d.has_export {
                        self.toast("no rekordbox export on this drive", Level::Info);
                        return;
                    }
                    let root = d.root.clone();
                    self.load_library(root);
                    self.view = View::Library;
                    self.focus = Focus::Playlists;
                }
            }
            _ => {}
        }
    }

    /// Enter in the tree: open a list, or open or close a folder.
    fn open_playlist(&mut self) {
        let Some(l) = &mut self.loaded else { return };
        let Some(node) = self.playlist_state.selected().and_then(|i| l.tree.get(i)) else { return };
        let id = node.id.clone();
        if node.folder {
            l.toggle(&id);
            // Browse groups by My Tag too: read them as it opens.
            if id == NodeId::Browse {
                self.read_my_tags();
            }
            return;
        }
        self.show_list(id);
    }

    /// Show the list `id` in the track list, from its top.
    pub(super) fn show_list(&mut self, id: NodeId) {
        let Some(l) = &mut self.loaded else { return };
        l.node = id;
        self.visible.clear();
        self.track_state.select(Some(0));
        *self.track_state.offset_mut() = 0;
        self.refresh_visible();
        self.focus = Focus::Tracks;
    }

    pub(super) fn list_len(&self) -> usize {
        match (self.view, self.focus) {
            (View::Library, _) if self.deck_showing() => {
                self.now.as_ref().and_then(|n| n.matches.as_ref()).map_or(0, |m| m.items.len())
            }
            (View::Library, Focus::Tracks) => self.visible.len(),
            (View::Library, Focus::Playlists) => self.loaded.as_ref().map_or(0, |l| l.tree.len()),
            (View::Library, Focus::Details) => {
                self.details.as_ref().filter(|_| self.details_current()).map_or(0, |d| d.matches.items.len())
            }
            (View::Library, Focus::Crate) => self.crate_list.len(),
            (View::Prepare, _) => crate::tui::prepare::len(),
            (View::Drives, _) if self.drive_pane == DrivePane::Snapshots => self.history_rows(),
            (View::Drives, _) if self.drive_pane == DrivePane::History => self.played_lists().len(),
            (View::Library, Focus::Drives) | (View::Drives, _) => self.drives.len(),
            (View::Profile, _) => 0,
            (View::Mixes, _) => self.mix.sources.len(),
        }
    }

    pub fn history_rows(&self) -> usize {
        match &self.history {
            Some(Ok(libs)) => libs.iter().map(|(_, s)| s.len()).sum(),
            _ => 0,
        }
    }

    pub(in crate::tui) fn list_selection(&mut self) -> (&mut Option<usize>, usize) {
        let len = self.list_len();
        let sel = match (self.view, self.focus) {
            (View::Library, _) if self.deck_showing() => self.next_pick.selected_mut(),
            (View::Library, Focus::Tracks) => self.track_state.selected_mut(),
            (View::Library, Focus::Playlists) => self.playlist_state.selected_mut(),
            (View::Library, Focus::Details) => self.details_state.selected_mut(),
            (View::Library, Focus::Crate) => self.crate_state.selected_mut(),
            (View::Prepare, _) => self.prepare.selected_mut(),
            (View::Drives, _) if self.drive_pane == DrivePane::Snapshots => self.history_state.selected_mut(),
            (View::Drives, _) if self.drive_pane == DrivePane::History => self.played.selected_mut(),
            (View::Library, Focus::Drives) | (View::Drives, _) => self.drive_state.selected_mut(),
            (View::Profile, _) => self.profile.sections.selected_mut(),
            (View::Mixes, _) => self.mix.list.selected_mut(),
        };
        (sel, len)
    }

    pub(super) fn move_selection(&mut self, delta: isize) {
        match (self.view, self.drive_pane) {
            (View::Drives, DrivePane::List) => return self.move_drive_row(delta),
            (View::Drives, DrivePane::Info) => {
                let by = delta.clamp(i16::MIN as isize, i16::MAX as isize) as i16;
                self.drive_scroll = self.drive_scroll.saturating_add_signed(by);
                return;
            }
            _ => {}
        }
        let (sel, len) = self.list_selection();
        if len == 0 {
            *sel = None;
            return;
        }
        let cur = sel.unwrap_or(0) as isize;
        *sel = Some((cur + delta).clamp(0, len as isize - 1) as usize);
    }
}
