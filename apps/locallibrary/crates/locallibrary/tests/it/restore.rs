//! Restoring one playlist or folder from a stored snapshot or a drive backup.

mod refusals;

use crate::common;

use std::path::Path;

use crate::common::apply;
use locallibrary::api::{self, PlaylistRestoreRequest, RestoreSource};
use locallibrary::device::{Device, LineageOp};
use locallibrary::edit::{self, Edit};
use locallibrary::error::Reason;
use locallibrary::progress::Ctx;
use locallibrary::query::TrackSelector;
use locallibrary::store::Store;
use locallibrary::sync::{self, PullOptions};
use locallibrary::{Error, Library, backup, paths, pdb};

fn pdb_library(root: &Path) -> Library {
    pdb::read_library(&root.join(paths::EXPORT_PDB)).unwrap()
}

/// Track file paths of a playlist, in order.
fn files(lib: &Library, playlist: &str) -> Vec<String> {
    let p = lib.find_playlist(playlist).unwrap_or_else(|| panic!("no playlist {playlist}"));
    p.track_ids.iter().map(|id| lib.track(*id).unwrap().file_path.clone()).collect()
}

fn children(lib: &Library, folder: &str) -> Vec<String> {
    let f = lib.find_playlist(folder).unwrap();
    lib.children(Some(f.id)).iter().map(|p| p.name.clone()).collect()
}

fn ids(sel: &[u32]) -> TrackSelector {
    TrackSelector { ids: sel.to_vec(), ..Default::default() }
}

fn request(root: &Path, playlist: &str) -> PlaylistRestoreRequest {
    PlaylistRestoreRequest {
        drive: root.display().to_string(),
        playlist: playlist.into(),
        snapshot: None,
        backup: None,
    }
}

/// Both databases with a Shows folder: Warmup, Friday (with a repeat) and Closing.
fn drive_with_gigs(dir: &Path) -> std::path::PathBuf {
    let root = common::bigger_drive(dir, "Main");
    common::add_onelibrary(&root);
    let t: Vec<u32> = pdb_library(&root).tracks.iter().map(|t| t.id).collect();
    apply(
        &root,
        &[
            Edit::SetTracks {
                playlist: "Shows/Warmup".into(),
                tracks: ids(&t[..2]),
                sort: None,
                limit: None,
                create: true,
            },
            Edit::SetTracks {
                playlist: "Shows/Friday".into(),
                tracks: ids(&t[2..6]),
                sort: None,
                limit: None,
                create: true,
            },
            Edit::AddTracks {
                playlist: "Shows/Friday".into(),
                tracks: ids(&t[2..3]),
                position: None,
                sort: None,
                allow_repeats: true,
            },
            Edit::SetTracks {
                playlist: "Shows/Closing".into(),
                tracks: ids(&t[6..8]),
                sort: None,
                limit: None,
                create: true,
            },
        ],
    );
    root
}

