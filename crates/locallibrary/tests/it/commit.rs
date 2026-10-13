//! Every operation that edits a drive in place writes through one routine: a write that fails
//! just before it swaps the new files and databases in leaves the drive byte-identical, and the
//! same plan then applies.

use crate::common::{self, media::png, media::set_artwork};

use indexmap::IndexMap;
use std::path::Path;

use locallibrary::convert::{self, ConvertOptions, Target};
use locallibrary::cue_edit::{self, CueEdit, CuePosition, SetCue};
use locallibrary::edit::{self, Edit, EditOptions};
use locallibrary::merge::{self, MergeOptions};
use locallibrary::progress::Ctx;
use locallibrary::query::TrackSelector;
use locallibrary::recordings::{self, CollectOptions, Tracklist};
use locallibrary::scrub::{self, ScrubOptions};
use locallibrary::sync::{self, PullOptions};
use locallibrary::{Device, Result, Store, clean, doctor, firmware, paths, player, settings, tags};

/// Every file and folder below `root` but locallibrary's lock file and hash cache, by path.
fn snapshot(root: &Path) -> IndexMap<String, Option<Vec<u8>>> {
    let mut out = IndexMap::new();
    for e in walkdir::WalkDir::new(root).min_depth(1) {
        let e = e.unwrap();
        let key = paths::key(&e.path().strip_prefix(root).unwrap().to_string_lossy());
        if [".locallibrary", ".locallibrary/lock", ".locallibrary/index.json"].contains(&key.as_str()) {
            continue;
        }
        out.insert(key, e.file_type().is_file().then(|| std::fs::read(e.path()).unwrap()));
    }
    out
}

/// Makes commits on this thread fail before their swap until dropped.
struct Failing;

impl Failing {
    fn start() -> Failing {
        locallibrary::testing::fail_before_swap(true);
        Failing
    }
}

impl Drop for Failing {
    fn drop(&mut self) {
        locallibrary::testing::fail_before_swap(false);
    }
}

/// `apply` fails before its swap and leaves the drive at `root` exactly as it was; then it works.
fn fails_cleanly<T>(root: &Path, mut apply: impl FnMut() -> Result<T>) -> T {
    let before = snapshot(root);
    let failing = Failing::start();
    let err = apply().err().expect("the write was made to fail");
    drop(failing);
    assert!(err.to_string().contains("before the swap"), "{err}");
    let after = snapshot(root);
    let changed: Vec<&String> = before.keys().chain(after.keys()).filter(|k| before.get(*k) != after.get(*k)).collect();
    assert!(changed.is_empty(), "a failed write changed {changed:?}");
    let out = apply().unwrap_or_else(|e| panic!("the same plan applies once nothing fails: {e}"));
    assert_ne!(snapshot(root), before, "and then it changes the drive");
    out
}

/// [`fails_cleanly`], and then `undo` puts every file the drive had back as it was (files the
/// write added may stay: nothing references them).
fn fails_cleanly_and_undoes<T>(root: &Path, apply: impl FnMut() -> Result<T>) -> T {
    let before = snapshot(root);
    let out = fails_cleanly(root, apply);
    let undo = locallibrary::api::UndoRequest { drive: Some(root.display().to_string()), ..Default::default() };
    let plan = locallibrary::api::undo_plan(&undo).unwrap();
    assert!(!plan.files.is_empty(), "the undo changes files: {:?}", plan.summary);
    plan.apply(None, &Ctx::silent()).unwrap();
    let after = snapshot(root);
    let changed: Vec<&String> = before
        .iter()
        .filter(|(k, v)| !k.starts_with(".locallibrary") && v.is_some() && after.get(*k) != Some(v))
        .map(|(k, _)| k)
        .collect();
    assert!(changed.is_empty(), "undo left {changed:?} as the write made them");
    out
}

fn ids(ids: &[u32]) -> TrackSelector {
    TrackSelector { ids: ids.to_vec(), ..Default::default() }
}

