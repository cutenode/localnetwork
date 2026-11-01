//! Model-based tests of the snapshot store: objects, snapshots, library heads, gc and verify,
//! with objects and manifests damaged behind the store's back; and snapshot ids and hashes.

use indexmap::IndexMap;
use std::collections::{BTreeSet, VecDeque};
use std::fs;
use std::path::PathBuf;

use chrono::{DateTime, TimeZone, Utc};
use hegel::generators as gs;
use hegel::{HealthCheck, TestCase};
use locallibrary::device::{LineageEntry, LineageOp};
use locallibrary::model::LibrarySummary;
use locallibrary::paths::FileClass;
use locallibrary::progress::Ctx;
use locallibrary::store::{Snapshot, SnapshotSource, Store};
use locallibrary::tree::FileEntry;
use locallibrary::{Error, Hash};
use uuid::Uuid;

/// Contents worth storing: empty, tiny, repeated, binary.
const CONTENTS: &[&[u8]] = &[b"", b"a", b"abc", b"ID3 placeholder audio\n", b"\x00\xff\x00", b"PMAI"];

/// Creation times: few enough that two snapshots can share every field an id covers.
fn created(n: u8) -> DateTime<Utc> {
    Utc.timestamp_opt(1_700_000_000 + i64::from(n) * 86_400, 0).unwrap()
}

/// A file entry naming its object (the path says which, so verify's report can be read back).
fn entry(h: Hash, size: u64) -> FileEntry {
    FileEntry { path: format!("Contents/{}.mp3", h.to_hex()), size, mtime: 0, class: FileClass::Audio, hash: Some(h) }
}

fn hash_of_path(path: &str) -> Hash {
    path.strip_prefix("Contents/").and_then(|p| p.strip_suffix(".mp3")).unwrap().parse().unwrap()
}

/// The parts of a snapshot as stored, for comparing what was saved with what loads.
fn json(s: &Snapshot) -> serde_json::Value {
    serde_json::to_value(s).unwrap()
}

fn draw_snapshot_extras(tc: &TestCase, s: &mut Snapshot) {
    s.summary = LibrarySummary { tracks: tc.draw(gs::integers::<usize>().max_value(3)), ..Default::default() };
    s.source = tc
        .draw(gs::optional(gs::sampled_from(&["A", "B"][..])))
        .map(|name| SnapshotSource { drive_id: None, drive_name: name.into() });
    s.lineage = (0..tc.draw(gs::integers::<usize>().max_value(2)))
        .map(|i| LineageEntry {
            fingerprint: Hash::of(&[i as u8]),
            at: created(i as u8),
            op: LineageOp::Pull,
            snapshot: None,
            source: None,
            tracks: i,
        })
        .collect();
}

struct Library {
    id: Uuid,
    name: String,
    head: Option<Hash>,
}

struct StoreMachine {
    dir: tempfile::TempDir,
    store: Store,
    /// What each imported object holds.
    objects: IndexMap<Hash, Vec<u8>>,
    /// Objects overwritten or deleted behind the store's back.
    tampered: BTreeSet<Hash>,
    deleted: BTreeSet<Hash>,
    /// Saved snapshots, and the order they were first saved in (for draws to pick from).
    snapshots: IndexMap<Hash, Snapshot>,
    order: Vec<Hash>,
    /// Manifests overwritten with garbage.
    damaged: BTreeSet<Hash>,
    libraries: Vec<Library>,
}

impl StoreMachine {
    fn new() -> StoreMachine {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open_or_init(dir.path().join("store")).unwrap();
        StoreMachine {
            dir,
            store,
            objects: IndexMap::new(),
            tampered: BTreeSet::new(),
            deleted: BTreeSet::new(),
            snapshots: IndexMap::new(),
            order: Vec::new(),
            damaged: BTreeSet::new(),
            libraries: Vec::new(),
        }
    }

