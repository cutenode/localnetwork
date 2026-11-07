//! Syncs, pulls and pushes that can't do what they must: a drive or store folder that can't be
//! written, a state file or store list that doesn't parse, a store file a folder stands in for, a
//! target folder that leads off the drive, a file that can't be read or removed, a snapshot whose
//! files have no hash. Each stops with the error that says so, the target's library as it was
//! unless the error came after the new one went in. Unix only: read-only folders and links stand
//! in for what can't be written or leads off the drive.
#![cfg(unix)]

use crate::common;

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use hegel::generators as gs;
use hegel::{HealthCheck, TestCase};
use locallibrary::device::Device;
use locallibrary::paths::{self, FileClass};
use locallibrary::progress::Ctx;
use locallibrary::store::Store;
use locallibrary::sync::{self, PullOptions, PushOptions, Source, SyncOptions};

fn set_mode(p: &Path, mode: u32) {
    fs::set_permissions(p, fs::Permissions::from_mode(mode)).unwrap();
}

/// The drive's state folder, holding a lock file (so it can still be locked), made read-only.
fn freeze_state(root: &Path) -> PathBuf {
    let dir = root.join(paths::STATE_DIR);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("lock"), b"").unwrap();
    set_mode(&dir, 0o555);
    dir
}

/// Adopt the drive at `root`, its files aged so its hash cache vouches for them, and save that
/// cache: scanning it again writes nothing.
fn settle(root: &Path) {
    common::age(root);
    let mut d = Device::open(root).unwrap();
    d.adopt(None).unwrap();
    let lib = d.library().ok();
    d.scan_fingerprinted(lib.as_ref(), &Ctx::silent()).unwrap();
}

/// What stands in a sync's way.
#[derive(Debug, Clone, Copy)]
enum Unwritable {
    /// The source's root can't be written: its state folder can't be made to lock it.
    SourceRoot,
    /// The target's root can't be written.
    TargetRoot,
    /// The source's state file doesn't parse when it's read again, once locked.
    SourceState,
    /// The source isn't adopted and its state folder can't be written: adopting it fails.
    SourceUnadopted,
    /// The source is adopted and its state folder can't be written: recording what it holds
    /// fails, or (verifying) saving the hashes it took.
    SourceAdopted,
    /// The target is adopted and its state folder can't be written: marking it as being written
    /// fails, or (verifying) saving the hashes it took.
    TargetAdopted,
}

/// A sync, to one drive or several, whose source or target can't be written or whose state
/// doesn't parse is refused with the error that says so, and the target's library stays as it
/// was.
#[hegel::test(test_cases = 2, suppress_health_check = [HealthCheck::TooSlow])]
fn prop_a_sync_that_cant_write_a_drives_state_leaves_the_target_as_it_was(tc: TestCase) {
    use Unwritable::*;
    let target_has_library = tc.draw(gs::booleans());
    for case in [SourceRoot, TargetRoot, SourceState, SourceUnadopted, SourceAdopted, TargetAdopted] {
        for many in [false, true] {
            for verify in [false, true] {
                let dir = tempfile::tempdir().unwrap();
                let a = common::demo_drive(dir.path(), "A");
                let b = if target_has_library || matches!(case, TargetAdopted) {
                    common::bigger_drive(dir.path(), "B")
                } else {
                    common::blank(dir.path(), "B")
                };
                let mut frozen = Vec::new();
                match case {
                    SourceRoot | TargetRoot => frozen.push(if matches!(case, SourceRoot) { &a } else { &b }.clone()),
                    SourceState => {}
                    SourceUnadopted => frozen.push(freeze_state(&a)),
                    SourceAdopted => {
                        settle(&a);
                        frozen.push(freeze_state(&a));
                    }
                    TargetAdopted => {
                        settle(&b);
                        frozen.push(freeze_state(&b));
                    }
                }
                let mut source = Device::open(&a).unwrap();
                let mut target = Device::open(&b).unwrap();
                if matches!(case, SourceState) {
                    fs::create_dir_all(a.join(paths::STATE_DIR)).unwrap();
                    fs::write(a.join(paths::STATE_DIR).join("state.json"), b"{ not json").unwrap();
                }
                for f in &frozen {
                    set_mode(f, 0o555);
                }
                let before = common::library_files(&b);
                let opts = SyncOptions { verify, ..SyncOptions::default() };
                let got = if many {
                    sync::sync_drive_to_many(&mut source, std::slice::from_mut(&mut target), &opts, &Ctx::silent())
                        .and_then(|mut r| r.remove(0))
                } else {
                    sync::sync_drive_to_drive(&mut source, &mut target, &opts, &Ctx::silent())
                };
                for f in &frozen {
                    set_mode(f, 0o755);
                }
                let e = got.expect_err(&format!("{case:?} (many {many}, verify {verify})"));
                let want = if matches!(case, SourceState) { "parse" } else { "io" };
                assert_eq!(e.code(), want, "{case:?} (many {many}, verify {verify}): {e}");
                assert_eq!(common::library_files(&b), before, "{case:?}: the target's library is as it was");
            }
        }
    }
}

