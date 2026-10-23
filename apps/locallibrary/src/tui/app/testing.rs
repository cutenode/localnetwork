//! What the TUI's tests share: the test library and the demo drive, apps showing them, keys and
//! clicks given the way a person gives them, and waiting for the work the app does in the
//! background.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use locallibrary::anlz::{Beat, Cue, TrackAnalysis, WaveColumn, WaveLayer, Waveform, WaveformStyle};
use locallibrary::api::{DriveStatus, FleetDrive};
use locallibrary::model::TrackColor;
use locallibrary::sets::{Set, SetEntry};
use locallibrary::{Device, DeviceInfo, Library, Playlist, Track, paths};
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Rect;
use tempfile::TempDir;

use super::super::audio::Audio;
use super::super::audio::tests::wav;
use super::super::edits::{Job, Modal};
use super::super::ui::screen;
use super::{App, Msg, NowPlaying, View};

// Copying a folder with its files' modification times, as the library's and the CLI's tests do.
#[path = "../../../../../crates/locallibrary/tests/support/files.rs"]
mod files;
pub use files::{Template, copy_dir};

pub fn track(id: u32, artist: &str, title: &str) -> Track {
    Track {
        id,
        title: title.into(),
        artist: artist.into(),
        album: String::new(),
        album_artist: String::new(),
        genre: String::new(),
        label: String::new(),
        key: String::new(),
        remixer: String::new(),
        original_artist: String::new(),
        composer: String::new(),
        comment: String::new(),
        mix_name: String::new(),
        isrc: String::new(),
        bpm: 0.0,
        duration_secs: 300,
        year: 0,
        track_number: 0,
        disc_number: 0,
        rating: 0,
        color: None,
        play_count: 0,
        bitrate: 320,
        sample_rate: 44100,
        sample_depth: 16,
        file_size: 1000,
        file_path: format!("/Contents/{id}.mp3"),
        analyze_path: String::new(),
        date_added: String::new(),
        release_date: String::new(),
        artwork_id: None,
        artwork_path: None,
    }
}

/// Four tracks (two of them the same recording), a folder with a playlist, and a history list.
pub fn library() -> Library {
    let mut a = track(1, "Daft Punk", "Around the World");
    a.key = "8A".into();
    a.bpm = 121.0;
    a.rating = 5;
    a.genre = "House".into();
    a.color = Some(TrackColor::Aqua);
    a.date_added = "2026-09-30".into();
    a.play_count = 2;
    let mut b = track(2, "floorplan", "Never Grow Old");
    b.key = "Am".into();
    b.bpm = 128.0;
    b.rating = 3;
    b.genre = "Techno".into();
    b.date_added = "2025-01-02".into();
    b.duration_secs = 420;
    let mut c = track(3, "Floorplan", "Tell You No Lie");
    c.key = "3B".into();
    c.bpm = 124.9;
    c.genre = "techno".into();
    let mut d = a.clone();
    d.id = 4;
    d.file_path = "/Contents/copy/1.mp3".into();
    let playlist = |id, parent, name: &str, folder, track_ids| Playlist {
        id,
        parent_id: parent,
        name: name.into(),
        is_folder: folder,
        sort_order: 0,
        track_ids,
    };
    Library {
        tracks: vec![a, b, c, d],
        playlists: vec![playlist(1, None, "Sets", true, vec![]), playlist(2, Some(1), "Friday", false, vec![2, 1, 3])],
        history_playlists: 1,
        history: vec![playlist(9, None, "HISTORY 001", false, vec![3, 2])],
    }
}

/// A set played on `date` (`2026-10-03`), made now.
pub fn set(id: &str, date: &str, title: Option<&str>, venue: Option<&str>, tracks: Vec<SetEntry>) -> Set {
    let at = chrono::Utc::now();
    Set {
        id: id.into(),
        title: title.map(String::from),
        date: date.parse().unwrap(),
        venue: venue.map(String::from),
        tracks,
        created_at: at,
        updated_at: at,
        ..Default::default()
    }
}

/// The demo export from the library's fixtures, with placeholder audio, as the drive `DEMO` in a
/// temporary folder.
pub fn demo_drive() -> (TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let root = demo_drive_in(dir.path(), "DEMO");
    (dir, root)
}

