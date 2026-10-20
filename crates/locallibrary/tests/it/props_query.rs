//! Properties of the filter language, sort orders and track selectors (`query`).

use std::collections::BTreeSet;

use hegel::generators as gs;
use hegel::{Generator, TestCase};
use locallibrary::fields::{Declared, FieldValue, TrackValues};
use locallibrary::model::{Library, Playlist, Track, TrackColor};
use locallibrary::query::{self, Filter, QueryContext, SortField, SortKey, TrackSelector};

const TEXT_FIELDS: &[&str] =
    &["", "artist:", "a:", "title:", "t:", "album:", "genre:", "g:", "label:", "comment:", "remixer:", "mix:", "path:"];
const FIELDS: &[&str] = &[
    "",
    "artist:",
    "a:",
    "title:",
    "album:",
    "genre:",
    "label:",
    "comment:",
    "c:",
    "remixer:",
    "mix:",
    "path:",
    "key:",
    "k:",
    "bpm:",
    "tempo:",
    "rating:",
    "r:",
    "plays:",
    "year:",
    "bitrate:",
    "time:",
    "color:",
    "added:",
    "playlist:",
    "in:",
    "tag:",
    "is:",
    "has:",
    "nope:",
];
const VALUES: &[&str] = &[
    "8A",
    "Am",
    "9A~",
    "12B",
    "1m",
    "120-128",
    "128-120",
    "120..128",
    ">=124",
    "<5",
    "~128",
    "4",
    "0",
    "2026",
    "2026-09",
    ">=2026-01-01",
    "2025-01..2025-12",
    "999999",
    "429497",
    ">99999999999",
    "4294967296-1",
    "6:00",
    ">5:30",
    "3:00-5:00",
    "nan",
    "inf",
    "-1",
    "aqua|none",
    "pink",
    "unfiled",
    "played",
    "artwork",
    "key",
    "sets",
    "sets/friday",
    "deep",
    "a|b",
    "x~",
];
const SORT_NAMES: &[(&str, SortField)] = &[
    ("title", SortField::Title),
    ("artist", SortField::Artist),
    ("album", SortField::Album),
    ("genre", SortField::Genre),
    ("label", SortField::Label),
    ("key", SortField::Key),
    ("bpm", SortField::Bpm),
    ("rating", SortField::Rating),
    ("color", SortField::Color),
    ("year", SortField::Year),
    ("plays", SortField::Plays),
    ("added", SortField::Added),
    ("time", SortField::Time),
    ("bitrate", SortField::Bitrate),
    ("path", SortField::Path),
];

/// Short words from a small alphabet, so substring matches between fields are common.
fn word() -> impl hegel::PrintableGenerator<String> {
    gs::text().alphabet("abAé ").max_size(4)
}

#[hegel::composite]
fn track(tc: &TestCase, id: u32) -> Track {
    Track {
        id,
        title: tc.draw(word()),
        artist: tc.draw(word()),
        album: tc.draw(word()),
        album_artist: tc.draw(word()),
        genre: tc.draw(word()),
        label: tc.draw(word()),
        key: tc.draw(hegel::one_of!(gs::sampled_from(vec!["8A", "Am", "9A", "3B", "", "x"]).map(String::from), word())),
        remixer: tc.draw(word()),
        original_artist: String::new(),
        composer: String::new(),
        comment: tc.draw(word()),
        mix_name: tc.draw(word()),
        isrc: String::new(),
        bpm: tc.draw(hegel::one_of!(gs::floats::<f64>().min_value(0.0).max_value(200.0), gs::floats::<f64>())),
        duration_secs: tc.draw(gs::integers::<u32>().max_value(1000)),
        year: tc.draw(gs::integers::<u16>().max_value(2030)),
        track_number: 0,
        disc_number: 0,
        rating: tc.draw(gs::integers::<u8>().max_value(5)),
        color: tc.draw(gs::optional(gs::integers::<u8>().min_value(1).max_value(8))).and_then(TrackColor::from_id),
        play_count: tc.draw(gs::integers::<u16>().max_value(3)),
        bitrate: tc.draw(gs::sampled_from(vec![128u32, 320, 1411])),
        sample_rate: 44100,
        sample_depth: 16,
        file_size: 1,
        file_path: format!("/Contents/{id}/{}.mp3", tc.draw(word())),
        analyze_path: if tc.draw(gs::booleans()) {
            format!("/PIONEER/USBANLZ/{id}/ANLZ0000.DAT")
        } else {
            String::new()
        },
        date_added: tc.draw(hegel::one_of!(
            gs::sampled_from(vec!["2026-09-30", "2025-01-02", "2026-01", "", "999999-01-01", "2026-99-99"])
                .map(String::from),
            gs::from_regex("[0-9]{4}-[0-9]{2}-[0-9]{2}").fullmatch(true),
            gs::text().max_size(10),
        )),
        release_date: String::new(),
        artwork_id: tc.draw(gs::optional(gs::integers::<u32>().max_value(3))),
        artwork_path: None,
    }
}

/// A small library with a folder "Sets" holding playlists "Friday" and "Deep".
#[hegel::composite]
fn library(tc: &TestCase) -> Library {
    let n = tc.draw(gs::integers::<u32>().max_value(8));
    let tracks: Vec<Track> = (1..=n).map(|id| tc.draw(track(id))).collect();
    let ids = || gs::vecs(gs::integers::<u32>().min_value(1).max_value(n.max(1))).max_size(6);
    let list = |id: u32, parent: Option<u32>, name: &str, track_ids: Vec<u32>| Playlist {
        id,
        parent_id: parent,
        name: name.into(),
        is_folder: parent.is_none(),
        sort_order: id,
        track_ids,
    };
    let playlists = vec![
        list(1, None, "Sets", Vec::new()),
        list(2, Some(1), "Friday", tc.draw(ids())),
        list(3, Some(1), "Deep", tc.draw(ids())),
    ];
    Library { tracks, playlists, ..Default::default() }
}