/// What stands in a pull's way.
#[derive(Debug, Clone, Copy)]
enum PullBlock {
    /// The drive's root can't be written: it can't be locked.
    DriveRoot,
    /// The drive's state file doesn't parse when it's read again, once locked.
    DriveState,
    /// The drive isn't adopted and its state folder can't be written.
    DriveUnadopted,
    /// The drive is adopted and its state folder can't be written: its hashes can't be saved, or
    /// (pulled before, and up to date) its link to the library.
    DriveAdopted { again: bool },
    /// The store's drive registry is held by a folder: the drive can't be recorded there.
    Registry { again: bool },
    /// The library head's snapshot file doesn't parse.
    Head,
    /// The store's list of libraries doesn't parse: the pull names a library, the drive is linked
    /// to one, or neither.
    Libraries { by: Lib },
    /// The store's root can't be written (its objects and snapshots can): no library can be made
    /// (named, linked to one this store lacks, or neither), nor a head set.
    StoreRoot { into: Lib },
    /// The store's snapshots folder can't be written.
    Snapshots,
}

#[derive(Debug, Clone, Copy)]
enum Lib {
    Named,
    Linked,
    Unlinked,
    Existing,
}

/// A pull that can't lock or record the drive, or can't read or write what the store needs, is
/// refused with the error that says so; the store's head stays where it was.
#[hegel::test(test_cases = 2, suppress_health_check = [HealthCheck::TooSlow])]
fn prop_a_pull_that_cant_write_the_drive_or_the_store_is_refused(tc: TestCase) {
    use PullBlock::*;
    let message = tc.draw(gs::sampled_from(vec![None, Some("pulled".to_string())]));
    let cases = [
        DriveRoot,
        DriveState,
        DriveUnadopted,
        DriveAdopted { again: false },
        DriveAdopted { again: true },
        Registry { again: false },
        Registry { again: true },
        Head,
        Libraries { by: Lib::Named },
        Libraries { by: Lib::Linked },
        Libraries { by: Lib::Unlinked },
        StoreRoot { into: Lib::Named },
        StoreRoot { into: Lib::Linked },
        StoreRoot { into: Lib::Unlinked },
        StoreRoot { into: Lib::Existing },
        Snapshots,
    ];
    for case in cases {
        let dir = tempfile::tempdir().unwrap();
        let store_root = dir.path().join("store");
        let store = Store::open_or_init(&store_root).unwrap();
        drop(store.lock().unwrap());
        let root = common::bigger_drive(dir.path(), "A");
        let pull = |opts: &PullOptions| sync::pull(&store, &mut Device::open(&root).unwrap(), opts, &Ctx::silent());
        let mut opts = PullOptions { message: message.clone(), ..Default::default() };
        let mut frozen = Vec::new();
        match case {
            DriveRoot => frozen.push(root.clone()),
            DriveState => {}
            DriveUnadopted => frozen.push(freeze_state(&root)),
            DriveAdopted { again } => {
                if again {
                    pull(&opts).unwrap();
                }
                settle(&root);
                frozen.push(freeze_state(&root));
            }
            Registry { again } => {
                if again {
                    pull(&opts).unwrap();
                }
                fs::remove_file(store_root.join("drives.lock")).ok();
                fs::create_dir_all(store_root.join("drives.lock/held")).unwrap();
            }
            Head => {
                pull(&opts).unwrap();
                for e in fs::read_dir(store_root.join("snapshots")).unwrap() {
                    fs::write(e.unwrap().path(), b"{").unwrap();
                }
            }
            Libraries { by } => {
                pull(&opts).unwrap();
                fs::write(store_root.join("libraries.json"), b"{").unwrap();
                match by {
                    Lib::Named => opts.library = Some("Elsewhere".into()),
                    Lib::Unlinked => fs::remove_dir_all(root.join(paths::STATE_DIR)).unwrap(),
                    _ => {}
                }
            }
            StoreRoot { into } => {
                match into {
                    Lib::Named => opts.library = Some("New".into()),
                    Lib::Linked => {
                        // Linked to a library of another store.
                        let other = Store::open_or_init(dir.path().join("other")).unwrap();
                        sync::pull(&other, &mut Device::open(&root).unwrap(), &opts, &Ctx::silent()).unwrap();
                    }
                    Lib::Unlinked => {}
                    Lib::Existing => {
                        pull(&opts).unwrap();
                        common::export_new_track(&root, "Later", "Track");
                    }
                }
                fs::create_dir_all(store_root.join("objects/tmp")).unwrap();
                fs::create_dir_all(store_root.join("snapshots")).unwrap();
                frozen.push(store_root.clone());
            }
            Snapshots => {
                pull(&opts).unwrap();
                common::export_new_track(&root, "Later", "Track");
                frozen.push(store_root.join("snapshots"));
            }
        }
        let heads = || store.libraries().ok().map(|l| l.iter().filter_map(|l| l.head).collect::<Vec<_>>());
        let before = heads();
        for f in &frozen {
            set_mode(f, 0o555);
        }
        let mut d = Device::open(&root).unwrap();
        if matches!(case, DriveState) {
            // Read once the drive is locked.
            fs::create_dir_all(root.join(paths::STATE_DIR)).unwrap();
            fs::write(root.join(paths::STATE_DIR).join("state.json"), b"[").unwrap();
        }
        let got = sync::pull(&store, &mut d, &opts, &Ctx::silent());
        for f in &frozen {
            set_mode(f, 0o755);
        }
        let e = got.expect_err(&format!("{case:?}"));
        let want = match case {
            DriveState | Head | Libraries { .. } => "parse",
            _ => "io",
        };
        assert_eq!(e.code(), want, "{case:?}: {e}");
        if !matches!(case, Registry { .. }) {
            assert_eq!(heads(), before, "{case:?}: no library or head changed");
        }
    }
}

