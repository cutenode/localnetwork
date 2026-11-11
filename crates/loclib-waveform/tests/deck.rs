//! The widgets a deck is made of: the zoomed view's ring, rows of cue markers, three bands, and a
//! playhead that moves in eighths of a cell.

use hegel::TestCase;
use hegel::generators as gs;
use loclib_model::analysis::{Bands, Beat, Cue, WaveColumn, WaveLayer, Waveform, WaveformStyle};
use loclib_model::tokens::bands::{HIGH, LOW, MID};
#[cfg(feature = "image")]
use loclib_waveform::Art;
use loclib_waveform::{
    Overview, Palette, PhraseStrip, Wave, WaveOverview, Zoom, ZoomRing, band_gain, cue_letter, place_markers,
    playhead_cells,
};
use ratatui::buffer::{Buffer, Cell};
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier};
use ratatui::widgets::Widget;

fn rgb([r, g, b]: [u8; 3]) -> Color {
    Color::Rgb(r, g, b)
}

fn wave(n: usize) -> Waveform {
    let columns = (0..n)
        .map(|i| WaveColumn {
            back: WaveLayer { height: ((i * 7) % 11) as f32 / 10.0, color: [40, 120, 255] },
            front: WaveLayer { height: ((i * 3) % 5) as f32 / 10.0, color: [255, 255, 255] },
        })
        .collect();
    Waveform { style: WaveformStyle::Rgb, columns_per_sec: Some(150.0), columns }
}

fn cue(hot: Option<u8>, start_ms: u32, end_ms: Option<u32>) -> Cue {
    Cue {
        hot_cue: hot,
        start_ms,
        end_ms,
        comment: String::new(),
        color: None,
        color_id: 0,
        loop_beats: None,
        active_loop: false,
    }
}

/// A hot cue or memory cue with a name.
fn cue_at(hot: Option<u8>, start_ms: u32, name: &str) -> Cue {
    Cue { comment: name.to_string(), ..cue(hot, start_ms, None) }
}

/// Beats at 120 from the start, every fourth starting a bar.
fn beats(n: u32) -> Vec<Beat> {
    (0..n).map(|i| Beat { beat_number: (i % 4) as u16 + 1, bpm: 120.0, time_ms: i * 500 }).collect()
}

fn drawn(ring: &mut ZoomRing, zoom: &Zoom<'_>, area: Rect) -> Buffer {
    let mut buf = Buffer::empty(area);
    ring.draw(zoom, area, &mut buf);
    buf
}

/// The zoomed view's middle column is the playhead wherever the track is and however far in it's
/// zoomed: a marker over it, the playhead's colour or line all the way down, and it's what's at the
/// playhead that's there, as a cue right on it shows.
#[hegel::test(test_cases = 200)]
fn prop_the_zoomed_views_middle_column_is_the_playhead(tc: TestCase) {
    let area = Rect::new(
        0,
        0,
        tc.draw(gs::integers::<u16>().min_value(8).max_value(120)),
        tc.draw(gs::integers::<u16>().min_value(4).max_value(16)),
    );
    let w = wave(3_000);
    let at = tc.draw(gs::integers::<u32>().max_value(30_000));
    let (beats, cues) = (beats(40), [cue(Some(0), at, None)]);
    let position_secs = f64::from(at) / 1000.0;
    let visible_secs = tc.draw(gs::floats::<f64>().min_value(1.5).max_value(60.0));
    let palette = Palette::default();
    let wave = Wave::Layers(&w);
    let zoom =
        Zoom { wave, position_secs, visible_secs, beats: &beats, cues: &cues, cue_rows: 1, bar_numbers: true, palette };
    let buf = drawn(&mut ZoomRing::default(), &zoom, area);
    let centre = area.width / 2;
    assert_eq!((buf[(centre, 0)].symbol(), buf[(centre, 0)].fg), ("▼", rgb(palette.fg)));
    assert_eq!(buf[(centre, 1)].symbol(), "A", "the cue at the playhead is in the middle");
    for y in 2..area.height {
        let cell = &buf[(centre, y)];
        let line = cell.symbol() == "│" && cell.fg == rgb(palette.fg);
        assert!(line || cell.fg == rgb(palette.playhead), "row {y}: {cell:?}");
    }
}

