//! The registry's view of drives of one library: each stands where its last state is in the
//! library's history (up to date, behind by so many snapshots, unsaved, or unknown), checks are
//! logged, one primary at a time, and identities are given back only where they belong.

use crate::common;

use std::fs;
use std::path::Path;

use hegel::generators as gs;
use hegel::{HealthCheck, TestCase};
use locallibrary::device::{Device, LineageOp};
use locallibrary::error::Reason;
use locallibrary::fleet::{self, Check, Role, Standing};
use locallibrary::paths;
use locallibrary::progress::Ctx;
use locallibrary::store::Store;
use locallibrary::sync::{self, PullOptions};
use uuid::Uuid;

/// A copy of the drive at `from`, with an identity of its own named `name`.
fn copy_of(from: &Path, to: &Path, name: &str) -> Device {
    common::copy_dir(from, to);
    let _ = fs::remove_dir_all(to.join(paths::STATE_DIR));
    let mut d = Device::open_export(to).unwrap();
    d.adopt(Some(name)).unwrap();
    d
}

/// Register `d` as holding the state on it now.
fn register(store: &Store, d: &Device) {
    let lib = d.library().unwrap();
    let fp = d.scan_fingerprinted(Some(&lib), &Ctx::silent()).unwrap().fingerprint;
    sync::register_drive(store, d, fp).unwrap();
}

#[hegel::test(test_cases = 5, suppress_health_check = [HealthCheck::TooSlow])]
fn drives_stand_where_their_state_is_in_the_library_history(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    let a = common::bigger_drive(dir.path(), "A");
    let mut main = Device::open_export(&a).unwrap();
    main.adopt(Some("A")).unwrap();
    let library = sync::pull(&store, &mut main, &PullOptions::default(), &Ctx::silent()).unwrap().library;
    let into = PullOptions { library: Some(library), ..Default::default() };
    let pull = |d: &mut Device| sync::pull(&store, d, &into, &Ctx::silent()).unwrap();
    // B holds the first state; then A moves on, `later` exports more, each pulled.
    let mut b = copy_of(&a, &dir.path().join("B"), "B");
    pull(&mut b);
    let later = tc.draw(gs::integers::<usize>().min_value(1).max_value(2));
    for n in 0..later {
        common::export_new_track(&a, "Later", &format!("Export {n}"));
        pull(&mut Device::open_export(&a).unwrap());
    }
    // C, a copy of A given an identity of its own (it keeps A's library), has an export of its
    // own nobody pulled; D is in no library.
    common::copy_dir(&a, &dir.path().join("C"));
    let mut c = Device::open_export(dir.path().join("C")).unwrap();
    fleet::reidentify(Some(&store), &mut c, Some("C")).unwrap();
    common::export_new_track(c.root(), "Only", "On C");
    let c = Device::open_export(c.root()).unwrap();
    register(&store, &c);
    let mut d = Device::open(common::blank(dir.path(), "D")).unwrap();
    d.adopt(Some("D")).unwrap();
    sync::register_drive(&store, &d, None).unwrap();

    let known = fleet::known(&store, &[], false).unwrap();
    let standing = |name: &str| known.iter().find(|k| k.name == name).unwrap().standing.clone();
    assert!(matches!(standing("A"), Standing::UpToDate), "{:?}", standing("A"));
    match standing("B") {
        Standing::Behind { snapshots, .. } => assert_eq!(snapshots, later),
        other => panic!("B is {other:?}"),
    }
    assert!(matches!(standing("C"), Standing::Unsaved), "{:?}", standing("C"));
    assert!(matches!(standing("D"), Standing::Unknown), "{:?}", standing("D"));

    // Checks are logged on the drive they were of.
    let ids: Vec<Uuid> = ["A", "B", "C"].iter().map(|n| known.iter().find(|k| k.name == *n).unwrap().id).collect();
    let check = tc.draw(gs::sampled_from(vec![Check::Scrub, Check::FileSystem]));
    let ok = tc.draw(gs::booleans());
    fleet::record_check(&store, ids[1], check, ok, "checked").unwrap();
    let log = fleet::known(&store, &[], false).unwrap().into_iter().find(|k| k.id == ids[1]).unwrap().log;
    let entry = match check {
        Check::Scrub => (log.last_scrub, log.last_fs_check),
        _ => (log.last_fs_check, log.last_scrub),
    };
    assert!(entry.0.is_some_and(|e| e.ok == ok && e.summary == "checked") && entry.1.is_none());

    // One primary a library: a second makes the first a backup.
    let (first, second) = (tc.draw(gs::sampled_from(&ids)), ids[0]);
    fleet::set_role(&store, first, Some(Role::Primary)).unwrap();
    fleet::set_role(&store, second, Some(Role::Primary)).unwrap();
    let roles = fleet::known(&store, &[], false).unwrap();
    let role = |id: Uuid| roles.iter().find(|k| k.id == id).unwrap().role;
    assert_eq!(role(second), Some(Role::Primary));
    if first != second {
        assert_eq!(role(first), Some(Role::Backup));
    }
}

