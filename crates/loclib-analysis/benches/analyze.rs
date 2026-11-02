//! Analysing a minute of a 16-bit stereo WAV: at 44.1 kHz (the analyzer's own rate) and at 48 kHz
//! (resampled for it), each measurement alone and all three from one decode.

use std::f64::consts::PI;
use std::hint::black_box;
use std::path::Path;

use criterion::{Criterion, criterion_group, criterion_main};
use loclib_analysis::{Want, analyze};
use loclib_mixid::test_util::wav_of;
use loclib_progress::Ctx;

const SECS: f64 = 60.0;

/// A kick drum on every beat at 128 BPM over a sustained A minor chord, at `rate`.
fn kick_track(rate: u32) -> Vec<i32> {
    let rate = f64::from(rate);
    let beat = 60.0 / 128.0;
    (0..(rate * SECS) as usize)
        .map(|i| {
            let t = i as f64 / rate;
            let since = t % beat;
            let kick = 0.6 * (-since * 25.0).exp() * (2.0 * PI * (50.0 + 100.0 * (-since * 40.0).exp()) * since).sin();
            let chord: f64 = [220.0, 261.63, 329.63].iter().map(|f| 0.1 * (2.0 * PI * f * t).sin()).sum();
            ((kick + chord).clamp(-1.0, 1.0) * 32_767.0).round() as i32
        })
        .collect()
}

fn bench(c: &mut Criterion) {
    let dir = tempfile::tempdir().unwrap();
    let mut group = c.benchmark_group("analyze");
    group.sample_size(10);
    for rate in [44_100, 48_000] {
        let path = dir.path().join(format!("{rate}.wav"));
        std::fs::write(&path, wav_of(rate, 16, kick_track(rate).into_iter())).unwrap();
        let wants = [
            ("estimate", Want { estimate: true, loudness: false, waveform: false }),
            ("loudness", Want { estimate: false, loudness: true, waveform: false }),
            ("waveform", Want { estimate: false, loudness: false, waveform: true }),
            ("all", Want::ALL),
        ];
        for (name, want) in wants {
            group.bench_function(format!("{name}_{rate}"), |b| {
                b.iter(|| analyze(black_box(Path::new(&path)), want, &Ctx::silent()).unwrap())
            });
        }
    }
    group.finish();
}

criterion_group!(benches, bench);
criterion_main!(benches);
