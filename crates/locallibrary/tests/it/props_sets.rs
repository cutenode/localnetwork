//! Property tests for sets and sessions through the API: a store's sets declared, edited, found,
//! listed, asked about, written out and deleted, against an in-memory model after every step;
//! `sets.json` round trips; set ids; play logs imported from Beat Link Trigger and history lists
//! on drives gathered as sessions, found, dismissed, built into sets and suggested as a model of
//! them predicts.

use crate::common;

use indexmap::IndexMap;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Duration, Local, NaiveDate, TimeZone, Utc};
use hegel::generators as gs;
use hegel::{HealthCheck, TestCase};
use locallibrary::api::{
    self, DismissSessions, EditSet, ImportSessionLog, Insert, Move, NewSet, SessionsRequest, SetExport, SetFields,
    SetFormat, SetRef, SetsQuery, SuggestSets, TrackPlaysQuery,
};
use locallibrary::device::Device;
use locallibrary::import::read_table;
use locallibrary::model::normalize;
use locallibrary::progress::Ctx;
use locallibrary::sets::{
    self, EntryOrigin, ImportedSession, MusicBrainzIds, Partner, Recording, SETS_FILE, Session, Set, SetBook, SetEntry,
    TimedPlay, Window, session,
};
use locallibrary::store::Store;
use locallibrary::sync::{self, PullOptions};
use locallibrary::{Library, paths, pdb};

/// A store named for this thread, and the folder this thread's drives are looked for in (empty
/// unless a test puts drives there).
struct Desk {
    dir: tempfile::TempDir,
    store: Store,
    volumes: PathBuf,
}

impl Desk {
    fn new() -> Desk {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open_or_init(dir.path().join("store")).unwrap();
        let volumes = common::isolate(dir.path(), Some(store.root()));
        Desk { dir, store, volumes }
    }

    fn book_bytes(&self) -> Option<Vec<u8>> {
        std::fs::read(self.store.root().join(SETS_FILE)).ok()
    }

    fn book(&self) -> SetBook {
        SetBook::load(&self.store).unwrap()
    }
}

/// One of `items`, for values Hegel can't print.
#[hegel::test_helper]
fn one_of<T: Clone>(tc: &TestCase, items: &[T]) -> T {
    items[tc.draw(gs::integers::<usize>().max_value(items.len() - 1))].clone()
}

fn day(y: i32, m: u32, d: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, d).unwrap()
}

/// What a detail given becomes: `Ok(None)` clears it, `Err(())` is refused.
type Want = Result<Option<&'static str>, ()>;

const TITLES: &[(&str, Want)] = &[
    ("", Ok(None)),
    (" Friday ", Ok(Some("Friday"))),
    ("friday", Ok(Some("friday"))),
    ("Warm-up", Ok(Some("Warm-up"))),
];
const KINDS: &[(&str, Want)] = &[("", Ok(None)), ("club", Ok(Some("club"))), ("Club ", Ok(Some("Club")))];
const VENUES: &[(&str, Want)] = &[
    ("", Ok(None)),
    ("The Warehouse", Ok(Some("The Warehouse"))),
    (" Warehouse Project", Ok(Some("Warehouse Project"))),
    ("NTS", Ok(Some("NTS"))),
];
const CITIES: &[(&str, Want)] = &[("", Ok(None)), (" Leeds ", Ok(Some("Leeds")))];
const COUNTRIES: &[(&str, Want)] =
    &[("", Ok(None)), ("gb", Ok(Some("GB"))), (" Gb ", Ok(Some("GB"))), ("GBR", Err(())), ("g1", Err(()))];
const EVENTS: &[(&str, Want)] = &[
    ("", Ok(None)),
    (
        "at://did:plc:abc/community.lexicon.calendar.event/1",
        Ok(Some("at://did:plc:abc/community.lexicon.calendar.event/1")),
    ),
    ("https://example.com/e", Err(())),
];
const LINKS: &[(&str, Want)] = &[
    ("", Ok(None)),
    ("https://mixcloud.com/x ", Ok(Some("https://mixcloud.com/x"))),
    ("http://x.test/set", Ok(Some("http://x.test/set"))),
    ("mixcloud.com/x", Err(())),
];
const NOTES: &[(&str, Want)] = &[("", Ok(None)), ("tired legs", Ok(Some("tired legs")))];
/// A year, month and day.
type Ymd = (i32, u32, u32);

/// A day as given, and the day it is (`None`: refused).
const DATES: &[(&str, Option<Ymd>)] = &[
    ("2026-10-01", Some((2026, 10, 1))),
    (" 2026-10-02 ", Some((2026, 10, 2))),
    ("2026-10-03", Some((2026, 10, 3))),
    ("", None),
    ("3 Oct", None),
    ("2026-02-30", None),
];
/// Who a set was played with, as given, and who that is (`None`: no one, a blank).
const PARTNERS: &[(&str, Option<(&str, bool)>)] = &[
    ("Someone", Some(("Someone", false))),
    (" did:plc:abc ", Some(("did:plc:abc", true))),
    (" ", None),
    ("B2B", Some(("B2B", false))),
];

/// Details for a set, and what each should do to it (`None`: left out).
#[derive(Debug, Clone)]
struct Details {
    fields: SetFields,
    /// Title, kind, venue, city, country, event, link, notes.
    want: [Option<Want>; 8],
    date: Option<Option<NaiveDate>>,
    with: Option<Vec<Partner>>,
}

