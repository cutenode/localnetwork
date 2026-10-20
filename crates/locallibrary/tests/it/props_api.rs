//! Properties of the service layer (`api`): the store's libraries, snapshots, labels, history,
//! pruning and garbage against a model; undo and putting backups back against the libraries
//! each change replaced; restoring one playlist from a snapshot or a backup; finding drives by
//! path, name and id; syncing, pulling and pushing going the way the drives' histories say; the
//! settings profile, artwork set on drives and embedded in files; the reads and plans agreeing
//! with the modules they wrap; and requests and responses agreeing with their JSON Schemas.

use crate::common::{self, library, path};
use crate::schema;

use indexmap::IndexMap;
use rustc_hash::FxHashMap;
use std::any::TypeId;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use base64::prelude::*;
use chrono::{DateTime, Duration, TimeZone, Utc};
use hegel::generators as gs;
use hegel::{HealthCheck, TestCase};
use locallibrary::api::{self, Direction, LabelRequest, RenameLibrary};
use locallibrary::compare::{self, Relation, Side};
use locallibrary::device::Device;
use locallibrary::edit::{Edit, EditOptions, TrackFields};
use locallibrary::error::Reason;
use locallibrary::paths::{self, FileClass};
use locallibrary::progress::Ctx;
use locallibrary::query::TrackSelector;
use locallibrary::retention::PruneOptions;
use locallibrary::settings::{self, DriveSettings, SCHEMA, SettingValue};
use locallibrary::store::{Snapshot, Store};
use locallibrary::tree::FileEntry;
use locallibrary::{Error, Hash, Library, artwork, backup, clean, convert, merge, xml};
use schemars::JsonSchema;
use schemars::generate::SchemaSettings;
use serde::Serialize;
use serde_json::Value;
use uuid::Uuid;

// ---------------------------------------------------------------------------------------------
// Schemas
// ---------------------------------------------------------------------------------------------

/// `root` with each struct it holds, where a value goes (at the top, as a field, an item or a
/// map's value), naming every field the value may have: `unevaluatedProperties: false`, which
/// sees through the parts of a flattened struct (`allOf`) and an enum's options. A part alone
/// names only some of the fields, so the parts themselves are left open; a map names none.
fn closed(mut root: Value) -> Value {
    fn names_fields(root: &Value, schema: &Value) -> bool {
        let s = schema::resolved(root, schema);
        s.get("properties").is_some()
            || ["allOf", "anyOf", "oneOf"].iter().any(|k| {
                s.get(*k).and_then(Value::as_array).is_some_and(|parts| parts.iter().any(|p| names_fields(root, p)))
            })
    }
    fn close(root: &Value, schema: &mut Value, a_value: bool) {
        if a_value && names_fields(root, schema) {
            schema["unevaluatedProperties"] = Value::Bool(false);
        }
        let Some(s) = schema.as_object_mut() else { return };
        for (k, inner) in s.iter_mut() {
            match (k.as_str(), inner) {
                ("allOf" | "anyOf" | "oneOf", Value::Array(parts)) => {
                    parts.iter_mut().for_each(|p| close(root, p, false))
                }
                ("properties", Value::Object(fields)) => fields.values_mut().for_each(|f| close(root, f, true)),
                ("items" | "additionalProperties", inner) => close(root, inner, true),
                _ => {}
            }
        }
    }
    let original = root.clone();
    for def in root.get_mut("$defs").and_then(Value::as_object_mut).into_iter().flat_map(|d| d.values_mut()) {
        close(&original, def, false);
    }
    close(&original, &mut root, true);
    root
}

/// The validator for `T`'s JSON Schema, made `for_serialize` (a field skipped when empty isn't
/// required) or for deserialising, with every struct [`closed`].
fn validator_for<T: JsonSchema>(serialize: bool) -> jsonschema::Validator {
    let settings = SchemaSettings::draft2020_12();
    let settings = if serialize { settings.for_serialize() } else { settings.for_deserialize() };
    let root = closed(settings.into_generator().into_root_schema_for::<T>().to_value());
    schema::options().build(&root).unwrap_or_else(|e| panic!("{e}: {root}"))
}

/// `value` serialises to what its type's JSON Schema describes, as the RPC contract gives a
/// result's.
fn assert_described<T: Serialize + JsonSchema + 'static>(value: &T) {
    thread_local! {
        /// Each type's validator, made once: making one walks every type it holds.
        static VALIDATORS: std::cell::RefCell<FxHashMap<TypeId, jsonschema::Validator>> = Default::default();
    }
    let json = serde_json::to_value(value).unwrap();
    let broken = VALIDATORS.with(|v| {
        let mut v = v.borrow_mut();
        schema::violations(v.entry(TypeId::of::<T>()).or_insert_with(|| validator_for::<T>(true)), &json)
    });
    if let Some(e) = broken {
        panic!("{} doesn't match its schema: {e}\n{json:#}", std::any::type_name::<T>());
    }
}

// ---------------------------------------------------------------------------------------------
// The store: libraries, snapshots, labels, history, prune and gc
// ---------------------------------------------------------------------------------------------

/// The demo export's database, which snapshots in the store machine list (so pruning can read
/// them for history lists).
fn demo_pdb() -> Vec<u8> {
    fs::read(common::fixtures().join("rekordcrate/demo_tracks").join(paths::EXPORT_PDB)).unwrap()
}

const LIBRARY_NAMES: &[&str] = &["Main", "main", "Club", "Ünï", "ünï", "Home"];
const LABELS: &[&str] = &["NYE", "nye", "Gig", "before the set", "  "];

/// A library the model knows, its snapshots in order (the line its latest goes back through).
struct ModelLibrary {
    id: Uuid,
    name: String,
    line: Vec<Hash>,
}

/// What the model knows of a saved snapshot.
struct ModelSnapshot {
    library_id: Uuid,
    created: DateTime<Utc>,
    /// Saved beside the library's line (as a drive back from a set is).
    beside: bool,
    /// The object only it lists, for one of no library: collected with it.
    own_object: Option<Hash>,
}

struct StoreApiMachine {
    dir: tempfile::TempDir,
    store: Store,
    pdb: Vec<u8>,
    /// The database's object is in the store (collecting garbage removes it when no snapshot
    /// that stays lists it).
    pdb_stored: bool,
    libraries: Vec<ModelLibrary>,
    snapshots: IndexMap<Hash, ModelSnapshot>,
    /// Gone by pruning: still walked through, and named as pruned when asked for.
    pruned: BTreeSet<Hash>,
    labels: Vec<(String, Hash)>,
    steps: u32,
}

impl StoreApiMachine {
    fn new(pdb: &[u8]) -> StoreApiMachine {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open_or_init(dir.path().join("store")).unwrap();
        store.import_bytes(pdb).unwrap();
        let mut m = StoreApiMachine {
            dir,
            store,
            pdb: pdb.to_vec(),
            pdb_stored: true,
            libraries: Vec::new(),
            snapshots: IndexMap::new(),
            pruned: BTreeSet::new(),
            labels: Vec::new(),
            steps: 0,
        };
        // A library with a snapshot to start from, so labels and history have something to name.
        let rec = m.store.create_library("Home").unwrap();
        let id = m.save(rec.id, None, None);
        m.store.set_head(rec.id, id).unwrap();
        m.libraries.push(ModelLibrary { id: rec.id, name: rec.name, line: vec![id] });
        m
    }

    fn pick<T>(tc: &TestCase, items: &[T]) -> Option<usize> {
        (!items.is_empty()).then(|| tc.draw(gs::integers::<usize>().max_value(items.len() - 1)))
    }

    /// Save a snapshot of `library_id` after `parent`, an hour after the last.
    fn save(&mut self, library_id: Uuid, parent: Option<Hash>, own: Option<&[u8]>) -> Hash {
        self.steps += 1;
        let created = Utc.timestamp_opt(1_750_000_000, 0).unwrap() + Duration::hours(i64::from(self.steps));
        let pdb = self.store.import_bytes(&self.pdb).unwrap();
        self.pdb_stored = true;
        let mut files = vec![FileEntry {
            path: paths::EXPORT_PDB.into(),
            size: self.pdb.len() as u64,
            mtime: 0,
            class: FileClass::Database,
            hash: Some(pdb),
        }];
        let own_object = own.map(|bytes| {
            let h = self.store.import_bytes(bytes).unwrap();
            files.push(FileEntry {
                path: format!("Contents/{}.mp3", h.short()),
                size: bytes.len() as u64,
                mtime: 0,
                class: FileClass::Audio,
                hash: Some(h),
            });
            h
        });
        let snap = Snapshot {
            id: Hash::of(b""),
            library_id,
            parents: parent.into_iter().collect(),
            created,
            message: Some(format!("step {}", self.steps)),
            source: None,
            fingerprint: Hash::of(&self.steps.to_le_bytes()),
            summary: Default::default(),
            lineage: Vec::new(),
            files,
        };
        let id = self.store.save_snapshot(snap).unwrap().id;
        self.snapshots.insert(id, ModelSnapshot { library_id, created, beside: false, own_object });
        id
    }

    fn library_of(&self, id: Uuid) -> Option<&ModelLibrary> {
        self.libraries.iter().find(|l| l.id == id)
    }

    fn label_of(&self, name: &str) -> Option<&(String, Hash)> {
        self.labels.iter().find(|(l, _)| l.eq_ignore_ascii_case(name.trim()))
    }

    /// Snapshots of no library, unlabelled: what collecting garbage deletes.
    fn garbage(&self) -> Vec<Hash> {
        self.snapshots
            .iter()
            .filter(|(id, s)| self.library_of(s.library_id).is_none() && !self.labels.iter().any(|(_, l)| l == *id))
            .map(|(id, _)| *id)
            .collect()
    }

    /// The files collecting garbage deletes: those only the garbage lists, and the database
    /// when no snapshot that stays does.
    fn garbage_objects(&self) -> usize {
        let garbage = self.garbage();
        let own = garbage.iter().filter_map(|id| self.snapshots[id].own_object).count();
        own + usize::from(self.pdb_stored && garbage.len() == self.snapshots.len())
    }

    /// Forget what collecting garbage deletes.
    fn collect(&mut self) {
        let garbage = self.garbage();
        self.pdb_stored &= garbage.len() < self.snapshots.len();
        for id in garbage {
            self.snapshots.shift_remove(&id);
        }
    }

    /// A way to name snapshot `id` to the api: a label it has, its short id, all of it, or a
    /// prefix, with or without `snapshot:`.
    fn spec_for(&self, tc: &TestCase, id: &Hash) -> String {
        let hex = id.to_hex();
        let labelled: Vec<&String> = self.labels.iter().filter(|(_, l)| l == id).map(|(n, _)| n).collect();
        match tc.draw(gs::integers::<u8>().max_value(4)) {
            0 if !labelled.is_empty() => labelled[0].to_uppercase(),
            1 => format!("snapshot:{}", &hex[..8]),
            2 => hex.to_ascii_uppercase(),
            3 => hex[..tc.draw(gs::integers::<usize>().min_value(4).max_value(12))].to_string(),
            _ => id.short(),
        }
    }
}

#[hegel::state_machine]
impl StoreApiMachine {
    /// A library to commit to.
    #[rule]
    fn create_library(&mut self, tc: TestCase) {
        let name = tc.draw(gs::sampled_from(LIBRARY_NAMES)).to_string();
        let taken = self.libraries.iter().any(|l| l.name.eq_ignore_ascii_case(&name));
        match self.store.create_library(&name) {
            Ok(rec) => {
                assert!(!taken);
                self.libraries.push(ModelLibrary { id: rec.id, name, line: Vec::new() });
            }
            Err(e) => assert!(taken && e.reason() == Some(Reason::Exists), "{e}"),
        }
    }

    /// Each commit adds one snapshot to the library's line, and becomes its latest.
    #[rule(weight = 3)]
    fn commit(&mut self, tc: TestCase) {
        let Some(i) = Self::pick(&tc, &self.libraries) else { return };
        let (lib, head) = (self.libraries[i].id, self.libraries[i].line.last().copied());
        let id = self.save(lib, head, None);
        self.store.set_head(lib, id).unwrap();
        self.libraries[i].line.push(id);
    }

    /// A snapshot saved beside a library's line, as a drive back from a set is.
    #[rule]
    fn save_beside(&mut self, tc: TestCase) {
        let Some(i) = Self::pick(&tc, &self.libraries) else { return };
        let Some(&head) = self.libraries[i].line.last() else { return };
        let id = self.save(self.libraries[i].id, Some(head), None);
        self.snapshots[&id].beside = true;
    }

    /// A snapshot of a library the store doesn't have (one removed by an older version), with a
    /// file of its own.
    #[rule]
    fn save_orphan(&mut self, tc: TestCase) {
        let lib = Uuid::from_u128(tc.draw(gs::integers::<u128>().max_value(1)) + 7);
        let own = format!("audio only snapshot {} has", self.steps + 1);
        self.save(lib, None, Some(own.as_bytes()));
    }