#[test]
fn a_deleted_playlist_comes_back_from_the_newest_snapshot_that_has_it() {
    let dir = tempfile::tempdir().unwrap();
    let root = drive_with_gigs(dir.path());
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    let ctx = Ctx::silent();
    let mut d = Device::open_export(&root).unwrap();
    let pulled = sync::pull(&store, &mut d, &PullOptions::default(), &ctx).unwrap().snapshot.unwrap();
    let before = files(&pdb_library(&root), "Shows/Friday");
    assert_eq!(before.len(), 5);

    // Friday is deleted, and one of its tracks with it; the drive is pulled again without it.
    let gone = pdb_library(&root).tracks.iter().find(|t| t.file_path == before[1]).unwrap().id;
    apply(
        &root,
        &[
            Edit::DeletePlaylist { playlist: "Shows/Friday".into(), with_tracks: false },
            Edit::DeleteTracks { tracks: ids(&[gone]), keep_files: false },
        ],
    );
    let mut d = Device::open_export(&root).unwrap();
    let opts = PullOptions { force: true, ..Default::default() };
    let newer = sync::pull(&store, &mut d, &opts, &ctx).unwrap().snapshot.unwrap();
    assert_ne!(newer.id, pulled.id);

    let preview = api::playlist_restore_plan(Some(&store), &request(&root, "Friday"), &ctx).unwrap();
    match &preview.source {
        RestoreSource::Snapshot { snapshot, library } => {
            assert_eq!(snapshot.id, pulled.id, "the newer snapshot doesn't have it");
            assert_eq!(library.as_deref(), Some("Main"));
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(preview.path, "Shows/Friday");
    assert_eq!((preview.playlists, preview.entries), (1, 4));
    assert_eq!(preview.missing.len(), 1);
    assert_eq!((preview.missing[0].position, preview.missing[0].file.as_str()), (2, before[1].as_str()));
    assert_eq!(preview.plan.plan.playlists_created, ["Shows/Friday"]);

    let report = api::playlist_restore_apply(Some(&store), &request(&root, "Shows/Friday"), &ctx).unwrap();
    assert!(report.backup.is_some());
    let want: Vec<String> = before.iter().filter(|f| **f != before[1]).cloned().collect();
    for lib in [pdb_library(&root), common::plus_library(&root)] {
        assert_eq!(files(&lib, "Shows/Friday"), want);
        // Back between its old neighbours.
        assert_eq!(children(&lib, "Shows"), ["Warmup", "Friday", "Closing"]);
    }

    // An ordinary edit: journaled (so it replays after an export) and undone by `restore`.
    let d = Device::open_export(&root).unwrap();
    let journal = edit::journal::edits(&d).unwrap();
    assert!(
        journal
            .iter()
            .any(|e| matches!(e, Edit::SetEntries { playlist, create: true, .. } if playlist == "Shows/Friday"))
    );
    let mut d = Device::open_export(&root).unwrap();
    backup::restore(&mut d, None, false).unwrap();
    assert!(pdb_library(&root).find_playlist("Shows/Friday").is_none());

    // Restoring what's already there changes nothing; a given snapshot is used as given.
    api::playlist_restore_apply(Some(&store), &request(&root, "Shows/Friday"), &ctx).unwrap();
    let mut req = request(&root, "Shows/Friday");
    req.snapshot = Some(pulled.short_id.clone());
    assert!(api::playlist_restore_plan(Some(&store), &req, &ctx).unwrap().noop);
    req.snapshot = Some(newer.short_id.clone());
    assert!(matches!(api::playlist_restore_plan(Some(&store), &req, &ctx), Err(Error::NotFound(_))));
}

#[test]
fn a_folder_comes_back_from_a_backup() {
    let dir = tempfile::tempdir().unwrap();
    let root = drive_with_gigs(dir.path());
    let ctx = Ctx::silent();
    let lib = pdb_library(&root);
    let want: Vec<(String, Vec<String>)> =
        ["Shows/Warmup", "Shows/Friday", "Shows/Closing"].iter().map(|p| (p.to_string(), files(&lib, p))).collect();

    // The folder goes, and a playlist of the drive's own is made in its place.
    apply(&root, &[Edit::DeletePlaylist { playlist: "Shows".into(), with_tracks: false }]);
    let deleted = backup::list(&Device::open_export(&root).unwrap()).unwrap().remove(0);
    let first = lib.tracks[0].id;
    apply(
        &root,
        &[Edit::SetTracks {
            playlist: "Shows/Mine".into(),
            tracks: ids(&[first]),
            sort: None,
            limit: None,
            create: true,
        }],
    );

    // No store: a backup has to be named.
    assert!(api::playlist_restore_plan(None, &request(&root, "Shows"), &ctx).is_err());
    let mut req = request(&root, "Shows");
    req.backup = Some(deleted.id.clone());
    let preview = api::playlist_restore_plan(None, &req, &ctx).unwrap();
    assert!(matches!(&preview.source, RestoreSource::Backup { backup } if backup.id == deleted.id));
    assert!(preview.folder);
    assert_eq!(preview.playlists, 3);
    assert!(preview.missing.is_empty());
    assert_eq!(preview.untouched, ["Shows/Mine"]);

    api::playlist_restore_apply(None, &req, &ctx).unwrap();
    for lib in [pdb_library(&root), common::plus_library(&root)] {
        for (path, entries) in &want {
            assert_eq!(&files(&lib, path), entries, "{path}");
        }
        assert_eq!(children(&lib, "Shows"), ["Warmup", "Friday", "Closing", "Mine"]);
    }
}

#[test]
fn undo_shows_the_files_and_the_journal_entries_it_takes_back() {
    let dir = tempfile::tempdir().unwrap();
    let root = common::demo_drive(dir.path(), "USB");
    // A cue edit rewrites analysis files, which only the file list shows (the databases are the same).
    let set = locallibrary::cue_edit::SetCue {
        hot_cue: Some("C".parse().unwrap()),
        at: locallibrary::cue_edit::CuePosition::Beat(9),
        snap: false,
        loop_beats: None,
        name: None,
        color: None,
    };
    let edit = api::EditCues {
        drive: root.display().to_string(),
        tracks: TrackSelector { ids: vec![2], ..Default::default() },
        edit: api::CueOp::Set(set),
    };
    api::cues_apply(&edit, &Ctx::silent()).unwrap();
    let list = api::undo_list(None, &api::UndoListRequest { drive: Some(root.display().to_string()) }).unwrap();
    assert_eq!(list.drives[0].backups.len(), 1);
    assert_eq!(list.drives[0].backups[0].journal, 1);

    let undo = api::UndoRequest { drive: Some(root.display().to_string()), ..Default::default() };
    let plan = api::undo_plan(&undo).unwrap();
    assert!(
        plan.files.iter().all(|f| f.change == "replaced" && f.path.starts_with("PIONEER/USBANLZ/")),
        "{:?}",
        plan.files
    );
    assert_eq!(plan.files.len(), 2, "the .DAT and the .EXT");
    assert_eq!(plan.journal.len(), 1);
    plan.apply(None, &Ctx::silent()).unwrap();
    assert!(edit::journal::list(&Device::open(&root).unwrap()).unwrap().is_empty());
    // Undoing again steps back to the change before, and there's none (it used to put the same
    // backup back again).
    let again = api::undo_plan(&undo).unwrap_err();
    assert!(matches!(again, Error::NotFound(_)), "{again}");
}

fn undo_request(root: &Path) -> api::UndoRequest {
    api::UndoRequest { drive: Some(root.display().to_string()), ..Default::default() }
}

fn playlist_names(root: &Path) -> Vec<String> {
    pdb_library(root).playlists.iter().map(|p| p.name.clone()).collect()
}

#[test]
fn undo_refuses_a_drive_that_changed_since_and_backs_up_what_it_replaces() {
    let dir = tempfile::tempdir().unwrap();
    let root = common::bigger_drive(dir.path(), "Main");
    apply(&root, &[Edit::CreatePlaylist { path: "First".into(), folder: false }]);
    let edited = backup::list(&Device::open(&root).unwrap()).unwrap().remove(0);
    // Players record a set's history on the drive afterwards.
    common::add_history(&root, "HISTORY 001", &[1, 2]);
    let with_history = std::fs::read(root.join(paths::EXPORT_PDB)).unwrap();

    let err = api::undo_plan(&undo_request(&root)).unwrap_err();
    assert_eq!(err.reason(), Some(Reason::ChangedSince), "{err}");
    assert!(err.to_string().contains(paths::EXPORT_PDB), "{err}");
    let mut d = Device::open(&root).unwrap();
    assert!(backup::restore(&mut d, None, false).is_err(), "restoring checks it too");

    // Forced, the plan names the history list it drops, and what it replaces is backed up first.
    let plan = api::undo_plan(&api::UndoRequest { force: true, ..undo_request(&root) }).unwrap();
    assert_eq!(plan.backup.id, edited.id);
    assert_eq!(plan.history_lists, ["HISTORY 001"]);
    plan.apply(None, &Ctx::silent()).unwrap();
    assert!(pdb_library(&root).history.is_empty());
    let d = Device::open(&root).unwrap();
    let taken = backup::list(&d).unwrap().remove(0);
    assert_eq!(taken.undoes.as_deref(), Some(edited.id.as_str()));
    assert_eq!(std::fs::read(backup::dir(&d, &taken.id).join(paths::EXPORT_PDB)).unwrap(), with_history);

    // That backup puts the history back (a redo); `undo` passes it by.
    assert!(matches!(api::undo_plan(&undo_request(&root)), Err(Error::NotFound(_))));
    let mut d = Device::open(&root).unwrap();
    backup::restore(&mut d, Some(&taken.id), false).unwrap();
    assert_eq!(std::fs::read(root.join(paths::EXPORT_PDB)).unwrap(), with_history);
}

#[test]
fn undo_steps_back_one_change_at_a_time() {
    let dir = tempfile::tempdir().unwrap();
    let root = common::bigger_drive(dir.path(), "Main");
    let original = playlist_names(&root);
    apply(&root, &[Edit::CreatePlaylist { path: "First".into(), folder: false }]);
    apply(&root, &[Edit::CreatePlaylist { path: "Second".into(), folder: false }]);
    let list = api::undo_list(None, &api::UndoListRequest { drive: Some(root.display().to_string()) }).unwrap();
    let next: Vec<bool> = list.drives[0].backups.iter().map(|b| b.next).collect();
    assert_eq!(next, [true, false]);

    api::undo_apply(&undo_request(&root), &Ctx::silent()).unwrap();
    assert!(playlist_names(&root).contains(&"First".to_string()));
    assert!(!playlist_names(&root).contains(&"Second".to_string()));
    api::undo_apply(&undo_request(&root), &Ctx::silent()).unwrap();
    assert_eq!(playlist_names(&root), original);
    assert!(matches!(api::undo_plan(&undo_request(&root)), Err(Error::NotFound(_))));
    assert!(edit::journal::list(&Device::open(&root).unwrap()).unwrap().is_empty());
}

/// Without a drive named, only the mounted drives with backups are listed, the one `undo`
/// takes marked; named, a drive is listed even with none.
#[test]
fn what_can_be_undone_is_listed_for_the_drives_that_have_something() {
    let dir = tempfile::tempdir().unwrap();
    locallibrary::testing::volumes_on_this_thread(Some(vec![dir.path().to_path_buf()]));
    let main = common::demo_drive(dir.path(), "Main");
    let other = common::demo_drive(dir.path(), "Other");
    apply(&main, &[Edit::CreatePlaylist { path: "Friday".into(), folder: false }]);
    apply(&main, &[Edit::CreatePlaylist { path: "Saturday".into(), folder: false }]);

    let all = api::undo_list(None, &api::UndoListRequest::default()).unwrap();
    assert_eq!(all.drives.iter().map(|d| d.drive.as_str()).collect::<Vec<_>>(), ["Main"]);
    let next: Vec<bool> = all.drives[0].backups.iter().map(|b| b.next).collect();
    assert_eq!(next, [true, false], "newest first, and that's the one undo takes");
    let journal: Vec<usize> = all.drives[0].backups.iter().map(|b| b.journal).collect();
    assert_eq!(journal, [1, 2], "putting back the older one takes both edits off the journal");
    let named = api::undo_list(None, &api::UndoListRequest { drive: Some(other.display().to_string()) }).unwrap();
    assert_eq!(named.drives.len(), 1);
    assert!(named.drives[0].backups.is_empty());
}

#[test]
fn backups_go_in_the_order_they_were_made_on_the_drive() {
    let dir = tempfile::tempdir().unwrap();
    let root = common::bigger_drive(dir.path(), "Main");
    apply(&root, &[Edit::CreatePlaylist { path: "First".into(), folder: false }]);
    let d = Device::open(&root).unwrap();
    let first = backup::list(&d).unwrap().remove(0);
    // Ten backups made by an earlier version on a computer whose clock is years ahead.
    for i in 0..10 {
        let to = backup::dir(&d, &format!("20300101T00000{i}.000Z"));
        common::copy_dir(&backup::dir(&d, &first.id), &to);
        let mut info: serde_json::Value =
            serde_json::from_slice(&std::fs::read(to.join("backup.json")).unwrap()).unwrap();
        info["created"] = format!("2030-01-01T00:00:0{i}Z").into();
        info.as_object_mut().unwrap().remove("seq");
        std::fs::write(to.join("backup.json"), serde_json::to_vec(&info).unwrap()).unwrap();
    }
    apply(&root, &[Edit::CreatePlaylist { path: "Second".into(), folder: false }]);
    let d = Device::open(&root).unwrap();
    let backups = backup::list(&d).unwrap();
    let second = &backups[0];
    assert_eq!(second.reason, "before editing");
    assert_eq!(backups.len(), backup::KEEP, "{:?}", backups.iter().map(|b| &b.id).collect::<Vec<_>>());
    assert_eq!(api::undo_plan(&undo_request(&root)).unwrap().backup.id, second.id);

    // The second edit was made on a computer whose clock is behind: putting the first backup
    // back still takes it off the journal.
    let journal = d.state_dir().join("edits.json");
    let mut entries: serde_json::Value = serde_json::from_slice(&std::fs::read(&journal).unwrap()).unwrap();
    entries["entries"][1]["at"] = "2020-01-01T00:00:00Z".into();
    std::fs::write(&journal, serde_json::to_vec(&entries).unwrap()).unwrap();
    let mut d = Device::open(&root).unwrap();
    backup::restore(&mut d, Some(&first.id), false).unwrap();
    assert!(edit::journal::list(&d).unwrap().is_empty());
}

#[test]
fn undo_puts_tracks_back_with_their_modification_times() {
    let dir = tempfile::tempdir().unwrap();
    let root = common::bigger_drive(dir.path(), "Main");
    let track = pdb_library(&root).tracks[0].clone();
    let audio = paths::to_fs(&root, &paths::key(&track.file_path));
    let then = filetime::FileTime::from_unix_time(1_577_836_800, 0);
    filetime::set_file_mtime(&audio, then).unwrap();
    apply(&root, &[Edit::DeleteTracks { tracks: ids(&[track.id]), keep_files: false }]);
    assert!(!audio.exists());
    api::undo_apply(&undo_request(&root), &Ctx::silent()).unwrap();
    let mtime = filetime::FileTime::from_last_modification_time(&std::fs::metadata(&audio).unwrap());
    assert_eq!(mtime.unix_seconds(), then.unix_seconds(), "a sync would copy it again otherwise");
}

/// Make a file nothing but root can read: a test that still passes shows it wasn't read.
#[cfg(unix)]
fn unreadable(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o000)).unwrap();
}