/// A detail from `pool` (or none), one it refuses only when `refused`.
#[hegel::test_helper]
fn given(tc: &TestCase, pool: &[(&'static str, Want)], refused: bool) -> (Option<String>, Option<Want>) {
    let pool: Vec<_> = pool.iter().filter(|(_, want)| refused || want.is_ok()).collect();
    let i = tc.draw(gs::integers::<usize>().max_value(pool.len()));
    match i.checked_sub(1).map(|i| pool[i]) {
        None => (None, None),
        Some(&(text, want)) => (Some(text.into()), Some(want)),
    }
}

#[hegel::test_helper]
fn details(tc: &TestCase) -> Details {
    // Now and then, details that may be refused.
    let refused = tc.draw(gs::integers::<u8>().max_value(4)) == 0;
    let mut fields = SetFields::default();
    let mut want = [None; 8];
    let slots = [
        (&mut fields.title, TITLES),
        (&mut fields.kind, KINDS),
        (&mut fields.venue, VENUES),
        (&mut fields.city, CITIES),
        (&mut fields.country, COUNTRIES),
        (&mut fields.event, EVENTS),
        (&mut fields.link, LINKS),
        (&mut fields.notes, NOTES),
    ];
    for (i, (slot, pool)) in slots.into_iter().enumerate() {
        (*slot, want[i]) = given(tc, pool, refused);
    }
    let dates: Vec<_> = DATES.iter().filter(|(_, d)| refused || d.is_some()).copied().collect();
    let date = tc.draw(gs::optional(gs::sampled_from(dates)));
    fields.date = date.map(|(text, _)| text.to_string());
    let with = tc.draw(gs::optional(gs::vecs(gs::sampled_from(PARTNERS.to_vec())).max_size(3)));
    fields.with = with.as_ref().map(|w| w.iter().map(|(text, _)| text.to_string()).collect());
    Details {
        fields,
        want,
        date: date.map(|(_, d)| d.map(|(y, m, d)| day(y, m, d))),
        with: with.map(|w| {
            w.iter()
                .filter_map(|(_, p)| *p)
                .map(|(name, did)| Partner { name: name.into(), did: did.then(|| name.into()) })
                .collect()
        }),
    }
}

/// `set` with `d` written in, or `None` when any of it is refused.
fn with_details(set: &Set, d: &Details) -> Option<Set> {
    let mut s = set.clone();
    if let Some(date) = d.date {
        s.date = date?;
    }
    let slots =
        [&mut s.title, &mut s.kind, &mut s.venue, &mut s.city, &mut s.country, &mut s.event, &mut s.link, &mut s.notes];
    for (slot, want) in slots.into_iter().zip(d.want) {
        if let Some(want) = want {
            *slot = want.ok()?.map(String::from);
        }
    }
    if let Some(with) = &d.with {
        s.with = with.clone();
    }
    // A link given, the name a plugin gave the one before goes with it.
    if d.want[6].is_some() {
        s.link_name = None;
    }
    Some(s)
}

/// The artist and title a track typed in goes in as (`None`: refused, a blank).
type AsTyped = Option<(&'static str, &'static str)>;

/// Tracks typed into a set, and what they go in as.
const TYPED: &[(&str, AsTyped)] = &[
    ("Floorplan - Never Grow Old", Some(("Floorplan", "Never Grow Old"))),
    ("ID", Some(("", "ID"))),
    ("  Someone - Dubplate ", Some(("Someone", "Dubplate"))),
    ("A - B - C", Some(("A", "B - C"))),
    (" ", None),
];

fn typed(artist: &str, title: &str) -> SetEntry {
    SetEntry { artist: artist.into(), title: title.into(), ..SetEntry::added("") }
}

/// `Artist - Title`, the title alone, or the artist alone.
fn entry_name(e: &SetEntry) -> String {
    match (e.artist.as_str(), e.title.as_str()) {
        ("", t) => t.to_string(),
        (a, "") => a.to_string(),
        (a, t) => format!("{a} - {t}"),
    }
}

/// Changes to a tracklist by position, as the API takes them.
#[derive(Debug, Clone, Default)]
struct Changes {
    drop: Vec<usize>,
    moves: Vec<(usize, usize)>,
    insert: Vec<(usize, &'static str, AsTyped)>,
    unnamed: Vec<usize>,
    named: Vec<usize>,
}

/// A position of a tracklist of `n` (from 1), or, when `refused`, now and then one it doesn't
/// have.
#[hegel::test_helper]
fn position(tc: &TestCase, n: usize, refused: bool) -> usize {
    match n > 0 && !(refused && tc.draw(gs::booleans())) {
        true => tc.draw(gs::integers::<usize>().min_value(1).max_value(n)),
        false => tc.draw(gs::sampled_from(vec![0, n + 1])),
    }
}

/// What may be wrong with changes drawn, now and then: a position the tracklist doesn't have,
/// an insert out of range, or a blank one. Changes may also contradict each other.
#[derive(Clone, Copy, PartialEq)]
enum Flaw {
    None,
    Position,
    Insert,
    Blank,
}

#[hegel::test_helper]
fn changes(tc: &TestCase, n: usize) -> Changes {
    let flaw = match tc.draw(gs::integers::<u8>().max_value(7)) {
        0 => Flaw::Position,
        1 => Flaw::Insert,
        2 => Flaw::Blank,
        _ => Flaw::None,
    };
    let wrong = flaw == Flaw::Position;
    let most = |m: usize| if n > 0 || wrong { m } else { 0 };
    let positions = |m: usize| (0..tc.draw(gs::integers::<usize>().max_value(most(m)))).map(|_| position(tc, n, wrong));
    let drop = positions(2).collect();
    let moves = (0..tc.draw(gs::integers::<usize>().max_value(most(2))))
        .map(|_| (position(tc, n, wrong), position(tc, n, wrong)))
        .collect();
    let unnamed = positions(2).collect();
    let named = positions(1).collect();
    let typed: Vec<_> =
        TYPED.iter().filter(|(_, as_typed)| flaw == Flaw::Blank || as_typed.is_some()).copied().collect();
    let insert = (0..tc.draw(gs::integers::<usize>().max_value(3)))
        .map(|_| {
            let (text, as_typed) = tc.draw(gs::sampled_from(typed.clone()));
            // One past the last appends.
            (position(tc, n + 1, flaw == Flaw::Insert), text, as_typed)
        })
        .collect();
    Changes { drop, moves, insert, unnamed, named }
}

/// `tracks` with `c` made, or `None` when it's refused. Each track gets a place: (the position
/// it lands at, then inserted tracks, tracks moved up to it, the track that was there, tracks
/// moved down to it, then the order given), and they're read off in that order.
fn changed(tracks: &[SetEntry], c: &Changes) -> Option<Vec<SetEntry>> {
    let n = tracks.len();
    let ok = |p: usize| (1..=n).contains(&p);
    let froms: Vec<usize> = c.moves.iter().map(|m| m.0).collect();
    let valid = c.drop.iter().chain(&froms).chain(&c.unnamed).chain(&c.named).all(|&p| ok(p))
        && c.moves.iter().all(|m| ok(m.1))
        && c.insert.iter().all(|i| (1..=n + 1).contains(&i.0))
        && froms.iter().collect::<BTreeSet<_>>().len() == froms.len()
        && !c.drop.iter().any(|p| froms.contains(p))
        && !c.unnamed.iter().any(|p| c.named.contains(p))
        && c.insert.iter().all(|i| i.2.is_some());
    if !valid {
        return None;
    }
    let mut tracks = tracks.to_vec();
    for &p in &c.unnamed {
        tracks[p - 1].unnamed = true;
    }
    for &p in &c.named {
        tracks[p - 1].unnamed = false;
    }
    let mut placed: Vec<((usize, u8, usize), SetEntry)> = Vec::new();
    for (i, &(at, _, as_typed)) in c.insert.iter().enumerate() {
        let (artist, title) = as_typed.unwrap();
        placed.push(((at, 0, i), typed(artist, title)));
    }
    for (i, &(from, to)) in c.moves.iter().enumerate().filter(|(_, m)| m.0 != m.1) {
        placed.push(((to, if to < from { 1 } else { 3 }, i), tracks[from - 1].clone()));
    }
    for p in (1..=n).filter(|p| !c.drop.contains(p) && !c.moves.iter().any(|m| m.0 == *p && m.1 != *p)) {
        placed.push(((p, 2, 0), tracks[p - 1].clone()));
    }
    placed.sort_by_key(|(at, _)| *at);
    Some(placed.into_iter().map(|(_, e)| e).collect())
}

/// The newest of `sets`: by the day it was played, then the one declared last.
fn newest<'a>(sets: impl Iterator<Item = &'a Set>) -> Option<&'a Set> {
    sets.max_by_key(|s| (s.date, s.created_at))
}

/// The set `query` names, as `SetBook::find` documents it, or the error's code.
fn find<'a>(sets: &'a IndexMap<String, Set>, query: &str) -> Result<&'a Set, &'static str> {
    let q = query.trim();
    if q.is_empty() {
        return Err("invalid");
    }
    if q.eq_ignore_ascii_case("last") {
        return newest(sets.values()).ok_or("not_found");
    }
    let day = NaiveDate::parse_from_str(q, "%Y-%m-%d").ok();
    let tiers: [&dyn Fn(&Set) -> bool; 4] = [
        &|s| s.id == q,
        &|s| q.len() >= 4 && s.id.starts_with(q),
        &|s| s.title.as_deref().is_some_and(|t| t.eq_ignore_ascii_case(q)),
        &|s| Some(s.date) == day,
    ];
    for tier in tiers {
        match sets.values().filter(|s| tier(s)).collect::<Vec<_>>()[..] {
            [] => continue,
            [s] => return Ok(s),
            _ => return Err("invalid"),
        }
    }
    Err("not_found")
}

/// Newest first.
fn newest_first(sets: &IndexMap<String, Set>) -> Vec<&Set> {
    let mut out: Vec<&Set> = sets.values().collect();
    out.sort_by_key(|s| std::cmp::Reverse((s.date, s.created_at)));
    out
}

/// A track as a published listing names it.
fn public(e: &SetEntry) -> String {
    match e.unnamed {
        true => "ID - ID".into(),
        false => entry_name(e),
    }
}

struct SetsMachine {
    desk: Desk,
    /// The sets, by id.
    sets: IndexMap<String, Set>,
    /// Whether the file sets are written out to is there.
    written: bool,
}

impl SetsMachine {
    fn pick(&self, tc: &TestCase) -> Option<String> {
        let ids: Vec<String> = self.sets.keys().cloned().collect();
        (!ids.is_empty()).then(|| tc.draw(gs::sampled_from(ids)))
    }

    fn output(&self) -> PathBuf {
        self.desk.dir.path().join("set.txt")
    }
}

#[hegel::state_machine]
impl SetsMachine {
    /// A set declared with valid details has them, no tracks, a new id and (without a day) today's
    /// date and a note asking for the day; with any detail refused, nothing is kept.
    #[rule(weight = 3)]
    fn declare(&mut self, tc: TestCase) {
        let d = details(&tc);
        let today = Local::now().date_naive();
        let want = with_details(&Set { date: today, ..Default::default() }, &d);
        let before = self.desk.book_bytes();
        let req = NewSet { fields: d.fields.clone(), ..Default::default() };
        match (api::set_new(&self.desk.store, &req, &Ctx::silent()), want) {
            (Ok(made), Some(want)) => {
                let set = made.set;
                assert_eq!(set.created_at, set.updated_at);
                assert_eq!(
                    set,
                    Set { id: set.id.clone(), created_at: set.created_at, updated_at: set.created_at, ..want }
                );
                let notes = match d.date {
                    Some(_) => Vec::new(),
                    None => vec![format!("dated today ({today}): give the day it was played")],
                };
                assert_eq!(made.notes, notes);
                assert!(!self.sets.contains_key(&set.id), "a new set has a new id");
                self.sets.insert(set.id.clone(), set);
            }
            (Err(e), None) => {
                assert_eq!(e.code(), "invalid", "{e}");
                assert_eq!(self.desk.book_bytes(), before, "a refused set isn't kept");
            }
            (got, want) => panic!("declaring {:?}: got {got:?}, the model has {want:?}", d.fields),
        }
    }

