use std::time::Duration;

use hegel::generators as gs;
use locallibrary::Track;
use locallibrary::anlz::{Bands, TrackAnalysis, TrackWaves};
use loclib_waveform::Wave;
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::style::{Color, Modifier};

use crate::tui::app::testing::*;
use crate::tui::app::{App, Focus, PhraseSpan, View};
use crate::tui::deck::{BandMode, Loop};
use crate::tui::keys::{Camelot, KeyPalette, Notation, Relation};
use crate::tui::theme::{self, Tier};
use crate::tui::ui::{draw, screen};

use super::*;

/// The frame round the tempo, which only the deck has.
const FRAME: &str = "╭ BPM";

fn drawn(app: &mut App, w: u16, h: u16) -> Buffer {
    let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
    terminal.draw(|f| draw(f, app)).unwrap();
    terminal.backend().buffer().clone()
}

/// A five-minute track at 120 with cues, in the key of 8A, a good way in.
fn on_deck() -> (App, tempfile::TempDir) {
    let analysis = TrackAnalysis {
        beats: grid(0, 120.0, 600),
        cues: vec![cue(Some(0), 32_000, None), cue(None, 40_000, None)],
        preview: Some(wave(400)),
        detail: Some(wave(150 * 300)),
        ..Default::default()
    };
    let (mut app, dir) = playing(analysis);
    let now = app.now.as_mut().unwrap();
    now.track.key = "8A".into();
    now.track.bpm = 120.0;
    now.key = Camelot::parse("8A");
    app.resume = Some(Duration::from_secs(20));
    (app, dir)
}

/// `f` gives the deck the screen, in place of the panes and the strip, and takes it back. It stays
/// through a visit to another view, and comes back with the next run.
#[test]
fn one_key_switches_the_layouts() {
    let (mut app, _dir) = on_deck();
    let library = screen(&mut app, 120, 40);
    assert!(library.contains("All tracks") && library.contains("1 Drives"), "{library}");
    press(&mut app, "f");
    let deck = screen(&mut app, 120, 40);
    assert!(deck.contains(FRAME) && !deck.contains("1 Drives"), "{deck}");
    assert_eq!(deck.matches(big_row("-4:40", 1).trim_end()).count(), 1, "large, in the frame\n{deck}");
    assert!(!deck.contains("-4:40"), "and nowhere else\n{deck}");
    assert!(app.saved_state().deck_layout);

    press(&mut app, "gd");
    assert!(!screen(&mut app, 120, 40).contains(FRAME), "only the Library view has it");
    assert!(!app.pads_on(), "the digits are the panes' again");
    press(&mut app, "gl");
    assert!(screen(&mut app, 120, 40).contains(FRAME));

    let saved = app.saved_state();
    press(&mut app, "f");
    assert!(screen(&mut app, 120, 40).contains("1 Drives"));
    app.apply_saved(&saved, false);
    assert!(app.deck_showing(), "back from what's saved");
}

/// Browse opens over the deck, with the panes and the strip. A track loaded there, or esc, is back
/// at the deck, and the digits are hot cues again.
#[test]
fn browse_opens_over_the_deck_and_loading_closes_it() {
    let (mut app, _dir) = on_deck();
    press(&mut app, "fb");
    assert!(app.browsing && !app.deck_showing());
    let s = screen(&mut app, 120, 40);
    assert!(s.contains("1 Drives") && s.contains(" BROWSE ") && !s.contains(FRAME), "{s}");
    assert_ne!(app.areas.overview.height, 0, "the strip's under it");
    assert!(!app.pads_on(), "digits go to panes");
    press(&mut app, "\u{1b}");
    assert!(app.deck_showing(), "esc: back at the deck");
    assert!(app.pads_on());

    press(&mut app, "b");
    app.focus = Focus::Tracks;
    app.track_state.select(Some(0));
    press(&mut app, "\n");
    assert!(app.deck_showing(), "a track loaded: back at the deck");
    press(&mut app, "\u{1b}");
    assert!(!app.deck_layout, "esc again: the Library layout");
}

/// The footer says what the deck's keys are, and `b` and `w` mean those things only on the deck.
#[test]
fn the_decks_keys_are_its_own() {
    let (mut app, _dir) = on_deck();
    let sort = app.sort;
    press(&mut app, "f");
    assert!(screen(&mut app, 160, 45).contains("b browse"));
    press(&mut app, "w");
    assert_eq!((app.deck.bands, app.sort), (Some(BandMode::Blue), sort));
    press(&mut app, "b");
    assert!(app.browsing);
    press(&mut app, "b");
    assert_ne!(app.sort, sort, "in Browse, b sorts");
}

/// The tempo's in large digits, three rows high, at every size, and no pitch is claimed: nothing
/// changes a track's speed yet.
#[test]
fn the_tempo_is_large() {
    let (mut app, _dir) = on_deck();
    app.deck_layout = true;
    for (w, h) in [(60, 20), (80, 24), (160, 45)] {
        let s = screen(&mut app, w, h);
        for row in big("120.00", Style::new()) {
            let text: String = row.spans.iter().map(|s| s.content.as_ref()).collect();
            assert!(s.contains(&text), "{text}\n{s}");
        }
        assert!(!s.contains("+0.0%"), "{s}");
    }
    let now = app.now.as_mut().unwrap();
    now.track.bpm = 0.0;
    now.analysis.beats.clear();
    let s = screen(&mut app, 80, 24);
    let row: String = big("  -.--", Style::new())[1].spans.iter().map(|s| s.content.as_ref()).collect();
    assert!(s.contains(row.trim()), "no tempo to show: {s}");
}

