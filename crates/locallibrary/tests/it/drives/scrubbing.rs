//! What a scrub compares each file with, and the files a repair leaves alone.

use crate::common::{self, rot};

use std::path::Path;

use locallibrary::device::Device;
use locallibrary::onelibrary::{self, OneLibrary};
use locallibrary::player;
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

/// A drive with a OneLibrary database, written well before it's pulled into a new store.
fn pulled_onelibrary(dir: &Path) -> (Store, Device) {
    let root = common::demo_drive(dir, "Main");
    common::add_onelibrary(&root);
    common::age(&root);
    let store = Store::open_or_init(dir.join("store")).unwrap();
    let mut d = Device::open(root).unwrap();
    sync::pull(&store, &mut d, &PullOptions::default(), &Ctx::silent()).unwrap();
    (store, d)
}

fn head(store: &Store) -> Hash {
    store.libraries().unwrap()[0].head.unwrap()
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

    // The drive no longer matches a snapshot by its fingerprint, but the one it last held (its
    // databases unchanged) still says what it should hold, and so does the one named.
    let found = scrub(&d, Some(&store), &ScrubOptions::default());
    let missing: Vec<(&str, Option<ReferenceKind>)> =
        found.missing.iter().map(|m| (m.path.as_str(), m.reference)).collect();
    assert_eq!((found.snapshot, missing), (None, vec![(dat.as_str(), Some(ReferenceKind::History))]));
    let r = scrub(&d, Some(&store), &ScrubOptions { snapshot: Some(head) });
    assert_eq!(r.snapshot, Some(head));
    let missing: Vec<(&str, Option<ReferenceKind>, Option<&RepairSource>)> =
        r.missing.iter().map(|m| (m.path.as_str(), m.reference, m.repair.as_ref())).collect();
    assert_eq!(missing, [(dat.as_str(), Some(ReferenceKind::Snapshot), Some(&RepairSource::Store))]);
    assert_eq!(r.tracks.len(), 1);
    assert_eq!(r.summary(), format!("0 damaged, 0 unreadable, 1 missing of {} files", r.files));
}

/// What the snapshot a drive last held lists is no longer what belongs once rekordbox exports
/// another library to it: the files of a track it dropped aren't missing.
#[test]
fn files_an_export_since_dropped_are_not_missing() {
    let dir = tempfile::tempdir().unwrap();
    let (store, d) = pulled(dir.path());
    let gone = d.library().unwrap().tracks[0].clone();
    let pdb = d.path(paths::EXPORT_PDB);
    let mut ed = rekordbox_pdb::PdbEditor::from_file(&pdb).unwrap();
    ed.remove_tracks(&[gone.id]).unwrap();
    ed.save(&pdb).unwrap();
    for key in [&gone.file_path, &gone.analyze_path] {
        std::fs::remove_file(d.path(&paths::key(key))).unwrap();
    }

    let r = scrub(&d, Some(&store), &ScrubOptions::default());
    assert_eq!(r.snapshot, None);
    assert!(r.missing.is_empty(), "{:?}", r.missing);
}

#[test]
fn a_file_the_hash_cache_forgot_is_checked_against_an_older_snapshot() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    let root = common::demo_drive(dir.path(), "Main");
    // Written well before the pull, so the snapshot tells a rewrite since by its later mtime.
    common::age(&root);
    let mut d = Device::open(root).unwrap();
    sync::pull(&store, &mut d, &PullOptions::default(), &Ctx::silent()).unwrap();
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

/// A database rekordbox or a player saves again, at the same size, in the seconds after a pull
/// isn't damage: its modification time can't tell it from the copy the pull saw, so it's taken as
/// it is rather than "repaired" back to that copy.
#[test]
fn a_database_saved_again_just_after_a_pull_is_not_damage() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    let mut d = Device::open(common::demo_drive(dir.path(), "Main")).unwrap();
    let pdb = d.path(paths::EXPORT_PDB);
    filetime::set_file_mtime(&pdb, filetime::FileTime::now()).unwrap();
    let pulled = sync::pull(&store, &mut d, &PullOptions::default(), &Ctx::silent()).unwrap().snapshot.unwrap();
    let taken = store.load_snapshot(&pulled.id).unwrap().created.timestamp();
    let mut bytes = std::fs::read(&pdb).unwrap();
    let last = bytes.len() - 1;
    bytes[last] ^= 0xFF;
    std::fs::write(&pdb, bytes).unwrap();
    filetime::set_file_mtime(&pdb, filetime::FileTime::from_unix_time(taken, 0)).unwrap();

    let r = scrub(&d, Some(&store), &ScrubOptions::default());
    assert_eq!(r.snapshot, None);
    assert!(r.damaged.is_empty(), "{:?}", r.damaged);
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

