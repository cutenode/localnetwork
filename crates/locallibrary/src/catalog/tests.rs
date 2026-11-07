use super::*;
use crate::model::LibrarySummary;
use crate::paths::FileClass;
use crate::tree::FileEntry;
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

/// A snapshot of `library` made `k`-th, with these parents and files.
fn made(library: Uuid, k: usize, parents: Vec<Hash>, files: Vec<FileEntry>) -> Snapshot {
    let mut snap = Snapshot {
        id: Hash::of(b""),
        library_id: library,
        parents,
        created: DateTime::from_timestamp(1_790_000_000, 0).unwrap(),
        message: Some(format!("snapshot {k}")),
        source: None,
        fingerprint: Hash::of(&[k as u8]),
        summary: LibrarySummary::default(),
        lineage: Vec::new(),
        files,
    };
    snap.id = snap.compute_id();
    snap
}

/// Where a snapshot of a history walked is, or what became of it.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Walked {
    Here,
    There,
    Pruned,
    Gone,
    Unreadable,
}

/// For any history whose snapshots are here, the other store's, pruned here, gone from both, or
/// unreadable here, one descends from another exactly when a path of parents joins them through
/// what's known of each (a gone one has none), unless the walk meets an unreadable snapshot, which
/// fails it, and only then.
#[hegel::test(test_cases = 40)]
fn prop_history_walks_through_pruned_gone_and_unreadable_snapshots(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    let library = Uuid::from_u128(7);
    let n = tc.draw(gs::integers::<usize>().min_value(1).max_value(7));
    let (mut ids, mut parents, mut kinds) = (Vec::new(), Vec::<Vec<usize>>::new(), Vec::new());
    let mut catalog: FxHashMap<Hash, Vec<Hash>> = FxHashMap::default();
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
        let snap = made(library, k, mine.iter().map(|&p| ids[p]).collect(), Vec::new());
        let kind = tc.draw(gs::sampled_from(vec![
            Walked::Here,
            Walked::There,
            Walked::Pruned,
            Walked::Gone,
            Walked::Unreadable,
        ]));
        match kind {
            Walked::Here => drop(store.save_snapshot(snap.clone()).unwrap()),
            Walked::There => drop(catalog.insert(snap.id, snap.parents.clone())),
            Walked::Pruned => {
                store.save_snapshot(snap.clone()).unwrap();
                store.forget_snapshots(&[snap.id]).unwrap();
            }
            Walked::Gone => {}
            Walked::Unreadable => std::fs::write(store.snapshot_path(&snap.id), b"{ not json").unwrap(),
        }
        ids.push(snap.id);
        parents.push(if kind == Walked::Gone { Vec::new() } else { mine });
        kinds.push(kind);
    }
    let (from, to) =
        (tc.draw(gs::integers::<usize>().max_value(n - 1)), tc.draw(gs::integers::<usize>().max_value(n - 1)));
    // Every snapshot reachable from `a` through what's known of each.
    let reachable = |a: usize| {
        let (mut queue, mut seen) = (vec![a], FxHashSet::default());
        while let Some(k) = queue.pop() {
            if seen.insert(k) {
                queue.extend(&parents[k]);
            }
        }
        seen
    };
    let seen = reachable(from);
    let meets_unreadable = seen.iter().any(|&k| kinds[k] == Walked::Unreadable);
    match descends(&store, &catalog, ids[from], ids[to]) {
        Ok(found) => assert_eq!(found, seen.contains(&to), "{kinds:?} {parents:?} {from} -> {to}"),
        Err(e) => assert!(meets_unreadable, "{e} from {kinds:?} {parents:?} {from} -> {to}"),
    }
    if !meets_unreadable {
        assert!(descends(&store, &catalog, ids[from], ids[to]).is_ok());
    }
}