    /// An edit changes the details and tracklist as the model does, all of it or (when anything
    /// in it is refused) none.
    #[rule(weight = 5)]
    fn edit(&mut self, tc: TestCase) {
        let Some(id) = self.pick(&tc) else { return };
        let current = self.sets[&id].clone();
        let d = if tc.draw(gs::booleans()) { details(&tc) } else { details_left_out() };
        let c = changes(&tc, current.tracks.len());
        let want = with_details(&current, &d).and_then(|s| Some(Set { tracks: changed(&s.tracks, &c)?, ..s }));
        let before = self.desk.book_bytes();
        let req = EditSet {
            set: id.clone(),
            fields: d.fields.clone(),
            drop: c.drop.clone(),
            moves: c.moves.iter().map(|&(from, to)| Move { from, to }).collect(),
            insert: c.insert.iter().map(|&(at, text, _)| Insert { at, track: text.into() }).collect(),
            unnamed: c.unnamed.clone(),
            named: c.named.clone(),
            ..Default::default()
        };
        match (api::set_edit(&self.desk.store, &req, &Ctx::silent()), want) {
            (Ok(made), Some(want)) => {
                let set = made.set;
                assert!(set.updated_at >= current.updated_at);
                assert_eq!(set, Set { updated_at: set.updated_at, ..want });
                let notes: Vec<String> = c
                    .insert
                    .iter()
                    .map(|(_, text, _)| format!("{:?} went in as typed: no drive to look it up on", text.trim()))
                    .collect();
                assert_eq!(made.notes, notes);
                self.sets.insert(id, set);
            }
            (Err(e), None) => {
                assert_eq!(e.code(), "invalid", "{e}");
                assert_eq!(self.desk.book_bytes(), before, "a refused edit changes nothing");
            }
            (got, want) => panic!("editing with {d:?} and {c:?}: got {got:?}, the model has {want:?}"),
        }
    }

    /// A deleted set is gone, and deleting it again finds nothing.
    #[rule]
    fn delete(&mut self, tc: TestCase) {
        let Some(id) = self.pick(&tc) else { return };
        let req = SetRef { set: Some(id.clone()), venue: None };
        assert_eq!(api::set_delete(&self.desk.store, &req).unwrap(), self.sets.shift_remove(&id).unwrap());
        assert_eq!(api::set_delete(&self.desk.store, &req).unwrap_err().code(), "not_found");
    }

    /// A set is found by `last`, its id, an id prefix of 4 or more, its title in any case or its
    /// day (when one set has it), or as the latest at a venue; anything else (or nothing) is
    /// refused or not found as the model says.
    #[rule(weight = 3)]
    fn get(&mut self, tc: TestCase) {
        let some = self.pick(&tc).map(|id| self.sets[&id].clone());
        let mut queries: Vec<String> =
            ["last", " LAST ", "nope", " ", "2026-10-02", "friday", "2026-13-01"].map(String::from).to_vec();
        if let Some(s) = &some {
            let prefix = tc.draw(gs::integers::<usize>().min_value(1).max_value(13));
            queries.extend([s.id.clone(), s.id[..prefix].to_string(), s.date.to_string()]);
            queries.extend(s.title.as_ref().map(|t| t.to_uppercase()));
        }
        for query in queries {
            let req = SetRef { set: Some(query.clone()), venue: None };
            match (api::set_get(&self.desk.store, &req), find(&self.sets, &query)) {
                (Ok(got), Ok(want)) => assert_eq!(&got, want),
                (Err(e), Err(code)) => assert_eq!(e.code(), code, "{query:?}: {e}"),
                (got, want) => panic!("finding {query:?}: got {got:?}, the model has {want:?}"),
            }
        }
        for venue in ["warehouse", "THE ware", "nts", "fabric"] {
            let req = SetRef { set: None, venue: Some(venue.into()) };
            let at = self
                .sets
                .values()
                .filter(|s| s.venue.as_ref().is_some_and(|v| v.to_lowercase().contains(&venue.to_lowercase())));
            match (api::set_get(&self.desk.store, &req), newest(at)) {
                (Ok(got), Some(want)) => assert_eq!(&got, want),
                (Err(e), None) => assert_eq!(e.code(), "not_found"),
                (got, want) => panic!("the latest at {venue:?}: got {got:?}, the model has {want:?}"),
            }
        }
        let neither = api::set_get(&self.desk.store, &SetRef::default());
        assert_eq!(neither.unwrap_err().code(), "invalid");
    }

    /// Listings show the sets the filter keeps, newest first; a day that isn't one is refused.
    #[rule(weight = 2)]
    fn list(&mut self, tc: TestCase) {
        let venue = tc.draw(gs::sampled_from(vec![None, Some("warehouse"), Some("THE ware"), Some("nts")]));
        let kind = tc.draw(gs::sampled_from(vec![None, Some("club"), Some("CLUB"), Some("clu")]));
        // As given, and the first and last day it covers (`None`: refused).
        let days = vec![
            None,
            Some(("2026", Some((day(2026, 1, 1), day(2026, 12, 31))))),
            Some(("2026-10", Some((day(2026, 10, 1), day(2026, 10, 31))))),
            Some(("2026-10-02", Some((day(2026, 10, 2), day(2026, 10, 2))))),
            Some(("soon", None)),
        ];
        let (since, until) = (one_of(&tc, &days), one_of(&tc, &days));
        let req = SetsQuery {
            venue: venue.map(String::from),
            kind: kind.map(String::from),
            since: since.map(|(text, _)| text.into()),
            until: until.map(|(text, _)| text.into()),
        };
        let got = api::sets_list(&self.desk.store, &req);
        let (Some(since), Some(until)) = (
            since.map_or(Some(None), |(_, d)| d.map(|d| Some(d.0))),
            until.map_or(Some(None), |(_, d)| d.map(|d| Some(d.1))),
        ) else {
            assert_eq!(got.unwrap_err().code(), "invalid");
            return;
        };
        let has = |field: &Option<String>, wanted: Option<&str>, whole: bool| {
            wanted.is_none_or(|w| {
                let (f, w) = (field.as_deref().unwrap_or_default().to_lowercase(), w.to_lowercase());
                field.is_some() && if whole { f == w } else { f.contains(&w) }
            })
        };
        let want: Vec<&Set> = newest_first(&self.sets)
            .into_iter()
            .filter(|s| has(&s.venue, venue, false) && has(&s.kind, kind, true))
            .filter(|s| since.is_none_or(|d| s.date >= d) && until.is_none_or(|d| s.date <= d))
            .collect();
        assert_eq!(got.unwrap().iter().collect::<Vec<_>>(), want);
    }

    /// A track is found at every place it has in the sets, newest set first; a blank query is
    /// refused.
    #[rule(weight = 2)]
    fn plays(&mut self, tc: TestCase) {
        let query = tc.draw(gs::sampled_from(vec![
            "floorplan",
            "NEVER grow old",
            "  ID ",
            "someone - dub",
            "b - c",
            "/Contents/x.mp3",
            " ",
            "nothing",
        ]));
        let got = api::track_plays(&self.desk.store, &TrackPlaysQuery { track: query.into() });
        let needle = query.trim().to_lowercase();
        if needle.is_empty() {
            assert_eq!(got.unwrap_err().code(), "invalid");
            return;
        }
        let want: Vec<(String, usize, String)> = newest_first(&self.sets)
            .into_iter()
            .flat_map(|s| {
                let needle = &needle;
                s.tracks.iter().enumerate().filter_map(move |(i, e)| {
                    // Typed tracks have no file, so a path finds none.
                    let found = !needle.starts_with('/') && entry_name(e).to_lowercase().contains(needle);
                    found.then(|| (s.id.clone(), i + 1, entry_name(e)))
                })
            })
            .collect();
        let got: Vec<(String, usize, String)> =
            got.unwrap().into_iter().map(|a| (a.set, a.position, a.track)).collect();
        assert_eq!(got, want);
    }

    /// A set written out as text numbers its tracks, naming the unnamed `ID - ID`; as CSV it names
    /// every track and marks the unnamed; written to a file, it's there, and a file there already
    /// is only replaced when asked.
    #[rule(weight = 2)]
    fn export(&mut self, tc: TestCase) {
        let Some(id) = self.pick(&tc) else { return };
        let set = &self.sets[&id];
        let csv = tc.draw(gs::booleans());
        let to_file = tc.draw(gs::booleans());
        let replace = tc.draw(gs::booleans());
        let req = SetExport {
            set: id.clone(),
            format: if csv { SetFormat::Csv } else { SetFormat::Text },
            drive: None,
            output: to_file.then(|| self.output()),
            replace,
        };
        let got = api::set_export(&self.desk.store, &req);
        if to_file && self.written && !replace {
            assert_eq!(got.unwrap_err().code(), "conflict");
            return;
        }
        let got = got.unwrap();
        assert_eq!(got.tracks, set.tracks.len());
        let text = match to_file {
            true => {
                self.written = true;
                assert_eq!((got.text, got.path), (None, Some(self.output())));
                std::fs::read_to_string(self.output()).unwrap()
            }
            false => got.text.unwrap(),
        };
        match csv {
            true => {
                // Readers trim fields (the last keeps the CR of its CRLF).
                let rows: Vec<Vec<String>> = read_table(&text, ',', true)
                    .into_iter()
                    .map(|r| r.fields.iter().map(|f| f.trim().to_string()).collect())
                    .collect();
                assert_eq!(rows.len(), set.tracks.len() + 1, "{text}");
                for (i, (row, e)) in rows[1..].iter().zip(&set.tracks).enumerate() {
                    let unnamed = if e.unnamed { "yes" } else { "" };
                    assert_eq!(
                        (row[0].as_str(), row[2].as_str(), row[3].as_str(), row[11].as_str()),
                        (&*(i + 1).to_string(), e.artist.as_str(), e.title.as_str(), unnamed)
                    );
                }
            }
            false => {
                let want: String =
                    set.tracks.iter().enumerate().map(|(i, e)| format!("{:02}. {}\n", i + 1, public(e))).collect();
                assert_eq!(text, want);
            }
        }
    }

