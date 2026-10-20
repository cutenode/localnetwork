//! Properties of the widgets and the colour arithmetic, for any track, cues and size.

use hegel::TestCase;
use hegel::generators as gs;
use loclib_model::analysis::{Beat, Cue, WaveColumn, WaveLayer, Waveform, WaveformStyle};
use loclib_model::tokens::PLAYHEAD;
use loclib_waveform::color::{gradient, mix, scale};
use loclib_waveform::{
    CueLane, Detail, Overview, Palette, PhraseStrip, Pixels, Rgb, Scope, Spectrum, bar_layout, cue_label, cue_letter,
    vinyl,
};
use ratatui::buffer::{Buffer, Cell};
use ratatui::layout::Rect;
use ratatui::style::Color;
use ratatui::widgets::Widget;

#[hegel::test_helper]
fn rgb(tc: &TestCase) -> Rgb {
    tc.draw(gs::arrays::<_, _, 3>(gs::integers::<u8>()))
}

#[hegel::test_helper]
fn wave(tc: &TestCase, max_columns: usize) -> Waveform {
    let heights = tc.draw(gs::vecs(gs::floats::<f32>().min_value(0.0).max_value(1.0)).max_size(max_columns));
    let columns = heights
        .iter()
        .map(|&h| WaveColumn {
            back: WaveLayer { height: h, color: [200, 50, 50] },
            front: WaveLayer { height: h * 0.5, color: [255, 255, 255] },
        })
        .collect();
    Waveform { style: WaveformStyle::Rgb, columns_per_sec: tc.draw(gs::optional(gs::just(150.0))), columns }
}

/// A cue as the drive's analysis gives them: a loop ends after it starts, and a stored loop
/// length has a denominator.
#[hegel::test_helper]
fn cue(tc: &TestCase, max_ms: u32) -> Cue {
    let start_ms = tc.draw(gs::integers::<u32>().max_value(max_ms));
    let length = tc.draw(gs::optional(gs::integers::<u32>().min_value(1).max_value(60_000)));
    let loop_beats = tc.draw(gs::optional(gs::tuples!(
        gs::integers::<u16>().min_value(1).max_value(64),
        gs::integers::<u16>().min_value(1).max_value(8)
    )));
    Cue {
        hot_cue: tc.draw(gs::optional(gs::integers::<u8>().max_value(30))),
        start_ms,
        end_ms: length.map(|l| start_ms + l),
        comment: tc.draw(gs::text().alphabet(" abcDrop↻é").max_size(12)),
        color: tc.draw(gs::optional(gs::arrays::<_, _, 3>(gs::integers::<u8>()))),
        color_id: tc.draw(gs::integers::<u8>().max_value(10)),
        loop_beats: length.and(loop_beats),
    }
}

#[hegel::test_helper]
fn cues(tc: &TestCase, max_ms: u32) -> Vec<Cue> {
    let n = tc.draw(gs::integers::<usize>().max_value(6));
    (0..n).map(|_| cue(tc, max_ms)).collect()
}

#[hegel::test_helper]
fn palette(tc: &TestCase) -> Palette {
    Palette { plain: tc.draw(gs::booleans()), ..Palette::default() }
}

