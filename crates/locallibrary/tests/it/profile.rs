mod sources;

use crate::common;

use indexmap::IndexMap;
use std::path::Path;

use locallibrary::api::{self, ProfileRequest, ProfileView};
use locallibrary::device::Device;
use locallibrary::paths;
use locallibrary::profile::{self, Basis, Count, Precision, PrefsUpdate, ProfilePrefs, Section};
use locallibrary::progress::Ctx;
use locallibrary::sets::{self, EntryOrigin, Set, SetBook, SetEntry};
use locallibrary::store::Store;
use locallibrary::sync::{self, PullOptions};
use rekordbox_pdb::PdbEditor;

/// bigger-export: twelve [a]pendics.shuffle tracks, 124-128 BPM, Techno and Electronic.
/// Tracks 1 ("It's In The Eyes") and 2 ("In Charge") follow each other in both sets.
fn club_drive(dir: &Path, name: &str) -> std::path::PathBuf {
    let root = common::bigger_drive(dir, name);
    common::add_history(&root, "HISTORY 001", &[1, 2, 3, 4, 5, 6]);
    common::add_history(&root, "HISTORY 002", &[9, 10, 1, 2, 12, 7]);
    // Two tracks: a sound check, not a set.
    common::add_history(&root, "HISTORY 003", &[3, 4]);
    root
}

fn request(drives: &[&Path]) -> ProfileRequest {
    ProfileRequest { drives: drives.iter().map(|d| d.display().to_string()).collect(), ..Default::default() }
}

fn build(store: Option<&Store>, req: &ProfileRequest) -> ProfileView {
    api::profile(store, req, &Ctx::silent()).unwrap()
}

/// The drive's tracks `ids`, in order, as a set's entries.
fn entries(root: &Path, ids: &[u32]) -> Vec<SetEntry> {
    let lib = locallibrary::pdb::read_library(&root.join(paths::EXPORT_PDB)).unwrap();
    ids.iter()
        .map(|id| SetEntry::from_track(lib.tracks.iter().find(|t| t.id == *id).unwrap(), EntryOrigin::Session, None))
        .collect()
}

/// Declares sets in `store`, each a title, the day it was played and its tracks, at Fabric.
fn declare(store: &Store, sets: Vec<(&str, &str, Vec<SetEntry>)>) -> SetBook {
    let mut book = SetBook::load(store).unwrap();
    for (title, date, tracks) in sets {
        let date = chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d").unwrap();
        let at = chrono::Utc::now();
        book.sets.push(Set {
            id: sets::new_id(date).unwrap(),
            title: Some(title.into()),
            date,
            venue: Some("Fabric".into()),
            city: Some("London".into()),
            tracks,
            created_at: at,
            updated_at: at,
            ..Default::default()
        });
    }
    book.save(store).unwrap();
    book
}

