//! The Mixes view: a recorded set and which of the open drive's tracks it plays (what
//! `locallibrary mix` finds), drawn like the HTML report: the set's 3-band waveform with each
//! track's lane under it, the tempo and keys as played, the tracklist with every handoff, and the
//! selected track's own waveform with the stretch that was played. The set plays in the player,
//! and the timeline and tracklist follow along.

use rustc_hash::FxHashSet;
use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use locallibrary::anlz::{self, TrackAnalysis, Waveform};
use locallibrary::api;
use locallibrary::mix::export;
use locallibrary::mix::index::Cache;
use locallibrary::mix::report::{self, Drive};
use locallibrary::mix::{self, Identifier, MixReport, MixWaveform, Play, Transition};
use locallibrary::profile::ProfilePrefs;
use locallibrary::progress::{CancelToken, Ctx, Event, Phase};
use locallibrary::tokens::PLAYHEAD;
use locallibrary::tokens::bands::{self, HIGH, LOW, MID};
use locallibrary::{Device, Library, Store, Track, fsutil};
use loclib_waveform::{Art, Pixels, cue_letter};
use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{KeyCode, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::{Alignment, Constraint, Layout, Position, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Cell, List, ListItem, ListState, Paragraph, Row, Table, TableState, Wrap};

use super::app::{App, Level, Msg, edit_line, typed_tail};
use super::keymap::Act;
use super::keys::Camelot;
use super::theme::{self, bold, dim, faint, fg, rgb3, text};
use super::ui::{Scrolled, clip};
use crate::output::clock;

/// Recordings remembered between runs.
const RECENT: usize = 20;

/// Each play's lane colour, by its number.
const LANES: [[u8; 3]; 5] = [theme::PINK, theme::CYAN, theme::AMBER, theme::VIOLET, theme::GREEN];
const INK: [u8; 3] = [14, 14, 20];
const LOWER: [&str; 9] = [" ", "▁", "▂", "▃", "▄", "▅", "▆", "▇", "█"];

/// A recording to choose from.
#[derive(Debug, Clone, PartialEq)]
pub struct Source {
    pub path: PathBuf,
    pub name: String,
    /// Where it is: the drive it was recorded to, the folder it's in, or "opened before".
    pub place: String,
    pub duration_secs: Option<f64>,
    pub when: Option<String>,
    /// The folder opened that it was found in.
    pub folder: Option<PathBuf>,
}

impl Source {
    fn recent(path: PathBuf) -> Source {
        Source {
            name: file_name(&path),
            path,
            place: "opened before".into(),
            duration_secs: None,
            when: None,
            folder: None,
        }
    }

    /// A recording found in a folder opened, placed by the folder it's in (the set's, usually).
    fn found(folder: &Path, path: PathBuf) -> Source {
        let place = match path.parent().and_then(|p| p.strip_prefix(folder).ok()) {
            Some(p) if !p.as_os_str().is_empty() => p.display().to_string(),
            _ => file_name(folder),
        };
        let modified = std::fs::metadata(&path).and_then(|m| m.modified()).ok();
        Source {
            name: file_name(&path),
            place,
            duration_secs: None,
            when: modified.map(|t| chrono::DateTime::<chrono::Local>::from(t).format("%-d %b %Y").to_string()),
            folder: Some(folder.to_path_buf()),
            path,
        }
    }
}

fn file_name(p: &Path) -> String {
    p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| p.display().to_string())
}

/// An identified set, and what drawing it needs from the drive.
pub struct Opened {
    pub report: MixReport,
    /// The drive whose library it was matched against.
    pub root: PathBuf,
    library: Arc<Library>,
    /// Each play's analysis files from the drive (waveform, cues, phrases), in play order.
    pub analyses: Arc<[TrackAnalysis]>,
    pub cover: Option<Art>,
    /// What the player plays: the recording, or for an Audacity project an export next to it.
    pub audio: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pane {
    Mixes,
    Tracks,
}

/// How far identifying has got: the phase, and items and bytes done of their totals.
#[derive(Debug, Default)]
struct Progress {
    phase: Option<Phase>,
    items: (u64, u64),
    bytes: (u64, u64),
}

struct Loading {
    path: PathBuf,
    progress: Arc<Mutex<Progress>>,
    cancel: CancelToken,
    started: Instant,
}

/// Where things were drawn, for the mouse.
#[derive(Default)]
struct Areas {
    list: Rect,
    timeline: Rect,
    rows: Rect,
    /// The stretch of the set the timeline shows, in seconds.
    window: (f64, f64),
}

pub struct MixTab {
    tx: Sender<Msg>,
    pub sources: Vec<Source>,
    pub list: ListState,
    /// Recordings opened before, newest first.
    pub recent: Vec<PathBuf>,
    /// Folders of recordings opened, newest first: what's in them and their subfolders is listed.
    pub folders: Vec<PathBuf>,
    /// The folders `folder_sources` was (or is being) listed for.
    folders_listed: Option<Vec<PathBuf>>,
    folder_sources: Vec<Source>,
    /// A folder just opened: what was found in it is said once it's listed.
    announce: Option<PathBuf>,
    pub opened: Option<Result<Opened, String>>,
    loading: Option<Loading>,
    generation: u64,
    pub pane: Pane,
    pub rows: TableState,
    /// A path being typed (`o`).
    pub input: Option<String>,
    /// Timeline zoom: 1 shows the whole set.
    pub zoom: u8,
    /// The play under the playhead when last drawn, so the selection follows the set.
    following: Option<usize>,
    /// The drive whose recordings are listed.
    drive: Option<PathBuf>,
    /// The latest listing of the drive asked for: one started earlier can finish later, with
    /// what was there then, and is dropped.
    pub drive_listing: u64,
    drive_sources: Vec<Source>,
    areas: Areas,
}

impl MixTab {
    pub fn new(tx: Sender<Msg>) -> MixTab {
        MixTab {
            tx,
            sources: Vec::new(),
            list: ListState::default(),
            recent: Vec::new(),
            folders: Vec::new(),
            folders_listed: None,
            folder_sources: Vec::new(),
            announce: None,
            opened: None,
            loading: None,
            generation: 0,
            pane: Pane::Mixes,
            rows: TableState::default(),
            input: None,
            zoom: 1,
            following: None,
            drive: None,
            drive_listing: 0,
            drive_sources: Vec::new(),
            areas: Areas::default(),
        }
    }

    /// Recordings remembered from the last run (those still there).
    pub fn set_recent(&mut self, paths: &[PathBuf]) {
        self.recent = paths.iter().filter(|p| p.is_file()).take(RECENT).cloned().collect();
        self.rebuild_sources();
    }

    pub fn is_loading(&self) -> bool {
        self.loading.is_some()
    }

    /// What the header shows while a set is being identified.
    pub fn status(&self) -> Option<String> {
        let l = self.loading.as_ref()?;
        let p = l.progress.lock().ok()?;
        let pct = |(done, total): (u64, u64)| (total > 0).then(|| format!(" {}%", (done * 100 / total).min(100)));
        let name = file_name(&l.path);
        Some(match p.phase {
            Some(Phase::Fingerprint) if p.items.1 > 0 => {
                format!("Fingerprinting the drive's tracks… {}/{}", p.items.0, p.items.1)
            }
            Some(Phase::Fingerprint) => "Fingerprinting the drive's tracks…".into(),
            Some(Phase::Index) => "Indexing fingerprints…".into(),
            Some(Phase::Listen) => format!("Listening to {name}…{}", pct(p.bytes).or(pct(p.items)).unwrap_or_default()),
            Some(Phase::Match) => format!("Finding tracks in {name}…"),
            _ => format!("Opening {name}…"),
        })
    }

    fn report(&self) -> Option<&MixReport> {
        match &self.opened {
            Some(Ok(o)) => Some(&o.report),
            _ => None,
        }
    }

    fn selected_play(&self) -> Option<&Play> {
        self.report()?.plays.get(self.rows.selected()?)
    }

    fn rebuild_sources(&mut self) {
        let selected = self.list.selected().and_then(|i| self.sources.get(i)).map(|s| s.path.clone());
        let recent = self.recent.iter().map(|p| Source::recent(p.clone()));
        let mut seen = FxHashSet::default();
        self.sources = (self.drive_sources.iter().chain(&self.folder_sources).cloned().chain(recent))
            .filter(|s| seen.insert(s.path.clone()))
            .collect();
        let i = selected
            .and_then(|p| self.sources.iter().position(|s| s.path == p))
            .or((!self.sources.is_empty()).then_some(0));
        self.list.select(i);
    }

    /// The open drive's recordings, read in the background.
    pub fn drive_listed(&mut self, root: PathBuf, sources: Vec<Source>) {
        if self.drive.as_ref() == Some(&root) {
            self.drive_sources = sources;
            self.rebuild_sources();
        }
    }

    fn remember(&mut self, path: &Path) {
        self.recent.retain(|p| p != path);
        self.recent.insert(0, path.to_path_buf());
        self.recent.truncate(RECENT);
        self.rebuild_sources();
    }

    fn move_by(&mut self, delta: isize) {
        let step = |sel: Option<usize>, n: usize| {
            (n > 0).then(|| (sel.unwrap_or(0) as isize + delta).clamp(0, n as isize - 1) as usize)
        };
        match self.pane {
            Pane::Mixes => self.list.select(step(self.list.selected(), self.sources.len())),
            Pane::Tracks => {
                let n = self.report().map_or(0, |r| r.plays.len());
                self.rows.select(step(self.rows.selected(), n));
            }
        }
    }