    /// Labelling resolves the snapshot however it's named; a label names one snapshot, can't be
    /// blank, and labelling again with it is a no-op.
    #[rule(weight = 2)]
    fn label(&mut self, tc: TestCase) {
        let label = tc.draw(gs::sampled_from(LABELS)).to_string();
        let ids: Vec<Hash> = self.snapshots.keys().copied().collect();
        let (spec, target) = match Self::pick(&tc, &ids) {
            Some(i) if tc.draw(gs::weighted_booleans(0.85)) => (Some(self.spec_for(&tc, &ids[i])), Some(ids[i])),
            _ => (tc.draw(gs::sampled_from(&[None, Some("ffffffff"), Some("ab")][..])).map(String::from), None),
        };
        let pruned_prefix = spec.as_deref().is_some_and(|s| self.pruned.iter().any(|p| p.to_hex().starts_with(s)));
        let res = api::snapshot_label(
            &self.store,
            &LabelRequest { label: label.clone(), snapshot: spec.clone(), remove: false },
        );
        match (spec.as_deref(), target) {
            (None, _) => assert!(matches!(res, Err(Error::Invalid(_))), "{res:?}"),
            (Some("ab"), _) => assert!(matches!(res, Err(Error::Invalid(_))), "{res:?}"),
            (Some(_), None) => match pruned_prefix {
                true => assert!(res.is_err_and(|e| e.to_string().contains("pruned"))),
                false => assert!(matches!(res, Err(Error::NotFound(_))), "{res:?}"),
            },
            (Some(spec), Some(id)) => {
                let blank = label.trim().is_empty();
                let held = self.label_of(&label).cloned();
                // Short ids and prefixes another snapshot shares are ambiguous.
                let bare = spec.strip_prefix("snapshot:").unwrap_or(spec).to_ascii_lowercase();
                let named_by_label = self.label_of(spec).is_some();
                let shared = !named_by_label && ids.iter().filter(|h| h.to_hex().starts_with(&bare)).count() > 1;
                match res {
                    Ok(l) => {
                        assert!(!blank && !shared, "{l:?}");
                        assert_eq!(l.snapshot, id, "{spec:?} labelled the wrong snapshot");
                        match held {
                            Some((name, on)) => assert_eq!((l.name, l.snapshot), (name, on), "labelled again"),
                            None => {
                                assert_eq!(l.name, label.trim());
                                self.labels.push((l.name, id));
                            }
                        }
                    }
                    Err(e) if blank || shared => assert!(matches!(e.inner(), Error::Invalid(_)), "{e}"),
                    Err(e) => {
                        let (_, on) = held.unwrap_or_else(|| panic!("labelling {spec:?} {label:?}: {e}"));
                        assert_ne!(on, id);
                        assert!(matches!(e.inner(), Error::Conflict(_)), "{e}");
                    }
                }
            }
        }
    }

    /// Taking a label off frees it; a label no snapshot has isn't found.
    #[rule]
    fn unlabel(&mut self, tc: TestCase) {
        let label = tc.draw(gs::sampled_from(LABELS)).to_string();
        let req = LabelRequest { label: label.clone(), snapshot: None, remove: true };
        match (api::snapshot_label(&self.store, &req), self.label_of(&label).cloned()) {
            (Ok(l), Some((name, id))) => {
                assert_eq!((l.name.as_str(), l.snapshot), (name.as_str(), id));
                self.labels.retain(|(n, _)| *n != name);
            }
            (Err(e), None) => assert!(matches!(e, Error::NotFound(_)), "{e}"),
            (res, held) => panic!("taking {label:?} off gave {res:?}; the model has {held:?}"),
        }
    }

    /// A library is renamed by its name in any case, or its id, to a name no other library
    /// has (ignoring ASCII case); an unknown one is refused naming the libraries there are.
    #[rule]
    fn rename(&mut self, tc: TestCase) {
        let i = Self::pick(&tc, &self.libraries).filter(|_| tc.draw(gs::weighted_booleans(0.9)));
        let spec = match i {
            Some(i) if tc.draw(gs::booleans()) => self.libraries[i].id.to_string(),
            Some(i) => format!(" {} ", self.libraries[i].name.to_ascii_uppercase()),
            None => "Nowhere".into(),
        };
        let name = tc.draw(gs::sampled_from(&["Main", "MAIN", "Club", "ünï", "Backup", "  "][..])).to_string();
        let res = api::library_rename(&self.store, &RenameLibrary { library: spec.clone(), name: name.clone() });
        let Some(i) = i else {
            let e = res.unwrap_err();
            assert!(matches!(e, Error::NotFound(_)), "{e}");
            let listed = match self.libraries.is_empty() {
                true => "it has no libraries yet".to_string(),
                false => format!("{:?}", self.libraries[0].name),
            };
            assert!(e.to_string().contains(&listed), "{e}");
            return;
        };
        let id = self.libraries[i].id;
        let taken = self.libraries.iter().any(|l| l.id != id && l.name.eq_ignore_ascii_case(name.trim()));
        match res {
            Ok(rec) => {
                assert!(!taken && !name.trim().is_empty());
                assert_eq!((rec.id, rec.name.as_str()), (id, name.trim()));
                self.libraries[i].name = name.trim().to_string();
            }
            Err(e) if name.trim().is_empty() => assert!(matches!(e, Error::Invalid(_)), "{e}"),
            Err(e) => assert!(taken && e.reason() == Some(Reason::Exists), "{e}"),
        }
    }

    /// Pruning keeps each library's latest, its labelled snapshots and its last N, removes
    /// exactly the rest of its snapshots (and with them what collecting garbage would), and
    /// is planned first: the plan names what goes, applying it removes that, and planning
    /// again finds nothing to remove.
    #[rule]
    fn prune(&mut self, tc: TestCase) {
        let keep_last = tc.draw(gs::integers::<usize>().max_value(3));
        let only = Self::pick(&tc, &self.libraries).filter(|_| tc.draw(gs::booleans()));
        let opts = PruneOptions {
            keep_last: Some(keep_last),
            library: only.map(|i| self.libraries[i].name.to_ascii_lowercase()),
            ..Default::default()
        };
        let ctx = Ctx::silent();
        let planned = api::store_prune_plan(&self.store, &opts, &ctx).unwrap();
        assert_described(&planned);
        let mut doomed = BTreeSet::new();
        for (i, lib) in self.libraries.iter().enumerate() {
            if only.is_some_and(|o| o != i) {
                continue;
            }
            let mut theirs: Vec<(&Hash, &ModelSnapshot)> =
                self.snapshots.iter().filter(|(_, s)| s.library_id == lib.id).collect();
            theirs.sort_by(|a, b| b.1.created.cmp(&a.1.created).then(b.0.cmp(a.0)));
            for (n, (id, _)) in theirs.iter().enumerate() {
                let kept = n < keep_last || lib.line.last() == Some(id) || self.labels.iter().any(|(_, l)| l == *id);
                if !kept {
                    doomed.insert(**id);
                }
            }
        }
        let planned_ids: BTreeSet<Hash> =
            planned.plan.libraries.iter().flat_map(|l| l.removed.iter().map(|s| s.id)).collect();
        assert_eq!(planned_ids, doomed, "the snapshots the plan removes");
        assert_eq!(planned.plan.removed_snapshots, doomed.len());
        assert_eq!((planned.noop, planned.destructive), (doomed.is_empty(), !doomed.is_empty()));
        // A snapshot that stays lists the database whenever one is pruned (its library's latest).
        let objects = self.garbage_objects();
        assert_eq!(planned.plan.removed_objects, if doomed.is_empty() { 0 } else { objects });

        // Without the store it's refused, and changes nothing.
        assert!(planned.apply(None, &ctx).is_err_and(|e| e.to_string().contains("needs the store")));
        let report = match tc.draw(gs::booleans()) {
            true => planned.apply(Some(&self.store), &ctx).unwrap(),
            false => api::store_prune(&self.store, &opts, &ctx).unwrap(),
        };
        assert!(!report.dry_run);
        assert_eq!(report.removed_snapshots, doomed.len());
        if !doomed.is_empty() {
            assert_eq!(report.removed_objects, objects);
            self.collect();
        }
        for id in &doomed {
            self.snapshots.shift_remove(id);
            self.pruned.insert(*id);
        }
        let again = api::store_prune_plan(&self.store, &opts, &ctx).unwrap();
        assert!(again.noop && again.plan.removed_snapshots == 0, "pruned again: {:?}", again.summary);
        // A dry run says what would go and removes nothing.
        let dry = api::store_prune(&self.store, &PruneOptions { dry_run: true, ..opts }, &ctx).unwrap();
        assert!(dry.dry_run && dry.removed_snapshots == 0);
    }

    /// Collecting garbage removes the snapshots of no library that no label keeps, and the
    /// files only they listed; the plan says so first, and after it there's nothing to collect.
    #[rule]
    fn gc(&mut self, tc: TestCase) {
        let planned = api::store_gc_plan(&self.store).unwrap();
        assert_described(&planned);
        let (garbage, objects) = (self.garbage(), self.garbage_objects());
        assert_eq!((planned.plan.removed_snapshots, planned.plan.removed_objects), (garbage.len(), objects));
        assert_eq!(planned.noop, garbage.is_empty() && objects == 0);
        if tc.draw(gs::booleans()) {
            return;
        }
        assert!(planned.apply(None, &Ctx::silent()).is_err());
        let report = planned.apply(Some(&self.store), &Ctx::silent()).unwrap();
        assert_eq!((report.removed_snapshots, report.removed_objects, report.dry_run), (garbage.len(), objects, false));
        self.collect();
        assert!(api::store_gc_plan(&self.store).unwrap().noop);
    }

    /// A copy of the store holds every snapshot it has, and is listed as there and up to date.
    #[rule]
    fn copy(&mut self, _: TestCase) {
        let to = self.dir.path().join("copy");
        // Mirrored, it holds what the store has now, whatever was pruned or collected since.
        let options = api::CopyOptions { mirror: true, ..Default::default() };
        let req = api::StoreCopyRequest { path: to, options };
        let report = api::store_copy(&self.store, &req, &Ctx::silent()).unwrap();
        assert_eq!(report.snapshots, self.snapshots.len());
        let copies = api::store_copies(&self.store).unwrap();
        assert_described(&copies);
        assert_eq!(copies.len(), 1, "one copy, however often it's brought up to date");
        assert!(copies[0].available && copies[0].up_to_date == Some(true), "{:?}", copies[0]);
    }

    /// Every library is listed by name, with the snapshots its line has and its latest; its
    /// history lists its snapshots, newest first, those beside the line marked, with their
    /// labels; and the store's figures count what it holds.
    #[invariant]
    fn matches_the_model(&mut self, tc: TestCase) {
        let listed = api::libraries_list(&self.store).unwrap();
        assert_described(&listed);
        let mut want: Vec<&ModelLibrary> = self.libraries.iter().collect();
        want.sort_by_key(|l| l.name.to_lowercase());
        let line = |l: &ModelLibrary| l.line.iter().filter(|id| !self.pruned.contains(*id)).count();
        let got: Vec<(Uuid, &str, usize, Option<Hash>)> =
            listed.iter().map(|l| (l.id, l.name.as_str(), l.snapshots, l.head.as_ref().map(|h| h.id))).collect();
        let expected: Vec<(Uuid, &str, usize, Option<Hash>)> =
            want.iter().map(|l| (l.id, l.name.as_str(), line(l), l.line.last().copied())).collect();
        assert_eq!(got, expected, "libraries");

        if let Some(i) = Self::pick(&tc, &self.libraries) {
            let lib = &self.libraries[i];
            let limit = tc.draw(gs::sampled_from(&[None, Some(0), Some(1), Some(3)][..]));
            let req = api::HistoryRequest { library: lib.name.to_ascii_uppercase(), limit };
            let history = api::history(&self.store, &req).unwrap();
            {
                assert_described(&history);
                let mut theirs: Vec<(&Hash, &ModelSnapshot)> =
                    self.snapshots.iter().filter(|(_, s)| s.library_id == lib.id).collect();
                theirs.sort_by_key(|(_, s)| std::cmp::Reverse(s.created));
                theirs.truncate(limit.unwrap_or(usize::MAX));
                let labels = |id: &Hash| -> Vec<String> {
                    self.labels.iter().filter(|(_, l)| l == id).map(|(n, _)| n.clone()).collect()
                };
                let got: Vec<(Hash, bool, Vec<String>)> =
                    history.iter().map(|s| (s.id, s.beside, s.labels.clone())).collect();
                let want: Vec<(Hash, bool, Vec<String>)> =
                    theirs.iter().map(|(id, s)| (**id, s.beside, labels(id))).collect();
                assert_eq!(got, want, "history of {}", lib.name);
            }
        }
        let unknown = api::history(&self.store, &api::HistoryRequest { library: "Nowhere".into(), limit: None });
        assert!(matches!(unknown, Err(Error::NotFound(_))));

        let info = api::store_info(&self.store).unwrap();
        assert_described(&info);
        assert_eq!((info.stats.libraries, info.stats.snapshots), (self.libraries.len(), self.snapshots.len()));
        let own = self.snapshots.values().filter(|s| s.own_object.is_some()).count();
        assert_eq!(info.stats.objects, usize::from(self.pdb_stored) + own, "the database, and each orphan's own file");
        assert!(info.copy_of.is_none());
    }
}