/// One filter term with no spaces or quotes, sometimes negated.
fn token() -> impl hegel::PrintableGenerator<String> {
    hegel::compose!(|tc| {
        let neg = if tc.draw(gs::booleans()) { "-" } else { "" };
        let field = tc.draw(gs::sampled_from(FIELDS.to_vec()));
        let value = tc.draw(hegel::one_of!(
            gs::sampled_from(VALUES.to_vec()).map(String::from),
            gs::text().alphabet("ab8A9é-|~<>=.:0123456789").min_size(1).max_size(8),
        ));
        format!("{neg}{field}{value}")
    })
}

fn matching(f: &Filter, lib: &Library) -> Vec<bool> {
    let ctx = QueryContext::new(lib);
    lib.tracks.iter().map(|t| f.matches(t, &ctx)).collect()
}

fn ids(tracks: &[&Track]) -> Vec<u32> {
    tracks.iter().map(|t| t.id).collect()
}

/// Parsing never panics, and a filter that parses can be run on any track.
#[hegel::test(test_cases = 500)]
#[hegel::explicit_test_case(s = "added:429497".to_string(), lib = Library::default())]
fn filters_never_panic(tc: TestCase) {
    let s = tc.draw(hegel::one_of!(gs::text().max_size(30), gs::vecs(token()).max_size(4).map(|v| v.join(" "))));
    let lib = tc.draw(library());
    if let Ok(f) = Filter::parse(&s) {
        let _ = matching(&f, &lib);
        let _ = f.select(&QueryContext::new(&lib));
    }
}

/// `A B` matches exactly the tracks both `A` and `B` match (and parses when both do).
#[hegel::test(test_cases = 300)]
fn terms_are_conjunctions(tc: TestCase) {
    let (a, b) = (tc.draw(token()), tc.draw(token()));
    let lib = tc.draw(library());
    let both = Filter::parse(&format!("{a} {b}"));
    match (Filter::parse(&a), Filter::parse(&b)) {
        (Ok(fa), Ok(fb)) => {
            let both = matching(&both.unwrap(), &lib);
            let expected: Vec<bool> =
                matching(&fa, &lib).iter().zip(matching(&fb, &lib)).map(|(x, y)| *x && y).collect();
            assert_eq!(both, expected);
        }
        _ => assert!(both.is_err()),
    }
}

/// `-X` matches exactly the tracks `X` doesn't.
#[hegel::test(test_cases = 300)]
fn minus_negates(tc: TestCase) {
    let x = tc.draw(token());
    tc.assume(!x.starts_with('-') && !x.starts_with(|c: char| c.is_ascii_digit()));
    let lib = tc.draw(library());
    match (Filter::parse(&x), Filter::parse(&format!("-{x}"))) {
        (Ok(f), Ok(not)) => {
            let negated: Vec<bool> = matching(&f, &lib).iter().map(|m| !m).collect();
            assert_eq!(matching(&not, &lib), negated);
        }
        (f, not) => assert_eq!(f.is_err(), not.is_err()),
    }
}

/// `field:a|b` matches exactly the tracks `field:a` or `field:b` matches.
#[hegel::test(test_cases = 300)]
fn bar_is_a_disjunction(tc: TestCase) {
    let field = tc.draw(gs::sampled_from(
        TEXT_FIELDS.iter().chain(&["key:", "color:", "playlist:", "tag:", "is:", "has:"]).copied().collect::<Vec<_>>(),
    ));
    let value = || {
        hegel::one_of!(
            gs::text()
                .alphabet("abAé 8")
                .min_size(1)
                .max_size(4)
                .filter(|v| !v.trim().is_empty())
                .map(|v| v.replace(' ', "")),
            gs::sampled_from(vec![
                "8A", "Am", "9A", "none", "aqua", "unfiled", "played", "key", "artwork", "sets", "deep"
            ])
            .map(String::from),
        )
        .filter(|v: &String| !v.is_empty())
    };
    let (a, b) = (tc.draw(value()), tc.draw(value()));
    let suffix = if field == "key:" && tc.draw(gs::booleans()) { "~" } else { "" };
    let lib = tc.draw(library());
    let either = Filter::parse(&format!("{field}{a}|{b}{suffix}"));
    match (Filter::parse(&format!("{field}{a}{suffix}")), Filter::parse(&format!("{field}{b}{suffix}"))) {
        (Ok(fa), Ok(fb)) => {
            let expected: Vec<bool> =
                matching(&fa, &lib).iter().zip(matching(&fb, &lib)).map(|(x, y)| *x || y).collect();
            assert_eq!(matching(&either.unwrap(), &lib), expected);
        }
        _ => assert!(either.is_err()),
    }
}

/// A range means the same whichever end comes first, written `a-b` or `a..b`.
#[hegel::test(test_cases = 300)]
fn ranges_are_unordered(tc: TestCase) {
    let field = tc.draw(gs::sampled_from(vec!["bpm", "rating", "plays", "year", "bitrate", "time"]));
    let value = || {
        hegel::one_of!(
            gs::integers::<u32>().map(|n| n.to_string()),
            gs::floats::<f64>().min_value(0.0).max_value(1e6).map(|x| x.to_string()),
            gs::tuples!(gs::integers::<u32>().max_value(99), gs::integers::<u32>().max_value(59))
                .map(|(m, s)| format!("{m}:{s:02}")),
        )
    };
    let (a, b) = (tc.draw(value()), tc.draw(value()));
    let forms = [format!("{a}-{b}"), format!("{b}-{a}"), format!("{a}..{b}"), format!("{b}..{a}")];
    let parsed: Vec<_> = forms.iter().map(|r| Filter::parse(&format!("{field}:{r}")).ok()).collect();
    assert!(parsed.windows(2).all(|w| w[0] == w[1]), "{parsed:?}");
}