/// The zoomed view's cue rows go when a track has no cues, and the waveform has them.
#[test]
fn cues_rows_only_show_for_cues() {
    assert_eq!(Rows::of(21, 78, true).zoom_cues, 1);
    assert_eq!(Rows::of(21, 78, false).zoom_cues, 0);
    assert_eq!(Rows::of(41, 158, true).zoom_cues, 2);
}

/// At every height the rows fit, and a mirrored waveform has an even number of them.
#[hegel::test(test_cases = 60)]
fn prop_the_rows_add_up(tc: hegel::TestCase) {
    let height = tc.draw(gs::integers::<u16>().min_value(17).max_value(120));
    let width = tc.draw(gs::integers::<u16>().min_value(58).max_value(240));
    let has_cues = tc.draw(gs::booleans());
    let r = Rows::of(height, width, has_cues);
    assert_eq!(r.head + r.zoom + r.overview + r.panels + r.readout, height, "{height}");
    assert_eq!((r.zoom - 1 - r.zoom_cues) % 2, 0, "{height}");
    assert!(r.zoom >= 4, "room for a waveform at {height}");
}

/// The zoomed view has every row going until it's as tall as it gets, and only then the overview
/// grows, by what's over.
#[hegel::test(test_cases = 60)]
fn prop_height_goes_to_the_zoomed_view_first(tc: hegel::TestCase) {
    let height = tc.draw(gs::integers::<u16>().min_value(17).max_value(120));
    let r = Rows::of(height, 158, true);
    let overview_wave = r.overview - r.overview_cues - 1;
    let filled = r.zoom + 1 >= ZOOM_MOST;
    assert!(filled || overview_wave <= 6, "{height}: {} rows of zoom, {overview_wave} of overview", r.zoom);
}

/// The rows of the tempo's large digits, as shown.
fn big_row(text: &str, row: usize) -> String {
    big(text, Style::new())[row].spans.iter().map(|s| s.content.as_ref()).collect()
}

const SIZES: [(u16, u16); 6] = [(60, 20), (80, 24), (100, 30), (120, 40), (160, 45), (200, 60)];

/// The decimal point is a dot on the digits' baseline, and doesn't hang below it as a comma would.
#[test]
fn the_decimal_point_is_a_dot() {
    assert_eq!([big_row(".", 0), big_row(".", 1), big_row(".", 2)].map(|r| r.trim().to_string()), ["", "", "▄"]);
}

/// With ASCII the numbers are seven segments, still three rows high and readable as digits.
#[test]
fn the_tempo_is_large_in_ascii() {
    let (mut app, _dir) = on_deck();
    app.deck_layout = true;
    icons::set_test_set(Some(icons::IconSet::Ascii));
    assert_eq!(big_row("120.00", 2).trim_end(), "  | |_  |_| . |_| |_|");
    let (s, row) = (screen(&mut app, 80, 24), big_row("120.00", 1));
    icons::set_test_set(None);
    assert!(s.is_ascii() && s.contains(row.trim_end()), "{s}");
}

/// The big number says it's the tempo at every size, on its frame, beside it or under the key.
#[test]
fn the_tempo_is_named_at_every_size() {
    let (mut app, _dir) = on_deck();
    app.deck_layout = true;
    for (w, h) in [(60, 20), (80, 24), (100, 30), (110, 33), (110, 34), (120, 40), (160, 45), (200, 60)] {
        let s = screen(&mut app, w, h);
        assert!(s.contains("BPM") && !s.contains("1 Drives"), "{w}x{h}\n{s}");
    }
}

/// The time left has one place: in large digits by the tempo wherever they fit, and a line beside
/// it where they don't. Under the waveforms it's the time gone and the track's length.
#[test]
fn the_time_left_is_said_once() {
    let (mut app, _dir) = on_deck();
    app.deck_layout = true;
    for (w, h) in SIZES {
        let s = screen(&mut app, w, h);
        let small = s.matches("-4:40").count();
        let large = s.matches(big_row("-4:40", 1).trim_end()).count();
        assert_eq!((small, large), if w == 60 { (1, 0) } else { (0, 1) }, "{w}x{h}\n{s}");
        assert!(!s.contains("0:20 / 5:00"), "the time's said by the time left alone\n{w}x{h}\n{s}");
    }
}

/// Everything on the deck keeps to one left edge and one right, the header's text's, at every size.
#[test]
fn the_deck_keeps_to_one_edge() {
    let (mut app, _dir) = on_deck();
    app.deck_layout = true;
    for (w, h) in SIZES {
        let buf = drawn(&mut app, w, h);
        let top = if h >= 30 { 2 } else { 1 };
        for y in top..h - 2 {
            for x in [0, w - 1] {
                assert_eq!((buf[(x, y)].symbol(), buf[(x, y)].bg), (" ", Color::Reset), "{x},{y} at {w}x{h}");
            }
        }
        let on_the_edge = (top..h - 2).filter(|y| buf[(1, *y)].symbol() != " ").count();
        assert!(on_the_edge >= 3, "{on_the_edge} rows on the edge at {w}x{h}");
    }
}

/// The key's a block three rows high beside the tempo, as loud as anything on the screen.
#[test]
fn the_key_is_loud() {
    let (mut app, _dir) = on_deck();
    app.deck_layout = true;
    theme::set_test_tier(Some(Tier::Truecolor));
    for (w, h) in [(80, 24), (160, 45)] {
        let buf = drawn(&mut app, w, h);
        let at = (0..h)
            .flat_map(|y| (0..w - 1).map(move |x| (x, y)))
            .find(|(x, y)| buf[(*x, *y)].symbol() == "8" && buf[(*x + 1, *y)].symbol() == "A")
            .unwrap();
        let block = buf[at].bg;
        assert_ne!(block, Color::Reset, "{at:?}");
        assert_eq!((buf[(at.0, at.1 - 1)].bg, buf[(at.0, at.1 + 1)].bg), (block, block), "{w}x{h}");
    }
    theme::set_test_tier(None);
}

