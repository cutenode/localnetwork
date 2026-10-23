//! Writing a set out: text, Mixcloud, CUE, YouTube chapters, 1001Tracklists, CSV and M3U8, with
//! the checks each needs.
//!
//! Every format but CSV and M3U8 is for publishing, and names a track marked unnamed `ID - ID`,
//! as tracklists write a track nobody's named. CSV and M3U8 are for the DJ: they name every
//! track, and CSV says which are unnamed in its last column. M3U8 only lists the files.

use loclib_model::audio::Format;
use rustc_hash::FxHashMap;
use std::fmt::Write as _;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::{Set, SetEntry};
use crate::export::{Locator, csv_line, numbered, one_line};
use crate::mix::export::UNNAMED;
use crate::model::artist_credits;
use crate::model::{Library, Track, normalize};
use crate::{Error, Result};

/// How a set is written out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum SetFormat {
    /// `01. Artist - Title`.
    Text,
    /// Mixcloud's tracklist paste: `Artist - Title - 00:04:12`, with its rules checked.
    Mixcloud,
    /// A CUE sheet for the recording.
    Cue,
    /// Chapters for a YouTube description: `04:12 Artist - Title`.
    Youtube,
    /// For 1001Tracklists: `[0:04:12] Artist - Title`. Unconfirmed: their site documents no
    /// paste format, so this is the cue-time style tracklists are usually written in.
    #[serde(rename = "1001")]
    Tracklists,
    /// `#,Start,Artist,Title,Album,Label,Genre,BPM,Key,ISRC,Year,Unnamed`.
    Csv,
    /// A playlist of the tracks on a drive.
    M3u8,
}

/// A set written out.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Rendered {
    pub text: String,
    /// Tracks written.
    pub tracks: usize,
    pub warnings: Vec<String>,
}

/// `set` as `format`. An M3U8 lists the set's tracks found on a drive: its library, and where
/// the file goes.
pub(crate) fn render(set: &Set, format: SetFormat, drive: Option<(&Library, &Locator)>) -> Result<Rendered> {
    let tracks = set.tracks.len();
    let lines = |line: &dyn Fn(&SetEntry) -> String| set.tracks.iter().map(|e| line(e) + "\n").collect::<String>();
    let (text, warnings) = match format {
        SetFormat::Text => (numbered(set.tracks.iter().map(public_name)), Vec::new()),
        SetFormat::Mixcloud => (
            lines(&|e| match e.start_seconds {
                Some(s) => {
                    let (h, m, s) = hms(s);
                    format!("{} - {h:02}:{m:02}:{s:02}", public_name(e))
                }
                None => public_name(e),
            }),
            mixcloud_check(set),
        ),
        SetFormat::Cue => cue(set)?,
        SetFormat::Youtube => youtube(set)?,
        SetFormat::Tracklists => (
            lines(&|e| match e.start_seconds {
                Some(s) => format!("[{}] {}", clock(s), public_name(e)),
                None => public_name(e),
            }),
            Vec::new(),
        ),
        SetFormat::Csv => (csv(set), Vec::new()),
        SetFormat::M3u8 => {
            let (library, locator) =
                drive.ok_or_else(|| Error::invalid("an M3U8 lists files on a drive: name the drive"))?;
            let (found, missing) = on_drive(set, library);
            let text = crate::export::m3u8(&set.heading(), &found, locator);
            return Ok(Rendered { text, tracks: found.len(), warnings: missing });
        }
    };
    Ok(Rendered { text, tracks, warnings })
}

/// The set's tracks found in `library` by file, and a warning for each that isn't there.
pub(crate) fn on_drive<'a>(set: &Set, library: &'a Library) -> (Vec<&'a Track>, Vec<String>) {
    let by_file: FxHashMap<String, &Track> =
        library.tracks.iter().map(|t| (crate::paths::file_key(&t.file_path), t)).collect();
    let (mut found, mut missing) = (Vec::new(), Vec::new());
    for (i, e) in set.tracks.iter().enumerate() {
        match e.file_key().and_then(|k| by_file.get(&k)) {
            Some(t) => found.push(*t),
            None => missing.push(format!("{}. {} isn't on the drive: left out", i + 1, e.name())),
        }
    }
    (found, missing)
}