/// Without declared sets, the history lists players recorded stand in for them, and say so.
#[test]
fn sessions_on_a_drive_describe_how_it_was_played() {
    let dir = tempfile::tempdir().unwrap();
    let root = club_drive(dir.path(), "Main");
    let v = build(None, &request(&[&root]));
    let r = &v.report;

    assert_eq!(r.basis, Basis::Sessions);
    assert_eq!(r.sets.count, 2);
    assert_eq!(r.sets.too_short, 1);
    assert_eq!(r.sets.plays, 12);
    assert_eq!(r.sets.distinct_tracks, 10);
    assert_eq!(r.sets.not_archived, 2);
    assert_eq!(r.sources.len(), 1);
    assert_eq!(r.sources[0].history_lists, 3);

    let core = r.tempo.core.unwrap();
    let (lo, hi) = (core.low, core.high);
    assert!((123.0..=125.0).contains(&lo) && (127.0..=128.0).contains(&hi), "{lo}-{hi}");
    assert_eq!(r.tempo.tracks, 12);
    assert_eq!(r.tempo.histogram.iter().map(|b| b.count).sum::<usize>(), 12);

    assert_eq!(r.mixing.transitions, 10);
    assert_eq!(r.mixing.keyed, 10);
    assert_eq!(
        r.mixing.same_key + r.mixing.adjacent + r.mixing.relative + r.mixing.energy_boost + r.mixing.clash,
        r.mixing.keyed
    );
    assert_eq!(r.keys.tracks, 12);
    assert_eq!(r.keys.wheel.len(), 24);
    assert_eq!(r.keys.wheel.iter().map(|k| k.count).sum::<usize>(), 12);

    assert_eq!(r.genres.top[0].name, "Techno");
    assert_eq!(r.genres.top[1].name, "Electronic");
    assert_eq!(r.artists.top[0].name, "[a]pendics.shuffle");
    assert_eq!(r.artists.top[0].count, 12, "spellings of one artist count together");
    assert_eq!(r.era.median_year, Some(2006));
    assert_eq!(r.era.recent_share, Some(0.0));

    let names: Vec<&str> = r.signature.tracks.iter().map(|t| t.title.as_str()).collect();
    assert_eq!(names, ["A1 - It's In The Eyes", "A2 - In Charge"]);
    assert_eq!(r.signature.transitions.len(), 1);
    assert_eq!(r.signature.transitions[0].sets, 2);
    assert_eq!(r.signature.played.len(), 10);
    assert!(r.summary.headline.contains("Techno"), "{}", r.summary.headline);
    assert!(r.summary.headline.contains("2 sessions"), "{}", r.summary.headline);

    // The document leaves out what names tracks unless asked, and never counts sessions.
    let doc = serde_json::to_value(&v.document).unwrap();
    assert!(doc.get("signature").is_none());
    assert!(doc.get("artists").is_none());
    assert_eq!(doc["basis"], "sessions");
    assert!(doc.get("sets").is_none(), "{doc}");
    let headline = doc["headline"].as_str().unwrap();
    assert!(headline.contains("Techno") && !headline.contains("session") && !headline.contains("set"), "{headline}");
    assert_eq!(doc["genres"][0]["name"], "Techno");
    assert!(doc["tempo"]["low"].as_f64().unwrap().fract() == 0.0, "rounded to whole BPM");
    let text = doc.to_string();
    assert!(!text.contains("HISTORY"), "history list names stay private");
    assert!(!text.contains("Contents/"), "paths stay private");
}

#[test]
fn preferences_choose_what_the_document_shows() {
    let dir = tempfile::tempdir().unwrap();
    let root = club_drive(dir.path(), "Main");
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    let req = ProfileRequest { no_store: true, ..request(&[&root]) };

    let update = PrefsUpdate {
        include: vec![Section::Signature, Section::Artists],
        exclude: vec![Section::Style, Section::Keys],
        pin: vec!["[a]pendics.shuffle - Deluxe Cynic".into()],
        hide: vec!["[a]pendics.shuffle - A1 - It's In The Eyes".into()],
        alias: IndexMap::from([("electronic".into(), "Electronica".into())]),
        headline: Some("Techno warm-ups".into()),
        precision: Some(locallibrary::profile::Precision::Exact),
        ..Default::default()
    };
    api::profile_prefs_update(&store, &update).unwrap();
    assert_eq!(api::profile_prefs(&store).unwrap().pinned.len(), 1);

    let v = build(Some(&store), &req);
    assert!(v.published.contains(&Section::Signature));
    assert!(!v.published.contains(&Section::Keys));
    let d = &v.document;
    assert_eq!(d.headline.as_deref(), Some("Techno warm-ups"));
    assert!(d.keys.is_none());
    let sig = d.signature.as_ref().unwrap();
    let titles: Vec<&str> = sig.iter().map(|t| t.title.as_str()).collect();
    // Pinned first; the hidden track never appears, though it still counts.
    assert_eq!(titles, ["Deluxe Cynic", "A2 - In Charge"]);
    assert_eq!(sig[0].artist, "[a]pendics.shuffle");
    assert_eq!(v.report.signature.tracks.len(), 2);
    assert_eq!(v.report.genres.top[1].name, "Electronica");
    assert!(d.sets.is_none(), "sessions aren't sets");

    // Hiding the artist hides every track by them, and the artist list.
    api::profile_prefs_update(
        &store,
        &PrefsUpdate { hide_artist: vec!["[A]pendics.Shuffle".into()], ..Default::default() },
    )
    .unwrap();
    let v = build(Some(&store), &req);
    assert!(v.document.signature.is_none());
    assert!(v.document.artists.is_none());

    // Invalid changes save nothing.
    let before = api::profile_prefs(&store).unwrap();
    assert!(
        api::profile_prefs_update(&store, &PrefsUpdate { unpin: vec!["nothing".into()], ..Default::default() })
            .is_err()
    );
    assert_eq!(api::profile_prefs(&store).unwrap(), before);
}

