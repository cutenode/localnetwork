//! Questions across sets: when and where a track was played, and which sets a listing shows.

use chrono::NaiveDate;
use schemars::JsonSchema;
use serde::Serialize;

use super::{Set, SetBook, newest};
use crate::model::normalize;
use crate::paths;

/// A track's place in a set.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct Appearance {
    /// The set's id.
    pub set: String,
    pub date: NaiveDate,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub venue: Option<String>,
    /// From 1.
    pub position: usize,
    /// Seconds from the start of the set, when known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_seconds: Option<u32>,
    /// The track as the set has it: `Artist - Title`.
    pub track: String,
}

/// Every time a track matching `query` was played in `book`'s sets, newest first: by file when
/// the query is a drive path (`/Contents/...`), else by `Artist - Title` containing it (ignoring
/// case and spacing).
pub(crate) fn played(book: &SetBook, query: &str) -> Vec<Appearance> {
    let query = query.trim();
    let file = query.starts_with('/').then(|| paths::key(query).to_lowercase());
    let needle = normalize(query);
    let matches = |e: &super::SetEntry| match &file {
        Some(f) => e.file_key().as_ref() == Some(f),
        None => normalize(&e.name()).contains(&needle),
    };
    let mut sets: Vec<&Set> = book.sets.iter().collect();
    sets.sort_by(|a, b| Set::newest_first(a, b));
    sets.into_iter()
        .flat_map(|s| {
            s.tracks.iter().enumerate().filter(|(_, e)| matches(e)).map(|(i, e)| Appearance {
                set: s.id.clone(),
                date: s.date,
                title: s.title.clone(),
                venue: s.venue.clone(),
                position: i + 1,
                start_seconds: e.start_seconds,
                track: e.name(),
            })
        })
        .collect()
}

/// Which sets a listing shows.
#[derive(Debug, Clone, Default)]
pub(crate) struct SetFilter {
    /// Part of the venue's name, ignoring case.
    pub venue: Option<String>,
    /// The kind, ignoring case.
    pub kind: Option<String>,
    /// Played on or after.
    pub since: Option<NaiveDate>,
    /// Played on or before.
    pub until: Option<NaiveDate>,
}

impl SetFilter {
    pub fn matches(&self, s: &Set) -> bool {
        let has = |field: &Option<String>, wanted: &Option<String>, whole: bool| match wanted {
            None => true,
            Some(w) => field.as_deref().map(normalize).is_some_and(|f| match whole {
                true => f == normalize(w),
                false => f.contains(&normalize(w)),
            }),
        };
        has(&s.venue, &self.venue, false)
            && has(&s.kind, &self.kind, true)
            && self.since.is_none_or(|d| s.date >= d)
            && self.until.is_none_or(|d| s.date <= d)
    }

    /// The newest of `book`'s sets it shows.
    pub fn latest<'a>(&self, book: &'a SetBook) -> Option<&'a Set> {
        newest(book.sets.iter().enumerate().filter(|(_, s)| self.matches(s))).map(|i| &book.sets[i])
    }
}

#[cfg(test)]
pub(crate) mod tests {
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
        warehouse.tracks =
            vec![entry("Floorplan - Never Grow Old", "/Contents/f.mp3"), entry("A - B", "/Contents/b.mp3")];
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
}