    fn cancel(&mut self) {
        if let Some(l) = self.loading.take() {
            l.cancel.cancel();
            // Whatever the cancelled run sends back is for an older generation.
            self.generation += 1;
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Opening and identifying
// ---------------------------------------------------------------------------------------------

/// `R`: list the open drive's recordings and the folders' again.
pub fn refresh(app: &mut App) {
    list_drive(app);
    list_folders(app);
}

/// List the open drive's recordings (in the background), next to the folders' and the ones
/// opened before.
fn list_drive(app: &mut App) {
    let root = app.loaded.as_ref().filter(|l| !l.stale).map(|l| l.root().to_path_buf());
    let name = root.as_ref().map(|r| app.drive_label(r)).unwrap_or_default();
    let t = &mut app.mix;
    t.drive = root.clone();
    t.drive_listing += 1;
    let listing = t.drive_listing;
    t.drive_sources.clear();
    t.rebuild_sources();
    let Some(root) = root else { return };
    app.spawn(move || {
        let sources = api::recordings_list(&api::DriveRef { drive: root.display().to_string() })
            .map(|recs| {
                recs.into_iter()
                    .map(|r| Source {
                        path: root.join(r.path.trim_start_matches('/')),
                        name: file_name(Path::new(&r.path)),
                        place: name.clone(),
                        duration_secs: r.duration_secs,
                        when: Some(r.local_time("%-d %b %H:%M")),
                        folder: None,
                    })
                    .collect()
            })
            .unwrap_or_default();
        Msg::MixSources(listing, root, sources)
    });
}

/// Open what was typed: a recording, or a folder whose recordings (in its subfolders too) stay
/// in the list.
fn open_path(app: &mut App, path: PathBuf) {
    if !path.is_dir() {
        open(app, path);
        return;
    }
    let dir = std::path::absolute(&path).unwrap_or(path);
    let t = &mut app.mix;
    t.folders.retain(|f| f != &dir);
    t.folders.insert(0, dir.clone());
    t.announce = Some(dir);
    list_folders(app);
}

/// List what's in the folders opened (in the background).
fn list_folders(app: &mut App) {
    let folders = app.mix.folders.clone();
    app.mix.folders_listed = Some(folders.clone());
    app.spawn(move || {
        let sources =
            folders.iter().flat_map(|d| mix::recordings_in(d).into_iter().map(|p| Source::found(d, p))).collect();
        Msg::MixFolders(folders, sources)
    });
}

/// The folders' recordings, listed (ignored when the folders changed since). A folder just opened
/// is said to have them, or forgotten when it has none.
pub fn folders_listed(app: &mut App, folders: Vec<PathBuf>, sources: Vec<Source>) {
    let t = &mut app.mix;
    if folders != t.folders {
        return;
    }
    t.folder_sources = sources;
    t.rebuild_sources();
    let Some(dir) = t.announce.take() else { return };
    let n = t.folder_sources.iter().filter(|s| s.folder.as_ref() == Some(&dir)).count();
    if n == 0 {
        t.folders.retain(|f| f != &dir);
        t.folders_listed = Some(t.folders.clone());
        app.toast(format!("no recordings in {}", dir.display()), Level::Info);
        return;
    }
    t.list.select(t.sources.iter().position(|s| s.folder.as_ref() == Some(&dir)));
    t.pane = Pane::Mixes;
    app.toast(format!("{n} recording{} in {}", if n == 1 { "" } else { "s" }, dir.display()), Level::Info);
}

/// `del` on a recording: forget the folder it was found in, or the recording when it was opened
/// on its own. The drive's own stay.
pub fn forget(app: &mut App) {
    let t = &mut app.mix;
    let Some(s) = t.list.selected().and_then(|i| t.sources.get(i)).filter(|_| t.pane == Pane::Mixes).cloned() else {
        return;
    };
    let forgot = if let Some(dir) = &s.folder {
        t.folders.retain(|f| f != dir);
        t.folders_listed = Some(t.folders.clone());
        t.folder_sources.retain(|x| x.folder.as_ref() != Some(dir));
        dir.display().to_string()
    } else if t.recent.contains(&s.path) {
        t.recent.retain(|p| p != &s.path);
        s.name
    } else {
        return;
    };
    t.rebuild_sources();
    app.toast(format!("forgot {forgot}"), Level::Info);
}

/// What a path typed or dropped into the terminal means: quotes and backslash escapes taken off,
/// `~` for the home folder.
pub(super) fn typed_path(s: &str) -> PathBuf {
    let s = s.trim();
    let s = s
        .strip_prefix('\'')
        .and_then(|x| x.strip_suffix('\''))
        .or_else(|| s.strip_prefix('"').and_then(|x| x.strip_suffix('"')))
        .map(str::to_string)
        .unwrap_or_else(|| {
            let mut out = String::new();
            let mut chars = s.chars();
            while let Some(c) = chars.next() {
                if c == '\\' {
                    out.extend(chars.next());
                } else {
                    out.push(c);
                }
            }
            out
        });
    match s.strip_prefix("~/") {
        Some(rest) => std::env::var_os("HOME").map(|h| PathBuf::from(h).join(rest)).unwrap_or_else(|| s.clone().into()),
        None => s.into(),
    }
}

fn open_selected(app: &mut App) {
    let Some(s) = app.mix.list.selected().and_then(|i| app.mix.sources.get(i)) else {
        app.mix.input = Some(String::new());
        return;
    };
    let path = s.path.clone();
    // Shown already, with tracks to go through: to them. Shown with none found, it's identified
    // again (the drive open may be another by now).
    let shown = matches!(&app.mix.opened, Some(Ok(o)) if o.report.mix.path == path && !o.report.plays.is_empty());
    if shown && app.mix.loading.is_none() {
        app.mix.pane = Pane::Tracks;
    } else {
        open(app, path);
    }
}

/// Identify a recording against the open drive's library, in the background.
pub fn open(app: &mut App, path: PathBuf) {
    if !path.is_file() {
        app.toast(format!("{} doesn't exist", path.display()), Level::Error);
        return;
    }
    if !mix::is_mix_file(&path) {
        app.toast(
            format!("{} isn't a recording (WAV, AIFF, MP3, M4A, FLAC, or an Audacity project)", file_name(&path)),
            Level::Error,
        );
        return;
    }
    let Some(l) = app.loaded.as_ref().filter(|l| !l.stale) else {
        app.toast("open a drive in the library first: the set's tracks are looked for in its library", Level::Info);
        return;
    };
    let (root, library) = (l.root().to_path_buf(), l.library.clone());
    let store_root = app.store_root.clone();
    let t = &mut app.mix;
    t.cancel();
    t.generation += 1;
    let generation = t.generation;
    let progress = Arc::new(Mutex::new(Progress::default()));
    let cancel = CancelToken::new();
    t.loading = Some(Loading {
        path: path.clone(),
        progress: progress.clone(),
        cancel: cancel.clone(),
        started: Instant::now(),
    });
    let tx = t.tx.clone();
    let job = move || {
        let r = identify(&store_root, &root, library, &path, &progress, cancel)
            .map_err(|e| format!("{}: {e}", file_name(&path)));
        Msg::Mix(generation, Box::new(r))
    };
    super::app::worker(tx, job, move |e| Msg::Mix(generation, Box::new(Err(e))));
}

fn identify(
    store_root: &Path,
    root: &Path,
    library: Arc<Library>,
    path: &Path,
    progress: &Mutex<Progress>,
    cancel: CancelToken,
) -> locallibrary::Result<Opened> {
    let on_event = |e: &Event| {
        let Ok(mut p) = progress.lock() else { return };
        match e {
            Event::PhaseStart { phase, items, bytes } => {
                *p = Progress { phase: Some(*phase), items: (0, *items), bytes: (0, *bytes) }
            }
            Event::Item { .. } => p.items.0 += 1,
            Event::Bytes { bytes, .. } => p.bytes.0 += bytes,
            _ => {}
        }
    };
    let ctx = Ctx::new(&on_event, cancel);
    let device = Device::open(root)?;
    // Fingerprints are cached in the store, made here if there's none yet.
    let store = Store::open_or_init(store_root)?;
    let id = Identifier::with_library(&device, library.clone(), &Cache::in_store(&store), &ctx)?;
    let report = id.identify(path, &mix::Options::default(), &ctx)?;
    let analyses = report.plays.iter().map(|p| anlz::load(&device, &p.track)).collect();
    let cover = report.mix.cover.as_ref().and_then(|c| c.read().ok()).and_then(|b| Art::decode(&b));
    // The player plays the recording itself; for a project, the audio exported from it.
    let audio = match mix::aup::is_project(&report.mix.path) {
        true => report::project_audio(&report.mix.path),
        false => Some(report.mix.path.clone()),
    };
    Ok(Opened { report, root: device.root().to_path_buf(), library, analyses, cover, audio })
}

/// A finished identification (ignored when another was started since).
pub fn loaded(app: &mut App, generation: u64, r: Result<Opened, String>) {
    let t = &mut app.mix;
    if generation != t.generation {
        return;
    }
    let Some(loading) = t.loading.take() else { return };
    match r {
        Ok(o) => {
            t.remember(&loading.path);
            let r = &o.report;
            let n = r.plays.len();
            let msg = format!(
                "{}: {n} track{} found, {:.0}% of the set",
                r.mix.title,
                if n == 1 { "" } else { "s" },
                r.coverage * 100.0
            );
            t.rows = TableState::default().with_selected((n > 0).then_some(0));
            t.pane = if n > 0 { Pane::Tracks } else { Pane::Mixes };
            t.following = None;
            t.zoom = 1;
            t.opened = Some(Ok(o));
            app.toast(msg, Level::Info);
        }
        Err(e) => t.opened = Some(Err(e)),
    }
}

// ---------------------------------------------------------------------------------------------
// Playing
// ---------------------------------------------------------------------------------------------

/// Whether the player has the open set loaded (playing or paused).
fn set_loaded(app: &App) -> bool {
    let (Some(Ok(o)), Some(now)) = (&app.mix.opened, &app.now) else { return false };
    o.audio.as_ref().is_some_and(|a| Path::new(&now.track.file_path) == a.as_path())
}

/// Where the playhead is in the set, when the set is what's loaded in the player.
fn playhead(app: &App) -> Option<f64> {
    set_loaded(app).then(|| app.position().as_secs_f64())
}

/// The play heard at `t`: the one that came in last, of those playing then.
fn play_at(plays: &[Play], t: f64) -> Option<usize> {
    plays
        .iter()
        .enumerate()
        .filter(|(_, p)| p.start <= t && t < p.end)
        .max_by(|a, b| a.1.start.total_cmp(&b.1.start))
        .map(|(i, _)| i)
}

/// The set as a track for the player.
fn set_track(r: &MixReport, audio: &Path) -> Track {
    let m = &r.mix;
    Track {
        id: 0,
        title: m.title.clone(),
        artist: m.artist.clone().unwrap_or_default(),
        album: String::new(),
        album_artist: String::new(),
        genre: m.genre.clone().unwrap_or_default(),
        label: String::new(),
        key: String::new(),
        remixer: String::new(),
        original_artist: String::new(),
        composer: String::new(),
        comment: String::new(),
        mix_name: String::new(),
        isrc: String::new(),
        bpm: 0.0,
        duration_secs: m.duration_secs.round() as u32,
        year: 0,
        track_number: 0,
        disc_number: 0,
        rating: 0,
        color: None,
        play_count: 0,
        bitrate: 0,
        sample_rate: m.sample_rate,
        sample_depth: 0,
        file_size: std::fs::metadata(audio).map_or(0, |md| md.len()),
        file_path: audio.display().to_string(),
        analyze_path: String::new(),
        date_added: String::new(),
        release_date: m.date.clone().unwrap_or_default(),
        artwork_id: None,
        artwork_path: None,
    }
}

/// Play the set from `secs` (seeking when it's already loaded).
fn play_from(app: &mut App, secs: f64) {
    let Some(Ok(o)) = &app.mix.opened else { return };
    let Some(audio) = o.audio.clone() else {
        app.toast("nothing to play: export the project's audio next to it, named like it", Level::Info);
        return;
    };
    let at = Duration::from_secs_f64(secs.max(0.0));
    if set_loaded(app) {
        app.seek_to(at);
        if let Some(a) = app.audio.as_ref().filter(|a| a.is_paused()) {
            a.toggle_pause();
        }
    } else {
        let track = set_track(&o.report, &audio);
        app.play_file(&audio, track, at);
    }
    app.mix.following = None;
}

/// Seek to the next play, or back to the start of this one (or the one before, near its start).
fn jump(app: &mut App, forward: bool) {
    let Some(pos) = playhead(app) else { return };
    let Some(r) = app.mix.report() else { return };
    let target = if forward {
        r.plays.iter().map(|p| p.start).find(|s| *s > pos + 0.5)
    } else {
        let current = play_at(&r.plays, pos).map(|i| r.plays[i].start);
        match current {
            Some(s) if pos - s > 3.0 => Some(s),
            _ => r.plays.iter().map(|p| p.start).rfind(|s| *s < pos - 3.0).or(Some(0.0)),
        }
    };
    match target {
        Some(t) => play_from(app, t),
        None => app.toast("that's the last track in the set", Level::Info),
    }
}

// ---------------------------------------------------------------------------------------------
// Other actions
// ---------------------------------------------------------------------------------------------

fn locate(app: &mut App) {
    let Some(Ok(o)) = &app.mix.opened else { return };
    let Some(p) = app.mix.selected_play() else { return };
    let (root, path) = (o.root.clone(), p.track.file_path.clone());
    app.locate(&root, &path);
}

/// What writing the set's report or export (to the store's reports folder, as `locallibrary mix
/// --open` and `--export` do) needs, read on a worker: the store, the DJ's profile choices and the
/// drive the set was matched against, which must still be the one at its root.
fn for_writing(store_root: &Path, report: &MixReport, root: &Path) -> Result<(Store, ProfilePrefs, Device), String> {
    let device = Device::open(root).map_err(|e| format!("the set's tracks are read from its drive: {e}"))?;
    if !report.matched_against(&device) {
        return Err(format!("the drive at {} isn't the one the set was matched against", root.display()));
    }
    let store = Store::open_or_init(store_root).map_err(|e| e.to_string())?;
    let prefs = ProfilePrefs::load(&store).map_err(|e| e.to_string())?;
    Ok((store, prefs, device))
}

/// Save the set as data for Local Network (as `locallibrary mix --export` does), in the
/// background: it reads the drive.
fn export_data(app: &mut App) {
    let Some(Ok(o)) = &app.mix.opened else {
        app.toast("open a set first", Level::Info);
        return;
    };
    let (report, library, analyses, root) = (o.report.clone(), o.library.clone(), o.analyses.clone(), o.root.clone());
    let store_root = app.store_root.clone();
    app.toast(format!("saving {}.mix.json…", report::id(&report)), Level::Info);
    app.spawn(move || {
        let (store, prefs, device) = match for_writing(&store_root, &report, &root) {
            Ok(w) => w,
            Err(e) => return Msg::Note(e, Level::Error),
        };
        let drive = Drive { device: &device, library: &library, analyses: Some(&analyses) };
        let doc = export::build(&report, Some(drive), &prefs);
        let path = report::report_file(&store, &report, "mix.json");
        let r = serde_json::to_vec(&doc).map_err(|e| e.to_string()).and_then(|json| write_file(&path, &json));
        match r {
            Ok(()) => {
                Msg::Note(format!("wrote {} (attach it to the mix on Local Network)", path.display()), Level::Info)
            }
            Err(e) => Msg::Note(e, Level::Error),
        }
    });
}

fn write_file(path: &Path, bytes: &[u8]) -> Result<(), String> {
    fsutil::write_atomic(path, bytes).map_err(|e| e.to_string())
}

/// Write the HTML report to the store's reports folder and open it in a browser, in the
/// background: it reads the drive's waveforms.
fn html_report(app: &mut App) {
    let Some(Ok(o)) = &app.mix.opened else {
        app.toast("open a set first", Level::Info);
        return;
    };
    let (report, library, analyses, root) = (o.report.clone(), o.library.clone(), o.analyses.clone(), o.root.clone());
    let store_root = app.store_root.clone();
    app.toast(format!("writing {}.html…", report::id(&report)), Level::Info);
    app.spawn(move || {
        let (store, prefs, device) = match for_writing(&store_root, &report, &root) {
            Ok(w) => w,
            Err(e) => return Msg::Note(e, Level::Error),
        };
        let drive = Drive { device: &device, library: &library, analyses: Some(&analyses) };
        let path = report::report_file(&store, &report, "html");
        let audio = report::playable_audio(&report.mix.path);
        let page = report::page(&report, Some(drive), audio.as_deref(), &path, &prefs, false);
        if let Err(e) = write_file(&path, page.as_bytes()) {
            return Msg::Note(e, Level::Error);
        }
        match locallibrary::opener::open_file(&path) {
            Ok(()) => Msg::Note(format!("opened {}", path.display()), Level::Info),
            Err(e) => Msg::Note(format!("wrote {}, but couldn't open it: {e}", path.display()), Level::Error),
        }
    });
}

// ---------------------------------------------------------------------------------------------
// Input
// ---------------------------------------------------------------------------------------------

/// Keys while a path is being typed: every one goes to it until enter or esc. Returns whether
/// the key was used.
pub fn on_input(app: &mut App, key: KeyEvent) -> bool {
    let Some(text) = &mut app.mix.input else { return false };
    match key.code {
        KeyCode::Esc => app.mix.input = None,
        KeyCode::Enter => {
            let path = typed_path(text);
            app.mix.input = None;
            if !path.as_os_str().is_empty() {
                open_path(app, path);
            }
        }
        _ => {
            edit_line(text, key);
        }
    }
    true
}

/// Up, down, a page or to an end (`delta` rows) in the list with focus.
pub fn move_by(app: &mut App, delta: isize) {
    app.mix.move_by(delta);
}

/// Tab: between the recordings and the set's tracks.
pub fn switch_pane(app: &mut App) {
    app.mix.pane = match app.mix.pane {
        Pane::Mixes if app.mix.report().is_some_and(|r| !r.plays.is_empty()) => Pane::Tracks,
        _ => Pane::Mixes,
    }
}

/// Enter: on a recording, find its tracks; on a track, play the set from there.
pub fn enter(app: &mut App) {
    if app.mix.pane == Pane::Tracks {
        if let Some(start) = app.mix.selected_play().map(|p| p.start) {
            play_from(app, start);
        }
    } else {
        open_selected(app);
    }
}

/// Space with nothing playing and a set open: play it (from the selected track). Returns
/// whether it did; otherwise space pauses as anywhere.
pub fn play(app: &mut App) -> bool {
    if app.now.is_some() || app.mix.report().is_none() {
        return false;
    }
    let tracks = app.mix.pane == Pane::Tracks;
    let start = app.mix.selected_play().filter(|_| tracks).map_or(0.0, |p| p.start);
    play_from(app, start);
    true
}

/// `n` and `p` while the set plays: its next or previous track. Returns whether it did;
/// otherwise they move through the queue as anywhere.
pub fn track_jump(app: &mut App, forward: bool) -> bool {
    if !set_loaded(app) {
        return false;
    }
    jump(app, forward);
    true
}

/// `o` with a set open: the selected track in the library. Returns whether it did; otherwise
/// `o` goes to the playing track as anywhere.
pub fn go_to(app: &mut App) -> bool {
    if app.mix.report().is_none() {
        return false;
    }
    locate(app);
    true
}

/// Esc: stop identifying a set, or go back to the recordings. Returns whether there was either.
pub fn escape(app: &mut App) -> bool {
    if app.mix.loading.is_some() {
        app.mix.cancel();
        app.toast("stopped", Level::Info);
    } else if app.mix.pane == Pane::Tracks {
        app.mix.pane = Pane::Mixes;
    } else {
        return false;
    }
    true
}

/// The Mixes view's own actions.
pub fn perform(app: &mut App, act: Act) {
    match act {
        Act::MixOpen => app.mix.input = Some(String::new()),
        Act::MixZoom => {
            app.mix.zoom = match app.mix.zoom {
                1 => 2,
                2 => 4,
                4 => 8,
                _ => 1,
            }
        }
        Act::MixExport => export_data(app),
        Act::MixReport => html_report(app),
        _ => {}
    }
}

pub fn on_mouse(app: &mut App, m: MouseEvent) -> bool {
    let p = Position::new(m.column, m.row);
    let a = &app.mix.areas;
    let (in_list, in_rows, in_time) = (a.list.contains(p), a.rows.contains(p), a.timeline.contains(p));
    if !(in_list || in_rows || in_time) {
        return false;
    }
    match m.kind {
        MouseEventKind::ScrollUp | MouseEventKind::ScrollDown => {
            app.mix.pane = if in_list { Pane::Mixes } else { Pane::Tracks };
            app.mix.move_by(if m.kind == MouseEventKind::ScrollUp { -3 } else { 3 });
        }
        MouseEventKind::Down(MouseButton::Left) if in_list => {
            app.mix.pane = Pane::Mixes;
            let a = app.mix.areas.list;
            // Two lines per recording, under the border.
            if m.row > a.y {
                let i = (m.row - a.y - 1) as usize / 2 + app.mix.list.offset();
                if i < app.mix.sources.len() {
                    if app.mix.list.selected() == Some(i) {
                        open_selected(app);
                    } else {
                        app.mix.list.select(Some(i));
                    }
                }
            }
        }
        MouseEventKind::Down(MouseButton::Left) if in_rows => {
            app.mix.pane = Pane::Tracks;
            // The header row sits under the border; each track takes two lines.
            let top = app.mix.areas.rows.y + 2;
            if m.row >= top {
                let i = (m.row - top) as usize / 2 + app.mix.rows.offset();
                if app.mix.report().is_some_and(|r| i < r.plays.len()) {
                    app.mix.rows.select(Some(i));
                }
            }
        }
        MouseEventKind::Down(MouseButton::Left) => {
            let (r, (from, to)) = (app.mix.areas.timeline, app.mix.areas.window);
            let t = from + (m.column.saturating_sub(r.x) as f64 + 0.5) / r.width.max(1) as f64 * (to - from);
            if set_loaded(app) {
                play_from(app, t);
            } else if let Some(i) = app.mix.report().and_then(|r| play_at(&r.plays, t)) {
                app.mix.pane = Pane::Tracks;
                app.mix.rows.select(Some(i));
            }
        }
        _ => {}
    }
    true
}

// ---------------------------------------------------------------------------------------------
// Drawing
// ---------------------------------------------------------------------------------------------

/// What drawing needs from outside the tab.
struct Context {
    /// Where the playhead is, when the set is loaded in the player.
    playhead: Option<f64>,
    drive: Option<String>,
    status: Option<String>,
}

pub fn draw(f: &mut Frame, app: &mut App, area: Rect) {
    // The open drive's recordings are listed with the folders': read them again when another
    // drive is open or the folders changed.
    let root = app.loaded.as_ref().filter(|l| !l.stale).map(|l| l.root().to_path_buf());
    if root != app.mix.drive {
        list_drive(app);
    }
    if app.mix.folders_listed.as_ref() != Some(&app.mix.folders) {
        list_folders(app);
    }
    let cx =
        Context { playhead: playhead(app), drive: root.as_ref().map(|r| app.drive_label(r)), status: app.mix.status() };
    draw_tab(f, &mut app.mix, area, &cx);
}

fn block(title: Line<'static>, focused: bool) -> Block<'static> {
    Block::bordered().border_type(theme::border_type(focused)).border_style(theme::border(focused)).title(title)
}

fn title(s: &str, focused: bool) -> Line<'static> {
    let style = if focused { bold(theme::CYAN) } else { bold(theme::DIM) };
    Line::from(vec![Span::raw(" "), Span::styled(s.to_string(), style), Span::raw(" ")])
}

fn draw_tab(f: &mut Frame, t: &mut MixTab, area: Rect, cx: &Context) {
    // The selection follows the set as it plays into each track.
    if let (Some(pos), Some(r)) = (cx.playhead, t.report()) {
        let now = play_at(&r.plays, pos);
        if now.is_some() && now != t.following {
            t.rows.select(now);
        }
        t.following = now;
    }
    let wide = area.width >= 110;
    let show_list = wide || t.pane == Pane::Mixes || t.report().is_none();
    let side_w = if show_list { 32.min(area.width / 2).max(20.min(area.width)) } else { 0 };
    let [side, main] = Layout::horizontal([Constraint::Length(side_w), Constraint::Min(0)]).areas(area);
    t.areas.list = side;
    if show_list {
        draw_list(f, t, side, cx);
    }
    t.areas.timeline = Rect::default();
    t.areas.rows = Rect::default();
    if main.width < 10 {
        return;
    }
    if let Some(l) = &t.loading {
        let mut lines = vec![
            Line::raw(""),
            Line::styled(cx.status.clone().unwrap_or_default(), bold(theme::FG)),
            Line::styled(format!("{}s", l.started.elapsed().as_secs()), faint()),
            Line::raw(""),
            Line::styled("Fingerprints are kept in the store: only tracks new to it take time.", dim()),
        ];
        lines.push(Line::from(vec![Span::styled("esc ", theme::key()), Span::styled("stops", dim())]));
        let p = Paragraph::new(lines).alignment(Alignment::Center).wrap(Wrap { trim: true });
        f.render_widget(p.block(block(title("Mix", false), false)), main);
        return;
    }
    match &t.opened {
        None => {
            let drive = match &cx.drive {
                Some(d) => format!("Tracks are looked for in the open drive's library ({d})."),
                None => "Open a drive in the library first (1): tracks are looked for in its library.".into(),
            };
            let lines = vec![
                Line::raw(""),
                Line::styled("A recorded set, and the tracks it plays", bold(theme::FG)),
                Line::raw(""),
                Line::from(vec![
                    Span::styled("Pick a recording, or press ", dim()),
                    Span::styled("O", theme::key()),
                    Span::styled(" to open a file or a folder.", dim()),
                ]),
                Line::styled(drive, dim()),
                Line::styled(
                    "You get the set's waveform with each track's lane, tempo and keys as played, and every handoff.",
                    faint(),
                ),
            ];
            let p = Paragraph::new(lines).alignment(Alignment::Center).wrap(Wrap { trim: true });
            f.render_widget(p.block(block(title("Mix", false), false)), main);
        }
        Some(Err(e)) => {
            let lines =
                vec![Line::raw(""), Line::styled(e.clone(), fg(theme::RED)), Line::styled("enter tries again", dim())];
            let p = Paragraph::new(lines).alignment(Alignment::Center).wrap(Wrap { trim: true });
            f.render_widget(p.block(block(title("Mix", false), false)), main);
        }
        Some(Ok(_)) => draw_report(f, t, main, cx),
    }
}

fn draw_list(f: &mut Frame, t: &mut MixTab, area: Rect, cx: &Context) {
    let focused = t.pane == Pane::Mixes;
    let b = block(title("Recordings", focused), focused)
        .title_bottom(Line::from(vec![Span::styled(" O ", theme::key()), Span::styled("open… ", faint())]));
    let mut inner = b.inner(area);
    f.render_widget(b, area);
    if let Some(text_) = &t.input {
        let [input, rest] = Layout::vertical([Constraint::Length(3), Constraint::Min(0)]).areas(inner);
        let shown = typed_tail(text_, (input.width as usize).saturating_sub(1));
        let lines = vec![
            Line::styled("Open a recording or folder:", bold(theme::AMBER)),
            Line::from(vec![Span::styled(shown.to_string(), text()), Span::styled("▏", fg(theme::PINK))]),
            Line::styled("enter open · esc cancel", faint()),
        ];
        f.render_widget(Paragraph::new(lines), input);
        inner = rest;
    }
    let opened = match &t.opened {
        Some(Ok(o)) => Some(o.report.mix.path.clone()),
        _ => None,
    };
    let loading = t.loading.as_ref().map(|l| l.path.clone());
    if t.sources.is_empty() {
        let lines = vec![
            Line::styled(
                match &cx.drive {
                    Some(d) => format!("No recordings on {d}."),
                    None => "No drive open.".into(),
                },
                dim(),
            ),
            Line::raw(""),
            Line::styled("Press O to open a recording, or a folder of them.", faint()),
        ];
        f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: true }), inner);
        t.areas.list = area;
        return;
    }
    let w = inner.width.saturating_sub(2) as usize;
    let items: Vec<ListItem> = t
        .sources
        .iter()
        .map(|s| {
            let mark = if loading.as_ref() == Some(&s.path) {
                Span::styled("⟳ ", fg(theme::CYAN))
            } else if opened.as_ref() == Some(&s.path) {
                Span::styled("● ", fg(theme::GREEN))
            } else {
                Span::raw("  ")
            };
            let mut facts = Vec::new();
            if let Some(d) = s.duration_secs {
                facts.push(clock(d));
            }
            facts.push(s.place.clone());
            if let Some(w) = &s.when {
                facts.push(w.clone());
            }
            ListItem::new(vec![
                Line::from(vec![mark, Span::styled(clip(&s.name, w.saturating_sub(2)), text())]),
                Line::styled(format!("  {}", clip(&facts.join(" · "), w.saturating_sub(2))), faint()),
            ])
        })
        .collect();
    let list = List::new(items)
        .highlight_style(if focused { theme::selected() } else { Style::new().add_modifier(Modifier::BOLD) })
        .highlight_symbol(Span::styled("▍", fg(theme::PINK)));
    // Two lines a recording.
    t.list.fit(inner.height as usize / 2, t.sources.len());
    f.render_stateful_widget(list, inner, &mut t.list);
}

/// `30s`, or `m:ss` from a minute.
fn secs(x: f64) -> String {
    if x < 59.5 { format!("{x:.0}s") } else { clock(x) }
}

/// The stretch of the set the timeline shows: all of it, or `1/zoom` of it around `focus`.
fn window(total: f64, zoom: u8, focus: f64) -> (f64, f64) {
    let total = total.max(1.0);
    let span = total / zoom.max(1) as f64;
    let start = (focus - span / 2.0).clamp(0.0, (total - span).max(0.0));
    (start, start + span)
}

/// Green for keys that mix, red for a clash.
fn harmony_color(h: &str) -> [u8; 3] {
    if h == "clash" { theme::RED } else { theme::GREEN }
}

/// How long nothing in the library was heard in the gap of a transition that has one.
fn unknown_in(r: &MixReport, tr: &Transition) -> Option<f64> {
    let (a, b) = (r.plays.get(tr.from)?, r.plays.get(tr.to)?);
    let heard: f64 =
        r.unidentified.iter().map(|g| (g.end.min(b.start + 1.0) - g.start.max(a.end - 1.0)).max(0.0)).sum();
    (tr.overlap <= -1.0 && heard >= 1.0).then_some(heard)
}

/// How one play hands over to the next: a blend, a cut, or a gap (with something not in the
/// library playing in it, perhaps), the keys (`keys` names them), the tempo.
fn handoff(tr: &Transition, r: &MixReport, keys: bool) -> Vec<Span<'static>> {
    let plays = &r.plays;
    let how = if tr.overlap >= 1.0 {
        format!("{} blend", secs(tr.overlap))
    } else if tr.overlap > -1.0 {
        "cut".to_string()
    } else if let Some(unknown) = unknown_in(r, tr) {
        format!("{} not in the library", secs(unknown))
    } else {
        format!("{} gap", secs(-tr.overlap))
    };
    let mut spans = vec![Span::styled(how, dim())];
    if let Some(h) = &tr.harmony {
        let keys = match (
            plays.get(tr.from).and_then(|p| p.played_key.clone()),
            plays.get(tr.to).and_then(|p| p.played_key.clone()),
        ) {
            (Some(a), Some(b)) if keys && a != b => format!(" ({a}→{b})"),
            _ => String::new(),
        };
        spans.push(Span::styled(" · ", faint()));
        spans.push(Span::styled(format!("{h}{keys}"), fg(harmony_color(h))));
    }
    match tr.tempo_change {
        Some(c) if c.abs() >= 0.05 => spans.push(Span::styled(format!(" · {c:+.1}% tempo"), dim())),
        Some(_) => spans.push(Span::styled(" · same tempo", dim())),
        None => {}
    }
    spans
}

