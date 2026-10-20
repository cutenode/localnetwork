//! The store's registry of drives: the logbook, drive groups, and telling drives (and copies of
//! them) apart by their contents and their hardware.

use std::path::Path;

use crate::common;

use hegel::generators as gs;
use hegel::{HealthCheck, TestCase};
use locallibrary::device::{Device, LineageOp};
use locallibrary::error::Reason;
use locallibrary::fleet::{self, Check, Hardware, Role, Standing};
use locallibrary::progress::Ctx;
use locallibrary::store::Store;
use locallibrary::sync::{self, PullOptions, SyncOptions};

fn store(dir: &Path) -> Store {
    Store::open_or_init(dir.join("store")).unwrap()
}

/// The demo drive `name`, adopted under that name.
fn drive(dir: &Path, name: &str) -> Device {
    let mut d = Device::open(common::demo_drive(dir, name)).unwrap();
    d.adopt(Some(name)).unwrap();
    d
}

/// Pull `d` into the library `library` (`None`: its own).
fn pull(store: &Store, d: &mut Device, library: Option<&str>) {
    let opts = PullOptions { library: library.map(str::to_string), message: None, force: false };
    sync::pull(store, d, &opts, &Ctx::silent()).unwrap();
}

fn stick(serial: &str) -> Hardware {
    Hardware { model: "Cruzer Glide".into(), serial: Some(serial.into()), volume_uuid: None, size: 64_000_000_000 }
}

#[test]
fn the_logbook_keeps_ejects_checks_and_the_latest_problems() {
    let dir = tempfile::tempdir().unwrap();
    let store = store(dir.path());
    let main = drive(dir.path(), "Main");
    sync::register_drive(&store, &main, None).unwrap();
    let id = main.id().unwrap();
    fleet::record_eject(&store, id).unwrap();
    fleet::record_eject(&store, id).unwrap();
    fleet::record_check(&store, id, Check::FileSystem, false, "file system errors found").unwrap();
    fleet::record_check(&store, id, Check::Scrub, true, "all files intact").unwrap();
    for n in 0..55 {
        fleet::record_problem(&store, id, "fs_errors", format!("check {n}")).unwrap();
    }
    let log = store.drives().unwrap()[&id].log.clone();
    assert_eq!(log.clean_ejects, 2);
    let fs = log.last_fs_check.unwrap();
    assert_eq!((fs.ok, fs.summary.as_str()), (false, "file system errors found"));
    assert!(log.last_scrub.unwrap().ok);
    assert_eq!(log.problems.len(), 50);
    assert_eq!((log.problems[0].detail.as_str(), log.problems[49].detail.as_str()), ("check 5", "check 54"));

    // A drive the registry doesn't have has no logbook to write to.
    assert!(!fleet::update(&store, uuid::Uuid::nil(), |r| r.log.clean_ejects += 1).unwrap());
    assert!(fleet::record_eject(&store, uuid::Uuid::nil()).is_ok());
}

#[test]
fn drives_not_linked_to_a_library_have_no_standing() {
    let dir = tempfile::tempdir().unwrap();
    let store = store(dir.path());
    let (mut a, z) = (drive(dir.path(), "Zed"), drive(dir.path(), "alpha"));
    sync::register_drive(&store, &z, None).unwrap();
    pull(&store, &mut a, None);
    // Mounted drives first, then by name whatever its case.
    let known = fleet::known(&store, std::slice::from_ref(&a), false).unwrap();
    let names: Vec<&str> = known.iter().map(|k| k.name.as_str()).collect();
    assert_eq!(names, ["Zed", "alpha"]);
    assert_eq!(known[0].mounted.as_deref(), Some(a.root()));
    assert!(matches!(known[1].standing, Standing::Unknown), "{:?}", known[1].standing);
    let known = fleet::known(&store, &[], false).unwrap();
    assert_eq!(known.iter().map(|k| k.name.as_str()).collect::<Vec<_>>(), ["alpha", "Zed"]);
}

