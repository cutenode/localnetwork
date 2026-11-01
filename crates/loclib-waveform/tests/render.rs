use loclib_model::TrackColor;
use loclib_model::analysis::{Beat, Cue, WaveColumn, WaveLayer, Waveform, WaveformStyle};
use loclib_waveform::color::scale;
use loclib_waveform::{CueLane, Detail, Overview, Palette, PhraseStrip, Pixels, Scope, Spectrum, cue_label, vinyl};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier};
use ratatui::widgets::Widget;

const RED: [u8; 3] = [255, 100, 100];

fn rendered(buf: &Buffer) -> String {
    buf.content.iter().map(|c| c.symbol()).collect()
}

/// A waveform of `n` columns at 150 a second, its back layer rising and falling every ten.
fn wave(n: usize) -> Waveform {
    let columns = (0..n)
        .map(|i| WaveColumn {
            back: WaveLayer { height: (i % 10) as f32 / 10.0, color: [200, 50, 50] },
            front: WaveLayer { height: 0.0, color: [255, 255, 255] },
        })
        .collect();
    Waveform { style: WaveformStyle::Rgb, columns_per_sec: Some(150.0), columns }
}

/// A hot cue (`hot` is its slot) or memory cue, a loop if it has an end, named `comment`.
fn cue(hot: Option<u8>, start_ms: u32, end_ms: Option<u32>, comment: &str) -> Cue {
    Cue {
        hot_cue: hot,
        start_ms,
        end_ms,
        comment: comment.into(),
        color: None,
        color_id: 0,
        loop_beats: None,
        active_loop: false,
    }
}

