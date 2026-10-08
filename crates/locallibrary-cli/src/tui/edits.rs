//! Changing the drive from the player, always opt-in: an actions menu (`e`, or right-click) for
//! the marked or selected tracks, the playlist in view and the crate; the prompts and playlist
//! picking some actions need; and the job every write goes through: plan it in the background,
//! show the plan and wait for a yes, apply exactly that plan (the library backs the drive up
//! first, and refuses if the drive changed since), then read the drive again, keeping the view.
//! Any kind of plan goes through the same dialog ([`AnyPlanned`]); playlist and track edits show
//! in detail.

use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::time::{Duration, Instant};

use locallibrary::api::{self, EditRequest};
use locallibrary::edit::{Edit, EditOptions, EditPlan, TrackFields};
use locallibrary::model::TrackColor;
use locallibrary::op::{AnyPlanned, Planned};
use locallibrary::progress::{Event, Phase, Progress};
use locallibrary::query::{SortKey, TrackSelector};
use locallibrary::{CancelToken, Ctx, Error};
use ratatui::crossterm::event::{KeyCode, KeyEvent};
use ratatui::widgets::ListState;

use super::app::{App, Focus, Level, Msg, View, edit_line};
use super::browse::{COLOURS, NodeId, colour_name, sort_label, sort_spec};
use super::drives::{After, Report};
use super::keymap::{self, Act};

// ---------------------------------------------------------------------------------------------
// Edits
// ---------------------------------------------------------------------------------------------

/// Tracks by their audio files, which stay put when the drive is read again (ids don't).
fn selector(paths: &[String]) -> TrackSelector {
    TrackSelector::paths(paths.iter().cloned())
}

pub fn set_rating(paths: &[String], stars: u8) -> Edit {
    Edit::EditTracks { tracks: selector(paths), fields: TrackFields { rating: Some(stars), ..Default::default() } }
}

pub fn set_colour(paths: &[String], colour: Option<TrackColor>) -> Edit {
    let name = colour.map_or("none", colour_name).to_string();
    Edit::EditTracks { tracks: selector(paths), fields: TrackFields { color: Some(name), ..Default::default() } }
}

pub fn add_to(playlist: &str, paths: &[String]) -> Edit {
    Edit::AddTracks {
        playlist: playlist.to_string(),
        tracks: selector(paths),
        position: None,
        sort: None,
        allow_repeats: false,
    }
}

pub fn remove_from(playlist: &str, paths: &[String]) -> Edit {
    Edit::RemoveTracks { playlist: playlist.to_string(), tracks: Some(selector(paths)), positions: Vec::new() }
}

/// A new playlist (or folder) at `path`, filled with `with`.
pub fn create(path: &str, folder: bool, with: &[String]) -> Vec<Edit> {
    let mut edits = vec![Edit::CreatePlaylist { path: path.to_string(), folder }];
    if !with.is_empty() {
        edits.push(add_to(path, with));
    }
    edits
}

pub fn rename(playlist: &str, name: &str) -> Edit {
    Edit::RenamePlaylist { playlist: playlist.to_string(), name: name.to_string() }
}

pub fn delete(playlist: &str) -> Edit {
    Edit::DeletePlaylist { playlist: playlist.to_string(), with_tracks: false }
}

pub fn sort(playlist: &str, by: SortKey) -> Edit {
    Edit::SortPlaylist { playlist: playlist.to_string(), by: sort_spec(by) }
}

/// A playlist path as typed (`Sets/2026/Friday`): trimmed, with no empty parts.
pub fn clean_path(s: &str) -> Option<String> {
    let parts: Vec<&str> = s.trim().trim_start_matches('/').split('/').map(str::trim).collect();
    parts.iter().all(|p| !p.is_empty()).then(|| parts.join("/"))
}