/// A track one of the DJ's sets marks unnamed (an unreleased dubplate, say) is never named in
/// public, even pinned, as if the profile hid it; it still counts.
#[test]
fn a_track_a_set_marks_unnamed_is_never_named() {
    let dir = tempfile::tempdir().unwrap();
    let root = club_drive(dir.path(), "Main");
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    let mut friday = entries(&root, &[1, 2, 3, 4, 5, 6]);
    friday[0].unnamed = true;
    declare(
        &store,
        vec![("Friday", "2026-09-12", friday), ("Warm-up", "2026-10-03", entries(&root, &[9, 10, 1, 2, 12, 7]))],
    );
    let pin = PrefsUpdate {
        include: vec![Section::Signature],
        pin: vec!["[a]pendics.shuffle - A1 - It's In The Eyes".into()],
        ..Default::default()
    };
    api::profile_prefs_update(&store, &pin).unwrap();

    let v = build(Some(&store), &request(&[&root]));
    let titles: Vec<&str> = v.document.signature.iter().flatten().map(|t| t.title.as_str()).collect();
    assert_eq!(titles, ["A2 - In Charge"]);
    let unnamed: Vec<&str> = v.report.signature.tracks.iter().filter(|t| t.unnamed).map(|t| t.title.as_str()).collect();
    assert_eq!(unnamed, ["A1 - It's In The Eyes"], "it still counts, in the report");
}

#[test]
fn the_summary_says_no_more_than_the_sections_in_the_document() {
    let dir = tempfile::tempdir().unwrap();
    let root = club_drive(dir.path(), "Main");
    let mut report = build(None, &request(&[&root])).report;
    // As if the DJ had declared those sessions as sets.
    report.basis = Basis::Plays;
    // Traits a bigger library would earn, one backed by each kind of section.
    let traits = [
        ("harmonic", "Mixes in key"),
        ("focused", "Mostly Techno"),
        ("signature", "Has signature tracks"),
        ("prepared", "Cues up every track"),
        ("quantize_off", "Plays with quantize off"),
    ];
    report.summary.tags =
        traits.iter().map(|(id, label)| profile::Tag { id, label: label.to_string(), why: String::new() }).collect();
    let with = |sections: &[Section]| ProfilePrefs {
        sections: Some(sections.iter().copied().collect()),
        ..Default::default()
    };

    let all = profile::document(&report, &with(&Section::ALL));
    let headline = all.headline.unwrap();
    assert!(headline.contains("Techno") && headline.contains("BPM") && headline.contains("2 sets"), "{headline}");
    assert_eq!(all.tags.len(), traits.len());

    // By default the opt-in sections stay out, and so do the traits they back.
    let defaults = profile::document(&report, &ProfilePrefs::default());
    assert_eq!(defaults.tags, ["Mixes in key", "Mostly Techno", "Plays with quantize off"]);

    // Without genres, tempo and sets, the generated headline names none of them.
    let mixing = profile::document(&report, &with(&[Section::Summary, Section::Mixing]));
    assert_eq!(mixing.headline, None);
    assert_eq!(mixing.tags, ["Mixes in key"]);
    let tempo = profile::document(&report, &with(&[Section::Summary, Section::Tempo]));
    let headline = tempo.headline.unwrap();
    assert!(headline.ends_with("BPM") && !headline.contains("Techno") && !headline.contains("set"), "{headline}");
    assert!(tempo.tags.is_empty());
    // The DJ's own headline is theirs to write.
    let own = ProfilePrefs { headline: Some("Warm-up specialist".into()), ..with(&[Section::Summary]) };
    assert_eq!(profile::document(&report, &own).headline.as_deref(), Some("Warm-up specialist"));

    // Counts in the headline are rounded like the document's.
    report.sets.count = 48;
    let sets = profile::document(&report, &with(&[Section::Summary, Section::Sets]));
    assert_eq!(sets.headline.as_deref(), Some("45+ sets"));
    let exact = ProfilePrefs { precision: Precision::Exact, ..with(&[Section::Summary, Section::Sets]) };
    assert_eq!(profile::document(&report, &exact).headline.as_deref(), Some("48 sets"));
}

