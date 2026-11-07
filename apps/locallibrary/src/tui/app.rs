//! TUI state and input handling. Drawing lives in `ui.rs`, the sidebar tree and sorting in
//! `browse.rs`, the crate in `crate_list.rs`, and changing the drive in `edits.rs`.

mod background;
mod chrome;
mod drives;
mod input;
mod library;
mod loaded;
mod mouse;
mod panes;
mod player;
mod playing;
mod saved;

pub(super) use background::*;
pub(super) use chrome::*;
pub(super) use drives::*;
pub(super) use input::*;
use library::*;
pub(super) use loaded::*;
pub(super) use mouse::*;
pub(super) use playing::*;

use rustc_hash::{FxHashMap, FxHashSet};
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::{Duration, Instant};

use locallibrary::query::{FieldName, SortKey};
use locallibrary::{DeviceInfo, api};
use ratatui::widgets::{ListState, TableState};
use uuid::Uuid;

use super::analyzer::Analyzer;
use super::audio::{self, Audio, Outputs};
use super::browse::SavedNode;
use super::crate_list::Crate;
use super::drives::{DriveKey, DriveRow};
use super::edits::{Job, Modal, Pick};
use super::keymap;
use super::state::{self, Saved, SavedQueue};
use super::terminal::LateAnswers;
// What the test modules find through `use super::*`.
#[cfg(test)]
use {
    super::browse::{self, Category, NodeId},
    super::edits::Action,
    super::keymap::Act,
    super::keys::{Camelot, Relation},
    locallibrary::anlz::{self, PhraseKind, TrackAnalysis},
    locallibrary::fields::TrackValues,
    locallibrary::query::{Filter, SortField},
    locallibrary::store::{LibraryRecord, SnapshotInfo},
    locallibrary::watch::WatchEvent,
    locallibrary::{Ctx, Device, Error, Library, Store, Track, paths},
    loclib_waveform::Art,
    ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEventKind},
    ratatui::layout::Rect,
    std::path::Path,
    std::sync::{Arc, Mutex},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    Library,
    Prepare,
    Drives,
    Mixes,
    Profile,
}

impl View {
    pub const ALL: [View; 5] = [View::Library, View::Prepare, View::Drives, View::Mixes, View::Profile];

    pub fn title(self) -> &'static str {
        match self {
            View::Library => "Library",
            View::Prepare => "Prepare",
            View::Drives => "Drives",
            View::Mixes => "Recordings",
            View::Profile => "Publish",
        }
    }

    /// The view a saved title names, as it was called in the run that saved it: the store's
    /// snapshots had a view of their own, first as `History`, and these two other names.
    fn from_title(s: &str) -> Option<View> {
        let s = match s {
            "History" | "Snapshots" => "Drives",
            "Mixes" => "Recordings",
            "Profile" => "Publish",
            s => s,
        };
        View::ALL.into_iter().find(|v| v.title() == s)
    }
}

/// The Drives view's panes: its list, and what the pane beside it can show.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DrivePane {
    List,
    /// The selected drive, or a library's clones.
    Info,
    Snapshots,
    /// What players recorded on the drive open in the library.
    History,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    Drives,
    Playlists,
    Tracks,
    /// The selected track's details panel (its matches list).
    Details,
    /// The crate panel (a CDJ's TAG LIST).
    Crate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Vis {
    Bars,
    Mirror,
    Scope,
    Deck,
}

impl Vis {
    pub fn next(self) -> Vis {
        match self {
            Vis::Bars => Vis::Mirror,
            Vis::Mirror => Vis::Scope,
            Vis::Scope => Vis::Deck,
            Vis::Deck => Vis::Bars,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Vis::Bars => "spectrum",
            Vis::Mirror => "mirror",
            Vis::Scope => "scope",
            Vis::Deck => "deck",
        }
    }

