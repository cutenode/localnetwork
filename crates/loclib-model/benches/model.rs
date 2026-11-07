//! What runs over a whole library: normalising its text, searching it, diffing two exports of it,
//! and harmonic suggestions for a track. `cargo bench -p loclib-model --features test-util`.

use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};
use loclib_model::harmony::suggestions;
use loclib_model::model::{LibraryDiff, normalize};
use loclib_model::test_util::library;

const TRACKS: u32 = 10_000;

fn bench(c: &mut Criterion) {
    let lib = library(TRACKS, 200);
    c.bench_function("normalize/10k_titles_and_artists", |b| {
        b.iter(|| {
            for t in &lib.tracks {
                black_box(normalize(black_box(&t.title)));
                black_box(normalize(black_box(&t.artist)));
            }
        })
    });
    c.bench_function("search/10k", |b| b.iter(|| lib.search(black_box("daft punk opus"))));
    let mut changed = lib.clone();
    for t in changed.tracks.iter_mut().step_by(10) {
        t.rating = 5 - t.rating;
    }
    c.bench_function("diff/10k", |b| b.iter(|| LibraryDiff::between(black_box(&lib), black_box(&changed))));
    c.bench_function("suggestions/10k", |b| b.iter(|| suggestions(black_box(&lib), &lib.tracks[42], 40)));
}

criterion_group!(benches, bench);
criterion_main!(benches);
