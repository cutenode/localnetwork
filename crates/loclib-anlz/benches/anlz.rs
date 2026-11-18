//! Reading and writing the analysis files of a made-up 5-minute track, as big as rekordbox's: every
//! reader over its `.DAT` and `.EXT`, and each rewrite an edit or a relocation makes.
//! `cargo bench -p loclib-anlz`.

use std::hint::black_box;
use std::sync::LazyLock;

use criterion::{Criterion, criterion_group, criterion_main};
use loclib_anlz::{self as anlz, Bands, Beat, CueFile, CuePoint, Mood, Phrase, PhraseKind, SongStructure};

const SECS: u32 = 300;

/// Three-band columns at 150 a second, varying with `seed`.
fn bands(seed: usize) -> Bands {
    let column = |i: usize| {
        let v = ((i + seed) * 37 % 251) as u8;
        [v, v / 2, 255 - v]
    };
    Bands((0..SECS as usize * 150).map(column).collect())
}

/// A beat every 469 ms (128 BPM), from `offset_ms`.
fn beats(offset_ms: u32) -> Vec<Beat> {
    let beat = |i: u32| Beat { beat_number: (i % 4) as u16 + 1, bpm: 128.0, time_ms: offset_ms + 469 * i };
    (0..SECS * 1000 / 469).map(beat).collect()
}

/// Hot cues A-H and eight named memory cues, every other one a loop.
fn cues() -> Vec<CuePoint> {
    (0..16u32)
        .map(|i| CuePoint {
            hot_cue: (i < 8).then_some(i as u8),
            start_ms: 1_000 + 15_000 * i,
            end_ms: (i % 2 == 1).then_some(4_750 + 15_000 * i),
            loop_beats: (i % 2 == 1).then_some((8, 1)),
            comment: format!("cue {i}"),
            color_id: if i < 8 { 0 } else { (i % 8) as u8 + 1 },
            ..Default::default()
        })
        .collect()
}

/// Sixteen phrases of 32 beats, as a high-mood track names them.
fn phrases() -> SongStructure {
    let kinds = [PhraseKind::Intro, PhraseKind::Up, PhraseKind::Chorus, PhraseKind::Down, PhraseKind::Outro];
    let phrase = |i: u16| Phrase {
        kind: kinds[usize::from(i) % kinds.len()],
        start_beat: 1 + 32 * i,
        end_beat: 33 + 32 * i,
        fill_beat: None,
    };
    SongStructure { mood: Mood::High, phrases: (0..16).map(phrase).collect() }
}

fn wave(file: Vec<u8>, tag: &[u8; 4], bands: &Bands) -> Vec<u8> {
    anlz::write_waveform(&file, tag, &anlz::waveform_from_bands(tag, bands).unwrap().unwrap()).unwrap()
}

/// A `.DAT` and `.EXT` holding the sections rekordbox writes for a track: its path, grid, cues,
/// blue and colour waveforms and phrases.
static FILES: LazyLock<(Vec<u8>, Vec<u8>)> = LazyLock::new(|| {
    let b = bands(0);
    let (dat, ext) = anlz::new_analysis_files("/Contents/Artist/Track.mp3").unwrap();
    let dat = wave(wave(anlz::write_beat_grid(&dat, &beats(100)).unwrap(), b"PWAV", &b), b"PWV2", &b);
    let ext = wave(wave(ext, b"PWV3", &b), b"PWV5", &b);
    // `PWV4` isn't worked out from bands: 1200 columns of 6 bytes, made up.
    let pwv4: Vec<u8> = (0..1200 * 6).map(|i| (i % 128) as u8).collect();
    let ext =
        anlz::write_song_structure(&anlz::write_waveform(&ext, b"PWV4", &pwv4).unwrap(), Some(&phrases())).unwrap();
    let cues = cues();
    (
        anlz::write_cue_points(&dat, CueFile::Dat, &cues).unwrap(),
        anlz::write_cue_points(&ext, CueFile::Ext, &cues).unwrap(),
    )
});

fn read(c: &mut Criterion) {
    let (dat, ext) = &*FILES;
    let mut group = c.benchmark_group("read");
    for (name, file) in [("dat", dat), ("ext", ext)] {
        group.bench_function(format!("every_reader_{name}"), |b| {
            b.iter(|| {
                let f = black_box(file);
                black_box((
                    anlz::read_path(f),
                    anlz::beat_grid(f),
                    anlz::cues(f),
                    anlz::preview(f),
                    anlz::detail(f),
                    anlz::song_structure(f),
                ))
            })
        });
    }
    group.bench_function("cue_points", |b| {
        b.iter(|| anlz::read_cue_points(black_box(dat), Some(black_box(ext.as_slice()))))
    });
    group.finish();
}

fn write(c: &mut Criterion) {
    let (dat, ext) = &*FILES;
    let mut group = c.benchmark_group("write");
    // Another track's waveform, so the section really changes.
    let other = bands(1);
    let detail = anlz::waveform_from_bands(b"PWV5", &other).unwrap().unwrap();
    group.bench_function("colour_detail_waveform", |b| {
        b.iter(|| anlz::write_waveform(black_box(ext), b"PWV5", black_box(&detail)).unwrap())
    });
    group.bench_function("waveforms_from_bands", |b| {
        b.iter(|| {
            anlz::waveform_tags()
                .map(|tag| anlz::waveform_from_bands(tag, black_box(&other)).unwrap())
                .collect::<Vec<_>>()
        })
    });
    let mut moved = cues();
    moved[0].start_ms += 500;
    group.bench_function("cue_points_ext", |b| {
        b.iter(|| anlz::write_cue_points(black_box(ext), CueFile::Ext, black_box(&moved)).unwrap())
    });
    let later = beats(125);
    group.bench_function("beat_grid", |b| b.iter(|| anlz::write_beat_grid(black_box(dat), black_box(&later)).unwrap()));
    group.bench_function("shift_times_ext", |b| b.iter(|| anlz::shift_times(black_box(ext), 25).unwrap()));
    group.bench_function("rewrite_path", |b| {
        b.iter(|| anlz::rewrite_path(black_box(dat), "/Contents/Elsewhere/Track.mp3").unwrap())
    });
    group.finish();
}

criterion_group!(benches, read, write);
criterion_main!(benches);
