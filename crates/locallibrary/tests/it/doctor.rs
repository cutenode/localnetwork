mod analysis;
mod checks;
pub(crate) mod sound;

use crate::common;

use std::fs;
use std::path::Path;

use locallibrary::device::{Device, LineageOp};
use locallibrary::doctor::{self, DoctorOptions, DuplicateReason, Problem, Section};
use locallibrary::progress::Ctx;
use locallibrary::{Error, backup, paths};
use rekordbox_pdb::{NewTrack, PdbEditor};

const A1: &str = "/Contents/[a]pendics.shuffle/Framed And Defamed/01 A1 - It's In The Eyes.mp3";

fn write(root: &Path, key: &str, bytes: &[u8]) {
    let p = paths::to_fs(root, &paths::key(key));
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, bytes).unwrap();
}

/// The bigger fixture plus: a Sync Manager copy of track 1, a second "A2 - In Charge" in its own
/// playlist, files nothing uses, and a few missing files.
fn sick_drive(dir: &Path) -> std::path::PathBuf {
    let root = common::bigger_drive(dir, "Main");
    let original = fs::read(paths::to_fs(&root, &paths::key(A1))).unwrap();
    let copy = common::add_track(
        &root,
        NewTrack {
            artist: Some("[a]pendics.shuffle".into()),
            analyze_path: "/PIONEER/USBANLZ/P0AA/00000001/ANLZ0000.DAT".into(),
            date_added: "2021-02-01".into(),
            // The original's file, so its bitrate (the fixture's track 1 has none).
            duration: 100,
            ..NewTrack::new("A1 (copy)", A1.replace("Eyes.mp3", "Eyes-1.mp3"))
        },
        &original,
    );
    let other = common::add_track(
        &root,
        NewTrack {
            artist: Some("[a]pendics.shuffle".into()),
            analyze_path: "/PIONEER/USBANLZ/P0AA/00000002/ANLZ0000.DAT".into(),
            bitrate: 320,
            duration: 308,
            ..NewTrack::new("A2 - In Charge", "/Contents/Elsewhere/In Charge.mp3")
        },
        b"a different file",
    );
    let pdb = root.join(paths::EXPORT_PDB);
    let mut ed = PdbEditor::from_file(&pdb).unwrap();
    let set = ed.create_playlist("Set", 0, false).unwrap();
    ed.add_to_playlist(set, other, None).unwrap();
    // Artwork for the copy, present; and for track 5, missing.
    let art = ed.add_artwork("/PIONEER/Artwork/00001/a1.jpg").unwrap();
    ed.set_track_field(copy, "artwork_id", art).unwrap();
    let gone = ed.add_artwork("/PIONEER/Artwork/00001/a2.jpg").unwrap();
    ed.set_track_field(5, "artwork_id", gone).unwrap();
    ed.save(&pdb).unwrap();
    write(&root, "PIONEER/Artwork/00001/a1.jpg", b"jpeg");
    write(&root, "PIONEER/Artwork/00001/a1_m.jpg", b"jpeg");

    // Missing: track 3's audio, track 4's analysis.
    let lib = locallibrary::pdb::read_library(&pdb).unwrap();
    fs::remove_file(paths::to_fs(&root, &paths::key(&lib.track(3).unwrap().file_path))).unwrap();
    fs::remove_file(paths::to_fs(&root, &paths::key(&lib.track(4).unwrap().analyze_path))).unwrap();

    // Orphans, plus a cover image that isn't audio (never reported).
    write(&root, "Contents/Old/gone.mp3", b"old audio");
    write(&root, "Contents/Old/cover.jpg", b"jpeg");
    write(&root, "PIONEER/USBANLZ/P0FF/00000009/ANLZ0000.DAT", b"anlz");
    write(&root, "PIONEER/USBANLZ/P0FF/00000009/ANLZ0000.EXT", b"anlz");
    write(&root, "PIONEER/Artwork/00001/a99.jpg", b"jpeg");
    write(&root, "PIONEER/Artwork/00001/a99_m.jpg", b"jpeg");
    root
}

