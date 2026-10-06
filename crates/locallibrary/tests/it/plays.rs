use crate::common;

use std::path::Path;

use locallibrary::api::{self, PlayStatsRequest};
use locallibrary::device::Device;
use locallibrary::onelibrary::{self, OneLibrary};
use locallibrary::plays;
use locallibrary::progress::Ctx;
use locallibrary::recordings::{self, CollectOptions, Tracklist};
use locallibrary::store::Store;
use locallibrary::sync::{self, PullOptions};
use locallibrary::{Library, paths};
use rekordbox_pdb::PdbEditor;

fn names(lists: &[plays::HistoryList]) -> Vec<&str> {
    lists.iter().map(|l| l.name.as_str()).collect()
}

/// Players recorded these plays in export.pdb (play counts too).
fn play(root: &Path, history: &str, tracks: &[u32]) {
    common::add_history(root, history, tracks);
    let pdb = root.join(paths::EXPORT_PDB);
    let lib = locallibrary::pdb::read_library(&pdb).unwrap();
    let mut ed = PdbEditor::from_file(&pdb).unwrap();
    for id in tracks {
        let n = lib.track(*id).unwrap().play_count + 1;
        ed.set_track_field(*id, "play_count", n.into()).unwrap();
    }
    ed.save(&pdb).unwrap();
}

/// What rekordbox does when it imports a drive's history: the lists are gone from the drive.
fn rekordbox_imports_history(root: &Path) {
    let pdb = root.join(paths::EXPORT_PDB);
    let ids: Vec<u32> = locallibrary::pdb::read_library(&pdb).unwrap().history.iter().map(|h| h.id).collect();
    let mut ed = PdbEditor::from_file(&pdb).unwrap();
    ed.remove_history_playlists(&ids).unwrap();
    ed.save(&pdb).unwrap();
}

#[test]
fn history_lists_from_both_databases_count_once() {
    let dir = tempfile::tempdir().unwrap();
    let root = common::bigger_drive(dir.path(), "Main");
    common::add_history(&root, "HISTORY 001", &[1, 2, 3]);
    common::add_onelibrary(&root); // both databases hold HISTORY 001
    // A CDJ-3000X recorded one more, in OneLibrary only.
    let d = Device::open_export(&root).unwrap();
    let mut one = OneLibrary::open_root(&root).unwrap();
    let plus: Library = one.library().unwrap();
    let h = one.create_history("HISTORY 002", 0, false).unwrap();
    for t in plus.tracks.iter().filter(|t| t.title.starts_with("B")) {
        one.add_to_history(h, t.id).unwrap();
    }
    onelibrary::write(&d, &one.to_encrypted().unwrap()).unwrap();

    let h = plays::drive_history(&d).unwrap();
    assert_eq!(names(&h.lists), ["HISTORY 001", "HISTORY 002"]);
    assert_eq!(h.lists[0].database, "device_library");
    assert_eq!(h.lists[1].database, "onelibrary");
    assert_eq!(h.lists[0].tracks.iter().map(|t| t.id).collect::<Vec<_>>(), [1, 2, 3]);
    assert_eq!(h.lists[1].tracks.len(), 2);
    assert_eq!(plays::find_list(&h.lists, "history 002").unwrap().name, "HISTORY 002");
}

/// Regression: a history list's path matched ignoring case before an exact match was looked for.
#[test]
fn history_lists_are_found_by_exact_path_first() {
    let list = |path: &str| plays::HistoryList {
        name: path.into(),
        path: path.into(),
        database: "device_library",
        tracks: Vec::new(),
        missing: 0,
        missing_at: Vec::new(),
    };
    let lists = [list("HISTORY 001"), list("history 001")];
    assert_eq!(plays::find_list(&lists, "history 001").unwrap().path, "history 001");
    assert!(plays::find_list(&lists, "History 001").is_none(), "two match ignoring case");
}