fn draw_report(f: &mut Frame, t: &mut MixTab, area: Rect, cx: &Context) {
    let Some(Ok(o)) = &mut t.opened else { return };
    let r = &o.report;
    let selected = t.rows.selected().filter(|&i| i < r.plays.len());
    let playing = cx.playhead.and_then(|pos| play_at(&r.plays, pos));

    // Heights: the header, then the timeline (waveform, lanes, axis, tempo), then the lists.
    let h = area.height;
    let wave_h = if h >= 44 {
        6
    } else if h >= 32 {
        4
    } else if h >= 22 {
        3
    } else {
        2
    };
    let tempo_h = if h >= 30 { 2 } else { 0 };
    let timeline_h = wave_h + 2 + 1 + tempo_h + 2;
    let [head, timeline, lists] =
        Layout::vertical([Constraint::Length(3), Constraint::Length(timeline_h), Constraint::Min(0)]).areas(area);

    // Header: cover, title, facts, keys as played.
    let mut text_area = head;
    if let Some(art) = o.cover.as_mut().filter(|_| head.width >= 80) {
        let [cover, rest] = Layout::horizontal([Constraint::Length(7), Constraint::Min(0)]).areas(head);
        let pixels = art.pixels(6, 3);
        f.render_widget(
            Pixels { width: 6, pixels, palette: theme::palette() },
            Rect { x: cover.x + 1, width: 6, ..cover },
        );
        text_area = rest;
    }
    let m = &r.mix;
    let mut first = vec![Span::styled(format!(" {}", m.title), bold(theme::CYAN))];
    if let Some(a) = &m.artist {
        first.push(Span::styled(format!("  by {a}"), text()));
    }
    if let Some(pos) = cx.playhead {
        first.push(Span::styled(format!("   ▶ {} / {}", clock(pos), clock(m.duration_secs)), fg(theme::PINK)));
        if let Some(i) = playing {
            first.push(Span::styled(format!("  {} · {}", i + 1, r.plays[i].track.title), dim()));
        }
    }
    let mut facts = vec![clock(m.duration_secs), format!("{} tracks from {}", r.plays.len(), r.drive)];
    facts.push(format!("{:.0}% identified", r.coverage * 100.0));
    if let Some(rec) = &m.recorder {
        facts.push(format!("recorded on {rec}"));
    }
    if let Some(d) = &m.date {
        facts.push(d.clone());
    }
    if o.audio.is_none() {
        facts.push("no audio to play".into());
    }
    let mut keys = vec![Span::styled(" keys ", faint())];
    for (i, p) in r.plays.iter().enumerate() {
        if i > 0 {
            let arrow = match r.transitions.iter().find(|tr| tr.to == i).and_then(|tr| tr.harmony.as_deref()) {
                Some("clash") => Span::styled(" ✕ ", bold(theme::RED)),
                Some(_) => Span::styled(" → ", fg(theme::GREEN)),
                None => Span::styled(" → ", faint()),
            };
            keys.push(arrow);
        }
        let k = p.played_key.clone().unwrap_or_else(|| "?".into());
        let style = if Some(i) == selected { bold(theme::FG) } else { fg(theme::VIOLET) };
        keys.push(Span::styled(k, style));
    }
    let header = vec![Line::from(first), Line::styled(format!(" {}", facts.join(" · ")), dim()), Line::from(keys)];
    f.render_widget(Paragraph::new(header), text_area);

    // Timeline.
    let focus = cx.playhead.or_else(|| selected.map(|i| (r.plays[i].start + r.plays[i].end) / 2.0)).unwrap_or(0.0);
    let win = window(m.duration_secs, t.zoom, focus);
    let bpms: Vec<f64> = r.plays.iter().filter_map(|p| p.played_bpm).collect();
    let (lo, hi) = (bpms.iter().copied().fold(f64::INFINITY, f64::min), bpms.iter().copied().fold(0.0, f64::max));
    let mut right = Vec::new();
    if !bpms.is_empty() {
        let range = if hi - lo < 0.5 { format!("{lo:.0} BPM") } else { format!("{lo:.0}–{hi:.0} BPM") };
        right.push(Span::styled(format!(" {range} "), fg(theme::AMBER)));
    }
    if t.zoom > 1 {
        right.push(Span::styled(format!("{}× ", t.zoom), fg(theme::CYAN)));
    }
    right.push(Span::styled("z ", theme::key()));
    right.push(Span::styled("zoom ", faint()));
    let b = block(title("Timeline", false), false).title(Line::from(right).right_aligned());
    let inner = b.inner(timeline);
    f.render_widget(b, timeline);
    let [wave, lanes, axis, tempo] = Layout::vertical([
        Constraint::Length(wave_h),
        Constraint::Length(2),
        Constraint::Length(1),
        Constraint::Length(tempo_h),
    ])
    .areas(inner);
    t.areas.timeline = Rect { height: wave_h + 2, ..inner };
    t.areas.window = win;
    let buf = f.buffer_mut();
    draw_wave(buf, wave, &r.waveform, win, cx.playhead);
    draw_lanes(buf, lanes, r, win, selected, cx.playhead);
    draw_axis(buf, axis, win, selected.map(|i| (r.plays[i].start, r.plays[i].end)));
    if tempo_h > 0 {
        draw_tempo(buf, tempo, &r.plays, win, (lo, hi), cx.playhead);
    }

    // The tracklist, and the selected track.
    if lists.height < 4 {
        return;
    }
    let side_by_side = lists.width >= 130;
    let detail_h = if lists.height >= 26 {
        14
    } else if lists.height >= 20 {
        11
    } else if lists.height >= 14 {
        7
    } else {
        0
    };
    let (list_area, detail_area) = if side_by_side {
        let w = (lists.width * 2 / 5).clamp(56, 72);
        let [a, b] = Layout::horizontal([Constraint::Min(60), Constraint::Length(w)]).areas(lists);
        (a, b)
    } else {
        let [a, b] = Layout::vertical([Constraint::Min(4), Constraint::Length(detail_h)]).areas(lists);
        (a, b)
    };
    t.areas.rows = list_area;
    let focused = t.pane == Pane::Tracks;
    draw_tracklist(f, r, &mut t.rows, list_area, focused, playing);
    if let Some(i) = selected.filter(|_| detail_area.height >= 4) {
        draw_detail(f, r, &o.analyses, i, detail_area, cx.playhead);
    }
}

