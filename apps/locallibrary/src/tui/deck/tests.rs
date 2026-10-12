use std::time::Duration;

use locallibrary::anlz::{PhraseKind, TrackAnalysis};

use super::super::app::PhraseSpan;
use super::super::app::testing::*;
use super::*;

/// Where playback goes once the track is open.
fn target(app: &App) -> Option<u64> {
    app.pending_seek.map(|d| d.as_millis() as u64)
}

#[test]
fn pads_play_hot_cues_in_full_screen_or_when_turned_on() {
    let cues = vec![cue(Some(0), 1_000, None), cue(Some(3), 64_000, None), cue(None, 30_000, None)];
    let (mut app, _dir) = playing(TrackAnalysis { cues, ..Default::default() });
    // Off: digits switch views as ever.
    press(&mut app, "2");
    assert_eq!(app.view, View::Drives);
    press(&mut app, "1");
    assert_eq!((app.view, target(&app)), (View::Library, None));

    // Full screen: 4 is hot cue D.
    press(&mut app, "f");
    press(&mut app, "4");
    assert_eq!((app.view, target(&app)), (View::Library, Some(64_000)));
    press(&mut app, "1");
    assert_eq!(target(&app), Some(1_000));
    // A pad with no hot cue says so and stays put.
    app.toast = None;
    press(&mut app, "2");
    assert_eq!(target(&app), Some(1_000));
    assert_eq!(app.toast.as_ref().map(|t| t.0.as_str()), Some("no hot cue B"));

    // Out of full screen, ' turns the pads on and off.
    press(&mut app, "f");
    press(&mut app, "'");
    assert!(app.pads_on());
    press(&mut app, "4");
    assert_eq!((app.view, target(&app)), (View::Library, Some(64_000)));
    press(&mut app, "'");
    press(&mut app, "3");
    assert_eq!(app.view, View::Snapshots);
    assert!(!app.pads_on(), "only in the library");
}

#[test]
fn beats_and_times_on_a_steady_grid() {
    let g = grid(100, 120.0, 64);
    assert_eq!(beat_index(&[], 1000.0), None);
    assert_eq!(beat_index(&g, 100.0), Some(0.0));
    assert_eq!(beat_index(&g, 1225.0), Some(2.25));
    // Before the first beat and after the last, the grid carries on.
    assert_eq!(beat_index(&g, 0.0), Some(-0.2));
    assert_eq!(beat_index(&g, 100.0 + 70.0 * 500.0), Some(70.0));
    for at in [-1.5, 0.0, 2.25, 33.75, 63.0, 80.5] {
        let t = beat_time(&g, at).unwrap();
        assert!((beat_index(&g, t).unwrap() - at).abs() < 1e-9, "{at} -> {t}");
    }
}

#[test]
fn beat_jumps_keep_the_beat_or_land_on_one() {
    let g = grid(100, 120.0, 64);
    // Playing, quantized: a bar on, as far into the beat as before.
    assert_eq!(jump_by(&g, 1225.0, 4.0, Snap::Phase), Some(3225.0));
    assert_eq!(jump_by(&g, 1225.0, -1.0, Snap::Phase), Some(725.0));
    // Paused, quantized: onto the nearest beat, then along.
    assert_eq!(jump_by(&g, 1225.0, 1.0, Snap::Beat), Some(1600.0));
    assert_eq!(jump_by(&g, 1400.0, 1.0, Snap::Beat), Some(2100.0));
    // Not quantized: by beat lengths from wherever it is.
    assert_eq!(jump_by(&g, 1234.0, 2.0, Snap::Off), Some(2234.0));
    assert_eq!(jump_by(&[], 1234.0, 2.0, Snap::Off), None);

    // A grid that speeds up from 120 to 150 BPM at beat 8: quantized jumps count beats, so
    // a phrase back from the faster part lands in the same place in the beat.
    let mut g = grid(0, 120.0, 8);
    g.extend(grid(4000, 150.0, 56));
    let at = 4000.0 + 10.5 * 400.0;
    let back = jump_by(&g, at, -16.0, Snap::Phase).unwrap();
    assert_eq!(back, 1250.0, "beat 2.5 of the 120 BPM part");
    assert_eq!(jump_by(&g, back, 16.0, Snap::Phase), Some(at));
}

