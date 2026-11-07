//! Properties of a plugin's store destination as the host uses it: copying the store there
//! (`offsite::copy_to`), bringing it back (`offsite::restore`) and repairing a drive from it
//! (`scrub::offer`, `scrub::fetch`), against a destination kept in memory that stops answering at
//! any request, or answers some stored files damaged.

use std::path::Path;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use hegel::TestCase;
use hegel::generators as gs;
use indexmap::IndexMap;
use locallibrary::catalog::LibraryOutcome;
use locallibrary::device::Device;
use locallibrary::hash::Hash;
use locallibrary::offsite::{self, CopyOptions, Destination, RestoreOptions};
use locallibrary::paths::FileClass;
use locallibrary::progress::Ctx;
use locallibrary::scrub::{self, FileProblem, RepairSource, ScrubReport};
use locallibrary::store::{LibraryRecord, Store};
use locallibrary::sync::{self, PullOptions};
use locallibrary::{Error, Result};
use rustc_hash::{FxHashMap, FxHashSet};

use crate::common;

/// A destination in memory: what it was given, by hash and by name.
#[derive(Default)]
struct Memory {
    blobs: Mutex<FxHashMap<Hash, Vec<u8>>>,
    records: Mutex<IndexMap<String, Vec<u8>>>,
    /// Requests answered so far.
    answered: AtomicUsize,
    /// It stops answering once it has answered this many.
    stops_after: Mutex<Option<usize>>,
    /// Stored files it answers with a byte changed.
    damages: FxHashSet<Hash>,
}

impl Memory {
    fn turn(&self) -> Result<()> {
        let n = self.answered.fetch_add(1, Ordering::SeqCst);
        match *self.stops_after.lock().unwrap() {
            Some(stop) if n >= stop => Err(Error::Cancelled),
            _ => Ok(()),
        }
    }

    fn record<T: serde::de::DeserializeOwned>(&self, name: &str) -> Option<T> {
        self.records.lock().unwrap().get(name).map(|b| serde_json::from_slice(b).unwrap())
    }

    /// Whether it has snapshot `id`'s record and every stored file it lists.
    fn whole(&self, id: &Hash) -> bool {
        let Some(snap) = self.record::<locallibrary::store::Snapshot>(&format!("snapshots/{}.json", id.to_hex()))
        else {
            return false;
        };
        let blobs = self.blobs.lock().unwrap();
        snap.files.iter().all(|f| f.hash.is_some_and(|h| blobs.contains_key(&h)))
    }

    /// Whether everything `libraries` (or an index of snapshots) reaches is whole.
    fn reaches_whole(&self, name: &str, bytes: &[u8]) -> bool {
        match name {
            "libraries.json" => {
                let libraries: Vec<LibraryRecord> = serde_json::from_slice(bytes).unwrap();
                libraries.iter().all(|l| l.head.is_none_or(|h| self.whole(&h)))
            }
            "snapshots.json" => {
                let index: Vec<serde_json::Value> = serde_json::from_slice(bytes).unwrap();
                index.iter().all(|s| self.whole(&s["id"].as_str().unwrap().parse().unwrap()))
            }
            _ => true,
        }
    }
}

impl Destination for Memory {
    fn plugin(&self) -> &str {
        "memory"
    }

    fn name(&self) -> &str {
        "here"
    }

    fn missing(&self, hashes: &[Hash], _: &Ctx<'_>) -> Result<Vec<Hash>> {
        self.turn()?;
        assert!(hashes.len() <= offsite::BATCH);
        let blobs = self.blobs.lock().unwrap();
        Ok(hashes.iter().filter(|h| !blobs.contains_key(h)).copied().collect())
    }

    fn put(&self, hash: &Hash, path: &Path, _: &Ctx<'_>) -> Result<()> {
        self.turn()?;
        self.blobs.lock().unwrap().insert(*hash, std::fs::read(path).unwrap());
        Ok(())
    }

    fn get(&self, hash: &Hash, into: &Path, _: &Ctx<'_>) -> Result<()> {
        self.turn()?;
        let mut bytes = self.blobs.lock().unwrap().get(hash).cloned().ok_or(Error::NotFound("a blob".into()))?;
        if self.damages.contains(hash) {
            bytes.push(0);
        }
        std::fs::write(into, bytes).unwrap();
        Ok(())
    }

    /// Checks, as it's given, that what libraries.json (or the index) reaches is here already.
    fn put_manifest(&self, name: &str, bytes: &[u8], _: &Ctx<'_>) -> Result<()> {
        self.turn()?;
        assert!(self.reaches_whole(name, bytes), "{name} was given before what it reaches");
        self.records.lock().unwrap().insert(name.into(), bytes.to_vec());
        Ok(())
    }

    fn get_manifest(&self, name: &str, _: &Ctx<'_>) -> Result<Option<Vec<u8>>> {
        self.turn()?;
        Ok(self.records.lock().unwrap().get(name).cloned())
    }
}

/// A store with a library of `pulls` snapshots (a track added before each after the first),
/// the latest labelled.
fn pulled(dir: &Path, pulls: usize) -> Store {
    let store = Store::open_or_init(dir.join("store")).unwrap();
    let root = common::demo_drive(dir, "Main");
    let mut d = Device::open(&root).unwrap();
    for i in 0..pulls {
        if i > 0 {
            common::export_new_track(&root, "Artist", &format!("Track {i}"));
            d = Device::open(&root).unwrap();
        }
        let opts = PullOptions { library: None, message: Some(format!("pull {i}")), force: false };
        sync::pull(&store, &mut d, &opts, &Ctx::silent()).unwrap();
    }
    let head = store.libraries().unwrap()[0].head.unwrap();
    store.add_label(&head, "latest").unwrap();
    store
}

