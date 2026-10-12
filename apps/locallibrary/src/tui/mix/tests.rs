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
    let mut plays =
        vec![play(0, 0.0, 330.0, "8A", 126.0), play(1, 300.0, 640.0, "9A", 126.0), play(2, 680.0, 1000.0, "3B", 128.0)];
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
            t.drive_sources = names.iter().map(|n| Source::recent(format!("/Volumes/DNA/{n}.wav").into())).collect();
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
    assert!(last_message(&app).starts_with("nothing to play: export the project's audio"), "{}", last_message(&app));
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
    let (y, x) =
        (0..48).find_map(|y| row(y).find("1 Night Bus").map(|i| (y, row(y)[..i].chars().count()))).expect("the lanes");
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

/// While a set is identified, the header says how far it's got, whatever the progress: the
/// drive's tracks fingerprinted out of how many, and the share listened to (by bytes, else by
/// items), never past all of it.
#[hegel::test(test_cases = 300)]
fn identifying_says_how_far_it_has_got(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let (tx, _rx) = std::sync::mpsc::channel();
    let mut t = MixTab::new(tx);
    assert_eq!(t.status(), None);
    let pair = |tc: &hegel::TestCase| {
        let total = tc.draw(gs::integers::<u64>().max_value(1000));
        (tc.draw(gs::integers::<u64>().max_value(total + 5)), total)
    };
    let (items, bytes) = (pair(&tc), pair(&tc));
    // At every phase (and none yet), with nothing counted yet as well.
    for phase in
        [None, Some(Phase::Fingerprint), Some(Phase::Index), Some(Phase::Listen), Some(Phase::Match), Some(Phase::Copy)]
    {
        for (items, bytes) in [(items, bytes), ((0, 0), (0, 0))] {
            says_how_far(&mut t, phase, items, bytes);
        }
    }
}

/// [`identifying_says_how_far_it_has_got`] at `phase`, with `items` and `bytes` done.
fn says_how_far(t: &mut MixTab, phase: Option<Phase>, items: (u64, u64), bytes: (u64, u64)) {
    identifying(t, phase, items, bytes);
    let said = t.status().unwrap();
    let share = |(done, total): (u64, u64)| (total > 0).then(|| (done * 100 / total).min(100));
    match phase {
        Some(Phase::Fingerprint) if items.1 > 0 => {
            assert_eq!(said, format!("Fingerprinting the drive's tracks… {}/{}", items.0, items.1))
        }
        Some(Phase::Fingerprint) => assert_eq!(said, "Fingerprinting the drive's tracks…"),
        Some(Phase::Index) => assert_eq!(said, "Indexing fingerprints…"),
        Some(Phase::Listen) => {
            let shown: Option<u64> = said
                .strip_prefix("Listening to Sunday.wav…")
                .expect(&said)
                .trim()
                .strip_suffix('%')
                .map(|p| p.parse().unwrap());
            assert_eq!(shown, share(bytes).or(share(items)), "{said}");
            assert!(shown.is_none_or(|p| p <= 100));
        }
        Some(Phase::Match) => assert_eq!(said, "Finding tracks in Sunday.wav…"),
        _ => assert_eq!(said, "Opening Sunday.wav…"),
    }
    // The view says it too, and how to stop.
    let screen = render(t, &Context { status: Some(said.clone()), ..quiet() }, 140, 20);
    assert!(screen.contains("esc stops"), "{screen}");
}

/// A path typed or dropped into the terminal reads as itself: in quotes, with backslashes before
/// what the shell would take, or as typed; `~/` is the home folder.
#[hegel::test(test_cases = 300)]
fn a_typed_path_reads_as_the_path(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let parts: Vec<&str> = tc.draw(
        gs::vecs(gs::sampled_from(&["Sets", "Sunday Warm-up", "it's", "a\\b", "日本"][..])).min_size(1).max_size(3),
    );
    let path = format!("/{}.wav", parts.join("/"));
    let escaped: String =
        path.chars().flat_map(|c| if matches!(c, ' ' | '\'' | '\\') { vec!['\\', c] } else { vec![c] }).collect();
    let pad = tc.draw(gs::sampled_from(&["", " ", "  "][..]));
    let typed = match tc.draw(gs::integers::<u8>().max_value(2)) {
        0 if !path.contains('\'') => format!("{pad}'{path}'{pad}"),
        1 => format!("{pad}\"{path}\"{pad}"),
        _ => format!("{pad}{escaped}{pad}"),
    };
    assert_eq!(typed_path(&typed), PathBuf::from(&path), "{typed:?}");
    if let Some(home) = std::env::var_os("HOME") {
        assert_eq!(typed_path(&format!("~{escaped}")), PathBuf::from(home).join(&path[1..]));
    }
}

