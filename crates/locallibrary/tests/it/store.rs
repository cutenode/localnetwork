use crate::common;

use locallibrary::compare::{self, Relation, Side};
use locallibrary::device::Device;
use locallibrary::progress::Ctx;
use locallibrary::store::Store;
use locallibrary::sync::{self, PullOptions, PushOptions};
use locallibrary::{Error, paths};

#[test]
fn pull_push_history_and_rollback() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    let mut a = Device::open_export(common::demo_drive(dir.path(), "A")).unwrap();

    // First pull creates a library named after the drive.
    let r = sync::pull(
        &store,
        &mut a,
        &PullOptions { message: Some("first".into()), ..Default::default() },
        &Ctx::silent(),
    )
    .unwrap();
    assert_eq!(r.library, "A");
    assert!(!r.up_to_date && r.imported_files > 0);
    let first = r.snapshot.unwrap();
    assert_eq!(a.state().unwrap().library_id, Some(r.library_id));

    // Pulling again is a no-op.
    let again = sync::pull(&store, &mut a, &PullOptions::default(), &Ctx::silent()).unwrap();
    assert!(again.up_to_date);

    // Push to a blank drive: byte-identical library, linked to the same library.
    std::fs::create_dir(dir.path().join("C")).unwrap();
    let mut c = Device::open(dir.path().join("C")).unwrap();
    let snap = sync::resolve_push_snapshot(&store, &a, &PushOptions::default()).unwrap();
    sync::push(&store, &snap, &mut c, &PushOptions::default(), &Ctx::silent()).unwrap();
    assert_eq!(common::library_files(a.root()), common::library_files(c.root()));
    assert_eq!(c.state().unwrap().library_id, Some(r.library_id));

    // A gets a new export; pulling makes a child snapshot. Audio dedupes against the store.
    common::export_new_track(a.root(), "New", "Track");
    let r2 = sync::pull(
        &store,
        &mut a,
        &PullOptions { message: Some("second".into()), ..Default::default() },
        &Ctx::silent(),
    )
    .unwrap();
    let second = r2.snapshot.unwrap();
    assert_eq!(second.parents, vec![first.id]);
    assert!(r2.reused_files > 0);
    let lib = store.find_library("A").unwrap().unwrap();
    let hist = store.history(&lib, None).unwrap();
    assert_eq!(hist.len(), 2);
    assert_eq!(hist[0].message.as_deref(), Some("second"));

    // C is now behind the store, by lineage.
    let head = store.load_snapshot(&lib.head.unwrap()).unwrap();
    let cmp = compare::compare(
        &Side::from_snapshot(&store, &head, "store").unwrap(),
        &Side::from_device(&c, &Ctx::silent()).unwrap(),
    );
    assert_eq!(cmp.relation, Relation::LeftNewer);

    // Push the head to C, then roll C back to the first snapshot.
    sync::push(&store, &head, &mut c, &PushOptions::default(), &Ctx::silent()).unwrap();
    assert_eq!(c.library().unwrap().tracks.len(), 3);
    let old = store.load_snapshot(&first.id).unwrap();
    sync::push(&store, &old, &mut c, &PushOptions::default(), &Ctx::silent()).unwrap();
    assert_eq!(c.library().unwrap().tracks.len(), 2);
    let on_c: Vec<String> = common::library_files(c.root()).into_iter().map(|(k, _)| k).collect();
    let in_snap: Vec<String> = store.load_snapshot(&first.id).unwrap().files.into_iter().map(|f| f.path).collect();
    assert_eq!(on_c, in_snap);

    // Pulling an older state on top of the head is refused.
    let err = sync::pull(&store, &mut c, &PullOptions::default(), &Ctx::silent()).unwrap_err();
    assert!(matches!(err.inner(), Error::Conflict(_)), "{err}");

    // GC keeps everything reachable.
    let gc = store.gc(false).unwrap();
    assert_eq!(gc.removed_objects, 0);
    assert!(store.verify(&Ctx::silent()).unwrap().corrupt.is_empty());
}

#[test]
fn push_refuses_to_clobber_unsaved_changes() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    let mut a = Device::open_export(common::demo_drive(dir.path(), "A")).unwrap();
    sync::pull(&store, &mut a, &PullOptions::default(), &Ctx::silent()).unwrap();
    let snap = sync::resolve_push_snapshot(&store, &a, &PushOptions::default()).unwrap();

    // B holds some other export the store has never seen.
    let mut b = Device::open_export(common::bigger_drive(dir.path(), "B")).unwrap();
    let err = sync::push(&store, &snap, &mut b, &PushOptions::default(), &Ctx::silent()).unwrap_err();
    assert!(matches!(err.inner(), Error::Conflict(_)));
    assert_eq!(b.library().unwrap().tracks.len(), 12, "nothing was written");

    let forced = PushOptions { force: true, ..Default::default() };
    sync::push(&store, &snap, &mut b, &forced, &Ctx::silent()).unwrap();
    assert_eq!(b.library().unwrap().tracks.len(), 2);
}

#[test]
fn pull_of_diverged_drive_needs_force() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    let mut a = Device::open_export(common::demo_drive(dir.path(), "A")).unwrap();
    sync::pull(&store, &mut a, &PullOptions { library: Some("Main".into()), ..Default::default() }, &Ctx::silent())
        .unwrap();

    // An unrelated drive pulled into the same library.
    let mut b = Device::open_export(common::bigger_drive(dir.path(), "B")).unwrap();
    let opts = PullOptions { library: Some("Main".into()), ..Default::default() };
    assert!(sync::pull(&store, &mut b, &opts, &Ctx::silent()).is_err_and(|e| matches!(e.inner(), Error::Conflict(_))));
    let forced = PullOptions { force: true, ..opts };
    let r = sync::pull(&store, &mut b, &forced, &Ctx::silent()).unwrap();
    assert_eq!(r.snapshot.unwrap().summary.tracks, 12);
    // The database files really are in the store.
    let lib = store.find_library("main").unwrap().unwrap();
    let head = store.load_snapshot(&lib.head.unwrap()).unwrap();
    assert!(head.files.iter().any(|f| f.path == paths::EXPORT_PDB && store.has_object(&f.hash.unwrap())));
}
