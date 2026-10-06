use crate::common;

use std::sync::atomic::{AtomicUsize, Ordering};

use locallibrary::compare::{self, Confidence, Relation};
use locallibrary::device::{Device, LineageOp};
use locallibrary::progress::{CancelToken, Ctx, Event};
use locallibrary::sync::{self, SyncOptions};
use locallibrary::{Error, paths};

#[test]
fn reads_real_exports() {
    let dir = tempfile::tempdir().unwrap();
    let demo = Device::open_export(common::demo_drive(dir.path(), "demo")).unwrap();
    let lib = demo.library().unwrap();
    assert_eq!(lib.tracks.len(), 2);
    assert_eq!(lib.tracks[0].title, "Demo Track 1");
    assert_eq!(lib.tracks[0].artist, "Loopmasters");
    assert_eq!(lib.tracks[0].key, "Fm");
    assert_eq!(lib.tracks[0].bpm, 128.0);
    assert!(lib.tracks[0].analyze_path.starts_with("/PIONEER/USBANLZ/"));

    let large = Device::open_export(common::large_drive(dir.path(), "large", false)).unwrap();
    let lib = large.library().unwrap();
    // Track and playlist counts match rekordcrate's own tests for this file.
    assert_eq!(lib.tracks.len(), 3886);
    assert_eq!(lib.playlists.len(), 104);
    // rekordcrate reports 6,637 playlist entries because it misreads the row count of pages
    // holding more than 255 rows. All 7,440 found here reference real tracks, and every
    // playlist's entries are complete (no gaps in their positions).
    assert_eq!(lib.playlists.iter().map(|p| p.track_ids.len()).sum::<usize>(), 7440);
    let ids: std::collections::HashSet<u32> = lib.tracks.iter().map(|t| t.id).collect();
    assert!(lib.playlists.iter().flat_map(|p| &p.track_ids).all(|id| ids.contains(id)));
    let s = lib.summary();
    assert!(s.playlists > 0 && s.folders > 0 && s.artists > 100);
    assert!(lib.playlists.iter().all(|p| !lib.playlist_path(p.id).is_empty()));
}

#[test]
fn mirror_to_blank_drive_and_resync_is_noop() {
    let dir = tempfile::tempdir().unwrap();
    let mut a = Device::open_export(common::demo_drive(dir.path(), "A")).unwrap();
    std::fs::create_dir(dir.path().join("B")).unwrap();
    let mut b = Device::open(dir.path().join("B")).unwrap();

    let report = sync::sync_drive_to_drive(&mut a, &mut b, &SyncOptions::default(), &Ctx::silent()).unwrap();
    assert!(report.copied > 0);
    assert_eq!(common::library_files(a.root()), common::library_files(b.root()));
    // Settings are never part of library sync.
    assert!(!b.path("PIONEER/MYSETTING.DAT").exists());

    let c = compare::compare_devices(&a, &b, &Ctx::silent()).unwrap();
    assert_eq!(c.relation, Relation::Identical);
    let b_state = b.state().unwrap();
    assert_eq!(b_state.head().unwrap().op, LineageOp::Sync);
    assert_eq!(b_state.head().unwrap().source, a.id());

    let (plan, _) = sync::plan_drive_to_drive(&a, &b, &SyncOptions::default(), &Ctx::silent()).unwrap();
    assert!(plan.is_noop(), "second sync should copy nothing: {:?}", plan.copies);
}