#[hegel::test(test_cases = 8, suppress_health_check = [HealthCheck::TooSlow])]
fn the_store_s_libraries_snapshots_and_labels_match_the_model(tc: TestCase) {
    hegel::stateful::machine(StoreApiMachine::new(&demo_pdb())).steps(15).run(tc);
}

// ---------------------------------------------------------------------------------------------
// Undo
// ---------------------------------------------------------------------------------------------

/// An edit a DJ makes: a playlist made or deleted, or a track rated.
fn draw_edit(tc: &TestCase, root: &Path) -> Edit {
    let lib = library(root);
    let pick = |n: usize| tc.draw(gs::integers::<usize>().max_value(n - 1));
    match tc.draw(gs::integers::<u8>().max_value(2)) {
        1 if !lib.playlists.is_empty() => {
            Edit::DeletePlaylist { playlist: lib.playlists[pick(lib.playlists.len())].name.clone(), with_tracks: false }
        }
        2 => {
            let tracks = TrackSelector { ids: vec![lib.tracks[pick(lib.tracks.len())].id], ..Default::default() };
            let rating = Some(tc.draw(gs::integers::<u8>().max_value(5)));
            Edit::EditTracks { tracks, fields: TrackFields { rating, ..Default::default() } }
        }
        _ => Edit::CreatePlaylist {
            path: tc.draw(gs::sampled_from(&["Friday", "Saturday", "Warmup"][..])).to_string(),
            folder: false,
        },
    }
}

/// An edit made through the api, and the library it replaced.
struct Change {
    n: usize,
    before: Library,
}

/// Undo through every way the api offers it, against the libraries each change replaced.
struct UndoMachine {
    _dir: tempfile::TempDir,
    root: PathBuf,
    /// What undo steps back through, the latest last.
    stack: Vec<Change>,
    /// The latest change, which a forced undo puts back whatever was undone since.
    latest: Option<Change>,
    changes: usize,
    /// A track's audio was deleted behind locallibrary's back: putting a backup back that
    /// lists it needs forcing.
    lost: bool,
}

impl UndoMachine {
    fn new() -> UndoMachine {
        let dir = tempfile::tempdir().unwrap();
        let vols = common::isolate(dir.path(), None);
        let root = Device::open(common::demo_drive(&vols, "USB")).unwrap().root().to_path_buf();
        UndoMachine { _dir: dir, root, stack: Vec::new(), latest: None, changes: 0, lost: false }
    }

    fn drive(&self) -> String {
        path(&self.root)
    }
}

impl Drop for UndoMachine {
    fn drop(&mut self) {
        locallibrary::testing::volumes_on_this_thread(None);
    }
}

#[hegel::state_machine]
impl UndoMachine {
    /// An edit that changes something is backed up first.
    #[rule(weight = 2)]
    fn edit(&mut self, tc: TestCase) {
        let edits = vec![draw_edit(&tc, &self.root)];
        let req = api::EditRequest { drive: self.drive(), edits, options: EditOptions::default() };
        let plan = api::edit_plan(&req, &Ctx::silent()).unwrap();
        if plan.noop {
            return;
        }
        let before = library(&self.root);
        plan.apply(None, &Ctx::silent()).unwrap();
        self.changes += 1;
        self.stack.push(Change { n: self.changes, before: before.clone() });
        self.latest = Some(Change { n: self.changes, before });
    }

    /// Undoing puts back the library the last change not yet undone replaced (forced, the
    /// latest change's, whatever was undone since); with nothing left to undo it's refused, and
    /// with audio its backup lists gone it's refused unless forced. Each way of asking agrees.
    #[rule(weight = 2)]
    fn undo(&mut self, tc: TestCase) {
        let force = self.lost && tc.draw(gs::weighted_booleans(0.7));
        let named = tc.draw(gs::weighted_booleans(0.8));
        let req = api::UndoRequest { drive: named.then(|| self.drive()), backup: None, force };
        let now = library(&self.root);
        let way = tc.draw(gs::integers::<u8>().max_value(2));
        let planned = api::undo_plan(&req);
        let expected = match (force, &self.latest) {
            (true, Some(latest)) => Some(&latest.before),
            (true, None) => None,
            (false, _) => self.stack.last().map(|c| &c.before),
        };
        let Some(expected) = expected.cloned() else {
            let e = planned.unwrap_err();
            assert!(matches!(e, Error::NotFound(_)), "{e}");
            let restored = api::restore_backup(&api::RestoreBackup { drive: self.drive(), backup: None, force });
            assert!(matches!(restored, Err(Error::NotFound(_))), "{restored:?}");
            assert_eq!(library(&self.root), now, "a refused undo changes nothing");
            return;
        };
        if self.lost && !force {
            let e = planned.unwrap_err();
            assert_eq!(e.reason(), Some(Reason::MissingAudio), "{e}");
            assert_eq!(library(&self.root), now, "a refused undo changes nothing");
            return;
        }
        let plan = planned.unwrap();
        assert_described(&plan);
        assert!(!plan.noop || force, "{:?}", plan.summary);
        assert!(plan.destructive);
        assert_eq!((plan.drive.as_str(), plan.root.as_path()), ("USB", self.root.as_path()));
        if !force {
            assert!(!plan.files.is_empty(), "the files it puts back");
            assert_eq!(plan.journal.len(), 1, "the change's edit comes off the journal");
        }
        let id = plan.backup.id.clone();
        let put_back = match way {
            0 => plan.apply(None, &Ctx::silent()).unwrap(),
            1 => api::undo_apply(&req, &Ctx::silent()).unwrap(),
            _ => api::restore_backup(&api::RestoreBackup { drive: self.drive(), backup: None, force }).unwrap(),
        };
        assert_eq!(put_back.id, id, "the backup the plan named");
        assert_eq!(library(&self.root), expected, "undo put back the library before the change");
        match force {
            true => {
                let latest = self.latest.as_ref().unwrap().n;
                if self.stack.last().is_some_and(|c| c.n == latest) {
                    self.stack.pop();
                }
            }
            false => {
                self.stack.pop();
            }
        }
    }

    /// Audio deleted outside locallibrary.
    #[rule]
    fn lose_audio(&mut self, _: TestCase) {
        let lib = library(&self.root);
        let audio = paths::to_fs(&self.root, &paths::key(&lib.tracks[0].file_path));
        if fs::remove_file(audio).is_ok() {
            self.lost = true;
        }
    }

    /// What can be undone is listed newest first, the one undo takes marked, and agrees with
    /// the drive's backups; asked for none in particular, the drive with backups is the one
    /// listed.
    #[invariant]
    fn the_listing_agrees(&mut self, _: TestCase) {
        let named = api::undo_list(None, &api::UndoListRequest { drive: Some(self.drive()) }).unwrap();
        assert_described(&named);
        assert_eq!(named.drives.len(), 1);
        let backups = &named.drives[0].backups;
        let ids: Vec<&str> = backups.iter().map(|b| b.backup.id.as_str()).collect();
        let listed = api::list_backups(&api::DriveRef { drive: self.drive() }).unwrap();
        assert_eq!(ids, listed.iter().map(|b| b.id.as_str()).collect::<Vec<_>>());
        let created: Vec<_> = listed.iter().map(|b| (b.seq, b.created)).collect();
        assert!(created.windows(2).all(|w| w[0] >= w[1]), "newest first: {created:?}");
        let next: Vec<&str> = backups.iter().filter(|b| b.next).map(|b| b.backup.id.as_str()).collect();
        assert!(next.len() <= 1, "{next:?}");
        if !self.stack.is_empty() {
            assert_eq!(next.len(), 1, "the one undo takes");
            if !self.lost {
                let plan = api::undo_plan(&api::UndoRequest { drive: Some(self.drive()), ..Default::default() });
                assert_eq!(plan.unwrap().backup.id, next[0]);
            }
        }
        let all = api::undo_list(None, &api::UndoListRequest::default()).unwrap();
        let drives: Vec<&str> = all.drives.iter().map(|d| d.drive.as_str()).collect();
        assert_eq!(drives, if listed.is_empty() { vec![] } else { vec!["USB"] });
    }
}

#[hegel::test(test_cases = 4, suppress_health_check = [HealthCheck::TooSlow])]
fn undo_steps_back_through_the_changes_made(tc: TestCase) {
    hegel::stateful::machine(UndoMachine::new()).steps(7).run(tc);
}

/// An undo naming no drive takes the only mounted drive with something to undo; with none it's
/// not found, and with several it's refused, saying which. Audio the backup lists gone from the
/// drive, or a database it has, needs forcing (the second is put back); a backup that isn't
/// there isn't found.
#[hegel::test(test_cases = 2, suppress_health_check = [HealthCheck::TooSlow])]
fn an_undo_naming_no_drive_needs_one_with_backups(tc: TestCase) {
    let m = UndoMachine::new();
    let vols = m.root.parent().unwrap().to_path_buf();
    let none = api::undo_plan(&api::UndoRequest::default()).unwrap_err();
    assert!(matches!(&none, Error::NotFound(m) if m.contains("something to undo")), "{none}");
    let edit = |root: &Path, tc: &TestCase| {
        let req =
            api::EditRequest { drive: path(root), edits: vec![draw_edit(tc, root)], options: EditOptions::default() };
        let plan = api::edit_plan(&req, &Ctx::silent()).unwrap();
        // Rating a track as it's rated changes nothing: make a playlist instead.
        let plan = match plan.noop {
            true => api::edit_plan(
                &api::EditRequest {
                    drive: path(root),
                    edits: vec![Edit::CreatePlaylist { path: "Undo me".into(), folder: false }],
                    options: EditOptions::default(),
                },
                &Ctx::silent(),
            )
            .unwrap(),
            false => plan,
        };
        plan.apply(None, &Ctx::silent()).unwrap();
    };
    edit(&m.root, &tc);
    assert_eq!(api::undo_plan(&api::UndoRequest::default()).unwrap().root, m.root, "the only one");

    let lib = library(&m.root);
    fs::remove_file(paths::to_fs(&m.root, &paths::key(&lib.tracks[0].file_path))).unwrap();
    let named = api::UndoRequest { drive: Some(path(&m.root)), backup: None, force: false };
    let e = api::undo_plan(&named).unwrap_err();
    assert_eq!(e.reason(), Some(Reason::MissingAudio), "{e}");
    assert_eq!(e.detail().fields["missing"].as_array().map(Vec::len), Some(1));
    let forced = api::undo_plan(&api::UndoRequest { force: true, ..named.clone() }).unwrap();
    assert_eq!(forced.missing_audio.len(), 1);
    assert!(forced.warnings.iter().any(|w| w.contains("have no audio")), "{:?}", forced.warnings);

    // A database the backup has gone from the drive: the drive changed since, so only forced
    // is it put back.
    fs::remove_file(m.root.join(paths::EXPORT_EXT_PDB)).unwrap();
    let e = api::undo_plan(&named).unwrap_err();
    assert_eq!(e.reason(), Some(Reason::ChangedSince), "{e}");
    let forced = api::undo_plan(&api::UndoRequest { force: true, ..named.clone() }).unwrap();
    let back: Vec<(&str, &str)> = forced.files.iter().map(|f| (f.path.as_str(), f.change)).collect();
    assert!(back.contains(&(paths::EXPORT_EXT_PDB, "put back")), "{back:?}");

    let other = Device::open(common::demo_drive(&vols, "Other")).unwrap().root().to_path_buf();
    edit(&other, &tc);
    let e = api::undo_plan(&api::UndoRequest::default()).unwrap_err();
    assert_eq!(e.reason(), Some(Reason::ChooseDrive), "{e}");
    let said: BTreeSet<String> = serde_json::from_value(e.detail().fields["drives"].clone()).unwrap();
    assert_eq!(said, [path(&m.root), path(&other)].into());
    assert_eq!(e.detail().actions[0].method.as_deref(), Some("undo.list"));
    let unknown = api::UndoRequest { backup: Some("nope".into()), ..named };
    assert!(matches!(api::undo_plan(&unknown), Err(Error::NotFound(_))));
}

// ---------------------------------------------------------------------------------------------
// Sync, pull and push
// ---------------------------------------------------------------------------------------------

/// The drive's library state as compare tells states apart.
fn fingerprint(root: &Path) -> Option<Hash> {
    let d = Device::open(root).unwrap();
    d.has_export().then(|| Side::from_device(&d, &Ctx::silent()).unwrap().fingerprint).flatten()
}

/// Three drives (two holding the demo export, one blank) and a store, changed through the api
/// and by rekordbox exporting to one of them; the model knows which library state each holds and
/// which states came from which, so it knows which of two drives is newer.
struct SyncMachine {
    _dir: tempfile::TempDir,
    store: Store,
    roots: Vec<PathBuf>,
    /// The state each drive holds (`None`: blank).
    held: Vec<Option<usize>>,
    /// Each state's parent, and its fingerprint: states with the same content are the same to
    /// compare, however they were reached.
    parents: Vec<Option<usize>>,
    fingerprints: Vec<Option<Hash>>,
    /// The state of the store library's latest, and the fingerprints of the states saved.
    head: Option<usize>,
    saved: BTreeSet<Hash>,
    labels: BTreeSet<String>,
    exports: usize,
}