/// The colour drawn for `c` at time `t`: dimmed once played, white at the playhead.
fn shade(c: [u8; 3], played: bool, head: bool) -> [u8; 3] {
    if head {
        PLAYHEAD
    } else if played {
        theme::scale(c, 0.5)
    } else {
        c
    }
}

fn put(buf: &mut Buffer, x: u16, y: u16, symbol: &str, fg: [u8; 3], bg: Option<[u8; 3]>) {
    if let Some(cell) = buf.cell_mut((x, y)) {
        cell.set_symbol(symbol).set_fg(rgb3(fg));
        if let Some(bg) = bg {
            cell.set_bg(rgb3(bg));
            // Without colours, a lane or a chip shows in reverse video.
            if theme::plain() {
                cell.modifier.insert(Modifier::REVERSED);
            }
        }
    }
}

/// The column the playhead is in, if it's in view.
fn head_x(area: Rect, (from, to): (f64, f64), head: Option<f64>) -> Option<u16> {
    let h = head?;
    (h >= from && h < to).then(|| (((h - from) / (to - from)) * area.width as f64) as u16).filter(|x| *x < area.width)
}

/// The set's waveform in rekordbox's three bands, lows at the back and highs in front, each
/// column the loudest moment it covers.
fn draw_wave(buf: &mut Buffer, area: Rect, w: &MixWaveform, (from, to): (f64, f64), head: Option<f64>) {
    let n = w.low.len().min(w.mid.len()).min(w.high.len());
    if area.width == 0 || area.height == 0 || n == 0 || w.columns_per_sec <= 0.0 {
        return;
    }
    let head_col = head_x(area, (from, to), head);
    let per_col = (to - from) / area.width as f64;
    let rows = area.height as f32;
    for x in 0..area.width {
        let t0 = from + x as f64 * per_col;
        let a = ((t0 * w.columns_per_sec) as usize).min(n - 1);
        let b = (((t0 + per_col) * w.columns_per_sec).ceil() as usize).clamp(a + 1, n);
        let max = |v: &[u8], band: usize| {
            v[a..b].iter().copied().max().unwrap_or(0) as f32 / 255.0 * rows * bands::SCALES[band]
        };
        let bands = [(max(&w.low, 0), LOW), (max(&w.mid, 1), MID), (max(&w.high, 2), HIGH)];
        let played = head.is_some_and(|h| t0 + per_col <= h);
        let is_head = head_col == Some(x);
        for row in 0..area.height {
            let y = area.bottom() - 1 - row;
            match wave_cell(&bands, row as f32) {
                Some((sym, f, b)) => {
                    put(buf, area.x + x, y, sym, shade(f, played, is_head), b.map(|b| shade(b, played, is_head)))
                }
                None if is_head => put(buf, area.x + x, y, "│", theme::FG, None),
                None => {}
            }
        }
    }
}

/// A cell of a waveform: its symbol, the symbol's colour, and the colour behind it.
type WaveCell = (&'static str, [u8; 3], Option<[u8; 3]>);

/// One cell of layered bars (back to front, heights in cells) at `row` from the bottom.
fn wave_cell(bands: &[(f32, [u8; 3])], row: f32) -> Option<WaveCell> {
    let full = bands.iter().rposition(|(h, _)| *h >= row + 1.0);
    // The tallest band ending inside this cell, of those in front of the one filling it.
    let part = bands
        .iter()
        .enumerate()
        .skip(full.map_or(0, |i| i + 1))
        .filter(|(_, (h, _))| *h > row && *h < row + 1.0)
        .max_by(|a, b| a.1.0.total_cmp(&b.1.0));
    match (full, part) {
        (full, Some((_, (h, c)))) => {
            let eighths = (((h - row) * 8.0).round() as usize).clamp(1, 8);
            Some((LOWER[eighths], *c, full.map(|i| bands[i].1)))
        }
        (Some(i), None) => Some(("█", bands[i].1, None)),
        (None, None) => None,
    }
}

/// Each play as a bar in one of two lanes (so blends show as overlaps), shaded by how clearly it's
/// heard, with its number and title; stretches nothing matched are hatched.
fn draw_lanes(
    buf: &mut Buffer,
    area: Rect,
    r: &MixReport,
    (from, to): (f64, f64),
    selected: Option<usize>,
    head: Option<f64>,
) {
    if area.width == 0 || area.height < 2 {
        return;
    }
    let w = area.width as f64;
    let x_of = |t: f64| (((t - from) / (to - from)) * w).round().clamp(0.0, w) as u16;
    for g in &r.unidentified {
        let (a, b) = (x_of(g.start), x_of(g.end));
        for x in a..b.min(area.width) {
            for y in [area.y, area.y + 1] {
                put(buf, area.x + x, y, "░", theme::FAINT, None);
            }
        }
    }
    for (i, p) in r.plays.iter().enumerate() {
        let (a, b) = (x_of(p.start), x_of(p.end).max(x_of(p.start) + 1).min(area.width));
        if a >= area.width || p.end <= from || p.start >= to {
            continue;
        }
        let y = area.y + (i % 2) as u16;
        let base = LANES[i % LANES.len()];
        let chosen = selected == Some(i);
        for x in a..b {
            let t = from + (x as f64 + 0.5) / w * (to - from);
            let k = ((t - p.start) / p.presence_step.max(0.1)) as usize;
            let heard = p.presence.get(k).or(p.presence.last()).copied().unwrap_or(1.0).clamp(0.0, 1.0);
            let level = (0.3 + 0.7 * heard) * if chosen { 1.0 } else { 0.7 };
            let played = head.is_some_and(|h| t < h);
            let c = theme::scale(base, if played { level * 0.6 } else { level });
            put(buf, area.x + x, y, " ", INK, Some(c));
        }
        let label = format!("{} {}", i + 1, p.track.title);
        let room = (b - a) as usize;
        if room >= 2 {
            let style =
                Style::new().fg(rgb3(INK)).add_modifier(if chosen { Modifier::BOLD } else { Modifier::empty() });
            buf.set_stringn(area.x + a, y, clip(&label, room), room, style);
        }
    }
    if let Some(x) = head_x(area, (from, to), head) {
        for y in [area.y, area.y + 1] {
            if let Some(cell) = buf.cell_mut((area.x + x, y)) {
                cell.set_symbol("▏").set_fg(rgb3(PLAYHEAD));
            }
        }
    }
}

/// Times along the timeline, at a step that leaves room for the labels; the selected track's
/// stretch underlined.
fn draw_axis(buf: &mut Buffer, area: Rect, (from, to): (f64, f64), selected: Option<(f64, f64)>) {
    // No row (a short screen squeezes the timeline): its labels would go below the screen.
    if area.width < 8 || area.height == 0 {
        return;
    }
    let w = area.width as f64;
    let x_of = |t: f64| (((t - from) / (to - from)) * w).round();
    if let Some((s, e)) = selected {
        let (a, b) = (x_of(s).clamp(0.0, w) as u16, x_of(e).clamp(0.0, w) as u16);
        for x in a..b.min(area.width) {
            put(buf, area.x + x, area.y, "━", theme::PINK, None);
        }
    }
    let step = [15.0, 30.0, 60.0, 120.0, 300.0, 600.0, 900.0, 1200.0, 1800.0, 3600.0]
        .into_iter()
        .find(|s| s / (to - from) * w >= 10.0)
        .unwrap_or(3600.0);
    let mut t = (from / step).ceil() * step;
    while t < to {
        let x = x_of(t) as u16;
        let label = clock(t);
        if x as usize + label.len() < area.width as usize {
            buf.set_string(area.x + x, area.y, format!("╵{label}"), faint());
        }
        t += step;
    }
}

/// The tempo the set was played at, track by track, between the slowest and fastest.
fn draw_tempo(
    buf: &mut Buffer,
    area: Rect,
    plays: &[Play],
    (from, to): (f64, f64),
    (lo, hi): (f64, f64),
    head: Option<f64>,
) {
    if area.width == 0 || area.height == 0 || !lo.is_finite() {
        return;
    }
    let (lo, hi) = (lo - 1.0, hi + 0.5);
    let rows = area.height as f64;
    for x in 0..area.width {
        let t = from + (x as f64 + 0.5) / area.width as f64 * (to - from);
        let Some(bpm) = play_at(plays, t).and_then(|i| plays[i].played_bpm) else { continue };
        let height = ((bpm - lo) / (hi - lo)).clamp(0.05, 1.0) * rows;
        let played = head.is_some_and(|h| t < h);
        for row in 0..area.height {
            let fill = (((height - row as f64) * 8.0).round()).clamp(0.0, 8.0) as usize;
            if fill > 0 {
                put(buf, area.x + x, area.bottom() - 1 - row, LOWER[fill], shade(theme::AMBER, played, false), None);
            }
        }
    }
}

fn draw_tracklist(
    f: &mut Frame,
    r: &MixReport,
    rows: &mut TableState,
    area: Rect,
    focused: bool,
    playing: Option<usize>,
) {
    let narrow = area.width < 96;
    let header = ["#", "in", "track", "key", "BPM", "speed", "", "match"];
    let mut widths = vec![
        Constraint::Length(3),
        Constraint::Length(8),
        Constraint::Fill(1),
        Constraint::Length(8),
        Constraint::Length(6),
        Constraint::Length(7),
    ];
    if !narrow {
        widths.extend([Constraint::Length(8), Constraint::Length(5)]);
    }
    let cols = widths.len();
    let body: Vec<Row> = r
        .plays
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let number = if playing == Some(i) {
                Span::styled("▶", fg(theme::PINK))
            } else {
                Span::styled(format!("{}", i + 1), fg(LANES[i % LANES.len()]))
            };
            let mut name = vec![Span::styled(p.track.title.clone(), bold(theme::FG))];
            if !p.track.artist.is_empty() {
                name.push(Span::styled(format!("  {}", p.track.artist), dim()));
            }
            // The line under the track: how it hands over to the next, or a stretch nothing matched.
            let tr = r.transitions.iter().find(|tr| tr.from == i);
            let mut next: Vec<Span<'static>> = match tr {
                Some(tr) => {
                    let mut s = vec![Span::styled("↓ ", faint())];
                    s.extend(handoff(tr, r, false));
                    s
                }
                None => Vec::new(),
            };
            let next_start = r.plays.get(i + 1).map_or(f64::INFINITY, |n| n.start);
            let told = tr.is_some_and(|tr| unknown_in(r, tr).is_some());
            let unknown = r.unidentified.iter().find(|g| g.start >= p.start && g.start < next_start).filter(|_| !told);
            if let Some(g) = unknown {
                if !next.is_empty() {
                    next.push(Span::styled("  ", faint()));
                }
                next.push(Span::styled(format!("? {} not in the library", clock(g.end - g.start)), faint()));
            }
            let key = match (&p.key, &p.played_key) {
                (Some(k), Some(played)) if k != played => format!("{k}→{played}"),
                (_, Some(played)) => played.clone(),
                _ => p.track.key.clone(),
            };
            let speed = format!("{:+.1}%", (p.tempo - 1.0) * 100.0);
            let mut cells = vec![
                Cell::from(Line::from(number)),
                Cell::from(Line::styled(clock(p.start), dim())),
                Cell::from(Text::from(vec![Line::from(name), Line::from(next)])),
                Cell::from(Line::styled(key, fg(theme::VIOLET))),
                Cell::from(Line::styled(p.played_bpm.map(|b| format!("{b:.1}")).unwrap_or_default(), text())),
                Cell::from(Line::styled(speed, dim())),
            ];
            if !narrow {
                let lock = match p.key_lock {
                    Some(true) => "key lock",
                    Some(false) => "pitched",
                    None => "",
                };
                let conf = format!("{:.0}%", p.confidence * 100.0);
                let c = if p.is_tentative() { theme::AMBER } else { theme::GREEN };
                cells.push(Cell::from(Line::styled(lock, faint())));
                cells.push(Cell::from(Line::styled(conf, fg(c))));
            }
            Row::new(cells).height(2)
        })
        .collect();
    let head = Row::new(header.iter().take(cols).map(|h| Cell::from(Line::styled(*h, faint()))));
    let table = Table::new(body, widths)
        .header(head)
        .block(block(title("Tracklist", focused), focused))
        .row_highlight_style(if focused { theme::selected() } else { Style::new().add_modifier(Modifier::BOLD) })
        .highlight_symbol(Text::from(vec![Line::styled("▍", fg(theme::PINK)), Line::styled("▍", fg(theme::PINK))]));
    // Two lines a track, under the border and the header row.
    rows.fit(area.height.saturating_sub(3) as usize / 2, r.plays.len());
    f.render_stateful_widget(table, area, rows);
}

