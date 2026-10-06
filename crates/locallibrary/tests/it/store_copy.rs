//! Copies of the store elsewhere (`store copy`), read with `--store` like the store itself.

use crate::common;

use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};

use locallibrary::api::{self, PruneOptions, PullRequest};
use locallibrary::device::Device;
use locallibrary::offsite::{self, CopyOptions};
use locallibrary::progress::{CancelToken, Ctx, Event, Progress};
use locallibrary::store::Store;
use locallibrary::sync::{self, PullOptions, PushOptions};
use locallibrary::{Error, Hash, paths};

fn pull(store: &Store, drive: &Path) -> Hash {
    let req = PullRequest { drive: drive.display().to_string(), force: true, ..Default::default() };
    api::pull(store, &req, &Ctx::silent()).unwrap().snapshot.unwrap().id
}

fn copy(store: &Store, dest: &Path, opts: &CopyOptions) -> locallibrary::Result<offsite::CopyReport> {
    offsite::copy(store, dest, opts, &Ctx::silent())
}

fn history(store: &Store, library: &str) -> Vec<Hash> {
    let lib = store.find_library(library).unwrap().unwrap();
    store.history(&lib, None).unwrap().into_iter().map(|s| s.id).collect()
}

fn objects(root: &Path) -> usize {
    walkdir::WalkDir::new(root.join("objects"))
        .into_iter()
        .filter(|e| e.as_ref().unwrap().file_type().is_file())
        .count()
}

/// Cancels the operation once it has started on `at` files.
struct StopAfter {
    seen: AtomicUsize,
    at: usize,
    cancel: CancelToken,
}

impl Progress for StopAfter {
    fn on_event(&self, event: &Event) {
        if matches!(event, Event::Item { .. }) && self.seen.fetch_add(1, Ordering::SeqCst) + 1 >= self.at {
            self.cancel.cancel();
        }
    }
}

fn stopped_copy(store: &Store, dest: &Path, at: usize) {
    let cancel = CancelToken::new();
    let stop = StopAfter { seen: AtomicUsize::new(0), at, cancel: cancel.clone() };
    let err = offsite::copy(store, dest, &CopyOptions::default(), &Ctx::new(&stop, cancel)).unwrap_err();
    assert!(matches!(err, Error::Cancelled), "{err}");
}

#[test]
fn copies_are_incremental_and_read_like_the_store() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    let a = common::demo_drive(dir.path(), "A");
    let first = pull(&store, &a);
    std::fs::create_dir(dir.path().join("nas")).unwrap();
    let dest = dir.path().join("nas/locallibrary");

    let r = copy(&store, &dest, &CopyOptions::default()).unwrap();
    let stored = objects(store.root());
    assert!(r.complete && r.problems.is_empty());
    assert_eq!((r.snapshots, r.snapshots_copied, r.objects_copied, r.objects_present), (1, 1, stored, 0));

    // A new export: only its files are copied.
    common::export_new_track(&a, "New", "Track");
    let second = pull(&store, &a);
    let r = copy(&store, &dest, &CopyOptions::default()).unwrap();
    assert_eq!((r.snapshots, r.snapshots_copied), (2, 1));
    assert_eq!((r.objects_present, r.objects_copied), (stored, objects(store.root()) - stored));
    assert_eq!(objects(&dest), objects(store.root()));
    let again = copy(&store, &dest, &CopyOptions { verify: true, ..Default::default() }).unwrap();
    assert_eq!((again.objects_copied, again.snapshots_copied, again.objects_repaired), (0, 0, 0));

    // The copy reads like the store (`--store DIR`): history, labels' snapshots, every file.
    let copied = Store::open(&dest).unwrap();
    assert_eq!(copied.id().unwrap(), store.id().unwrap());
    assert_eq!(history(&copied, "A"), [second, first]);
    let v = copied.verify(&Ctx::silent()).unwrap();
    assert!(v.missing.is_empty() && v.corrupt.is_empty() && v.checked > 0);
    // A drive written from the copy matches one written from the store.
    std::fs::create_dir(dir.path().join("B")).unwrap();
    let mut b = Device::open(dir.path().join("B")).unwrap();
    let snap = copied.load_snapshot(&first).unwrap();
    sync::push(&copied, &snap, &mut b, &PushOptions::default(), &Ctx::silent()).unwrap();
    std::fs::create_dir(dir.path().join("C")).unwrap();
    let mut c = Device::open(dir.path().join("C")).unwrap();
    sync::push(&store, &store.load_snapshot(&first).unwrap(), &mut c, &PushOptions::default(), &Ctx::silent()).unwrap();
    assert_eq!(common::library_files(b.root()), common::library_files(c.root()));

    // The store remembers the copy; the copy says what it is.
    let copies = offsite::copies(&store).unwrap();
    assert_eq!(copies.len(), 1);
    assert!(copies[0].available && copies[0].up_to_date == Some(true));
    assert!(copies[0].record.last_complete.is_some());
    assert_eq!(offsite::marker(&dest).unwrap().unwrap().store, store.id().unwrap());
    // Out of date once the store moves on.
    common::export_new_track(&a, "Newer", "Track");
    pull(&store, &a);
    assert_eq!(offsite::copies(&store).unwrap()[0].up_to_date, Some(false));
}

