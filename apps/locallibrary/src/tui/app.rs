//! TUI state and input handling. Drawing lives in `ui.rs`, the sidebar tree and sorting in
//! `browse.rs`, the crate in `crate_list.rs`, and changing the drive in `edits.rs`.

use rustc_hash::{FxHashMap, FxHashSet};
use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use locallibrary::anlz::{self, PhraseKind, TrackAnalysis};
use locallibrary::api;
use locallibrary::error::panic_text;
use locallibrary::model::normalize;
use locallibrary::op::AnyPlanned;
use locallibrary::query::{Filter, SortField, SortKey};
use locallibrary::sets::Set;
use locallibrary::store::{LibraryRecord, SnapshotInfo};
use locallibrary::watch::{VolumeWatcher, WatchEvent};
use locallibrary::{Ctx, Device, DeviceInfo, Error, Library, Store, Track, artwork, paths};
use loclib_waveform::{Art, bar_layout};
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::{Position, Rect};
use ratatui::widgets::{ListState, TableState};
use uuid::Uuid;

use super::analyzer::Analyzer;
use super::audio::{self, Audio, Outputs};
use super::browse::{self, Category, Group, Node, NodeId, Problems, SavedNode};
use super::crate_list::{Crate, CrateTrack};
use super::drives::{DriveKey, DriveRow};
use super::edits::{Action, Job, Menu, Modal, Pick};
use super::keymap::{self, Act};
use super::keys::{self, Camelot, Relation};
use super::state::{self, Saved, SavedCrateTrack, SavedQueue};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    Library,
    Drives,
    Snapshots,
    Profile,
    Mixes,
}

impl View {
    pub const ALL: [View; 5] = [View::Library, View::Drives, View::Snapshots, View::Profile, View::Mixes];

    pub fn title(self) -> &'static str {
        match self {
            View::Library => "Library",
            View::Drives => "Drives",
            View::Snapshots => "Snapshots",
            View::Profile => "Profile",
            View::Mixes => "Mixes",
        }
    }

    /// The view a saved title names (`History` was the store's snapshots before they were called
    /// that).
    fn from_title(s: &str) -> Option<View> {
        let s = if s == "History" { "Snapshots" } else { s };
        View::ALL.into_iter().find(|v| v.title() == s)
    }
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

/// How many messages the log (`!`) keeps.
const LOG: usize = 200;

/// Above this many tracks, the search box waits for a pause in typing before narrowing the list.
const TYPE_AHEAD: usize = 5_000;

/// How long an info message stays: five seconds, longer for long ones.
pub fn reading_time(msg: &str) -> Duration {
    Duration::from_secs(5 + (msg.chars().count() as u64).saturating_sub(100) / 20)
}

/// Numbers each library load, so results worked out in the background for one (a file check,
/// a track's matches) aren't applied to another.
static GENERATION: AtomicU64 = AtomicU64::new(1);

/// The drive's My Tags (by lowercase file key, as `Category/Tag`), for `tag:` filters: read in
/// the background the first time a filter asks for them.
#[derive(Default)]
pub enum MyTags {
    #[default]
    Unread,
    Reading,
    Read(Arc<FxHashMap<String, Vec<String>>>),
    Failed(String),
}

/// A drive's library, loaded for browsing.
pub struct Loaded {
    pub device: Device,
    pub library: Arc<Library>,
    pub generation: u64,
    by_id: FxHashMap<u32, usize>,
    by_path: FxHashMap<String, usize>,
    /// Every playlist's path, by id.
    paths: FxHashMap<u32, String>,
    haystack: Vec<String>,
    expanded: FxHashSet<NodeId>,
    pub tree: Vec<Node>,
    /// The list in the track pane: all tracks, a playlist, or one value of a category.
    pub node: NodeId,
    /// Each category's values, worked out when Browse is first opened.
    groups: FxHashMap<Category, Vec<Group>>,
    /// Each track's place under a sort field, worked out when first sorted by it.
    ranks: Vec<(SortField, Vec<u32>)>,
    pub problems: Problems,
    pub my_tags: MyTags,
    /// The store's sets, for Browse › Sets: `None` until first read (see `App::read_sets`).
    sets: Option<Vec<Set>>,
    /// The drive went away: what's shown may be out of date, and can't be played or edited.
    pub stale: bool,
}

impl Loaded {
    /// Index a library for browsing. Runs off the UI thread: it normalises every track's text.
    pub fn prepare(device: Device, library: Library) -> Loaded {
        let by_id = library.tracks.iter().enumerate().map(|(i, t)| (t.id, i)).collect();
        let by_path = library.tracks.iter().enumerate().map(|(i, t)| (t.file_path.clone(), i)).collect();
        let haystack = library.tracks.iter().map(browse::haystack).collect();
        let paths = browse::playlist_paths(&library);
        let (copy_of, copies) = browse::duplicates(&library);
        let mut l = Loaded {
            device,
            library: Arc::new(library),
            generation: GENERATION.fetch_add(1, Ordering::Relaxed),
            by_id,
            by_path,
            paths,
            haystack,
            expanded: FxHashSet::default(),
            tree: Vec::new(),
            node: NodeId::All,
            groups: FxHashMap::default(),
            ranks: Vec::new(),
            problems: Problems { missing: None, copy_of, copies },
            my_tags: MyTags::Unread,
            sets: None,
            stale: false,
        };
        l.rebuild_tree();
        l
    }

    pub fn rebuild_tree(&mut self) {
        if self.expanded.contains(&NodeId::Browse) {
            for c in Category::ALL {
                self.ensure_groups(c);
            }
        }
        self.tree = browse::tree(&self.library, &self.expanded, &self.groups);
    }

    fn ensure_groups(&mut self, c: Category) {
        if !self.groups.contains_key(&c) {
            let tags = match &self.my_tags {
                MyTags::Read(t) => Some(t.clone()),
                _ => None,
            };
            let sets = self.sets.as_deref().unwrap_or_default();
            let g = browse::groups(&self.library, c, &self.by_id, &self.problems, tags.as_deref(), sets);
            self.groups.insert(c, g);
        }
    }

    pub fn group(&mut self, c: Category, key: &str) -> Option<&Group> {
        self.ensure_groups(c);
        self.grouped(c, key)
    }

    /// A value of a category as last grouped (none before it is).
    pub fn grouped(&self, c: Category, key: &str) -> Option<&Group> {
        self.groups.get(&c)?.iter().find(|g| g.key == key)
    }

    /// How many entries of the list shown aren't on the drive (a set's, played from another).
    pub fn left_out(&self) -> usize {
        match &self.node {
            NodeId::Group(c, key) => self.grouped(*c, key).map_or(0, |g| g.left_out),
            _ => 0,
        }
    }

    /// The list shown is a set, and the store's sets haven't been read yet.
    pub fn reading_set(&self) -> bool {
        self.sets.is_none() && matches!(self.node, NodeId::Group(Category::Sets, _))
    }

    /// Open or close a folder (or Browse, or a category).
    pub fn toggle(&mut self, id: &NodeId) {
        if !self.expanded.remove(id) {
            self.expanded.insert(id.clone());
        }
        self.rebuild_tree();
    }

    pub fn rank(&mut self, field: SortField) -> &[u32] {
        let i = match self.ranks.iter().position(|(f, _)| *f == field) {
            Some(i) => i,
            None => {
                self.ranks.push((field, browse::ranks(&self.library, field)));
                self.ranks.len() - 1
            }
        };
        &self.ranks[i].1
    }

    /// The tracks of the list shown (library indexes, in the list's order).
    pub fn base(&mut self) -> Vec<usize> {
        match self.node.clone() {
            NodeId::Playlist(id) => match self.library.playlist(id) {
                Some(p) => p.track_ids.iter().filter_map(|id| self.by_id.get(id).copied()).collect(),
                None => Vec::new(),
            },
            NodeId::Group(c, key) => self.group(c, &key).map(|g| g.tracks.clone()).unwrap_or_default(),
            _ => (0..self.library.tracks.len()).collect(),
        }
    }

    pub fn root(&self) -> &Path {
        self.device.root()
    }

    pub fn index_of(&self, file_path: &str) -> Option<usize> {
        self.by_path.get(file_path).copied()
    }

    /// How a node is named in titles: `Sets › Friday`, `Key › 8A Am`.
    pub fn node_title(&self, id: &NodeId) -> String {
        match id {
            NodeId::All => "All tracks".into(),
            NodeId::Playlist(id) => self.playlist_path(*id).replace('/', " › "),
            NodeId::Browse => "Browse".into(),
            NodeId::Category(c) => c.name().into(),
            NodeId::Group(c, key) => {
                let label = self.grouped(*c, key).map(|g| g.label.trim());
                format!("{} › {}", c.name(), label.unwrap_or(key))
            }
        }
    }

    pub fn playlist_name(&self) -> String {
        self.node_title(&self.node)
    }

    /// A playlist's path (`Sets/Friday`).
    pub fn playlist_path(&self, id: u32) -> String {
        self.paths.get(&id).cloned().unwrap_or_default()
    }

    pub fn save_node(&self, id: &NodeId) -> SavedNode {
        match id {
            NodeId::All => SavedNode::All,
            NodeId::Browse => SavedNode::Browse,
            NodeId::Playlist(id) => SavedNode::Playlist { path: self.playlist_path(*id) },
            NodeId::Category(c) => SavedNode::Category { category: *c },
            NodeId::Group(c, key) => SavedNode::Group { category: *c, key: key.clone() },
        }
    }

    /// A remembered node in this library, if it's still there.
    pub fn find_node(&self, s: &SavedNode) -> Option<NodeId> {
        Some(match s {
            SavedNode::All => NodeId::All,
            SavedNode::Browse => NodeId::Browse,
            SavedNode::Category { category } => NodeId::Category(*category),
            SavedNode::Group { category, key } => NodeId::Group(*category, key.clone()),
            SavedNode::Playlist { path } => {
                NodeId::Playlist(self.library.playlists.iter().find(|p| self.paths.get(&p.id) == Some(path))?.id)
            }
        })
    }

    /// Open the folders above a node so it shows in the tree.
    pub fn reveal(&mut self, id: &NodeId) {
        match id {
            NodeId::Playlist(pid) => {
                let mut cur = self.library.playlist(*pid).and_then(|p| p.parent_id);
                for _ in 0..64 {
                    let Some(f) = cur else { break };
                    self.expanded.insert(NodeId::Playlist(f));
                    cur = self.library.playlist(f).and_then(|p| p.parent_id);
                }
            }
            NodeId::Category(_) => {
                self.expanded.insert(NodeId::Browse);
            }
            NodeId::Group(c, _) => {
                self.expanded.insert(NodeId::Browse);
                self.expanded.insert(NodeId::Category(*c));
            }
            NodeId::All | NodeId::Browse => {}
        }
        self.rebuild_tree();
    }

    pub fn tree_position(&self, id: &NodeId) -> Option<usize> {
        self.tree.iter().position(|n| &n.id == id)
    }

    fn set_missing(&mut self, missing: Vec<bool>) {
        self.problems.missing = Some(missing);
        self.groups.remove(&Category::Problems);
        self.rebuild_tree();
    }
}

/// Tracks that mix into one track (from `harmony::suggestions`), as indexes into the library they
/// were found in, which they keep.
pub struct Matches {
    pub library: Arc<Library>,
    pub root: PathBuf,
    pub items: Vec<Mix>,
}

pub struct Mix {
    pub index: usize,
    pub key: Camelot,
    pub relation: Relation,
    pub tempo_gap: f64,
}

impl Matches {
    pub(super) fn find(library: Arc<Library>, root: PathBuf, track: &Track, limit: usize) -> Matches {
        let items = keys::suggestions(&library, track, limit)
            .into_iter()
            .filter_map(|s| {
                let index = library.tracks.iter().position(|t| t.id == s.track.id)?;
                Some(Mix { index, key: s.key, relation: s.relation, tempo_gap: s.tempo_gap })
            })
            .collect();
        Matches { library, root, items }
    }

    pub fn track(&self, m: &Mix) -> &Track {
        &self.library.tracks[m.index]
    }
}

/// Playlists holding a track (any entry for its file), by path, sorted.
fn playlists_of(lib: &Library, track: &Track) -> Vec<String> {
    let ids: Vec<u32> = lib.tracks.iter().filter(|t| t.file_path == track.file_path).map(|t| t.id).collect();
    let mut v: Vec<String> = lib
        .playlists
        .iter()
        .filter(|p| !p.is_folder && p.track_ids.iter().any(|id| ids.contains(id)))
        .map(|p| lib.playlist_path(p.id).replace('/', " › "))
        .collect();
    v.sort();
    v
}

/// The selected track's matches and playlists, found in the background.
pub struct Details {
    pub generation: u64,
    pub index: usize,
    pub matches: Matches,
    pub playlists: Vec<String>,
}

/// What starting a track reads in the background: its analysis files and artwork from the
/// drive, and its matches and playlists from the library.
#[derive(Default)]
pub struct TrackInfo {
    analysis: TrackAnalysis,
    art: Option<Art>,
    matches: Option<Matches>,
    playlists: Vec<String>,
}