/// Renaming takes a name, and changes no volume label while the machine's drives are hidden;
/// a name makes a volume label FAT holds (11 characters at most, upper case, never none).
#[hegel::test(test_cases = 100)]
fn names_and_labels(tc: TestCase) {
    let name = tc.draw(gs::text().max_size(20));
    let label = fleet::volume_label(&name);
    assert!(!label.is_empty() && label.chars().count() <= 11, "{label:?} for {name:?}");
    assert_eq!(label, label.to_uppercase(), "{label:?}");
    let dir = tempfile::tempdir().unwrap();
    let mut d = Device::open(common::blank(dir.path(), "Drive")).unwrap();
    let blank = tc.draw(gs::sampled_from(vec!["", "  ", "\t"]));
    assert!(fleet::rename(None, &mut d, blank, false).is_err_and(|e| e.to_string().contains("the name is empty")));
    locallibrary::testing::volumes_on_this_thread(Some(Vec::new()));
    let refused = fleet::rename(None, &mut d, "Club", true);
    locallibrary::testing::volumes_on_this_thread(None);
    assert!(refused.is_err_and(|e| e.to_string().contains("LOCALLIBRARY_VOLUMES")));
    assert!(d.id().is_none(), "refused before anything changed");
}

/// A drive that lost its identity takes back the one its content says, with the lineage up to
/// the state it holds (observed, when that state was never saved); a drive with an identity of
/// its own is refused, with the way to give it another; a drive with no export is recognised as
/// nothing the store holds.
#[hegel::test(test_cases = 4, suppress_health_check = [HealthCheck::TooSlow])]
fn identities_go_back_only_where_they_belong(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    let a = common::bigger_drive(dir.path(), "A");
    let mut main = Device::open_export(&a).unwrap();
    main.adopt(Some("A")).unwrap();
    sync::pull(&store, &mut main, &PullOptions::default(), &Ctx::silent()).unwrap();
    let id = main.id().unwrap();
    let moved_on = tc.draw(gs::booleans());
    if moved_on {
        common::export_new_track(&a, "Later", "Unsaved");
    }
    fs::remove_dir_all(main.state_dir()).unwrap();
    let mut lost = Device::open_export(&a).unwrap();
    let info = fleet::adopt_as(&store, &mut lost, &id.to_string(), &[], &Ctx::silent()).unwrap();
    assert_eq!(info.id, Some(id));
    let head = Device::open_export(&a).unwrap().state().unwrap().head().unwrap().op;
    assert_eq!(head == LineageOp::Observed, moved_on, "{head:?}");

    let mut other = copy_of(&a, &dir.path().join("Other"), "Other");
    let err = fleet::adopt_as(&store, &mut other, &id.to_string(), &[], &Ctx::silent()).unwrap_err();
    assert_eq!(err.reason(), Some(Reason::HasIdentity), "{err}");
    let methods: Vec<_> = err.detail().actions.iter().map(|a| a.method.clone()).collect();
    assert_eq!(methods, [Some("drive.reidentify".to_string())]);

    let blank = Device::open(common::blank(dir.path(), "Blank")).unwrap();
    let found = fleet::recognise(&store, &blank, &Ctx::silent()).unwrap();
    assert!(found.snapshot.is_none() && found.drive_by_content.is_none());
}

