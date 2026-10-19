//! Properties of the profile's privacy controls and genre and artist parsing (`profile`).

use indexmap::IndexMap;
use std::collections::BTreeSet;

use chrono::{Datelike, NaiveDate, Utc};
use hegel::generators as gs;
use hegel::{Generator, TestCase};
use rustc_hash::{FxHashMap, FxHashSet};

use crate::common;
use locallibrary::api::{self, ProfileRequest};
use locallibrary::device::Device;
use locallibrary::harmony::Camelot;
use locallibrary::model::{Track, normalize};
use locallibrary::profile::{
    self, ArcSection, Basis, CollectionSection, EraSection, KeysSection, MixingSection, PlayedTrack, Precision,
    PrefsUpdate, ProfileOptions, ProfilePrefs, ProfileReport, RankedSection, Section, SetsSection, Share,
    SignatureSection, Summary, Tag, TempoSection,
};
use locallibrary::progress::Ctx;
use locallibrary::sets::{self, EntryOrigin, Set, SetBook, SetEntry};
use locallibrary::store::Store;
use locallibrary::sync::{self, PullOptions};
use locallibrary::{paths, pdb};

const NAMES: &[&str] = &["Bicep", "Clara La San", "Röyksopp", "Robyn", "Fred again..", "Four Tet", "İzel", "x", "A"];
const SEPARATORS: &[&str] = &[
    ", ",
    "; ",
    ";",
    " feat. ",
    " FEAT. ",
    " feat ",
    " ft. ",
    " ft ",
    " featuring ",
    " (feat. ",
    " (ft. ",
    " & ",
    " x ",
    " vs ",
    " vs. ",
    " and ",
    " (",
    ") ",
    ",",
];

/// Artist credits as tags write them: names joined by commas, semicolons, featuring credits,
/// brackets, `&`, `x`...
fn artist() -> impl hegel::PrintableGenerator<String> {
    let name = || hegel::one_of!(gs::sampled_from(NAMES.to_vec()).map(String::from), gs::text().max_size(6));
    hegel::one_of!(
        gs::vecs(gs::tuples!(name(), gs::sampled_from(SEPARATORS.to_vec()))).min_size(1).max_size(4).map(|parts| parts
            .iter()
            .map(|(n, s)| format!("{n}{s}"))
            .collect::<String>()
            .trim_end_matches(' ')
            .to_string()),
        gs::text().max_size(20),
    )
}

/// Hiding any artist a credit names hides the credit, however it's written, so a hidden
/// artist is never named in the published profile.
#[hegel::test(test_cases = 1000)]
#[hegel::explicit_test_case(a = "A; B".to_string())]
#[hegel::explicit_test_case(a = "A (feat. B)".to_string())]
fn hiding_a_credited_artist_hides_the_credit(tc: TestCase) {
    let a = tc.draw(artist());
    for credit in profile::artist_credits(&a) {
        let prefs = ProfilePrefs { hidden_artists: vec![credit.clone()], ..Default::default() };
        assert!(prefs.is_hidden_artist(&a), "hiding {credit:?} doesn't hide {a:?}");
        assert!(!prefs.may_name("whatever", &a));
    }
}

/// Credits are trimmed, non-empty and distinct.
#[hegel::test(test_cases = 500)]
fn credits_are_distinct_names(tc: TestCase) {
    let a = tc.draw(artist());
    let credits = profile::artist_credits(&a);
    let distinct: BTreeSet<String> = credits.iter().map(|c| normalize(c)).collect();
    assert_eq!(distinct.len(), credits.len(), "{credits:?}");
    assert!(credits.iter().all(|c| !c.is_empty() && c.trim() == c), "{credits:?}");
}

fn report(tracks: &[(String, String)], top_artists: &[String]) -> ProfileReport {
    let played: Vec<PlayedTrack> = tracks
        .iter()
        .map(|(artist, title)| PlayedTrack {
            name: format!("{artist} - {title}"),
            artist: artist.clone(),
            title: title.clone(),
            sets: 2,
            plays: 2,
            opened: 0,
            closed: 0,
            unnamed: false,
        })
        .collect();
    let share = |name: &String| Share { name: name.clone(), count: 1, share: 0.1 };
    ProfileReport {
        generated: Utc::now(),
        options: ProfileOptions::default(),
        basis: Basis::Plays,
        sources: Vec::new(),
        summary: Summary { headline: String::new(), tags: Vec::new() },
        sets: SetsSection::default(),
        tempo: TempoSection::default(),
        arc: ArcSection::default(),
        mixing: MixingSection::default(),
        keys: KeysSection::default(),
        energy: None,
        genres: RankedSection::default(),
        labels: RankedSection::default(),
        artists: RankedSection { known: 1, distinct: 1, top: top_artists.iter().map(share).collect() },
        era: EraSection::default(),
        signature: SignatureSection { tracks: played.clone(), played, ..Default::default() },
        prep: None,
        collection: CollectionSection::default(),
        style: None,
        warnings: Vec::new(),
    }
}