/// Any widget, with anything to draw, draws only inside the area it's given: the cells around
/// it are left as they were.
#[hegel::test(test_cases = 400)]
fn widgets_draw_only_inside_their_area(tc: TestCase) {
    let (width, height) = (tc.draw(gs::integers::<u16>().max_value(90)), tc.draw(gs::integers::<u16>().max_value(14)));
    let area = Rect::new(
        tc.draw(gs::integers::<u16>().max_value(3)),
        tc.draw(gs::integers::<u16>().max_value(3)),
        width,
        height,
    );
    let whole = Rect::new(0, 0, area.right() + 3, area.bottom() + 3);
    let mut buf = Buffer::empty(whole);
    let palette = palette(&tc);
    let duration_ms = tc.draw(gs::floats::<f64>().min_value(0.0).max_value(30_000.0));
    let progress = tc.draw(gs::floats::<f32>().min_value(-0.5).max_value(1.5));
    let position_secs = tc.draw(gs::floats::<f64>().min_value(-10.0).max_value(40.0));
    let visible_secs = tc.draw(gs::floats::<f64>().min_value(0.5).max_value(30.0));
    match tc.draw(gs::integers::<u8>().max_value(6)) {
        0 => {
            let w = wave(&tc, 400);
            let wave = tc.draw(gs::booleans()).then_some(&w);
            let cues = cues(&tc, 30_000);
            Overview { wave, progress, cues: &cues, duration_ms, palette }.render(area, &mut buf);
        }
        1 => {
            let w = wave(&tc, 6000);
            let beats: Vec<Beat> = tc
                .draw(gs::vecs(gs::integers::<u32>().max_value(40_000)).max_size(20))
                .into_iter()
                .enumerate()
                .map(|(i, time_ms)| Beat { beat_number: (i % 4) as u16 + 1, bpm: 128.0, time_ms })
                .collect();
            let cues = cues(&tc, 40_000);
            Detail { wave: &w, position_secs, visible_secs, beats: &beats, cues: &cues, palette }
                .render(area, &mut buf);
        }
        2 => {
            let cues = cues(&tc, 40_000);
            let bpm = tc.draw(gs::floats::<f64>().min_value(0.0).max_value(200.0));
            CueLane { cues: &cues, position_secs, visible_secs, bpm, palette }.render(area, &mut buf);
        }
        3 => {
            let spans = tc.draw(gs::vecs(gs::tuples!(
                gs::floats::<f64>().min_value(0.0).max_value(40_000.0),
                gs::floats::<f64>().min_value(0.0).max_value(40_000.0),
                gs::text().alphabet("IntroUpChorus").max_size(8)
            )));
            let phrases: Vec<(f64, f64, String, Rgb)> =
                spans.into_iter().map(|(a, b, name)| (a.min(b), a.max(b), name, [90, 160, 80])).collect();
            PhraseStrip { phrases: &phrases, duration_ms, progress, palette }.render(area, &mut buf);
        }
        4 => {
            let mirror = tc.draw(gs::booleans());
            let n = bar_layout(area.width, mirror).0;
            let bands = tc.draw(gs::vecs(gs::floats::<f32>().min_value(0.0).max_value(1.0)).max_size(n));
            let peaks = tc.draw(
                gs::vecs(gs::tuples!(gs::floats::<f32>().min_value(0.0).max_value(1.0), gs::just(0.0f32))).max_size(n),
            );
            let tint = tc.draw(gs::optional(gs::tuples!(
                gs::just([255u8, 0, 0]),
                gs::floats::<f32>().min_value(0.0).max_value(1.0)
            )));
            Spectrum { bands: &bands, peaks: &peaks, mirror, tint, palette }.render(area, &mut buf);
        }
        5 => {
            let samples = tc.draw(gs::vecs(gs::floats::<f32>().min_value(-1.0).max_value(1.0)).max_size(400));
            let tint = tc.draw(gs::optional(gs::tuples!(
                gs::just([0u8, 255, 0]),
                gs::floats::<f32>().min_value(0.0).max_value(1.0)
            )));
            Scope { samples: &samples, tint, palette }.render(area, &mut buf);
        }
        _ => {
            let px =
                vinyl(width, height, tc.draw(gs::floats::<f32>().min_value(0.0).max_value(7.0)), rgb(&tc), rgb(&tc));
            Pixels { width, pixels: &px, palette }.render(area, &mut buf);
        }
    }
    for y in 0..whole.height {
        for x in 0..whole.width {
            if !area.contains((x, y).into()) {
                assert_eq!(buf[(x, y)], Cell::default(), "({x}, {y}) outside {area:?}");
            }
        }
    }
}