#[test]
fn a_stopped_copy_keeps_its_last_state_and_carries_on() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    let a = common::demo_drive(dir.path(), "A");
    let first = pull(&store, &a);
    let dest = dir.path().join("copy");

    // Stopped during the first copy: no snapshot is there yet, so none is listed.
    stopped_copy(&store, &dest, 3);
    let partial = objects(&dest);
    assert_eq!(partial, 2, "the third file was stopped part way and cleared up");
    let copied = Store::open(&dest).unwrap();
    assert!(copied.libraries().unwrap().is_empty() && copied.snapshot_ids().unwrap().is_empty());
    let marker = offsite::marker(&dest).unwrap().unwrap();
    assert!(marker.finished.is_none() && marker.last_complete.is_none());
    assert!(offsite::copies(&store).unwrap()[0].record.finished.is_none());

    // Carried on: only the rest is copied.
    let r = copy(&store, &dest, &CopyOptions::default()).unwrap();
    assert!(r.complete);
    assert_eq!((r.objects_present, r.objects_copied), (partial, objects(store.root()) - partial));
    assert_eq!(history(&Store::open(&dest).unwrap(), "A"), [first]);

    // Stopped during a later copy: the copy still holds the earlier state, whole.
    common::export_new_track(&a, "New", "Track");
    let second = pull(&store, &a);
    stopped_copy(&store, &dest, 2);
    let copied = Store::open(&dest).unwrap();
    assert_eq!(history(&copied, "A"), [first]);
    let v = copied.verify(&Ctx::silent()).unwrap();
    assert!(v.missing.is_empty() && v.corrupt.is_empty());
    let marker = offsite::marker(&dest).unwrap().unwrap();
    assert!(marker.finished.is_none() && marker.last_complete.is_some());
    copy(&store, &dest, &CopyOptions::default()).unwrap();
    assert_eq!(history(&Store::open(&dest).unwrap(), "A"), [second, first]);
}

#[test]
fn mirroring_removes_what_the_store_pruned() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    let a = common::demo_drive(dir.path(), "A");
    let first = pull(&store, &a);
    common::export_new_track(&a, "New", "Track");
    let second = pull(&store, &a);
    let dest = dir.path().join("copy");
    copy(&store, &dest, &CopyOptions::default()).unwrap();

    let prune = PruneOptions { keep_last: Some(1), ..Default::default() };
    assert_eq!(api::store_prune(&store, &prune, &Ctx::silent()).unwrap().removed_snapshots, 1);
    // A plain copy keeps what it had.
    copy(&store, &dest, &CopyOptions::default()).unwrap();
    assert_eq!(history(&Store::open(&dest).unwrap(), "A"), [second, first]);

    let mirror = CopyOptions { mirror: true, ..Default::default() };
    let dry = copy(&store, &dest, &CopyOptions { dry_run: true, ..mirror.clone() }).unwrap();
    assert_eq!(dry.removed_snapshots, 1);
    assert!(dry.removed_objects > 0);
    let r = copy(&store, &dest, &mirror).unwrap();
    assert_eq!((r.removed_snapshots, r.removed_objects), (dry.removed_snapshots, dry.removed_objects));
    let copied = Store::open(&dest).unwrap();
    assert_eq!(history(&copied, "A"), [second]);
    assert_eq!(objects(&dest), objects(store.root()));
    let v = copied.verify(&Ctx::silent()).unwrap();
    assert!(v.missing.is_empty() && v.corrupt.is_empty());
}

#[test]
fn copies_go_only_where_they_belong() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    let a = common::demo_drive(dir.path(), "A");
    pull(&store, &a);
    let opts = CopyOptions::default();

    assert!(matches!(copy(&store, &store.root().join("copy"), &opts), Err(Error::Invalid(_))));
    assert!(matches!(copy(&store, dir.path(), &opts), Err(Error::Invalid(_))), "the store is inside it");
    std::fs::create_dir(dir.path().join("photos")).unwrap();
    std::fs::write(dir.path().join("photos/a.jpg"), b"jpeg").unwrap();
    assert!(matches!(copy(&store, &dir.path().join("photos"), &opts), Err(Error::Invalid(_))));
    let other = Store::open_or_init(dir.path().join("other")).unwrap();
    assert!(copy(&store, other.root(), &opts).is_err_and(|e| matches!(e.inner(), Error::Conflict(_))));
    assert!(matches!(copy(&store, &dir.path().join("unmounted/copy"), &opts), Err(Error::NotFound(_))));
    assert!(!dir.path().join("unmounted").exists());

    // A copy pulled into on its own isn't copied over without force.
    let dest = dir.path().join("copy");
    copy(&store, &dest, &opts).unwrap();
    let copied = Store::open(&dest).unwrap();
    common::export_new_track(&a, "New", "Track");
    let mut d = Device::open_export(&a).unwrap();
    sync::pull(&copied, &mut d, &PullOptions::default(), &Ctx::silent()).unwrap();
    assert!(copy(&store, &dest, &opts).is_err_and(|e| matches!(e.inner(), Error::Conflict(_))));
    copy(&store, &dest, &CopyOptions { force: true, ..Default::default() }).unwrap();
}