/// The published document never names a hidden track, or a track or artist credit naming a
/// hidden artist, even when the track is pinned.
#[hegel::test(test_cases = 300)]
fn the_document_never_names_what_is_hidden(tc: TestCase) {
    let tracks: Vec<(String, String)> = tc.draw(
        gs::vecs(gs::tuples!(artist(), gs::sampled_from(vec!["Glue", "Saku", "Apricots"]).map(String::from)))
            .max_size(6),
    );
    let credits: Vec<String> = tracks.iter().flat_map(|(a, _)| profile::artist_credits(a)).collect();
    tc.assume(!credits.is_empty());
    let hidden_artists = tc.draw(gs::vecs(gs::sampled_from(credits.clone())).min_size(1).max_size(2));
    let names: Vec<String> = tracks.iter().map(|(a, t)| format!("{a} - {t}")).collect();
    let hidden =
        if names.is_empty() { Vec::new() } else { tc.draw(gs::vecs(gs::sampled_from(names.clone())).max_size(2)) };
    let pinned = if names.is_empty() { Vec::new() } else { tc.draw(gs::vecs(gs::sampled_from(names)).max_size(3)) };
    let prefs = ProfilePrefs {
        sections: Some(Section::ALL.into_iter().collect()),
        hidden_artists: hidden_artists.clone(),
        hidden: hidden.clone(),
        pinned,
        ..Default::default()
    };
    let d = profile::document(&report(&tracks, &credits), &prefs);
    let is_hidden_artist = |a: &str| {
        let named: Vec<String> =
            profile::artist_credits(a).iter().chain([&a.to_string()]).map(|c| normalize(c)).collect();
        hidden_artists.iter().any(|h| named.contains(&normalize(h)))
    };
    for t in d.signature.iter().flatten() {
        assert!(!is_hidden_artist(&t.artist), "{:?} is named", t.artist);
        let name = normalize(&format!("{} - {}", t.artist, t.title));
        assert!(!hidden.iter().any(|h| normalize(h) == name), "{name:?} is hidden");
    }
    for a in d.artists.iter().flatten() {
        assert!(!is_hidden_artist(&a.name), "{:?} is named", a.name);
    }
}

/// Whatever the sections and rounding, a document built on sessions never counts them: no sets
/// section, no count in the headline, and no trait the sets section backs.
#[hegel::test(test_cases = 300)]
fn sessions_are_never_counted_in_public(tc: TestCase) {
    let mut r = report(&[], &[]);
    r.basis = Basis::Sessions;
    r.sets = SetsSection { count: tc.draw(gs::integers::<usize>().max_value(5000)), ..Default::default() };
    r.summary.tags = vec![Tag { id: "rarely_repeats", label: "Rarely repeats tracks".into(), why: String::new() }];
    let sections = tc.draw(gs::vecs(gs::sampled_from(Section::ALL.to_vec()).print_as_debug()).max_size(15));
    let precision = if tc.draw(gs::booleans()) { Precision::Exact } else { Precision::Rounded };
    let prefs = ProfilePrefs { sections: Some(sections.into_iter().collect()), precision, ..Default::default() };
    let d = profile::document(&r, &prefs);
    assert!(d.sets.is_none());
    assert!(d.tags.is_empty(), "{:?}", d.tags);
    let headline = d.headline.unwrap_or_default();
    assert!(!headline.contains("session") && !headline.contains("set"), "{headline:?}");
}

/// Shrunk counterexample: hiding "Bicep" didn't hide the signature track credited "Bicep;".
#[test]
fn a_hidden_artist_is_not_named_through_a_semicolon_credit() {
    let tracks = [("Bicep;".to_string(), "Glue".to_string()), ("Bicep (feat. Clara La San)".into(), "Saku".into())];
    let prefs = ProfilePrefs {
        sections: Some(Section::ALL.into_iter().collect()),
        hidden_artists: vec!["Bicep".into(), "Clara La San".into()],
        ..Default::default()
    };
    let d = profile::document(&report(&tracks, &[]), &prefs);
    assert_eq!(d.signature, None);
}

fn update() -> impl hegel::Generator<PrefsUpdate> {
    hegel::compose!(|tc| {
        let names = || {
            gs::vecs(gs::sampled_from(vec!["Bicep - Glue", "bicep -  glue", "Four Tet - Baby", " ", "X - Y"]))
                .max_size(2)
                .map(|v| v.into_iter().map(String::from).collect::<Vec<_>>())
        };
        // Undoing what isn't there fails the whole update, so undo rarely.
        let rarely = |v: Vec<String>| if tc.draw(gs::weighted_booleans(0.2)) { v } else { Vec::new() };
        let artists = || gs::vecs(gs::sampled_from(vec!["Bicep", "Four Tet"]).map(String::from)).max_size(1);
        let sections = || gs::vecs(gs::sampled_from(Section::ALL.to_vec()).print_as_debug()).max_size(3);
        PrefsUpdate {
            reset: tc.draw(gs::weighted_booleans(0.1)),
            sections: tc.draw(gs::optional(sections())),
            include: tc.draw(sections()),
            exclude: tc.draw(sections()),
            default_sections: tc.draw(gs::weighted_booleans(0.1)),
            precision: None,
            pin: tc.draw(names()),
            unpin: rarely(tc.draw(names())),
            hide: tc.draw(names()),
            unhide: rarely(tc.draw(names())),
            hide_artist: tc.draw(artists()),
            unhide_artist: rarely(tc.draw(artists())),
            alias: IndexMap::new(),
            unalias: Vec::new(),
            headline: tc.draw(gs::optional(gs::sampled_from(vec!["Warm-up DJ", " "]).map(String::from))),
        }
    })
}

/// However the preferences got there, a hidden track is never also pinned.
#[hegel::test(test_cases = 500)]
fn a_hidden_track_is_never_pinned(tc: TestCase) {
    let updates = tc.draw(gs::vecs(update().print_as_debug()).max_size(5));
    let mut prefs = ProfilePrefs::default();
    for u in &updates {
        let before = prefs.clone();
        if prefs.apply(u).is_err() {
            prefs = before; // callers drop a failed update
        }
        for p in &prefs.pinned {
            assert!(!prefs.is_hidden_track(p), "{p:?} is pinned and hidden: {prefs:?}");
        }
    }
}