/// Every change to a drive the registry doesn't have is refused as not found, naming it (a
/// role, retiring, forgetting it by name), or passes it by (the logbook's notes); either way the
/// registry is as it was. A drive with no identity of its own can't be given a new one.
#[hegel::test(test_cases = 20, suppress_health_check = [HealthCheck::TooSlow])]
fn changes_to_a_drive_the_registry_lacks_change_nothing(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    // Now and then the registry holds another drive, which nothing here may touch.
    if tc.draw(gs::booleans()) {
        let mut d = Device::open(common::blank(dir.path(), "Other")).unwrap();
        d.adopt(Some("Other")).unwrap();
        sync::register_drive(&store, &d, None).unwrap();
    }
    let registry = || serde_json::to_value(store.drives().unwrap()).unwrap();
    let before = registry();
    let id = Uuid::from_u128(tc.draw(gs::integers::<u128>()));
    let role = tc.draw(gs::sampled_from(vec![Some(Role::Primary), Some(Role::Backup), Some(Role::Archive), None]));
    let not_found = |e: locallibrary::Error| {
        assert!(matches!(e, locallibrary::Error::NotFound(_)), "{e}");
        assert!(e.to_string().contains(&id.to_string()), "{e}");
    };
    not_found(fleet::set_role(&store, id, role).unwrap_err());
    not_found(fleet::retire(&store, id, tc.draw(gs::booleans())).unwrap_err());
    let name = tc.draw(gs::sampled_from(vec!["Main", "Club USB", ""]));
    let err = fleet::forget(&store, name).unwrap_err();
    assert!(matches!(err, locallibrary::Error::NotFound(_)) && err.to_string().contains(&format!("{name:?}")), "{err}");
    let summary = tc.draw(gs::text().max_size(12));
    fleet::record_check(&store, id, Check::FileSystem, tc.draw(gs::booleans()), summary.as_str()).unwrap();
    fleet::record_problem(&store, id, "damaged_files", summary.as_str()).unwrap();
    fleet::record_eject(&store, id).unwrap();
    assert!(!fleet::update(&store, id, |r| r.log.writes += 1).unwrap());
    assert_eq!(registry(), before);

    let mut blank = Device::open(common::blank(dir.path(), "Blank")).unwrap();
    let err =
        fleet::reidentify(Some(&store), &mut blank, tc.draw(gs::optional(gs::sampled_from(vec!["Copy"])))).unwrap_err();
    assert!(err.to_string().contains("has no identity yet"), "{err}");
    assert!(blank.id().is_none());
    assert_eq!(registry(), before);
}