#[test]
#[cfg(feature = "account")]
fn the_published_profile_is_the_document_in_whole_numbers() {
    let dir = tempfile::tempdir().unwrap();
    let root = club_drive(dir.path(), "Main");
    // Signed out, publishing can still be planned: the record, with nowhere to write it yet.
    let plan = api::profile_publish_plan(&dir.path().join("store"), &request(&[&root]), &Ctx::silent()).unwrap();
    assert!(plan.account.is_none() && plan.uri.is_none() && plan.missing_scope.is_none());
    let r = &plan.record;
    assert_eq!(r["$type"], "computer.localnetwork.dj.profile");
    locallibrary::atmosphere::lexicon::validate(locallibrary::atmosphere::lexicon::PROFILE, r).unwrap();
    // No sets declared: sessions are counted in the numbers, never as sets.
    assert_eq!(r["basis"], "sessions");
    assert!(r.get("sets").is_none());
    assert_eq!(r["genres"][0]["name"], "Techno");
    assert!(r["genres"][0]["share"].is_i64() && r["tempo"]["low"].is_i64());
    assert!(r.get("signature").is_none() && r.get("artists").is_none(), "opt-in sections stay out");
    let text = r.to_string();
    assert!(!text.contains("HISTORY") && !text.contains("Contents/"));
    let err = api::profile_publish_apply(&dir.path().join("store"), &request(&[&root]), &Ctx::silent()).unwrap_err();
    assert_eq!(err.code(), "account");
}

#[test]
fn archived_sessions_keep_counting_after_rekordbox_takes_them_off_the_drive() {
    let dir = tempfile::tempdir().unwrap();
    let root = club_drive(dir.path(), "Main");
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    let mut d = Device::open(&root).unwrap();
    let opts = PullOptions { library: Some("Club library".into()), ..Default::default() };
    sync::pull(&store, &mut d, &opts, &Ctx::silent()).unwrap();

    // rekordbox imports the history (the lists leave the drive), then a new club is played.
    let pdb = root.join(paths::EXPORT_PDB);
    let ids: Vec<u32> = locallibrary::pdb::read_library(&pdb).unwrap().history.iter().map(|h| h.id).collect();
    let mut ed = PdbEditor::from_file(&pdb).unwrap();
    ed.remove_history_playlists(&ids).unwrap();
    ed.save(&pdb).unwrap();
    common::add_history(&root, "HISTORY 001", &[6, 7, 8, 9, 10]);

    let v = build(Some(&store), &request(&[&root]));
    let r = &v.report;
    assert_eq!(r.sets.count, 3);
    assert_eq!(r.sets.not_archived, 1);
    assert!(r.sets.oldest.is_some());
    // Newest first: the one still on the drive.
    assert!(r.sets.list[0].date.is_none());
    assert_eq!(r.sources.iter().map(|s| s.kind).collect::<Vec<_>>(), ["store", "drive"]);
    // The same tracks from the drive and the store's snapshot are one collection.
    assert_eq!(r.collection.tracks, 12);

    // Without the store, only the drive's list is left.
    let v = build(Some(&store), &ProfileRequest { no_store: true, ..request(&[&root]) });
    assert_eq!(v.report.sets.count, 1);

    // Dates: archived sets were first seen today; sets still on a drive are newer than any date.
    let tomorrow = (chrono::Utc::now() + chrono::Duration::days(1)).format("%Y-%m-%d").to_string();
    let v = build(Some(&store), &ProfileRequest { since: Some(tomorrow.clone()), ..request(&[&root]) });
    assert_eq!(v.report.sets.count, 1);
    let v = build(Some(&store), &ProfileRequest { until: Some(tomorrow), ..request(&[&root]) });
    assert_eq!(v.report.sets.count, 2);
    assert_eq!(v.report.sets.not_archived, 0);
    assert!(v.report.sets.oldest.is_some());
}

