//! The store: a local vault of library snapshots.
//!
//! ```text
//! <store>/
//!   store.json            format marker + store id
//!   objects/ab/cdef...    file contents, addressed by BLAKE3 hash (deduplicated across
//!                         snapshots, libraries and drives)
//!   snapshots/<id>.json   one manifest per snapshot: every file's path/size/mtime/hash, the
//!                         parent snapshot(s), library summary and drive lineage
//!   libraries.json        named libraries and their head snapshot
//!   labels.json           names given to snapshots (`pull --label`)
//!   pruned.json           what's left of snapshots `store prune` removed (their parents), so
//!                         history still walks through them
//!   drives.json           drives seen so far (id -> name, last mount point, library)
//!   drives.lock           held while drives.json is read and written back (see
//!                         Store::update_drives)
//!   settings.json         the settings profile (see crate::settings)
//!   players.json          the player models `check` uses when none are given (see
//!                         crate::equipment::Remembered)
//!   lock                  advisory lock so a CLI and a GUI can't write concurrently
//! ```
//!
//! Snapshots form a DAG through `parents`, so "is X newer than Y" can be answered from
//! history, and any previous state of a library can be restored onto a drive.

use indexmap::IndexMap;
use std::collections::VecDeque;

use rustc_hash::FxHashSet;
use std::fs;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::device::LineageEntry;
use crate::error::IoContext;
use crate::error::{Action, Reason};
use crate::fsutil;
use crate::hash::{self, Hash};
use crate::model::LibrarySummary;
use crate::progress::{Ctx, Event, Phase};
use crate::settings::SettingsProfile;
use crate::tree::FileEntry;
use crate::{Error, Result};

