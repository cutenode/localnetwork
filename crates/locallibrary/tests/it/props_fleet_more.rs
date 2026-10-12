//! The registry's view of drives of one library: each stands where its last state is in the
//! library's history (up to date, behind by so many snapshots, unsaved, or unknown), checks are
//! logged, one primary at a time, and identities are given back only where they belong.

use crate::common;

use std::fs;
use std::path::Path;

use hegel::generators as gs;
use hegel::{Generator, HealthCheck, TestCase};
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
fn drives_stand_where_their_state_is_in_the_librarys_history(tc: TestCase) {
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
    let check = tc.draw(gs::sampled_from(vec![Check::Scrub, Check::FileSystem]).print_as_debug());
    let ok = tc.draw(gs::booleans());
    fleet::record_check(&store, ids[1], check, ok, "checked").unwrap();
    let log = fleet::known(&store, &[], false).unwrap().into_iter().find(|k| k.id == ids[1]).unwrap().log;
    let entry = match check {
        Check::Scrub => (log.last_scrub, log.last_fs_check),
        _ => (log.last_fs_check, log.last_scrub),
    };
    assert!(entry.0.is_some_and(|e| e.ok == ok && e.summary == "checked") && entry.1.is_none());

    // One primary a library: a second makes the first a backup.
    let (first, second) = (ids[tc.draw(gs::integers::<usize>().max_value(2))], ids[0]);
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