/// Scrolling and seeking, a ring draws just what one starting afresh would: kept columns are never
/// out of place, however it got there.
#[hegel::test(test_cases = 100)]
fn prop_a_ring_draws_what_a_fresh_one_would(tc: TestCase) {
    let area = Rect::new(0, 0, tc.draw(gs::integers::<u16>().min_value(8).max_value(80)), 9);
    let w = wave(3_000);
    let (beats, cues) = (beats(40), [cue(Some(1), 4_000, None), cue(None, 8_000, Some(9_000))]);
    let palette = Palette::default();
    let mut ring = ZoomRing::default();
    let steps = tc.draw(gs::vecs(gs::floats::<f64>().min_value(-3.0).max_value(3.0)).min_size(1).max_size(12));
    let mut position_secs = tc.draw(gs::floats::<f64>().min_value(0.0).max_value(20.0));
    for step in steps {
        position_secs = (position_secs + step).max(0.0);
        let zoom = Zoom {
            wave: Wave::Layers(&w),
            position_secs,
            visible_secs: 4.0,
            beats: &beats,
            cues: &cues,
            cue_rows: 1,
            bar_numbers: true,
            palette,
        };
        let kept = drawn(&mut ring, &zoom, area);
        assert_eq!(kept, drawn(&mut ZoomRing::default(), &zoom, area), "at {position_secs}");
    }
}

/// `forget` is what makes a ring look again: it keeps what it drew until it's told.
#[test]
fn a_ring_redraws_when_told_what_it_shows_is_new() {
    let area = Rect::new(0, 0, 20, 6);
    let (quiet, loud) = (wave(600), Waveform { columns: vec![WaveColumn::default(); 600], ..wave(0) });
    let zoom = |w| Zoom {
        wave: Wave::Layers(w),
        position_secs: 2.0,
        visible_secs: 4.0,
        beats: &[],
        cues: &[],
        cue_rows: 0,
        bar_numbers: false,
        palette: Palette::default(),
    };
    let mut ring = ZoomRing::default();
    let first = drawn(&mut ring, &zoom(&quiet), area);
    assert_eq!(drawn(&mut ring, &zoom(&loud), area), first, "the same shape, the same columns");
    ring.forget();
    assert_ne!(drawn(&mut ring, &zoom(&loud), area), first);
}

/// The ruler has a stronger tick on a bar's first beat, a light one on the others, and the bar's
/// number after it. Zoomed out, beats go and numbers space out so they stay readable.
#[test]
fn the_ruler_marks_beats_and_numbers_bars() {
    let (w, beats) = (wave(6_000), beats(200));
    let palette = Palette::default();
    let zoom = |visible_secs| Zoom {
        wave: Wave::Layers(&w),
        position_secs: 8.0,
        visible_secs,
        beats: &beats,
        cues: &[],
        cue_rows: 0,
        bar_numbers: true,
        palette,
    };
    let row = |buf: &Buffer| (0..buf.area.width).map(|x| buf[(x, 0)].symbol()).collect::<String>();
    let area = Rect::new(0, 0, 80, 5);
    // 0.1 s a column, the playhead at 8 s in column 40: from 4 s, where bar 3 starts.
    let close = drawn(&mut ZoomRing::default(), &zoom(8.0), area);
    let ruler = row(&close);
    assert!(ruler.starts_with("╻3   ╷"), "{ruler}");
    assert_eq!(close[(0, 0)].fg, rgb(palette.accent));
    assert_eq!(close[(5, 0)].fg, rgb(palette.dim));
    assert!(ruler.contains("╻4"), "{ruler}");

    let far = row(&drawn(&mut ZoomRing::default(), &zoom(80.0), area));
    assert!(!far.contains('╷'), "no room for beats: {far}");
    let digits: Vec<char> = far.chars().filter(char::is_ascii_digit).collect();
    assert!(!digits.is_empty() && digits.len() < far.matches('╻').count(), "{far}");
}

