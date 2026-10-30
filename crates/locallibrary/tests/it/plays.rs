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

    // As a session: by its id, or the start of it.
    let id = lists[0].fingerprint();
    assert_eq!(plays::find_list(&lists, &id[..8].to_uppercase()).unwrap().path, "HISTORY 001");
    assert!(plays::find_list(&lists, &id[..3]).is_none(), "an id prefix needs 4 characters");
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
    // Regression: a snapshot id that happened to be all digits was taken for a date.
    let err = api::play_stats(Some(&store), &req(Some("123456789012".into()))).unwrap_err();
    assert_eq!(err.code(), "not_found", "{err}");

    // A library in the store means its latest snapshot.
    let req = PlayStatsRequest { source: "Club library".into(), since: None, limit: None };
    let head = store.find_library("Club library").unwrap().unwrap().head.unwrap();
    assert_eq!(api::play_stats(Some(&store), &req).unwrap().source, format!("snapshot {}", head.short()));
}

/// A list was recorded after the drive's last pull before the one that first saved it. Another
/// drive of the library pulled in between says nothing about when.
#[test]
fn archived_lists_were_recorded_after_their_drives_previous_pull() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    let main = common::bigger_drive(dir.path(), "Main");
    let backup = dir.path().join("Backup");
    common::copy_dir(&main, &backup);
    let opts = PullOptions { library: Some("Club library".into()), force: true, ..Default::default() };
    let pull = |root: &Path| {
        sync::pull(&store, &mut Device::open(root).unwrap(), &opts, &Ctx::silent()).unwrap().snapshot.unwrap()
    };
    let first = pull(&main);
    play(&backup, "HISTORY 001", &[5, 6]);
    let other = pull(&backup);
    play(&main, "HISTORY 001", &[1, 2, 3]);
    let second = pull(&main);

    let archive = plays::archive(&store, None, &Ctx::silent()).unwrap();
    let after: Vec<(usize, Option<chrono::DateTime<chrono::Utc>>)> =
        archive.lists.iter().map(|a| (a.list.tracks.len(), a.recorded_after)).collect();
    assert_eq!(after, [(2, None), (3, Some(first.created))], "Backup wasn't pulled before; Main was");
    assert_eq!(archive.lists[1].first_seen.snapshot, second.id);
    assert!(other.created > first.created);
    assert_eq!(archive.last_pulled(&Device::open(&main).unwrap()), Some(second.created));
    assert_eq!(archive.last_pulled(&Device::open(&backup).unwrap()), Some(other.created));
    assert_eq!(archive.last_pulled(&Device::open(dir.path()).unwrap()), None);
}

/// Reading the store's history lists again, from what the first read kept in the store's
/// cache, finds exactly what the first read did (where entries gone from the library were
/// included) and parses no database; a kept file that can't be read is parsed again.
#[hegel::test(test_cases = 4, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn the_archive_read_again_from_the_cache_is_the_archive(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    let root = common::bigger_drive(dir.path(), "Main");
    let mut d = Device::open_export(&root).unwrap();
    let opts = PullOptions { library: Some("Club library".into()), force: true, ..Default::default() };
    sync::pull(&store, &mut d, &opts, &Ctx::silent()).unwrap();
    // Sessions between pulls (ids past the library's 12 are tracks gone from it), and rekordbox
    // taking them off the drive now and then.
    let sessions: Vec<(Vec<u32>, bool)> = tc.draw(
        gs::vecs(gs::tuples!(
            gs::vecs(gs::integers::<u32>().min_value(1).max_value(14)).min_size(1).max_size(5),
            gs::booleans()
        ))
        .min_size(1)
        .max_size(3),
    );
    for (i, (tracks, imported)) in sessions.iter().enumerate() {
        // The first session played one more track gone from the library since.
        let tracks = [tracks.as_slice(), if i == 0 { &[13] } else { &[] }].concat();
        common::add_history(&root, &format!("HISTORY {:03}", i + 1), &tracks);
        sync::pull(&store, &mut d, &opts, &Ctx::silent()).unwrap();
        if *imported {
            rekordbox_imports_history(&root);
        }
    }

    let read = || {
        let a = plays::archive(&store, None, &Ctx::silent()).unwrap();
        let lists: Vec<(plays::HistoryList, Vec<usize>)> =
            a.lists.iter().map(|l| (l.list.clone(), l.list.missing_at.clone())).collect();
        (serde_json::to_value(&a).unwrap(), lists, a.cached)
    };
    let (cold, cold_lists, (hits, sets)) = read();
    assert_eq!(hits, 0);
    assert!(cold_lists.iter().any(|(_, missing_at)| !missing_at.is_empty()), "{cold_lists:?}");
    assert!(sets >= 1, "{sets}");
    let (warm, warm_lists, cached) = read();
    assert_eq!(cached, (sets, sets), "every set of databases came from the cache");
    assert_eq!(warm, cold);
    assert_eq!(warm_lists, cold_lists);

    // A kept file cut short is as good as none.
    let kept = store.root().join("cache/histories");
    for f in walkdir::WalkDir::new(&kept).into_iter().flatten().filter(|e| e.file_type().is_file()) {
        std::fs::write(f.path(), b"[").unwrap();
    }
    let (again, again_lists, cached) = read();
    assert_eq!(cached, (0, sets));
    assert_eq!((again, again_lists), (cold, cold_lists));
}
