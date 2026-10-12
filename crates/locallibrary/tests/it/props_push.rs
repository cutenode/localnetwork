//! Pushing snapshots onto drives, with drawn histories of pulls and drawn changes on the drive
//! pushed to: a push writes the snapshot asked for exactly, never overwrites a state the store
//! hasn't saved unless forced, and a drive the library moved on from is saved beside the head
//! without moving it.

use crate::common;

use hegel::generators as gs;
use hegel::{HealthCheck, TestCase};
use locallibrary::Error;
use locallibrary::device::Device;
use locallibrary::progress::Ctx;
use locallibrary::store::Store;
use locallibrary::sync::{self, PullOptions, PushOptions};

#[hegel::test(test_cases = 6, suppress_health_check = [HealthCheck::TooSlow])]
fn a_push_writes_the_snapshot_and_never_loses_unsaved_work(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    let main_root = common::bigger_drive(dir.path(), "Main");
    let mut main = Device::open_export(&main_root).unwrap();
    main.adopt(Some("Main")).unwrap();
    // Snapshots: the first pull, then new exports pulled one by one.
    let mut states = Vec::new();
    for n in 0..tc.draw(gs::integers::<usize>().min_value(1).max_value(3)) {
        if n > 0 {
            common::export_new_track(&main_root, "New", &format!("Export {n}"));
        }
        let mut d = Device::open_export(&main_root).unwrap();
        let r = sync::pull(&store, &mut d, &PullOptions::default(), &Ctx::silent()).unwrap();
        states.push((r.snapshot.unwrap().id, common::library_files(&main_root)));
    }

    // A backup drive gets one of them.
    let backup_root = common::blank(dir.path(), "Backup");
    let mut backup = Device::open(&backup_root).unwrap();
    let pick = tc.draw(gs::integers::<usize>().max_value(states.len() - 1));
    let (snap_id, files) = &states[pick];
    let snapshot = store.load_snapshot(snap_id).unwrap();
    let opts = PushOptions { snapshot: Some(snap_id.to_hex()), ..Default::default() };
    assert_eq!(sync::resolve_push_snapshot(&store, &backup, &opts).unwrap().id, *snap_id);
    sync::push(&store, &snapshot, &mut backup, &opts, &Ctx::silent()).unwrap();
    assert_eq!(&common::library_files(&backup_root), files, "the snapshot, exactly");

    // Changed since (a new export, or a player's history list), the backup holds unsaved work.
    let changed = tc.draw(gs::booleans());
    if changed {
        match tc.draw(gs::booleans()) {
            true => {
                common::export_new_track(&backup_root, "Only", "On Backup");
            }
            false => common::add_history(&backup_root, "HISTORY 001", &[1]),
        }
    }
    let head = store.libraries().unwrap()[0].head.unwrap();
    let latest = store.load_snapshot(&head).unwrap();
    let mut backup = Device::open_export(&backup_root).unwrap();
    let r = sync::push(&store, &latest, &mut backup, &PushOptions::default(), &Ctx::silent());
    match (changed, r) {
        (true, Err(e)) => {
            assert!(matches!(e.inner(), Error::Conflict(_)), "{e}");
            // Saved beside the head, the push may go ahead; the head stays.
            let lineage_parent = *snap_id;
            sync::pull_beside(&store, &mut backup, &PullOptions::default(), lineage_parent, &Ctx::silent()).unwrap();
            assert_eq!(store.libraries().unwrap()[0].head, Some(head), "the head stays where it was");
            let mut backup = Device::open_export(&backup_root).unwrap();
            sync::push(&store, &latest, &mut backup, &PushOptions::default(), &Ctx::silent()).unwrap();
        }
        (false, Ok(_)) => {}
        (changed, r) => panic!("changed: {changed}, push: {:?}", r.map(|_| ())),
    }
    assert_eq!(common::library_files(&backup_root), states.last().unwrap().1, "the latest, exactly");
}