    fn on_disk(&self) -> Vec<Hash> {
        let mut out: Vec<Hash> = self.objects.keys().filter(|h| !self.deleted.contains(*h)).copied().collect();
        out.sort_unstable();
        out
    }

    fn manifest(&self, id: &Hash) -> PathBuf {
        self.store.root().join("snapshots").join(format!("{}.json", id.to_hex()))
    }

    fn pick<T: Clone + Send + Sync + std::fmt::Debug>(tc: &TestCase, items: &[T]) -> Option<T> {
        (!items.is_empty()).then(|| tc.draw(gs::sampled_from(items)))
    }

    /// The snapshots gc keeps, as the model knows them: every snapshot of a library the store
    /// has, the heads, and what they descend from.
    fn reachable(&self) -> BTreeSet<Hash> {
        let mut seen = BTreeSet::new();
        let mut queue: VecDeque<Hash> = self.libraries.iter().filter_map(|l| l.head).collect();
        queue.extend(
            self.snapshots.values().filter(|s| self.libraries.iter().any(|l| l.id == s.library_id)).map(|s| s.id),
        );
        while let Some(id) = queue.pop_front() {
            if seen.insert(id)
                && let Some(s) = self.snapshots.get(&id)
            {
                queue.extend(s.parents.iter().copied());
            }
        }
        seen.retain(|id| self.snapshots.contains_key(id));
        seen
    }

    fn import(&mut self, tc: &TestCase, bytes: Vec<u8>) {
        // An object whose size is wrong (tampered with here; cut short by a crash) is replaced.
        let existed = self.store.has_object(&Hash::of(&bytes)) && !self.tampered.contains(&Hash::of(&bytes));
        let h = if tc.draw(gs::booleans()) {
            self.store.import_bytes(&bytes).unwrap()
        } else {
            let src = self.dir.path().join("src.bin");
            fs::write(&src, &bytes).unwrap();
            let (h, new) = self.store.import_file(&src, None, &Ctx::silent()).unwrap();
            assert_eq!(new, !existed, "import_file says whether the object is new");
            h
        };
        assert_eq!(h, Hash::of(&bytes), "objects are addressed by their BLAKE3 hash");
        self.deleted.remove(&h);
        self.tampered.remove(&h);
        self.objects.insert(h, bytes);
    }
}

#[hegel::state_machine]
impl StoreMachine {
    /// Importing stores an object under its content's hash (once, however often it's imported).
    #[rule(weight = 3)]
    fn import_content(&mut self, tc: TestCase) {
        let bytes = match tc.draw(gs::booleans()) {
            true => tc.draw(gs::sampled_from(CONTENTS)).to_vec(),
            false => tc.draw(gs::binary().max_size(64)),
        };
        self.import(&tc, bytes);
    }

    /// Library names are unique ignoring ASCII case.
    #[rule]
    fn create_library(&mut self, tc: TestCase) {
        let name = tc.draw(gs::sampled_from(&["Main", "main", "MAIN", "Club", "Ünï", "ünï"][..])).to_string();
        let taken = self.libraries.iter().any(|l| l.name.eq_ignore_ascii_case(&name));
        match self.store.create_library(&name) {
            Ok(rec) => {
                assert!(!taken, "a second library named {name:?}");
                assert_eq!((rec.name.as_str(), rec.head), (name.as_str(), None));
                self.libraries.push(Library { id: rec.id, name, head: None });
            }
            Err(e) => assert!(taken && matches!(e.inner(), Error::Conflict(_)), "creating {name:?}: {e}"),
        }
    }

