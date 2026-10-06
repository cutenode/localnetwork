use crate::common;

use std::collections::BTreeMap;
use std::path::Path;

use locallibrary::api::{self, ProfileRequest, ProfileView};
use locallibrary::device::Device;
use locallibrary::paths;
use locallibrary::profile::{self, Basis, Count, Precision, PrefsUpdate, ProfilePrefs, Section};
use locallibrary::progress::Ctx;
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

#[test]
fn sets_on_a_drive_describe_how_it_was_played() {
    let dir = tempfile::tempdir().unwrap();
    let root = club_drive(dir.path(), "Main");
    let v = build(None, &request(&[&root]));
    let r = &v.report;

    assert_eq!(r.basis, Basis::Plays);
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
    assert!(r.summary.headline.contains("2 sets"), "{}", r.summary.headline);

    // The document leaves out what names tracks unless asked.
    let doc = serde_json::to_value(&v.document).unwrap();
    assert!(doc.get("signature").is_none());
    assert!(doc.get("artists").is_none());
    assert_eq!(doc["sets"]["count"], 2);
    assert_eq!(doc["sets"]["medianTracks"], 6);
    assert!(doc["sets"].get("since").is_none(), "sets still on a drive have no date");
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
        alias: BTreeMap::from([("electronic".into(), "Electronica".into())]),
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
    assert!(matches!(d.sets.as_ref().unwrap().count, Count::Exact { count: 2 }));

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

#[test]
fn the_summary_says_no_more_than_the_sections_in_the_document() {
    let dir = tempfile::tempdir().unwrap();
    let root = club_drive(dir.path(), "Main");
    let mut report = build(None, &request(&[&root])).report;
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
    assert_eq!(r["$type"], "computer.localnetwork.locallibrary.profile");
    locallibrary::atmosphere::lexicon::validate(locallibrary::atmosphere::lexicon::PROFILE, r).unwrap();
    assert_eq!(r["sets"], serde_json::json!({ "count": 2, "medianTracks": 6 }));
    assert_eq!(r["genres"][0]["name"], "Techno");
    assert!(r["genres"][0]["share"].is_i64() && r["tempo"]["low"].is_i64());
    assert!(r.get("signature").is_none() && r.get("artists").is_none(), "opt-in sections stay out");
    let text = r.to_string();
    assert!(!text.contains("HISTORY") && !text.contains("Contents/"));
    let err = api::profile_publish_apply(&dir.path().join("store"), &request(&[&root]), &Ctx::silent()).unwrap_err();
    assert_eq!(err.code(), "account");
}

#[test]
fn archived_sets_keep_counting_after_rekordbox_takes_them_off_the_drive() {
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
    assert!(r.sets.first_seen.is_some());
    // Newest first: the one still on the drive.
    assert!(r.sets.list[0].on_drive && r.sets.list[0].first_seen.is_none());
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
    assert!(v.document.sets.as_ref().unwrap().since.is_some());
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