/// Shares of one whole add up as published (84/8/8% isn't 85/10/10%), and the set shape is the
/// one they show: 24 of 49 sets building is published as half, so the sets build.
#[test]
fn published_shares_add_up() {
    let dir = tempfile::tempdir().unwrap();
    let root = club_drive(dir.path(), "Main");
    let mut report = build(None, &request(&[&root])).report;
    report.arc.sets = 49;
    (report.arc.builds, report.arc.steady, report.arc.winds_down) = (24.0 / 49.0, 15.0 / 49.0, 10.0 / 49.0);
    report.energy = Some(profile::EnergySection { tracks: 25, high: 0.84, mid: 0.08, low: 0.08, curve: Vec::new() });
    let doc = serde_json::to_value(profile::document(&report, &ProfilePrefs::default())).unwrap();
    let sum = |v: &serde_json::Value, keys: &[&str]| keys.iter().map(|k| v[k].as_f64().unwrap()).sum::<f64>();
    assert!((sum(&doc["energy"], &["high", "mid", "low"]) - 1.0).abs() < 1e-9, "{}", doc["energy"]);
    assert!((sum(&doc["arc"], &["builds", "steady", "windsDown"]) - 1.0).abs() < 1e-9, "{}", doc["arc"]);
    assert_eq!((&doc["arc"]["builds"], &doc["arc"]["shape"]), (&serde_json::json!(0.5), &serde_json::json!("builds")));
}

/// A transition is two tracks played one after the other: not two either side of a track that's
/// gone from the library, nor one track logged twice (reloaded, or on both decks).
#[test]
fn transitions_skip_gaps_and_repeats() {
    let dir = tempfile::tempdir().unwrap();
    let root = common::bigger_drive(dir.path(), "Main");
    common::add_history(&root, "HISTORY 001", &[1, 999, 2, 3, 3, 4, 5]);
    let r = build(None, &request(&[&root])).report;
    assert_eq!(r.sets.count, 1);
    // 2→3, 3→4 and 4→5: not 1→2 across the missing entry, nor 3→3.
    assert_eq!(r.mixing.transitions, 3);
}

#[test]
fn a_set_on_two_drives_counts_once() {
    let dir = tempfile::tempdir().unwrap();
    let main = club_drive(dir.path(), "Main");
    let backup = dir.path().join("Backup");
    common::copy_dir(&main, &backup);
    let v = build(None, &request(&[&main, &backup]));
    assert_eq!(v.report.sets.count, 2);
    assert_eq!(v.report.sets.plays, 12);
    assert_eq!(v.report.collection.tracks, 12);
    assert_eq!(v.report.sources.len(), 2);
}

#[test]
fn a_library_without_sets_is_described_from_its_collection() {
    let dir = tempfile::tempdir().unwrap();
    let root = common::demo_drive(dir.path(), "Demo");
    let v = build(None, &request(&[&root]));
    let r = &v.report;
    assert_eq!(r.basis, Basis::Collection);
    assert_eq!(r.sets.count, 0);
    assert!(r.tempo.median.is_some());
    assert_eq!(r.collection.tracks, 2);
    // Real analysis files: cues were read.
    let prep = r.prep.as_ref().expect("cue statistics");
    assert_eq!(prep.tracks, 2);
    assert!(r.summary.headline.contains("2 tracks"), "{}", r.summary.headline);
    assert!(v.document.sets.is_none());

    // Without analysis, no cue statistics.
    let v = build(None, &ProfileRequest { no_analysis: true, ..request(&[&root]) });
    assert!(v.report.prep.is_none());
    assert!(v.report.energy.is_none());
}

#[test]
fn bad_requests_explain_themselves() {
    let dir = tempfile::tempdir().unwrap();
    let root = club_drive(dir.path(), "Main");
    let err =
        api::profile(None, &ProfileRequest { since: Some("yesterday".into()), ..request(&[&root]) }, &Ctx::silent())
            .unwrap_err();
    assert_eq!(err.code(), "invalid");
    let err = api::profile(
        None,
        &ProfileRequest { since: Some("2026-09".into()), until: Some("2026-01".into()), ..request(&[&root]) },
        &Ctx::silent(),
    )
    .unwrap_err();
    assert!(err.to_string().contains("after"), "{err}");
}

