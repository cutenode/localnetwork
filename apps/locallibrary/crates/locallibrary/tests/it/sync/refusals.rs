//! Syncs, pulls and pushes that are refused or have nothing to do, and the files a sync plan
//! leaves out.

use crate::common;

use std::fs;
use std::path::{Path, PathBuf};

use locallibrary::device::{Device, LineageOp};
use locallibrary::error::Reason;
use locallibrary::progress::Ctx;
use locallibrary::store::Store;
use locallibrary::sync::{self, PullOptions, PushOptions, Source, SyncOptions};
use locallibrary::tree::FileEntry;
use locallibrary::{Error, paths};

fn blank(dir: &Path, name: &str) -> Device {
    Device::open(common::blank(dir, name)).unwrap()
}

#[test]
fn a_sync_needs_an_export_to_copy_and_another_drive_to_copy_it_to() {
    let dir = tempfile::tempdir().unwrap();
    let root = common::demo_drive(dir.path(), "A");
    let (mut a, mut again) = (Device::open_export(&root).unwrap(), Device::open_export(&root).unwrap());
    let mut empty = blank(dir.path(), "Empty");
    let opts = SyncOptions::default();
    let ctx = Ctx::silent();

    let err = sync::sync_drive_to_drive(&mut a, &mut again, &opts, &ctx).unwrap_err();
    assert!(err.to_string().contains("same drive"), "{err}");
    let err = sync::plan_drive_to_drive(&empty, &a, &opts, &ctx).unwrap_err();
    assert!(matches!(err, Error::NotAnExport(_)), "{err}");
    let err = sync::sync_drive_to_many(&mut empty, std::slice::from_mut(&mut a), &opts, &ctx).unwrap_err();
    assert!(matches!(err, Error::NotAnExport(_)), "{err}");
    let err = sync::sync_drive_to_many(&mut a, std::slice::from_mut(&mut again), &opts, &ctx).unwrap_err();
    assert_eq!(err.to_string(), "A is both the source and a target");
    let err =
        sync::sync_drive_to_many_as_planned(&mut a, std::slice::from_mut(&mut empty), &opts, &[], &ctx).unwrap_err();
    assert_eq!(err.to_string(), "one confirmed plan per target");
    assert!(!empty.has_export(), "nothing was written");
}

/// The twelve-track drive with one more track, whose audio rekordbox left at `Music/Set.mp3`.
fn with_audio_outside_contents(dir: &Path) -> PathBuf {
    let root = common::bigger_drive(dir, "A");
    let pdb_path = root.join(paths::EXPORT_PDB);
    let mut ed = rekordbox_pdb::PdbEditor::from_file(&pdb_path).unwrap();
    ed.add_track(&rekordbox_pdb::NewTrack { file_size: 9, ..rekordbox_pdb::NewTrack::new("Set", "/Music/Set.mp3") })
        .unwrap();
    ed.save(&pdb_path).unwrap();
    fs::create_dir_all(root.join("Music")).unwrap();
    fs::write(root.join("Music/Set.mp3"), "A's audio").unwrap();
    root
}

#[test]
fn a_folder_where_the_source_keeps_audio_is_never_replaced() {
    let dir = tempfile::tempdir().unwrap();
    let mut a = Device::open_export(with_audio_outside_contents(dir.path())).unwrap();
    let mut b = blank(dir.path(), "B");
    fs::create_dir_all(b.path("Music/Set.mp3")).unwrap();
    let err = sync::sync_drive_to_drive(&mut a, &mut b, &SyncOptions::default(), &Ctx::silent()).unwrap_err();
    assert_eq!(err.reason(), Some(Reason::InTheWay), "{err}");
    // Refused when planning, before anything is copied.
    assert!(
        err.to_string().starts_with("B has files of its own where A's library keeps audio outside Contents/"),
        "{err}"
    );
    assert!(b.path("Music/Set.mp3").is_dir() && !b.has_export());
}

#[test]
fn audio_outside_contents_is_compared_by_content_when_verifying() {
    let dir = tempfile::tempdir().unwrap();
    let mut a = Device::open_export(with_audio_outside_contents(dir.path())).unwrap();
    let mut b = blank(dir.path(), "B");
    let verify = SyncOptions { verify: true, ..SyncOptions::default() };
    sync::sync_drive_to_drive(&mut a, &mut b, &verify, &Ctx::silent()).unwrap();
    // B's library goes; the file it left there is the same, so it's kept as it is.
    fs::remove_file(b.path(paths::EXPORT_PDB)).unwrap();
    let b = Device::open(b.root()).unwrap();
    let (plan, _) = sync::plan_drive_to_drive(&a, &b, &verify, &Ctx::silent()).unwrap();
    assert!(plan.copies.iter().all(|c| c.path != "Music/Set.mp3"), "{:?}", plan.copies);
}

/// A file outside Contents/ that a sync cut off on this drive wrote (it's newer than the run's
/// start) is that sync's own, so it's copied again rather than refused as the DJ's.
#[test]
fn a_file_an_interrupted_sync_left_outside_contents_is_copied_again() {
    let dir = tempfile::tempdir().unwrap();
    let mut a = Device::open_export(with_audio_outside_contents(dir.path())).unwrap();
    let mut b = blank(dir.path(), "B");
    b.adopt(None).unwrap();
    b.begin(LineageOp::Sync, "sync from A", None).unwrap();
    fs::create_dir_all(b.path("Music")).unwrap();
    fs::write(b.path("Music/Set.mp3"), "torn").unwrap();

    let (plan, _) = sync::plan_drive_to_drive(&a, &b, &SyncOptions::default(), &Ctx::silent()).unwrap();
    let set = plan.copies.iter().find(|c| c.path == "Music/Set.mp3").unwrap();
    assert_eq!((set.reason, set.target_size), (sync::CopyReason::Changed, Some(4)));
    sync::sync_drive_to_drive(&mut a, &mut b, &SyncOptions::default(), &Ctx::silent()).unwrap();
    assert_eq!(fs::read_to_string(b.path("Music/Set.mp3")).unwrap(), "A's audio");
}