/// Shrunk counterexample: a track hidden in one update could be pinned in the next.
#[test]
fn pinning_a_hidden_track_is_refused() {
    let mut prefs = ProfilePrefs::default();
    prefs.apply(&PrefsUpdate { hide: vec!["Bicep - Glue".into()], ..Default::default() }).unwrap();
    assert!(prefs.apply(&PrefsUpdate { pin: vec!["bicep -  glue".into()], ..Default::default() }).is_err());
    assert!(prefs.pinned.is_empty());
}

/// Hiding something and then unhiding it (a track, an artist), or pinning and unpinning a
/// track, leaves the preferences as they were; a reset gives the defaults.
#[hegel::test(test_cases = 500)]
fn changes_undo(tc: TestCase) {
    let mut prefs = ProfilePrefs::default();
    for u in tc.draw(gs::vecs(update().print_as_debug()).max_size(3)) {
        let _ = prefs.apply(&u);
    }
    let name = tc.draw(gs::sampled_from(vec!["Bicep - Glue", "Floorplan - Never Grow Old", "Four Tet - Baby"]));
    let start = prefs.clone();
    let undo = |done: PrefsUpdate, undo: PrefsUpdate| {
        let mut p = start.clone();
        p.apply(&done).unwrap();
        p.apply(&undo).unwrap();
        p
    };
    let s = || vec![name.to_string()];
    if !start.is_hidden_track(name) && !start.is_pinned(name) {
        assert_eq!(
            undo(PrefsUpdate { hide: s(), ..Default::default() }, PrefsUpdate { unhide: s(), ..Default::default() }),
            start
        );
        assert_eq!(
            undo(PrefsUpdate { pin: s(), ..Default::default() }, PrefsUpdate { unpin: s(), ..Default::default() }),
            start
        );
    }
    let artist = name.split(" - ").next().unwrap();
    if !start.hidden_artists.iter().any(|a| normalize(a) == normalize(artist)) {
        let a = || vec![artist.to_string()];
        let p = undo(
            PrefsUpdate { hide_artist: a(), ..Default::default() },
            PrefsUpdate { unhide_artist: a(), ..Default::default() },
        );
        assert_eq!(p, start);
    }
    let mut reset = start.clone();
    reset.apply(&PrefsUpdate { reset: true, ..Default::default() }).unwrap();
    assert_eq!(reset, ProfilePrefs::default());
}

/// Including and excluding sections adds and removes exactly those from what's published.
#[hegel::test(test_cases = 300)]
fn include_and_exclude_edit_the_published_sections(tc: TestCase) {
    let mut prefs = ProfilePrefs::default();
    for u in tc.draw(gs::vecs(update().print_as_debug()).max_size(2)) {
        let _ = prefs.apply(&u);
    }
    let sections = || gs::vecs(gs::sampled_from(Section::ALL.to_vec()).print_as_debug()).max_size(4);
    let (include, exclude) = (tc.draw(sections()), tc.draw(sections()));
    let mut want = prefs.published();
    want.extend(include.iter().copied());
    for x in &exclude {
        want.remove(x);
    }
    prefs.apply(&PrefsUpdate { include, exclude, ..Default::default() }).unwrap();
    assert_eq!(prefs.published(), want);
}

/// Every section reads back from its name, upper-cased or with dashes.
#[hegel::test(test_cases = 100)]
fn sections_read_back_from_their_names(tc: TestCase) {
    let s = tc.draw(gs::sampled_from(Section::ALL.to_vec()).print_as_debug());
    assert_eq!(Section::parse(s.name()), Some(s));
    assert_eq!(Section::parse(&format!(" {} ", s.name().to_uppercase().replace('_', "-"))), Some(s));
    assert_eq!(Section::parse(&s.to_string()), Some(s));
    let junk = tc.draw(gs::text().max_size(12));
    if let Some(x) = Section::parse(&junk) {
        assert!(Section::ALL.contains(&x));
    }
}

/// Genre text splits into distinct, non-empty genres, and reading the result again changes
/// nothing.
#[hegel::test(test_cases = 500)]
#[hegel::explicit_test_case(raw = "ßA".to_string())]
fn genres_are_distinct_and_stable(tc: TestCase) {
    let raw = tc.draw(hegel::one_of!(
        gs::text().max_size(20),
        gs::vecs(gs::sampled_from(vec![
            "deep house",
            "Tech-House",
            "DnB",
            "drum & bass",
            "UKG",
            "Unknown",
            "",
            " ",
            "edm"
        ]))
        .max_size(4)
        .map(|v| v.join(" / ")),
    ));
    let none = IndexMap::new();
    let genres = profile::genres(&raw, &none);
    let distinct: BTreeSet<String> = genres.iter().map(|g| normalize(g)).collect();
    assert_eq!(distinct.len(), genres.len(), "{genres:?}");
    assert!(genres.iter().all(|g| !g.trim().is_empty()), "{genres:?}");
    for g in &genres {
        tc.assume(!g.contains(['/', ',', ';', '|']));
        assert_eq!(profile::genres(g, &none), std::slice::from_ref(g), "{g:?}");
    }
}

/// The id of a track gone from the drive's library: a history list keeps its place.
const GONE: u32 = 999;

/// What a set (or session) is in the drive's tracks, by id: the tracks left, and where those gone
/// from the library were (the positions they came before).
fn present(ids: &[u32], lib: &FxHashMap<u32, Track>) -> (Vec<u32>, Vec<usize>) {
    let (mut tracks, mut missing_at) = (Vec::new(), Vec::new());
    for id in ids {
        match lib.contains_key(id) {
            true => tracks.push(*id),
            false => missing_at.push(tracks.len()),
        }
    }
    (tracks, missing_at)
}