/// What a store holds: its libraries' latest snapshots, its stored files and its labels.
type Holding = (Vec<(String, Option<Hash>)>, Vec<String>, Vec<(String, Hash)>);

/// What `store` holds.
fn holding(store: &Store) -> Holding {
    let libraries = store.libraries().unwrap().into_iter().map(|l| (l.name, l.head)).collect();
    let all =
        walkdir::WalkDir::new(store.root().join("objects")).into_iter().flatten().filter(|e| e.file_type().is_file());
    let mut objects: Vec<String> =
        all.map(|e| e.path().strip_prefix(store.root()).unwrap().to_string_lossy().replace('\\', "/")).collect();
    objects.retain(|o| !o.starts_with("objects/tmp/"));
    objects.sort();
    let labels = store.labels().unwrap().into_iter().map(|l| (l.name, l.snapshot)).collect();
    (libraries, objects, labels)
}

/// Every stored file in `store` hashes to its name.
fn sound(store: &Store) -> bool {
    let all =
        walkdir::WalkDir::new(store.root().join("objects")).into_iter().flatten().filter(|e| e.file_type().is_file());
    let tmp = store.root().join("objects").join("tmp");
    all.filter(|e| !e.path().starts_with(&tmp)).all(|e| {
        let shard = e.path().parent().and_then(Path::file_name).unwrap_or_default().to_string_lossy();
        let name = format!("{shard}{}", e.file_name().to_string_lossy());
        Hash::of(&std::fs::read(e.path()).unwrap()).to_hex() == name
    })
}

/// A copy, stopped at any request or not, gives `libraries.json` (and the index of snapshots)
/// only once everything it reaches is whole there: each snapshot's record and every stored file
/// it lists (see `Memory::put_manifest`). It holds only the store's records the host gives (never the account session), and the next
/// copy finishes it. Brought back into an empty store, it's the store as it was: its libraries'
/// latest snapshots, every stored file, its labels and its other records.
#[hegel::test(test_cases = 15, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn a_copy_stopped_anywhere_reaches_only_whole_snapshots_and_comes_back_whole(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let store = pulled(dir.path(), tc.draw(gs::integers::<usize>().min_value(1).max_value(2)));
    std::fs::write(store.root().join("atmosphere-session.json"), b"{\"session\": \"never leaves\"}").unwrap();
    let memory = Memory::default();
    *memory.stops_after.lock().unwrap() = tc.draw(gs::optional(gs::integers::<usize>().max_value(60)));
    let stopped = offsite::copy_to(&store, &memory, &CopyOptions::default(), &Ctx::silent());
    let given = ["store.json", "drives.json", "labels.json", "snapshots.json", "libraries.json"];
    for name in memory.records.lock().unwrap().keys() {
        assert!(given.contains(&name.as_str()) || name.starts_with("snapshots/"), "{name} was given");
    }
    // Finished.
    *memory.stops_after.lock().unwrap() = None;
    let r = offsite::copy_to(&store, &memory, &CopyOptions::default(), &Ctx::silent()).unwrap();
    assert!(r.complete && r.problems.is_empty(), "{r:?}");
    if stopped.is_ok() {
        assert_eq!((r.objects_copied, r.snapshots_copied), (0, 0), "a finished copy has nothing left to copy");
    }
    let other = Store::open_or_init(dir.path().join("other")).unwrap();
    offsite::restore(&other, &memory, &RestoreOptions::default(), &Ctx::silent()).unwrap();
    assert_eq!(holding(&other), holding(&store));
    let drives = |s: &Store| std::fs::read(s.root().join("drives.json")).ok();
    assert_eq!(drives(&other), drives(&store));
    assert!(!other.root().join("atmosphere-session.json").exists());
}

/// Whatever stored files a destination answers damaged, none goes into the store: bringing it
/// back fails, and the store has only files that hash to their names, and no library whose
/// snapshot lists a file it lacks. Answered whole, it all comes back.
#[hegel::test(test_cases = 15, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn what_comes_back_damaged_never_goes_into_the_store(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let store = pulled(dir.path(), 1);
    let mut memory = Memory::default();
    offsite::copy_to(&store, &memory, &CopyOptions::default(), &Ctx::silent()).unwrap();
    let hashes: Vec<Hash> = memory.blobs.lock().unwrap().keys().copied().collect();
    memory.damages = tc.draw(gs::subsequences(hashes)).into_iter().collect();
    let other = Store::open_or_init(dir.path().join("other")).unwrap();
    let restored = offsite::restore(&other, &memory, &RestoreOptions::default(), &Ctx::silent());
    assert!(sound(&other));
    match memory.damages.is_empty() {
        true => {
            restored.unwrap();
            assert_eq!(holding(&other), holding(&store));
        }
        false => {
            let e = restored.unwrap_err().to_string();
            assert!(e.starts_with("the memory:here destination failed: a stored file came back damaged ("), "{e}");
            assert!(other.libraries().unwrap().is_empty(), "no library reaches what's missing");
            assert!(memory.damages.iter().all(|h| !other.has_object(h)));
        }
    }
}