/// What stands in a push's way.
#[derive(Debug, Clone, Copy)]
enum PushBlock {
    /// The store's list of libraries doesn't parse: the snapshot to push can't be found, by the
    /// library named, the drive's own, or the store's only one; nor told saved or not.
    Libraries { by: Lib },
    /// A folder holds the store's lock file.
    StoreLock,
    /// The drive's root can't be written.
    DriveRoot,
    /// The drive changed since the plan the user confirmed.
    Stale,
    /// A folder holds the store's drive registry's lock: the drive can't be recorded there.
    Registry,
}

/// A push that can't find its snapshot, lock the store or the drive, record the drive, or
/// whose plan is stale is refused with the error that says so; refused before writing, the drive
/// holds what it held.
#[hegel::test(test_cases = 2, suppress_health_check = [HealthCheck::TooSlow])]
fn prop_a_push_that_cant_read_or_lock_what_it_needs_is_refused(tc: TestCase) {
    use PushBlock::*;
    let force = tc.draw(gs::booleans());
    let cases = [
        Libraries { by: Lib::Named },
        Libraries { by: Lib::Linked },
        Libraries { by: Lib::Unlinked },
        Libraries { by: Lib::Existing },
        StoreLock,
        DriveRoot,
        Stale,
        Registry,
    ];
    for case in cases {
        let dir = tempfile::tempdir().unwrap();
        let store_root = dir.path().join("store");
        let store = Store::open_or_init(&store_root).unwrap();
        let a = common::demo_drive(dir.path(), "A");
        let pulled =
            sync::pull(&store, &mut Device::open(&a).unwrap(), &PullOptions::default(), &Ctx::silent()).unwrap();
        let snap = store.load_snapshot(&pulled.snapshot.unwrap().id).unwrap();
        let b = match case {
            Libraries { by: Lib::Linked } => a.clone(),
            Libraries { by: Lib::Existing } => common::bigger_drive(dir.path(), "B"),
            _ => common::blank(dir.path(), "B"),
        };
        let mut d = Device::open(&b).unwrap();
        let opts = PushOptions {
            force,
            library: matches!(case, Libraries { by: Lib::Named }).then(|| pulled.library.clone()),
            ..Default::default()
        };
        let before = common::library_files(&b);
        let mut frozen = Vec::new();
        let got = match case {
            Libraries { by } => {
                fs::write(store_root.join("libraries.json"), b"{").unwrap();
                match by {
                    Lib::Existing => sync::plan_push(&store, &snap, &d, &PushOptions::default(), &Ctx::silent()),
                    _ => sync::resolve_push_snapshot(&store, &d, &opts).map(|_| unreachable!()),
                }
            }
            StoreLock | Registry => {
                let held = store_root.join(if matches!(case, StoreLock) { "lock" } else { "drives.lock" });
                fs::remove_file(&held).ok();
                fs::create_dir_all(held.join("held")).unwrap();
                sync::push(&store, &snap, &mut d, &opts, &Ctx::silent()).map(|_| unreachable!())
            }
            DriveRoot => {
                frozen.push(b.clone());
                set_mode(&b, 0o555);
                sync::push(&store, &snap, &mut d, &opts, &Ctx::silent()).map(|_| unreachable!())
            }
            Stale => {
                let plan = sync::plan_push(&store, &snap, &d, &opts, &Ctx::silent()).unwrap();
                fs::create_dir_all(b.join("Contents")).unwrap();
                fs::write(b.join("Contents/Late.mp3"), b"late").unwrap();
                sync::push_as_planned(&store, &snap, &mut d, &opts, &plan, &Ctx::silent()).map(|_| unreachable!())
            }
        };
        for f in &frozen {
            set_mode(f, 0o755);
        }
        let e = got.expect_err(&format!("{case:?}"));
        let want = match case {
            Libraries { .. } => "parse",
            Stale => "stale_plan",
            _ => "io",
        };
        assert_eq!(e.code(), want, "{case:?}: {e}");
        match case {
            Registry => assert_eq!(common::library_files(&b), common::library_files(&a), "the push went in first"),
            Stale => assert!(Device::open(&b).unwrap().library().is_err(), "nothing went in"),
            _ => assert_eq!(common::library_files(&b), before, "{case:?}: the drive holds what it held"),
        }
    }
}

