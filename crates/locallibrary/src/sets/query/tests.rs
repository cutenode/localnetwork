use chrono::{Duration, Utc};

use super::*;
use crate::sets::{EntryOrigin, SetEntry};

pub(crate) fn set(id: &str, date: &str, title: Option<&str>) -> Set {
    let at = Utc::now();
    Set {
        id: id.into(),
        title: title.map(String::from),
        date: NaiveDate::parse_from_str(date, "%Y-%m-%d").unwrap(),
        created_at: at,
        updated_at: at,
        ..Default::default()
    }
}

fn entry(name: &str, file: &str) -> SetEntry {
    SetEntry { file: Some(file.into()), ..SetEntry::added(name) }
}

fn book() -> SetBook {
    let mut warehouse = set("3m2aaaaaaaa22", "2026-09-12", Some("Warm-up"));
    warehouse.venue = Some("The Warehouse".into());
    warehouse.kind = Some("club".into());
    warehouse.tracks = vec![entry("Floorplan - Never Grow Old", "/Contents/f.mp3"), entry("A - B", "/Contents/b.mp3")];
    let mut later = set("3m2bbbbbbbb22", "2026-10-03", Some("Friday"));
    later.venue = Some("Warehouse Project".into());
    later.kind = Some("Club".into());
    later.tracks = vec![entry("C - D", "/Contents/d.mp3"), entry("Floorplan - Never Grow Old", "/Contents/F.mp3")];
    later.tracks[1].start_seconds = Some(300);
    // Played the same day, but declared after.
    let mut radio = set("3m2cccccccc22", "2026-10-03", Some("Radio"));
    radio.kind = Some("radio".into());
    radio.created_at = later.created_at + Duration::seconds(1);
    radio.tracks =
        vec![SetEntry { origin: EntryOrigin::Recording, ..entry("FLOORPLAN - never  grow old (edit)", "/x.mp3") }];
    SetBook { sets: vec![warehouse, later, radio], ..Default::default() }
}

#[test]
fn a_track_is_found_in_every_set_it_was_played_in() {
    let book = book();
    let found = played(&book, "floorplan - never grow");
    let at: Vec<(&str, usize)> = found.iter().map(|a| (a.set.as_str(), a.position)).collect();
    assert_eq!(at, [("3m2cccccccc22", 1), ("3m2bbbbbbbb22", 2), ("3m2aaaaaaaa22", 1)], "newest first");
    assert_eq!(found[1].start_seconds, Some(300));
    assert_eq!(found[2].venue.as_deref(), Some("The Warehouse"));
    assert_eq!(found[2].track, "Floorplan - Never Grow Old");

    let by_file: Vec<String> = played(&book, "/Contents/f.MP3").into_iter().map(|a| a.set).collect();
    assert_eq!(by_file, ["3m2bbbbbbbb22", "3m2aaaaaaaa22"], "by file, not by name");
    assert!(played(&book, "nothing like it").is_empty());
}

#[test]
fn listings_filter_by_venue_kind_and_day() {
    let book = book();
    let ids = |f: SetFilter| book.sets.iter().filter(|s| f.matches(s)).map(|s| &s.id[4..5]).collect::<String>();
    assert_eq!(ids(SetFilter::default()), "abc");
    assert_eq!(ids(SetFilter { venue: Some("WAREHOUSE".into()), ..Default::default() }), "ab");
    assert_eq!(ids(SetFilter { venue: Some("the ware".into()), ..Default::default() }), "a");
    assert_eq!(ids(SetFilter { kind: Some("club".into()), ..Default::default() }), "ab");
    assert_eq!(ids(SetFilter { kind: Some("clu".into()), ..Default::default() }), "", "a kind is matched whole");
    let day = |d: &str| Some(NaiveDate::parse_from_str(d, "%Y-%m-%d").unwrap());
    assert_eq!(ids(SetFilter { since: day("2026-10-03"), ..Default::default() }), "bc");
    assert_eq!(ids(SetFilter { until: day("2026-10-02"), ..Default::default() }), "a");
    assert_eq!(ids(SetFilter { since: day("2026-09-12"), until: day("2026-09-12"), ..Default::default() }), "a");

    let latest = |venue: &str| SetFilter { venue: Some(venue.into()), ..Default::default() }.latest(&book);
    assert_eq!(latest("warehouse").unwrap().id, "3m2bbbbbbbb22");
    assert!(latest("fabric").is_none());
    assert_eq!(SetFilter::default().latest(&book).unwrap().id, "3m2cccccccc22", "declared last on the day");
}

use hegel::TestCase;
use hegel::generators as gs;

