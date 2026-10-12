//! Changing a set: its details, and its tracklist.
//!
//! Tracklist positions start at 1 and always name the tracklist as it was before the edit, so
//! the changes of one edit don't shift each other: dropping track 1 and moving track 5 to 2 puts
//! the track that was fifth where the track that was second is (now first). A track moved up
//! lands just before the track that was at `to`, one moved down just after it, so a move on its
//! own leaves the track at `to`. An inserted track lands just before the track that was at `at`
//! (one past the last appends). Tracks landing in one place keep the order they were given in,
//! inserted ones first.

use chrono::NaiveDate;
use rustc_hash::FxHashSet;
use schemars::JsonSchema;
use serde::Deserialize;

use super::{EntryOrigin, Partner, Set, SetEntry};
use crate::model::Library;
use crate::query::{QueryContext, TrackSelector};
use crate::{Error, Result};

/// A set's details. Left out, a field stays as it is; empty (`""`), it's cleared.
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct SetFields {
    #[serde(default)]
    pub title: Option<String>,
    /// The day it was played, `2026-10-03`.
    #[serde(default)]
    pub date: Option<String>,
    /// `club`, `festival`, `radio`, `livestream`, `mix`, or another word.
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub venue: Option<String>,
    #[serde(default)]
    pub city: Option<String>,
    /// ISO 3166 code, `GB`.
    #[serde(default)]
    pub country: Option<String>,
    /// The `at://` URI of the event's `community.lexicon.calendar.event` record.
    #[serde(default)]
    pub event: Option<String>,
    /// Who they played with: names, or `did:...` for an Atmosphere account. Replaces the list;
    /// empty clears it.
    #[serde(default)]
    pub with: Option<Vec<String>>,
    /// Where people can listen to it (`https://...`).
    #[serde(default)]
    pub link: Option<String>,
    /// Private notes, never published.
    #[serde(default)]
    pub notes: Option<String>,
}

impl SetFields {
    /// The day given, if one is.
    pub fn date(&self) -> Result<Option<NaiveDate>> {
        let Some(d) = self.date.as_deref().map(str::trim) else { return Ok(None) };
        if d.is_empty() {
            return Err(Error::invalid("a set needs its day: give one like 2026-10-03"));
        }
        NaiveDate::parse_from_str(d, "%Y-%m-%d")
            .map(Some)
            .map_err(|_| Error::invalid(format!("{d:?} isn't a day: give one like 2026-10-03")))
    }

    /// Write the fields given into `set`, once they're all valid.
    pub fn apply(&self, set: &mut Set) -> Result<()> {
        let date = self.date()?;
        let text = |v: &Option<String>| v.as_deref().map(|s| Some(s.trim().to_string()).filter(|s| !s.is_empty()));
        let country = text(&self.country).map(|c| c.map(|c| c.to_ascii_uppercase()));
        if let Some(Some(c)) = &country
            && !(c.len() == 2 && c.bytes().all(|b| b.is_ascii_alphabetic()))
        {
            return Err(Error::invalid(format!("{c:?} isn't a country code: give two letters, like GB")));
        }
        let event = text(&self.event);
        if let Some(Some(e)) = &event
            && !e.starts_with("at://")
        {
            return Err(Error::invalid(format!("{e:?} isn't an event: give the at:// URI of its record")));
        }
        let link = text(&self.link);
        if let Some(Some(l)) = &link
            && !(l.starts_with("https://") || l.starts_with("http://"))
        {
            return Err(Error::invalid(format!("{l:?} isn't a link: give it with https://")));
        }
        if let Some(d) = date {
            set.date = d;
        }
        let fields = [
            (&mut set.title, text(&self.title)),
            (&mut set.kind, text(&self.kind)),
            (&mut set.venue, text(&self.venue)),
            (&mut set.city, text(&self.city)),
            (&mut set.country, country),
            (&mut set.event, event),
            (&mut set.link, link),
            (&mut set.notes, text(&self.notes)),
        ];
        for (field, value) in fields {
            if let Some(v) = value {
                *field = v;
            }
        }
        if let Some(with) = &self.with {
            set.with = partners(with);
        }
        Ok(())
    }
}