/// Regression: the snapshot found by fingerprint was compared byte for byte, and a drive's name
/// isn't in its fingerprint, so `drive verify` called `exportLibrary.db` damaged and `--repair`
/// would have taken the name away.
#[test]
fn a_drives_own_name_and_colour_arent_damage() {
    let dir = tempfile::tempdir().unwrap();
    let (store, mut d) = pulled_onelibrary(dir.path());
    player::set(&mut d, Some("OTHER"), Some(3)).unwrap();

    for opts in [ScrubOptions::default(), ScrubOptions { snapshot: Some(head(&store)) }] {
        let r = scrub(&d, Some(&store), &opts);
        assert!(r.is_clean(), "{:?}", r.damaged);
        assert_eq!((r.snapshot, r.verified, r.unverified), (Some(head(&store)), r.files, 0));
    }
}

/// A sync that puts the target's name back rewrites `exportLibrary.db`, as `drive player` does.
#[test]
fn a_drive_a_sync_left_its_name_is_intact() {
    let dir = tempfile::tempdir().unwrap();
    let (store, mut main) = pulled_onelibrary(dir.path());
    let mut backup = Device::open(common::blank(dir.path(), "Backup")).unwrap();
    let opts = SyncOptions { delete: true, ..Default::default() };
    sync::sync_drive_to_drive(&mut main, &mut backup, &opts, &Ctx::silent()).unwrap();
    player::set(&mut backup, Some("BACKUP"), Some(6)).unwrap();

    common::export_new_track(main.root(), "Artist", "New");
    common::add_onelibrary(main.root());
    let mut main = Device::open(main.root()).unwrap();
    sync::pull(&store, &mut main, &PullOptions::default(), &Ctx::silent()).unwrap();
    sync::sync_drive_to_drive(&mut main, &mut backup, &opts, &Ctx::silent()).unwrap();

    assert_eq!(player::read(&backup).onelibrary.unwrap().name.as_deref(), Some("BACKUP"));
    let r = scrub(&backup, Some(&store), &ScrubOptions::default());
    assert_eq!(r.snapshot, Some(head(&store)));
    assert!(r.is_clean(), "{:?} {:?}", r.damaged, r.missing);
}

/// What a player does on opening a database: the library stays and the WAL goes into the main
/// file. Neither file is what the snapshot holds any more, and neither is damaged or missing.
#[test]
fn a_checkpoint_leaves_the_database_intact_and_its_wal_not_missing() {
    let dir = tempfile::tempdir().unwrap();
    let root = common::wal_drive(dir.path(), "Main");
    common::age(&root);
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    let mut d = Device::open(root).unwrap();
    sync::pull(&store, &mut d, &PullOptions::default(), &Ctx::silent()).unwrap();
    assert!(store.load_snapshot(&head(&store)).unwrap().files.iter().any(|f| f.path == paths::LIBRARY_PLUS_WAL));

    onelibrary::write(&d, &OneLibrary::open(&d).unwrap().to_encrypted().unwrap()).unwrap();
    assert!(!d.path(paths::LIBRARY_PLUS_WAL).exists());

    let r = scrub(&d, Some(&store), &ScrubOptions::default());
    assert_eq!(r.snapshot, Some(head(&store)));
    assert!(r.is_clean(), "{:?} {:?}", r.damaged, r.missing);

    // With the state changed too, the snapshot the drive last held still says what's gone, and
    // that isn't the WAL.
    let gone = paths::key(&d.library().unwrap().tracks[0].file_path);
    std::fs::remove_file(d.path(&gone)).unwrap();
    let r = scrub(&d, Some(&store), &ScrubOptions::default());
    assert_eq!(r.snapshot, None);
    let missing: Vec<&str> = r.missing.iter().map(|m| m.path.as_str()).collect();
    assert_eq!(missing, [gone.as_str()]);
    assert!(r.damaged.is_empty(), "{:?}", r.damaged);
}