impl SyncMachine {
    fn new() -> SyncMachine {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open_or_init(dir.path().join("store")).unwrap();
        let vols = common::isolate(dir.path(), Some(store.root()));
        // Backup starts synced from Main, so the two share their history (a copy made outside
        // locallibrary shares none, and which of two such copies is newer is only a guess); Spare
        // is blank.
        let roots = [common::demo_drive(&vols, "Main"), common::blank(&vols, "Backup"), common::blank(&vols, "Spare")];
        let roots: Vec<PathBuf> = roots.iter().map(|r| Device::open(r).unwrap().root().to_path_buf()).collect();
        api::sync_apply(None, &common::sync_pair(&roots[0], &roots[1]), &Ctx::silent()).unwrap();
        let first = fingerprint(&roots[0]);
        SyncMachine {
            _dir: dir,
            store,
            roots,
            held: vec![Some(0), Some(0), None],
            parents: vec![None],
            fingerprints: vec![first],
            head: None,
            saved: BTreeSet::new(),
            labels: BTreeSet::new(),
            exports: 0,
        }
    }

    /// The fingerprints of the states `s` came through, oldest first, as a drive holding it
    /// records them.
    fn history(&self, mut s: usize) -> Vec<Option<Hash>> {
        let mut history = vec![self.fingerprints[s]];
        while let Some(p) = self.parents[s] {
            history.push(self.fingerprints[p]);
            s = p;
        }
        history.reverse();
        history.dedup();
        history
    }

    /// Whether state `from` has been through state `of`, by content: `from` held `of`'s
    /// fingerprint after every state the two histories share, and after every state `of` came
    /// through since first holding it. Those `of` came through before that and `from` never held
    /// don't count (a drive's lineage keeps only its latest entries).
    fn descends(&self, from: usize, of: usize) -> bool {
        let (ours, theirs, current) = (self.history(from), self.history(of), self.fingerprints[of]);
        let Some(at) = ours.iter().rposition(|f| *f == current) else { return false };
        let since = theirs.iter().position(|f| *f == current).unwrap();
        theirs.iter().enumerate().all(|(n, f)| ours.iter().position(|o| o == f).map_or(n < since, |i| i <= at))
    }

    /// How two states stand, as compare should say: the same, left newer, right newer, or
    /// neither (`None`).
    fn order(&self, l: usize, r: usize) -> Option<Relation> {
        if self.fingerprints[l] == self.fingerprints[r] {
            return Some(Relation::Identical);
        }
        match (self.descends(l, r), self.descends(r, l)) {
            (true, false) => Some(Relation::LeftNewer),
            (false, true) => Some(Relation::RightNewer),
            _ => None,
        }
    }

    /// Drive `i` changed to a new state after the one it held.
    fn changed(&mut self, i: usize) -> Option<usize> {
        self.parents.push(self.held[i]);
        self.fingerprints.push(fingerprint(&self.roots[i]));
        self.held[i] = Some(self.parents.len() - 1);
        self.held[i]
    }

    /// A drive holding an export (the first always does, until it's overwritten with another's).
    fn with_export(&self, tc: &TestCase) -> Option<usize> {
        let held: Vec<usize> = (0..3).filter(|i| self.held[*i].is_some()).collect();
        (!held.is_empty()).then(|| held[tc.draw(gs::integers::<usize>().max_value(held.len() - 1))])
    }

    /// Two different drives.
    fn pair(&self, tc: &TestCase) -> (usize, usize) {
        let i = tc.draw(gs::integers::<usize>().max_value(2));
        let j = (i + tc.draw(gs::integers::<usize>().min_value(1).max_value(2))) % 3;
        (i, j)
    }

    /// Drive `to` now holds what `from` does.
    fn copied(&mut self, from: usize, to: usize) {
        self.held[to] = self.held[from];
        assert_eq!(fingerprint(&self.roots[to]), fingerprint(&self.roots[from]), "the copy holds the same library");
    }
}

impl Drop for SyncMachine {
    fn drop(&mut self) {
        locallibrary::testing::volumes_on_this_thread(None);
        locallibrary::testing::store_root_on_this_thread(None);
    }
}

#[hegel::state_machine]
impl SyncMachine {
    /// An edit makes a new state of the library, after the one the drive held; it reaches the
    /// library's other mounted clones, which then hold the same.
    #[rule]
    fn edit(&mut self, tc: TestCase) {
        let Some(i) = self.with_export(&tc) else { return };
        let req = api::EditRequest {
            drive: path(&self.roots[i]),
            edits: vec![draw_edit(&tc, &self.roots[i])],
            options: EditOptions::default(),
        };
        let plan = api::edit_plan(&req, &Ctx::silent()).unwrap();
        if plan.noop {
            return;
        }
        plan.apply(Some(&self.store), &Ctx::silent()).unwrap();
        let new = self.changed(i);
        for c in plan.clones.iter().filter(|c| c.left_alone.is_none()) {
            let j = self.roots.iter().position(|r| *r == c.root).unwrap();
            self.copied(i, j);
            assert_eq!(self.held[j], new);
        }
    }

    /// rekordbox exports a track to one drive: it alone moves on.
    #[rule(weight = 2)]
    fn rekordbox_export(&mut self, tc: TestCase) {
        let Some(i) = self.with_export(&tc) else { return };
        self.exports += 1;
        common::export_new_track(&self.roots[i], "Exported", &format!("Track {}", self.exports));
        self.changed(i);
    }

    /// A sync copies the newer drive onto the other, the way compare says it should, and after
    /// it they hold the same library: syncing them again has nothing to do. Drives neither of
    /// which is newer are refused, offering to copy either way or merge; so is a drive synced
    /// with itself, and two blank ones.
    #[rule(weight = 2)]
    fn sync(&mut self, tc: TestCase) {
        let (i, j) = self.pair(&tc);
        if tc.draw(gs::weighted_booleans(0.1)) {
            let e = api::sync_plan(&common::sync_pair(&self.roots[i], &self.roots[i]), &Ctx::silent()).unwrap_err();
            assert!(e.to_string().contains("same drive"), "{e}");
        }
        let req = common::sync_pair(&self.roots[i], &self.roots[j]);
        let planned = api::sync_plan(&req, &Ctx::silent());
        let (from, to) = match (self.held[i], self.held[j]) {
            (None, None) => {
                assert!(planned.is_err_and(|e| e.to_string().contains("neither drive")));
                return;
            }
            (Some(_), None) => (i, j),
            (None, Some(_)) => (j, i),
            (Some(l), Some(r)) => match self.order(l, r) {
                Some(Relation::Identical) => {
                    let planned = planned.unwrap();
                    assert!(planned.noop && planned.plan.plan.is_none(), "{:?}", planned.summary);
                    assert_eq!(planned.comparison.relation, Relation::Identical);
                    let done = planned.apply(Some(&self.store), &Ctx::silent()).unwrap();
                    assert!(done.report.is_none() && done.source.is_none());
                    return;
                }
                Some(Relation::LeftNewer) => (i, j),
                Some(_) => (j, i),
                None => {
                    let e = planned.unwrap_err();
                    assert_eq!(e.reason(), Some(Reason::Diverged), "{e}");
                    let methods: Vec<Option<String>> = e.detail().actions.into_iter().map(|a| a.method).collect();
                    let want = ["sync.plan", "sync.plan", "merge.plan", "merge.plan"].map(|m| Some(m.to_string()));
                    assert_eq!(methods, want);
                    return;
                }
            },
        };
        let planned = planned.unwrap();
        assert_described(&planned);
        assert_eq!(planned.source.as_deref(), Some(self.roots[from].as_path()));
        assert_eq!(planned.target.as_deref(), Some(self.roots[to].as_path()));
        // Compare takes two exports; a sync's comparison with a blank drive is its own.
        if self.held[i].is_some() && self.held[j].is_some() {
            let pair = api::ComparePair { left: path(&self.roots[i]), right: path(&self.roots[j]) };
            let compared = api::compare(&pair, &Ctx::silent()).unwrap();
            assert_eq!(compared.relation, planned.comparison.relation, "the sync's comparison is compare's");
        }
        let result = match tc.draw(gs::booleans()) {
            true => planned.apply(Some(&self.store), &Ctx::silent()).unwrap(),
            false => api::sync_apply(Some(&self.store), &req, &Ctx::silent()).unwrap(),
        };
        assert_described(&result);
        assert_eq!(result.target.as_deref(), Some(self.roots[to].as_path()));
        self.copied(from, to);
        assert!(api::sync_plan(&req, &Ctx::silent()).unwrap().noop, "synced again");
    }

    /// Copying one way regardless overwrites the other drive with the first's library; keeping
    /// the players' changes is only for a drive back from a set.
    #[rule]
    fn sync_one_way(&mut self, tc: TestCase) {
        let (i, j) = self.pair(&tc);
        if self.held[i].is_none() {
            return;
        }
        let (left, right, direction) = match tc.draw(gs::booleans()) {
            true => (i, j, Direction::LeftToRight),
            false => (j, i, Direction::RightToLeft),
        };
        let req = api::SyncRequest { direction, ..common::sync_pair(&self.roots[left], &self.roots[right]) };
        if self.held[j].is_some() && tc.draw(gs::weighted_booleans(0.3)) {
            let kept = api::SyncRequest { keep_player_changes: true, ..req.clone() };
            assert!(matches!(api::sync_plan(&kept, &Ctx::silent()), Err(Error::StalePlan(_))));
        }
        api::sync_apply(Some(&self.store), &req, &Ctx::silent()).unwrap();
        self.copied(i, j);
    }

    /// Syncing all three copies the one newer than (or the same as) each other onto the rest;
    /// with no such drive it's refused. One drive, or one twice, isn't several.
    #[rule]
    fn sync_all(&mut self, tc: TestCase) {
        let one = common::sync_many(&[&self.roots[0]]);
        assert!(api::sync_many_plan(&one, &Ctx::silent()).is_err_and(|e| e.to_string().contains("at least two")));
        let twice = common::sync_many(&[&self.roots[1], &self.roots[1]]);
        assert!(api::sync_many_plan(&twice, &Ctx::silent()).is_err_and(|e| e.to_string().contains("listed twice")));
        let req = common::sync_many(&self.roots.iter().map(PathBuf::as_path).collect::<Vec<_>>());
        let planned = api::sync_many_plan(&req, &Ctx::silent());
        let states: Vec<usize> = self.held.iter().flatten().copied().collect();
        let newest = states.iter().copied().find(|&s| {
            states.iter().all(|&o| matches!(self.order(s, o), Some(Relation::Identical | Relation::LeftNewer)))
        });
        let Some(state) = newest else {
            let e = planned.unwrap_err();
            assert_eq!(e.reason(), Some(Reason::Diverged), "{e}");
            return;
        };
        let planned = planned.unwrap();
        assert_described(&planned);
        assert_eq!(fingerprint(&planned.source), self.fingerprints[state]);
        let all_same = self.held.iter().all(|s| s.is_some_and(|s| self.order(s, state) == Some(Relation::Identical)));
        assert_eq!(planned.noop, all_same, "{:?}", planned.summary);
        let results = match tc.draw(gs::booleans()) {
            true => planned.apply(Some(&self.store), &Ctx::silent()).unwrap(),
            false => api::sync_many_apply(Some(&self.store), &req, &planned.plan, &Ctx::silent()).unwrap(),
        };
        assert_described(&results);
        assert!(results.iter().all(|r| r.error.is_none()), "{results:?}");
        let source = self.roots.iter().position(|r| *r == planned.source).unwrap();
        for i in 0..3 {
            self.copied(source, i);
        }
    }