fn track_info(root: &Path, track: &Track, library: Option<Arc<Library>>) -> TrackInfo {
    let device = Device::open(root).ok();
    let analysis = device.as_ref().map(|d| anlz::load(d, track)).unwrap_or_default();
    let art = (device.as_ref().and_then(|d| artwork::file_on_drive(d, track)))
        .and_then(|f| std::fs::read(f).ok())
        .and_then(|b| Art::decode(&b));
    let playlists = library.as_deref().map(|lib| playlists_of(lib, track)).unwrap_or_default();
    let matches = library.map(|lib| Matches::find(lib, root.to_path_buf(), track, 40));
    TrackInfo { analysis, art, matches, playlists }
}

pub struct NowPlaying {
    /// Matches the background read of the track's files to this play.
    pub id: u64,
    pub track: Track,
    pub drive: String,
    pub root: PathBuf,
    pub analysis: TrackAnalysis,
    pub art: Option<Art>,
    pub duration: Duration,
    pub key: Option<Camelot>,
    /// rekordbox's phrases, placed in time with the beat grid.
    pub phrases: Vec<PhraseSpan>,
    /// Tracks on the same drive that mix into this one (best first). `None` when the drive's
    /// library wasn't open.
    pub matches: Option<Matches>,
    /// Playlists on the drive that contain the track.
    pub playlists: Vec<String>,
    /// The analysis, artwork and matches are still being read.
    pub loading: bool,
    /// The audio file is still being opened.
    pub opening: bool,
    /// Started by moving through the queue: if it won't play, the queue moves on past it.
    pub advancing: bool,
}

pub struct PhraseSpan {
    pub kind: PhraseKind,
    pub start_ms: f64,
    pub end_ms: f64,
    /// Length in bars of four beats.
    pub bars: u16,
}

/// Phrases with times: a phrase's beats are 1-based indexes into the grid; one that runs past the
/// grid ends with the track. One that doesn't end after it starts isn't drawn (the reader drops
/// those, but made-up data can hold them).
fn phrase_spans(analysis: &TrackAnalysis, duration: Duration) -> Vec<PhraseSpan> {
    let Some(st) = &analysis.structure else { return Vec::new() };
    let at = |beat: u16| analysis.beats.get(beat as usize - 1).map(|b| b.time_ms as f64);
    let end_of_track = duration.as_secs_f64() * 1000.0;
    st.phrases
        .iter()
        .filter(|p| p.start_beat > 0 && p.end_beat > p.start_beat)
        .filter_map(|p| {
            Some(PhraseSpan {
                kind: p.kind,
                start_ms: at(p.start_beat)?,
                end_ms: at(p.end_beat).unwrap_or(end_of_track),
                bars: (p.end_beat - p.start_beat).div_ceil(4),
            })
        })
        .collect()
}

impl NowPlaying {
    /// The phrase playing at `ms`, its index, and the bar within it (from 1).
    pub fn phrase_at(&self, ms: f64) -> Option<(usize, &PhraseSpan, u16)> {
        let i = self.phrases.iter().position(|p| p.start_ms <= ms && ms < p.end_ms)?;
        let p = &self.phrases[i];
        let beats = self.analysis.beats.iter().filter(|b| b.time_ms as f64 >= p.start_ms && b.time_ms as f64 <= ms);
        Some((i, p, (beats.count().saturating_sub(1) / 4 + 1) as u16))
    }

    /// Hot cues and memory cues, in time order.
    pub fn cue_times(&self) -> Vec<u32> {
        let mut t: Vec<u32> = self.analysis.cues.iter().map(|c| c.start_ms).collect();
        t.sort_unstable();
        t.dedup();
        t
    }

    /// The beat at `ms`: (index into the grid, beat in bar 1-4, bar number from 1).
    pub fn beat_at(&self, ms: f64) -> Option<(usize, u16, usize)> {
        let beats = &self.analysis.beats;
        let i = beats.partition_point(|b| b.time_ms as f64 <= ms).checked_sub(1)?;
        let bars = beats[..=i].iter().filter(|b| b.beat_number == 1).count().max(1);
        Some((i, beats[i].beat_number, bars))
    }
}

/// What plays next: tracks of one library, by index, so queueing a list of thousands copies
/// none of them.
#[derive(Default)]
pub struct Queue {
    pub library: Arc<Library>,
    /// The tracks, as indexes into `library`, in the order of the list they came from.
    pub items: Vec<usize>,
    pub root: PathBuf,
    pub drive: String,
    /// Play order (indexes into `items`), shuffled or not.
    pub order: Vec<usize>,
    /// Position in `order`.
    pub pos: usize,
}

impl Queue {
    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Track `i` of the list (not of the play order).
    pub fn track(&self, i: usize) -> &Track {
        &self.library.tracks[self.items[i]]
    }
}

/// The track after the playing one, opened ahead so the player goes straight on to it.
struct Upcoming {
    now: NowPlaying,
    /// Its place in the queue's play order.
    pos: usize,
    /// Queued behind the playing track (else it's opening, or wouldn't open).
    queued: bool,
    /// Opened, but not to be queued (another sample rate): started as the playing one ends.
    file: Option<audio::OpenedFile>,
}

/// How long before the end of a track the next one is opened, to follow it without a gap.
const AHEAD: Duration = Duration::from_secs(30);

/// Opening a track, on the [`Opener`]'s worker.
type OpenJob = Box<dyn FnOnce(&Opening) + Send>;

/// Opens the tracks played (their audio files, then their analysis, artwork and matches) on a
/// worker of its own, one at a time. Starting a track drops what was asked for before it and
/// hasn't been done (holding `n` asks for dozens a second), and a file opened for a track no
/// longer wanted is closed rather than handed over: an eject waits for the worker (`idle`), so
/// no file on the drive is left open.
#[derive(Default)]
struct Opener {
    jobs: Option<Sender<(u64, u64, OpenJob)>>,
    /// Bumped by each track started and each stop: work asked for before then is dropped.
    current: Arc<Mutex<u64>>,
    /// Held while a track is being opened.
    idle: Arc<Mutex<()>>,
}

/// What an open on the [`Opener`]'s worker is for, and where its results go.
struct Opening {
    generation: u64,
    current: Arc<Mutex<u64>>,
    tx: Sender<Msg>,
}

impl Opening {
    /// Whether what it's for is still wanted.
    fn wanted(&self) -> bool {
        *self.current.lock().unwrap_or_else(PoisonError::into_inner) == self.generation
    }

    /// Send `msg` while what it's for is still wanted (else it's dropped here, closing any file
    /// in it). Returns whether it was sent.
    fn send(&self, msg: Msg) -> bool {
        let current = self.current.lock().unwrap_or_else(PoisonError::into_inner);
        *current == self.generation && self.tx.send(msg).is_ok()
    }
}

impl Opener {
    /// Open track `id` with `job`; `starts`: it's the track to play now, so whatever was asked
    /// for before it is dropped (the next track, opened ahead, isn't).
    fn open(&mut self, tx: &Sender<Msg>, id: u64, starts: bool, job: OpenJob) {
        let generation = {
            let mut current = self.current.lock().unwrap_or_else(PoisonError::into_inner);
            *current += u64::from(starts);
            *current
        };
        let jobs = self.jobs.get_or_insert_with(|| {
            let (jobs, rx) = channel::<(u64, u64, OpenJob)>();
            let (current, idle, tx) = (self.current.clone(), self.idle.clone(), tx.clone());
            std::thread::spawn(move || {
                for (generation, id, job) in rx {
                    let _busy = idle.lock().unwrap_or_else(PoisonError::into_inner);
                    let opening = Opening { generation, current: current.clone(), tx: tx.clone() };
                    if !opening.wanted() {
                        continue;
                    }
                    if let Err(e) = caught(|| job(&opening)) {
                        opening.send(Msg::Opened(id, Err(e)));
                        opening.send(Msg::TrackInfo(id, Box::default()));
                    }
                }
            });
            jobs
        });
        let _ = jobs.send((generation, id, job));
    }

    /// Drop every track asked for that isn't open yet.
    fn drop_all(&self) {
        *self.current.lock().unwrap_or_else(PoisonError::into_inner) += 1;
    }
}

/// How long an opened file plays: what the decoder says, unless that's far from what rekordbox
/// says (some decoders guess from the start of a VBR file).
fn file_duration(track: &Track, file: &audio::OpenedFile) -> Duration {
    let rekordbox = Duration::from_secs(track.duration_secs as u64);
    match file.duration {
        Some(d) if track.duration_secs == 0 || d.abs_diff(rekordbox) < Duration::from_secs(5) => d,
        _ => rekordbox,
    }
}

/// Sort ranks and Browse's groups, worked out in the background once a library is shown, so
/// the first sort by a column and opening Browse don't hold up the screen.
pub struct Index {
    ranks: Vec<(SortField, Vec<u32>)>,
    groups: FxHashMap<Category, Vec<Group>>,
}

/// Where things were drawn last frame, for mouse hit-testing.
#[derive(Default)]
pub struct Areas {
    pub tabs: Vec<(View, Rect)>,
    pub drives: Rect,
    pub playlists: Rect,
    pub tracks: Rect,
    /// The track list's column headers, to sort by clicking one.
    pub columns: Vec<(SortField, Rect)>,
    pub details: Rect,
    /// The rows of the details panel's list of matches.
    pub matches: Rect,
    pub crate_panel: Rect,
    /// The rows of the crate.
    pub crate_rows: Rect,
    /// The actions menu, when open.
    pub menu: Rect,
    pub overview: Rect,
    /// Every spectrum drawn: width and whether it's mirrored (each needs its own bands).
    pub spectra: Vec<(u16, bool)>,
    pub drive_list: Rect,
    pub history: Rect,
}

/// A row clicked: its view, its pane (none for the full-screen player's matches) and its index.
type ClickedRow = (View, Option<Focus>, usize);

/// Every library in the store with its snapshots, newest first.
pub type History = Vec<(LibraryRecord, Vec<SnapshotInfo>)>;

/// The volume watcher, shared with the threads polling it. It starts with the first read of
/// the drive list (see `App::refresh_drives`).
type Watcher = Arc<std::sync::Mutex<Option<VolumeWatcher>>>;

pub enum Msg {
    /// What the volume watcher noticed since its last poll (drives coming and going), and
    /// whether the open drive is still there.
    Volumes(Vec<WatchEvent>, Option<(PathBuf, bool)>),
    /// The drives.
    Drives(Vec<DeviceInfo>),
    /// A drive's library read, for the load with this number (see `App::load`).
    Library(u64, PathBuf, Result<Box<Loaded>, String>),
    /// Which tracks' audio files are missing, for the library load with this generation.
    Missing(u64, Vec<bool>),
    /// The drive's My Tags, for the library load with this generation.
    MyTags(u64, Result<FxHashMap<String, Vec<String>>, String>),
    /// Sort ranks and Browse's groups, for the library load with this generation.
    Indexed(u64, Box<Index>),
    /// The store's sets, for the library load with this generation.
    Sets(u64, Result<Vec<Set>, String>),
    /// A track's audio file opened (or not), for the play with this id.
    Opened(u64, Result<audio::OpenedFile, String>),
    /// Something background work has to say (a report written, say).
    Note(String, Level),
    /// The playing track's analysis, artwork and matches, for the play with this id.
    TrackInfo(u64, Box<TrackInfo>),
    Details(Box<Details>),
    Status(Result<api::FleetStatus, String>),
    History(Result<History, String>),
    /// A profile build: which one, and its result.
    Profile(u64, Box<Result<api::ProfileView, String>>),
    /// Signing in to (or out of) the Atmosphere account.
    #[cfg(feature = "account")]
    Account(super::account::AccountEvent),
    /// A recorded set identified: which identification, and its result.
    Mix(u64, Box<Result<super::mix::Opened, String>>),
    /// The recordings on a drive, from which listing of it.
    MixSources(u64, PathBuf, Vec<super::mix::Source>),
    /// The recordings in the folders opened in the Mixes view (which ones, and what's in them).
    MixFolders(Vec<PathBuf>, Vec<super::mix::Source>),
    Planned {
        job: u64,
        root: PathBuf,
        result: Result<Box<dyn AnyPlanned>, Error>,
    },
    Applied {
        job: u64,
        root: PathBuf,
        result: Result<super::edits::Applied, Error>,
    },
    /// Whether replaying the open drive's recorded edits would change it, for the library load
    /// with this generation: how many edits are recorded, 0 when replaying changes nothing.
    Replay(u64, usize),
    /// A background task (a check) finished: what it hands back finishes on the UI thread.
    Done {
        job: u64,
        result: Result<super::edits::Done, Error>,
    },
    Progress {
        job: u64,
        detail: String,
        committed: bool,
    },
    /// Work in the background failed (it panicked: see [`worker`]), and what went wrong.
    Failed(Work, String),
}

/// Background work that something waits for, to stop waiting when it fails.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Work {
    Drives,
    Volumes,
    /// The details of the track at this index, in the library load with this generation.
    Details(u64, usize),
}

thread_local! {
    /// What a panic on this thread said and where, while [`caught`] runs on it (kept by the panic
    /// hook `tui::run` installs).
    static PANIC: RefCell<Option<String>> = const { RefCell::new(None) };
    /// Whether [`caught`] is running on this thread, to report a panic here itself.
    static CATCHING: Cell<bool> = const { Cell::new(false) };
}