#[test]
fn newer_drive_is_detected_by_lineage_and_older_one_updated() {
    let dir = tempfile::tempdir().unwrap();
    let mut a = Device::open_export(common::demo_drive(dir.path(), "A")).unwrap();
    std::fs::create_dir(dir.path().join("B")).unwrap();
    let mut b = Device::open(dir.path().join("B")).unwrap();
    sync::sync_drive_to_drive(&mut a, &mut b, &SyncOptions::default(), &Ctx::silent()).unwrap();

    // rekordbox exports a new track to A.
    common::export_new_track(a.root(), "New Artist", "Fresh Track");
    let c = compare::compare_devices(&a, &b, &Ctx::silent()).unwrap();
    assert_eq!((c.relation, c.confidence), (Relation::LeftNewer, Confidence::High));
    assert_eq!(c.diff.added_tracks.len(), 1);
    assert!(c.left.changed_since_last_sync);
    // Argument order doesn't matter.
    assert_eq!(compare::compare_devices(&b, &a, &Ctx::silent()).unwrap().relation, Relation::RightNewer);

    let (plan, _) = sync::plan_drive_to_drive(&a, &b, &SyncOptions::default(), &Ctx::silent()).unwrap();
    // New audio + analysis + database; nothing deleted; database written last.
    assert_eq!(plan.copies_by_class.audio, 1);
    assert!(plan.deletes.is_empty());
    assert_eq!(plan.copies.last().unwrap().path, paths::EXPORT_PDB);

    sync::sync_drive_to_drive(&mut a, &mut b, &SyncOptions::default(), &Ctx::silent()).unwrap();
    assert_eq!(common::library_files(a.root()), common::library_files(b.root()));
    assert_eq!(compare::compare_devices(&a, &b, &Ctx::silent()).unwrap().relation, Relation::Identical);

    // Both drives now change independently: diverged.
    common::export_new_track(a.root(), "X", "Only on A");
    common::export_new_track(b.root(), "Y", "Only on B");
    let c = compare::compare_devices(&a, &b, &Ctx::silent()).unwrap();
    assert_eq!(c.relation, Relation::Diverged);
}

#[test]
fn heuristics_for_drives_never_seen_before() {
    let dir = tempfile::tempdir().unwrap();
    let a_root = common::demo_drive(dir.path(), "A");
    let b_root = dir.path().join("B");
    common::copy_dir(&a_root, &b_root);
    common::export_new_track(&b_root, "New Artist", "Fresh Track");
    let a = Device::open_export(&a_root).unwrap();
    let b = Device::open_export(&b_root).unwrap();
    let c = compare::compare_devices(&a, &b, &Ctx::silent()).unwrap();
    assert_eq!(c.relation, Relation::RightNewer);
    assert!(c.confidence >= Confidence::Medium);
    assert!(c.evidence.iter().any(|e| e.signal == "tracks"));
    // Read-only commands must not touch unadopted drives.
    assert!(!a.state_dir().exists() && !b.state_dir().exists());

    let other = Device::open_export(common::bigger_drive(dir.path(), "other")).unwrap();
    assert_eq!(compare::compare_devices(&a, &other, &Ctx::silent()).unwrap().relation, Relation::Unrelated);
}

#[test]
fn deletes_stale_files_but_never_unmanaged_ones() {
    let dir = tempfile::tempdir().unwrap();
    let mut a = Device::open_export(common::demo_drive(dir.path(), "A")).unwrap();
    let b_root = common::bigger_drive(dir.path(), "B");
    std::fs::write(b_root.join("notes.txt"), "keep me").unwrap();
    std::fs::create_dir_all(b_root.join("Music")).unwrap();
    std::fs::write(b_root.join("Music/mine.mp3"), "keep me too").unwrap();
    std::fs::write(b_root.join("PIONEER/MYSETTING.DAT"), "per-drive settings").unwrap();
    let mut b = Device::open_export(&b_root).unwrap();

    let (plan, _) = sync::plan_drive_to_drive(&a, &b, &SyncOptions::default(), &Ctx::silent()).unwrap();
    assert_eq!(plan.deletes_by_class.audio, 12);
    sync::sync_drive_to_drive(&mut a, &mut b, &SyncOptions::default(), &Ctx::silent()).unwrap();
    assert_eq!(common::library_files(a.root()), common::library_files(b.root()));
    assert!(b_root.join("notes.txt").exists());
    assert!(b_root.join("Music/mine.mp3").exists());
    assert_eq!(std::fs::read_to_string(b_root.join("PIONEER/MYSETTING.DAT")).unwrap(), "per-drive settings");
    // Emptied artist folders are pruned.
    assert!(!b_root.join("Contents/[a]pendics.shuffle").exists());
}