/// A book of sets on a few days, at a few venues, of a few kinds, playing a few tracks.
#[hegel::test_helper]
fn some_book(tc: &TestCase) -> SetBook {
    let start = Utc::now();
    let sets = (0..tc.draw(gs::integers::<usize>().max_value(5)))
        .map(|i| {
            let date = format!("2026-10-0{}", tc.draw(gs::integers::<u8>().min_value(1).max_value(4)));
            let mut s = set(&format!("3m2{i}aaaaaaa22"), &date, None);
            s.created_at = start + Duration::seconds(tc.draw(gs::integers::<i64>().min_value(0).max_value(3)));
            s.venue = tc
                .draw(gs::sampled_from(vec![None, Some("The Warehouse"), Some("Warehouse Project"), Some("NTS")]))
                .map(String::from);
            s.kind = tc.draw(gs::sampled_from(vec![None, Some("club"), Some("Club"), Some("radio")])).map(String::from);
            s.tracks = (0..tc.draw(gs::integers::<usize>().max_value(4)))
                .map(|_| {
                    let n = tc.draw(gs::integers::<u8>().min_value(1).max_value(4));
                    let mut e = entry(&format!("Artist {n} - Track {n}"), &format!("/Contents/{n}.mp3"));
                    e.file = tc.draw(gs::booleans()).then(|| format!("/Contents/{n}.mp3"));
                    e.start_seconds = tc.draw(gs::booleans()).then_some(u32::from(n) * 60);
                    e
                })
                .collect();
            s
        })
        .collect();
    SetBook { sets, ..Default::default() }
}

/// The book's sets newest first: by day, then the one declared last (ties keep the book's
/// order, as a stable sort does).
fn newest_first(book: &SetBook) -> Vec<&Set> {
    let mut sets: Vec<&Set> = book.sets.iter().collect();
    sets.sort_by_key(|s| std::cmp::Reverse((s.date, s.created_at)));
    sets
}

/// A track is found at every place it has in the sets, newest set first: by file (in any
/// case) for a path, else by its name containing the query (ignoring case and spacing).
#[hegel::test(test_cases = 300)]
fn a_track_is_found_wherever_it_was_played(tc: TestCase) {
    let book = some_book(&tc);
    let wanted = tc.draw(gs::integers::<u8>().min_value(1).max_value(5));
    let query = tc.draw(gs::sampled_from(vec![
        format!("/Contents/{wanted}.mp3"),
        format!(" /CONTENTS/{wanted}.MP3 "),
        format!("artist {wanted}  -  track"),
        format!("TRACK {wanted}"),
        "artist".to_string(),
    ]));
    let by_file = query.trim().starts_with('/');
    let want: Vec<(String, usize, Option<u32>, String)> = newest_first(&book)
        .into_iter()
        .flat_map(|s| s.tracks.iter().enumerate().map(move |(i, e)| (s, i, e)))
        .filter(|(_, _, e)| match by_file {
            true => e.file.as_deref() == Some(format!("/Contents/{wanted}.mp3").as_str()),
            false => {
                e.name().to_lowercase().contains(&query.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase())
            }
        })
        .map(|(s, i, e)| (s.id.clone(), i + 1, e.start_seconds, e.name()))
        .collect();
    let got: Vec<(String, usize, Option<u32>, String)> =
        played(&book, &query).into_iter().map(|a| (a.set, a.position, a.start_seconds, a.track)).collect();
    assert_eq!(got, want, "{query:?}");
}

/// A listing shows the sets at a venue containing the one asked for, of the kind asked for
/// (whole, ignoring case), played between the days asked for; the latest of them is the
/// newest.
#[hegel::test(test_cases = 300)]
fn listings_keep_what_the_filter_asks_for(tc: TestCase) {
    let book = some_book(&tc);
    let day = |d: Option<u8>| d.map(|d| NaiveDate::from_ymd_opt(2026, 10, d.into()).unwrap());
    let filter = SetFilter {
        venue: tc
            .draw(gs::sampled_from(vec![None, Some("warehouse"), Some("THE  ware"), Some("nts")]))
            .map(String::from),
        kind: tc.draw(gs::sampled_from(vec![None, Some("CLUB"), Some("clu")])).map(String::from),
        since: day(tc.draw(gs::sampled_from(vec![None, Some(2), Some(3)]))),
        until: day(tc.draw(gs::sampled_from(vec![None, Some(2), Some(4)]))),
    };
    let squash = |s: &str| s.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase();
    let keeps = |s: &Set| {
        filter.venue.as_ref().is_none_or(|v| s.venue.as_ref().is_some_and(|x| squash(x).contains(&squash(v))))
            && filter.kind.as_ref().is_none_or(|k| s.kind.as_ref().is_some_and(|x| squash(x) == squash(k)))
            && filter.since.is_none_or(|d| s.date >= d)
            && filter.until.is_none_or(|d| s.date <= d)
    };
    for s in &book.sets {
        assert_eq!(filter.matches(s), keeps(s), "{filter:?} on {s:?}");
    }
    let latest = newest_first(&book).into_iter().find(|s| keeps(s));
    assert_eq!(filter.latest(&book).map(|s| &s.id), latest.map(|s| &s.id));
}