/// Phrase spans with times (ms), from the beat grid; one that runs past it ends with the track,
/// and one that doesn't end after it starts isn't drawn.
fn phrase_spans(a: &TrackAnalysis, track_ms: f64) -> Vec<(f64, f64, String, [u8; 3])> {
    let Some(st) = &a.structure else { return Vec::new() };
    let at = |beat: u16| a.beats.get(beat as usize - 1).map(|b| b.time_ms as f64);
    st.phrases
        .iter()
        .filter(|p| p.start_beat > 0 && p.end_beat > p.start_beat)
        .filter_map(|p| {
            Some((at(p.start_beat)?, at(p.end_beat).unwrap_or(track_ms), p.kind.name(), theme::phrase_color(p.kind)))
        })
        .collect()
}

/// The selected track: its overview waveform with the stretch played lit and the rest dimmed,
/// its cues and phrases, and what was done with it.
fn draw_detail(f: &mut Frame, r: &MixReport, analyses: &[TrackAnalysis], i: usize, area: Rect, head: Option<f64>) {
    let p = &r.plays[i];
    let a = analyses.get(i);
    let range = format!(" {}–{} in the set ", clock(p.start), clock(p.end));
    let b = block(title(&format!("{} · {}", i + 1, p.track.title), false), false)
        .title(Line::styled(range, faint()).right_aligned());
    let inner = b.inner(area);
    f.render_widget(b, area);
    let inner = Rect { x: inner.x + 1, width: inner.width.saturating_sub(2), ..inner };
    let track_secs = (p.track.duration_secs as f64).max(p.track_end).max(1.0);
    // Where the playhead is in the track, while this play is heard.
    let in_track = head.filter(|h| *h >= p.start && *h < p.end).map(|h| p.track_start + (h - p.start) * p.tempo);

    let graphic = inner.height >= 9 && inner.width >= 30;
    let text_area = if graphic {
        let [marks, wave, phrases, edges, rest] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Length(3),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(0),
        ])
        .areas(inner);
        let buf = f.buffer_mut();
        let w = inner.width as f64;
        let x_of = |secs: f64| ((secs / track_secs) * w).clamp(0.0, w - 1.0) as u16;
        let played = |x: u16| {
            let t = (x as f64 + 0.5) / w * track_secs;
            t >= p.track_start && t <= p.track_end
        };
        let head_col = in_track.map(x_of);
        if let Some(a) = a {
            draw_track_wave(buf, wave, a.preview.as_ref(), &played, head_col);
            let mut cues: Vec<_> = a.cues.iter().collect();
            cues.sort_by_key(|c| c.hot_cue.is_some());
            for c in cues {
                let x = x_of(c.start_ms as f64 / 1000.0);
                let color = theme::palette().cue_color(c);
                let color = if played(x) { color } else { theme::scale(color, 0.5) };
                match cue_letter(c) {
                    Some(l) => put(buf, marks.x + x, marks.y, &l.to_string(), INK, Some(color)),
                    None => put(buf, marks.x + x, marks.y, "▼", color, None),
                }
            }
            for (s, e, name, color) in phrase_spans(a, track_secs * 1000.0) {
                let (xa, xb) = (x_of(s / 1000.0), x_of(e / 1000.0).max(x_of(s / 1000.0) + 1));
                for x in xa..xb.min(phrases.width) {
                    let c = if played(x) { color } else { theme::scale(color, 0.35) };
                    let c = if x == xa { theme::scale(c, 0.7) } else { c };
                    put(buf, phrases.x + x, phrases.y, " ", INK, Some(c));
                }
                let room = xb.saturating_sub(xa + 1) as usize;
                if room >= 2 {
                    buf.set_stringn(phrases.x + xa + 1, phrases.y, &name, room, Style::new().fg(rgb3(INK)));
                }
            }
        }
        let (xi, xo) = (x_of(p.track_start), x_of(p.track_end));
        let out = format!("out {}┘", clock(p.track_end));
        buf.set_string(edges.x + xi, edges.y, format!("└in {}", clock(p.track_start)), fg(theme::AMBER));
        let out_x = (xo + 1).saturating_sub(out.chars().count() as u16);
        if out_x > xi + 10 {
            buf.set_string(edges.x + out_x, edges.y, out, fg(theme::AMBER));
        }
        rest
    } else {
        inner
    };

    let t = &p.track;
    let mut lines = Vec::new();
    let mut who = vec![Span::styled(t.title.clone(), bold(theme::FG))];
    if !t.artist.is_empty() {
        who.push(Span::styled(format!("  {}", t.artist), text()));
    }
    let extra: Vec<&str> = [t.label.as_str(), t.genre.as_str()].into_iter().filter(|s| !s.is_empty()).collect();
    if !extra.is_empty() {
        who.push(Span::styled(format!("  · {}", extra.join(" · ")), faint()));
    }
    lines.push(Line::from(who));
    let mut how = vec![Span::styled(
        format!("Played {}–{} of {}", clock(p.track_start), clock(p.track_end), clock(track_secs)),
        dim(),
    )];
    if let Some(b) = p.played_bpm {
        how.push(Span::styled(format!(" · {b:.1} BPM"), text()));
        if t.bpm > 0.0 {
            how.push(Span::styled(format!(" ({:+.1}% from {:.1})", (p.tempo - 1.0) * 100.0, t.bpm), dim()));
        }
    }
    match p.key_lock {
        Some(true) => how.push(Span::styled(" · key lock", dim())),
        Some(false) if p.semitones.abs() >= 0.05 => {
            how.push(Span::styled(format!(" · pitched {:+.1} semitones", p.semitones), dim()))
        }
        _ => {}
    }
    if let Some(k) = &p.played_key {
        how.push(Span::styled(" · heard in ", dim()));
        how.push(Span::styled(k.clone(), fg(theme::VIOLET)));
    }
    lines.push(Line::from(how));
    for (label, tr, other) in [
        ("From", r.transitions.iter().find(|tr| tr.to == i), i.checked_sub(1)),
        ("Into", r.transitions.iter().find(|tr| tr.from == i), Some(i + 1)),
    ] {
        let (Some(tr), Some(o)) = (tr, other) else { continue };
        let Some(op) = r.plays.get(o) else { continue };
        let mut s = vec![
            Span::styled(format!("{label} "), dim()),
            Span::styled(format!("{} ", o + 1), fg(LANES[o % LANES.len()])),
            Span::styled(clip(&op.track.title, 32), text()),
            Span::styled(": ", dim()),
        ];
        s.extend(handoff(tr, r, true));
        lines.push(Line::from(s));
    }
    let mut sure = vec![Span::styled(
        format!("Match {:.0}% from {} fingerprint hits", p.confidence * 100.0, p.votes),
        if p.is_tentative() { fg(theme::AMBER) } else { faint() },
    )];
    if p.is_tentative() {
        sure.push(Span::styled(" (tentative)", fg(theme::AMBER)));
    }
    if p.jumps > 0 {
        sure.push(Span::styled(
            format!(" · {} jump{}: looped or skipped", p.jumps, if p.jumps == 1 { "" } else { "s" }),
            faint(),
        ));
    }
    lines.push(Line::from(sure));
    let mut seen = rustc_hash::FxHashSet::default();
    for a in p.alternates.iter().filter(|a| seen.insert(a.name.clone())) {
        let note = if a.name == t.display_name() {
            format!("Also on the drive as {}", a.file_path)
        } else if a.same_audio {
            format!("Same audio as {}", a.name)
        } else {
            format!("Shares material with {}", a.name)
        };
        lines.push(Line::styled(note, faint()));
    }
    if Camelot::parse(&t.key).is_none() && p.played_key.is_none() {
        lines.push(Line::styled("No key in the library for this track.", faint()));
    }
    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: true }), text_area);
}

/// A track's overview waveform (rekordbox's two layers) with the stretch outside the set dimmed.
fn draw_track_wave(
    buf: &mut Buffer,
    area: Rect,
    wave: Option<&Waveform>,
    played: &dyn Fn(u16) -> bool,
    head: Option<u16>,
) {
    let Some(wave) = wave.filter(|w| !w.columns.is_empty()) else {
        let y = area.y + area.height / 2;
        for x in 0..area.width {
            let c = if played(x) { theme::PINK } else { theme::FAINT };
            put(buf, area.x + x, y, if played(x) { "━" } else { "─" }, c, None);
        }
        return;
    };
    let n = wave.columns.len() as f64;
    let w = area.width as f64;
    let rows = area.height as f32;
    for x in 0..area.width {
        let a = (x as f64 * n / w) as usize;
        let b = (((x + 1) as f64 * n / w).ceil() as usize).clamp(a + 1, wave.columns.len());
        let Some(col) = wave.columns[a.min(wave.columns.len() - 1)..b]
            .iter()
            .max_by(|p, q| p.back.height.total_cmp(&q.back.height))
        else {
            continue;
        };
        let lit = played(x);
        let tone = |c: [u8; 3]| {
            if head == Some(x) {
                PLAYHEAD
            } else if lit {
                theme::scale(c, 1.2)
            } else {
                theme::scale(c, 0.3)
            }
        };
        let bands = [(col.back.height * rows, tone(col.back.color)), (col.front.height * rows, tone(col.front.color))];
        for row in 0..area.height {
            if let Some((sym, f, bg)) = wave_cell(&bands, row as f32) {
                put(buf, area.x + x, area.bottom() - 1 - row, sym, f, bg);
            } else if head == Some(x) {
                put(buf, area.x + x, area.bottom() - 1 - row, "│", theme::FG, None);
            }
        }
    }
}

#[cfg(test)]
pub(super) mod tests {
    use locallibrary::mix::{Alternate, MixInfo, Span as Gap};

    use super::super::app::View;
    use super::super::app::testing::*;
    use super::super::audio::Audio;
    use super::super::ui::screen;
    use super::*;

    fn play(i: usize, start: f64, end: f64, key: &str, bpm: f64) -> Play {
        let mut t = track(i as u32 + 1, "Ferrule", ["Night Bus", "Signal Path", "Halogen"][i]);
        t.key = key.into();
        t.bpm = 124.0;
        t.duration_secs = 400;
        Play {
            track: t,
            start,
            end,
            track_start: 10.0,
            track_end: 10.0 + (end - start),
            tempo: bpm / 124.0,
            played_bpm: Some(bpm),
            key_lock: Some(i.is_multiple_of(2)),
            semitones: 0.0,
            key: Some(key.into()),
            played_key: Some(key.into()),
            confidence: if i == 2 { 0.4 } else { 0.9 },
            votes: 120,
            presence: (0..((end - start) / 2.0) as usize).map(|k| (k as f32 / 40.0).min(1.0)).collect(),
            presence_step: 2.0,
            jumps: 0,
            alternates: Vec::new(),
        }
    }