    /// `sets.json` holds the model's sets, newest first.
    #[invariant]
    fn the_book_matches_the_model(&mut self, _: TestCase) {
        let book = self.desk.book();
        for w in book.sets.windows(2) {
            assert!((w[0].date, w[0].created_at) >= (w[1].date, w[1].created_at), "newest first");
        }
        let stored: IndexMap<String, Set> = book.sets.into_iter().map(|s| (s.id.clone(), s)).collect();
        assert_eq!(stored.len(), self.sets.len());
        for (id, s) in &self.sets {
            assert_eq!(stored.get(id), Some(s));
        }
    }
}

/// Details that leave everything as it is.
fn details_left_out() -> Details {
    Details { fields: SetFields::default(), want: [None; 8], date: None, with: None }
}

#[hegel::test(test_cases = 20, suppress_health_check = [HealthCheck::TooSlow])]
fn sets_through_the_api_match_the_model(tc: TestCase) {
    let desk = Desk::new();
    // Two sets of four tracks to start from, so that every step has sets to work on.
    let sets = [("Friday", "2026-10-03", "The Warehouse"), ("Warm-up", "2026-10-02", "NTS")]
        .into_iter()
        .map(|(title, date, venue)| {
            let fields = SetFields {
                title: Some(title.into()),
                date: Some(date.into()),
                venue: Some(venue.into()),
                ..Default::default()
            };
            let set = api::set_new(&desk.store, &NewSet { fields, ..Default::default() }, &Ctx::silent()).unwrap().set;
            // With tracks to move about.
            let insert = TYPED[..4].iter().map(|(text, _)| Insert { at: 1, track: text.to_string() }).collect();
            let req = EditSet { set: set.id.clone(), insert, ..Default::default() };
            let set = api::set_edit(&desk.store, &req, &Ctx::silent()).unwrap().set;
            (set.id.clone(), set)
        })
        .collect();
    hegel::stateful::machine(SetsMachine { desk, sets, written: false }).steps(12).run(tc);
}

/// Details are written into a set as the model has them: given, trimmed, cleared when blank, a
/// country upper-cased, partners with a DID kept as accounts, a link without the name a plugin
/// gave the one before; all of them, or (when any is refused) none.
#[hegel::test(test_cases = 300)]
fn details_are_written_whole_or_not_at_all(tc: TestCase) {
    let base = Set {
        id: "3m2abcdefgh22".into(),
        title: Some("Before".into()),
        date: day(2026, 9, 30),
        venue: Some("Fabric".into()),
        city: Some("London".into()),
        country: Some("GB".into()),
        with: vec![Partner { name: "Old".into(), did: None }],
        link: Some("https://plyr.fm/track/1".into()),
        link_name: Some("plyr.fm".into()),
        ..Default::default()
    };
    let d = details(&tc);
    let mut set = base.clone();
    match (d.fields.apply(&mut set), with_details(&base, &d)) {
        (Ok(()), Some(want)) => assert_eq!(set, want),
        (Err(e), None) => {
            assert_eq!(e.code(), "invalid", "{e}");
            assert_eq!(set, base, "a refused edit changes nothing");
        }
        (got, want) => panic!("{:?}: got {got:?}, the model has {want:?}", d.fields),
    }
}

#[hegel::test_helper]
fn text(tc: &TestCase) -> String {
    tc.draw(hegel::one_of!(gs::text().max_size(6), gs::sampled_from(vec![String::new(), "Friday".into()])))
}

#[hegel::test_helper]
fn maybe_text(tc: &TestCase) -> Option<String> {
    tc.draw(gs::booleans()).then(|| text(tc))
}

#[hegel::test_helper]
fn time(tc: &TestCase) -> DateTime<Utc> {
    let last = DateTime::from_timestamp(4_000_000_000, 999_999_999).unwrap();
    tc.draw(gs::default::<DateTime<Utc>>().min_value(DateTime::UNIX_EPOCH.naive_utc()).max_value(last.naive_utc()))
}

#[hegel::test_helper]
fn any_entry(tc: &TestCase) -> SetEntry {
    SetEntry {
        title: text(tc),
        artist: text(tc),
        album: text(tc),
        label: text(tc),
        genre: text(tc),
        isrc: text(tc),
        key: text(tc),
        bpm: tc.draw(gs::floats::<f64>().allow_nan(false).allow_infinity(false)),
        year: tc.draw(gs::integers::<u16>()),
        duration_secs: tc.draw(gs::integers::<u32>()),
        file: maybe_text(tc),
        start_seconds: tc.draw(gs::optional(gs::integers::<u32>())),
        musicbrainz: MusicBrainzIds {
            recording: tc.draw(gs::optional(gs::integers::<u128>())).map(uuid::Uuid::from_u128),
            release: tc.draw(gs::optional(gs::integers::<u128>())).map(uuid::Uuid::from_u128),
            artists: tc
                .draw(gs::vecs(gs::integers::<u128>()).max_size(3))
                .into_iter()
                .map(uuid::Uuid::from_u128)
                .collect(),
        },
        unnamed: tc.draw(gs::booleans()),
        origin: one_of(tc, &[EntryOrigin::Session, EntryOrigin::Recording, EntryOrigin::Added]),
        session: maybe_text(tc),
    }
}

#[hegel::test_helper]
fn any_set(tc: &TestCase) -> Set {
    let few = |most: usize| tc.draw(gs::integers::<usize>().max_value(most));
    Set {
        id: text(tc),
        title: maybe_text(tc),
        date: tc.draw(
            gs::default::<NaiveDate>()
                .min_value(NaiveDate::from_num_days_from_ce_opt(1).unwrap())
                .max_value(NaiveDate::from_num_days_from_ce_opt(800_000).unwrap()),
        ),
        kind: maybe_text(tc),
        venue: maybe_text(tc),
        city: maybe_text(tc),
        country: maybe_text(tc),
        event: maybe_text(tc),
        with: (0..few(2)).map(|_| Partner { name: text(tc), did: maybe_text(tc) }).collect(),
        link: maybe_text(tc),
        link_name: maybe_text(tc),
        recording_record: tc.draw(gs::booleans()).then(|| loclib_plugin::StrongRef { uri: text(tc), cid: text(tc) }),
        recording: tc.draw(gs::booleans()).then(|| Recording { path: text(tc).into(), report: maybe_text(tc) }),
        notes: maybe_text(tc),
        sessions: (0..few(2)).map(|_| text(tc)).collect(),
        tracks: (0..few(3)).map(|_| any_entry(tc)).collect(),
        created_at: time(tc),
        updated_at: time(tc),
    }
}

/// Saving the book writes its sets newest first, and loading it gives back exactly what was
/// saved: every set, entry and imported log, whatever text they hold; saved again, the file is
/// the same.
#[hegel::test(test_cases = 40)]
fn the_book_round_trips_through_sets_json(tc: TestCase) {
    let desk = Desk::new();
    assert!(desk.book().sets.is_empty(), "no sets.json is an empty book");
    let few = |most: usize| tc.draw(gs::integers::<usize>().max_value(most));
    let mut book = SetBook {
        sets: (0..few(4)).map(|_| any_set(&tc)).collect(),
        dismissed: (0..few(3)).map(|_| text(&tc)).collect(),
        imported: (0..few(2))
            .map(|_| ImportedSession {
                id: text(&tc),
                name: text(&tc),
                tool: text(&tc),
                file: text(&tc).into(),
                imported_at: time(&tc),
                plays: (0..few(2))
                    .map(|_| TimedPlay {
                        entry: any_entry(&tc),
                        started: time(&tc),
                        stopped: tc.draw(gs::booleans()).then(|| time(&tc)),
                        player: maybe_text(&tc),
                    })
                    .collect(),
            })
            .collect(),
    };
    let sets = book.sets.clone();
    book.save(&desk.store).unwrap();
    for w in book.sets.windows(2) {
        assert!((w[0].date, w[0].created_at) >= (w[1].date, w[1].created_at), "newest first");
    }
    assert!(book.sets.iter().all(|s| sets.contains(s)) && sets.iter().all(|s| book.sets.contains(s)));
    let saved = desk.book_bytes().unwrap();
    let mut loaded = desk.book();
    assert_eq!(loaded.sets, book.sets);
    assert_eq!(loaded.dismissed, book.dismissed);
    assert_eq!(loaded.imported, book.imported);
    loaded.save(&desk.store).unwrap();
    assert_eq!(desk.book_bytes().unwrap(), saved);
}

const TID_ALPHABET: &[u8] = b"234567abcdefghijklmnopqrstuvwxyz";

/// The microseconds a TID holds.
fn tid_micros(id: &str) -> u64 {
    id.bytes().fold(0u64, |v, b| (v << 5) | TID_ALPHABET.iter().position(|&a| a == b).unwrap() as u64) >> 10
}