/// A drive last seen holding a state of its library (synced from the primary after an export,
/// saved or not) stands as the library went on since: up to date while that's its latest, behind
/// once the primary's next export is pulled on top (naming the snapshot when one holds its
/// state), and holding changes the store lacks once it gets an export of its own.
#[hegel::test(test_cases = 6, suppress_health_check = [HealthCheck::TooSlow])]
fn a_drive_stands_as_its_library_went_on_since(tc: TestCase) {
    let saved = tc.draw(gs::booleans());
    let then = tc.draw(gs::sampled_from(vec!["nothing", "primary", "own"]));
    let dir = tempfile::tempdir().unwrap();
    let store = store(dir.path());
    let mut main = drive(dir.path(), "Main");
    pull(&store, &mut main, None);
    common::export_new_track(main.root(), "Artist", "Synced");
    let mut main = Device::open_export(main.root()).unwrap();
    if saved {
        pull(&store, &mut main, None);
    }
    let mut backup = Device::open(common::blank(dir.path(), "Backup")).unwrap();
    sync::sync_drive_to_drive(&mut main, &mut backup, &SyncOptions::default(), &Ctx::silent()).unwrap();
    sync::register_drive(&store, &backup, None).unwrap();
    match then {
        "primary" => {
            common::export_new_track(main.root(), "Artist", "Pulled");
            pull(&store, &mut Device::open_export(main.root()).unwrap(), None);
        }
        "own" => {
            common::export_new_track(backup.root(), "Artist", "Own");
            let own = Device::open_export(backup.root()).unwrap();
            let fp = own.scan_fingerprinted(None, &Ctx::silent()).unwrap().fingerprint;
            sync::register_drive(&store, &own, fp).unwrap();
        }
        _ => {}
    }
    let known = fleet::known(&store, &[], false).unwrap();
    let standing = &known.iter().find(|k| k.name == "Backup").expect("Backup is registered").standing;
    match (then, saved) {
        ("nothing", true) => assert!(matches!(standing, Standing::UpToDate), "{standing:?}"),
        ("primary", _) => {
            assert!(
                matches!(standing, Standing::Behind { snapshots: 1, held } if held.is_some() == saved),
                "{standing:?}"
            )
        }
        _ => assert!(matches!(standing, Standing::Unsaved), "{standing:?}"),
    }
}

/// The name is trimmed, an empty one refused, and the registry follows.
#[test]
fn renaming_updates_the_registry() {
    let dir = tempfile::tempdir().unwrap();
    let store = store(dir.path());
    let mut main = drive(dir.path(), "Main");
    assert!(fleet::rename(Some(&store), &mut main, "  ", false).unwrap_err().to_string().contains("empty"));
    let r = fleet::rename(Some(&store), &mut main, " Club ", false).unwrap();
    assert_eq!((r.old_name.as_str(), r.name.as_str(), r.label), ("Main", "Club", None));
    assert_eq!(store.drives().unwrap()[&r.id].name, "Club");
}

/// Mounted drives are grouped by library; archive and retired ones are left out of the group
/// they belong to, and the primary leads it.
#[test]
fn drive_groups_follow_the_library_and_the_roles() {
    let dir = tempfile::tempdir().unwrap();
    let store = store(dir.path());
    let mut main = drive(dir.path(), "Main");
    pull(&store, &mut main, None);
    let library = main.state().unwrap().library_id;
    let mut members = vec![main];
    for name in ["Backup", "Vault", "Old"] {
        let mut d = drive(dir.path(), name);
        pull(&store, &mut d, Some("Main"));
        members.push(d);
    }
    let ids: Vec<uuid::Uuid> = members.iter().map(|d| d.id().unwrap()).collect();
    fleet::set_role(&store, ids[0], Some(Role::Primary)).unwrap();
    fleet::set_role(&store, ids[2], Some(Role::Archive)).unwrap();
    fleet::retire(&store, ids[3], false).unwrap();
    members.push(Device::open(dir.path()).unwrap());

    let sets = fleet::sets(&store, &members).unwrap();
    assert_eq!(sets.len(), 1, "a folder that isn't a drive is in no group");
    let set = &sets[0];
    assert_eq!((set.library_id, set.library.as_deref()), (library, Some("Main")));
    assert_eq!(set.primary.as_deref(), Some(members[0].root()));
    assert_eq!(set.members, [members[0].root(), members[1].root()]);
    assert_eq!(set.excluded, ["Vault", "Old"]);
    assert_eq!(fleet::primary_of(&store, library).unwrap().unwrap().id, ids[0]);
    assert!(fleet::primary_of(&store, None).unwrap().is_none());
}

/// Several registered drives last held the state of a drive that lost its identity: the one the
/// snapshot was taken from is the best guess. A serial number names the disk.
#[test]
fn recognising_by_content_and_by_hardware() {
    let dir = tempfile::tempdir().unwrap();
    let store = store(dir.path());
    let mut main = drive(dir.path(), "Main");
    main.state_mut().unwrap().hardware = Some(stick("4C530001"));
    main.save_state().unwrap();
    pull(&store, &mut main, None);
    let fp = store.drives().unwrap()[&main.id().unwrap()].last_fingerprint;
    let backup = drive(dir.path(), "Backup");
    sync::register_drive(&store, &backup, fp).unwrap();

    let erased = Device::open(common::demo_drive(dir.path(), "Erased")).unwrap();
    let r = fleet::recognise_with(&store, &erased, fp, None).unwrap();
    assert_eq!(r.snapshot.as_ref().map(|s| s.library.as_str()), Some("Main"));
    assert_eq!(r.best().map(|b| b.name.as_str()), Some("Main"), "the snapshot's source");
    assert!(r.drive_by_hardware.is_none());

    let r = fleet::recognise_with(&store, &erased, None, Some(&stick("4C530001"))).unwrap();
    assert_eq!(r.drive_by_hardware.as_ref().map(|b| b.name.as_str()), Some("Main"));
    assert_eq!(r.best().map(|b| b.name.as_str()), Some("Main"));
    let r = fleet::recognise_with(&store, &erased, None, Some(&stick("4C530002"))).unwrap();
    assert!(r.is_empty() && r.best().is_none());
}