#[test]
fn doctor_reports_duplicates_missing_files_and_orphans() {
    let dir = tempfile::tempdir().unwrap();
    let root = sick_drive(dir.path());
    // OneLibrary references the copy's artwork as b1.jpg: in use, though export.pdb doesn't know it.
    common::add_onelibrary(&root);
    assert!(root.join("PIONEER/Artwork/00001/b1.jpg").is_file());
    let before = common::library_files(&root);

    let d = Device::open_export(&root).unwrap();
    let r = doctor::run(&d, &DoctorOptions::default(), &Ctx::silent()).unwrap();
    assert_eq!(r.tracks, 14);

    let dups = r.duplicates.as_ref().unwrap();
    assert_eq!(dups.len(), 2, "{dups:#?}");
    let copy = dups.iter().find(|g| g.reasons == [DuplicateReason::SyncManagerCopy]).expect("copy found by name");
    assert_eq!(copy.tracks.len(), 2);
    // Track 1 is in a playlist, the copy isn't.
    assert_eq!(copy.keep, 1);
    let meta = dups.iter().find(|g| g.reasons == [DuplicateReason::SameMetadata]).unwrap();
    // Both in one playlist; the second has the higher bitrate.
    assert_eq!(meta.tracks.iter().map(|t| t.playlists).collect::<Vec<_>>(), [1, 1]);
    assert_eq!(meta.keep, 14);
    assert_eq!(meta.tracks[0].track.id, 14);

    let missing = r.missing.as_ref().unwrap();
    let has = |problem: Problem, id: u32| missing.iter().any(|m| m.problem == problem && m.track.id == id);
    assert!(has(Problem::MissingAudio, 3));
    assert!(has(Problem::MissingAnalysis, 4));
    assert!(has(Problem::MissingArtwork, 5));
    // The fixture's placeholder audio is smaller than export.pdb recorded, but it isn't audio that
    // stops early (it doesn't open at all, which only the audio section looks at).
    assert!(!missing.iter().any(|m| m.problem == Problem::Truncated));
    assert!(!missing.iter().any(|m| m.track.id >= 13));
    // A file both databases reference is reported once.
    assert_eq!(missing.iter().filter(|m| m.problem == Problem::MissingAudio).count(), 1);

    let mut orphans: Vec<&str> = r.orphans.as_ref().unwrap().iter().map(|o| o.path.as_str()).collect();
    orphans.sort();
    assert_eq!(
        orphans,
        [
            "Contents/Old/gone.mp3",
            "PIONEER/Artwork/00001/a99.jpg",
            "PIONEER/Artwork/00001/a99_m.jpg",
            "PIONEER/USBANLZ/P0FF/00000009/ANLZ0000.DAT",
            "PIONEER/USBANLZ/P0FF/00000009/ANLZ0000.EXT",
        ]
    );
    assert_eq!(r.unfiled.as_ref().unwrap().iter().map(|t| t.id).collect::<Vec<_>>(), [13]);
    assert!(r.drift.as_ref().unwrap().is_empty());
    assert!(r.large_files.as_ref().unwrap().is_empty());
    // A temporary folder isn't a volume.
    assert!(r.compatibility.is_none());

    // Read-only: nothing on the drive changed, and an unadopted drive got no state.
    assert_eq!(common::library_files(&root), before);
    assert!(!root.join(".locallibrary").exists());

    // Content hashing finds the copy by its audio too; sections can be picked.
    let only = DoctorOptions { hash_audio: true, only: vec![Section::Duplicates], ..Default::default() };
    let r = doctor::run(&d, &only, &Ctx::silent()).unwrap();
    assert!(r.missing.is_none() && r.orphans.is_none());
    let copy = r.duplicates.unwrap().into_iter().find(|g| g.keep == 1).unwrap();
    assert_eq!(copy.reasons, [DuplicateReason::SameAudio, DuplicateReason::SyncManagerCopy]);
    assert!(!root.join(".locallibrary").exists());
}

