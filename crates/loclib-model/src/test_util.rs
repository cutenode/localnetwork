//! Fixtures for tests, behind the `test-util` feature. Not part of the stable API: they change
//! with the tests that use them.

use crate::model::Track;

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
