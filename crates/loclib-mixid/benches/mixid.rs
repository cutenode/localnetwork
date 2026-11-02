//! The steps a library scan and a mix search spend their time in, on synthetic audio:
//! `cargo bench -p loclib-mixid --features test-util`.

use std::hint::black_box;
use std::sync::LazyLock;

use criterion::{Criterion, criterion_group, criterion_main};
use loclib_mixid::fingerprint::{self, Analyzer, LIBRARY, Peak, QUERY, RATE};
use loclib_mixid::test_util::{resample, synth};
use loclib_mixid::{LibraryIndex, TrackPrint, matcher};
use loclib_progress::Ctx;

/// Library size and track length for the index and the search.
const TRACKS: u32 = 200;
const TRACK_SECS: f64 = 120.0;
/// Random prints of 5 min tracks added to the search's index, for a library's worth of entries.
const FILLER: u32 = 1000;

fn print(x: &[f32]) -> TrackPrint {
    let mut a = Analyzer::new(LIBRARY, false);
    a.push(x);
    TrackPrint { duration_secs: x.len() as f32 / RATE as f32, peaks: a.finish().0 }
}

/// `TRACKS` prints, made on every core.
fn library() -> Vec<(u32, TrackPrint)> {
    std::thread::scope(|s| {
        let handles: Vec<_> =
            (0..TRACKS).map(|id| s.spawn(move || (id, print(&synth(TRACK_SECS, (id + 1) * 7919))))).collect();
        handles.into_iter().map(|h| h.join().unwrap()).collect()
    })
}

/// Prints of `n` tracks of `secs` seconds whose peaks are random (about 21 a second, anywhere above
/// the lowest bins): cheap to make, they fill the index as a big library would, so every lookup
/// goes through as many entries as it would there.
fn filler(n: u32, secs: f64) -> Vec<(u32, TrackPrint)> {
    let mut state = 0x9e37_79b9u32;
    let mut rand = move || {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        state
    };
    let frames = (secs * fingerprint::FRAMES_PER_SEC) as u32;
    (0..n)
        .map(|i| {
            let mut peaks = Vec::new();
            let mut t = 0;
            while t < frames {
                peaks.push(Peak { t, f: (3 + rand() % 252) as u8 });
                t += rand() % 5;
            }
            peaks.sort_unstable();
            (TRACKS + i, TrackPrint { duration_secs: secs as f32, peaks })
        })
        .collect()
}

/// A mix of six of the library's tracks, 90 s each at a few speeds (pitched), crossfaded over 10 s.
fn mix() -> (Vec<Peak>, u32) {
    let secs = |s: f64| (s * RATE as f64) as usize;
    let mut out = Vec::new();
    for (i, speed) in [1.0, 1.03, 0.97, 1.05, 1.0, 0.98].into_iter().enumerate() {
        let x = synth(TRACK_SECS, (i as u32 * 7 + 1) * 7919);
        let x = resample(&x[secs(10.0)..], (RATE as f64 * speed).round() as u32, RATE);
        let x = &x[..secs(90.0)];
        let start = out.len().saturating_sub(secs(10.0));
        out.resize(start + x.len(), 0.0);
        for (j, s) in x.iter().enumerate() {
            out[start + j] += s * (j as f32 / secs(10.0) as f32).min(1.0);
        }
    }
    let mut a = Analyzer::new(QUERY, true);
    a.push(&out);
    let frames = a.frames();
    (a.finish().0, frames)
}

/// The library's prints and the mix's peaks and frames, made once and only for the benches that
/// use them.
static PRINTS: LazyLock<Vec<(u32, TrackPrint)>> = LazyLock::new(library);
static MIX: LazyLock<(Vec<Peak>, u32)> = LazyLock::new(mix);

fn signal(c: &mut Criterion) {
    let audio = synth(180.0, 7919);
    let at_44k = resample(&audio[..60 * RATE as usize], RATE, 44_100);
    c.bench_function("analyzer/library_180s", |b| {
        b.iter(|| {
            let mut a = Analyzer::new(LIBRARY, false);
            a.push(black_box(&audio));
            a.finish()
        })
    });
    c.bench_function("analyzer/query_levels_180s", |b| {
        b.iter(|| {
            let mut a = Analyzer::new(QUERY, true);
            a.push(black_box(&audio));
            a.finish()
        })
    });
    c.bench_function("resampler/44k_to_11k_60s", |b| b.iter(|| resample(black_box(&at_44k), 44_100, RATE)));
}

fn prints(c: &mut Criterion) {
    c.bench_function("landmarks/query_8min", |b| b.iter(|| fingerprint::landmarks(black_box(&MIX.0), 8)));
    c.bench_function("index/build_200", |b| b.iter(|| LibraryIndex::build(black_box(PRINTS.clone()))));
    c.bench_function("print/encode_200", |b| {
        b.iter(|| black_box(&*PRINTS).iter().map(|(_, p)| p.encode()).collect::<Vec<_>>())
    });
    c.bench_function("print/decode_200", |b| {
        let encoded: Vec<Vec<u8>> = PRINTS.iter().map(|(_, p)| p.encode()).collect();
        b.iter(|| black_box(&encoded).iter().map(|e| TrackPrint::decode(e).unwrap()).collect::<Vec<_>>())
    });
    // The same recording, and two different ones (most of the pairs a duplicate search compares).
    c.bench_function("matcher/align_120s_same", |b| {
        let a = &PRINTS[0].1;
        b.iter(|| matcher::align(black_box(a), black_box(a)))
    });
    c.bench_function("matcher/align_120s_other", |b| {
        let (x, y) = (&PRINTS[0].1, &PRINTS[1].1);
        b.iter(|| matcher::align(black_box(x), black_box(y)))
    });
}

fn search(c: &mut Criterion) {
    let mut group = c.benchmark_group("matcher");
    group.sample_size(10);
    let index = LazyLock::new(|| LibraryIndex::build([PRINTS.clone(), filler(FILLER, 300.0)].concat()));
    let ctx = Ctx::silent();
    let (query, frames) = (&MIX.0, MIX.1);
    group.bench_function("find_8min_1200_tracks", |b| {
        // The search finds what the mix plays, so the bench measures real work.
        let found = matcher::find(&index, query, frames, 0.08, None, &ctx).unwrap();
        assert_eq!(found.iter().map(|f| f.slot).collect::<Vec<_>>(), [0, 7, 14, 21, 28, 35], "{found:#?}");
        b.iter(|| matcher::find(&index, black_box(query), frames, 0.08, None, &ctx).unwrap())
    });
    group.finish();
}

criterion_group!(benches, signal, prints, search);
criterion_main!(benches);