/// The set played from the Mixes view: n goes to the start of the next track (or says that was
/// the last), p back to the start of this one, or, near its start, the one before (or the set's
/// start); space plays from the selected track, enter on a track from it, and a click on the
/// timeline from there; z zooms in two, four, eight times and back.
#[hegel::test(test_cases = 30, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn the_set_plays_through_its_tracks(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let (mut app, _dir, _out) = playing_app();
    let starts: Vec<f64> = app.mix.report().unwrap().plays.iter().map(|p| p.start).collect();
    let plays: Vec<(f64, f64)> = app.mix.report().unwrap().plays.iter().map(|p| (p.start, p.end)).collect();
    let mut zoom = 1;
    // Each kind of step in turn, in an order drawn for the case.
    let mut turns: Vec<u8> = (0..7).collect();
    for k in (1..turns.len()).rev() {
        turns.swap(k, tc.draw(gs::integers::<usize>().max_value(k)));
    }
    for step in 0..tc.draw(gs::integers::<usize>().min_value(7).max_value(21)) {
        let loaded = set_loaded(&app);
        let pos = at(&app);
        match turns[step % turns.len()] {
            0 => {
                press(&mut app, "n");
                if !loaded {
                    continue;
                }
                match starts.iter().find(|s| **s > pos + 0.5) {
                    Some(&s) => assert_eq!(at(&app), s, "n from {pos}"),
                    None => assert_eq!(last_message(&app), "that's the last track in the set"),
                }
            }
            1 => {
                press(&mut app, "p");
                if !loaded {
                    continue;
                }
                // The track heard: the one that came in last of those playing.
                let current = (plays.iter())
                    .filter(|(s, e)| *s <= pos && pos < *e)
                    .map(|p| p.0)
                    .fold(None, |a: Option<f64>, s| Some(a.map_or(s, |a| a.max(s))));
                let want = match current {
                    Some(s) if pos - s > 3.0 => s,
                    _ => starts.iter().rev().find(|s| **s < pos - 3.0).copied().unwrap_or(0.0),
                };
                assert_eq!(at(&app), want, "p from {pos}");
            }
            2 => {
                let to = tc.draw(gs::sampled_from(&[0u64, 2, 299, 305, 640, 700, 990][..]));
                app.seek_to(Duration::from_secs(to));
            }
            3 => {
                let row = tc.draw(gs::integers::<usize>().max_value(starts.len() - 1));
                (app.mix.pane, app.mix.rows) = (Pane::Tracks, TableState::default().with_selected(Some(row)));
                press(&mut app, if loaded { "\n" } else { " " });
                opened(&mut app);
                assert_eq!(at(&app), starts[row], "from the track picked");
            }
            4 => {
                screen(&mut app, 160, 48);
                let r = app.mix.areas.timeline;
                let (from, to) = app.mix.areas.window;
                let x = tc.draw(gs::integers::<u16>().max_value(r.width.saturating_sub(1)));
                mouse(&mut app, LEFT, (r.x + x, r.y));
                let t = from + (x as f64 + 0.5) / r.width.max(1) as f64 * (to - from);
                if loaded {
                    assert!((at(&app) - t).abs() < 0.01, "clicked at {t}, playing from {}", at(&app));
                } else if let Some(i) = play_at(&app.mix.report().unwrap().plays, t) {
                    assert_eq!((app.mix.pane, app.mix.rows.selected()), (Pane::Tracks, Some(i)));
                }
            }
            5 => {
                // Round the zooms and back.
                for _ in 0..4 {
                    press(&mut app, "z");
                    zoom = if zoom == 8 { 1 } else { zoom * 2 };
                    assert_eq!(app.mix.zoom, zoom);
                }
            }
            _ => {
                let s = screen(&mut app, 160, 48);
                if loaded {
                    let head = format!("▶ {} / 16:40", clock(at(&app)));
                    assert!(s.contains(&head), "{head}\n{s}");
                }
                if zoom > 1 {
                    assert!(s.contains(&format!("{zoom}× ")), "{s}");
                }
            }
        }
    }
}