/// A file back where a change removed one is a change since, told without reading it (every
/// `undo` after undoing a big `track rm` hashed all its audio).
#[cfg(unix)]
#[test]
fn a_file_back_where_a_change_removed_one_is_told_without_reading_it() {
    let dir = tempfile::tempdir().unwrap();
    let root = common::bigger_drive(dir.path(), "Main");
    let track = pdb_library(&root).tracks[0].clone();
    let key = paths::key(&track.file_path);
    apply(&root, &[Edit::DeleteTracks { tracks: ids(&[track.id]), keep_files: false }]);
    let audio = paths::to_fs(&root, &key);
    std::fs::write(&audio, b"copied back by hand").unwrap();
    unreadable(&audio);

    let err = api::undo_plan(&undo_request(&root)).unwrap_err();
    assert_eq!(err.reason(), Some(Reason::ChangedSince), "{err}");
    assert!(err.to_string().contains(&key), "{err}");
}

/// The undo plan knows a backed-up file by the hash its backup recorded, and reads only the
/// drive's: a track put back by hand as it was doesn't change.
#[cfg(unix)]
#[test]
fn undo_knows_a_backed_up_file_by_its_recorded_hash() {
    let dir = tempfile::tempdir().unwrap();
    let root = common::bigger_drive(dir.path(), "Main");
    let track = pdb_library(&root).tracks[0].clone();
    let key = paths::key(&track.file_path);
    let audio = paths::to_fs(&root, &key);
    let bytes = std::fs::read(&audio).unwrap();
    apply(&root, &[Edit::DeleteTracks { tracks: ids(&[track.id]), keep_files: false }]);
    std::fs::write(&audio, bytes).unwrap();
    let d = Device::open(&root).unwrap();
    unreadable(&paths::to_fs(&backup::dir(&d, &backup::list(&d).unwrap()[0].id), &key));

    let plan = api::undo_plan(&api::UndoRequest { force: true, ..undo_request(&root) }).unwrap();
    assert!(!plan.files.iter().any(|f| f.path == key), "{:?}", plan.files);
}