/// Declares `sets` (each its tracks by id, from `lib`) in `store`, played on successive days.
fn declare(store: &Store, lib: &FxHashMap<u32, Track>, sets: &[Vec<u32>]) {
    let mut book = SetBook::load(store).unwrap();
    for (k, ids) in sets.iter().enumerate() {
        let date = NaiveDate::from_ymd_opt(2026, 1, 1).unwrap() + chrono::Duration::days(k as i64);
        let at = Utc::now();
        book.sets.push(Set {
            id: sets::new_id(date).unwrap(),
            title: Some(format!("Set {k}")),
            date,
            tracks: ids.iter().map(|id| SetEntry::from_track(&lib[id], EntryOrigin::Session, None)).collect(),
            created_at: at,
            updated_at: at,
            ..Default::default()
        });
    }
    book.save(store).unwrap();
}

/// The profile of a drive with random history lists (sound checks among them, and entries whose
/// track is gone), with or without sets declared in the store, counts what a direct reading of
/// the drive's tracks says: the declared sets, else the sessions long enough to stand in for
/// them; their plays, distinct tracks, BPMs, keys, transitions (not across a gone track or a
/// repeat) and every track played. The view's document is `profile::document` of its report,
/// and `profile_document` gives the same.
#[hegel::test(test_cases = 3)]
fn a_drives_profile_counts_what_its_sessions_and_sets_hold(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let root = common::bigger_drive(dir.path(), "Main");
    let lib: FxHashMap<u32, Track> =
        pdb::read_library(&root.join(paths::EXPORT_PDB)).unwrap().tracks.into_iter().map(|t| (t.id, t)).collect();
    let mut ids: Vec<u32> = lib.keys().copied().collect();
    ids.sort();
    let id = || hegel::one_of!(gs::sampled_from(ids.clone()), gs::just(GONE));
    let lists: Vec<Vec<u32>> = tc.draw(gs::vecs(gs::vecs(id()).max_size(8)).max_size(4));
    for (k, list) in lists.iter().enumerate() {
        common::add_history(&root, &format!("HISTORY {k:03}"), list);
    }
    let declared: Vec<Vec<u32>> = tc.draw(gs::vecs(gs::vecs(gs::sampled_from(ids.clone())).max_size(7)).max_size(3));
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    declare(&store, &lib, &declared);
    // Player settings: the drive's My Settings files, or the store's settings profile.
    let (on_drive, profile): (bool, Vec<(&str, &str)>) = (
        tc.draw(gs::booleans()),
        tc.draw(
            gs::vecs(gs::sampled_from(vec![
                ("quantize", "off"),
                ("quantize", "on"),
                ("tempo_range", "16"),
                ("sync", "nope"),
            ]))
            .max_size(2),
        ),
    );
    if on_drive {
        for f in ["MYSETTING.DAT", "MYSETTING2.DAT", "DJMMYSETTING.DAT", "DEVSETTING.DAT"] {
            std::fs::copy(
                common::fixtures().join("rekordcrate/demo_tracks/PIONEER").join(f),
                root.join("PIONEER").join(f),
            )
            .unwrap();
        }
    }
    let mut base = IndexMap::new();
    for (k, v) in &profile {
        base.entry(k.to_string()).or_insert(v.to_string());
    }
    store
        .save_settings_profile(&locallibrary::settings::SettingsProfile { base: base.clone(), ..Default::default() })
        .unwrap();
    // A drive whose database can't be read is left out, saying so.
    let broken = tc.draw(gs::weighted_booleans(0.3)).then(|| {
        let b = dir.path().join("Broken");
        std::fs::create_dir_all(b.join("PIONEER/rekordbox")).unwrap();
        std::fs::write(b.join(paths::EXPORT_PDB), b"not a database").unwrap();
        common::path(&b)
    });
    let min = tc.draw(gs::integers::<usize>().min_value(2).max_value(5));
    let req = ProfileRequest {
        drives: std::iter::once(common::path(&root)).chain(broken.clone()).collect(),
        min_set_tracks: Some(min),
        top: Some(tc.draw(gs::integers::<usize>().min_value(1).max_value(5))),
        no_analysis: tc.draw(gs::booleans()),
        ..Default::default()
    };
    let v = api::profile(Some(&store), &req, &Ctx::silent()).unwrap();
    let r = &v.report;

    // The sets the model counts.
    let played: Vec<(Vec<u32>, Vec<usize>)> = match declared.is_empty() {
        false => declared.iter().map(|s| (s.clone(), Vec::new())).collect(),
        true => lists.iter().map(|l| present(l, &lib)).filter(|(t, _)| t.len() >= min).collect(),
    };
    let too_short =
        if declared.is_empty() { lists.iter().filter(|l| present(l, &lib).0.len() < min).count() } else { 0 };
    let plays: usize = played.iter().map(|p| p.0.len()).sum();
    let basis = match (plays, declared.is_empty()) {
        (0, _) => profile::Basis::Collection,
        (_, false) => profile::Basis::Plays,
        (_, true) => profile::Basis::Sessions,
    };
    assert_eq!(r.basis, basis);
    let distinct: FxHashSet<u32> = played.iter().flat_map(|p| p.0.iter().copied()).collect();
    assert_eq!((r.sets.count, r.sets.too_short), (played.len(), too_short));
    assert_eq!((r.sets.plays, r.sets.distinct_tracks), (if plays == 0 { 0 } else { plays }, distinct.len()));
    assert_eq!(r.sets.not_archived, if declared.is_empty() { played.len() } else { 0 });
    assert_eq!(r.collection.tracks, lib.len());

    let pool: Vec<&Track> = match plays {
        0 => lib.values().collect(),
        _ => played.iter().flat_map(|p| p.0.iter().map(|id| &lib[id])).collect(),
    };
    let bpm = |t: &Track| (40.0..=300.0).contains(&t.bpm);
    assert_eq!(r.tempo.tracks, pool.iter().filter(|t| bpm(t)).count());
    assert_eq!(r.keys.tracks, pool.iter().filter(|t| Camelot::parse(&t.key).is_some()).count());
    let (mut transitions, mut keyed) = (0, 0);
    let mut per: FxHashMap<String, usize> = FxHashMap::default();
    for (tracks, missing_at) in &played {
        for k in 1..tracks.len() {
            let (x, y) = (&lib[&tracks[k - 1]], &lib[&tracks[k]]);
            if x.id == y.id || missing_at.contains(&k) {
                continue;
            }
            transitions += 1;
            keyed += usize::from(Camelot::parse(&x.key).is_some() && Camelot::parse(&y.key).is_some());
        }
        for id in tracks.iter().collect::<FxHashSet<_>>() {
            *per.entry(lib[id].display_name()).or_default() += 1;
        }
    }
    assert_eq!((r.mixing.transitions, r.mixing.keyed), (transitions, keyed));
    let got: FxHashMap<String, usize> = r.signature.played.iter().map(|p| (p.name.clone(), p.sets)).collect();
    assert_eq!(got, per);
    if let Some(e) = &r.energy {
        assert!(!req.no_analysis && e.tracks <= pool.len());
    }
    assert!(r.prep.is_none() || !req.no_analysis);
    assert_eq!(broken.is_some(), r.warnings.iter().any(|w| w.starts_with("Broken: ")), "{:?}", r.warnings);
    assert_eq!(r.sources.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(), ["Main"]);

    // The style: the store's profile when it sets a known value, else the drive's files.
    let known: Vec<(&str, &str)> = locallibrary::profile::STYLE_SETTINGS
        .iter()
        .filter_map(|k| {
            base.get(*k)
                .filter(|v| locallibrary::settings::find(k).and_then(|d| d.choice(v)).is_some())
                .map(|v| (*k, v.as_str()))
        })
        .collect();
    let style = r
        .style
        .as_ref()
        .map(|s| (s.source.as_str(), s.values.iter().map(|v| (v.key.as_str(), v.value.as_str())).collect::<Vec<_>>()));
    match (known.is_empty(), on_drive) {
        (false, _) => assert_eq!(style, Some(("settings profile", known.clone()))),
        (true, true) => {
            assert!(style.as_ref().is_some_and(|(source, values)| *source == "Main" && !values.is_empty()), "{style:?}")
        }
        (true, false) => assert_eq!(style, None),
    }
    let tagged = r.summary.tags.iter().any(|t| t.id == "quantize_off");
    assert_eq!(
        tagged,
        r.style.as_ref().is_some_and(|s| s.values.iter().any(|v| v.key == "quantize" && v.value == "off"))
    );

    // The view's document is the report's, as the preferences allow; asking for it alone gives
    // the same (but for when it was made).
    let doc = |d: &profile::ProfileDocument| {
        let mut v = serde_json::to_value(d).unwrap();
        v.as_object_mut().unwrap().remove("generatedAt");
        v
    };
    assert_eq!(doc(&v.document), doc(&profile::document(r, &v.prefs)));
    assert_eq!(v.published, v.prefs.published().into_iter().collect::<Vec<_>>());
    if tc.draw(gs::weighted_booleans(0.3)) {
        assert_eq!(doc(&api::profile_document(Some(&store), &req, &Ctx::silent()).unwrap()), doc(&v.document));
    }
}