/// A set id is a 13-character TID whose time falls on the set's day (UTC; days before 1970 all
/// go on its first), so ids sort by day; ids drawn for one day differ; and listings show the
/// first 8 characters.
#[hegel::test(test_cases = 300)]
fn set_ids_are_tids_on_their_day(tc: TestCase) {
    // From 1900 to 2240: a TID's 53 bits of microseconds last until 2255.
    let days = || gs::integers::<i64>().min_value(-25_567).max_value(98_600);
    let epoch = day(1970, 1, 1);
    let (a, b) = (tc.draw(days()), tc.draw(days()));
    let (a, b) = (epoch + Duration::days(a.min(b)), epoch + Duration::days(a.max(b)));
    let ids: Vec<String> = (0..4).map(|_| sets::new_id(a).unwrap()).collect();
    for id in &ids {
        assert_eq!(id.len(), 13);
        assert!(id.bytes().all(|b| TID_ALPHABET.contains(&b)), "{id}");
        let midnight = a.and_hms_opt(0, 0, 0).unwrap().and_utc().timestamp_micros().max(0) as u64;
        let micros = tid_micros(id);
        assert!((midnight..midnight + 86_400_000_000).contains(&micros), "{id} is at {micros}, not on {a}");
        let set = Set { id: id.clone(), ..Default::default() };
        assert_eq!(set.short_id(), &id[..8]);
    }
    assert_eq!(ids.iter().collect::<BTreeSet<_>>().len(), ids.len(), "{ids:?}");
    let later = sets::new_id(b).unwrap();
    if b > a && b > epoch {
        assert!(later > ids[0], "{later} for {b} sorts after {} for {a}", ids[0]);
    }
    let short = tc.draw(gs::text().alphabet("abc").max_size(9));
    assert_eq!(Set { id: short.clone(), ..Default::default() }.short_id(), &short[..short.len().min(8)]);
}

/// A set is called by its title, else its venue; headed by that, else its day; placed at its
/// venue and city as far as they're known; and described by day, name and short id.
#[hegel::test(test_cases = 200)]
fn a_set_names_itself_by_what_it_has(tc: TestCase) {
    let pick = |pool: Vec<Option<&str>>| tc.draw(gs::sampled_from(pool)).map(String::from);
    let set = Set {
        id: "3m2abcdefgh22".into(),
        title: pick(vec![None, Some("Friday")]),
        venue: pick(vec![None, Some("Warehouse")]),
        city: pick(vec![None, Some("Leeds")]),
        date: day(2026, 10, 3),
        ..Default::default()
    };
    let name = set.title.clone().or(set.venue.clone());
    assert_eq!(set.name(), name.as_deref());
    assert_eq!(set.heading(), name.clone().unwrap_or_else(|| "2026-10-03".into()));
    let place = match (&set.venue, &set.city) {
        (Some(v), Some(c)) => format!("{v}, {c}"),
        (v, c) => v.clone().or(c.clone()).unwrap_or_default(),
    };
    assert_eq!(set.place(), place);
    assert_eq!(sets::describe(&set), format!("2026-10-03 {} (3m2abcde)", name.as_deref().unwrap_or("untitled")));
}

/// Songs logs play: title, artist and album.
const SONGS: &[(&str, &str, &str)] = &[
    ("Never Grow Old", "Floorplan", "Victorious"),
    ("A2 - In Charge", "[A]pendics.Shuffle", ""),
    ("Never Grow Old, Pt. 1", "Floorplan", ""),
    ("Say \"Yes\"", "Someone", "Some, Album"),
    ("Unknown (no metadata found)", "", ""),
];

/// A row of a Beat Link Trigger log.
#[derive(Debug, Clone, PartialEq)]
struct Row {
    song: (&'static str, &'static str, &'static str),
    started: DateTime<Utc>,
    stopped: Option<DateTime<Utc>>,
}

/// When the night starts: logs play from here, on this night or the next two.
fn night() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 10, 3, 21, 0, 0).unwrap()
}

/// A log's rows, in the order they were written (when each track stopped, but anything goes).
#[hegel::test_helper]
fn rows(tc: &TestCase, most: usize) -> Vec<Row> {
    let night = night() + Duration::days(tc.draw(gs::integers::<i64>().min_value(0).max_value(2)));
    (0..tc.draw(gs::integers::<usize>().min_value(1).max_value(most)))
        .map(|_| {
            let started = night + Duration::minutes(tc.draw(gs::integers::<i64>().min_value(0).max_value(240)));
            let played = tc.draw(gs::sampled_from(vec![None, Some(1), Some(4), Some(30), Some(120)]));
            Row {
                song: tc.draw(gs::sampled_from(SONGS.to_vec())),
                started,
                stopped: played.map(|m| started + Duration::minutes(m)),
            }
        })
        .collect()
}

/// A time as `java.util.Date` writes one, in UTC.
fn java(t: DateTime<Utc>) -> String {
    t.format("%a %b %d %H:%M:%S UTC %Y").to_string()
}

fn write_log(path: &Path, rows: &[Row]) {
    let q = |s: &str| format!("\"{}\"", s.replace('"', "\"\""));
    let mut text = String::from("Title,Artist,Album,Player,Source,Started,Stopped,Play Time\n");
    for r in rows {
        let (title, artist, album) = r.song;
        let stopped = r.stopped.map(java).unwrap_or_default();
        text +=
            &format!("{},{},{},2,Player 2 USB,{},{stopped},05:00\n", q(title), q(artist), q(album), java(r.started));
    }
    std::fs::write(path, text).unwrap();
}

/// An imported log as the model has it: its plays in the order they started.
#[derive(Debug, Clone)]
struct Log {
    id: String,
    name: String,
    plays: Vec<Row>,
}

impl Log {
    fn window(&self) -> Window {
        Window {
            after: self.plays.iter().map(|p| p.started).min(),
            before: self.plays.iter().map(|p| p.stopped.unwrap_or(p.started)).max(),
            exact: true,
        }
    }

    /// Its tracks' names and starts, from its first play.
    fn tracks(&self) -> Vec<(String, Option<u32>)> {
        let first = self.plays[0].started;
        self.plays.iter().map(|p| (song_name(p.song), Some((p.started - first).num_seconds() as u32))).collect()
    }
}

fn song_name((title, artist, _): (&str, &str, &str)) -> String {
    entry_name(&typed(artist, title))
}

/// Import `rows` as a log named `name`, check what's kept against the model, and add it to the
/// model's `logs` (the same log imported again replaces its session).
fn import(desk: &Desk, logs: &mut Vec<Log>, file: &str, name: Option<&str>, rows: &[Row]) -> ImportedSession {
    let path = desk.dir.path().join(format!("{file}.csv"));
    write_log(&path, rows);
    let req = ImportSessionLog { file: path.clone(), drive: None, name: name.map(String::from) };
    let log = api::sessions_import(&desk.store, &req).unwrap();
    let mut plays = rows.to_vec();
    plays.sort_by_key(|p| p.started);
    let want_name = name.map(str::trim).filter(|n| !n.is_empty()).unwrap_or(file);
    assert_eq!(
        (log.name.as_str(), log.tool.as_str(), log.file.as_path()),
        (want_name, "beat_link_trigger", path.as_path())
    );
    let got: Vec<Row> = log
        .plays
        .iter()
        .map(|p| {
            let (title, artist, album) = (p.entry.title.as_str(), p.entry.artist.as_str(), p.entry.album.as_str());
            assert_eq!(
                (p.entry.origin, p.entry.session.as_deref(), p.entry.file.as_deref()),
                (EntryOrigin::Session, Some(log.id.as_str()), None)
            );
            assert_eq!(p.player.as_deref(), Some("2"));
            let song = *SONGS.iter().find(|s| (s.0, s.1, s.2) == (title, artist, album)).expect("a song of the log");
            Row { song, started: p.started, stopped: p.stopped }
        })
        .collect();
    assert_eq!(got, plays, "the plays, in the order they started");
    logs.retain(|l| l.id != log.id);
    logs.push(Log { id: log.id.clone(), name: want_name.into(), plays });
    assert_eq!(
        desk.book().imported.iter().map(|l| &l.id).collect::<Vec<_>>(),
        logs.iter().map(|l| &l.id).collect::<Vec<_>>()
    );
    log
}

/// The model's sessions, newest first, with the sets built from them and whether they were
/// dismissed.
fn gathered(logs: &[Log], book: &SetBook) -> Vec<(Log, Vec<String>, bool)> {
    let mut out: Vec<(Log, Vec<String>, bool)> = logs
        .iter()
        .map(|l| {
            let sets = book.sets.iter().filter(|s| s.sessions.contains(&l.id)).map(|s| s.id.clone()).collect();
            (l.clone(), sets, book.dismissed.contains(&l.id))
        })
        .collect();
    out.sort_by(|(a, ..), (b, ..)| {
        let (wa, wb) = (a.window(), b.window());
        (wb.before, wb.after, &b.name).cmp(&(wa.before, wa.after, &a.name))
    });
    out
}

fn assert_sessions(got: &[Session], want: &[(Log, Vec<String>, bool)]) {
    let got: Vec<_> = got
        .iter()
        .map(|s| {
            assert_eq!((s.path.as_str(), s.source.as_str(), s.missing), (s.name.as_str(), "beat_link_trigger", 0));
            assert!(s.drives.is_empty() && s.first_seen_on.is_none());
            let tracks: Vec<(String, Option<u32>)> = s.tracks.iter().map(|e| (e.name(), e.start_seconds)).collect();
            (s.id.clone(), s.name.clone(), s.window, tracks, s.sets.clone(), s.dismissed)
        })
        .collect();
    let want: Vec<_> = want
        .iter()
        .map(|(l, sets, dismissed)| (l.id.clone(), l.name.clone(), l.window(), l.tracks(), sets.clone(), *dismissed))
        .collect();
    assert_eq!(got, want);
}