/// Saving a set's data or its report needs a set open, and the drive it was matched against at
/// its root: without that drive it says why; with it, the data is written into the store.
#[hegel::test(test_cases = 20, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn a_sets_data_is_written_only_from_its_drive(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let (mut app, dir) = blank(View::Mixes);
    let report = tc.draw(gs::booleans());
    let key = if report { "w" } else { "E" };
    let drive = dir.path().join("drive");
    match tc.draw(gs::integers::<u8>().max_value(2)) {
        0 => {
            press(&mut app, key);
            return assert_eq!(last_message(&app), "open a set first");
        }
        1 => {
            app.mix = tab();
            if let Some(Ok(o)) = &mut app.mix.opened {
                o.root = dir.path().join("gone");
            }
        }
        _ => {
            std::fs::create_dir_all(&drive).unwrap();
            app.mix = tab();
            if let Some(Ok(o)) = &mut app.mix.opened {
                o.root = drive.clone();
                o.report.drive = "drive".into();
                // Another drive's set, now and then.
                if tc.draw(gs::booleans()) {
                    o.report.drive_id = Some(uuid::Uuid::new_v4());
                }
            }
        }
    }
    let matched = drive.is_dir() && matches!(&app.mix.opened, Some(Ok(o)) if o.report.drive_id.is_none());
    if report && matched {
        // (Written, it would open in the browser.)
        return;
    }
    press(&mut app, key);
    let (start, end) = if report { ("writing ", ".html…") } else { ("saving ", ".mix.json…") };
    assert!(last_message(&app).starts_with(start) && last_message(&app).ends_with(end), "{}", last_message(&app));
    wait(&mut app, |a| !last_message(a).starts_with(start));
    let said = last_message(&app);
    match (matched, drive.is_dir()) {
        (true, _) => {
            assert!(said.starts_with("wrote ") && said.ends_with("(attach it to the mix on Local Network)"), "{said}")
        }
        (false, true) => assert!(said.contains("isn't the one the set was matched against"), "{said}"),
        (false, false) => assert!(said.starts_with("the set's tracks are read from its drive"), "{said}"),
    }
}