/// What a published tracklist calls `e`.
fn public_name(e: &SetEntry) -> String {
    match e.unnamed {
        true => format!("{UNNAMED} - {UNNAMED}"),
        false => one_line(&e.name()),
    }
}

fn hms(secs: u32) -> (u32, u32, u32) {
    (secs / 3600, secs / 60 % 60, secs % 60)
}

/// `H:MM:SS`.
fn clock(secs: u32) -> String {
    let (h, m, s) = hms(secs);
    format!("{h}:{m:02}:{s:02}")
}

/// Every track's start, for a format that needs them all.
fn starts(set: &Set, what: &str) -> Result<Vec<u32>> {
    set.tracks
        .iter()
        .enumerate()
        .map(|(i, e)| {
            e.start_seconds.ok_or_else(|| {
                Error::invalid(format!(
                    "track {} ({}) has no start time, and {what} needs one for every track: attach the recording",
                    i + 1,
                    e.name()
                ))
            })
        })
        .collect()
}

/// A warning for each track that doesn't start after the one before it.
fn out_of_order(starts: &[u32]) -> Vec<String> {
    starts
        .windows(2)
        .enumerate()
        .filter(|(_, w)| w[1] <= w[0])
        .map(|(i, w)| {
            format!("track {} ({}) doesn't start after track {} ({})", i + 2, clock(w[1]), i + 1, clock(w[0]))
        })
        .collect()
}

/// A CUE sheet indexing the recording. Starts are whole seconds, so frames (75 a second) are 0.
fn cue(set: &Set) -> Result<(String, Vec<String>)> {
    let file = set
        .recording
        .as_ref()
        .and_then(|r| r.path.file_name())
        .map(|f| f.to_string_lossy().into_owned())
        .ok_or_else(|| Error::invalid("a CUE sheet indexes the recording: attach it to the set first"))?;
    let starts = starts(set, "a CUE sheet")?;
    let quoted = |s: &str| format!("\"{}\"", one_line(s).replace('"', "'"));
    let kind = match Format::from_name(&file) {
        Some(Format::Mp3) => "MP3",
        Some(Format::Aiff) => "AIFF",
        _ => "WAVE",
    };
    let mut out = format!("TITLE {}\nFILE {} {kind}\n", quoted(&set.heading()), quoted(&file));
    for (i, (e, start)) in set.tracks.iter().zip(&starts).enumerate() {
        let (title, artist) = match e.unnamed {
            true => (UNNAMED, UNNAMED),
            false => (e.title.as_str(), e.artist.as_str()),
        };
        let _ = writeln!(out, "  TRACK {:02} AUDIO\n    TITLE {}", i + 1, quoted(title));
        if !artist.is_empty() {
            let _ = writeln!(out, "    PERFORMER {}", quoted(artist));
        }
        let _ = writeln!(out, "    INDEX 01 {:02}:{:02}:00", start / 60, start % 60);
    }
    Ok((out, out_of_order(&starts)))
}

/// YouTube chapters: the first at 00:00 (YouTube insists), with a warning for each of its other
/// rules broken (at least three, in order, each 10 seconds or longer).
fn youtube(set: &Set) -> Result<(String, Vec<String>)> {
    let mut starts = starts(set, "YouTube")?;
    if let Some(first) = starts.first_mut() {
        *first = 0;
    }
    let mut out = String::new();
    for (e, &start) in set.tracks.iter().zip(&starts) {
        let at = match hms(start) {
            (0, m, s) => format!("{m:02}:{s:02}"),
            _ => clock(start),
        };
        let _ = writeln!(out, "{at} {}", public_name(e));
    }
    let mut warnings = Vec::new();
    if starts.len() < 3 {
        warnings.push(format!("YouTube needs at least 3 chapters; this has {}", starts.len()));
    }
    warnings.extend(out_of_order(&starts));
    for (i, w) in starts.windows(2).enumerate().filter(|(_, w)| w[1] > w[0] && w[1] - w[0] < 10) {
        warnings.push(format!("chapter {} is {} seconds long; YouTube needs 10", i + 1, w[1] - w[0]));
    }
    Ok((out, warnings))
}