    /// Pulling a drive saves its state as the library's latest when it's newer than the
    /// latest (once: pulling it again is up to date), labelled when asked; an older state is
    /// refused offering a push that can be made (saved or not), and one that went its own way as
    /// diverged; so is a label blank or already taken, or a library not there yet.
    #[rule(weight = 2)]
    fn pull(&mut self, tc: TestCase) {
        let Some(i) = self.with_export(&tc) else { return };
        let state = self.held[i].unwrap();
        let label = match tc.draw(gs::integers::<u8>().max_value(4)) {
            0 => Some(format!("gig {}", self.labels.len())),
            1 => self.labels.iter().next().cloned(),
            2 => Some(" ".to_string()),
            _ => None,
        };
        let new_library = self.head.is_none() && tc.draw(gs::weighted_booleans(0.7));
        let req = api::PullRequest {
            drive: path(&self.roots[i]),
            library: Some("Home".into()),
            new_library,
            label: label.clone(),
            ..Default::default()
        };
        let res = api::pull(&self.store, &req, &Ctx::silent());
        if self.head.is_none() && !new_library {
            let e = res.unwrap_err();
            assert_eq!(e.reason(), Some(Reason::UnknownLibrary), "{e}");
            assert_eq!(e.detail().actions[0].params["new_library"], true);
            return;
        }
        if label.as_deref().is_some_and(|l| l.trim().is_empty()) {
            assert!(matches!(res, Err(Error::Invalid(_))), "{res:?}");
            return;
        }
        if let Some(l) = label.as_ref().filter(|l| self.labels.contains(*l)) {
            let e = res.unwrap_err();
            assert!(matches!(e.inner(), Error::Conflict(_)) && e.to_string().contains(l.as_str()), "{e}");
            return;
        }
        match self.head.map(|h| self.order(state, h)) {
            None | Some(Some(Relation::LeftNewer)) => {
                let report = res.unwrap();
                assert_described(&report);
                let snap = report.snapshot.expect("a new snapshot");
                assert_eq!(snap.labels, label.iter().cloned().collect::<Vec<_>>());
                self.labels.extend(label);
                self.head = Some(state);
                self.saved.extend(self.fingerprints[state]);
                // The library's name now means its mounted clones, the drive pulled among them,
                // and a request naming it goes to one of them.
                let group = api::library_clones("HOME").unwrap().expect("a library, not a drive");
                assert!(group.drives.contains(&self.roots[i]), "{:?}", group.drives);
                let (drives, left_out) = api::expand_libraries(&["home".to_string()]).unwrap();
                assert_eq!((drives, left_out), (group.drives.iter().map(|d| path(d)).collect(), vec![]));
                if let Ok(lead) = api::resolve_drive("Home") {
                    assert!(group.drives.iter().any(|d| d == lead.root()));
                }
            }
            Some(Some(Relation::Identical)) => {
                let report = res.unwrap();
                assert!(report.up_to_date, "{report:?}");
                self.labels.extend(label);
            }
            // `RightNewer`: the latest went through this state.
            Some(Some(_)) => {
                let e = res.unwrap_err();
                assert_eq!(e.reason(), Some(Reason::OlderThanStore), "{e}");
                let push = e.detail().actions.into_iter().find(|a| a.method.as_deref() == Some("push.plan"));
                let req: api::PushRequest =
                    serde_json::from_value(Value::Object(push.expect("a push").params)).unwrap();
                assert_described(&api::push_plan(&self.store, &req, &Ctx::silent()).unwrap());
            }
            // Neither went through the other's state.
            Some(None) => {
                let e = res.unwrap_err();
                assert_eq!(e.reason(), Some(Reason::DivergedFromStore), "{e}");
            }
        }
        let lib = self.store.find_library("Home").unwrap();
        let history = api::history(&self.store, &api::HistoryRequest { library: "home".into(), limit: None }).unwrap();
        assert_eq!(history.first().map(|s| s.id), lib.and_then(|l| l.head));
    }

    /// Pushing writes the library's latest onto a drive that's blank, holds a state the store
    /// has or one the latest went through, after which it holds exactly that state; a drive with
    /// changes the store doesn't have is refused.
    #[rule(weight = 2)]
    fn push(&mut self, tc: TestCase) {
        let Some(head) = self.head else { return };
        let j = tc.draw(gs::integers::<usize>().max_value(2));
        let req = api::PushRequest {
            drive: path(&self.roots[j]),
            library: Some("Home".into()),
            snapshot: None,
            force: false,
            verify: false,
            keep_extra: false,
            delete_first: false,
        };
        let planned = api::push_plan(&self.store, &req, &Ctx::silent());
        let saved = self.held[j]
            .is_none_or(|s| self.fingerprints[s].is_some_and(|f| self.saved.contains(&f)) || self.descends(head, s));
        if !saved {
            let e = planned.unwrap_err();
            assert_eq!(e.reason(), Some(Reason::NotSaved), "{e}");
            return;
        }
        let planned = planned.unwrap();
        assert_described(&planned);
        let lib = self.store.find_library("Home").unwrap().unwrap();
        assert_eq!(Some(planned.snapshot.id), lib.head);
        match tc.draw(gs::booleans()) {
            true => planned.apply(Some(&self.store), &Ctx::silent()).map(|_| ()).unwrap(),
            false => api::push_apply(&self.store, &req, &Ctx::silent()).map(|_| ()).unwrap(),
        }
        self.held[j] = Some(head);
        let snap = self.store.load_snapshot(&lib.head.unwrap()).unwrap();
        assert_eq!(fingerprint(&self.roots[j]), Some(snap.fingerprint));
    }

    /// Each mounted drive linked to the library stands against its latest as the model says.
    #[invariant]
    fn status_agrees(&mut self, _: TestCase) {
        let statuses = api::status(Some(&self.store), &Ctx::silent()).unwrap();
        assert_described(&statuses);
        for st in statuses {
            let i = self.roots.iter().position(|r| *r == st.drive.root).unwrap();
            let (Some(state), Some(head), Some(relation)) = (self.held[i], self.head, st.relation) else { continue };
            assert_eq!(st.library.as_deref(), Some("Home"));
            if let Some(want) = self.order(head, state) {
                assert_eq!(relation, want, "{} against the library's latest", st.drive.root.display());
            }
        }
    }
}

#[hegel::test(test_cases = 3, suppress_health_check = [HealthCheck::TooSlow])]
fn syncs_pulls_and_pushes_go_the_way_the_states_say(tc: TestCase) {
    hegel::stateful::machine(SyncMachine::new()).steps(8).run(tc);
}

/// The examples hegel found for `syncs_pulls_and_pushes_go_the_way_the_states_say` make their
/// edits by hand: drive `i` takes `edit`.
fn edit_drive(m: &SyncMachine, i: usize, edit: Edit) {
    let req = api::EditRequest { drive: path(&m.roots[i]), edits: vec![edit], options: EditOptions::default() };
    api::edit_plan(&req, &Ctx::silent()).unwrap().apply(Some(&m.store), &Ctx::silent()).unwrap();
}

/// Drive `i`'s first track rated `rating`.
fn rate(m: &SyncMachine, i: usize, rating: u8) {
    let tracks = TrackSelector { ids: vec![library(&m.roots[i]).tracks[0].id], ..Default::default() };
    edit_drive(m, i, Edit::EditTracks { tracks, fields: TrackFields { rating: Some(rating), ..Default::default() } });
}

/// Drive `i` pulled into the library "Home" (made by it when `new_library`).
fn pull_home(m: &SyncMachine, i: usize, new_library: bool) -> locallibrary::Result<locallibrary::sync::PullReport> {
    let req =
        api::PullRequest { drive: path(&m.roots[i]), library: Some("Home".into()), new_library, ..Default::default() };
    api::pull(&m.store, &req, &Ctx::silent())
}

/// The first example hegel found for `syncs_pulls_and_pushes_go_the_way_the_states_say`: a track
/// rated on Main, synced onto Backup, then rated back. Main is back to a state Backup's lineage
/// holds from before the sync, but only Main moved on since, so it's the newer. Once rekordbox
/// exports to Backup too, both moved on: they diverged, and Backup isn't pulled on top of Main's.
#[test]
fn a_drive_edited_back_to_an_earlier_state_moved_on_from_the_other() {
    let m = SyncMachine::new();
    let (main, backup) = (&m.roots[0], &m.roots[1]);
    let relation = || api::compare(&api::ComparePair { left: path(main), right: path(backup) }, &Ctx::silent());
    let was = library(main).tracks[0].rating;
    rate(&m, 0, was % 5 + 1);
    api::sync_apply(Some(&m.store), &common::sync_pair(main, backup), &Ctx::silent()).unwrap();
    rate(&m, 0, was);
    assert_eq!(fingerprint(main), m.fingerprints[0], "Main holds what it did before the sync");
    assert_eq!(relation().unwrap().relation, Relation::LeftNewer);
    let all = common::sync_many(&m.roots.iter().map(PathBuf::as_path).collect::<Vec<_>>());
    assert_eq!(api::sync_many_plan(&all, &Ctx::silent()).unwrap().source, *main);

    common::export_new_track(backup, "Exported", "Track 1");
    assert_eq!(relation().unwrap().relation, Relation::Diverged);
    pull_home(&m, 0, true).unwrap();
    let e = pull_home(&m, 1, false).unwrap_err();
    assert_eq!(e.reason(), Some(Reason::DivergedFromStore), "{e}");
}

/// The second: Spare, synced from Backup, gets a playlist; meanwhile Backup's track is rated,
/// synced onto Main and rated back there. Main's state, pulled, is the one Spare's lineage starts
/// with, but Spare changed it before Main went through the rating: Spare isn't pulled on top.
#[test]
fn a_drive_changed_before_another_went_and_came_back_isnt_pulled_on_top_of_it() {
    let m = SyncMachine::new();
    let (main, backup, spare) = (&m.roots[0], &m.roots[1], &m.roots[2]);
    api::sync_apply(Some(&m.store), &common::sync_pair(backup, spare), &Ctx::silent()).unwrap();
    let was = library(main).tracks[0].rating;
    rate(&m, 1, was % 5 + 1);
    api::sync_apply(Some(&m.store), &common::sync_pair(main, backup), &Ctx::silent()).unwrap();
    rate(&m, 0, was);
    edit_drive(&m, 2, Edit::CreatePlaylist { path: "Friday".into(), folder: false });
    pull_home(&m, 0, true).unwrap();
    let e = pull_home(&m, 2, false).unwrap_err();
    assert_eq!(e.reason(), Some(Reason::DivergedFromStore), "{e}");
    let pair = api::ComparePair { left: path(main), right: path(spare) };
    assert_eq!(api::compare(&pair, &Ctx::silent()).unwrap().relation, Relation::Diverged);
}

/// The third: Main and Backup each make the playlist Friday, and Main then rates a track. Backup
/// holds what Main held after its playlist, and only Main moved on from there: it's the newer,
/// and syncing all three copies it over the others.
#[test]
fn a_drive_that_made_the_change_another_made_is_behind_it_once_that_moved_on() {
    let m = SyncMachine::new();
    let (main, backup) = (&m.roots[0], &m.roots[1]);
    let friday = || Edit::CreatePlaylist { path: "Friday".into(), folder: false };
    edit_drive(&m, 0, friday());
    let made = fingerprint(main);
    rate(&m, 0, library(main).tracks[0].rating % 5 + 1);
    edit_drive(&m, 1, friday());
    assert_eq!(fingerprint(backup), made, "Backup holds what Main held after its playlist");
    let pair = api::ComparePair { left: path(main), right: path(backup) };
    assert_eq!(api::compare(&pair, &Ctx::silent()).unwrap().relation, Relation::LeftNewer);
    let all = common::sync_many(&m.roots.iter().map(PathBuf::as_path).collect::<Vec<_>>());
    assert_eq!(api::sync_many_plan(&all, &Ctx::silent()).unwrap().source, *main);
}

/// The fourth: Main gets a playlist, is synced onto Backup, then rates a track and is pulled.
/// Backup's state was never saved, but the library's latest went through it: Backup is behind,
/// so its pull is refused as older, and the push it offers brings it up to date.
#[test]
fn a_drive_the_librarys_latest_went_through_is_behind_it_unsaved() {
    let m = SyncMachine::new();
    let (main, backup) = (&m.roots[0], &m.roots[1]);
    edit_drive(&m, 0, Edit::CreatePlaylist { path: "Friday".into(), folder: false });
    api::sync_apply(Some(&m.store), &common::sync_pair(main, backup), &Ctx::silent()).unwrap();
    rate(&m, 0, library(main).tracks[0].rating % 5 + 1);
    pull_home(&m, 0, true).unwrap();
    let e = pull_home(&m, 1, false).unwrap_err();
    assert_eq!(e.reason(), Some(Reason::OlderThanStore), "{e}");
    assert!(!e.to_string().contains("snapshot"), "names no snapshot: {e}");
    let detail = e.detail();
    assert!(!detail.fields.contains_key("snapshot"), "{:?}", detail.fields);
    let push = detail.actions.iter().find(|a| a.method.as_deref() == Some("push.plan")).expect("offers a push");
    let req: api::PushRequest = serde_json::from_value(Value::Object(push.params.clone())).unwrap();
    api::push_apply(&m.store, &req, &Ctx::silent()).unwrap();
    assert_eq!(fingerprint(backup), fingerprint(main), "Backup holds the library's latest");
}

// ---------------------------------------------------------------------------------------------
// Finding drives
// ---------------------------------------------------------------------------------------------

/// A mounted folder as the model knows it.
struct Mounted {
    root: PathBuf,
    volume: String,
    name: Option<String>,
    export: bool,
}