#[test]
fn spectrum_fills_from_the_bottom() {
    let area = Rect::new(0, 0, 10, 4);
    let mut buf = Buffer::empty(area);
    Spectrum {
        bands: &[1.0, 0.5, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
        peaks: &[],
        mirror: false,
        tint: None,
        palette: Palette::default(),
    }
    .render(area, &mut buf);
    // Full bar in column 0, half bar in column 1.
    assert_eq!(buf[(0, 0)].symbol(), "█");
    assert_eq!(buf[(0, 3)].symbol(), "█");
    assert_eq!(buf[(1, 0)].symbol(), " ");
    assert_eq!(buf[(1, 3)].symbol(), "█");
    assert_eq!(buf[(2, 3)].symbol(), " ");
}

#[test]
fn waveforms_render_without_panicking_at_any_size() {
    let w = wave(1500);
    let cues = [cue(Some(0), 1000, None, "")];
    let beats = [Beat { beat_number: 1, bpm: 128.0, time_ms: 500 }];
    let palette = Palette::default();
    for (width, height) in [(1, 1), (4, 2), (80, 4), (200, 12)] {
        let area = Rect::new(2, 1, width, height);
        let mut buf = Buffer::empty(Rect::new(0, 0, width + 4, height + 2));
        Overview { wave: Some(&w), progress: 0.5, cues: &cues, duration_ms: 10_000.0, palette }.render(area, &mut buf);
        Overview { wave: None, progress: 1.0, cues: &cues, duration_ms: 0.0, palette }.render(area, &mut buf);
        Detail { wave: &w, position_secs: 0.2, visible_secs: 8.0, beats: &beats, cues: &cues, palette }
            .render(area, &mut buf);
        Detail { wave: &w, position_secs: 9999.0, visible_secs: 8.0, beats: &beats, cues: &cues, palette }
            .render(area, &mut buf);
        let px = vinyl(width, height, 1.0, [255, 0, 0], [40, 40, 52]);
        Pixels { width, pixels: &px, palette }.render(area, &mut buf);
    }
}

#[test]
fn overview_marks_hot_cues_with_letters() {
    let w = wave(100);
    let cues = [cue(Some(1), 5000, None, "")];
    let area = Rect::new(0, 0, 20, 3);
    let mut buf = Buffer::empty(area);
    Overview { wave: Some(&w), progress: 0.0, cues: &cues, duration_ms: 10_000.0, palette: Palette::default() }
        .render(area, &mut buf);
    assert_eq!(buf[(10, 0)].symbol(), "B");
    assert!(rendered(&buf).contains('█'));
}

#[test]
fn cue_labels_name_loops_by_length() {
    assert_eq!(cue_label(&cue(Some(1), 70_764, Some(74_574), "loop in"), 126.0), "loop in ↻8");
    assert_eq!(cue_label(&cue(None, 0, Some(238), ""), 126.0), "↻1/2");
    assert_eq!(cue_label(&cue(None, 288, None, "1.1Bars"), 126.0), "1.1Bars");
    assert_eq!(cue_label(&cue(Some(0), 288, None, ""), 126.0), "");
    // The length rekordbox stored wins over the tempo (wrong where the grid changes speed).
    let stored = |beats| Cue { loop_beats: Some(beats), ..cue(None, 0, Some(1000), "") };
    assert_eq!((cue_label(&stored((16, 1)), 126.0), cue_label(&stored((3, 2)), 126.0)), ("↻16".into(), "↻3/2".into()));
}

#[test]
fn memory_cues_take_their_colour_from_the_table() {
    let p = Palette::default();
    let red = Cue { color_id: 2, ..cue(None, 0, None, "") };
    assert_eq!(p.cue_color(&red), p.track_color(TrackColor::Red));
    assert_eq!(p.cue_color(&cue(None, 0, None, "")), p.memory_cue);
    let hot = Cue { color: Some([1, 2, 3]), color_id: 2, ..cue(Some(0), 0, None, "") };
    assert_eq!(p.cue_color(&hot), [1, 2, 3], "a hot cue's own colour first");
}

#[test]
fn cue_lane_places_cues_around_the_playhead() {
    // 20 s across 40 columns, playhead at 10 s in column 20.
    let cues = [
        cue(Some(0), 2_000, None, "Drop"),
        cue(None, 15_000, None, ""),
        cue(None, 16_000, Some(18_000), ""),
        cue(Some(1), 30_000, None, "out of view"),
    ];
    let area = Rect::new(0, 0, 40, 2);
    let mut buf = Buffer::empty(area);
    CueLane { cues: &cues, position_secs: 10.0, visible_secs: 20.0, bpm: 120.0, palette: Palette::default() }
        .render(area, &mut buf);
    assert_eq!(buf[(4, 0)].symbol(), "A");
    assert_eq!(buf[(20, 0)].symbol(), "┃");
    assert_eq!(buf[(30, 0)].symbol(), "◆");
    assert_eq!(buf[(32, 0)].symbol(), "◆");
    assert_eq!(buf[(34, 0)].symbol(), "━");
    assert_eq!(buf[(37, 0)].symbol(), "─");
    let labels: String = (0..40).map(|x| buf[(x, 1)].symbol().to_string()).collect();
    assert!(labels.starts_with("    Drop"), "{labels:?}");
    assert!(labels.contains("↻4"), "{labels:?}");
    assert!(!rendered(&buf).contains('B'));
}

#[test]
fn deck_draws_cue_lines_and_shades_loops() {
    let w = wave(150 * 20);
    let cues = [cue(Some(2), 9_000, None, "Vox"), cue(None, 11_000, Some(13_000), "")];
    let area = Rect::new(0, 0, 40, 6);
    let mut buf = Buffer::empty(area);
    let p = Palette::default();
    Detail { wave: &w, position_secs: 10.0, visible_secs: 8.0, beats: &[], cues: &cues, palette: p }
        .render(area, &mut buf);
    // 0.2 s per column from 6 s: the hot cue at 9 s is column 15, the loop spans 25-35.
    assert_eq!(buf[(15, 0)].symbol(), "C");
    assert_eq!(buf[(16, 0)].symbol(), "V");
    let cue_fg = (p.paint)(p.hot_cue);
    assert!((1..6).all(|y| buf[(15, y)].fg == cue_fg));
    let wash = (p.paint)(scale(p.loop_cue, 0.22));
    assert!((1..6).all(|y| buf[(30, y)].bg == wash));
    assert_ne!(buf[(20, 3)].bg, wash);
    assert_eq!(buf[(25, 5)].symbol(), "▲");
}

/// A band that has fallen leaves its peak cap behind for a while, and a cue just passed
/// tints the bars.
#[test]
fn spectrum_peaks_hang_above_the_bars() {
    let area = Rect::new(0, 0, 1, 4);
    let draw = |tint| {
        let mut buf = Buffer::empty(area);
        Spectrum { bands: &[0.25], peaks: &[(0.75, 0.0)], mirror: false, tint, palette: Palette::default() }
            .render(area, &mut buf);
        buf
    };
    let buf = draw(None);
    let column: Vec<&str> = (0..4).map(|y| buf[(0, y)].symbol()).collect();
    assert_eq!(column, ["▁", " ", " ", "█"], "the cap three quarters up, the bar a quarter");
    assert_ne!(draw(Some((RED, 1.0)))[(0, 3)].fg, buf[(0, 3)].fg);
}

/// A cue just passed tints the scope too.
#[test]
fn scope_is_tinted_by_a_cue_just_passed() {
    let area = Rect::new(0, 0, 10, 4);
    let samples: Vec<f32> = (0..40).map(|i| (i as f32 / 3.0).sin()).collect();
    let draw = |tint| {
        let mut buf = Buffer::empty(area);
        Scope { samples: &samples, tint, palette: Palette::default() }.render(area, &mut buf);
        buf.content.iter().map(|c| c.fg).collect::<Vec<_>>()
    };
    assert_ne!(draw(None), draw(Some((RED, 1.0))));
}

/// Where the front layer ends inside a cell, the cell shows it in front of the back layer.
#[test]
fn overview_layers_the_front_over_the_back() {
    let col = WaveColumn {
        back: WaveLayer { height: 1.0, color: [200, 50, 50] },
        front: WaveLayer { height: 0.3, color: [255, 255, 255] },
    };
    let w = Waveform { style: WaveformStyle::Rgb, columns_per_sec: None, columns: vec![col; 10] };
    let area = Rect::new(0, 0, 10, 3);
    let mut buf = Buffer::empty(area);
    Overview { wave: Some(&w), progress: 0.0, cues: &[], duration_ms: 10_000.0, palette: Palette::default() }
        .render(area, &mut buf);
    // Two rows of waveform: the front fills just over half of the bottom one.
    let (bottom, top) = (&buf[(5, 2)], &buf[(5, 1)]);
    assert_eq!((bottom.symbol(), top.symbol()), ("▅", "█"));
    assert_ne!(bottom.bg, Color::Reset, "the back layer behind it");
}

/// Each phrase is a block of its colour with its name inside, darker once played and in its
/// first cell, with the playhead over them.
#[test]
fn phrase_strip_draws_named_blocks_and_the_playhead() {
    let phrases = [(0.0, 5_000.0, "Intro".to_string(), [70, 112, 232]), (5_000.0, 10_000.0, "Chorus".into(), RED)];
    let area = Rect::new(0, 0, 20, 1);
    let mut buf = Buffer::empty(area);
    PhraseStrip { phrases: &phrases, duration_ms: 10_000.0, progress: 0.25, palette: Palette::default() }
        .render(area, &mut buf);
    assert_eq!(rendered(&buf), " Intr┃     Chorus   ");
    let bg = |x: u16| buf[(x, 0)].bg;
    assert_eq!(bg(2), Color::Rgb(31, 50, 104), "played: at 45%");
    assert_eq!(bg(10), Color::Rgb(178, 70, 70), "a phrase's first cell: at 70%");
    assert_eq!(bg(12), Color::Rgb(255, 100, 100), "still to come: as it is");
    assert_eq!(buf[(5, 0)].fg, Color::Rgb(255, 255, 255));
}

/// The palette's `paint` decides every colour the waveforms draw, and `image` the pictures'.
#[test]
fn palette_paints_every_colour() {
    let palette = Palette { paint: |_| Color::Indexed(9), image: |_| Color::Indexed(4), ..Palette::default() };
    let area = Rect::new(0, 0, 30, 5);
    let mut buf = Buffer::empty(area);
    let cues = [cue(Some(0), 2_000, None, "Drop"), cue(None, 4_000, Some(6_000), "")];
    Overview { wave: Some(&wave(300)), progress: 0.4, cues: &cues, duration_ms: 10_000.0, palette }
        .render(area, &mut buf);
    let painted = |buf: &Buffer, c| buf.content.iter().filter(|cell| cell.symbol() != " ").all(|cell| cell.fg == c);
    assert!(painted(&buf, Color::Indexed(9)), "{buf:?}");
    let mut buf = Buffer::empty(area);
    Pixels { width: 30, pixels: &vinyl(30, 5, 0.0, RED, [40, 40, 52]), palette }.render(area, &mut buf);
    assert!(painted(&buf, Color::Indexed(4)), "{buf:?}");
}

/// Without colours, a hot cue's letter shows in reverse video and pictures aren't drawn.
#[test]
fn plain_palette_reverses_chips_and_leaves_out_pictures() {
    let palette = Palette { paint: |_| Color::Reset, plain: true, ..Palette::default() };
    let area = Rect::new(0, 0, 20, 2);
    let mut buf = Buffer::empty(area);
    CueLane { cues: &[cue(Some(0), 12_000, None, "")], position_secs: 10.0, visible_secs: 20.0, bpm: 0.0, palette }
        .render(area, &mut buf);
    assert_eq!(buf[(12, 0)].symbol(), "A");
    assert!(buf[(12, 0)].modifier.contains(Modifier::REVERSED));
    assert!(!buf[(11, 0)].modifier.contains(Modifier::REVERSED), "only coloured cells");
    let mut buf = Buffer::empty(area);
    Pixels { width: 20, pixels: &vinyl(20, 2, 0.0, RED, [40, 40, 52]), palette }.render(area, &mut buf);
    assert_eq!(rendered(&buf).trim(), "");
}

/// The record: round, with a spindle hole, a label in the middle and the rest grooved vinyl.
#[test]
fn vinyl_is_a_record_with_a_label_and_a_hole() {
    // 40 x 40 pixels: the record's centre is at (20, 20), its radius 20.
    let px = vinyl(40, 20, std::f32::consts::FRAC_PI_2, [0, 0, 255], [40, 40, 52]);
    assert_eq!(px.len(), 40 * 40);
    let at = |x: usize, y: usize| px[y * 40 + x];
    assert_eq!((at(0, 0), at(39, 39)), (None, None), "corners are outside");
    assert_eq!(at(20, 20), None, "the spindle hole");
    assert_eq!(at(24, 20), Some([0, 0, 219]), "the label, lit from the top left");
    let groove = at(32, 20).unwrap();
    assert!(groove == [40, 40, 52] || groove == [31, 31, 40], "vinyl, away from the streak: {groove:?}");
}

#[cfg(feature = "image")]
#[test]
fn art_decodes_and_resamples_to_cells() {
    use loclib_waveform::Art;

    assert!(Art::decode(b"not an image").is_none());
    let mut png = Vec::new();
    image::RgbImage::from_pixel(8, 8, image::Rgb([200, 40, 90]))
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .unwrap();
    let mut art = Art::decode(&png).unwrap();
    assert_eq!(art.pixels(3, 2), [Some([200, 40, 90]); 12], "two pixels a cell, top to bottom");
    assert_eq!(art.pixels(1, 1).len(), 2, "resampled for a new size");
}