/// A snapshot whose file has no hash can't say where in the store its content is: pushing it is
/// refused naming the file, whether the file is copied (audio or a database) or compared with the
/// target's copy of it, written moments ago (a database, never taken by size and time, must be
/// hashed to be compared).
#[hegel::test(test_cases = 3, suppress_health_check = [HealthCheck::TooSlow])]
fn prop_a_snapshot_file_with_no_hash_stops_the_push_naming_it(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    let a = common::demo_drive(dir.path(), "A");
    let pulled = sync::pull(&store, &mut Device::open(&a).unwrap(), &PullOptions::default(), &Ctx::silent()).unwrap();
    let snap = store.load_snapshot(&pulled.snapshot.unwrap().id).unwrap();
    for (n, class) in [FileClass::Audio, FileClass::Database].into_iter().enumerate() {
        for compared in [false, true] {
            let b = common::blank(dir.path(), &format!("B{n}{compared}"));
            let mut d = Device::open(&b).unwrap();
            if compared {
                // The target got the snapshot moments ago: its files' times say nothing of them.
                sync::push(&store, &snap, &mut d, &PushOptions::default(), &Ctx::silent()).unwrap();
            }
            let of_class: Vec<usize> = (0..snap.files.len()).filter(|i| snap.files[*i].class == class).collect();
            let which = tc.draw(gs::sampled_from(of_class));
            let mut unhashed = snap.clone();
            unhashed.files[which].hash = None;
            let path = unhashed.files[which].path.clone();
            let before = common::library_files(&b);
            let e = sync::push(&store, &unhashed, &mut d, &PushOptions::default(), &Ctx::silent())
                .expect_err(&format!("{path} (compared {compared})"));
            assert_eq!(e.code(), "invalid", "{e}");
            // A database is never taken as it is by size and time: compared, it must be hashed.
            let said = match (class, compared) {
                (FileClass::Database, true) => format!("{path} must be hashed on both sides"),
                _ => format!("{path} has no hash"),
            };
            assert!(e.to_string().contains(&said), "{e}");
            if compared {
                assert_eq!(common::library_files(&b), before, "{path}: refused as it's planned");
            } else if class == FileClass::Database {
                assert!(!Device::open(&b).unwrap().has_export(), "{path}: the databases never went in");
            }
        }
    }
}

