//! Fixtures and Hegel generators for tests, behind the `test-util` feature. Not part of the
//! stable API: they change with the tests that use them.

use hegel::generators::{self as gs, Generator, PrintableGenerator};

use crate::harmony::Camelot;
use crate::model::{Library, Playlist, Track, TrackColor};

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

/// A made-up library of `tracks` tracks and `playlists` playlists and folders, the same every
/// time, for benchmarks: titles and artists in the shapes real tags take (accents, featured
/// artists, remixes, "Original Mix"), keys, tempos, dates, ISRCs, and playlists of 50 tracks,
/// every tenth node a top-level folder holding the nine after it.
pub fn library(tracks: u32, playlists: u32) -> Library {
    const WORDS: &str = "night|light|dream|fire|love|deep|sun|moon|city|drive|echo|wave|café|über|niño|don't|Mr.|\
                         rock & roll|satisfaction|around|colour|strobe|glue|opus";
    const NAMES: &[&str] =
        &["Daft Punk", "Kölsch", "deadmau5", "The Chemical Brothers", "Above & Beyond", "Bicep", "Árvore", "Moby"];
    const MIXES: &[&str] = &["", " (Extended Mix)", " (Radio Edit)", " - Original Mix", " (feat. Nile Rodgers)"];
    let words: Vec<&str> = WORDS.split('|').collect();
    let word = |i: u32| words[i as usize % words.len()];
    let tracks: Vec<Track> = (0..tracks)
        .map(|i| {
            let artist = match i % 10 {
                0 => format!("{} feat. {}", NAMES[i as usize % NAMES.len()], NAMES[(i / 3) as usize % NAMES.len()]),
                _ => format!("{} {}", NAMES[i as usize % NAMES.len()], i % 997),
            };
            let mix =
                if i % 7 == 3 { format!(" ({} Remix)", NAMES[(i / 7) as usize % NAMES.len()]) } else { String::new() };
            let title = format!("{} {} {i}{mix}{}", word(i / 3), word(i / 5), MIXES[i as usize % MIXES.len()]);
            let album = format!("{} {}", word(i / 11), i / 12);
            let ext = ["mp3", "flac", "aiff", "wav", "m4a"][i as usize % 5];
            let mut t =
                track(i + 1, &artist, &title, &format!("/Contents/{artist}/{album}/{:02} {title}.{ext}", i % 12));
            t.album = album;
            t.genre = ["House", "Techno", "Drum & Bass"][i as usize % 3].into();
            t.mix_name = if i % 4 == 0 { "Original Mix".into() } else { String::new() };
            t.key = format!("{}{}", i % 12 + 1, if i % 2 == 0 { 'A' } else { 'B' });
            t.bpm = 118.0 + f64::from(i % 140) / 7.0;
            t.duration_secs = 180 + i * 37 % 300;
            t.track_number = i % 12 + 1;
            t.year = 1990 + (i % 35) as u16;
            t.rating = (i % 6) as u8;
            t.play_count = (i % 40) as u16;
            t.file_size = 8_000_000 + u64::from(i) * 1_000;
            t.date_added = format!("2024-{:02}-{:02}", i % 12 + 1, i % 28 + 1);
            t.isrc = if i % 3 == 0 { format!("GB-ABC-{:02}-{:05}", i % 100, i) } else { String::new() };
            t
        })
        .collect();
    let n = tracks.len().max(1) as u32;
    let mut folder = None;
    let playlists = (0..playlists)
        .map(|p| {
            let id = p + 1;
            let is_folder = p % 10 == 0;
            let node = Playlist {
                id,
                parent_id: if is_folder { None } else { folder },
                name: format!("{} {p}", if is_folder { "Shows" } else { word(p) }),
                is_folder,
                sort_order: p,
                track_ids: if is_folder { Vec::new() } else { (0..50).map(|k| (p * 131 + k * 197) % n + 1).collect() },
            };
            if is_folder {
                folder = Some(id);
            }
            node
        })
        .collect();
    Library { tracks, playlists, ..Default::default() }
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