/// [`demo_drive`], as the drive `name` in `dir`.
pub fn demo_drive_in(dir: &Path, name: &str) -> PathBuf {
    let fixture =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../crates/locallibrary/tests/fixtures/rekordcrate/demo_tracks");
    let root = dir.join(name);
    copy_dir(&fixture, &root);
    let device = Device::open_export(&root).unwrap();
    for t in device.library().unwrap().tracks {
        let p = device.path(&paths::key(&t.file_path));
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, "audio".repeat(100)).unwrap();
    }
    device.root().to_path_buf()
}

/// A hot cue (`hot`: 0 is A), or a memory cue, at `start_ms`; a loop when it has an end.
pub fn cue(hot: Option<u8>, start_ms: u32, end_ms: Option<u32>) -> Cue {
    Cue { hot_cue: hot, start_ms, end_ms, comment: String::new(), color: None, color_id: 0, loop_beats: None }
}

/// A grid at `bpm` from `first_ms`, `n` beats long.
pub fn grid(first_ms: u32, bpm: f64, n: u32) -> Vec<Beat> {
    let len = 60_000.0 / bpm;
    (0..n)
        .map(|i| Beat { beat_number: (i % 4 + 1) as u16, bpm, time_ms: first_ms + (i as f64 * len).round() as u32 })
        .collect()
}

/// A waveform of `n` columns at 150 a second, its back layer rising and falling every ten.
pub fn wave(n: usize) -> Waveform {
    let columns = (0..n)
        .map(|i| WaveColumn {
            back: WaveLayer { height: (i % 10) as f32 / 10.0, color: [200, 50, 50] },
            front: WaveLayer { height: 0.0, color: [255, 255, 255] },
        })
        .collect();
    Waveform { style: WaveformStyle::Rgb, columns_per_sec: Some(150.0), columns }
}

/// What the store and the drive's logbook know of the drive `drive`, as the Drives view reads it:
/// in the library `library`, and nothing more.
pub fn fleet(drive: DeviceInfo, library: Option<&str>) -> FleetDrive {
    FleetDrive {
        status: DriveStatus {
            drive,
            library: library.map(Into::into),
            relation: None,
            head: None,
            back_from_set: None,
            settings_drift: 0,
            error: None,
        },
        role: None,
        read_only: false,
        low_space: false,
        log: None,
        same_identity_as: Vec::new(),
        exported_over_edits: 0,
    }
}

/// An app showing the test library on a drive (an empty folder: nothing on it can be played or
/// edited) with no audio and no background polling.
pub fn app_with(lib: Library) -> (App, TempDir) {
    let dir = tempfile::tempdir().unwrap();
    (App::with_library(dir.path(), lib), dir)
}

/// An app showing the rekordbox export at `root` (its store inside it).
pub fn app_on(root: &Path) -> App {
    App::with_library(root, Device::open_export(root).unwrap().library().unwrap())
}

/// An app with no drive open, showing `view`, its store (not made yet) in a temporary folder.
pub fn blank(view: View) -> (App, TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let mut app = App::blank(dir.path().join("store"), None);
    app.view = view;
    (app, dir)
}

/// [`app_with`] the test library, its first track playing with `analysis` (its audio still
/// opening, so a jump shows as where playback will start), and no audio device.
pub fn playing(analysis: TrackAnalysis) -> (App, TempDir) {
    let (mut app, dir) = app_with(library());
    let l = app.loaded.as_ref().unwrap();
    let (track, root) = (l.library.tracks[0].clone(), l.root().to_path_buf());
    app.now = Some(NowPlaying {
        id: 1,
        key: None,
        drive: "TEST".into(),
        root,
        analysis,
        from_plugins: None,
        art: None,
        duration: Duration::from_secs(300),
        phrases: Vec::new(),
        matches: None,
        playlists: Vec::new(),
        loading: false,
        opening: true,
        advancing: false,
        track,
    });
    (app, dir)
}

/// Half-second WAV tracks at these sample rates on a drive (track `n` holds samples of `1000 n`),
/// queued and playing from the first, into a mixer at 8 kHz that's returned.
pub fn playing_wavs(rates: &[u32]) -> (TempDir, App, rodio::mixer::MixerSource) {
    playing_files(&rates.iter().map(|&r| (r, r as usize / 2)).collect::<Vec<_>>())
}

