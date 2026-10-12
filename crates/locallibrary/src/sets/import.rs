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

    use hegel::TestCase;
    use hegel::generators as gs;

    /// A Java date written in UTC, GMT or a GMT offset reads back as the moment written; with a
    /// zone abbreviation, as that time on this computer; with the wrong weekday, not at all.
    #[hegel::test(test_cases = 300)]
    fn java_dates_read_back_the_moment_written(tc: TestCase) {
        let secs = tc.draw(gs::integers::<i64>().min_value(946_684_800).max_value(2_208_988_800));
        let t = DateTime::from_timestamp(secs, 0).unwrap();
        let minutes = tc.draw(gs::integers::<i32>().min_value(-48).max_value(56)) * 15;
        let offset = FixedOffset::east_opt(minutes * 60).unwrap();
        let (zone, shown) = match tc.draw(gs::integers::<u8>().max_value(3)) {
            0 => ("UTC".to_string(), FixedOffset::east_opt(0).unwrap()),
            1 => ("GMT".to_string(), FixedOffset::east_opt(0).unwrap()),
            2 => (format!("GMT{offset}"), offset),
            _ => ("CEST".to_string(), offset),
        };
        let at = t.with_timezone(&shown);
        let text = format!("{} {zone} {}", at.format("%a %b %d %H:%M:%S"), at.format("%Y"));
        let want = match zone.as_str() {
            "CEST" => Local.from_local_datetime(&at.naive_local()).earliest().map(|l| l.with_timezone(&Utc)),
            _ => Some(t),
        };
        assert_eq!(java_date(&text), want, "{text}");
        let next = (at + chrono::Duration::days(1)).format("%a").to_string();
        let wrong = text.replacen(&at.format("%a").to_string(), &next, 1);
        assert_eq!(java_date(&wrong), None, "{wrong}");
    }

    /// A log's rows are its plays in the order they started, matched to the first library track
    /// with their artist and title (ignoring case and spacing) or kept as the log has them; a row
    /// whose start isn't a time is refused by its line, and the same plays are the same log.
    #[hegel::test(test_cases = 300)]
    fn logs_are_read_as_their_plays(tc: TestCase) {
        const SONGS: &[(&str, &str)] =
            &[("Never Grow Old", "Floorplan"), ("never  grow old", "FLOORPLAN"), ("ID", ""), ("B, \"C\"", "A")];
        let lib = Library {
            tracks: (0..tc.draw(gs::integers::<usize>().max_value(3)))
                .map(|i| {
                    let (title, artist) = tc.draw(gs::sampled_from(SONGS.to_vec()));
                    loclib_model::test_util::track(i as u32 + 1, artist, title, &format!("/Contents/{i}.mp3"))
                })
                .collect(),
            ..Default::default()
        };
        let rows: Vec<((&str, &str), u32, bool)> = (0..tc.draw(gs::integers::<usize>().min_value(1).max_value(5)))
            .map(|_| {
                (
                    tc.draw(gs::sampled_from(SONGS.to_vec())),
                    tc.draw(gs::integers::<u32>().max_value(5)),
                    tc.draw(gs::booleans()),
                )
            })
            .collect();
        let bad = tc.draw(gs::sampled_from(vec![None, Some(0), Some(2)])).filter(|&i| i < rows.len());
        let q = |s: &str| format!("\"{}\"", s.replace('"', "\"\""));
        let mut text = String::from("Title,Artist,Album,Player,Source,Started,Stopped,Play Time\n");
        for (i, ((title, artist), minute, stopped)) in rows.iter().enumerate() {
            let started = match bad == Some(i) {
                true => "soon".to_string(),
                false => format!("Sat Oct 03 23:{minute:02}:00 UTC 2026"),
            };
            let stopped = if *stopped { format!("Sat Oct 03 23:{:02}:30 UTC 2026", minute + 1) } else { String::new() };
            text += &format!("{},{},,{},Player USB,{started},{stopped},01:00\n", q(title), q(artist), i % 2);
        }
        let library = tc.draw(gs::booleans()).then_some(&lib);
        let header = text.lines().next().unwrap();
        assert_eq!(read_log(header, library).unwrap_err(), "it has no plays");
        let read = read_log(&text, library);
        if let Some(i) = bad {
            assert_eq!(read.unwrap_err(), format!("line {}: \"soon\" isn't a time", i + 2));
            return;
        }
        let (id, plays) = read.unwrap();
        let mut by_start: Vec<_> = rows.clone();
        by_start.sort_by_key(|r| r.1);
        let first = |title: &str, artist: &str| {
            library.and_then(|l| {
                l.tracks
                    .iter()
                    .find(|t| normalize(&t.title) == normalize(title) && normalize(&t.artist) == normalize(artist))
            })
        };
        for (p, ((title, artist), minute, stopped)) in plays.iter().zip(&by_start) {
            let at = |m: u32, s: u32| Utc.with_ymd_and_hms(2026, 10, 3, 23, m, s).unwrap();
            assert_eq!((p.started, p.stopped), (at(*minute, 0), stopped.then(|| at(minute + 1, 30))));
            match first(title, artist) {
                Some(t) => assert_eq!(p.entry.file.as_deref(), Some(t.file_path.as_str())),
                None => assert_eq!(
                    (p.entry.title.as_str(), p.entry.artist.as_str(), p.entry.file.as_deref()),
                    (*title, *artist, None)
                ),
            }
            assert_eq!(p.entry.origin, EntryOrigin::Session);
        }
        assert_eq!(plays.len(), rows.len());
        assert_eq!(read_log(&text, None).unwrap().0, id, "the library doesn't change the log's id");
        let other = text.replacen(" 23:", " 22:", 1);
        assert_ne!(read_log(&other, None).unwrap().0, id);
    }
}