#[test]
fn backups_holding_audio_outlast_small_edits() {
    let dir = tempfile::tempdir().unwrap();
    let root = common::bigger_drive(dir.path(), "Main");
    let track = pdb_library(&root).tracks[0].id;
    apply(&root, &[Edit::DeleteTracks { tracks: ids(&[track]), keep_files: false }]);
    for i in 0..=backup::KEEP {
        apply(&root, &[Edit::CreatePlaylist { path: format!("P{i}"), folder: false }]);
    }
    let backups = backup::list(&Device::open(&root).unwrap()).unwrap();
    let audio: Vec<&backup::BackupInfo> = backups.iter().filter(|b| b.audio > 0).collect();
    assert_eq!(audio.len(), 1, "the only copy of the deleted track is kept");
    assert_eq!(backups.len(), backup::KEEP + 1);
}

#[test]
fn an_interrupted_edit_refuses_other_edits_and_undo_takes_it_back() {
    let dir = tempfile::tempdir().unwrap();
    let root = common::bigger_drive(dir.path(), "Main");
    let original = playlist_names(&root);
    apply(&root, &[Edit::CreatePlaylist { path: "First".into(), folder: false }]);
    // As if it was cut off after its swap: the drive still marked, and the backup not told what
    // the edit left.
    let mut d = Device::open(&root).unwrap();
    let made = backup::list(&d).unwrap().remove(0);
    let json = backup::dir(&d, &made.id).join("backup.json");
    let mut info: serde_json::Value = serde_json::from_slice(&std::fs::read(&json).unwrap()).unwrap();
    info.as_object_mut().unwrap().remove("after");
    std::fs::write(&json, serde_json::to_vec(&info).unwrap()).unwrap();
    d.begin(LineageOp::Edit, "create playlist First", Some(made.id.clone())).unwrap();

    let mut d = Device::open_export(&root).unwrap();
    let edits = [Edit::CreatePlaylist { path: "Second".into(), folder: false }];
    let plan = edit::plan(&d, &edits, &edit::EditOptions::default(), &Ctx::silent()).unwrap();
    let err = edit::apply(&plan, &mut d, &Ctx::silent()).unwrap_err();
    assert_eq!(err.reason(), Some(Reason::Interrupted), "{err}");
    assert_eq!(backup::list(&d).unwrap().len(), 1, "the refused edit's backup went again");

    let plan = api::undo_plan(&undo_request(&root)).unwrap();
    assert_eq!(plan.backup.id, made.id);
    plan.apply(None, &Ctx::silent()).unwrap();
    assert_eq!(playlist_names(&root), original);
    assert!(Device::open(&root).unwrap().state().unwrap().pending.is_none());
}