/// Above the waveform, a marker has its row unless another's there; a second row takes it if there
/// is one, and otherwise it moves along.
#[test]
fn markers_that_collide_take_a_second_row() {
    let cues = [cue(Some(0), 1_000, None), cue(Some(1), 1_010, None), cue(None, 1_000, None)];
    let x_of = |c: &Cue| Some(i64::from(c.start_ms) / 100);
    let two = place_markers(&cues, 2, 100, x_of);
    let at = |letter| two.iter().find(|m| cue_letter(m.cue) == letter).map(|m| (m.x, m.row));
    assert_eq!(at(Some('A')), Some((10, 0)));
    assert_eq!(at(Some('B')), Some((10, 1)));
    assert_eq!(at(None), Some((11, 0)), "both rows taken: the memory cue moves along");
    let one = place_markers(&cues, 1, 11, x_of);
    assert_eq!(one.len(), 1, "no cell free before the end: {one:?}");

    let area = Rect::new(0, 0, 100, 4);
    let mut buf = Buffer::empty(area);
    let overview = |cues| WaveOverview {
        wave: None,
        progress: 0.0,
        cues,
        duration_ms: 10_000.0,
        cue_rows: 2,
        palette: Palette::default(),
    };
    assert_eq!(overview(&cues[..2]).marker_rows(area), 2);
    overview(&cues[..2]).render(area, &mut buf);
    assert_eq!((buf[(10, 0)].symbol(), buf[(10, 1)].symbol()), ("A", "B"));
    let apart = [cue(Some(0), 1_000, None), cue(Some(1), 5_000, None)];
    assert_eq!(overview(&apart).marker_rows(area), 1, "they don't collide");
}

/// However many there are, wherever they fall, no two markers share a cell, and nothing is placed
/// left of where it belongs or past the end.
#[hegel::test(test_cases = 300)]
fn prop_markers_never_overlap_in_a_row(tc: TestCase) {
    let cues: Vec<Cue> = tc
        .draw(gs::vecs(gs::tuples!(gs::integers::<u32>().max_value(5_000), gs::booleans())).max_size(24))
        .into_iter()
        .enumerate()
        .map(|(i, (ms, hot))| cue(hot.then_some(i as u8), ms, None))
        .collect();
    let rows = tc.draw(gs::integers::<u16>().min_value(1).max_value(3));
    let end = tc.draw(gs::integers::<i64>().min_value(1).max_value(120));
    let x_of = |c: &Cue| Some(i64::from(c.start_ms) * end / 5_000);
    let placed = place_markers(&cues, rows, end, x_of);
    let mut cells: Vec<(i64, u16)> = placed.iter().map(|m| (m.x, m.row)).collect();
    cells.sort_unstable();
    let count = cells.len();
    cells.dedup();
    assert_eq!(cells.len(), count, "two share a cell: {placed:?}");
    for m in &placed {
        assert!(m.row < rows && m.x < end && m.x >= i64::from(m.cue.start_ms) * end / 5_000, "{m:?}");
    }
}

/// Two hot cues' letters never sit side by side in a row, where they'd read as a word, however
/// close together the cues are.
#[hegel::test(test_cases = 300)]
fn prop_hot_cue_letters_never_touch(tc: TestCase) {
    let cues: Vec<Cue> = tc
        .draw(gs::vecs(gs::integers::<u32>().max_value(5_000)).max_size(12))
        .into_iter()
        .enumerate()
        .map(|(i, ms)| cue(Some(i as u8), ms, None))
        .collect();
    let rows = tc.draw(gs::integers::<u16>().min_value(1).max_value(3));
    let x_of = |c: &Cue| Some(i64::from(c.start_ms) / 100);
    let placed = place_markers(&cues, rows, 120, x_of);
    for (a, b) in placed.iter().flat_map(|a| placed.iter().map(move |b| (a, b))) {
        assert!(a.row != b.row || a.x.abs_diff(b.x) != 1, "{a:?} beside {b:?}");
    }
}

