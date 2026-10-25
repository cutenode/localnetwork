//! Fixtures and Hegel generators for tests, behind the `test-util` feature. Not part of the
//! stable API: they change with the tests that use them.

use hegel::generators::{self as gs, Generator, PrintableGenerator};

use crate::harmony::Camelot;
use crate::model::{Library, Track, TrackColor};

/// A track at `path`: 128 BPM, five minutes, a 320 kbps 44.1 kHz file of 1,000 bytes, everything
/// else empty.
pub fn track(id: u32, artist: &str, title: &str, path: &str) -> Track {
    Track {
        id,
        title: title.into(),
        artist: artist.into(),
        album: String::new(),
        album_artist: String::new(),
        genre: String::new(),
        label: String::new(),
        key: String::new(),
        remixer: String::new(),
        original_artist: String::new(),
        composer: String::new(),
        comment: String::new(),
        mix_name: String::new(),
        isrc: String::new(),
        bpm: 128.0,
        duration_secs: 300,
        year: 0,
        track_number: 0,
        disc_number: 0,
        rating: 0,
        color: None,
        play_count: 0,
        bitrate: 320,
        sample_rate: 44100,
        sample_depth: 16,
        file_size: 1000,
        file_path: path.into(),
        analyze_path: String::new(),
        date_added: String::new(),
        release_date: String::new(),
        artwork_id: None,
        artwork_path: None,
    }
}

/// [`TrackGenerator`] and [`tracks_like`] from one list of `Track`'s fields, so the two can't
/// drift apart (a field missing from the list doesn't compile).
macro_rules! track_generator {
    ($($field:ident: $ty:ty),+ $(,)?) => {
        hegel::derive_generator!(TrackGenerator for Track { $($field: $ty),+ });

        /// Tracks that are `base` but for the fields a test sets with [`TrackGenerator`]'s
        /// builder methods (`.title(gen)`), so each test draws only the fields it's about, from
        /// generators of its own. `TrackGenerator::new()` draws every field from Hegel's
        /// defaults instead.
        pub fn tracks_like(base: Track) -> TrackGenerator<'static> {
            TrackGenerator::new()$(.$field(gs::just(base.$field)))+
        }
    };
}

track_generator! {
    id: u32,
    title: String,
    artist: String,
    album: String,
    album_artist: String,
    genre: String,
    label: String,
    key: String,
    remixer: String,
    original_artist: String,
    composer: String,
    comment: String,
    mix_name: String,
    isrc: String,
    bpm: f64,
    duration_secs: u32,
    year: u16,
    track_number: u32,
    disc_number: u16,
    rating: u8,
    color: Option<TrackColor>,
    play_count: u16,
    bitrate: u32,
    sample_rate: u32,
    sample_depth: u16,
    file_size: u64,
    file_path: String,
    analyze_path: String,
    date_added: String,
    release_date: String,
    artwork_id: Option<u32>,
    artwork_path: Option<String>,
}

/// Libraries of `min` to `max` tracks, ids 1 up, each drawn from `track(id)`, and no playlists.
/// A test that needs playlists adds them to what it draws.
pub fn libraries<G: PrintableGenerator<Track>>(
    min: u32,
    max: u32,
    track: impl Fn(u32) -> G + Send + Sync,
) -> impl PrintableGenerator<Library> {
    hegel::compose!(|tc| {
        let n = tc.draw(gs::integers::<u32>().min_value(min).max_value(max));
        Library { tracks: (1..=n).map(|id| tc.draw(track(id))).collect(), ..Default::default() }
    })
}

/// Any of the 24 Camelot keys.
pub fn keys() -> impl PrintableGenerator<Camelot> {
    hegel::tuples!(gs::integers::<u8>().min_value(1).max_value(12), gs::booleans())
        .map(|(number, major)| Camelot { number, major })
}