/// The last part of a playlist path, for short labels.
fn leaf(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

// ---------------------------------------------------------------------------------------------
// The actions menu
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Action {
    CrateAdd,
    RatingMenu,
    Rate(u8),
    ColourMenu,
    Colour(Option<TrackColor>),
    AddToPlaylist,
    NewPlaylistWith,
    RemoveFromList,
    SortList,
    NewPlaylist,
    NewFolder,
    Rename,
    Delete,
    SaveCrate,
    ClearCrate,
    /// Replay the edits rekordbox exported over.
    Replay,
    /// Bring back a playlist from a snapshot.
    Restore,
    /// Give the tracks a My Tag, or take one off.
    Tag,
}

/// What an action works on, captured when the menu opens so moving about afterwards (picking a
/// playlist, say) doesn't change it.
#[derive(Debug, Clone, Default)]
pub struct Target {
    pub root: PathBuf,
    /// Device paths of the tracks: the marked ones, else the selected one, in list order.
    pub tracks: Vec<String>,
    /// "3 marked tracks", or the track's name.
    pub what: String,
    /// The playlist shown in the track list, by path.
    pub list: Option<String>,
    /// The playlist or folder selected in the tree (the one shown, from elsewhere), by path.
    pub node: Option<(String, bool)>,
    /// The folder new playlists go in, by path ("" for the top level).
    pub folder: String,
    pub sort: Option<SortKey>,
    /// The crate's tracks on this drive, for saving it as a playlist.
    pub crated: Vec<String>,
    /// How many tracks the crate holds in all.
    pub crate_total: usize,
    /// The drive was unplugged.
    pub stale: bool,
}

pub struct MenuItem {
    pub key: &'static str,
    pub label: String,
    pub action: Action,
    /// Why it can't be done here, if it can't.
    pub unavailable: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Sub {
    Rating,
    Colour,
}

pub struct Menu {
    pub items: Vec<MenuItem>,
    pub state: ListState,
    pub target: Target,
    /// Picking a rating or colour.
    pub sub: Option<Sub>,
    pub sub_state: ListState,
    /// Opened with the mouse here, rather than in the middle.
    pub at: Option<(u16, u16)>,
}

impl Menu {
    pub fn new(target: Target, at: Option<(u16, u16)>) -> Menu {
        let items = menu_items(&target);
        let first = items.iter().position(|i| i.unavailable.is_none()).unwrap_or(0);
        Menu {
            items,
            state: ListState::default().with_selected(Some(first)),
            target,
            sub: None,
            sub_state: ListState::default(),
            at,
        }
    }

    /// The choices of the rating or colour picker.
    pub fn sub_items(sub: Sub) -> Vec<Action> {
        match sub {
            Sub::Rating => (0..=5).rev().map(Action::Rate).collect(),
            Sub::Colour => std::iter::once(None).chain(COLOURS.map(Some)).map(Action::Colour).collect(),
        }
    }
}

pub fn menu_items(t: &Target) -> Vec<MenuItem> {
    let item = |key, label: String, action, unavailable: Option<String>| MenuItem { key, label, action, unavailable };
    let because = |cond: bool, why: &str| cond.then(|| why.to_string());
    let stale = because(t.stale, "the drive was unplugged");
    let no_tracks = because(t.tracks.is_empty(), "no track selected");
    let tracks = stale.clone().or(no_tracks.clone());
    let list = t.list.as_deref();
    let not_list = because(list.is_none(), "the list shown isn't a playlist");
    let node_name =
        t.node.as_ref().map(|(p, folder)| format!("{} {}", if *folder { "folder" } else { "playlist" }, leaf(p)));
    let no_node = because(t.node.is_none(), "select a playlist or folder");
    let into = if t.folder.is_empty() { String::new() } else { format!(" in {}", leaf(&t.folder)) };
    vec![
        item("a", "add to the crate".into(), Action::CrateAdd, no_tracks),
        item("0-5", "set the rating…".into(), Action::RatingMenu, tracks.clone()),
        item("c", "set the colour…".into(), Action::ColourMenu, tracks.clone()),
        item("T", "give them a My Tag (or take one off)…".into(), Action::Tag, tracks.clone()),
        item("p", "add to a playlist…".into(), Action::AddToPlaylist, tracks.clone()),
        item("n", "new playlist with them…".into(), Action::NewPlaylistWith, tracks.clone()),
        item(
            "x",
            format!("remove from {}", list.map_or("this playlist", leaf)),
            Action::RemoveFromList,
            tracks.or(not_list.clone()),
        ),
        item(
            "s",
            match (list, t.sort) {
                (Some(p), Some(k)) => {
                    format!(
                        "sort {} by {} {} on the drive",
                        leaf(p),
                        sort_label(k.field),
                        if k.descending { "↓" } else { "↑" }
                    )
                }
                _ => "sort this playlist on the drive".into(),
            },
            Action::SortList,
            stale.clone().or(not_list).or(because(t.sort.is_none(), "sort the list first (b, or click a column)")),
        ),
        item("N", format!("new playlist{into}…"), Action::NewPlaylist, stale.clone()),
        item("F", format!("new folder{into}…"), Action::NewFolder, stale.clone()),
        item(
            "r",
            format!("rename {}…", node_name.as_deref().unwrap_or("playlist")),
            Action::Rename,
            stale.clone().or(no_node.clone()),
        ),
        item(
            "d",
            format!("delete {}…", node_name.as_deref().unwrap_or("playlist")),
            Action::Delete,
            stale.clone().or(no_node),
        ),
        item(
            "w",
            "save the crate as a playlist…".into(),
            Action::SaveCrate,
            stale.clone().or(because(t.crated.is_empty(), "the crate has nothing from this drive")),
        ),
        item("X", "clear the crate".into(), Action::ClearCrate, because(t.crate_total == 0, "the crate is empty")),
        item(
            "b",
            format!("bring back {} from a snapshot…", node_name.as_deref().unwrap_or("a playlist")),
            Action::Restore,
            stale.clone(),
        ),
        item("y", keymap::name(Act::Replay).into(), Action::Replay, stale),
    ]
}

// ---------------------------------------------------------------------------------------------
// Prompts, confirmations, picking
// ---------------------------------------------------------------------------------------------

pub enum PromptFor {
    NewPlaylist {
        with: Vec<String>,
    },
    NewFolder,
    Rename {
        playlist: String,
    },
    SaveCrate {
        tracks: Vec<String>,
    },
    /// The crate's tracks, saved as the playlist typed and then packed onto `target`.
    CrateForPack {
        tracks: Vec<String>,
        target: PathBuf,
    },
    /// The playlist or folder to bring back (the snapshot is asked next).
    RestorePlaylist,
    /// The playlist or folder to bring back from this snapshot.
    RestoreFrom {
        snapshot: String,
    },
    /// A new My Tag (`Category/Tag`) for these tracks.
    NewTag {
        tracks: Vec<String>,
    },
    /// Where to write the list shown.
    ExportList {
        list: super::drives::ExportOf,
    },
    /// Where to write rekordbox XML (of these playlists, all when empty).
    ExportXml {
        only: Vec<String>,
    },
    /// Where the playlist a rule fills goes.
    SmartRule {
        filter: String,
        sort: Option<String>,
    },
}

/// A line of text typed in the footer, like the search box.
pub struct Prompt {
    pub label: String,
    pub text: String,
    pub purpose: PromptFor,
    pub root: PathBuf,
}

/// A plan waiting for a yes.
pub struct Confirm {
    pub root: PathBuf,
    pub drive: String,
    pub plan: Box<dyn AnyPlanned>,
    pub scroll: u16,
    /// When it appeared: the plan arrives in the background, so keys pressed just then (enter to
    /// play a track, a y typed in the search box) weren't meant for it.
    pub shown: Instant,
    /// What comes next once it's made.
    pub after: Option<After>,
}

impl Confirm {
    /// The plan, when it's a playlist and track edit (shown in detail).
    pub fn edit(&self) -> Option<&EditPlan> {
        self.plan.as_any().downcast_ref::<Planned<EditPlan>>().map(|p| &p.plan)
    }

    /// The plan, when it's undoing (shown in detail).
    pub fn undo(&self) -> Option<&api::UndoPlan> {
        self.plan.as_any().downcast_ref::<Planned<api::UndoPlan, bool>>().map(|p| &p.plan)
    }
}

/// What applying a plan came to.
pub struct Applied {
    /// The plan's operation: `edit`, `pack`...
    pub operation: &'static str,
    /// Its report, as `*.apply` answers over RPC.
    pub report: serde_json::Value,
}

/// How long a confirmation ignores keys after appearing.
const SETTLE: Duration = Duration::from_millis(400);

pub enum Modal {
    Menu(Menu),
    Prompt(Prompt),
    Confirm(Confirm),
    /// A yes/no question about something that doesn't touch the drive, and what a yes does.
    Ask(String, Box<dyn FnOnce(&mut App)>),
    /// A list to choose from by typing: the command palette, or an answer an action needs.
    Pick(super::palette::Picker),
    /// Results to read (a check's).
    Report(Report),
}

/// Choosing the playlist to add tracks to, in the sidebar tree.
pub struct Pick {
    pub target: Target,
    /// Where focus goes back to afterwards.
    pub focus: Focus,
}

// ---------------------------------------------------------------------------------------------
// Jobs
// ---------------------------------------------------------------------------------------------

/// Planning or applying an edit, in the background.
pub struct Job {
    pub id: u64,
    pub label: String,
    pub detail: String,
    pub cancel: CancelToken,
    /// Applying (rather than planning).
    pub writing: bool,
    /// The databases are being written: too late to stop.
    pub committed: bool,
    pub started: Instant,
    /// The drives a write changes, read again once it's done.
    pub touched: Vec<PathBuf>,
    /// What comes next once the change is made.
    pub after: Option<After>,
}

/// What a background task hands back, to finish on the UI thread.
pub type Done = Box<dyn FnOnce(&mut App) + Send>;

/// Turns the library's progress events into a line for the header, and passes a cancel on to the
/// job only until it starts writing, so a cancelled edit never stops halfway.
struct Reporter {
    tx: Sender<Msg>,
    job: u64,
    outer: CancelToken,
    inner: CancelToken,
    committed: AtomicBool,
    /// The phase under way, and when the header was last told.
    state: Mutex<(String, Option<Instant>)>,
}

impl Reporter {
    fn new(tx: Sender<Msg>, job: u64, outer: CancelToken, inner: CancelToken) -> Reporter {
        Reporter { tx, job, outer, inner, committed: AtomicBool::new(false), state: Mutex::new((String::new(), None)) }
    }
}

fn phase_name(p: Phase) -> &'static str {
    match p {
        Phase::Scan => "scanning",
        Phase::Hash => "hashing",
        Phase::Copy => "copying",
        Phase::Write => "writing the databases",
        Phase::Delete => "deleting files",
        Phase::Verify => "verifying",
        Phase::Format => "formatting",
        // Mix identification's phases (edits don't run them), and any newer.
        _ => "working",
    }
}

impl Progress for Reporter {
    fn on_event(&self, e: &Event) {
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        let (detail, now) = match e {
            Event::PhaseStart { phase, .. } => {
                if matches!(phase, Phase::Write | Phase::Delete) {
                    self.committed.store(true, Ordering::SeqCst);
                }
                state.0 = phase_name(*phase).to_string();
                (state.0.clone(), true)
            }
            Event::Item { path, .. } => (format!("{} · {}", state.0, leaf(path)), false),
            _ => (String::new(), false),
        };
        let committed = self.committed.load(Ordering::SeqCst);
        if !committed && self.outer.is_cancelled() {
            self.inner.cancel();
        }
        if detail.is_empty() || !(now || state.1.is_none_or(|t| t.elapsed() > Duration::from_millis(80))) {
            return;
        }
        state.1 = Some(Instant::now());
        let _ = self.tx.send(Msg::Progress { job: self.job, detail, committed });
    }
}

/// How the player puts a plan: the confirmation's question and its yes, the drives it changes
/// and what it does to them (for plans that aren't edits or an undo, which show in detail), and
/// the header's label while it's made. `drive` names the drive it was planned for.
pub struct Wording {
    pub ask: String,
    pub yes: &'static str,
    pub who: String,
    pub what: &'static str,
    pub doing: String,
}

pub fn wording(plan: &dyn AnyPlanned, drive: &str) -> Wording {
    let written: Vec<&str> = (plan.drives().iter()).filter(|t| t.writes && t.drive).map(|t| t.name.as_str()).collect();
    let who = if written.is_empty() { drive.to_string() } else { written.join(", ") };
    let w = |ask: String, yes, what, doing: String| Wording { ask, yes, who: who.clone(), what, doing };
    match plan.operation() {
        "pull" => w(
            format!("Save {drive} in the store?"),
            "save",
            "save it in the store",
            format!("Saving {drive} in the store"),
        ),
        "eject" => w(format!("Eject {drive}?"), "eject", "unmount it and power it off", format!("Ejecting {drive}")),
        "sync.group" => {
            let n = written.len();
            let ask = format!("Sync {n} drive{}?", if n == 1 { "" } else { "s" });
            w(ask, "sync", "bring up to date from the newest drive", "Syncing".into())
        }
        "pack" => w(format!("Pack a set onto {drive}?"), "pack", "pack a set onto it", format!("Packing onto {drive}")),
        "undo" => w(format!("Undo the last change on {drive}?"), "undo", "undo", format!("Undoing on {drive}")),
        op => {
            let what = match op {
                "playlist.restore" => "bring back a playlist",
                "tags" => "change My Tags",
                op => op,
            };
            w(format!("Write to {drive}?"), "write", what, format!("Writing {drive}"))
        }
    }
}

/// An error as the player says it.
pub fn describe(e: &Error, drive: &str) -> String {
    match e {
        Error::Locked(_) => {
            format!("{drive} is busy: another locallibrary command is changing it. Try again when it's done.")
        }
        Error::NoSpace { .. } => format!("not enough free space on {drive} for this change"),
        e => e.to_string(),
    }
}

impl App {
    // -----------------------------------------------------------------------------------------
    // Opening the menu
    // -----------------------------------------------------------------------------------------

    /// What the actions menu would work on now.
    pub(super) fn target(&self) -> Option<Target> {
        let l = self.loaded.as_ref()?;
        let lib = &l.library;
        let root = l.root().to_path_buf();
        let tracks: Vec<usize> = match self.focus {
            Focus::Details => self.selected_match().into_iter().collect(),
            Focus::Crate => self
                .crate_state
                .selected()
                .and_then(|i| self.crate_list.items.get(i))
                .filter(|t| t.root == root)
                .and_then(|t| l.index_of(&t.track.file_path))
                .into_iter()
                .collect(),
            _ => self.targets(),
        };
        let what = match tracks.as_slice() {
            [] => "nothing selected".to_string(),
            [i] if self.marks.is_empty() || self.focus != Focus::Tracks => lib.tracks[*i].display_name(),
            many => format!("{} marked track{}", many.len(), if many.len() == 1 { "" } else { "s" }),
        };
        let playlist = |id: &NodeId| match id {
            NodeId::Playlist(pid) => lib.playlist(*pid).map(|p| (lib.playlist_path(p.id), p.is_folder)),
            _ => None,
        };
        let list = playlist(&l.node).filter(|(_, folder)| !folder).map(|(p, _)| p);
        let node = match self.focus {
            Focus::Playlists => {
                self.playlist_state.selected().and_then(|i| l.tree.get(i)).and_then(|n| playlist(&n.id))
            }
            _ => playlist(&l.node),
        };
        let folder = match &node {
            Some((p, true)) => p.clone(),
            Some((p, false)) => p.rsplit_once('/').map(|(f, _)| f.to_string()).unwrap_or_default(),
            None => String::new(),
        };
        let crated =
            self.crate_list.items.iter().filter(|t| t.root == root).map(|t| t.track.file_path.clone()).collect();
        Some(Target {
            root,
            tracks: tracks.iter().map(|&i| lib.tracks[i].file_path.clone()).collect(),
            what,
            list,
            node,
            folder,
            sort: self.sort,
            crated,
            crate_total: self.crate_list.len(),
            stale: l.stale,
        })
    }

    pub(super) fn open_menu(&mut self, at: Option<(u16, u16)>) {
        match self.target() {
            Some(t) => self.modal = Some(Modal::Menu(Menu::new(t, at))),
            None => self.toast("open a drive first", Level::Info),
        }
    }

    // -----------------------------------------------------------------------------------------
    // Dialogs from the background
    // -----------------------------------------------------------------------------------------

    /// Whether keys go somewhere other than the view: to a dialog, or a line being typed.
    pub(super) fn input_taken(&self) -> bool {
        self.modal.is_some()
            || self.searching
            || self.picking.is_some()
            || self.deck.outputs.is_some()
            || self.help
            || self.show_log
            || self.mix.input.is_some()
            || self.profile.account.typing()
    }

    /// Open a dialog that comes from the background (a plan, a task's results). While another
    /// dialog or a line being typed has the keys, it waits for that to close (see
    /// [`Self::show_waiting`]), so nothing typed for one answers the other.
    pub(super) fn show(&mut self, m: Modal) {
        if self.input_taken() {
            self.waiting.push_back(m);
            self.toast("done in the background: it shows once this is closed", Level::Info);
        } else {
            self.present(m);
        }
    }

    /// Show the next dialog waiting, once nothing else has the keys.
    pub(super) fn show_waiting(&mut self) {
        if !self.input_taken()
            && let Some(m) = self.waiting.pop_front()
        {
            self.present(m);
        }
    }

    fn present(&mut self, mut m: Modal) {
        // A confirmation ignores keys for a moment from when it's shown (see `SETTLE`).
        if let Modal::Confirm(c) = &mut m {
            c.shown = Instant::now();
        }
        self.modal = Some(m);
    }

    // -----------------------------------------------------------------------------------------
    // Keys while a dialog is open
    // -----------------------------------------------------------------------------------------

    pub(super) fn on_modal_key(&mut self, key: KeyEvent) {
        let Some(modal) = self.modal.take() else { return };
        match modal {
            Modal::Menu(m) => self.on_menu_key(m, key),
            Modal::Prompt(mut p) => match key.code {
                KeyCode::Esc => {}
                KeyCode::Enter => self.submit(p),
                _ => {
                    edit_line(&mut p.text, key);
                    self.modal = Some(Modal::Prompt(p));
                }
            },
            // Only y writes, and only once the dialog has been up long enough to be read.
            Modal::Confirm(c) if c.shown.elapsed() < SETTLE && key.code != KeyCode::Esc => {
                self.modal = Some(Modal::Confirm(c));
            }
            Modal::Confirm(mut c) => match key.code {
                KeyCode::Char('y') | KeyCode::Char('Y') => self.apply(c),
                KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Char('q') | KeyCode::Esc => {
                    self.toast("nothing written", Level::Info)
                }
                code => {
                    c.scroll = match code {
                        KeyCode::Up | KeyCode::Char('k') => c.scroll.saturating_sub(1),
                        KeyCode::Down | KeyCode::Char('j') => c.scroll.saturating_add(1),
                        KeyCode::PageUp => c.scroll.saturating_sub(10),
                        KeyCode::PageDown => c.scroll.saturating_add(10),
                        _ => c.scroll,
                    };
                    self.modal = Some(Modal::Confirm(c));
                }
            },
            Modal::Pick(p) => self.on_picker_key(p, key),
            Modal::Report(mut r) => {
                r.scroll = match key.code {
                    KeyCode::Up | KeyCode::Char('k') => r.scroll.saturating_sub(1),
                    KeyCode::Down | KeyCode::Char('j') => r.scroll.saturating_add(1),
                    KeyCode::PageUp => r.scroll.saturating_sub(10),
                    KeyCode::PageDown | KeyCode::Char(' ') => r.scroll.saturating_add(10),
                    _ => return,
                };
                self.modal = Some(Modal::Report(r));
            }
            Modal::Ask(question, yes) => match key.code {
                KeyCode::Char('y') | KeyCode::Char('Y') => yes(self),
                KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc | KeyCode::Char('q') => {}
                _ => self.modal = Some(Modal::Ask(question, yes)),
            },
        }
    }

    fn on_menu_key(&mut self, mut m: Menu, key: KeyEvent) {
        if let Some(sub) = m.sub {
            let choices = Menu::sub_items(sub);
            let pick = match key.code {
                KeyCode::Esc | KeyCode::Backspace | KeyCode::Left => {
                    m.sub = None;
                    None
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    m.sub_state.select(Some(m.sub_state.selected().unwrap_or(0).saturating_sub(1)));
                    None
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    m.sub_state.select(Some((m.sub_state.selected().unwrap_or(0) + 1).min(choices.len() - 1)));
                    None
                }
                KeyCode::Enter => m.sub_state.selected().and_then(|i| choices.get(i)).copied(),
                KeyCode::Char(d @ '0'..='9') => {
                    let n = d as u8 - b'0';
                    match sub {
                        Sub::Rating if n <= 5 => Some(Action::Rate(n)),
                        Sub::Colour if n <= 8 => Some(Action::Colour(n.checked_sub(1).map(|c| COLOURS[c as usize]))),
                        _ => None,
                    }
                }
                _ => None,
            };
            match pick {
                Some(action) => self.run_action(action, m.target),
                None => self.modal = Some(Modal::Menu(m)),
            }
            return;
        }
        let pick = match key.code {
            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('e') => return,
            KeyCode::Up | KeyCode::Char('k') => {
                m.state.select(Some(m.state.selected().unwrap_or(0).saturating_sub(1)));
                None
            }
            KeyCode::Down | KeyCode::Char('j') => {
                m.state.select(Some((m.state.selected().unwrap_or(0) + 1).min(m.items.len() - 1)));
                None
            }
            KeyCode::Enter | KeyCode::Right => m.state.selected(),
            KeyCode::Char(d @ '0'..='5') => {
                let i = m.items.iter().position(|i| i.action == Action::RatingMenu);
                if i.is_some_and(|i| m.items[i].unavailable.is_none()) {
                    self.run_action(Action::Rate(d as u8 - b'0'), m.target);
                    return;
                }
                i
            }
            KeyCode::Char(c) => m.items.iter().position(|i| i.key.len() == 1 && i.key.starts_with(c)),
            _ => None,
        };
        match pick {
            Some(i) => self.choose(m, i),
            None => self.modal = Some(Modal::Menu(m)),
        }
    }

    /// Choose menu item `i` (by key, click or enter).
    pub(super) fn choose(&mut self, mut m: Menu, i: usize) {
        m.state.select(Some(i));
        let Some(item) = m.items.get(i) else { return };
        if let Some(why) = &item.unavailable {
            self.toast(why.clone(), Level::Info);
            self.modal = Some(Modal::Menu(m));
            return;
        }
        match item.action {
            Action::RatingMenu => {
                m.sub = Some(Sub::Rating);
                m.sub_state.select(Some(0));
                self.modal = Some(Modal::Menu(m));
            }
            Action::ColourMenu => {
                m.sub = Some(Sub::Colour);
                m.sub_state.select(Some(0));
                self.modal = Some(Modal::Menu(m));
            }
            action => self.run_action(action, m.target),
        }
    }

    pub(super) fn run_action(&mut self, action: Action, t: Target) {
        let prompt =
            |label: String, text: String, purpose| Modal::Prompt(Prompt { label, text, purpose, root: t.root.clone() });
        let in_folder = if t.folder.is_empty() { String::new() } else { format!("{}/", t.folder) };
        match action {
            Action::CrateAdd => self.add_to_crate(&t.root, &t.tracks),
            Action::Rate(n) => self.plan_edits(t.root.clone(), vec![set_rating(&t.tracks, n)]),
            Action::Colour(c) => self.plan_edits(t.root.clone(), vec![set_colour(&t.tracks, c)]),
            Action::AddToPlaylist => {
                self.picking = Some(Pick { focus: self.focus, target: t });
                self.focus = Focus::Playlists;
            }
            Action::NewPlaylistWith => {
                let label = format!("New playlist with {}", t.what);
                self.modal = Some(prompt(label, in_folder, PromptFor::NewPlaylist { with: t.tracks.clone() }));
            }
            Action::RemoveFromList => {
                if let Some(list) = &t.list {
                    self.plan_edits(t.root.clone(), vec![remove_from(list, &t.tracks)]);
                }
            }
            Action::SortList => {
                if let (Some(list), Some(k)) = (&t.list, t.sort) {
                    self.plan_edits(t.root.clone(), vec![sort(list, k)]);
                }
            }
            Action::NewPlaylist => {
                self.modal = Some(prompt("New playlist".into(), in_folder, PromptFor::NewPlaylist { with: Vec::new() }))
            }
            Action::NewFolder => self.modal = Some(prompt("New folder".into(), in_folder, PromptFor::NewFolder)),
            Action::Rename => {
                if let Some((path, _)) = &t.node {
                    let label = format!("Rename {} to", leaf(path));
                    self.modal =
                        Some(prompt(label, leaf(path).to_string(), PromptFor::Rename { playlist: path.clone() }));
                }
            }
            Action::Delete => {
                if let Some((path, _)) = &t.node {
                    self.plan_edits(t.root.clone(), vec![delete(path)]);
                }
            }
            Action::SaveCrate => {
                let label = format!(
                    "Save the crate's {} track{} as",
                    t.crated.len(),
                    if t.crated.len() == 1 { "" } else { "s" }
                );
                let text = format!("{in_folder}Crate");
                self.modal = Some(prompt(label, text, PromptFor::SaveCrate { tracks: t.crated.clone() }));
            }
            Action::ClearCrate => {
                let q = format!("Clear the crate ({} tracks)? The drive isn't touched.", t.crate_total);
                self.modal = Some(Modal::Ask(
                    q,
                    Box::new(|app: &mut App| {
                        app.crate_list.clear();
                        app.crate_state.select(None);
                        app.toast("crate cleared", Level::Info);
                    }),
                ));
            }
            Action::Replay => self.replay(t.root.clone()),
            Action::Tag => self.tag_tracks(),
            Action::Restore => self.restore_playlist(t.root.clone()),
            Action::RatingMenu | Action::ColourMenu => {}
        }
    }

    fn submit(&mut self, p: Prompt) {
        let edits = match &p.purpose {
            PromptFor::NewPlaylist { with } => clean_path(&p.text).map(|path| create(&path, false, with)),
            PromptFor::NewFolder => clean_path(&p.text).map(|path| create(&path, true, &[])),
            PromptFor::SaveCrate { tracks } => clean_path(&p.text).map(|path| create(&path, false, tracks)),
            PromptFor::Rename { playlist } => {
                let name = p.text.trim();
                (!name.is_empty() && !name.contains('/')).then(|| vec![rename(playlist, name)])
            }
            PromptFor::CrateForPack { tracks, target } => match clean_path(&p.text) {
                Some(path) => {
                    let after = After::Pack { source: p.root.clone(), target: target.clone(), playlist: path.clone() };
                    let drive = p.root.display().to_string();
                    let edits = create(&path, false, tracks);
                    self.plan_then(p.root.clone(), Some(after), move |ctx| {
                        let req = EditRequest { drive, edits, options: EditOptions::default() };
                        Ok(Box::new(api::edit_plan(&req, ctx)?))
                    });
                    return;
                }
                None => None,
            },
            PromptFor::ExportList { list } => {
                let what = super::drives::Export::List(list.clone());
                self.write_export(p.root, &p.text, what);
                return;
            }
            PromptFor::ExportXml { only } => {
                let what = super::drives::Export::Xml(only.clone());
                self.write_export(p.root, &p.text, what);
                return;
            }
            PromptFor::NewTag { tracks } => {
                let tag = p.text.trim().trim_matches('/').to_string();
                match tag.split_once('/') {
                    Some((c, t)) if !c.trim().is_empty() && !t.trim().is_empty() && !t.contains('/') => {
                        let tracks = tracks.clone();
                        self.plan_tag(p.root, tracks, tag, true);
                    }
                    _ => {
                        self.toast("a My Tag is Category/Tag: Genre/Deep House, Situation/Warmup", Level::Info);
                        self.modal = Some(Modal::Prompt(p));
                    }
                }
                return;
            }
            PromptFor::SmartRule { filter, sort } => {
                match clean_path(&p.text) {
                    Some(path) => {
                        let (filter, sort) = (filter.clone(), sort.clone());
                        self.save_smart_rule(p.root, path, filter, sort);
                    }
                    None => {
                        self.toast("give it a path, with any folders before it: Smart/Peak time", Level::Info);
                        self.modal = Some(Modal::Prompt(p));
                    }
                }
                return;
            }
            PromptFor::RestorePlaylist | PromptFor::RestoreFrom { .. } => {
                let name = p.text.trim().trim_matches('/').to_string();
                if name.is_empty() {
                    self.toast("name the playlist or folder: Sets/2026/Friday", Level::Info);
                    self.modal = Some(Modal::Prompt(p));
                    return;
                }
                match p.purpose {
                    PromptFor::RestoreFrom { snapshot } => self.plan_restore(p.root, name, Some(snapshot)),
                    _ => self.pick_snapshot(p.root, name),
                }
                return;
            }
        };
        match edits {
            Some(edits) => self.plan_edits(p.root.clone(), edits),
            None => {
                let why = match p.purpose {
                    PromptFor::Rename { .. } => "give it a name (without /)",
                    _ => "give it a name, with any folders before it: Sets/2026/Friday",
                };
                self.toast(why, Level::Info);
                self.modal = Some(Modal::Prompt(p));
            }
        }
    }

    /// Keys while picking a playlist in the tree. Returns whether the key was used up: moving
    /// about and playback keys still work, nothing else does until the pick is made or dropped.
    pub(super) fn on_pick_key(&mut self, key: KeyEvent) -> bool {
        match key.code {
            KeyCode::Esc => {
                if let Some(p) = self.picking.take() {
                    self.focus = p.focus;
                }
                self.toast("nothing added", Level::Info);
                true
            }
            KeyCode::Enter => {
                self.pick();
                true
            }
            KeyCode::Up
            | KeyCode::Down
            | KeyCode::PageUp
            | KeyCode::PageDown
            | KeyCode::Home
            | KeyCode::End
            | KeyCode::Left
            | KeyCode::Right
            | KeyCode::Char('j' | 'k' | 'g' | 'G' | ' ' | '+' | '=' | '-' | '_' | '?') => false,
            _ => true,
        }
    }

    /// Enter in the tree while picking: a playlist is the pick; folders open and close.
    pub(super) fn pick(&mut self) {
        let Some(l) = &mut self.loaded else { return };
        let Some(node) = self.playlist_state.selected().and_then(|i| l.tree.get(i)) else { return };
        let id = node.id.clone();
        if node.folder {
            l.toggle(&id);
            return;
        }
        let NodeId::Playlist(pid) = id else {
            self.toast("pick a playlist (esc to cancel)", Level::Info);
            return;
        };
        let path = l.library.playlist_path(pid);
        let Some(p) = self.picking.take() else { return };
        self.focus = p.focus;
        self.plan_edits(p.target.root.clone(), vec![add_to(&path, &p.target.tracks)]);
    }

    // -----------------------------------------------------------------------------------------
    // Plan, confirm, apply
    // -----------------------------------------------------------------------------------------

    /// The drive actions work on in the view shown: the open one in the library, the selected
    /// one in the Drives view.
    pub(super) fn action_drive(&self) -> Option<PathBuf> {
        match self.view {
            View::Drives => self.drive_state.selected().and_then(|i| self.drives.get(i)).map(|d| d.root.clone()),
            _ => self.loaded.as_ref().map(|l| l.root().to_path_buf()),
        }
    }

    /// `u`: take back the last change made, on the drive it was made to (before any this run,
    /// the drive actions work on), after showing what that puts back (see [`Self::undo_on`]).
    pub(super) fn undo(&mut self) {
        match self.undo_drive() {
            Some(root) => self.undo_on(root),
            None => self.toast("open a drive first", Level::Info),
        }
    }

    /// The drive `u` undoes on: the one changed last this run, else the drive actions work on.
    pub(super) fn undo_drive(&self) -> Option<PathBuf> {
        self.last_backup.as_ref().map(|k| k.root.clone()).or_else(|| self.action_drive())
    }

    /// Take back the last change made to the drive at `root`, after showing what that puts back
    /// (the files, how the playlists and tracks change, the recorded edits it takes off the
    /// journal).
    pub(super) fn undo_on(&mut self, root: PathBuf) {
        let drive = root.display().to_string();
        self.plan(root, move |_| {
            Ok(Box::new(api::undo_plan(&api::UndoRequest { drive: Some(drive), ..Default::default() })?))
        });
    }

    /// Work out what `edits` would do to the drive at `root`, in the background; the plan comes
    /// back as a confirmation.
    pub(super) fn plan_edits(&mut self, root: PathBuf, edits: Vec<Edit>) {
        let drive = root.display().to_string();
        self.plan(root, move |ctx| {
            let req = EditRequest { drive, edits, options: EditOptions::default() };
            Ok(Box::new(api::edit_plan(&req, ctx)?))
        });
    }

    /// Plan a change to the drive at `root` in the background with `make`; the plan comes back as
    /// a confirmation, and a yes applies exactly it.
    pub(super) fn plan(
        &mut self,
        root: PathBuf,
        make: impl FnOnce(&Ctx<'_>) -> Result<Box<dyn AnyPlanned>, Error> + Send + 'static,
    ) {
        self.plan_then(root, None, make);
    }

    /// [`Self::plan`], with `after` to do once the change is made.
    pub(super) fn plan_then(
        &mut self,
        root: PathBuf,
        after: Option<After>,
        make: impl FnOnce(&Ctx<'_>) -> Result<Box<dyn AnyPlanned>, Error> + Send + 'static,
    ) {
        if let Some(j) = &self.job {
            let msg = format!("wait for {} to finish", j.label.to_lowercase());
            self.toast(msg, Level::Info);
            return;
        }
        if self.loaded.as_ref().is_some_and(|l| l.stale && l.root() == root) {
            self.toast("the drive was unplugged", Level::Error);
            return;
        }
        let id = self.next_id();
        let cancel = CancelToken::new();
        self.job = Some(Job {
            id,
            label: "Planning".into(),
            detail: String::new(),
            cancel: cancel.clone(),
            writing: false,
            committed: false,
            started: Instant::now(),
            touched: Vec::new(),
            after,
        });
        let tx = self.sender();
        // `--overwrite-archive` holds for plans made on the worker thread too.
        let archive = locallibrary::op::archive_writes_allowed();
        let failed_root = root.clone();
        let job = move || {
            locallibrary::op::allow_archive_writes(archive);
            let reporter = Reporter::new(tx, id, cancel.clone(), cancel.clone());
            let ctx = Ctx::new(&reporter, cancel);
            let result = make(&ctx);
            Msg::Planned { job: id, root, result }
        };
        self.spawn_or(job, move |e| Msg::Planned { job: id, root: failed_root, result: Err(Error::Invalid(e)) });
    }
    /// Run `make` in the background (something that changes nothing, like a check), with its
    /// progress in the header and esc to stop it; what it hands back finishes on the UI thread.
    pub(super) fn task(&mut self, label: String, make: impl FnOnce(&Ctx<'_>) -> Result<Done, Error> + Send + 'static) {
        if let Some(j) = &self.job {
            let msg = format!("wait for {} to finish", j.label.to_lowercase());
            self.toast(msg, Level::Info);
            return;
        }
        let id = self.next_id();
        let cancel = CancelToken::new();
        self.job = Some(Job {
            id,
            label,
            detail: String::new(),
            cancel: cancel.clone(),
            writing: false,
            committed: false,
            started: Instant::now(),
            touched: Vec::new(),
            after: None,
        });
        let tx = self.sender();
        let job = move || {
            let reporter = Reporter::new(tx, id, cancel.clone(), cancel.clone());
            let ctx = Ctx::new(&reporter, cancel);
            Msg::Done { job: id, result: make(&ctx) }
        };
        self.spawn_or(job, move |e| Msg::Done { job: id, result: Err(Error::Invalid(e)) });
    }

    pub(super) fn on_done(&mut self, job: u64, result: Result<Done, Error>) {
        let Some(j) = self.job.take_if(|j| j.id == job) else { return }; // cancelled
        match result {
            Ok(finish) => {
                // A dialog it opens (results, a list to choose from) waits its turn.
                let open = self.modal.take();
                finish(self);
                if let Some(new) = std::mem::replace(&mut self.modal, open) {
                    self.show(new);
                }
            }
            Err(Error::Cancelled) => {}
            Err(e) => {
                let what = j.label.to_lowercase();
                self.toast(format!("{what}: {e}"), Level::Error)
            }
        }
    }

    pub(super) fn on_planned(&mut self, job: u64, root: PathBuf, result: Result<Box<dyn AnyPlanned>, Error>) {
        if self.job.as_ref().is_none_or(|j| j.id != job) {
            return; // cancelled
        }
        let after = self.job.take().and_then(|j| j.after);
        let drive = self.drive_label(&root);
        match result {
            Ok(plan) if plan.is_noop() => {
                let msg = match (plan.operation(), plan.warnings().first()) {
                    ("undo", _) => format!("nothing to undo on {drive}: it's as its last backup left it"),
                    ("pull", _) => plan.summary().first().cloned().unwrap_or_else(|| "saved already".into()),
                    (_, Some(w)) => format!("nothing to change: {w}"),
                    (_, None) => "nothing to change".into(),
                };
                self.toast(msg, Level::Info);
            }
            Ok(plan) => {
                self.show(Modal::Confirm(Confirm { root, drive, plan, scroll: 0, shown: Instant::now(), after }))
            }
            Err(Error::Cancelled) => {}
            Err(e) => self.toast(describe(&e, &drive), Level::Error),
        }
    }

    /// Apply the confirmed plan. The library refuses if the drive changed since it was planned.
    fn apply(&mut self, c: Confirm) {
        if let Some(j) = &self.job {
            let msg = format!("wait for {} to finish", j.label.to_lowercase());
            self.toast(msg, Level::Info);
            return;
        }
        let id = self.next_id();
        let outer = CancelToken::new();
        let touched: Vec<PathBuf> = c.plan.drives().iter().filter(|t| t.writes).map(|t| t.root.clone()).collect();
        let label = wording(&*c.plan, &c.drive).doing;
        // Ejecting waits for a track being opened, whose file would keep the drive busy.
        let opening = (c.plan.operation() == "eject").then(|| self.opener_idle());
        if opening.is_some() {
            for t in c.plan.drives() {
                let root = t.root.clone();
                self.let_go_of(&root);
            }
        }
        self.job = Some(Job {
            id,
            label,
            detail: String::new(),
            cancel: outer.clone(),
            writing: true,
            committed: false,
            started: Instant::now(),
            touched,
            after: c.after.clone(),
        });
        let tx = self.sender();
        let Confirm { root, plan, .. } = c;
        let store_root = self.store_root.clone();
        let failed_root = root.clone();
        let job = move || {
            let _opened = opening.as_ref().map(|o| o.lock().unwrap_or_else(std::sync::PoisonError::into_inner));
            let inner = CancelToken::new();
            let reporter = Reporter::new(tx, id, outer.clone(), inner.clone());
            let ctx = Ctx::new(&reporter, inner);
            let result = if outer.is_cancelled() {
                Err(Error::Cancelled)
            } else {
                let store = locallibrary::Store::open(&store_root).ok();
                let operation = plan.operation();
                plan.apply_json(store.as_ref(), &ctx).map(|report| Applied { operation, report })
            };
            Msg::Applied { job: id, root, result }
        };
        self.spawn_or(job, move |e| Msg::Applied { job: id, root: failed_root, result: Err(Error::Invalid(e)) });
    }

    pub(super) fn on_applied(&mut self, job: u64, root: PathBuf, result: Result<Applied, Error>) {
        let (touched, after) = match self.job.take_if(|j| j.id == job) {
            Some(j) => (j.touched, j.after),
            None => (Vec::new(), None),
        };
        let drive = self.drive_label(&root);
        let done = result.is_ok();
        if let Ok(a) = &result
            && (a.report["backup"].is_string() || a.operation == "undo")
        {
            self.last_backup = Some(self.drive_at(root.clone()));
        }
        match result {
            Ok(Applied { operation: "pull", report }) => {
                let lib = report["library"].as_str().unwrap_or("its library");
                let snap = report["snapshot"]["short_id"].as_str().map(|s| &s[..8.min(s.len())]).unwrap_or("");
                let msg = if report["up_to_date"].as_bool() == Some(true) {
                    format!("{drive} was saved in the store already ({lib}, {snap})")
                } else {
                    let files = report["imported_files"].as_u64().unwrap_or(0);
                    let bytes = report["imported_bytes"].as_u64().unwrap_or(0);
                    format!(
                        "saved {drive} in the store: snapshot {snap} of {lib} ({files} new file{}, {})",
                        if files == 1 { "" } else { "s" },
                        locallibrary::op::bytes(bytes)
                    )
                };
                self.toast(msg, Level::Info);
                self.refresh_status();
                self.history = None;
            }
            Ok(Applied { operation: "eject", report }) => {
                let off = report.as_array().is_some_and(|r| r.iter().all(|e| e["powered_off"].as_bool() == Some(true)));
                let msg =
                    if off { format!("ejected {drive}: it can be unplugged") } else { format!("unmounted {drive}") };
                self.toast(msg, Level::Info);
                self.refresh_drives();
            }
            Ok(Applied { operation: "sync.group", report }) => {
                let results = report.as_array().cloned().unwrap_or_default();
                let failed: Vec<String> = (results.iter())
                    .filter(|r| !r["error"].is_null())
                    .map(|r| {
                        format!(
                            "{}: {}",
                            r["name"].as_str().unwrap_or("?"),
                            r["error"]["message"].as_str().unwrap_or("")
                        )
                    })
                    .collect();
                if failed.is_empty() {
                    self.toast(
                        format!("synced {} drive{}", results.len(), if results.len() == 1 { "" } else { "s" }),
                        Level::Info,
                    );
                } else {
                    self.toast(format!("sync: {}", failed.join("; ")), Level::Error);
                }
                self.refresh_status();
            }
            Ok(Applied { operation: "tags", report }) => {
                let n = report["tracks_changed"].as_u64().unwrap_or(0);
                let mut msg = format!("My Tags changed on {n} track{} on {drive}", if n == 1 { "" } else { "s" });
                if report["backup"].is_string() {
                    msg.push_str(" · u to undo");
                }
                self.toast(msg, Level::Info);
            }
            Ok(Applied { operation: "pack", report }) => {
                let copied = report["copied"].as_u64().unwrap_or(0);
                let bytes = report["copied_bytes"].as_u64().unwrap_or(0);
                let mut msg = format!(
                    "packed onto {drive}: {copied} file{} copied ({})",
                    if copied == 1 { "" } else { "s" },
                    locallibrary::op::bytes(bytes)
                );
                if report["backup"].is_string() {
                    msg.push_str(" · u to undo");
                }
                self.toast(msg, Level::Info);
                self.refresh_status();
            }
            Ok(Applied { operation: "undo", report }) => {
                let when = report["created"].as_str().and_then(|t| t.get(..16)).unwrap_or("").replace('T', " ");
                let why = report["reason"].as_str().unwrap_or("a change");
                self.toast(format!("undone on {drive}: put back the backup ({why}, {when})"), Level::Info);
            }
            Ok(Applied { operation, report }) => {
                let databases: Vec<&str> =
                    report["databases"].as_array().into_iter().flatten().filter_map(|d| d.as_str()).collect();
                let mut msg = match report["changes"].as_u64() {
                    Some(n) if !databases.is_empty() => format!(
                        "{n} change{} written to {} on {drive}",
                        if n == 1 { "" } else { "s" },
                        databases.join(" and ")
                    ),
                    _ => format!("{operation}: done on {drive}"),
                };
                if report["backup"].is_string() {
                    msg.push_str(" · u to undo");
                }
                if let Some(w) = report["warnings"].as_array().and_then(|w| w.first()).and_then(|w| w.as_str()) {
                    msg.push_str(&format!(" · {w}"));
                }
                self.toast(msg, Level::Info);
            }
            Err(Error::Cancelled) => self.toast("cancelled: nothing was written", Level::Info),
            Err(e) => self.toast(describe(&e, &drive), Level::Error),
        }
        // Read the drive again (keeping the view), whatever happened: it's the truth now.
        let open = self.loaded.as_ref().map(|l| l.root().to_path_buf());
        if let Some(open) = open.filter(|o| *o == root || touched.contains(o)) {
            self.load_library(open);
        }
        if self.view == View::Drives {
            self.refresh_drives();
        }
        match after {
            Some(After::Pack { source, target, playlist }) if done => self.plan_pack(source, target, vec![playlist]),
            _ => {}
        }
        if self.quit_after_write && self.job.is_none() {
            self.quit = true;
            self.exit_message = self.toast.as_ref().map(|(m, ..)| m.clone());
        }
    }

    pub(super) fn on_progress(&mut self, job: u64, detail: String, committed: bool) {
        if let Some(j) = self.job.as_mut().filter(|j| j.id == job) {
            if !j.cancel.is_cancelled() {
                j.detail = detail;
            }
            j.committed |= committed;
        }
    }

    /// Esc while a job runs: planning stops at once; writing only until the databases are being
    /// written.
    pub(super) fn cancel_job(&mut self) {
        let Some(j) = &mut self.job else { return };
        if j.committed {
            self.toast("writing the drive: too late to stop", Level::Info);
        } else if j.writing {
            j.cancel.cancel();
            j.detail = "cancelling…".into();
        } else {
            j.cancel.cancel();
            self.job = None;
            self.toast("cancelled", Level::Info);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc::channel;

    use locallibrary::progress::Phase;
    use ratatui::crossterm::event::KeyModifiers;

    use super::super::app::testing::*;
    use super::super::drives::Tone;
    use super::*;

    fn paths() -> Vec<String> {
        vec!["/Contents/a.mp3".into(), "/Contents/b.mp3".into()]
    }

    #[test]
    fn builds_edits_naming_tracks_by_file() {
        match set_rating(&paths(), 4) {
            Edit::EditTracks { tracks, fields } => {
                assert_eq!(tracks.paths, paths());
                assert_eq!(fields, TrackFields { rating: Some(4), ..Default::default() });
            }
            e => panic!("{e:?}"),
        }
        match set_colour(&paths(), None) {
            Edit::EditTracks { fields, .. } => assert_eq!(fields.color.as_deref(), Some("none")),
            e => panic!("{e:?}"),
        }
        match set_colour(&paths(), Some(TrackColor::Aqua)) {
            Edit::EditTracks { fields, .. } => assert_eq!(fields.color_value().unwrap(), Some(Some(TrackColor::Aqua))),
            e => panic!("{e:?}"),
        }
        let e = create("Shows/Friday", false, &paths());
        assert_eq!(e.len(), 2);
        assert!(matches!(&e[0], Edit::CreatePlaylist { path, folder: false } if path == "Shows/Friday"));
        assert!(
            matches!(&e[1], Edit::AddTracks { playlist, tracks, .. } if playlist == "Shows/Friday" && tracks.paths.len() == 2)
        );
        assert_eq!(create("Shows", true, &[]).len(), 1);
        let by = SortKey { field: locallibrary::query::SortField::Key, descending: false };
        assert!(matches!(sort("Shows/Friday", by), Edit::SortPlaylist { by, .. } if by == "key"));
        assert!(
            matches!(remove_from("A", &paths()), Edit::RemoveTracks { tracks: Some(_), positions, .. } if positions.is_empty())
        );
    }

    #[test]
    fn typed_paths() {
        assert_eq!(clean_path(" Shows / 2026/Friday "), Some("Shows/2026/Friday".into()));
        assert_eq!(clean_path("/Warmup"), Some("Warmup".into()));
        assert_eq!(clean_path("Shows/"), None);
        assert_eq!(clean_path("a//b"), None);
        assert_eq!(clean_path("  "), None);
    }

    #[test]
    fn menu_says_why_actions_are_unavailable() {
        let t =
            Target { root: "/Volumes/A".into(), tracks: paths(), what: "2 marked tracks".into(), ..Default::default() };
        let items = menu_items(&t);
        let find = |items: &[MenuItem], a| items.iter().find(|i| i.action == a).map(|i| i.unavailable.clone()).unwrap();
        assert_eq!(find(&items, Action::RatingMenu), None);
        assert_eq!(find(&items, Action::RemoveFromList).as_deref(), Some("the list shown isn't a playlist"));
        assert!(find(&items, Action::Rename).is_some());
        assert_eq!(find(&items, Action::ClearCrate).as_deref(), Some("the crate is empty"));
        let t = Target {
            list: Some("Sets/Friday".into()),
            node: Some(("Sets/Friday".into(), false)),
            folder: "Sets".into(),
            sort: Some(SortKey { field: locallibrary::query::SortField::Bpm, descending: true }),
            ..t
        };
        let items = menu_items(&t);
        assert_eq!(find(&items, Action::RemoveFromList), None);
        assert_eq!(find(&items, Action::SortList), None);
        assert!(items.iter().any(|i| i.label == "sort Friday by BPM ↓ on the drive"));
        assert!(items.iter().any(|i| i.label == "new playlist in Sets…"));
        let t = Target { stale: true, ..t };
        assert!(menu_items(&t).iter().filter(|i| i.action != Action::CrateAdd).all(|i| i.unavailable.is_some()));
    }

    fn clear_line(app: &mut App) {
        app.on_key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));
    }

    /// The menu open now: whether a picker is open in it, and the rows selected.
    fn menu(app: &App) -> (Option<Sub>, Option<usize>, Option<usize>) {
        match &app.modal {
            Some(Modal::Menu(m)) => (m.sub, m.state.selected(), m.sub_state.selected()),
            _ => panic!("no menu: {}", last_message(app)),
        }
    }

    /// The menu moves with the arrows and j/k, opens what's selected with enter or right, and
    /// says why an item can't be done (staying open). The rating and colour pickers move the
    /// same way, take a digit, and go back to the menu with esc or left.
    #[test]
    fn the_menu_and_its_pickers_answer_to_keys() {
        let (mut app, _dir) = app_with(library());
        press(&mut app, "ej");
        key(&mut app, KeyCode::Down);
        press(&mut app, "k");
        assert_eq!(menu(&app).1, Some(1));
        key(&mut app, KeyCode::Up);
        key(&mut app, KeyCode::Up);
        assert_eq!(menu(&app).1, Some(0));
        press(&mut app, "x");
        assert_eq!(last_message(&app), "the list shown isn't a playlist");
        assert_eq!(menu(&app).1, Some(6), "still open, on that item");

        // The rating picker: opened with right, moved through, and left again.
        press(&mut app, "kkkkk");
        key(&mut app, KeyCode::Right);
        assert_eq!(menu(&app), (Some(Sub::Rating), Some(1), Some(0)));
        press(&mut app, "jj");
        key(&mut app, KeyCode::Down);
        press(&mut app, "k");
        assert_eq!(menu(&app).2, Some(2));
        key(&mut app, KeyCode::Up);
        press(&mut app, "9x");
        key(&mut app, KeyCode::Tab);
        assert_eq!(menu(&app), (Some(Sub::Rating), Some(1), Some(1)), "no 9 stars, nor anything else");
        key(&mut app, KeyCode::Left);
        key(&mut app, KeyCode::Tab);
        assert_eq!(menu(&app), (None, Some(1), Some(1)));

        // The colour picker: as far down as it goes; esc goes back, then closes the menu.
        press(&mut app, "c");
        assert_eq!(menu(&app).0, Some(Sub::Colour));
        for _ in 0..12 {
            key(&mut app, KeyCode::Down);
        }
        assert_eq!(menu(&app).2, Some(8));
        key(&mut app, KeyCode::Esc);
        assert_eq!(menu(&app).0, None);
        press(&mut app, "q");
        assert!(app.modal.is_none());

        // A colour by its digit, a rating chosen with enter: each planned.
        press(&mut app, "ec3");
        assert!(app.modal.is_none() && app.job.is_some());
        wait(&mut app, |a| a.job.is_none());
        press(&mut app, "e");
        key(&mut app, KeyCode::Enter);
        assert!(app.job.is_none(), "adding to the crate plans nothing");
        assert_eq!(app.crate_list.len(), 1);
        press(&mut app, "ej\nj\n");
        assert!(app.modal.is_none() && app.job.is_some(), "4 stars, planned");
        wait(&mut app, |a| a.job.is_none());
    }

    /// The prompts for a new folder, a rename and saving the crate: a name that won't do says
    /// what would and stays; one that will is planned.
    #[test]
    fn prompts_say_what_a_name_needs() {
        let (mut app, _dir) = app_with(library());
        let prompt = |app: &App| match &app.modal {
            Some(Modal::Prompt(p)) => (p.label.clone(), p.text.clone()),
            _ => panic!("no prompt: {}", last_message(app)),
        };
        press(&mut app, "eF");
        assert_eq!(prompt(&app), ("New folder".into(), String::new()));
        press(&mut app, "\n");
        assert_eq!(last_message(&app), "give it a name, with any folders before it: Sets/2026/Friday");
        press(&mut app, "Crates\n");
        assert!(app.modal.is_none() && app.job.is_some());
        wait(&mut app, |a| a.job.is_none());

        // Friday, in the tree: renamed (a name, not a path), deleted, and the crate saved beside it.
        let l = app.loaded.as_mut().unwrap();
        l.reveal(&NodeId::Playlist(2));
        let friday = l.tree_position(&NodeId::Playlist(2));
        app.focus = Focus::Playlists;
        app.playlist_state.select(friday);
        press(&mut app, "er");
        assert_eq!(prompt(&app), ("Rename Friday to".into(), "Friday".into()));
        clear_line(&mut app);
        press(&mut app, "Fri/day\n");
        assert_eq!(last_message(&app), "give it a name (without /)");
        clear_line(&mut app);
        press(&mut app, "Saturday\n");
        assert!(app.modal.is_none() && app.job.is_some());
        wait(&mut app, |a| a.job.is_none());
        press(&mut app, "ed");
        assert!(app.modal.is_none() && app.job.is_some(), "deleting it is planned");
        wait(&mut app, |a| a.job.is_none());

        app.focus = Focus::Tracks;
        press(&mut app, "a");
        app.focus = Focus::Playlists;
        press(&mut app, "ew");
        assert_eq!(prompt(&app), ("Save the crate's 1 track as".into(), "Sets/Crate".into()));
        press(&mut app, "\n");
        assert!(app.modal.is_none() && app.job.is_some());
        wait(&mut app, |a| a.job.is_none());
        // Esc puts a prompt away.
        press(&mut app, "eN\u{1b}");
        assert!(app.modal.is_none() && app.job.is_none());
    }

    /// A new My Tag, a smart playlist's path and the playlist to bring back each say what they
    /// need when it's missing, and stay open.
    #[test]
    fn prompts_for_tags_rules_and_restores_say_what_they_need() {
        let (mut app, _dir) = app_with(library());
        let root = app.loaded.as_ref().unwrap().root().to_path_buf();
        let still_asking = |app: &App| assert!(matches!(app.modal, Some(Modal::Prompt(_))), "{}", last_message(app));
        app.tag_chosen(root, vec!["/Contents/1.mp3".into()], super::super::mytags::TagChoice::New);
        clear_line(&mut app);
        press(&mut app, "Deep\n");
        assert!(last_message(&app).starts_with("a My Tag is Category/Tag"));
        still_asking(&app);
        press(&mut app, "\u{1b}");

        press(&mut app, "/bpm:>120\n");
        app.perform(Act::SmartNew, None);
        clear_line(&mut app);
        press(&mut app, "\n");
        assert!(last_message(&app).starts_with("give it a path"));
        still_asking(&app);
        press(&mut app, "\u{1b}");

        press(&mut app, "eb \n");
        assert_eq!(last_message(&app), "name the playlist or folder: Sets/2026/Friday");
        still_asking(&app);
    }

    /// A report scrolls with the keys and closes with any other; a question waits for y or n.
    #[test]
    fn reports_scroll_and_questions_wait_for_an_answer() {
        let (mut app, _dir) = app_with(library());
        let lines = vec![(Tone::Text, "one".to_string())];
        app.modal = Some(Modal::Report(Report { title: "Check".into(), lines, scroll: 0 }));
        let scroll = |app: &App| match &app.modal {
            Some(Modal::Report(r)) => r.scroll,
            _ => panic!("closed"),
        };
        press(&mut app, "jj");
        key(&mut app, KeyCode::PageDown);
        press(&mut app, "k ");
        key(&mut app, KeyCode::PageUp);
        key(&mut app, KeyCode::Up);
        assert_eq!(scroll(&app), 10);
        press(&mut app, "x");
        assert!(app.modal.is_none());

        let ask = |app: &mut App| {
            app.modal = Some(Modal::Ask("Sure?".into(), Box::new(|app: &mut App| app.toast("done it", Level::Info))));
        };
        ask(&mut app);
        press(&mut app, "x");
        assert!(matches!(app.modal, Some(Modal::Ask(..))), "only y or n answers");
        press(&mut app, "n");
        assert!(app.modal.is_none() && app.toast.is_none());
        ask(&mut app);
        press(&mut app, "Y");
        assert_eq!(last_message(&app), "done it");
    }

    /// A plan scrolls while it's shown; n says nothing was written. Asked to write while
    /// another job runs, it waits.
    #[test]
    fn a_confirmation_scrolls_and_waits_for_other_work() {
        let (_dir, root) = demo_drive();
        let mut app = app_on(&root);
        press(&mut app, "e4");
        assert_eq!(confirming(&mut app).0, "edit");
        settle(&mut app);
        press(&mut app, "jj");
        key(&mut app, KeyCode::PageDown);
        press(&mut app, "k");
        key(&mut app, KeyCode::PageUp);
        key(&mut app, KeyCode::Down);
        assert!(matches!(&app.modal, Some(Modal::Confirm(c)) if c.scroll == 2));
        app.job = Some(job(false, false));
        press(&mut app, "y");
        assert_eq!(last_message(&app), "wait for planning to finish");
        app.job = None;
        press(&mut app, "e4");
        settle(&mut app);
        press(&mut app, "n");
        assert!(app.modal.is_none() && app.job.is_none());
        assert_eq!(last_message(&app), "nothing written");
    }

    /// Work asked for while a job runs waits for it; nothing is planned for a drive that was
    /// unplugged, or with no drive open. What comes back for a job that was cancelled is dropped,
    /// and a cancelled plan says nothing.
    #[test]
    fn jobs_one_at_a_time_and_only_for_a_drive_that_is_there() {
        let (mut app, _dir) = app_with(library());
        let root = app.loaded.as_ref().unwrap().root().to_path_buf();
        app.job = Some(job(false, false));
        press(&mut app, "u");
        assert_eq!(last_message(&app), "wait for planning to finish");
        app.task("Checking".into(), |_| Ok(Box::new(|_: &mut App| {})));
        assert_eq!(last_message(&app), "wait for planning to finish");

        app.handle_msg(Msg::Planned { job: 98, root: root.clone(), result: Err(Error::Cancelled) });
        assert!(app.job.is_some(), "another job's plan");
        app.toast = None;
        app.handle_msg(Msg::Planned { job: 99, root: root.clone(), result: Err(Error::Cancelled) });
        assert!(app.job.is_none() && app.toast.is_none());

        app.handle_msg(Msg::Volumes(Vec::new(), Some((root, false))));
        press(&mut app, "u");
        assert!(app.job.is_none());
        assert_eq!(last_message(&app), "the drive was unplugged");
        // A rating by its digit too: the menu stays, and says why.
        press(&mut app, "e3");
        assert!(matches!(app.modal, Some(Modal::Menu(_))) && app.job.is_none());
        assert_eq!(last_message(&app), "the drive was unplugged");

        let (mut no_drive, _store) = blank(View::Library);
        press(&mut no_drive, "u");
        assert_eq!(last_message(&no_drive), "open a drive first");
        press(&mut no_drive, "e");
        assert!(no_drive.modal.is_none());
        assert_eq!(last_message(&no_drive), "open a drive first");
    }

    /// A task that fails says what it was doing; one cancelled says nothing.
    #[test]
    fn a_failed_task_says_what_it_was_doing() {
        let (mut app, _dir) = app_with(library());
        app.task("Checking TEST".into(), |_| Err(Error::Invalid("no export here".into())));
        wait(&mut app, |a| a.job.is_none());
        let said = last_message(&app);
        assert!(said.starts_with("checking test: ") && said.ends_with("no export here"), "{said}");
        app.toast = None;
        app.task("Checking TEST".into(), |_| Err(Error::Cancelled));
        wait(&mut app, |a| a.job.is_none());
        assert!(app.toast.is_none());
    }

    /// Esc while a job runs: planning stops at once; a write stops only until the databases are
    /// being written, and a stopped job's progress no longer shows.
    #[test]
    fn esc_stops_a_job_only_before_it_writes() {
        let (mut app, _dir) = app_with(library());
        app.job = Some(job(true, false));
        app.on_progress(99, "copying · 1.mp3".into(), false);
        press(&mut app, "\u{1b}");
        let j = app.job.as_ref().unwrap();
        assert!(j.cancel.is_cancelled() && j.detail == "cancelling…");
        app.on_progress(99, "copying · 2.mp3".into(), true);
        let j = app.job.as_ref().unwrap();
        assert_eq!((j.detail.as_str(), j.committed), ("cancelling…", true));

        app.job = Some(job(true, true));
        press(&mut app, "\u{1b}");
        assert_eq!(last_message(&app), "writing the drive: too late to stop");
        assert!(!app.job.as_ref().unwrap().cancel.is_cancelled());
        app.job = Some(job(false, false));
        press(&mut app, "\u{1b}");
        assert!(app.job.is_none());
        assert_eq!(last_message(&app), "cancelled");
    }

    /// What applying came to, as the footer says it.
    #[test]
    fn says_what_a_write_came_to() {
        let (mut app, _dir) = app_with(library());
        let root = app.loaded.as_ref().unwrap().root().to_path_buf();
        let drive = app.drive_label(&root);
        let mut applied = |operation: &'static str, report: serde_json::Value| {
            let result = Ok(Applied { operation, report });
            app.handle_msg(Msg::Applied { job: 1, root: root.clone(), result });
            last_message(&app).to_string()
        };
        assert_eq!(
            applied("eject", serde_json::json!([{ "powered_off": true }])),
            format!("ejected {drive}: it can be unplugged")
        );
        assert_eq!(applied("eject", serde_json::json!([{ "powered_off": false }])), format!("unmounted {drive}"));
        let synced = serde_json::json!([{ "name": "A", "error": null }, { "name": "B", "error": null }]);
        assert_eq!(applied("sync.group", synced), "synced 2 drives");
        let failed =
            serde_json::json!([{ "name": "A", "error": null }, { "name": "B", "error": { "message": "it's full" } }]);
        assert_eq!(applied("sync.group", failed), "sync: B: it's full");
        let saved =
            serde_json::json!({ "up_to_date": true, "library": "Main", "snapshot": { "short_id": "3lzabcdefgh" } });
        assert_eq!(applied("pull", saved), format!("{drive} was saved in the store already (Main, 3lzabcde)"));
        let warned = serde_json::json!({ "warnings": ["a track was missing"] });
        assert_eq!(applied("smart.write", warned), format!("smart.write: done on {drive} · a track was missing"));

        let full = Error::NoSpace { path: root.clone(), needed: 10, available: 3 };
        app.handle_msg(Msg::Applied { job: 1, root, result: Err(full) });
        assert_eq!(last_message(&app), format!("not enough free space on {drive} for this change"));
    }

    /// The header follows a job's progress: each phase as it starts, then the files in it (not
    /// more often than it can be read). Cancelling stops the job only until it writes.
    #[test]
    fn progress_is_told_and_cancelling_stops_only_before_writing() {
        let (tx, rx) = channel();
        let (outer, inner) = (CancelToken::new(), CancelToken::new());
        let reporter = Reporter::new(tx, 7, outer.clone(), inner.clone());
        let start = |phase| Event::PhaseStart { phase, items: 0, bytes: 0 };
        let told = || {
            rx.try_iter()
                .map(|m| match m {
                    Msg::Progress { job: 7, detail, committed } => (detail, committed),
                    _ => panic!("not progress"),
                })
                .collect::<Vec<_>>()
        };
        for phase in [Phase::Scan, Phase::Hash, Phase::Copy, Phase::Verify, Phase::Format, Phase::Match] {
            reporter.on_event(&start(phase));
        }
        let names: Vec<String> = told().into_iter().map(|(d, _)| d).collect();
        assert_eq!(names, ["scanning", "hashing", "copying", "verifying", "formatting", "working"]);
        // A file is told as the phase and its name (unless one was told a moment ago); what
        // else happens isn't.
        reporter.on_event(&Event::Item { phase: Phase::Copy, path: "/Contents/a/1.mp3".into(), bytes: 0 });
        reporter.on_event(&Event::Bytes { phase: Phase::Copy, bytes: 10 });
        assert!(told().iter().all(|(d, _)| d == "copying · 1.mp3"));

        outer.cancel();
        reporter.on_event(&Event::Bytes { phase: Phase::Copy, bytes: 10 });
        assert!(inner.is_cancelled(), "not writing yet: it stops");
        let (outer, inner) = (CancelToken::new(), CancelToken::new());
        let (tx, rx) = channel();
        let reporter = Reporter::new(tx, 7, outer.clone(), inner.clone());
        reporter.on_event(&start(Phase::Delete));
        outer.cancel();
        reporter.on_event(&start(Phase::Write));
        assert!(!inner.is_cancelled(), "writing: too late to stop");
        let committed: Vec<bool> = rx.try_iter().map(|m| matches!(m, Msg::Progress { committed: true, .. })).collect();
        assert_eq!(committed, [true, true]);
    }

    /// Packing the crate asks what to call the playlist it's saved as first; a name that won't
    /// do says what would.
    #[test]
    fn packing_the_crate_needs_a_playlist_name() {
        let (mut app, _dir) = app_with(library());
        let other = tempfile::tempdir().unwrap();
        let root = app.loaded.as_ref().unwrap().root().to_path_buf();
        let info = |r: &std::path::Path| locallibrary::Device::open(r).unwrap().info();
        app.drives = vec![info(&root), info(other.path())];
        press(&mut app, "a");
        app.perform(Act::Pack, None);
        press(&mut app, "crate\n\n");
        assert!(matches!(&app.modal, Some(Modal::Prompt(p)) if p.text.starts_with("Sets/")), "{}", last_message(&app));
        clear_line(&mut app);
        press(&mut app, "\n");
        assert_eq!(last_message(&app), "give it a name, with any folders before it: Sets/2026/Friday");
        assert!(matches!(app.modal, Some(Modal::Prompt(_))) && app.job.is_none());
    }
}