/// What stands in the way of writing the target's files.
#[derive(Debug, Clone, Copy)]
enum Blocked {
    /// A stale file the sync deletes (before copying, or after) sits in a folder that can't be
    /// written.
    StaleInLockedFolder { first: bool },
    /// With `delete_first`, the old databases come off first, from a folder that can't be written.
    DatabasesLocked,
    /// The target's `Contents/` is a link to a folder off the drive.
    ContentsLinked,
    /// The target's database folder is a link to a folder off the drive.
    DatabaseFolderLinked,
    /// The target's file, copied there moments ago, can't be read to compare it.
    TargetUnreadable,
    /// The source's file can't be read to compare it with the target's copy, made moments ago.
    SourceUnreadable,
}

/// A sync that can't remove, reach or read what it must on the target (or read the source's
/// file) is refused with the error that says so, and nothing is written off the drive.
#[hegel::test(test_cases = 2, suppress_health_check = [HealthCheck::TooSlow])]
fn prop_a_sync_stops_at_a_file_it_cant_remove_reach_or_read(tc: TestCase) {
    use Blocked::*;
    let verify = tc.draw(gs::booleans());
    let cases = [
        StaleInLockedFolder { first: false },
        StaleInLockedFolder { first: true },
        DatabasesLocked,
        ContentsLinked,
        DatabaseFolderLinked,
        TargetUnreadable,
        SourceUnreadable,
    ];
    for case in cases {
        let dir = tempfile::tempdir().unwrap();
        let a = common::demo_drive(dir.path(), "A");
        let b = common::blank(dir.path(), "B");
        let off = common::blank(dir.path(), "off the drive");
        let mut source = Device::open_export(&a).unwrap();
        let mut opts = SyncOptions::default();
        let mut frozen = Vec::new();
        let synced = |source: &mut Device| {
            let mut t = Device::open(&b).unwrap();
            sync::sync_drive_to_drive(source, &mut t, &SyncOptions::default(), &Ctx::silent()).unwrap();
        };
        match case {
            StaleInLockedFolder { first } => {
                synced(&mut source);
                opts.delete_first = first;
                fs::create_dir_all(b.join("Contents/Locked")).unwrap();
                fs::write(b.join("Contents/Locked/stale.mp3"), b"stale").unwrap();
                frozen.push((b.join("Contents/Locked"), 0o555));
            }
            DatabasesLocked => {
                synced(&mut source);
                fs::write(b.join("Contents/stale.mp3"), b"stale").unwrap();
                opts.delete_first = true;
                frozen.push((b.join("PIONEER/rekordbox"), 0o555));
            }
            ContentsLinked => std::os::unix::fs::symlink(&off, b.join("Contents")).unwrap(),
            DatabaseFolderLinked => {
                fs::create_dir_all(b.join("PIONEER")).unwrap();
                std::os::unix::fs::symlink(&off, b.join("PIONEER/rekordbox")).unwrap();
            }
            TargetUnreadable | SourceUnreadable => {
                synced(&mut source);
                let audio = source.library().unwrap().tracks[0].file_path.clone();
                let on = if matches!(case, TargetUnreadable) { &b } else { &a };
                frozen.push((paths::to_fs(on, &paths::key(&audio)), 0o000));
            }
        }
        if !matches!(case, TargetUnreadable | SourceUnreadable) {
            opts.verify = verify;
        }
        for (f, mode) in &frozen {
            set_mode(f, *mode);
        }
        let got = sync::sync_drive_to_drive(&mut source, &mut Device::open(&b).unwrap(), &opts, &Ctx::silent());
        for (f, _) in &frozen {
            set_mode(f, 0o755);
        }
        let e = got.expect_err(&format!("{case:?}"));
        let want = match case {
            ContentsLinked | DatabaseFolderLinked => "invalid",
            _ => "io",
        };
        assert_eq!(e.code(), want, "{case:?}: {e}");
        if want == "invalid" {
            assert!(e.to_string().contains("resolves outside"), "{case:?}: {e}");
        }
        assert!(common::drive_files(&off).is_empty(), "{case:?}: nothing written off the drive");
    }
}

