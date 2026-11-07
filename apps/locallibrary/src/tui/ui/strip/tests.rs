use std::time::Duration;

use hegel::generators as gs;
use locallibrary::anlz::TrackAnalysis;
use ratatui::Terminal;
use ratatui::backend::TestBackend;

use crate::tui::app::testing::*;
use crate::tui::app::{App, View};
use crate::tui::deck::Loop;
use crate::tui::ui::{draw, screen};

use super::*;

/// The deck's rows of a frame, at 120x40: its line, then the waveform.
fn deck(app: &mut App) -> Vec<Vec<Cell>> {
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    terminal.draw(|f| draw(f, app)).unwrap();
    let buf = terminal.backend().buffer();
    let r = app.areas.overview;
    (r.y..r.bottom()).map(|y| (r.x..r.right()).map(|x| buf[(x, y)].clone()).collect()).collect()
}

/// Only what the playhead crossed changes between frames, and the frames' waveform is what the
/// widget draws in one go.
#[hegel::test(test_cases = 40)]
fn prop_a_frame_only_moves_the_playhead(tc: hegel::TestCase) {
    let cues = vec![cue(Some(0), 40_000, None), cue(Some(1), 200_000, Some(210_000))];
    let (mut app, _dir) = playing(TrackAnalysis { preview: Some(wave(400)), cues, ..Default::default() });
    let at = |s: u64| Some(Duration::from_secs(s));
    let (from, to) = (tc.draw(gs::integers::<u64>().max_value(300)), tc.draw(gs::integers::<u64>().max_value(300)));
    app.resume = at(from);
    let first = deck(&mut app);
    app.resume = at(to);
    let second = deck(&mut app);
    let head = |s: u64| ((s as f64 / 300.0 * 120.0) as usize).min(119);
    let (lo, hi) = (head(from.min(to)).saturating_sub(1), head(from.max(to)) + 1);
    for (y, (a, b)) in first.iter().zip(&second).enumerate() {
        for x in (0..120).filter(|x| !(lo..=hi).contains(x)) {
            assert_eq!(a[x], b[x], "row {y} column {x}, the playhead going from {lo} to {hi}");
        }
    }
    // The same as drawing it whole, playhead aside.
    let drawn = {
        let mut buf = Buffer::empty(Rect::new(0, 0, 120, second.len() as u16));
        let n = app.now.as_ref().unwrap();
        let cues = &n.analysis.cues;
        let progress = to as f32 / 300.0;
        let palette = theme::palette();
        Overview { wave: n.analysis.preview.as_ref(), progress, cues, duration_ms: 300_000.0, palette }
            .render(buf.area, &mut buf);
        buf
    };
    for (y, row) in second.iter().enumerate() {
        for (x, cell) in row.iter().enumerate().filter(|(x, _)| x.abs_diff(head(to)) > 1) {
            assert_eq!(cell, &drawn[(x as u16, y as u16)], "row {y} column {x}");
        }
    }
}

/// The waveform is drawn again for what it shows: another track or size, the analysis arriving, a
/// loop coming on.
#[test]
fn the_waveform_follows_what_it_shows() {
    let (mut app, _dir) = playing(TrackAnalysis::default());
    let rail = deck(&mut app);
    assert!(rail.iter().flatten().any(|c| c.symbol() == "●"), "a rail with no waveform");
    assert!(rail.iter().flatten().all(|c| c.symbol() != "│"), "and no line through it");
    app.now.as_mut().unwrap().analysis.preview = Some(wave(400));
    let waved = deck(&mut app);
    assert_ne!(rail, waved, "the waveform, once it's read");
    app.deck.looped = Some(Loop { start: 30_000.0, end: 60_000.0, on: true });
    let looped = deck(&mut app);
    assert_ne!(waved, looped, "the loop's marked");
    app.now = None;
    let idle = deck(&mut app);
    assert!(idle.iter().flatten().all(|c| c.symbol() != "▁"), "no wave left behind");
}

#[test]
fn the_line_says_whats_playing() {
    let cues = vec![cue(Some(0), 1_000, None), cue(Some(3), 64_000, None), cue(Some(5), 90_000, None)];
    let cues = [cues, (1..7).map(|n| cue(Some(n), 1_000 * u32::from(n), None)).collect()].concat();
    let (mut app, _dir) = playing(TrackAnalysis { preview: Some(wave(400)), cues, ..Default::default() });
    let t = &mut app.now.as_mut().unwrap().track;
    t.bpm = 124.0;
    t.key = "8A".into();
    let (title, artist) = (t.title.clone(), t.artist.clone());
    app.resume = Some(Duration::from_secs(60));
    app.deck.looped = Some(Loop { start: 30_000.0, end: 90_000.0, on: true });
    let s = screen(&mut app, 120, 40);
    let line = s.lines().nth(app.areas.overview.y as usize - 1).unwrap();
    for said in [title.as_str(), artist.as_str(), "124.00", "8A", "-4:00", "LOOP", " A ", " D "] {
        assert!(line.contains(said), "{said}: {line}");
    }
    // Narrow, it keeps the tempo, key and time, and the cues give way.
    let s = screen(&mut app, 60, 20);
    let line = s.lines().nth(app.areas.overview.y as usize - 1).unwrap();
    assert!(line.contains("124.00") && line.contains("8A") && line.contains("-4:00"), "{line}");
    assert!(!line.contains(" D "), "{line}");
    // A track with neither title nor key.
    let t = &mut app.now.as_mut().unwrap().track;
    (t.title, t.artist, t.key) = (String::new(), String::new(), String::new());
    assert!(screen(&mut app, 120, 40).contains("Untitled"));
}

/// Nothing changes a track's speed, so the tempo has no pitch beside it, however wide the line.
#[test]
fn the_tempo_has_no_pitch_beside_it() {
    let (mut app, _dir) = playing(TrackAnalysis::default());
    let t = &mut app.now.as_mut().unwrap().track;
    (t.bpm, t.key) = (124.0, "8A".into());
    for (w, h) in [(60, 20), (120, 40), (200, 60)] {
        let s = screen(&mut app, w, h);
        let line = s.lines().nth(app.areas.overview.y as usize - 1).unwrap();
        assert!(line.contains("124.00  8A"), "{w}x{h}: {line}");
    }
}

#[test]
fn nothing_playing_says_what_would_carry_on() {
    let (mut app, _dir) = blank(View::Drives);
    let s = screen(&mut app, 120, 40);
    assert!(s.contains("Nothing playing") && s.contains("Pick a track and press enter."), "{s}");
    app.audio_error = Some("no device".into());
    assert!(screen(&mut app, 120, 40).contains("No audio output: no device"));
    app.audio_error = None;
    app.deck.pads = true;
    assert!(screen(&mut app, 120, 40).contains("' pads on"));
    // Short of room, it still says so, and the deck's states give way.
    (app.shuffle, app.deck.single, app.deck.auto_cue) = (true, true, true);
    let s = screen(&mut app, 55, 5);
    assert!(s.contains("Nothing playing") && !s.contains("auto cue"), "{s}");
    assert!(screen(&mut app, 120, 40).contains("auto cue"));
}