/// The fingerprint alone says the drive holds the snapshot's library: its database needn't be in
/// the store, which a copy elsewhere may have taken over from.
#[test]
fn a_snapshot_vouches_for_a_database_the_store_doesnt_hold() {
    let dir = tempfile::tempdir().unwrap();
    let (store, mut d) = pulled_onelibrary(dir.path());
    let snapshot = store.load_snapshot(&head(&store)).unwrap();
    let db = snapshot.files.iter().find(|f| f.path == paths::LIBRARY_PLUS_DB).unwrap();
    std::fs::remove_file(store.object_path(&db.hash.unwrap())).unwrap();
    player::set(&mut d, Some("OTHER"), None).unwrap();

    let r = scrub(&d, Some(&store), &ScrubOptions::default());
    assert!(r.is_clean(), "{:?}", r.damaged);
}

/// Two databases that can't be decrypted don't hold the same library: nothing says what they
/// hold, so their bytes still count.
#[test]
fn damage_to_a_database_that_cant_be_decrypted_is_found() {
    let dir = tempfile::tempdir().unwrap();
    let root = common::demo_drive(dir.path(), "Main");
    std::fs::write(root.join(paths::LIBRARY_PLUS_DB), vec![7u8; 8192]).unwrap();
    common::age(&root);
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    let mut d = Device::open(root).unwrap();
    sync::pull(&store, &mut d, &PullOptions::default(), &Ctx::silent()).unwrap();

    rot(&d.path(paths::LIBRARY_PLUS_DB));
    let r = scrub(&d, Some(&store), &ScrubOptions::default());
    let damaged: Vec<&str> = r.damaged.iter().map(|p| p.path.as_str()).collect();
    assert_eq!(damaged, [paths::LIBRARY_PLUS_DB]);
}

/// Regression: a snapshot lists what belongs on the drive only while the drive holds its
/// databases, and that compared their bytes, so a drive with a name of its own lost the check.
#[test]
fn a_named_drive_still_finds_a_file_gone() {
    let dir = tempfile::tempdir().unwrap();
    let (store, mut d) = pulled_onelibrary(dir.path());
    player::set(&mut d, Some("OTHER"), None).unwrap();
    let dat = analysis_key(&d);
    std::fs::remove_file(d.path(&dat)).unwrap();

    let r = scrub(&d, Some(&store), &ScrubOptions::default());
    assert_eq!(r.snapshot, None);
    let missing: Vec<(&str, Option<ReferenceKind>)> =
        r.missing.iter().map(|m| (m.path.as_str(), m.reference)).collect();
    assert_eq!(missing, [(dat.as_str(), Some(ReferenceKind::History))]);
    assert!(r.damaged.is_empty(), "{:?}", r.damaged);
}

/// A damaged page stops a named drive's database decrypting, so nothing vouches for it.
#[test]
fn damage_to_a_named_drives_database_is_found() {
    let dir = tempfile::tempdir().unwrap();
    let (store, mut d) = pulled_onelibrary(dir.path());
    player::set(&mut d, Some("OTHER"), None).unwrap();
    common::age(d.root());
    assert!(scrub(&d, Some(&store), &ScrubOptions::default()).is_clean());

    rot(&d.path(paths::LIBRARY_PLUS_DB));
    let r = scrub(&d, Some(&store), &ScrubOptions::default());
    let damaged: Vec<(&str, Option<ReferenceKind>)> =
        r.damaged.iter().map(|p| (p.path.as_str(), p.reference)).collect();
    assert_eq!(damaged, [(paths::LIBRARY_PLUS_DB, Some(ReferenceKind::Cache))]);
    // Its bytes are in no snapshot, and only those would do.
    assert_eq!(r.damaged[0].repair, None);
}

/// Going back to a snapshot of another state with a repair rewrites `exportLibrary.db` with that
/// snapshot's name. The drive keeps its own, from the file or, if that's the damaged one, from its
/// identity.
#[test]
fn a_repair_keeps_the_drives_name() {
    for damaged in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let (store, mut d) = pulled_onelibrary(dir.path());
        let first = head(&store);
        common::export_new_track(d.root(), "Artist", "New");
        common::add_onelibrary(d.root());
        d = Device::open(d.root()).unwrap();
        player::set(&mut d, Some("OTHER"), Some(3)).unwrap();
        if damaged {
            rot(&d.path(paths::LIBRARY_PLUS_DB));
        }

        let opts = ScrubOptions { snapshot: Some(first) };
        let r = scrub(&d, Some(&store), &opts);
        let problems: Vec<FileProblem> = r.problems().cloned().collect();
        assert!(problems.iter().any(|p| p.path == paths::LIBRARY_PLUS_DB), "{problems:?}");
        let done = scrub::repair(&mut d, Some(&store), &[], &problems, &Ctx::silent()).unwrap();
        assert!(done.repaired.iter().any(|k| k == paths::LIBRARY_PLUS_DB), "{done:?}");

        let identity = player::read(&d).onelibrary.unwrap();
        assert_eq!((identity.name.as_deref(), identity.colour), (Some("OTHER"), Some(3)));
        assert!(scrub(&d, Some(&store), &opts).damaged.is_empty());
    }
}