    fn report() -> MixReport {
        let mut plays = vec![
            play(0, 0.0, 330.0, "8A", 126.0),
            play(1, 300.0, 640.0, "9A", 126.0),
            play(2, 680.0, 1000.0, "3B", 128.0),
        ];
        plays[1].alternates.push(Alternate {
            track_id: 9,
            name: "Ferrule - Signal Path (Dub)".into(),
            file_path: "/Contents/9.mp3".into(),
            same_audio: false,
        });
        let wave = |seed: usize| (0..4000).map(|i| ((i * seed) % 255) as u8).collect::<Vec<u8>>();
        MixReport {
            mix: MixInfo {
                path: "/tmp/Sunday Warm-up.wav".into(),
                kind: "audio",
                title: "Sunday Warm-up".into(),
                artist: Some("Example DJ".into()),
                genre: None,
                date: Some("2026-10-04".into()),
                recorder: Some("XDJ-AZ".into()),
                duration_secs: 1000.0,
                sample_rate: 44100,
                channels: 2,
                labels: Vec::new(),
                cover: None,
            },
            drive: "DNA".into(),
            drive_id: None,
            library: Default::default(),
            transitions: vec![
                Transition {
                    from: 0,
                    to: 1,
                    at: 300.0,
                    overlap: 30.0,
                    harmony: Some("±1".into()),
                    tempo_change: Some(0.0),
                },
                Transition {
                    from: 1,
                    to: 2,
                    at: 680.0,
                    overlap: -40.0,
                    harmony: Some("clash".into()),
                    tempo_change: Some(1.6),
                },
            ],
            unidentified: vec![Gap { start: 640.0, end: 680.0 }],
            plays,
            coverage: 0.96,
            waveform: MixWaveform { columns_per_sec: 4.0, peak: wave(3), low: wave(7), mid: wave(5), high: wave(3) },
            tempo_range: 0.16,
            analysed_at: chrono::Utc::now(),
            elapsed_secs: 3.0,
        }
    }

    fn opened_set() -> Opened {
        let r = report();
        let analyses = r.plays.iter().map(|_| TrackAnalysis::default()).collect();
        Opened {
            report: r,
            root: "/Volumes/DNA".into(),
            library: Arc::new(Library::default()),
            analyses,
            cover: None,
            audio: Some("/tmp/Sunday Warm-up.wav".into()),
        }
    }

    /// A tab showing a made-up set of three tracks.
    pub(crate) fn tab() -> MixTab {
        let (tx, _rx) = std::sync::mpsc::channel();
        let mut t = MixTab::new(tx);
        t.opened = Some(Ok(opened_set()));
        t.rows.select(Some(1));
        t.pane = Pane::Tracks;
        t
    }

    /// [`tab`], its background work answering to `tx` (an app's), so what it reads gets back.
    pub(crate) fn tab_on(tx: Sender<Msg>) -> MixTab {
        MixTab { tx, ..tab() }
    }

    fn render(t: &mut MixTab, cx: &Context, w: u16, h: u16) -> String {
        let mut term = ratatui::Terminal::new(ratatui::backend::TestBackend::new(w, h)).unwrap();
        term.draw(|f| draw_tab(f, t, f.area(), cx)).unwrap();
        let buf = term.backend().buffer().clone();
        (0..h)
            .map(|y| (0..w).map(|x| buf[(x, y)].symbol().to_string()).collect::<String>().trim_end().to_string())
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn quiet() -> Context {
        Context { playhead: None, drive: Some("DNA".into()), status: None }
    }

    /// The report and export are written only with the drive the set was matched against, into
    /// a store (made then, if there's none).
    #[test]
    fn writing_needs_the_drive_it_was_matched_against() {
        let dir = tempfile::tempdir().unwrap();
        let (drive, store) = (dir.path().join("drive"), dir.path().join("store"));
        std::fs::create_dir_all(&drive).unwrap();
        let mut r = report();
        r.drive = "drive".into();
        assert!(for_writing(&store, &r, &dir.path().join("gone")).is_err(), "unplugged");
        assert!(for_writing(&store, &r, &drive).is_ok());
        assert!(store.join("store.json").is_file());
        r.drive_id = Some(uuid::Uuid::new_v4());
        let err = for_writing(&store, &r, &drive).err().unwrap();
        assert!(err.contains("isn't the one"), "{err}");
    }

    /// The recordings and the tracklist scrolled down to their selection on a short screen come
    /// back up once they fit again: drawn short, then tall, the first row and the one selected
    /// show.
    #[hegel::test(test_cases = 50)]
    fn the_lists_come_back_up_once_they_fit(tc: hegel::TestCase) {
        use hegel::generators as gs;
        let n = tc.draw(gs::integers::<usize>().min_value(1).max_value(10));
        let sel = tc.draw(gs::integers::<usize>().max_value(n - 1));
        let mut t = tab();
        let named = |what: &str| (0..n).map(|i| format!("{what} {i:02}")).collect::<Vec<_>>();
        let rows = match tc.draw(gs::booleans()) {
            true => {
                let names = named("Recording");
                t.drive_sources =
                    names.iter().map(|n| Source::recent(format!("/Volumes/DNA/{n}.wav").into())).collect();
                t.rebuild_sources();
                (t.pane, t.list) = (Pane::Mixes, ListState::default().with_selected(Some(sel)));
                names
            }
            false => {
                let names = named("Played");
                let Some(Ok(o)) = &mut t.opened else { unreachable!() };
                let r = &mut o.report;
                r.plays = (names.iter().enumerate())
                    .map(|(i, name)| {
                        let mut p = r.plays[i % 3].clone();
                        p.track.title = name.clone();
                        p
                    })
                    .collect();
                o.analyses = r.plays.iter().map(|_| TrackAnalysis::default()).collect();
                t.rows.select(Some(sel));
                names
            }
        };
        render(&mut t, &quiet(), 200, tc.draw(gs::integers::<u16>().min_value(1).max_value(30)));
        let s = render(&mut t, &quiet(), 200, 80);
        for row in [&rows[0], &rows[sel]] {
            assert!(s.contains(row.as_str()), "{row:?} isn't shown:\n{s}");
        }
    }

    #[test]
    fn draws_the_set_at_any_size() {
        let mut t = tab();
        for (w, h) in [(200, 60), (160, 48), (120, 40), (80, 24), (40, 12), (12, 5)] {
            let screen = render(&mut t, &quiet(), w, h);
            if w >= 80 {
                assert!(screen.contains("Sunday Warm-up"), "{w}x{h}\n{screen}");
                assert!(screen.contains("Signal Path"), "{w}x{h}\n{screen}");
                assert!(screen.contains("30s blend"), "{w}x{h}\n{screen}");
            }
            if w >= 120 {
                assert!(screen.contains("clash (9A→3B)"), "{screen}");
                assert!(screen.contains("↓ 40s not in the library · clash · +1.6% tempo"), "{screen}");
                assert!(screen.contains("124–128 BPM") || screen.contains("126–128 BPM"), "{screen}");
            }
            if w >= 160 {
                assert!(screen.contains("Shares material with"), "{screen}");
            }
        }
        // Zoomed in, following the playhead, part way into the third track.
        t.zoom = 4;
        let cx = Context { playhead: Some(700.0), ..quiet() };
        let screen = render(&mut t, &cx, 160, 48);
        assert!(screen.contains("▶ 11:40 / 16:40"), "{screen}");
        assert_eq!(t.rows.selected(), Some(2), "the selection follows the set");
        assert!(screen.contains("4×"));
    }

    #[test]
    fn empty_loading_and_failed_states_draw() {
        let (tx, _rx) = std::sync::mpsc::channel();
        let mut t = MixTab::new(tx);
        assert!(render(&mut t, &quiet(), 100, 20).contains("to open a file or a folder"));
        assert!(render(&mut t, &quiet(), 100, 20).contains("No recordings on DNA"));
        t.set_recent(&[]);
        t.input = Some("~/Music/set.wav".into());
        assert!(render(&mut t, &quiet(), 100, 20).contains("~/Music/set.wav"));
        t.input = None;
        t.opened = Some(Err("set.wav: no rekordbox drive".into()));
        assert!(render(&mut t, &quiet(), 100, 20).contains("enter tries again"));
    }

    #[test]
    fn plays_follow_the_playhead() {
        let r = report();
        assert_eq!(play_at(&r.plays, 10.0), Some(0));
        assert_eq!(play_at(&r.plays, 310.0), Some(1), "the incoming track during a blend");
        assert_eq!(play_at(&r.plays, 660.0), None, "a gap");
        assert_eq!(play_at(&r.plays, 999.0), Some(2));
        assert_eq!(window(1000.0, 1, 500.0), (0.0, 1000.0));
        assert_eq!(window(1000.0, 4, 10.0), (0.0, 250.0));
        assert_eq!(window(1000.0, 4, 990.0), (750.0, 1000.0));
        assert_eq!(window(1000.0, 2, 500.0), (250.0, 750.0));
    }

    #[test]
    fn layered_wave_cells() {
        let bands = [(2.5, LOW), (1.25, MID), (0.5, HIGH)];
        assert_eq!(wave_cell(&bands, 0.0), Some(("▄", HIGH, Some(MID))));
        assert_eq!(wave_cell(&bands, 1.0), Some(("▂", MID, Some(LOW))));
        assert_eq!(wave_cell(&bands, 2.0), Some(("▄", LOW, None)));
        assert_eq!(wave_cell(&bands, 3.0), None);
        assert_eq!(wave_cell(&[(1.0, LOW)], 0.0), Some(("█", LOW, None)));
    }

    #[test]
    fn typed_and_dropped_paths() {
        assert_eq!(typed_path("  '/Volumes/DNA/set one.wav' "), PathBuf::from("/Volumes/DNA/set one.wav"));
        assert_eq!(typed_path("\"/a b.wav\""), PathBuf::from("/a b.wav"));
        assert_eq!(typed_path("/Volumes/DNA/set\\ one.wav"), PathBuf::from("/Volumes/DNA/set one.wav"));
        if let Some(home) = std::env::var_os("HOME") {
            assert_eq!(typed_path("~/x.wav"), PathBuf::from(home).join("x.wav"));
        }
    }

    #[test]
    fn keys_move_zoom_and_type() {
        use super::super::app::View;

        let store = tempfile::tempdir().unwrap();
        let mut app = App::blank(store.path().to_path_buf(), None);
        app.mix = tab();
        app.mix.rows.select(Some(0));
        app.view = View::Mixes;

        key(&mut app, KeyCode::Down);
        assert_eq!(app.mix.rows.selected(), Some(1));
        key(&mut app, KeyCode::End);
        assert_eq!(app.mix.rows.selected(), Some(2));
        press(&mut app, "z");
        press(&mut app, "z");
        assert_eq!(app.mix.zoom, 4);
        key(&mut app, KeyCode::Tab);
        assert_eq!(app.mix.pane, Pane::Mixes);
        key(&mut app, KeyCode::Tab);
        assert_eq!(app.mix.pane, Pane::Tracks);
        key(&mut app, KeyCode::Esc);
        assert_eq!(app.mix.pane, Pane::Mixes);

        // Typing a path: every key goes to it, even view keys.
        press(&mut app, "O");
        press(&mut app, "1q");
        assert_eq!(app.mix.input.as_deref(), Some("1q"));
        assert!(!app.quit);
        key(&mut app, KeyCode::Esc);
        assert_eq!(app.mix.input, None);
        // A folder with no recordings in it says so.
        press(&mut app, "O");
        press(&mut app, &store.path().display().to_string());
        key(&mut app, KeyCode::Enter);
        // The folder is listed in the background.
        wait(&mut app, |a| a.toast.is_some());
        assert!(app.toast.as_ref().is_some_and(|(m, ..)| m.starts_with("no recordings in")), "{:?}", app.toast);
        assert!(app.mix.folders.is_empty(), "a folder with none isn't kept");

        // Keys the view doesn't use still work.
        press(&mut app, "1");
        assert_eq!(app.view, View::Library);
        press(&mut app, "5");
        assert_eq!(app.view, View::Mixes);
        press(&mut app, "q");
        assert!(app.quit);
    }

    #[test]
    fn recent_recordings_join_the_drive_ones() {
        let dir = tempfile::tempdir().unwrap();
        let (a, b) = (dir.path().join("a.wav"), dir.path().join("b.wav"));
        std::fs::write(&a, "x").unwrap();
        std::fs::write(&b, "x").unwrap();
        let (tx, _rx) = std::sync::mpsc::channel();
        let mut t = MixTab::new(tx);
        t.set_recent(&[a.clone(), dir.path().join("gone.wav")]);
        assert_eq!(t.sources.len(), 1, "missing files are dropped");
        t.drive = Some("/Volumes/DNA".into());
        let rec = Source {
            path: b.clone(),
            name: "b.wav".into(),
            place: "DNA".into(),
            duration_secs: Some(60.0),
            when: None,
            folder: None,
        };
        t.drive_listed("/Volumes/DNA".into(), vec![rec]);
        assert_eq!(t.sources.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(), ["b.wav", "a.wav"]);
        t.drive_listed("/Volumes/OTHER".into(), Vec::new());
        assert_eq!(t.sources.len(), 2, "a listing for another drive is ignored");
        t.remember(&b);
        assert_eq!(t.recent, [b, a]);
    }

    /// A tiny WAV file at `path` (its folders made).
    fn wav_at(path: &Path) -> PathBuf {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, super::super::audio::tests::wav(8000, 800, 1000)).unwrap();
        path.to_path_buf()
    }