#[test]
fn cancelled_sync_leaves_old_database_and_pending_marker() {
    let dir = tempfile::tempdir().unwrap();
    let mut a = Device::open_export(common::bigger_drive(dir.path(), "A")).unwrap();
    let b_root = common::demo_drive(dir.path(), "B");
    let old_pdb = std::fs::read(b_root.join(paths::EXPORT_PDB)).unwrap();
    let mut b = Device::open_export(&b_root).unwrap();

    let cancel = CancelToken::new();
    let copied = AtomicUsize::new(0);
    let progress = |e: &Event| {
        if let Event::Item { phase: locallibrary::progress::Phase::Copy, .. } = e
            && copied.fetch_add(1, Ordering::SeqCst) == 3
        {
            cancel.cancel();
        }
    };
    let ctx = Ctx::new(&progress, cancel.clone());
    let err = sync::sync_drive_to_drive(&mut a, &mut b, &SyncOptions::default(), &ctx).unwrap_err();
    assert!(matches!(err, Error::Cancelled));

    // Still the complete old library: database untouched, its audio still present.
    assert_eq!(std::fs::read(b_root.join(paths::EXPORT_PDB)).unwrap(), old_pdb);
    let b = Device::open_export(&b_root).unwrap();
    for t in b.library().unwrap().tracks {
        assert!(b.path(&paths::key(&t.file_path)).exists());
    }
    assert!(b.state().unwrap().pending.is_some());
    let c = compare::compare_devices(&a, &b, &Ctx::silent()).unwrap();
    assert!(c.right.interrupted_operation);

    // Re-running finishes the job.
    let mut b = b;
    sync::sync_drive_to_drive(&mut a, &mut b, &SyncOptions::default(), &Ctx::silent()).unwrap();
    assert_eq!(common::library_files(a.root()), common::library_files(b.root()));
    assert!(b.state().unwrap().pending.is_none());
}

#[test]
fn verify_mode_catches_same_size_same_mtime_corruption() {
    let dir = tempfile::tempdir().unwrap();
    let mut a = Device::open_export(common::demo_drive(dir.path(), "A")).unwrap();
    std::fs::create_dir(dir.path().join("B")).unwrap();
    let mut b = Device::open(dir.path().join("B")).unwrap();
    sync::sync_drive_to_drive(&mut a, &mut b, &SyncOptions::default(), &Ctx::silent()).unwrap();

    // Flip a byte in an audio file on B, keeping size and mtime.
    let key = paths::key(&a.library().unwrap().tracks[0].file_path);
    let p = b.path(&key);
    let meta = std::fs::metadata(&p).unwrap();
    let mut bytes = std::fs::read(&p).unwrap();
    bytes[0] ^= 0xff;
    std::fs::write(&p, &bytes).unwrap();
    filetime::set_file_mtime(&p, filetime::FileTime::from_last_modification_time(&meta)).unwrap();
    std::fs::remove_file(b.state_dir().join("index.json")).unwrap();

    let quick = sync::plan_drive_to_drive(&a, &b, &SyncOptions::default(), &Ctx::silent()).unwrap().0;
    assert!(quick.is_noop());
    let opts = SyncOptions { verify: true, ..SyncOptions::default() };
    let thorough = sync::plan_drive_to_drive(&a, &b, &opts, &Ctx::silent()).unwrap().0;
    assert_eq!(thorough.copies.len(), 1);
    assert_eq!(thorough.copies[0].path, key);
}