    fn from_name(s: &str) -> Option<Vis> {
        [Vis::Bars, Vis::Mirror, Vis::Scope, Vis::Deck].into_iter().find(|v| v.name() == s)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Repeat {
    Off,
    All,
    One,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Info,
    Error,
}

pub struct App {
    pub store_root: PathBuf,
    pub view: View,
    pub focus: Focus,
    pub drives: Vec<DeviceInfo>,
    pub drive_state: ListState,
    pub loaded: Option<Loaded>,
    pub loading: Option<String>,
    /// Numbers library reads: only the latest one asked for is shown, however they finish.
    load: u64,
    pub playlist_state: ListState,
    pub track_state: TableState,
    pub visible: Vec<usize>,
    pub searching: bool,
    pub query: String,
    /// Search the whole drive rather than the list shown.
    pub search_all: bool,
    /// Why the filter typed in the search box doesn't read.
    pub filter_error: Option<String>,
    /// What the filter is waiting for, or did without (My Tags being read, or unreadable).
    pub filter_note: Option<String>,
    /// On a big library, the search box narrows the list once typing pauses, at this time.
    search_due: Option<Instant>,
    /// The track list's order; the list's own when `None`.
    pub sort: Option<SortKey>,
    /// Marked tracks on the open drive, by file path.
    pub marks: FxHashSet<String>,
    /// Playlists and folders marked in the tree (to pack), by path, in the order marked.
    pub list_marks: Vec<String>,
    /// Replaying edits rekordbox exported over, offered.
    pub replay_offer: Option<ReplayOffer>,
    /// Drives whose offer to replay was put away this run.
    replay_dismissed: FxHashSet<PathBuf>,
    /// The library load whose edits were last looked at for replaying.
    replay_checked: u64,
    pub crate_list: Crate,
    pub crate_state: ListState,
    pub show_details: bool,
    pub show_crate: bool,
    /// The track list's rating column (hidden unless asked for, or sorted by).
    pub show_ratings: bool,
    /// The track list's My Tags column.
    pub show_tags_column: bool,
    /// The track list's columns of plugins' fields (`sound.bpm`), in the order shown.
    pub field_columns: Vec<FieldName>,
    pub details: Option<Details>,
    pub details_state: ListState,
    /// The selected track's panels, plugins' (see `plugins::panels`).
    pub panels: super::plugins::panels::Panels,
    /// The selected track and since when, to look up its details once the selection settles.
    details_seen: Option<(u64, usize, Instant)>,
    details_asked: Option<(u64, usize)>,
    /// A details lookup that failed: not asked again while that track stays selected (one that
    /// panics on a damaged row would every tick).
    details_failed: Option<(u64, usize)>,
    pub modal: Option<Modal>,
    /// Dialogs from the background (a plan, a task's results) waiting for the one open to close.
    pub(super) waiting: VecDeque<Modal>,
    pub picking: Option<Pick>,
    /// The fixes the last check gave (plugins' and locallibrary's own), offered in the palette.
    pub fixes: Vec<super::plugins::checks::Fixing>,
    /// The keys the user bound to plugins' commands and operations (see `keymap::bound`).
    pub plugin_keys: Vec<keymap::bound::Bound>,
    pub job: Option<Job>,
    /// The drive the last change that made a backup was made to: `u` takes it back.
    pub(super) last_backup: Option<DriveKey>,
    /// Every mounted drive's status, role, logbook and copies (the Drives view).
    pub status: Option<Result<Vec<api::FleetDrive>, String>>,
    pub status_loading: bool,
    /// The mounted drives' libraries, each with its clones (those not plugged in too): read with
    /// `status`.
    pub fleet_libraries: Vec<api::FleetLibrary>,
    /// The store's libraries' names by id, read with the drive list (a small file) so the Drives
    /// view groups the drives under them at once.
    pub library_names: FxHashMap<Uuid, String>,
    /// The Drives view's row of a library (or of the drives of none) when that's what's selected
    /// rather than a drive.
    pub drive_header: Option<DriveRow>,
    /// The Drives view's rows, scrolled.
    pub drive_rows: ListState,
    /// The Drives view's pane with focus.
    pub drive_pane: DrivePane,
    /// What's beside the Drives view's list: never `DrivePane::List`.
    pub drive_tab: DrivePane,
    /// The Drives view's details, scrolled.
    pub drive_scroll: u16,
    /// The Drives view's list of what players recorded.
    pub played: ListState,
    /// The Prepare view's list.
    pub prepare: ListState,
    pub history: Option<Result<History, String>>,
    pub history_state: TableState,
    pub profile: super::profile::ProfileTab,
    pub mix: super::mix::MixTab,
    pub audio: Option<Audio>,
    pub audio_error: Option<String>,
    /// Where it plays: the machine's outputs, or none (see `Outputs`).
    pub(super) outputs: Outputs,
    /// How loud it plays, kept while no output is open.
    pub volume: f32,
    pub now: Option<NowPlaying>,
    /// Opens the tracks played.
    opener: Opener,
    /// The next track, opened ahead of the playing one's end.
    upcoming: Option<Upcoming>,
    /// The playing track's file (for the play with this id), opened before the output it plays
    /// through: it waits for the output (see `output_ready`).
    waiting_file: Option<(u64, audio::OpenedFile)>,
    pub queue: Queue,
    pub shuffle: bool,
    pub repeat: Repeat,
    pub analyzer: Analyzer,
    /// Hot cue pads and the rest of what the deck does (see `deck.rs`).
    pub deck: super::deck::Deck,
    pub vis: Vis,
    pub deck_secs: f64,
    /// Colour of the cue the playhead last crossed, and when.
    flash: Option<([u8; 3], Instant)>,
    /// `g` was pressed at this time and waits for what follows (see `keymap`).
    pub chord: Option<Instant>,
    /// The deck strip's waveform, kept between frames.
    pub strip: super::ui::StripWave,
    /// Keeps the terminal's late answers out of the keys.
    late_answers: LateAnswers,
    last_pos_ms: f64,
    pub fullscreen: bool,
    /// Full screen: the pick among the tracks that mix into the playing one, to play next.
    pub next_pick: ListState,
    pub help: bool,
    pub help_scroll: u16,
    /// The latest message, in the footer: an info for a few seconds (see `reading_time`), an
    /// error until esc dismisses it.
    pub toast: Option<(String, Level, Instant)>,
    /// An error a newer message came after before it was dismissed: still shown, above it.
    pub held: Option<String>,
    /// Every message since starting, oldest first (the last `LOG`), shown by `!`.
    pub log: VecDeque<(String, Level, Instant)>,
    pub show_log: bool,
    pub log_scroll: u16,
    /// When esc was pressed with marks to clear: a second esc soon after clears them.
    clear_marks_armed: Option<Instant>,
    /// The queue's track was this far in when the last run ended: space carries on from there.
    pub resume: Option<Duration>,
    /// The queue the last run had, until its drive's library is read.
    restore_queue: Option<SavedQueue>,
    /// Where to seek to once the track being opened is playing.
    pub(super) pending_seek: Option<Duration>,
    /// The track being opened is to wait, paused, once it's open.
    start_paused: bool,
    /// Tracks in a row the queue moved past because they wouldn't play.
    failed_in_row: usize,
    /// What was saved last, and when it was last checked for changes.
    saved: Option<Saved>,
    last_save_check: Instant,
    pub areas: Areas,
    pub started: Instant,
    pub quit: bool,
    /// Asked to quit while the drive was being written: quit once the write is done.
    pub quit_after_write: bool,
    /// Said on the terminal after the screen is put back (how a write that ended the run went).
    pub exit_message: Option<String>,
    /// Drives opened by path that aren't mounted volumes (a folder holding an export).
    pub(super) extra_roots: Vec<PathBuf>,
    /// Look at no drives but those it's given: the mounted ones aren't listed, nor their status
    /// read (tests, which mustn't touch the machine's drives).
    offline: bool,
    /// Whether the drive list has been read: the first read announces nothing.
    listed: bool,
    /// Notices drives coming and going: a poll that finds any reads the list again.
    watcher: Watcher,
    /// What plugins suggested, to review from the palette, oldest first, as the store keeps it
    /// (see [`locallibrary::plugin::keep_suggested`]).
    pub(super) suggested: Vec<locallibrary::plugin::PluginNotice>,
    /// The marks of suggestions reviewed or dismissed that the store may still keep: what's kept
    /// is told the player after each change, and one told before the store forgot them leaves
    /// them out.
    pub(super) forgetting: Vec<u64>,
    drives_in_flight: bool,
    /// Read the drives again when the read in flight finishes (asked for while it ran).
    drives_again: bool,
    /// A poll of the volumes is under way (a mount whose `stat` hangs mustn't pile them up).
    volumes_in_flight: bool,
    /// The last poll of the volumes failed: the next failure in a row isn't said again.
    volumes_failing: bool,
    last_volume_poll: Instant,
    last_tick: Instant,
    last_seek: Instant,
    /// When a row was last clicked, and which. Only a click on that same row soon after is a
    /// second click.
    last_click: Option<(Instant, ClickedRow)>,
    rng: u64,
    next_id: u64,
    /// The drive the last run had open: opened first when it's plugged in.
    preferred_root: Option<PathBuf>,
    /// The list the last run showed on that drive.
    restore: Option<(PathBuf, SavedNode)>,
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
}

impl App {
    pub fn new(store_root: PathBuf, drive: Option<String>) -> App {
        let saved = state::load(&store_root);
        let mut app = App::blank(store_root, None);
        app.offline = false;
        app.outputs = Outputs::from_env();
        app.apply_saved(&saved, drive.is_none());
        // Who's signed in, for the status line: a small file.
        app.profile.account.load(&app.store_root);
        // The output picked last time, else (unplugged, say) the default.
        app.open_output();
        if let Some(v) = saved.view.as_deref().and_then(View::from_title) {
            app.set_view(v);
        }
        app.saved = Some(saved);
        if let Some(spec) = drive {
            // A path or name given on the command line: open it straight away.
            match api::resolve_drive(&spec) {
                Ok(d) => {
                    let root = d.root().to_path_buf();
                    if !api::drives_mounted().iter().any(|m| m.root == root) {
                        app.extra_roots.push(root.clone());
                    }
                    app.load_library(root);
                }
                Err(e) => app.toast(e.to_string(), Level::Error),
            }
        }
        app.refresh_drives();
        app
    }

    /// An app with nothing open, no background work started and no output devices (see
    /// `Outputs`): what the tests use.
    pub(super) fn blank(store_root: PathBuf, audio: Option<Audio>) -> App {
        // A test's app never sends the browser of whoever runs it anywhere, or reads their drives.
        #[cfg(all(test, debug_assertions))]
        locallibrary::opener::never_open();
        #[cfg(test)]
        testing::no_drives();
        let (tx, rx) = channel();
        let seed =
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(1, |d| d.as_nanos() as u64);
        App {
            store_root,
            view: View::Library,
            focus: Focus::Tracks,
            drives: Vec::new(),
            drive_state: ListState::default(),
            loaded: None,
            loading: None,
            load: 0,
            playlist_state: ListState::default().with_selected(Some(0)),
            track_state: TableState::default(),
            visible: Vec::new(),
            searching: false,
            query: String::new(),
            search_all: false,
            filter_error: None,
            filter_note: None,
            search_due: None,
            sort: None,
            marks: FxHashSet::default(),
            list_marks: Vec::new(),
            replay_offer: None,
            replay_dismissed: FxHashSet::default(),
            replay_checked: 0,
            crate_list: Crate::default(),
            crate_state: ListState::default(),
            show_details: false,
            show_crate: false,
            show_ratings: false,
            show_tags_column: false,
            field_columns: Vec::new(),
            details: None,
            details_state: ListState::default(),
            panels: Default::default(),
            details_seen: None,
            details_asked: None,
            details_failed: None,
            modal: None,
            waiting: VecDeque::new(),
            picking: None,
            fixes: Vec::new(),
            plugin_keys: Vec::new(),
            job: None,
            last_backup: None,
            status: None,
            status_loading: false,
            fleet_libraries: Vec::new(),
            library_names: FxHashMap::default(),
            drive_header: None,
            drive_rows: ListState::default(),
            drive_pane: DrivePane::List,
            drive_tab: DrivePane::Info,
            drive_scroll: 0,
            played: ListState::default().with_selected(Some(0)),
            prepare: ListState::default().with_selected(Some(0)),
            history: None,
            history_state: TableState::default(),
            profile: super::profile::ProfileTab::new(tx.clone()),
            mix: super::mix::MixTab::new(tx.clone()),
            volume: audio.as_ref().map_or(0.8, Audio::volume),
            audio,
            audio_error: None,
            outputs: Outputs::NONE,
            now: None,
            opener: Opener::default(),
            upcoming: None,
            waiting_file: None,
            queue: Queue::default(),
            shuffle: false,
            repeat: Repeat::Off,
            analyzer: Analyzer::default(),
            deck: Default::default(),
            vis: Vis::Bars,
            deck_secs: 8.0,
            flash: None,
            chord: None,
            strip: Default::default(),
            late_answers: LateAnswers::default(),
            last_pos_ms: 0.0,
            fullscreen: false,
            next_pick: ListState::default(),
            help: false,
            help_scroll: 0,
            toast: None,
            held: None,
            log: VecDeque::new(),
            show_log: false,
            log_scroll: 0,
            clear_marks_armed: None,
            resume: None,
            restore_queue: None,
            pending_seek: None,
            start_paused: false,
            failed_in_row: 0,
            saved: None,
            last_save_check: Instant::now(),
            areas: Areas::default(),
            started: Instant::now(),
            quit: false,
            quit_after_write: false,
            exit_message: None,
            extra_roots: Vec::new(),
            offline: true,
            listed: false,
            watcher: Watcher::default(),
            suggested: Vec::new(),
            forgetting: Vec::new(),
            drives_in_flight: false,
            drives_again: false,
            volumes_in_flight: false,
            volumes_failing: false,
            last_volume_poll: Instant::now(),
            last_tick: Instant::now(),
            last_seek: Instant::now(),
            last_click: None,
            rng: seed | 1,
            next_id: 1,
            preferred_root: None,
            restore: None,
            tx,
            rx,
        }
    }

    /// For tests: an app showing `lib` as the drive at `root` (any folder), with no audio and
    /// no background polling.
    #[cfg(test)]
    pub(super) fn with_library(root: &Path, lib: Library) -> App {
        let mut app = App::blank(root.join("store"), None);
        // The volumes are polled only when a test asks: a poll (once a second of the clock, as
        // the background is waited for) takes a folder with no export for an unplugged drive, so
        // a test slowed past a second by a loaded machine would find its drive gone.
        app.last_volume_poll = testing::just_now();
        let device = Device::open(root).unwrap();
        let root = device.root().to_path_buf();
        app.install(root, Loaded::prepare(device, lib));
        app
    }
}

#[cfg(test)]
mod props;

#[cfg(test)]
pub(super) mod testing;

#[cfg(test)]
mod tests;