/// Regression: what was written to a copy opened with `--store` (labels, player models, a pull
/// saved beside the latest) was replaced or removed by the next copy without a word.
#[test]
fn a_copy_written_to_is_copied_over_only_when_forced() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    let a = common::demo_drive(dir.path(), "A");
    let first = pull(&store, &a);
    let dest = dir.path().join("copy");
    let opts = CopyOptions::default();
    copy(&store, &dest, &opts).unwrap();
    copy(&store, &dest, &opts).unwrap();
    let copied = Store::open(&dest).unwrap();

    copied.add_label(&first, "in the copy").unwrap();
    let refused =
        |opts: &CopyOptions| copy(&store, &dest, opts).is_err_and(|e| matches!(e.inner(), Error::Conflict(_)));
    assert!(refused(&opts));
    assert!(refused(&CopyOptions { dry_run: true, mirror: true, ..Default::default() }));
    assert_eq!(copied.labels().unwrap().len(), 1, "left as it was");
    copy(&store, &dest, &CopyOptions { force: true, ..Default::default() }).unwrap();
    assert!(copied.labels().unwrap().is_empty(), "forced: the store's records");

    let players = locallibrary::equipment::Remembered { default: vec!["CDJ-3000".into()], ..Default::default() };
    copied.save_player_models(&players).unwrap();
    assert!(refused(&opts));
    copy(&store, &dest, &CopyOptions { force: true, ..Default::default() }).unwrap();
    copy(&store, &dest, &opts).unwrap();
}

/// Reading a drive's logbook from a copy isn't writing to it; giving the drive a role is.
#[test]
fn a_drive_log_read_from_a_copy_doesnt_count_as_a_write() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    let a = common::demo_drive(dir.path(), "A");
    pull(&store, &a);
    let dest = dir.path().join("copy");
    let opts = CopyOptions::default();
    copy(&store, &dest, &opts).unwrap();
    let copied = Store::open(&dest).unwrap();
    let drive = a.display().to_string();

    api::drive_log(&copied, &api::DriveRef { drive: drive.clone() }).unwrap();
    copy(&store, &dest, &opts).unwrap();
    api::drive_role(&copied, &api::SetRole { drive, role: Some("backup".into()) }).unwrap();
    let err = copy(&store, &dest, &opts).unwrap_err();
    assert!(matches!(err.inner(), Error::Conflict(_)), "{err}");
}

#[test]
fn a_damaged_file_leaves_its_snapshot_out() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    let a = common::demo_drive(dir.path(), "A");
    let first = pull(&store, &a);
    let dest = dir.path().join("copy");
    let done = copy(&store, &dest, &CopyOptions::default()).unwrap();
    assert!(done.complete);
    let complete = offsite::marker(&dest).unwrap().unwrap().last_complete;

    // The second snapshot's database is damaged in the store.
    common::export_new_track(&a, "New", "Track");
    let second = pull(&store, &a);
    let pdb = store.load_snapshot(&second).unwrap().files.into_iter().find(|f| f.path == paths::EXPORT_PDB).unwrap();
    std::fs::write(store.object_path(&pdb.hash.unwrap()), b"damaged").unwrap();

    let r = copy(&store, &dest, &CopyOptions { mirror: true, ..Default::default() }).unwrap();
    assert!(!r.complete);
    assert_eq!(r.problems.len(), 1, "{:?}", r.problems);
    assert!(r.problems[0].contains(&second.short()) && r.problems[0].contains("damaged"));
    // The copy keeps the latest snapshot it had, whole; nothing is mirrored away.
    let copied = Store::open(&dest).unwrap();
    assert_eq!(history(&copied, "A"), [first]);
    assert!(copied.load_snapshot(&second).is_err());
    assert!(!copied.has_object(&pdb.hash.unwrap()));
    let marker = offsite::marker(&dest).unwrap().unwrap();
    assert!(marker.finished.is_some() && marker.last_complete == complete);
    assert_eq!(marker.problems, r.problems);
}