#[test]
fn orphans_are_removed_after_a_backup_and_can_be_restored() {
    let dir = tempfile::tempdir().unwrap();
    let root = sick_drive(dir.path());
    let mut d = Device::open_export(&root).unwrap();

    let plan = doctor::plan_orphans(&d, &Ctx::silent()).unwrap();
    assert_eq!(plan.files.len(), 5);
    assert_eq!(plan.bytes, 9 + 4 + 4 + 4 + 4);
    assert!(plan.fits && plan.needed_bytes > 0);

    let report = doctor::apply_orphans(&plan, &mut d, &Ctx::silent()).unwrap();
    assert_eq!(report.files_deleted, 5);
    assert!(!root.join("Contents/Old/gone.mp3").exists());
    assert!(!root.join("PIONEER/USBANLZ/P0FF").exists(), "empty folders pruned");
    assert!(root.join("Contents/Old/cover.jpg").is_file());
    assert!(root.join("PIONEER/Artwork/00001/a1_m.jpg").is_file());
    assert_eq!(d.library().unwrap().tracks.len(), 14);
    let d = Device::open_export(&root).unwrap();
    assert_eq!(d.state().unwrap().head().unwrap().op, LineageOp::Edit);
    assert!(doctor::plan_orphans(&d, &Ctx::silent()).unwrap().is_noop());

    let mut d = Device::open_export(&root).unwrap();
    // Forced: track 3's audio was missing before (sick_drive) and isn't in the backup.
    backup::restore(&mut d, report.backup.as_deref(), true).unwrap();
    assert_eq!(fs::read(root.join("Contents/Old/gone.mp3")).unwrap(), b"old audio");
    assert!(root.join("PIONEER/USBANLZ/P0FF/00000009/ANLZ0000.EXT").is_file());
}

#[test]
fn orphans_are_left_alone_when_a_database_is_unreadable_or_an_operation_was_interrupted() {
    let dir = tempfile::tempdir().unwrap();
    let root = sick_drive(dir.path());
    let mut d = Device::open_export(&root).unwrap();
    d.begin(LineageOp::Sync, "sync from Backup", None).unwrap();
    assert!(doctor::plan_orphans(&d, &Ctx::silent()).is_err_and(|e| matches!(e.inner(), Error::Conflict(_))));
    let r = doctor::run(&d, &DoctorOptions::default(), &Ctx::silent()).unwrap();
    assert!(r.warnings.iter().any(|w| w.contains("interrupted")));
    d.finish(None).unwrap();

    // OneLibrary can't be read: what it references is unknown.
    fs::write(root.join(paths::LIBRARY_PLUS_DB), b"not sqlcipher").unwrap();
    let d = Device::open_export(&root).unwrap();
    assert!(matches!(doctor::plan_orphans(&d, &Ctx::silent()), Err(Error::OneLibrary(_))));
    let r = doctor::run(&d, &DoctorOptions::default(), &Ctx::silent()).unwrap();
    assert!(r.orphans.is_none());
    assert!(r.drift.unwrap().error.is_some());
    assert!(r.missing.is_some());
}

#[cfg(unix)]
#[test]
fn files_over_4_gib() {
    let dir = tempfile::tempdir().unwrap();
    let root = common::bigger_drive(dir.path(), "Main");
    let big = "/Contents/Mixes/Five hours.wav";
    let id = common::add_track(&root, NewTrack::new("Five hours", big), b"");
    // A sparse file: export.pdb's 32-bit size field holds the size modulo 4 GiB.
    let size = doctor::FAT32_MAX_FILE + 1000;
    locallibrary::testing::sparse(&paths::to_fs(&root, &paths::key(big)), size).unwrap();
    let pdb = root.join(paths::EXPORT_PDB);
    let mut ed = PdbEditor::from_file(&pdb).unwrap();
    ed.set_track_field(id, "file_size", 1000).unwrap();
    ed.save(&pdb).unwrap();

    let d = Device::open_export(&root).unwrap();
    let r = doctor::run(&d, &DoctorOptions::default(), &Ctx::silent()).unwrap();
    let large = r.large_files.unwrap();
    assert_eq!(large.len(), 1);
    assert_eq!((large[0].size, large[0].track.as_ref().map(|t| t.id)), (size, Some(id)));
    assert!(!r.missing.unwrap().iter().any(|m| m.track.id == id), "the wrapped size matches");
}