/// Three bands in eight rows of a cell each: each boundary falls within a cell, which shows the
/// band on either side of it in its two colours.
#[test]
fn bands_meet_inside_a_cell_at_eighths() {
    // Lows at full height, mids to 0.66 * 200/255, highs to 0.32 * 100/255.
    let bands = Bands(vec![[255, 200, 100]; 40]);
    let area = Rect::new(0, 0, 40, 5);
    let mut buf = Buffer::empty(area);
    let wave = Some(Wave::Bands { bands: &bands, columns_per_sec: None, gain: 1.0 });
    let palette = Palette { marker: |c| c, ..Palette::default() };
    WaveOverview { wave, progress: 0.0, cues: &[], duration_ms: 1000.0, cue_rows: 1, palette }.render(area, &mut buf);
    // Four rows. High: 0.5 rows; mid: 2.07; low: 4. From the bottom:
    let at = |row: u16| &buf[(20, 4 - row)];
    assert_eq!((at(0).symbol(), at(0).fg, at(0).bg), ("▄", rgb(HIGH), rgb(MID)));
    assert_eq!((at(1).symbol(), at(1).fg), ("█", rgb(MID)));
    assert_eq!((at(2).symbol(), at(2).fg, at(2).bg), ("▁", rgb(MID), rgb(LOW)));
    assert_eq!((at(3).symbol(), at(3).fg), ("█", rgb(LOW)));
}

/// A drive's own colours are muted a little, and the bands', chosen to be drawn, aren't.
#[test]
fn a_drives_waveform_colours_are_muted() {
    let column = WaveColumn { back: WaveLayer { height: 1.0, color: [255, 0, 0] }, front: WaveLayer::default() };
    let w = Waveform { style: WaveformStyle::Rgb, columns_per_sec: None, columns: vec![column; 40] };
    let area = Rect::new(0, 0, 40, 5);
    let mut buf = Buffer::empty(area);
    let (palette, wave) = (Palette::default(), Some(Wave::Layers(&w)));
    WaveOverview { wave, progress: 0.0, cues: &[], duration_ms: 1000.0, cue_rows: 1, palette }.render(area, &mut buf);
    assert_eq!(buf[(20, 3)].fg, rgb(loclib_waveform::color::mute([255, 0, 0], 0.15)));
}

/// The waveform that's read is the brighter of the two, whatever the colours.
#[test]
fn the_zoomed_view_is_brighter_than_the_overview() {
    let bands = Bands(vec![[255, 0, 0]; 3_000]);
    let wave = Wave::Bands { bands: &bands, columns_per_sec: Some(150.0), gain: 1.0 };
    let (beats, palette) = (&[], Palette::default());
    let zoom = Zoom {
        wave,
        position_secs: 10.0,
        visible_secs: 4.0,
        beats,
        cues: &[],
        cue_rows: 0,
        bar_numbers: false,
        palette,
    };
    let zoomed = drawn(&mut ZoomRing::default(), &zoom, Rect::new(0, 0, 20, 7));
    let area = Rect::new(0, 0, 20, 5);
    let mut overview = Buffer::empty(area);
    WaveOverview { wave: Some(wave), progress: 0.0, cues: &[], duration_ms: 20_000.0, cue_rows: 1, palette }
        .render(area, &mut overview);
    let lit = |c: Color| match c {
        Color::Rgb(r, g, b) => loclib_waveform::color::luminance([r, g, b]),
        other => panic!("{other:?}"),
    };
    assert!(lit(zoomed[(5, 2)].fg) > lit(overview[(10, 3)].fg) * 1.1);
}