/// `time:` takes the same numbers `plays:` does (in seconds): nothing that isn't a finite number.
#[hegel::test(test_cases = 500)]
#[hegel::explicit_test_case(v = "NaN".to_string())]
#[hegel::explicit_test_case(v = "inf".to_string())]
fn time_takes_only_finite_numbers(tc: TestCase) {
    let v = tc.draw(hegel::one_of!(
        gs::text().alphabet("0123456789.e-+<>=infaNI").min_size(1).max_size(6),
        gs::sampled_from(vec!["nan", "NaN", "inf", "-inf", "+infinity", ">=inf", "<NaN", "1e400"]).map(String::from),
    ));
    assert_eq!(Filter::parse(&format!("time:{v}")).is_ok(), Filter::parse(&format!("plays:{v}")).is_ok(), "{v:?}");
}

/// `time:m:ss` matches tracks exactly that long.
#[hegel::test(test_cases = 200)]
fn time_matches_the_duration(tc: TestCase) {
    let (m, s) = (tc.draw(gs::integers::<u32>().max_value(20)), tc.draw(gs::integers::<u32>().max_value(59)));
    let lib = tc.draw(library());
    let f = Filter::parse(&format!("time:{m}:{s:02}")).unwrap();
    let expected: Vec<bool> = lib.tracks.iter().map(|t| t.duration_secs == m * 60 + s).collect();
    assert_eq!(matching(&f, &lib), expected);
}

/// `added:2026` and `added:2026-09` match the dates in that year or month.
#[hegel::test(test_cases = 200)]
fn added_matches_date_prefixes(tc: TestCase) {
    let lib = tc.draw(library());
    let (y, m) = (
        tc.draw(gs::integers::<u32>().min_value(2024).max_value(2026)),
        tc.draw(gs::integers::<u32>().min_value(1).max_value(12)),
    );
    for q in [format!("{y}"), format!("{y}-{m:02}")] {
        let f = Filter::parse(&format!("added:{q}")).unwrap();
        let well_formed = |d: &str| {
            let p: Vec<&str> = d.split('-').collect();
            p.len() == 3
                && p[0].parse::<u32>().is_ok_and(|v| (1000..=9999).contains(&v))
                && p[1].parse::<u32>().is_ok_and(|v| (1..=12).contains(&v))
                && p[2].parse::<u32>().is_ok_and(|v| (1..=31).contains(&v))
        };
        for (t, hit) in lib.tracks.iter().zip(matching(&f, &lib)) {
            if well_formed(&t.date_added) {
                let p: Vec<u32> = t.date_added.split('-').map(|x| x.parse().unwrap()).collect();
                let want = p[0] == y && (q.len() == 4 || p[1] == m);
                assert_eq!(hit, want, "{q} vs {:?}", t.date_added);
            }
        }
    }
}

/// Single-letter field names take precedence over bare words: `c:\Music` is a comment filter for
/// `\Music`, not a search for the text `c:\Music` (paths on a drive never have drive letters). A
/// letter that names no field is refused; quoted, it's the text.
#[test]
fn single_letter_fields_win_over_bare_words() {
    assert_eq!(Filter::parse("c:\\Music").unwrap(), Filter::parse("comment:\\Music").unwrap());
    assert!(Filter::parse("x:y").is_err());
    assert!(Filter::parse("\"x:y\"").is_ok());
}

fn sort_spec(keys: &[SortKey]) -> String {
    let name = |f: SortField| SORT_NAMES.iter().find(|(_, x)| *x == f).unwrap().0;
    keys.iter()
        .map(|k| format!("{}{}", if k.descending { "-" } else { "" }, name(k.field)))
        .collect::<Vec<_>>()
        .join(",")
}

fn sort_keys() -> impl hegel::PrintableGenerator<Vec<SortKey>> {
    gs::vecs(
        gs::tuples!(gs::sampled_from(SORT_NAMES.iter().map(|(_, f)| *f).collect::<Vec<_>>()), gs::booleans())
            .map(|(field, descending)| SortKey { field, descending }),
    )
    .min_size(1)
    .max_size(3)
}

/// A sort order reads back from its text form, and parsing any text never panics.
#[hegel::test(test_cases = 300)]
fn sort_orders_round_trip(tc: TestCase) {
    let keys = tc.draw(sort_keys());
    assert_eq!(query::parse_sort(&sort_spec(&keys)).unwrap(), keys);
    let junk = tc.draw(gs::text().alphabet("-,abkeytl ").max_size(12));
    let _ = query::parse_sort(&junk);
}