/// Fetched manifests settle: the stored files they list are wanted unless the store holds them
/// (hashless entries aren't), a manifest reads back only as the snapshot it names, a library new
/// here is added under its name or the next free one (saying what it was), labels come along
/// unless taken or on a snapshot this store lacks, and the store lists the snapshots of its own
/// libraries only. A dry run says the same and changes nothing.
#[hegel::test(test_cases = 30)]
fn prop_fetched_snapshots_settle_under_free_names_with_their_labels(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    let home = store.create_library("Home").unwrap();
    let mine = store.save_snapshot(made(home.id, 0, Vec::new(), Vec::new())).unwrap();
    store.set_head(home.id, mine.id).unwrap();
    let label_taken = tc.draw(gs::booleans());
    if label_taken {
        store.add_label(&mine.id, "keep").unwrap();
    }
    // A snapshot of a library this store doesn't list.
    store.save_snapshot(made(Uuid::from_u128(99), 1, Vec::new(), Vec::new())).unwrap();

    let contents: Vec<Vec<u8>> = tc.draw(gs::vecs(gs::vecs(gs::integers::<u8>()).min_size(1).max_size(8)).max_size(4));
    let held: Vec<bool> = contents.iter().map(|_| tc.draw(gs::booleans())).collect();
    let entry = |i: usize, bytes: &[u8]| FileEntry {
        path: format!("Contents/{i}.mp3"),
        size: bytes.len() as u64,
        mtime: 0,
        class: FileClass::Audio,
        hash: Some(Hash::of(bytes)),
    };
    let files: Vec<FileEntry> = contents.iter().enumerate().map(|(i, b)| entry(i, b)).collect();
    for (bytes, _) in contents.iter().zip(&held).filter(|(_, h)| **h) {
        store.import_bytes(bytes).unwrap();
    }
    let remote = Uuid::from_u128(5);
    let theirs = made(remote, 2, Vec::new(), files.clone());
    let mut hashless = theirs.clone();
    hashless.files.push(FileEntry { hash: None, ..entry(9, b"unhashed") });
    let (wanted, present) = needed(&store, &[hashless]);
    let want_held: FxHashSet<Hash> = files.iter().zip(&held).filter(|(_, h)| **h).filter_map(|(f, _)| f.hash).collect();
    let want_missing: FxHashSet<Hash> =
        files.iter().filter_map(|f| f.hash).filter(|h| !want_held.contains(h)).collect();
    assert_eq!(present, want_held.len());
    assert_eq!(wanted.keys().copied().collect::<FxHashSet<_>>(), want_missing);

    let bytes = serde_json::to_vec(&theirs).unwrap();
    assert_eq!(checked(&theirs.id, &bytes).unwrap().id, theirs.id);
    assert!(checked(&theirs.id, &bytes[..bytes.len() / 2]).unwrap_err().contains("can't be read"));
    assert!(checked(&mine.id, &bytes).unwrap_err().contains("damaged"));

    // The files fetched.
    for bytes in &contents {
        store.import_bytes(bytes).unwrap();
    }
    let name = tc.draw(gs::sampled_from(vec!["Home", "home", "Away"]));
    let at = DateTime::from_timestamp(1_790_000_000, 0).unwrap();
    let record = LibraryRecord { id: remote, name: name.into(), head: Some(theirs.id), created: at, updated: at };
    let label = |name: &str, snapshot: Hash| SnapshotLabel { name: name.into(), snapshot, created: at };
    let catalog = Catalog {
        store: Uuid::from_u128(1),
        libraries: vec![record.clone()],
        snapshots: vec![CatalogSnapshot { id: theirs.id, library: remote, parents: Vec::new() }],
        labels: vec![label("keep", theirs.id), label("elsewhere", Hash::of(b"never fetched"))],
    };
    let dry_run = tc.draw(gs::booleans());
    let got = settle(&store, &catalog, &[record], vec![theirs.clone()], dry_run).unwrap();
    let renamed = name != "Away";
    let want_name = if renamed { format!("{name} 2") } else { name.to_string() };
    assert_eq!(got.len(), 1);
    assert_eq!(
        (got[0].name.as_str(), got[0].was.as_deref(), got[0].outcome),
        (want_name.as_str(), renamed.then_some(name), LibraryOutcome::Added)
    );
    let listed: Vec<Hash> = snapshots(&store).unwrap().iter().map(|s| s.id).collect();
    let keep = store.find_label("keep").unwrap().map(|l| l.snapshot);
    assert!(store.find_label("elsewhere").unwrap().is_none());
    if dry_run {
        assert_eq!(listed, vec![mine.id]);
        assert!(store.library_by_id(remote).unwrap().is_none());
        assert_eq!(keep, label_taken.then_some(mine.id));
        return;
    }
    let mut want = vec![mine.id, theirs.id];
    want.sort();
    assert_eq!(listed, want);
    let added = store.library_by_id(remote).unwrap().unwrap();
    assert_eq!((added.name.as_str(), added.head), (want_name.as_str(), Some(theirs.id)));
    assert_eq!(keep, Some(if label_taken { mine.id } else { theirs.id }));
}

/// Moving a library that the other store has no latest for leaves its record as it was.
#[test]
fn moving_to_no_latest_keeps_the_library() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    let home = store.create_library("Home").unwrap();
    let lib =
        FetchedLibrary { name: "Home".into(), was: None, id: home.id, head: None, outcome: LibraryOutcome::Updated };
    apply(&store, &lib).unwrap();
    assert_eq!(store.library_by_id(home.id).unwrap().unwrap().head, None);
}