#[test]
fn quantized_cue_jumps_keep_the_beat_while_playing() {
    let g = grid(0, 120.0, 128);
    // A quarter beat past a beat: lands a quarter beat past the cue; just before a beat,
    // just before the cue.
    assert_eq!(cue_jump(&g, 10_125.0, 32_000.0, Snap::Phase), 32_125.0);
    assert_eq!(cue_jump(&g, 10_400.0, 32_000.0, Snap::Phase), 31_900.0);
    // Paused or not quantized: on the cue.
    for snap in [Snap::Beat, Snap::Off] {
        assert_eq!(cue_jump(&g, 10_125.0, 32_000.0, snap), 32_000.0);
    }
    assert_eq!(cue_jump(&[], 10_125.0, 32_000.0, Snap::Phase), 32_000.0);
}

#[test]
fn beat_jump_keys() {
    let (mut app, _dir) = playing(TrackAnalysis { beats: grid(100, 120.0, 600), ..Default::default() });
    // Paused (the audio's still opening), quantized: along the grid from the beat nearest 0.
    for (key, to) in [('.', 600), ('>', 2100), ('}', 16_100)] {
        app.pending_seek = None;
        press(&mut app, &key.to_string());
        assert_eq!(target(&app), Some(to), "{key}");
    }
    press(&mut app, "Q");
    assert!(!app.deck.quantize && app.snap() == Snap::Off);
    app.pending_seek = None;
    press(&mut app, ">");
    assert_eq!(target(&app), Some(2000), "4 beats of 500 ms from 0");
    // Back past the start stops at it.
    press(&mut app, "{");
    assert_eq!(target(&app), Some(0));
    // No grid: the track's BPM (121) says how long a beat is.
    app.now.as_mut().unwrap().analysis.beats.clear();
    press(&mut app, ">");
    assert_eq!(target(&app), Some(1983));
}

#[test]
fn phrase_and_cue_jumps_land_on_the_next_or_previous_mark() {
    let g = grid(0, 120.0, 128);
    let marks = [0.0, 16_000.0, 32_000.0, 48_000.0];
    let at = |ms: f64, forward: bool, snap: Snap| mark_jump(&g, ms, &marks, forward, snap);
    assert_eq!(at(1_000.0, true, Snap::Beat), Some((16_000.0, 16_000.0)));
    // One just ahead is passed over; playing quantized, the jump keeps the beat.
    assert_eq!(at(15_900.0, true, Snap::Beat), Some((32_000.0, 32_000.0)));
    assert_eq!(at(10_125.0, true, Snap::Phase), Some((16_000.0, 16_125.0)));
    assert_eq!(at(50_000.0, true, Snap::Off), None, "none ahead");
    // Back: past one just passed, and to the start before the first.
    assert_eq!(at(20_000.0, false, Snap::Off), Some((16_000.0, 16_000.0)));
    assert_eq!(at(17_000.0, false, Snap::Off), Some((0.0, 0.0)));
    assert_eq!(mark_jump(&g, 5_000.0, &[8_000.0], false, Snap::Off), Some((0.0, 0.0)));
}

#[test]
fn phrase_jump_keys() {
    use locallibrary::anlz::PhraseKind;

    use super::super::app::PhraseSpan;
    let (mut app, _dir) = playing(TrackAnalysis { beats: grid(0, 120.0, 600), ..Default::default() });
    press(&mut app, "N");
    assert_eq!(
        (target(&app), app.toast.as_ref().map(|t| t.0.as_str())),
        (None, Some("no phrase analysis for this track"))
    );
    let span = |kind, start_ms, end_ms| PhraseSpan { kind, start_ms, end_ms, bars: 8 };
    app.now.as_mut().unwrap().phrases =
        vec![span(PhraseKind::Intro, 0.0, 16_000.0), span(PhraseKind::Chorus, 16_000.0, 32_000.0)];
    // (The audio's still opening: jumps go from the start.)
    press(&mut app, "N");
    assert_eq!(target(&app), Some(16_000));
    press(&mut app, "P");
    assert_eq!(target(&app), Some(0));
}

#[test]
fn auto_cue_finds_the_first_memory_cue_or_beat() {
    let beats = grid(320, 120.0, 8);
    let cues = vec![cue(Some(0), 100, None), cue(None, 2_000, None), cue(None, 900, Some(1_900))];
    let at = |beats: &[Beat], cues: &[Cue]| {
        auto_cue(&TrackAnalysis { beats: beats.to_vec(), cues: cues.to_vec(), ..Default::default() })
    };
    assert_eq!(at(&beats, &cues), Some(Duration::from_millis(900)), "hot cues don't count");
    assert_eq!(at(&beats, &cues[..1]), Some(Duration::from_millis(320)));
    assert_eq!(at(&grid(0, 120.0, 8), &[]), None, "the first beat is the start");
    assert_eq!(at(&[], &[]), None);
}