/// Regression: a recording's tracklist came from the first history list whose name matched
/// ignoring case, or from a history folder of that name. The list of exactly that name wins.
#[test]
fn a_recordings_tracklist_is_the_history_list_of_exactly_its_name() {
    let dir = tempfile::tempdir().unwrap();
    let root = common::bigger_drive(dir.path(), "AZ");
    common::onelibrary_only(&root);
    let mut d = Device::open_export(&root).unwrap();
    let lib = common::plus_library(&root);
    let mut one = OneLibrary::open_root(&root).unwrap();
    let folder = one.create_history("Friday", 0, true).unwrap();
    for (name, track) in [("FRIDAY", &lib.tracks[0]), ("Friday", &lib.tracks[1])] {
        let h = one.create_history(name, folder, false).unwrap();
        one.add_to_history(h, track.id).unwrap();
    }
    onelibrary::write(&d, &one.to_encrypted().unwrap()).unwrap();
    std::fs::create_dir_all(root.join("ALPHATHETA REC")).unwrap();
    std::fs::write(root.join("ALPHATHETA REC/REC001.WAV"), b"RIFF").unwrap();

    let tracklist = Tracklist::Named("Friday".into());
    let opts = CollectOptions { to: dir.path().join("Sets"), only: vec![], tracklist, delete: false };
    let report = recordings::collect(&mut d, &opts, &Ctx::silent()).unwrap();
    let text = std::fs::read_to_string(report.collected[0].tracklist.as_ref().unwrap()).unwrap();
    assert_eq!(text, format!("Friday\n\n01. {}\n", lib.tracks[1].display_name()));
}

#[test]
#[cfg(feature = "account")]
fn a_history_list_becomes_a_set_record() {
    let dir = tempfile::tempdir().unwrap();
    let store_root = dir.path().join("store");
    let store = Store::open_or_init(&store_root).unwrap();
    let root = common::bigger_drive(dir.path(), "Main");
    common::add_history(&root, "HISTORY 001", &[1, 2, 3, 4]);
    // The profile never names this one, so the set doesn't either.
    let hide = locallibrary::profile::PrefsUpdate {
        hide: vec!["[a]pendics.shuffle - A2 - In Charge".into()],
        ..Default::default()
    };
    api::profile_prefs_update(&store, &hide).unwrap();

    let req = api::HistoryPublishRequest {
        drive: root.display().to_string(),
        list: "history 001".into(),
        title: Some("Friday warm-up".into()),
        date: Some("2026-10-03".into()),
        hide: vec![4],
        ..Default::default()
    };
    // Signed out, the record can still be looked at.
    let plan = api::history_publish_plan(&store_root, &req, &Ctx::silent()).unwrap();
    assert!(plan.account.is_none() && plan.uri.is_none());
    let r = &plan.record;
    assert_eq!(
        (r["title"].as_str(), r["date"].as_str(), r["source"].as_str()),
        (Some("Friday warm-up"), Some("2026-10-03"), Some("history"))
    );
    let tracks = r["tracks"].as_array().unwrap();
    assert_eq!(tracks.len(), 4);
    assert_eq!(tracks[0]["artist"], "[a]pendics.shuffle");
    assert!(tracks[0]["bpmCentis"].as_i64().unwrap() > 10_000 && tracks[0]["key"].is_string());
    assert_eq!(tracks[1], serde_json::json!({ "unidentified": true }), "hidden by the profile");
    assert!(tracks[2]["title"].is_string());
    assert_eq!(tracks[3], serde_json::json!({ "unidentified": true }), "hidden here");
    assert!(plan.notes.iter().any(|n| n.contains("track 2 isn't named")), "{:?}", plan.notes);
    let text = r.to_string();
    assert!(!text.contains("HISTORY") && !text.contains("Contents/"), "no list names or paths");
    // A TID in the day it was played, the same however often it's planned.
    let tid = jacquard_common::types::tid::Tid::new(&plan.rkey).unwrap();
    let midnight = chrono::NaiveDate::from_ymd_opt(2026, 10, 3).unwrap().and_hms_opt(0, 0, 0).unwrap().and_utc();
    let micros = tid.timestamp() as i64 - midnight.timestamp_micros();
    assert!((0..86_400_000_000).contains(&micros), "{micros}");
    assert_eq!(api::history_publish_plan(&store_root, &req, &Ctx::silent()).unwrap().rkey, plan.rkey);

    // Without a date, today's (nothing archived it yet).
    let undated = api::HistoryPublishRequest { date: None, ..req.clone() };
    let plan = api::history_publish_plan(&store_root, &undated, &Ctx::silent()).unwrap();
    assert!(plan.notes.iter().any(|n| n.starts_with("dated today")), "{:?}", plan.notes);

    let bad =
        |req: api::HistoryPublishRequest| api::history_publish_plan(&store_root, &req, &Ctx::silent()).unwrap_err();
    assert_eq!(bad(api::HistoryPublishRequest { hide: vec![5], ..req.clone() }).code(), "invalid");
    assert_eq!(bad(api::HistoryPublishRequest { date: Some("Friday".into()), ..req.clone() }).code(), "invalid");
    assert_eq!(bad(api::HistoryPublishRequest { list: "HISTORY 009".into(), ..req.clone() }).code(), "not_found");
    assert_eq!(api::history_publish_apply(&store_root, &req, &Ctx::silent()).unwrap_err().code(), "account");

    // Once rekordbox took it off the drive, the store's copy is published, dated the day the
    // store first saw it.
    let mut d = Device::open_export(&root).unwrap();
    let opts = PullOptions { library: Some("Main library".into()), ..Default::default() };
    sync::pull(&store, &mut d, &opts, &Ctx::silent()).unwrap();
    rekordbox_imports_history(&root);
    let plan = api::history_publish_plan(&store_root, &undated, &Ctx::silent()).unwrap();
    assert_eq!(plan.record["tracks"].as_array().unwrap().len(), 4);
    let today = chrono::Utc::now().date_naive().format("%Y-%m-%d").to_string();
    assert!(plan.notes.iter().any(|n| n.starts_with(&format!("dated {today}, the day the store"))), "{:?}", plan.notes);
}