/// A file a scrub found damaged, unreadable or missing with no good copy is offered from the
/// destination exactly when it has that copy; one with a copy elsewhere, or none expected, keeps
/// what it had. Fetching brings those it offers into the store, each checked: one answered
/// damaged fails the fetch and stays out.
#[hegel::test(test_cases = 50)]
fn a_destination_offers_what_it_has_and_only_that_comes_in(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    let mut memory = Memory::default();
    let n = tc.draw(gs::integers::<usize>().max_value(6));
    let mut problems = Vec::new();
    for i in 0..n {
        let bytes = format!("file {i}").into_bytes();
        let hash = Hash::of(&bytes);
        if tc.draw(gs::booleans()) {
            memory.blobs.lock().unwrap().insert(hash, bytes);
        }
        let repair = tc.draw(gs::weighted_booleans(0.2)).then_some(RepairSource::Store);
        let expected = tc.draw(gs::weighted_booleans(0.9)).then_some(hash);
        problems.push(FileProblem {
            path: format!("contents/{i}.mp3"),
            class: FileClass::Audio,
            size: 6,
            expected,
            actual: None,
            reference: None,
            mtime: None,
            error: None,
            repair,
        });
    }
    let kinds: Vec<usize> = (0..n).map(|_| tc.draw(gs::integers::<usize>().max_value(2))).collect();
    let of = |k: usize| problems.iter().zip(&kinds).filter(|(_, x)| **x == k).map(|(p, _)| p.clone()).collect();
    let mut report = ScrubReport {
        drive: "Main".into(),
        root: dir.path().into(),
        snapshot: None,
        files: n,
        bytes: 0,
        verified: 0,
        unverified: 0,
        damaged: of(0),
        unreadable: of(1),
        missing: of(2),
        tracks: Vec::new(),
        playlists: Vec::new(),
    };
    let before: Vec<FileProblem> = report.problems().cloned().collect();
    scrub::offer(&mut report, &memory, &Ctx::silent()).unwrap();
    let from = RepairSource::Destination { plugin: "memory".into(), destination: "here".into() };
    let has = |h: &Hash| memory.blobs.lock().unwrap().contains_key(h);
    for (was, now) in before.iter().zip(report.problems()) {
        let offered = was.repair.is_none() && was.expected.is_some_and(|h| has(&h));
        let want = if offered { Some(from.clone()) } else { was.repair.clone() };
        assert_eq!(now.repair, want, "{}", was.path);
    }
    let offered: Vec<FileProblem> = report.problems().cloned().collect();
    let wanted: FxHashSet<Hash> =
        offered.iter().filter(|p| p.repair.as_ref() == Some(&from)).filter_map(|p| p.expected).collect();
    memory.damages = tc.draw(gs::subsequences(wanted.iter().copied().collect::<Vec<_>>())).into_iter().collect();
    let fetched = scrub::fetch(&store, &memory, &offered, &Ctx::silent());
    assert_eq!(fetched.is_ok(), memory.damages.is_empty(), "{fetched:?}");
    assert!(sound(&store));
    for h in &wanted {
        if memory.damages.contains(h) {
            assert!(!store.has_object(h));
        } else if fetched.is_ok() {
            assert!(store.has_object(h));
        }
    }
}