/// What's under the waveforms is in columns that don't move as its numbers change.
#[test]
fn the_readout_keeps_its_columns() {
    let (mut app, _dir) = on_deck();
    app.deck_layout = true;
    app.now.as_mut().unwrap().phrases = vec![
        PhraseSpan { kind: locallibrary::anlz::PhraseKind::Intro, start_ms: 0.0, end_ms: 30_500.0, bars: 16 },
        PhraseSpan { kind: locallibrary::anlz::PhraseKind::Chorus, start_ms: 30_500.0, end_ms: 300_000.0, bars: 134 },
    ];
    let phrase_at = |app: &mut App, secs| {
        app.resume = Some(Duration::from_secs(secs));
        let s = screen(app, 120, 40);
        let row = s.lines().find(|l| l.contains("phrase ")).map(str::to_string).unwrap();
        row.chars().position(|c| c == 'p').unwrap()
    };
    assert_eq!(phrase_at(&mut app, 20), phrase_at(&mut app, 2), "6.0 bars or 15.0, the phrase stays");
}

/// The readout says only what's on, on one row as on two, however narrow: no time (it's large
/// by the tempo), no waveform colours (`w` says them), no volume at full and no mode that's off.
#[test]
fn the_readout_says_only_whats_on() {
    let (mut app, _dir) = on_deck();
    app.deck_layout = true;
    let off = [format!("{} off", icons::REPEAT.get()), format!("{} off", icons::MIX.get()), "Q off".into()];
    let row_of = |s: &str, said: &str| s.lines().collect::<Vec<_>>().iter().rposition(|l| l.contains(said));
    for (w, h) in SIZES {
        let s = screen(&mut app, w, h);
        for said in ["vol 80%", "Q on"] {
            assert!(s.contains(said), "{said} at {w}x{h}\n{s}");
        }
        for unsaid in ["0:20 / 5:00", "3Band", &off[0], &off[1]] {
            assert!(!s.contains(unsaid), "{unsaid} at {w}x{h}\n{s}");
        }
        let (cue, vol) = (row_of(&s, " A "), row_of(&s, "vol 80%"));
        assert_eq!(cue, vol.map(|v| if w < 76 { v - 1 } else { v }), "{w}x{h}\n{s}");
    }
    (app.volume, app.deck.quantize) = (1.0, false);
    let s = screen(&mut app, 120, 40);
    assert!(!s.contains("vol ") && !s.contains(&off[2]), "full volume and Q off go unsaid\n{s}");
    (app.deck.single, app.deck.auto_cue, app.shuffle) = (true, true, true);
    let s = screen(&mut app, 80, 24);
    assert!(s.contains("single") && s.contains("auto cue") && s.contains(&format!("{} on", icons::MIX.get())), "{s}");
}

/// A phrase's name stays whole under the playhead, which shows as the letter in its colour,
/// underlined.
#[test]
fn the_playhead_leaves_a_phrase_name_whole() {
    let (mut app, _dir) = on_deck();
    app.deck_layout = true;
    app.now.as_mut().unwrap().phrases =
        vec![PhraseSpan { kind: locallibrary::anlz::PhraseKind::Chorus, start_ms: 0.0, end_ms: 300_000.0, bars: 150 }];
    // Three cells of 118 in: the name's second letter.
    app.resume = Some(Duration::from_secs(9));
    let buf = drawn(&mut app, 120, 40);
    let r = app.areas.overview;
    let row: String = (r.x..r.right()).map(|x| buf[(x, r.bottom() - 1)].symbol()).collect();
    assert!(row.starts_with(" Chorus"), "{row}");
    let head = buf[(r.x + 3, r.bottom() - 1)].clone();
    let p = theme::palette();
    assert_eq!(head.fg, (p.paint)(p.playhead), "{row}");
    assert!(head.modifier.contains(Modifier::UNDERLINED) && head.bg != head.fg, "{head:?}");
}

/// The analysis counts a thing in the singular.
#[test]
fn one_of_anything_is_said_as_one() {
    let (mut app, _dir) = on_deck();
    app.deck_layout = true;
    let now = app.now.as_mut().unwrap();
    now.analysis.beats = grid(0, 120.0, 1);
    now.analysis.cues = vec![cue(Some(0), 32_000, Some(34_000))];
    now.phrases =
        vec![PhraseSpan { kind: locallibrary::anlz::PhraseKind::Chorus, start_ms: 0.0, end_ms: 1.0, bars: 1 }];
    now.playlists = vec!["Sets › Friday".into()];
    let s = screen(&mut app, 120, 40);
    assert!(s.contains("in 1 playlist "), "{s}");
}

/// Keys are written as asked, wherever one's shown, the colours coming from the key palette.
#[test]
fn keys_are_written_as_asked() {
    let (mut app, _dir) = on_deck();
    let line_of = |app: &mut App| screen(app, 120, 40);
    for (notation, code, chip) in
        [(Notation::Camelot, "8A", " 8A Am "), (Notation::OpenKey, "1m", " 1m Am "), (Notation::Musical, "Am", " Am ")]
    {
        app.deck.notation = notation;
        app.deck_layout = false;
        assert!(line_of(&mut app).contains(&format!("  {code}  -4:40")), "the strip in {notation:?}");
        app.deck_layout = true;
        assert!(line_of(&mut app).contains(chip), "the deck in {notation:?}");
    }
    app.deck.key_palette = KeyPalette::Tritanopia;
    let k = Camelot::parse("8A").unwrap();
    let buf = drawn(&mut app, 120, 40);
    let paper = theme::rgb3(KeyPalette::Tritanopia.colour(k).unwrap());
    assert!(buf.content.iter().any(|c| c.symbol() == "A" && c.bg == paper), "the key on its colour");
}

