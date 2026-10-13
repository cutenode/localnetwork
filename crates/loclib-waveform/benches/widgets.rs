//! What a frame of the terminal player costs: each widget drawn at the size it gets on a 200x50
//! terminal, with a six minute track's data.

use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};
use loclib_model::analysis::{Beat, Cue, WaveColumn, WaveLayer, Waveform, WaveformStyle};
use loclib_waveform::{CueLane, Detail, Overview, Palette, PhraseStrip, Pixels, Scope, Spectrum, bar_layout, vinyl};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::widgets::Widget;

const SECS: f64 = 360.0;
const BPM: f64 = 128.0;

/// Deterministic pseudo-random numbers, 0-1.
fn noise(seed: u32) -> impl FnMut() -> f32 {
    let mut state = seed.wrapping_mul(2_654_435_761).max(1);
    move || {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        state as f32 / u32::MAX as f32
    }
}

/// `n` columns of a loud, busy track: heights and colours that change every column.
fn wave(n: usize, columns_per_sec: Option<f64>) -> Waveform {
    let mut rand = noise(n as u32);
    let columns = (0..n)
        .map(|_| {
            let back = 0.3 + 0.7 * rand();
            let color = [(rand() * 255.0) as u8, (rand() * 255.0) as u8, 255];
            WaveColumn {
                back: WaveLayer { height: back, color },
                front: WaveLayer { height: back * rand(), color: [255, 255, 255] },
            }
        })
        .collect();
    Waveform { style: WaveformStyle::Rgb, columns_per_sec, columns }
}

fn beats() -> Vec<Beat> {
    let n = (SECS / 60.0 * BPM) as u32;
    (0..n)
        .map(|i| Beat { beat_number: (i % 4 + 1) as u16, bpm: BPM, time_ms: (f64::from(i) * 60_000.0 / BPM) as u32 })
        .collect()
}

/// Eight hot cues, a few memory cues and a loop, one near the playhead.
fn cues() -> Vec<Cue> {
    let cue = |hot_cue, start_ms, end_ms, comment: &str| Cue {
        hot_cue,
        start_ms,
        end_ms,
        comment: comment.into(),
        color: None,
        color_id: 0,
        loop_beats: None,
    };
    let mut cues: Vec<Cue> = (0..8).map(|i| cue(Some(i), 20_000 + u32::from(i) * 40_000, None, "Drop")).collect();
    cues.extend((0..4).map(|i| cue(None, 30_000 + i * 80_000, None, "")));
    cues.push(cue(None, 178_000, Some(181_750), "loop in"));
    cues
}

/// Bench drawing what `widget` makes into a cleared `area`, as a frame does.
fn draw<W: Widget>(c: &mut Criterion, name: &str, area: Rect, widget: impl Fn() -> W) {
    let mut buf = Buffer::empty(area);
    c.bench_function(name, |b| {
        b.iter(|| {
            buf.reset();
            widget().render(black_box(area), &mut buf);
            black_box(&buf);
        })
    });
}

fn bench(c: &mut Criterion) {
    let palette = Palette::default();
    let (preview, detail, beats, cues) =
        (wave(1200, None), wave((SECS * 150.0) as usize, Some(150.0)), beats(), cues());
    let position_secs = 180.0;
    let progress = (position_secs / SECS) as f32;

    draw(c, "overview", Rect::new(0, 0, 200, 6), || Overview {
        wave: Some(&preview),
        progress,
        cues: &cues,
        duration_ms: SECS * 1000.0,
        palette,
    });
    draw(c, "detail", Rect::new(0, 0, 200, 20), || Detail {
        wave: &detail,
        position_secs,
        visible_secs: 8.0,
        beats: &beats,
        cues: &cues,
        palette,
    });
    draw(c, "cue_lane", Rect::new(0, 0, 200, 2), || CueLane {
        cues: &cues,
        position_secs,
        visible_secs: 32.0,
        bpm: BPM,
        palette,
    });
    let phrases: Vec<(f64, f64, String, [u8; 3])> = (0..24)
        .map(|i| (f64::from(i) * 15_000.0, f64::from(i + 1) * 15_000.0, format!("Verse {i}"), [80, 160, 255]))
        .collect();
    draw(c, "phrases", Rect::new(0, 0, 200, 1), || PhraseStrip {
        phrases: &phrases,
        duration_ms: SECS * 1000.0,
        progress,
        palette,
    });
    let spectrum = Rect::new(0, 0, 200, 16);
    let mut rand = noise(7);
    let n = bar_layout(spectrum.width, true).0;
    let bands: Vec<f32> = (0..n).map(|_| rand()).collect();
    let peaks: Vec<(f32, f32)> = bands.iter().map(|b| ((b + 0.1).min(1.0), 0.0)).collect();
    draw(c, "spectrum_mirror", spectrum, || Spectrum {
        bands: &bands,
        peaks: &peaks,
        mirror: true,
        tint: None,
        palette,
    });
    let samples: Vec<f32> = (0..1024).map(|i| (i as f32 * 0.05).sin() * 0.8).collect();
    draw(c, "scope", Rect::new(0, 0, 200, 16), || Scope { samples: &samples, tint: None, palette });
    let art = Rect::new(0, 0, 40, 20);
    c.bench_function("vinyl", |b| {
        b.iter(|| black_box(vinyl(black_box(art.width), art.height, 1.0, [255, 92, 160], [40, 40, 52])))
    });
    let px = vinyl(art.width, art.height, 1.0, [255, 92, 160], [40, 40, 52]);
    draw(c, "pixels", art, || Pixels { width: art.width, pixels: &px, palette });
}

criterion_group!(benches, bench);
criterion_main!(benches);
