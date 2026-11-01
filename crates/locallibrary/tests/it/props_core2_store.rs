//! Snapshots read back from the store: a snapshot's library is the drive's as it was pulled,
//! from export.pdb or, without one, OneLibrary; pruning keeps an older snapshot for a history list
//! only it holds, or because its databases can't be read, and what's pruned still links a
//! snapshot to its ancestors; labels and the store's counts.

use crate::common;
use crate::drawn;

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use hegel::generators as gs;
use hegel::{HealthCheck, TestCase};
use locallibrary::compare::Side;
use locallibrary::device::Device;
use locallibrary::hash::Hash;
use locallibrary::progress::Ctx;
use locallibrary::retention::{self, KeepReason, PruneOptions};
use locallibrary::store::{Snapshot, Store};
use locallibrary::sync::{self, PullOptions};
use locallibrary::{Library, paths, plays};
use rekordbox_pdb::PdbEditor;

/// A library's track files, comparable across databases.
fn files(lib: &Library) -> BTreeSet<String> {
    lib.tracks.iter().map(|t| paths::file_key(&t.file_path)).collect()
}

/// Pull the drive at `root` into `store`: the snapshot made.
fn pull(store: &Store, root: &Path) -> Snapshot {
    let report =
        sync::pull(store, &mut Device::open_export(root).unwrap(), &PullOptions::default(), &Ctx::silent()).unwrap();
    store.load_snapshot(&report.snapshot.unwrap().id).unwrap()
}

/// Overwrite the stored copy of `key` in `snapshot` with bytes no database reader takes.
fn rot_object(store: &Store, snapshot: &Snapshot, key: &str) {
    let h = snapshot.files.iter().find(|f| f.path == key).unwrap().hash.unwrap();
    let p = store.object_path(&h);
    fs::remove_file(&p).unwrap();
    fs::write(&p, b"not a database at all").unwrap();
}

/// A snapshot's side of a comparison holds the library pulled: export.pdb's, or OneLibrary's on a
/// drive without export.pdb. Without either readable, there's no side.
#[hegel::test(test_cases = 3, suppress_health_check = [HealthCheck::TooSlow])]
fn a_snapshots_side_is_the_library_pulled(tc: TestCase) {
    for plus_only in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open_or_init(dir.path().join("store")).unwrap();
        let (root, _) = drawn::drive(&tc, dir.path(), "A");
        if plus_only {
            common::onelibrary_only(&root);
        }
        let want = files(&Device::open_export(&root).unwrap().library().unwrap());
        let snap = pull(&store, &root);
        let side = Side::from_snapshot(&store, &snap, "store").unwrap();
        assert_eq!(files(&side.library), want);
        assert_eq!(side.sequence.is_some(), !plus_only, "only export.pdb has a sequence");
        assert_eq!(side.fingerprint, Some(snap.fingerprint));
        let key = if plus_only { paths::LIBRARY_PLUS_DB } else { paths::EXPORT_PDB };
        rot_object(&store, &snap, key);
        assert!(Side::from_snapshot(&store, &snap, "store").is_err());
    }
}