/// Keys and their colours round the wheel: 8A is 1m, and neighbours on it are near in a palette.
#[test]
fn notations_go_round_the_same_wheel() {
    let k = |s| Camelot::parse(s).unwrap();
    assert_eq!(Notation::OpenKey.write(k("8A")), "1m");
    assert_eq!(Notation::OpenKey.write(k("8B")), "1d");
    assert_eq!(Notation::OpenKey.write(k("7A")), "12m");
    assert_eq!(Notation::Musical.write(k("8B")), "C");
    for number in 1..=12 {
        for major in [false, true] {
            let key = Camelot { number, major };
            assert_eq!(Camelot::parse(&Notation::OpenKey.write(key)), Some(key), "{key:?}");
            assert_eq!(Camelot::parse(&Notation::Musical.write(key)), Some(key), "{key:?}");
        }
    }
    assert_eq!(KeyPalette::Theme.colour(k("8A")), None);
    assert_eq!(KeyPalette::Tritanopia.colour(k("8A")), Some([0x26, 0xD9, 0x26]), "1 is Mixxx's first");
    assert_eq!(KeyPalette::Deuteranopia.colour(k("7A")), Some([0xA5, 0x0D, 0x22]), "12 its last");
}

/// A row's mark says how its key mixes with the playing track's: = the same, ± a step round the
/// wheel, ↔ its relative, and exactly when the wheel says it does.
#[hegel::test(test_cases = 60)]
fn prop_a_match_symbol_shows_exactly_when_harmony_says(tc: hegel::TestCase) {
    let camelot = |tc: &hegel::TestCase| Camelot {
        number: tc.draw(gs::integers::<u8>().min_value(1).max_value(12)),
        major: tc.draw(gs::booleans()),
    };
    let (master, other) = (camelot(&tc), camelot(&tc));
    let mut lib = library();
    lib.tracks[1].key = other.code();
    let (mut app, _dir) = app_with(lib);
    let l = app.loaded.as_ref().unwrap();
    let track = l.library.tracks[0].clone();
    let root = l.root().to_path_buf();
    let title = l.library.tracks[1].title.clone();
    app.now = playing(TrackAnalysis::default()).0.now.take().map(|n| crate::tui::app::NowPlaying {
        key: Some(master),
        track,
        root,
        ..n
    });
    if tc.draw(gs::booleans()) {
        theme::set_test_mode(Some(theme::Mode::Plain));
    }
    let s = screen(&mut app, 120, 40);
    theme::set_test_mode(None);
    let row = s.lines().find(|l| l.contains(&title)).unwrap_or_else(|| panic!("{title}\n{s}"));
    let mark = match master.relation(other) {
        Some(Relation::Same) => "=",
        Some(Relation::Adjacent) => "±",
        Some(Relation::Relative) => "↔",
        Some(Relation::EnergyBoost) => unreachable!("only asked for with a boost"),
        None => " ",
    };
    assert!(row.contains(&format!("{mark}{} ", other.code())), "{master:?} and {other:?}: {row}");
    for other_mark in ["=", "±", "↔"].into_iter().filter(|m| *m != mark) {
        assert!(!row.contains(&format!("{other_mark}{}", other.code())), "{row}");
    }
}

/// For any keys, notation, key palette and tracks heard: each row writes its key in the notation
/// asked for, the rows heard (other than the one playing) are dim and only they are, and the deck
/// writes the master's key the same way, with its note.
#[hegel::test(test_cases = 40)]
fn prop_the_table_writes_keys_as_asked_and_dims_what_was_heard(tc: hegel::TestCase) {
    let camelot = |tc: &hegel::TestCase| Camelot {
        number: tc.draw(gs::integers::<u8>().min_value(1).max_value(12)),
        major: tc.draw(gs::booleans()),
    };
    let mut lib = library();
    // Its own title, so its row is found by it.
    lib.tracks[3].title = "Second Copy".into();
    let keys: Vec<Camelot> = lib.tracks.iter().map(|_| camelot(&tc)).collect();
    for (t, k) in lib.tracks.iter_mut().zip(&keys) {
        t.key = k.code();
    }
    let heard: Vec<bool> = lib.tracks.iter().map(|_| tc.draw(gs::booleans())).collect();
    let (mut app, _dir) = app_with(lib);
    app.deck.notation = tc.draw(gs::sampled_from(Notation::ALL.to_vec()));
    app.deck.key_palette = tc.draw(gs::sampled_from(KeyPalette::ALL.to_vec()));
    let l = app.loaded.as_ref().unwrap();
    let tracks = l.library.tracks.clone();
    let root = l.root().to_path_buf();
    app.now = playing(TrackAnalysis::default()).0.now.take().map(|n| crate::tui::app::NowPlaying {
        key: Some(keys[0]),
        track: tracks[0].clone(),
        root,
        ..n
    });
    for (t, _) in tracks.iter().zip(&heard).filter(|(_, h)| **h) {
        app.played_tracks.insert(t.file_path.clone());
    }
    let w = tc.draw(gs::integers::<u16>().min_value(100).max_value(200));
    let buf = drawn(&mut app, w, 40);
    let text = |y: u16| (0..w).map(|x| buf[(x, y)].symbol()).collect::<String>();
    for ((t, k), &was_heard) in tracks.iter().zip(&keys).zip(&heard) {
        let y = (0..40)
            .find(|&y| text(y).contains(&format!(" {} ", t.title)))
            .unwrap_or_else(|| panic!("{}: no row", t.title));
        let written = app.deck.notation.write(*k);
        assert!(text(y).contains(&format!("{written} ")), "{:?} {k:?} as {written}: {}", app.deck.notation, text(y));
        let dim = (0..w).any(|x| buf[(x, y)].modifier.contains(Modifier::DIM));
        assert_eq!(dim, was_heard && t.file_path != tracks[0].file_path, "{}", text(y));
    }
    app.deck_layout = true;
    let s = screen(&mut app, w, 40);
    let master = match app.deck.notation {
        Notation::Musical => keys[0].note(),
        n => format!("{} {}", n.write(keys[0]), keys[0].note()),
    };
    assert!(s.contains(&master), "{master}\n{s}");
}