/// The overview shows where the track is played to: the playhead's column is the playhead's
/// colour (or its line where the waveform is empty), and with no waveform a rail ends in a dot
/// there.
#[hegel::test(test_cases = 300)]
fn the_overview_marks_the_playhead(tc: TestCase) {
    let area = Rect::new(
        0,
        0,
        tc.draw(gs::integers::<u16>().min_value(4).max_value(120)),
        tc.draw(gs::integers::<u16>().min_value(2).max_value(10)),
    );
    let progress = tc.draw(gs::floats::<f32>().min_value(0.0).max_value(1.0));
    let w = wave(&tc, 500);
    let with_wave = !w.columns.is_empty();
    let mut buf = Buffer::empty(area);
    let p = Palette::default();
    Overview { wave: Some(&w), progress, cues: &[], duration_ms: 0.0, palette: p }.render(area, &mut buf);
    let head = ((progress as f64 * area.width as f64) as u16).min(area.width - 1);
    if with_wave {
        for y in 1..area.height {
            let cell = &buf[(head, y)];
            let line = cell.symbol() == "│" && cell.fg == (p.paint)(p.fg);
            assert!(line || cell.fg == (p.paint)(PLAYHEAD), "row {y} of column {head}: {cell:?}");
        }
    } else {
        let y = 1 + (area.height - 1) / 2;
        assert_eq!(buf[(head, y)].symbol(), "●");
        assert!(
            (0..head).all(|x| buf[(x, y)].symbol() == "━")
                && (head + 1..area.width).all(|x| buf[(x, y)].symbol() == "─")
        );
    }
}

/// A cue is marked in the overview's top row where it falls in the track: a hot cue by its
/// letter, a memory cue by a triangle.
#[hegel::test(test_cases = 300)]
fn the_overview_marks_a_cue_where_it_falls(tc: TestCase) {
    let area = Rect::new(0, 0, tc.draw(gs::integers::<u16>().min_value(4).max_value(120)), 3);
    let duration_ms = tc.draw(gs::floats::<f64>().min_value(1000.0).max_value(600_000.0));
    let c = Cue { end_ms: None, ..cue(&tc, duration_ms as u32 - 1) };
    let mut buf = Buffer::empty(area);
    let cues = [c.clone()];
    Overview { wave: None, progress: 0.0, cues: &cues, duration_ms, palette: Palette::default() }
        .render(area, &mut buf);
    let x = (c.start_ms as f64 / duration_ms * area.width as f64) as u16;
    let mark = cue_letter(&c).map_or("▼".to_string(), |l| l.to_string());
    assert_eq!(buf[(x, 0)].symbol(), mark);
    assert_eq!(cue_letter(&c), c.hot_cue.filter(|h| *h < 26).map(|h| char::from(b'A' + h)));
}

/// The detail view keeps the playhead in its middle, whatever is around it.
#[hegel::test(test_cases = 200)]
fn the_detail_view_centres_the_playhead(tc: TestCase) {
    let area = Rect::new(
        0,
        0,
        tc.draw(gs::integers::<u16>().min_value(8).max_value(120)),
        tc.draw(gs::integers::<u16>().min_value(3).max_value(10)),
    );
    let w = wave(&tc, 3000);
    let cues = cues(&tc, 20_000);
    let position_secs = tc.draw(gs::floats::<f64>().min_value(0.0).max_value(20.0));
    let mut buf = Buffer::empty(area);
    let p = Palette::default();
    Detail { wave: &w, position_secs, visible_secs: 8.0, beats: &[], cues: &cues, palette: p }.render(area, &mut buf);
    assert_eq!(buf[(area.width / 2, 0)].symbol(), "▼");
    assert_eq!(buf[(area.width / 2, 0)].fg, (p.paint)(p.fg));
}

