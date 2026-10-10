//! What a scrub compares each file with, and the files a repair leaves alone.

use crate::common::{self, rot};

use std::path::Path;

use locallibrary::device::Device;
use locallibrary::progress::Ctx;
use locallibrary::scrub::{self, FileProblem, ReferenceKind, RepairSource, ScrubOptions, ScrubReport};
use locallibrary::store::Store;
use locallibrary::sync::{self, PullOptions, SyncOptions};
use locallibrary::{Hash, paths};

/// The demo drive, pulled into a new store.
fn pulled(dir: &Path) -> (Store, Device) {
    let store = Store::open_or_init(dir.join("store")).unwrap();
    let mut d = Device::open(common::demo_drive(dir, "Main")).unwrap();
    sync::pull(&store, &mut d, &PullOptions::default(), &Ctx::silent()).unwrap();
    (store, d)
}

fn scrub(d: &Device, store: Option<&Store>, opts: &ScrubOptions) -> ScrubReport {
    scrub::scrub(d, store, &[], opts, &Ctx::silent()).unwrap()
}

fn analysis_key(d: &Device) -> String {
    paths::key(&d.library().unwrap().tracks[0].analyze_path)
}

#[test]
fn a_named_snapshot_finds_every_library_file_gone_since() {
    let dir = tempfile::tempdir().unwrap();
    let (store, d) = pulled(dir.path());
    let head = store.libraries().unwrap()[0].head.unwrap();
    let dat = analysis_key(&d);
    std::fs::remove_file(d.path(&dat)).unwrap();

    // The drive no longer matches a snapshot by its fingerprint, but the one named still says
    // what it should hold.
    assert!(scrub(&d, Some(&store), &ScrubOptions::default()).missing.is_empty());
    let r = scrub(&d, Some(&store), &ScrubOptions { snapshot: Some(head) });
    assert_eq!(r.snapshot, Some(head));
    let missing: Vec<(&str, Option<ReferenceKind>, Option<&RepairSource>)> =
        r.missing.iter().map(|m| (m.path.as_str(), m.reference, m.repair.as_ref())).collect();
    assert_eq!(missing, [(dat.as_str(), Some(ReferenceKind::Snapshot), Some(&RepairSource::Store))]);
    assert_eq!(r.tracks.len(), 1);
    assert_eq!(r.summary(), format!("0 damaged, 0 unreadable, 1 missing of {} files", r.files));
}

#[test]
fn a_file_the_hash_cache_forgot_is_checked_against_an_older_snapshot() {
    let dir = tempfile::tempdir().unwrap();
    let (store, d) = pulled(dir.path());
    // The library changes since the pull, and the drive loses its hash cache.
    common::export_new_track(d.root(), "New", "Track");
    std::fs::remove_file(d.state_dir().join("index.json")).unwrap();
    let audio = paths::key(&d.library().unwrap().tracks[0].file_path);
    rot(&d.path(&audio));

    let r = scrub(&d, Some(&store), &ScrubOptions::default());
    assert_eq!(r.snapshot, None);
    assert_eq!(r.damaged.len(), 1, "{r:?}");
    let damaged = &r.damaged[0];
    assert_eq!((damaged.path.as_str(), damaged.reference), (audio.as_str(), Some(ReferenceKind::History)));
    assert_eq!(damaged.repair, Some(RepairSource::Store));
    assert_ne!(damaged.expected, damaged.actual);
}

#[cfg(unix)]
#[test]
fn a_file_that_cant_be_read_is_reported_as_unreadable() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let (store, d) = pulled(dir.path());
    let dat = analysis_key(&d);
    std::fs::set_permissions(d.path(&dat), std::fs::Permissions::from_mode(0o000)).unwrap();
    if std::fs::read(d.path(&dat)).is_ok() {
        return; // Running as root, which reads anything.
    }

    let r = scrub(&d, Some(&store), &ScrubOptions::default());
    assert_eq!(r.unreadable.len(), 1, "{r:?}");
    assert_eq!(r.unreadable[0].path, dat);
    assert!(r.unreadable[0].error.is_some());
    assert_eq!(r.snapshot, None, "a drive with a file it can't read has no fingerprint to look up");
    assert!(!r.is_clean() && r.summary().starts_with("0 damaged, 1 unreadable"), "{}", r.summary());
}

