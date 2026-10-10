//! Store copies (`store copy`) that find files changed or missing on either side, or nowhere to
//! go.

use crate::common;

use std::fs;
use std::path::Path;

use locallibrary::device::Device;
use locallibrary::offsite::{self, CopyOptions, CopyReport};
use locallibrary::progress::Ctx;
use locallibrary::store::Store;
use locallibrary::sync::{self, PullOptions};
use locallibrary::{Error, Hash, paths};

/// A store holding one pull of the demo drive, and the snapshot's export.pdb.
fn pulled(dir: &Path) -> (Store, Hash) {
    let store = Store::open_or_init(dir.join("store")).unwrap();
    let mut d = Device::open(common::demo_drive(dir, "A")).unwrap();
    let id = sync::pull(&store, &mut d, &PullOptions::default(), &Ctx::silent()).unwrap().snapshot.unwrap().id;
    let pdb = store.load_snapshot(&id).unwrap().files.into_iter().find(|f| f.path == paths::EXPORT_PDB).unwrap();
    (store, pdb.hash.unwrap())
}

fn copy(store: &Store, dest: &Path, opts: &CopyOptions) -> locallibrary::Result<CopyReport> {
    offsite::copy(store, dest, opts, &Ctx::silent())
}

#[test]
fn a_new_copy_needs_the_folder_it_goes_in() {
    let dir = tempfile::tempdir().unwrap();
    let (store, _) = pulled(dir.path());
    let unmounted = dir.path().join("Volumes/NAS");
    let err = copy(&store, &unmounted.join("locallibrary"), &CopyOptions::default()).unwrap_err();
    // The store names the folder as the computer resolves it (/private/var for /var on macOS).
    let want = "NAS, the folder to make the copy in (is its disk or share mounted?)";
    assert!(matches!(&err, Error::NotFound(what) if what.ends_with(want)), "{err}");
    assert!(!unmounted.exists());
}

#[test]
fn verifying_a_copy_copies_again_a_file_that_changed_there() {
    let dir = tempfile::tempdir().unwrap();
    let (store, pdb) = pulled(dir.path());
    let dest = dir.path().join("copy");
    copy(&store, &dest, &CopyOptions::default()).unwrap();
    let copied = Store::open(&dest).unwrap();
    let good = fs::read(copied.object_path(&pdb)).unwrap();
    let mut bad = good.clone();
    bad[100] ^= 0xFF;
    fs::write(copied.object_path(&pdb), &bad).unwrap();

    let r = copy(&store, &dest, &CopyOptions::default()).unwrap();
    assert_eq!(r.objects_repaired, 0, "without verifying, the size alone vouches for it");
    let r = copy(&store, &dest, &CopyOptions { verify: true, ..Default::default() }).unwrap();
    assert_eq!((r.objects_repaired, r.objects_copied), (1, 1));
    assert!(r.complete);
    assert_eq!(fs::read(copied.object_path(&pdb)).unwrap(), good);
}

#[test]
fn a_file_missing_from_the_store_leaves_its_snapshot_out() {
    let dir = tempfile::tempdir().unwrap();
    let (store, pdb) = pulled(dir.path());
    fs::remove_file(store.object_path(&pdb)).unwrap();
    let dest = dir.path().join("copy");

    let r = copy(&store, &dest, &CopyOptions::default()).unwrap();
    assert!(!r.complete);
    assert_eq!(r.problems.len(), 1, "{:?}", r.problems);
    assert!(r.problems[0].contains(&format!("{} is missing from the store", paths::EXPORT_PDB)), "{:?}", r.problems);
    assert!(Store::open(&dest).unwrap().snapshot_ids().unwrap().is_empty());
}