/// Once the DJ declares sets, they are what the profile counts: a 12-track practice session on
/// the drive doesn't count, however long, and `--min-tracks` is only for sessions.
#[test]
fn declared_sets_are_the_sets() {
    let dir = tempfile::tempdir().unwrap();
    let root = club_drive(dir.path(), "Main");
    common::add_history(&root, "HISTORY 004", &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12]);
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    // Without sets, the practice session counts as much as anything else.
    let v = build(Some(&store), &request(&[&root]));
    assert_eq!((v.report.basis, v.report.sets.count), (Basis::Sessions, 3));

    let book = declare(
        &store,
        vec![
            ("Friday", "2026-09-12", entries(&root, &[1, 2, 3, 4, 5, 6])),
            ("Warm-up", "2026-10-03", entries(&root, &[9, 10, 1, 2, 12, 7])),
        ],
    );
    let v = build(Some(&store), &ProfileRequest { min_set_tracks: Some(7), ..request(&[&root]) });
    let r = &v.report;
    assert_eq!(r.basis, Basis::Plays);
    assert_eq!((r.sets.count, r.sets.plays, r.sets.distinct_tracks), (2, 12, 10));
    assert_eq!((r.sets.too_short, r.sets.not_archived), (0, 0));
    let day = |s: &str| chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").ok();
    assert_eq!((r.sets.oldest, r.sets.newest), (day("2026-09-12"), day("2026-10-03")));
    // Newest first, as the DJ named them.
    let newest = &r.sets.list[0];
    assert_eq!(
        (newest.name.as_str(), newest.place.as_str(), newest.date),
        ("Warm-up", "Fabric, London", day("2026-10-03"))
    );
    assert_eq!(newest.id, book.find("Warm-up").unwrap().id);
    // Transitions are consecutive entries: 1 into 2 opens one set and sits in the other.
    assert_eq!(r.mixing.transitions, 10);
    assert_eq!(r.signature.transitions.len(), 1);
    assert_eq!(r.signature.transitions[0].sets, 2);
    assert!(r.summary.headline.ends_with("2 sets"), "{}", r.summary.headline);

    let doc = serde_json::to_value(&v.document).unwrap();
    assert_eq!(doc["basis"], "plays");
    assert_eq!(doc["sets"], serde_json::json!({ "count": 2, "since": 2026, "medianTracks": 6 }));
}

/// A set's entries are the library's tracks, found by file whatever the entry kept, so their
/// analysis counts; an entry the library doesn't have still counts, from what it kept.
#[test]
fn set_entries_are_found_in_the_library_by_file() {
    let dir = tempfile::tempdir().unwrap();
    let root = common::demo_drive(dir.path(), "Demo");
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    let mut tracks = entries(&root, &[1, 2]);
    (tracks[0].title, tracks[0].bpm) = ("Renamed since".into(), 0.0);
    let mut dubplate = SetEntry::added("Unknown Artist - Dubplate");
    (dubplate.bpm, dubplate.genre) = (140.0, "Jungle".into());
    tracks.push(dubplate);
    declare(&store, vec![("Listening party", "2026-09-20", tracks)]);

    let r = build(Some(&store), &request(&[&root])).report;
    assert_eq!((r.basis, r.sets.count, r.sets.plays), (Basis::Plays, 1, 3));
    let played: Vec<&str> = r.signature.played.iter().map(|t| t.name.as_str()).collect();
    assert!(played.contains(&"Loopmasters - Demo Track 1"), "{played:?}");
    assert!(played.contains(&"Unknown Artist - Dubplate"), "{played:?}");
    // The library's BPMs (128 and 120), and the dubplate's own.
    assert_eq!((r.tempo.tracks, r.tempo.min, r.tempo.max), (3, Some(120.0), Some(140.0)));
    assert_eq!(r.genres.top[0].name, "Jungle");
    // Phrase mood comes from the two library tracks' analysis files.
    assert_eq!(r.energy.as_ref().map(|e| e.tracks), Some(2));
    assert_eq!(r.mixing.transitions, 2);
}