/// Copy a drive as it stands, modification times included.
fn image(from: &std::path::Path, to: &std::path::Path) {
    for e in walkdir::WalkDir::new(from).into_iter().flatten() {
        let dst = to.join(e.path().strip_prefix(from).unwrap());
        if e.file_type().is_dir() {
            std::fs::create_dir_all(&dst).unwrap();
        } else if e.file_type().is_file() && std::fs::copy(e.path(), &dst).is_ok() {
            let meta = std::fs::metadata(e.path()).unwrap();
            filetime::set_file_mtime(&dst, filetime::FileTime::from_last_modification_time(&meta)).unwrap();
        }
    }
}

/// Regression: a sync stamped each copy with its source's modification time before the data was
/// flushed, so a drive unplugged mid-sync could keep audio files of the right size and time but
/// torn data, and the next sync took them as copied. It also called the two drives diverged, so
/// `sync` refused to finish the job.
#[test]
fn a_sync_cut_off_before_its_flush_is_finished_by_the_next_run() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let mut a = Device::open_export(common::bigger_drive(&root, "A")).unwrap();
    std::fs::create_dir(root.join("B")).unwrap();
    let mut b = Device::open(root.join("B")).unwrap();
    sync::sync_drive_to_drive(&mut a, &mut b, &SyncOptions::default(), &Ctx::silent()).unwrap();
    for n in 0..4 {
        common::export_new_track(a.root(), "New Artist", &format!("Track {n}"));
    }

    // The plug is pulled as the third new track is copied: B as it stood then, with the data of
    // what was copied since the last flush missing (zeros), is what the next run finds.
    let copied = AtomicUsize::new(0);
    let progress = |e: &Event| {
        if let Event::Item { phase: locallibrary::progress::Phase::Copy, path, .. } = e
            && path.starts_with("Contents/")
            && copied.fetch_add(1, Ordering::SeqCst) == 2
        {
            image(&root.join("B"), &root.join("image"));
        }
    };
    sync::sync_drive_to_drive(&mut a, &mut b, &SyncOptions::default(), &Ctx::new(&progress, CancelToken::new()))
        .unwrap();
    let torn: Vec<String> = common::library_files(&root.join("image"))
        .into_iter()
        .filter(|(k, _)| k.contains("New Artist"))
        .map(|(k, _)| k)
        .collect();
    assert_eq!(torn.len(), 2, "two new tracks were copied when the plug was pulled");
    for key in &torn {
        let p = root.join("image").join(key);
        let meta = std::fs::metadata(&p).unwrap();
        std::fs::write(&p, vec![0u8; meta.len() as usize]).unwrap();
        filetime::set_file_mtime(&p, filetime::FileTime::from_last_modification_time(&meta)).unwrap();
    }
    std::fs::remove_dir_all(root.join("B")).unwrap();
    std::fs::rename(root.join("image"), root.join("B")).unwrap();

    // A is newer than the half-written B, and syncing again finishes the job.
    let mut b = Device::open(root.join("B")).unwrap();
    assert!(b.state().unwrap().pending.is_some());
    let c = compare::compare_devices(&a, &b, &Ctx::silent()).unwrap();
    assert_eq!((c.relation, c.confidence), (Relation::LeftNewer, Confidence::High), "{:?}", c.evidence);
    sync::sync_drive_to_drive(&mut a, &mut b, &SyncOptions::default(), &Ctx::silent()).unwrap();
    assert_eq!(common::library_files(a.root()), common::library_files(b.root()));
    assert_eq!(compare::compare_devices(&a, &b, &Ctx::silent()).unwrap().relation, Relation::Identical);
}