#[test]
fn repair_leaves_alone_what_it_has_no_good_copy_for_or_is_already_intact() {
    let dir = tempfile::tempdir().unwrap();
    let (store, mut d) = pulled(dir.path());
    let audio = paths::key(&d.library().unwrap().tracks[0].file_path);
    rot(&d.path(&audio));
    let damaged = scrub(&d, Some(&store), &ScrubOptions::default()).damaged.remove(0);
    // Put right by hand after the scrub.
    rot(&d.path(&audio));

    let unknown = Hash::of(b"no such object");
    let problems = [
        damaged.clone(),
        FileProblem { path: "Contents/nowhere.mp3".into(), expected: None, ..damaged.clone() },
        FileProblem { path: "Music/loose.mp3".into(), expected: Some(unknown), ..damaged.clone() },
        FileProblem { path: "Contents/other.mp3".into(), expected: Some(unknown), ..damaged.clone() },
        FileProblem { path: "Contents/lost.mp3".into(), expected: Some(unknown), repair: None, ..damaged },
    ];
    let r = scrub::repair(&mut d, Some(&store), &[], &problems, &Ctx::silent()).unwrap();
    assert!(r.repaired.is_empty() && r.failed.is_empty());
    let skipped: Vec<(&str, &str)> = r.skipped.iter().map(|s| (s.path.as_str(), s.reason.as_str())).collect();
    assert_eq!(
        skipped,
        [
            (audio.as_str(), "already intact"),
            ("Contents/nowhere.mp3", "no known-good copy"),
            ("Music/loose.mp3", "not a library file"),
            ("Contents/other.mp3", "no known-good copy is available now"),
            ("Contents/lost.mp3", "no known-good copy is available now"),
        ]
    );
}

#[test]
fn repair_from_a_sibling_whose_copy_is_damaged_too_fails_that_file() {
    let dir = tempfile::tempdir().unwrap();
    let mut a = Device::open(common::demo_drive(dir.path(), "A")).unwrap();
    std::fs::create_dir(dir.path().join("B")).unwrap();
    let mut b = Device::open(dir.path().join("B")).unwrap();
    sync::sync_drive_to_drive(&mut a, &mut b, &SyncOptions::default(), &Ctx::silent()).unwrap();
    // B's hash cache vouches for the analysis file it was given; the file then rots on both.
    let dat = analysis_key(&b);
    rot(&b.path(&dat));
    let siblings = vec![Device::open(a.root()).unwrap()];
    let r = scrub::scrub(&b, None, &siblings, &ScrubOptions::default(), &Ctx::silent()).unwrap();
    assert_eq!(r.damaged.len(), 1, "{r:?}");
    assert!(matches!(r.damaged[0].repair, Some(RepairSource::Drive { .. })));
    rot(&a.path(&dat));
    let before = std::fs::read(b.path(&dat)).unwrap();

    let rep = scrub::repair(&mut b, None, &siblings, &r.damaged, &Ctx::silent()).unwrap();
    assert!(rep.repaired.is_empty(), "{rep:?}");
    let failed: Vec<(&str, &str)> = rep.failed.iter().map(|f| (f.path.as_str(), f.error.as_str())).collect();
    assert_eq!(failed, [(dat.as_str(), "the copy it would come from is damaged too")]);
    assert_eq!(std::fs::read(b.path(&dat)).unwrap(), before, "the damaged file isn't replaced by another");
}

/// A drive whose database can't be read still has its files read; it just can't say which tracks
/// they belong to.
#[test]
fn a_drive_whose_library_cant_be_read_is_scrubbed_all_the_same() {
    let dir = tempfile::tempdir().unwrap();
    let root = common::demo_drive(dir.path(), "Main");
    std::fs::write(root.join(paths::EXPORT_PDB), b"not a database").unwrap();
    let d = Device::open(&root).unwrap();
    let r = scrub(&d, None, &ScrubOptions::default());
    assert!(r.is_clean() && r.files > 0, "{r:?}");
    assert!(r.tracks.is_empty() && r.playlists.is_empty());
    assert_eq!(r.unverified, r.files, "with nothing to compare with, every file is unverified");
}
