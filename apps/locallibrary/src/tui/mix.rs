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

use locallibrary::anlz::{TrackAnalysis, Waveform};
use locallibrary::api;
use locallibrary::mix::export;
use locallibrary::mix::index::Cache;
use locallibrary::mix::report::{self, Drive};
use locallibrary::mix::{self, Identifier, MixReport, MixWaveform, Play, Transition};
use locallibrary::profile::ProfilePrefs;
use locallibrary::progress::{CancelToken, Ctx, Event, Phase};
use locallibrary::tokens::bands;
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

/// Play `i`'s lane colour, by its number.
fn lane(i: usize) -> [u8; 3] {
    let t = theme::current();
    [t.accent, t.info, t.warning, t.secondary, t.good][i % 5]
}
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

/// What a path typed or dropped into the terminal means: quotes and (on Unix, where a shell
/// escapes with them) backslash escapes taken off, `~` for the home folder.
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
                if c == '\\' && cfg!(unix) {
                    out.extend(chars.next());
                } else {
                    out.push(c);
                }
            }
            out
        });
    // `~` is the home folder, which Windows names by its profile (no `HOME`), with either slash.
    let rest = s.strip_prefix("~/").or_else(|| s.strip_prefix("~\\").filter(|_| cfg!(windows)));
    match rest {
        Some(rest) => std::env::home_dir().map(|h| h.join(rest)).unwrap_or_else(|| s.clone().into()),
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
    let analyses = report.plays.iter().map(|p| api::analysis_of(&device, &p.track)).collect();
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
    let prefs = api::profile_prefs(&store).map_err(|e| e.to_string())?;
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
        let drive = Drive { device: &device, library: &library, analyses: Some(&analyses), fields: None };
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
        let shown = crate::plugins::shown_fields(Some(store.clone()));
        let drive = Drive { device: &device, library: &library, analyses: Some(&analyses), fields: Some(&shown) };
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
    let style = if focused { bold(theme::current().info) } else { bold(theme::current().dim) };
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
            Line::styled(cx.status.clone().unwrap_or_default(), bold(theme::current().text)),
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
                Line::styled("A recorded set, and the tracks it plays", bold(theme::current().text)),
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
            let lines = vec![
                Line::raw(""),
                Line::styled(e.clone(), fg(theme::current().critical)),
                Line::styled("enter tries again", dim()),
            ];
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
            Line::styled("Open a recording or folder:", bold(theme::current().warning)),
            Line::from(vec![Span::styled(shown.to_string(), text()), Span::styled("▏", fg(theme::current().accent))]),
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
                Span::styled("⟳ ", fg(theme::current().info))
            } else if opened.as_ref() == Some(&s.path) {
                Span::styled("● ", fg(theme::current().good))
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
        .highlight_symbol(Span::styled("▍", fg(theme::current().accent)));
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
    if h == "clash" { theme::current().critical } else { theme::current().good }
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
    let mut first = vec![Span::styled(format!(" {}", m.title), bold(theme::current().info))];
    if let Some(a) = &m.artist {
        first.push(Span::styled(format!("  by {a}"), text()));
    }
    if let Some(pos) = cx.playhead {
        first.push(Span::styled(
            format!("   ▶ {} / {}", clock(pos), clock(m.duration_secs)),
            fg(theme::current().accent),
        ));
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
                Some("clash") => Span::styled(" ✕ ", bold(theme::current().critical)),
                Some(_) => Span::styled(" → ", fg(theme::current().good)),
                None => Span::styled(" → ", faint()),
            };
            keys.push(arrow);
        }
        let k = p.played_key.clone().unwrap_or_else(|| "?".into());
        let style = if Some(i) == selected { bold(theme::current().text) } else { fg(theme::current().secondary) };
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
        right.push(Span::styled(format!(" {range} "), fg(theme::current().warning)));
    }
    if t.zoom > 1 {
        right.push(Span::styled(format!("{}× ", t.zoom), fg(theme::current().info)));
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
        theme::current().playhead
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
    let [low, mid, high] = theme::current().bands;
    for x in 0..area.width {
        let t0 = from + x as f64 * per_col;
        let a = ((t0 * w.columns_per_sec) as usize).min(n - 1);
        let b = (((t0 + per_col) * w.columns_per_sec).ceil() as usize).clamp(a + 1, n);
        let max = |v: &[u8], band: usize| {
            v[a..b].iter().copied().max().unwrap_or(0) as f32 / 255.0 * rows * bands::SCALES[band]
        };
        let bands = [(max(&w.low, 0), low), (max(&w.mid, 1), mid), (max(&w.high, 2), high)];
        let played = head.is_some_and(|h| t0 + per_col <= h);
        let is_head = head_col == Some(x);
        for row in 0..area.height {
            let y = area.bottom() - 1 - row;
            match wave_cell(&bands, row as f32) {
                Some((sym, f, b)) => {
                    put(buf, area.x + x, y, sym, shade(f, played, is_head), b.map(|b| shade(b, played, is_head)))
                }
                None if is_head => put(buf, area.x + x, y, "│", theme::current().text, None),
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
                put(buf, area.x + x, y, "░", theme::current().faint, None);
            }
        }
    }
    for (i, p) in r.plays.iter().enumerate() {
        let (a, b) = (x_of(p.start), x_of(p.end).max(x_of(p.start) + 1).min(area.width));
        if a >= area.width || p.end <= from || p.start >= to {
            continue;
        }
        let y = area.y + (i % 2) as u16;
        let base = lane(i);
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
                cell.set_symbol("▏").set_fg(rgb3(theme::current().playhead));
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
            put(buf, area.x + x, area.y, "━", theme::current().accent, None);
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
                put(
                    buf,
                    area.x + x,
                    area.bottom() - 1 - row,
                    LOWER[fill],
                    shade(theme::current().warning, played, false),
                    None,
                );
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
                Span::styled("▶", fg(theme::current().accent))
            } else {
                Span::styled(format!("{}", i + 1), fg(lane(i)))
            };
            let mut name = vec![Span::styled(p.track.title.clone(), bold(theme::current().text))];
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
                Cell::from(Line::styled(key, fg(theme::current().secondary))),
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
                let c = if p.is_tentative() { theme::current().warning } else { theme::current().good };
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
        .highlight_symbol(Text::from(vec![
            Line::styled("▍", fg(theme::current().accent)),
            Line::styled("▍", fg(theme::current().accent)),
        ]));
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
            Some((
                at(p.start_beat)?,
                at(p.end_beat).unwrap_or(track_ms),
                p.kind.name(),
                theme::current().phrase(p.kind),
            ))
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
        buf.set_string(edges.x + xi, edges.y, format!("└in {}", clock(p.track_start)), fg(theme::current().warning));
        let out_x = (xo + 1).saturating_sub(out.chars().count() as u16);
        if out_x > xi + 10 {
            buf.set_string(edges.x + out_x, edges.y, out, fg(theme::current().warning));
        }
        rest
    } else {
        inner
    };

    let t = &p.track;
    let mut lines = Vec::new();
    let mut who = vec![Span::styled(t.title.clone(), bold(theme::current().text))];
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
        how.push(Span::styled(k.clone(), fg(theme::current().secondary)));
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
            Span::styled(format!("{} ", o + 1), fg(lane(o))),
            Span::styled(clip(&op.track.title, 32), text()),
            Span::styled(": ", dim()),
        ];
        s.extend(handoff(tr, r, true));
        lines.push(Line::from(s));
    }
    let mut sure = vec![Span::styled(
        format!("Match {:.0}% from {} fingerprint hits", p.confidence * 100.0, p.votes),
        if p.is_tentative() { fg(theme::current().warning) } else { faint() },
    )];
    if p.is_tentative() {
        sure.push(Span::styled(" (tentative)", fg(theme::current().warning)));
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
            let c = if played(x) { theme::current().accent } else { theme::current().faint };
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
                theme::current().playhead
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
                put(buf, area.x + x, area.bottom() - 1 - row, "│", theme::current().text, None);
            }
        }
    }
}

#[cfg(test)]
pub(super) mod tests;
