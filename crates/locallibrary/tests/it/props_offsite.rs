//! A store's history pruned and copied elsewhere, from drawn sequences of pulls (new exports and
//! history lists between them), labels and damage. Pruning keeps exactly what its plan says and
//! every history list the store has seen; a copy holds the store's snapshots and files
//! byte for byte, is brought up to date incrementally, leaves out what's damaged rather than
//! copying it broken, and refuses to go over a copy written to since.

use crate::common;

use std::collections::BTreeSet;
use std::fs;

use hegel::generators as gs;
use hegel::{HealthCheck, TestCase};
use locallibrary::device::Device;
use locallibrary::offsite::{self, CopyOptions};
use locallibrary::progress::Ctx;
use locallibrary::retention::{self, KeepReason, PruneOptions};
use locallibrary::store::Store;
use locallibrary::sync::{self, PullOptions};
use locallibrary::{Error, Hash, plays};

fn ids(store: &Store) -> BTreeSet<Hash> {
    store.snapshot_ids().unwrap().into_iter().collect()
}

/// Every history list in any of the store's snapshots, by identity.
fn lists(store: &Store) -> BTreeSet<String> {
    let archive = plays::archive(store, None, &Ctx::silent()).unwrap();
    archive.lists.iter().map(|a| a.list.fingerprint()).collect()
}

fn manifest(store: &Store, id: &Hash) -> Vec<u8> {
    fs::read(store.root().join("snapshots").join(format!("{}.json", id.to_hex()))).unwrap()
}

/// The stored files, by hash and size, as found on disk.
fn objects(store: &Store) -> BTreeSet<(Hash, u64)> {
    walkdir::WalkDir::new(store.root().join("objects"))
        .into_iter()
        .map(Result::unwrap)
        .filter(|e| e.file_type().is_file())
        .filter_map(|e| {
            let shard = e.path().parent()?.file_name()?.to_string_lossy().into_owned();
            let hash: Hash = format!("{shard}{}", e.file_name().to_string_lossy()).parse().ok()?;
            Some((hash, e.metadata().unwrap().len()))
        })
        .collect()
}

/// A store holding a drive's library as pulled `pulls` times, the drive changing in between.
#[hegel::test_helper]
fn pulled(tc: &TestCase, dir: &std::path::Path) -> Store {
    let store = Store::open_or_init(dir.join("store")).unwrap();
    let root = common::bigger_drive(dir, "Main");
    let mut d = Device::open_export(&root).unwrap();
    d.adopt(Some("Main")).unwrap();
    for n in 0..tc.draw(gs::integers::<usize>().min_value(1).max_value(5)) {
        match tc.draw(gs::integers::<u8>().max_value(2)) {
            0 => {
                common::export_new_track(&root, "New", &format!("Export {n}"));
            }
            1 => common::add_history(&root, &format!("HISTORY {n:03}"), &[1 + n as u32]),
            _ => {}
        }
        let mut d = Device::open_export(&root).unwrap();
        sync::pull(&store, &mut d, &PullOptions::default(), &Ctx::silent()).unwrap();
    }
    store
}

#[hegel::test(test_cases = 8, suppress_health_check = [HealthCheck::TooSlow])]
fn pruning_removes_exactly_its_plan_and_keeps_every_history_list(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let store = pulled(&tc, dir.path());
    let all: Vec<Hash> = ids(&store).into_iter().collect();
    for id in all.iter().filter(|_| tc.draw(gs::weighted_booleans(0.2))) {
        store.add_label(id, &format!("keep {}", id.short())).unwrap();
    }
    let heard = lists(&store);
    let opts = PruneOptions { keep_last: Some(tc.draw(gs::integers::<usize>().max_value(2))), ..Default::default() };
    let plan = retention::plan(&store, &opts, &Ctx::silent()).unwrap();
    assert!(plan.dry_run);
    assert_eq!(ids(&store).len(), all.len(), "planning changes nothing");
    let lib = &plan.libraries[0];
    let kept: BTreeSet<Hash> = lib.kept.iter().map(|k| k.snapshot.id).collect();
    let removed: BTreeSet<Hash> = lib.removed.iter().map(|s| s.id).collect();
    assert!(kept.is_disjoint(&removed));
    assert_eq!(kept.union(&removed).copied().collect::<BTreeSet<_>>(), ids(&store));
    assert!(lib.kept.iter().all(|k| !k.reasons.is_empty()));
    let head = store.libraries().unwrap()[0].head.unwrap();
    assert!(lib.kept.iter().any(|k| k.snapshot.id == head && k.reasons.contains(&KeepReason::Latest)));
    for k in &lib.kept {
        assert_eq!(
            k.reasons.contains(&KeepReason::Label),
            !store.labels_by_snapshot().unwrap().get(&k.snapshot.id).is_none_or(Vec::is_empty)
        );
    }
    assert_eq!(plan.removed_snapshots, removed.len());

    let done = retention::apply(&store, &plan).unwrap();
    assert_eq!(ids(&store), kept);
    assert_eq!(done.removed_objects, plan.removed_objects, "the plan said what would be freed");
    assert_eq!(lists(&store), heard, "every history list is still in some snapshot");
    let verified = store.verify(&Ctx::silent()).unwrap();
    assert!(verified.corrupt.is_empty() && verified.missing.is_empty(), "{verified:?}");
    // Pruned again with the same policy, nothing more goes.
    let again = retention::plan(&store, &opts, &Ctx::silent()).unwrap();
    assert_eq!(again.removed_snapshots, 0);
}

