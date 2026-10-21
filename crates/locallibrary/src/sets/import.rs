//! Importing play logs from other tools as sessions (Beat Link Trigger's Playlist Writer CSV; a
//! plugin's importer's are read by the plugin host, and kept as these are).
//!
//! Beat Link Trigger follows the players over Pro DJ Link, and its Playlist Writer adds a row to a
//! CSV file each time a track that played long enough stops (`playlist_writer.clj`):
//!
//! ```text
//! Title,Artist,Album,Player,Source,Started,Stopped,Play Time
//! Never Grow Old,Floorplan,Victorious,2,Player 2 USB,Sat Oct 03 23:14:05 CEST 2026,Sat Oct 03 23:19:40 CEST 2026,05:35
//! ```
//!
//! Unlike a history list, it says when each track played.

use rustc_hash::FxHashMap;
use std::path::Path;

use chrono::{DateTime, FixedOffset, Local, NaiveDateTime, TimeZone, Utc};

use crate::error::IoContext;
use crate::import::{read_table, text_of};
use crate::model::{Library, Track, normalize};
use crate::sets::{EntryOrigin, ImportedSession, SetEntry, TimedPlay};
use crate::{Error, Result};

/// [`ImportedSession::tool`] for Beat Link Trigger's logs.
const BEAT_LINK_TRIGGER: &str = "beat_link_trigger";

/// A Playlist Writer CSV as a session, named `name` (default: the file's name). Rows are matched
/// to `library`'s tracks by artist and title; a row without a match keeps the log's artist,
/// title and album. The session's id hashes the plays, so the same log imported again has the
/// same one.
pub(crate) fn beat_link_trigger(file: &Path, name: Option<&str>, library: Option<&Library>) -> Result<ImportedSession> {
    let text = text_of(&std::fs::read(file).at(file)?);
    let (id, mut plays) = read_log(&text, library).map_err(|e| Error::invalid(format!("{}: {e}", file.display())))?;
    for p in &mut plays {
        p.entry.session = Some(id.clone());
    }
    let stem = file.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    Ok(ImportedSession {
        id,
        name: name.map(str::trim).filter(|n| !n.is_empty()).map_or(stem, String::from),
        tool: BEAT_LINK_TRIGGER.into(),
        file: file.to_path_buf(),
        imported_at: Utc::now(),
        plays,
    })
}

/// The log's id and its plays in the order they started (rows are written when a track stops).
fn read_log(text: &str, library: Option<&Library>) -> std::result::Result<(String, Vec<TimedPlay>), String> {
    let mut records = read_table(text, ',', true).into_iter();
    let header = records.next().ok_or("it's empty")?;
    let column = |name: &str| header.fields.iter().position(|f| f.trim() == name);
    let (Some(title), Some(started)) = (column("Title"), column("Started")) else {
        return Err("it isn't a Beat Link Trigger playlist (no Title and Started columns)".into());
    };
    let (artist, album, player, stopped) = (column("Artist"), column("Album"), column("Player"), column("Stopped"));
    // The first of tracks with one artist and title, as library lookups take the first.
    let tracks: FxHashMap<(String, String), &Track> = library
        .iter()
        .flat_map(|l| l.tracks.iter().rev())
        .map(|t| ((normalize(&t.artist), normalize(&t.title)), t))
        .collect();

    let (mut hashed, mut plays) = (Vec::new(), Vec::new());
    for r in records {
        let field = |i: Option<usize>| i.and_then(|i| r.fields.get(i)).map_or("", |f| f.trim());
        let (title, artist, started_at) = (field(Some(title)), field(artist), field(Some(started)));
        // The time as written, so the id doesn't depend on the time zone the log is read in.
        hashed.extend([started_at, artist, title].map(String::from));
        let entry = match tracks.get(&(normalize(artist), normalize(title))) {
            Some(t) => SetEntry::from_track(t, EntryOrigin::Session, None),
            None => SetEntry {
                title: title.into(),
                artist: artist.into(),
                album: field(album).into(),
                origin: EntryOrigin::Session,
                ..SetEntry::added("")
            },
        };
        plays.push(TimedPlay {
            entry,
            started: java_date(started_at).ok_or_else(|| format!("line {}: {started_at:?} isn't a time", r.line))?,
            stopped: java_date(field(stopped)),
            player: Some(field(player).to_string()).filter(|p| !p.is_empty()),
        });
    }
    if plays.is_empty() {
        return Err("it has no plays".into());
    }
    plays.sort_by_key(|p| p.started);
    Ok((log_id(BEAT_LINK_TRIGGER, &hashed), plays))
}

/// An imported log's id: its `tool` and each play's start, artist and title (`parts`, in the
/// log's order, as it writes them) hashed, so the same log imported again has the same one.
pub(crate) fn log_id(tool: &str, parts: impl IntoIterator<Item = impl AsRef<str>>) -> String {
    let mut id = blake3::Hasher::new();
    id.update(tool.as_bytes());
    for part in parts {
        id.update(b"\x1f");
        id.update(part.as_ref().as_bytes());
    }
    id.finalize().to_hex()[..32].to_string()
}

/// A `java.util.Date` as Beat Link Trigger writes one: `Sat Oct 03 23:14:05 CEST 2026`. chrono
/// can't read zone abbreviations, and one can stand for several zones (CST is three), so the time
/// is taken as this computer's local time, where the log was most likely written. `UTC`, `GMT`
/// and `GMT+02:00` (Java's name for a zone without an abbreviation) are read as written.
fn java_date(s: &str) -> Option<DateTime<Utc>> {
    let parts: Vec<&str> = s.split_whitespace().collect();
    let [weekday, month, day, time, zone, year] = parts[..] else { return None };
    let at = NaiveDateTime::parse_from_str(&format!("{weekday} {month} {day} {time} {year}"), "%a %b %d %H:%M:%S %Y")
        .ok()?;
    let offset = match zone.strip_prefix("GMT").or_else(|| zone.strip_prefix("UTC")) {
        Some("") => return Some(at.and_utc()),
        Some(offset) => offset.parse::<FixedOffset>().ok()?,
        None => *Local.from_local_datetime(&at).earliest()?.offset(),
    };
    offset.from_local_datetime(&at).single().map(|t| t.with_timezone(&Utc))
}

#[cfg(test)]
mod tests;
