//! Finding a tracklist in a library, at library scale: indexing 10,000 tracks, and matching 1,000
//! entries (by path, by artist and title, fuzzily, and misses). `cargo bench -p loclib-tracklist`.

use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};
use loclib_model::test_util::library;
use loclib_tracklist::{Entry, Matcher, match_entries};

const TRACKS: u32 = 10_000;
const ENTRIES: u32 = 1_000;

fn bench(c: &mut Criterion) {
    let lib = library(TRACKS, 0);
    let entries: Vec<Entry> = (0..ENTRIES)
        .map(|k| {
            let t = &lib.tracks[(k * 37 % TRACKS) as usize];
            let mut e = Entry { artist: t.artist.clone(), title: t.title.clone(), ..Default::default() };
            match k % 4 {
                0 => e = Entry { path: Some(format!("/Users/me/Music{}", &t.file_path[9..])), ..Default::default() },
                1 => e.duration_secs = Some(t.duration_secs),
                2 => e.title.push('x'),
                _ => (e.artist, e.title) = ("Nobody".into(), format!("Nothing {k}")),
            }
            e
        })
        .collect();
    c.bench_function("matcher_new/10k", |b| b.iter(|| Matcher::new(black_box(&lib))));
    c.bench_function("match_entries/10k_x_1k", |b| b.iter(|| match_entries(black_box(&lib), black_box(&entries))));
}

criterion_group!(benches, bench);
criterion_main!(benches);
