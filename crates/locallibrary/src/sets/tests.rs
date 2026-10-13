use super::*;
use crate::sets::query::tests::set;

#[test]
fn sets_are_found_by_id_title_day_or_last() {
    let book = SetBook {
        sets: vec![
            set("3m2abcdefgh22", "2026-10-03", Some("Friday")),
            set("3m2abzzzzzz22", "2026-09-12", Some("Warm-up")),
            set("3m4qqqqqqqq22", "2026-09-12", None),
        ],
        ..Default::default()
    };
    assert_eq!(book.find("last").unwrap().title.as_deref(), Some("Friday"));
    assert_eq!(book.find("3m2abc").unwrap().title.as_deref(), Some("Friday"));
    assert_eq!(book.find("warm-UP").unwrap().id, "3m2abzzzzzz22");
    assert_eq!(book.find("2026-10-03").unwrap().id, "3m2abcdefgh22");
    let err = book.find("2026-09-12").unwrap_err().to_string();
    assert!(err.contains("could be any of"), "{err}");
    let err = book.find("3m2ab").unwrap_err().to_string();
    assert!(err.contains("could be any of"), "{err}");
    assert!(book.find("3m2").is_err(), "a prefix needs 4 characters");
    assert_eq!(book.find("nope").unwrap_err().code(), "not_found");
}

/// Regression: an infinite or NaN tempo went into `sets.json` as `null`, and then the book
/// didn't read.
#[test]
fn a_tempo_that_is_no_number_is_saved_as_unknown() {
    for bpm in [f64::INFINITY, f64::NEG_INFINITY, f64::NAN] {
        let e = SetEntry { bpm, ..SetEntry::added("Bicep - Glue") };
        let back: SetEntry = serde_json::from_str(&serde_json::to_string(&e).unwrap()).unwrap();
        assert_eq!(back, SetEntry { bpm: 0.0, ..e });
    }
}

#[test]
fn ids_are_tids_on_the_day() {
    // The spec's example: 2023-11-07T20:21:04.000Z with clock id 0.
    assert_eq!(tid(1_699_388_464_000_000, 0).len(), 13);
    let day = NaiveDate::from_ymd_opt(2026, 10, 3).unwrap();
    let id = new_id(day).unwrap();
    assert_eq!(id.len(), 13);
    assert!(id.bytes().all(|b| b"234567abcdefghijklmnopqrstuvwxyz".contains(&b)));
    let first = id.as_bytes()[0];
    assert!(b"234567abcdefghij".contains(&first), "the top bit is 0");
    // Sorting by id sorts by day.
    let later = new_id(day.succ_opt().unwrap().succ_opt().unwrap()).unwrap();
    assert!(later > id);
}

/// The same TIDs as the AT Protocol library's.
#[cfg(feature = "account")]
#[test]
fn tids_match_the_protocol_library() {
    for (micros, clock) in [(0, 0), (1_699_388_464_000_000, 0), (1_791_321_253_123_456, 1023), (u64::MAX >> 11, 517)] {
        let theirs = jacquard_common::types::tid::Tid::from_time(micros, clock);
        assert_eq!(tid(micros, clock), theirs.as_str(), "{micros} {clock}");
    }
}

#[test]
fn entries_name_and_key_their_track() {
    let e = SetEntry::added("Floorplan - Never Grow Old");
    assert_eq!((e.artist.as_str(), e.title.as_str(), e.origin), ("Floorplan", "Never Grow Old", EntryOrigin::Added));
    assert_eq!(e.name(), "Floorplan - Never Grow Old");
    assert_eq!(SetEntry::added("ID").name(), "ID");
    let t = loclib_model::test_util::track(1, "", "A", "/Contents/X/A.mp3");
    assert_eq!(
        SetEntry::from_track(&t, EntryOrigin::Session, Some("ab")).file_key().as_deref(),
        Some("contents/x/a.mp3")
    );
}