/// On a light page a waveform's highs, the core of it, go nearly to the ink, not to the grey a
/// marker would stop at.
#[test]
fn the_highs_are_the_core_of_the_waveform_on_a_light_page() {
    let bands = Bands(vec![[0, 0, 255]; 40]);
    let area = Rect::new(0, 0, 40, 5);
    let mut buf = Buffer::empty(area);
    let wave = Some(Wave::Bands { bands: &bands, columns_per_sec: None, gain: 3.0 });
    let palette =
        Palette { marker: |c| c.map(|v| v / 2), background: [250, 250, 245], fg: [20, 20, 40], ..Palette::default() };
    WaveOverview { wave, progress: 0.0, cues: &[], duration_ms: 1000.0, cue_rows: 1, palette }.render(area, &mut buf);
    assert_eq!(buf[(20, 4)].fg, rgb(palette.fade(palette.fg, 0.8)));
}

/// A cue's line crosses the waveform thin, so the playhead's the one thing a whole cell wide, and
/// the waveform's colour stays either side of it.
#[test]
fn a_cues_line_is_thinner_than_the_playhead() {
    let column = WaveColumn { back: WaveLayer { height: 1.0, color: [10, 200, 90] }, front: WaveLayer::default() };
    let w = Waveform { style: WaveformStyle::Rgb, columns_per_sec: Some(10.0), columns: vec![column; 200] };
    let cues = [cue_at(Some(0), 10_800, "")];
    let palette = Palette::default();
    let z = Zoom {
        wave: Wave::Layers(&w),
        position_secs: 10.0,
        visible_secs: 4.0,
        beats: &[],
        cues: &cues,
        cue_rows: 0,
        bar_numbers: false,
        palette,
    };
    let buf = drawn(&mut ZoomRing::default(), &z, Rect::new(0, 0, 20, 7));
    let (line, beside) = (&buf[(14, 3)], &buf[(13, 3)]);
    assert_eq!((line.symbol(), line.fg, line.bg), ("┃", rgb(palette.hot_cue), beside.fg));
}

/// A cue's name keeps a cell from its marker, and shows whole or not at all: one that won't fit
/// before the next marker in the row is left out.
#[test]
fn a_cues_name_is_clear_of_its_marker_and_whole() {
    let w = Waveform { style: WaveformStyle::Rgb, columns_per_sec: Some(10.0), columns: vec![Default::default(); 200] };
    let row_for = |cues: &[loclib_model::analysis::Cue]| {
        let z = Zoom {
            wave: Wave::Layers(&w),
            position_secs: 10.0,
            visible_secs: 4.0,
            beats: &[],
            cues,
            cue_rows: 1,
            bar_numbers: false,
            palette: Palette::default(),
        };
        let buf = drawn(&mut ZoomRing::default(), &z, Rect::new(0, 0, 40, 7));
        (0..40).map(|x| buf[(x, 1)].symbol().to_string()).collect::<String>()
    };
    // 0.1 s a column from 8 s: 8.4 s is column 4.
    let alone = row_for(&[cue_at(Some(0), 8_400, "Intro")]);
    assert!(alone.starts_with("    A Intro "), "{alone:?}");
    let crowded = row_for(&[cue_at(Some(0), 8_400, "Intro"), cue_at(Some(1), 8_900, "")]);
    assert!(crowded.starts_with("    A    B "), "{crowded:?}");
}

/// The waveform's lower half mirrors its upper one, cell for cell. Blocks only fill from the
/// bottom, so what fills from the top is the rest of a block in reverse video.
#[test]
fn the_zoomed_view_mirrors_in_eighths() {
    let column = WaveColumn { back: WaveLayer { height: 0.75, color: [10, 200, 90] }, front: WaveLayer::default() };
    let w = Waveform { style: WaveformStyle::Rgb, columns_per_sec: Some(10.0), columns: vec![column; 200] };
    let area = Rect::new(0, 0, 20, 7);
    let zoom = Zoom {
        wave: Wave::Layers(&w),
        position_secs: 10.0,
        visible_secs: 4.0,
        beats: &[],
        cues: &[],
        cue_rows: 0,
        bar_numbers: false,
        palette: Palette::default(),
    };
    let buf = drawn(&mut ZoomRing::default(), &zoom, area);
    // Three rows each way, so 2.25 cells: from the middle out, two full ones and a quarter.
    let up: Vec<&str> = (1..4).map(|y| buf[(3, y)].symbol()).collect();
    assert_eq!(up, ["▂", "█", "█"]);
    let down: Vec<&str> = (4..7).map(|y| buf[(3, y)].symbol()).collect();
    assert_eq!(down, ["█", "█", "▆"]);
    assert!(buf[(3, 6)].modifier.contains(Modifier::REVERSED), "a quarter from the top, in reverse");
    assert!(!buf[(3, 2)].modifier.contains(Modifier::REVERSED));
}