    /// A snapshot saves when every file's object is in the store, and loads back as saved.
    #[rule(weight = 3)]
    fn save_snapshot(&mut self, tc: TestCase) {
        // Not the libraries' own (random) ids, so snapshot ids, and the order gc and verify read
        // manifests in, are the same each run.
        let library_id = Uuid::from_u128(tc.draw(gs::integers::<u128>().max_value(2)));
        let mut parents = Vec::new();
        for _ in 0..tc.draw(gs::integers::<usize>().max_value(2)) {
            match Self::pick(&tc, &self.order) {
                Some(id) if tc.draw(gs::weighted_booleans(0.9)) => parents.push(id),
                _ => parents.push(Hash::of(b"no such snapshot")),
            }
        }
        let objects: Vec<Hash> = self.objects.keys().copied().collect();
        let mut files = Vec::new();
        for _ in 0..tc.draw(gs::integers::<usize>().max_value(3)) {
            match (Self::pick(&tc, &objects), tc.draw(gs::integers::<u8>().max_value(19))) {
                (_, 0) => files.push(entry(Hash::of(b"never imported"), 0)),
                (_, 1) => files.push(FileEntry { hash: None, ..entry(Hash::of(b""), 0) }),
                (Some(h), _) => files.push(entry(h, self.objects[&h].len() as u64)),
                (None, _) => {}
            }
        }
        let mut snap = Snapshot {
            id: Hash::of(b"ignored"),
            library_id,
            parents,
            created: created(tc.draw(gs::integers::<u8>().max_value(2))),
            message: tc.draw(gs::optional(gs::sampled_from(&["", "first", "日本語"][..]))).map(String::from),
            source: None,
            fingerprint: Hash::of(&[tc.draw(gs::integers::<u8>().max_value(2))]),
            summary: LibrarySummary::default(),
            lineage: Vec::new(),
            files,
        };
        draw_snapshot_extras(&tc, &mut snap);
        let storable = snap
            .files
            .iter()
            .all(|f| f.hash.is_some_and(|h| self.objects.contains_key(&h) && !self.deleted.contains(&h)));
        match self.store.save_snapshot(snap.clone()) {
            Ok(saved) => {
                assert!(storable, "saved a snapshot whose objects aren't all stored: {:?}", snap.files);
                assert_eq!(json(&saved), json(&Snapshot { id: saved.id, ..snap }), "save only sets the id");
                assert_eq!(json(&self.store.load_snapshot(&saved.id).unwrap()), json(&saved), "loads as saved");
                self.damaged.remove(&saved.id);
                if !self.order.contains(&saved.id) {
                    self.order.push(saved.id);
                }
                self.snapshots.insert(saved.id, saved);
            }
            Err(e) => {
                assert!(!storable, "refused a storable snapshot: {e}");
                tc.event("save refused");
            }
        }
    }

    /// Heads can be set for libraries that exist.
    #[rule(weight = 2)]
    fn set_head(&mut self, tc: TestCase) {
        let snapshot = match Self::pick(&tc, &self.order) {
            Some(id) if tc.draw(gs::weighted_booleans(0.9)) => id,
            _ => Hash::of(b"dangling head"),
        };
        let lib = Self::pick(&tc, &(0..self.libraries.len()).collect::<Vec<_>>());
        match lib {
            Some(i) if tc.draw(gs::weighted_booleans(0.95)) => {
                self.store.set_head(self.libraries[i].id, snapshot).unwrap();
                self.libraries[i].head = Some(snapshot);
            }
            _ => assert!(matches!(self.store.set_head(Uuid::from_u128(7), snapshot), Err(Error::NotFound(_)))),
        }
    }

    /// Something other than the store changes an object's bytes.
    #[rule(weight = 2)]
    fn tamper_with_object(&mut self, tc: TestCase) {
        let Some(h) = Self::pick(&tc, &self.on_disk()) else { return };
        let mut bytes = self.objects[&h].clone();
        bytes.push(b'!');
        fs::write(self.store.object_path(&h), bytes).unwrap();
        self.tampered.insert(h);
    }

    /// Something other than the store deletes an object.
    #[rule]
    fn delete_object(&mut self, tc: TestCase) {
        let Some(h) = Self::pick(&tc, &self.on_disk()) else { return };
        fs::remove_file(self.store.object_path(&h)).unwrap();
        self.deleted.insert(h);
        self.tampered.remove(&h);
    }