/// Sorting is a stable permutation, and sorting by `k1,k2` is sorting by `k2` then (stably) by
/// `k1`.
#[hegel::test(test_cases = 300)]
fn sorting_is_stable_and_lexicographic(tc: TestCase) {
    let mut lib = tc.draw(library());
    // Exact copies (but for the id) must keep their relative order.
    let copies: Vec<Track> =
        lib.tracks.iter().map(|t| Track { id: t.id + 100, ..t.clone() }).filter(|_| tc.draw(gs::booleans())).collect();
    lib.tracks.extend(copies);
    let keys = tc.draw(sort_keys());

    let mut sorted: Vec<&Track> = lib.tracks.iter().collect();
    query::sort_tracks(&mut sorted, &keys);
    let mut a = ids(&sorted);
    let mut b = ids(&lib.tracks.iter().collect::<Vec<_>>());
    a.sort_unstable();
    b.sort_unstable();
    assert_eq!(a, b, "a permutation");
    for t in &lib.tracks {
        let pos = |id| sorted.iter().position(|x| x.id == id);
        if let (Some(orig), Some(copy)) = (pos(t.id), pos(t.id + 100)) {
            assert!(orig < copy, "stable");
        }
    }

    let mut again = sorted.clone();
    query::sort_tracks(&mut again, &keys);
    assert_eq!(ids(&again), ids(&sorted), "idempotent");

    let mut stepwise: Vec<&Track> = lib.tracks.iter().collect();
    for k in keys.iter().rev() {
        query::sort_tracks(&mut stepwise, std::slice::from_ref(k));
    }
    assert_eq!(ids(&stepwise), ids(&sorted), "lexicographic");
}

#[hegel::composite]
fn selector(tc: &TestCase, lib: Library) -> TrackSelector {
    let n = lib.tracks.len() as u32;
    let paths: Vec<String> = lib.tracks.iter().map(|t| t.file_path.to_uppercase()).collect();
    // Each part that can fail (nothing matches) is there now and then, so most selectors work.
    let sometimes = || gs::weighted_booleans(0.25);
    TrackSelector {
        ids: tc.draw(gs::vecs(gs::integers::<u32>().min_value(1).max_value(n + 1)).max_size(3)),
        query: if tc.draw(sometimes()) { Some(tc.draw(word())) } else { None },
        playlist: tc.draw(gs::optional(gs::sampled_from(vec!["Friday", "sets/deep", "Nope"]).map(String::from))),
        album: if tc.draw(sometimes()) { Some(tc.draw(word())) } else { None },
        filter: if tc.draw(sometimes()) { Some(tc.draw(token())) } else { None },
        paths: if paths.is_empty() { Vec::new() } else { tc.draw(gs::vecs(gs::sampled_from(paths)).max_size(2)) },
        history: None,
    }
}

/// Shrunk counterexample: a playlist entry whose track isn't in the collection was selected.
#[test]
fn dangling_playlist_entries_select_nothing() {
    let friday =
        Playlist { id: 1, parent_id: None, name: "Friday".into(), is_folder: false, sort_order: 0, track_ids: vec![1] };
    let lib = Library { playlists: vec![friday], ..Default::default() };
    let sel = TrackSelector { playlist: Some("Friday".into()), ..Default::default() };
    assert_eq!(sel.select(&lib).unwrap(), Vec::<u32>::new());
    assert_eq!(sel.select_ordered(&QueryContext::new(&lib), false).unwrap(), Vec::<u32>::new());
}

/// `select` gives sorted ids without repeats, the same set `select_ordered` gives (which has no
/// repeats either), and both fail together.
#[hegel::test(test_cases = 300)]
fn select_and_select_ordered_agree(tc: TestCase) {
    let lib = tc.draw(library());
    let sel = tc.draw(selector(lib.clone()));
    let ctx = QueryContext::new(&lib);
    match (sel.select(&lib), sel.select_ordered(&ctx, false)) {
        (Ok(set), Ok(ordered)) => {
            assert!(set.windows(2).all(|w| w[0] < w[1]), "sorted, no repeats: {set:?}");
            let unique: BTreeSet<u32> = ordered.iter().copied().collect();
            assert_eq!(unique.len(), ordered.len(), "no repeats: {ordered:?}");
            assert_eq!(unique.into_iter().collect::<Vec<_>>(), set);
            assert!(set.iter().all(|id| lib.track(*id).is_some()));
        }
        (a, b) => assert!(a.is_err() && b.is_err(), "{a:?} {b:?}"),
    }
}

/// Each field a selector sets, as a selector of its own.
fn fields_of(sel: &TrackSelector) -> Vec<TrackSelector> {
    let none = TrackSelector::default;
    let mut out = Vec::new();
    if !sel.ids.is_empty() {
        out.push(TrackSelector { ids: sel.ids.clone(), ..none() });
    }
    if !sel.paths.is_empty() {
        out.push(TrackSelector { paths: sel.paths.clone(), ..none() });
    }
    for (set, field) in [
        (sel.playlist.is_some(), TrackSelector { playlist: sel.playlist.clone(), ..none() }),
        (sel.history.is_some(), TrackSelector { history: sel.history.clone(), ..none() }),
        (sel.query.is_some(), TrackSelector { query: sel.query.clone(), ..none() }),
        (sel.album.is_some(), TrackSelector { album: sel.album.clone(), ..none() }),
        (sel.filter.is_some(), TrackSelector { filter: sel.filter.clone(), ..none() }),
    ] {
        if set {
            out.push(field);
        }
    }
    out
}

/// A selector picks the tracks every one of its fields picks on its own, in the order of the
/// first of them (ids, paths, playlist, history list, then library order), and fails when any
/// field does. Without `allow_empty` it also fails when no track fits every field.
#[hegel::test(test_cases = 500)]
fn selection_is_the_intersection_of_its_fields(tc: TestCase) {
    let lib = tc.draw(library());
    let sel = tc.draw(selector(lib.clone()));
    let ctx = QueryContext::new(&lib);
    let fields = fields_of(&sel);
    if fields.is_empty() {
        assert!(sel.select_ordered(&ctx, true).is_err(), "no field selects nothing, not everything");
        return;
    }
    let alone: Result<Vec<Vec<u32>>, _> = fields.iter().map(|f| f.select_ordered(&ctx, true)).collect();
    let expected = match (sel.select_ordered(&ctx, true), alone) {
        (Ok(together), Ok(alone)) => {
            let mut expected = alone[0].clone();
            for other in &alone[1..] {
                expected.retain(|id| other.contains(id));
            }
            assert_eq!(together, expected);
            expected
        }
        (Err(_), Err(_)) => {
            assert!(sel.select(&lib).is_err());
            return;
        }
        (together, alone) => panic!("{together:?} but alone {alone:?}"),
    };
    let fields_work = fields.iter().all(|f| f.select(&lib).is_ok());
    match sel.select(&lib) {
        Ok(set) => {
            assert!(fields_work);
            let mut sorted = expected;
            sorted.sort_unstable();
            assert_eq!(set, sorted);
        }
        Err(_) => assert!(!fields_work || (fields.len() > 1 && expected.is_empty())),
    }
}