/// Bars rise with their level, from the bottom: none for silence, the full height at full scale,
/// and a louder band never lower than a quieter one.
#[hegel::test(test_cases = 300)]
fn spectrum_bars_rise_with_their_level(tc: TestCase) {
    let area = Rect::new(
        0,
        0,
        tc.draw(gs::integers::<u16>().min_value(1).max_value(120)),
        tc.draw(gs::integers::<u16>().min_value(1).max_value(12)),
    );
    let (n, bar, gap) = bar_layout(area.width, false);
    let levels = tc.draw(gs::vecs(gs::sampled_from(vec![0.0f32, 0.1, 0.33, 0.5, 0.77, 1.0])).min_size(n).max_size(n));
    let mut buf = Buffer::empty(area);
    Spectrum { bands: &levels, peaks: &[], mirror: false, tint: None, palette: Palette::default() }
        .render(area, &mut buf);
    let used = n as u16 * (bar + gap) - gap;
    let left = area.width.saturating_sub(used) / 2;
    let filled: Vec<u16> = (0..n as u16)
        .map(|i| (0..area.height).filter(|&y| buf[(left + i * (bar + gap), y)].symbol() != " ").count() as u16)
        .collect();
    for (i, (&level, &rows)) in levels.iter().zip(&filled).enumerate() {
        match level {
            0.0 => assert_eq!(rows, 0, "band {i}"),
            1.0 => assert_eq!(rows, area.height, "band {i}"),
            _ => {}
        }
        // Filled from the bottom: no gap under a filled cell.
        let x = left + i as u16 * (bar + gap);
        assert!((area.height - rows..area.height).all(|y| buf[(x, y)].symbol() != " "), "band {i}");
    }
    for (a, b) in levels.iter().zip(&filled).flat_map(|a| levels.iter().zip(&filled).map(move |b| (a, b))) {
        assert!(a.0 > b.0 || a.1 <= b.1, "{a:?} {b:?}");
    }
}

/// As many bars fit as the width takes, and one more wouldn't.
#[hegel::test(test_cases = 300)]
fn bars_fill_the_width(tc: TestCase) {
    let width = tc.draw(gs::integers::<u16>().min_value(2).max_value(400));
    let mirror = tc.draw(gs::booleans());
    let (n, bar, gap) = bar_layout(width, mirror);
    let bars = if mirror { 2 * n } else { n } as u16;
    let span = |bars: u16| bars * (bar + gap) - gap;
    assert!(n >= 1 && span(bars) <= width, "{n} bars of {bar} and gaps of {gap} in {width}");
    let more = if mirror { bars + 2 } else { bars + 1 };
    assert!(span(more) > width);
}

/// Mixing stays between the two colours and reaches each at its end; scaling darkens below 1
/// and brightens above; a gradient passes through each of its stops.
#[hegel::test(test_cases = 500)]
fn colours_mix_between_their_ends(tc: TestCase) {
    let (a, b) = (rgb(&tc), rgb(&tc));
    let t = tc.draw(gs::floats::<f32>().min_value(-1.0).max_value(2.0));
    let m = mix(a, b, t);
    assert!((0..3).all(|i| a[i].min(b[i]) <= m[i] && m[i] <= a[i].max(b[i])), "{m:?}");
    assert_eq!(mix(a, b, t.min(0.0)), a);
    assert_eq!(mix(a, b, t.max(1.0)), b);
    let f = tc.draw(gs::floats::<f32>().min_value(0.0).max_value(3.0));
    let s = scale(a, f);
    assert!((0..3).all(|i| if f <= 1.0 { s[i] <= a[i] } else { s[i] >= a[i] }), "{a:?} * {f} = {s:?}");
    let stops = tc.draw(gs::vecs(gs::arrays::<_, _, 3>(gs::integers::<u8>())).min_size(2).max_size(6));
    for (k, stop) in stops.iter().enumerate() {
        assert_eq!(gradient(&stops, k as f32 / (stops.len() - 1) as f32), *stop, "stop {k} of {stops:?}");
    }
}

/// A cue is called by its comment, and a loop by its length too: as rekordbox stored it, or in
/// beats at the tempo.
#[hegel::test(test_cases = 500)]
fn cue_labels_name_loops_by_their_length(tc: TestCase) {
    let c = cue(&tc, 600_000);
    let bpm = tc.draw(gs::floats::<f64>().min_value(0.0).max_value(200.0));
    let label = cue_label(&c, bpm);
    let name = c.comment.trim();
    assert!(label.starts_with(name), "{label:?} for {name:?}");
    let rest = label[name.len()..].trim_start();
    match (c.end_ms, c.loop_beats) {
        (None, _) => assert_eq!(label, name),
        (Some(_), Some((n, d))) if n % d == 0 => assert_eq!(rest, format!("↻{}", n / d)),
        (Some(_), Some((n, d))) => assert_eq!(rest, format!("↻{n}/{d}")),
        (Some(end), None) if bpm > 0.0 => {
            let beats = (end - c.start_ms) as f64 / 60_000.0 * bpm;
            let shown: f64 = match rest.strip_prefix("↻1/") {
                Some(over) => 1.0 / over.parse::<f64>().unwrap(),
                None => rest.strip_prefix('↻').unwrap().parse().unwrap(),
            };
            assert!(shown == beats.round() || beats < 0.9, "{rest} for {beats} beats");
        }
        (Some(_), None) => assert_eq!(rest, "↻"),
    }
}