/// Whatever stored files the store has lost or has damaged, a copy leaves out each snapshot
/// listing one, saying why, and gives every other: it's complete only when none is left out, and
/// a library whose latest snapshot is left out isn't in the copy. Brought back into an empty store,
/// it's those libraries and the stored files their snapshots list, unless the destination's
/// records come back rewritten: a snapshot's that isn't the snapshot it's named after, or a
/// `libraries.json` that isn't one, is refused, and nothing goes in.
#[hegel::test(test_cases = 10, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn a_copy_leaves_out_what_the_store_lost_and_what_comes_back_is_checked(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let store = pulled(dir.path(), tc.draw(gs::integers::<usize>().min_value(1).max_value(3)));
    let snapshots: Vec<(Hash, FxHashSet<Hash>)> = store
        .snapshot_ids()
        .unwrap()
        .into_iter()
        .map(|id| (id, store.load_snapshot(&id).unwrap().files.iter().filter_map(|f| f.hash).collect()))
        .collect();
    let mut all: Vec<Hash> = snapshots.iter().flat_map(|(_, files)| files.iter().copied()).collect();
    all.sort_by_key(Hash::to_hex);
    all.dedup();
    let broken: FxHashSet<Hash> = tc.draw(gs::subsequences(all).max_size(2)).into_iter().collect();
    for h in &broken {
        match tc.draw(gs::booleans()) {
            true => std::fs::remove_file(store.object_path(h)).unwrap(),
            false => common::rot(&store.object_path(h)),
        }
    }
    let memory = Memory::default();
    let r = offsite::copy_to(&store, &memory, &CopyOptions::default(), &Ctx::silent()).unwrap();
    let left_out: FxHashSet<Hash> =
        snapshots.iter().filter(|(_, files)| !files.is_disjoint(&broken)).map(|(id, _)| *id).collect();
    assert_eq!((r.complete, r.problems.len()), (left_out.is_empty(), left_out.len()), "{r:?}");
    for (id, _) in &snapshots {
        let given = memory.records.lock().unwrap().contains_key(&format!("snapshots/{}.json", id.to_hex()));
        assert_eq!(given, !left_out.contains(id), "snapshot {}", id.short());
        assert_eq!(r.problems.iter().any(|p| p.starts_with(&format!("snapshot {} left out: ", id.short()))), !given);
    }
    let reached: Vec<(String, Option<Hash>)> = store
        .libraries()
        .unwrap()
        .into_iter()
        .filter(|l| l.head.is_some_and(|h| !left_out.contains(&h)))
        .map(|l| (l.name, l.head))
        .collect();
    let copied: Vec<LibraryRecord> = memory.record("libraries.json").unwrap();
    assert_eq!(copied.into_iter().map(|l| (l.name, l.head)).collect::<Vec<_>>(), reached);

    let other = Store::open_or_init(dir.path().join("other")).unwrap();
    let rewritten = tc.draw(gs::sampled_from(vec![None, Some("libraries.json"), Some("snapshot")]));
    let head = reached.first().and_then(|(_, h)| *h);
    match (rewritten, head) {
        (Some("libraries.json"), _) => {
            memory.records.lock().unwrap().insert("libraries.json".into(), b"{\"not\": \"libraries\"}".to_vec());
            let e = offsite::restore(&other, &memory, &RestoreOptions::default(), &Ctx::silent()).unwrap_err();
            let said = "the memory:here destination failed: its libraries.json can't be read: ";
            assert!(e.to_string().starts_with(said), "{e}");
        }
        (Some(_), Some(head)) => {
            let name = format!("snapshots/{}.json", head.to_hex());
            let mut record: serde_json::Value = memory.record(&name).unwrap();
            record["message"] = "rewritten".into();
            memory.records.lock().unwrap().insert(name, record.to_string().into_bytes());
            let e = offsite::restore(&other, &memory, &RestoreOptions::default(), &Ctx::silent()).unwrap_err();
            let said = format!("the memory:here destination failed: snapshot {} came back damaged", head.short());
            assert_eq!(e.to_string(), said);
        }
        _ => {
            offsite::restore(&other, &memory, &RestoreOptions::default(), &Ctx::silent()).unwrap();
            let back: Vec<(String, Option<Hash>)> =
                other.libraries().unwrap().into_iter().map(|l| (l.name, l.head)).collect();
            assert_eq!(back, reached);
            assert!(sound(&other));
            for (id, files) in &snapshots {
                if reached.iter().any(|(_, h)| *h == Some(*id)) {
                    assert!(files.iter().all(|h| other.has_object(h)), "snapshot {} came back short", id.short());
                }
            }
            return;
        }
    }
    assert!(other.libraries().unwrap().is_empty(), "a library came back from rewritten records");
}

/// The snapshot `id` and those it descends from, in whichever of `stores` has each.
fn ancestry(stores: &[&Store], id: Hash) -> FxHashSet<Hash> {
    let mut seen = FxHashSet::default();
    let mut todo = vec![id];
    while let Some(h) = todo.pop() {
        if seen.insert(h) {
            let snap = stores.iter().find_map(|s| s.load_snapshot(&h).ok()).unwrap();
            todo.extend(snap.parents);
        }
    }
    seen
}

/// For any rounds of pulls into a store, pulls of the same drive into a second store that has the
/// library, and copies of the first to a destination, each round ending with the second store
/// brought up to date from there: a restore before any copy is told there's none; the library is
/// added the first time, with the store's labels, then moves to the copy's latest snapshot exactly
/// when that descends from its own, stays as it is when its own is as new or newer, and keeps its
/// own when both have changed, with the copy's beside it. A dry run says the same and changes
/// nothing.
#[hegel::test(test_cases = 10, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn a_store_restored_again_moves_on_only_to_what_descends_from_its_own(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let store = pulled(dir.path(), 1);
    let root = dir.path().join("Main");
    let other = Store::open_or_init(dir.path().join("other")).unwrap();
    let memory = Memory::default();
    let head = |s: &Store| s.libraries().unwrap().first().and_then(|l| l.head);
    if tc.draw(gs::booleans()) {
        let e = offsite::restore(&other, &memory, &RestoreOptions::default(), &Ctx::silent()).unwrap_err();
        assert_eq!(e.to_string(), "not found: a copy of a store at memory:here");
    }
    let pull = |into: &Store, i: usize| {
        common::export_new_track(&root, "Artist", &format!("Track {i}"));
        let mut d = Device::open(&root).unwrap();
        let opts = PullOptions { library: None, message: Some(format!("pull {i}")), force: false };
        sync::pull(into, &mut d, &opts, &Ctx::silent()).unwrap();
        assert_eq!(into.libraries().unwrap().len(), 1, "pulled into a library of its own");
    };
    let mut copied = None;
    let rounds = tc.draw(gs::integers::<usize>().min_value(1).max_value(3));
    for round in 0..=rounds {
        // The first round copies the store, and adds the library to the second.
        let (here, there, copy) = match round {
            0 => (false, false, true),
            _ => (tc.draw(gs::booleans()), tc.draw(gs::booleans()), tc.draw(gs::booleans())),
        };
        let order = tc.draw(gs::booleans());
        for (into, pulls) in if order { [(&store, here), (&other, there)] } else { [(&other, there), (&store, here)] } {
            if pulls {
                pull(into, round * 2 + usize::from(std::ptr::eq(into, &other)));
            }
        }
        if copy {
            offsite::copy_to(&store, &memory, &CopyOptions::default(), &Ctx::silent()).unwrap();
            copied = head(&store);
        }
        let theirs = copied.unwrap();
        let ours = head(&other);
        let want = match ours {
            None => LibraryOutcome::Added,
            Some(o) if ancestry(&[&store, &other], o).contains(&theirs) => LibraryOutcome::UpToDate,
            Some(o) if ancestry(&[&store, &other], theirs).contains(&o) => LibraryOutcome::Updated,
            Some(_) => LibraryOutcome::Diverged,
        };
        let before = holding(&other);
        let dry = RestoreOptions { dry_run: true, ..Default::default() };
        let planned = offsite::restore(&other, &memory, &dry, &Ctx::silent()).unwrap();
        assert_eq!(planned.libraries[0].outcome, want, "round {round}: a dry run says otherwise");
        assert_eq!(holding(&other), before, "round {round}: a dry run changed the store");
        let r = offsite::restore(&other, &memory, &RestoreOptions::default(), &Ctx::silent()).unwrap();
        assert_eq!((r.libraries.len(), r.libraries[0].outcome), (1, want), "round {round}: {r:?}");
        let now = match want {
            LibraryOutcome::Added | LibraryOutcome::Updated => Some(theirs),
            _ => ours,
        };
        assert_eq!(head(&other), now, "round {round}");
        assert!(other.load_snapshot(&theirs).is_ok(), "round {round}: the copy's latest isn't here");
        assert!(other.find_label("latest").unwrap().is_some(), "round {round}: the labels didn't come back");
        assert!(sound(&other));
    }
}

