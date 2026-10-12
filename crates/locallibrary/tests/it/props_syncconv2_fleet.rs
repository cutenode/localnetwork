//! Telling registered drives apart by what they hold and the disk they were seen on, giving a
//! drive a registered drive's identity, keeping archive and retired drives as they are, and the
//! logbook's list of problems.

use crate::common;

use hegel::generators as gs;
use hegel::{Generator, HealthCheck, TestCase};
use locallibrary::device::{Device, LineageOp};
use locallibrary::error::Reason;
use locallibrary::fleet::{self, Hardware, Role};
use locallibrary::paths;
use locallibrary::progress::Ctx;
use locallibrary::store::Store;
use locallibrary::sync::{self, PullOptions};

/// A disk as the OS might describe it: one of two models and sizes, with one of two real serial
/// numbers, a placeholder one, or none.
#[hegel::test_helper]
fn disk(tc: &TestCase) -> Hardware {
    Hardware {
        model: tc.draw(gs::sampled_from(vec!["Cruzer".to_string(), "Ultra".to_string()])),
        serial: tc.draw(gs::sampled_from(vec![
            Some("4C530001123456789012".to_string()),
            Some("4C530002123456789012".to_string()),
            None,
        ])),
        volume_uuid: None,
        size: tc.draw(gs::sampled_from(vec![32_000_000_000u64, 64_000_000_000])),
    }
}

/// A drive is recognised as a registered drive (never itself): by content, the one last seen
/// holding its state, or when several were, the one its snapshot was taken from; by disk, one
/// seen on it (same serial number, model and size; none without a serial number on both sides).
#[hegel::test(test_cases = 4, suppress_health_check = [HealthCheck::TooSlow])]
fn a_drive_is_recognised_by_what_it_holds_and_its_disk(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    // A state the store saved, pulled from a drive, beside a library that lacks it.
    store.create_library("Empty").unwrap();
    let mut source = Device::open_export(common::demo_drive(dir.path(), "Source")).unwrap();
    let pulled = sync::pull(&store, &mut source, &PullOptions::default(), &Ctx::silent()).unwrap();
    let saved = pulled.snapshot.unwrap();
    let source_holds = tc.draw(gs::booleans());
    if !source_holds {
        sync::register_drive(&store, &source, Some(locallibrary::Hash::of(b"moved on"))).unwrap();
    }
    let mut drives = Vec::new();
    for n in 0..tc.draw(gs::integers::<usize>().min_value(1).max_value(3)) {
        let mut d = Device::open(common::blank(dir.path(), &format!("Drive {n}"))).unwrap();
        d.adopt(Some(&format!("Drive {n}"))).unwrap();
        d.state_mut().unwrap().hardware = tc.draw(gs::booleans()).then(|| disk(&tc));
        d.save_state().unwrap();
        let holds = tc.draw(gs::booleans()).then_some(saved.fingerprint);
        sync::register_drive(&store, &d, holds).unwrap();
        drives.push(d);
    }
    let stranger = Device::open(common::blank(dir.path(), "Stranger")).unwrap();
    let probe = disk(&tc);
    let check = |devices: &[&Device]| {
        let registry = store.drives().unwrap();
        for d in devices {
            let found = fleet::recognise_with(&store, d, Some(saved.fingerprint), Some(&probe)).unwrap();
            assert_eq!(found.snapshot.as_ref().map(|m| m.snapshot.id), Some(saved.id));
            let holding: Vec<uuid::Uuid> = registry
                .values()
                .filter(|r| Some(r.id) != d.id() && r.last_fingerprint == Some(saved.fingerprint))
                .map(|r| r.id)
                .collect();
            let by_content = match holding.as_slice() {
                [one] => Some(*one),
                _ => source.id().filter(|id| holding.contains(id)),
            };
            assert_eq!(found.drive_by_content.as_ref().map(|r| r.id), by_content, "{holding:?}");
            assert!(!found.is_empty());
            let unknown = fleet::recognise_with(&store, d, None, None).unwrap();
            assert!(unknown.is_empty(), "nothing to go by");
            let matching: Vec<uuid::Uuid> = registry
                .values()
                .filter(|r| Some(r.id) != d.id())
                .filter(|r| {
                    r.hardware.as_ref().is_some_and(|h| {
                        h.serial.is_some() && h.serial == probe.serial && h.model == probe.model && h.size == probe.size
                    })
                })
                .map(|r| r.id)
                .collect();
            match found.drive_by_hardware {
                Some(r) => assert!(matching.contains(&r.id), "{} isn't on {probe:?}", r.name),
                None => assert!(matching.is_empty(), "{matching:?} are on {probe:?}"),
            }
        }
    };
    let devices: Vec<&Device> = drives.iter().chain([&stranger, &source]).collect();
    check(&devices);
    // Two more drives that hold it: several do now.
    for n in 0..2 {
        let mut twin = Device::open(common::blank(dir.path(), &format!("Twin {n}"))).unwrap();
        twin.adopt(None).unwrap();
        sync::register_drive(&store, &twin, Some(saved.fingerprint)).unwrap();
    }
    check(&devices);
}