#[test]
fn the_store_keeps_history_rekordbox_deleted_and_counts_plays() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    let root = common::bigger_drive(dir.path(), "Main");
    let mut d = Device::open_export(&root).unwrap();
    let opts = PullOptions { library: Some("Club library".into()), ..Default::default() };
    let pull = |d: &mut Device| sync::pull(&store, d, &opts, &Ctx::silent()).unwrap().snapshot.unwrap();
    let first = pull(&mut d);

    play(&root, "HISTORY 001", &[1, 2]);
    let second = pull(&mut d);
    play(&root, "HISTORY 002", &[2, 3]);
    pull(&mut d);
    // rekordbox imports and deletes them; the next club records another.
    rekordbox_imports_history(&root);
    play(&root, "HISTORY 001", &[4]);
    pull(&mut d);

    let on_drive = plays::drive_history(&d).unwrap();
    assert_eq!(names(&on_drive.lists), ["HISTORY 001"]);
    let archive = plays::archive(&store, None, &Ctx::silent()).unwrap();
    assert_eq!(archive.snapshots, 4);
    let lists: Vec<(&str, usize, usize)> =
        archive.lists.iter().map(|a| (a.list.name.as_str(), a.list.tracks.len(), a.snapshots)).collect();
    // The two "HISTORY 001"s are different lists (different tracks).
    assert_eq!(lists, [("HISTORY 001", 2, 2), ("HISTORY 002", 2, 1), ("HISTORY 001", 1, 1)]);
    assert_eq!(archive.lists[0].first_seen.snapshot, second.id);
    assert_eq!(archive.lists[0].first_seen.drive.as_deref(), Some("Main"));
    assert!(plays::is_archived(&on_drive.lists[0], &archive));
    let other = store.create_library("Other").unwrap();
    assert!(plays::archive(&store, Some(other.id), &Ctx::silent()).unwrap().lists.is_empty());

    // Track 2 was played twice, track 4 once on the drive as it is (its other lists are gone, but
    // play counts stay).
    let req = |since: Option<String>| PlayStatsRequest { source: root.display().to_string(), since, limit: Some(3) };
    let stats = api::play_stats(Some(&store), &req(None)).unwrap();
    assert_eq!(stats.tracks, 12);
    assert_eq!(stats.played, 4);
    assert_eq!(stats.history_lists, 1);
    assert_eq!(stats.most_played.len(), 3);
    assert_eq!((stats.most_played[0].track.id, stats.most_played[0].play_count), (2, 2));
    assert_eq!(stats.never_played.len(), 8);

    let stats = api::play_stats(Some(&store), &req(Some(first.short_id.clone()))).unwrap();
    let since = stats.since.unwrap();
    assert_eq!(since.short_id, first.short_id);
    assert_eq!(since.new_histories, ["HISTORY 001"]);
    let deltas: Vec<(u32, u16, u16, usize)> =
        since.tracks.iter().map(|t| (t.track.id, t.before, t.after, t.history_lists)).collect();
    assert_eq!(deltas, [(2, 0, 2, 0), (4, 0, 1, 1), (1, 0, 1, 0), (3, 0, 1, 0)]);

    // Since a date: since the newest snapshot from before it.
    let latest = store.find_library("Club library").unwrap().unwrap().head.unwrap();
    let stats = api::play_stats(Some(&store), &req(Some("2999-01".into()))).unwrap();
    assert_eq!(stats.since.unwrap().short_id, latest.short());
    let err = api::play_stats(Some(&store), &req(Some("2000".into()))).unwrap_err();
    assert!(err.to_string().contains("from before 2000; the oldest is"), "{err}");

    // A library in the store means its latest snapshot.
    let req = PlayStatsRequest { source: "Club library".into(), since: None, limit: None };
    let head = store.find_library("Club library").unwrap().unwrap().head.unwrap();
    assert_eq!(api::play_stats(Some(&store), &req).unwrap().source, format!("snapshot {}", head.short()));
}