/// The record is round: the spindle hole and the corners are clear, every other pixel is
/// drawn, and it looks the same turned half way round, but for the light on it.
#[hegel::test(test_cases = 200)]
fn the_record_is_round(tc: TestCase) {
    let (cols, rows) = (tc.draw(gs::integers::<u16>().max_value(60)), tc.draw(gs::integers::<u16>().max_value(30)));
    let px = vinyl(cols, rows, tc.draw(gs::floats::<f32>().min_value(0.0).max_value(7.0)), rgb(&tc), rgb(&tc));
    let (w, h) = (cols as usize, rows as usize * 2);
    assert_eq!(px.len(), w * h);
    if w >= 4 && h >= 4 {
        assert_eq!(px[0], None);
        assert_eq!(px[w * h - 1], None);
    }
    for y in 0..h {
        for x in 0..w {
            assert_eq!(px[y * w + x].is_some(), px[(h - 1 - y) * w + (w - 1 - x)].is_some(), "({x}, {y})");
        }
    }
    // Drawn as half blocks, two pixel rows to a cell.
    let area = Rect::new(0, 0, cols, rows);
    let mut buf = Buffer::empty(area);
    Pixels { width: cols, pixels: &px, palette: Palette::default() }.render(area, &mut buf);
    for cy in 0..rows {
        for cx in 0..cols {
            let (top, bottom) = (px[cy as usize * 2 * w + cx as usize], px[(cy as usize * 2 + 1) * w + cx as usize]);
            let symbol = match (top, bottom) {
                (Some(_), _) => "▀",
                (None, Some(_)) => "▄",
                (None, None) => " ",
            };
            assert_eq!(buf[(cx, cy)].symbol(), symbol);
            if let Some([r, g, b]) = top {
                assert_eq!(buf[(cx, cy)].fg, Color::Rgb(r, g, b));
            }
        }
    }
}

/// Cover art in any of its formats is drawn at any cell size from its own colours: a picture of
/// one colour stays that colour at every size, the same each time it's asked for. Bytes that
/// aren't a picture are none.
#[cfg(feature = "image")]
#[hegel::test(test_cases = 60)]
fn art_is_resampled_to_any_size(tc: TestCase) {
    use loclib_waveform::Art;
    let colour = rgb(&tc);
    let (w, h) = (
        tc.draw(gs::integers::<u32>().min_value(1).max_value(24)),
        tc.draw(gs::integers::<u32>().min_value(1).max_value(24)),
    );
    let format =
        tc.draw(gs::sampled_from(vec![image::ImageFormat::Png, image::ImageFormat::Bmp, image::ImageFormat::Jpeg]));
    let mut bytes = std::io::Cursor::new(Vec::new());
    image::RgbImage::from_pixel(w, h, image::Rgb(colour)).write_to(&mut bytes, format).unwrap();
    let mut art = Art::decode(bytes.get_ref()).unwrap();
    let (cols, rows) = (
        tc.draw(gs::integers::<u16>().min_value(1).max_value(30)),
        tc.draw(gs::integers::<u16>().min_value(1).max_value(15)),
    );
    let first = art.pixels(cols, rows).to_vec();
    assert_eq!(first.len(), cols as usize * rows as usize * 2);
    // JPEG's compression moves a flat colour a little.
    let slack = if format == image::ImageFormat::Jpeg { 12 } else { 1 };
    assert!(
        first.iter().all(|p| p.is_some_and(|c| (0..3).all(|i| c[i].abs_diff(colour[i]) <= slack))),
        "{colour:?}: {first:?}"
    );
    let _ = art.pixels(cols + 1, rows);
    assert_eq!(art.pixels(cols, rows), first.as_slice());
    let junk = tc.draw(gs::binary().max_size(64));
    assert!(Art::decode(&junk).is_none());
}