#[hegel::test(test_cases = 4, suppress_health_check = [HealthCheck::TooSlow])]
fn a_copy_holds_the_store_and_catches_up_incrementally(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let store = pulled(&tc, dir.path());
    let dest = dir.path().join("NAS");
    let first = offsite::copy(&store, &dest, &CopyOptions::default(), &Ctx::silent()).unwrap();
    assert!(first.complete && first.problems.is_empty());
    let copy = Store::open(&dest).unwrap();
    assert_eq!(copy.id().unwrap(), store.id().unwrap());
    assert_eq!(ids(&copy), ids(&store));
    for id in ids(&store) {
        assert_eq!(manifest(&copy, &id), manifest(&store, &id));
    }
    let stored = objects(&store);
    assert_eq!(objects(&copy), stored);
    assert_eq!(first.objects_copied, stored.len());
    let status = offsite::copies(&store).unwrap();
    assert_eq!((status.len(), status[0].available, status[0].up_to_date), (1, true, Some(true)));
    assert_eq!(offsite::marker(&dest).unwrap().unwrap().store, store.id().unwrap());

    // Once up to date, copying again copies nothing.
    let again = offsite::copy(
        &store,
        &dest,
        &CopyOptions { verify: tc.draw(gs::booleans()), ..Default::default() },
        &Ctx::silent(),
    )
    .unwrap();
    assert_eq!((again.objects_copied, again.snapshots_copied, again.objects_repaired), (0, 0, 0));

    // The store moves on; a dry run says what the copy then does.
    let root = dir.path().join("Main");
    common::export_new_track(&root, "Later", "Export");
    sync::pull(&store, &mut Device::open_export(&root).unwrap(), &PullOptions::default(), &Ctx::silent()).unwrap();
    let prune = tc.draw(gs::booleans());
    if prune {
        let plan = retention::plan(&store, &PruneOptions { keep_last: Some(1), ..Default::default() }, &Ctx::silent())
            .unwrap();
        retention::apply(&store, &plan).unwrap();
    }
    let mirror = tc.draw(gs::booleans());
    let opts = CopyOptions { mirror, ..Default::default() };
    let dry = offsite::copy(&store, &dest, &CopyOptions { dry_run: true, ..opts.clone() }, &Ctx::silent()).unwrap();
    let real = offsite::copy(&store, &dest, &opts, &Ctx::silent()).unwrap();
    assert_eq!(
        (dry.objects_copied, dry.bytes_copied, dry.snapshots_copied),
        (real.objects_copied, real.bytes_copied, real.snapshots_copied)
    );
    assert_eq!((dry.removed_snapshots, dry.removed_objects), (real.removed_snapshots, real.removed_objects));
    assert!(real.snapshots_copied >= 1);
    match mirror {
        true => assert_eq!(ids(&copy), ids(&store)),
        false => assert!(ids(&copy).is_superset(&ids(&store))),
    }
    assert_eq!(copy.libraries().unwrap()[0].head, store.libraries().unwrap()[0].head);

    // Written to since (a label, as `--store` would add), it's only copied over when forced.
    let any = *ids(&copy).iter().next().unwrap();
    copy.add_label(&any, "on the NAS").unwrap();
    let refused = offsite::copy(&store, &dest, &CopyOptions::default(), &Ctx::silent()).unwrap_err();
    assert!(matches!(refused.inner(), Error::Conflict(_)), "{refused}");
    offsite::copy(&store, &dest, &CopyOptions { force: true, ..Default::default() }, &Ctx::silent()).unwrap();

    // A file damaged in the store isn't copied: its snapshots are left out, the rest go.
    let (h, _) = *objects(&store).iter().next().unwrap();
    let fresh = dir.path().join("Disk");
    fs::write(store.object_path(&h), b"rotten").unwrap();
    let r = offsite::copy(&store, &fresh, &CopyOptions::default(), &Ctx::silent()).unwrap();
    assert!(!r.complete && !r.problems.is_empty());
    let left_out: BTreeSet<Hash> = ids(&store)
        .into_iter()
        .filter(|s| store.load_snapshot(s).unwrap().files.iter().any(|f| f.hash == Some(h)))
        .collect();
    let fresh_copy = Store::open(&fresh).unwrap();
    assert!(ids(&fresh_copy).is_disjoint(&left_out));
    assert_eq!(ids(&fresh_copy).len() + left_out.len(), ids(&store).len());
    assert!(!fresh_copy.has_object(&h));
}