/// What a fleet operation needs and can't read stops it: a damaged registry fails each one that
/// reads or changes it, a damaged library list each one that names libraries, a drive locked by
/// another process each change to the drive, a drive whose identity file is damaged each one
/// that reads it again under the lock, and a folder on the drive that can't be read each one that
/// scans it. Whatever was damaged stays as it was, and only noting a write to a drive with no
/// identity needs nothing.
#[cfg(unix)]
#[hegel::test(test_cases = 3, suppress_health_check = [HealthCheck::TooSlow])]
fn prop_what_a_fleet_operation_cant_read_stops_it(tc: TestCase) {
    use std::os::unix::fs::PermissionsExt;
    for fault in 0..5u8 {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open_or_init(dir.path().join("store")).unwrap();
        let root = common::demo_drive(dir.path(), "A");
        let mut d = Device::open_export(&root).unwrap();
        d.adopt(Some("A")).unwrap();
        register(&store, &d);
        let (id, ctx) = (d.id().unwrap(), Ctx::silent());
        let blank = Device::open(common::blank(dir.path(), "Blank")).unwrap();
        let garbage = tc.draw(gs::sampled_from(vec![&b"not json"[..], b"{", b"[1, 2"]));
        let damaged = match fault {
            0 => store.root().join("drives.json"),
            1 => store.root().join("libraries.json"),
            4 => root.join("Contents/locked"),
            _ => d.state_dir().join("state.json"),
        };
        let held = match fault {
            2 => Some(Device::open(&root).unwrap().lock().unwrap()),
            4 => {
                fs::create_dir_all(&damaged).unwrap();
                fs::set_permissions(&damaged, fs::Permissions::from_mode(0o000)).unwrap();
                None
            }
            _ => {
                fs::write(&damaged, garbage).unwrap();
                None
            }
        };
        let before = (fault != 4).then(|| fs::read(&damaged).unwrap());
        let mounted = || vec![Device::open(&root).unwrap()];
        match fault {
            0 => {
                assert!(fleet::record_write(&store, &d, 1).is_err());
                assert!(fleet::record_eject(&store, id).is_err());
                assert!(fleet::record_check(&store, id, Check::Scrub, true, "ok").is_err());
                assert!(fleet::record_problem(&store, id, "damaged_files", "x").is_err());
                assert!(fleet::find_record(&store, "A").is_err());
                assert!(fleet::forget(&store, "A").is_err());
                assert!(fleet::retire(&store, id, false).is_err());
                assert!(fleet::check_target(Some(&store), &d, false).is_err());
                assert!(fleet::primary_of(&store, None).is_err());
                assert!(fleet::recognise_with(&store, &d, None, None).is_err());
                assert!(fleet::adopt_as(&store, &mut Device::open(&root).unwrap(), "A", &[], &ctx).is_err());
                // Nothing to note for a drive with no identity.
                fleet::record_write(&store, &blank, 1).unwrap();
            }
            1 => {
                let fp = locallibrary::hash::Hash::of(b"x");
                assert!(fleet::recognise_with(&store, &d, Some(fp), None).is_err());
            }
            _ => {}
        }
        match fault {
            0 | 1 => {
                assert!(fleet::known(&store, &[], false).is_err());
                assert!(fleet::sets(Some(&store), &mounted()).is_err());
                assert!(fleet::recognise(&store, &d, &ctx).is_err());
                // The drive is changed first; only telling the registry fails.
                if fault == 0 {
                    assert!(fleet::rename(Some(&store), &mut d, "B", false).is_err());
                    assert!(fleet::reidentify(Some(&store), &mut d, None).is_err());
                }
            }
            4 => {
                let recognised = fleet::recognise(&store, &d, &ctx);
                // Taking back its identity scans it too.
                fs::remove_dir_all(d.state_dir()).unwrap();
                let adopted = fleet::adopt_as(&store, &mut Device::open(&root).unwrap(), &id.to_string(), &[], &ctx);
                fs::set_permissions(&damaged, fs::Permissions::from_mode(0o755)).unwrap();
                assert!(matches!(recognised, Err(locallibrary::Error::Io { .. })), "{recognised:?}");
                assert!(matches!(adopted, Err(locallibrary::Error::Io { .. })), "{adopted:?}");
            }
            _ => {
                let refused = |r: locallibrary::Result<locallibrary::device::DeviceInfo>| match fault {
                    2 => assert!(matches!(r, Err(locallibrary::Error::Locked(_))), "{r:?}"),
                    _ => assert!(r.is_err(), "{r:?}"),
                };
                refused(fleet::rename(None, &mut d, "B", false).map(|_| blank.info()));
                refused(fleet::adopt_as(&store, &mut d, "A", &[], &ctx));
                refused(fleet::reidentify(None, &mut d, None));
            }
        }
        drop(held);
        if let Some(before) = before {
            assert_eq!(fs::read(&damaged).unwrap(), before, "fault {fault}");
        }
    }
}