/// A request naming a drive is taken as its path (or a folder inside its export), its name or
/// its volume's in any case, or its id's first characters: one drive so named is the one
/// meant, several are refused as ambiguous, none isn't found. Drives are listed as mounted
/// (blank volumes only when asked), described as they are, and adopted under the name asked
/// for, which then finds them.
#[hegel::test(test_cases = 10)]
fn drives_are_found_by_path_name_and_id(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let vols = common::isolate(dir.path(), None);
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    let pdb = demo_pdb();
    let volumes: Vec<&str> =
        tc.draw(gs::vecs(gs::sampled_from(&["USB", "Stick", "Gig", "CLUB"][..])).min_size(1).max_size(4).unique(true));
    let mut mounted = Vec::new();
    for v in volumes {
        let root = vols.join(v);
        fs::create_dir(&root).unwrap();
        let export = tc.draw(gs::booleans());
        if export {
            fs::create_dir_all(root.join("PIONEER/rekordbox")).unwrap();
            fs::write(root.join(paths::EXPORT_PDB), &pdb).unwrap();
        }
        let name = tc.draw(gs::optional(gs::sampled_from(&["Main", "main", "Club"][..]))).map(String::from);
        if let Some(n) = &name {
            Device::open(&root).unwrap().adopt(Some(n)).unwrap();
        }
        let root = Device::open(&root).unwrap().root().to_path_buf();
        mounted.push(Mounted { root, volume: v.to_string(), name, export });
    }

    let listed = |all: bool| -> BTreeSet<PathBuf> {
        api::list_drives(&api::ListDrives { all }).into_iter().map(|d| d.root).collect()
    };
    assert_eq!(listed(true), mounted.iter().map(|m| m.root.clone()).collect());
    let ours: BTreeSet<PathBuf> =
        mounted.iter().filter(|m| m.export || m.name.is_some()).map(|m| m.root.clone()).collect();
    assert_eq!(listed(false), ours);
    let (targets, left) = api::unnamed_targets(None);
    let exports: BTreeSet<PathBuf> = mounted.iter().filter(|m| m.export).map(|m| m.root.clone()).collect();
    assert_eq!(targets.iter().map(|d| d.root().to_path_buf()).collect::<BTreeSet<_>>(), exports);
    assert!(left.is_empty() && api::left_out(&left).is_none());

    for _ in 0..tc.draw(gs::integers::<usize>().min_value(1).max_value(4)) {
        let m = &mounted[tc.draw(gs::integers::<usize>().max_value(mounted.len() - 1))];
        let id = Device::open(&m.root).unwrap().id();
        let word = match (tc.draw(gs::integers::<u8>().max_value(4)), &m.name, id) {
            (0, _, _) => path(&m.root),
            (1, _, _) if m.export => path(&m.root.join("PIONEER")),
            (2, Some(n), _) => swap_case(&tc, n),
            (3, _, Some(id)) => {
                id.to_string()[..tc.draw(gs::integers::<usize>().min_value(4).max_value(12))].to_string()
            }
            (4, _, _) => "Nowhere".to_string(),
            _ => swap_case(&tc, &m.volume),
        };
        // What the word names, as the model takes it.
        let meant: Vec<&Mounted> = if word.contains(std::path::is_separator) {
            vec![m]
        } else {
            let lower = word.to_lowercase();
            mounted
                .iter()
                .filter(|o| {
                    let name = o.name.clone().unwrap_or_else(|| o.volume.clone());
                    let id = Device::open(&o.root).unwrap().id().map(|i| i.to_string()).unwrap_or_default();
                    name.to_lowercase() == lower
                        || o.volume.to_lowercase() == lower
                        || (lower.len() >= 4 && id.starts_with(&lower))
                })
                .collect()
        };
        for found in [api::resolve_drive(&word), api::resolve_one_drive(&word)] {
            match meant.len() {
                1 => assert_eq!(found.unwrap().root(), meant[0].root, "{word:?}"),
                0 => assert!(
                    matches!(found, Err(Error::NotFound(_))),
                    "{word:?}: {:?}",
                    found.map(|d| d.root().to_path_buf())
                ),
                n => {
                    let e = found.map(|d| d.root().to_path_buf()).unwrap_err();
                    assert!(e.to_string().contains(&format!("matches {n} drives")), "{word:?}: {e}");
                }
            }
        }
        if meant.len() != 1 {
            continue;
        }
        let root = meant[0].root.clone();
        assert!(api::library_clones(&word).unwrap().is_none(), "{word:?} names a drive, not a library");
        let twice = vec![word.clone(), word.clone()];
        assert_eq!(api::expand_libraries(&twice).unwrap(), (twice.clone(), vec![]), "left for the command to refuse");
        let info = api::drive_info(&api::DriveRef { drive: word.clone() }).unwrap();
        assert_described(&info);
        let d = Device::open(&root).unwrap();
        assert_eq!((info.root.as_path(), info.id, info.has_export), (d.root(), d.id(), d.has_export()));
        assert_eq!(info.name.unwrap_or_else(|| info.volume_name.clone()), d.name());

        // Adopting, under a name or the one it has.
        let name = tc.draw(gs::optional(gs::sampled_from(&["Home", "Main"][..]))).map(String::from);
        let req = api::AdoptDrive { drive: word.clone(), name: name.clone() };
        let plan = api::adopt_plan(&req).unwrap();
        assert_described(&plan);
        let after = name.clone().unwrap_or_else(|| d.name());
        assert_eq!((plan.plan.id, plan.plan.name.as_str()), (d.id(), after.as_str()));
        assert_eq!(plan.noop, d.id().is_some() && after == d.name());
        let adopted = match tc.draw(gs::booleans()) {
            true => plan.apply(Some(&store), &Ctx::silent()).unwrap(),
            false => api::adopt_drive(Some(&store), &req).unwrap(),
        };
        let id = adopted.id.unwrap();
        assert!(d.id().is_none_or(|was| was == id), "adopting keeps an identity");
        assert_eq!(adopted.name.as_deref(), Some(after.as_str()));
        assert_eq!(store.drives().unwrap()[&id].name, after, "registered in the store");
        mounted.iter_mut().find(|o| o.root == root).unwrap().name = Some(after);
    }
    locallibrary::testing::volumes_on_this_thread(None);
}

/// `word` with each letter's case drawn.
fn swap_case(tc: &TestCase, word: &str) -> String {
    word.chars()
        .map(|c| match tc.draw(gs::booleans()) {
            true => c.to_ascii_uppercase(),
            false => c.to_ascii_lowercase(),
        })
        .collect()
}

// ---------------------------------------------------------------------------------------------
// Restoring a playlist
// ---------------------------------------------------------------------------------------------

/// The audio files of a playlist's entries, in order.
fn entries(lib: &Library, spec: &str) -> Option<Vec<String>> {
    let p = lib.find_playlist(spec)?;
    Some(lib.tracks_by_id(&p.track_ids).iter().map(|t| t.file_path.clone()).collect())
}

/// The demo drive's databases with playlists made: Sets/Friday (with a repeat), Sets/Warmup and
/// Solo (made once: an edit through the api takes a while).
fn with_playlists(dir: &Path) -> PathBuf {
    static DATABASES: OnceLock<Vec<(String, Vec<u8>)>> = OnceLock::new();
    let databases = DATABASES.get_or_init(|| {
        let tmp = tempfile::tempdir().unwrap();
        let root = common::demo_drive(tmp.path(), "USB");
        let f: Vec<String> = library(&root).tracks.iter().map(|t| t.file_path.clone()).collect();
        let fill =
            |playlist: &str, paths: Vec<String>| Edit::SetEntries { playlist: playlist.into(), paths, create: true };
        common::apply(
            &root,
            &[
                fill("Sets/Friday", vec![f[0].clone(), f[1].clone(), f[0].clone()]),
                fill("Sets/Warmup", vec![f[1].clone()]),
                fill("Solo", vec![f[0].clone()]),
            ],
        );
        paths::DATABASE_FILES
            .iter()
            .filter_map(|k| fs::read(paths::to_fs(&root, k)).ok().map(|b| (k.to_string(), b)))
            .collect()
    });
    let root = common::demo_drive(dir, "USB");
    for (key, bytes) in databases {
        fs::write(paths::to_fs(&root, key), bytes).unwrap();
    }
    root
}

/// Playlists brought back from a snapshot (named by a label, its id, or the newest that has
/// it) or a backup get the entries they had whose tracks the drive still has, in order, the
/// rest listed as missing; with none left it's refused. Restoring again changes nothing.
/// Restoring needs a playlist, one source at most, the store for a snapshot, and something
/// to restore from; what isn't there isn't found.
#[hegel::test(test_cases = 3, suppress_health_check = [HealthCheck::TooSlow])]
fn a_playlist_comes_back_with_the_entries_it_had(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let root = with_playlists(dir.path());
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    let ctx = Ctx::silent();
    let source = library(&root);
    let files: Vec<String> = source.tracks.iter().map(|t| t.file_path.clone()).collect();
    let req = |playlist: &str, snapshot: Option<&str>, backup: Option<&str>| api::PlaylistRestoreRequest {
        drive: path(&root),
        playlist: playlist.into(),
        snapshot: snapshot.map(String::from),
        backup: backup.map(String::from),
    };
    let refused = |r: api::PlaylistRestoreRequest, store: Option<&Store>| {
        api::playlist_restore_plan(store, &r, &ctx).map(|p| p.path.clone()).unwrap_err()
    };
    let some_store = tc.draw(gs::booleans()).then_some(&store);
    let blank = tc.draw(gs::sampled_from(&["", "  ", "/", "//"][..]));
    assert!(matches!(refused(req(blank, None, None), some_store), Error::Invalid(_)));
    assert!(matches!(refused(req("Solo", Some("abcd"), Some("x")), some_store), Error::Invalid(_)));
    assert!(matches!(refused(req("Solo", Some("abcd"), None), None), Error::Invalid(_)));
    assert_eq!(refused(req("Solo", None, None), None).reason(), Some(Reason::NoStore));
    assert!(matches!(refused(req("Solo", None, Some("nope")), some_store), Error::NotFound(_)));

    // Saved in the store, or not; then playlists deleted, and tracks with their audio.
    let pulled = tc.draw(gs::booleans()).then(|| {
        let req = api::PullRequest {
            drive: path(&root),
            library: Some("Home".into()),
            new_library: true,
            label: Some("before".into()),
            ..Default::default()
        };
        api::pull(&store, &req, &ctx).unwrap().snapshot.unwrap()
    });
    match &pulled {
        None => {
            let e = refused(req("Solo", None, None), Some(&store));
            assert!(e.to_string().contains("isn't linked to a library"), "{e}");
        }
        Some(_) => {
            let e = refused(req("Nope", None, None), Some(&store));
            assert!(matches!(e.inner(), Error::NotFound(m) if m.contains("any snapshot")), "{e}");
            assert_eq!(e.detail().actions[0].method.as_deref(), Some("drive.backups"));
            assert!(matches!(refused(req("Nope", Some("BEFORE"), None), Some(&store)), Error::NotFound(_)));
        }
    }
    let deleted = tc.draw(gs::sampled_from(&["Sets", "Sets/Friday", "Solo"][..])).to_string();
    let mut gone = vec![Edit::DeletePlaylist { playlist: deleted, with_tracks: false }];
    let lost: Vec<String> = files.iter().filter(|_| tc.draw(gs::weighted_booleans(0.3))).cloned().collect();
    if !lost.is_empty() {
        let ids = source.tracks.iter().filter(|t| lost.contains(&t.file_path)).map(|t| t.id).collect();
        gone.push(Edit::DeleteTracks { tracks: TrackSelector { ids, ..Default::default() }, keep_files: false });
    }
    api::edit_apply(&api::EditRequest { drive: path(&root), edits: gone, options: EditOptions::default() }, &ctx)
        .unwrap();
    let backup = backup::list(&Device::open(&root).unwrap()).unwrap().remove(0);

    let spec = tc.draw(gs::sampled_from(&["Sets/Friday", "Friday", "Sets", "Solo", "/Solo/"][..]));
    let label;
    let (snapshot, from_backup) = match (&pulled, tc.draw(gs::integers::<u8>().max_value(3))) {
        (Some(_), 0) => (Some("BEFORE"), false),
        (Some(s), 1) => {
            label = format!("snapshot:{}", s.short_id);
            (Some(label.as_str()), false)
        }
        (Some(_), 2) => (None, false),
        _ => (None, true),
    };
    let req = req(spec, snapshot, from_backup.then_some(backup.id.as_str()));
    // The model: each playlist of it, with its entries the drive still has.
    let node = source.find_playlist(spec.trim_matches('/')).unwrap();
    let under = source.playlist_path(node.id);
    let inside: Vec<String> = source
        .playlists
        .iter()
        .filter(|p| !p.is_folder && source.playlist_path(p.id).starts_with(&under))
        .map(|p| source.playlist_path(p.id))
        .collect();
    let want: Vec<(String, Vec<String>)> = inside
        .iter()
        .map(|p| (p.clone(), entries(&source, p).unwrap().into_iter().filter(|f| !lost.contains(f)).collect()))
        .collect();
    let kept: usize = want.iter().map(|(_, e)| e.len()).sum();
    let missing: usize = inside.iter().map(|p| entries(&source, p).unwrap().len()).sum::<usize>() - kept;

    let planned = api::playlist_restore_plan(Some(&store), &req, &ctx);
    if kept == 0 && missing > 0 {
        let e = planned.unwrap_err();
        assert!(matches!(e, Error::NotFound(_)), "{e}");
        return;
    }
    let planned = planned.unwrap();
    assert_described(&planned);
    assert_eq!(planned.path, under);
    assert_eq!(
        (planned.folder, planned.playlists, planned.entries, planned.missing.len()),
        (node.is_folder, inside.len(), kept, missing)
    );
    match (&planned.source, &pulled) {
        (api::RestoreSource::Snapshot { snapshot, library }, Some(s)) => {
            assert_eq!((snapshot.id, library.as_deref()), (s.id, Some("Home")));
            assert_eq!(snapshot.labels, ["before"]);
        }
        (api::RestoreSource::Backup { backup: b }, _) => assert_eq!(b.id, backup.id),
        (s, _) => panic!("restored from {s:?}"),
    }
    let report = match tc.draw(gs::booleans()) {
        true => planned.apply(Some(&store), &ctx).unwrap(),
        false => api::playlist_restore_apply(Some(&store), &req, &ctx).unwrap(),
    };
    assert!(report.backup.is_some() || planned.noop);
    let now = library(&root);
    for (p, files) in &want {
        assert_eq!(entries(&now, p).as_ref(), Some(files), "{p}");
    }
    assert!(api::playlist_restore_plan(Some(&store), &req, &ctx).unwrap().noop, "restored again");
}