pub const STORE_FORMAT: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
struct StoreMeta {
    format: u32,
    id: Uuid,
    created: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct LibraryRecord {
    pub id: Uuid,
    pub name: String,
    pub head: Option<Hash>,
    pub created: DateTime<Utc>,
    pub updated: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct DriveRecord {
    pub id: Uuid,
    pub name: String,
    #[serde(default)]
    pub library_id: Option<Uuid>,
    pub last_mount: PathBuf,
    pub last_seen: DateTime<Utc>,
    #[serde(default)]
    pub last_fingerprint: Option<Hash>,
    /// What the drive is for (see [`crate::fleet`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<crate::fleet::Role>,
    /// The disk it was last seen on: hints for spotting copies and drives erased elsewhere.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hardware: Option<crate::fleet::Hardware>,
    /// Size of its volume when last seen.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capacity: Option<u64>,
    #[serde(default)]
    pub log: crate::fleet::DriveLog,
}

impl DriveRecord {
    pub fn new(id: Uuid, name: impl Into<String>, last_mount: PathBuf) -> Self {
        DriveRecord {
            id,
            name: name.into(),
            library_id: None,
            last_mount,
            last_seen: Utc::now(),
            last_fingerprint: None,
            role: None,
            hardware: None,
            capacity: None,
            log: Default::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SnapshotSource {
    pub drive_id: Option<Uuid>,
    pub drive_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Snapshot {
    pub id: Hash,
    pub library_id: Uuid,
    pub parents: Vec<Hash>,
    pub created: DateTime<Utc>,
    #[serde(default)]
    pub message: Option<String>,
    #[serde(default)]
    pub source: Option<SnapshotSource>,
    pub fingerprint: Hash,
    pub summary: LibrarySummary,
    /// Lineage of the drive this snapshot was taken from, ending with this state. Pushing the
    /// snapshot to a drive hands the drive this history.
    #[serde(default)]
    pub lineage: Vec<LineageEntry>,
    /// Every library file, all hashed.
    pub files: Vec<FileEntry>,
}

/// Snapshot without its file list, for listings.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct SnapshotInfo {
    pub id: Hash,
    pub short_id: String,
    pub library_id: Uuid,
    pub parents: Vec<Hash>,
    pub created: DateTime<Utc>,
    pub message: Option<String>,
    pub source: Option<SnapshotSource>,
    pub fingerprint: Hash,
    pub summary: LibrarySummary,
    pub files: usize,
    pub bytes: u64,
    /// Its labels (see [`Store::add_label`]). Empty from [`Snapshot::info`]; the store's
    /// listings fill them in.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub labels: Vec<String>,
    /// Off the line back from the library's latest: saved beside it (a drive back from a set, see
    /// [`crate::sync::pull_beside`]). False from [`Snapshot::info`]; [`crate::api::history`]
    /// fills it in.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub beside: bool,
}

/// A name given to a snapshot (`pull --label "before NYE"`). Labels are unique in a store (any
/// case), stand in for the snapshot's id wherever one is asked for, and keep the snapshot from
/// being pruned or collected.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SnapshotLabel {
    pub name: String,
    pub snapshot: Hash,
    pub created: DateTime<Utc>,
}

impl Snapshot {
    pub fn info(&self) -> SnapshotInfo {
        SnapshotHead {
            id: self.id,
            library_id: self.library_id,
            parents: self.parents.clone(),
            created: self.created,
            message: self.message.clone(),
            source: self.source.clone(),
            fingerprint: self.fingerprint,
            summary: self.summary.clone(),
            files: self.files.iter().map(|f| FileSize { size: f.size }).collect(),
        }
        .info()
    }

    fn compute_id(&self) -> Hash {
        #[derive(Serialize)]
        struct Content<'a> {
            library_id: &'a Uuid,
            parents: &'a [Hash],
            created: &'a DateTime<Utc>,
            message: &'a Option<String>,
            fingerprint: &'a Hash,
            files: &'a [FileEntry],
        }
        let bytes = serde_json::to_vec(&Content {
            library_id: &self.library_id,
            parents: &self.parents,
            created: &self.created,
            message: &self.message,
            fingerprint: &self.fingerprint,
            files: &self.files,
        })
        .expect("snapshot serialises");
        Hash::of(&bytes)
    }
}

/// A manifest as [`Store::load_info`] reads it: each file's size, and nothing else of the file
/// list. [`Snapshot::info`] goes through it too, so a [`SnapshotInfo`] is made in one place.
#[derive(Deserialize)]
struct SnapshotHead {
    id: Hash,
    library_id: Uuid,
    parents: Vec<Hash>,
    created: DateTime<Utc>,
    #[serde(default)]
    message: Option<String>,
    #[serde(default)]
    source: Option<SnapshotSource>,
    fingerprint: Hash,
    summary: LibrarySummary,
    files: Vec<FileSize>,
}

#[derive(Deserialize)]
struct FileSize {
    size: u64,
}

impl SnapshotHead {
    fn info(self) -> SnapshotInfo {
        SnapshotInfo {
            id: self.id,
            short_id: self.id.short(),
            library_id: self.library_id,
            parents: self.parents,
            created: self.created,
            message: self.message,
            source: self.source,
            fingerprint: self.fingerprint,
            summary: self.summary,
            files: self.files.len(),
            bytes: self.files.iter().map(|f| f.size).sum(),
            labels: Vec::new(),
            beside: false,
        }
    }
}

/// What's left of a snapshot `store prune` removed: enough to walk history through it.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct PrunedSnapshot {
    pub library_id: Uuid,
    pub parents: Vec<Hash>,
    pub created: DateTime<Utc>,
    pub fingerprint: Hash,
    /// When it was pruned.
    pub pruned: DateTime<Utc>,
}

#[derive(Debug, Clone, Default, Serialize, JsonSchema)]
pub struct StoreStats {
    pub libraries: usize,
    pub snapshots: usize,
    pub objects: usize,
    pub object_bytes: u64,
    pub drives: usize,
}

#[derive(Debug, Clone, Default, Serialize, JsonSchema)]
pub struct GcReport {
    pub removed_objects: usize,
    pub freed_bytes: u64,
    pub removed_snapshots: usize,
    pub dry_run: bool,
}

/// What collecting garbage deletes (see [`Store::garbage`]).
pub(crate) struct Garbage {
    pub snapshots: Vec<Hash>,
    /// Stored files and their sizes.
    pub objects: Vec<(Hash, u64)>,
    /// `pruned.json` without what nothing reaches, when that's less.
    pub pruned: Option<IndexMap<Hash, PrunedSnapshot>>,
}

#[derive(Debug, Clone, Default, Serialize, JsonSchema)]
pub struct VerifyReport {
    pub checked: usize,
    pub corrupt: Vec<String>,
    pub missing: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct Store {
    root: PathBuf,
}

/// Held while modifying the store. Released on drop.
pub struct StoreLock {
    _lock: fsutil::FileLock,
}

impl Store {
    /// `$LOCALLIBRARY_STORE`, else the platform data directory (e.g.
    /// `~/Library/Application Support/locallibrary` on macOS).
    pub fn default_root() -> PathBuf {
        if let Some(p) = std::env::var_os("LOCALLIBRARY_STORE") {
            return PathBuf::from(p);
        }
        directories::ProjectDirs::from("", "", "locallibrary")
            .map(|d| d.data_dir().to_path_buf())
            .unwrap_or_else(|| PathBuf::from(".locallibrary-store"))
    }

    /// Open a store, creating it if the directory is empty or missing, or holds only what
    /// locallibrary itself keeps beside a store (an older version wrote some of it before the
    /// store was made: fingerprints, mix reports, the profile document).
    pub fn open_or_init(root: impl AsRef<Path>) -> Result<Store> {
        const OURS: [&str; 4] =
            [crate::mix::index::CACHE_DIR, crate::mix::report::REPORTS_DIR, "profile-document.json", "tui.json"];
        let root = root.as_ref().to_path_buf();
        let meta_path = root.join("store.json");
        if !meta_path.exists() {
            for e in fs::read_dir(&root).into_iter().flatten() {
                if !OURS.iter().any(|o| e.as_ref().is_ok_and(|e| e.file_name() == *o)) {
                    return Err(Error::invalid(format!(
                        "{} is not empty and is not an locallibrary store",
                        root.display()
                    )));
                }
            }
            for d in ["objects", "snapshots"] {
                fs::create_dir_all(root.join(d)).at(root.join(d))?;
            }
            fsutil::write_json_atomic(
                &meta_path,
                &StoreMeta { format: STORE_FORMAT, id: Uuid::new_v4(), created: Utc::now() },
            )?;
        }
        Store::open(root)
    }

    pub fn open(root: impl AsRef<Path>) -> Result<Store> {
        let root = root.as_ref().to_path_buf();
        let meta: StoreMeta = fsutil::read_json(&root.join("store.json"))
            .map_err(|_| Error::NotFound(format!("no locallibrary store at {}", root.display())))?;
        if meta.format > STORE_FORMAT {
            return Err(Error::invalid(format!(
                "store format {} is newer than this version of locallibrary supports ({STORE_FORMAT})",
                meta.format
            )));
        }
        Ok(Store { root })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The store's id (its copies share it).
    pub fn id(&self) -> Result<Uuid> {
        Ok(fsutil::read_json::<StoreMeta>(&self.root.join("store.json"))?.id)
    }

    /// Take the store's write lock (fails fast if another process holds it).
    pub fn lock(&self) -> Result<StoreLock> {
        match fsutil::FileLock::try_take(&self.root.join("lock"))? {
            Some(lock) => Ok(StoreLock { _lock: lock }),
            None => Err(Error::Locked(self.root.clone())),
        }
    }

    // -- objects ----------------------------------------------------------------------------

    pub fn object_path(&self, h: &Hash) -> PathBuf {
        let hex = h.to_hex();
        self.root.join("objects").join(&hex[..2]).join(&hex[2..])
    }

    pub fn has_object(&self, h: &Hash) -> bool {
        self.object_path(h).is_file()
    }

    /// Whether the store holds the object `h` at its full `size`. New objects are renamed into
    /// place before they're flushed (see [`Store::import_file`]), so a crash can leave one cut
    /// short under its right name: imports check the size before trusting it.
    fn holds(&self, h: &Hash, size: u64) -> bool {
        fs::metadata(self.object_path(h)).is_ok_and(|m| m.is_file() && m.len() == size)
    }

    /// Import a file. If `known` (a hash from a trusted cache) is given and the object already
    /// exists at the file's size, the file is not read at all. New objects (and ones a crash cut
    /// short, which are replaced) are not fsynced individually; call [`Store::flush_objects`]
    /// before recording a snapshot that references them.
    pub fn import_file(&self, src: &Path, known: Option<Hash>, ctx: &Ctx<'_>) -> Result<(Hash, bool)> {
        if let Some(h) = known
            && self.holds(&h, fs::metadata(src).at(src)?.len())
        {
            return Ok((h, false));
        }
        let tmp_dir = self.root.join("objects").join("tmp");
        fs::create_dir_all(&tmp_dir).at(&tmp_dir)?;
        let mut tmp = tempfile::Builder::new().prefix("import-").tempfile_in(&tmp_dir).at(&tmp_dir)?;
        let tmp_path = tmp.path().to_path_buf();
        let (h, size) = hash::copy_hashing(src, tmp.as_file_mut(), &tmp_path, ctx, Phase::Copy)?;
        let dst = self.object_path(&h);
        if self.holds(&h, size) {
            return Ok((h, false));
        }
        let parent = dst.parent().expect("object path has parent");
        fs::create_dir_all(parent).at(parent)?;
        tmp.persist(&dst).map_err(|e| Error::Io { path: dst.clone(), source: e.error })?;
        Ok((h, true))
    }

    /// Make imported objects durable.
    pub fn flush_objects(&self, hashes: &[Hash]) -> Result<()> {
        fsutil::flush(&hashes.iter().map(|h| self.object_path(h)).collect::<Vec<_>>())
    }

    pub fn import_bytes(&self, bytes: &[u8]) -> Result<Hash> {
        let h = Hash::of(bytes);
        if !self.holds(&h, bytes.len() as u64) {
            fsutil::write_atomic(&self.object_path(&h), bytes)?;
        }
        Ok(h)
    }

    // -- libraries --------------------------------------------------------------------------

    pub fn libraries(&self) -> Result<Vec<LibraryRecord>> {
        Ok(fsutil::read_json_opt(&self.root.join("libraries.json"))?.unwrap_or_default())
    }

    fn save_libraries(&self, libs: &[LibraryRecord]) -> Result<()> {
        fsutil::write_json_atomic(&self.root.join("libraries.json"), &libs)
    }

    /// Find a library by name (case-insensitive) or id.
    pub fn find_library(&self, name_or_id: &str) -> Result<Option<LibraryRecord>> {
        Ok(self
            .libraries()?
            .into_iter()
            .find(|l| l.name.eq_ignore_ascii_case(name_or_id) || l.id.to_string() == name_or_id))
    }

    /// `base`, or `base 2`, `base 3`... when the store has a library of that name.
    pub fn unused_library_name(&self, base: &str) -> Result<String> {
        let mut name = base.to_string();
        for n in 2.. {
            if self.find_library(&name)?.is_none() {
                break;
            }
            name = format!("{base} {n}");
        }
        Ok(name)
    }

    pub fn library_by_id(&self, id: Uuid) -> Result<Option<LibraryRecord>> {
        Ok(self.libraries()?.into_iter().find(|l| l.id == id))
    }

    pub fn create_library(&self, name: &str) -> Result<LibraryRecord> {
        let mut libs = self.libraries()?;
        if libs.iter().any(|l| l.name.eq_ignore_ascii_case(name)) {
            return Err(
                Error::conflict(Reason::Exists, format!("a library named {name:?} already exists")).with("name", name)
            );
        }
        let rec = LibraryRecord {
            id: Uuid::new_v4(),
            name: name.to_string(),
            head: None,
            created: Utc::now(),
            updated: Utc::now(),
        };
        libs.push(rec.clone());
        self.save_libraries(&libs)?;
        Ok(rec)
    }

    pub fn set_head(&self, library: Uuid, head: Hash) -> Result<()> {
        let mut libs = self.libraries()?;
        let lib =
            libs.iter_mut().find(|l| l.id == library).ok_or_else(|| Error::NotFound(format!("library {library}")))?;
        lib.head = Some(head);
        lib.updated = Utc::now();
        self.save_libraries(&libs)
    }

    pub fn rename_library(&self, library: Uuid, name: &str) -> Result<()> {
        let name = name.trim();
        if name.is_empty() {
            return Err(Error::invalid("a library needs a name"));
        }
        let mut libs = self.libraries()?;
        if libs.iter().any(|l| l.id != library && l.name.eq_ignore_ascii_case(name)) {
            return Err(
                Error::conflict(Reason::Exists, format!("a library named {name:?} already exists")).with("name", name)
            );
        }
        let lib =
            libs.iter_mut().find(|l| l.id == library).ok_or_else(|| Error::NotFound(format!("library {library}")))?;
        lib.name = name.to_string();
        self.save_libraries(&libs)
    }

    // -- snapshots --------------------------------------------------------------------------

    pub(crate) fn snapshot_path(&self, id: &Hash) -> PathBuf {
        self.root.join("snapshots").join(format!("{}.json", id.to_hex()))
    }

    /// Persist a snapshot (its id is computed from content). All referenced objects must exist.
    pub fn save_snapshot(&self, mut snap: Snapshot) -> Result<Snapshot> {
        for f in &snap.files {
            let h = f.hash.ok_or_else(|| Error::invalid(format!("{} has no hash", f.path)))?;
            if !self.has_object(&h) {
                return Err(Error::NotFound(format!("object for {} ({})", f.path, h.short())));
            }
        }
        snap.id = snap.compute_id();
        fsutil::write_json_atomic(&self.snapshot_path(&snap.id), &snap)?;
        Ok(snap)
    }

    pub fn load_snapshot(&self, id: &Hash) -> Result<Snapshot> {
        let p = self.snapshot_path(id);
        if !p.is_file() {
            return Err(Error::NotFound(format!("snapshot {}", id.short())));
        }
        fsutil::read_json(&p)
    }

    /// A snapshot's [`SnapshotInfo`] (without its labels), read without keeping its file list:
    /// for walks over many snapshots, whose manifests list every file of a library.
    pub fn load_info(&self, id: &Hash) -> Result<SnapshotInfo> {
        let p = self.snapshot_path(id);
        if !p.is_file() {
            return Err(Error::NotFound(format!("snapshot {}", id.short())));
        }
        Ok(fsutil::read_json::<SnapshotHead>(&p)?.info())
    }

    /// Resolve a snapshot label (any case), or a full or abbreviated (>= 4 hex chars) id.
    pub fn resolve_snapshot(&self, prefix: &str) -> Result<Hash> {
        if let Some(l) = self.find_label(prefix)? {
            return Ok(l.snapshot);
        }
        let prefix = prefix.trim().to_ascii_lowercase();
        if prefix.len() < 4 {
            return Err(Error::invalid("snapshot id prefix must be at least 4 characters"));
        }
        let mut found = Vec::new();
        for id in self.snapshot_ids()? {
            if id.to_hex().starts_with(&prefix) {
                found.push(id);
            }
        }
        if found.is_empty()
            && let Some((_, p)) = self.pruned()?.into_iter().find(|(id, _)| id.to_hex().starts_with(&prefix))
        {
            return Err(Error::NotFound(format!(
                "snapshot {prefix}: it was pruned on {}",
                p.pruned.format("%Y-%m-%d %H:%M")
            )));
        }
        match found.len() {
            0 => Err(Error::NotFound(format!("snapshot {prefix}"))),
            1 => Ok(found[0]),
            n => Err(Error::invalid(format!("snapshot id {prefix} is ambiguous ({n} matches)"))),
        }
    }

    pub fn snapshot_ids(&self) -> Result<Vec<Hash>> {
        let dir = self.root.join("snapshots");
        let mut ids = Vec::new();
        for e in fs::read_dir(&dir).at(&dir)? {
            let e = e.at(&dir)?;
            if let Some(stem) = e.file_name().to_str().and_then(|n| n.strip_suffix(".json"))
                && let Ok(h) = stem.parse()
            {
                ids.push(h);
            }
        }
        ids.sort_unstable();
        Ok(ids)
    }

    /// Snapshots of a library from head back through first parents (and through pruned ones,
    /// which aren't listed).
    pub fn history(&self, library: &LibraryRecord, limit: Option<usize>) -> Result<Vec<SnapshotInfo>> {
        let labels = self.labels_by_snapshot()?;
        let pruned = self.pruned()?;
        let mut out = Vec::new();
        let mut cur = library.head;
        while let Some(id) = cur {
            if limit.is_some_and(|l| out.len() >= l) {
                break;
            }
            let mut info = match self.load_info(&id) {
                Ok(info) => info,
                Err(Error::NotFound(_)) if pruned.contains_key(&id) => {
                    cur = pruned[&id].parents.first().copied();
                    continue;
                }
                Err(e) => return Err(e),
            };
            cur = info.parents.first().copied();
            info.labels = labels.get(&id).cloned().unwrap_or_default();
            out.push(info);
        }
        Ok(out)
    }

    /// A snapshot's [`SnapshotInfo`], with its labels.
    pub fn snapshot_info(&self, snapshot: &Snapshot) -> Result<SnapshotInfo> {
        let mut info = snapshot.info();
        info.labels = self.labels_by_snapshot()?.shift_remove(&snapshot.id).unwrap_or_default();
        Ok(info)
    }

    // -- labels -----------------------------------------------------------------------------

    /// Every label, oldest first.
    pub fn labels(&self) -> Result<Vec<SnapshotLabel>> {
        Ok(fsutil::read_json_opt(&self.root.join("labels.json"))?.unwrap_or_default())
    }

    fn save_labels(&self, labels: &[SnapshotLabel]) -> Result<()> {
        fsutil::write_json_atomic(&self.root.join("labels.json"), &labels)
    }

    /// Label names by snapshot.
    pub fn labels_by_snapshot(&self) -> Result<IndexMap<Hash, Vec<String>>> {
        let mut out: IndexMap<Hash, Vec<String>> = IndexMap::new();
        for l in self.labels()? {
            out.entry(l.snapshot).or_default().push(l.name);
        }
        Ok(out)
    }

    /// A label by name, ignoring ASCII case.
    pub fn find_label(&self, name: &str) -> Result<Option<SnapshotLabel>> {
        let name = name.trim();
        Ok(self.labels()?.into_iter().find(|l| l.name.eq_ignore_ascii_case(name)))
    }

    /// Give a snapshot a label. Labelling a snapshot with a label it has already does nothing; a
    /// label on another snapshot is a conflict (take it off first).
    pub fn add_label(&self, snapshot: &Hash, name: &str) -> Result<SnapshotLabel> {
        let name = name.trim();
        if name.is_empty() {
            return Err(Error::invalid("a label can't be empty"));
        }
        if !self.snapshot_path(snapshot).is_file() {
            return Err(Error::NotFound(format!("snapshot {}", snapshot.short())));
        }
        let mut labels = self.labels()?;
        if let Some(l) = labels.iter().find(|l| l.name.eq_ignore_ascii_case(name)) {
            if l.snapshot == *snapshot {
                return Ok(l.clone());
            }
            return Err(label_taken(l));
        }
        let label = SnapshotLabel { name: name.to_string(), snapshot: *snapshot, created: Utc::now() };
        labels.push(label.clone());
        self.save_labels(&labels)?;
        Ok(label)
    }

    /// Take a label off its snapshot.
    pub fn remove_label(&self, name: &str) -> Result<SnapshotLabel> {
        let mut labels = self.labels()?;
        let name = name.trim();
        let i = labels
            .iter()
            .position(|l| l.name.eq_ignore_ascii_case(name))
            .ok_or_else(|| Error::NotFound(format!("label {name:?}")))?;
        let removed = labels.remove(i);
        self.save_labels(&labels)?;
        Ok(removed)
    }

    /// Whether `ancestor` is reachable from `descendant` through parents (inclusive).
    pub fn is_ancestor(&self, ancestor: &Hash, descendant: &Hash) -> Result<bool> {
        let pruned = self.pruned()?;
        let mut seen = FxHashSet::default();
        let mut queue = VecDeque::from([*descendant]);
        while let Some(id) = queue.pop_front() {
            if &id == ancestor {
                return Ok(true);
            }
            if seen.insert(id) {
                match self.load_snapshot(&id) {
                    Ok(s) => queue.extend(s.parents),
                    Err(_) => queue.extend(pruned.get(&id).into_iter().flat_map(|p| p.parents.iter().copied())),
                }
            }
        }
        Ok(false)
    }

    // -- pruning ----------------------------------------------------------------------------

    /// Snapshots `store prune` removed, by id.
    pub fn pruned(&self) -> Result<IndexMap<Hash, PrunedSnapshot>> {
        Ok(fsutil::read_json_opt(&self.root.join("pruned.json"))?.unwrap_or_default())
    }

    /// Remove snapshots, keeping their parents so history walks through them (see
    /// [`crate::retention`], which decides which). A library's latest snapshot and labelled ones
    /// are refused. Their files stay until [`Store::gc`].
    pub fn forget_snapshots(&self, ids: &[Hash]) -> Result<()> {
        let heads: FxHashSet<Hash> = self.libraries()?.iter().filter_map(|l| l.head).collect();
        let labelled: FxHashSet<Hash> = self.labels()?.iter().map(|l| l.snapshot).collect();
        let mut pruned = self.pruned()?;
        let mut doomed = Vec::new();
        for id in ids {
            if heads.contains(id) || labelled.contains(id) {
                return Err(Error::conflict(
                    Reason::SnapshotKept,
                    format!("snapshot {} is a library's latest or labelled, and stays", id.short()),
                )
                .with("snapshot", id));
            }
            let s = self.load_snapshot(id)?;
            let p = PrunedSnapshot {
                library_id: s.library_id,
                parents: s.parents,
                created: s.created,
                fingerprint: s.fingerprint,
                pruned: Utc::now(),
            };
            pruned.insert(*id, p);
            doomed.push(self.snapshot_path(id));
        }
        // The parents first: a prune stopped halfway leaves history whole.
        fsutil::write_json_atomic(&self.root.join("pruned.json"), &pruned)?;
        for p in doomed {
            fs::remove_file(&p).at(&p)?;
        }
        Ok(())
    }

    /// Find the snapshot of `library` whose content fingerprint is `fp`, if any.
    pub fn find_by_fingerprint(&self, library: &LibraryRecord, fp: &Hash) -> Result<Option<SnapshotInfo>> {
        Ok(self.history(library, None)?.into_iter().find(|s| &s.fingerprint == fp))
    }

    // -- drives -----------------------------------------------------------------------------

    pub fn drives(&self) -> Result<IndexMap<Uuid, DriveRecord>> {
        Ok(fsutil::read_json_opt(&self.root.join("drives.json"))?.unwrap_or_default())
    }

    /// Change the drive registry: `f` gets every record, and what it leaves is written back (when
    /// it changed anything). The read and the write happen under a lock of their own
    /// (`drives.lock`, waited for, and taken whether or not the caller holds the store's), so a
    /// CLI and the sidecar recording at once don't drop each other's changes. `f` mustn't
    /// change the registry itself.
    pub fn update_drives<T>(&self, f: impl FnOnce(&mut IndexMap<Uuid, DriveRecord>) -> Result<T>) -> Result<T> {
        let _lock = fsutil::FileLock::wait(&self.root.join("drives.lock"))?;
        let mut drives = self.drives()?;
        let before = serde_json::to_value(&drives).map_err(|e| Error::invalid(e.to_string()))?;
        let out = f(&mut drives)?;
        if serde_json::to_value(&drives).map_err(|e| Error::invalid(e.to_string()))? != before {
            fsutil::write_json_atomic(&self.root.join("drives.json"), &drives)?;
        }
        Ok(out)
    }

    pub fn forget_drive(&self, id: Uuid) -> Result<bool> {
        self.update_drives(|drives| Ok(drives.shift_remove(&id).is_some()))
    }

    // -- settings ---------------------------------------------------------------------------

    pub fn settings_profile(&self) -> Result<SettingsProfile> {
        Ok(fsutil::read_json_opt(&self.root.join("settings.json"))?.unwrap_or_default())
    }

    pub fn save_settings_profile(&self, p: &SettingsProfile) -> Result<()> {
        fsutil::write_json_atomic(&self.root.join("settings.json"), p)
    }

    /// The player models remembered for `check`.
    pub fn player_models(&self) -> Result<crate::equipment::Remembered> {
        Ok(fsutil::read_json_opt(&self.root.join("players.json"))?.unwrap_or_default())
    }

    pub fn save_player_models(&self, r: &crate::equipment::Remembered) -> Result<()> {
        fsutil::write_json_atomic(&self.root.join("players.json"), r)
    }

    /// Rule-based playlists (see [`crate::smart`]).
    pub fn smart_playlists(&self) -> Result<crate::smart::SmartPlaylists> {
        Ok(fsutil::read_json_opt(&self.root.join("smart.json"))?.unwrap_or_default())
    }

    pub fn save_smart_playlists(&self, p: &crate::smart::SmartPlaylists) -> Result<()> {
        fsutil::write_json_atomic(&self.root.join("smart.json"), p)
    }

    // -- maintenance ------------------------------------------------------------------------

    pub fn stats(&self) -> Result<StoreStats> {
        let mut stats = StoreStats {
            libraries: self.libraries()?.len(),
            snapshots: self.snapshot_ids()?.len(),
            drives: self.drives()?.len(),
            ..Default::default()
        };
        for (_, size) in self.objects()? {
            stats.objects += 1;
            stats.object_bytes += size;
        }
        Ok(stats)
    }

    /// Every stored object and its size.
    pub(crate) fn objects(&self) -> Result<Vec<(Hash, u64)>> {
        let dir = self.root.join("objects");
        let mut out = Vec::new();
        for shard in fs::read_dir(&dir).at(&dir)? {
            let shard = shard.at(&dir)?;
            let name = shard.file_name().to_string_lossy().into_owned();
            if name.len() != 2 || !shard.path().is_dir() {
                continue;
            }
            for obj in fs::read_dir(shard.path()).at(shard.path())? {
                let obj = obj.at(shard.path())?;
                let hex = format!("{name}{}", obj.file_name().to_string_lossy());
                if let Ok(h) = hex.parse::<Hash>() {
                    out.push((h, obj.metadata().map(|m| m.len()).unwrap_or(0)));
                }
            }
        }
        Ok(out)
    }

    /// What writes cut off by a crash or a pulled plug (a pull, a copy of the store) left among
    /// stored files and snapshots, and their sizes: everything in `objects/tmp/`, and temp files.
    /// Those writes run under the store's lock, so to whoever holds it, all of them are leftovers.
    pub(crate) fn leftovers(&self) -> Result<Vec<(PathBuf, u64)>> {
        let staging = self.root.join("objects").join("tmp");
        let mut out = Vec::new();
        for top in ["objects", "snapshots"].map(|d| self.root.join(d)) {
            let Ok(entries) = fs::read_dir(&top) else { continue };
            let mut dirs = vec![(top.clone(), false)];
            for e in entries.flatten() {
                if e.file_type().is_ok_and(|t| t.is_dir()) {
                    dirs.push((e.path(), e.path() == staging));
                }
            }
            for (dir, all) in dirs {
                for e in fs::read_dir(&dir).at(&dir)?.flatten() {
                    if all || e.file_name().to_string_lossy().starts_with(crate::paths::TEMP_PREFIX) {
                        out.push((e.path(), e.metadata().map(|m| m.len()).unwrap_or(0)));
                    }
                }
            }
        }
        Ok(out)
    }

    /// Delete what nothing uses: snapshots of no library the store has (unless labelled, a
    /// library's latest, or a parent of a snapshot that stays), then the stored files no
    /// remaining snapshot lists, and what writes cut off by a crash left among them. Every
    /// snapshot of a library stays, whether its latest reaches it or not (a drive back from a set
    /// is saved beside it, see [`crate::sync::pull_beside`]): which of them go is `store prune`'s
    /// choice ([`crate::retention`]). A manifest that can't be read hides which library it
    /// belongs to and which files it uses, so then nothing is deleted. Unless `dry_run`, the
    /// caller holds the store's lock.
    pub fn gc(&self, dry_run: bool) -> Result<GcReport> {
        let g = self.garbage(&FxHashSet::default())?;
        let leftovers = self.leftovers()?;
        let report = GcReport {
            removed_snapshots: g.snapshots.len(),
            removed_objects: g.objects.len() + leftovers.len(),
            freed_bytes: g.objects.iter().map(|(_, size)| size).chain(leftovers.iter().map(|(_, size)| size)).sum(),
            dry_run,
        };
        if dry_run {
            return Ok(report);
        }
        for id in &g.snapshots {
            let p = self.snapshot_path(id);
            fs::remove_file(&p).at(&p)?;
        }
        for (h, _) in &g.objects {
            let p = self.object_path(h);
            fs::remove_file(&p).at(&p)?;
        }
        for (p, _) in &leftovers {
            fsutil::remove_if_exists(p)?;
        }
        if let Some(pruned) = g.pruned {
            fsutil::write_json_atomic(&self.root.join("pruned.json"), &pruned)?;
        }
        Ok(report)
    }

    /// What [`Store::gc`] would delete once the `forgotten` snapshots are gone as well (what
    /// `store prune` plans to remove: it says what that frees this way). Every manifest is read
    /// first, and nothing is planned if one can't be.
    pub(crate) fn garbage(&self, forgotten: &FxHashSet<Hash>) -> Result<Garbage> {
        let libs = self.libraries()?;
        let libraries: FxHashSet<Uuid> = libs.iter().map(|l| l.id).collect();
        let mut roots: FxHashSet<Hash> = self.labels()?.iter().map(|l| l.snapshot).collect();
        roots.extend(libs.iter().filter_map(|l| l.head));
        let mut pruned = self.pruned()?;
        let mut live = FxHashSet::default();
        let mut queue = VecDeque::new();
        // Snapshots of no library, until a snapshot that stays turns out to descend from one:
        // their parents and files.
        let mut orphans: IndexMap<Hash, (Vec<Hash>, Vec<Hash>)> = IndexMap::new();
        // Forgotten snapshots walk on to their parents, as pruned ones do.
        let mut gone: IndexMap<Hash, Vec<Hash>> = IndexMap::new();
        for id in self.snapshot_ids()? {
            let s = self.load_snapshot(&id)?;
            let files = s.files.iter().filter_map(|f| f.hash);
            if forgotten.contains(&id) {
                gone.insert(id, s.parents);
            } else if libraries.contains(&s.library_id) || roots.contains(&id) {
                live.extend(files);
                queue.extend(s.parents);
            } else {
                orphans.insert(id, (s.parents, files.collect()));
            }
        }
        let mut reached = FxHashSet::default();
        while let Some(id) = queue.pop_front() {
            if !reached.insert(id) {
                continue;
            }
            if let Some((parents, files)) = orphans.shift_remove(&id) {
                live.extend(files);
                queue.extend(parents);
            } else if let Some(parents) = gone.get(&id) {
                queue.extend(parents.iter().copied());
            } else if let Some(p) = pruned.get(&id) {
                queue.extend(p.parents.iter().copied());
            }
        }
        let objects = self.objects()?.into_iter().filter(|(h, _)| !live.contains(h)).collect();
        // What's left of pruned snapshots nothing reaches any more can go too.
        let before = pruned.len();
        pruned.retain(|id, _| reached.contains(id));
        let pruned = (pruned.len() != before).then_some(pruned);
        Ok(Garbage { snapshots: orphans.into_keys().collect(), objects, pruned })
    }

    /// Re-hash every object referenced by any snapshot.
    pub fn verify(&self, ctx: &Ctx<'_>) -> Result<VerifyReport> {
        let mut report = VerifyReport::default();
        let mut wanted: IndexMap<Hash, String> = IndexMap::new();
        for id in self.snapshot_ids()? {
            for f in self.load_snapshot(&id)?.files {
                if let Some(h) = f.hash {
                    wanted.entry(h).or_insert(f.path);
                }
            }
        }
        ctx.phase_start(Phase::Verify, wanted.len() as u64, 0);
        for (h, path) in wanted {
            ctx.check()?;
            let p = self.object_path(&h);
            ctx.emit(Event::Item { phase: Phase::Verify, path: path.clone(), bytes: 0 });
            if !p.is_file() {
                report.missing.push(path);
                continue;
            }
            report.checked += 1;
            if hash::hash_file(&p, ctx, Phase::Verify)? != h {
                report.corrupt.push(path);
            }
        }
        ctx.phase_end(Phase::Verify);
        Ok(report)
    }
}

/// A label that's on another snapshot already: it has to come off first.
pub(crate) fn label_taken(l: &SnapshotLabel) -> Error {
    Error::conflict(
        Reason::LabelTaken,
        format!("the label {:?} is on snapshot {} already; take it off first", l.name, l.snapshot.short()),
    )
    .with("label", &l.name)
    .with("snapshot", l.snapshot)
    .action(Action::call(
        "snapshot.label",
        serde_json::json!({ "label": l.name, "remove": true }),
        format!("Take {:?} off", l.name),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths::FileClass;

    #[test]
    fn init_import_snapshot_history_gc() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open_or_init(dir.path().join("s")).unwrap();
        let _lock = store.lock().unwrap();
        assert!(matches!(store.lock(), Err(Error::Locked(_))));

        let src = dir.path().join("a.bin");
        fs::write(&src, b"abc").unwrap();
        let (h, new) = store.import_file(&src, None, &Ctx::silent()).unwrap();
        assert!(new);
        assert_eq!(store.import_file(&src, Some(h), &Ctx::silent()).unwrap(), (h, false));
        let orphan = store.import_bytes(b"orphan").unwrap();

        let lib = store.create_library("Main").unwrap();
        assert!(store.create_library("main").is_err());
        let entry =
            FileEntry { path: "Contents/a.mp3".into(), size: 3, mtime: 0, class: FileClass::Audio, hash: Some(h) };
        let mk = |parents: Vec<Hash>, msg: &str| Snapshot {
            id: Hash::of(b""),
            library_id: lib.id,
            parents,
            created: Utc::now(),
            message: Some(msg.into()),
            source: None,
            fingerprint: Hash::of(msg.as_bytes()),
            summary: LibrarySummary::default(),
            lineage: vec![],
            files: vec![entry.clone()],
        };
        let s1 = store.save_snapshot(mk(vec![], "one")).unwrap();
        let s2 = store.save_snapshot(mk(vec![s1.id], "two")).unwrap();
        store.set_head(lib.id, s2.id).unwrap();
        let lib = store.find_library("MAIN").unwrap().unwrap();
        let hist = store.history(&lib, None).unwrap();
        assert_eq!(hist.iter().map(|s| s.message.clone().unwrap()).collect::<Vec<_>>(), ["two", "one"]);
        assert!(store.is_ancestor(&s1.id, &s2.id).unwrap());
        assert!(!store.is_ancestor(&s2.id, &s1.id).unwrap());
        assert_eq!(store.resolve_snapshot(&s1.id.short()).unwrap(), s1.id);

        let gc = store.gc(false).unwrap();
        assert_eq!(gc.removed_objects, 1);
        assert!(!store.has_object(&orphan));
        assert!(store.has_object(&h));
        assert!(store.verify(&Ctx::silent()).unwrap().corrupt.is_empty());
    }

    #[test]
    fn a_label_keeps_a_snapshot_of_no_library() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open_or_init(dir.path().join("s")).unwrap();
        let lib = store.create_library("Main").unwrap();
        let h = store.import_bytes(b"db").unwrap();
        let entry =
            FileEntry { path: "PIONEER/x.pdb".into(), size: 2, mtime: 0, class: FileClass::Database, hash: Some(h) };
        let snapshot = |library_id: Uuid, fp: &[u8]| Snapshot {
            id: Hash::of(b""),
            library_id,
            parents: vec![],
            created: Utc::now(),
            message: None,
            source: None,
            fingerprint: Hash::of(fp),
            summary: LibrarySummary::default(),
            lineage: vec![],
            files: vec![entry.clone()],
        };
        // Beside the library's latest (it has none), as a drive back from a set is saved: it
        // stays, whatever the labels.
        let beside = store.save_snapshot(snapshot(lib.id, b"beside")).unwrap();
        let lone = store.save_snapshot(snapshot(Uuid::new_v4(), b"fp")).unwrap();
        store.add_label(&lone.id, "keep me").unwrap();
        assert_eq!(store.gc(false).unwrap().removed_snapshots, 0);
        assert!(store.has_object(&h));
        assert_eq!(store.resolve_snapshot("KEEP ME").unwrap(), lone.id);
        store.remove_label("keep me").unwrap();
        let gc = store.gc(false).unwrap();
        assert_eq!((gc.removed_snapshots, gc.removed_objects), (1, 0), "the beside snapshot uses the file too");
        assert!(store.add_label(&lone.id, "too late").is_err());
        assert!(store.load_snapshot(&beside.id).is_ok());
    }

    /// A library's snapshots, oldest first, each on the last, the newest its head.
    fn chain(store: &Store, lib: &LibraryRecord, n: u8) -> Vec<Snapshot> {
        let mut out: Vec<Snapshot> = Vec::new();
        for i in 0..n {
            let h = store.import_bytes(&[i]).unwrap();
            let entry = FileEntry {
                path: format!("Contents/{i}.mp3"),
                size: 1,
                mtime: 0,
                class: FileClass::Audio,
                hash: Some(h),
            };
            let s = store
                .save_snapshot(Snapshot {
                    id: Hash::of(b""),
                    library_id: lib.id,
                    parents: out.last().map(|s| s.id).into_iter().collect(),
                    created: Utc::now(),
                    message: None,
                    source: None,
                    fingerprint: Hash::of(&[i]),
                    summary: LibrarySummary::default(),
                    lineage: vec![],
                    files: vec![entry],
                })
                .unwrap();
            store.set_head(lib.id, s.id).unwrap();
            out.push(s);
        }
        out
    }

    /// Regression: a head whose manifest was missing kept nothing alive, so gc deleted the
    /// library's history and its files.
    #[test]
    fn gc_keeps_a_library_whose_latest_is_missing() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open_or_init(dir.path().join("s")).unwrap();
        let lib = store.create_library("Main").unwrap();
        let snaps = chain(&store, &lib, 3);
        fs::remove_file(store.snapshot_path(&snaps[2].id)).unwrap();
        let gc = store.gc(false).unwrap();
        assert_eq!((gc.removed_snapshots, gc.removed_objects), (0, 1), "only the missing snapshot's own file");
        for s in &snaps[..2] {
            assert!(store.load_snapshot(&s.id).is_ok());
            assert!(store.has_object(&s.files[0].hash.unwrap()));
        }
    }

    /// Regression: an object a crash cut short (renamed into place before it was flushed) was
    /// trusted by every later import, as long as a file had its name.
    #[test]
    fn imports_replace_an_object_cut_short() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open_or_init(dir.path().join("s")).unwrap();
        let src = dir.path().join("a.bin");
        fs::write(&src, b"the whole file").unwrap();
        let (h, _) = store.import_file(&src, None, &Ctx::silent()).unwrap();
        let cut = |store: &Store| fs::write(store.object_path(&h), b"the wh").unwrap();
        let whole = |store: &Store| fs::read(store.object_path(&h)).unwrap() == b"the whole file";

        cut(&store);
        assert_eq!(store.import_file(&src, Some(h), &Ctx::silent()).unwrap(), (h, true), "a cached hash");
        assert!(whole(&store));
        cut(&store);
        assert_eq!(store.import_file(&src, None, &Ctx::silent()).unwrap(), (h, true), "hashed again");
        assert!(whole(&store));
        cut(&store);
        assert_eq!(store.import_bytes(b"the whole file").unwrap(), h);
        assert!(whole(&store));
        assert_eq!(store.import_file(&src, Some(h), &Ctx::silent()).unwrap(), (h, false), "whole: not read");
    }

    /// Regression: the registry was read and written back with no lock, so two writers at once
    /// dropped each other's changes.
    #[test]
    fn registry_writers_at_once_keep_every_change() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open_or_init(dir.path().join("s")).unwrap();
        let id = Uuid::new_v4();
        store
            .update_drives(|d| {
                d.insert(id, DriveRecord::new(id, "Main", dir.path().to_path_buf()));
                Ok(())
            })
            .unwrap();
        std::thread::scope(|scope| {
            for _ in 0..8 {
                scope.spawn(|| {
                    for _ in 0..25 {
                        crate::fleet::update(&store, id, |r| r.log.writes += 1).unwrap();
                    }
                });
            }
        });
        assert_eq!(store.drives().unwrap()[&id].log.writes, 200);
    }

    /// History walks read manifests without their file lists: one listing a kind of file this
    /// version doesn't know (written by a newer one) still walks.
    #[test]
    fn history_reads_manifests_without_their_files() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open_or_init(dir.path().join("s")).unwrap();
        let lib = store.create_library("Main").unwrap();
        let snaps = chain(&store, &lib, 2);
        let path = store.snapshot_path(&snaps[0].id);
        let text = fs::read_to_string(&path).unwrap().replace("\"audio\"", "\"stems\"");
        fs::write(&path, text).unwrap();
        assert!(store.load_snapshot(&snaps[0].id).is_err());
        let lib = store.find_library("Main").unwrap().unwrap();
        let hist = store.history(&lib, None).unwrap();
        assert_eq!(
            hist.iter().map(|s| (s.id, s.files, s.bytes)).collect::<Vec<_>>(),
            [(snaps[1].id, 1, 1), (snaps[0].id, 1, 1)]
        );
        assert_eq!(store.find_by_fingerprint(&lib, &snaps[0].fingerprint).unwrap().map(|s| s.id), Some(snaps[0].id));
    }