/// A blank filter or query never selects anything, whatever else is given: selecting fails
/// instead of taking every track (`track rm --filter ""` would delete the whole library).
#[hegel::test(test_cases = 300)]
fn blank_filters_and_queries_never_select_anything(tc: TestCase) {
    let lib = tc.draw(library());
    let mut sel = tc.draw(selector(lib.clone()));
    let blank = tc.draw(gs::text().alphabet(" \t\n\"").max_size(4));
    if tc.draw(gs::booleans()) {
        sel.filter = Some(blank);
    } else {
        // Quotes mean nothing in a search.
        sel.query = Some(blank.replace('"', " "));
    }
    assert!(sel.select_ordered(&QueryContext::new(&lib), true).is_err(), "{sel:?}");
    assert!(sel.select(&lib).is_err(), "{sel:?}");
}

/// A track's number of some kind.
type NumberOf = fn(&Track) -> f64;

/// Whether a track is what an `is:` or `has:` value names.
type Holds<'a> = Box<dyn Fn(&Track) -> bool + 'a>;

/// `rating:`, `plays:`, `year:` and `bitrate:` compare the number as written (`<`, `<=`, `>`,
/// `>=`, an exact value or a range either way round); `bpm:` widens an exact tempo by half a
/// beat; dates compare by day, a bare prefix standing for its first or last day as the operator
/// needs.
#[hegel::test(test_cases = 300)]
fn numeric_terms_compare_as_written(tc: TestCase) {
    let lib = tc.draw(library());
    let fields: Vec<(&str, NumberOf)> = vec![
        ("rating", |t| f64::from(t.rating)),
        ("plays", |t| f64::from(t.play_count)),
        ("year", |t| f64::from(t.year)),
        ("bitrate", |t| f64::from(t.bitrate)),
        ("bpm", |t| t.bpm),
    ];
    let (name, read) = fields[tc.draw(gs::integers::<usize>().max_value(fields.len() - 1))];
    // Bounds at a track's own value as often as not, where the operators differ.
    let values: Vec<f64> =
        lib.tracks.iter().map(read).filter(|v| v.fract() == 0.0 && *v >= 0.0 && *v <= 2030.0).collect();
    let bound = |tc: &TestCase| match values.is_empty() || tc.draw(gs::booleans()) {
        true => f64::from(tc.draw(gs::integers::<u16>().max_value(2030))),
        false => values[tc.draw(gs::integers::<usize>().max_value(values.len() - 1))],
    };
    let (x, y) = (bound(&tc), bound(&tc));
    let tolerance = if name == "bpm" { 0.5 } else { 0.0 };
    let (q, want): (String, Box<dyn Fn(f64) -> bool>) = match tc.draw(gs::integers::<u8>().max_value(5)) {
        0 => (format!("<{x}"), Box::new(move |v| v < x)),
        1 => (format!("<={x}"), Box::new(move |v| v <= x)),
        2 => (format!(">{x}"), Box::new(move |v| v > x)),
        3 => (format!(">={x}"), Box::new(move |v| v >= x)),
        4 => (format!("{x}..{y}"), Box::new(move |v| v >= x.min(y) && v <= x.max(y))),
        _ => (format!("{x}"), Box::new(move |v| (v - x).abs() <= tolerance)),
    };
    let f = Filter::parse(&format!("{name}:{q}")).unwrap();
    let expected: Vec<bool> = lib.tracks.iter().map(|t| want(read(t))).collect();
    assert_eq!(matching(&f, &lib), expected, "{name}:{q}");

    // Dates: a day `YYYY-MM-DD` against a bound that may stop at the month or year.
    let (by, bm, bd) = (
        tc.draw(gs::integers::<u32>().min_value(2024).max_value(2026)),
        tc.draw(gs::optional(gs::integers::<u32>().min_value(1).max_value(12))),
        tc.draw(gs::integers::<u32>().min_value(1).max_value(28)),
    );
    let day = tc.draw(gs::booleans()) && bm.is_some();
    let bound = match (bm, day) {
        (Some(m), true) => format!("{by}-{m:02}-{bd:02}"),
        (Some(m), false) => format!("{by}-{m:02}"),
        (None, _) => format!("{by}"),
    };
    let first = by * 10_000 + bm.unwrap_or(1) * 100 + if day { bd } else { 1 };
    let last = by * 10_000 + bm.unwrap_or(12) * 100 + if day { bd } else { 31 };
    let op = tc.draw(gs::sampled_from(vec!["<", "<=", ">", ">="]));
    let f = Filter::parse(&format!("added:{op}{bound}")).unwrap();
    for (t, hit) in lib.tracks.iter().zip(matching(&f, &lib)) {
        let p: Vec<u32> = t.date_added.split('-').filter_map(|x| x.parse().ok()).collect();
        let well_formed = p.len() == 3
            && t.date_added.len() == 10
            && (1000..=9999).contains(&p[0])
            && (1..=12).contains(&p[1])
            && (1..=31).contains(&p[2]);
        if !well_formed {
            continue;
        }
        let v = p[0] * 10_000 + p[1] * 100 + p[2];
        let want = match op {
            "<" => v < first,
            "<=" => v <= last,
            ">" => v > last,
            _ => v >= first,
        };
        assert_eq!(hit, want, "added:{op}{bound} vs {}", t.date_added);
    }
}