/// What `memory` holds: its records' names and its stored files.
fn kept(memory: &Memory) -> (Vec<String>, Vec<String>) {
    let mut records: Vec<String> = memory.records.lock().unwrap().keys().cloned().collect();
    let mut blobs: Vec<String> = memory.blobs.lock().unwrap().keys().map(Hash::to_hex).collect();
    records.sort();
    blobs.sort();
    (records, blobs)
}

/// A copy to a destination that stops answering at any one of the requests a whole copy makes
/// fails, and the next copy leaves it holding what a copy never stopped does; restoring from it
/// into an empty store, stopped at any one of a restore's requests, fails too, and the next
/// restore brings back the store as it was.
#[hegel::test(test_cases = 2, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn prop_a_copy_or_restore_stopped_at_any_request_fails_and_the_next_finishes(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let store = pulled(dir.path(), tc.draw(gs::integers::<usize>().min_value(1).max_value(2)));
    let silent = Ctx::silent();
    let whole = Memory::default();
    offsite::copy_to(&store, &whole, &CopyOptions::default(), &silent).unwrap();
    let requests = whole.answered.load(Ordering::SeqCst);
    for stop in 0..requests {
        let memory = Memory::default();
        *memory.stops_after.lock().unwrap() = Some(stop);
        let err = offsite::copy_to(&store, &memory, &CopyOptions::default(), &silent).unwrap_err();
        assert!(matches!(err, Error::Cancelled), "stopped at request {stop}: {err}");
        *memory.stops_after.lock().unwrap() = None;
        let r = offsite::copy_to(&store, &memory, &CopyOptions::default(), &silent).unwrap();
        assert!(r.complete, "{r:?}");
        assert_eq!(kept(&memory), kept(&whole), "stopped at request {stop}");
    }

    let asked = whole.answered.load(Ordering::SeqCst);
    let first = Store::open_or_init(dir.path().join("restored")).unwrap();
    offsite::restore(&first, &whole, &RestoreOptions::default(), &silent).unwrap();
    let requests = whole.answered.load(Ordering::SeqCst) - asked;
    for stop in 0..requests {
        let other = Store::open_or_init(dir.path().join(format!("restored {stop}"))).unwrap();
        *whole.stops_after.lock().unwrap() = Some(whole.answered.load(Ordering::SeqCst) + stop);
        let err = offsite::restore(&other, &whole, &RestoreOptions::default(), &silent).unwrap_err();
        assert!(matches!(err, Error::Cancelled), "stopped at request {stop}: {err}");
        *whole.stops_after.lock().unwrap() = None;
        offsite::restore(&other, &whole, &RestoreOptions::default(), &silent).unwrap();
        assert_eq!(holding(&other), holding(&store), "stopped at request {stop}");
        assert!(sound(&other));
    }
}

