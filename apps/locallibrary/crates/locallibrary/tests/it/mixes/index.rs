//! Indexing a drive's library for mix identification: what can't be fingerprinted is reported,
//! and a file that doesn't decode is remembered in the store's cache rather than decoded again.

use crate::common::{self, Warnings};

use locallibrary::device::Device;
use locallibrary::mix::Identifier;
use locallibrary::mix::index::Cache;
use locallibrary::progress::Ctx;
use locallibrary::{Store, paths};

#[test]
fn a_drive_is_indexed_with_what_can_be_read() {
    let dir = tempfile::tempdir().unwrap();
    let root = common::demo_drive(dir.path(), "DNA");
    let device = Device::open_export(&root).unwrap();
    let library = device.library().unwrap();
    // Track 2's audio is gone; track 1's is the fixture's placeholder, which isn't audio.
    std::fs::remove_file(paths::to_fs(&root, &paths::key(&library.track(2).unwrap().file_path))).unwrap();
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    let cache = Cache::in_store(&store);
    let warnings = Warnings::default();

    let id = Identifier::new(&device, &cache, &warnings.ctx()).unwrap();
    let r = &id.report;
    assert_eq!((r.tracks, r.indexed, r.fingerprinted, r.hashes), (2, 0, 0, 0));
    assert_eq!(r.missing.iter().map(|t| t.id).collect::<Vec<_>>(), [2]);
    assert_eq!(r.failed.len(), 1);
    assert_eq!(r.failed[0].track.id, 1);
    let said = warnings.said();
    assert!(said.iter().any(|w| w.starts_with("couldn't fingerprint")), "{said:?}");

    // Remembered: the next run reports it from the cache, as it was.
    let audio = paths::to_fs(&root, &paths::key(&library.track(1).unwrap().file_path));
    assert_eq!(cache.failure(&Cache::key(&audio).unwrap()), Some(r.failed[0].error.clone()));
    let again = Identifier::new(&device, &cache, &Ctx::silent()).unwrap();
    assert_eq!(again.report.failed[0].error, r.failed[0].error);
    assert!(cache.root().starts_with(store.root()));
}

/// A cache that can't be written is a warning, not a failure: the drive is still indexed.
#[test]
fn a_cache_that_cant_be_written_is_only_a_warning() {
    let dir = tempfile::tempdir().unwrap();
    let device = Device::open_export(common::demo_drive(dir.path(), "DNA")).unwrap();
    std::fs::write(dir.path().join("cache"), b"a file where the folder goes").unwrap();
    let warnings = Warnings::default();
    let id = Identifier::new(&device, &Cache::new(dir.path().join("cache")), &warnings.ctx());
    assert_eq!(id.unwrap().report.failed.len(), 2);
    let said = warnings.said();
    assert!(said.iter().any(|w| w.starts_with("couldn't cache a fingerprint")), "{said:?}");
}