#[test]
fn edits() {
    let dir = tempfile::tempdir().unwrap();
    let root = common::bigger_drive(dir.path(), "Main");
    common::add_onelibrary(&root);
    let edits = [
        Edit::CreatePlaylist { path: "New".into(), folder: false },
        Edit::DeleteTracks { tracks: ids(&[1]), keep_files: false },
    ];
    let mut d = Device::open_export(&root).unwrap();
    let plan = edit::plan(&d, &edits, &EditOptions::default(), &Ctx::silent()).unwrap();
    let report = fails_cleanly_and_undoes(&root, || edit::apply(&plan, &mut d, &Ctx::silent()));
    assert!(report.files_deleted > 0 && report.backup.is_some(), "{report:?}");
    assert!(edit::journal::list(&d).unwrap().is_empty(), "the undone edit comes off the journal");
}

#[test]
fn my_tags() {
    let dir = tempfile::tempdir().unwrap();
    let root = common::demo_drive(dir.path(), "Main");
    common::add_onelibrary(&root);
    let mut d = Device::open_export(&root).unwrap();
    let all: Vec<u32> = d.library().unwrap().tracks.iter().map(|t| t.id).collect();
    let plan = tags::plan(&d, &tags::Edit::Add { tag: "Genre/Techno".into(), tracks: all }).unwrap();
    let report = fails_cleanly_and_undoes(&root, || tags::apply(&plan, &mut d, &Ctx::silent()));
    assert_eq!(report.databases.len(), 2, "{report:?}");
}

#[test]
fn cues() {
    let dir = tempfile::tempdir().unwrap();
    let root = common::demo_drive(dir.path(), "Main");
    common::add_onelibrary(&root);
    let mut d = Device::open_export(&root).unwrap();
    let set = SetCue {
        hot_cue: Some("C".parse().unwrap()),
        at: CuePosition::Beat(9),
        snap: false,
        loop_beats: None,
        name: Some("Drop".into()),
        color: None,
    };
    let plan = cue_edit::plan(&d, &[2], &CueEdit::Set(set), &Ctx::silent()).unwrap();
    let report = fails_cleanly_and_undoes(&root, || cue_edit::apply(&plan, &mut d, &Ctx::silent()));
    assert!(!report.files_written.is_empty() && report.onelibrary.is_some(), "{report:?}");
}

#[test]
fn clean() {
    let dir = tempfile::tempdir().unwrap();
    let root = common::bigger_drive(dir.path(), "Main");
    common::export_new_track(&root, "rekordbox", "House 1");
    let mut d = Device::open_export(&root).unwrap();
    let plan = clean::plan(&d, &Ctx::silent()).unwrap();
    assert_eq!(plan.tracks.len(), 1);
    let report = fails_cleanly_and_undoes(&root, || clean::apply(&plan, &mut d, &Ctx::silent()));
    assert_eq!(report.tracks_removed, 1);
}

#[test]
fn putting_a_backup_back() {
    let dir = tempfile::tempdir().unwrap();
    let root = common::bigger_drive(dir.path(), "Main");
    common::add_onelibrary(&root);
    common::export_new_track(&root, "rekordbox", "House 1");
    let mut d = Device::open_export(&root).unwrap();
    let plan = clean::plan(&d, &Ctx::silent()).unwrap();
    clean::apply(&plan, &mut d, &Ctx::silent()).unwrap();
    // The restore itself stages the backup's files and swaps them in through the same routine.
    let info = fails_cleanly(&root, || locallibrary::backup::restore(&mut d, None, false));
    assert_eq!(info.reason, "before cleaning");
    assert!(d.library().unwrap().tracks.iter().any(|t| t.title == "House 1"));
}

#[test]
fn convert() {
    let dir = tempfile::tempdir().unwrap();
    let root = common::bigger_drive(dir.path(), "Main");
    let mut d = Device::open_export(&root).unwrap();
    set_artwork(&mut d, &[1, 2], &png(120, 90, 7), &Ctx::silent()).unwrap();
    let opts = ConvertOptions { to: Target::OneLibrary, replace: false };
    let plan = convert::plan(&d, &opts, &Ctx::silent()).unwrap();
    let report = fails_cleanly_and_undoes(&root, || convert::apply(&plan, &mut d, &opts, &Ctx::silent()));
    assert!(report.artwork > 0 && !d.has_library_plus(), "undone: {report:?}");
}