/// A drive takes a registered drive's identity, by name or id: a blank one (nothing to record),
/// or one holding a library, which records the state it holds when the store hasn't saved it.
/// Mounted together, the drives that claim it are duplicates.
#[hegel::test(test_cases = 6, suppress_health_check = [HealthCheck::TooSlow])]
fn a_drive_takes_a_registered_identity(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    let mut known = Device::open(common::blank(dir.path(), "Known")).unwrap();
    let name = tc.draw(gs::sampled_from(vec!["Main", "Gig stick", "Ünï"]));
    known.adopt(Some(name)).unwrap();
    sync::register_drive(&store, &known, None).unwrap();
    let id = known.id().unwrap();
    let spec = match tc.draw(gs::booleans()) {
        true => name.to_string(),
        false => id.to_string(),
    };
    for export in [false, true] {
        let root = match export {
            false => common::blank(dir.path(), "Blank"),
            true => common::demo_drive(dir.path(), "Exported"),
        };
        let mut d = Device::open(&root).unwrap();
        let info = fleet::adopt_as(&store, &mut d, &spec, &[], &Ctx::silent()).unwrap();
        assert_eq!((info.id, info.name.as_deref()), (Some(id), Some(name)));
        let again = Device::open(&root).unwrap();
        assert_eq!((again.id(), again.name()), (Some(id), name.to_string()));
        let ops: Vec<LineageOp> = again.state().unwrap().lineage.iter().map(|e| e.op).collect();
        assert_eq!(ops, if export { vec![LineageOp::Observed] } else { Vec::new() });
    }
    // Both now claim it: mounted together they're duplicates; a drive with no identity isn't one.
    common::blank(dir.path(), "Unknown");
    let mounted: Vec<Device> = ["Blank", "Unknown", "Exported", "Known"]
        .into_iter()
        .map(|n| Device::open(dir.path().join(n)).unwrap())
        .collect();
    let dupes = fleet::duplicates(&mounted, &|_| None);
    assert_eq!(dupes.len(), 1, "{dupes:?}");
    assert_eq!((dupes[0].id, dupes[0].roots.len(), dupes[0].original.as_ref()), (id, 3, None));
}

/// An archive or retired drive is written only when forced: retired first, then archive.
#[hegel::test(test_cases = 10, suppress_health_check = [HealthCheck::TooSlow])]
fn archive_and_retired_drives_are_written_only_when_forced(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    let mut d = Device::open(common::blank(dir.path(), "Kept")).unwrap();
    d.adopt(Some("Kept")).unwrap();
    sync::register_drive(&store, &d, None).unwrap();
    let id = d.id().unwrap();
    let role = tc.draw(
        gs::sampled_from(vec![None, Some(Role::Primary), Some(Role::Backup), Some(Role::Archive), Some(Role::Set)])
            .print_as_debug(),
    );
    fleet::set_role(&store, id, role).unwrap();
    let retired = tc.draw(gs::booleans());
    if retired {
        fleet::retire(&store, id, false).unwrap();
    }
    for force in [false, true] {
        let got = fleet::check_target(Some(&store), &d, force).err().and_then(|e| e.reason());
        let want = match (force, retired, role) {
            (true, _, _) => None,
            (false, true, _) => Some(Reason::Retired),
            (false, false, Some(Role::Archive)) => Some(Reason::Archive),
            _ => None,
        };
        assert_eq!(got, want, "{role:?}, retired {retired}, forced {force}");
    }
    assert!(fleet::check_target(None, &d, false).is_ok(), "no store, no registry to go by");
}

