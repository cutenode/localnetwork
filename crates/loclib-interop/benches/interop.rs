//! Writing a whole library for other software: 10,000 tracks in 200 playlists and folders, as
//! Traktor's collection.nml (with and without beat grids and cues), Serato crates, and an Engine
//! Library. `cargo bench -p loclib-interop`.

use std::hint::black_box;
use std::path::Path;

use criterion::{Criterion, criterion_group, criterion_main};
use loclib_interop::{Source, engine, serato, traktor};
use loclib_model::analysis::{Beat, Cue, TrackAnalysis};
use loclib_model::paths::PathStyle;
use loclib_model::test_util::library;
use loclib_model::{Library, Track};
use loclib_progress::Ctx;

fn bench(c: &mut Criterion) {
    let lib: Library = library(10_000, 200);
    let analysis = TrackAnalysis {
        beats: (0..640).map(|i| Beat { beat_number: (i % 4 + 1) as u16, bpm: 128.0, time_ms: 40 + i * 469 }).collect(),
        cues: (0..6)
            .map(|i| Cue {
                hot_cue: (i < 4).then_some(i as u8),
                start_ms: 40 + i * 30_000,
                end_ms: (i % 3 == 2).then_some(40 + i * 30_000 + 7_500),
                comment: if i == 0 { "Drop".into() } else { String::new() },
                color: Some([0x30, 0x5a, 0xff]),
                color_id: 0,
                loop_beats: None,
            })
            .collect(),
        ..Default::default()
    };
    let load = |_: &Track| analysis.clone();
    let out = Path::new("/Volumes/USB/out");
    let ctx = Ctx::silent();
    let src = |analysis| Source { library: &lib, root: "/Volumes/USB".into(), analysis };

    let bare = src(None);
    c.bench_function("traktor/10k", |b| b.iter(|| traktor::build(black_box(&bare), out, &ctx).unwrap()));
    let analysed = src(Some(&load));
    c.bench_function("traktor/10k_analysed", |b| b.iter(|| traktor::build(black_box(&analysed), out, &ctx).unwrap()));
    c.bench_function("serato/10k", |b| {
        b.iter(|| serato::build::<loclib_interop::Error>(black_box(&bare), out, None, &ctx).unwrap())
    });
    let mut g = c.benchmark_group("engine");
    g.sample_size(10);
    g.bench_function("10k", |b| b.iter(|| engine::build(black_box(&bare), out, PathStyle::Drive, &ctx).unwrap()));
    g.finish();
}

criterion_group!(benches, bench);
criterion_main!(benches);
