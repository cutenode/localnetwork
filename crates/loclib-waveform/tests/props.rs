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
    use hegel::Generator;
    use loclib_waveform::Art;
    let colour = rgb(&tc);
    let (w, h) = (
        tc.draw(gs::integers::<u32>().min_value(1).max_value(24)),
        tc.draw(gs::integers::<u32>().min_value(1).max_value(24)),
    );
    let format = tc.draw(
        gs::sampled_from(vec![image::ImageFormat::Png, image::ImageFormat::Bmp, image::ImageFormat::Jpeg])
            .print_as_debug(),
    );
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