/// A destination whose records read as garbage is refused saying which: copying there, its
/// store's id, libraries or index of snapshots; restoring from it, those or its labels. One
/// naming a store by something that isn't an id is another store's copy to a copy (refused
/// unless forced) and a damaged one to a restore.
#[hegel::test(test_cases = 10)]
fn prop_a_destination_whose_records_read_as_garbage_is_refused_saying_which(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let store = pulled(dir.path(), 1);
    let silent = Ctx::silent();
    // Not JSON at all: the store's id is read as any JSON first.
    let garbage = tc.draw(gs::sampled_from(vec!["", "{", "[1", "nope"])).as_bytes().to_vec();
    let said = |name: &str| format!("the memory:here destination failed: its {name} can't be read: ");
    let name = tc.draw(gs::sampled_from(vec!["store.json", "libraries.json", "snapshots.json"]));
    let memory = Memory::default();
    memory.records.lock().unwrap().insert(name.into(), garbage.clone());
    let e = offsite::copy_to(&store, &memory, &CopyOptions::default(), &silent).unwrap_err().to_string();
    assert!(e.starts_with(&said(name)), "{e}");

    let whole = Memory::default();
    offsite::copy_to(&store, &whole, &CopyOptions::default(), &silent).unwrap();
    let other = Store::open_or_init(dir.path().join("other")).unwrap();
    let name = tc.draw(gs::sampled_from(vec!["store.json", "snapshots.json", "labels.json"]));
    let was = whole.records.lock().unwrap().insert(name.into(), garbage).unwrap();
    let e = offsite::restore(&other, &whole, &RestoreOptions::default(), &silent).unwrap_err().to_string();
    assert!(e.starts_with(&said(name)), "{e}");
    whole.records.lock().unwrap().insert(name.into(), was);

    let id = tc.draw(gs::sampled_from(vec!["\"not an id\"", "1", "null"]));
    let named = format!("{{\"id\": {id}}}").into_bytes();
    let was = whole.records.lock().unwrap().insert("store.json".into(), named).unwrap();
    let e = offsite::restore(&other, &whole, &RestoreOptions::default(), &silent).unwrap_err().to_string();
    assert!(e.starts_with("the memory:here destination failed: its store.json: "), "{e}");
    let e = offsite::copy_to(&store, &whole, &CopyOptions::default(), &silent).unwrap_err();
    assert_eq!(e.reason(), Some(locallibrary::error::Reason::DifferentStore), "{e}");
    let forced = CopyOptions { force: true, ..Default::default() };
    offsite::copy_to(&store, &whole, &forced, &silent).unwrap();
    assert_eq!(whole.records.lock().unwrap().get("store.json"), Some(&was), "the store's own id replaced it");
    assert!(other.libraries().unwrap().is_empty(), "nothing came back from garbage");
}

/// A destination whose libraries' latest snapshots include one the store has neither kept nor
/// pruned (pulled into elsewhere) is copied over only when forced, which leaves it the store's.
#[hegel::test(test_cases = 10)]
fn prop_a_destination_ahead_of_the_store_is_copied_over_only_when_forced(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let store = pulled(dir.path(), 1);
    let silent = Ctx::silent();
    let memory = Memory::default();
    offsite::copy_to(&store, &memory, &CopyOptions::default(), &silent).unwrap();
    let mut libraries: Vec<LibraryRecord> = memory.record("libraries.json").unwrap();
    let ahead = Hash::of(tc.draw(gs::text().max_size(8)).as_bytes());
    libraries[0].head = Some(ahead);
    memory.records.lock().unwrap().insert("libraries.json".into(), serde_json::to_vec(&libraries).unwrap());
    let e = offsite::copy_to(&store, &memory, &CopyOptions::default(), &silent).unwrap_err();
    assert_eq!(e.reason(), Some(locallibrary::error::Reason::CopyAhead), "{e}");
    let name = &libraries[0].name;
    assert!(e.to_string().contains(&format!("{name} ({})", ahead.short())), "{e}");
    let forced = CopyOptions { force: true, ..Default::default() };
    offsite::copy_to(&store, &memory, &forced, &silent).unwrap();
    let now: Vec<LibraryRecord> = memory.record("libraries.json").unwrap();
    assert_eq!(now[0].head, store.libraries().unwrap()[0].head);
}

/// A copy to a destination fails, saying which, when one of the store's records can't be read
/// (its id, libraries, pruned snapshots, labels or settings) or a snapshot lists a file without its
/// hash, and while another holds the store; put right, the copy goes through.
#[hegel::test(test_cases = 3, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn prop_a_copy_to_a_destination_names_the_store_record_it_cant_read(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let store = pulled(dir.path(), 2);
    let silent = Ctx::silent();
    let memory = Memory::default();
    let held = store.lock().unwrap();
    let e = offsite::copy_to(&store, &memory, &CopyOptions::default(), &silent).unwrap_err();
    assert!(matches!(e, Error::Locked(_)), "{e}");
    let e = offsite::restore(&store, &memory, &RestoreOptions::default(), &silent).unwrap_err();
    assert!(matches!(e, Error::Locked(_)), "{e}");
    drop(held);
    for name in ["store.json", "libraries.json", "pruned.json", "labels.json", "settings.json"] {
        let path = store.root().join(name);
        let was = std::fs::read(&path).ok();
        let _ = std::fs::remove_file(&path);
        match name {
            "settings.json" => std::fs::create_dir(&path).unwrap(),
            _ => std::fs::write(&path, tc.draw(gs::sampled_from(vec!["", "{", "[1]"]))).unwrap(),
        }
        let e = offsite::copy_to(&store, &memory, &CopyOptions::default(), &silent).unwrap_err();
        assert_eq!(e.detail().fields.get("path"), Some(&path.display().to_string().into()), "{e}");
        let _ = std::fs::remove_dir(&path);
        if let Some(bytes) = was {
            std::fs::write(&path, bytes).unwrap();
        } else {
            let _ = std::fs::remove_file(&path);
        }
    }
    let sid = tc.draw(gs::sampled_from(store.snapshot_ids().unwrap()));
    let manifest = store.root().join("snapshots").join(format!("{}.json", sid.to_hex()));
    let was = std::fs::read(&manifest).unwrap();
    let mut snap: serde_json::Value = serde_json::from_slice(&was).unwrap();
    snap["files"][0].as_object_mut().unwrap().remove("hash");
    std::fs::write(&manifest, snap.to_string()).unwrap();
    let e = offsite::copy_to(&store, &memory, &CopyOptions::default(), &silent).unwrap_err().to_string();
    assert_eq!(e, format!("snapshot {}: {} has no hash", sid.short(), snap["files"][0]["path"].as_str().unwrap()));
    std::fs::write(&manifest, was).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let h = store.load_snapshot(&sid).unwrap().files.iter().find_map(|f| f.hash).unwrap();
        let object = store.object_path(&h);
        let memory = Memory::default();
        std::fs::set_permissions(&object, std::fs::Permissions::from_mode(0o000)).unwrap();
        let e = offsite::copy_to(&store, &memory, &CopyOptions::default(), &silent).unwrap_err();
        std::fs::set_permissions(&object, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert_eq!(e.detail().fields.get("path"), Some(&object.display().to_string().into()), "{e}");
    }
    let r = offsite::copy_to(&store, &memory, &CopyOptions::default(), &silent).unwrap();
    assert!(r.complete, "{r:?}");
}

