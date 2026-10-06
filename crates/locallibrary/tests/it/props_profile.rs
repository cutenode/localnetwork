//! Properties of the profile's privacy controls and genre and artist parsing (`profile`).

use std::collections::{BTreeMap, BTreeSet};

use chrono::Utc;
use hegel::generators as gs;
use hegel::{Generator, TestCase};
use locallibrary::model::normalize;
use locallibrary::profile::{
    self, ArcSection, Basis, CollectionSection, EraSection, KeysSection, MixingSection, PlayedTrack, PrefsUpdate,
    ProfileOptions, ProfilePrefs, ProfileReport, RankedSection, Section, SetsSection, Share, SignatureSection, Summary,
    TempoSection,
};

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
        assert!(!is_hidden_artist(a), "{a:?} is named");
    }
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
            alias: BTreeMap::new(),
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
    let none = BTreeMap::new();
    let genres = profile::genres(&raw, &none);
    let distinct: BTreeSet<String> = genres.iter().map(|g| normalize(g)).collect();
    assert_eq!(distinct.len(), genres.len(), "{genres:?}");
    assert!(genres.iter().all(|g| !g.trim().is_empty()), "{genres:?}");
    for g in &genres {
        tc.assume(!g.contains(['/', ',', ';', '|']));
        assert_eq!(profile::genres(g, &none), std::slice::from_ref(g), "{g:?}");
    }
}