#[test]
fn loops_from_the_grid() {
    let g = grid(100, 120.0, 64);
    // From the beat the playhead is in, or from the playhead.
    assert_eq!(auto_loop(&g, 1225.0, 4.0, true), Some((1100.0, 3100.0)));
    assert_eq!(auto_loop(&g, 1225.0, 4.0, false), Some((1225.0, 3225.0)));
    assert_eq!(auto_loop(&[], 1225.0, 4.0, true), None);
    // The beat before the first starts before the track: the first one instead.
    assert_eq!(auto_loop(&g, 0.0, 4.0, true), Some((100.0, 2100.0)));
    assert_eq!((beats_label(4.0), beats_label(0.5), beats_label(0.25)), ("4".into(), "1/2".into(), "1/4".into()));
}

#[test]
fn loop_keys() {
    let cues = vec![cue(Some(1), 8_000, Some(10_000)), cue(None, 20_000, Some(22_000))];
    let (mut app, _dir) = playing(TrackAnalysis { beats: grid(0, 120.0, 600), cues, ..Default::default() });
    let looped = |app: &App| app.deck.looped.map(|l| (l.start as u32, l.end as u32, l.on));
    // Nothing saved here: 4 beats from the beat the playhead's in.
    press(&mut app, "l");
    assert_eq!(looped(&app), Some((0, 2000, true)));
    assert_eq!(app.loop_playing().unwrap().1, Some(4.0));
    // Halved, doubled twice: along the grid.
    press(&mut app, "(");
    assert_eq!(looped(&app), Some((0, 1000, true)));
    press(&mut app, ")");
    press(&mut app, ")");
    assert_eq!(looped(&app), Some((0, 4000, true)));
    assert!(app.drawn_cues().iter().any(|c| c.end_ms == Some(4000) && c.comment == "loop ↻8"));
    let s = super::super::ui::screen(&mut app, 120, 50);
    assert!(s.contains("LOOP 8"), "{s}");
    // Exited: playback carries on; l again (still in it) loops it again.
    press(&mut app, "l");
    assert_eq!(looped(&app), Some((0, 4000, false)));
    assert!(app.loop_playing().is_none() && app.drawn_cues().len() == 2);
    press(&mut app, "l");
    assert_eq!(looped(&app), Some((0, 4000, true)));
    // A hot cue saved as a loop plays it; seeking out of it leaves it.
    app.deck.pads = true;
    press(&mut app, "2");
    assert_eq!(looped(&app), Some((8_000, 10_000, true)));
    assert_eq!(target(&app), Some(8_000));
    assert_eq!(app.drawn_cues().len(), 2, "a saved loop is drawn already");
    app.seek_to(Duration::from_secs(60));
    assert_eq!(looped(&app), Some((8_000, 10_000, false)));
}

/// A loop made in the track's last beats ends with it (else it would never come round), and
/// a seek in a track whose length isn't known goes where it's asked.
#[test]
fn made_loops_and_seeks_keep_to_the_track() {
    let (mut app, _dir) = playing(TrackAnalysis { beats: grid(0, 120.0, 600), ..Default::default() });
    app.now.as_mut().unwrap().duration = Duration::from_millis(1500);
    press(&mut app, "l");
    assert_eq!(app.deck.looped.map(|l| (l.start, l.end)), Some((0.0, 1500.0)));
    app.now.as_mut().unwrap().duration = Duration::ZERO;
    app.seek_to(Duration::from_secs(30));
    assert_eq!(target(&app), Some(30_000));
}