/// Restoring fails, saying where, when the store restored into can't take what comes back: its
/// pruned snapshots can't be read, its folder for stored files on the way in or one they go in
/// is a file, or (where permissions say so) its own folder can't be written; and when the
/// destination lacks a stored file a snapshot lists. Put right, it all comes back.
#[hegel::test(test_cases = 3, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn prop_a_restore_names_what_it_cant_bring_back_and_brings_it_once_its_put_right(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let store = pulled(dir.path(), 1);
    let silent = Ctx::silent();
    let memory = Memory::default();
    offsite::copy_to(&store, &memory, &CopyOptions::default(), &silent).unwrap();
    let blobs: Vec<Hash> = memory.blobs.lock().unwrap().keys().copied().collect();
    let h = tc.draw(gs::sampled_from(blobs));
    let restore = |other: &Store| offsite::restore(other, &memory, &RestoreOptions::default(), &silent);
    let hex = h.to_hex();
    let shard = format!("objects/{}", &hex[..2]);
    // Records it can't read, files where its folders go, a stored file the destination lacks, and
    // folders it can't write: its own, and the one stored files come in through.
    let faults = ["pruned.json", "libraries.json", "objects/tmp", &shard, "a blob", "read-only", "read-only tmp"];
    for (n, fault) in faults.into_iter().enumerate() {
        if fault.starts_with("read-only") && !cfg!(unix) {
            continue;
        }
        let other = Store::open_or_init(dir.path().join(format!("other {n}"))).unwrap();
        let tmp = other.root().join("objects").join("tmp");
        let path = other.root().join(fault);
        let mut lost = None;
        match fault {
            "pruned.json" | "libraries.json" => std::fs::write(&path, b"{").unwrap(),
            "a blob" => lost = memory.blobs.lock().unwrap().remove(&h),
            "read-only" => read_only(other.root(), true),
            "read-only tmp" => {
                std::fs::create_dir_all(&tmp).unwrap();
                read_only(&tmp, true);
            }
            _ => {
                std::fs::create_dir_all(path.parent().unwrap()).unwrap();
                std::fs::write(&path, b"a file").unwrap();
            }
        }
        if fault == "libraries.json" {
            let dry = RestoreOptions { dry_run: true, ..Default::default() };
            let e = offsite::restore(&other, &memory, &dry, &silent).unwrap_err();
            assert_eq!(e.detail().fields.get("path"), Some(&path.display().to_string().into()), "{e}");
        }
        let e = restore(&other).unwrap_err();
        let at = e.detail().fields.get("path").and_then(|p| p.as_str().map(std::path::PathBuf::from));
        match fault {
            "a blob" => assert_eq!(e.to_string(), "not found: a blob"),
            "read-only" => assert!(at.is_some_and(|p| p.starts_with(other.root())), "{e}"),
            "read-only tmp" => assert!(at.is_some_and(|p| p.starts_with(&tmp)), "{e}"),
            _ => assert_eq!(at.as_deref(), Some(path.as_path()), "{e}"),
        }
        match fault {
            "a blob" => {
                memory.blobs.lock().unwrap().insert(h, lost.unwrap());
            }
            "read-only" => read_only(other.root(), false),
            "read-only tmp" => read_only(&tmp, false),
            _ => std::fs::remove_file(&path).unwrap(),
        }
        restore(&other).unwrap();
        assert_eq!(holding(&other), holding(&store), "{fault}");
    }
}

/// A snapshot a destination lists but no longer has the record of (removed there since) is left
/// out of a restore, and the rest comes back; one whose record comes back rewritten is refused.
#[hegel::test(test_cases = 3, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn prop_a_restore_leaves_out_a_snapshot_the_destination_no_longer_has(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let store = pulled(dir.path(), tc.draw(gs::integers::<usize>().min_value(2).max_value(3)));
    let memory = Memory::default();
    offsite::copy_to(&store, &memory, &CopyOptions::default(), &Ctx::silent()).unwrap();
    let head = store.libraries().unwrap()[0].head.unwrap();
    let older: Vec<Hash> = store.snapshot_ids().unwrap().into_iter().filter(|id| *id != head).collect();
    let gone = tc.draw(gs::sampled_from(older.clone()));
    memory.records.lock().unwrap().shift_remove(&format!("snapshots/{}.json", gone.to_hex()));
    let other = Store::open_or_init(dir.path().join("other")).unwrap();
    let r = offsite::restore(&other, &memory, &RestoreOptions::default(), &Ctx::silent()).unwrap();
    assert_eq!(r.snapshots, older.len(), "{r:?}");
    assert!(other.load_snapshot(&gone).is_err());
    assert_eq!(other.libraries().unwrap()[0].head, Some(head));

    // One whose record comes back rewritten is refused.
    let name = format!("snapshots/{}.json", head.to_hex());
    let mut record: serde_json::Value = memory.record(&name).unwrap();
    record["message"] = format!("rewritten {}", tc.draw(gs::text().max_size(8))).into();
    memory.records.lock().unwrap().insert(name, record.to_string().into_bytes());
    let fresh = Store::open_or_init(dir.path().join("fresh")).unwrap();
    let e = offsite::restore(&fresh, &memory, &RestoreOptions::default(), &Ctx::silent()).unwrap_err();
    assert_eq!(
        e.to_string(),
        format!("the memory:here destination failed: snapshot {} came back damaged", head.short())
    );
}