/// A sync to one drive or several is refused when the target changed since the plan the user
/// confirmed, or the source's last write never finished; neither writes the target.
#[hegel::test(test_cases = 2, suppress_health_check = [HealthCheck::TooSlow])]
fn prop_a_stale_plan_or_a_half_written_source_is_refused(tc: TestCase) {
    let verify = tc.draw(gs::booleans());
    for unfinished in [false, true] {
        for many in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let mut source = Device::open_export(common::demo_drive(dir.path(), "A")).unwrap();
            let b = common::blank(dir.path(), "B");
            let opts = SyncOptions { verify, ..SyncOptions::default() };
            let mut target = Device::open(&b).unwrap();
            let (plan, _) = sync::plan_drive_to_drive(&source, &target, &opts, &Ctx::silent()).unwrap();
            if unfinished {
                source.adopt(None).unwrap();
                source.begin(locallibrary::device::LineageOp::Sync, "a sync cut off", None).unwrap();
            } else {
                fs::create_dir_all(b.join("Contents")).unwrap();
                fs::write(b.join("Contents/Late.mp3"), b"late").unwrap();
            }
            let before = common::drive_files(&b);
            let got = if many {
                sync::sync_drive_to_many_as_planned(
                    &mut source,
                    std::slice::from_mut(&mut target),
                    &opts,
                    std::slice::from_ref(&plan),
                    &Ctx::silent(),
                )
                .and_then(|mut r| r.remove(0))
            } else {
                sync::sync_drive_to_drive_as_planned(&mut source, &mut target, &opts, &plan, &Ctx::silent())
            };
            let e = got.expect_err(&format!("unfinished {unfinished}, many {many}"));
            let want = if unfinished { Some(locallibrary::error::Reason::Interrupted) } else { None };
            assert_eq!(e.reason(), want, "{e}");
            assert_eq!(e.code(), if unfinished { "conflict" } else { "stale_plan" }, "{e}");
            assert_eq!(common::drive_files(&b), before);
        }
    }
}