    /// Something other than the store garbles a snapshot's manifest.
    #[rule]
    fn damage_manifest(&mut self, tc: TestCase) {
        // Rarer than the other damage: a garbled manifest stops verify and gc (see below) until
        // it's saved again.
        if !tc.draw(gs::weighted_booleans(0.6)) {
            return;
        }
        let Some(id) = Self::pick(&tc, &self.order) else { return };
        if !self.snapshots.contains_key(&id) {
            return;
        }
        fs::write(self.manifest(&id), b"{ not a snapshot").unwrap();
        self.damaged.insert(id);
    }

    /// A garbled manifest is put back (from a copy of the store, say).
    #[rule]
    fn restore_manifest(&mut self, tc: TestCase) {
        let damaged: Vec<Hash> = self.damaged.iter().copied().collect();
        let Some(id) = Self::pick(&tc, &damaged) else { return };
        fs::write(self.manifest(&id), serde_json::to_vec(&self.snapshots[&id]).unwrap()).unwrap();
        self.damaged.remove(&id);
    }

    /// gc never removes a snapshot of a library the store has, one reachable from a head, or an
    /// object such a snapshot uses; it removes everything else. If it can't read a manifest it
    /// removes nothing: what the manifest belongs to and uses is unknown.
    #[rule(weight = 2)]
    fn gc(&mut self, tc: TestCase) {
        let dry_run = tc.draw(gs::booleans());
        let reachable = self.reachable();
        let before = (self.store.snapshot_ids().unwrap().len(), self.on_disk());
        let result = self.store.gc(dry_run);
        if !self.damaged.is_empty() {
            let after = (self.store.snapshot_ids().unwrap().len(), self.on_disk_now());
            assert_eq!(
                after,
                (before.0, before.1.clone()),
                "gc removed things with a manifest unreadable ({result:?})"
            );
            assert!(result.is_err(), "gc passed a store with a garbled manifest");
            tc.event("gc with a manifest unreadable");
            return;
        }
        let report = result.unwrap();
        if report.removed_snapshots > 0 && report.removed_objects > 0 {
            tc.event("gc removed snapshots and objects");
        }
        let live: BTreeSet<Hash> =
            reachable.iter().flat_map(|id| self.snapshots[id].files.iter().filter_map(|f| f.hash)).collect();
        let dead_snapshots: Vec<Hash> = self.snapshots.keys().filter(|id| !reachable.contains(*id)).copied().collect();
        let dead_objects: Vec<Hash> = self.on_disk().into_iter().filter(|h| !live.contains(h)).collect();
        assert_eq!(report.removed_snapshots, dead_snapshots.len(), "snapshots gc removes");
        assert_eq!(report.removed_objects, dead_objects.len(), "objects gc removes");
        let size = |h: &Hash| self.objects[h].len() as u64 + u64::from(self.tampered.contains(h));
        assert_eq!(report.freed_bytes, dead_objects.iter().map(size).sum::<u64>(), "bytes gc frees");
        if !dry_run {
            for id in &dead_snapshots {
                self.snapshots.shift_remove(id);
                self.order.retain(|o| o != id);
            }
            for h in &dead_objects {
                self.objects.shift_remove(h);
                self.tampered.remove(h);
            }
        }
        for id in &reachable {
            assert_eq!(
                json(&self.store.load_snapshot(id).unwrap()),
                json(&self.snapshots[id]),
                "reachable snapshot kept"
            );
        }
    }