#[test]
fn a_snapshot_with_unsafe_or_case_colliding_paths_leaves_them_out() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    let mut a = Device::open(common::demo_drive(dir.path(), "A")).unwrap();
    let pulled = sync::pull(&store, &mut a, &PullOptions::default(), &Ctx::silent()).unwrap();
    let mut snap = store.load_snapshot(&pulled.snapshot.unwrap().id).unwrap();
    let audio = snap.files.iter().find(|f| f.class == paths::FileClass::Audio).unwrap().clone();
    let upper = FileEntry { path: audio.path.to_uppercase().replacen("CONTENTS", "Contents", 1), ..audio.clone() };
    let unsafe_path = FileEntry { path: "Contents/../../escaped.mp3".into(), ..audio };
    snap.files.extend([upper.clone(), unsafe_path]);

    let b = blank(dir.path(), "B");
    let tree = b.scan(None, &Ctx::silent()).unwrap();
    let source = Source::Snapshot { store: &store, snapshot: &snap };
    let plan = sync::plan(&source, &b, &tree, &SyncOptions::default()).unwrap();
    let warnings = [
        format!("{} collides with another file differing only in case; skipped", upper.path),
        "skipping unsafe path \"Contents/../../escaped.mp3\"".to_string(),
    ];
    for w in &warnings {
        assert!(plan.warnings.contains(w), "{w}: {:?}", plan.warnings);
    }
    assert!(plan.copies.iter().all(|c| c.path != upper.path && !c.path.contains("..")), "{:?}", plan.copies);
    assert!(plan.copies.iter().any(|c| c.path.eq_ignore_ascii_case(&upper.path)), "the first spelling is copied");
}

#[test]
fn a_push_has_to_know_which_library_to_write() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    let d = Device::open(common::demo_drive(dir.path(), "A")).unwrap();
    let resolve = |opts: &PushOptions| sync::resolve_push_snapshot(&store, &d, opts).unwrap_err();

    let err = resolve(&PushOptions::default());
    assert_eq!(
        err.to_string(),
        Error::NotFound("the store has no libraries yet; pull a drive first".into()).to_string()
    );
    store.create_library("One").unwrap();
    store.create_library("Two").unwrap();
    let err = resolve(&PushOptions::default());
    assert_eq!(err.reason(), Some(Reason::ChooseLibrary), "{err}");
    let err = resolve(&PushOptions { library: Some("Three".into()), ..Default::default() });
    assert!(matches!(err, Error::NotFound(ref what) if what == "library \"Three\""), "{err}");
}

#[test]
fn pulling_a_drive_into_a_store_that_lacks_its_library_starts_one() {
    let dir = tempfile::tempdir().unwrap();
    let first = Store::open_or_init(dir.path().join("first")).unwrap();
    let mut d = Device::open(common::demo_drive(dir.path(), "Main")).unwrap();
    sync::pull(&first, &mut d, &PullOptions::default(), &Ctx::silent()).unwrap();

    let other = Store::open_or_init(dir.path().join("other")).unwrap();
    let r = sync::pull(&other, &mut d, &PullOptions::default(), &Ctx::silent()).unwrap();
    assert!(!r.up_to_date);
    assert_eq!(r.library, "Main");
    assert_eq!(other.libraries().unwrap().len(), 1);
}

#[test]
fn a_drive_whose_state_is_saved_is_up_to_date_beside_the_head_and_for_a_push() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    let mut d = Device::open(common::demo_drive(dir.path(), "Main")).unwrap();
    let pulled = sync::pull(&store, &mut d, &PullOptions::default(), &Ctx::silent()).unwrap();
    let saved = pulled.snapshot.unwrap();

    let beside = sync::pull_beside(&store, &mut d, &PullOptions::default(), saved.id, &Ctx::silent()).unwrap();
    assert!(beside.up_to_date);
    assert_eq!(beside.snapshot.map(|s| s.id), Some(saved.id));
    assert_eq!(store.libraries().unwrap()[0].head, Some(saved.id));

    let snap = store.load_snapshot(&saved.id).unwrap();
    let plan = sync::plan_push(&store, &snap, &d, &PushOptions::default(), &Ctx::silent()).unwrap();
    assert!(plan.is_noop(), "{plan:?}");
}

/// Planning compares database files by content: trees that weren't hashed are refused rather
/// than compared by size and time.
#[test]
fn a_plan_needs_database_files_hashed_on_both_sides() {
    let dir = tempfile::tempdir().unwrap();
    let a = Device::open_export(common::demo_drive(dir.path(), "A")).unwrap();
    let b = Device::open_export(common::demo_drive(dir.path(), "B")).unwrap();
    let (tree_a, tree_b) = (a.scan(None, &Ctx::silent()).unwrap(), b.scan(None, &Ctx::silent()).unwrap());
    let source = Source::Drive { device: &a, tree: &tree_a };
    let err = sync::plan(&source, &b, &tree_b, &SyncOptions::default()).unwrap_err();
    assert!(err.to_string().ends_with(" must be hashed on both sides before planning"), "{err}");
}
