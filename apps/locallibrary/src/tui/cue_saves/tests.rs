use std::time::Duration;

use locallibrary::Device;
use locallibrary::anlz::TrackAnalysis;
use locallibrary::cue_edit;

use super::super::app::testing::*;
use super::super::app::{App, NowPlaying, View};
use super::super::keymap::Scope;
use ratatui::crossterm::event::KeyCode;

/// An app on the demo drive with its first track playing from `at_ms`, in player mode, on a
/// 120 BPM grid from 0.1 s.
fn playing_on_drive(root: &std::path::Path, at_ms: u64) -> App {
    let mut app = app_on(root);
    let track = app.loaded.as_ref().unwrap().library.tracks[0].clone();
    app.now = Some(NowPlaying {
        id: 1,
        key: None,
        root: root.to_path_buf(),
        analysis: TrackAnalysis { beats: grid(100, 120.0, 400), ..Default::default() },
        waves: Default::default(),
        from_plugins: None,
        art: None,
        duration: Duration::from_secs(200),
        phrases: Vec::new(),
        matches: None,
        playlists: Vec::new(),
        loading: false,
        opening: true,
        advancing: false,
        track,
    });
    app.pending_seek = Some(Duration::from_millis(at_ms));
    app.view = View::Library;
    app.deck_layout = true;
    app
}

/// The hot cues the drive's analysis files hold for the playing track: slot and time.
fn on_drive(app: &App) -> Vec<(String, u32)> {
    let now = app.now.as_ref().unwrap();
    let d = Device::open_export(&now.root).unwrap();
    let cues = cue_edit::list(&d, &[now.track.id]).unwrap().remove(0).cues;
    cues.into_iter().filter_map(|c| Some((c.hot_cue?, c.start_ms))).collect()
}

fn on_deck(app: &App) -> Vec<(u8, u32)> {
    let cues = &app.now.as_ref().unwrap().analysis.cues;
    cues.iter().filter_map(|c| Some((c.hot_cue?, c.start_ms))).collect()
}

fn settled(app: &App) -> bool {
    app.job.is_none() && app.cue_saving.is_none() && app.cue_saves.is_empty()
}

#[test]
fn an_empty_pad_sets_a_hot_cue_on_the_beat_and_del_takes_it_off() {
    let (_dir, root) = demo_drive();
    let before = on_drive(&playing_on_drive(&root, 0));
    // 1.23 s: the nearest beat of the grid is 1.1 s.
    let mut app = playing_on_drive(&root, 1_230);
    let free = (0..8u8).find(|s| before.iter().all(|(h, _)| h != &((b'A' + s) as char).to_string())).unwrap();
    let (digit, letter) = (((b'1' + free) as char).to_string(), ((b'A' + free) as char).to_string());

    press(&mut app, &digit);
    assert_eq!(on_deck(&app), [(free, 1_100)], "drawn at once");
    wait(&mut app, settled);
    assert!(on_drive(&app).contains(&(letter.clone(), 1_100)), "{:?}", on_drive(&app));
    assert_eq!(last_message(&app), format!("hot cue {letter} · u undo"));
    assert!(app.last_backup.is_some(), "u takes it back");

    // del, then the pad: the letters turn red while it waits for one.
    key(&mut app, KeyCode::Delete);
    assert_eq!((app.chord.is_some(), app.chord_scope), (true, Scope::DeleteCue));
    press(&mut app, &digit);
    assert!(app.chord.is_none() && on_deck(&app).is_empty());
    wait(&mut app, settled);
    assert!(!on_drive(&app).iter().any(|(h, _)| *h == letter));
    assert_eq!(last_message(&app), format!("hot cue {letter} deleted · u undo"));
}

#[test]
fn pads_pressed_quickly_are_all_written() {
    let (_dir, root) = demo_drive();
    let mut app = playing_on_drive(&root, 2_100);
    let taken: Vec<String> = on_drive(&app).into_iter().map(|(h, _)| h).collect();
    let free: Vec<u8> = (0..8u8).filter(|s| !taken.contains(&((b'A' + s) as char).to_string())).take(3).collect();
    for s in &free {
        press(&mut app, &((b'1' + s) as char).to_string());
    }
    wait(&mut app, settled);
    let written = on_drive(&app);
    for s in &free {
        assert!(written.contains(&(((b'A' + s) as char).to_string(), 2_100)), "{written:?}");
    }
}