/// A set the DJ declared from these sessions.
fn set_from(id: &str, sessions: Vec<String>) -> Set {
    Set { id: id.into(), date: day(2026, 10, 3), sessions, ..Default::default() }
}

/// Logs imported from Beat Link Trigger are kept once each (the same rows are the same log, any
/// others another; imported again, it's renamed) and gathered as sessions in the order, with the
/// windows, timed tracks, sets and dismissals the model has; `unassigned` leaves out those in
/// sets or dismissed, a dismissal is taken back with `undo`, and `session::find` finds each by
/// id, id prefix in any case, or name when only one has it.
#[hegel::test(test_cases = 12, suppress_health_check = [HealthCheck::TooSlow])]
fn imported_logs_are_the_sessions_the_model_predicts(tc: TestCase) {
    let desk = Desk::new();
    let mut logs: Vec<Log> = Vec::new();
    let mut written: Vec<(String, Vec<Row>, String)> = Vec::new();
    for i in 0..tc.draw(gs::integers::<usize>().min_value(2).max_value(4)) {
        let rows = rows(&tc, 4);
        let name = tc.draw(gs::sampled_from(vec![Some(" Warehouse "), Some("Warehouse"), None, Some(" ")]));
        let file = format!("log{i}");
        let id = import(&desk, &mut logs, &file, name, &rows).id;
        let key = |rows: &[Row]| -> Vec<(String, &str, &str)> {
            rows.iter().map(|r| (java(r.started), r.song.1, r.song.0)).collect()
        };
        for (other, other_rows, _) in &written {
            assert_eq!(*other == id, key(other_rows) == key(&rows), "ids are the same exactly when the rows are");
        }
        written.push((id, rows, file));
    }
    // Imported again under another name: the same session, renamed.
    let (id, rows, file) = tc.draw(gs::sampled_from(&written));
    assert_eq!(import(&desk, &mut logs, &file, Some("Again"), &rows).id, id);

    let ids: Vec<String> = logs.iter().map(|l| l.id.clone()).collect();
    let mut book = desk.book();
    for k in 0..tc.draw(gs::integers::<usize>().max_value(2)) {
        let from = tc.draw(gs::vecs(gs::sampled_from(ids.clone())).max_size(2));
        book.sets.push(set_from(&format!("set{k}"), from));
    }
    book.save(&desk.store).unwrap();
    let dismiss = tc.draw(gs::vecs(gs::sampled_from(ids.clone())).max_size(3));
    let undo = tc.draw(gs::vecs(gs::sampled_from(ids.clone())).max_size(2));
    for (list, undo) in [(&dismiss, false), (&undo, true)] {
        let queries = list.iter().map(|id| if tc.draw(gs::booleans()) { id.clone() } else { id[..8].to_uppercase() });
        let req = DismissSessions { sessions: queries.collect(), undo };
        assert_eq!(&api::sessions_dismiss(&desk.store, &req, &Ctx::silent()).unwrap(), list);
    }
    let dismissed: BTreeSet<&String> = dismiss.iter().filter(|id| !undo.contains(id)).collect();
    assert_eq!(desk.book().dismissed.iter().collect::<BTreeSet<_>>(), dismissed);

    let want = gathered(&logs, &desk.book());
    let all = api::sessions(Some(&desk.store), &SessionsRequest::default(), &Ctx::silent()).unwrap();
    assert!(all.warnings.is_empty(), "{:?}", all.warnings);
    assert_sessions(&all.sessions, &want);
    let open =
        api::sessions(Some(&desk.store), &SessionsRequest { unassigned: true, ..Default::default() }, &Ctx::silent());
    let open_want: Vec<_> = want.iter().filter(|(_, sets, dismissed)| sets.is_empty() && !dismissed).cloned().collect();
    assert_sessions(&open.unwrap().sessions, &open_want);

    let sessions = &all.sessions;
    let found = |q: &str| session::find(sessions, q).map(|s| s.id.clone()).map_err(|e| e.code().to_string());
    for l in &logs {
        assert_eq!(found(&l.id), Ok(l.id.clone()));
        assert_eq!(found(&l.id[..6].to_uppercase()), Ok(l.id.clone()));
    }
    for name in ["warehouse", "/Again/", "log0", "LOG1", "again"] {
        let q = name.trim_matches('/');
        let tiers: [&dyn Fn(&Log) -> bool; 2] = [&|l| l.name == q, &|l| l.name.eq_ignore_ascii_case(q)];
        let want = tiers
            .iter()
            .map(|tier| logs.iter().filter(|l| tier(l)).collect::<Vec<_>>())
            .find(|found| !found.is_empty())
            .map_or(Err("not_found".to_string()), |found| match found[..] {
                [l] => Ok(l.id.clone()),
                _ => Err("invalid".into()),
            });
        assert_eq!(found(name), want, "{name:?}");
    }
    assert_eq!(found(" "), Err("invalid".into()));
    assert_eq!(found("zzzzzz"), Err("not_found".into()));

    // A file that isn't a log is refused, and nothing changes.
    let kept = desk.book_bytes();
    let notes = desk.dir.path().join("notes.csv");
    std::fs::write(&notes, "Track Name,Artist Name(s)\nx,y\n").unwrap();
    let req = ImportSessionLog { file: notes, drive: None, name: None };
    let err = api::sessions_import(&desk.store, &req).unwrap_err();
    assert!(err.code() == "invalid" && err.to_string().contains("isn't a Beat Link Trigger playlist"), "{err}");
    assert_eq!(desk.book_bytes(), kept);
}

/// The sets imported logs suggest are the model's: the logs in no set and not dismissed, grouped
/// while their play times overlap, the groups that have a log of 3 tracks or more, newest first,
/// each counting the different tracks it logged (a minute of play each).
#[hegel::test(test_cases = 12, suppress_health_check = [HealthCheck::TooSlow])]
fn imported_logs_suggest_the_sets_the_model_predicts(tc: TestCase) {
    let desk = Desk::new();
    let mut logs: Vec<Log> = Vec::new();
    for i in 0..tc.draw(gs::integers::<usize>().min_value(1).max_value(5)) {
        import(&desk, &mut logs, &format!("log{i}"), None, &rows(&tc, 5));
    }
    let ids: Vec<String> = logs.iter().map(|l| l.id.clone()).collect();
    let mut book = desk.book();
    if tc.draw(gs::booleans()) {
        book.sets.push(set_from("set", vec![tc.draw(gs::sampled_from(ids.clone()))]));
    }
    book.dismissed.extend(tc.draw(gs::vecs(gs::sampled_from(ids)).max_size(1)));
    book.save(&desk.store).unwrap();

    let open: Vec<Log> = gathered(&logs, &book)
        .into_iter()
        .filter(|(_, sets, dismissed)| sets.is_empty() && !dismissed)
        .map(|(l, ..)| l)
        .collect();
    // Groups: logs joined while a log of the group shares a moment with one outside it.
    let overlap = |a: &Window, b: &Window| a.after.unwrap() < b.before.unwrap() && b.after.unwrap() < a.before.unwrap();
    let mut group: Vec<usize> = (0..open.len()).collect();
    loop {
        let mut joined = false;
        for i in 0..open.len() {
            for j in 0..open.len() {
                if group[i] != group[j] && overlap(&open[i].window(), &open[j].window()) {
                    let (from, to) = (group[i].max(group[j]), group[i].min(group[j]));
                    group.iter_mut().filter(|g| **g == from).for_each(|g| *g = to);
                    joined = true;
                }
            }
        }
        if !joined {
            break;
        }
    }
    let firsts: Vec<usize> = (0..open.len()).filter(|&i| group[i] == i).collect();
    let mut want: Vec<(Vec<String>, Window, usize, u32, String)> = firsts
        .iter()
        .map(|&g| open.iter().enumerate().filter(|(i, _)| group[*i] == g).map(|(_, l)| l).collect::<Vec<_>>())
        .filter(|g| g.iter().any(|l| l.plays.len() >= 3))
        .map(|g| {
            let window = Window {
                after: g.iter().filter_map(|l| l.window().after).min(),
                before: g.iter().filter_map(|l| l.window().before).max(),
                exact: true,
            };
            let tracks =
                g.iter().flat_map(|l| &l.plays).map(|p| normalize(&song_name(p.song))).collect::<BTreeSet<_>>().len();
            let reason = match tracks {
                1 => "1 track: at least a minute of play".to_string(),
                n => format!("{n} tracks: at least {n} minutes of play"),
            };
            (g.iter().map(|l| l.id.clone()).collect(), window, tracks, tracks.min(40) as u32, reason)
        })
        .collect();
    want.sort_by_key(|c| std::cmp::Reverse((c.1.before, c.1.after)));

    let got = api::sets_suggest(Some(&desk.store), &SuggestSets::default(), &Ctx::silent()).unwrap();
    let candidates: Vec<(Vec<String>, Window, usize, u32, String)> = got
        .candidates
        .iter()
        .map(|c| {
            assert_eq!(c.reasons.len(), 1, "{:?}", c.reasons);
            (c.sessions.clone(), c.window, c.tracks, c.score, c.reasons[0].clone())
        })
        .collect();
    assert_eq!(candidates, want);
    let suggested: Vec<&String> = got.sessions.iter().map(|s| &s.id).collect();
    let want: Vec<&String> = open.iter().map(|l| &l.id).filter(|id| want.iter().any(|c| c.0.contains(id))).collect();
    assert_eq!(suggested, want, "the candidates' sessions, newest first");
}