/// [`playing_wavs`], with tracks of so many samples at each rate.
pub fn playing_files(files: &[(u32, usize)]) -> (TempDir, App, rodio::mixer::MixerSource) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("Contents")).unwrap();
    let mut tracks = Vec::new();
    for (id, &(rate, samples)) in (1u32..).zip(files) {
        std::fs::write(dir.path().join(format!("Contents/{id}.wav")), wav(rate, samples, 1000 * id as i16)).unwrap();
        let mut t = track(id, "DJ", &format!("Track {id}"));
        t.file_path = format!("/Contents/{id}.wav");
        t.duration_secs = 0; // half a second, as the file says
        tracks.push(t);
    }
    let n = tracks.len();
    let mut app = App::with_library(dir.path(), Library { tracks, ..Default::default() });
    let (mut audio, out) = Audio::offline(8000);
    audio.set_volume(1.0);
    app.audio = Some(audio);
    let l = app.loaded.as_ref().unwrap();
    let (library, root) = (l.library.clone(), l.root().to_path_buf());
    app.play_tracks(library, (0..n).collect(), root, 0);
    (dir, app, out)
}

/// Wait for the track playing to be open and its analysis read, so what a test sets on it
/// isn't replaced by the analysis arriving after.
pub fn opened(app: &mut App) {
    wait(app, |a| a.now.as_ref().is_some_and(|n| !n.opening && !n.loading));
}

/// A job as the player runs one: planning, or writing (and whether it's too late to stop).
pub fn job(writing: bool, committed: bool) -> Job {
    Job {
        id: 99,
        label: if writing { "Writing TEST".into() } else { "Planning".into() },
        detail: String::new(),
        cancel: Default::default(),
        writing,
        committed,
        started: Instant::now(),
        touched: Vec::new(),
        after: None,
    }
}

/// Typing `keys`, one by one: `\n` is enter, `\t` tab, `\u{1b}` esc and `\u{7f}` del.
pub fn press(app: &mut App, keys: &str) {
    for c in keys.chars() {
        let code = match c {
            '\n' => KeyCode::Enter,
            '\t' => KeyCode::Tab,
            '\u{1b}' => KeyCode::Esc,
            '\u{7f}' => KeyCode::Delete,
            c => KeyCode::Char(c),
        };
        // As a terminal sends them: capitals with shift.
        let shifted = c.is_ascii_uppercase();
        app.on_key(KeyEvent::new(code, if shifted { KeyModifiers::SHIFT } else { KeyModifiers::NONE }));
    }
}

/// A dialog, the outputs list, the help or the log is open: drawn over the screen, it covers the
/// header and footer when it doesn't fit between them.
pub fn overlay(app: &App) -> bool {
    app.help || app.show_log || app.modal.is_some() || app.deck.outputs.is_some()
}

/// A key that isn't typed, such as an arrow or page down.
pub fn key(app: &mut App, code: KeyCode) {
    app.on_key(KeyEvent::new(code, KeyModifiers::NONE));
}

pub const LEFT: MouseEventKind = MouseEventKind::Down(MouseButton::Left);

pub fn mouse(app: &mut App, kind: MouseEventKind, (column, row): (u16, u16)) {
    app.on_mouse(MouseEvent { kind, column, row, modifiers: KeyModifiers::NONE });
}

/// The next click comes as soon after the last as a double click's does, however long the test
/// took between them (a loaded machine, a draw over Hegel's pipe): a double click is two within
/// 400 ms by the clock.
pub fn quick_again(app: &mut App) {
    app.last_click = app.last_click.take().map(|(_, row)| (Instant::now(), row));
}

/// Where `text` is drawn on a 120 by 30 screen, as the column and row to point at.
pub fn spot(app: &mut App, text: &str) -> (u16, u16) {
    spot_in(app, text, |_| Rect::new(0, 0, 120, 30))
}

/// Where `text` is drawn inside the pane `area` (as drawn on a 120 by 30 screen), to point at.
pub fn spot_in(app: &mut App, text: &str, area: impl Fn(&App) -> Rect) -> (u16, u16) {
    let s = screen(app, 120, 30);
    let r = area(app);
    for (row, line) in s.lines().enumerate().skip(r.y as usize).take(r.height as usize) {
        let inside: String = line.chars().skip(r.x as usize).take(r.width as usize).collect();
        if let Some(i) = inside.find(text) {
            return (r.x + inside[..i].chars().count() as u16, row as u16);
        }
    }
    panic!("{text:?} isn't in {r:?}:\n{s}")
}