/// Makes `dir` read-only, or writable again.
fn read_only(dir: &Path, read_only: bool) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(if read_only { 0o555 } else { 0o755 })).unwrap();
    }
    #[cfg(not(unix))]
    let _ = (dir, read_only);
}

/// A copy to a destination or a restore from it stopped before it starts, or as it starts on the
/// stored files, or (copying) once they're all there, stops there; the next one finishes.
#[hegel::test(test_cases = 10)]
fn prop_a_copy_or_restore_stopped_between_its_steps_finishes_next_time(tc: TestCase) {
    use locallibrary::progress::{CancelToken, Event, Phase};
    let dir = tempfile::tempdir().unwrap();
    let store = pulled(dir.path(), 1);
    let at = tc.draw(gs::sampled_from(vec![None, Some(true), Some(false)]));
    // Cancelled before it starts (`None`), as the stored files start (`Some(true)`) or once
    // they're done (`Some(false)`).
    let stopper = || {
        let token = CancelToken::new();
        let stop = token.clone();
        let progress = move |e: &Event| match (e, at) {
            (Event::PhaseStart { phase: Phase::Copy, .. }, Some(true))
            | (Event::PhaseEnd { phase: Phase::Copy }, Some(false)) => stop.cancel(),
            _ => {}
        };
        if at.is_none() {
            token.cancel();
        }
        (progress, token)
    };
    let (progress, token) = stopper();
    let memory = Memory::default();
    let e = offsite::copy_to(&store, &memory, &CopyOptions::default(), &Ctx::new(&progress, token)).unwrap_err();
    assert!(matches!(e, Error::Cancelled), "{e}");
    assert!(memory.record::<serde_json::Value>("libraries.json").is_none(), "no copy reaches a library yet");
    offsite::copy_to(&store, &memory, &CopyOptions::default(), &Ctx::silent()).unwrap();

    // A restore checks nothing more once its stored files are in.
    let other = Store::open_or_init(dir.path().join("other")).unwrap();
    if at != Some(false) {
        let (progress, token) = stopper();
        let e = offsite::restore(&other, &memory, &RestoreOptions::default(), &Ctx::new(&progress, token)).unwrap_err();
        assert!(matches!(e, Error::Cancelled), "{e}");
        assert!(other.libraries().unwrap().is_empty());
    }
    offsite::restore(&other, &memory, &RestoreOptions::default(), &Ctx::silent()).unwrap();
    assert_eq!(holding(&other), holding(&store));
}

/// Offering repairs from a destination that doesn't answer fails, offering none. Fetching the
/// files offered takes each once, however many problems share it, and not while another holds
/// the store's lock.
#[hegel::test(test_cases = 20)]
fn prop_a_fetch_from_a_destination_takes_each_file_once_and_only_with_the_store_to_itself(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    let memory = Memory::default();
    let bytes = tc.draw(gs::binary().min_size(1).max_size(64));
    let hash = Hash::of(&bytes);
    memory.blobs.lock().unwrap().insert(hash, bytes.clone());
    let problem = |i: usize| FileProblem {
        path: format!("contents/{i}.mp3"),
        class: FileClass::Audio,
        size: bytes.len() as u64,
        expected: Some(hash),
        actual: None,
        reference: None,
        mtime: None,
        error: None,
        repair: None,
    };
    let sharing = tc.draw(gs::integers::<usize>().min_value(1).max_value(4));
    let mut report = ScrubReport {
        drive: "Main".into(),
        root: dir.path().into(),
        snapshot: None,
        files: sharing,
        bytes: 0,
        verified: 0,
        unverified: 0,
        damaged: (0..sharing).map(problem).collect(),
        unreadable: Vec::new(),
        missing: Vec::new(),
        tracks: Vec::new(),
        playlists: Vec::new(),
    };
    *memory.stops_after.lock().unwrap() = Some(0);
    assert!(matches!(scrub::offer(&mut report, &memory, &Ctx::silent()), Err(Error::Cancelled)));
    assert!(report.problems().all(|p| p.repair.is_none()), "nothing offered");
    *memory.stops_after.lock().unwrap() = None;
    scrub::offer(&mut report, &memory, &Ctx::silent()).unwrap();
    let offered: Vec<FileProblem> = report.problems().cloned().collect();
    let held = store.lock().unwrap();
    let e = scrub::fetch(&store, &memory, &offered, &Ctx::silent()).unwrap_err();
    assert!(matches!(e, Error::Locked(_)), "{e}");
    assert!(!store.has_object(&hash));
    drop(held);
    let asked = memory.answered.load(Ordering::SeqCst);
    scrub::fetch(&store, &memory, &offered, &Ctx::silent()).unwrap();
    assert_eq!(memory.answered.load(Ordering::SeqCst) - asked, 1, "fetched once for {sharing} problems");
    assert_eq!(std::fs::read(store.object_path(&hash)).unwrap(), bytes);
}