/// Regression: a track whose audio rekordbox left outside `Contents/` was copied over whatever the
/// target had at that path, though only the target's own library is scanned there: a file of the
/// DJ's was lost. Now the sync refuses, unless it's the same file.
#[test]
fn sync_never_overwrites_the_djs_own_files_outside_contents() {
    let dir = tempfile::tempdir().unwrap();
    let a_root = common::bigger_drive(dir.path(), "A");
    let pdb_path = a_root.join(paths::EXPORT_PDB);
    let mut ed = rekordbox_pdb::PdbEditor::from_file(&pdb_path).unwrap();
    ed.add_track(&rekordbox_pdb::NewTrack { file_size: 10, ..rekordbox_pdb::NewTrack::new("Set", "/Music/Set.mp3") })
        .unwrap();
    ed.save(&pdb_path).unwrap();
    std::fs::create_dir_all(a_root.join("Music")).unwrap();
    std::fs::write(a_root.join("Music/Set.mp3"), "A's audio").unwrap();
    let b_root = dir.path().join("B");
    std::fs::create_dir_all(b_root.join("Music")).unwrap();
    std::fs::write(b_root.join("Music/Set.mp3"), "mine").unwrap();
    let mut a = Device::open_export(&a_root).unwrap();
    let mut b = Device::open(&b_root).unwrap();

    let err = sync::sync_drive_to_drive(&mut a, &mut b, &SyncOptions::default(), &Ctx::silent()).unwrap_err();
    assert!(matches!(err.inner(), Error::Conflict(_)), "{err}");
    assert!(err.to_string().contains("Music/Set.mp3"), "{err}");
    assert_eq!(std::fs::read_to_string(b_root.join("Music/Set.mp3")).unwrap(), "mine");
    assert!(!b.has_export(), "nothing was written");

    // Moved away, the track's audio is copied there; synced again, it's the same file.
    std::fs::rename(b_root.join("Music/Set.mp3"), b_root.join("Music/mine.mp3")).unwrap();
    sync::sync_drive_to_drive(&mut a, &mut b, &SyncOptions::default(), &Ctx::silent()).unwrap();
    assert_eq!(std::fs::read_to_string(b_root.join("Music/Set.mp3")).unwrap(), "A's audio");
    assert_eq!(std::fs::read_to_string(b_root.join("Music/mine.mp3")).unwrap(), "mine");
    assert!(sync::plan_drive_to_drive(&a, &b, &SyncOptions::default(), &Ctx::silent()).unwrap().0.is_noop());

    // A file left there by an earlier sync, which B's library no longer uses, is taken as it is.
    let mut c = Device::open(dir.path().join("B")).unwrap();
    std::fs::remove_file(c.path(paths::EXPORT_PDB)).unwrap();
    let (plan, _) = sync::plan_drive_to_drive(&a, &c, &SyncOptions::default(), &Ctx::silent()).unwrap();
    assert!(!plan.copies.iter().any(|p| p.path == "Music/Set.mp3"), "{:?}", plan.copies);
    sync::sync_drive_to_drive(&mut a, &mut c, &SyncOptions::default(), &Ctx::silent()).unwrap();
}