    /// A pull cut off by a crash leaves what it was writing behind: `gc` counts it with the files
    /// nothing uses, and removes it.
    #[test]
    fn gc_removes_what_cut_off_imports_left() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open_or_init(dir.path().join("s")).unwrap();
        let _lock = store.lock().unwrap();
        let stray = store.import_bytes(b"abc").unwrap();
        let shard = store.object_path(&stray).parent().unwrap().to_path_buf();
        let left = [
            store.root().join("objects/tmp/import-x1y2"),
            shard.join(".locallibrary-tmp-ab"),
            store.root().join("snapshots/.locallibrary-tmp-cd"),
        ];
        for p in &left {
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            fs::write(p, b"half").unwrap();
        }
        // The stray object and the three leftovers.
        let plan = store.gc(true).unwrap();
        assert_eq!((plan.removed_objects, plan.freed_bytes), (4, 15));
        store.gc(false).unwrap();
        assert!(left.iter().all(|p| !p.exists()));
        assert_eq!(store.gc(true).unwrap().removed_objects, 0);
    }

    #[test]
    fn refuses_to_init_over_unrelated_directory() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("something.txt"), "hi").unwrap();
        assert!(Store::open_or_init(dir.path()).is_err());
    }

    /// What older versions wrote into the store's folder before making the store doesn't stop it
    /// being made there.
    #[test]
    fn inits_over_what_locallibrary_left_there() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("cache/fingerprints/3a")).unwrap();
        fs::create_dir_all(dir.path().join("reports")).unwrap();
        fs::write(dir.path().join("profile-document.json"), "{}").unwrap();
        Store::open_or_init(dir.path()).unwrap();
        assert!(dir.path().join("store.json").is_file());
    }
}