/// A loop is underlined along its length in the overview's top row, after its flag; in the
/// cue lane every cue in view is marked where it falls, a hot cue by its letter and a memory
/// cue by a diamond, and a lane under 8 columns wide draws nothing.
#[hegel::test(test_cases = 300)]
fn cues_are_marked_along_their_length(tc: TestCase) {
    let width = tc.draw(gs::integers::<u16>().min_value(8).max_value(120));
    let duration_ms = tc.draw(gs::floats::<f64>().min_value(1000.0).max_value(600_000.0));
    let c = cue(&tc, duration_ms as u32 - 1);
    let cues = [c.clone()];
    let area = Rect::new(0, 0, width, 3);
    let mut buf = Buffer::empty(area);
    Overview { wave: None, progress: 0.0, cues: &cues, duration_ms, palette: palette(&tc) }.render(area, &mut buf);
    let x_of = |ms: u32| (ms as f64 / duration_ms * width as f64) as u16;
    if let Some(end) = c.end_ms {
        for x in x_of(c.start_ms) + 1..=x_of(end).min(width - 1) {
            assert_eq!(buf[(x, 0)].symbol(), "▁", "column {x} of a loop {}..{end} ms", c.start_ms);
        }
    }

    let visible_secs = tc.draw(gs::floats::<f64>().min_value(1.0).max_value(60.0));
    // With the playhead where the cue falls in the middle of any column.
    let column = tc.draw(gs::integers::<u16>().max_value(width - 1));
    let secs_per_col = visible_secs / width as f64;
    let position_secs = c.start_ms as f64 / 1000.0 - (column as f64 + 0.5 - width as f64 / 2.0) * secs_per_col;
    // Names go underneath, when there's room.
    let area = Rect { height: tc.draw(gs::integers::<u16>().min_value(1).max_value(3)), ..area };
    let mut buf = Buffer::empty(area);
    CueLane { cues: &cues, position_secs, visible_secs, bpm: 120.0, palette: palette(&tc) }.render(area, &mut buf);
    let mark = cue_letter(&c).map_or("◆".to_string(), |l| l.to_string());
    assert_eq!(buf[(column, 0)].symbol(), mark);
    // Under 8 columns there's no room for a lane at all.
    let cramped = Rect { width: tc.draw(gs::integers::<u16>().max_value(7)), ..area };
    let mut buf = Buffer::empty(area);
    CueLane { cues: &cues, position_secs, visible_secs, bpm: 120.0, palette: palette(&tc) }.render(cramped, &mut buf);
    assert_eq!(buf, Buffer::empty(area));
}

/// Drawn into a buffer narrower than its area (a terminal that shrank before the next layout),
/// a widget draws what fits and nothing more: the columns there are as it draws them where the
/// whole area fits.
#[hegel::test(test_cases = 300)]
fn widgets_draw_what_fits_in_a_narrower_buffer(tc: TestCase) {
    let area = Rect::new(
        0,
        0,
        tc.draw(gs::integers::<u16>().min_value(8).max_value(90)),
        tc.draw(gs::integers::<u16>().min_value(3).max_value(10)),
    );
    let narrow = Rect { width: tc.draw(gs::integers::<u16>().min_value(1).max_value(area.width - 1)), ..area };
    let p = palette(&tc);
    let (w, cues) = (wave(&tc, 2000), cues(&tc, 20_000));
    let duration_ms = tc.draw(gs::floats::<f64>().min_value(1000.0).max_value(20_000.0));
    let position_secs = tc.draw(gs::floats::<f64>().min_value(0.0).max_value(20.0));
    let progress = tc.draw(gs::floats::<f32>().min_value(0.0).max_value(1.0));
    let phrases: Vec<(f64, f64, String, Rgb)> = vec![
        (0.0, duration_ms / 3.0, "Intro".into(), [90, 160, 80]),
        (duration_ms / 3.0, duration_ms, "Chorus".into(), rgb(&tc)),
    ];
    let px = vinyl(area.width, area.height, 1.0, rgb(&tc), rgb(&tc));
    let kind = tc.draw(gs::integers::<u8>().max_value(4));
    let draw = |buf: &mut Buffer| match kind {
        0 => Overview { wave: Some(&w), progress, cues: &cues, duration_ms, palette: p }.render(area, buf),
        1 => {
            Detail { wave: &w, position_secs, visible_secs: 8.0, beats: &[], cues: &cues, palette: p }.render(area, buf)
        }
        2 => CueLane { cues: &cues, position_secs, visible_secs: 8.0, bpm: 128.0, palette: p }.render(area, buf),
        3 => PhraseStrip { phrases: &phrases, duration_ms, progress, palette: p }.render(area, buf),
        _ => Pixels { width: area.width, pixels: &px, palette: p }.render(area, buf),
    };
    let (mut whole, mut cut) = (Buffer::empty(area), Buffer::empty(narrow));
    draw(&mut whole);
    draw(&mut cut);
    for y in 0..area.height {
        for x in 0..narrow.width {
            assert_eq!(cut[(x, y)], whole[(x, y)], "({x}, {y}) of {narrow:?} drawing {kind}");
        }
    }
}