/// `is:` and `has:` match what they name: played or not, in a playlist or not, with artwork, a
/// comment, a key or analysis files; several values separated by `|` match any of them.
#[hegel::test(test_cases = 300)]
fn is_and_has_match_what_they_name(tc: TestCase) {
    let lib = tc.draw(library());
    let filed: BTreeSet<u32> = lib.playlists.iter().flat_map(|p| p.track_ids.iter().copied()).collect();
    let all: Vec<(&str, &str, Holds)> = vec![
        ("is", "played", Box::new(|t: &Track| t.play_count > 0)),
        ("is", "unplayed", Box::new(|t: &Track| t.play_count == 0)),
        ("is", "unfiled", Box::new(|t: &Track| !filed.contains(&t.id))),
        ("has", "artwork", Box::new(|t: &Track| t.artwork_id.is_some())),
        ("has", "art", Box::new(|t: &Track| t.artwork_id.is_some())),
        ("has", "comment", Box::new(|t: &Track| !t.comment.trim().is_empty())),
        ("has", "key", Box::new(|t: &Track| !t.key.trim().is_empty())),
        ("has", "analysis", Box::new(|t: &Track| !t.analyze_path.is_empty())),
    ];
    let field = tc.draw(gs::sampled_from(vec!["is", "has"]));
    let mine: Vec<&(&str, &str, Holds)> = all.iter().filter(|(f, _, _)| *f == field).collect();
    let picked: Vec<usize> = (0..mine.len()).filter(|_| tc.draw(gs::booleans())).collect();
    if picked.is_empty() {
        return;
    }
    let values: Vec<&str> = picked.iter().map(|i| mine[*i].1).collect();
    let f = Filter::parse(&format!("{field}:{}", values.join("|"))).unwrap();
    let expected: Vec<bool> = lib.tracks.iter().map(|t| picked.iter().any(|i| (mine[*i].2)(t))).collect();
    assert_eq!(matching(&f, &lib), expected, "{field}:{}", values.join("|"));
    assert!(Filter::parse(&format!("{field}:nonsense")).is_err());
}

/// `tag:` matches a track carrying a My Tag of that path or name, ignoring case and spacing, and
/// can't be run without the drive's My Tags; `playlist:` names a playlist, or a folder and every
/// playlist below it.
#[hegel::test(test_cases = 300)]
fn tag_and_playlist_terms_match_what_they_name(tc: TestCase) {
    use rustc_hash::FxHashMap;
    let lib = tc.draw(library());
    let paths = ["Genre/Deep", "Genre/Peak Time", "Mood/Deep"];
    let tags: FxHashMap<String, Vec<String>> = lib
        .tracks
        .iter()
        .map(|t| {
            let mine: Vec<String> = paths.iter().filter(|_| tc.draw(gs::booleans())).map(|p| p.to_string()).collect();
            (locallibrary::paths::key(&t.file_path).to_lowercase(), mine)
        })
        .collect();
    let want = tc.draw(gs::sampled_from(vec!["deep", "Genre/Deep", "peak  time", "GENRE/PEAK TIME", "mood"]));
    let f = Filter::parse(&format!("tag:\"{want}\"")).unwrap();
    assert!(f.check(&QueryContext::new(&lib)).is_err(), "a tag: term without My Tags");
    let ctx = QueryContext::new(&lib).with_tags(&tags);
    f.check(&ctx).unwrap();
    let fold = |s: &str| s.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase();
    for t in &lib.tracks {
        let mine = &tags[&locallibrary::paths::key(&t.file_path).to_lowercase()];
        let hit = mine.iter().any(|p| fold(p) == fold(want) || fold(p.rsplit('/').next().unwrap()) == fold(want));
        assert_eq!(f.matches(t, &ctx), hit, "tag:{want} on {mine:?}");
    }

    let (spec, lists): (&str, &[u32]) = match tc.draw(gs::integers::<u8>().max_value(2)) {
        0 => ("Sets", &[2, 3]),
        1 => ("Sets/Friday", &[2]),
        _ => ("Deep", &[3]),
    };
    let f = Filter::parse(&format!("playlist:{spec}")).unwrap();
    let held: BTreeSet<u32> =
        lib.playlists.iter().filter(|p| lists.contains(&p.id)).flat_map(|p| p.track_ids.iter().copied()).collect();
    let expected: Vec<bool> = lib.tracks.iter().map(|t| held.contains(&t.id)).collect();
    assert_eq!(matching(&f, &lib), expected, "playlist:{spec}");
}