/// `--since` and `--until` take sets by the day they were played. With none in the dates, the
/// collection is described: sessions don't stand in once there are sets.
#[test]
fn sets_are_taken_by_the_day_they_were_played() {
    let dir = tempfile::tempdir().unwrap();
    let root = club_drive(dir.path(), "Main");
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    declare(
        &store,
        vec![
            ("NYE", "2025-12-31", entries(&root, &[1, 2, 3, 4, 5, 6])),
            ("Spring", "2026-03-14", entries(&root, &[7, 8, 9, 10, 11, 12])),
            ("Autumn", "2026-10-03", entries(&root, &[1, 2, 3, 4, 5, 6])),
        ],
    );
    let names = |since: Option<&str>, until: Option<&str>| {
        let req =
            ProfileRequest { since: since.map(String::from), until: until.map(String::from), ..request(&[&root]) };
        let r = build(Some(&store), &req).report;
        (r.basis, r.sets.list.iter().map(|s| s.name.clone()).collect::<Vec<_>>())
    };
    assert_eq!(names(Some("2026"), None), (Basis::Plays, vec!["Autumn".to_string(), "Spring".into()]));
    assert_eq!(names(None, Some("2025")), (Basis::Plays, vec!["NYE".to_string()]));
    assert_eq!(names(Some("2026-03"), Some("2026-03")), (Basis::Plays, vec!["Spring".to_string()]));
    assert_eq!(names(Some("2027"), None), (Basis::Collection, Vec::new()));
}

/// Sessions stand in for sets in the numbers, but the document never counts them: no sets
/// section, no count in the headline, no trait the sets section backs.
#[test]
fn sessions_are_never_counted_in_public() {
    let dir = tempfile::tempdir().unwrap();
    let root = club_drive(dir.path(), "Main");
    let mut report = build(None, &request(&[&root])).report;
    assert_eq!(report.basis, Basis::Sessions);
    report.summary.tags = [("rarely_repeats", "Rarely repeats tracks"), ("harmonic", "Mixes in key")]
        .iter()
        .map(|(id, label)| profile::Tag { id, label: label.to_string(), why: String::new() })
        .collect();
    let all = ProfilePrefs {
        sections: Some(Section::ALL.into_iter().collect()),
        precision: Precision::Exact,
        ..Default::default()
    };
    let d = profile::document(&report, &all);
    assert!(d.sets.is_none());
    assert_eq!(d.tags, ["Mixes in key"]);
    let headline = d.headline.unwrap();
    assert!(headline.ends_with("BPM") && !headline.contains("session"), "{headline}");

    // The same numbers from declared sets are counted.
    report.basis = Basis::Plays;
    let d = profile::document(&report, &all);
    assert_eq!(d.sets.map(|s| s.count), Some(Count::Exact { count: 2 }));
    assert_eq!(d.tags.len(), 2);
    assert!(d.headline.unwrap().ends_with("2 sets"));
}

/// What a report says, but for when it was built.
fn said(r: &profile::ProfileReport) -> serde_json::Value {
    let mut v = serde_json::to_value(r).unwrap();
    v.as_object_mut().unwrap().remove("generated");
    v
}

/// A profile built again, with the snapshots' history lists and the analysis files read the first
/// time kept in the store's cache, reads none of them again and says exactly what the first one
/// did; and so does one built after a pull adds a snapshot, from what's still kept.
#[test]
fn a_profile_built_from_the_cache_says_what_one_built_afresh_does() {
    let dir = tempfile::tempdir().unwrap();
    // Real analysis files, a session and a set played from them.
    let root = common::demo_drive(dir.path(), "Demo");
    common::add_history(&root, "HISTORY 001", &[1, 2]);
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    let mut d = Device::open(&root).unwrap();
    let opts = PullOptions { library: Some("Demo library".into()), force: true, ..Default::default() };
    sync::pull(&store, &mut d, &opts, &Ctx::silent()).unwrap();
    declare(&store, vec![("Listening party", "2026-09-20", entries(&root, &[2, 1]))]);

    let cold = build(Some(&store), &request(&[&root])).report;
    let (databases, analysis) = (cold.cached.databases, cold.cached.analysis);
    assert_eq!((databases.0, analysis.0), (0, 0), "nothing kept yet");
    assert!(databases.1 > 0 && analysis.1 > 0, "{:?}", cold.cached);
    assert!(cold.energy.is_some() && cold.prep.is_some(), "the analysis files were read");
    let warm = build(Some(&store), &request(&[&root])).report;
    assert_eq!(warm.cached.databases, (databases.1, databases.1));
    assert_eq!(warm.cached.analysis, (analysis.1, analysis.1));
    assert_eq!(said(&warm), said(&cold));

    common::add_history(&root, "HISTORY 002", &[2, 1, 2]);
    sync::pull(&store, &mut d, &opts, &Ctx::silent()).unwrap();
    let after = build(Some(&store), &request(&[&root])).report;
    let fresh = tempfile::tempdir().unwrap();
    let copy = Store::open_or_init(fresh.path().join("store")).unwrap();
    common::copy_dir(store.root(), copy.root());
    std::fs::remove_dir_all(copy.root().join("cache")).unwrap();
    let afresh = build(Some(&copy), &request(&[&root])).report;
    assert!(after.cached.databases.0 > 0 && afresh.cached.databases.0 == 0);
    assert_eq!(said(&after), said(&afresh));
}