/// In the detail view the playhead crosses silence as a line through the middle.
#[hegel::test(test_cases = 100)]
fn the_playhead_crosses_silence_as_a_line(tc: TestCase) {
    let area = Rect::new(
        0,
        0,
        tc.draw(gs::integers::<u16>().min_value(8).max_value(120)),
        tc.draw(gs::integers::<u16>().min_value(3).max_value(10)),
    );
    // 16 to 20 s, and the view inside it.
    let columns = tc.draw(gs::integers::<usize>().min_value(2400).max_value(3000));
    let silent = WaveColumn {
        back: WaveLayer { height: 0.0, color: [200, 50, 50] },
        front: WaveLayer { height: 0.0, color: [255, 255, 255] },
    };
    let w = Waveform { style: WaveformStyle::Rgb, columns_per_sec: Some(150.0), columns: vec![silent; columns] };
    let position_secs = tc.draw(gs::floats::<f64>().min_value(5.0).max_value(columns as f64 / 150.0 - 5.0));
    let mut buf = Buffer::empty(area);
    let p = Palette::default();
    Detail { wave: &w, position_secs, visible_secs: 8.0, beats: &[], cues: &[], palette: p }.render(area, &mut buf);
    let (centre, wave_rows) = (area.width / 2, area.height - 1);
    let cell = &buf[(centre, 1 + wave_rows / 2)];
    assert_eq!((cell.symbol(), cell.fg), ("│", (p.paint)(p.fg)));
}

/// The phrase strip shows each phrase in its colour, dimmed where it's been played, and the
/// playhead over it.
#[hegel::test(test_cases = 200)]
fn phrases_dim_once_played(tc: TestCase) {
    let width = tc.draw(gs::integers::<u16>().min_value(8).max_value(120));
    let area = Rect::new(0, 0, width, 1);
    let duration_ms = 60_000.0;
    let mut cuts = tc.draw(gs::vecs(gs::floats::<f64>().min_value(0.0).max_value(duration_ms)).max_size(4));
    cuts.sort_by(f64::total_cmp);
    let ends: Vec<f64> = std::iter::once(0.0).chain(cuts).chain([duration_ms]).collect();
    let phrases: Vec<(f64, f64, String, Rgb)> =
        ends.windows(2).map(|e| (e[0], e[1], String::new(), rgb(&tc))).collect();
    let progress = tc.draw(gs::floats::<f32>().min_value(0.0).max_value(1.0));
    let p = Palette::default();
    let mut buf = Buffer::empty(area);
    PhraseStrip { phrases: &phrases, duration_ms, progress, palette: p }.render(area, &mut buf);
    let head = ((progress as f64 * width as f64) as u16).min(width - 1);
    let x_of = |ms: f64| ((ms / duration_ms) * width as f64).round().clamp(0.0, width as f64) as u16;
    assert_eq!(buf[(head, 0)].symbol(), "┃");
    // The last phrase drawn over a cell is the one it shows.
    for x in 0..width {
        let Some((start, _, _, color)) =
            phrases.iter().rev().find(|(s, e, _, _)| (x_of(*s)..x_of(*e).max(x_of(*s) + 1)).contains(&x))
        else {
            continue;
        };
        let played = if x < head { scale(*color, 0.45) } else { *color };
        let shown = if x == x_of(*start) { scale(played, 0.7) } else { played };
        assert_eq!(buf[(x, 0)].bg, (p.paint)(shown), "column {x}");
    }
}