/// A source keeping audio at `Music/Set.mp3`, outside `Contents/`.
fn with_audio_outside_contents(dir: &Path) -> PathBuf {
    let root = common::bigger_drive(dir, "A");
    common::add_track(&root, rekordbox_pdb::NewTrack::new("Set", "/Music/Set.mp3"), b"A's audio");
    root
}

/// Where the source keeps audio outside `Contents/`, the target's file there is read to compare
/// it (verifying) or its folder resolved: one that can't be read, or a folder that leads off the
/// drive, stops the plan with the error that says so.
#[hegel::test(test_cases = 2, suppress_health_check = [HealthCheck::TooSlow])]
fn prop_a_file_outside_the_library_that_cant_be_read_or_reached_stops_the_plan(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let a = Device::open_export(with_audio_outside_contents(dir.path())).unwrap();
    let theirs = tc.draw(gs::sampled_from(vec![&b"B's audio"[..], &b"A's audio"[..]]));
    for linked in [false, true] {
        let b = common::blank(dir.path(), &format!("B{linked}"));
        let off = common::blank(dir.path(), &format!("off the drive {linked}"));
        let set = b.join("Music/Set.mp3");
        if linked {
            std::os::unix::fs::symlink(&off, b.join("Music")).unwrap();
        } else {
            fs::create_dir_all(set.parent().unwrap()).unwrap();
            fs::write(&set, theirs).unwrap();
            set_mode(&set, 0o000);
        }
        let opts = SyncOptions { verify: true, ..SyncOptions::default() };
        let got = sync::plan_drive_to_drive(&a, &Device::open(&b).unwrap(), &opts, &Ctx::silent());
        if !linked {
            set_mode(&set, 0o644);
        }
        let e = got.unwrap_err();
        assert_eq!(e.code(), if linked { "invalid" } else { "io" }, "linked {linked}: {e}");
    }
}

/// The file a sync left outside `Contents/` moments ago, which a scan of the target that doesn't
/// look for it doesn't list: its size and time say nothing of it, so it's compared by content,
/// and taken as it is when it's the same; when it isn't, it's the DJ's and the plan is refused.
#[hegel::test(test_cases = 3, suppress_health_check = [HealthCheck::TooSlow])]
fn prop_a_file_outside_the_library_written_moments_ago_is_compared_by_content(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let mut a = Device::open_export(with_audio_outside_contents(dir.path())).unwrap();
    let b = common::blank(dir.path(), "B");
    sync::sync_drive_to_drive(&mut a, &mut Device::open(&b).unwrap(), &SyncOptions::default(), &Ctx::silent()).unwrap();
    let rewritten = tc.draw(gs::booleans());
    let set = b.join("Music/Set.mp3");
    if rewritten {
        // Rewritten in place: the same size and time, other bytes.
        let mtime = filetime::FileTime::from_last_modification_time(&fs::metadata(&set).unwrap());
        fs::write(&set, b"B's audio").unwrap();
        filetime::set_file_mtime(&set, mtime).unwrap();
    }
    let tree = a.scan_fingerprinted(a.library().ok().as_ref(), &Ctx::silent()).unwrap().tree;
    let unlisted = locallibrary::tree::scan(&b, &Default::default(), &Ctx::silent()).unwrap();
    assert!(unlisted.resolve("Music/Set.mp3").is_none());
    let target = Device::open(&b).unwrap();
    let got = sync::plan(&Source::Drive { device: &a, tree: &tree }, &target, &unlisted, &SyncOptions::default());
    match got {
        Ok(plan) => {
            assert!(!rewritten);
            assert!(!plan.copies.iter().any(|c| c.path == "Music/Set.mp3"), "taken as it is");
        }
        Err(e) => {
            assert!(rewritten, "{e}");
            assert_eq!(e.reason(), Some(locallibrary::error::Reason::InTheWay), "{e}");
        }
    }
}