/// `kind` (a click, the wheel) where `text` is drawn.
pub fn point(app: &mut App, kind: MouseEventKind, text: &str) {
    let at = spot(app, text);
    mouse(app, kind, at);
}

/// Run the app's background work until `done` (failing after a minute).
#[track_caller]
pub fn wait(app: &mut App, done: impl Fn(&App) -> bool) {
    let t = Instant::now();
    while !done(app) && t.elapsed() < Duration::from_secs(60) {
        // Woken by what the background sends, and now and then for what it doesn't (playback).
        if let Ok(msg) = app.rx.recv_timeout(Duration::from_millis(5)) {
            app.handle_msg(msg);
        }
        app.tick();
    }
    if !done(app) {
        // Hegel keeps a failure's panic message to itself when its replays pass, so where it
        // waited is written out first.
        let (job, modal) = (app.job.is_some(), app.modal.is_some());
        eprintln!(
            "wait at {} gave up after 60 s (job running: {job}, modal open: {modal})",
            std::panic::Location::caller()
        );
    }
    assert!(done(app), "timed out; last message: {:?}", last_message(app));
}

/// Handle the app's background messages until the store's sets are read. Opening a drive starts
/// a read of the sets in the store the app has then: a test that points the app at another store
/// waits for that read first, or it can land after the other store's and leave no sets.
pub fn read_sets(app: &mut App) {
    loop {
        let msg = app.rx.recv_timeout(Duration::from_secs(60)).expect("the sets read");
        let sets = matches!(msg, Msg::Sets(..));
        app.handle_msg(msg);
        if sets {
            return;
        }
    }
}

/// The message the footer shows now ("" for none).
pub fn last_message(app: &App) -> &str {
    app.toast.as_ref().map_or("", |t| t.0.as_str())
}

/// The confirmation up now, once planned, as the plan's operation and first summary line.
#[track_caller]
pub fn confirming(app: &mut App) -> (String, String) {
    wait(app, |a| a.job.is_none());
    match &app.modal {
        Some(Modal::Confirm(c)) => (c.plan.operation().into(), c.plan.summary().first().cloned().unwrap_or_default()),
        _ => panic!("no confirmation: {}", last_message(app)),
    }
}

/// Wait for the confirmation, and let it take keys (it ignores them right as it appears).
#[track_caller]
pub fn settle(app: &mut App) {
    wait(app, |a| a.job.is_none());
    let Some(Modal::Confirm(c)) = &mut app.modal else { panic!("no confirmation: {:?}", app.toast) };
    c.shown -= Duration::from_secs(1);
}

/// Say yes to the confirmation, and wait for the write and the reads after it.
#[track_caller]
pub fn say_yes(app: &mut App) {
    settle(app);
    press(app, "y");
    wait(app, |a| a.job.is_none() && a.loading.is_none());
}

/// Values made-up tracks' fields take, each with ties: some the same but for case or spacing,
/// some empty, some a category can't read (a key that isn't one, a date that isn't).
const ARTISTS: &[&str] = &["Daft Punk", "daft  punk ", "Floorplan", "Moodymann", "", " "];
const TITLES: &[&str] = &["Around the World", "Never Grow Old", "Pick Up", ""];
const ALBUMS: &[&str] = &["Homework", "homework", "", "Paradise"];
const GENRES: &[&str] = &["House", "Techno", "techno", "Deep House", ""];
const LABELS: &[&str] = &["Warp", "ostgut", "", "R&S"];
const KEYS: &[&str] = &["8A", "Am", "3B", "12B", "Db", "", "x"];
const BPMS: &[f64] = &[0.0, 119.99, 120.0, 124.9, 125.0, 128.0, 174.5];
const ADDED: &[&str] = &["2026-09-30", "2026-09-01", "2025-01-02", "", "2026", "abcdefg", "2026/09/30"];