    /// verify reports exactly the used objects that are missing or don't match their hash (or
    /// fails, when a manifest can't be read).
    #[rule(weight = 3)]
    fn verify(&mut self, tc: TestCase) {
        let result = self.store.verify(&Ctx::silent());
        if !self.damaged.is_empty() {
            assert!(result.is_err(), "verify passed a store with a garbled manifest: {result:?}");
            tc.event("verify with a garbled manifest");
            return;
        }
        let report = result.unwrap();
        let used: BTreeSet<Hash> =
            self.snapshots.values().flat_map(|s| s.files.iter().filter_map(|f| f.hash)).collect();
        let hashes = |paths: &[String]| paths.iter().map(|p| hash_of_path(p)).collect::<BTreeSet<_>>();
        let missing: BTreeSet<Hash> = used.iter().filter(|h| self.deleted.contains(*h)).copied().collect();
        let corrupt: BTreeSet<Hash> = used.iter().filter(|h| self.tampered.contains(*h)).copied().collect();
        assert_eq!(hashes(&report.missing), missing, "missing objects");
        assert_eq!(hashes(&report.corrupt), corrupt, "corrupt objects");
        assert_eq!(report.checked, used.len() - missing.len(), "objects checked");
        for (found, what) in [(&missing, "verify found missing objects"), (&corrupt, "verify found corrupt objects")] {
            if !found.is_empty() {
                tc.event(what);
            }
        }
    }

    /// A snapshot is found by any prefix of its id of four or more characters (either case)
    /// that no other id shares.
    #[rule]
    fn resolve(&mut self, tc: TestCase) {
        let Some(id) = Self::pick(&tc, &self.order) else { return };
        if !self.snapshots.contains_key(&id) {
            return;
        }
        let hex = id.to_hex();
        let len = tc.draw(gs::integers::<usize>().max_value(64));
        let prefix = if tc.draw(gs::booleans()) { hex[..len].to_ascii_uppercase() } else { hex[..len].to_string() };
        let matching = self.snapshots.keys().filter(|s| s.to_hex().starts_with(&hex[..len])).count();
        match self.store.resolve_snapshot(&prefix) {
            Ok(found) => assert!(len >= 4 && matching == 1 && found == id, "{prefix:?} resolved to {found}"),
            Err(e) => assert!(len < 4 || matching > 1, "{prefix:?} didn't resolve: {e}"),
        }
    }

    /// Ancestry follows parents (a snapshot is its own ancestor).
    #[rule]
    fn ancestry(&mut self, tc: TestCase) {
        let (Some(a), Some(b)) = (Self::pick(&tc, &self.order), Self::pick(&tc, &self.order)) else { return };
        let mut seen = BTreeSet::new();
        let mut queue = VecDeque::from([b]);
        let mut found = false;
        while let Some(id) = queue.pop_front() {
            found |= id == a;
            if seen.insert(id)
                && !self.damaged.contains(&id)
                && let Some(s) = self.snapshots.get(&id)
            {
                queue.extend(s.parents.iter().copied());
            }
        }
        assert_eq!(self.store.is_ancestor(&a, &b).unwrap(), found, "is {a:?} an ancestor of {b:?}");
    }

    /// The store lists the libraries, heads, snapshots and objects the model has.
    #[invariant]
    fn matches_the_model(&mut self, _: TestCase) {
        let libs: Vec<(Uuid, String, Option<Hash>)> =
            self.store.libraries().unwrap().into_iter().map(|l| (l.id, l.name, l.head)).collect();
        let want: Vec<(Uuid, String, Option<Hash>)> =
            self.libraries.iter().map(|l| (l.id, l.name.clone(), l.head)).collect();
        assert_eq!(libs, want, "libraries");
        let ids: BTreeSet<Hash> = self.store.snapshot_ids().unwrap().into_iter().collect();
        assert_eq!(ids, self.snapshots.keys().copied().collect(), "snapshots");
        assert_eq!(self.on_disk_now(), self.on_disk(), "objects");
    }
}

impl StoreMachine {
    /// Objects the store's directory holds.
    fn on_disk_now(&self) -> Vec<Hash> {
        let dir = self.store.root().join("objects");
        let mut out = Vec::new();
        for shard in fs::read_dir(&dir).unwrap() {
            let shard = shard.unwrap();
            let name = shard.file_name().to_string_lossy().into_owned();
            if name.len() != 2 {
                continue;
            }
            for obj in fs::read_dir(shard.path()).unwrap() {
                let file = obj.unwrap().file_name().to_string_lossy().into_owned();
                out.push(format!("{name}{file}").parse::<Hash>().unwrap());
            }
        }
        out.sort();
        out
    }
}

