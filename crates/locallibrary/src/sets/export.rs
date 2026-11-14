//! Writing a set out: text, Mixcloud, CUE, YouTube chapters, 1001Tracklists, Podcasting 2.0
//! chapters, CSV and M3U8, with the checks each needs.
//!
//! Every format but CSV and M3U8 is for publishing, and names a track marked unnamed or hidden by
//! the DJ's profile `ID - ID` (see [`SetEntry::may_name`]), as tracklists write a track nobody's
//! named. CSV and M3U8 are for the DJ: they name every track, and CSV says which the set marks
//! unnamed in its last column. M3U8 only lists the files.
//!
//! YouTube's description and Mixcloud's checks are the ones the youtube and mixcloud plugins
//! publish with ([`loclib_plugin::youtube`], [`loclib_plugin::mixcloud`]), so what's pasted is
//! what would be uploaded.

use loclib_model::audio::Format;
use loclib_plugin::{mixcloud, youtube};
use rustc_hash::FxHashMap;
use std::collections::BTreeSet;
use std::fmt::Write as _;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;

use super::{Set, SetEntry, chapters};
use crate::export::{Locator, csv_line, numbered, one_line};
use crate::mix::export::UNNAMED;
use crate::model::{Library, Track};
use crate::profile::ProfilePrefs;
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
    /// Podcasting 2.0 JSON chapters, for podcast apps: a chapter for each track with a start time.
    Chapters,
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

/// `set` as `format`, naming in a format for publishing only the tracks `prefs`, the DJ's
/// profile, lets it. An M3U8 lists the set's tracks found on a drive: its library, and where the
/// file goes.
pub(crate) fn render(
    set: &Set,
    format: SetFormat,
    prefs: &ProfilePrefs,
    drive: Option<(&Library, &Locator)>,
) -> Result<Rendered> {
    let tracks = set.tracks.len();
    let lines = |line: &dyn Fn(&SetEntry) -> String| set.tracks.iter().map(|e| line(e) + "\n").collect::<String>();
    let (text, warnings) = match format {
        SetFormat::Text => (numbered(set.tracks.iter().map(|e| public_name(e, prefs))), Vec::new()),
        SetFormat::Mixcloud => (
            lines(&|e| match e.start_seconds {
                Some(s) => {
                    let (h, m, s) = hms(s);
                    format!("{} - {h:02}:{m:02}:{s:02}", public_name(e, prefs))
                }
                None => public_name(e, prefs),
            }),
            // Every track counts, named or not: the rules are about the music played.
            mixcloud::warnings(&set.tracklist(&BTreeSet::new())),
        ),
        SetFormat::Cue => cue(set, prefs)?,
        SetFormat::Youtube => {
            // How long the recording is, when it's on this computer: no chapter starts in its
            // last ten seconds. The youtube plugin reads it with ffprobe, to the same second.
            let length = set.recording.as_ref().and_then(|r| crate::probe::format(&r.path).ok()?.duration_secs);
            return youtube(set, prefs, length.map_or(u32::MAX, |l| l as u32));
        }
        SetFormat::Tracklists => (
            lines(&|e| match e.start_seconds {
                Some(s) => format!("[{}] {}", clock(s), public_name(e, prefs)),
                None => public_name(e, prefs),
            }),
            Vec::new(),
        ),
        SetFormat::Chapters => {
            let (chapters, notes) = chapters::chapters(set, None, |_, e| public_name(e, prefs));
            let text = chapters_json(&set.heading(), &chapters);
            return Ok(Rendered { text, tracks: chapters.len(), warnings: notes });
        }
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
fn public_name(e: &SetEntry, prefs: &ProfilePrefs) -> String {
    shown_name(e, e.may_name(prefs))
}

/// `e`'s name on one line, or `ID - ID` when it mustn't be `named`.
pub(crate) fn shown_name(e: &SetEntry, named: bool) -> String {
    match named {
        true => one_line(&e.name()),
        false => format!("{UNNAMED} - {UNNAMED}"),
    }
}

fn hms(secs: u32) -> (u32, u32, u32) {
    (secs / 3600, secs / 60 % 60, secs % 60)
}

/// `H:MM:SS`.
pub(super) fn clock(secs: u32) -> String {
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
fn cue(set: &Set, prefs: &ProfilePrefs) -> Result<(String, Vec<String>)> {
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
        let (title, artist) = match e.may_name(prefs) {
            true => (e.title.as_str(), e.artist.as_str()),
            false => (UNNAMED, UNNAMED),
        };
        let _ = writeln!(out, "  TRACK {:02} AUDIO\n    TITLE {}", i + 1, quoted(title));
        if !artist.is_empty() {
            let _ = writeln!(out, "    PERFORMER {}", quoted(artist));
        }
        let _ = writeln!(out, "    INDEX 01 {:02}:{:02}:00", start / 60, start % 60);
    }
    Ok((out, out_of_order(&starts)))
}

/// The description the youtube plugin uploads for `set` with a recording `length` seconds long
/// (see [`youtube::description`]), naming only the tracks `prefs` lets it, with its notes as the
/// warnings.
fn youtube(set: &Set, prefs: &ProfilePrefs, length: u32) -> Result<Rendered> {
    let unnamed = set.unnamed(&[], prefs)?.positions;
    let d = youtube::description(&set.tracklist(&unnamed), length);
    let text = if d.text.is_empty() { d.text } else { d.text + "\n" };
    Ok(Rendered { text, tracks: d.tracks, warnings: d.notes })
}

/// `chapters` as Podcasting 2.0 JSON chapters, version 1.2.0
/// (<https://github.com/Podcastindex-org/podcast-namespace/blob/main/docs/examples/chapters/jsonChapters.md>),
/// titled `title`.
fn chapters_json(title: &str, chapters: &[chapters::Chapter]) -> String {
    let chapters: Vec<_> = chapters.iter().map(|c| json!({ "startTime": c.start, "title": c.title })).collect();
    format!("{:#}\n", json!({ "version": "1.2.0", "title": title, "chapters": chapters }))
}

/// CSV for the DJ: every track named, `yes` in the last column for those the set marks unnamed.
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

#[cfg(test)]
mod tests;