#[test]
fn a_cue_that_cant_be_written_comes_back_off_the_deck() {
    // The test library's folder isn't a rekordbox export.
    let (mut app, _dir) = playing(TrackAnalysis::default());
    press(&mut app, "f");
    press(&mut app, "3");
    assert_eq!(on_deck(&app).len(), 1);
    wait(&mut app, settled);
    assert!(on_deck(&app).is_empty(), "taken back");
}

#[test]
fn quantize_off_sets_it_at_the_playhead() {
    let (_dir, root) = demo_drive();
    let mut app = playing_on_drive(&root, 1_230);
    app.deck.quantize = false;
    app.now.as_mut().unwrap().analysis.cues.clear();
    super::super::deck::set_hot_cue(&mut app, 7);
    assert_eq!(on_deck(&app), [(7, 1_230)]);
    wait(&mut app, settled);
}

#[test]
fn the_palette_sets_the_first_empty_pad() {
    let (mut app, _dir) = playing(TrackAnalysis { cues: vec![cue(Some(0), 0, None)], ..Default::default() });
    super::super::deck::set_next_hot_cue(&mut app);
    assert_eq!(on_deck(&app).iter().map(|c| c.0).collect::<Vec<_>>(), [0, 1]);
    let full =
        TrackAnalysis { cues: (0..8).map(|s| cue(Some(s), 1_000 * s as u32, None)).collect(), ..Default::default() };
    let (mut app, _dir) = playing(full);
    super::super::deck::set_next_hot_cue(&mut app);
    assert_eq!(last_message(&app), "pads A-H are all set");
}

#[test]
fn del_with_no_hot_cues_says_so() {
    let (mut app, _dir) = playing(TrackAnalysis::default());
    press(&mut app, "f");
    key(&mut app, KeyCode::Delete);
    assert!(app.chord.is_none());
    assert_eq!(last_message(&app), "no hot cues to delete");
}

/// What player mode draws, row by row, and the colour of the first cell holding `letter` on the
/// line over the waveform.
fn screen(app: &mut App) -> (Vec<String>, ratatui::buffer::Buffer) {
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(120, 30)).unwrap();
    terminal.draw(|f| super::super::ui::draw(f, app)).unwrap();
    let buf = terminal.backend().buffer().clone();
    let rows = (0..30).map(|y| (0..120).map(|x| buf[(x, y)].symbol()).collect::<String>()).collect();
    (rows, buf)
}

#[test]
fn after_del_the_cues_turn_red_until_a_pad_is_pressed() {
    let cues = vec![cue(Some(0), 1_000, None), cue(Some(2), 9_000, None)];
    let (mut app, _dir) = playing(TrackAnalysis { cues, beats: grid(100, 120.0, 400), ..Default::default() });
    press(&mut app, "f");
    let theme = super::super::theme::palette();
    let red = super::super::theme::rgb3((theme.marker)(super::super::theme::current().critical));
    let letters = |app: &mut App| {
        let (rows, buf) = screen(app);
        let y = rows.iter().position(|r| r.contains(" A ") && r.contains(" C ")).expect("the cue letters");
        let x = rows[y].find(" A ").unwrap() as u16 + 1;
        let cell = &buf[(x, y as u16)];
        (rows, cell.bg)
    };
    let (rows, before) = letters(&mut app);
    assert_ne!(before, red);
    key(&mut app, KeyCode::Delete);
    let (armed, during) = letters(&mut app);
    if std::env::var_os("SHOW_SCREEN").is_some() {
        eprintln!("{}\n----\n{}", rows.join("\n"), armed.join("\n"));
    }
    assert_eq!(during, red);
    assert!(armed.iter().any(|r| r.contains("DELETE CUE")), "the status line says what keys do now");
    press(&mut app, "9");
    assert_ne!(letters(&mut app).1, red, "any other key ends it");
}

#[test]
fn the_palettes_cue_sets_one() {
    let (mut app, _dir) = playing(TrackAnalysis::default());
    app.perform(super::super::keymap::Act::SetHotCue, None);
    assert_eq!(on_deck(&app).len(), 1);
}

#[test]
fn with_nothing_playing_the_commands_say_so() {
    let (mut app, _dir) = blank(View::Library);
    super::super::deck::set_next_hot_cue(&mut app);
    assert_eq!(last_message(&app), "nothing playing");
    super::super::deck::delete_chord(&mut app);
    assert_eq!(last_message(&app), "nothing playing");
}

#[test]
fn an_unplugged_drive_takes_no_cue() {
    let (mut app, _dir) = playing(TrackAnalysis::default());
    app.loaded.as_mut().unwrap().stale = true;
    press(&mut app, "f1");
    assert!(on_deck(&app).is_empty());
    assert_eq!(last_message(&app), "the drive was unplugged");
}