/// The playhead moves an eighth of a cell at a time across two cells, as wide as one all the way.
#[test]
fn the_playhead_moves_in_eighths() {
    let under = Color::Rgb(10, 20, 30);
    let white = Color::Rgb(255, 255, 255);
    let cell = |symbol: &str| {
        let mut c = Cell::default();
        c.set_symbol(symbol).set_fg(under);
        c
    };
    let (mut head, mut next) = (cell("█"), cell("▅"));
    playhead_cells(&mut head, Some(&mut next), 0, white);
    assert_eq!((head.symbol(), head.fg, next.symbol(), next.fg), ("█", white, "▅", under), "on the cell");

    // Three eighths in: the first three of the cell it left stay as they were, and the three it
    // reached in the next are lit.
    let (mut head, mut next) = (cell("█"), cell("▅"));
    playhead_cells(&mut head, Some(&mut next), 3, white);
    assert_eq!((head.symbol(), head.fg, head.bg), ("▍", white, under));
    assert!(head.modifier.contains(Modifier::REVERSED), "what's lit is what the block leaves");
    assert_eq!((next.symbol(), next.fg, next.bg), ("▍", white, under));
    assert!(!next.modifier.contains(Modifier::REVERSED));

    // At the end of the overview there's no next cell.
    let mut last = cell(" ");
    playhead_cells(&mut last, None, 5, white);
    assert_eq!((last.symbol(), last.bg), ("▋", Color::Reset), "nothing under it, the terminal's own");
}

/// With colours that can't shade, what's been played takes the dim attribute, and a loop no tint.
#[test]
fn a_flat_palette_dims_with_the_attribute() {
    let w = wave(100);
    let cues = [cue(None, 6_000, Some(8_000))];
    let area = Rect::new(0, 0, 40, 4);
    let draw = |flat| {
        let mut buf = Buffer::empty(area);
        let palette = Palette { flat, ..Palette::default() };
        Overview { wave: Some(&w), progress: 0.5, cues: &cues, duration_ms: 10_000.0, palette }.render(area, &mut buf);
        buf
    };
    let dim = |b: &Buffer, x: u16| (1..4).any(|y| b[(x, y)].modifier.contains(Modifier::DIM));
    let (flat, coloured) = (draw(true), draw(false));
    assert!(dim(&flat, 5) && !dim(&flat, 35), "played dim, ahead not");
    assert!(!dim(&coloured, 5), "a colour's enough");
    let tint = rgb(loclib_waveform::color::scale(Palette::default().loop_cue, 0.22));
    let tinted = |b: &Buffer| (1..4).any(|y| b[(26, y)].bg == tint);
    assert!(tinted(&coloured) && !tinted(&flat));
}

