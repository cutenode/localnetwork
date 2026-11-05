//! What a frame of the player allocates, counted exactly: the 3,886-track fixture open, a demo
//! track with its real analysis playing into a test mixer, and the playhead moved a fixed step a
//! frame, so no clock or worker decides what's drawn. As with
//! `crates/locallibrary/tests/counters.rs`, with `LOCALLIBRARY_BUDGETS_OUT` set the counts go there
//! as `tui.json`, for `tools/budgets/cli.ts` to hold to `benchmarks/budgets/tui.json` (see
//! `benchmarks/README.md`).
//!
//! `LOCALLIBRARY_TUI_FRAMES` and `LOCALLIBRARY_TUI_VIEW` narrow a run: a run of no frames against
//! one of many is how `.github/actions/release-budgets` gets a frame's instructions. It draws in
//! `LOCALLIBRARY_COLOR`'s tier, or as the environment says without it.

use std::path::Path;
use std::sync::atomic::Ordering;
use std::time::Duration;

use indexmap::IndexMap;
use locallibrary::{Device, anlz, pdb};
use loclib_waveform::{Art, bar_layout};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use rodio::mixer::MixerSource;
use tempfile::TempDir;

use super::super::app::testing::{read_sets, wait};
use super::super::app::{App, Matches, Msg, NowPlaying, TrackInfo, View};
use super::super::audio::tests::wav_of;
use super::super::audio::{Audio, open_file};
use super::super::keys::Camelot;
use super::super::screens::{Fixture, fixture};
use super::super::{theme, ui};
use crate::counting::{ALLOCATIONS, BYTES};

/// Frames counted for each view, after a first one that isn't.
const FRAMES: u64 = 120;
const SIZE: (u16, u16) = (160, 45);
/// The test mixer's rate, and what plays at it between two frames at 60 fps.
const RATE: u32 = 8_000;
const SAMPLES_A_FRAME: usize = RATE as usize / 60;
/// Where playback starts: what's under the playhead is found from the track's start, so the first
/// seconds would flatter a frame.
const START: Duration = Duration::from_secs(45);

/// The views counted: each one's name in the budget, how it's shown, and text it has to show.
const VIEWS: [(&str, View, bool, &str); 3] = [
    ("library", View::Library, false, "Now playing"),
    ("deck", View::Library, true, "spectrum"),
    ("drives", View::Drives, false, "Demo"),
];

/// A minute of two tones, so the spectrum and the scope have something to show.
fn tones() -> Vec<u8> {
    let tone = |i: usize, hz: f32| (i as f32 * hz * std::f32::consts::TAU / RATE as f32).sin();
    let samples: Vec<i16> = (0..RATE as usize * 60)
        .map(|i| ((tone(i, 110.0) + tone(i, 1_760.0) * 0.4) * f32::from(i16::MAX) * 0.5) as i16)
        .collect();
    wav_of(RATE, &samples)
}

fn cover() -> Option<Art> {
    let mut png = Vec::new();
    image::RgbImage::from_fn(64, 64, |x, y| image::Rgb([x as u8 * 4, y as u8 * 4, 90]))
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .unwrap();
    Art::decode(&png)
}