/// Regression: `--keep-extra` kept database files the source lacks (here exportExt.pdb, beside the
/// source's export.pdb), and recorded the source's state on a drive whose files differ from it, so
/// the drive looked changed since and newer, and the next sync ran backwards.
#[test]
fn keep_extra_keeps_audio_but_not_old_databases_and_the_next_sync_runs_forwards() {
    let dir = tempfile::tempdir().unwrap();
    let mut a = Device::open_export(common::bigger_drive(dir.path(), "A")).unwrap();
    let b_root = common::demo_drive(dir.path(), "B");
    let demo = Device::open_export(&b_root).unwrap().library().unwrap();
    let demo_audio: Vec<String> = demo.tracks.iter().map(|t| paths::key(&t.file_path)).collect();
    let mut b = Device::open_export(&b_root).unwrap();
    let keep = SyncOptions { delete: false, ..SyncOptions::default() };

    let (plan, _) = sync::plan_drive_to_drive(&a, &b, &keep, &Ctx::silent()).unwrap();
    assert_eq!(plan.deletes.iter().map(|d| d.path.as_str()).collect::<Vec<_>>(), [paths::EXPORT_EXT_PDB]);
    sync::sync_drive_to_drive(&mut a, &mut b, &keep, &Ctx::silent()).unwrap();
    assert!(!b_root.join(paths::EXPORT_EXT_PDB).exists(), "B's old exportExt.pdb stays beside A's export.pdb");
    for key in &demo_audio {
        assert!(b_root.join(key).is_file(), "{key} was kept");
    }

    // B holds A's library (and its own audio besides): identical, and nothing to do.
    let c = compare::compare_devices(&a, &b, &Ctx::silent()).unwrap();
    assert_eq!(c.relation, Relation::Identical, "{:?}", c.evidence);
    assert!(!c.right.changed_since_last_sync);
    assert!(sync::plan_drive_to_drive(&a, &b, &keep, &Ctx::silent()).unwrap().0.is_noop());

    // A moves on: A is newer, and B gets A's new track.
    common::export_new_track(a.root(), "New Artist", "Fresh Track");
    let c = compare::compare_devices(&a, &b, &Ctx::silent()).unwrap();
    assert_eq!(c.relation, Relation::LeftNewer, "{:?}", c.evidence);
    sync::sync_drive_to_drive(&mut a, &mut b, &keep, &Ctx::silent()).unwrap();
    assert_eq!(compare::compare_devices(&a, &b, &Ctx::silent()).unwrap().relation, Relation::Identical);

    // A sync without --keep-extra removes B's own files, and the drives are copies again.
    sync::sync_drive_to_drive(&mut a, &mut b, &SyncOptions::default(), &Ctx::silent()).unwrap();
    assert_eq!(common::library_files(a.root()), common::library_files(b.root()));
    assert!(Device::open(&b_root).unwrap().state().unwrap().alias.is_none());
}

#[test]
fn a_cancel_once_the_library_is_copied_finishes_recording_it() {
    let dir = tempfile::tempdir().unwrap();
    let mut a = Device::open_export(common::demo_drive(dir.path(), "A")).unwrap();
    std::fs::create_dir(dir.path().join("B")).unwrap();
    let mut b = Device::open(dir.path().join("B")).unwrap();
    let cancel = CancelToken::new();
    let token = cancel.clone();
    let progress = move |e: &Event| {
        if matches!(e, Event::PhaseEnd { phase: locallibrary::progress::Phase::Copy }) {
            token.cancel();
        }
    };
    sync::sync_drive_to_drive(&mut a, &mut b, &SyncOptions::default(), &Ctx::new(&progress, cancel.clone())).unwrap();
    assert!(cancel.is_cancelled());
    let b = Device::open(dir.path().join("B")).unwrap();
    assert!(b.state().unwrap().pending.is_none());
    assert_eq!(compare::compare_devices(&a, &b, &Ctx::silent()).unwrap().relation, Relation::Identical);
}

#[test]
fn a_sync_over_a_library_forgets_the_edits_recorded_for_it() {
    let dir = tempfile::tempdir().unwrap();
    let mut a = Device::open_export(common::demo_drive(dir.path(), "A")).unwrap();
    let b_root = common::bigger_drive(dir.path(), "B");
    common::apply(&b_root, &[locallibrary::edit::Edit::CreatePlaylist { path: "Mine".into(), folder: false }]);
    let mut b = Device::open_export(&b_root).unwrap();
    let (plan, _) = sync::plan_drive_to_drive(&a, &b, &SyncOptions::default(), &Ctx::silent()).unwrap();
    assert!(plan.warnings.iter().any(|w| w.contains("1 recorded edit(s)")), "{:?}", plan.warnings);
    sync::sync_drive_to_drive(&mut a, &mut b, &SyncOptions::default(), &Ctx::silent()).unwrap();
    let b = Device::open_export(&b_root).unwrap();
    assert!(locallibrary::edit::journal::list(&b).unwrap().is_empty());
    assert_eq!(locallibrary::api::exported_over_edits(&b), 0);
}