/// Up to `max` made-up tracks (ids from 1), whose fields share values often enough for ties and
/// groups: now and then the same recording twice.
#[hegel::test_helper]
pub fn any_tracks(tc: &hegel::TestCase, max: usize) -> Vec<Track> {
    use hegel::generators as gs;
    let n = tc.draw(gs::integers::<u32>().max_value(max as u32));
    (1..=n)
        .map(|id| {
            let mut t = track(id, tc.draw(gs::sampled_from(ARTISTS)), tc.draw(gs::sampled_from(TITLES)));
            t.album = tc.draw(gs::sampled_from(ALBUMS)).into();
            t.genre = tc.draw(gs::sampled_from(GENRES)).into();
            t.label = tc.draw(gs::sampled_from(LABELS)).into();
            t.key = tc.draw(gs::sampled_from(KEYS)).into();
            t.bpm = tc.draw(gs::sampled_from(BPMS));
            t.date_added = tc.draw(gs::sampled_from(ADDED)).into();
            t.rating = tc.draw(gs::integers::<u8>().max_value(6));
            t.color = tc.draw(gs::booleans()).then(|| tc.draw(gs::sampled_from(&super::super::browse::COLOURS[..])));
            t.year = tc.draw(gs::sampled_from(&[0u16, 1997, 2014][..]));
            t.play_count = tc.draw(gs::integers::<u16>().max_value(3));
            t.bitrate = tc.draw(gs::sampled_from(&[0u32, 256, 320, 1411][..]));
            t.duration_secs = tc.draw(gs::sampled_from(&[300u32, 420][..]));
            t.comment = tc.draw(gs::sampled_from(&["", "Peak", "warm-up"][..])).into();
            t
        })
        .collect()
}

/// Every column the library sorts by.
pub const EVERY_FIELD: [locallibrary::query::SortField; 15] = {
    use locallibrary::query::SortField::*;
    [Title, Artist, Album, Genre, Label, Key, Bpm, Rating, Color, Year, Plays, Added, Time, Bitrate, Path]
};

/// A plan for the player's dialogs, as the library hands one back: `operation`, said in
/// `summary`, with `warnings`; `noop` when applying it would change nothing. Applying it writes
/// nothing, and reports `{}`.
#[derive(Default)]
pub struct Plan {
    pub operation: &'static str,
    pub summary: Vec<String>,
    pub warnings: Vec<String>,
    pub noop: bool,
    pub drives: Vec<locallibrary::op::Touched>,
}