/// Given more bands than fit, the spectrum draws the bars that do, from the left edge, and
/// leaves the rest out: at full level each bar that fits is filled top to bottom, and the gaps
/// between them stay empty.
#[hegel::test(test_cases = 200)]
fn spectra_draw_the_bars_that_fit(tc: TestCase) {
    let area = Rect::new(
        0,
        0,
        tc.draw(gs::integers::<u16>().min_value(1).max_value(120)),
        tc.draw(gs::integers::<u16>().min_value(1).max_value(8)),
    );
    let mirror = tc.draw(gs::booleans());
    let (n, bar, gap) = bar_layout(area.width, mirror);
    let bands = vec![1.0; n + tc.draw(gs::integers::<usize>().min_value(1).max_value(40))];
    let mut buf = Buffer::empty(area);
    Spectrum { bands: &bands, peaks: &[], mirror, tint: None, palette: Palette::default() }.render(area, &mut buf);
    for x in 0..area.width {
        let in_bar = x % (bar + gap) < bar && x / (bar + gap) * (bar + gap) + bar <= area.width;
        let filled = (0..area.height).filter(|&y| buf[(x, y)].symbol() == "█").count() as u16;
        assert_eq!(
            filled,
            if in_bar { area.height } else { 0 },
            "column {x} of {area:?}, bars of {bar} and gaps of {gap}"
        );
    }
}

/// Silence on the scope is a flat line across the middle: one row, the whole width.
#[hegel::test(test_cases = 100)]
fn the_scope_draws_silence_flat(tc: TestCase) {
    let area = Rect::new(
        0,
        0,
        tc.draw(gs::integers::<u16>().min_value(2).max_value(80)),
        tc.draw(gs::integers::<u16>().min_value(1).max_value(8)),
    );
    let samples = vec![0.0f32; tc.draw(gs::integers::<usize>().min_value(2).max_value(400))];
    let mut buf = Buffer::empty(area);
    Scope { samples: &samples, tint: None, palette: Palette::default() }.render(area, &mut buf);
    let rows: Vec<u16> = (0..area.height).filter(|&y| (0..area.width).any(|x| buf[(x, y)].symbol() != " ")).collect();
    assert_eq!(rows.len(), 1, "{rows:?} of {area:?}");
    let drawn = (0..area.width).filter(|&x| buf[(x, rows[0])].symbol() != " ").count() as u16;
    assert!(drawn >= area.width - 1, "{drawn} of {} columns", area.width);
}

/// Cover art is resampled once per cell size: asking again at the size it was last drawn at
/// hands back the same pixels, without resampling them again.
#[cfg(feature = "image")]
#[hegel::test(test_cases = 30)]
fn art_is_resampled_once_per_size(tc: TestCase) {
    use loclib_waveform::Art;
    let side = tc.draw(gs::integers::<u32>().min_value(1).max_value(24));
    let mut bytes = std::io::Cursor::new(Vec::new());
    image::RgbImage::from_fn(side, side, |x, y| image::Rgb([x as u8 * 9, y as u8 * 9, 7]))
        .write_to(&mut bytes, image::ImageFormat::Png)
        .unwrap();
    let mut art = Art::decode(bytes.get_ref()).unwrap();
    let (cols, rows) = (
        tc.draw(gs::integers::<u16>().min_value(1).max_value(30)),
        tc.draw(gs::integers::<u16>().min_value(1).max_value(15)),
    );
    let first = art.pixels(cols, rows);
    let (at, copy) = (first.as_ptr(), first.to_vec());
    let again = art.pixels(cols, rows);
    assert_eq!(again.as_ptr(), at, "resampled again");
    assert_eq!(again, copy.as_slice());
}
