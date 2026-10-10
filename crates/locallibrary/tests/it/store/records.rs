//! The store's own records: opening one, its libraries, forgotten snapshots and its size.

use crate::common;

use indexmap::IndexMap;
use std::fs;

use locallibrary::device::Device;
use locallibrary::error::Reason;
use locallibrary::progress::Ctx;
use locallibrary::store::Store;
use locallibrary::sync::{self, PullOptions};
use locallibrary::{Error, Hash};

#[test]
fn only_a_store_this_version_can_read_is_opened() {
    let dir = tempfile::tempdir().unwrap();
    let err = Store::open(dir.path()).unwrap_err();
    assert!(matches!(&err, Error::NotFound(what) if what.starts_with("no locallibrary store at ")), "{err}");

    let root = dir.path().join("store");
    Store::open_or_init(&root).unwrap();
    let meta = root.join("store.json");
    let mut json: serde_json::Value = serde_json::from_slice(&fs::read(&meta).unwrap()).unwrap();
    json["format"] = 999.into();
    fs::write(&meta, json.to_string()).unwrap();
    let err = Store::open(&root).unwrap_err();
    assert!(
        err.to_string().starts_with("store format 999 is newer than this version of locallibrary supports"),
        "{err}"
    );
}

#[test]
fn a_library_keeps_a_name() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    let lib = store.create_library("Main").unwrap();
    assert!(matches!(store.rename_library(lib.id, "  "), Err(Error::Invalid(_))));
    assert_eq!(store.libraries().unwrap()[0].name, "Main");
}

#[test]
fn the_latest_snapshot_stays_and_history_passes_over_forgotten_ones() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    let mut d = Device::open(common::demo_drive(dir.path(), "Main")).unwrap();
    let mut ids: Vec<Hash> = Vec::new();
    for title in ["", "Second", "Third"] {
        if !title.is_empty() {
            common::export_new_track(d.root(), "New", title);
        }
        let r = sync::pull(&store, &mut d, &PullOptions::default(), &Ctx::silent()).unwrap();
        ids.push(r.snapshot.unwrap().id);
    }
    let err = store.forget_snapshots(&[ids[2]]).unwrap_err();
    assert_eq!(err.reason(), Some(Reason::SnapshotKept), "{err}");

    store.forget_snapshots(&[ids[1]]).unwrap();
    let lib = store.libraries().unwrap().remove(0);
    let history: Vec<Hash> = store.history(&lib, None).unwrap().into_iter().map(|s| s.id).collect();
    assert_eq!(history, [ids[2], ids[0]]);
    assert_eq!(store.history(&lib, Some(1)).unwrap().len(), 1);
}

#[test]
fn stats_count_each_stored_file_once() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    let mut d = Device::open(common::demo_drive(dir.path(), "Main")).unwrap();
    let r = sync::pull(&store, &mut d, &PullOptions::default(), &Ctx::silent()).unwrap();
    let snap = store.load_snapshot(&r.snapshot.unwrap().id).unwrap();
    let objects: IndexMap<Hash, u64> = snap.files.iter().map(|f| (f.hash.unwrap(), f.size)).collect();

    let stats = store.stats().unwrap();
    assert_eq!((stats.libraries, stats.snapshots, stats.drives), (1, 1, 1));
    assert_eq!((stats.objects, stats.object_bytes), (objects.len(), objects.values().sum()));
}
