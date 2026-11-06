//! Pruning a store's snapshots when it can't go ahead: a library named that the store doesn't
//! have, a cancel while it reads what snapshots hold, store records it can't read, and another
//! write holding the store. Each stops with nothing removed; the library is found by its name
//! in any case, with spaces around it.

use crate::common;

use std::fs;
use std::path::Path;

use hegel::generators as gs;
use hegel::{HealthCheck, TestCase};
use locallibrary::device::Device;
use locallibrary::progress::Ctx;
use locallibrary::retention::{self, PruneOptions};
use locallibrary::store::Store;
use locallibrary::sync::{self, PullOptions};
use locallibrary::{CancelToken, Error};

/// The store records a draw damages: files by name, a snapshot's record, or only the list of
/// files in it.
const RECORDS: &[&str] = &["libraries.json", "labels.json", "drives.json", "snapshot", "snapshot files"];

fn pull(store: &Store, root: &Path) {
    sync::pull(store, &mut Device::open_export(root).unwrap(), &PullOptions::default(), &Ctx::silent()).unwrap();
}

/// Pruning named a library the store lacks, cancelled, refused the store's lock or unable to read
/// a store record stops, and the store keeps every snapshot.
#[hegel::test(test_cases = 3, suppress_health_check = [HealthCheck::TooSlow])]
#[hegel::explicit_test_case(pad = " ", record = "libraries.json", by_name = false)]
#[hegel::explicit_test_case(pad = " ", record = "labels.json", by_name = true)]
#[hegel::explicit_test_case(pad = " ", record = "drives.json", by_name = false)]
#[hegel::explicit_test_case(pad = " ", record = "snapshot", by_name = true)]
#[hegel::explicit_test_case(pad = " ", record = "snapshot files", by_name = false)]
fn prop_pruning_that_cant_go_ahead_removes_nothing(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    let root = common::bigger_drive(dir.path(), "A");
    // Adopted, so the store remembers the state the drive was last seen in.
    Device::open_export(&root).unwrap().adopt(Some("A")).unwrap();
    pull(&store, &root);
    common::export_new_track(&root, "Later", "Second");
    pull(&store, &root);
    let name = store.libraries().unwrap()[0].name.clone();
    let ids = || {
        let mut ids = store.snapshot_ids().unwrap();
        ids.sort_by_key(|h| h.to_hex());
        ids
    };
    let all = ids();
    let pad = tc.draw(gs::sampled_from(vec!["", " ", "  \t"]));
    let named = |library: &str| PruneOptions {
        keep_last: Some(1),
        library: Some(format!("{pad}{library}{pad}")),
        ..Default::default()
    };

    let unknown = retention::plan(&store, &named("Nowhere"), &Ctx::silent()).unwrap_err();
    assert!(matches!(&unknown, Error::NotFound(m) if m.contains("\"Nowhere\"")), "{unknown}");
    // Cancelled once it reads the snapshot it would remove, to see what history lists it holds.
    let cancel = CancelToken::new();
    cancel.cancel();
    let stopped = retention::plan(&store, &named(&name), &Ctx { cancel, ..Ctx::silent() });
    assert!(matches!(stopped, Err(Error::Cancelled)), "{stopped:?}");
    let plan = retention::plan(&store, &named(&name), &Ctx::silent()).unwrap();
    assert_eq!(plan.removed_snapshots, 1);
    {
        let _other_write = store.lock().unwrap();
        let held = retention::apply(&store, &plan);
        assert!(matches!(held, Err(Error::Locked(_))), "{held:?}");
    }
    assert_eq!(ids(), all, "nothing removed");

    // A store record that can't be read stops the plan.
    let record = tc.draw(gs::sampled_from(RECORDS.to_vec()));
    let removed = &plan.libraries[0].removed[0];
    let manifest = store.root().join("snapshots").join(format!("{}.json", removed.id.to_hex()));
    match record {
        "snapshot" => fs::write(&manifest, b"{ not json").unwrap(),
        "snapshot files" => {
            let mut json: serde_json::Value = serde_json::from_slice(&fs::read(&manifest).unwrap()).unwrap();
            json["files"] = serde_json::json!("not a list");
            fs::write(&manifest, serde_json::to_vec(&json).unwrap()).unwrap();
        }
        file => fs::write(store.root().join(file), b"{ not json").unwrap(),
    }
    let by_name = tc.draw(gs::booleans());
    let opts = match by_name {
        true => named(&name),
        false => PruneOptions { keep_last: Some(1), ..Default::default() },
    };
    let e = retention::plan(&store, &opts, &Ctx::silent()).unwrap_err();
    assert!(matches!(e, Error::Parse { .. }), "{record}: {e}");
    assert_eq!(ids(), all);
}