/// The selected play is drawn as the set's report says it went, whatever that was: how it came in
/// and went out (a blend, a cut, a gap) and the tempo then, its label and genre, played with key
/// lock or pitched, its jumps, how sure the match is, the other tracks on the drive it may be,
/// whether its key is known; with its waveform, cues and phrases from the drive, and the set's
/// cover beside the title.
#[hegel::test(test_cases = 60)]
fn a_play_is_drawn_as_the_report_says(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let mut t = tab();
    let i = 1;
    let Some(Ok(o)) = &mut t.opened else { unreachable!() };
    let r = &mut o.report;
    r.unidentified.clear();
    let overlap = tc.draw(gs::sampled_from(&[30.0, 0.5, -75.0, -5.0][..]));
    r.transitions[0].overlap = overlap;
    r.transitions[0].tempo_change = tc.draw(gs::sampled_from(&[None, Some(0.0), Some(1.6)][..]));
    let p = &mut r.plays[i];
    p.key_lock = tc.draw(gs::sampled_from(&[None, Some(true), Some(false)][..]));
    p.semitones = tc.draw(gs::sampled_from(&[0.0, -1.0, 2.5][..]));
    p.jumps = tc.draw(gs::integers::<u32>().max_value(3));
    p.confidence = tc.draw(gs::sampled_from(&[0.4, 0.9][..]));
    (p.track.label, p.track.genre) = (
        tc.draw(gs::sampled_from(&["", "Hessle Audio"][..])).into(),
        tc.draw(gs::sampled_from(&["", "Techno"][..])).into(),
    );
    let known = tc.draw(gs::booleans());
    if !known {
        (p.key, p.played_key, p.track.key) = (None, None, String::new());
    }
    let alternate = |name: &str, same_audio| Alternate {
        track_id: 9,
        name: name.into(),
        file_path: "/Contents/9.mp3".into(),
        same_audio,
    };
    let alt = tc.draw(gs::integers::<u8>().max_value(3));
    p.alternates = match alt {
        0 => Vec::new(),
        1 => vec![alternate(&p.track.display_name(), false)],
        2 => vec![alternate("Ferrule - Halogen (Edit)", true)],
        _ => vec![alternate("Ferrule - Halogen (Dub)", false)],
    };
    let analysed = tc.draw(gs::booleans());
    let mut a = analysis();
    a.detail = tc.draw(gs::booleans()).then(|| wave(150 * 400));
    o.analyses =
        vec![TrackAnalysis::default(), if analysed { a } else { TrackAnalysis::default() }, TrackAnalysis::default()]
            .into();
    if tc.draw(gs::booleans()) {
        let mut png = Vec::new();
        image::RgbImage::from_pixel(8, 8, image::Rgb([200, 40, 90]))
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        o.cover = Art::decode(&png);
    }
    let p = o.report.plays[i].clone();
    t.rows.select(Some(i));
    t.zoom = tc.draw(gs::sampled_from(&[1u8, 2, 8][..]));
    // (Where it plays, when it's playing: the selection follows the playhead.)
    let playhead = tc.draw(gs::optional(gs::sampled_from(&[350.0, 500.0][..])));
    let screen = render(&mut t, &Context { playhead, ..quiet() }, 160, 60);
    let mut want: Vec<String> = Vec::new();
    want.push(match overlap {
        o if o >= 1.0 => format!("{} blend", secs(o)),
        o if o > -1.0 => "cut".into(),
        o => format!("{} gap", secs(-o)),
    });
    match p.key_lock {
        Some(true) => want.push("key lock".into()),
        Some(false) if p.semitones.abs() >= 0.05 => want.push(format!("pitched {:+.1} semitones", p.semitones)),
        _ => {}
    }
    if p.jumps > 0 {
        want.push(format!("{} jump{}: looped or skipped", p.jumps, if p.jumps == 1 { "" } else { "s" }));
    }
    if p.is_tentative() {
        want.push("(tentative)".into());
    }
    let extra: Vec<&str> =
        [p.track.label.as_str(), p.track.genre.as_str()].into_iter().filter(|s| !s.is_empty()).collect();
    if !extra.is_empty() {
        want.push(extra.join(" · "));
    }
    match alt {
        1 => want.push("Also on the drive as /Contents/9.mp3".into()),
        2 => want.push("Same audio as Ferrule - Halogen (Edit)".into()),
        3 => want.push("Shares material with Ferrule - Halogen (Dub)".into()),
        _ => {}
    }
    // (Its last line, when the panel has room for it.)
    if !known && p.jumps == 0 && alt == 0 {
        want.push("No key in the library for this track.".into());
    }
    if t.zoom > 1 {
        want.push(format!("{}× ", t.zoom));
    }
    if let Some(pos) = playhead {
        want.push(format!("▶ {} / 16:40", clock(pos)));
    }
    if analysed {
        want.push("Intro".into());
    }
    for said in &want {
        assert!(screen.contains(said.as_str()), "{said:?}\n{screen}");
    }
    for (w, h) in [(100, 34), (60, 20)] {
        assert_eq!(render(&mut t, &quiet(), w, h).lines().count(), h as usize);
    }
}

