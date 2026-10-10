//! Importing play logs from other tools as sessions (Beat Link Trigger's Playlist Writer CSV).
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

    let mut id = blake3::Hasher::new();
    id.update(BEAT_LINK_TRIGGER.as_bytes());
    let mut plays = Vec::new();
    for r in records {
        let field = |i: Option<usize>| i.and_then(|i| r.fields.get(i)).map_or("", |f| f.trim());
        let (title, artist, started_at) = (field(Some(title)), field(artist), field(Some(started)));
        // The time as written, so the id doesn't depend on the time zone the log is read in.
        for part in [started_at, artist, title] {
            id.update(b"\x1f");
            id.update(part.as_bytes());
        }
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
    Ok((id.finalize().to_hex()[..32].to_string(), plays))
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
mod tests {
    use super::*;

    const LOG: &str = "\
Title,Artist,Album,Player,Source,Started,Stopped,Play Time
A2 - In Charge,[A]pendics.Shuffle,,3,Player 3 USB,Sat Oct 03 23:20:00 UTC 2026,Sat Oct 03 23:26:10 UTC 2026,06:10
\"Never Grow Old, Pt. 1\",Floorplan,Victorious,2,Player 2 USB,Sat Oct 03 23:14:05 UTC 2026,Sat Oct 03 23:21:00 UTC 2026,06:55
Unknown (no metadata found),,,1,Player 1 USB,Sat Oct 03 23:30:00 UTC 2026,Sat Oct 03 23:31:00 UTC 2026,01:00
";

    fn at(s: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc)
    }

    #[test]
    fn java_dates_are_read_in_their_zone_or_local_time() {
        assert_eq!(java_date("Sat Oct 03 23:14:05 UTC 2026"), Some(at("2026-10-03T23:14:05Z")));
        assert_eq!(java_date("Sat Oct 03 23:14:05 GMT 2026"), Some(at("2026-10-03T23:14:05Z")));
        assert_eq!(java_date("Sat Oct 03 23:14:05 GMT+02:00 2026"), Some(at("2026-10-03T21:14:05Z")));
        assert_eq!(java_date("Sun Oct 04 01:00:00 GMT-05:00 2026"), Some(at("2026-10-04T06:00:00Z")));
        let local = Local.with_ymd_and_hms(2026, 10, 3, 23, 14, 5).earliest().unwrap().with_timezone(&Utc);
        assert_eq!(java_date("Sat Oct 03 23:14:05 CEST 2026"), Some(local));
        for bad in ["", "Sat Oct 03 23:14:05 2026", "Fri Oct 03 23:14:05 UTC 2026", "Sat Oct 03 25:14:05 UTC 2026"] {
            assert_eq!(java_date(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn logs_are_read_in_the_order_tracks_started() {
        let (id, plays) = read_log(LOG, None).unwrap();
        assert_eq!(id.len(), 32);
        let names: Vec<String> = plays.iter().map(|p| p.entry.name()).collect();
        assert_eq!(
            names,
            ["Floorplan - Never Grow Old, Pt. 1", "[A]pendics.Shuffle - A2 - In Charge", "Unknown (no metadata found)"]
        );
        assert_eq!(plays[0].entry.album, "Victorious");
        assert_eq!(
            (plays[0].started, plays[0].stopped),
            (at("2026-10-03T23:14:05Z"), Some(at("2026-10-03T23:21:00Z")))
        );
        assert_eq!(plays[0].player.as_deref(), Some("2"));
        assert!(plays.iter().all(|p| p.entry.origin == EntryOrigin::Session && p.entry.file.is_none()));

        // The same plays are the same log; other plays aren't.
        assert_eq!(read_log(LOG, None).unwrap().0, id);
        assert_ne!(read_log(&LOG.replace("23:30:00", "23:30:01"), None).unwrap().0, id);
    }

    #[test]
    fn rows_are_matched_to_the_library_by_artist_and_title() {
        let mut t =
            loclib_model::test_util::track(7, "[a]pendics.shuffle", "A2 - In Charge", "/Contents/a/In Charge.mp3");
        t.bpm = 128.0;
        let lib = Library { tracks: vec![t], ..Default::default() };
        let (_, plays) = read_log(LOG, Some(&lib)).unwrap();
        let matched = &plays[1].entry;
        assert_eq!((matched.file.as_deref(), matched.bpm), (Some("/Contents/a/In Charge.mp3"), 128.0));
        assert!(plays[0].entry.file.is_none());
    }

    #[test]
    fn other_files_are_refused() {
        let err = |text: &str| read_log(text, None).unwrap_err();
        assert!(err("").contains("empty"));
        assert!(err("Track Name,Artist Name(s)\nx,y\n").contains("isn't a Beat Link Trigger playlist"));
        assert!(err("Title,Artist,Started\n").contains("no plays"));
        assert_eq!(err("Title,Artist,Started\nx,y,yesterday\n"), "line 2: \"yesterday\" isn't a time");
    }
}
