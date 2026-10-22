use super::*;
use crate::model::LibrarySummary;
use chrono::{DateTime, Utc};
use hegel::TestCase;
use hegel::generators as gs;

/// Where a snapshot of the library is: this store, the other computer's, or both.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Kept {
    Here,
    There,
    Both,
}

/// For any history of a library across the two stores (snapshots with any earlier ones as
/// parents, each here, there or both) and any latest on each side, the library is: left
/// alone when the other's latest can't be had, added when it's new here, moved to the other's
/// latest when that descends from this store's (or this store has none), kept when this
/// store's is as new or newer, and kept, though both changed, otherwise. Only what's added or
/// moved is recorded, at the other's latest.
#[hegel::test(test_cases = 30)]
fn a_library_follows_the_other_store_only_forwards(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    let library = Uuid::from_u128(7);
    let at: DateTime<Utc> = DateTime::from_timestamp(1_790_000_000, 0).unwrap();
    let n = tc.draw(gs::integers::<usize>().min_value(1).max_value(6));
    let (mut ids, mut parents, mut kept) = (Vec::new(), Vec::<Vec<usize>>::new(), Vec::new());
    let mut catalog =
        Catalog { store: Uuid::from_u128(1), libraries: Vec::new(), snapshots: Vec::new(), labels: Vec::new() };
    // Each fetched snapshot there-only: as if this fetch had just copied it.
    let mut fetched = FxHashSet::default();
    for k in 0..n {
        let mine: Vec<usize> = match k {
            0 => Vec::new(),
            _ => {
                let mut p = tc.draw(gs::vecs(gs::integers::<usize>().max_value(k - 1)).min_size(1).max_size(2));
                p.sort();
                p.dedup();
                p
            }
        };
        let snap = Snapshot {
            id: Hash::of(b""),
            library_id: library,
            parents: mine.iter().map(|&p| ids[p]).collect(),
            created: at,
            message: Some(format!("snapshot {k}")),
            source: None,
            fingerprint: Hash::of(&[k as u8]),
            summary: LibrarySummary::default(),
            lineage: Vec::new(),
            files: Vec::new(),
        };
        let where_ = tc.draw(gs::sampled_from(vec![Kept::Here, Kept::There, Kept::Both]));
        let id = match where_ {
            Kept::There => snap.compute_id(),
            _ => store.save_snapshot(snap.clone()).unwrap().id,
        };
        if where_ != Kept::Here {
            catalog.snapshots.push(CatalogSnapshot { id, library, parents: snap.parents.clone() });
            if where_ == Kept::There && tc.draw(gs::booleans()) {
                fetched.insert(id);
            }
        }
        ids.push(id);
        parents.push(mine);
        kept.push(where_);
    }
    let index = |pick: usize, f: &dyn Fn(Kept) -> bool| (0..n).filter(|&k| f(kept[k])).nth(pick);
    let count = |f: &dyn Fn(Kept) -> bool| (0..n).filter(|&k| f(kept[k])).count();
    let there = |k: Kept| k != Kept::Here;
    let here = |k: Kept| k != Kept::There;
    // The other's latest: one it has, or none; this store's: one it has, none, or no library.
    // Mostly a latest on both sides, so their histories are compared.
    let pick = |m: usize, f: &dyn Fn(Kept) -> bool| {
        let p = tc.draw(gs::integers::<usize>().max_value(m.max(1) - 1));
        (m > 0 && tc.draw(gs::weighted_booleans(0.85))).then(|| index(p, f)).flatten()
    };
    let remote = pick(count(&there), &there);
    let local: Option<Option<usize>> = tc.draw(gs::weighted_booleans(0.85)).then(|| pick(count(&here), &here));
    if let Some(head) = local {
        store.add_library(library, "Home", head.map(|k| ids[k])).unwrap();
    }
    let remote_record =
        LibraryRecord { id: library, name: "Home".into(), head: remote.map(|k| ids[k]), created: at, updated: at };
    catalog.libraries.push(remote_record.clone());

    // Whether `a` is `b` or descends from it.
    let reaches = |a: usize, b: usize| {
        let mut queue = vec![a];
        let mut seen = FxHashSet::default();
        while let Some(k) = queue.pop() {
            if k == b {
                return true;
            }
            if seen.insert(k) {
                queue.extend(&parents[k]);
            }
        }
        false
    };
    let had = |k: usize| kept[k] != Kept::There || fetched.contains(&ids[k]);
    let want = match (local, remote) {
        (_, Some(r)) if !had(r) => LibraryOutcome::Unavailable,
        (None, _) => LibraryOutcome::Added,
        (_, None) => LibraryOutcome::UpToDate,
        (Some(None), Some(_)) => LibraryOutcome::Updated,
        (Some(Some(l)), Some(r)) if reaches(l, r) => LibraryOutcome::UpToDate,
        (Some(Some(l)), Some(r)) if reaches(r, l) => LibraryOutcome::Updated,
        _ => LibraryOutcome::Diverged,
    };
    let mut applied = Vec::new();
    let got = outcomes(&store, &catalog, &[remote_record], &fetched, &mut |lib| {
        applied.push(lib.outcome);
        apply(&store, lib)
    })
    .unwrap();
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].outcome, want, "{kept:?} {parents:?} here {local:?} there {remote:?}");
    assert_eq!((got[0].id, got[0].head, got[0].name.as_str()), (library, remote.map(|k| ids[k]), "Home"));
    let recorded = matches!(want, LibraryOutcome::Added | LibraryOutcome::Updated);
    assert_eq!(applied, if recorded { vec![want] } else { Vec::new() });
    let head = store.library_by_id(library).unwrap().map(|l| l.head);
    let expected = match recorded {
        true => Some(remote.map(|k| ids[k])),
        false => local.map(|h| h.map(|k| ids[k])),
    };
    assert_eq!(head, expected);
}