/// The last panic on a thread [`caught`] isn't running on, and when: one of the library's own
/// workers, whose panic the thread that started them re-raises (saying less) or turns into an
/// error once the worker has ended.
static ELSEWHERE: Mutex<Option<(Instant, String)>> = Mutex::new(None);

/// Keep what a panic said and where, for [`caught`] to report (the panic hook can't draw).
pub fn note_panic(info: &std::panic::PanicHookInfo) {
    let at = info.location().map(|l| format!(" ({}:{})", l.file(), l.line())).unwrap_or_default();
    note(format!("{}{at}", panic_text(info.payload())));
}

/// Keep `said` for the [`caught`] running on this thread, or (none is) for the next to fail.
fn note(said: String) {
    if CATCHING.get() {
        PANIC.set(Some(said));
    } else {
        *ELSEWHERE.lock().unwrap_or_else(PoisonError::into_inner) = Some((Instant::now(), said));
    }
}

/// Run `f`, turning a panic in it (a bug, or a file the library trips over) into what it said.
/// When one of the library's own threads panicked meanwhile, that's what's said: the panic here
/// only re-raises it.
pub(super) fn caught<T>(f: impl FnOnce() -> T) -> Result<T, String> {
    let since = Instant::now();
    let outer = CATCHING.replace(true);
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f));
    CATCHING.set(outer);
    result.map_err(|payload| {
        let here = PANIC.take();
        let elsewhere = ELSEWHERE.lock().unwrap_or_else(PoisonError::into_inner).take_if(|(at, _)| *at >= since);
        let said = elsewhere.map(|(_, s)| s).or(here).unwrap_or_else(|| panic_text(&*payload));
        format!("something went wrong (a bug, please report it): {said}")
    })
}

/// Run `job` on a worker thread and send the UI what it comes to; should it panic, `failed` with
/// what went wrong, so whatever waits for it (a busy flag, a write) doesn't wait forever.
pub(super) fn worker(
    tx: Sender<Msg>,
    job: impl FnOnce() -> Msg + Send + 'static,
    failed: impl FnOnce(String) -> Msg + Send + 'static,
) {
    std::thread::spawn(move || {
        let _ = tx.send(caught(job).unwrap_or_else(failed));
    });
}

/// rekordbox exported over edits made to the open drive: replaying them is offered (`y`).
pub struct ReplayOffer {
    pub root: PathBuf,
    pub drive: String,
    /// The recorded edits (journal entries).
    pub edits: usize,
}