// ---------------------------------------------------------------------------------------------
// Settings
// ---------------------------------------------------------------------------------------------

/// The profile's values set through the api, for every drive or one, are what each drive's
/// settings are checked against: what's shown as drift is what differs, writing the profile
/// leaves none (planned first, and a no-op once written), and a dry run writes nothing.
/// Importing a drive's settings makes them the base. Unknown settings are refused.
#[hegel::test(test_cases = 4, suppress_health_check = [HealthCheck::TooSlow])]
fn the_settings_profile_is_what_drives_are_checked_against(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let vols = common::isolate(dir.path(), None);
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    let root = Device::open(common::demo_drive(&vols, "USB")).unwrap().root().to_path_buf();
    let drive = api::DriveRef { drive: path(&root) };
    let current = || settings::read_all(&DriveSettings(&root));

    let per_drive = tc.draw(gs::booleans());
    let mut values = IndexMap::new();
    for _ in 0..tc.draw(gs::integers::<usize>().min_value(1).max_value(4)) {
        let def = &SCHEMA[tc.draw(gs::integers::<usize>().max_value(SCHEMA.len() - 1))];
        let choice = &def.choices[tc.draw(gs::integers::<usize>().max_value(def.choices.len() - 1))];
        values.insert(def.key.to_string(), choice.id.to_string());
    }
    let req = api::SetSettings { values: values.clone(), drive: per_drive.then(|| path(&root)) };
    let profile = api::settings_set(&store, &req).unwrap();
    let id = Device::open(&root).unwrap().id();
    assert_eq!(id.is_some(), per_drive, "a per-drive value adopts the drive");
    let effective = profile.effective(id);
    assert!(values.iter().all(|(k, v)| effective.get(k) == Some(v)), "{effective:?}");
    let bad = api::SetSettings { values: [("no such setting".to_string(), "on".to_string())].into(), drive: None };
    let e = api::settings_set(&store, &bad).unwrap_err();
    assert_eq!((e.code(), e.reason()), ("invalid", Some(Reason::UnknownSetting)), "{e}");
    assert_eq!(store.settings_profile().unwrap().effective(id), effective, "a refused set changes nothing");

    let shown = api::settings_show(Some(&store), &drive).unwrap();
    assert_described(&shown);
    let differs: BTreeSet<String> = values
        .iter()
        .filter(|(k, v)| !matches!(current().values.get(*k), Some(SettingValue::Known(now)) if now == *v))
        .map(|(k, _)| k.clone())
        .collect();
    assert_eq!(shown.drift.iter().map(|d| d.key.clone()).collect::<BTreeSet<_>>(), differs);
    assert!(shown.drift.iter().all(|d| d.overridden == per_drive));

    let dry =
        api::settings_apply(&store, &api::ApplySettings { drive: path(&root), dry_run: true, create_missing: false });
    let dry = dry.unwrap();
    assert!(dry.dry_run && dry.files_written.is_empty() == differs.is_empty());
    assert_eq!(api::settings_show(Some(&store), &drive).unwrap().drift.len(), differs.len(), "a dry run wrote nothing");

    let named = tc.draw(gs::booleans());
    let write =
        api::SettingsApplyRequest { drives: if named { vec![path(&root)] } else { vec![] }, create_missing: false };
    let planned = api::settings_apply_plan(&store, &write).unwrap();
    assert_described(&planned);
    assert_eq!(planned.noop, differs.is_empty(), "{:?}", planned.summary);
    assert!(planned.apply(None, &Ctx::silent()).is_err(), "the profile is the store's");
    let results = planned.apply(Some(&store), &Ctx::silent()).unwrap();
    assert!(results.iter().all(|r| r.error.is_none()), "{results:?}");
    assert_eq!(results.len(), usize::from(!differs.is_empty()), "only drives with changes are written");
    assert!(api::settings_show(Some(&store), &drive).unwrap().drift.is_empty(), "written, nothing drifts");
    assert!(api::settings_apply_plan(&store, &write).unwrap().noop, "written again");

    // A value changed again, written in one go.
    let def = &SCHEMA[tc.draw(gs::integers::<usize>().max_value(SCHEMA.len() - 1))];
    let choice = def.choices[tc.draw(gs::integers::<usize>().max_value(def.choices.len() - 1))].id;
    let again = api::SetSettings { values: [(def.key.to_string(), choice.to_string())].into(), drive: None };
    api::settings_set(&store, &again).unwrap();
    let apply = api::ApplySettings { drive: path(&root), dry_run: false, create_missing: false };
    let report = api::settings_apply(&store, &apply).unwrap();
    assert!(!report.dry_run);
    assert!(api::settings_show(Some(&store), &drive).unwrap().drift.is_empty(), "written in one go");

    // Taking values off leaves the drive's as they are.
    let keys: Vec<String> = values.keys().cloned().collect();
    let unset = api::UnsetSettings { keys: keys.clone(), drive: per_drive.then(|| path(&root)) };
    let profile = api::settings_unset(&store, &unset).unwrap();
    // Taking a drive's own values off leaves what the base holds: the value set above for all.
    let effective = profile.effective(id);
    for k in &keys {
        match effective.get(k) {
            Some(v) => assert!(per_drive && k == def.key && v == choice, "{k} = {v:?} is left"),
            None => assert!(!(per_drive && k == def.key), "{k} lost the base's value"),
        }
    }

    // Importing the drive's settings makes them the base.
    let import = api::ImportSettings { drive: Some(path(&root)) };
    let planned = api::settings_import_plan(&store, &import).unwrap();
    assert_described(&planned);
    let known: IndexMap<String, String> = current()
        .values
        .into_iter()
        .filter_map(|(k, v)| match v {
            SettingValue::Known(id) => Some((k, id)),
            _ => None,
        })
        .collect();
    let base = store.settings_profile().unwrap().base;
    let changing: BTreeSet<&String> = known.iter().filter(|(k, v)| base.get(*k) != Some(*v)).map(|(k, _)| k).collect();
    assert_eq!(planned.plan.changes.iter().map(|c| &c.key).collect::<BTreeSet<_>>(), changing);
    let profile = planned.apply(Some(&store), &Ctx::silent()).unwrap();
    assert!(known.iter().all(|(k, v)| profile.base.get(k) == Some(v)));
    assert!(api::settings_import_plan(&store, &import).unwrap().noop, "imported again");

    // A drive that isn't mounted is found in the store's registry by name, for overrides (an
    // override for a mounted one adopted it and registered it).
    let here = api::SetSettings { values: values.clone(), drive: Some(path(&root)) };
    api::settings_set(&store, &here).unwrap();
    let id = Device::open(&root).unwrap().id();
    fs::rename(&root, dir.path().join("away")).unwrap();
    let away = api::SetSettings { values: values.clone(), drive: Some("usb".into()) };
    let profile = api::settings_set(&store, &away).unwrap();
    assert!(values.iter().all(|(k, v)| profile.drives[&id.unwrap()].values.get(k) == Some(v)));
    let unknown = api::SetSettings { values, drive: Some("Nowhere".into()) };
    assert!(matches!(api::settings_set(&store, &unknown), Err(Error::NotFound(_))));
    locallibrary::testing::volumes_on_this_thread(None);
}

// ---------------------------------------------------------------------------------------------
// Reads and plans agree with the modules they wrap
// ---------------------------------------------------------------------------------------------

/// A plan or report as JSON, without the free space on the disk: other programs write to it
/// between two plans.
fn json_of<T: Serialize>(v: &T) -> Value {
    fn strip(v: &mut Value) {
        match v {
            Value::Object(m) => {
                m.remove("free_space");
                m.values_mut().for_each(strip);
            }
            Value::Array(a) => a.iter_mut().for_each(strip),
            _ => {}
        }
    }
    let mut v = serde_json::to_value(v).unwrap();
    strip(&mut v);
    v
}

/// A drive named by its path, adopted name in any case, or id's first characters, reads and
/// plans through the api as the modules do for it directly: its library and backups, compare,
/// clean, convert, merge, missing and automatic artwork, its tracks' art and an XML export
/// (which won't replace a file unless asked). Applying a clean or a merge leaves nothing more
/// to do; a conversion leaves the drive with the database it was for.
#[hegel::test(test_cases = 2, suppress_health_check = [HealthCheck::TooSlow])]
fn reads_and_plans_agree_with_the_modules(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let vols = common::isolate(dir.path(), None);
    let ctx = Ctx::silent();
    let root = Device::open(common::demo_drive(&vols, "USB")).unwrap().root().to_path_buf();
    let other = vols.join("Other");
    common::copy_dir(&root, &other);
    common::export_new_track(&other, "Guest", "Only Here");
    let mut d = Device::open(&root).unwrap();
    d.adopt(Some("Home")).unwrap();
    let spec = match tc.draw(gs::integers::<u8>().max_value(2)) {
        0 => path(&root),
        1 => swap_case(&tc, "home"),
        _ => d.id().unwrap().to_string()[..8].to_string(),
    };
    let drive = api::DriveRef { drive: spec.clone() };
    let lib = d.library().unwrap();

    let view = api::drive_library(&drive).unwrap();
    assert_eq!((view.library.clone(), view.summary.clone()), (lib.clone(), lib.summary()));
    let listed: Vec<String> = api::list_backups(&drive).unwrap().into_iter().map(|b| b.id).collect();
    assert_eq!(listed, backup::list(&d).unwrap().into_iter().map(|b| b.id).collect::<Vec<_>>());
    let other_dev = Device::open(&other).unwrap();
    let pair = api::ComparePair { left: spec.clone(), right: path(&other) };
    let compared = api::compare(&pair, &ctx).unwrap();
    assert_described(&compared);
    assert_eq!(json_of(&compared), json_of(&compare::compare_devices(&d, &other_dev, &ctx).unwrap()));

    let to = match tc.draw(gs::booleans()) {
        true => convert::Target::OneLibrary,
        false => convert::Target::DeviceLibrary,
    };
    let options = convert::ConvertOptions { to, replace: tc.draw(gs::booleans()) };
    let converted = api::convert_plan(&api::ConvertRequest { drive: spec.clone(), options: options.clone() }, &ctx);
    match (converted, convert::plan(&d, &options, &ctx)) {
        (Ok(p), Ok(direct)) => {
            assert_described(&p);
            assert_eq!(json_of(&p.plan), json_of(&direct));
            assert_eq!(p.destructive, direct.replaces);
        }
        (Err(a), Err(b)) => assert_eq!(a.to_string(), b.to_string()),
        (a, b) => panic!("the api and convert disagree: {:?} {:?}", a.map(|p| p.noop), b.map(|p| p.tracks)),
    }

    assert_eq!(json_of(&api::artwork_missing(&drive).unwrap()), json_of(&artwork::missing(&d, &lib)));
    let auto = api::artwork_auto_plan(&drive).unwrap();
    assert_described(&auto);
    assert_eq!(json_of(&auto.plan), json_of(&artwork::plan_auto(&d, &lib)));
    assert_eq!(auto.noop, auto.plan.found.is_empty());
    for t in &lib.tracks {
        let got = api::artwork_get(&api::GetArtwork { drive: spec.clone(), track: t.id }).unwrap();
        let bytes = got.map(|a| BASE64_STANDARD.decode(a.base64).unwrap());
        assert_eq!(bytes, artwork::read_from_drive(&d, &lib, t.id).unwrap(), "track {}", t.id);
    }

    let only: Vec<String> = match tc.draw(gs::booleans()) {
        true => lib.playlists.iter().take(1).map(|p| p.name.clone()).collect(),
        false => vec![],
    };
    let opts = xml::XmlOptions { location_root: None, skip_analysis: tc.draw(gs::booleans()), only: only.clone() };
    let output = dir.path().join("library.xml");
    let export = |replace| api::ExportXml {
        drive: spec.clone(),
        output: output.clone(),
        location_root: None,
        skip_analysis: opts.skip_analysis,
        only: only.clone(),
        replace,
    };
    let report = api::export_xml(&export(false), &ctx).unwrap();
    let (text, direct) = xml::export(&d, &opts, &ctx).unwrap();
    assert_eq!(fs::read_to_string(&output).unwrap(), text);
    assert_eq!(json_of(&report), json_of(&direct));
    assert!(api::export_xml(&export(false), &ctx).is_err(), "an XML export replaced a file unasked");
    api::export_xml(&export(true), &ctx).unwrap();

    let merge = api::MergeRequest { source: path(&other), target: spec.clone(), options: Default::default() };
    let merged = api::merge_plan(&merge, &ctx).unwrap();
    assert_described(&merged);
    assert_eq!(json_of(&merged.plan), json_of(&merge::plan(&other_dev, &d, &Default::default(), &ctx).unwrap()));
    assert_eq!(merged.plan.new_tracks.len(), 1, "the guest's track");
    let cleaned = api::clean_plan(&drive, &ctx).unwrap();
    assert_described(&cleaned);
    assert_eq!(json_of(&cleaned.plan), json_of(&clean::plan(&d, &ctx).unwrap()));
    assert_eq!((cleaned.noop, cleaned.destructive), (cleaned.plan.is_noop(), !cleaned.plan.is_noop()));
    // Each applied, through the plan or in one go: then there's nothing more to do.
    match tc.draw(gs::booleans()) {
        true => merged.apply(None, &ctx).map(|_| ()).unwrap(),
        false => api::merge_apply(&merge, &ctx).map(|_| ()).unwrap(),
    }
    assert!(api::merge_plan(&merge, &ctx).unwrap().plan.new_tracks.is_empty(), "merged again");
    let req = api::ConvertRequest { drive: spec.clone(), options: options.clone() };
    if let Some(plan) = api::convert_plan(&req, &ctx).ok().filter(|p| !p.noop) {
        match tc.draw(gs::booleans()) {
            true => plan.apply(None, &ctx).map(|_| ()).unwrap(),
            false => api::convert_apply(&req, &ctx).map(|_| ()).unwrap(),
        }
        let d = Device::open(&root).unwrap();
        let has = match options.to {
            convert::Target::OneLibrary => d.has_library_plus(),
            convert::Target::DeviceLibrary => d.has_device_library(),
        };
        assert!(has, "converted, the drive has the database");
    }
    let cleaned = api::clean_plan(&drive, &ctx).unwrap();
    if !cleaned.noop {
        match tc.draw(gs::booleans()) {
            true => cleaned.apply(None, &ctx).map(|_| ()).unwrap(),
            false => api::clean_apply(&drive, &ctx).map(|_| ()).unwrap(),
        }
        assert!(api::clean_plan(&drive, &ctx).unwrap().noop, "cleaned again");
    }
    locallibrary::testing::volumes_on_this_thread(None);
}