#[test]
fn a_window_names_its_day_or_its_span() {
    let at = |s: &str| DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc);
    let w = Window { after: Some(at("2026-09-29T12:00:00Z")), before: Some(at("2026-10-03T12:00:00Z")), exact: false };
    assert!(w.label().starts_with("between "), "{}", w.label());
    assert_eq!(Window::default().label(), "unknown");
    let w = Window { after: None, before: Some(at("2026-10-03T12:00:00Z")), exact: false };
    assert!(w.label().starts_with("before "), "{}", w.label());
    // A timed log from 22:00 to 03:00 (local) is the night it started.
    let local = |h: u32, d: u32| {
        NaiveDate::from_ymd_opt(2026, 10, d)
            .unwrap()
            .and_hms_opt(h, 0, 0)
            .unwrap()
            .and_local_timezone(Local)
            .unwrap()
            .with_timezone(&Utc)
    };
    let night = Window { after: Some(local(22, 3)), before: Some(local(3, 4)), exact: true };
    assert_eq!(night.day(), NaiveDate::from_ymd_opt(2026, 10, 3));
    assert_eq!(Window { exact: false, ..night }.day(), None, "a window across days is no one day");
}

use hegel::TestCase;
use hegel::generators as gs;

/// A window names the local day it falls on (a timed log, the day it started), else the span
/// it's known to lie in, as far as that's known.
#[hegel::test(test_cases = 300)]
fn a_window_names_its_day_or_what_it_spans(tc: TestCase) {
    // Days no time zone changes its clocks on.
    let local = |d: u32, h: u32| {
        NaiveDate::from_ymd_opt(2026, 11, d)
            .unwrap()
            .and_hms_opt(h, 0, 0)
            .unwrap()
            .and_local_timezone(Local)
            .earliest()
            .unwrap()
            .with_timezone(&Utc)
    };
    let at = || {
        tc.draw(gs::booleans()).then(|| {
            local(
                tc.draw(gs::integers::<u32>().min_value(10).max_value(12)),
                tc.draw(gs::integers::<u32>().max_value(23)),
            )
        })
    };
    let w = Window { after: at(), before: at(), exact: tc.draw(gs::booleans()) };
    let day = |t: DateTime<Utc>| t.with_timezone(&Local).date_naive();
    let on = match (w.after, w.before) {
        (Some(a), _) if w.exact => Some(day(a)),
        (Some(a), Some(b)) if day(a) == day(b) => Some(day(a)),
        _ => None,
    };
    assert_eq!(w.day(), on);
    let named = |t: DateTime<Utc>| day(t).format("%-d %b %Y").to_string();
    let label = match (on, w.after, w.before) {
        (Some(d), ..) => format!("on {}", d.format("%-d %b %Y")),
        (None, Some(a), Some(b)) => format!("between {} and {}", named(a), named(b)),
        (None, None, Some(b)) => format!("before {}", named(b)),
        (None, Some(a), None) => format!("after {}", named(a)),
        (None, None, None) => "unknown".into(),
    };
    assert_eq!(w.label(), label);
}

/// A track typed in is `Artist - Title` split at the first ` - ` (else a title), trimmed, and
/// named back that way; a library track keeps its details trimmed, its file, and where it
/// came from.
#[hegel::test(test_cases = 300)]
fn entries_name_the_track_they_hold(tc: TestCase) {
    let part = || tc.draw(gs::sampled_from(vec!["", " Floorplan", "Never Grow Old ", "A - B", "ID"]));
    let (artist, title) = (part(), part());
    let text = match tc.draw(gs::booleans()) {
        true => format!("{artist} - {title}"),
        false => title.to_string(),
    };
    let e = SetEntry::added(&text);
    let (a, t) = text.split_once(" - ").map_or(("", text.as_str()), |(a, t)| (a.trim(), t.trim()));
    assert_eq!(
        (e.artist.as_str(), e.title.as_str(), e.origin, e.file.as_deref()),
        (a, t.trim(), EntryOrigin::Added, None)
    );
    let name = match (a, t.trim()) {
        ("", t) => t.to_string(),
        (a, "") => a.to_string(),
        (a, t) => format!("{a} - {t}"),
    };
    assert_eq!(e.name(), name);

    let file = tc.draw(gs::sampled_from(vec!["", "/Contents/A/B.mp3", "/CONTENTS/x.MP3"]));
    let mut track = loclib_model::test_util::track(1, artist, title, file);
    track.album = format!(" {title} ");
    let origin = if tc.draw(gs::booleans()) { EntryOrigin::Session } else { EntryOrigin::Recording };
    let session = tc.draw(gs::sampled_from(vec![None, Some("ab12")]));
    let e = SetEntry::from_track(&track, origin, session);
    assert_eq!(
        (e.artist.as_str(), e.title.as_str(), e.album.as_str(), e.bpm, e.duration_secs),
        (artist.trim(), title.trim(), title.trim(), track.bpm, track.duration_secs)
    );
    assert_eq!((e.origin, e.session.as_deref(), e.start_seconds, e.unnamed), (origin, session, None, false));
    assert_eq!(e.file.as_deref(), Some(file).filter(|f| !f.is_empty()));
    assert_eq!(e.file_key(), Some(file).filter(|f| !f.is_empty()).map(|f| f[1..].to_lowercase()));
}