#[hegel::test(test_cases = 100, suppress_health_check = [HealthCheck::TooSlow])]
fn the_store_matches_the_model(tc: TestCase) {
    hegel::stateful::machine(StoreMachine::new()).steps(30).run(tc);
}

/// A snapshot's id covers its library, parents, creation time, message, fingerprint and files,
/// and nothing else: the summary is derived from the files, and the source drive and its lineage
/// are provenance, so the same content saved from two drives (or re-saved) keeps one id.
#[hegel::test(test_cases = 50)]
fn snapshot_ids_cover_content_but_not_provenance(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    let h = store.import_bytes(b"audio").unwrap();
    let other = store.import_bytes(b"other audio").unwrap();
    let base = Snapshot {
        id: Hash::of(b""),
        library_id: Uuid::from_u128(1),
        parents: vec![],
        created: created(0),
        message: None,
        source: None,
        fingerprint: Hash::of(b"fp"),
        summary: LibrarySummary::default(),
        lineage: vec![],
        files: vec![entry(h, 5)],
    };
    let first = store.save_snapshot(base.clone()).unwrap();
    let mut provenance = base.clone();
    draw_snapshot_extras(&tc, &mut provenance);
    let again = store.save_snapshot(provenance.clone()).unwrap();
    assert_eq!(again.id, first.id, "provenance isn't part of the id");
    let loaded = json(&store.load_snapshot(&first.id).unwrap());
    assert!(loaded == json(&first) || loaded == json(&again), "one of the saves loads: {loaded}");
    let mut changed = base.clone();
    match tc.draw(gs::integers::<u8>().max_value(5)) {
        0 => changed.library_id = Uuid::from_u128(2),
        1 => changed.parents = vec![first.id],
        2 => changed.created = created(1),
        3 => changed.message = Some(tc.draw(gs::text())),
        4 => changed.fingerprint = Hash::of(b"other"),
        _ => changed.files = vec![entry(other, 11)],
    }
    assert_ne!(store.save_snapshot(changed).unwrap().id, first.id, "content is part of the id");
}

/// Hashes print as 64 lower-case hex digits and parse back from either case; anything else is
/// refused, also from JSON.
#[hegel::test(test_cases = 200)]
fn hashes_round_trip_through_hex_and_serde(tc: TestCase) {
    let bytes = tc.draw(gs::binary().min_size(32).max_size(32));
    let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    let h: Hash = hex.parse().unwrap();
    assert_eq!((h.to_hex(), h.to_string(), h.short()), (hex.clone(), hex.clone(), hex[..12].to_string()));
    assert_eq!(hex.to_ascii_uppercase().parse::<Hash>().unwrap(), h);
    let json = serde_json::to_string(&h).unwrap();
    assert_eq!(json, format!("\"{hex}\""));
    assert_eq!(serde_json::from_str::<Hash>(&json).unwrap(), h);

    let s = match tc.draw(gs::integers::<u8>().max_value(3)) {
        0 => tc.draw(gs::text().max_size(70)),
        1 => tc.draw(gs::from_regex("[0-9a-fA-F]{60,68}").fullmatch(true)),
        2 => {
            let mut s = hex.clone();
            let at = tc.draw(gs::integers::<usize>().max_value(63));
            s.replace_range(at..at + 1, &tc.draw(gs::text().min_size(1).max_size(1)));
            s
        }
        _ => hex,
    };
    let valid = s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit());
    assert_eq!(s.parse::<Hash>().is_ok(), valid, "parsing {s:?}");
    assert_eq!(serde_json::from_value::<Hash>(serde_json::Value::String(s.clone())).is_ok(), valid, "{s:?} from JSON");
}