/// Beside the tempo, how far each track is from the master's, with a sign, when something's playing
/// and for the tracks that have a tempo.
#[test]
fn the_table_says_how_far_from_the_master() {
    let mut lib = library();
    lib.tracks[1].bpm = 124.5;
    lib.tracks[2].bpm = 0.0;
    let (t1, t2) = (lib.tracks[1].title.clone(), lib.tracks[2].title.clone());
    let (mut app, _dir) = app_with(lib);
    assert!(!screen(&mut app, 120, 40).contains("ΔBPM"), "nothing to be against");
    let l = app.loaded.as_ref().unwrap();
    let (track, root) = (l.library.tracks[0].clone(), l.root().to_path_buf());
    let now = playing(TrackAnalysis::default()).0.now.take().unwrap();
    app.now = Some(crate::tui::app::NowPlaying { track: Track { bpm: 126.0, ..track }, root, ..now });
    let s = screen(&mut app, 120, 40);
    assert!(s.contains("ΔBPM"), "{s}");
    let row = |t: &str| s.lines().find(|l| l.contains(t)).unwrap().to_string();
    assert!(row(&t1).contains("-1.5"), "{}", row(&t1));
    assert!(!row(&t2).contains('.'), "no tempo, no distance: {}", row(&t2));
    assert!(!screen(&mut app, 60, 20).contains("ΔBPM"), "no room for it");
}

/// The tracks heard this session are dimmed in the library until `U`. A few seconds' listen
/// doesn't count, nor does seeking through one.
#[test]
fn played_tracks_dim_until_they_are_reset() {
    let (mut app, _dir) = on_deck();
    let path = app.now.as_ref().unwrap().track.file_path.clone();
    let title = app.now.as_ref().unwrap().track.title.clone();
    let hear = |app: &mut App, from: u64, steps: u64| {
        for i in 0..steps {
            app.resume = Some(Duration::from_millis(from + i * 400));
            app.tick();
        }
    };
    hear(&mut app, 1_000, 20);
    assert!(!app.played_tracks.contains(&path), "8 seconds' listen");
    app.resume = Some(Duration::from_secs(200));
    app.tick();
    assert!(!app.played_tracks.contains(&path), "a seek isn't listening");
    hear(&mut app, 201_000, 70);
    assert!(app.played_tracks.contains(&path));

    // The playing row stays lit; another track, it's dim.
    let next = app.loaded.as_ref().unwrap().library.tracks[1].clone();
    app.played_tracks.insert(next.file_path.clone());
    let buf = drawn(&mut app, 120, 40);
    let y =
        (0..40).find(|y| (0..120).map(|x| buf[(x, *y)].symbol()).collect::<String>().contains(&next.title)).unwrap();
    assert!((0..120).any(|x| buf[(x, y)].modifier.contains(Modifier::DIM)), "{title}");

    press(&mut app, "U");
    assert!(app.played_tracks.is_empty());
    let buf = drawn(&mut app, 120, 40);
    let dim_text = (0..120).filter(|x| buf[(*x, y)].modifier.contains(Modifier::DIM)).count();
    assert_eq!(dim_text, 0, "{}", last_message(&app));
}

/// Under the waveforms, how many bars and beats to the next cue and phrase, on the grid.
#[test]
fn the_deck_counts_down_in_bars() {
    let (mut app, _dir) = on_deck();
    app.deck_layout = true;
    app.resume = Some(Duration::from_secs(20));
    let now = app.now.as_mut().unwrap();
    now.phrases = vec![
        PhraseSpan { kind: locallibrary::anlz::PhraseKind::Intro, start_ms: 0.0, end_ms: 30_500.0, bars: 16 },
        PhraseSpan { kind: locallibrary::anlz::PhraseKind::Chorus, start_ms: 30_500.0, end_ms: 300_000.0, bars: 134 },
    ];
    // 12 s to the cue is 24 beats; 10.5 s to the chorus 21.
    let s = screen(&mut app, 120, 40);
    assert!(s.contains("cue  A  6.0 bars"), "{s}");
    assert!(s.contains("phrase  Chorus  5.1 bars"), "{s}");
    // With no grid there it's the time.
    let n = app.now.as_mut().unwrap();
    assert_eq!(n.bars_to(20_000.0, 32_000.0), Some((6, 0)));
    assert_eq!(n.bars_to(-1.0, 32_000.0), None);
    n.analysis.beats.clear();
    assert!(screen(&mut app, 120, 40).contains("cue  A  0:12"));
}

/// Near the end of a track the time left turns red, says END, and blinks; with less motion it
/// doesn't blink. How near is a setting, and 0 turns it off.
#[test]
fn the_end_of_a_track_is_warned_of() {
    let (mut app, _dir) = on_deck();
    app.deck_layout = true;
    app.resume = Some(Duration::from_secs(250));
    assert!(end_style(&app).is_none(), "50 s to go");
    assert!(!screen(&mut app, 120, 40).contains(" END "));
    app.resume = Some(Duration::from_secs(280));
    assert_eq!(end_style(&app), Some(bold(theme::current().critical)), "paused, it's lit");
    assert!(screen(&mut app, 120, 40).contains(" END "));
    app.deck.end_warning = 10;
    assert!(end_style(&app).is_none());
    app.resume = Some(Duration::from_secs(295));
    assert!(end_style(&app).is_some());
    app.deck.end_warning = 0;
    assert!(end_style(&app).is_none());
}

