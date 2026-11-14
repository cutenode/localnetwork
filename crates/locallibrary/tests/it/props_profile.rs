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
    for credit in locallibrary::model::artist_credits(&a) {
        let prefs = ProfilePrefs { hidden_artists: vec![credit.clone()], ..Default::default() };
        assert!(prefs.is_hidden_artist(&a), "hiding {credit:?} doesn't hide {a:?}");
        assert!(!prefs.may_name("whatever", &a));
    }
}

/// Credits are trimmed, non-empty and distinct.
#[hegel::test(test_cases = 500)]
fn credits_are_distinct_names(tc: TestCase) {
    let a = tc.draw(artist());
    let credits = locallibrary::model::artist_credits(&a);
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
        cached: Default::default(),
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
    let credits: Vec<String> = tracks.iter().flat_map(|(a, _)| locallibrary::model::artist_credits(a)).collect();
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
            locallibrary::model::artist_credits(a).iter().chain([&a.to_string()]).map(|c| normalize(c)).collect();
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
    let sections = tc.draw(gs::vecs(gs::sampled_from(Section::ALL.to_vec())).max_size(15));
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

fn update() -> impl hegel::PrintableGenerator<PrefsUpdate> {
    hegel::compose!(|tc| {
        let names = || {
            gs::vecs(gs::sampled_from(vec!["Bicep - Glue", "bicep -  glue", "Four Tet - Baby", " ", "X - Y"]))
                .max_size(2)
                .map(|v| v.into_iter().map(String::from).collect::<Vec<_>>())
        };
        // Undoing what isn't there fails the whole update, so undo rarely.
        let rarely = |v: Vec<String>| if tc.draw(gs::weighted_booleans(0.2)) { v } else { Vec::new() };
        let artists = || gs::vecs(gs::sampled_from(vec!["Bicep", "Four Tet"]).map(String::from)).max_size(1);
        // Asking for a section that's never published fails the whole update, so ask for those
        // only now and then.
        let sections = || {
            let pool: Vec<Section> = if tc.draw(gs::weighted_booleans(0.1)) {
                Section::ALL.to_vec()
            } else {
                Section::ALL.into_iter().filter(|s| s.publishable()).collect()
            };
            gs::vecs(gs::sampled_from(pool)).max_size(3)
        };
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
    let updates = tc.draw(gs::vecs(update()).max_size(5));
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
    for u in tc.draw(gs::vecs(update()).max_size(3)) {
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
/// Including one that can't be published is refused and changes nothing; published sections are
/// only ever publishable ones.
#[hegel::test(test_cases = 300)]
fn include_and_exclude_edit_the_published_sections(tc: TestCase) {
    let mut prefs = ProfilePrefs::default();
    for u in tc.draw(gs::vecs(update()).max_size(2)) {
        let _ = prefs.apply(&u);
    }
    let sections = || gs::vecs(gs::sampled_from(Section::ALL.to_vec())).max_size(4);
    let (include, exclude) = (tc.draw(sections()), tc.draw(sections()));
    let before = prefs.clone();
    let mut want = prefs.published();
    want.extend(include.iter().copied());
    for x in &exclude {
        want.remove(x);
    }
    let refused = include.iter().any(|s| !s.publishable());
    let r = prefs.apply(&PrefsUpdate { include, exclude, ..Default::default() });
    if refused {
        assert!(r.is_err());
        assert_eq!(prefs, before);
    } else {
        r.unwrap();
        assert_eq!(prefs.published(), want);
    }
    assert!(prefs.published().iter().all(|s| s.publishable()));
}

/// Every section reads back from its name, upper-cased or with dashes.
#[hegel::test(test_cases = 100)]
fn sections_read_back_from_their_names(tc: TestCase) {
    let s = tc.draw(gs::sampled_from(Section::ALL.to_vec()));
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
    let req = ProfileRequest {
        drives: vec![common::path(&root)],
        min_set_tracks: Some(2),
        no_analysis: tc.draw(gs::booleans()),
        ..Default::default()
    };
    let r = api::profile(Some(&store), &req, &Ctx::silent()).unwrap().report;
    // Named (in any case), the library reads the same; a library the store hasn't isn't found.
    let library = tc.draw(gs::sampled_from(vec!["club", "Club", "CLUB"]));
    let named = ProfileRequest { libraries: vec![library.into()], ..req.clone() };
    let by_name = api::profile(Some(&store), &named, &Ctx::silent()).unwrap().report;
    assert_eq!(serde_json::to_value(&by_name.sets).unwrap(), serde_json::to_value(&r.sets).unwrap());
    let nowhere = ProfileRequest { libraries: vec!["Nowhere".into()], ..req.clone() };
    let e = api::profile(Some(&store), &nowhere, &Ctx::silent()).unwrap_err();
    assert_eq!((e.code(), e.to_string().contains("library \"Nowhere\"")), ("not_found", true), "{e}");
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
    assert_eq!(ok(&format!("{year}-{}", month + 12), true), None);
    assert_eq!(ok(&format!("{year}-m{month}"), true), None);
    assert_eq!(ok(&format!("{year}-m{month}"), false), None);
    assert_eq!(ok(&format!("{year}-{month}-d{day}"), false), None);
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
    for u in tc.draw(gs::vecs(update()).max_size(4)) {
        let mut u = u;
        u.precision = tc.draw(gs::optional(gs::sampled_from(vec![Precision::Exact, Precision::Rounded])));
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
/// and their shape are titled as sessions. Only sets and what describes the collection can be
/// published, and of those, artists and the collection's size are opt-in.
#[hegel::test(test_cases = 50)]
fn every_section_says_what_it_reveals(tc: TestCase) {
    let s = tc.draw(gs::sampled_from(Section::ALL.to_vec()));
    assert!(!s.title().is_empty() && !s.reveals().is_empty());
    assert_eq!(s.title_in(Basis::Plays), s.title());
    assert_eq!(s.title_in(Basis::Collection), s.title());
    let renamed = s.title_in(Basis::Sessions) != s.title();
    assert_eq!(renamed, matches!(s, Section::Sets | Section::Arc));
    assert!(!renamed || s.title_in(Basis::Sessions).starts_with("Session"));
    let publishable = matches!(
        s,
        Section::Sets | Section::Energy | Section::Genres | Section::Artists | Section::Era | Section::Collection
    );
    assert_eq!(s.publishable(), publishable);
    let opt_in = matches!(s, Section::Artists | Section::Collection);
    assert_eq!(s.published_by_default(), publishable && !opt_in);
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
    for u in tc.draw(gs::vecs(update()).max_size(3)) {
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
        let t = &tc.draw(gs::sampled_from(&lib.tracks));
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

/// A report as JSON, but for when it was built.
fn said(r: &ProfileReport) -> serde_json::Value {
    let mut v = serde_json::to_value(r).unwrap();
    v.as_object_mut().unwrap().remove("generated");
    v
}

/// A context whose warnings land in `warnings`, and that cancels once `cancel_after` events
/// have gone by.
fn recording<'a>(
    warnings: &'a std::sync::Mutex<Vec<String>>,
    cancel_after: Option<usize>,
) -> (impl Fn(&locallibrary::progress::Event) + Send + Sync + 'a, locallibrary::progress::CancelToken) {
    let cancel = locallibrary::progress::CancelToken::new();
    let token = cancel.clone();
    let seen = std::sync::atomic::AtomicUsize::new(0);
    let progress = move |e: &locallibrary::progress::Event| {
        if let locallibrary::progress::Event::Warning { message } = e {
            warnings.lock().unwrap().push(message.clone());
        }
        if cancel_after.is_some_and(|n| seen.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1 >= n) {
            token.cancel();
        }
    };
    (progress, cancel)
}

/// Pulls `drives` drives, each with its own random history lists of at least two tracks, into
/// one store as libraries `Club 0`, `Club 1`...; returns the store and how many lists it holds.
#[hegel::test_helper]
fn pulled_store(tc: &TestCase, dir: &std::path::Path, drives: usize) -> (Store, usize) {
    let store = Store::open_or_init(dir.join("store")).unwrap();
    let mut lists = 0;
    for k in 0..drives {
        // The demo drive's tracks have analysis files.
        let root = common::demo_drive(dir, &format!("Drive {k}"));
        let ids: Vec<u32> =
            pdb::read_library(&root.join(paths::EXPORT_PDB)).unwrap().tracks.iter().map(|t| t.id).collect();
        let drawn: Vec<Vec<u32>> =
            tc.draw(gs::vecs(gs::vecs(gs::sampled_from(ids)).min_size(2).max_size(5)).min_size(2).max_size(3));
        for (j, l) in drawn.iter().enumerate() {
            common::add_history(&root, &format!("HISTORY {k}{j:02}"), l);
        }
        lists += drawn.len();
        let mut d = Device::open(&root).unwrap();
        let opts = PullOptions { library: Some(format!("Club {k}")), force: true, ..Default::default() };
        sync::pull(&store, &mut d, &opts, &Ctx::silent()).unwrap();
    }
    (store, lists)
}

/// A profile of the store alone reads its libraries' sessions and their tracks' analysis files
/// from the snapshots, and keeps what it read: built again, it reads none of the files and says
/// the same. When that can't be kept it warns (a few times, then sums up), and the next profile
/// reads every file again.
#[hegel::test(test_cases = 3, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn prop_a_stores_profile_reads_its_snapshots_analysis_once(tc: TestCase) {
    // Both ways each time: whether the cache can be kept is no draw a few cases can miss.
    for unkept in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let libraries = tc.draw(gs::integers::<usize>().min_value(1).max_value(2));
        let (store, lists) = pulled_store(&tc, dir.path(), libraries);
        // A library nothing was ever pulled into has nothing to read, but counts as one.
        let empty = tc.draw(gs::booleans());
        if empty {
            store.create_library("Empty").unwrap();
        }
        if unkept {
            // The analysis cache's folder is a file.
            std::fs::create_dir_all(store.root().join("cache")).unwrap();
            std::fs::write(store.root().join("cache/profile"), b"").unwrap();
        }
        let sources = profile::Sources { drives: Vec::new(), store: Some(&store), libraries: None };
        let options = ProfileOptions { min_set_tracks: 2, ..ProfileOptions::default() };
        let build = || {
            let warnings = std::sync::Mutex::new(Vec::new());
            let r = {
                let (progress, cancel) = recording(&warnings, None);
                profile::build(&sources, &options, &Ctx::new(&progress, cancel)).unwrap()
            };
            (r, warnings.into_inner().unwrap())
        };
        let (cold, warned) = build();
        assert_eq!(cold.basis, Basis::Sessions);
        assert_eq!(cold.sets.count, lists);
        let names: Vec<(&str, &str, usize)> =
            cold.sources.iter().map(|s| (s.kind, s.name.as_str(), s.history_lists)).collect();
        let name = match libraries + usize::from(empty) {
            1 => "store (1 library)",
            2 => "store (2 libraries)",
            _ => "store (3 libraries)",
        };
        assert_eq!(names, [("store", name, lists)]);
        let (kept, read) = cold.cached.analysis;
        assert!(kept == 0 && read > 0 && cold.prep.is_some(), "{:?}", cold.cached);
        let uncached = warned.iter().filter(|w| w.starts_with("couldn't cache")).count();
        assert_eq!(uncached, if unkept { read.min(3) } else { 0 }, "{warned:?}");
        assert_eq!(warned.iter().any(|w| w.contains("more like that")), unkept && read > 3, "{warned:?}");

        let (warm, _) = build();
        assert_eq!(warm.cached.analysis, if unkept { (0, read) } else { (read, read) });
        assert_eq!(said(&warm), said(&cold));
    }
}

/// How a test leaves a track's analysis files on a drive.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Analysis {
    Kept,
    Gone,
    /// A folder where each file was.
    Unreadable,
    /// A link to a file outside the drive.
    Outside,
}

/// A drive's tracks' cue statistics count the tracks whose analysis files are on the drive and
/// readable, and none whose files are gone, can't be read or lie outside the drive. A drive whose
/// exportLibrary.db can't be read is described from export.pdb, saying so, and one whose
/// export.pdb can't be read is left out, saying so.
#[hegel::test(test_cases = 10, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn prop_a_drives_analysis_is_read_only_where_it_can_be(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let root = common::demo_drive(dir.path(), "Main");
    let outside = dir.path().join("elsewhere.DAT");
    std::fs::write(&outside, b"PMAI").unwrap();
    let lib = Device::open_export(&root).unwrap().library().unwrap();
    let mut ways = vec![Analysis::Kept, Analysis::Gone, Analysis::Unreadable];
    if cfg!(unix) {
        ways.push(Analysis::Outside);
    }
    let mut kept = 0;
    for t in lib.tracks.iter().filter(|t| !t.analyze_path.is_empty()) {
        let way = tc.draw(gs::sampled_from(ways.clone()));
        kept += usize::from(way == Analysis::Kept);
        let dat = paths::to_fs(&root, &paths::key(&t.analyze_path));
        for f in [dat.clone(), dat.with_extension("EXT")] {
            if way == Analysis::Kept || !f.exists() {
                continue;
            }
            std::fs::remove_file(&f).unwrap();
            match way {
                Analysis::Unreadable => std::fs::create_dir(&f).unwrap(),
                #[cfg(unix)]
                Analysis::Outside => std::os::unix::fs::symlink(&outside, &f).unwrap(),
                _ => {}
            }
        }
    }
    let broken_plus = tc.draw(gs::booleans());
    if broken_plus {
        std::fs::write(root.join(paths::LIBRARY_PLUS_DB), b"not a database").unwrap();
    }
    // A drive beside it whose database can't be read is left out, saying so.
    let broken = tc.draw(gs::booleans()).then(|| {
        let b = dir.path().join("Broken");
        std::fs::create_dir_all(b.join("PIONEER/rekordbox")).unwrap();
        std::fs::write(b.join(paths::EXPORT_PDB), b"not a database").unwrap();
        common::path(&b)
    });
    let req = ProfileRequest {
        drives: std::iter::once(common::path(&root)).chain(broken.clone()).collect(),
        ..Default::default()
    };
    let r = api::profile(None, &req, &Ctx::silent()).unwrap().report;
    assert_eq!(broken.is_some(), r.warnings.iter().any(|w| w.starts_with("Broken: ")), "{:?}", r.warnings);
    assert_eq!(r.prep.map_or(0, |p| p.tracks), kept);
    assert_eq!(r.collection.tracks, lib.tracks.len());
    let plus_warning = r.warnings.iter().any(|w| w.starts_with("Main: ") && w.ends_with("only export.pdb was read"));
    assert_eq!(plus_warning, broken_plus, "{:?}", r.warnings);
}

/// Declared sets count when they were played within the range asked for, newest first; with
/// none in range, the collection is described.
#[hegel::test(test_cases = 20)]
fn prop_declared_sets_count_when_played_in_the_range(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let root = common::bigger_drive(dir.path(), "Main");
    let lib: FxHashMap<u32, Track> =
        pdb::read_library(&root.join(paths::EXPORT_PDB)).unwrap().tracks.into_iter().map(|t| (t.id, t)).collect();
    let ids: Vec<u32> = (1..=12).collect();
    let declared: Vec<Vec<u32>> =
        tc.draw(gs::vecs(gs::vecs(gs::sampled_from(ids)).min_size(1).max_size(4)).min_size(1).max_size(5));
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    declare(&store, &lib, &declared);
    // `declare` plays set k on day k of 2026.
    let day = |k: i64| NaiveDate::from_ymd_opt(2026, 1, 1).unwrap() + chrono::Duration::days(k);
    let edge = || gs::optional(gs::integers::<i64>().min_value(-1).max_value(5));
    let (since, until) = (tc.draw(edge()).map(day), tc.draw(edge()).map(day));
    let options = ProfileOptions { since, until, analysis: false, ..ProfileOptions::default() };
    let sources = profile::Sources { drives: vec![Device::open(&root).unwrap()], store: Some(&store), libraries: None };
    let r = profile::build(&sources, &options, &Ctx::silent()).unwrap();
    let mut want: Vec<NaiveDate> = (0..declared.len() as i64)
        .map(day)
        .filter(|d| since.is_none_or(|s| *d >= s) && until.is_none_or(|u| *d <= u))
        .collect();
    want.reverse();
    assert_eq!(
        r.sets.list.iter().map(|s| s.date).collect::<Vec<_>>(),
        want.iter().map(|d| Some(*d)).collect::<Vec<_>>()
    );
    assert_eq!(r.basis, if want.is_empty() { Basis::Collection } else { Basis::Plays });
}

/// A profile cancelled after any number of its progress events stops with
/// [`locallibrary::Error::Cancelled`] (at once, when cancelled before it starts), or, cancelled
/// once there's nothing left to check, says what an uncancelled one does.
#[hegel::test(test_cases = 3, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn prop_a_cancelled_profile_stops_or_says_it_all(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let (store, _) = pulled_store(&tc, dir.path(), 1);
    let drive = common::bigger_drive(dir.path(), "Mounted");
    let sources =
        profile::Sources { drives: vec![Device::open(&drive).unwrap()], store: Some(&store), libraries: None };
    let options = ProfileOptions { min_set_tracks: 2, ..ProfileOptions::default() };
    let build = |cancel_after: Option<usize>| {
        let warnings = std::sync::Mutex::new(Vec::new());
        let (progress, cancel) = recording(&warnings, cancel_after);
        if cancel_after == Some(0) {
            cancel.cancel();
        }
        profile::build(&sources, &options, &Ctx::new(&progress, cancel))
    };
    let whole = build(None).unwrap();
    let events = std::sync::atomic::AtomicUsize::new(0);
    let counting = |_: &locallibrary::progress::Event| {
        events.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    };
    profile::build(&sources, &options, &Ctx::new(&counting, locallibrary::progress::CancelToken::new())).unwrap();
    let n = events.into_inner();
    for at in 0..=n {
        match build(Some(at)) {
            Ok(r) => assert!(at > 0 && said(&r) == said(&whole), "cancelled after {at} of {n} events"),
            Err(e) => assert!(matches!(e, locallibrary::Error::Cancelled), "{e}"),
        }
    }
}

/// A library whose latest snapshot's export.pdb is gone from the store is left out of the
/// profile, saying so, and the others are read as before.
#[hegel::test(test_cases = 3, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn prop_a_library_whose_databases_are_gone_is_left_out_saying_so(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let (store, _) = pulled_store(&tc, dir.path(), 2);
    let libraries = store.libraries().unwrap();
    let gone = &libraries[tc.draw(gs::integers::<usize>().max_value(libraries.len() - 1))];
    let snap = store.load_snapshot(&gone.head.unwrap()).unwrap();
    let pdb = snap.files.iter().find(|f| f.path == paths::EXPORT_PDB).and_then(|f| f.hash).unwrap();
    std::fs::remove_file(store.object_path(&pdb)).unwrap();
    let sources = profile::Sources { drives: Vec::new(), store: Some(&store), libraries: None };
    let options = ProfileOptions { analysis: false, ..ProfileOptions::default() };
    let r = profile::build(&sources, &options, &Ctx::silent()).unwrap();
    let left_out = format!("library {}: ", gone.name);
    assert!(r.warnings.iter().any(|w| w.starts_with(&left_out)), "{:?}", r.warnings);
    // Both libraries hold the same recordings: the tracks read are one library's.
    assert_eq!(r.sources[0].tracks, r.collection.tracks, "the other library's tracks alone");
}

/// Where in the store a profile reads from can't be read (its list of libraries, a library's
/// latest snapshot, or the declared sets), the profile stops with that error rather than
/// describe part of the DJ's playing.
#[hegel::test(test_cases = 6, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn prop_a_store_that_cant_be_read_stops_the_profile(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let (store, _) = pulled_store(&tc, dir.path(), 1);
    let head = store.libraries().unwrap()[0].head.unwrap().to_hex();
    let broken = tc.draw(gs::sampled_from(vec!["libraries", "snapshot", "sets"]));
    match broken {
        "libraries" => std::fs::write(store.root().join("libraries.json"), b"{").unwrap(),
        "snapshot" => std::fs::remove_file(store.root().join(format!("snapshots/{head}.json"))).unwrap(),
        _ => std::fs::write(store.root().join(sets::SETS_FILE), b"[").unwrap(),
    }
    let sources = profile::Sources { drives: Vec::new(), store: Some(&store), libraries: None };
    let options = ProfileOptions { analysis: false, ..ProfileOptions::default() };
    let err = profile::build(&sources, &options, &Ctx::silent()).expect_err("no profile");
    let missing = matches!(&err, locallibrary::Error::NotFound(m) if m.starts_with("snapshot "));
    assert_eq!(missing, broken == "snapshot", "{broken}: {err}");
}

/// The style a store's settings profile gives names the player settings that describe how a DJ
/// plays and are set to a value the player knows, in display order; with none, there's no style
/// (a store whose only library nothing was pulled into still makes a profile).
#[hegel::test(test_cases = 50)]
fn prop_the_style_names_the_known_settings_the_store_sets(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    store.create_library("Empty").unwrap();
    let keys: Vec<&str> = profile::STYLE_SETTINGS.iter().copied().chain(["waveform_color"]).collect();
    let set = gs::tuples!(gs::sampled_from(keys), gs::sampled_from(vec!["on", "off", "16", "wide", "nope"]));
    let mut base = IndexMap::new();
    for (k, v) in tc.draw(gs::vecs(set).max_size(5)) {
        base.insert(k.to_string(), v.to_string());
    }
    store
        .save_settings_profile(&locallibrary::settings::SettingsProfile { base: base.clone(), ..Default::default() })
        .unwrap();
    let sources = profile::Sources { drives: Vec::new(), store: Some(&store), libraries: None };
    let r = profile::build(&sources, &ProfileOptions::default(), &Ctx::silent()).unwrap();
    assert_eq!(r.sources.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(), ["store (1 library)"]);
    let known: Vec<(&str, &str)> = profile::STYLE_SETTINGS
        .iter()
        .filter_map(|k| {
            let v = base.get(*k)?;
            locallibrary::settings::find(k)?.choice(v)?;
            Some((*k, v.as_str()))
        })
        .collect();
    let style = r.style.as_ref().map(|s| {
        let values: Vec<(&str, &str)> = s.values.iter().map(|v| (v.key.as_str(), v.value.as_str())).collect();
        (s.source.as_str(), values)
    });
    assert_eq!(style, (!known.is_empty()).then_some(("settings profile", known.clone())), "{base:?}");
}