/// A drive with a database, written well before it's pulled into a new store.
fn pulled_aged(dir: &Path) -> (Store, Device) {
    let root = common::demo_drive(dir, "Main");
    common::age(&root);
    let store = Store::open_or_init(dir.join("store")).unwrap();
    let mut d = Device::open(root).unwrap();
    sync::pull(&store, &mut d, &PullOptions::default(), &Ctx::silent()).unwrap();
    (store, d)
}

/// Regression: every scan reads the databases again and records what it finds, so a `check`
/// between the damage and `drive verify` made the damage the hash the scrub compared with.
#[test]
fn damage_a_scan_has_read_is_still_damage() {
    let dir = tempfile::tempdir().unwrap();
    let (store, d) = pulled_aged(dir.path());
    rot(&d.path(paths::EXPORT_PDB));
    d.scan_fingerprinted(None, &Ctx::silent()).unwrap();

    let r = scrub(&d, Some(&store), &ScrubOptions::default());
    let damaged: Vec<&str> = r.damaged.iter().map(|p| p.path.as_str()).collect();
    assert_eq!(damaged, [paths::EXPORT_PDB]);
    assert_eq!(r.damaged[0].reference, Some(ReferenceKind::History));
}

/// Without a store, nothing says what a file should hold, however often a scan has read it.
#[test]
fn a_scan_doesnt_vouch_for_a_file() {
    let dir = tempfile::tempdir().unwrap();
    let root = common::demo_drive(dir.path(), "Main");
    common::age(&root);
    let mut d = Device::open(root).unwrap();
    d.adopt(None).unwrap();
    d.scan_fingerprinted(None, &Ctx::silent()).unwrap();
    d.scan_fingerprinted(None, &Ctx::silent()).unwrap();

    let r = scrub(&d, None, &ScrubOptions::default());
    assert!(r.is_clean(), "{r:?}");
    assert_eq!(r.verified, 0, "{r:?}");
}

/// A scrub takes what it reads as the next scrub's reference, and a change after that is found.
#[test]
fn a_scrub_is_the_next_scrubs_reference() {
    let dir = tempfile::tempdir().unwrap();
    let root = common::demo_drive(dir.path(), "Main");
    common::age(&root);
    let mut d = Device::open(root).unwrap();
    d.adopt(None).unwrap();
    let dat = analysis_key(&d);

    let opts = ScrubOptions::default();
    let first = scrub(&d, None, &opts);
    assert_eq!((first.verified, first.unverified), (0, first.files));
    let second = scrub(&d, None, &opts);
    assert_eq!((second.verified, second.unverified), (second.files, 0));

    rot(&d.path(&dat));
    let third = scrub(&d, None, &opts);
    let damaged: Vec<&str> = third.damaged.iter().map(|p| p.path.as_str()).collect();
    assert_eq!(damaged, [dat.as_str()]);
}

/// A write knows what it wrote, so a store isn't needed to verify what it left.
#[test]
fn a_file_locallibrary_wrote_is_verified_without_a_store() {
    let dir = tempfile::tempdir().unwrap();
    let (_store, mut main) = pulled_aged(dir.path());
    let mut backup = Device::open(common::blank(dir.path(), "Backup")).unwrap();
    sync::sync_drive_to_drive(&mut main, &mut backup, &SyncOptions::default(), &Ctx::silent()).unwrap();
    let dat = analysis_key(&backup);

    let r = scrub(&backup, None, &ScrubOptions::default());
    assert!(r.is_clean() && r.verified > 0, "{r:?}");
    rot(&backup.path(&dat));
    let r = scrub(&backup, None, &ScrubOptions::default());
    assert_eq!(r.damaged.len(), 1, "{r:?}");
}