#[test]
fn outputs_list_and_close() {
    let (mut app, _dir) = playing(TrackAnalysis::default());
    press(&mut app, "d");
    let o = app.deck.outputs.as_ref().expect("listed");
    assert_eq!(o.state.selected(), Some(0), "the default, with nothing picked");
    let n = o.names.len();
    let s = super::super::ui::screen(&mut app, 100, 30);
    assert!(s.contains("Output") && s.contains("the system's default"), "{s}");
    // Keys move through the list and nothing else.
    for _ in 0..n + 2 {
        key(&mut app, KeyCode::Down);
    }
    assert_eq!(app.deck.outputs.as_ref().unwrap().state.selected(), Some(n));
    press(&mut app, "2");
    assert!(app.deck.outputs.is_none() && app.view == View::Library, "closed, not switched");
    // Picking the default: plays through it (with a device), or says there isn't one.
    press(&mut app, "d");
    key(&mut app, KeyCode::Enter);
    let said = app.toast.as_ref().map(|t| t.0.clone()).unwrap_or_default();
    match &app.audio {
        Some(_) => assert_eq!(said, "playing through the default output"),
        None => assert!(said.contains("no audio output"), "{said}"),
    }
    assert_eq!(app.deck.output, None);
}

/// Full screen, each hot cue has its key before it, with colours or without.
#[test]
fn full_screen_shows_which_key_plays_which_pad() {
    use super::super::theme::{self, Mode};
    use super::super::ui::screen;
    let cues = vec![cue(Some(0), 1_000, None), cue(Some(3), 64_000, None)];
    let (mut app, _dir) = playing(TrackAnalysis { cues, ..Default::default() });
    app.now.as_mut().unwrap().opening = false;
    let s = screen(&mut app, 120, 50);
    assert!(s.contains(" A  D ") && !s.contains("1 A 4 D"), "{s}");
    app.deck.pads = true;
    let s = screen(&mut app, 120, 50);
    assert!(s.contains("1 A 4 D") && s.contains("' pads on"), "{s}");
    app.deck.pads = false;
    app.fullscreen = true;
    for mode in [None, Some(Mode::Plain)] {
        theme::set_test_mode(mode);
        let s = screen(&mut app, 160, 45);
        theme::set_test_mode(None);
        assert!(s.contains("1 A 4 D") && s.contains("1-8 hot cue"), "{s}");
    }
}

/// With no beat grid and no tempo there's nothing to jump or loop by, which is said (while
/// the track's analysis is still being read, that it is); without a loop, there's nothing
/// to resize.
#[test]
fn says_why_a_jump_or_loop_cant_be_made() {
    let (mut app, _dir) = playing(TrackAnalysis::default());
    app.now.as_mut().unwrap().track.bpm = 0.0;
    press(&mut app, ".");
    assert_eq!(last_message(&app), "no beat grid or tempo to jump by");
    press(&mut app, "l");
    assert_eq!(last_message(&app), "no beat grid or tempo to loop by");
    press(&mut app, "(");
    assert_eq!(last_message(&app), "no loop: l makes one");
    app.now.as_mut().unwrap().loading = true;
    press(&mut app, ".");
    assert_eq!(last_message(&app), "reading the beat grid…");
    press(&mut app, "l");
    assert_eq!(last_message(&app), "reading the beat grid…");
    press(&mut app, "N");
    assert_eq!(last_message(&app), "reading the phrases…");
    press(&mut app, "'1");
    assert_eq!(last_message(&app), "reading the cues…");
    assert!(app.deck.looped.is_none() && target(&app).is_none());

    // Past the last phrase there's nowhere to go.
    app.now.as_mut().unwrap().phrases =
        vec![PhraseSpan { kind: PhraseKind::Intro, start_ms: 0.0, end_ms: 16_000.0, bars: 8 }];
    press(&mut app, "N");
    assert_eq!(last_message(&app), "no more phrases");
}

/// `A` turns auto cue on and off, and says what it does.
#[test]
fn auto_cue_is_turned_on_and_off() {
    let (mut app, _dir) = playing(TrackAnalysis::default());
    press(&mut app, "A");
    assert!(app.deck.auto_cue);
    assert!(last_message(&app).starts_with("auto cue on"), "{}", last_message(&app));
    press(&mut app, "A");
    assert!(!app.deck.auto_cue);
    assert!(last_message(&app).starts_with("auto cue off"), "{}", last_message(&app));
}

/// An output that won't open is said, and playback stays where it was.
#[test]
fn an_output_that_wont_open_is_said() {
    let (mut app, _dir) = playing(TrackAnalysis::default());
    app.outputs = super::super::audio::Outputs {
        list: || (vec!["Interface".into()], None),
        open: |_| anyhow::bail!("the interface is gone"),
    };
    // The default and the interface: the list stops at either end.
    press(&mut app, "djjk");
    assert_eq!(app.deck.outputs.as_ref().and_then(|o| o.state.selected()), Some(0));
    press(&mut app, "j\n");
    assert_eq!(last_message(&app), "the interface is gone");
    assert!(app.deck.output.is_none() && app.now.is_some());
}