/// The player as it is mid-set: the large fixture open, the Drives view's data read, and a demo
/// track playing with everything read for it, into the mixer that's returned.
fn mid_set() -> (App, MixerSource, TempDir, Fixture) {
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../crates/locallibrary/tests/fixtures");
    let export = std::fs::read(fixtures.join("rekordcrate/large/export.pdb")).unwrap();
    let large = pdb::into_library(pdb::parse_database(export, Path::new("export.pdb")).unwrap());
    let dir = tempfile::tempdir().unwrap();
    let mut app = App::with_library(dir.path(), large);
    read_sets(&mut app);
    wait(&mut app, |a| a.loaded.as_ref().is_some_and(|l| l.problems.missing.is_some() && !l.ranks.is_empty()));

    let mut drives = fixture();
    app.status = drives.app.status.take();
    app.drives = std::mem::take(&mut drives.app.drives);
    app.drive_state.select(Some(0));
    app.history = drives.app.history.take();

    let device = Device::open(&drives.root).unwrap();
    let track = device.library().unwrap().tracks.remove(0);
    let analysis = anlz::load(&device, &track);
    let l = app.loaded.as_ref().unwrap();
    let (library, root) = (l.library.clone(), l.root().to_path_buf());
    let matches = Some(Matches::find(library, root.clone(), &track, 40));
    app.now = Some(NowPlaying {
        id: 1,
        key: Camelot::parse(&track.key),
        drive: "LARGE".into(),
        root,
        analysis: Default::default(),
        from_plugins: None,
        art: None,
        duration: Duration::from_secs(u64::from(track.duration_secs)),
        phrases: Vec::new(),
        matches: None,
        playlists: Vec::new(),
        loading: true,
        opening: false,
        advancing: false,
        track,
    });
    let playlists = vec!["Sets › Friday".into()];
    let info = TrackInfo { analysis, from_plugins: None, art: cover(), matches, playlists };
    app.handle_msg(Msg::TrackInfo(1, Box::new(info)));

    let (mut audio, out) = Audio::offline(RATE);
    let file = dir.path().join("tones.wav");
    std::fs::write(&file, tones()).unwrap();
    audio.play(open_file(&file).unwrap(), false);
    audio.seek(START, false);
    app.audio = Some(audio);
    (app, out, dir, drives)
}

/// A frame's worth of audio played, and the visualisers fed as a tick feeds them, a fixed step on.
fn play_on(app: &mut App, out: &mut MixerSource) {
    out.by_ref().take(SAMPLES_A_FRAME).for_each(drop);
    let mut counts: Vec<usize> = app.areas.spectra.iter().map(|&(w, m)| bar_layout(w, m).0).collect();
    counts.dedup();
    let tap = app.audio.as_ref().unwrap().tap.clone();
    app.analyzer.update(&tap, true, &counts, 1.0 / 60.0);
}

/// What drawing `frames` frames after the first allocates in all, and the last one's text.
fn draw_frames(app: &mut App, out: &mut MixerSource, frames: u64) -> (u64, u64, String) {
    let mut terminal = Terminal::new(TestBackend::new(SIZE.0, SIZE.1)).unwrap();
    terminal.draw(|f| ui::draw(f, app)).unwrap();
    let (mut allocations, mut bytes) = (0, 0);
    for _ in 0..frames {
        play_on(app, out);
        let before = (ALLOCATIONS.load(Ordering::Relaxed), BYTES.load(Ordering::Relaxed));
        terminal.draw(|f| ui::draw(f, app)).unwrap();
        allocations += ALLOCATIONS.load(Ordering::Relaxed) - before.0;
        bytes += BYTES.load(Ordering::Relaxed) - before.1;
    }
    let text = terminal.backend().buffer().content().iter().map(|c| c.symbol()).collect();
    (allocations, bytes, text)
}

#[test]
fn frames_allocate_what_their_budgets_say() {
    let frames = std::env::var("LOCALLIBRARY_TUI_FRAMES").map_or(FRAMES, |n| n.parse().unwrap());
    let view = std::env::var("LOCALLIBRARY_TUI_VIEW").ok();
    theme::set_test_mode(Some(theme::Mode::Dark));
    theme::set_test_tier(std::env::var("LOCALLIBRARY_COLOR").ok().and_then(|t| theme::Tier::parse(&t)));
    let (mut app, mut out, _dir, _drives) = mid_set();

    let mut counts = IndexMap::new();
    for (name, shown, fullscreen, says) in VIEWS {
        if view.as_deref().is_some_and(|v| v != name) {
            continue;
        }
        (app.view, app.fullscreen) = (shown, fullscreen);
        let (allocations, bytes, text) = draw_frames(&mut app, &mut out, frames);
        assert!(text.contains(says), "{name} doesn't show {says:?}:\n{text}");
        counts.insert(format!("{name}.allocations"), allocations / frames.max(1));
        counts.insert(format!("{name}.bytes"), bytes / frames.max(1));
    }

    let Some(out) = std::env::var_os("LOCALLIBRARY_BUDGETS_OUT") else { return };
    std::fs::create_dir_all(&out).unwrap();
    std::fs::write(Path::new(&out).join("tui.json"), serde_json::to_vec_pretty(&counts).unwrap()).unwrap();
}