/// CSV for the DJ: every track named, `yes` in the last column for those published unnamed.
fn csv(set: &Set) -> String {
    let mut out = String::new();
    csv_line(
        &mut out,
        &["#", "Start", "Artist", "Title", "Album", "Label", "Genre", "BPM", "Key", "ISRC", "Year", "Unnamed"],
    );
    for (i, e) in set.tracks.iter().enumerate() {
        let row = [
            (i + 1).to_string(),
            e.start_seconds.map(clock).unwrap_or_default(),
            e.artist.clone(),
            e.title.clone(),
            e.album.clone(),
            e.label.clone(),
            e.genre.clone(),
            if e.bpm > 0.0 { format!("{:.2}", e.bpm) } else { String::new() },
            e.key.clone(),
            e.isrc.clone(),
            if e.year > 0 { e.year.to_string() } else { String::new() },
            if e.unnamed { "yes".into() } else { String::new() },
        ];
        csv_line(&mut out, &row);
    }
    out
}

/// Mixcloud's rules for a show: over 15 minutes, at most 3 tracks from one release (2 in a row)
/// and 4 by one artist (3 in a row), or only subscribers can listen; under 15 minutes, no
/// copyrighted music at all. The show's length is to the end of its last track when that has a
/// start time (about the recording's), else its tracks' lengths added up.
fn mixcloud_check(set: &Set) -> Vec<String> {
    let length = match set.tracks.last() {
        Some(SetEntry { start_seconds: Some(s), duration_secs, .. }) => s + duration_secs,
        _ => set.tracks.iter().map(|e| e.duration_secs).sum(),
    };
    if length > 0 && length < 15 * 60 {
        return vec![format!(
            "it's {} minutes long: Mixcloud doesn't allow copyrighted music in uploads under 15 minutes",
            length / 60
        )];
    }
    let releases: Vec<Vec<String>> =
        set.tracks.iter().map(|e| Some(e.album.clone()).filter(|a| !a.is_empty()).into_iter().collect()).collect();
    let artists: Vec<Vec<String>> = set.tracks.iter().map(|e| artist_credits(&e.artist)).collect();
    let mut out = limits(&releases, |r| format!("from {r:?}"), 3, 2);
    out.extend(limits(&artists, |a| format!("by {a}"), 4, 3));
    out
}

/// A warning for each name (a release or an artist; `tracks` holds each track's) on more than
/// `most` tracks, and each run of more than `in_a_row` tracks one after another with it.
fn limits(tracks: &[Vec<String>], what: impl Fn(&str) -> String, most: usize, in_a_row: usize) -> Vec<String> {
    const LIMITED: &str = "before it limits the show to subscribers";
    // Each name by its key, as first spelled: how many tracks have it, and where its run started.
    let mut names: Vec<(String, String, usize, Option<usize>)> = Vec::new();
    let mut runs = Vec::new();
    // One past the last track ends every run.
    for i in 0..=tracks.len() {
        let credits = tracks.get(i).map(Vec::as_slice).unwrap_or_default();
        let keys: Vec<String> = credits.iter().map(|c| normalize(c)).collect();
        for (key, name, _, run) in names.iter_mut() {
            if let Some(from) = run.take_if(|_| !keys.contains(key))
                && i - from > in_a_row
            {
                runs.push(format!(
                    "tracks {}-{i} are {}: Mixcloud allows {in_a_row} in a row {LIMITED}",
                    from + 1,
                    what(name)
                ));
            }
        }
        for (key, credit) in keys.into_iter().zip(credits) {
            match names.iter_mut().find(|n| n.0 == key) {
                Some((_, _, count, run)) => {
                    *count += 1;
                    run.get_or_insert(i);
                }
                None => names.push((key, credit.clone(), 1, Some(i))),
            }
        }
    }
    let mut out: Vec<String> = names
        .iter()
        .filter(|n| n.2 > most)
        .map(|(_, name, count, _)| format!("{count} tracks {}: Mixcloud allows {most} {LIMITED}", what(name)))
        .collect();
    out.extend(runs);
    out
}

#[cfg(test)]
mod tests;