/// A hot cue pressed while paused plays from there, as a CDJ's pad does; pressed while
/// playing, it jumps back there and plays on.
#[test]
fn a_hot_cue_plays_from_its_place() {
    let (_dir, mut app, mut out) = playing_wavs(&[8000]);
    opened(&mut app);
    app.now.as_mut().unwrap().analysis.cues = vec![cue(Some(0), 200, None)];
    app.deck.pads = true;
    press(&mut app, " ");
    assert!(!app.is_playing());
    press(&mut app, "1");
    assert!(app.is_playing());
    assert_eq!(app.position(), Duration::from_millis(200));
    (&mut out).take(800).for_each(drop);
    assert_eq!(app.position(), Duration::from_millis(300));
    press(&mut app, "1");
    assert!(app.is_playing());
    assert_eq!(app.position(), Duration::from_millis(200));
}

/// Without a beat grid a loop is made, and halved, by the track's tempo.
#[test]
fn loops_by_the_tempo_without_a_grid() {
    let (mut app, _dir) = playing(TrackAnalysis::default());
    app.now.as_mut().unwrap().track.bpm = 120.0;
    press(&mut app, "l");
    assert_eq!(app.deck.looped.map(|l| (l.start, l.end)), Some((0.0, 2000.0)));
    press(&mut app, "(");
    assert_eq!(app.deck.looped.map(|l| (l.start, l.end)), Some((0.0, 1000.0)));
}

/// A beat grid as rekordbox's can be: from any time, its tempo changing beat to beat.
#[hegel::test_helper]
fn any_grid(tc: &hegel::TestCase) -> Vec<locallibrary::anlz::Beat> {
    use hegel::generators as gs;
    let mut at = tc.draw(gs::integers::<u32>().max_value(5_000));
    let n = tc.draw(gs::integers::<usize>().min_value(1).max_value(40));
    (0..n)
        .map(|i| {
            let bpm = tc.draw(gs::integers::<u16>().min_value(60).max_value(200)) as f64;
            let b = locallibrary::anlz::Beat { beat_number: (i % 4 + 1) as u16, bpm, time_ms: at };
            at += tc.draw(gs::integers::<u32>().min_value(100).max_value(1_500));
            b
        })
        .collect()
}

#[hegel::test_helper]
fn any_ms(tc: &hegel::TestCase, beats: &[locallibrary::anlz::Beat]) -> f64 {
    use hegel::generators as gs;
    let end = beats.last().unwrap().time_ms as i64 + 5_000;
    tc.draw(gs::integers::<i64>().min_value(-5_000).max_value(end)) as f64
}

/// A time's place on any grid (before it, along it, past it) and that place's time agree, and
/// later times are further along.
#[hegel::test(test_cases = 300)]
fn places_on_the_grid_and_their_times_agree(tc: hegel::TestCase) {
    let g = any_grid(&tc);
    let (a, b) = (any_ms(&tc, &g), any_ms(&tc, &g));
    let (ia, ib) = (beat_index(&g, a).unwrap(), beat_index(&g, b).unwrap());
    assert!((beat_time(&g, ia).unwrap() - a).abs() < 1e-6, "{a} is beat {ia}");
    assert!(a > b || ia <= ib, "{a} is beat {ia}, {b} is beat {ib}");
    for (i, beat) in g.iter().enumerate() {
        assert!((beat_index(&g, beat.time_ms as f64).unwrap() - i as f64).abs() < 1e-9, "beat {i}");
    }
}

/// A beat jump moves the playhead along the grid by the beats asked: keeping its place in the
/// beat while playing, onto a beat when paused, and by the beat it's in with quantize off.
#[hegel::test(test_cases = 300)]
fn beat_jumps_move_by_the_beats_asked(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let g = any_grid(&tc);
    let ms = any_ms(&tc, &g);
    let by = tc.draw(gs::sampled_from(&[-32.0, -4.0, -1.0, -0.5, 0.5, 1.0, 4.0, 32.0][..]));
    let at = beat_index(&g, ms).unwrap();
    let landed = |snap| beat_index(&g, jump_by(&g, ms, by, snap).unwrap()).unwrap();
    assert!((landed(Snap::Phase) - (at + by)).abs() < 1e-6, "from beat {at} by {by}");
    assert!((landed(Snap::Beat) - (at.round() + by)).abs() < 1e-6, "from beat {at} by {by} onto a beat");
    let i = at.floor().clamp(0.0, (g.len() - 1) as f64) as usize;
    assert!((jump_by(&g, ms, by, Snap::Off).unwrap() - (ms + by * beat_len(&g, i))).abs() < 1e-6);
    assert_eq!(jump_by(&[], ms, by, Snap::Phase), None);
}