/// Blinking is half a second on and half off, and with less motion only on.
#[test]
fn the_warning_blinks_unless_motion_is_reduced() {
    assert!(lit(false, true, 200));
    assert!(!lit(false, true, 700));
    assert!(lit(true, true, 700), "less motion");
    assert!(lit(false, false, 700), "paused");
}

fn kinds() -> TrackWaves {
    let blue = wave(400);
    let rgb = wave(401);
    TrackWaves {
        blue_preview: Some(blue.clone()),
        blue_detail: Some(wave(45_000)),
        rgb_preview: Some(rgb),
        rgb_detail: Some(wave(45_001)),
        band_preview: Some(Bands(vec![[200, 100, 50]; 402])),
        band_detail: Some((Bands(vec![[200, 100, 50]; 45_002]), 150.0)),
    }
}

/// The mode's waveforms, and with a mode a track doesn't have, the nearest it has.
#[test]
fn band_modes_pick_their_waveforms() {
    let (mut app, _dir) = on_deck();
    let columns = |w: Option<(Wave, Option<BandMode>)>| w.map(|(w, m)| (w.len(), m));
    let now = app.now.as_mut().unwrap();
    assert_eq!(columns(now.overview_wave(BandMode::Rgb)), Some((400, None)), "what the analysis has");
    now.waves.kinds = kinds();
    assert_eq!(columns(now.overview_wave(BandMode::Blue)), Some((400, Some(BandMode::Blue))));
    assert_eq!(columns(now.overview_wave(BandMode::Rgb)), Some((401, Some(BandMode::Rgb))));
    assert_eq!(columns(now.overview_wave(BandMode::ThreeBand)), Some((402, Some(BandMode::ThreeBand))));
    assert_eq!(columns(now.zoom_wave(BandMode::Blue)), Some((45_000, Some(BandMode::Blue))));
    assert_eq!(columns(now.zoom_wave(BandMode::Rgb)), Some((45_001, Some(BandMode::Rgb))));
    assert_eq!(columns(now.zoom_wave(BandMode::ThreeBand)), Some((45_002, Some(BandMode::ThreeBand))));
    now.waves.kinds = TrackWaves { blue_preview: Some(wave(9)), blue_detail: Some(wave(8)), ..Default::default() };
    assert_eq!(columns(now.overview_wave(BandMode::ThreeBand)), Some((9, Some(BandMode::Blue))), "older analysis");
    assert_eq!(columns(now.zoom_wave(BandMode::Rgb)), Some((8, Some(BandMode::Blue))));
}

/// It's the drive's My Settings unless one's chosen, and without them 3Band. `w` goes round, each
/// drawn from the next frame.
#[test]
fn waveforms_are_coloured_as_the_drive_says_unless_told() {
    let (mut app, _dir) = on_deck();
    app.deck_layout = true;
    app.now.as_mut().unwrap().waves.kinds = kinds();
    assert_eq!(app.band_mode(), BandMode::ThreeBand);
    app.now.as_mut().unwrap().waves.bands = Some(BandMode::Rgb);
    assert_eq!(app.band_mode(), BandMode::Rgb);
    let rgb = drawn(&mut app, 120, 40);
    press(&mut app, "w");
    assert_eq!(app.band_mode(), BandMode::Blue);
    assert_ne!(rgb, drawn(&mut app, 120, 40));
    press(&mut app, "w");
    assert_eq!(app.band_mode(), BandMode::Rgb);
    press(&mut app, "w");
    assert_eq!(app.band_mode(), BandMode::ThreeBand);
    press(&mut app, "w");
    assert_eq!((app.deck.bands, app.band_mode()), (None, BandMode::Rgb), "as the drive says again");
}

/// Each tier and mode keeps the deck in its colours, and played against ahead reads without them.
#[test]
fn the_deck_reads_in_every_tier() {
    let (mut app, _dir) = on_deck();
    app.deck_layout = true;
    app.now.as_mut().unwrap().waves.kinds = kinds();
    app.resume = Some(Duration::from_secs(150));
    theme::set_test_mode(Some(theme::Mode::Plain));
    let buf = drawn(&mut app, 120, 40);
    theme::set_test_mode(None);
    let r = app.areas.overview;
    let dim_in = |range: std::ops::Range<u16>| {
        (r.y..r.bottom()).any(|y| range.clone().any(|x| buf[(x, y)].modifier.contains(Modifier::DIM)))
    };
    assert!(dim_in(r.x..r.x + 20) && !dim_in(r.right() - 20..r.right()), "played dim without colours");

    theme::set_test_tier(Some(Tier::Colors16));
    let buf = drawn(&mut app, 120, 40);
    theme::set_test_tier(None);
    let indexed = |c: Color| matches!(c, Color::Rgb(..));
    assert!(!buf.content.iter().any(|c| indexed(c.fg) || indexed(c.bg)), "16 colours only");
    assert!(!app.shows_art() || theme::tier() != Tier::Colors16);
}