/// Which of a drive's three snapshots pruning to the last one keeps: the first, when it alone
/// holds a history list (rekordbox deletes them once it imports them) or its databases can't be
/// read; never the middle one. After pruning, the first is still the latest's ancestor.
#[hegel::test(test_cases = 3, suppress_health_check = [HealthCheck::TooSlow])]
#[hegel::explicit_test_case(plus = true, imported = true, rotted = 2)]
#[hegel::explicit_test_case(plus = false, imported = true, rotted = 1)]
#[hegel::explicit_test_case(plus = false, imported = false, rotted = 0)]
fn pruning_keeps_the_only_snapshot_of_a_history_list(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    let root = common::bigger_drive(dir.path(), "A");
    let plus = tc.draw(gs::booleans());
    let pdb = root.join(paths::EXPORT_PDB);
    let mut ed = PdbEditor::from_file(&pdb).unwrap();
    let ids: Vec<u32> = ed.database().unwrap().tracks.iter().map(|t| t.id).take(3).collect();
    let history = ed.create_history_playlist("HISTORY 001").unwrap();
    for t in &ids {
        ed.add_to_history(history, *t).unwrap();
    }
    ed.save(&pdb).unwrap();
    let mirror = || {
        if plus {
            common::add_onelibrary(&root);
        }
    };
    mirror();
    let first = pull(&store, &root);
    let imported = tc.draw(gs::booleans());
    if imported {
        let mut ed = PdbEditor::from_file(&pdb).unwrap();
        ed.remove_history_playlists(&[history]).unwrap();
        ed.save(&pdb).unwrap();
    }
    common::export_new_track(&root, "Later", "Second");
    mirror();
    let middle = pull(&store, &root);
    common::export_new_track(&root, "Later", "Third");
    mirror();
    let last = pull(&store, &root);
    // What rotted in the first snapshot's stored databases: nothing, export.pdb, or OneLibrary.
    let rotted = tc.draw(gs::sampled_from(if plus { vec![0, 1, 2] } else { vec![0, 1] }));
    match rotted {
        1 => rot_object(&store, &first, paths::EXPORT_PDB),
        2 => rot_object(&store, &first, paths::LIBRARY_PLUS_DB),
        _ => {}
    }

    // The history archive reads every snapshot it can, and says which it couldn't.
    let archive = plays::archive(&store, None, &Ctx::silent()).unwrap();
    assert_eq!(archive.snapshots, 3);
    assert_eq!(archive.warnings.iter().any(|w| w.contains(&first.id.short())), rotted == 1, "{:?}", archive.warnings);
    let archived = archive.lists.iter().any(|l| l.list.name == "HISTORY 001");
    assert_eq!(archived, rotted != 1 || !imported, "only the first snapshot holds it unless it stayed");

    assert!(retention::plan(&store, &PruneOptions::default(), &Ctx::silent()).is_err(), "nothing said to keep");
    let opts = PruneOptions { keep_last: Some(1), ..Default::default() };
    let plan = retention::plan(&store, &opts, &Ctx::silent()).unwrap();
    assert_eq!(plan.libraries.len(), 1);
    let lib = &plan.libraries[0];
    let kept = |id: Hash| lib.kept.iter().find(|k| k.snapshot.id == id);
    assert!(kept(last.id).is_some_and(|k| k.reasons.contains(&KeepReason::Latest)));
    assert!(kept(middle.id).is_none(), "{:?}", lib.kept);
    // export.pdb unreadable: what the first holds isn't known. OneLibrary unreadable beside a
    // readable export.pdb: export.pdb says.
    let want_first = match (rotted, imported) {
        (1, _) => Some(KeepReason::Unreadable),
        (_, true) => Some(KeepReason::History),
        (_, false) => None,
    };
    let first_kept = kept(first.id);
    assert_eq!(first_kept.map(|k| k.reasons.clone()), want_first.map(|r| vec![r]), "{:?}", plan.warnings);
    assert_eq!(plan.warnings.iter().any(|w| w.contains(&first.id.short())), rotted == 1, "{:?}", plan.warnings);
    if want_first == Some(KeepReason::History) {
        assert_eq!(first_kept.unwrap().history, ["HISTORY 001"]);
    }

    let done = retention::apply(&store, &plan).unwrap();
    assert_eq!(done.removed_snapshots, 1 + usize::from(want_first.is_none()));
    if want_first.is_some() {
        // Linked through the pruned middle snapshot.
        assert!(store.is_ancestor(&first.id, &last.id).unwrap());
    }
    assert!(!store.is_ancestor(&last.id, &first.id).unwrap());
    // The history walks on through what was pruned, and stops at a snapshot that can't be read.
    let library = store.libraries().unwrap().remove(0);
    let listed: Vec<Hash> = store.history(&library, None).unwrap().into_iter().map(|s| s.id).collect();
    let want: Vec<Hash> = [last.id].into_iter().chain(want_first.map(|_| first.id)).collect();
    assert_eq!(listed, want);
    let manifest = store.root().join("snapshots").join(format!("{}.json", last.id.to_hex()));
    fs::write(&manifest, b"{ not json").unwrap();
    assert!(store.history(&library, None).is_err());
}

/// A label goes on one snapshot: labelling it again is a no-op, and another snapshot can't take
/// the name. The store counts only the files in its object folders whose names are hashes.
#[hegel::test(test_cases = 3, suppress_health_check = [HealthCheck::TooSlow])]
fn labels_and_counts(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    let root = common::bigger_drive(dir.path(), "A");
    let a = pull(&store, &root);
    common::export_new_track(&root, "Later", "Second");
    let b = pull(&store, &root);
    let name = tc.draw(gs::sampled_from(vec!["gig", "Gig", "before the festival"]));
    let first = store.add_label(&a.id, name).unwrap();
    let again = store.add_label(&a.id, &name.to_uppercase()).unwrap();
    assert_eq!((again.snapshot, again.name), (first.snapshot, first.name));
    assert!(store.add_label(&b.id, name).is_err());

    let objects: BTreeSet<Hash> = a.files.iter().chain(&b.files).filter_map(|f| f.hash).collect();
    let before = store.stats().unwrap();
    assert_eq!(before.objects, objects.len());
    assert_eq!(before.snapshots, 2);
    // Strays among the objects: a file whose name isn't a hash, and a file where a shard folder goes.
    let shard = store.object_path(objects.iter().next().unwrap()).parent().unwrap().to_path_buf();
    let stray = tc.draw(gs::sampled_from(vec!["notes.txt", "zz"]));
    fs::write(shard.join(stray), b"stray").unwrap();
    fs::write(store.root().join("objects").join("ab.txt"), b"stray").unwrap();
    let after = store.stats().unwrap();
    assert_eq!((after.objects, after.object_bytes), (before.objects, before.object_bytes));
}