/// Without declared sets, the profile's sessions are the ones `sets` and `plays` find, with the
/// windows they find them in: the drive's and the store's lists and imported logs, newest first,
/// but those the DJ dismissed and those too short.
#[test]
fn a_profile_stands_in_the_sessions_sets_finds() {
    let dir = tempfile::tempdir().unwrap();
    let root = club_drive(dir.path(), "Main");
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    let mut d = Device::open(&root).unwrap();
    let opts = PullOptions { library: Some("Club library".into()), force: true, ..Default::default() };
    sync::pull(&store, &mut d, &opts, &Ctx::silent()).unwrap();
    common::add_history(&root, "HISTORY 004", &[5, 6, 7, 8, 9, 10]);

    let mut book = SetBook::load(&store).unwrap();
    let started = chrono::Utc::now() - chrono::Duration::days(3);
    let plays = entries(&root, &[1, 2, 3, 4, 5])
        .into_iter()
        .enumerate()
        .map(|(i, entry)| sets::TimedPlay {
            entry,
            started: started + chrono::Duration::minutes(5 * i as i64),
            stopped: None,
            player: None,
        })
        .collect();
    book.imported.push(sets::ImportedSession {
        id: "0123456789abcdef0123456789abcdef".into(),
        name: "Friday".into(),
        tool: "beat_link_trigger".into(),
        file: dir.path().join("friday.log"),
        imported_at: chrono::Utc::now(),
        plays,
    });
    book.save(&store).unwrap();
    let req = api::SessionsRequest { drive: Some(root.display().to_string()), unassigned: false };
    let found = api::sessions(Some(&store), &req, &Ctx::silent()).unwrap().sessions;
    let dismissed = found.iter().find(|s| s.name == "HISTORY 002").unwrap().id.clone();
    book.dismissed.insert(dismissed.clone());
    book.save(&store).unwrap();

    let r = build(Some(&store), &request(&[&root])).report;
    assert_eq!(r.basis, Basis::Sessions);
    let want: Vec<&sets::Session> =
        found.iter().filter(|s| s.id != dismissed && s.tracks.len() >= profile::DEFAULT_MIN_SET_TRACKS).collect();
    let ids = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    assert_eq!(
        r.sets.list.iter().map(|s| s.id.as_str()).collect::<Vec<_>>(),
        want.iter().map(|s| s.id.as_str()).collect::<Vec<_>>()
    );
    assert_eq!(
        ids(&r.sets.list.iter().map(|s| s.name.as_str()).collect::<Vec<_>>()),
        ids(&["HISTORY 004", "HISTORY 001", "Friday"]),
        "newest first: the drive's, then by when each was recorded"
    );
    // The log on the day it was played, the archived list the day it was pulled, the drive's not
    // yet archived.
    let local = |t: chrono::DateTime<chrono::Utc>| t.with_timezone(&chrono::Local).date_naive();
    let dates: Vec<Option<chrono::NaiveDate>> = r.sets.list.iter().map(|s| s.date).collect();
    assert_eq!(dates, [None, want[1].window.before.map(local), Some(local(started))]);
    assert_eq!((r.sets.too_short, r.sets.not_archived), (1, 1));
}