/// Selecting a history list's tracks takes the list of that name: the library's own, or with the
/// drive's lists from both its databases, the tracks of the one it names by file. A list that
/// isn't there is an error.
#[hegel::test(test_cases = 200)]
fn a_history_list_selects_its_tracks(tc: TestCase) {
    use locallibrary::plays::HistoryList;
    let mut lib = tc.draw(library());
    let n = u32::try_from(lib.tracks.len()).unwrap();
    if n == 0 {
        return;
    }
    let entries =
        |tc: &TestCase| tc.draw(gs::vecs(gs::integers::<u32>().min_value(1).max_value(n)).min_size(1).max_size(4));
    let names = ["HISTORY 001", "HISTORY 002"];
    lib.history = names
        .iter()
        .enumerate()
        .map(|(i, name)| Playlist {
            id: 10 + u32::try_from(i).unwrap(),
            parent_id: None,
            name: name.to_string(),
            is_folder: false,
            sort_order: 0,
            track_ids: entries(&tc),
        })
        .collect();
    let name = tc.draw(gs::sampled_from(vec!["HISTORY 001", "history 002", "HISTORY 009"]));
    let sel = TrackSelector { history: Some(name.into()), ..Default::default() };
    let own = lib.history.iter().find(|h| h.name.eq_ignore_ascii_case(name));
    let sorted = |mut v: Vec<u32>| {
        v.sort_unstable();
        v.dedup();
        v
    };
    match (sel.select_ordered(&QueryContext::new(&lib), false), own) {
        (Ok(ids), Some(h)) => assert_eq!(sorted(ids), sorted(h.track_ids.clone())),
        (Err(_), None) => {}
        (got, own) => panic!("{got:?} for {own:?}"),
    }
    // The drive's lists, read from both databases: the tracks by file.
    let drives: Vec<HistoryList> = names
        .iter()
        .map(|name| HistoryList {
            name: name.to_string(),
            path: name.to_string(),
            database: "onelibrary",
            tracks: entries(&tc).iter().map(|id| lib.track(*id).unwrap().clone()).collect(),
            missing: 0,
            missing_at: Vec::new(),
        })
        .collect();
    let ctx = QueryContext::new(&lib).with_history(&drives);
    let theirs = drives.iter().find(|h| h.name.eq_ignore_ascii_case(name));
    match (sel.select_ordered(&ctx, false), theirs) {
        (Ok(ids), Some(h)) => assert_eq!(sorted(ids), sorted(h.tracks.iter().map(|t| t.id).collect())),
        (Err(_), None) => {}
        (got, theirs) => panic!("{got:?} for {theirs:?}"),
    }
}

/// What analyzer plugins worked out for `lib`'s tracks, and the fields they declare: `sound.plays`
/// (an integer) and `sound.year` (a number) holding the track's own play count and year, `sound.mood`
/// (text), `sound.tags` (lists of text) and `sound.vocal` (true or false), each left out for some
/// tracks.
#[hegel::test_helper]
fn plugin_values(tc: &TestCase, lib: &Library) -> TrackValues {
    use locallibrary::fields::FieldType::*;
    let declared: Declared = [("sound.plays", Integer), ("sound.year", Number), ("sound.mood", String)]
        .into_iter()
        .chain([("sound.tags", Strings), ("sound.vocal", Boolean)])
        .map(|(name, kind)| (name.to_string(), kind))
        .collect();
    let words = || gs::text().alphabet("abAB1 -").max_size(6);
    let mut values = TrackValues { declared, ..Default::default() };
    for t in &lib.tracks {
        let mut fields = locallibrary::fields::Fields::new();
        let mut has = |name: &str, value: FieldValue| {
            if tc.draw(gs::integers::<u8>().max_value(3)) > 0 {
                fields.insert(name.to_string(), value);
            }
        };
        has("sound.plays", FieldValue::Integer(t.play_count.into()));
        has("sound.year", FieldValue::Number(t.year.into()));
        has("sound.mood", FieldValue::String(tc.draw(words())));
        has("sound.tags", FieldValue::Strings(tc.draw(gs::vecs(words()).max_size(2))));
        has("sound.vocal", FieldValue::Boolean(tc.draw(gs::booleans())));
        if !fields.is_empty() {
            values.tracks.insert(t.id, fields);
        }
    }
    values
}

fn matching_with(f: &Filter, lib: &Library, values: &TrackValues) -> Vec<bool> {
    let mut ctx = QueryContext::new(lib);
    ctx.fields = Some(values);
    lib.tracks.iter().map(|t| f.matches(t, &ctx)).collect()
}

/// A number condition on a plugin's field holding a track's play count or year (`sound.plays:>2`)
/// reads as the same condition on the track's own (`plays:>2`) does: refused when that is,
/// matching the tracks that one matches among those with a value, and no track without one
/// (every one, negated).
#[hegel::test(test_cases = 300)]
fn a_plugins_number_field_filters_as_the_tracks_own_does(tc: TestCase) {
    let lib = tc.draw(library());
    let values = plugin_values(&tc, &lib);
    let (own, name) = tc.draw(gs::sampled_from(vec![("plays", "sound.plays"), ("year", "Sound.Year")]));
    let value = tc.draw(hegel::one_of!(
        gs::sampled_from(vec!["2", "0", ">=2", "<1", "1-3", "3..1", "2026", ">=2025", "2.5", "1|2", "x", "nan"])
            .map(String::from),
        gs::text().alphabet("0123456789.-<>=|").min_size(1).max_size(6),
    ));
    let negate = if tc.draw(gs::booleans()) { "-" } else { "" };
    let plugin = Filter::parse(&format!("{negate}{name}:{value}"));
    let theirs = Filter::parse(&format!("{own}:{value}"));
    let mut ctx = QueryContext::new(&lib);
    ctx.fields = Some(&values);
    let plugin = plugin.and_then(|f| f.check(&ctx).map(|()| f));
    let (plugin, theirs) = match (plugin, theirs) {
        (Ok(p), Ok(t)) => (p, t),
        (p, t) => {
            assert_eq!(p.is_err(), t.is_err(), "{name}:{value} against {own}:{value}");
            return;
        }
    };
    assert!(plugin.uses_fields());
    let field = name.to_ascii_lowercase();
    let theirs = matching(&theirs, &lib);
    for ((t, got), own) in lib.tracks.iter().zip(matching_with(&plugin, &lib, &values)).zip(theirs) {
        let expected = values.get(t.id, &field).is_some() && own;
        let negated = !negate.is_empty();
        assert_eq!(got, expected != negated, "track {} {name}:{value}", t.id);
    }
    // Without the analyzers' values at hand, it can't be answered; one no analyzer declares, never.
    assert!(
        plugin.check(&QueryContext::new(&lib)).unwrap_err().to_string().contains("needs what analyzers worked out")
    );
    let nope = Filter::parse(&format!("sound.nope:{value}")).unwrap();
    assert!(nope.check(&ctx).unwrap_err().to_string().contains("there's no field \"sound.nope\""));
}