/// A quantized cue jump while playing lands as far from the cue (within half a beat) as the
/// playhead is from its nearest beat; otherwise on the cue. Marks ahead are the first to land
/// past the playhead; back, the last to land well before it, or the start.
#[hegel::test(test_cases = 300)]
fn cue_and_mark_jumps_land_where_they_say(tc: hegel::TestCase) {
    use hegel::Generator;
    use hegel::generators as gs;
    let g = any_grid(&tc);
    let (ms, cue) = (any_ms(&tc, &g), any_ms(&tc, &g).max(0.0));
    let (at, c) = (beat_index(&g, ms).unwrap(), beat_index(&g, cue).unwrap());
    let land = cue_jump(&g, ms, cue, Snap::Phase);
    let off = beat_index(&g, land).unwrap() - c;
    assert!((off - (at - at.round())).abs() < 1e-6 && off.abs() <= 0.5 + 1e-9, "{off} from the cue");
    assert_eq!((cue_jump(&g, ms, cue, Snap::Beat), cue_jump(&g, ms, cue, Snap::Off)), (cue, cue));

    let mut marks: Vec<f64> = (0..tc.draw(gs::integers::<usize>().max_value(6))).map(|_| any_ms(&tc, &g)).collect();
    marks.retain(|m| *m >= 0.0);
    marks.sort_by(f64::total_cmp);
    let snap = tc.draw(gs::sampled_from(&[Snap::Off, Snap::Beat, Snap::Phase][..]).print_as_debug());
    let lands = |m: f64| cue_jump(&g, ms, m, snap);
    match mark_jump(&g, ms, &marks, true, snap) {
        Some((m, l)) => {
            assert_eq!(l, lands(m));
            assert!(l > ms + 250.0, "{m} lands at {l}, not past {ms}");
            let first = marks.iter().position(|x| *x == m).unwrap();
            assert!(marks[..first].iter().all(|x| lands(*x) <= ms + 250.0), "an earlier mark than {m}");
        }
        None => assert!(marks.iter().all(|x| lands(*x) <= ms + 250.0), "a mark ahead of {ms}"),
    }
    let (m, l) = mark_jump(&g, ms, &marks, false, snap).unwrap();
    let before: Vec<&f64> = marks.iter().filter(|x| lands(**x) + 1500.0 < ms).collect();
    assert_eq!(m, before.last().map_or(0.0, |x| **x), "back from {ms}");
    assert_eq!(l, lands(m));
}

/// A loop made on the grid is as many beats long as asked, from the beat the playhead is in
/// (or the playhead itself with quantize off), never from before the track starts.
#[hegel::test(test_cases = 300)]
fn loops_made_on_the_grid_span_the_beats_asked(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let g = any_grid(&tc);
    let ms = any_ms(&tc, &g).max(0.0);
    let len = tc.draw(gs::sampled_from(&[0.25, 1.0, 4.0, 16.0][..]));
    let on_beat = tc.draw(gs::booleans());
    let (start, end) = auto_loop(&g, ms, len, on_beat).unwrap();
    let (s, e) = (beat_index(&g, start).unwrap(), beat_index(&g, end).unwrap());
    assert!((e - s - len).abs() < 1e-6, "{s}..{e} for {len} beats");
    let at = beat_index(&g, ms).unwrap();
    if on_beat {
        assert!((s - s.round()).abs() < 1e-6 && (s - at).abs() < 1.0 + 1e-6, "beat {s} for the playhead at {at}");
        assert!(start >= 0.0, "a loop from {start}");
    } else {
        assert!((start - ms).abs() < 1e-6);
    }
    assert_eq!(auto_loop(&[], ms, len, on_beat), None);
}