/// A loop made on the deck shows on the waveforms from the next frame, and goes with it.
#[test]
fn a_loop_on_the_deck_shows_on_the_waveforms() {
    let (mut app, _dir) = on_deck();
    app.deck_layout = true;
    let overview = |app: &mut App| {
        let buf = drawn(app, 120, 40);
        let r = app.areas.overview;
        (r.y..r.bottom())
            .flat_map(|y| (r.x..r.right()).map(move |x| (x, y)))
            .map(|at| buf[at].clone())
            .collect::<Vec<_>>()
    };
    let before = overview(&mut app);
    app.deck.looped = Some(Loop { start: 100_000.0, end: 140_000.0, on: true });
    assert_ne!(before, overview(&mut app), "drawn on the overview, with no other cue changing");
    app.deck.looped = None;
    assert_eq!(before, overview(&mut app));
}

/// The deck with nothing playing says how to get going, and draws nothing that isn't there.
#[test]
fn an_empty_deck_says_what_to_do() {
    let (mut app, _dir) = blank(View::Library);
    app.deck_layout = true;
    for (w, h) in [(60, 20), (80, 24), (120, 40), (160, 45)] {
        let s = screen(&mut app, w, h);
        assert!(s.contains("player mode: play a track") && s.contains("Nothing playing"), "{w}x{h}\n{s}");
    }
}

/// Playlists that share their folders show by their own names, the folder after, so the rows
/// differ in a narrow column.
#[test]
fn playlists_in_one_folder_show_by_name() {
    let (mut app, _dir) = on_deck();
    press(&mut app, "f");
    let folder = "current set 2021 reduced › CLOSING (END) SMALL";
    let names = ["1.2 BEATDOWN/acid", "1.2 BEATDOWN/acid 1", "4.2 ECLECTIC", "4.2 ECLECTIC 1"];
    app.now.as_mut().unwrap().playlists = names.iter().map(|n| format!("{folder} › {n}")).collect();
    let s = screen(&mut app, 120, 40);
    let rows: Vec<&str> = names.iter().filter_map(|n| s.lines().find(|l| l.contains(&format!(" {n}  ")))).collect();
    assert_eq!(rows.len(), 4, "a row each\n{s}");
    assert!(rows[0].contains("acid  CLOS"), "the folder after\n{s}");
}

/// With no detail waveform, a line says so and the panels below get its rows.
#[test]
fn no_detail_waveform_gives_the_panels_its_rows() {
    let (mut app, _dir) = on_deck();
    press(&mut app, "f");
    let now = app.now.as_mut().unwrap();
    now.waves = Default::default();
    now.analysis.detail = None;
    for (w, h) in [(80, 24), (120, 40)] {
        let s = screen(&mut app, w, h);
        let lines: Vec<&str> = s.lines().collect();
        let row = |text: &str| lines.iter().position(|l| l.contains(text)).unwrap_or_else(|| panic!("{text}\n{s}"));
        let (note, panels) = (row("no detail waveform"), row("mixes into"));
        assert!(note < h as usize / 3 && panels <= h as usize / 2, "{w}x{h}: note {note}, panels {panels}\n{s}");
    }
}

/// In either font, every row of any text in large digits is as wide as the rest: each character
/// takes its glyph's width and a column after it, so nothing beside them moves. With ASCII asked
/// for, it's all ASCII.
#[hegel::test(test_cases = 100)]
fn prop_the_large_digits_rows_line_up(tc: hegel::TestCase) {
    let ascii = tc.draw(gs::booleans());
    let text = tc.draw(gs::text().alphabet("0123456789.:-x").max_size(9));
    icons::set_test_set(ascii.then_some(icons::IconSet::Ascii));
    let rows = [0, 1, 2].map(|row| big_row(&text, row));
    let glyphs: Vec<[&str; 3]> = text.chars().map(big_glyph).collect();
    icons::set_test_set(None);
    let wide: usize = glyphs.iter().map(|g| g[0].chars().count() + 1).sum();
    for (row, shown) in rows.iter().enumerate() {
        assert_eq!(shown.chars().count(), wide, "{text:?} row {row}: {shown:?}");
        assert!(!ascii || shown.is_ascii(), "{shown:?}");
    }
    for g in glyphs {
        assert!(g.iter().all(|r| r.chars().count() == g[0].chars().count()), "{g:?}");
    }
}

/// Any key reads back from how each notation writes it, Open Key's from 1 to 12, and a key
/// palette gives the twelve numbers twelve colours, the same for a key's minor and major.
#[hegel::test(test_cases = 100)]
fn prop_keys_read_back_and_keep_their_colours(tc: hegel::TestCase) {
    let camelot = |tc: &hegel::TestCase| Camelot {
        number: tc.draw(gs::integers::<u8>().min_value(1).max_value(12)),
        major: tc.draw(gs::booleans()),
    };
    let (k, other) = (camelot(&tc), camelot(&tc));
    let notation = tc.draw(gs::sampled_from(Notation::ALL.to_vec()));
    let palette = tc.draw(gs::sampled_from(KeyPalette::ALL.to_vec()));
    assert_eq!(Notation::parse(notation.name()), Some(notation));
    assert_eq!(KeyPalette::parse(palette.name()), Some(palette));
    let written = notation.write(k);
    assert_eq!(Camelot::parse(&written), Some(k), "{notation:?} wrote {written}");
    if notation == Notation::OpenKey {
        let (number, mode) = written.split_at(written.len() - 1);
        assert!(number.parse::<u8>().is_ok_and(|n| (1..=12).contains(&n)) && ["d", "m"].contains(&mode), "{written}");
    }
    let (ours, theirs) = (palette.colour(k), palette.colour(other));
    match palette {
        KeyPalette::Theme => assert_eq!((ours, theirs), (None, None)),
        _ => assert_eq!(ours == theirs, k.number == other.number, "{palette:?}: {k:?} {other:?}"),
    }
}