/// Words a plugin's text, list and true-or-false fields are filtered by.
const WORDS: &[&str] = &["a", "B", "ab", "1", "true", "false", "yes", "no", "a|b", "b|yes", "|"];

/// A plugin's text field matches a track whose value has one of the words given as a whole word
/// (any case), a list field one of whose items does, and a true-or-false field one any of the
/// words says (`true` or `yes`, `false` or `no`, only those); nothing to look for, or other words
/// for a true-or-false field, is refused.
#[hegel::test(test_cases = 300)]
fn a_plugins_text_list_and_switch_fields_match_whole_words(tc: TestCase) {
    let lib = tc.draw(library());
    let values = plugin_values(&tc, &lib);
    let name = tc.draw(gs::sampled_from(vec!["sound.mood", "sound.tags", "sound.vocal"]));
    let value = tc.draw(gs::sampled_from(WORDS.to_vec()));
    let words: Vec<String> = value.split('|').map(str::to_lowercase).filter(|w| !w.is_empty()).collect();
    let mut ctx = QueryContext::new(&lib);
    ctx.fields = Some(&values);
    let filter = Filter::parse(&format!("{name}:{value}")).and_then(|f| f.check(&ctx).map(|()| f));
    let switch = |w: &String| ["true", "false", "yes", "no"].contains(&w.as_str());
    if words.is_empty() || (name == "sound.vocal" && !words.iter().all(switch)) {
        assert!(filter.is_err(), "{name}:{value}");
        return;
    }
    let filter = filter.unwrap();
    let has_word =
        |s: &str| s.to_lowercase().split(|c: char| !c.is_alphanumeric()).any(|w| words.iter().any(|x| x == w));
    for (t, got) in lib.tracks.iter().zip(matching_with(&filter, &lib, &values)) {
        let expected = match values.get(t.id, name) {
            None => false,
            Some(FieldValue::String(s)) => has_word(s),
            Some(FieldValue::Strings(all)) => all.iter().any(|s| has_word(s)),
            Some(FieldValue::Boolean(b)) => words.iter().any(|w| (w == "true" || w == "yes") == *b),
            Some(other) => panic!("{other:?}"),
        };
        assert_eq!(got, expected, "track {} {name}:{value} {:?}", t.id, values.get(t.id, name));
    }
}

/// Sorting by a plugin's number field (`-sound.year`) orders the tracks with a value by it, either
/// way, keeping ties in their order, and puts those without one last, in theirs, whichever way;
/// any other field of a plugin's sorts those with one first too. A sort naming a field no
/// analyzer declares is refused, as one naming any is where the values can't be at hand.
#[hegel::test(test_cases = 300)]
fn sorting_by_a_plugins_field_puts_tracks_without_one_last(tc: TestCase) {
    let lib = tc.draw(library());
    let values = plugin_values(&tc, &lib);
    let name = tc.draw(gs::sampled_from(vec!["sound.year", "sound.plays", "sound.mood", "sound.tags", "sound.vocal"]));
    let descending = tc.draw(gs::booleans());
    let spec = format!("{}{name}", if descending { "-" } else { "" });
    let keys = query::parse_sort_with(&spec, Some(&values.declared)).unwrap();
    assert_eq!(keys.len(), 1);
    let mut sorted: Vec<&Track> = lib.tracks.iter().collect();
    query::sort_tracks_with(&mut sorted, &keys, Some(&values));
    let value = |t: &Track| values.get(t.id, name);
    let with = sorted.iter().take_while(|t| value(t).is_some()).count();
    assert!(sorted[with..].iter().all(|t| value(t).is_none()), "those without a value last");
    let without: Vec<u32> = lib.tracks.iter().filter(|t| value(t).is_none()).map(|t| t.id).collect();
    assert_eq!(ids(&sorted[with..]), without, "in their order");
    let number = |t: &Track| match value(t) {
        Some(FieldValue::Integer(n)) => Some(*n as f64),
        Some(FieldValue::Number(n)) => Some(*n),
        _ => None,
    };
    if name == "sound.year" || name == "sound.plays" {
        let mut expected: Vec<&Track> = lib.tracks.iter().filter(|t| value(t).is_some()).collect();
        expected.sort_by(|a, b| {
            let o = number(a).unwrap().total_cmp(&number(b).unwrap());
            if descending { o.reverse() } else { o }
        });
        assert_eq!(ids(&sorted[..with]), ids(&expected));
    }
    for w in sorted[..with].windows(2) {
        let o = query::compare_values(value(w[0]).unwrap(), value(w[1]).unwrap());
        assert!(if descending { o.is_ge() } else { o.is_le() }, "{:?}", (value(w[0]), value(w[1])));
    }
    let e = query::parse_sort_with("sound.nope", Some(&values.declared)).unwrap_err();
    assert!(e.to_string().contains("there's no field \"sound.nope\""), "{e}");
    let e = query::parse_sort(&spec).unwrap_err();
    assert_eq!(e.to_string(), format!("can't sort by \"{name}\" here: a plugin's field sorts track listings only"));
}
