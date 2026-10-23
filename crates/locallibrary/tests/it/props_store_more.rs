//! The store's edges: it's made only in a folder that's empty or holds only its own files, and
//! refuses a format newer than it reads; a library's history is the same however much of it is
//! asked for; a pruned snapshot is named as pruned; a label goes only on a snapshot the store has;
//! and what writes cut off by a crash left behind goes with the next clean-up.

use crate::common;

use std::fs;

use hegel::generators as gs;
use hegel::{HealthCheck, TestCase};
use locallibrary::device::Device;
use locallibrary::hash::Hash;
use locallibrary::paths;
use locallibrary::progress::Ctx;
use locallibrary::store::Store;
use locallibrary::sync::{self, PullOptions};

#[hegel::test(test_cases = 20)]
fn a_store_is_made_only_where_it_can_be(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("store");
    fs::create_dir_all(&root).unwrap();
    // The files a store's folder may hold before it's made (the TUI's and the profile's), or another.
    let name = tc.draw(gs::sampled_from(vec!["tui.json", "profile-document.json", "notes.txt", "Music"]));
    fs::write(root.join(name), b"{}").unwrap();
    let ours = matches!(name, "tui.json" | "profile-document.json");
    match Store::open_or_init(&root) {
        Ok(store) => {
            assert!(ours, "made a store over {name}");
            // A store written by a newer locallibrary isn't read.
            let meta = store.root().join("store.json");
            let mut json: serde_json::Value = serde_json::from_slice(&fs::read(&meta).unwrap()).unwrap();
            let newer = json["format"].as_u64().unwrap() + tc.draw(gs::integers::<u64>().min_value(1).max_value(3));
            json["format"] = newer.into();
            fs::write(&meta, serde_json::to_vec(&json).unwrap()).unwrap();
            let err = Store::open(&root).unwrap_err();
            assert!(err.to_string().contains("is newer than this version"), "{err}");
        }
        Err(e) => {
            assert!(!ours, "{e}");
            assert!(e.to_string().contains("is not empty"), "{e}");
        }
    }
}

#[hegel::test(test_cases = 5, suppress_health_check = [HealthCheck::TooSlow])]
fn a_librarys_history_reads_alike_however_much_is_asked_for(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    let root = common::bigger_drive(dir.path(), "A");
    let pulls = tc.draw(gs::integers::<usize>().min_value(2).max_value(4));
    let mut library = String::new();
    for n in 0..pulls {
        if n > 0 {
            common::export_new_track(&root, "Later", &format!("Export {n}"));
        }
        library = sync::pull(&store, &mut Device::open_export(&root).unwrap(), &PullOptions::default(), &Ctx::silent())
            .unwrap()
            .library;
    }
    let lib = store.find_library(&library).unwrap().unwrap();
    let all = store.history(&lib, None).unwrap();
    assert_eq!(all.len(), pulls);
    let n = tc.draw(gs::integers::<usize>().max_value(pulls + 1));
    let some = store.history(&lib, Some(n)).unwrap();
    assert_eq!(some.iter().map(|s| s.id).collect::<Vec<_>>(), all.iter().take(n).map(|s| s.id).collect::<Vec<_>>());

    // A label only on a snapshot the store has.
    let stranger = Hash::of(b"no snapshot has this id");
    assert!(store.add_label(&stranger, "Gig").is_err_and(|e| e.to_string().contains("snapshot")));
    store.add_label(&all[0].id, "Gig").unwrap();
    // An older snapshot pruned: asked for by its id, it's said to have been pruned.
    let older = tc.draw(gs::sampled_from(&all[1..])).id;
    store.forget_snapshots(&[older]).unwrap();
    let err = store.resolve_snapshot(&older.to_hex()[..8]).unwrap_err();
    assert!(err.to_string().contains("was pruned on"), "{err}");
    assert_eq!(store.history(&lib, None).unwrap().len(), pulls - 1, "history walks through it");

    // What a cut-off write left: temporary files beside stored ones and snapshots, and staged ones.
    let leftovers = [
        store.root().join("objects").join(format!("{}a", paths::TEMP_PREFIX)),
        store.root().join("snapshots").join(format!("{}b", paths::TEMP_PREFIX)),
        store.root().join("objects/tmp/staged"),
    ];
    let left: Vec<_> = tc.draw(gs::subsequences(&leftovers));
    for p in &left {
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, b"half written").unwrap();
    }
    let dry = store.gc(true).unwrap();
    assert!(dry.removed_objects >= left.len() && left.iter().all(|p| p.is_file()), "a dry run removes nothing");
    store.gc(false).unwrap();
    assert!(left.iter().all(|p| !p.exists()));
    let checked = store.verify(&Ctx::silent()).unwrap();
    assert!(checked.corrupt.is_empty() && checked.missing.is_empty(), "the clean-up took nothing a snapshot uses");
}