/// A plan for a target whose free space can't be told (its folder gone) can't say it won't fit:
/// it fits, and warns of nothing about space.
#[hegel::test(test_cases = 3, suppress_health_check = [HealthCheck::TooSlow])]
fn prop_a_target_whose_free_space_cant_be_told_fits(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let a = Device::open_export(common::demo_drive(dir.path(), "A")).unwrap();
    let tree = a.scan_fingerprinted(a.library().ok().as_ref(), &Ctx::silent()).unwrap().tree;
    let b = common::blank(dir.path(), "B");
    let target = Device::open(&b).unwrap();
    let empty = locallibrary::tree::scan(&b, &Default::default(), &Ctx::silent()).unwrap();
    fs::remove_dir(&b).unwrap();
    let opts = SyncOptions { delete_first: tc.draw(gs::booleans()), ..SyncOptions::default() };
    let plan = sync::plan(&Source::Drive { device: &a, tree: &tree }, &target, &empty, &opts).unwrap();
    assert_eq!(plan.free_space, None);
    assert!(plan.fits);
    assert_eq!(plan.copies.len(), tree.library_entries().count());
    assert!(!plan.warnings.iter().any(|w| w.contains("free")), "{:?}", plan.warnings);
}

/// A drive that was never adopted has nothing to record in the store's registry: registering it
/// leaves the registry as it was.
#[hegel::test(test_cases = 3)]
fn prop_a_drive_never_adopted_isnt_registered(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    let d = Device::open(common::blank(dir.path(), "A")).unwrap();
    let fp = tc.draw(gs::booleans()).then(|| locallibrary::hash::Hash::of(b"a state"));
    sync::register_drive(&store, &d, fp).unwrap();
    assert!(store.drives().unwrap().is_empty());
}

/// A file of the target's own state that a whole new library makes stale (its pack record and
/// transcodes, its collection export record, its recorded edits) is forgotten once the library is
/// in; when a folder stands where the file goes, the sync or push says so, the new library in.
#[hegel::test(test_cases = 2, suppress_health_check = [HealthCheck::TooSlow])]
fn prop_a_folder_where_a_stale_state_file_goes_stops_the_copy_once_the_library_is_in(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    let a = common::demo_drive(dir.path(), "A");
    let pulled = sync::pull(&store, &mut Device::open(&a).unwrap(), &PullOptions::default(), &Ctx::silent()).unwrap();
    let snap = store.load_snapshot(&pulled.snapshot.unwrap().id).unwrap();
    let verify = tc.draw(gs::booleans());
    for (n, file) in ["pack.json", "transcodes.json", "collection.json", "edits.json"].into_iter().enumerate() {
        for push in [false, true] {
            let b = common::blank(dir.path(), &format!("B{n}{push}"));
            fs::create_dir_all(b.join(paths::STATE_DIR).join(file).join("in the way")).unwrap();
            let mut d = Device::open(&b).unwrap();
            let got = if push {
                let opts = PushOptions { sync: SyncOptions { verify, ..SyncOptions::default() }, ..Default::default() };
                sync::push(&store, &snap, &mut d, &opts, &Ctx::silent())
            } else {
                let opts = SyncOptions { verify, ..SyncOptions::default() };
                sync::sync_drive_to_drive(&mut Device::open(&a).unwrap(), &mut d, &opts, &Ctx::silent())
            };
            let e = got.expect_err(&format!("{file} (push {push})"));
            assert_eq!(e.code(), "io", "{file}: {e}");
            assert!(e.to_string().contains(file), "{file}: {e}");
            assert_eq!(common::library_files(&b), common::library_files(&a), "{file}: the library is in");
        }
    }
}