#[test]
fn deleting_an_empty_pad_says_so() {
    let (mut app, _dir) = playing(TrackAnalysis { cues: vec![cue(Some(0), 0, None)], ..Default::default() });
    press(&mut app, "f");
    key(&mut app, KeyCode::Delete);
    press(&mut app, "2");
    assert_eq!((last_message(&app), on_deck(&app).len()), ("no hot cue B", 1));
}

#[test]
fn a_cancelled_plan_or_a_failed_write_puts_the_deck_back() {
    let (mut app, _dir) = playing(TrackAnalysis { cues: vec![cue(Some(0), 0, None)], ..Default::default() });
    let root = app.now.as_ref().unwrap().root.clone();
    // Cancelled while planning: the cue set comes back off.
    press(&mut app, "f2");
    let id = app.job.as_ref().expect("planning").id;
    app.on_planned(id, root.clone(), Err(locallibrary::Error::Cancelled));
    assert_eq!(on_deck(&app), [(0, 0)]);
    assert_eq!(last_message(&app), "hot cue B: not saved");

    // Failed while writing: the cue deleted comes back.
    key(&mut app, KeyCode::Delete);
    press(&mut app, "1");
    assert!(on_deck(&app).is_empty());
    app.job = Some(job(true, false));
    app.on_applied(99, root, Err(locallibrary::Error::Invalid("the drive is full".into())));
    assert_eq!(on_deck(&app), [(0, 0)]);
    assert!(last_message(&app).contains("the drive is full"), "{}", last_message(&app));
}

#[test]
fn a_save_refused_as_it_starts_puts_the_deck_back() {
    let (mut app, _dir) = playing(TrackAnalysis::default());
    // Something else is being written: the pad's cue waits its turn.
    app.job = Some(job(true, false));
    press(&mut app, "f1");
    assert_eq!((on_deck(&app).len(), app.cue_saves.len()), (1, 1));
    // ...and the drive goes before its turn comes.
    app.loaded.as_mut().unwrap().stale = true;
    app.job = None;
    app.next_cue_save();
    assert!(on_deck(&app).is_empty() && app.cue_saves.is_empty() && app.cue_saving.is_none());
}

#[test]
fn a_cue_the_drive_has_already_changes_nothing() {
    let (_dir, root) = demo_drive();
    let mut app = playing_on_drive(&root, 4_100);
    let taken: Vec<String> = on_drive(&app).into_iter().map(|(h, _)| h).collect();
    let free = (0..8u8).find(|s| !taken.contains(&((b'A' + s) as char).to_string())).unwrap();
    let digit = ((b'1' + free) as char).to_string();
    press(&mut app, &digit);
    wait(&mut app, settled);
    // The deck forgets it (as a re-read might), and the pad sets it again where it is.
    app.now.as_mut().unwrap().analysis.cues.clear();
    app.toast = None;
    press(&mut app, &digit);
    wait(&mut app, settled);
    assert!(app.toast.is_none(), "nothing to say: {:?}", app.toast);
}

#[test]
fn the_hint_after_del_lists_the_pads() {
    let (mut app, _dir) = playing(TrackAnalysis { cues: vec![cue(Some(0), 0, None)], ..Default::default() });
    press(&mut app, "f");
    key(&mut app, KeyCode::Delete);
    app.chord = Some(std::time::Instant::now() - super::super::app::CHORD_HINT * 2);
    let (rows, _) = screen(&mut app);
    assert!(rows.iter().any(|r| r.contains("Delete cue")), "{}", rows.join("\n"));
    assert!(rows.iter().any(|r| r.contains("1 … 8") && r.contains("hot cue A … H")), "{}", rows.join("\n"));
}

#[test]
fn a_cue_not_saved_after_the_track_changed_leaves_the_deck_be() {
    let (mut app, _dir) = playing(TrackAnalysis::default());
    let root = app.now.as_ref().unwrap().root.clone();
    press(&mut app, "f1");
    let id = app.job.as_ref().expect("planning").id;
    // Another track is playing by the time it comes back.
    app.now.as_mut().unwrap().track.file_path = "/Contents/other.mp3".into();
    app.on_planned(id, root, Err(locallibrary::Error::Cancelled));
    assert_eq!(on_deck(&app).len(), 1, "the other track's cues aren't touched");
    assert_eq!(last_message(&app), "hot cue A: not saved");
}