/// A set declared from imported logs holds their tracks, one log after another in the order
/// named (each once), timed from the first play of them all, and is dated the day they started
/// (or, across days, the first day one was seen, with a note); logs added to it later go after
/// its tracks, the times all still counting from the earliest play.
#[hegel::test(test_cases = 12, suppress_health_check = [HealthCheck::TooSlow])]
fn sets_built_from_imported_logs_are_timed_from_the_first_play(tc: TestCase) {
    let desk = Desk::new();
    let mut logs: Vec<Log> = Vec::new();
    for i in 0..tc.draw(gs::integers::<usize>().min_value(1).max_value(4)) {
        import(&desk, &mut logs, &format!("log{i}"), None, &rows(&tc, 3));
    }
    let picked = |tc: &TestCase| {
        let n = tc.draw(gs::integers::<usize>().min_value(1).max_value(3));
        (0..n).map(|_| one_of(tc, &logs)).collect::<Vec<Log>>()
    };
    let (first, then) = (picked(&tc), picked(&tc));
    // Each once, in the order named: the logs a set is declared from, then those added that it
    // doesn't have yet.
    let mut from: Vec<Log> = Vec::new();
    for l in &first {
        if !from.iter().any(|n| n.id == l.id) {
            from.push(l.clone());
        }
    }
    let mut named = from.clone();
    for l in &then {
        if !named.iter().any(|n| n.id == l.id) {
            named.push(l.clone());
        }
    }
    let tracks = |logs: &[Log]| -> Vec<(String, Option<u32>, Option<String>)> {
        let origin = logs.iter().flat_map(|l| &l.plays).map(|p| p.started).min().unwrap();
        logs.iter()
            .flat_map(|l| {
                l.plays.iter().map(move |p| {
                    (song_name(p.song), Some((p.started - origin).num_seconds() as u32), Some(l.id.clone()))
                })
            })
            .collect()
    };
    let of = |set: &Set| -> Vec<(String, Option<u32>, Option<String>)> {
        assert!(set.tracks.iter().all(|e| e.origin == EntryOrigin::Session));
        set.tracks.iter().map(|e| (e.name(), e.start_seconds, e.session.clone())).collect()
    };
    let queries = |logs: &[Log]| logs.iter().map(|l| l.id[..8].to_string()).collect();

    let made =
        api::set_new(&desk.store, &NewSet { sessions: queries(&first), ..Default::default() }, &Ctx::silent()).unwrap();
    assert_eq!(made.set.sessions, from.iter().map(|l| l.id.clone()).collect::<Vec<_>>());
    assert_eq!(of(&made.set), tracks(&from));
    let local = |t: DateTime<Utc>| t.with_timezone(&Local).date_naive();
    let days: BTreeSet<NaiveDate> = from.iter().map(|l| local(l.window().after.unwrap())).collect();
    let (date, note) = match days.len() {
        1 => (*days.first().unwrap(), None),
        _ => {
            let d = local(from.iter().filter_map(|l| l.window().before).min().unwrap());
            (d, Some(format!("dated {d}, the day locallibrary first saw it: give the day it was played")))
        }
    };
    assert_eq!((made.set.date, made.notes), (date, note.into_iter().collect::<Vec<_>>()));

    let req = EditSet { set: made.set.id.clone(), add_sessions: queries(&then), ..Default::default() };
    let grown = api::set_edit(&desk.store, &req, &Ctx::silent()).unwrap();
    assert_eq!(grown.set.sessions, named.iter().map(|l| l.id.clone()).collect::<Vec<_>>());
    assert_eq!(of(&grown.set), tracks(&named));
    assert!(grown.notes.is_empty());

    let missing = NewSet { sessions: vec!["zzzzzzzz".into()], ..Default::default() };
    assert_eq!(api::set_new(&desk.store, &missing, &Ctx::silent()).unwrap_err().code(), "not_found");
    assert_eq!(desk.book().sets.len(), 1);
}

/// A suggestion as compared: its lists (names and files), its different tracks, and whether it
/// says it can't tell which night it was.
type Suggested = (Vec<(String, Vec<String>)>, usize, bool);

/// A history list on drives, as the model has it.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Listed {
    name: String,
    tracks: Vec<u32>,
    drives: BTreeSet<String>,
    /// A pull of Main saved it.
    saved: bool,
}

/// History lists on two drives are sessions once each, whichever drives hold them: each with its
/// tracks in order and the drives it's on; one a pull of Main saved is dated by that pull, one
/// that's only on drives was recorded since Main was pulled (when it's only on Main) and before
/// about now; read with a drive named, that drive's lists and every saved one; a drive whose
/// database can't be read is warned about. Lists saved by one pull, or with no start to their
/// window, are each suggested alone (saying so) once they have 3 tracks; lists on a drive since
/// its pull go together.
#[hegel::test(test_cases = 6, suppress_health_check = [HealthCheck::TooSlow])]
fn history_lists_on_drives_are_sessions_once_each(tc: TestCase) {
    let desk = Desk::new();
    let main = common::bigger_drive(&desk.volumes, "Main");
    let library: Library = pdb::read_library(&main.join(paths::EXPORT_PDB)).unwrap();
    let backup = desk.volumes.join("Backup");
    common::copy_dir(&main, &backup);
    let broken = tc.draw(gs::booleans());
    if broken {
        let pdb = desk.volumes.join("Broken").join(paths::EXPORT_PDB);
        std::fs::create_dir_all(pdb.parent().unwrap()).unwrap();
        std::fs::write(&pdb, b"not a database").unwrap();
    }
    let ids: Vec<u32> = library.tracks.iter().map(|t| t.id).collect();
    let mut lists: Vec<Listed> = Vec::new();
    let mut pulled = None;
    for (phase, names) in [["HISTORY 001", "HISTORY 002"], ["HISTORY 003", "HISTORY 004"]].into_iter().enumerate() {
        if phase == 1 && tc.draw(gs::integers::<u8>().max_value(3)) > 0 {
            let opts = PullOptions { library: Some("Club".into()), force: true, ..Default::default() };
            let mut device = Device::open(&main).unwrap();
            let report = sync::pull(&desk.store, &mut device, &opts, &Ctx::silent()).unwrap();
            pulled = Some(report.snapshot.unwrap().created);
            for l in lists.iter_mut().filter(|l| l.drives.contains("Main")) {
                l.saved = true;
            }
        }
        for name in names {
            // Main always has the first list, so a pull has something to save.
            let first = name == "HISTORY 001";
            if !first && tc.draw(gs::booleans()) {
                continue;
            }
            let tracks = || tc.draw(gs::vecs(gs::sampled_from(ids.clone())).min_size(1).max_size(4));
            let on_main = (first || tc.draw(gs::booleans())).then(tracks);
            let on_backup = match tc.draw(gs::integers::<u8>().max_value(2)) {
                0 => None,
                1 => on_main.clone().or_else(|| Some(tracks())),
                _ => Some(tracks()),
            };
            for (root, drive, tracks) in [(&main, "Main", on_main), (&backup, "Backup", on_backup)] {
                let Some(tracks) = tracks else { continue };
                common::add_history(root, name, &tracks);
                match lists.iter_mut().find(|l| l.name == name && l.tracks == tracks) {
                    Some(l) => {
                        l.drives.insert(drive.into());
                    }
                    None => {
                        let drives = BTreeSet::from([drive.to_string()]);
                        lists.push(Listed { name: name.into(), tracks, drives, saved: false });
                    }
                }
            }
        }
    }
    // Recorded after this: since Main's pull, for a list only on Main.
    let after = |l: &Listed| pulled.filter(|_| !l.saved && l.drives.iter().all(|d| d == "Main"));
    let file = |id: u32| library.track(id).unwrap().file_path.clone();
    let files = |l: &Listed| l.tracks.iter().map(|&t| file(t)).collect::<Vec<_>>();
    let check = |got: &[Session], want: &[Listed], from: DateTime<Utc>| {
        for w in got.windows(2) {
            let key = |s: &Session| (s.window.before, s.window.after, s.name.clone());
            assert!(key(&w[0]) >= key(&w[1]), "newest first");
        }
        let mut got: Vec<(String, Vec<String>, BTreeSet<String>)> = got
            .iter()
            .map(|s| {
                let l = want.iter().find(|l| {
                    l.name == s.name && files(l) == s.tracks.iter().map(|e| e.file.clone().unwrap()).collect::<Vec<_>>()
                });
                let l = l.unwrap_or_else(|| panic!("{} isn't one of the lists", s.name));
                assert_eq!((s.source.as_str(), s.window.exact), ("device_library", false));
                match l.saved {
                    true => assert_eq!(
                        (s.first_seen_on.as_deref(), s.window.after, s.window.before),
                        (Some("Main"), None, pulled)
                    ),
                    false => {
                        assert_eq!((s.first_seen_on.as_deref(), s.window.after), (None, after(l)), "{}", s.name);
                        assert!(s.window.before.is_some_and(|b| b >= from && b <= Utc::now()), "{:?}", s.window);
                    }
                }
                assert!(s.tracks.iter().all(|e| e.session.as_ref() == Some(&s.id) && e.origin == EntryOrigin::Session));
                (s.name.clone(), files(l), s.drives.iter().cloned().collect())
            })
            .collect();
        let mut want: Vec<(String, Vec<String>, BTreeSet<String>)> =
            want.iter().map(|l| (l.name.clone(), files(l), l.drives.clone())).collect();
        got.sort();
        want.sort();
        assert_eq!(got, want);
    };

    let from = Utc::now();
    let all = api::sessions(Some(&desk.store), &SessionsRequest::default(), &Ctx::silent()).unwrap();
    check(&all.sessions, &lists, from);
    let warned: Vec<bool> = all.warnings.iter().map(|w| w.starts_with("Broken: ")).collect();
    assert_eq!(warned, [broken].repeat(broken.into()), "{:?}", all.warnings);
    let req = SessionsRequest { drive: Some(backup.display().to_string()), ..Default::default() };
    let on_backup: Vec<Listed> = lists
        .iter()
        .filter(|l| l.saved || l.drives.contains("Backup"))
        .map(|l| Listed { drives: l.drives.iter().filter(|d| *d == "Backup").cloned().collect(), ..l.clone() })
        .collect();
    let from = Utc::now();
    check(&api::sessions(Some(&desk.store), &req, &Ctx::silent()).unwrap().sessions, &on_backup, from);

    // Lists on drives since they were pulled go together when they share a drive; a group whose
    // lists are all known to be since Main's pull is one night, any other each its own.
    let unsaved: Vec<&Listed> = lists.iter().filter(|l| !l.saved).collect();
    let mut group: Vec<usize> = (0..unsaved.len()).collect();
    for _ in 0..unsaved.len() {
        for i in 0..unsaved.len() {
            for j in 0..unsaved.len() {
                if !unsaved[i].drives.is_disjoint(&unsaved[j].drives) {
                    let g = group[i].min(group[j]);
                    (group[i], group[j]) = (g, g);
                }
            }
        }
    }
    let mut want: Vec<Suggested> = Vec::new();
    let distinct = |ls: &[&Listed]| ls.iter().flat_map(|l| l.tracks.iter()).collect::<BTreeSet<_>>().len();
    let alone = |l: &Listed, night: bool| (vec![(l.name.clone(), files(l))], distinct(&[l]), night);
    for g in (0..unsaved.len()).filter(|&i| group[i] == i) {
        let members: Vec<&Listed> = (0..unsaved.len()).filter(|&i| group[i] == g).map(|i| unsaved[i]).collect();
        if !members.iter().any(|l| l.tracks.len() >= 3) {
            continue;
        }
        match members.iter().all(|l| after(l).is_some()) {
            true => {
                let mut names: Vec<(String, Vec<String>)> =
                    members.iter().map(|l| (l.name.clone(), files(l))).collect();
                names.sort();
                want.push((names, distinct(&members), false));
            }
            false => want.extend(members.iter().filter(|l| l.tracks.len() >= 3).map(|l| alone(l, after(l).is_none()))),
        }
    }
    want.extend(lists.iter().filter(|l| l.saved && l.tracks.len() >= 3).map(|l| alone(l, true)));
    let got = api::sets_suggest(Some(&desk.store), &SuggestSets::default(), &Ctx::silent()).unwrap();
    let mut suggested: Vec<Suggested> = got
        .candidates
        .iter()
        .map(|c| {
            let sessions: Vec<&Session> =
                c.sessions.iter().map(|id| all.sessions.iter().find(|s| &s.id == id).unwrap()).collect();
            assert_eq!(c.score, c.tracks.min(40) as u32, "{:?}", c.reasons);
            let mut names: Vec<(String, Vec<String>)> = sessions
                .iter()
                .map(|s| (s.name.clone(), s.tracks.iter().map(|e| e.file.clone().unwrap()).collect()))
                .collect();
            names.sort();
            (names, c.tracks, c.reasons.iter().any(|r| r.contains("can't tell which night")))
        })
        .collect();
    suggested.sort();
    want.sort();
    assert_eq!(suggested, want);
    assert!(Device::open(&main).unwrap().has_export(), "the drives are only read");
}