#[test]
fn merge() {
    let dir = tempfile::tempdir().unwrap();
    let src = Device::open_export(common::demo_drive(dir.path(), "Guest")).unwrap();
    let root = common::bigger_drive(dir.path(), "Main");
    let mut d = Device::open_export(&root).unwrap();
    let plan = merge::plan(&src, &d, &MergeOptions::default(), &Ctx::silent()).unwrap();
    let report =
        fails_cleanly_and_undoes(&root, || merge::apply(&plan, &src, &mut d, &MergeOptions::default(), &Ctx::silent()));
    assert_eq!(report.tracks_added, 2);
    assert!(report.copied_bytes > 0);
}

#[test]
fn artwork() {
    let dir = tempfile::tempdir().unwrap();
    let root = common::demo_drive(dir.path(), "Main");
    common::add_onelibrary(&root);
    let mut d = Device::open_export(&root).unwrap();
    let database = |f: &String| f == "export.pdb" || f == "exportLibrary.db";
    let art = png(120, 90, 9);
    let report = fails_cleanly(&root, || set_artwork(&mut d, &[1], &art, &Ctx::silent()));
    assert_eq!(report.databases.iter().filter(|f| database(f)).count(), 2, "a new row in each: {report:?}");
    // Again: the track is the only one using its art, so the images are overwritten in place, and
    // the backup keeps the old ones.
    let art = png(120, 90, 99);
    let report = fails_cleanly_and_undoes(&root, || set_artwork(&mut d, &[1], &art, &Ctx::silent()));
    assert!(!report.databases.iter().any(database), "{report:?}");
    let backup = locallibrary::backup::list(&d).unwrap().remove(0);
    assert!(backup.files.iter().any(|f| f.starts_with("PIONEER/Artwork/")), "{backup:?}");
}

#[test]
fn player_name_and_contact() {
    let dir = tempfile::tempdir().unwrap();
    let root = common::demo_drive(dir.path(), "Main");
    common::add_onelibrary(&root);
    let mut d = Device::open(&root).unwrap();
    fails_cleanly_and_undoes(&root, || player::set(&mut d, Some("MAIN"), Some(3)));
    assert_ne!(player::read(&d).onelibrary.unwrap().name.as_deref(), Some("MAIN"), "undone");
    let report = fails_cleanly(&root, || player::set_contact(&mut d, "sam@example.com", false, &Ctx::silent()));
    assert!(report.file.is_file());
}

#[test]
fn orphaned_files() {
    let dir = tempfile::tempdir().unwrap();
    let root = common::bigger_drive(dir.path(), "Main");
    for key in ["Contents/Old/gone.mp3", "PIONEER/USBANLZ/P0FF/00000001/ANLZ0000.DAT"] {
        let p = paths::to_fs(&root, key);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, b"nothing uses this").unwrap();
    }
    let mut d = Device::open_export(&root).unwrap();
    let plan = doctor::plan_orphans(&d, &Ctx::silent()).unwrap();
    assert_eq!(plan.files.len(), 2);
    let report = fails_cleanly_and_undoes(&root, || doctor::apply_orphans(&plan, &mut d, &Ctx::silent()));
    assert_eq!(report.files_deleted, 2);
}

#[test]
fn repair() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    let root = common::demo_drive(dir.path(), "Main");
    let mut d = Device::open(&root).unwrap();
    sync::pull(&store, &mut d, &PullOptions::default(), &Ctx::silent()).unwrap();
    // A byte of the audio rots; its size and time stay.
    let audio = paths::key(&d.library().unwrap().tracks[0].file_path);
    let p = paths::to_fs(&root, &audio);
    let mtime = filetime::FileTime::from_last_modification_time(&std::fs::metadata(&p).unwrap());
    let mut bytes = std::fs::read(&p).unwrap();
    let mid = bytes.len() / 2;
    bytes[mid] ^= 0xff;
    std::fs::write(&p, bytes).unwrap();
    filetime::set_file_mtime(&p, mtime).unwrap();
    let r = scrub::scrub(&d, Some(&store), &[], &ScrubOptions::default(), &Ctx::silent()).unwrap();
    let problems: Vec<_> = r.problems().cloned().collect();
    assert_eq!(problems.len(), 1);
    let report = fails_cleanly(&root, || scrub::repair(&mut d, Some(&store), &[], &problems, &Ctx::silent()));
    assert_eq!(report.repaired, [audio]);
}

