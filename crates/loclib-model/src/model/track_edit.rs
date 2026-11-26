//! New values for a track's details, as an edit gives them, and what a track's database rows say
//! about its audio file.

use serde::{Deserialize, Serialize};

use super::{Track, TrackColor};

/// New values for track details; fields left out stay as they are, and an empty name clears
/// artist, album, genre, label, key and the like (an empty ISRC or release date clears it too).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct TrackFields {
    /// The title.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// The artist, as one credit (`A feat. B`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artist: Option<String>,
    /// The album's title.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub album: Option<String>,
    /// The databases keep it with the album, so a track needs one (given here too, or its own).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub album_artist: Option<String>,
    /// The genre.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub genre: Option<String>,
    /// The record label.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// The key, in any notation rekordbox shows.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    /// The remixer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remixer: Option<String>,
    /// The original's artist, for a remix or a cover.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub original_artist: Option<String>,
    /// The composer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub composer: Option<String>,
    /// The comment tag.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
    /// The mix's name (`Extended Mix`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mix_name: Option<String>,
    /// 0-5 stars.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rating: Option<u8>,
    /// A colour name (`aqua`), rekordbox's number (1-8), or `none`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    /// The release year.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub year: Option<u16>,
    /// How many times it's been played.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub play_count: Option<u16>,
    /// Its number on its album.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub track_number: Option<u32>,
    /// Its album's disc number.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disc_number: Option<u16>,
    /// 1-999.99, kept to hundredths as rekordbox keeps it. The beat grid in the analysis files,
    /// which players count beats by, stays as it is.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bpm: Option<f64>,
    /// Like `GBDUW0000059`: two letters, three letters or digits, and seven digits.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub isrc: Option<String>,
    /// `YYYY-MM-DD`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub release_date: Option<String>,
}

impl TrackFields {
    /// The colour to set: `None` leaves it, `Some(None)` clears it. An error says why `color` isn't
    /// one (see [`TrackColor::parse`]).
    pub fn color_value(&self) -> Result<Option<Option<TrackColor>>, String> {
        self.color.as_deref().map(TrackColor::parse).transpose()
    }

    /// Whether it changes nothing.
    pub fn is_empty(&self) -> bool {
        *self == TrackFields::default()
    }

    /// The details as they will be after the edit.
    pub fn apply_to(&self, t: &mut Track) {
        let set = |v: &Option<String>, f: &mut String| {
            if let Some(v) = v {
                *f = v.clone();
            }
        };
        // The databases store no name for these when one is given blank; names the edit leaves
        // alone are kept as they are.
        let name = |v: &Option<String>, f: &mut String| {
            if let Some(v) = v {
                *f = if v.trim().is_empty() { String::new() } else { v.clone() };
            }
        };
        set(&self.title, &mut t.title);
        name(&self.artist, &mut t.artist);
        name(&self.genre, &mut t.genre);
        name(&self.label, &mut t.label);
        name(&self.key, &mut t.key);
        name(&self.remixer, &mut t.remixer);
        name(&self.original_artist, &mut t.original_artist);
        name(&self.composer, &mut t.composer);
        set(&self.comment, &mut t.comment);
        set(&self.mix_name, &mut t.mix_name);
        name(&self.album_artist, &mut t.album_artist);
        set(&self.isrc, &mut t.isrc);
        set(&self.release_date, &mut t.release_date);
        name(&self.album, &mut t.album);
        // As a cleared album's row goes, its artist goes with it.
        if self.album.is_some() && t.album.is_empty() {
            t.album_artist.clear();
        }
        if let Some(r) = self.rating {
            t.rating = r;
        }
        if let Ok(Some(c)) = self.color_value() {
            t.color = c;
        }
        if let Some(y) = self.year {
            t.year = y;
        }
        if let Some(p) = self.play_count {
            t.play_count = p;
        }
        if let Some(n) = self.track_number {
            t.track_number = n;
        }
        if let Some(n) = self.disc_number {
            t.disc_number = n;
        }
        if let Some(b) = self.bpm {
            t.bpm = (b * 100.0).round() / 100.0;
        }
    }

    /// `rating 5, colour aqua, genre "Deep House"`.
    pub fn describe(&self) -> String {
        let mut parts = Vec::new();
        for (name, v) in [
            ("title", &self.title),
            ("artist", &self.artist),
            ("album", &self.album),
            ("genre", &self.genre),
            ("label", &self.label),
            ("key", &self.key),
            ("remixer", &self.remixer),
            ("original artist", &self.original_artist),
            ("composer", &self.composer),
            ("comment", &self.comment),
            ("mix", &self.mix_name),
            ("album artist", &self.album_artist),
            ("colour", &self.color),
            ("ISRC", &self.isrc),
            ("release date", &self.release_date),
        ] {
            if let Some(v) = v {
                parts.push(format!("{name} {v:?}"));
            }
        }
        for (name, v) in [
            ("rating", self.rating.map(u32::from)),
            ("year", self.year.map(u32::from)),
            ("play count", self.play_count.map(u32::from)),
            ("track number", self.track_number),
            ("disc", self.disc_number.map(u32::from)),
        ] {
            if let Some(v) = v {
                parts.push(format!("{name} {v}"));
            }
        }
        if let Some(b) = self.bpm {
            parts.push(format!("BPM {}", (b * 100.0).round() / 100.0));
        }
        parts.join(", ")
    }
}

/// What a track's rows say about its audio file, for a track pointed at another encoding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct AudioFile {
    /// Size in bytes.
    pub size: u64,
    /// Sample rate, in Hz.
    pub sample_rate: u32,
    /// Bits per sample.
    pub bit_depth: u16,
    /// kbps.
    pub bitrate: u32,
}

#[cfg(test)]
mod tests;