/// Sessions a pull archived in the store and those still on the drive are counted once each,
/// whether the drive still has them or rekordbox took them off; the drive's tracks and the
/// store's snapshot of them are one collection, and naming the library reads the same.
#[hegel::test(test_cases = 2)]
fn archived_and_mounted_sessions_count_once_each(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let root = common::bigger_drive(dir.path(), "Main");
    let ids: Vec<u32> = (1..=12).collect();
    let list = || gs::vecs(gs::sampled_from(ids.clone())).min_size(2).max_size(6);
    let archived: Vec<Vec<u32>> = tc.draw(gs::vecs(list()).min_size(1).max_size(3));
    for (k, l) in archived.iter().enumerate() {
        common::add_history(&root, &format!("HISTORY {k:03}"), l);
    }
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    let mut d = Device::open(&root).unwrap();
    let opts = PullOptions { library: Some("Club".into()), ..Default::default() };
    sync::pull(&store, &mut d, &opts, &Ctx::silent()).unwrap();
    // rekordbox imports the history (the lists leave the drive), or it stays.
    let taken_off = tc.draw(gs::booleans());
    let pdb_path = root.join(paths::EXPORT_PDB);
    if taken_off {
        let history: Vec<u32> = pdb::read_library(&pdb_path).unwrap().history.iter().map(|h| h.id).collect();
        let mut ed = rekordbox_pdb::PdbEditor::from_file(&pdb_path).unwrap();
        ed.remove_history_playlists(&history).unwrap();
        ed.save(&pdb_path).unwrap();
    }
    let fresh: Vec<Vec<u32>> = tc.draw(gs::vecs(list()).max_size(2));
    for (k, l) in fresh.iter().enumerate() {
        common::add_history(&root, &format!("HISTORY {:03}", k + 10), l);
    }
    let named = tc.draw(gs::booleans());
    let req = ProfileRequest {
        drives: vec![common::path(&root)],
        libraries: if named { vec!["club".into()] } else { Vec::new() },
        min_set_tracks: Some(2),
        no_analysis: tc.draw(gs::booleans()),
        ..Default::default()
    };
    let r = api::profile(Some(&store), &req, &Ctx::silent()).unwrap().report;
    assert_eq!(r.basis, profile::Basis::Sessions);
    assert_eq!((r.sets.count, r.sets.not_archived), (archived.len() + fresh.len(), fresh.len()));
    assert_eq!(r.sets.plays, archived.iter().chain(&fresh).map(Vec::len).sum::<usize>());
    assert_eq!(r.sets.oldest.is_some(), !archived.is_empty());
    assert_eq!(r.collection.tracks, 12, "the drive's tracks and the snapshot's are one collection");
    assert_eq!(r.sources.iter().map(|s| s.kind).collect::<Vec<_>>(), ["store", "drive"]);
    assert_eq!(r.sources[0].history_lists, archived.len());
    // Newest first: the sessions still only on the drive.
    assert!(r.sets.list.iter().take(fresh.len()).all(|s| s.date.is_none()), "{:?}", r.sets.list);
}