/// Resolving a prefix agrees with scanning the snapshot ids: one match resolves, none is not
/// found, several are ambiguous, under four characters is refused; other files are ignored.
#[hegel::test(test_cases = 100)]
fn resolving_a_prefix_agrees_with_a_scan(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    let snapshots = store.root().join("snapshots");
    // Ids that share prefixes of different lengths (only the file names matter here).
    let base = Hash::of(&tc.draw(gs::binary().max_size(8))).to_hex();
    let mut ids = BTreeSet::new();
    for _ in 0..tc.draw(gs::integers::<usize>().min_value(1).max_value(6)) {
        let keep = tc.draw(gs::integers::<usize>().max_value(64));
        let tail = Hash::of(&tc.draw(gs::binary().max_size(4))).to_hex();
        ids.insert(format!("{}{}", &base[..keep], &tail[keep..]));
    }
    for id in &ids {
        fs::write(snapshots.join(format!("{id}.json")), b"{}").unwrap();
    }
    for junk in ["notes.txt", "zzzz.json", "abcd.json.tmp"] {
        fs::write(snapshots.join(junk), b"").unwrap();
    }
    let len = tc.draw(gs::integers::<usize>().max_value(66));
    let prefix: String = match tc.draw(gs::booleans()) {
        true => base.chars().take(len).collect(),
        false => tc.draw(gs::from_regex("[0-9a-fA-Fz]{0,8}").fullmatch(true)),
    };
    let lower = prefix.to_ascii_lowercase();
    let matches: Vec<&String> = ids.iter().filter(|id| id.starts_with(&lower)).collect();
    match store.resolve_snapshot(&prefix) {
        Ok(found) => {
            assert!(prefix.len() >= 4 && matches.len() == 1, "{prefix:?} resolved to {found} ({matches:?})");
            assert_eq!(&found.to_hex(), matches[0]);
        }
        Err(Error::NotFound(_)) => {
            assert!(prefix.len() >= 4 && matches.is_empty(), "{prefix:?} not found ({matches:?})")
        }
        Err(e) => assert!(prefix.len() < 4 || matches.len() > 1, "{prefix:?}: {e} ({matches:?})"),
    }
}

/// verify reports every object a snapshot uses that is missing or whose bytes no longer match
/// its hash (same length or not), and nothing else.
#[hegel::test(test_cases = 50)]
fn verify_reports_exactly_the_damaged_objects(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    let mut objects: IndexMap<Hash, Vec<u8>> = IndexMap::new();
    for bytes in tc.draw(gs::vecs(gs::binary().max_size(16)).min_size(1).max_size(6)) {
        objects.insert(store.import_bytes(&bytes).unwrap(), bytes);
    }
    let hashes: Vec<Hash> = objects.keys().copied().collect();
    let mut used = BTreeSet::new();
    for n in 0..tc.draw(gs::integers::<u8>().min_value(1).max_value(3)) {
        let picked = tc.draw(gs::vecs(gs::integers::<usize>().max_value(hashes.len() - 1)).max_size(4));
        let files: Vec<FileEntry> =
            picked.iter().map(|i| entry(hashes[*i], objects[&hashes[*i]].len() as u64)).collect();
        used.extend(files.iter().filter_map(|f| f.hash));
        let snap = Snapshot {
            id: Hash::of(b""),
            library_id: Uuid::from_u128(1),
            parents: vec![],
            created: created(n),
            message: None,
            source: None,
            fingerprint: Hash::of(&[n]),
            summary: LibrarySummary::default(),
            lineage: vec![],
            files,
        };
        store.save_snapshot(snap).unwrap();
    }
    let (mut missing, mut corrupt) = (BTreeSet::new(), BTreeSet::new());
    for (h, bytes) in &objects {
        match tc.draw(gs::integers::<u8>().max_value(3)) {
            0 => {
                fs::remove_file(store.object_path(h)).unwrap();
                missing.insert(*h);
            }
            1 => {
                let mut changed = bytes.clone();
                match changed.first_mut() {
                    Some(b) => *b ^= 1 << tc.draw(gs::integers::<u8>().max_value(7)),
                    None => changed.push(0),
                }
                fs::write(store.object_path(h), changed).unwrap();
                corrupt.insert(*h);
            }
            _ => {}
        }
    }
    let report = store.verify(&Ctx::silent()).unwrap();
    let hashes = |paths: &[String]| paths.iter().map(|p| hash_of_path(p)).collect::<BTreeSet<_>>();
    assert_eq!(hashes(&report.missing), &missing & &used, "missing objects");
    assert_eq!(hashes(&report.corrupt), &corrupt & &used, "corrupt objects");
    assert_eq!(report.checked, used.len() - (&missing & &used).len(), "objects checked");
}