#[test]
fn adopting_a_known_identity() {
    let dir = tempfile::tempdir().unwrap();
    let store = store(dir.path());
    let mut main = drive(dir.path(), "Main");
    pull(&store, &mut main, None);

    // Not while the drive holding it is mounted elsewhere.
    let mut copy = Device::open(common::demo_drive(dir.path(), "Copy")).unwrap();
    let err = fleet::adopt_as(&store, &mut copy, "Main", std::slice::from_ref(&main), &Ctx::silent()).unwrap_err();
    assert_eq!(err.reason(), Some(Reason::IdentityInUse), "{err}");

    // A state the store never saw is recorded as observed, on top of nothing.
    common::export_new_track(copy.root(), "Artist", "Since");
    let mut copy = Device::open(copy.root()).unwrap();
    let info = fleet::adopt_as(&store, &mut copy, "Main", &[], &Ctx::silent()).unwrap();
    assert_eq!(info.id, main.id());
    let lineage = &copy.state().unwrap().lineage;
    assert_eq!(lineage.iter().map(|e| e.op).collect::<Vec<_>>(), [LineageOp::Observed]);
    assert_eq!(lineage[0].tracks, 3);

    // A drive with no library takes the identity alone.
    std::fs::create_dir(dir.path().join("Empty")).unwrap();
    let mut empty = Device::open(dir.path().join("Empty")).unwrap();
    fleet::adopt_as(&store, &mut empty, "Main", &[], &Ctx::silent()).unwrap();
    assert!(empty.state().unwrap().lineage.is_empty());
}

/// Two mounted drives claim one identity: the one on the disk the identity was given on is the
/// original, when exactly one is.
#[test]
fn the_original_of_two_copies_is_on_the_recorded_disk() {
    let dir = tempfile::tempdir().unwrap();
    let mut a = drive(dir.path(), "Main");
    a.state_mut().unwrap().hardware = Some(stick("4C530001"));
    a.save_state().unwrap();
    common::copy_dir(a.root(), &dir.path().join("Copy"));
    let copy = Device::open(dir.path().join("Copy")).unwrap();
    let mounted = vec![Device::open(a.root()).unwrap(), copy];
    let (a_root, copy_root) = (mounted[0].root().to_path_buf(), mounted[1].root().to_path_buf());

    let on_disks = |root: &Path| Some(stick(if root == a_root { "4C530001" } else { "4C530002" }));
    let dups = fleet::duplicates(&mounted, &on_disks);
    assert_eq!(dups[0].original.as_deref(), Some(a_root.as_path()));
    assert_eq!(dups[0].roots, [a_root.clone(), copy_root]);
    // Both on the recorded disk (cheap sticks share serials): no telling.
    assert_eq!(fleet::duplicates(&mounted, &|_| Some(stick("4C530001")))[0].original, None);
    // The copy, given an identity of its own, is registered under it.
    let store = store(dir.path());
    let mut copy = Device::open(dir.path().join("Copy")).unwrap();
    let info = fleet::reidentify(Some(&store), &mut copy, Some("Spare")).unwrap();
    assert_eq!(store.drives().unwrap()[&info.id.unwrap()].name, "Spare");
}

/// A library has one primary: a primary pulled into a library that has one becomes a backup there,
/// and the library's primary stays.
#[test]
fn a_primary_joining_a_library_with_one_becomes_a_backup() {
    let dir = tempfile::tempdir().unwrap();
    let store = store(dir.path());
    let mut main = drive(dir.path(), "Main");
    pull(&store, &mut main, None);
    let mut other = drive(dir.path(), "Other");
    pull(&store, &mut other, None);
    let (main_id, other_id) = (main.id().unwrap(), other.id().unwrap());
    fleet::set_role(&store, main_id, Some(Role::Primary)).unwrap();
    fleet::set_role(&store, other_id, Some(Role::Primary)).unwrap();

    pull(&store, &mut other, Some("Main"));
    let drives = store.drives().unwrap();
    assert_eq!((drives[&main_id].role, drives[&other_id].role), (Some(Role::Primary), Some(Role::Backup)));
    assert_eq!(fleet::primary_of(&store, main.state().unwrap().library_id).unwrap().unwrap().id, main_id);
}