/// The view to come back to when the open drive is read again (after an edit, say).
struct ViewKeep {
    node: SavedNode,
    expanded: Vec<SavedNode>,
    tree_row: Option<SavedNode>,
    track: Option<String>,
    row: usize,
    offset: usize,
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
    pub details: Option<Details>,
    pub details_state: ListState,
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
    extra_roots: Vec<PathBuf>,
    /// Look at no drives but those it's given: the mounted ones aren't listed, nor their status
    /// read (tests, which mustn't touch the machine's drives).
    offline: bool,
    /// Whether the drive list has been read: the first read announces nothing.
    listed: bool,
    /// Notices drives coming and going: a poll that finds any reads the list again.
    watcher: Watcher,
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
        app.outputs = Outputs::DEVICES;
        app.apply_saved(&saved, drive.is_none());
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
            details: None,
            details_state: ListState::default(),
            details_seen: None,
            details_asked: None,
            details_failed: None,
            modal: None,
            waiting: VecDeque::new(),
            picking: None,
            job: None,
            last_backup: None,
            status: None,
            status_loading: false,
            fleet_libraries: Vec::new(),
            library_names: FxHashMap::default(),
            drive_header: None,
            drive_rows: ListState::default(),
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
            queue: Queue::default(),
            shuffle: false,
            repeat: Repeat::Off,
            analyzer: Analyzer::default(),
            deck: Default::default(),
            vis: Vis::Bars,
            deck_secs: 8.0,
            flash: None,
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
        let device = Device::open(root).unwrap();
        let root = device.root().to_path_buf();
        app.install(root, Loaded::prepare(device, lib));
        app
    }

    fn apply_saved(&mut self, s: &Saved, open_last_drive: bool) {
        self.sort = s.sort;
        self.search_all = s.search_all;
        self.show_details = s.details;
        self.show_crate = s.crate_panel;
        self.show_ratings = s.ratings;
        self.show_tags_column = s.tags_column;
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
    fn saved_state(&self) -> Saved {
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
    fn autosave(&mut self) {
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
    fn restore_queue(&mut self, root: &Path) {
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

    /// Run `job` in the background (see [`worker`]); should it panic, the UI says so.
    pub(super) fn spawn(&self, job: impl FnOnce() -> Msg + Send + 'static) {
        self.spawn_or(job, |e| Msg::Note(e, Level::Error));
    }

    /// [`Self::spawn`] for work something waits for: should it panic, `failed` is sent in place
    /// of its message.
    pub(super) fn spawn_or(
        &self,
        job: impl FnOnce() -> Msg + Send + 'static,
        failed: impl FnOnce(String) -> Msg + Send + 'static,
    ) {
        worker(self.tx.clone(), job, failed);
    }

    pub(super) fn sender(&self) -> Sender<Msg> {
        self.tx.clone()
    }

    pub(super) fn next_id(&mut self) -> u64 {
        self.next_id += 1;
        self.next_id
    }

    /// Say something in the footer (and the log).
    pub fn toast(&mut self, msg: impl Into<String>, level: Level) {
        let msg = msg.into();
        if self.log.len() >= LOG {
            self.log.pop_front();
        }
        self.log.push_back((msg.clone(), level, Instant::now()));
        // An error that wasn't dismissed stays on screen, above what came after it.
        if let Some((m, Level::Error, _)) = self.toast.take()
            && m != msg
        {
            self.held = Some(m);
        }
        self.toast = Some((msg, level, Instant::now()));
    }

    /// Esc with an error on screen: put it away. Returns whether there was one.
    fn dismiss_errors(&mut self) -> bool {
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

    /// Full drive details (this reads every drive's database).
    pub(super) fn refresh_drives(&mut self) {
        if self.offline {
            return;
        }
        if self.drives_in_flight {
            self.drives_again = true;
            return;
        }
        self.drives_in_flight = true;
        let extra = self.extra_roots.clone();
        let watcher = self.watcher.clone();
        let job = move || {
            // Watching starts before the list is read: a drive mounted while it's read shows
            // as a change at the next poll.
            watcher.lock().unwrap_or_else(PoisonError::into_inner).get_or_insert_with(VolumeWatcher::new);
            let mut list = api::list_drives(&api::ListDrives { all: false });
            for root in extra {
                if !list.iter().any(|d| d.root == root)
                    && let Ok(info) = api::drive_info(&api::DriveRef { drive: root.display().to_string() })
                {
                    list.push(info);
                }
            }
            Msg::Drives(list)
        };
        self.spawn_or(job, |e| Msg::Failed(Work::Drives, e));
    }

    pub(super) fn refresh_status(&mut self) {
        if self.status_loading || self.offline {
            return;
        }
        self.status_loading = true;
        let root = self.store_root.clone();
        let job = move || {
            let r = Store::open(&root)
                .map_err(|_| "no store yet: saving a snapshot of a drive (e) makes one".to_string())
                .and_then(|s| {
                    let req = api::FleetRequest::default();
                    api::fleet_status(Some(&s), &req, &Ctx::silent()).map_err(|e| e.to_string())
                });
            Msg::Status(r)
        };
        self.spawn_or(job, |e| Msg::Status(Err(e)));
    }

    fn refresh_history(&mut self) {
        let root = self.store_root.clone();
        let job = move || {
            let r = (|| -> Result<_, String> {
                let store = Store::open(&root)
                    .map_err(|_| "no store yet: `locallibrary pull <drive>` creates one".to_string())?;
                let mut out = Vec::new();
                for lib in store.libraries().map_err(|e| e.to_string())? {
                    let req = api::HistoryRequest { library: lib.name.clone(), limit: Some(200) };
                    let snaps = api::history(&store, &req).map_err(|e| e.to_string())?;
                    out.push((lib, snaps));
                }
                Ok(out)
            })();
            Msg::History(r)
        };
        self.spawn_or(job, |e| Msg::History(Err(e)));
    }

    /// Read a drive's library in the background. Reading the open drive again keeps the view.
    /// Opening one drive and then another shows the other, even if the first takes longer.
    pub(super) fn load_library(&mut self, root: PathBuf) {
        let name = root.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        self.read_from(name, root.clone(), move || root);
    }

    /// Read `name` in the background from the drive `pick` answers (`first` when picking fails),
    /// and show it.
    pub(super) fn read_from(&mut self, name: String, first: PathBuf, pick: impl FnOnce() -> PathBuf + Send + 'static) {
        self.loading = Some(format!("Reading {name}…"));
        self.load += 1;
        let load = self.load;
        let job = move || {
            let root = pick();
            let drive = api::DriveRef { drive: root.display().to_string() };
            let r = api::drive_library(&drive)
                .and_then(|view| Ok(Box::new(Loaded::prepare(Device::open_export(&root)?, view.library))));
            Msg::Library(load, root, r.map_err(|e| e.to_string()))
        };
        self.spawn_or(job, move |e| Msg::Library(load, first, Err(e)));
    }

    /// Look for tracks whose audio file is missing, in the background.
    fn check_files(&mut self) {
        let Some(l) = &self.loaded else { return };
        let (lib, root, generation) = (l.library.clone(), l.root().to_path_buf(), l.generation);
        self.spawn(move || {
            let missing = match Device::open(&root) {
                Ok(d) => lib
                    .tracks
                    .iter()
                    .map(|t| !d.checked_path(&paths::key(&t.file_path)).is_ok_and(|p| p.is_file()))
                    .collect(),
                Err(_) => Vec::new(),
            };
            Msg::Missing(generation, missing)
        });
    }

    /// Background results, playback progress, animation.
    pub fn tick(&mut self) {
        while let Ok(msg) = self.rx.try_recv() {
            self.handle_msg(msg);
        }
        self.show_waiting();
        if !self.volumes_in_flight && self.last_volume_poll.elapsed() > Duration::from_secs(1) {
            self.last_volume_poll = Instant::now();
            self.volumes_in_flight = true;
            let open = self.loaded.as_ref().map(|l| l.root().to_path_buf());
            let watcher = self.watcher.clone();
            let job = move || {
                let present = open.map(|r| {
                    let ok = Device::open(&r).is_ok_and(|d| d.has_export());
                    (r, ok)
                });
                let mut watcher = watcher.lock().unwrap_or_else(PoisonError::into_inner);
                Msg::Volumes(watcher.as_mut().map(VolumeWatcher::poll).unwrap_or_default(), present)
            };
            self.spawn_or(job, |e| Msg::Failed(Work::Volumes, e));
        }
        if let Some((msg, Level::Info, at)) = &self.toast
            && at.elapsed() > reading_time(msg)
        {
            self.toast = None;
        }
        if self.search_due.is_some_and(|at| Instant::now() >= at) {
            self.flush_search();
        }
        self.autosave();
        self.watch_output();
        self.follow_queue();
        self.want_details();

        let dt = self.last_tick.elapsed().as_secs_f32().min(0.25);
        self.last_tick = Instant::now();
        let mut counts: Vec<usize> = self.areas.spectra.iter().map(|&(w, m)| bar_layout(w, m).0).collect();
        counts.dedup();
        if let Some(a) = &self.audio {
            let playing = a.is_loaded() && !a.is_paused();
            self.analyzer.update(&a.tap, playing, &counts, dt);
        }

        // Flash the visualiser in a cue's colour as the playhead crosses it (not when seeking past).
        let pos_ms = self.position().as_secs_f64() * 1000.0;
        let prev = self.last_pos_ms;
        let crossed = self.now.as_ref().filter(|_| pos_ms > prev && pos_ms - prev < 500.0).and_then(|n| {
            n.analysis
                .cues
                .iter()
                .filter(|c| (c.start_ms as f64) > prev && (c.start_ms as f64) <= pos_ms)
                .max_by_key(|c| c.hot_cue.is_some())
                .map(|c| super::theme::palette().cue_color(c))
        });
        if let Some(color) = crossed {
            self.flash = Some((color, Instant::now()));
        }
        self.last_pos_ms = pos_ms;
    }

    /// The cue flash's colour and strength (1 fading to 0). None with reduced motion.
    pub fn flash(&self) -> Option<([u8; 3], f32)> {
        let (color, at) = self.flash.filter(|_| !super::theme::reduced_motion())?;
        let t = 1.0 - at.elapsed().as_secs_f32() / 0.9;
        (t > 0.0).then_some((color, t * t))
    }

    /// Seconds of track the cue lane shows: zoomed together with the deck.
    pub fn lane_secs(&self) -> f64 {
        self.deck_secs * 4.0
    }

    /// Whether the screen needs redrawing at animation rate.
    pub fn animating(&self) -> bool {
        self.loading.is_some()
            || self.search_due.is_some()
            || self.status_loading
            || self.profile.loading
            || self.profile.account.busy()
            || self.mix.is_loading()
            || self.job.is_some()
            || self.details_asked.is_some()
            || (self.show_details
                && self.view == View::Library
                && self.selected_track().is_some()
                && !self.details_current()
                && !self.details_lookup_failed())
            || self.now.as_ref().is_some_and(|n| n.loading)
            || self.loaded.as_ref().is_some_and(Loaded::reading_set)
            || !self.analyzer.is_idle()
            || self.is_playing()
            || self.flash().is_some()
    }

    pub fn is_playing(&self) -> bool {
        self.audio.as_ref().is_some_and(|a| a.is_loaded() && !a.is_paused())
    }

    /// Where the playing track is (with its output gone, where it waits to play on from).
    pub fn position(&self) -> Duration {
        match &self.audio {
            Some(a) => a.position(),
            None => self.resume.filter(|_| self.now.is_some()).unwrap_or_default(),
        }
    }

    pub(super) fn handle_msg(&mut self, msg: Msg) {
        match msg {
            Msg::Volumes(events, present) => {
                self.volumes_in_flight = false;
                self.volumes_failing = false;
                // A drive mounted while a read is in flight is read once that one is done.
                if events.iter().any(|e| matches!(e, WatchEvent::Attached(_) | WatchEvent::Detached(_))) {
                    self.refresh_drives();
                }
                self.check_present(present);
            }
            Msg::Drives(list) => {
                self.drives_in_flight = false;
                let first = !std::mem::replace(&mut self.listed, true);
                let selected = self.drive_state.selected().and_then(|i| self.drives.get(i)).map(|d| d.root.clone());
                let changed = list.iter().map(|d| &d.root).ne(self.drives.iter().map(|d| &d.root));
                let added: Vec<PathBuf> = list
                    .iter()
                    .filter(|d| !first && !self.drives.iter().any(|o| o.root == d.root))
                    .map(|d| d.root.clone())
                    .collect();
                self.drives = list;
                self.library_names = (Store::open(&self.store_root).and_then(|s| s.libraries()).unwrap_or_default())
                    .into_iter()
                    .map(|l| (l.id, l.name))
                    .collect();
                if self.drive_header.is_some_and(|h| !self.drive_rows().contains(&h)) {
                    self.drive_header = None;
                }
                // The drive changed last has gone (ejected, unplugged): `u` goes back to the open or
                // selected drive. Mounted somewhere else since, it's followed there.
                let last = self.last_backup.take().and_then(|k| Some(DriveKey { root: self.mounted(&k)?, ..k }));
                self.last_backup = last;
                if !added.is_empty() {
                    let names: Vec<String> = added.iter().map(|r| self.drive_label(r)).collect();
                    self.toast(format!("{} plugged in", names.join(", ")), Level::Info);
                }
                let idx = selected
                    .and_then(|r| self.drives.iter().position(|d| d.root == r))
                    .or(if self.drives.is_empty() { None } else { Some(0) });
                self.drive_state.select(idx);
                if changed && self.status.is_some() {
                    self.refresh_status();
                }
                // Open a library while none is (on startup, or when the first drive is plugged in):
                // the drive open last time when it's here, else the first library (its lead
                // clone), else the first drive with an export.
                if self.loaded.is_none() && self.loading.is_none() {
                    let with_export = |r: &PathBuf| self.drives.iter().any(|d| d.has_export && d.root == *r);
                    let last = self.preferred_root.clone().filter(with_export);
                    let library = self.drive_rows().into_iter().find_map(|r| match r {
                        DriveRow::Library(id) => Some(id),
                        _ => None,
                    });
                    let first = self.drives.iter().find(|d| d.has_export).map(|d| d.root.clone());
                    match (last, library, first) {
                        (Some(root), ..) | (None, None, Some(root)) => {
                            self.preferred_root = None;
                            self.drive_state.select(self.drives.iter().position(|x| x.root == root));
                            self.load_library(root);
                        }
                        (None, Some(id), _) => {
                            self.preferred_root = None;
                            self.open_library(id);
                        }
                        (None, None, None) => {}
                    }
                }
                if std::mem::take(&mut self.drives_again) {
                    self.refresh_drives();
                }
            }
            Msg::Library(load, root, r) => {
                if load != self.load {
                    return; // another drive (or the same one again) was asked for since
                }
                self.loading = None;
                match r {
                    Ok(b) => self.install(root, *b),
                    Err(e) => self.toast(e, Level::Error),
                }
            }
            Msg::Missing(generation, missing) => {
                if let Some(l) = self.loaded.as_mut().filter(|l| l.generation == generation)
                    && missing.len() == l.library.tracks.len()
                {
                    l.set_missing(missing);
                }
            }
            Msg::MyTags(generation, r) => {
                if let Some(l) = self.loaded.as_mut().filter(|l| l.generation == generation) {
                    l.my_tags = match r {
                        Ok(tags) => MyTags::Read(Arc::new(tags)),
                        Err(e) => MyTags::Failed(e),
                    };
                    // Browse › My Tags, grouped again now there are tags to group by.
                    self.regroup(Category::MyTags);
                    self.refresh_visible();
                }
            }
            Msg::Sets(generation, r) => {
                let Some(l) = self.loaded.as_mut().filter(|l| l.generation == generation) else { return };
                let sets = match r {
                    Ok(sets) => sets,
                    Err(e) => {
                        l.sets.get_or_insert_default();
                        self.toast(format!("can't read the sets: {e}"), Level::Error);
                        return;
                    }
                };
                // Only a set shown last run is shown before the sets are first read.
                let restored = l.reading_set();
                let showing_a_set = matches!(l.node, NodeId::Group(Category::Sets, _));
                l.sets = Some(sets);
                self.regroup(Category::Sets);
                if showing_a_set {
                    self.refresh_visible();
                }
                // In the tree now: the cursor goes to it, as it does to a playlist shown last run.
                if let Some(l) = self.loaded.as_ref().filter(|_| restored) {
                    self.playlist_state.select(l.tree_position(&l.node).or(self.playlist_state.selected()));
                }
            }
            Msg::Indexed(generation, index) => {
                if let Some(l) = self.loaded.as_mut().filter(|l| l.generation == generation) {
                    let Index { ranks, groups } = *index;
                    for (field, r) in ranks {
                        if !l.ranks.iter().any(|(f, _)| *f == field) {
                            l.ranks.push((field, r));
                        }
                    }
                    for (c, g) in groups {
                        l.groups.entry(c).or_insert(g);
                    }
                }
            }
            Msg::Opened(id, r) => self.opened(id, r),
            Msg::Note(msg, level) => self.toast(msg, level),
            Msg::TrackInfo(id, info) => {
                let upcoming = self.upcoming.as_mut().map(|u| &mut u.now);
                if let Some(now) = self.now.as_mut().into_iter().chain(upcoming).find(|n| n.id == id) {
                    let info = *info;
                    now.phrases = phrase_spans(&info.analysis, now.duration);
                    now.analysis = info.analysis;
                    now.art = info.art;
                    now.matches = info.matches;
                    now.playlists = info.playlists;
                    now.loading = false;
                }
                if self.now.as_ref().is_some_and(|n| n.id == id) {
                    self.reset_next_pick();
                }
            }
            Msg::Details(d) => {
                if self.details_asked == Some((d.generation, d.index)) {
                    self.details_asked = None;
                    let fresh = self.details.as_ref().is_none_or(|old| old.index != d.index);
                    if fresh {
                        self.details_state.select(if d.matches.items.is_empty() { None } else { Some(0) });
                    }
                    self.details = Some(*d);
                }
            }
            Msg::Status(r) => {
                self.status_loading = false;
                let r = r.map(|s| {
                    self.fleet_libraries = s.libraries;
                    s.mounted
                });
                self.status = Some(r);
            }
            Msg::History(r) => {
                self.history = Some(r);
                // Read again, the list can be shorter, or empty.
                let n = self.history_rows();
                let sel = self.history_state.selected().unwrap_or(0).min(n.saturating_sub(1));
                self.history_state.select((n > 0).then_some(sel));
            }
            Msg::Profile(generation, r) => self.profile.loaded(generation, *r),
            #[cfg(feature = "account")]
            Msg::Account(e) => super::account::on_event(self, e),
            Msg::Mix(generation, r) => super::mix::loaded(self, generation, *r),
            Msg::MixSources(listing, root, sources) => {
                if listing == self.mix.drive_listing {
                    self.mix.drive_listed(root, sources);
                }
            }
            Msg::MixFolders(folders, sources) => super::mix::folders_listed(self, folders, sources),
            Msg::Planned { job, root, result } => self.on_planned(job, root, result),
            Msg::Applied { job, root, result } => self.on_applied(job, root, result),
            Msg::Done { job, result } => self.on_done(job, result),
            Msg::Replay(generation, edits) => {
                let Some(l) = self.loaded.as_ref().filter(|l| l.generation == generation) else { return };
                let root = l.root().to_path_buf();
                self.replay_checked = generation;
                self.replay_offer = (edits > 0).then(|| ReplayOffer { drive: self.drive_label(&root), root, edits });
            }
            Msg::Progress { job, detail, committed } => self.on_progress(job, detail, committed),
            Msg::Failed(work, e) => {
                // A poll that keeps failing is said once, not every second.
                let again = work == Work::Volumes && std::mem::replace(&mut self.volumes_failing, true);
                match work {
                    Work::Drives => self.drives_in_flight = false,
                    Work::Volumes => self.volumes_in_flight = false,
                    Work::Details(generation, index) => {
                        self.details_asked.take_if(|k| *k == (generation, index));
                        self.details_failed = Some((generation, index));
                    }
                }
                if !again {
                    self.toast(e, Level::Error);
                }
                if work == Work::Drives && std::mem::take(&mut self.drives_again) {
                    self.refresh_drives();
                }
            }
        }
    }

    /// The open drive went away, or came back.
    fn check_present(&mut self, present: Option<(PathBuf, bool)>) {
        let Some((root, here)) = present else { return };
        let Some(l) = self.loaded.as_mut().filter(|l| l.root() == root) else { return };
        if !here && !l.stale {
            l.stale = true;
            self.picking = None;
            let name = self.drive_label(&root);
            self.toast(format!("{name} was unplugged: what's shown may be out of date"), Level::Error);
        } else if here && l.stale && self.loading.is_none() {
            self.load_library(root);
        }
    }

    /// Show a freshly read library. The drive that was already open keeps its view (list,
    /// folders, selection, marks); another drive starts at All tracks, or where the last run
    /// left it.
    fn install(&mut self, root: PathBuf, mut new: Loaded) {
        let keep = self.loaded.as_ref().filter(|l| l.root() == new.root()).map(|l| self.view_keep(l));
        // The store's sets go with every drive: the ones read last stand in until they're read
        // again, so a set shown doesn't empty meanwhile.
        new.sets = self.loaded.as_mut().and_then(|l| l.sets.take());
        self.crate_list.refresh(new.root(), &new.library);
        self.loaded = Some(new);
        self.details = None;
        self.details_asked = None;
        match keep {
            Some(k) => self.restore_view(k),
            None => {
                self.query.clear();
                self.marks.clear();
                let restore = self.restore.take().filter(|(r, _)| *r == root).map(|(_, n)| n);
                let Some(l) = &mut self.loaded else { return };
                if let Some(id) = restore
                    .and_then(|n| l.find_node(&n))
                    .filter(|id| !matches!(id, NodeId::Browse | NodeId::Category(_)))
                {
                    l.reveal(&id);
                    l.node = id;
                }
                self.playlist_state.select(l.tree_position(&l.node).or(Some(0)));
                self.track_state = TableState::default().with_selected(Some(0));
                self.visible.clear();
                self.refresh_visible();
                if let Some(i) = self.drives.iter().position(|d| d.root == root) {
                    self.drive_state.select(Some(i));
                }
            }
        }
        self.restore_queue(&root);
        self.check_files();
        self.index_library();
        self.read_sets();
        self.check_replay();
        if self.show_tags_column {
            self.read_my_tags();
        }
    }

    /// Whether rekordbox exported over edits recorded on the open drive, in the background: it
    /// exported after locallibrary last wrote to the drive (or saw it; see `Device::exported_at`),
    /// and replaying the recorded edits would change something. Then replaying them is offered.
    fn check_replay(&mut self) {
        if self.replay_offer.as_ref().is_some_and(|o| self.loaded.as_ref().is_none_or(|l| l.root() != o.root)) {
            self.replay_offer = None;
        }
        let Some(l) = self.loaded.as_ref() else { return };
        let (root, generation) = (l.root().to_path_buf(), l.generation);
        if l.stale || self.replay_dismissed.contains(&root) {
            self.replay_checked = generation;
            return;
        }
        self.spawn(move || {
            let drive = api::DriveRef { drive: root.display().to_string() };
            let edits = api::edits_list(&drive).map_or(0, |e| e.len());
            // Written since: later than the state locallibrary recorded last (allowing for FAT's
            // two-second times).
            let written_since = || {
                api::drive_info(&drive).is_ok_and(|i| match (i.exported_at, &i.last_known) {
                    (Some(at), Some(known)) => at > known.at + chrono::Duration::seconds(3),
                    (Some(_), None) => true,
                    (None, _) => false,
                })
            };
            let changes =
                edits > 0 && written_since() && api::edits_replay_plan(&drive, &Ctx::silent()).is_ok_and(|p| !p.noop);
            Msg::Replay(generation, if changes { edits } else { 0 })
        });
    }

    /// `y` with the offer up: replay the edits (shows the plan first).
    fn take_replay_offer(&mut self) {
        let Some(o) = self.replay_offer.take() else { return };
        let drive = o.root.display().to_string();
        self.plan(o.root, move |ctx| Ok(Box::new(api::edits_replay_plan(&api::DriveRef { drive }, ctx)?)));
    }

    /// Work out every sort's ranks and Browse's groups in the background.
    fn index_library(&mut self) {
        let Some(l) = &self.loaded else { return };
        let (lib, generation, by_id) = (l.library.clone(), l.generation, l.by_id.clone());
        self.spawn(move || {
            let ranks = browse::SORT_FIELDS.iter().map(|&f| (f, browse::ranks(&lib, f))).collect();
            // Problems waits for the file check, and is quick; My Tags and Sets wait for their reads.
            let later = |c: &Category| matches!(c, Category::Problems | Category::MyTags | Category::Sets);
            let groups = (Category::ALL.into_iter().filter(|c| !later(c)))
                .map(|c| (c, browse::groups(&lib, c, &by_id, &Problems::default(), None, &[])))
                .collect();
            Msg::Indexed(generation, Box::new(Index { ranks, groups }))
        });
    }

    fn view_keep(&self, l: &Loaded) -> ViewKeep {
        let selected = self.track_state.selected();
        ViewKeep {
            node: l.save_node(&l.node),
            expanded: l.expanded.iter().map(|id| l.save_node(id)).collect(),
            tree_row: self.playlist_state.selected().and_then(|i| l.tree.get(i)).map(|n| l.save_node(&n.id)),
            track: selected.and_then(|s| self.visible.get(s)).map(|&i| l.library.tracks[i].file_path.clone()),
            row: selected.unwrap_or(0),
            offset: self.track_state.offset(),
        }
    }

    fn restore_view(&mut self, k: ViewKeep) {
        let Some(l) = &mut self.loaded else { return };
        for s in &k.expanded {
            if let Some(id) = l.find_node(s) {
                l.expanded.insert(id);
            }
        }
        l.node = l.find_node(&k.node).unwrap_or(NodeId::All);
        l.rebuild_tree();
        let row = k.tree_row.and_then(|s| l.find_node(&s)).and_then(|id| l.tree_position(&id));
        let rows = l.tree.len();
        self.playlist_state.select(Some(row.unwrap_or(self.playlist_state.selected().unwrap_or(0).min(rows - 1))));
        self.marks.retain(|p| l.index_of(p).is_some());
        let target = k.track.and_then(|p| l.index_of(&p));
        self.visible.clear();
        self.refresh_visible();
        let pos = target.and_then(|i| self.visible.iter().position(|&v| v == i));
        if !self.visible.is_empty() {
            self.track_state.select(Some(pos.unwrap_or(k.row.min(self.visible.len() - 1))));
            *self.track_state.offset_mut() = k.offset;
        }
    }

    /// Work out the track list: the list shown (or the whole drive, when searching it), narrowed
    /// by the search box, in the chosen order. Keeps the selected track selected if it's still
    /// there.
    pub fn refresh_visible(&mut self) {
        self.search_due = None;
        let query = self.query.trim().to_string();
        let search = browse::Search::parse(&query);
        let filter = (!search.rest.is_empty()).then(|| Filter::parse(&search.rest));
        let tags = filter.as_ref().is_some_and(|f| f.as_ref().is_ok_and(Filter::uses_tags));
        if tags {
            self.read_my_tags();
        }
        self.filter_note = None;
        let Some(l) = &mut self.loaded else {
            self.visible.clear();
            return;
        };
        let selected = self.track_state.selected().and_then(|s| self.visible.get(s)).copied();
        let query = query.as_str();
        let mut list =
            if self.search_all && !query.is_empty() { (0..l.library.tracks.len()).collect() } else { l.base() };
        match filter {
            Some(Err(e)) => {
                self.filter_error = Some(e.to_string());
                if self.searching && !self.visible.is_empty() {
                    return; // keep the last good result while the filter is being typed
                }
                let q = normalize(query);
                list.retain(|&i| l.haystack[i].contains(&q));
            }
            None => {
                self.filter_error = None;
                if search.has_words() {
                    list.retain(|&i| search.matches(&l.haystack[i]));
                }
            }
            Some(Ok(f)) => {
                self.filter_error = None;
                if search.has_words() {
                    list.retain(|&i| search.matches(&l.haystack[i]));
                }
                let lib = l.library.clone();
                // My Tags as `tracks --filter` reads them; until they're in, `tag:` matches nothing.
                let none = FxHashMap::default();
                let my_tags = match &l.my_tags {
                    MyTags::Read(t) => Some(t.clone()),
                    _ if !tags => None,
                    MyTags::Reading | MyTags::Unread => {
                        self.filter_note = Some("reading My Tags…".into());
                        None
                    }
                    MyTags::Failed(e) => {
                        self.filter_note = Some(format!("can't read My Tags: {e}"));
                        None
                    }
                };
                // Refused as `tracks --filter` refuses it: a `playlist:` naming nothing would match
                // no track, and negated, every one.
                match api::filter_context(&lib, &f, tags.then(|| my_tags.as_deref().unwrap_or(&none))) {
                    Ok(ctx) => list.retain(|&i| f.matches(&lib.tracks[i], &ctx)),
                    Err(e) => {
                        self.filter_error = Some(e.to_string());
                        if self.searching && !self.visible.is_empty() {
                            return; // keep the last good result while the filter is being typed
                        }
                        list.clear();
                    }
                }
            }
        }
        if let Some(s) = self.sort {
            browse::sort_list(&mut list, l.rank(s.field), s.descending);
        }
        self.visible = list;
        let len = self.visible.len();
        let pos = selected.and_then(|i| self.visible.iter().position(|&v| v == i));
        let sel = pos.unwrap_or_else(|| self.track_state.selected().unwrap_or(0).min(len.saturating_sub(1)));
        self.track_state.select(if len == 0 { None } else { Some(sel) });
    }

    /// The search box changed. On a small library the list narrows at once; on a big one once
    /// typing pauses, so keys don't each wait for a pass over every track.
    fn search_changed(&mut self) {
        let big = self.loaded.as_ref().is_some_and(|l| l.library.tracks.len() > TYPE_AHEAD);
        if big {
            self.search_due = Some(Instant::now() + Duration::from_millis(150));
        } else {
            self.refresh_visible();
        }
    }

    /// Narrow the list now if the search box is waiting to.
    fn flush_search(&mut self) {
        if self.search_due.is_some() {
            self.refresh_visible();
        }
    }

    /// Group `c` again now there's more to group by (read in the background), keeping the tree's
    /// cursor on its row as rows come and go above it.
    fn regroup(&mut self, c: Category) {
        let Some(l) = self.loaded.as_mut() else { return };
        let row = self.playlist_state.selected().and_then(|i| l.tree.get(i)).map(|n| n.id.clone());
        l.groups.remove(&c);
        l.rebuild_tree();
        if let Some(i) = row.and_then(|id| l.tree_position(&id)) {
            self.playlist_state.select(Some(i));
        }
    }

    /// Read the store's sets in the background, for Browse › Sets. Without a store there are
    /// none.
    pub(super) fn read_sets(&mut self) {
        let Some(l) = &self.loaded else { return };
        let (store_root, generation) = (self.store_root.clone(), l.generation);
        let job = move || {
            let sets = match Store::open(&store_root) {
                Ok(store) => api::sets_list(&store, &api::SetsQuery::default()).map_err(|e| e.to_string()),
                Err(_) => Ok(Vec::new()),
            };
            Msg::Sets(generation, sets)
        };
        self.spawn_or(job, move |e| Msg::Sets(generation, Err(e)));
    }

    /// Read the open drive's My Tags in the background, the first time a filter needs them.
    pub(super) fn read_my_tags(&mut self) {
        let Some(l) = self.loaded.as_mut().filter(|l| matches!(l.my_tags, MyTags::Unread)) else { return };
        l.my_tags = MyTags::Reading;
        let (root, generation) = (l.root().to_path_buf(), l.generation);
        let job = move || {
            let drive = api::DriveRef { drive: root.display().to_string() };
            Msg::MyTags(generation, api::tags_by_track(&drive).map_err(|e| e.to_string()))
        };
        self.spawn_or(job, move |e| Msg::MyTags(generation, Err(e)));
    }

    /// The selected track, as an index into the library.
    pub fn selected_track(&self) -> Option<usize> {
        self.track_state.selected().and_then(|s| self.visible.get(s)).copied()
    }

    /// The tracks actions work on: the marked ones (those in view first, in list order), else the
    /// selected one.
    pub fn targets(&self) -> Vec<usize> {
        let Some(l) = &self.loaded else { return Vec::new() };
        if self.marks.is_empty() {
            return self.selected_track().into_iter().collect();
        }
        let mut seen = FxHashSet::default();
        let marked = |i: &usize| self.marks.contains(&l.library.tracks[*i].file_path);
        let mut out: Vec<usize> = self.visible.iter().copied().filter(marked).filter(|i| seen.insert(*i)).collect();
        out.extend((0..l.library.tracks.len()).filter(marked).filter(|i| seen.insert(*i)));
        out
    }

    /// Whether the details shown are for the selected track.
    pub fn details_current(&self) -> bool {
        let (Some(l), Some(d)) = (&self.loaded, &self.details) else { return false };
        d.generation == l.generation && Some(d.index) == self.selected_track()
    }

    /// Whether looking up the selected track's details failed (the log says why).
    pub fn details_lookup_failed(&self) -> bool {
        let Some(l) = &self.loaded else { return false };
        self.details_failed.is_some() && self.details_failed == self.selected_track().map(|i| (l.generation, i))
    }

    /// The match selected in the details panel, as an index into the open library.
    pub fn selected_match(&self) -> Option<usize> {
        if !self.details_current() {
            return None;
        }
        let d = self.details.as_ref()?;
        d.matches.items.get(self.details_state.selected()?).map(|m| m.index)
    }

    /// Look up the selected track's matches once the selection has settled for a moment, so
    /// holding an arrow key doesn't start a search per row.
    fn want_details(&mut self) {
        if !self.show_details || self.view != View::Library {
            return;
        }
        let (Some(l), Some(i)) = (&self.loaded, self.selected_track()) else { return };
        let generation = l.generation;
        if self.details_current() || [self.details_asked, self.details_failed].contains(&Some((generation, i))) {
            return;
        }
        match self.details_seen {
            Some((g, j, at)) if (g, j) == (generation, i) => {
                if at.elapsed() < Duration::from_millis(90) {
                    return;
                }
            }
            _ => {
                self.details_seen = Some((generation, i, Instant::now()));
                return;
            }
        }
        self.details_asked = Some((generation, i));
        let (lib, root) = (l.library.clone(), l.root().to_path_buf());
        let job = move || {
            let track = lib.tracks[i].clone();
            let playlists = playlists_of(&lib, &track);
            let matches = Matches::find(lib, root, &track, 60);
            Msg::Details(Box::new(Details { generation, index: i, matches, playlists }))
        };
        self.spawn_or(job, move |e| Msg::Failed(Work::Details(generation, i), e));
    }

    // -----------------------------------------------------------------------------------------
    // Playback
    // -----------------------------------------------------------------------------------------

    fn next_random(&mut self) -> u64 {
        // xorshift64
        let mut x = self.rng;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.rng = x;
        x
    }

    fn shuffle_order(&mut self, first: usize) {
        let n = self.queue.len();
        let mut order: Vec<usize> = (0..n).filter(|&i| i != first).collect();
        for i in (1..order.len()).rev() {
            let j = (self.next_random() % (i as u64 + 1)) as usize;
            order.swap(i, j);
        }
        order.insert(0, first);
        self.queue.order = order;
        self.queue.pos = 0;
    }

    /// Play `items` of `library` (all on the drive at `root`) from `start`, as the queue.
    fn play_tracks(&mut self, library: Arc<Library>, items: Vec<usize>, root: PathBuf, start: usize) {
        if start >= items.len() {
            return;
        }
        self.resume = None;
        if self.loaded.as_ref().is_some_and(|l| l.stale && l.root() == root) {
            self.toast("the drive was unplugged", Level::Error);
            return;
        }
        let drive = self.drive_label(&root);
        self.queue = Queue { order: (0..items.len()).collect(), library, items, root, drive, pos: start };
        self.failed_in_row = 0;
        if self.shuffle {
            self.shuffle_order(start);
        }
        self.start_current();
    }

    /// Play the selected track, queueing the rest of the visible list after it.
    fn play_selected(&mut self) {
        let (Some(l), Some(sel)) = (&self.loaded, self.track_state.selected()) else { return };
        let (library, root) = (l.library.clone(), l.root().to_path_buf());
        self.play_tracks(library, self.visible.clone(), root, sel);
    }

    /// Play the selected match in the details panel, queueing the other matches.
    fn play_match(&mut self) {
        let (Some(d), Some(sel)) = (&self.details, self.details_state.selected()) else { return };
        if !self.details_current() {
            return;
        }
        let items = d.matches.items.iter().map(|m| m.index).collect();
        let (library, root) = (d.matches.library.clone(), d.matches.root.clone());
        self.play_tracks(library, items, root, sel);
    }

    /// Full screen: the pick among the playing track's matches back on the best one.
    fn reset_next_pick(&mut self) {
        let any = self.now.as_ref().and_then(|n| n.matches.as_ref()).is_some_and(|m| !m.items.is_empty());
        self.next_pick = ListState::default().with_selected(any.then_some(0));
    }

    /// Full screen: queue the picked match to play after the playing track.
    fn play_next(&mut self) {
        if !self.playing_queue() {
            return;
        }
        let Some(m) = self.now.as_ref().and_then(|n| n.matches.as_ref()) else { return };
        let Some(mix) = self.next_pick.selected().and_then(|i| m.items.get(i)) else { return };
        let track = m.track(mix);
        // The matches index the library as it was read when the track started; the queue's
        // copy is older when the drive was read again since.
        let index = if Arc::ptr_eq(&m.library, &self.queue.library) {
            Some(mix.index)
        } else {
            self.queue.library.tracks.iter().position(|t| t.id == track.id)
        };
        let Some(index) = index else { return };
        let name = track.display_name();
        self.queue.items.push(index);
        self.queue.order.insert(self.queue.pos + 1, self.queue.items.len() - 1);
        self.drop_upcoming();
        self.toast(format!("next: {name}"), Level::Info);
    }

    /// Play the crate from the selected entry (the entries on its drive).
    fn play_crate(&mut self) {
        let Some(sel) = self.crate_state.selected().filter(|&i| i < self.crate_list.len()) else { return };
        let root = self.crate_list.items[sel].root.clone();
        let mut start = 0;
        let mut tracks = Vec::new();
        for (i, t) in self.crate_list.items.iter().enumerate().filter(|(_, t)| t.root == root) {
            if i == sel {
                start = tracks.len();
            }
            tracks.push(t.track.clone());
        }
        if tracks.len() < self.crate_list.len() {
            let msg = format!("playing the crate's tracks on {}", self.crate_list.items[sel].drive);
            self.toast(msg, Level::Info);
        }
        let items = (0..tracks.len()).collect();
        self.play_tracks(Arc::new(Library { tracks, ..Default::default() }), items, root, start);
    }

    /// The output to play through: the one open, else (there was none, or it went away) the one
    /// picked, or the default, opened now. False, with `audio_error` saying why, when none opens.
    pub(super) fn open_output(&mut self) -> bool {
        if self.audio.is_none() {
            let (audio, missing) = self.outputs.open_preferring(self.deck.output.as_deref());
            match audio {
                Ok(mut a) => {
                    a.set_volume(self.volume);
                    self.audio = Some(a);
                    self.audio_error = None;
                    if let Some(why) = missing {
                        self.toast(format!("{why}: playing through the default output"), Level::Info);
                    }
                }
                Err(e) => self.audio_error = Some(format!("{e:#}")),
            }
        }
        self.audio.is_some()
    }

    /// Stop what's playing, to play something else. False, saying why, when there's no output.
    fn ready_to_play(&mut self) -> bool {
        if !self.open_output() {
            let e = self.audio_error.clone().unwrap_or_default();
            self.toast(format!("can't play: {e}"), Level::Error);
            return false;
        }
        if let Some(a) = &mut self.audio {
            a.stop();
        }
        self.upcoming = None;
        self.pending_seek = None;
        self.start_paused = false;
        self.deck.looped = None;
        true
    }

    /// The output's news: a seek it couldn't make, or its stream failing. When the output is
    /// gone (unplugged, say), it's let go of, and with it the file it had open on the drive: the
    /// track waits where it was, and the next play opens whatever output there is then.
    fn watch_output(&mut self) {
        let Some(a) = &self.audio else { return };
        let (seek_failed, error) = (a.seek_failed(), a.stream_error());
        if seek_failed {
            self.toast("can't seek in this file", Level::Error);
        }
        let Some((e, gone)) = error else { return };
        if !gone {
            self.toast(format!("audio: {e}"), Level::Error);
            return;
        }
        let (at, _) = self.playback();
        self.audio = None;
        self.upcoming = None;
        self.audio_error = Some(e.clone());
        if self.now.is_some() {
            self.resume = at;
        }
        self.toast(format!("audio: {e} Space plays on through the output there is now."), Level::Error);
    }

    /// Whether the track playing is the queue's (rather than a recorded set, played on its own).
    fn playing_queue(&self) -> bool {
        let Some(now) = &self.now else { return false };
        (self.queue.order.get(self.queue.pos))
            .is_some_and(|&i| self.queue.track(i).file_path == now.track.file_path && self.queue.root == now.root)
    }

    /// Start the queue's current track. It shows at once; its audio file is opened in the
    /// background (see `opened`), and its analysis, artwork and matches are read in the
    /// background too, filling in when they arrive. False when there's no track or no audio.
    fn start_current(&mut self) -> bool {
        let Some(&i) = self.queue.order.get(self.queue.pos) else { return false };
        let track = self.queue.track(i).clone();
        // Where the last run left off was for the track it was on: once another starts, it's gone.
        self.resume = None;
        if !self.ready_to_play() {
            return false;
        }
        self.now = Some(self.open(track, true));
        true
    }

    /// Open one of the queue's tracks in the background: its audio file (see `opened`), then its
    /// analysis, artwork and matches, which fill in as they arrive. `starts`: it's the one to
    /// play now (rather than the next, opened ahead).
    fn open(&mut self, track: Track, starts: bool) -> NowPlaying {
        let id = self.next_id();
        let root = self.queue.root.clone();
        let (t, r, auto_cue) = (track.clone(), root.clone(), self.deck.auto_cue);
        // Matches and playlists need the drive's library, which is open unless the player
        // moved to another drive since the queue was made.
        let library = self.loaded.as_ref().filter(|l| l.root() == self.queue.root).map(|l| l.library.clone());
        let job = move |o: &Opening| {
            let file = Device::open(&r).map_err(anyhow::Error::from).and_then(|d| {
                let mut file = audio::open_file(&d.checked_path(&paths::key(&t.file_path))?)?;
                if auto_cue {
                    file.start = super::deck::auto_cue(&anlz::load_grid(&d, &t));
                }
                Ok(file)
            });
            if o.send(Msg::Opened(id, file.map_err(|e| format!("{e:#}")))) {
                o.send(Msg::TrackInfo(id, Box::new(track_info(&r, &t, library))));
            }
        };
        self.opener.open(&self.tx, id, starts, Box::new(job));
        NowPlaying {
            id,
            key: Camelot::parse(&track.key),
            duration: Duration::from_secs(track.duration_secs as u64),
            track,
            drive: self.queue.drive.clone(),
            root,
            analysis: TrackAnalysis::default(),
            art: None,
            phrases: Vec::new(),
            matches: None,
            playlists: Vec::new(),
            loading: true,
            opening: true,
            advancing: false,
        }
    }

    /// Keep up with the player: when it has gone on to the queued track, that's the one playing;
    /// when the playing one ends with nothing queued, start the next; and near the end of a
    /// track, open the next one and queue it, to follow without a gap.
    fn follow_queue(&mut self) {
        let broken = self.audio.as_ref().and_then(Audio::broken);
        if self.audio.as_mut().is_some_and(Audio::advanced) {
            match (self.upcoming.take().filter(|u| u.queued), broken) {
                (Some(_), Some(why)) => self.cut_short(why),
                (Some(up), None) => self.went_on(up),
                (None, _) => {}
            }
        } else if self.now.is_some() && self.audio.as_ref().is_some_and(Audio::finished) {
            match (self.upcoming.take(), broken) {
                (_, Some(why)) => self.cut_short(why),
                // Opened already: straight on with it.
                (Some(mut up), None) if up.file.is_some() => {
                    if let (Some(a), Some(file)) = (&mut self.audio, up.file.take()) {
                        a.play(file, false);
                    }
                    self.went_on(up);
                }
                _ if self.playing_queue() => {
                    self.failed_in_row = 0;
                    self.advance(true);
                }
                // A recorded set, played on its own.
                _ => self.stop(),
            }
        }
        self.open_next();
    }

    /// The playing track ended without playing through, `why` (see `Audio::broken`). Like one
    /// that won't open, the queue moves past it (with repeat one, it would come round forever).
    fn cut_short(&mut self, why: String) {
        if let Some(a) = &mut self.audio {
            a.stop();
        }
        self.wont_play(why);
    }

    /// The player went on to the next track, opened ahead.
    fn went_on(&mut self, up: Upcoming) {
        self.queue.pos = up.pos;
        self.now = Some(up.now);
        self.reset_next_pick();
        self.deck.looped = None;
        self.last_pos_ms = 0.0;
        self.failed_in_row = 0;
    }

    /// Where the track is (while it opens, where it's to start: `None` for where it starts on its
    /// own) and whether it plays (or will, once it's open).
    pub(super) fn playback(&self) -> (Option<Duration>, bool) {
        match &self.now {
            Some(n) if n.opening => (self.pending_seek, !self.start_paused),
            _ => (Some(self.position()), self.is_playing()),
        }
    }

    /// Open the playing track again (its output changed, or went away) from `at` (`None`: where
    /// it starts on its own), with its loop, and paused unless `playing`. With no output to play
    /// it through, it waits there for the next play.
    pub(super) fn restart(&mut self, at: Option<Duration>, playing: bool) {
        let Some(now) = &self.now else { return };
        let looped = self.deck.looped;
        let started = if self.playing_queue() {
            self.start_current()
        } else {
            // A recorded set, played on its own.
            let (path, track) = (PathBuf::from(&now.track.file_path), now.track.clone());
            self.play_file(&path, track, Duration::ZERO)
        };
        if !started {
            self.resume = at.or(Some(Duration::ZERO));
            return;
        }
        self.deck.looped = looped;
        self.start_paused = !playing;
        if let Some(at) = at {
            self.seek_to(at);
        }
    }

    /// Open the track that plays next, once the playing one is near its end.
    fn open_next(&mut self) {
        let Some(now) = self.now.as_ref().filter(|n| !n.opening) else { return };
        if self.upcoming.is_some() || self.audio.is_none() || !self.playing_queue() || self.deck.single {
            return;
        }
        if now.duration.saturating_sub(self.position()) > AHEAD {
            return;
        }
        let Some(pos) = self.next_pos(true) else { return };
        let track = self.queue.track(self.queue.order[pos]).clone();
        let mut now = self.open(track, false);
        now.advancing = true;
        self.upcoming = Some(Upcoming { now, pos, queued: false, file: None });
    }

    /// The next track was opened: queue it behind the playing one, or keep it to start as that
    /// one ends. One that won't open is left, so the queue moves on to it when the playing one
    /// ends and says why it won't play.
    fn opened_next(&mut self, file: Result<audio::OpenedFile, String>) {
        let (Some(up), Some(a)) = (&mut self.upcoming, &mut self.audio) else { return };
        up.now.opening = false;
        if let Ok(file) = file {
            up.now.duration = file_duration(&up.now.track, &file);
            match a.queue(file) {
                Ok(()) => up.queued = true,
                Err(file) => up.file = Some(file),
            }
        }
    }

    /// What comes next changed (shuffle, repeat, single play): forget the track opened for it.
    pub(super) fn drop_upcoming(&mut self) {
        if self.upcoming.take().is_some_and(|u| u.queued)
            && let Some(a) = &mut self.audio
        {
            a.unqueue();
        }
    }

    /// A track's audio file opened in the background: play it, or say why it won't (and, moving
    /// through the queue, go on to the next).
    fn opened(&mut self, id: u64, file: Result<audio::OpenedFile, String>) {
        if self.upcoming.as_ref().is_some_and(|u| u.now.id == id && u.now.opening) {
            self.opened_next(file);
            return;
        }
        let Some(now) = self.now.as_mut().filter(|n| n.id == id && n.opening) else {
            return; // another track was started since: this one's dropped
        };
        now.opening = false;
        match file {
            Ok(file) => {
                now.duration = file_duration(&now.track, &file);
                let paused = std::mem::take(&mut self.start_paused);
                // (The output went away while it opened: it waits for the next play.)
                let Some(a) = &mut self.audio else { return };
                a.play(file, paused);
                super::deck::apply_loop(self);
                if let Some(at) = self.pending_seek.take() {
                    self.seek_to(at);
                }
            }
            Err(e) => self.wont_play(e),
        }
    }

    /// The playing track won't play, `why`: say so, and moving through the queue, go on to the
    /// next (but not round and round when none of them plays).
    fn wont_play(&mut self, why: String) {
        let Some(now) = self.now.take() else { return };
        self.toast(format!("{}: {why}", now.track.display_name()), Level::Error);
        self.failed_in_row += 1;
        if now.advancing && self.failed_in_row < self.queue.len() {
            self.advance(false);
        } else {
            self.failed_in_row = 0;
        }
    }

    /// Where the queue goes next: on, round to the start unless repeat is off, or at the end of
    /// a track (`auto`) with repeat one, the same track again.
    fn next_pos(&self, auto: bool) -> Option<usize> {
        let (pos, n) = (self.queue.pos, self.queue.order.len());
        if auto && self.repeat == Repeat::One && pos < n {
            Some(pos)
        } else if pos + 1 < n {
            Some(pos + 1)
        } else if self.repeat != Repeat::Off && n > 0 {
            Some(0)
        } else {
            None
        }
    }

    /// Move through the queue. `auto` is the end of a track (repeat-one applies).
    fn advance(&mut self, auto: bool) {
        let Some(pos) = self.next_pos(auto) else {
            self.stop();
            return;
        };
        self.queue.pos = pos;
        if auto && self.deck.single {
            // Single play: stop, with the next track ready for space.
            self.stop();
            self.resume = Some(Duration::ZERO);
            return;
        }
        if self.start_current() {
            if let Some(now) = &mut self.now {
                now.advancing = true;
            }
        } else {
            self.stop();
        }
    }

    fn previous(&mut self) {
        if self.position() > Duration::from_secs(3) || self.queue.pos == 0 {
            self.seek_to(Duration::ZERO);
        } else {
            self.queue.pos -= 1;
            self.start_current();
        }
    }

    fn stop(&mut self) {
        if let Some(a) = &mut self.audio {
            a.stop();
        }
        self.now = None;
        self.upcoming = None;
        self.opener.drop_all();
    }

    /// Before ejecting the drive at `root`: stop playing from it (the track, and the next one
    /// opened ahead), or its open files keep it from being unmounted. Files opened for them and
    /// on their way here are closed now rather than at the next tick; the eject waits for one
    /// being opened (see `opener_idle`).
    pub(super) fn let_go_of(&mut self, root: &Path) {
        let on = |p: &Path| p.starts_with(root);
        if self.now.as_ref().is_some_and(|n| on(&n.root)) || self.upcoming.as_ref().is_some_and(|u| on(&u.now.root)) {
            self.stop();
            self.resume = None;
            while let Ok(msg) = self.rx.try_recv() {
                self.handle_msg(msg);
            }
        }
    }

    /// Held while a track is being opened: an eject waits for it.
    pub(super) fn opener_idle(&self) -> Arc<Mutex<()>> {
        self.opener.idle.clone()
    }

    fn toggle_pause(&mut self) {
        match (&self.audio, &self.now) {
            // Still opening (a slow drive): it starts paused or playing as last asked.
            (Some(_), Some(n)) if n.opening => self.start_paused = !self.start_paused,
            (Some(a), Some(_)) => a.toggle_pause(),
            // The output went away: play on from where it was, through what there is now.
            (None, Some(_)) => {
                let at = self.resume.take();
                self.restart(at, true);
            }
            _ if self.resumable().is_some() => {
                // Where the last run left off.
                let at = self.resume.take().unwrap_or_default();
                if self.start_current() && at > Duration::ZERO {
                    self.seek_to(at);
                }
            }
            _ => self.play_selected(),
        }
    }

    pub(super) fn seek_to(&mut self, pos: Duration) {
        self.seek(pos, false);
    }

    /// Seek, `keep_time` for a target worked out from the playhead (see `Audio::seek`).
    fn seek(&mut self, pos: Duration, keep_time: bool) {
        let Some(now) = &self.now else { return };
        // Short of the end, so a seek doesn't end the track; a length that isn't known doesn't
        // hold one back.
        let end = now.duration.checked_sub(Duration::from_millis(250)).filter(|_| !now.duration.is_zero());
        let (pos, opening) = (end.map_or(pos, |end| pos.min(end)), now.opening);
        super::deck::seeking(self, pos);
        if opening {
            self.pending_seek = Some(pos);
            return;
        }
        match &self.audio {
            Some(a) => a.seek(pos, keep_time),
            // The output went away: where it plays on from.
            None => self.resume = Some(pos),
        }
        self.last_seek = Instant::now();
    }

    fn seek_by(&mut self, secs: f64) {
        let target = (self.position().as_secs_f64() + secs).max(0.0);
        self.seek_to(Duration::from_secs_f64(target));
    }

    /// Play a file that isn't one of a drive's tracks (a recorded set) on its own, from `start`
    /// (the queue stays, for afterwards). `track` is what the player shows for it. The file is
    /// opened in the background.
    pub(super) fn play_file(&mut self, path: &Path, track: Track, start: Duration) -> bool {
        if !self.ready_to_play() {
            return false;
        }
        self.resume = None;
        self.pending_seek = (start > Duration::ZERO).then_some(start);
        let id = self.next_id();
        let file = path.to_path_buf();
        let job = move |o: &Opening| {
            o.send(Msg::Opened(id, audio::open_file(&file).map_err(|e| format!("{e:#}"))));
        };
        self.opener.open(&self.tx, id, true, Box::new(job));
        self.now = Some(NowPlaying {
            id,
            key: None,
            drive: "recorded set".into(),
            root: path.parent().map(Path::to_path_buf).unwrap_or_default(),
            duration: Duration::from_secs(track.duration_secs as u64),
            track,
            analysis: TrackAnalysis::default(),
            art: None,
            phrases: Vec::new(),
            matches: None,
            playlists: Vec::new(),
            loading: false,
            opening: true,
            advancing: false,
        });
        true
    }

    fn jump_cue(&mut self, forward: bool) {
        let Some(now) = &self.now else { return };
        let marks: Vec<f64> = now.cue_times().into_iter().map(f64::from).collect();
        let Some((t, at)) =
            super::deck::mark_jump(&now.analysis.beats, self.position_ms(), &marks, forward, self.snap())
        else {
            self.toast("no more cues", Level::Info);
            return;
        };
        let color = (now.analysis.cues.iter())
            .filter(|c| f64::from(c.start_ms) == t)
            .max_by_key(|c| c.hot_cue.is_some())
            .map(|c| super::theme::palette().cue_color(c));
        self.jump(at, color);
    }

    /// Jump to `ms` into the playing track, flashing the visualiser in a cue's colour.
    pub(super) fn jump(&mut self, ms: f64, color: Option<[u8; 3]>) {
        let keep_time = self.snap() == super::deck::Snap::Phase;
        self.seek(Duration::from_micros((ms.max(0.0) * 1000.0).round() as u64), keep_time);
        self.last_pos_ms = ms;
        if let Some(color) = color {
            self.flash = Some((color, Instant::now()));
        }
    }

    fn change_volume(&mut self, delta: f32) {
        self.set_volume(((self.volume + delta) * 20.0).round() / 20.0);
    }

    fn set_volume(&mut self, v: f32) {
        self.volume = v.clamp(0.0, 1.5);
        if let Some(a) = &mut self.audio {
            a.set_volume(self.volume);
        }
    }

    /// Select the playing track in the list, switching to all tracks if needed. A recorded set
    /// (played from the Mixes view, on its own) is shown there instead.
    fn locate_playing(&mut self) {
        let Some(now) = &self.now else { return };
        if !self.playing_queue() {
            self.set_view(View::Mixes);
            return;
        }
        let (root, path) = (now.root.clone(), now.track.file_path.clone());
        self.locate(&root, &path);
    }

    /// Select a track of the drive at `root` in the library, switching to all tracks if needed.
    pub(super) fn locate(&mut self, root: &Path, path: &str) {
        let find = |app: &App| {
            let l = app.loaded.as_ref()?;
            if l.root() != root {
                return None;
            }
            app.visible.iter().position(|&i| l.library.tracks[i].file_path == path)
        };
        if let Some(i) = find(self) {
            self.track_state.select(Some(i));
        } else if self.loaded.as_ref().is_some_and(|l| l.root() == root) {
            // Not in the current view: show all tracks.
            self.query.clear();
            if let Some(l) = &mut self.loaded {
                l.node = NodeId::All;
            }
            self.playlist_state.select(Some(0));
            self.refresh_visible();
            if let Some(i) = find(self) {
                self.track_state.select(Some(i));
            }
        } else {
            self.toast("the track is on another drive", Level::Info);
            return;
        }
        self.view = View::Library;
        self.focus = Focus::Tracks;
    }

    // -----------------------------------------------------------------------------------------
    // Marks and the crate
    // -----------------------------------------------------------------------------------------

    fn toggle_mark(&mut self) {
        if self.focus == Focus::Playlists {
            return self.toggle_list_mark();
        }
        if self.focus != Focus::Tracks {
            return;
        }
        let Some(path) =
            self.selected_track().and_then(|i| Some(self.loaded.as_ref()?.library.tracks[i].file_path.clone()))
        else {
            return;
        };
        if !self.marks.remove(&path) {
            self.marks.insert(path);
        }
        self.move_selection(1);
    }

    /// `m` in the tree: mark the playlist or folder there (to pack it), or unmark it.
    fn toggle_list_mark(&mut self) {
        let Some(l) = &self.loaded else { return };
        let Some(NodeId::Playlist(id)) =
            self.playlist_state.selected().and_then(|i| l.tree.get(i)).map(|n| n.id.clone())
        else {
            return;
        };
        let path = l.playlist_path(id);
        match self.list_marks.iter().position(|p| *p == path) {
            Some(i) => {
                self.list_marks.remove(i);
            }
            None => self.list_marks.push(path),
        }
        self.move_selection(1);
    }

    /// Mark every track in view, or, with marks already, clear them.
    fn mark_all(&mut self) {
        let Some(l) = &self.loaded else { return };
        if self.marks.is_empty() {
            self.marks.extend(self.visible.iter().map(|&i| l.library.tracks[i].file_path.clone()));
            let n = self.marks.len();
            self.toast(format!("{n} marked"), Level::Info);
        } else {
            self.marks.clear();
            self.toast("marks cleared", Level::Info);
        }
    }

    /// Add tracks on the drive at `root` (by file path) to the crate.
    pub(super) fn add_to_crate(&mut self, root: &Path, paths: &[String]) {
        let Some(l) = self.loaded.as_ref().filter(|l| l.root() == root) else { return };
        let tracks: Vec<Track> =
            paths.iter().filter_map(|p| l.index_of(p)).map(|i| l.library.tracks[i].clone()).collect();
        let n = tracks.len();
        let drive = self.drive_label(root);
        let added = self.crate_list.add(root, &drive, tracks);
        let total = self.crate_list.len();
        let msg = match (added, n - added) {
            (0, _) => "already in the crate".to_string(),
            (a, 0) => format!("added {a} to the crate ({total} in all)"),
            (a, d) => format!("added {a} to the crate ({d} already there)"),
        };
        self.toast(msg, Level::Info);
        if self.crate_state.selected().is_none() && total > 0 {
            self.crate_state.select(Some(0));
        }
    }

    fn shift_crate(&mut self, delta: isize) {
        if let Some(i) = self.crate_state.selected() {
            let to = self.crate_list.shift(i, delta);
            self.crate_state.select(Some(to));
        }
    }

    fn remove_from_crate(&mut self) {
        let Some(i) = self.crate_state.selected() else { return };
        if self.crate_list.remove(i).is_some() {
            let len = self.crate_list.len();
            self.crate_state.select(if len == 0 { None } else { Some(i.min(len - 1)) });
        }
    }

    /// An action from a key rather than the menu: done if it can be, else say why not.
    fn quick(&mut self, action: Action) {
        let Some(t) = self.target() else { return };
        let menu = Menu::new(t, None);
        match menu.items.iter().find(|i| i.action == action).and_then(|i| i.unavailable.clone()) {
            Some(why) => self.toast(why, Level::Info),
            None => self.run_action(action, menu.target),
        }
    }

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

    // -----------------------------------------------------------------------------------------
    // Input
    // -----------------------------------------------------------------------------------------

    pub fn on_key(&mut self, key: KeyEvent) {
        // The player may have gone on to the next track since the last tick: keys act on the one
        // playing (a seek asked of the one that ended would be lost).
        self.follow_queue();
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        if ctrl && matches!(key.code, KeyCode::Char('c') | KeyCode::Char('q')) {
            self.request_quit();
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
        if self.modal.is_some() {
            self.on_modal_key(key);
            return;
        }
        if self.deck.outputs.is_some() {
            super::deck::on_outputs_key(self, key);
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

        if self.view == View::Profile && super::account::on_input(self, key) {
            return;
        }
        if self.view == View::Mixes && super::mix::on_input(self, key) {
            return;
        }
        let scopes = keymap::scopes(self);
        if let Some(b) = keymap::lookup(&scopes, &key) {
            self.perform(b.act, Some(key));
        }
    }

    /// Do what a key (or the command palette) asks for. `key` is the key that was pressed, for
    /// the actions that take something from it (which digit, whether shift was held).
    pub(super) fn perform(&mut self, act: Act, key: Option<KeyEvent>) {
        use super::{deck, mix, profile};
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
            Act::ViewKey => {
                if let Some(&v) = digit.and_then(|d| View::ALL.get(d as usize - 1)) {
                    self.set_view(v);
                }
            }
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
            Act::FullScreen => self.fullscreen = !self.fullscreen,
            Act::Zoom(true) => self.deck_secs = (self.deck_secs / 1.5).max(1.5),
            Act::Zoom(false) => self.deck_secs = (self.deck_secs * 1.5).min(60.0),
            Act::Move(dir) if in_profile => profile::move_by(self, dir as isize),
            Act::Move(dir) if in_mixes => mix::move_by(self, dir as isize),
            Act::Move(dir) => self.move_selection(dir as isize),
            Act::Page(dir) if in_profile => profile::move_by(self, 10 * dir as isize),
            Act::Page(dir) if in_mixes => mix::move_by(self, 10 * dir as isize),
            Act::Page(dir) => {
                let page = match self.focus {
                    Focus::Tracks if self.view == View::Library && !self.fullscreen => {
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
                View::Drives | View::Snapshots => {}
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
            Act::ExportXml => self.export_xml(),
            Act::Details => {
                self.show_details = !self.show_details;
                if !self.show_details && self.focus == Focus::Details {
                    self.focus = Focus::Tracks;
                }
            }
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
    fn refresh(&mut self) {
        self.refresh_drives();
        match self.view {
            View::Drives => self.refresh_status(),
            View::Snapshots => self.refresh_history(),
            View::Profile => super::profile::refresh(self),
            View::Mixes => super::mix::refresh(self),
            View::Library => {
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
        if (self.view == View::Profile && super::profile::escape(self))
            || (self.view == View::Mixes && super::mix::escape(self))
        {
            return;
        }
        if self.quit_after_write {
            self.quit_after_write = false;
            self.toast("staying: not quitting after the write", Level::Info);
        } else if self.fullscreen {
            self.fullscreen = false;
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

    /// Tab: drives, playlists, tracks, then the side panels that are open.
    fn cycle_focus(&mut self, back: bool) {
        let mut order = vec![Focus::Drives, Focus::Playlists, Focus::Tracks];
        if self.show_details {
            order.push(Focus::Details);
        }
        if self.show_crate {
            order.push(Focus::Crate);
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

    fn set_view(&mut self, v: View) {
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
        self.fullscreen = false;
        match v {
            View::Drives if self.status.is_none() => self.refresh_status(),
            View::Snapshots if self.history.is_none() => self.refresh_history(),
            View::Profile if self.profile.view.is_none() && !self.profile.loading => super::profile::refresh(self),
            _ => {}
        }
    }

    fn activate(&mut self) {
        match (self.view, self.focus) {
            (View::Library, _) if self.fullscreen => self.play_next(),
            (View::Library, Focus::Tracks) => self.play_selected(),
            (View::Library, Focus::Playlists) => self.open_playlist(),
            (View::Library, Focus::Details) => self.play_match(),
            (View::Library, Focus::Crate) => self.play_crate(),
            (View::Snapshots, _) => self.restore_from_selected_snapshot(),
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
        l.node = id;
        self.visible.clear();
        self.track_state.select(Some(0));
        *self.track_state.offset_mut() = 0;
        self.refresh_visible();
        self.focus = Focus::Tracks;
    }

    fn list_len(&self) -> usize {
        match (self.view, self.focus) {
            (View::Library, _) if self.fullscreen => {
                self.now.as_ref().and_then(|n| n.matches.as_ref()).map_or(0, |m| m.items.len())
            }
            (View::Library, Focus::Tracks) => self.visible.len(),
            (View::Library, Focus::Playlists) => self.loaded.as_ref().map_or(0, |l| l.tree.len()),
            (View::Library, Focus::Details) => {
                self.details.as_ref().filter(|_| self.details_current()).map_or(0, |d| d.matches.items.len())
            }
            (View::Library, Focus::Crate) => self.crate_list.len(),
            (View::Library, Focus::Drives) | (View::Drives, _) => self.drives.len(),
            (View::Snapshots, _) => self.history_rows(),
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

    fn list_selection(&mut self) -> (&mut Option<usize>, usize) {
        let len = self.list_len();
        let sel = match (self.view, self.focus) {
            (View::Library, _) if self.fullscreen => self.next_pick.selected_mut(),
            (View::Library, Focus::Tracks) => self.track_state.selected_mut(),
            (View::Library, Focus::Playlists) => self.playlist_state.selected_mut(),
            (View::Library, Focus::Details) => self.details_state.selected_mut(),
            (View::Library, Focus::Crate) => self.crate_state.selected_mut(),
            (View::Library, Focus::Drives) | (View::Drives, _) => self.drive_state.selected_mut(),
            (View::Snapshots, _) => self.history_state.selected_mut(),
            (View::Profile, _) => self.profile.sections.selected_mut(),
            (View::Mixes, _) => self.mix.list.selected_mut(),
        };
        (sel, len)
    }

    fn move_selection(&mut self, delta: isize) {
        if self.view == View::Drives {
            return self.move_drive_row(delta);
        }
        let (sel, len) = self.list_selection();
        if len == 0 {
            *sel = None;
            return;
        }
        let cur = sel.unwrap_or(0) as isize;
        *sel = Some((cur + delta).clamp(0, len as isize - 1) as usize);
    }

    pub fn on_mouse(&mut self, m: MouseEvent) {
        self.follow_queue(); // as for a key
        let p = Position::new(m.column, m.row);
        if self.help {
            match m.kind {
                MouseEventKind::ScrollUp => self.help_scroll = self.help_scroll.saturating_sub(3),
                MouseEventKind::ScrollDown => self.help_scroll = self.help_scroll.saturating_add(3),
                MouseEventKind::Down(_) => self.help = false,
                _ => {}
            }
            return;
        }
        if self.show_log {
            match m.kind {
                MouseEventKind::ScrollUp => self.log_scroll = self.log_scroll.saturating_sub(3),
                MouseEventKind::ScrollDown => self.log_scroll = self.log_scroll.saturating_add(3),
                MouseEventKind::Down(_) => self.show_log = false,
                _ => {}
            }
            return;
        }
        if self.modal.is_some() {
            self.on_modal_mouse(m);
            return;
        }
        if self.deck.outputs.is_some() {
            if matches!(m.kind, MouseEventKind::Down(_)) {
                self.deck.outputs = None;
            }
            return;
        }
        if self.view == View::Profile
            && !self.areas.tabs.iter().any(|(_, r)| r.contains(p))
            && super::profile::on_mouse(self, m)
        {
            return;
        }
        if self.view == View::Mixes
            && !self.areas.tabs.iter().any(|(_, r)| r.contains(p))
            && super::mix::on_mouse(self, m)
        {
            return;
        }
        match m.kind {
            MouseEventKind::ScrollUp | MouseEventKind::ScrollDown => {
                let delta = if m.kind == MouseEventKind::ScrollUp { -3 } else { 3 };
                if let Some(f) = self.pane_at(p).filter(|f| self.picking.is_none() || *f == Focus::Playlists) {
                    self.focus = f;
                }
                self.move_selection(delta);
            }
            MouseEventKind::Down(MouseButton::Left) => {
                // A second click is one on the row the last click was on (not on the same place in
                // another view or pane, or with the list scrolled), and a third starts over.
                let last = self.last_click.take();
                let again = |app: &mut Self, row| {
                    let again = last.is_some_and(|(t, was)| t.elapsed() < Duration::from_millis(400) && was == row);
                    if !again {
                        app.last_click = Some((Instant::now(), row));
                    }
                    again
                };
                if let Some(&(v, _)) = self.areas.tabs.iter().find(|(_, r)| r.contains(p)) {
                    if self.picking.is_none() {
                        self.set_view(v);
                    }
                } else if self.areas.overview.contains(p) {
                    self.seek_click(m.column);
                } else if self.fullscreen && self.areas.matches.contains(p) {
                    let i = self.next_pick.offset() + (m.row - self.areas.matches.y) as usize;
                    if i < self.list_len() {
                        self.next_pick.select(Some(i));
                        if again(self, (self.view, None, i)) {
                            self.play_next();
                        }
                    }
                } else if let Some(&(field, _)) = self.areas.columns.iter().find(|(_, r)| r.contains(p)) {
                    self.sort = browse::click(self.sort, field);
                    self.refresh_visible();
                } else if let Some(f) = self.pane_at(p) {
                    if self.picking.is_some() && f != Focus::Playlists {
                        return;
                    }
                    self.focus = f;
                    if let Some(i) = self.click_row(f, m.row)
                        && again(self, (self.view, Some(f), i))
                    {
                        if self.picking.is_some() { self.pick() } else { self.activate() }
                    }
                }
            }
            // Right-click: select what's under the pointer and open the actions menu there.
            MouseEventKind::Down(MouseButton::Right) if self.view == View::Library && self.picking.is_none() => {
                if let Some(f) = self.pane_at(p).filter(|f| *f != Focus::Drives) {
                    self.focus = f;
                    self.click_row(f, m.row);
                    self.open_menu(Some((m.column, m.row)));
                }
            }
            MouseEventKind::Drag(MouseButton::Left)
                if self.areas.overview.contains(p) && self.last_seek.elapsed() > Duration::from_millis(60) =>
            {
                self.seek_click(m.column);
            }
            _ => {}
        }
    }

    /// Clicks while a dialog is open: menu items can be clicked; a click elsewhere closes it.
    fn on_modal_mouse(&mut self, m: MouseEvent) {
        if let Some(Modal::Pick(_)) = &self.modal {
            if let Some(Modal::Pick(p)) = self.modal.take() {
                self.on_picker_mouse(p, m);
            }
            return;
        }
        if m.kind != MouseEventKind::Down(MouseButton::Left) {
            return;
        }
        match self.modal.take() {
            Some(Modal::Menu(menu)) => {
                let r = self.areas.menu;
                if !r.contains(Position::new(m.column, m.row)) {
                    return; // closed
                }
                let row = m.row.saturating_sub(r.y + 1) as usize;
                match menu.sub {
                    Some(sub) => {
                        let i = menu.sub_state.offset() + row;
                        match Menu::sub_items(sub).get(i).copied() {
                            Some(action) => self.run_action(action, menu.target),
                            None => self.modal = Some(Modal::Menu(menu)),
                        }
                    }
                    None => {
                        let i = menu.state.offset() + row;
                        if i < menu.items.len() {
                            self.choose(menu, i);
                        } else {
                            self.modal = Some(Modal::Menu(menu));
                        }
                    }
                }
            }
            other => self.modal = other,
        }
    }

    fn pane_at(&self, p: Position) -> Option<Focus> {
        match self.view {
            View::Library => [
                (self.areas.drives, Focus::Drives),
                (self.areas.playlists, Focus::Playlists),
                (self.areas.tracks, Focus::Tracks),
                (self.areas.details, Focus::Details),
                (self.areas.crate_panel, Focus::Crate),
            ]
            .into_iter()
            .find(|(r, _)| r.contains(p))
            .map(|(_, f)| f),
            View::Drives => self.areas.drive_list.contains(p).then_some(Focus::Drives),
            View::Snapshots => self.areas.history.contains(p).then_some(Focus::Tracks),
            View::Profile | View::Mixes => None,
        }
    }

    /// Select the row of `f` drawn at `row`, returning its index, or none when there's no row
    /// there.
    fn click_row(&mut self, f: Focus, row: u16) -> Option<usize> {
        // The first row of each list, and how far it's scrolled.
        let (top, offset) = match (self.view, f) {
            (View::Library, Focus::Tracks) => (self.areas.tracks.y + 2, self.track_state.offset()),
            (View::Library, Focus::Playlists) => (self.areas.playlists.y + 1, self.playlist_state.offset()),
            (View::Library, Focus::Details) => (self.areas.matches.y, self.details_state.offset()),
            (View::Library, Focus::Crate) => (self.areas.crate_rows.y, self.crate_state.offset()),
            (View::Library, Focus::Drives) => (self.areas.drives.y + 1, self.drive_state.offset()),
            (View::Drives, _) => (self.areas.drive_list.y + 1, self.drive_rows.offset()),
            (View::Snapshots, _) => (self.areas.history.y + 2, self.history_state.offset()),
            (View::Profile | View::Mixes, _) => return None,
        };
        if row < top || (f == Focus::Details && self.areas.matches.height == 0) {
            return None;
        }
        // The Drives view shows each row on two lines: a drive's name, then where it's mounted (a
        // library's, then how its clones stand).
        let lines = if self.view == View::Drives { 2 } else { 1 };
        let i = offset + (row - top) as usize / lines;
        if self.view == View::Drives {
            let &r = self.drive_rows().get(i)?;
            self.select_drive_row(r);
            return Some(i);
        }
        let (sel, len) = self.list_selection();
        (i < len).then(|| {
            *sel = Some(i);
            i
        })
    }

    fn seek_click(&mut self, column: u16) {
        let r = self.areas.overview;
        let Some(now) = &self.now else { return };
        let frac = (column.saturating_sub(r.x) as f64 + 0.5) / r.width.max(1) as f64;
        let target = now.duration.mul_f64(frac.clamp(0.0, 1.0));
        self.seek_to(target);
    }
}

fn plural_marks(n: usize) -> String {
    if n == 1 { "mark".into() } else { format!("{n} marks") }
}

#[cfg(test)]
mod props;
#[cfg(test)]
pub(super) mod testing;

#[cfg(test)]
mod tests;