    /// Identifying under way, at `phase` with `items` and `bytes` done of their totals.
    fn identifying(t: &mut MixTab, phase: Option<Phase>, items: (u64, u64), bytes: (u64, u64)) -> CancelToken {
        let cancel = CancelToken::new();
        t.loading = Some(Loading {
            path: "/sets/Sunday.wav".into(),
            progress: Arc::new(Mutex::new(Progress { phase, items, bytes })),
            cancel: cancel.clone(),
            started: Instant::now(),
        });
        cancel
    }

    #[test]
    fn the_header_says_how_far_identifying_has_got() {
        let (tx, _rx) = std::sync::mpsc::channel();
        let mut t = MixTab::new(tx);
        assert_eq!(t.status(), None);
        let says = |t: &mut MixTab, phase, items, bytes| {
            identifying(t, phase, items, bytes);
            t.status().unwrap()
        };
        assert_eq!(says(&mut t, None, (0, 0), (0, 0)), "Opening Sunday.wav…");
        assert_eq!(says(&mut t, Some(Phase::Fingerprint), (0, 0), (0, 0)), "Fingerprinting the drive's tracks…");
        assert_eq!(says(&mut t, Some(Phase::Fingerprint), (3, 40), (0, 0)), "Fingerprinting the drive's tracks… 3/40");
        assert_eq!(says(&mut t, Some(Phase::Index), (0, 0), (0, 0)), "Indexing fingerprints…");
        assert_eq!(says(&mut t, Some(Phase::Listen), (0, 0), (0, 0)), "Listening to Sunday.wav…");
        assert_eq!(says(&mut t, Some(Phase::Listen), (1, 4), (0, 0)), "Listening to Sunday.wav… 25%");
        assert_eq!(says(&mut t, Some(Phase::Listen), (1, 4), (900, 1000)), "Listening to Sunday.wav… 90%");
        assert_eq!(says(&mut t, Some(Phase::Match), (0, 0), (0, 0)), "Finding tracks in Sunday.wav…");

        // The view says it too, and how to stop.
        let cx = Context { status: t.status(), ..quiet() };
        let screen = render(&mut t, &cx, 120, 20);
        assert!(screen.contains("Finding tracks in Sunday.wav…") && screen.contains("esc stops"), "{screen}");
    }

    /// Esc stops identifying, and what the stopped run sends back afterwards is ignored.
    #[test]
    fn esc_stops_identifying() {
        let (mut app, _dir) = blank(View::Mixes);
        let cancel = identifying(&mut app.mix, Some(Phase::Listen), (0, 0), (0, 0));
        let generation = app.mix.generation;
        press(&mut app, "\u{1b}");
        assert!(cancel.is_cancelled() && !app.mix.is_loading());
        assert_eq!(last_message(&app), "stopped");
        loaded(&mut app, generation, Err("too late".into()));
        assert!(app.mix.opened.is_none());
        // With nothing to stop and the recordings shown, esc goes on to the app.
        assert!(!escape(&mut app));
    }

    #[test]
    fn opening_a_file_says_why_it_cant() {
        let (mut app, dir) = blank(View::Mixes);
        let open_typed = |app: &mut App, path: &Path| {
            press(app, &format!("O{}\n", path.display()));
            last_message(app).to_string()
        };
        let gone = open_typed(&mut app, &dir.path().join("gone.wav"));
        assert!(gone.ends_with("gone.wav doesn't exist"), "{gone}");
        let notes = dir.path().join("notes.txt");
        std::fs::write(&notes, "set list").unwrap();
        assert!(open_typed(&mut app, &notes).starts_with("notes.txt isn't a recording"));
        let set = wav_at(&dir.path().join("set.wav"));
        assert!(open_typed(&mut app, &set).starts_with("open a drive in the library first"));
        // Enter on nothing types a path; enter on nothing typed opens nothing.
        press(&mut app, "\n");
        assert_eq!(app.mix.input.as_deref(), Some(""));
        press(&mut app, "\n");
        assert_eq!(app.mix.input, None);
    }

    /// A tab clicked while a path is typed shows its view and puts the line away, so esc and the
    /// keys go to the view shown, and the line isn't there on coming back.
    #[test]
    fn a_tab_clicked_while_typing_a_path_puts_the_line_away() {
        let (mut app, _dir) = blank(View::Mixes);
        press(&mut app, "O~/Music");
        assert!(screen(&mut app, 120, 30).contains("Open a recording or folder:"));
        point(&mut app, LEFT, "1 Library");
        assert_eq!(app.view, View::Library);
        press(&mut app, "\u{1b}5");
        let s = screen(&mut app, 120, 30);
        assert!(!s.contains("Open a recording or folder:") && !s.contains("~/Music"), "{s}");
        press(&mut app, "q");
        assert!(app.quit, "q quits, not typed");
    }

    /// A folder's listing that comes back after another folder was opened is too late to count;
    /// del on one of its recordings forgets the folder, and says so.
    #[test]
    fn a_folder_opened_is_listed_until_forgotten() {
        let (mut app, dir) = blank(View::Mixes);
        let music = dir.path().join("music");
        wav_at(&music.join("Friday/warm-up.wav"));
        wav_at(&music.join("closing.wav"));
        press(&mut app, &format!("O{}\n", music.display()));
        wait(&mut app, |a| a.toast.is_some());
        assert!(last_message(&app).starts_with("2 recordings in "), "{}", last_message(&app));
        folders_listed(&mut app, Vec::new(), Vec::new());
        assert_eq!(app.mix.sources.len(), 2);

        press(&mut app, "\u{7f}");
        assert_eq!(last_message(&app), format!("forgot {}", music.display()));
        assert!(app.mix.sources.is_empty() && app.mix.folders.is_empty());
    }

    /// The open drive's own recordings are listed, read again with `R`.
    #[test]
    fn the_open_drives_recordings_are_listed() {
        let dir = tempfile::tempdir().unwrap();
        let drive = dir.path().join("DNA");
        std::fs::create_dir_all(&drive).unwrap();
        let mut app = App::with_library(&drive, library());
        app.view = View::Mixes;
        assert!(screen(&mut app, 120, 30).contains("No recordings on DNA."));
        wav_at(&drive.join("PIONEER REC/REC001.wav"));
        press(&mut app, "R");
        wait(&mut app, |a| !a.mix.sources.is_empty());
        let s = &app.mix.sources[0];
        assert_eq!((s.name.as_str(), s.place.as_str(), s.folder.as_ref()), ("REC001.wav", "DNA", None));
        let s = screen(&mut app, 120, 30);
        assert!(s.contains("REC001.wav") && s.contains("0:00 · DNA ·"), "{s}");
    }

    /// A listing of the drive that finishes after a newer one is dropped.
    #[test]
    fn an_older_listing_of_the_drive_is_dropped() {
        let dir = tempfile::tempdir().unwrap();
        let drive = dir.path().join("DNA");
        let rec = wav_at(&drive.join("PIONEER REC/REC001.wav"));
        let mut app = App::with_library(&drive, library());
        app.view = View::Mixes;
        press(&mut app, "R");
        let older = app.mix.drive_listing - 1;
        let root = app.mix.drive.clone().unwrap();
        app.handle_msg(Msg::MixSources(older, root, vec![Source::recent(rec)]));
        assert!(app.mix.sources.is_empty());
    }

    /// Enter on the set already shown goes to its tracks, without identifying it again.
    #[test]
    fn enter_on_the_set_shown_goes_to_its_tracks() {
        let (mut app, dir) = blank(View::Mixes);
        app.mix = tab();
        let set = wav_at(&dir.path().join("Sunday Warm-up.wav"));
        if let Some(Ok(o)) = &mut app.mix.opened {
            o.report.mix.path = set.clone();
        }
        app.mix.set_recent(&[set]);
        app.mix.pane = Pane::Mixes;
        press(&mut app, "\n");
        assert_eq!(app.mix.pane, Pane::Tracks);
        assert!(!app.mix.is_loading());
        // The zoom goes round: 2, 4 and 8 times, and back to the whole set.
        for zoom in [2, 4, 8, 1] {
            press(&mut app, "z");
            assert_eq!(app.mix.zoom, zoom);
        }
    }

    /// del forgets a recording opened on its own, but not one on the open drive.
    #[test]
    fn del_forgets_a_recording_from_this_computer_only() {
        let (mut app, dir) = blank(View::Mixes);
        let set = wav_at(&dir.path().join("set.wav"));
        app.mix.set_recent(std::slice::from_ref(&set));
        press(&mut app, "\u{7f}");
        assert_eq!(last_message(&app), "forgot set.wav");
        assert!(app.mix.recent.is_empty() && app.mix.sources.is_empty());

        let on_drive = Source::recent(set);
        app.mix.drive_sources = vec![Source { place: "DNA".into(), ..on_drive }];
        app.mix.rebuild_sources();
        app.toast = None;
        press(&mut app, "\u{7f}");
        assert_eq!(app.mix.sources.len(), 1, "the drive's own stay");
        assert!(app.toast.is_none());
    }

    /// A recording is identified against the open drive's library in the background; one whose
    /// tracks aren't in it opens with none found, and stays on the recordings.
    #[test]
    fn a_recording_is_identified_against_the_open_drive() {
        let dir = tempfile::tempdir().unwrap();
        let drive = dir.path().join("drive");
        std::fs::create_dir_all(&drive).unwrap();
        let mut app = App::with_library(&drive, library());
        app.view = View::Mixes;
        let set = wav_at(&dir.path().join("sets/set.wav"));
        app.mix.set_recent(std::slice::from_ref(&set));
        press(&mut app, "\n");
        assert!(app.mix.is_loading());
        assert!(screen(&mut app, 120, 30).contains("⟳ set.wav"));
        wait(&mut app, |a| !a.mix.is_loading());
        let Some(Ok(o)) = &app.mix.opened else { panic!("not opened: {}", last_message(&app)) };
        assert!(o.report.plays.is_empty());
        assert!(last_message(&app).contains("0 tracks found"), "{}", last_message(&app));
        assert_eq!(app.mix.pane, Pane::Mixes, "nothing to go through");
        let s = screen(&mut app, 120, 30);
        assert!(s.contains("● set.wav"), "{s}");
        // Enter on it again identifies it again (against the drive open now), rather than going to
        // a tracklist with nothing in it.
        press(&mut app, "\n");
        assert_eq!((app.mix.is_loading(), app.mix.pane), (true, Pane::Mixes));
        wait(&mut app, |a| !a.mix.is_loading());

        // One that isn't audio after all fails, and says so in place of the set.
        let bad = dir.path().join("sets/bad.wav");
        std::fs::write(&bad, "not audio").unwrap();
        open(&mut app, bad);
        wait(&mut app, |a| !a.mix.is_loading());
        assert!(matches!(&app.mix.opened, Some(Err(e)) if e.starts_with("bad.wav: ")), "{}", last_message(&app));
        let s = screen(&mut app, 120, 30);
        assert!(s.contains("enter tries again"), "{s}");
    }

    /// The made-up set open in an app that plays into a mixer (no device, returned with it),
    /// its audio a short WAV file.
    fn playing_app() -> (App, tempfile::TempDir, rodio::mixer::MixerSource) {
        let (mut app, dir) = blank(View::Mixes);
        let (audio, out) = Audio::offline(8000);
        app.audio = Some(audio);
        app.mix = tab();
        let wav = wav_at(&dir.path().join("Sunday Warm-up.wav"));
        if let Some(Ok(o)) = &mut app.mix.opened {
            o.audio = Some(wav);
        }
        (app, dir, out)
    }

    fn at(app: &App) -> f64 {
        app.position().as_secs_f64()
    }

    /// Space plays the set from the selected track, `n` and `p` go from track to track, and
    /// the tracklist follows.
    #[test]
    fn space_plays_the_set_and_n_and_p_go_through_its_tracks() {
        let (mut app, _dir, _out) = playing_app();
        assert!(!track_jump(&mut app, true), "nothing playing: n moves through the queue");
        press(&mut app, " ");
        assert_eq!(app.now.as_ref().map(|n| n.track.title.as_str()), Some("Sunday Warm-up"));
        opened(&mut app);
        assert_eq!(at(&app), 300.0, "from Signal Path");
        let s = screen(&mut app, 160, 48);
        assert!(s.contains("▶ 5:00 / 16:40") && s.contains("2 · Signal Path"), "{s}");

        press(&mut app, "n");
        assert_eq!(at(&app), 680.0);
        screen(&mut app, 160, 48);
        assert_eq!(app.mix.rows.selected(), Some(2), "the selection follows the set");
        press(&mut app, "n");
        assert_eq!(last_message(&app), "that's the last track in the set");
        // Back to the start of the one before (just into this one), then of this one.
        press(&mut app, "p");
        assert_eq!(at(&app), 300.0);
        app.seek_to(Duration::from_secs(320));
        press(&mut app, "p");
        assert_eq!(at(&app), 300.0);
        press(&mut app, "p");
        assert_eq!(at(&app), 0.0);

        // Enter on a track plays the set from it, paused or not.
        press(&mut app, " ");
        assert!(app.audio.as_ref().unwrap().is_paused(), "with the set playing, space pauses as anywhere");
        key(&mut app, KeyCode::Up);
        press(&mut app, "\n");
        assert_eq!(at(&app), 300.0);
        assert!(!app.audio.as_ref().unwrap().is_paused());
    }

    /// A project without its audio exported next to it has nothing to play.
    #[test]
    fn a_set_without_audio_says_so() {
        let (mut app, _dir) = blank(View::Mixes);
        app.mix = tab();
        if let Some(Ok(o)) = &mut app.mix.opened {
            o.audio = None;
        }
        assert!(screen(&mut app, 160, 48).contains("no audio to play"));
        press(&mut app, " ");
        assert!(
            last_message(&app).starts_with("nothing to play: export the project's audio"),
            "{}",
            last_message(&app)
        );
        assert!(app.now.is_none());
    }