/// The deck's keys on any track, its analysis read or still being read: a beat jump moves by the
/// grid, or the track's tempo without one, or says there's neither (or that the grid is being
/// read); a phrase jump goes to the next or previous phrase, or says there's none (more); l makes
/// a loop from the grid or tempo and l again leaves it; ( and ) halve and double it within its
/// bounds, or say there's no loop; and the outputs listed take the arrows and enter, playing on
/// through the one picked.
#[hegel::test(test_cases = 200)]
fn the_decks_keys_do_what_they_say_or_say_why_not(tc: hegel::TestCase) {
    use super::super::keymap::Act;
    use hegel::generators as gs;
    let beats = if tc.draw(gs::booleans()) { grid(0, 120.0, 400) } else { Vec::new() };
    let (mut app, _dir) = playing(TrackAnalysis { beats, ..Default::default() });
    app.outputs = super::super::audio::tests::OUTPUTS;
    let now = app.now.as_mut().unwrap();
    now.track.bpm = tc.draw(gs::sampled_from(&[0.0, 120.0][..]));
    now.loading = tc.draw(gs::booleans());
    if tc.draw(gs::booleans()) {
        now.phrases = vec![
            PhraseSpan { kind: PhraseKind::Intro, start_ms: 0.0, end_ms: 16_000.0, bars: 8 },
            PhraseSpan { kind: PhraseKind::Up, start_ms: 16_000.0, end_ms: 32_000.0, bars: 8 },
        ];
    }
    app.deck.quantize = tc.draw(gs::booleans());
    let quantized = app.deck.quantize;
    for _ in 0..tc.draw(gs::integers::<usize>().max_value(8)) {
        let at = app.position_ms();
        // (Played on through another output, it's opened afresh.)
        let now = app.now.as_ref().unwrap();
        let (gridded, tempo, loading) = (!now.analysis.beats.is_empty(), now.track.bpm > 0.0, now.loading);
        let phrased = !now.phrases.is_empty();
        match tc.draw(gs::integers::<u8>().max_value(5)) {
            0 => {
                let by = tc.draw(gs::sampled_from(&[-4i8, -1, 1, 4][..]));
                app.perform(Act::BeatJump(by), None);
                let to = target(&app).map(|t| t as f64).unwrap_or(app.position_ms());
                match (gridded, tempo) {
                    (false, false) => {
                        let why = if loading { "reading the beat grid…" } else { "no beat grid or tempo to jump by" };
                        assert_eq!(last_message(&app), why);
                    }
                    // 500 ms a beat, either way; on a beat once quantized.
                    _ => {
                        let want = (at + by as f64 * 500.0).max(0.0);
                        let off = if quantized && gridded { 500.0 } else { 1.0 };
                        assert!((to - want).abs() < off, "{by} beats from {at}: {to}");
                    }
                }
            }
            1 => {
                let forward = tc.draw(gs::booleans());
                app.perform(Act::Phrase(forward), None);
                let to = target(&app).map(|t| t as f64).unwrap_or(app.position_ms());
                if !phrased {
                    let why = if loading { "reading the phrases…" } else { "no phrase analysis for this track" };
                    assert_eq!(last_message(&app), why);
                } else if last_message(&app) != "no more phrases" {
                    assert!([0.0, 16_000.0].iter().any(|s| (to - s).abs() <= 500.0), "to a phrase: {to}");
                    assert!(if forward { to >= at - 500.0 } else { to <= at + 500.0 }, "{forward}: {at} to {to}");
                }
            }
            2 => {
                let was = app.deck.looped.filter(|l| l.on);
                app.perform(Act::Loop, None);
                match (was, gridded || tempo) {
                    (Some(l), _) => assert_eq!(app.deck.looped, Some(Loop { on: false, ..l }), "l leaves the loop"),
                    (None, true) => {
                        let l = app.deck.looped.expect("a loop");
                        assert!(l.on && l.end > l.start, "{l:?}");
                    }
                    (None, false) if app.deck.looped.is_none() => {
                        let why = if loading { "reading the beat grid…" } else { "no beat grid or tempo to loop by" };
                        assert_eq!(last_message(&app), why);
                    }
                    _ => {}
                }
            }
            3 => {
                let double = tc.draw(gs::booleans());
                let was = app.deck.looped;
                app.perform(Act::LoopSize(double), None);
                match was {
                    None => assert_eq!(last_message(&app), "no loop: l makes one"),
                    Some(l) => {
                        let n = app.deck.looped.unwrap();
                        let (old, new) = (l.end - l.start, n.end - n.start);
                        assert_eq!(n.start, l.start, "from where it starts");
                        assert!((15.0..=300_000.0).contains(&new) && n.end <= 300_000.0, "{n:?}");
                        assert!(if double { new >= old - 1e-6 } else { new <= old + 1e-6 }, "{old} to {new}");
                    }
                }
            }
            4 => {
                // Listed with the one playing through selected (the default first).
                let playing = app.audio.as_ref().and_then(|a| a.device()).map(str::to_string);
                app.perform(Act::Outputs, None);
                let o = app.deck.outputs.as_ref().expect("listed");
                let n = o.names.len();
                let from = playing.and_then(|p| o.names.iter().position(|x| *x == p)).map_or(0, |i| i + 1);
                assert_eq!(o.state.selected(), Some(from));
                let downs = tc.draw(gs::integers::<usize>().max_value(n + 1));
                for _ in 0..downs {
                    key(&mut app, KeyCode::Down);
                }
                let mut picked = (from + downs).min(n);
                assert_eq!(app.deck.outputs.as_ref().unwrap().state.selected(), Some(picked));
                if tc.draw(gs::booleans()) {
                    key(&mut app, KeyCode::Up);
                    key(&mut app, KeyCode::Down);
                    picked = (picked.saturating_sub(1) + 1).min(n);
                }
                key(&mut app, KeyCode::Enter);
                let name = picked.checked_sub(1).map(|i| ["Phones"][i]);
                assert!(app.deck.outputs.is_none() && app.audio.is_some());
                assert_eq!(app.deck.output.as_deref(), name);
                assert_eq!(last_message(&app), format!("playing through {}", name.unwrap_or("the default output")));
            }
            _ => {
                app.perform(Act::Outputs, None);
                press(&mut app, "x");
                assert!(app.deck.outputs.is_none(), "any other key closes the list");
            }
        }
    }
}