/// A day reads back from each of its forms: `2026-09-30` is that day, `2026-09` its month's
/// first day to last, `2026` its year's; anything else, or a year before 1901, is refused.
#[hegel::test(test_cases = 300)]
fn days_read_back_from_their_forms(tc: TestCase) {
    let year = tc.draw(gs::integers::<i32>().min_value(1850).max_value(2200));
    let month = tc.draw(gs::integers::<u32>().min_value(1).max_value(12));
    let day = tc.draw(gs::integers::<u32>().min_value(1).max_value(31));
    let valid = year > 1900;
    let first = NaiveDate::from_ymd_opt(year, month, 1).unwrap();
    let next = first.checked_add_months(chrono::Months::new(1)).unwrap();
    let ok = |s: &str, end: bool| api::parse_day(s, end).ok();
    assert_eq!(ok(&format!("{year}-{month:02}"), false), valid.then_some(first));
    assert_eq!(ok(&format!("{year}-{month}"), true), valid.then(|| next.pred_opt().unwrap()));
    assert_eq!(ok(&format!(" {year} "), false), NaiveDate::from_ymd_opt(year, 1, 1).filter(|_| valid));
    assert_eq!(ok(&year.to_string(), true), NaiveDate::from_ymd_opt(year, 12, 31).filter(|_| valid));
    let date = NaiveDate::from_ymd_opt(year, month, day).filter(|_| valid);
    for end in [false, true] {
        assert_eq!(ok(&format!("{year}-{month:02}-{day:02}"), end), date);
    }
    assert_eq!(ok(&format!("{year}-{}", month + 12), false), None);
    assert_eq!(ok(&format!("{year}-{month}-{day}-1"), false), None);
    let junk = tc.draw(gs::text().max_size(6));
    if let Some(d) = ok(&junk, false) {
        assert!(d.year() > 1900 && junk.trim().starts_with(|c: char| c.is_ascii_digit()), "{junk:?}");
    }
}

/// A request's range must run forwards: a start after the end is refused, naming both, before
/// anything is read; one that runs forwards (or names one end) is taken.
#[hegel::test(test_cases = 40)]
fn a_range_must_run_forwards(tc: TestCase) {
    let day = || gs::integers::<i64>().min_value(0).max_value(3000);
    let (a, b) = (tc.draw(day()), tc.draw(day()));
    let at = |n: i64| (NaiveDate::from_ymd_opt(2020, 1, 1).unwrap() + chrono::Duration::days(n)).to_string();
    let dir = tempfile::tempdir().unwrap();
    // Nothing to read: a range that's taken fails for that instead.
    let req = ProfileRequest {
        drives: vec![common::path(&dir.path().join("nowhere"))],
        since: Some(at(a)),
        until: Some(at(b)),
        ..Default::default()
    };
    let err = api::profile(None, &req, &Ctx::silent()).unwrap_err();
    assert_eq!(err.to_string().contains("is after the end"), a > b, "{err}");
}

/// Preferences changed through the API are saved whole: what an update returns is what loads
/// back, and is what applying the update directly gives; an update that fails saves nothing.
#[hegel::test(test_cases = 10)]
fn saved_preferences_load_back(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    let mut model = ProfilePrefs::default();
    for u in tc.draw(gs::vecs(update().print_as_debug()).max_size(4)) {
        let mut u = u;
        u.precision =
            tc.draw(gs::optional(gs::sampled_from(vec![Precision::Exact, Precision::Rounded]).print_as_debug()));
        let aliases = gs::vecs(gs::sampled_from(vec![
            ("dnb", "Jungle"),
            ("DNB ", "Drum & Bass"),
            ("techno", "Techno"),
            (" ", "x"),
        ]));
        u.alias = tc.draw(aliases.max_size(2)).into_iter().map(|(a, b)| (a.to_string(), b.to_string())).collect();
        u.unalias = tc.draw(gs::vecs(gs::sampled_from(vec!["dnb".to_string(), "house".to_string()])).max_size(1));
        let mut want = model.clone();
        let applied = want.apply(&u);
        match api::profile_prefs_update(&store, &u) {
            Ok(saved) => {
                assert!(applied.is_ok());
                assert_eq!(saved, want);
                model = want;
            }
            Err(e) => assert!(applied.is_err(), "{e}"),
        }
        assert_eq!(api::profile_prefs(&store).unwrap(), model);
        assert_eq!(ProfilePrefs::load(&store).unwrap(), model);
    }
    // Aliases are kept sorted, one per spelling.
    let keys: Vec<String> = model.genre_aliases.keys().map(|k| normalize(k)).collect();
    assert!(keys.windows(2).all(|w| w[0] != w[1]), "{keys:?}");
}