    /// Clicking the timeline picks the track under the pointer, or with the set playing, plays
    /// from there; the tracklist and the recordings take clicks and the wheel too.
    #[test]
    fn the_mouse_picks_tracks_and_recordings() {
        let (mut app, dir, _out) = playing_app();
        let sets = [wav_at(&dir.path().join("a.wav")), wav_at(&dir.path().join("b.wav"))];
        app.mix.set_recent(&sets);
        let tracklist = |app: &App| app.mix.areas.rows;
        let recordings = |app: &App| app.mix.areas.list;
        screen(&mut app, 120, 30);
        let timeline = app.mix.areas.timeline;
        // The middle of the timeline is 500 s in: the second track.
        app.mix.rows.select(Some(0));
        mouse(&mut app, LEFT, (timeline.x + timeline.width / 2, timeline.y));
        assert_eq!((app.mix.pane, app.mix.rows.selected()), (Pane::Tracks, Some(1)));
        let halogen = spot_in(&mut app, "Halogen", tracklist);
        mouse(&mut app, LEFT, halogen);
        assert_eq!(app.mix.rows.selected(), Some(2));
        mouse(&mut app, MouseEventKind::ScrollUp, halogen);
        assert_eq!(app.mix.rows.selected(), Some(0));
        // The recordings: one click selects, a second opens.
        let b = spot_in(&mut app, "b.wav", recordings);
        mouse(&mut app, LEFT, b);
        assert_eq!(
            (app.mix.pane, app.mix.list.selected().map(|i| app.mix.sources[i].name.as_str())),
            (Pane::Mixes, Some("b.wav"))
        );
        mouse(&mut app, MouseEventKind::ScrollUp, b);
        assert_eq!(app.mix.list.selected(), Some(0));
        let a = spot_in(&mut app, "a.wav", recordings);
        mouse(&mut app, LEFT, a);
        mouse(&mut app, LEFT, a);
        assert!(last_message(&app).starts_with("open a drive in the library first"), "{}", last_message(&app));

        // Playing, a click on the timeline plays from there.
        press(&mut app, " ");
        opened(&mut app);
        screen(&mut app, 120, 30);
        let timeline = app.mix.areas.timeline;
        mouse(&mut app, LEFT, (timeline.x, timeline.y));
        assert!(at(&app) < 10.0, "{}", at(&app));
    }

    /// `E` saves the set as data for Local Network into the store's reports folder, from the
    /// drive it was matched against; `w` won't write the report without that drive.
    #[test]
    fn the_set_is_exported_from_its_drive() {
        let (mut app, dir) = blank(View::Mixes);
        let drive = dir.path().join("drive");
        std::fs::create_dir_all(&drive).unwrap();
        press(&mut app, "E");
        assert_eq!(last_message(&app), "open a set first");
        press(&mut app, "w");
        assert_eq!(last_message(&app), "open a set first");

        app.mix = tab();
        if let Some(Ok(o)) = &mut app.mix.opened {
            o.root = drive.clone();
            o.report.drive = "drive".into();
        }
        press(&mut app, "E");
        assert!(
            last_message(&app).starts_with("saving ") && last_message(&app).ends_with(".mix.json…"),
            "{}",
            last_message(&app)
        );
        wait(&mut app, |a| last_message(a).starts_with("wrote "));
        let written = last_message(&app).trim_start_matches("wrote ").split(" (").next().unwrap().to_string();
        let doc: serde_json::Value = serde_json::from_slice(&std::fs::read(&written).unwrap()).unwrap();
        assert!(doc.to_string().contains("Signal Path"), "{doc}");

        std::fs::remove_dir(&drive).unwrap();
        press(&mut app, "E");
        wait(&mut app, |a| a.toast.as_ref().is_some_and(|t| t.1 == Level::Error));
        assert!(last_message(&app).starts_with("the set's tracks are read from its drive"), "{}", last_message(&app));
        app.toast = None;
        press(&mut app, "w");
        assert!(
            last_message(&app).starts_with("writing ") && last_message(&app).ends_with(".html…"),
            "{}",
            last_message(&app)
        );
        wait(&mut app, |a| a.toast.as_ref().is_some_and(|t| t.1 == Level::Error));
        assert!(last_message(&app).starts_with("the set's tracks are read from its drive"), "{}", last_message(&app));
    }

    /// `o` with a set open goes to the selected track in the library, which is on the drive
    /// the set was matched against.
    #[test]
    fn o_finds_the_selected_track_on_its_drive() {
        let (mut app, _dir) = blank(View::Mixes);
        assert!(!go_to(&mut app), "no set: o goes to the playing track");
        app.mix = tab();
        press(&mut app, "o");
        assert_eq!(last_message(&app), "the track is on another drive");
        assert_eq!(app.view, View::Mixes);
    }

    /// A track's analysis: a preview waveform, a hot cue and a memory cue, and two phrases on
    /// a beat grid of one beat a second.
    fn analysis() -> TrackAnalysis {
        use locallibrary::anlz::{Mood, Phrase, PhraseKind, SongStructure};
        let phrase = |kind, start_beat, end_beat| Phrase { kind, start_beat, end_beat, fill_beat: None };
        TrackAnalysis {
            beats: grid(0, 60.0, 400),
            cues: vec![cue(Some(0), 20_000, None), cue(None, 300_000, None)],
            preview: Some(wave(400)),
            detail: None,
            structure: Some(SongStructure {
                mood: Mood::High,
                phrases: vec![phrase(PhraseKind::Intro, 1, 60), phrase(PhraseKind::Chorus, 60, 999)],
            }),
        }
    }

    /// A phrase that doesn't end after it starts (made-up analysis: the reader drops those) isn't
    /// drawn; one on the grid is, from its first beat to its last.
    #[test]
    fn only_phrases_that_end_after_they_start_are_drawn() {
        use locallibrary::anlz::{Mood, Phrase, PhraseKind, SongStructure};
        let phrase = |start_beat, end_beat| Phrase { kind: PhraseKind::Up, start_beat, end_beat, fill_beat: None };
        let phrases = vec![phrase(1, 0), phrase(3, 2), phrase(2, 2), phrase(0, 3), phrase(2, 4)];
        let a = TrackAnalysis {
            beats: grid(0, 60.0, 4),
            structure: Some(SongStructure { mood: Mood::High, phrases }),
            ..Default::default()
        };
        let spans: Vec<(f64, f64)> = phrase_spans(&a, 10_000.0).into_iter().map(|s| (s.0, s.1)).collect();
        assert_eq!(spans, [(1000.0, 3000.0)]);
    }
    /// A cut, a plain gap, a track pitched without key lock and one with no key, alternates on
    /// the drive, a cover: each is drawn as the set's report says it.
    #[test]
    fn every_kind_of_handoff_and_track_is_drawn() {
        let mut t = tab();
        let Some(Ok(o)) = &mut t.opened else { unreachable!() };
        let r = &mut o.report;
        r.unidentified.clear();
        r.transitions[0] = Transition { overlap: 0.5, harmony: None, tempo_change: None, ..r.transitions[0].clone() };
        r.transitions[1].overlap = -75.0;
        let p = &mut r.plays[1];
        (p.key_lock, p.semitones, p.jumps, p.confidence) = (Some(false), -1.0, 2, 0.4);
        (p.track.label, p.track.genre) = ("Hessle Audio".into(), "Techno".into());
        let alternate = |name: &str, same_audio| Alternate {
            track_id: 9,
            name: name.into(),
            file_path: "/Contents/9.mp3".into(),
            same_audio,
        };
        p.alternates = vec![alternate("Ferrule - Signal Path", false)];
        let p = &mut r.plays[2];
        p.alternates = vec![alternate("Ferrule - Halogen (Edit)", true)];
        (p.key, p.played_key, p.track.key) = (None, None, String::new());
        r.plays[1].played_key = None;
        o.analyses = vec![TrackAnalysis::default(), analysis(), TrackAnalysis::default()].into();
        let mut png = Vec::new();
        image::RgbImage::from_pixel(8, 8, image::Rgb([200, 40, 90]))
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        o.cover = Art::decode(&png);

        let screen = render(&mut t, &quiet(), 160, 60);
        for said in [
            "↓ cut",
            "1:15 gap",
            "pitched -1.0 semitones",
            "Hessle Audio · Techno",
            "2 jumps: looped or skipped",
            "(tentative)",
            "Also on the drive as /Contents/9.mp3",
            "└in 0:10",
            "Intro",
        ] {
            assert!(screen.contains(said), "{said}:\n{screen}");
        }
        // The keys row has a question mark for a key not heard, and a cross for a clash.
        assert!(screen.contains("keys 8A → ? ✕ ?"), "{screen}");
        // Over the track's waveform, its hot cue by its letter (at 20 s of 400) before its
        // memory cue (at 300 s).
        let lines: Vec<&str> = screen.lines().collect();
        let panel = lines.iter().position(|l| l.contains("╭ 2 · Signal Path")).expect("the track's panel");
        let cues = lines[panel + 1];
        assert!(cues.find('A').is_some_and(|a| cues.find('▼').is_some_and(|m| a < m)), "{cues}");
        assert!(lines[panel + 2..panel + 5].iter().any(|l| l.contains('█')), "{screen}");
        t.rows.select(Some(2));
        let screen = render(&mut t, &quiet(), 160, 60);
        for said in ["Same audio as Ferrule - Halogen (Edit)", "No key in the library for this track."] {
            assert!(screen.contains(said), "{said}:\n{screen}");
        }

        // Smaller, with less room for the detail (two of its sizes), it keeps what was done with
        // the track.
        t.rows.select(Some(1));
        for h in [34, 40] {
            let screen = render(&mut t, &quiet(), 100, h);
            assert!(screen.contains("Played 0:10–5:50 of 6:40"), "{h}\n{screen}");
        }
    }

    /// Without colours, the tracks' lanes under the timeline show in reverse video.
    #[test]
    fn lanes_show_in_reverse_video_without_colours() {
        let mut t = tab();
        let cx = Context { playhead: Some(500.0), ..quiet() };
        theme::set_test_mode(Some(theme::Mode::Plain));
        let mut term = ratatui::Terminal::new(ratatui::backend::TestBackend::new(160, 48)).unwrap();
        term.draw(|f| draw_tab(f, &mut t, f.area(), &cx)).unwrap();
        theme::set_test_mode(None);
        let buf = term.backend().buffer();
        let row = |y| (0..160).map(|x| buf[(x, y)].symbol()).collect::<String>();
        let (y, x) = (0..48)
            .find_map(|y| row(y).find("1 Night Bus").map(|i| (y, row(y)[..i].chars().count())))
            .expect("the lanes");
        assert!(buf[(x as u16, y)].modifier.contains(Modifier::REVERSED));
    }

    /// Recordings opened by path stay listed: a folder's (in its subfolders too) until it's
    /// forgotten, newest folder first; one opened on its own, once identified against the drive,
    /// among the recent ones until it's forgotten. A folder with none isn't kept, and a path
    /// that isn't a recording says why and changes nothing.
    #[hegel::test(test_cases = 12, suppress_health_check = [hegel::HealthCheck::TooSlow])]
    fn recordings_opened_by_path_stay_listed_until_forgotten(tc: hegel::TestCase) {
        use hegel::generators as gs;
        let dir = tempfile::tempdir().unwrap();
        let drive = dir.path().join("drive");
        std::fs::create_dir_all(&drive).unwrap();
        let mut app = App::with_library(&drive, library());
        app.view = View::Mixes;
        let sets = dir.path().join("sets");
        let in_folders = [
            vec![wav_at(&sets.join("a/one.wav")), wav_at(&sets.join("a/later/two.wav"))],
            vec![wav_at(&sets.join("b/three.wav"))],
            Vec::new(),
        ];
        let folders = [sets.join("a"), sets.join("b"), sets.join("empty")];
        std::fs::create_dir_all(&folders[2]).unwrap();
        std::fs::write(folders[2].join("notes.txt"), "set list").unwrap();
        let solo = wav_at(&dir.path().join("solo.wav"));
        let (notes, gone) = (folders[2].join("notes.txt"), dir.path().join("gone.wav"));
        let (mut kept, mut recent): (Vec<usize>, Vec<PathBuf>) = (Vec::new(), Vec::new());
        for _ in 0..tc.draw(gs::integers::<usize>().min_value(1).max_value(6)) {
            app.toast = None;
            match tc.draw(gs::integers::<u8>().max_value(4)) {
                0 | 1 => {
                    let f = tc.draw(gs::integers::<usize>().max_value(2));
                    press(&mut app, &format!("O{}\n", folders[f].display()));
                    wait(&mut app, |a| a.mix.announce.is_none());
                    let said = last_message(&app).to_string();
                    kept.retain(|&k| k != f);
                    match in_folders[f].len() {
                        0 => assert!(said.starts_with("no recordings in"), "{said}"),
                        n => {
                            kept.insert(0, f);
                            assert!(said.starts_with(&format!("{n} recording")), "{said}");
                        }
                    }
                }
                2 => {
                    press(&mut app, &format!("O{}\n", solo.display()));
                    wait(&mut app, |a| !a.mix.is_loading());
                    assert!(last_message(&app).contains("0 tracks found"), "{}", last_message(&app));
                    recent.retain(|p| *p != solo);
                    recent.insert(0, solo.clone());
                }
                3 => {
                    let (path, why) =
                        if tc.draw(gs::booleans()) { (&notes, "isn't a recording") } else { (&gone, "doesn't exist") };
                    press(&mut app, &format!("O{}\n", path.display()));
                    assert!(last_message(&app).contains(why), "{}", last_message(&app));
                }
                _ if !app.mix.sources.is_empty() => {
                    let i = tc.draw(gs::integers::<usize>().max_value(app.mix.sources.len() - 1));
                    app.mix.pane = Pane::Mixes;
                    app.mix.list.select(Some(i));
                    let s = app.mix.sources[i].clone();
                    press(&mut app, "\u{7f}");
                    match &s.folder {
                        Some(d) => kept.retain(|&k| folders[k] != *d),
                        None => recent.retain(|p| *p != s.path),
                    }
                }
                _ => continue,
            }
            let want_folders: Vec<&PathBuf> = kept.iter().map(|&k| &folders[k]).collect();
            assert_eq!(app.mix.folders.iter().collect::<Vec<_>>(), want_folders);
            assert_eq!(app.mix.recent, recent);
            let mut listed: Vec<&PathBuf> = app.mix.sources.iter().map(|s| &s.path).collect();
            let mut want: Vec<&PathBuf> = kept.iter().flat_map(|&k| &in_folders[k]).chain(&recent).collect();
            listed.sort();
            want.sort();
            want.dedup();
            assert_eq!(listed, want);
            assert!(screen(&mut app, 120, 30).lines().count() == 30);
        }
    }
}