#[test]
fn a_drive_a_sync_was_cut_off_on_is_finished_before_anything_else() {
    let dir = tempfile::tempdir().unwrap();
    let store = locallibrary::Store::open_or_init(dir.path().join("store")).unwrap();
    let mut a = Device::open_export(common::demo_drive(dir.path(), "A")).unwrap();
    let b_root = common::demo_drive(dir.path(), "B");
    let mut b = Device::open_export(&b_root).unwrap();
    b.begin(LineageOp::Sync, "sync from A", None).unwrap();
    let interrupted = |e: Error| assert_eq!(e.reason(), Some(locallibrary::error::Reason::Interrupted), "{e}");

    // No edit on top of it, no pull or merge from it.
    let edits = [locallibrary::edit::Edit::CreatePlaylist { path: "Mine".into(), folder: false }];
    let plan = locallibrary::edit::plan(&b, &edits, &Default::default(), &Ctx::silent()).unwrap();
    interrupted(locallibrary::edit::apply(&plan, &mut b, &Ctx::silent()).unwrap_err());
    interrupted(sync::pull(&store, &mut b, &sync::PullOptions::default(), &Ctx::silent()).unwrap_err());
    let merge = locallibrary::merge::plan(&b, &a, &Default::default(), &Ctx::silent());
    interrupted(merge.unwrap_err());
    assert!(Device::open(&b_root).unwrap().state().unwrap().pending.is_some(), "the marker stays");

    // Syncing onto it again finishes it.
    let mut b = Device::open_export(&b_root).unwrap();
    sync::sync_drive_to_drive(&mut a, &mut b, &SyncOptions::default(), &Ctx::silent()).unwrap();
    assert!(Device::open(&b_root).unwrap().state().unwrap().pending.is_none());
}

/// Saying which way to copy doesn't copy a library an edit was cut off in the middle of, to one
/// drive or several, nor pack from it.
#[test]
fn a_drive_an_edit_was_cut_off_on_isnt_copied_from() {
    let dir = tempfile::tempdir().unwrap();
    let mut a = Device::open_export(common::demo_drive(dir.path(), "A")).unwrap();
    let mut b = Device::open_export(common::demo_drive(dir.path(), "B")).unwrap();
    b.begin(LineageOp::Merge, "merge from C", Some("20261006T000000.000Z".into())).unwrap();
    let interrupted = |e: Error| assert_eq!(e.reason(), Some(locallibrary::error::Reason::Interrupted), "{e}");

    interrupted(sync::sync_drive_to_drive(&mut b, &mut a, &SyncOptions::default(), &Ctx::silent()).unwrap_err());
    let mut targets = [a];
    interrupted(sync::sync_drive_to_many(&mut b, &mut targets, &SyncOptions::default(), &Ctx::silent()).unwrap_err());
    std::fs::create_dir(dir.path().join("Club")).unwrap();
    let club = Device::open(dir.path().join("Club")).unwrap();
    let opts = locallibrary::pack::PackOptions { playlists: vec!["Set".into()], ..Default::default() };
    interrupted(locallibrary::pack::plan(&b, &club, &opts, &Ctx::silent()).unwrap_err());
}

/// Sync says why it can't tell which drive is newer, here that an edit on the newer one didn't
/// finish (it used to say both drives had changes the other lacked), for a pair and for several.
#[test]
fn sync_says_why_it_cant_tell_which_drive_is_newer() {
    let dir = tempfile::tempdir().unwrap();
    let a_root = common::demo_drive(dir.path(), "A");
    let b_root = dir.path().join("B");
    std::fs::create_dir(&b_root).unwrap();
    let mut a = Device::open_export(&a_root).unwrap();
    sync::sync_drive_to_drive(&mut a, &mut Device::open(&b_root).unwrap(), &SyncOptions::default(), &Ctx::silent())
        .unwrap();
    common::apply(&b_root, &[locallibrary::edit::Edit::CreatePlaylist { path: "Mine".into(), folder: false }]);
    let mut b = Device::open(&b_root).unwrap();
    b.begin(LineageOp::Edit, "create playlist Mine", Some("20261006T000000.000Z".into())).unwrap();

    let (a, b) = (a_root.display().to_string(), b_root.display().to_string());
    let pair = serde_json::from_value(serde_json::json!({ "left": a, "right": b })).unwrap();
    let many = serde_json::from_value(serde_json::json!({ "drives": [a, b] })).unwrap();
    let ctx = Ctx::silent();
    let errors = [
        locallibrary::api::sync_plan(&pair, &ctx).map(drop).unwrap_err(),
        locallibrary::api::sync_many_plan(&many, &ctx).map(drop).unwrap_err(),
    ];
    for e in errors {
        assert_eq!(e.reason(), Some(locallibrary::error::Reason::Uncertain), "{e}");
        assert!(e.to_string().contains("didn't finish"), "{e}");
    }
}