/// With one colour for everything, the playhead is a line cut out of the waveform, and with 16 a
/// cue's letter is in reverse video, which reads whatever the scheme's colours are.
#[test]
fn the_deck_reads_without_colours() {
    let column = WaveColumn { back: WaveLayer { height: 1.0, color: [10, 200, 90] }, front: WaveLayer::default() };
    let w = Waveform { style: WaveformStyle::Rgb, columns_per_sec: Some(10.0), columns: vec![column; 200] };
    let cues = [cue(Some(0), 11_000, None)];
    let area = Rect::new(0, 0, 20, 7);
    let palette = Palette { plain: true, flat: true, paint: |_| Color::Reset, ..Palette::default() };
    let zoom = Zoom {
        wave: Wave::Layers(&w),
        position_secs: 10.0,
        visible_secs: 4.0,
        beats: &[],
        cues: &cues,
        cue_rows: 1,
        bar_numbers: false,
        palette,
    };
    let buf = drawn(&mut ZoomRing::default(), &zoom, area);
    let head = &buf[(10, 4)];
    assert!(head.symbol() == "│" && head.modifier.contains(Modifier::REVERSED), "{head:?}");
    let letter = &buf[(15, 1)];
    assert!(letter.symbol() == "A" && letter.modifier.contains(Modifier::REVERSED), "{letter:?}");
}

/// With 16 colours a phrase's name is in reverse video, as its ink can't be chosen, and what's been
/// played is dim.
#[test]
fn phrases_read_on_a_flat_palette() {
    let phrases =
        [(0.0, 5_000.0, "Intro".to_string(), [70, 112, 232]), (5_000.0, 10_000.0, "Up".to_string(), [232, 168, 60])];
    let area = Rect::new(0, 0, 40, 1);
    let mut buf = Buffer::empty(area);
    let palette = Palette { flat: true, ..Palette::default() };
    PhraseStrip { phrases: &phrases, duration_ms: 10_000.0, progress: 0.75, palette }.render(area, &mut buf);
    let name = &buf[(1, 0)];
    assert_eq!((name.symbol(), name.fg, name.bg), ("I", rgb([70, 112, 232]), Color::Reset));
    assert!(name.modifier.contains(Modifier::REVERSED | Modifier::DIM), "played: {name:?}");
    let ahead = &buf[(35, 0)];
    assert!(ahead.modifier.contains(Modifier::REVERSED) && !ahead.modifier.contains(Modifier::DIM), "{ahead:?}");
}

/// A quiet master is turned up until its tallest band reaches the top of the waveform, however
/// that band is scaled, and all of it by the same amount.
#[test]
fn bands_fill_the_height() {
    let bands = Bands(vec![[64, 100, 0], [32, 50, 10]]);
    let gain = band_gain(&bands);
    // Lows count in full and mids two thirds, so mids set it at 66 of 255.
    assert!((gain - 255.0 / 66.0).abs() < 0.01, "{gain}");
    assert_eq!(band_gain(&Bands(vec![[0, 0, 0]])), 1.0, "silence stays silent");
    assert_eq!(band_gain(&Bands(Vec::new())), 1.0);
    let area = Rect::new(0, 0, 10, 5);
    let draw = |gain| {
        let mut buf = Buffer::empty(area);
        let wave = Some(Wave::Bands { bands: &bands, columns_per_sec: None, gain });
        let palette = Palette::default();
        WaveOverview { wave, progress: 0.0, cues: &[], duration_ms: 1000.0, cue_rows: 1, palette }
            .render(area, &mut buf);
        buf
    };
    assert_eq!(draw(1.0)[(1, 1)].symbol(), " ", "as stored it stops well short");
    assert_ne!(draw(gain)[(1, 1)].symbol(), " ", "turned up, it reaches the top");
}

/// A cover's main colours: what most of it is, and the most of what's far from that.
#[cfg(feature = "image")]
#[test]
fn a_covers_main_colours() {
    let cover = image::RgbImage::from_fn(64, 64, |x, _| {
        image::Rgb(match x {
            0..=39 => [200, 30, 30],
            40..=59 => [30, 30, 200],
            _ => [190, 40, 30],
        })
    });
    let mut png = Vec::new();
    cover.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
    assert_eq!(Art::decode(&png).unwrap().main_colours(), [[200, 30, 30], [30, 30, 200]]);
    let mut png = Vec::new();
    let flat = image::RgbImage::from_pixel(8, 8, image::Rgb([9, 99, 199]));
    flat.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
    assert_eq!(Art::decode(&png).unwrap().main_colours(), [[9, 99, 199]; 2]);
}