#[test]
fn settings_files() {
    let dir = tempfile::tempdir().unwrap();
    let root = common::demo_drive(dir.path(), "Main");
    let mut d = Device::open(&root).unwrap();
    let want: IndexMap<String, String> = [("quantize".to_string(), "off".to_string())].into();
    let report = fails_cleanly_and_undoes(&root, || settings::apply_to_drive(&mut d, &want, true, &Ctx::silent()));
    assert!(!report.files_written.is_empty(), "{report:?}");
}

#[test]
fn firmware_files() {
    let dir = tempfile::tempdir().unwrap();
    let root = common::demo_drive(dir.path(), "Main");
    std::fs::write(root.join("DJM-A9_110.upd"), b"old").unwrap();
    let file = dir.path().join("DJM-A9_120.upd");
    std::fs::write(&file, b"new firmware").unwrap();
    let mut d = Device::open(&root).unwrap();
    let plan = firmware::plan(&d, &file, true).unwrap();
    let report = fails_cleanly(&root, || firmware::apply(&plan, &mut d, true, &Ctx::silent()));
    assert_eq!(report.removed, ["DJM-A9_110.upd"]);
    let removed = fails_cleanly(&root, || firmware::remove(&mut d, &Ctx::silent()));
    assert_eq!(removed, ["DJM-A9_120.upd"]);
}

#[test]
fn recordings_deleted_once_copied() {
    let dir = tempfile::tempdir().unwrap();
    let root = common::demo_drive(dir.path(), "Main");
    std::fs::create_dir_all(root.join("ALPHATHETA REC")).unwrap();
    std::fs::write(root.join("ALPHATHETA REC/REC001.WAV"), b"RIFF").unwrap();
    let mut d = Device::open(&root).unwrap();
    let opts = CollectOptions { to: dir.path().join("Sets"), only: vec![], tracklist: Tracklist::None, delete: true };
    let failing = Failing::start();
    assert!(recordings::collect(&mut d, &opts, &Ctx::silent()).is_err());
    drop(failing);
    assert!(root.join("ALPHATHETA REC/REC001.WAV").is_file(), "a recording stays until its delete is committed");
    // The copy is there already; collect it again to another folder, and delete it this time.
    let opts = CollectOptions { to: dir.path().join("Again"), ..opts };
    let report = recordings::collect(&mut d, &opts, &Ctx::silent()).unwrap();
    assert!(report.collected[0].deleted);
    assert!(!root.join("ALPHATHETA REC/REC001.WAV").exists());
}

#[test]
fn a_cancel_once_the_drive_is_written_finishes_recording_it() {
    let dir = tempfile::tempdir().unwrap();
    let root = common::bigger_drive(dir.path(), "Main");
    let mut d = Device::open_export(&root).unwrap();
    let edits = [Edit::CreatePlaylist { path: "First".into(), folder: false }];
    let plan = edit::plan(&d, &edits, &EditOptions::default(), &Ctx::silent()).unwrap();
    // Ctrl-C as the scan that records the new state starts, after the swap.
    let token = locallibrary::CancelToken::new();
    let progress = |e: &locallibrary::Event| {
        if matches!(e, locallibrary::Event::PhaseStart { phase: locallibrary::progress::Phase::Scan, .. }) {
            token.cancel();
        }
    };
    edit::apply(&plan, &mut d, &Ctx::new(&progress, token.clone())).unwrap();
    assert!(token.is_cancelled());
    let d = Device::open_export(&root).unwrap();
    assert!(d.state().unwrap().pending.is_none(), "not left marked as interrupted");
    assert_eq!(edit::journal::list(&d).unwrap().len(), 1, "the edit is in the journal");
}