/// A sync records the source's state from its identity file as it is once the source is locked:
/// what another process wrote there after this one opened the drive stays.
#[test]
fn a_sync_keeps_what_another_process_recorded_on_its_source() {
    let dir = tempfile::tempdir().unwrap();
    let a_root = common::demo_drive(dir.path(), "A");
    let mut a = Device::open_export(&a_root).unwrap();
    std::fs::create_dir(dir.path().join("B")).unwrap();
    let mut b = Device::open(dir.path().join("B")).unwrap();
    common::apply(&a_root, &[locallibrary::edit::Edit::CreatePlaylist { path: "Mine".into(), folder: false }]);
    let id = Device::open(&a_root).unwrap().id();

    sync::sync_drive_to_drive(&mut a, &mut b, &SyncOptions::default(), &Ctx::silent()).unwrap();
    let a = Device::open(&a_root).unwrap();
    assert_eq!(a.id(), id);
    assert!(a.state().unwrap().lineage.iter().any(|e| e.op == LineageOp::Edit), "the edit's entry is kept");
}

#[test]
fn what_players_and_rekordbox_keep_on_a_drive_stays_with_it() {
    let dir = tempfile::tempdir().unwrap();
    let a_root = common::demo_drive(dir.path(), "A");
    let b_root = dir.path().join("B");
    common::copy_dir(&a_root, &b_root);
    let write = |root: &std::path::Path, key: &str| {
        let p = paths::to_fs(root, key);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, key.as_bytes()).unwrap();
    };
    for key in ["PIONEER/LOG/0001.LOG", "PIONEER/rekordbox/RBFLTR.DAT", paths::PLAYLIST_SYNC] {
        write(&a_root, key);
    }
    write(&b_root, "PIONEER/MPJ/state.dat");
    let mut a = Device::open_export(&a_root).unwrap();
    let mut b = Device::open_export(&b_root).unwrap();
    assert_eq!(compare::compare_devices(&a, &b, &Ctx::silent()).unwrap().relation, Relation::Identical);
    let (plan, _) = sync::plan_drive_to_drive(&a, &b, &SyncOptions::default(), &Ctx::silent()).unwrap();
    assert!(plan.is_noop(), "copies {:?}, deletes {:?}", plan.copies, plan.deletes);
    sync::sync_drive_to_drive(&mut a, &mut b, &SyncOptions::default(), &Ctx::silent()).unwrap();
    assert!(paths::to_fs(&b_root, "PIONEER/MPJ/state.dat").is_file());
    assert!(!paths::to_fs(&b_root, "PIONEER/LOG/0001.LOG").exists());

    // Lineage recorded before they were left out still matches: it counted them.
    let scanned = a.scan_fingerprinted(a.library().ok().as_ref(), &Ctx::silent()).unwrap();
    let counted: Vec<locallibrary::tree::FileEntry> = scanned
        .tree
        .entries
        .values()
        .cloned()
        .map(|mut e| {
            if e.class == paths::FileClass::DriveState {
                e.class = paths::FileClass::Other;
            }
            e
        })
        .collect();
    let before = locallibrary::device::fingerprint(&counted, None).unwrap().unwrap();
    assert_ne!(Some(before), scanned.fingerprint);
    assert!(scanned.legacy_fingerprints.contains(&before));
}