/// Every section has a title and says what publishing it tells; without declared sets, the sets
/// and their shape are titled as sessions, and the opt-in sections are exactly those naming
/// tracks, artists or labels, or describing the collection.
#[hegel::test(test_cases = 50)]
fn every_section_says_what_it_reveals(tc: TestCase) {
    let s = tc.draw(gs::sampled_from(Section::ALL.to_vec()).print_as_debug());
    assert!(!s.title().is_empty() && !s.reveals().is_empty());
    assert_eq!(s.title_in(Basis::Plays), s.title());
    assert_eq!(s.title_in(Basis::Collection), s.title());
    let renamed = s.title_in(Basis::Sessions) != s.title();
    assert_eq!(renamed, matches!(s, Section::Sets | Section::Arc));
    assert!(!renamed || s.title_in(Basis::Sessions).starts_with("Session"));
    let opt_in =
        matches!(s, Section::Labels | Section::Artists | Section::Signature | Section::Prep | Section::Collection);
    assert_eq!(s.published_by_default(), !opt_in);
    assert_eq!(Basis::Sessions.publishes(s), s != Section::Sets);
    assert!(profile::tag_section("nope").is_none());
}

/// Without drives named, the profile reads the mounted drives that hold an export: those the
/// store's registry knows once it knows any, else all of them. A drive named must hold an
/// export, and with no drive and no store (or a store with no library) there's nothing to read.
#[hegel::test(test_cases = 4)]
fn the_drives_read_are_the_mounted_ones_the_registry_knows(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let volumes = common::isolate(dir.path(), None);
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    // Each drive: whether it holds an export, and whether the registry knows it.
    let drives: Vec<(bool, bool)> = tc.draw(gs::vecs(gs::tuples!(gs::booleans(), gs::booleans())).max_size(3));
    for (i, &(export, known)) in drives.iter().enumerate() {
        let name = format!("D{i}");
        let root = match export {
            true => common::bigger_drive(&volumes, &name),
            false => common::blank(&volumes, &name),
        };
        if known {
            common::adopt(Some(&store), &root, &name);
        }
    }
    let read = |store: Option<&Store>| {
        let req = ProfileRequest { no_analysis: true, ..Default::default() };
        api::profile(store, &req, &Ctx::silent()).map(|v| {
            let mut names: Vec<String> =
                v.report.sources.iter().filter(|s| s.kind == "drive").map(|s| s.name.clone()).collect();
            names.sort();
            names
        })
    };
    let registry = drives.iter().any(|d| d.1);
    let want: Vec<String> =
        (0..drives.len()).filter(|&i| drives[i].0 && (!registry || drives[i].1)).map(|i| format!("D{i}")).collect();
    match read(Some(&store)) {
        Ok(names) => assert_eq!(names, want),
        Err(e) => assert!(want.is_empty() && e.to_string().contains("nothing to build a profile from"), "{e}"),
    }
    let all: Vec<String> = (0..drives.len()).filter(|&i| drives[i].0).map(|i| format!("D{i}")).collect();
    match read(None) {
        Ok(names) => assert_eq!(names, all, "without a store, every mounted drive"),
        Err(e) => assert!(all.is_empty() && e.reason() == Some(locallibrary::error::Reason::NoStore), "{e}"),
    }
    if let Some(i) = drives.iter().position(|d| !d.0) {
        let req = ProfileRequest { drives: vec![common::path(&volumes.join(format!("D{i}")))], ..Default::default() };
        let err = api::profile(Some(&store), &req, &Ctx::silent()).unwrap_err();
        assert!(err.to_string().contains("has no rekordbox export"), "{err}");
    }
    locallibrary::testing::volumes_on_this_thread(None);
}

/// A blank name to hide (a track or an artist), a genre alias missing either side, and taking
/// back what was never set (a hidden track or artist, an alias) are each refused: blanks as
/// invalid, the rest as not found. Whatever was there before stays.
#[hegel::test(test_cases = 300)]
fn blank_and_unknown_preference_changes_are_refused(tc: TestCase) {
    let mut prefs = ProfilePrefs::default();
    for u in tc.draw(gs::vecs(update().print_as_debug()).max_size(3)) {
        let before = prefs.clone();
        if prefs.apply(&u).is_err() {
            prefs = before;
        }
    }
    let blank = tc.draw(gs::sampled_from(vec!["", " ", "\t "]));
    let unknown = tc.draw(gs::sampled_from(vec!["Nobody - Nothing", "No One", "zouk"]));
    let field = tc.draw(gs::integers::<u8>().max_value(6));
    let s = |v: &str| vec![v.to_string()];
    let (u, invalid) = match field {
        0 => (PrefsUpdate { hide: s(blank), ..Default::default() }, true),
        1 => (PrefsUpdate { hide_artist: s(blank), ..Default::default() }, true),
        2 => (
            PrefsUpdate { alias: IndexMap::from([(blank.to_string(), "House".to_string())]), ..Default::default() },
            true,
        ),
        3 => (
            PrefsUpdate { alias: IndexMap::from([("deep".to_string(), blank.to_string())]), ..Default::default() },
            true,
        ),
        4 => (PrefsUpdate { unhide: s(unknown), ..Default::default() }, false),
        5 => (PrefsUpdate { unhide_artist: s(unknown), ..Default::default() }, false),
        _ => (PrefsUpdate { unalias: s(unknown), ..Default::default() }, false),
    };
    let unknown_there = match field {
        4 => prefs.is_hidden_track(unknown),
        5 => prefs.hidden_artists.iter().any(|a| normalize(a) == normalize(unknown)),
        6 => prefs.genre_aliases.keys().any(|k| normalize(k) == normalize(unknown)),
        _ => false,
    };
    let before = prefs.clone();
    match (prefs.apply(&u), invalid) {
        (Err(locallibrary::Error::Invalid(_)), true) => {}
        (Err(locallibrary::Error::NotFound(_)), false) => {}
        (Ok(()), false) if unknown_there => return,
        (got, _) => panic!("{u:?}: {got:?}"),
    }
    assert_eq!(prefs, before, "nothing changed");
}