/// Auto cue starts a track at its first memory cue, else its first beat, else (at the very start)
/// the beginning; a hot cue pressed while paused jumps there and plays; a phrase jump past the
/// last phrase says there's none; an output that won't open says why, and nothing changes.
#[hegel::test(test_cases = 100)]
fn cues_phrases_and_outputs_on_the_deck(tc: hegel::TestCase) {
    use super::super::keymap::Act;
    use hegel::generators as gs;
    let memory: Vec<u32> = tc.draw(gs::vecs(gs::integers::<u32>().max_value(60_000)).max_size(3));
    let first_beat = tc.draw(gs::optional(gs::integers::<u32>().max_value(2_000)));
    let mut cues: Vec<_> = memory.iter().map(|&ms| cue(None, ms, None)).collect();
    cues.push(cue(Some(0), 100, None));
    let a = TrackAnalysis { beats: first_beat.map_or(Vec::new(), |b| grid(b, 120.0, 8)), cues, ..Default::default() };
    let want =
        memory.iter().min().copied().or(first_beat).filter(|&ms| ms > 0).map(|ms| Duration::from_millis(ms as u64));
    assert_eq!(auto_cue(&a), want);

    let (_dir, mut app, _out) = playing_wavs(&[8000, 8000]);
    opened(&mut app);
    app.now.as_mut().unwrap().analysis.cues = vec![cue(Some(0), 200, None)];
    app.audio.as_ref().unwrap().toggle_pause();
    assert!(!app.is_playing());
    hot_cue(&mut app, 0);
    assert!(app.is_playing(), "a hot cue plays");
    assert_eq!(app.position(), Duration::from_millis(200));

    let starts = [0.0, 100.0];
    app.now.as_mut().unwrap().phrases = starts
        .iter()
        .map(|&s| PhraseSpan { kind: PhraseKind::Intro, start_ms: s, end_ms: s + 100.0, bars: 1 })
        .collect();
    app.deck.quantize = false;
    app.seek_to(Duration::from_millis(tc.draw(gs::integers::<u64>().min_value(150).max_value(400))));
    app.perform(Act::Phrase(true), None);
    assert_eq!(last_message(&app), "no more phrases");

    app.outputs = super::super::audio::Outputs {
        list: || (vec!["Gone".into()], None),
        open: |_| anyhow::bail!("the output is gone"),
    };
    let before = app.audio.as_ref().map(|a| a.device().map(str::to_string));
    app.perform(Act::Outputs, None);
    key(&mut app, KeyCode::Down);
    key(&mut app, KeyCode::Enter);
    assert_eq!(last_message(&app), "the output is gone");
    assert_eq!(
        app.audio.as_ref().map(|a| a.device().map(str::to_string)),
        before,
        "still playing through the one it was"
    );
}