/// The Mixes view answers to its keys and clicks: enter on the set shown goes to its tracks, on
/// another recording identifies it (with no drive open, saying one's needed), and esc stops that;
/// a result for an identification since dropped changes nothing, a failed one is said in place of
/// the set; space with no audio to play says so; n and p with the set not playing move through the
/// queue instead, o finds the selected track's drive (or says it's another); a click picks a
/// recording (a second opens it) or a track; del forgets a recording opened on its own, not the
/// drive's.
#[hegel::test(test_cases = 20, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn the_mixes_view_answers_to_its_keys_and_clicks(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let (mut app, dir) = blank(View::Mixes);
    let set = wav_at(&dir.path().join("Sunday Warm-up.wav"));
    let other = wav_at(&dir.path().join("Friday.wav"));
    app.mix = tab_on(app.sender());
    let audio = tc.draw(gs::booleans());
    if let Some(Ok(o)) = &mut app.mix.opened {
        o.report.mix.path = set.clone();
        o.audio = audio.then(|| set.clone());
    }
    app.mix.set_recent(&[set.clone(), other.clone()]);
    let mut turns: Vec<u8> = (0..9).collect();
    for k in (1..turns.len()).rev() {
        turns.swap(k, tc.draw(gs::integers::<usize>().max_value(k)));
    }
    for step in 0..tc.draw(gs::integers::<usize>().min_value(9).max_value(18)) {
        screen(&mut app, 160, 48);
        app.mix.input = None;
        match turns[step % turns.len()] {
            0 => {
                app.mix.pane = Pane::Mixes;
                // (Forgotten by now, maybe.)
                let Some(i) = app.mix.sources.iter().position(|s| s.path == set) else { continue };
                app.mix.list.select(Some(i));
                press(&mut app, "\n");
                if matches!(&app.mix.opened, Some(Ok(_))) {
                    assert_eq!(app.mix.pane, Pane::Tracks, "on to its tracks");
                }
            }
            1 => {
                app.mix.pane = Pane::Mixes;
                if let Some(i) = app.mix.sources.iter().position(|s| s.path == other) {
                    app.mix.list.select(Some(i));
                    press(&mut app, "\n");
                    assert!(
                        last_message(&app).starts_with("open a drive in the library first"),
                        "{}",
                        last_message(&app)
                    );
                }
                // Identifying one (against a drive opened since): esc stops it.
                identifying(&mut app.mix, Some(Phase::Listen), (0, 0), (0, 0));
                assert!(screen(&mut app, 160, 48).contains("Listening to Sunday.wav"));
                press(&mut app, "\u{1b}");
                assert!(!app.mix.is_loading() && last_message(&app) == "stopped");
            }
            2 => {
                let opened = app.mix.opened.take();
                let generation = app.mix.generation;
                identifying(&mut app.mix, None, (0, 0), (0, 0));
                loaded(&mut app, generation + 7, Err("too late".into()));
                assert!(app.mix.opened.is_none() && app.mix.is_loading(), "a dropped one's result changes nothing");
                loaded(&mut app, generation, Err("set.wav: no audio in it".into()));
                let s = screen(&mut app, 160, 48);
                assert!(s.contains("set.wav: no audio in it") && s.contains("enter tries again"), "{s}");
                app.mix.opened = opened;
            }
            3 => {
                if app.now.is_some() || !matches!(&app.mix.opened, Some(Ok(_))) {
                    continue;
                }
                press(&mut app, " ");
                if !audio {
                    assert!(last_message(&app).starts_with("nothing to play: export the project's audio"));
                }
            }
            4 => {
                let playing = app.now.is_some();
                press(&mut app, if tc.draw(gs::booleans()) { "n" } else { "p" });
                if !playing {
                    assert!(app.now.is_none(), "nothing in the queue to move through");
                }
            }
            5 => {
                press(&mut app, "o");
                if matches!(&app.mix.opened, Some(Ok(_))) && app.mix.rows.selected().is_some() {
                    assert_eq!((last_message(&app), app.view), ("the track is on another drive", View::Mixes));
                }
                app.view = View::Mixes;
            }
            6 => {
                // The list's rows take two lines each, under its border.
                let a = app.mix.areas.list;
                let k = tc.draw(gs::integers::<usize>().max_value(app.mix.sources.len().saturating_sub(1)));
                let row = a.y + 1 + 2 * (k - app.mix.list.offset().min(k)) as u16;
                if app.mix.sources.is_empty() || row >= a.y + a.height {
                    continue;
                }
                app.mix.list.select(None);
                mouse(&mut app, LEFT, (a.x + 2, row));
                assert_eq!((app.mix.pane, app.mix.list.selected()), (Pane::Mixes, Some(k)));
                mouse(&mut app, LEFT, (a.x + 2, row));
            }
            7 => {
                let a = app.mix.areas.rows;
                let n = app.mix.report().map_or(0, |r| r.plays.len());
                if a.is_empty() || n == 0 {
                    continue;
                }
                let k = tc.draw(gs::integers::<usize>().max_value(n - 1));
                let row = a.y + 2 + 2 * (k - app.mix.rows.offset().min(k)) as u16;
                if row >= a.y + a.height {
                    continue;
                }
                mouse(&mut app, LEFT, (a.x + 2, row));
                assert_eq!((app.mix.pane, app.mix.rows.selected()), (Pane::Tracks, Some(k)));
            }
            _ => {
                app.mix.pane = Pane::Mixes;
                let before = app.mix.sources.len();
                let Some(i) = app.mix.list.selected() else { continue };
                let drive = app.mix.sources[i].folder.is_none() && !app.mix.recent.contains(&app.mix.sources[i].path);
                key(&mut app, KeyCode::Delete);
                assert_eq!(app.mix.sources.len(), if drive { before } else { before - 1 });
            }
        }
        let (w, h) = (tc.draw(gs::integers::<u16>().min_value(1).max_value(200)), 40);
        assert_eq!(screen(&mut app, w, h).lines().count(), h as usize);
    }
}