/// A write in place that made no backup (an update file, a repair, an earlier version's edit)
/// can't be put back: the next change finishes it instead of being refused for good.
#[test]
fn a_change_cut_off_without_a_backup_is_finished_by_the_next() {
    let dir = tempfile::tempdir().unwrap();
    let root = common::bigger_drive(dir.path(), "Main");
    let mut d = Device::open(&root).unwrap();
    d.begin(LineageOp::Edit, "copy the update XDJ-AZ.UPD", None).unwrap();

    let mut d = Device::open_export(&root).unwrap();
    let edits = [Edit::CreatePlaylist { path: "First".into(), folder: false }];
    let plan = edit::plan(&d, &edits, &edit::EditOptions::default(), &Ctx::silent()).unwrap();
    edit::apply(&plan, &mut d, &Ctx::silent()).unwrap();
    assert!(playlist_names(&root).contains(&"First".to_string()));
    assert!(Device::open(&root).unwrap().state().unwrap().pending.is_none());
}

/// An undo cut off part way leaves the drive marked with the backup it took first: `undo` then
/// puts that back, as the marker says, though it's an undo's own backup.
#[test]
fn an_interrupted_undo_is_taken_back_by_undo() {
    let dir = tempfile::tempdir().unwrap();
    let root = common::bigger_drive(dir.path(), "Main");
    apply(&root, &[Edit::CreatePlaylist { path: "First".into(), folder: false }]);
    let edited = playlist_names(&root);
    api::undo_apply(&undo_request(&root), &Ctx::silent()).unwrap();
    // As if that undo was cut off after its swap: still marked, and its backup not told what the
    // undo left.
    let mut d = Device::open(&root).unwrap();
    let taken = backup::list(&d).unwrap().remove(0);
    assert!(taken.undoes.is_some());
    let json = backup::dir(&d, &taken.id).join("backup.json");
    let mut info: serde_json::Value = serde_json::from_slice(&std::fs::read(&json).unwrap()).unwrap();
    info.as_object_mut().unwrap().remove("after");
    std::fs::write(&json, serde_json::to_vec(&info).unwrap()).unwrap();
    d.begin(LineageOp::Restore, "restore backup", Some(taken.id.clone())).unwrap();

    let plan = api::undo_plan(&undo_request(&root)).unwrap();
    assert_eq!(plan.backup.id, taken.id);
    plan.apply(None, &Ctx::silent()).unwrap();
    assert_eq!(playlist_names(&root), edited);
    assert!(Device::open(&root).unwrap().state().unwrap().pending.is_none());
}