impl locallibrary::op::AnyPlanned for Plan {
    fn id(&self) -> &str {
        "plan"
    }
    fn operation(&self) -> &'static str {
        self.operation
    }
    fn summary(&self) -> &[String] {
        &self.summary
    }
    fn warnings(&self) -> &[String] {
        &self.warnings
    }
    fn destructive(&self) -> bool {
        false
    }
    fn is_noop(&self) -> bool {
        self.noop
    }
    fn drives(&self) -> &[locallibrary::op::Touched] {
        &self.drives
    }
    fn planned_at(&self) -> chrono::DateTime<chrono::Utc> {
        chrono::Utc::now()
    }
    fn to_json(&self) -> serde_json::Value {
        serde_json::json!({})
    }
    fn check(&self, _: Option<&locallibrary::Store>) -> locallibrary::Result<()> {
        Ok(())
    }
    fn apply_json(
        &self,
        _: Option<&locallibrary::Store>,
        _: &locallibrary::Ctx<'_>,
    ) -> locallibrary::Result<serde_json::Value> {
        Ok(serde_json::json!({}))
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// Poll the volumes only when a test asks: a test's drive is a folder of files, which a poll
/// (once a second, as the background is waited for) takes for an unplugged drive.
pub fn hold_volume_polls(app: &mut App) {
    app.last_volume_poll = Instant::now() + Duration::from_secs(3600);
}

/// loclib-plugin's echo plugin, which the plugin host's test plugin serves: its export format
/// renamed (`m3u` is locallibrary's), its `echo` command taking a param of each kind the player's
/// form fills in, besides the test plugin's `log` and `context`, its `playlist` operation
/// `tracks` from the selection too (which it leaves be), and a setting of each kind beside its
/// greeting.
const ECHO: &str = include_str!("../../../../../crates/loclib-plugin/examples/echo/locallibrary-plugin.toml");

/// The plugins with the echo plugin among them, for this test: its manifest written in `dir` and
/// named by `LOCALLIBRARY_TEST_PLUGINS`, which the host reads once, as it's first used. Call it
/// before anything opens the palette or a menu.
pub fn echo_plugin(dir: &Path) {
    let params = r#"params = { type = "object", required = ["name"], properties = { name = { type = "string", minLength = 2, description = "Who to greet" }, times = { type = "integer" }, loud = { type = "boolean" }, mood = { type = "string", enum = ["calm", "wild"] }, file = { type = "string", x-path = true }, tags = { type = "array", items = { type = "string" } }, log = { type = "string" }, context = { type = "boolean" } } }"#;
    let operation = r#"params = { type = "object", properties = { name = { type = "string" }, tracks = { type = "array", items = { type = "integer" }, x-from = "selection.tracks" } }, required = ["name"] }"#;
    let manifest = ECHO
        .replace("format = \"m3u\"", "format = \"echo-m3u\"")
        .replacen("params = { type = \"object\" }", params, 1)
        .replace(
            r#"params = { type = "object", properties = { name = { type = "string" } }, required = ["name"] }"#,
            operation,
        );
    // Settings of each kind the player's form fills in, besides its greeting.
    let settings = r#"
[[contributes.settings]]
key = "speed"
title = "How fast"
type = "number"
default = 1.5

[[contributes.settings]]
key = "loud"
title = "Shout"
type = "boolean"

[[contributes.settings]]
key = "mood"
title = "Mood"
type = "enum"
choices = ["calm", "wild"]
default = "calm"
"#;
    let file = dir.join("echo.toml");
    std::fs::write(&file, manifest + settings).unwrap();
    // SAFETY: nextest runs each test in a process of its own, and this runs before the test
    // starts a thread.
    unsafe { std::env::set_var("LOCALLIBRARY_TEST_PLUGINS", &file) };
}

/// The echo plugin [`echo_plugin`] wrote in `dir` given an account, Echo Cloud (`main`), whose
/// sign-in page is on this computer, which only tests' plugin hosts reach: signing in fails
/// before anything is opened. Its secrets are kept in files.
pub fn echo_account(dir: &Path) {
    let file = dir.join("echo.toml");
    let manifest = std::fs::read_to_string(&file).unwrap();
    // First in the list, whatever a test granted after it.
    let granted = "capabilities = [";
    assert!(manifest.contains(granted));
    let account = r#"
[[contributes.accounts]]
id = "main"
title = "Echo Cloud"
authorize_url = "http://127.0.0.1:9/authorize"
token_url = "http://127.0.0.1:9/token"
client_id = "echo"
"#;
    let manifest = manifest.replacen(granted, r#"capabilities = ["net:echo.example", "#, 1) + account;
    std::fs::write(&file, manifest).unwrap();
    // SAFETY: as in `echo_plugin`.
    unsafe { std::env::set_var("LOCALLIBRARY_SESSION_STORE", "file") };
}

/// The drives in `dir` taken for mounted ones, for this test: a plugin finds the drive it's
/// given by its name among those (the echo plugin's exporter reads the drive). Call it before the
/// app starts.
pub fn mounted(dir: &Path) {
    // SAFETY: as in `echo_plugin`.
    unsafe { std::env::set_var("LOCALLIBRARY_VOLUMES", dir) };
}

/// The folder a property test's drives are mounted in, with the echo plugin's manifest beside
/// them: set up as the first case starts, for every case after it (the host reads its plugins
/// once a process, and the environment is best set before any thread runs).
pub fn plugins_once() -> &'static Path {
    plugins_once_with(|manifest| manifest)
}

/// [`plugins_once`], the echo plugin's manifest changed by `change` for this test's process (the
/// first call sets it up).
pub fn plugins_once_with(change: fn(String) -> String) -> &'static Path {
    static DIR: std::sync::OnceLock<TempDir> = std::sync::OnceLock::new();
    DIR.get_or_init(|| {
        let dir = tempfile::tempdir().unwrap();
        echo_plugin(dir.path());
        let file = dir.path().join("echo.toml");
        std::fs::write(&file, change(std::fs::read_to_string(&file).unwrap())).unwrap();
        echo_account(dir.path());
        mounted(dir.path());
        dir
    })
    .path()
}

/// A demo drive plugged in (see [`plugins_once`]) for one case, named for it alone, and taken out
/// again when it's dropped.
pub struct Plugged(pub PathBuf);

pub fn plugged() -> Plugged {
    static CASES: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let n = CASES.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    Plugged(demo_drive_in(plugins_once(), &format!("DEMO{n}")))
}

impl Drop for Plugged {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