/// A band mode reads back by its name, and the waveforms drawn are the mode asked for whenever the
/// track has it, or else one it has; with none of them, the analysis's own.
#[hegel::test(test_cases = 60)]
fn prop_the_waveforms_drawn_are_ones_the_track_has(tc: hegel::TestCase) {
    let mode = tc.draw(gs::sampled_from(BandMode::ALL.to_vec()));
    assert_eq!(BandMode::parse(mode.name()), Some(mode));
    let has = [0, 1, 2].map(|_| tc.draw(gs::booleans()));
    let all = kinds();
    let (mut app, _dir) = on_deck();
    let now = app.now.as_mut().unwrap();
    now.waves.kinds = TrackWaves {
        blue_preview: all.blue_preview.filter(|_| has[0]),
        blue_detail: all.blue_detail.filter(|_| has[0]),
        rgb_preview: all.rgb_preview.filter(|_| has[1]),
        rgb_detail: all.rgb_detail.filter(|_| has[1]),
        band_preview: all.band_preview.filter(|_| has[2]),
        band_detail: all.band_detail.filter(|_| has[2]),
    };
    let had = |m: BandMode| has[BandMode::ALL.iter().position(|b| *b == m).unwrap()];
    for (what, got) in [("overview", now.overview_wave(mode)), ("zoom", now.zoom_wave(mode))] {
        let drawn = got.map(|(_, m)| m).unwrap_or_else(|| panic!("{what}: the analysis has one"));
        match drawn {
            Some(m) => assert!(had(m) && (m == mode || !had(mode)), "{what}: {m:?} for {mode:?} with {has:?}"),
            None => assert_eq!(has, [false; 3], "{what}"),
        }
    }
}

/// With nothing playing, the deck at any size says so, and where it can carry on a track it says
/// with what and from where, as much of it as fits and never another word.
#[hegel::test(test_cases = 40)]
fn prop_an_empty_deck_says_what_comes_next(tc: hegel::TestCase) {
    let w = tc.draw(gs::integers::<u16>().min_value(60).max_value(200));
    let h = tc.draw(gs::integers::<u16>().min_value(20).max_value(ROOMY - 1));
    let resume = tc.draw(gs::optional(gs::integers::<u64>().max_value(600)));
    let (mut app, _dir) = app_with(library());
    app.deck_layout = true;
    let lib = app.loaded.as_ref().unwrap().library.clone();
    let n = lib.tracks.len();
    app.queue =
        crate::tui::app::Queue { library: lib, items: (0..n).collect(), order: (0..n).collect(), ..Default::default() };
    app.resume = resume.map(Duration::from_secs);
    let s = screen(&mut app, w, h);
    assert!(s.contains("Nothing playing"), "{w}x{h}\n{s}");
    let said = match resume {
        Some(at) => format!(
            "space carries on with {} from {}",
            app.queue.track(0).display_name(),
            crate::output::clock(at as f64)
        ),
        None => "Pick a track and press enter.".into(),
    };
    let row = s.lines().find_map(|l| l.find(&said[..5]).map(|at| &l[at..])).unwrap_or_else(|| panic!("{said}\n{s}"));
    let shown = row.split("  ").next().unwrap_or_default().trim_end();
    assert!(shown.len() >= 5 && said.starts_with(shown), "{shown:?} of {said:?}\n{s}");
}

/// `f` opens player mode from any tab, and closing it, with `f` or esc, goes back to that tab. In
/// the Library view it switches the layouts as it always has, and a tab picked while it's open is
/// where the user stays.
#[test]
fn player_mode_opens_from_any_tab_and_goes_back_there() {
    let (mut app, _dir) = on_deck();
    press(&mut app, "gd");
    press(&mut app, "f");
    assert!(app.deck_showing() && screen(&mut app, 120, 40).contains(FRAME));
    press(&mut app, "f");
    assert_eq!(app.view, View::Drives, "back where it was opened");
    assert!(!app.deck_layout);

    press(&mut app, "gpf\u{1b}");
    assert_eq!((app.view, app.deck_layout), (View::Prepare, false), "esc closes it too");

    press(&mut app, "gdf");
    press(&mut app, "gl");
    press(&mut app, "f");
    assert_eq!((app.view, app.deck_layout), (View::Library, false), "a tab picked is kept");

    press(&mut app, "f");
    assert!(app.deck_showing());
    press(&mut app, "f");
    assert_eq!((app.view, app.deck_layout), (View::Library, false), "the Library view's own switch");
}

/// The footer's hints are buttons: a click on a key or its label is that key. A range isn't one.
#[test]
fn a_footer_hint_clicked_is_its_key() {
    let (mut app, _dir) = on_deck();
    app.deck_layout = true;
    let at = spot(&mut app, "loop ");
    mouse(&mut app, LEFT, at);
    assert!(app.loop_playing().is_some(), "l makes a loop");
    let hot = spot(&mut app, "1-8");
    let before = app.position_ms();
    mouse(&mut app, LEFT, hot);
    assert_eq!(app.position_ms(), before, "a range isn't a key");
    let at = spot(&mut app, "b browse");
    mouse(&mut app, LEFT, (at.0 + 2, at.1));
    assert!(app.browsing, "the label: b");
    press(&mut app, "\u{1b}");
    let at = spot(&mut app, "f back");
    mouse(&mut app, LEFT, at);
    assert!(!app.deck_layout, "the key: f, back to the Library layout");
}

/// The loop chip under the waveforms ends the loop when clicked, as l does.
#[test]
fn the_loop_chip_clicked_ends_the_loop() {
    let (mut app, _dir) = on_deck();
    app.deck_layout = true;
    press(&mut app, "l");
    assert!(app.loop_playing().is_some());
    let at = spot(&mut app, "LOOP");
    mouse(&mut app, LEFT, at);
    assert!(app.loop_playing().is_none(), "{}", screen(&mut app, 120, 30));
}
