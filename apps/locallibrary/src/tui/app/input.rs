//! Keys: what each does, and the lists they move through.

use std::time::{Duration, Instant};

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::tui::browse::{self, NodeId};
use crate::tui::edits::Action;
use crate::tui::keymap::{self, Act, dialogs};

use super::*;

/// Above this many tracks, the search box waits for a pause in typing before narrowing the list.
pub(super) const TYPE_AHEAD: usize = 5_000;

/// How the question before quitting with anything under way (what the jobs list shows) ends.
const QUIT_ANYWAY: &str = "quit anyway?";

/// How long after an esc with nothing left to close a second one quits.
pub const QUIT_AGAIN: Duration = Duration::from_secs(1);

/// A letter typed with shift, as a terminal reporting keys by their codes sends it (CSI u, or
/// xterm's modifyOtherKeys): the lowercase letter, and shift. Taken as the capital, as other
/// terminals send it, which no key is, so shift and `n` isn't `n` (next track).
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
    /// Quitting as a key or the palette asks: with anything under way (what the jobs list
    /// shows), asked first, and y or a second ctrl-c says yes. Already waiting for a write, there's
    /// nothing to ask. A signal quits through [`App::request_quit`], with no one to answer.
    fn ask_to_quit(&mut self) {
        let asked = matches!(&self.modal, Some(Modal::Ask(q, _)) if q.ends_with(QUIT_ANYWAY));
        let under_way = crate::tui::jobs::rows(self);
        match under_way.first() {
            Some(first) if !asked && !self.quit_after_write => {
                let what = match under_way.len() {
                    1 => format!("{} is", first.label),
                    n => format!("{n} jobs are"),
                };
                let question = format!("{what} under way: {QUIT_ANYWAY}");
                self.modal = Some(Modal::Ask(question, Box::new(|app: &mut App| app.request_quit())));
            }
            _ => {
                if asked {
                    self.modal = None;
                }
                self.request_quit();
            }
        }
    }

    /// Quit (esc twice with nothing open, ctrl-c, the palette's quit, or the terminal closing), except
    /// while the drive is being written. Stopping a write halfway would leave the drive half edited (and
    /// marked as interrupted), so the player waits for it and quits once it's done. Planning reads only,
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
        self.intro = false;
        let key = capital(key);
        // Only esc after esc quits.
        if key.code != KeyCode::Esc {
            self.quit_armed = None;
        }
        // The player may have gone on to the next track since the last tick: keys act on the one
        // playing (a seek asked of the one that ended would be lost).
        self.follow_queue();
        // The Keys screen takes every key but ctrl-c, which always quits: it's waiting for one.
        if self.keys.is_some() && !keymap::K::Ctrl('c').matches(&key) {
            return self.on_keys_key(key);
        }
        if keymap::quits(&key) {
            self.ask_to_quit();
            return;
        }
        if self.help {
            self.on_help_key(key);
            return;
        }
        if self.show_log {
            match keymap::dialog_act(&dialogs::MESSAGES, &key) {
                Some(Act::Escape) => self.show_log = false,
                Some(act) => self.log_scroll = scrolled(self.log_scroll, act),
                None => {}
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
            match keymap::dialog_act(&dialogs::SEARCH, &key) {
                Some(Act::Escape) => self.close_search(),
                Some(Act::Activate) => {
                    self.searching = false;
                    self.flush_search();
                    if let Some(e) = self.filter_error.clone() {
                        self.toast(e, Level::Error);
                    }
                }
                Some(Act::SearchWhere) => {
                    self.search_all = !self.search_all;
                    self.refresh_visible();
                }
                Some(act @ (Act::Move(_) | Act::Page(_) | Act::End(_))) => {
                    self.flush_search();
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
        if let Some(hit) = keymap::lookup(&scopes, &key) {
            self.perform(hit.act, Some(key));
        }
    }

    /// Keys while the help is open: typing narrows it, the arrows move, enter runs the row selected
    /// (with no key matching what's typed, searches the palette for it; never while picking a
    /// playlist, which keeps its own keys) and esc closes it (`?` too, with nothing typed).
    fn on_help_key(&mut self, key: KeyEvent) {
        let typed = key.code == KeyCode::Char('?') && !self.help_query.is_empty();
        match keymap::dialog_act(&dialogs::HELP, &key).filter(|_| !typed) {
            Some(Act::Escape) => self.help = false,
            Some(Act::Activate) if self.picking.is_some() => {}
            Some(Act::Activate) => {
                let rows = keymap::help_rows(self, &self.help_query);
                if rows.is_empty() && !self.help_query.trim().is_empty() {
                    self.help = false;
                    let query = std::mem::take(&mut self.help_query);
                    return self.open_palette_with(&query);
                }
                let chosen = rows.get(self.help_row).filter(|r| keymap::runs(r.binding)).map(|r| r.binding.act);
                if let Some(act) = chosen {
                    self.help = false;
                    self.perform(act, None);
                }
            }
            Some(act @ (Act::Move(_) | Act::Page(_) | Act::End(_))) => {
                self.help_row = usize::from(scrolled(self.help_row.min(u16::MAX.into()) as u16, act));
            }
            _ => {
                if edit_line(&mut self.help_query, key) {
                    self.help_row = 0;
                }
            }
        }
    }

    /// Do what a key (or the command palette) asks for. `key` is the key that was pressed, for
    /// the actions that take something from it (which digit, whether shift was held).
    pub(in crate::tui) fn perform(&mut self, act: Act, key: Option<KeyEvent>) {
        use crate::tui::{deck, mix, profile};
        let shift = key.is_some_and(|k| k.modifiers.contains(KeyModifiers::SHIFT));
        let (in_profile, in_mixes) = (self.view == View::Profile, self.view == View::Mixes);
        match act {
            // A dialog's own, done by the dialog.
            Act::None | Act::Answer(_) | Act::Change(_) | Act::CancelJob | Act::SearchWhere => {}
            Act::Quit => self.ask_to_quit(),
            Act::Help => {
                self.help = true;
                self.help_row = 0;
                self.help_query.clear();
            }
            Act::Palette => self.open_palette(),
            Act::Jobs => self.open_jobs(),
            Act::PaneActions => self.open_actions(),
            Act::OpenFolder => self.open_folder(),
            Act::Snapshots => self.browse_snapshots(),
            Act::SignIn => self.sign_in(),
            Act::Hints(hints) => self.use_hints(hints),
            Act::HelpHint(on) => self.use_help_hint(on),
            Act::Messages => {
                self.show_log = true;
                self.log_scroll = 0;
            }
            Act::QueueNext => self.queue_picked(true),
            Act::Edit(action) => match self.target() {
                Some(t) => self.run_action(action, t),
                None => self.toast("open a drive first", Level::Info),
            },
            Act::Fill => {
                self.fill = !self.fill;
                self.queue_changed();
                let msg = match self.fill {
                    true => "fill on: when the queue runs out, the best match that hasn't played goes on",
                    false => "fill off: the queue stops when it runs out",
                };
                self.toast(msg, Level::Info);
            }
            Act::ShowView(v) => self.set_view(v),
            Act::Refresh => self.refresh(),
            Act::PlayPause => self.toggle_pause(),
            Act::MixPlay => {
                if !mix::play(self) {
                    self.toggle_pause();
                }
            }
            Act::Seek(dir) => self.seek_by(dir as f64 * if shift { 30.0 } else { 5.0 }),
            Act::Next => self.advance(false),
            Act::Prev => self.previous(),
            Act::MixTrack(forward) => {
                if !mix::track_jump(self, forward) {
                    self.perform(if forward { Act::Next } else { Act::Prev }, None);
                }
            }
            Act::Stop => self.stop(),
            Act::Cue(forward) => self.jump_cue(forward),
            Act::Phrase(forward) => deck::phrase_jump(self, forward),
            Act::Volume(dir) => self.change_volume(0.05 * dir as f32),
            Act::Shuffle => {
                self.shuffle = !self.shuffle;
                if let Some(&cur) = self.queue.order.get(self.queue.pos) {
                    if self.shuffle {
                        self.shuffle_order(cur);
                    } else {
                        self.queue.order = (0..self.queue.len()).collect();
                        self.queue.pos = cur;
                    }
                }
                self.queue_changed();
                self.toast(if self.shuffle { "shuffle on" } else { "shuffle off" }, Level::Info);
            }
            Act::Repeat => {
                self.repeat = match self.repeat {
                    Repeat::Off => Repeat::All,
                    Repeat::All => Repeat::One,
                    Repeat::One => Repeat::Off,
                };
                self.queue_changed();
            }
            Act::GoTo => self.locate_playing(),
            Act::MixGoTo => {
                if !mix::go_to(self) {
                    self.locate_playing();
                }
            }
            Act::Pads => deck::toggle_pads(self),
            Act::HotCue => {
                if let Some(pad) = key.as_ref().and_then(keymap::pad_of) {
                    deck::hot_cue(self, pad);
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
            Act::DeckLayout => self.toggle_player_mode(),
            Act::Browse => self.browsing = true,
            Act::NextBands => self.next_bands(),
            Act::Bands(mode) => self.use_bands(mode),
            Act::KeyNotation(n) => self.use_notation(n),
            Act::KeyPalette(k) => self.use_key_palette(k),
            Act::EndWarning(secs) => self.use_end_warning(secs),
            Act::Art(on) => self.use_art(on),
            Act::ArtHalfblocks(on) => self.use_art_halfblocks(on),
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
            Act::Activate if self.empty() && self.focus == Focus::Tracks => self.take_step(),
            Act::Activate if in_profile => profile::switch_pane(self),
            Act::Activate if in_mixes => mix::enter(self),
            Act::Activate => self.activate(),
            Act::Escape => {
                if !self.escape() {
                    self.arm_quit();
                }
            }
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
            Act::Run(job) => self.run_cli(job),
            Act::Copy(command) => self.copy_command(command),
            Act::Keys => self.open_keys(),
            #[cfg(feature = "account")]
            Act::KeysImport => self.ask_keys_from(),
            #[cfg(not(feature = "account"))]
            Act::KeysImport => self.toast("this locallibrary was built without the Atmosphere account", Level::Info),
            Act::KeysReset => self.reset_keys(),
            Act::KeysPreset(preset) => {
                use crate::tui::keybinds::presets;
                self.preview_keys(presets::title(preset).into(), presets::keys(preset), None, 0);
            }
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
            Act::CratePanel => self.toggle_queue_panel(),
            Act::Mark => self.toggle_mark(),
            Act::MarkAll => self.mark_all(),
            Act::CrateAdd => self.queue_picked(false),
            Act::Actions => self.open_menu(None),
            Act::MixForget => mix::forget(self),
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
            | Act::ProfileHideArtist
            | Act::ProfileDocument
            | Act::ProfileWrite
            | Act::ProfileRounding
            | Act::ProfileAccount => profile::perform(self, act),
            Act::MixOpen | Act::MixZoom | Act::MixExport | Act::MixReport => mix::perform(self, act),
        }
    }

    /// f5: read again what the view shows, and the drives.
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
    /// first; then staying when asked to quit after a write, leaving full screen, asking to stop
    /// the job under way, hiding errors, clearing the search, closing the pane with focus, and
    /// (pressed twice) the marks. Returns whether it closed anything.
    fn escape(&mut self) -> bool {
        if (self.view == View::Profile && crate::tui::profile::escape(self))
            || (self.view == View::Mixes && crate::tui::mix::escape(self))
        {
            return true;
        }
        if self.quit_after_write {
            self.quit_after_write = false;
            self.toast("staying: not quitting after the write", Level::Info);
        } else if self.view == View::Library && self.deck_layout {
            // Browse closes first; then player mode, back to the tab it was opened from.
            self.deck_layout = std::mem::take(&mut self.browsing);
            if !self.deck_layout {
                self.leave_player_mode();
            }
        } else if self.deck.pads {
            // The pads take the keys over: leaving them comes before anything else goes.
            crate::tui::deck::toggle_pads(self);
        } else if self.job.as_ref().is_some_and(|j| j.committed) {
            // Too late to stop: it says so.
            self.cancel_job();
        } else if let Some(j) = &self.job {
            // Work stopped by a stray esc can take long to do again: asked first.
            let question = format!("{} is under way: stop it?", j.label);
            self.modal = Some(Modal::Ask(question, Box::new(|app: &mut App| app.cancel_job())));
        } else if self.dismiss_errors() {
            // The error was put away: that's all this esc does.
        } else if let Some(o) = self.replay_offer.take() {
            self.replay_dismissed.insert(o.root);
            self.toast("not replaying: : and replay does it later", Level::Info);
        } else if !self.query.is_empty() {
            self.query.clear();
            self.refresh_visible();
        } else if self.view == View::Library && self.focus == Focus::Details {
            self.toggle_details();
        } else if self.view == View::Library && self.focus == Focus::Crate {
            self.toggle_queue_panel();
        } else if self.view == View::Drives && matches!(self.drive_pane, DrivePane::Snapshots | DrivePane::History) {
            self.show_drive_pane(DrivePane::List);
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
        } else {
            return false;
        }
        true
    }

    /// Esc with nothing left to close: the first says a second quits, and a second soon after does.
    fn arm_quit(&mut self) {
        if self.quit_armed.take().is_some_and(|at| at.elapsed() < QUIT_AGAIN) {
            self.ask_to_quit();
        } else {
            self.quit_armed = Some(Instant::now());
        }
    }

    /// Whether esc now would quit: what the footer says while it's so.
    pub fn quit_waiting(&self) -> bool {
        self.quit_armed.is_some_and(|at| at.elapsed() < QUIT_AGAIN)
    }

    /// `t`: show or hide the queue. Over player mode it opens with Browse, and from another
    /// view in the library, with focus.
    fn toggle_queue_panel(&mut self) {
        let away = self.view != View::Library || self.deck_showing();
        if self.deck_showing() {
            self.browsing = true;
        } else if self.view != View::Library {
            self.set_view(View::Library);
        }
        self.show_crate = away || !self.show_crate;
        if away {
            self.focus = Focus::Crate;
        } else if !self.show_crate && self.focus == Focus::Crate {
            self.focus = Focus::Tracks;
        }
        if self.crate_state.selected().is_none() && !self.crate_list.is_empty() {
            self.crate_state.select(Some(0));
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

    /// `f`: player mode from any tab, the playing track on the whole screen. Opened from another
    /// tab, closing it goes back there; in the Library view it switches the layouts.
    fn toggle_player_mode(&mut self) {
        self.browsing = false;
        if self.view != View::Library {
            let from = self.view;
            self.set_view(View::Library);
            self.player_from = Some(from);
            self.deck_layout = true;
        } else if self.deck_layout {
            self.deck_layout = false;
            self.leave_player_mode();
        } else {
            self.deck_layout = true;
        }
    }

    /// Player mode closed: back to the tab it was opened from, if it was.
    fn leave_player_mode(&mut self) {
        if let Some(v) = self.player_from.take() {
            self.set_view(v);
        }
    }

    pub(in crate::tui) fn set_view(&mut self, v: View) {
        // A tab picked some other way is where the user is now.
        self.player_from = None;
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
        // An error is about what was on screen when it came: leaving the view puts it away, as
        // esc does (it's still in the messages, in `:`).
        if v != self.view {
            self.dismiss_errors();
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
            (View::Library, _) if self.deck_showing() && self.picked_tracks().is_some() => self.queue_picked(true),
            (View::Library, _) if self.deck_showing() => {}
            (View::Library, Focus::Tracks) => {
                self.play_selected();
                // As a CDJ does: a track loaded, the deck's back.
                self.browsing = false;
            }
            (View::Library, Focus::Playlists) => self.open_playlist(),
            (View::Library, Focus::Details) => self.play_match(),
            (View::Library, Focus::Crate) => self.play_pick(),
            (View::Prepare, _) => crate::tui::prepare::run(self),
            (View::Drives, _) if self.drive_pane == DrivePane::Snapshots => self.restore_from_selected_snapshot(),
            (View::Drives, _) if self.drive_pane == DrivePane::History => self.open_played(),
            (View::Drives, _) if self.drive_header.is_some() => self.activate_header(),
            (View::Library, Focus::Drives) if self.collection_row && self.collection_db.is_some() => {
                self.load_collection();
                self.focus = Focus::Playlists;
            }
            (View::Library, Focus::Drives) | (View::Drives, _) => {
                if let Some(d) = self.drive_state.selected().and_then(|i| self.drives.get(i)) {
                    if !d.has_export {
                        self.toast("no rekordbox export on this drive", Level::Info);
                        return;
                    }
                    let root = d.root.clone();
                    // Before loading, which says why when it can't.
                    self.set_view(View::Library);
                    self.load_library(root);
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
            (View::Library, Focus::Tracks) if self.empty() => self.next_steps().len(),
            (View::Library, Focus::Tracks) => self.visible.len(),
            (View::Library, Focus::Playlists) => self.loaded.as_ref().map_or(0, |l| l.tree.len()),
            (View::Library, Focus::Details) => {
                self.details.as_ref().filter(|_| self.details_current()).map_or(0, |d| d.matches.items.len())
            }
            (View::Library, Focus::Crate) => self.crate_list.len(),
            (View::Prepare, _) => crate::tui::prepare::len(),
            (View::Drives, _) if self.drive_pane == DrivePane::Snapshots => self.history_rows(),
            (View::Drives, _) if self.drive_pane == DrivePane::History => self.played_lists().len(),
            (View::Library, Focus::Drives) => self.source_rows(),
            (View::Drives, _) => self.drives.len(),
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
            (View::Library, Focus::Tracks) if self.empty() => self.steps.selected_mut(),
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
        if self.view == View::Library && self.focus == Focus::Drives && !self.deck_showing() {
            return self.move_source_row(delta);
        }
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

/// Where a dialog that scrolls is after `act`: a row, a page or an end.
pub(in crate::tui) fn scrolled(at: u16, act: Act) -> u16 {
    match act {
        Act::Move(d) => at.saturating_add_signed(d.into()),
        Act::Page(d) => at.saturating_add_signed(10 * i16::from(d)),
        Act::End(d) if d < 0 => 0,
        Act::End(_) => u16::MAX,
        _ => at,
    }
}

/// Move the selection of a dialog's list of `n` by `act` (a row, a page, an end), stopping at its ends.
pub(in crate::tui) fn step_list(state: &mut ratatui::widgets::ListState, n: usize, act: Act) {
    if n == 0 {
        return;
    }
    let at = state.selected().unwrap_or(0).min(u16::MAX.into()) as u16;
    state.select(Some(usize::from(scrolled(at, act)).min(n - 1)));
}

#[cfg(test)]
mod tests;