/// Who a set was played with: a `did:...` is an Atmosphere account (named by the DID until a
/// handle is looked up), anything else a name. Handles stay names: resolving one takes the
/// network.
fn partners(with: &[String]) -> Vec<Partner> {
    with.iter()
        .map(|w| w.trim())
        .filter(|w| !w.is_empty())
        .map(|w| Partner { name: w.to_string(), did: w.starts_with("did:").then(|| w.to_string()) })
        .collect()
}

/// Changes to a tracklist, by position (see the module docs for how they combine).
#[derive(Debug, Clone, Default)]
pub(crate) struct Changes {
    pub drop: Vec<usize>,
    /// `(from, to)`.
    pub moves: Vec<(usize, usize)>,
    /// `(at, text)`: a search for a library track, or `Artist - Title`.
    pub insert: Vec<(usize, String)>,
    /// Published without naming the track.
    pub unnamed: Vec<usize>,
    pub named: Vec<usize>,
}

impl Changes {
    pub fn is_empty(&self) -> bool {
        self.drop.is_empty()
            && self.moves.is_empty()
            && self.insert.is_empty()
            && self.unnamed.is_empty()
            && self.named.is_empty()
    }

    /// Refuse positions a tracklist of `len` tracks doesn't have, and changes that contradict
    /// each other.
    pub fn check(&self, len: usize) -> Result<()> {
        let track = |p: usize| match (p, len) {
            (1.., _) if p <= len => Ok(()),
            (_, 0) => Err(Error::invalid(format!("there's no track {p}: the set has no tracks"))),
            _ => Err(Error::invalid(format!("there's no track {p}: the set has {len}"))),
        };
        let moved: Vec<usize> = self.moves.iter().map(|m| m.0).collect();
        for &p in self.drop.iter().chain(&moved).chain(&self.unnamed).chain(&self.named) {
            track(p)?;
        }
        for &(_, to) in &self.moves {
            track(to)?;
        }
        if let Some((at, _)) = self.insert.iter().find(|(at, _)| *at == 0 || *at > len + 1) {
            return Err(Error::invalid(format!(
                "can't insert at {at}: positions go from 1 to {} (one past the last track appends)",
                len + 1
            )));
        }
        let mut seen = FxHashSet::default();
        if let Some(p) = moved.iter().find(|p| !seen.insert(**p)) {
            return Err(Error::invalid(format!("track {p} is moved twice")));
        }
        if let Some(p) = self.drop.iter().find(|p| moved.contains(p)) {
            return Err(Error::invalid(format!("track {p} can't be dropped and moved")));
        }
        if let Some(p) = self.unnamed.iter().find(|p| self.named.contains(p)) {
            return Err(Error::invalid(format!("track {p} can't be named and unnamed")));
        }
        Ok(())
    }

    /// `tracks` changed, once every change is checked. `find` turns an insert's text into the
    /// entry that goes in.
    pub fn apply(&self, tracks: &[SetEntry], mut find: impl FnMut(&str) -> Result<SetEntry>) -> Result<Vec<SetEntry>> {
        self.check(tracks.len())?;
        let n = tracks.len();
        // What lands just before and just after each original position (`before[n + 1]` appends).
        let mut before: Vec<Vec<SetEntry>> = vec![Vec::new(); n + 2];
        let mut after: Vec<Vec<SetEntry>> = vec![Vec::new(); n + 1];
        for (at, text) in &self.insert {
            before[*at].push(find(text)?);
        }
        let mut kept: Vec<Option<SetEntry>> = tracks.iter().cloned().map(Some).collect();
        for (positions, unnamed) in [(&self.unnamed, true), (&self.named, false)] {
            for p in positions {
                if let Some(e) = kept[p - 1].as_mut() {
                    e.unnamed = unnamed;
                }
            }
        }
        for &(from, to) in self.moves.iter().filter(|(from, to)| from != to) {
            let Some(e) = kept[from - 1].take() else { continue };
            match to < from {
                true => before[to].push(e),
                false => after[to].push(e),
            }
        }
        for p in &self.drop {
            kept[p - 1] = None;
        }
        let mut out = Vec::with_capacity(n + self.insert.len());
        for p in 1..=n {
            out.append(&mut before[p]);
            out.extend(kept[p - 1].take());
            out.append(&mut after[p]);
        }
        out.append(&mut before[n + 1]);
        Ok(out)
    }
}