/// A session's short id is its first 8 characters (all of a shorter one), and it names the
/// session again: `find` takes it back to that session unless another id starts the same way.
/// Ids are hex hashes; a two-letter alphabet makes them share prefixes.
#[hegel::test(test_cases = 100)]
fn sessions_are_listed_by_a_short_id(tc: TestCase) {
    use crate::sets::build::tests::session;
    assert_eq!(session("0123456789abcdef", &[], Window::default()).short_id(), "01234567");
    assert_eq!(session("ab12", &[], Window::default()).short_id(), "ab12");

    let ids = tc.draw(gs::hashsets(gs::text().alphabet("ab").min_size(4).max_size(12)).min_size(1).max_size(4));
    let sessions: Vec<Session> = ids.iter().map(|id| session(id, &[], Window::default())).collect();
    for s in &sessions {
        let short = s.short_id();
        assert!(s.id.starts_with(short), "{short:?} doesn't start {:?}", s.id);
        assert_eq!(short.chars().count(), s.id.chars().count().min(8));
        let sharing = sessions.iter().filter(|o| o.id.starts_with(short)).count();
        if sharing == 1 {
            assert_eq!(super::session::find(&sessions, short).unwrap().id, s.id);
        }
    }
}

/// A file's MusicBrainz IDs are read as Picard tags them, whichever way ID3 joins several (a
/// frame each in ID3v2.4, `/` in ID3v2.3); anything that isn't a UUID is left out, and a file
/// without tags, or that isn't audio, has none.
#[test]
fn musicbrainz_ids_are_read_from_the_tags() {
    use lofty::config::WriteOptions;
    use lofty::tag::{Tag, TagType};
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("glue.wav");
    std::fs::write(&path, crate::probe::tests::wav(8_000, 16, 0.25)).unwrap();
    assert!(MusicBrainzIds::read(&path).is_empty(), "no tags");
    let ids = [
        "c1a3e5c0-7b5e-4d51-9d0e-3c8b1f1f8a2b",
        "5f6f2c4e-1d7a-4b4e-8f5c-2a9b3c1d0e7f",
        "e0ddb3c4-0a5c-44d5-9c38-04d9a2e4e8a0",
    ];
    let uuid = |i: usize| Uuid::parse_str(ids[i]).unwrap();
    let mut tag = Tag::new(TagType::Id3v2);
    tag.insert_text(ItemKey::MusicBrainzRecordingId, ids[0].into());
    tag.insert_text(ItemKey::MusicBrainzReleaseId, "not a uuid".into());
    tag.push(lofty::tag::TagItem::new(
        ItemKey::MusicBrainzArtistId,
        lofty::tag::ItemValue::Text(format!("{}/{}", ids[1], ids[2])),
    ));
    tag.push(lofty::tag::TagItem::new(ItemKey::MusicBrainzArtistId, lofty::tag::ItemValue::Text(ids[0].into())));
    tag.save_to_path(&path, WriteOptions::default()).unwrap();
    assert_eq!(
        MusicBrainzIds::read(&path),
        MusicBrainzIds { recording: Some(uuid(0)), release: None, artists: vec![uuid(1), uuid(2), uuid(0)] }
    );
    std::fs::write(&path, b"not audio").unwrap();
    assert!(MusicBrainzIds::read(&path).is_empty());
}