/// The open drive's recordings are listed with how long each is and where it's from, beside
/// those opened from this computer (which del forgets; the drive's stay); one being identified is
/// marked; tab goes to the set's tracks only when there's a set with some; space with the set
/// paused plays it on, enter on nothing asks for a path, and esc, o and space do the app's when
/// there's nothing of the Mixes view's to do; with no drive open the view says one's needed.
#[hegel::test(test_cases = 15, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn the_drives_recordings_are_listed_and_answer_to_keys(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let dir = tempfile::tempdir().unwrap();
    let drive = dir.path().join("DNA");
    let n = tc.draw(gs::integers::<usize>().max_value(3));
    for i in 0..n {
        wav_at(&drive.join(format!("PIONEER REC/REC00{i}.wav")));
    }
    std::fs::create_dir_all(&drive).unwrap();
    let mut app = App::with_library(&drive, library());
    app.view = View::Mixes;
    hold_volume_polls(&mut app);
    let own = wav_at(&dir.path().join("own.wav"));
    app.mix.set_recent(std::slice::from_ref(&own));
    screen(&mut app, 120, 30);
    wait(&mut app, |a| a.mix.sources.len() == n + 1);
    let s = screen(&mut app, 120, 30);
    for i in 0..n {
        assert!(s.contains(&format!("REC00{i}.wav")) && s.contains("0:00 · DNA"), "{s}");
    }
    let mut turns: Vec<u8> = (0..7).collect();
    for k in (1..turns.len()).rev() {
        turns.swap(k, tc.draw(gs::integers::<usize>().max_value(k)));
    }
    for turn in turns {
        app.view = View::Mixes;
        match turn {
            0 => {
                // One being identified is marked.
                let Some(rec) = app.mix.sources.iter().find(|s| s.place == "DNA").map(|s| s.path.clone()) else {
                    continue;
                };
                identifying(&mut app.mix, None, (0, 0), (0, 0));
                app.mix.loading.as_mut().unwrap().path = rec.clone();
                let s = screen(&mut app, 120, 30);
                assert!(s.contains(&format!("⟳ {}", rec.file_name().unwrap().to_string_lossy())), "{s}");
                press(&mut app, "\u{1b}");
            }
            1 => {
                // del on a drive's recording keeps it; on one from this computer, forgets it.
                let i = tc.draw(gs::integers::<usize>().max_value(app.mix.sources.len().saturating_sub(1)));
                let Some(s) = app.mix.sources.get(i).cloned() else { continue };
                (app.mix.pane, app.mix.list) = (Pane::Mixes, ListState::default().with_selected(Some(i)));
                let before = app.mix.sources.len();
                key(&mut app, KeyCode::Delete);
                assert_eq!(app.mix.sources.len(), if s.place == "DNA" { before } else { before - 1 });
            }
            2 => {
                app.mix.pane = Pane::Mixes;
                press(&mut app, "\t");
                let tracks = app.mix.report().is_some_and(|r| !r.plays.is_empty());
                assert_eq!(app.mix.pane, if tracks { Pane::Tracks } else { Pane::Mixes });
            }
            3 => {
                // Nothing selected: enter asks for a path.
                (app.mix.pane, app.mix.list) = (Pane::Mixes, ListState::default());
                press(&mut app, "\n");
                assert_eq!(app.mix.input.as_deref(), Some(""));
                press(&mut app, "\u{1b}");
            }
            4 => {
                // Nothing of the view's to do: esc, o and space go on to the app.
                (app.mix.pane, app.mix.opened) = (Pane::Mixes, None);
                assert!(!escape(&mut app) && !go_to(&mut app));
                assert!(!super::play(&mut app), "nothing to play here");
                assert!(!track_jump(&mut app, true));
            }
            5 => {
                // The set paused: space plays it on.
                let (audio, _out) = Audio::offline(8000);
                app.audio = Some(audio);
                (app.mix.opened, app.mix.rows) = (Some(Ok(opened_set())), TableState::default().with_selected(Some(1)));
                let set = wav_at(&dir.path().join("Sunday Warm-up.wav"));
                if let Some(Ok(o)) = &mut app.mix.opened {
                    o.audio = Some(set);
                }
                press(&mut app, " ");
                opened(&mut app);
                app.audio.as_ref().unwrap().toggle_pause();
                let at = app.mix.report().unwrap().plays[1].start;
                play_from(&mut app, at);
                assert!(!app.audio.as_ref().unwrap().is_paused(), "on again");
                press(&mut app, "x");
            }
            _ => {
                // With no drive open, the view says one's needed.
                let (mut none, _store) = blank(View::Mixes);
                let s = screen(&mut none, 120, 30);
                assert!(s.contains("No drive open.") && s.contains("Open a drive in the library first (1)"), "{s}");
            }
        }
    }
}