/// The entry to insert for `text`: the library's track when exactly one matches it as a search
/// (`Artist - Title`, part of it, or a file path), else the text as typed (`Artist - Title`).
/// With a note saying which.
pub(crate) fn lookup(text: &str, library: Option<&Library>) -> Result<(SetEntry, String)> {
    let text = text.trim();
    if text.is_empty() {
        return Err(Error::invalid("say what to insert: a track to look for, or Artist - Title"));
    }
    let Some(lib) = library else {
        return Ok((SetEntry::added(text), format!("{text:?} went in as typed: no drive to look it up on")));
    };
    let selector = TrackSelector { query: Some(text.to_string()), ..Default::default() };
    let ids = selector.select_ordered(&QueryContext::new(lib), true)?;
    Ok(match lib.tracks_by_id(&ids).as_slice() {
        [t] => (
            SetEntry::from_track(t, EntryOrigin::Added, None),
            format!("{} went in from the library", t.display_name()),
        ),
        [] => (SetEntry::added(text), format!("{text:?} went in as typed: no track matches it")),
        many => (SetEntry::added(text), format!("{text:?} went in as typed: {} tracks match it", many.len())),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use loclib_model::test_util::track;

    fn tracks(n: u32) -> Vec<SetEntry> {
        (1..=n).map(|i| SetEntry::added(&format!("A - {i}"))).collect()
    }

    fn titles(tracks: &[SetEntry]) -> Vec<&str> {
        tracks.iter().map(|e| e.title.as_str()).collect()
    }

    fn typed(text: &str) -> Result<SetEntry> {
        Ok(SetEntry::added(text))
    }

    #[test]
    fn positions_name_the_tracklist_before_the_edit() {
        let c = Changes { drop: vec![1], moves: vec![(5, 2)], ..Default::default() };
        assert_eq!(titles(&c.apply(&tracks(6), typed).unwrap()), ["5", "2", "3", "4", "6"]);

        let up = Changes { moves: vec![(5, 2)], ..Default::default() };
        assert_eq!(titles(&up.apply(&tracks(6), typed).unwrap()), ["1", "5", "2", "3", "4", "6"]);
        let down = Changes { moves: vec![(2, 5)], ..Default::default() };
        assert_eq!(titles(&down.apply(&tracks(6), typed).unwrap()), ["1", "3", "4", "5", "2", "6"]);
        let still = Changes { moves: vec![(3, 3)], ..Default::default() };
        assert_eq!(titles(&still.apply(&tracks(3), typed).unwrap()), ["1", "2", "3"]);

        let c = Changes {
            drop: vec![2, 3],
            insert: vec![(3, "X - a".into()), (5, "X - end".into()), (3, "X - b".into())],
            moves: vec![(4, 3)],
            ..Default::default()
        };
        assert_eq!(titles(&c.apply(&tracks(4), typed).unwrap()), ["1", "a", "b", "4", "end"]);
    }

    #[test]
    fn tracks_are_marked_unnamed_and_named() {
        let mut list = tracks(3);
        list[2].unnamed = true;
        let c = Changes { unnamed: vec![1], named: vec![3], moves: vec![(1, 3)], ..Default::default() };
        let out = c.apply(&list, typed).unwrap();
        assert_eq!(titles(&out), ["2", "3", "1"]);
        assert_eq!(out.iter().map(|e| e.unnamed).collect::<Vec<_>>(), [false, false, true]);
    }

    #[test]
    fn bad_positions_are_refused_before_anything_changes() {
        let err = |c: Changes, len: u32| {
            let mut asked = false;
            let e = c.apply(&tracks(len), |t| {
                asked = true;
                typed(t)
            });
            assert!(!asked, "nothing is looked up for a refused edit");
            e.unwrap_err().to_string()
        };
        assert_eq!(err(Changes { drop: vec![4], ..Default::default() }, 3), "there's no track 4: the set has 3");
        assert_eq!(err(Changes { drop: vec![0], ..Default::default() }, 3), "there's no track 0: the set has 3");
        assert_eq!(
            err(Changes { named: vec![1], ..Default::default() }, 0),
            "there's no track 1: the set has no tracks"
        );
        assert_eq!(err(Changes { moves: vec![(1, 4)], ..Default::default() }, 3), "there's no track 4: the set has 3");
        let insert = |at: usize| Changes { insert: vec![(at, "x".into())], ..Default::default() };
        assert_eq!(err(insert(5), 3), "can't insert at 5: positions go from 1 to 4 (one past the last track appends)");
        assert!(err(insert(0), 3).starts_with("can't insert at 0"));
        assert_eq!(err(Changes { moves: vec![(1, 2), (1, 3)], ..Default::default() }, 3), "track 1 is moved twice");
        assert_eq!(
            err(Changes { drop: vec![2], moves: vec![(2, 1)], ..Default::default() }, 3),
            "track 2 can't be dropped and moved"
        );
        assert_eq!(
            err(Changes { unnamed: vec![2], named: vec![2], ..Default::default() }, 3),
            "track 2 can't be named and unnamed"
        );
        assert_eq!(titles(&insert(4).apply(&tracks(3), typed).unwrap()), ["1", "2", "3", "x"]);
    }

    #[test]
    fn inserts_take_the_one_library_track_that_matches() {
        let lib = Library {
            tracks: vec![
                track(1, "Floorplan", "Never Grow Old", "/Contents/f.mp3"),
                track(2, "Floorplan", "Tell You No Lie", "/Contents/g.mp3"),
            ],
            ..Default::default()
        };
        let (e, note) = lookup("floorplan - never grow", Some(&lib)).unwrap();
        assert_eq!(
            (e.title.as_str(), e.file.as_deref(), e.origin),
            ("Never Grow Old", Some("/Contents/f.mp3"), EntryOrigin::Added)
        );
        assert_eq!(note, "Floorplan - Never Grow Old went in from the library");
        let (e, note) = lookup("Floorplan", Some(&lib)).unwrap();
        assert_eq!((e.artist.as_str(), e.file.as_deref()), ("", None));
        assert_eq!(note, "\"Floorplan\" went in as typed: 2 tracks match it");
        let (e, note) = lookup("Someone - Dubplate", Some(&lib)).unwrap();
        assert_eq!((e.artist.as_str(), e.title.as_str()), ("Someone", "Dubplate"));
        assert_eq!(note, "\"Someone - Dubplate\" went in as typed: no track matches it");
        assert!(lookup("x", None).unwrap().1.ends_with("no drive to look it up on"));
        assert!(lookup("  ", Some(&lib)).is_err());
    }

    #[test]
    fn fields_are_set_cleared_and_checked() {
        let mut set = crate::sets::query::tests::set("3m2abcdefgh22", "2026-10-03", None);
        set.city = Some("Leeds".into());
        let fields = SetFields {
            title: Some(" Friday ".into()),
            date: Some("2026-10-02".into()),
            country: Some("gb".into()),
            city: Some(String::new()),
            with: Some(vec!["Someone".into(), "did:plc:abc".into(), " ".into()]),
            ..Default::default()
        };
        fields.apply(&mut set).unwrap();
        assert_eq!(set.title.as_deref(), Some("Friday"));
        assert_eq!(set.date.to_string(), "2026-10-02");
        assert_eq!((set.country.as_deref(), set.city.as_deref()), (Some("GB"), None));
        assert_eq!(
            set.with,
            [
                Partner { name: "Someone".into(), did: None },
                Partner { name: "did:plc:abc".into(), did: Some("did:plc:abc".into()) }
            ]
        );
        SetFields { with: Some(Vec::new()), ..Default::default() }.apply(&mut set).unwrap();
        assert!(set.with.is_empty());

        let before = set.clone();
        for bad in [
            SetFields { date: Some(String::new()), ..Default::default() },
            SetFields { date: Some("3 Oct".into()), ..Default::default() },
            SetFields { title: Some("Changed".into()), country: Some("GBR".into()), ..Default::default() },
            SetFields { event: Some("https://example.com/e".into()), ..Default::default() },
            SetFields { link: Some("mixcloud.com/x".into()), ..Default::default() },
        ] {
            assert!(bad.apply(&mut set).is_err(), "{bad:?}");
        }
        assert_eq!(set, before, "a refused edit changes nothing");
    }

    use hegel::TestCase;
    use hegel::generators as gs;

    /// A track typed in by its file path is that library track; any other text is the library's
    /// track when it's the only one that matches, else it goes in as typed, saying how many
    /// matched; with no library to look in, as typed.
    #[hegel::test(test_cases = 300)]
    fn inserts_are_the_library_track_or_as_typed(tc: TestCase) {
        const NAMES: &[(&str, &str)] =
            &[("Floorplan", "Never Grow Old"), ("Floorplan", "Tell You No Lie"), ("Someone", "Dubplate"), ("", "ID")];
        let lib = Library {
            tracks: (1..=tc.draw(gs::integers::<u32>().min_value(1).max_value(5)))
                .map(|n| {
                    let (artist, title) = tc.draw(gs::sampled_from(NAMES.to_vec()));
                    track(n, artist, title, &format!("/Contents/{n}.mp3"))
                })
                .collect(),
            ..Default::default()
        };
        let n = tc.draw(gs::integers::<usize>().max_value(lib.tracks.len() - 1));
        let by_path = &lib.tracks[n];
        let (e, note) = lookup(&format!(" {} ", by_path.file_path), Some(&lib)).unwrap();
        assert_eq!(e, SetEntry::from_track(by_path, EntryOrigin::Added, None));
        assert_eq!(note, format!("{} went in from the library", by_path.display_name()));

        let text = tc.draw(gs::sampled_from(vec![
            "floorplan",
            "Never Grow Old",
            "someone - dubplate",
            "Nobody - Nothing",
            "ID",
        ]));
        let (e, note) = lookup(text, Some(&lib)).unwrap();
        match lib.tracks.iter().find(|t| e == SetEntry::from_track(t, EntryOrigin::Added, None)) {
            Some(t) => assert_eq!(note, format!("{} went in from the library", t.display_name())),
            None => {
                assert_eq!(e, SetEntry::added(text));
                let matched = note.strip_prefix(&format!("{text:?} went in as typed: ")).unwrap();
                let many = matched.strip_suffix(" tracks match it").map(|k| k.parse::<usize>().unwrap());
                assert!(
                    matched == "no track matches it" || many.is_some_and(|k| (2..=lib.tracks.len()).contains(&k)),
                    "{note}"
                );
            }
        }
        assert!(lookup(&" ".repeat(n), Some(&lib)).is_err(), "a blank is refused");
        let (e, note) = lookup(text, None).unwrap();
        assert_eq!((e, note), (SetEntry::added(text), format!("{text:?} went in as typed: no drive to look it up on")));
    }
}