/// Regression: gc with a head whose manifest can't be read deleted that snapshot's ancestors
/// (reachability skipped the unreadable manifest; gc only failed on reaching it afterwards).
#[test]
fn gc_removes_nothing_when_a_reachable_manifest_is_garbled() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    let lib = store.create_library("Main").unwrap();
    let h = store.import_bytes(b"audio").unwrap();
    let mut parents = vec![];
    let mut ids = vec![];
    for n in 0..5 {
        let snap = Snapshot {
            id: Hash::of(b""),
            library_id: lib.id,
            parents: parents.clone(),
            created: created(n),
            message: None,
            source: None,
            fingerprint: Hash::of(&[n]),
            summary: LibrarySummary::default(),
            lineage: vec![],
            files: vec![entry(h, 5)],
        };
        let id = store.save_snapshot(snap).unwrap().id;
        parents = vec![id];
        ids.push(id);
    }
    let head = *ids.last().unwrap();
    store.set_head(lib.id, head).unwrap();
    fs::write(store.root().join("snapshots").join(format!("{}.json", head.to_hex())), b"{ garbled").unwrap();
    assert!(store.gc(false).is_err());
    assert_eq!(store.snapshot_ids().unwrap().len(), 5, "every snapshot is kept");
    assert!(store.has_object(&h));
}

/// Any number making the store at once (threads let go together), in a folder that's missing,
/// empty or holding only what locallibrary keeps beside a store, all open the same one: one
/// made it and the rest found it. A folder holding anything else is refused by each of them and
/// left as it was.
#[hegel::test(test_cases = 50)]
fn making_the_store_at_once_makes_one(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("store");
    match tc.draw(gs::integers::<u8>().max_value(3)) {
        0 => {}
        1 => fs::create_dir(&root).unwrap(),
        2 => {
            fs::create_dir_all(root.join("cache")).unwrap();
            fs::write(root.join("tui.json"), "{}").unwrap();
        }
        _ => {
            fs::create_dir(&root).unwrap();
            fs::write(root.join("notes.txt"), "mine").unwrap();
        }
    }
    let before: BTreeSet<_> = fs::read_dir(&root).into_iter().flatten().map(|e| e.unwrap().file_name()).collect();
    let n = tc.draw(gs::integers::<usize>().min_value(2).max_value(8));
    let start = std::sync::Barrier::new(n);
    let made: Vec<Result<Uuid, Error>> = std::thread::scope(|s| {
        let handles: Vec<_> = (0..n)
            .map(|_| {
                s.spawn(|| {
                    start.wait();
                    Store::open_or_init(&root).and_then(|store| store.id())
                })
            })
            .collect();
        handles.into_iter().map(|h| h.join().unwrap()).collect()
    });
    if before.contains(std::ffi::OsStr::new("notes.txt")) {
        assert!(made.iter().all(|m| matches!(m, Err(Error::Invalid(_)))), "{made:?}");
        let after: BTreeSet<_> = fs::read_dir(&root).unwrap().map(|e| e.unwrap().file_name()).collect();
        assert_eq!(after, before);
        return;
    }
    let ids: BTreeSet<Uuid> = made.into_iter().map(|m| m.unwrap()).collect();
    assert_eq!(ids, BTreeSet::from([Store::open(&root).unwrap().id().unwrap()]));
}