// ---------------------------------------------------------------------------------------------
// Artwork
// ---------------------------------------------------------------------------------------------

/// Art set on a drive's tracks is planned for exactly those, at the image's size (one under 16
/// pixels either way is refused), and read back
/// as a JPEG for each of them, which are then no longer missing art. Art embedded in files on
/// this computer goes in only those it can (not missing ones, folders or files on an export),
/// and taking it back out gives each unchanged file its bytes back.
#[hegel::test(test_cases = 3, suppress_health_check = [HealthCheck::TooSlow])]
fn art_set_on_a_drive_reads_back_and_embedded_art_comes_back_out(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let root = common::demo_drive(dir.path(), "USB");
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    let ctx = Ctx::silent();
    let lib = library(&root);
    let ids: Vec<u32> = lib.tracks.iter().map(|t| t.id).filter(|_| tc.draw(gs::booleans())).collect();
    let set = |image: &Path, ids: &[u32]| api::SetArtwork {
        drive: path(&root),
        image: image.to_path_buf(),
        tracks: TrackSelector { ids: ids.to_vec(), ..Default::default() },
    };
    // An image smaller than 16 pixels either way is refused, for drives and files alike.
    let small = dir.path().join("small.png");
    let (w, h) = (
        tc.draw(gs::integers::<u32>().min_value(1).max_value(15)),
        tc.draw(gs::integers::<u32>().min_value(1).max_value(48)),
    );
    fs::write(&small, common::media::png(w, h, 0)).unwrap();
    let refused = api::artwork_set_plan(&set(&small, &[lib.tracks[0].id]), &ctx).map(|p| p.noop).unwrap_err();
    assert_eq!(refused.code(), "image", "{refused}");
    let embed = api::EmbedArtwork { files: vec![], image: small };
    assert_eq!(api::artwork_embed_plan(&embed).map(|p| p.noop).unwrap_err().code(), "image");

    let side = || gs::integers::<u32>().min_value(16).max_value(48);
    let (w, h) = (tc.draw(side()), tc.draw(side()));
    let image = dir.path().join("cover.png");
    fs::write(&image, common::media::png(w, h, tc.draw(gs::integers::<u8>()))).unwrap();
    let set = set(&image, &ids);
    match api::artwork_set_plan(&set, &ctx) {
        Err(e) => assert!(ids.is_empty() && matches!(e, Error::Invalid(_)), "selecting no tracks: {e}"),
        Ok(plan) => {
            assert_described(&plan);
            assert_eq!(plan.changes, [format!("set the artwork of {} track(s)", ids.len())]);
            let files: Vec<String> = lib.tracks_by_id(&ids).into_iter().map(|t| t.file_path.clone()).collect();
            assert!(
                matches!(&plan.recorded[..], [Edit::SetArtwork { tracks, .. }] if tracks.paths == files),
                "{:?}",
                plan.recorded
            );
            let report = match tc.draw(gs::booleans()) {
                true => plan.apply(None, &ctx).unwrap(),
                false => api::artwork_set(&set, &ctx).unwrap(),
            };
            assert_eq!(report.changes, 1);
            for id in &ids {
                let art = api::artwork_get(&api::GetArtwork { drive: path(&root), track: *id }).unwrap().unwrap();
                let jpeg = BASE64_STANDARD.decode(&art.base64).unwrap();
                assert_eq!((art.mime, &jpeg[..2]), ("image/jpeg", &[0xFF, 0xD8][..]));
            }
            let missing: BTreeSet<u32> = api::artwork_missing(&api::DriveRef { drive: path(&root) })
                .unwrap()
                .into_iter()
                .map(|t| t.id)
                .collect();
            assert!(ids.iter().all(|id| !missing.contains(id)), "{missing:?}");
        }
    }
    // Nothing next to the audio or in it: finding art automatically finds none.
    assert_eq!(api::artwork_auto_apply(&api::DriveRef { drive: path(&root) }, &ctx).unwrap().tracks_updated, 0);

    let music = dir.path().join("music");
    fs::create_dir(&music).unwrap();
    let mut files = Vec::new();
    let mut original = IndexMap::new();
    // One file to embed in at least, and maybe others it can't.
    let kinds: Vec<u8> = [vec![3], tc.draw(gs::vecs(gs::integers::<u8>().max_value(4)).max_size(3))].concat();
    for (i, kind) in kinds.into_iter().enumerate() {
        let f = music.join(format!("{i}.wav"));
        match kind {
            0 => {}
            1 => fs::create_dir(&f).unwrap(),
            2 => {
                files.push(paths::to_fs(&root, &paths::key(&lib.tracks[0].file_path)));
                continue;
            }
            _ => {
                fs::write(&f, common::media::wav()).unwrap();
                original.insert(f.clone(), common::media::wav());
            }
        }
        files.push(f);
    }
    let req = api::EmbedArtwork { files: files.clone(), image };
    let plan = api::artwork_embed_plan(&req).unwrap();
    assert_described(&plan);
    let planned: Vec<&PathBuf> = plan.files.iter().map(|f| &f.path).collect();
    assert_eq!(planned, original.keys().collect::<Vec<_>>());
    assert_eq!(plan.skipped.len(), files.len() - original.len());
    assert_eq!(plan.backup_bytes, original.values().map(|b| b.len() as u64).sum::<u64>());
    assert_eq!(plan.noop, original.is_empty());
    assert!(plan.apply(None, &ctx).is_err(), "the copies go in the store");
    let report = match tc.draw(gs::booleans()) {
        true => plan.apply(Some(&store), &ctx).unwrap(),
        false => api::artwork_embed(&store, &req).unwrap(),
    };
    assert_eq!(report.embedded.iter().collect::<Vec<_>>(), original.keys().collect::<Vec<_>>());
    let unknown = api::UnembedRequest { backup: Some("nope".into()) };
    assert!(api::artwork_unembed_plan(&store, &unknown).is_err_and(|e| matches!(e, Error::NotFound(_))));
    // One changes again after the art went in: it's left as it is.
    let changed = original.keys().next().cloned().filter(|_| tc.draw(gs::booleans()));
    if let Some(f) = &changed {
        fs::write(f, b"rewritten since").unwrap();
    }
    let undo = api::UnembedRequest { backup: report.backup.clone() };
    let plan = api::artwork_unembed_plan(&store, &undo).unwrap();
    assert_described(&plan);
    assert_eq!(plan.skipped.len(), usize::from(changed.is_some()));
    let r = plan.apply(Some(&store), &ctx).unwrap();
    for (f, bytes) in &original {
        match Some(f) == changed.as_ref() {
            true => assert_eq!(fs::read(f).unwrap(), b"rewritten since"),
            false => assert_eq!(&fs::read(f).unwrap(), bytes, "{} came back", f.display()),
        }
    }
    assert_eq!(r.restored.len(), original.len() - usize::from(changed.is_some()));
}

// ---------------------------------------------------------------------------------------------
// Requests
// ---------------------------------------------------------------------------------------------

/// A string for a request's field: a UUID or a snapshot's hash where the schema asks for one.
fn request_string(tc: &TestCase, _key: &str, s: &serde_json::Map<String, Value>) -> String {
    match (s.get("format").and_then(Value::as_str), s.get("pattern")) {
        (Some("uuid"), _) => Uuid::from_u128(tc.draw(gs::integers::<u128>().max_value(3))).to_string(),
        (_, Some(_)) => Hash::of(&[tc.draw(gs::integers::<u8>())]).to_hex(),
        _ => tc.draw(gs::sampled_from(&["", "Main", "Sets/Friday", "./x"][..])).to_string(),
    }
}

/// A request is taken exactly when its JSON Schema (as the RPC contract gives a request's, for
/// deserialising) says it's one: what it requires, and of the right types.
fn request_agrees<T: serde::de::DeserializeOwned + JsonSchema>(tc: &TestCase) {
    let root = SchemaSettings::draft2020_12().for_deserialize().into_generator().into_root_schema_for::<T>().to_value();
    let value = schema::draw(tc, &root, &root, "", 0, &request_string);
    let described = schema::violations(&validator_for::<T>(false), &value);
    let taken = serde_json::from_value::<T>(value.clone());
    assert_eq!(
        described.is_none(),
        taken.is_ok(),
        "{}: the schema says {described:?}, serde {:?}\n{value:#}",
        std::any::type_name::<T>(),
        taken.err().map(|e| e.to_string())
    );
}

/// The service layer's requests, as front ends send them, agree with their schemas.
#[hegel::test(test_cases = 300)]
fn requests_are_taken_as_their_schemas_describe(tc: TestCase) {
    let checks: &[fn(&TestCase)] = &[
        request_agrees::<api::SyncRequest>,
        request_agrees::<api::SyncManyRequest>,
        request_agrees::<api::ComparePair>,
        request_agrees::<api::PullRequest>,
        request_agrees::<api::PushRequest>,
        request_agrees::<api::HistoryRequest>,
        request_agrees::<api::UndoRequest>,
        request_agrees::<api::UndoListRequest>,
        request_agrees::<api::RestoreBackup>,
        request_agrees::<LabelRequest>,
        request_agrees::<RenameLibrary>,
        request_agrees::<api::PlaylistRestoreRequest>,
        request_agrees::<PruneOptions>,
        request_agrees::<api::StoreCopyRequest>,
        request_agrees::<api::MergeRequest>,
        request_agrees::<api::ConvertRequest>,
        request_agrees::<api::SetSettings>,
        request_agrees::<api::UnsetSettings>,
        request_agrees::<api::ApplySettings>,
        request_agrees::<api::SettingsApplyRequest>,
        request_agrees::<api::ImportSettings>,
        request_agrees::<api::AdoptDrive>,
        request_agrees::<api::ListDrives>,
        request_agrees::<api::DriveRef>,
        request_agrees::<api::ExportXml>,
        request_agrees::<api::SetArtwork>,
        request_agrees::<api::GetArtwork>,
        request_agrees::<api::EmbedArtwork>,
        request_agrees::<api::UnembedRequest>,
        request_agrees::<api::FormatRequest>,
        request_agrees::<api::FormatApply>,
    ];
    checks[tc.draw(gs::integers::<usize>().max_value(checks.len() - 1))](&tc);
    // A sync's direction goes out and comes back as itself.
    let d = match tc.draw(gs::integers::<u8>().max_value(2)) {
        0 => Direction::Auto,
        1 => Direction::LeftToRight,
        _ => Direction::RightToLeft,
    };
    assert_eq!(serde_json::from_value::<Direction>(serde_json::to_value(d).unwrap()).unwrap(), d);
}