/// A drive's logbook keeps its latest 50 problems, oldest first.
#[hegel::test(test_cases = 3, suppress_health_check = [HealthCheck::TooSlow])]
fn the_logbook_keeps_the_latest_problems(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    let mut d = Device::open(common::blank(dir.path(), "Worn")).unwrap();
    d.adopt(Some("Worn")).unwrap();
    sync::register_drive(&store, &d, None).unwrap();
    let id = d.id().unwrap();
    let n = tc.draw(gs::integers::<usize>().min_value(48).max_value(53));
    for k in 0..n {
        fleet::record_problem(&store, id, "damaged_files", format!("problem {k}")).unwrap();
    }
    let kept: Vec<String> = store.drives().unwrap()[&id].log.problems.iter().map(|p| p.detail.clone()).collect();
    let want: Vec<String> = (n.saturating_sub(50)..n).map(|k| format!("problem {k}")).collect();
    assert_eq!(kept, want);
}

/// A state an earlier version saved, under the fingerprint it gave then (when what players keep
/// on a drive still counted), is still recognised by it; where the store saved the state as this
/// version fingerprints it too, that snapshot is the one recognised.
#[hegel::test(test_cases = 2, suppress_health_check = [HealthCheck::TooSlow])]
fn a_state_an_earlier_version_saved_is_still_recognised(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let root = common::demo_drive(dir.path(), "A");
    // What a player keeps on the drive: its track filter, or a log.
    let kept = tc.draw(gs::sampled_from(vec!["PIONEER/rekordbox/RBFLTR.DAT", "PIONEER/LOG/0001.LOG"]));
    let at = paths::to_fs(&root, kept);
    std::fs::create_dir_all(at.parent().unwrap()).unwrap();
    std::fs::write(&at, tc.draw(gs::sampled_from(vec![b"filter".to_vec(), vec![0u8; 64]]))).unwrap();
    let mut d = Device::open_export(&root).unwrap();
    let scanned = d.scan_fingerprinted(d.library().ok().as_ref(), &Ctx::silent()).unwrap();
    let legacy = *scanned.legacy_fingerprints.first().expect("the player's file made it differ");
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    let current = sync::pull(&store, &mut d, &PullOptions::default(), &Ctx::silent()).unwrap().snapshot.unwrap();

    // The earlier version's store: the same state, under its old fingerprint.
    let old = Store::open_or_init(dir.path().join("old")).unwrap();
    let save_old = |s: &Store| {
        let lib = s.create_library("Earlier").unwrap();
        let mut snap = store.load_snapshot(&current.id).unwrap();
        for f in &snap.files {
            s.import_file(&paths::to_fs(&root, &f.path), f.hash, &Ctx::silent()).unwrap();
        }
        (snap.library_id, snap.parents, snap.fingerprint) = (lib.id, Vec::new(), legacy);
        let saved = s.save_snapshot(snap).unwrap();
        s.set_head(lib.id, saved.id).unwrap();
        saved.id
    };
    let earlier = save_old(&old);
    let d = Device::open_export(&root).unwrap();
    let found = fleet::recognise(&old, &d, &Ctx::silent()).unwrap();
    assert_eq!(found.snapshot.map(|m| m.snapshot.id), Some(earlier), "by its old fingerprint");
    save_old(&store);
    let found = fleet::recognise(&store, &d, &Ctx::silent()).unwrap();
    assert_eq!(found.snapshot.map(|m| m.snapshot.id), Some(current.id), "by its fingerprint now");
}