/// The DJ's own spellings win: a genre whose text means one of their aliases (ignoring case and
/// spacing, as genres are compared) reads as the first such alias's genre, wherever it is in
/// the text; every other genre reads as it does without aliases, words that aren't genres stay
/// out, and no genre is named twice.
#[hegel::test(test_cases = 300)]
fn the_djs_own_genre_spellings_win(tc: TestCase) {
    let parts: Vec<&str> = tc.draw(
        gs::vecs(gs::sampled_from(vec![
            "deep house",
            "Tech-House",
            "DnB",
            "minimal",
            " MINIMAL ",
            " Deep  House ",
            "Unknown",
            "",
            "techno",
        ]))
        .min_size(1)
        .max_size(5),
    );
    let mut raw = parts[0].to_string();
    for p in &parts[1..] {
        raw.push(tc.draw(gs::sampled_from(vec!['/', ',', ';', '|'])));
        raw.push_str(p);
    }
    let aliases: IndexMap<String, String> = tc
        .draw(
            gs::vecs(gs::tuples!(
                gs::sampled_from(vec!["minimal", "Deep House", "DEEP HOUSE", "dnb", "techno", "unknown"]),
                gs::sampled_from(vec!["Minimal / Deep Tech", "House", "Jungle", "Techno (Peak)"])
            ))
            .max_size(4),
        )
        .into_iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    let none = IndexMap::new();
    let mut want: Vec<String> = Vec::new();
    for p in &parts {
        // A part that names no genre without aliases (`Unknown`, nothing) names none with them.
        let Some(plain) = profile::genres(p, &none).into_iter().next() else { continue };
        let name = aliases.iter().find(|(k, _)| normalize(k) == normalize(p)).map_or(plain, |(_, v)| v.clone());
        if !want.iter().any(|g| normalize(g) == normalize(&name)) {
            want.push(name);
        }
    }
    assert_eq!(profile::genres(&raw, &aliases), want, "{raw:?} {aliases:?}");
}

/// The collection's cue statistics count each track's cues as its analysis files hold them,
/// whatever cues the DJ set in hot cue slots A to C or as memory cues: the share of tracks with
/// a hot cue, a memory cue and a loop, and hot cues per track among those with any.
#[hegel::test(test_cases = 10, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn cue_statistics_count_the_cues_the_analysis_files_hold(tc: TestCase) {
    cue_statistics_after_cue_edits(&tc, 2);
}

/// The same for hot cues in slots D to H, which a `.EXT` file holds alone.
#[hegel::test(test_cases = 10, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn cue_statistics_count_hot_cues_past_c(tc: TestCase) {
    cue_statistics_after_cue_edits(&tc, 7);
}

/// Set cues (hot cues up to slot `last`, memory cues, loops) on the demo drive's tracks, and
/// check the profile's cue statistics against the cues the drive lists for each track.
#[hegel::test_helper]
fn cue_statistics_after_cue_edits(tc: &TestCase, last: u8) {
    use locallibrary::cue_edit::{self, CueEdit, CuePosition, HotCue, SetCue};
    let dir = tempfile::tempdir().unwrap();
    let root = common::demo_drive(dir.path(), "Main");
    let lib = Device::open_export(&root).unwrap().library().unwrap();
    for _ in 0..tc.draw(gs::integers::<usize>().max_value(4)) {
        let t = &lib.tracks[tc.draw(gs::integers::<usize>().max_value(lib.tracks.len() - 1))];
        let edit = CueEdit::Set(SetCue {
            hot_cue: tc.draw(gs::optional(gs::integers::<u8>().max_value(last))).map(HotCue),
            at: CuePosition::Ms(tc.draw(gs::integers::<u32>().max_value(t.duration_secs * 1000 / 2))),
            snap: false,
            loop_beats: tc.draw(gs::optional(gs::sampled_from(vec![4.0, 0.5]))),
            name: None,
            color: None,
        });
        let mut d = Device::open_export(&root).unwrap();
        if let Ok(plan) = cue_edit::plan(&d, &[t.id], &edit, &Ctx::silent()) {
            cue_edit::apply(&plan, &mut d, &Ctx::silent()).unwrap();
        }
    }
    let d = Device::open_export(&root).unwrap();
    let ids: Vec<u32> = lib.tracks.iter().map(|t| t.id).collect();
    let counts: Vec<(usize, usize, usize)> = cue_edit::list(&d, &ids)
        .unwrap()
        .iter()
        .map(|t| {
            let hot = t.cues.iter().filter(|c| c.hot_cue.is_some()).count();
            let loops = t.cues.iter().filter(|c| c.end_ms.is_some()).count();
            let memory = t.cues.iter().filter(|c| c.hot_cue.is_none() && c.end_ms.is_none()).count();
            (hot, memory, loops)
        })
        .collect();
    let req = ProfileRequest { drives: vec![common::path(&root)], ..Default::default() };
    let r = api::profile(None, &req, &Ctx::silent()).unwrap().report;
    let prep = r.prep.expect("the demo drive's tracks are analysed");
    let n = counts.len();
    let share = |f: &dyn Fn(&(usize, usize, usize)) -> bool| counts.iter().filter(|c| f(c)).count() as f64 / n as f64;
    assert_eq!(prep.tracks, n);
    assert_eq!(prep.hot_cue_share, share(&|c| c.0 > 0), "{counts:?}");
    assert_eq!(prep.memory_cue_share, share(&|c| c.1 > 0), "{counts:?}");
    assert_eq!(prep.loop_share, share(&|c| c.2 > 0), "{counts:?}");
    let with_hot: Vec<usize> = counts.iter().map(|c| c.0).filter(|h| *h > 0).collect();
    let per = (!with_hot.is_empty()).then(|| with_hot.iter().sum::<usize>() as f64 / with_hot.len() as f64);
    assert_eq!(prep.hot_cues_per_track, per);
}