/// A set's tracks typed in by their paths on the only drive there is go in from its library (on a
/// drive that isn't there, nothing goes in); written out as an M3U8 for that drive (its paths
/// relative to the file, or absolute), or made into a playlist on it (named for the set or its day,
/// or as asked), the set lists those of its tracks the drive has, in set order (the playlist each once;
/// made again, it has nothing to add), warning about each of the others by its place in the set.
#[hegel::test(test_cases = 5, suppress_health_check = [HealthCheck::TooSlow])]
fn sets_become_playlists_on_a_drive(tc: TestCase) {
    let desk = Desk::new();
    let main = common::bigger_drive(&desk.volumes, "Main");
    let library: Library = pdb::read_library(&main.join(paths::EXPORT_PDB)).unwrap();
    let picks: Vec<Option<usize>> = (0..tc.draw(gs::integers::<usize>().min_value(1).max_value(5)))
        .map(|_| tc.draw(gs::optional(gs::integers::<usize>().max_value(library.tracks.len() - 1))))
        .collect();
    let title = tc.draw(gs::booleans()).then(|| "Fri/day".to_string());
    let fields = SetFields { title: title.clone(), date: Some("2026-10-03".into()), ..Default::default() };
    let set = api::set_new(&desk.store, &NewSet { fields, ..Default::default() }, &Ctx::silent()).unwrap().set;
    let typed = |p: &Option<usize>| p.map_or("Someone - Dubplate".to_string(), |i| library.tracks[i].file_path.clone());
    let insert = picks.iter().map(|p| Insert { at: 1, track: typed(p) }).collect();
    let req = EditSet { set: set.id.clone(), insert, ..Default::default() };
    let made = api::set_edit(&desk.store, &req, &Ctx::silent()).unwrap();
    let notes: Vec<String> = picks
        .iter()
        .map(|p| match p {
            Some(i) => format!("{} went in from the library", library.tracks[*i].display_name()),
            None => "\"Someone - Dubplate\" went in as typed: no track matches it".into(),
        })
        .collect();
    assert_eq!(made.notes, notes);
    for (e, p) in made.set.tracks.iter().zip(&picks) {
        assert_eq!(e.file.as_ref(), p.map(|i| &library.tracks[i].file_path));
    }
    let found: Vec<&locallibrary::Track> = picks.iter().flatten().map(|&i| &library.tracks[i]).collect();
    let warnings: Vec<String> = picks
        .iter()
        .enumerate()
        .filter(|(_, p)| p.is_none())
        .map(|(i, _)| format!("{}. Someone - Dubplate isn't on the drive: left out", i + 1))
        .collect();

    let nowhere = EditSet { insert: vec![Insert { at: 1, track: "x".into() }], drive: Some("Nowhere".into()), ..req };
    assert_eq!(api::set_edit(&desk.store, &nowhere, &Ctx::silent()).unwrap_err().code(), "not_found");

    // Written next to the drive's own folders, its paths are relative to it.
    let output = tc.draw(gs::booleans()).then(|| main.join("friday.m3u8"));
    let req =
        SetExport { set: set.id.clone(), format: SetFormat::M3u8, drive: None, output: output.clone(), replace: false };
    let m3u8 = api::set_export(&desk.store, &req).unwrap();
    let text = match &output {
        Some(path) => std::fs::read_to_string(path).unwrap(),
        None => m3u8.text.unwrap(),
    };
    let listed: Vec<String> = text.lines().filter(|l| !l.starts_with('#')).map(String::from).collect();
    assert_eq!((m3u8.tracks, &m3u8.warnings), (found.len(), &warnings));
    assert_eq!(listed.len(), found.len());
    for (line, t) in listed.iter().zip(&found) {
        match output {
            Some(_) => assert_eq!(line, &t.file_path[1..]),
            None => {
                let file = Path::new(line);
                assert!(file.is_absolute() && file.ends_with(&t.file_path[1..]), "{line} is {}", t.file_path);
            }
        }
    }

    let req = api::SetPlaylist { set: set.id.clone(), drive: common::path(&main), name: None };
    let planned = api::set_playlist_plan(&desk.store, &req, &Ctx::silent());
    if found.is_empty() {
        assert_eq!(planned.unwrap_err().code(), "not_found");
        return;
    }
    assert_eq!(planned.unwrap().warnings, warnings);
    api::set_playlist_apply(&desk.store, &req, &Ctx::silent()).unwrap();
    let mut once: Vec<u32> = Vec::new();
    for t in &found {
        if !once.contains(&t.id) {
            once.push(t.id);
        }
    }
    let tracks_of =
        |name: &str| Device::open(&main).unwrap().library().unwrap().find_playlist(name).map(|p| p.track_ids.clone());
    let playlist = if title.is_some() { "Sets/2026-10-03 Fri-day" } else { "Sets/2026-10-03" };
    assert_eq!(tracks_of(playlist), Some(once.clone()));
    let again = api::set_playlist_plan(&desk.store, &req, &Ctx::silent()).unwrap();
    assert!(again.noop, "made again, there's nothing to add");
    if tc.draw(gs::booleans()) {
        let named = api::SetPlaylist { name: Some("Gigs/Friday".into()), ..req };
        api::set_playlist_apply(&desk.store, &named, &Ctx::silent()).unwrap();
        assert_eq!(tracks_of("Gigs/Friday"), Some(once));
    }
}
