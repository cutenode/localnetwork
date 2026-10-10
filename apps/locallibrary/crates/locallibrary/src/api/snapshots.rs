//! The store's libraries, snapshot labels and pruning.

use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::hash::Hash;
use crate::op::{Operation, Planned, Probe, Touched, bytes};
use crate::progress::Ctx;
use crate::store::{GcReport, LibraryRecord, SnapshotInfo, SnapshotLabel, Store};
use crate::{Error, Result};

/// A library in the store, for listings.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct LibraryListing {
    pub id: Uuid,
    pub name: String,
    pub created: DateTime<Utc>,
    pub updated: DateTime<Utc>,
    /// Its snapshots, back from the latest through first parents.
    pub snapshots: usize,
    /// The latest snapshot.
    pub head: Option<SnapshotInfo>,
    /// Drives linked to it, by name.
    pub drives: Vec<String>,
}

/// Every library, by name.
pub fn libraries_list(store: &Store) -> Result<Vec<LibraryListing>> {
    let drives = store.drives()?;
    let mut out = Vec::new();
    for lib in store.libraries()? {
        let history = store.history(&lib, None)?;
        let mut names: Vec<String> =
            drives.values().filter(|d| d.library_id == Some(lib.id)).map(|d| d.name.clone()).collect();
        names.sort_by_key(|n| n.to_lowercase());
        out.push(LibraryListing {
            id: lib.id,
            name: lib.name,
            created: lib.created,
            updated: lib.updated,
            snapshots: history.len(),
            head: history.into_iter().next(),
            drives: names,
        });
    }
    out.sort_by_key(|l| l.name.to_lowercase());
    Ok(out)
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct RenameLibrary {
    /// The library's name (any case) or id.
    pub library: String,
    pub name: String,
}

/// Rename a library. Drives and snapshots refer to it by id, so nothing else changes.
pub fn library_rename(store: &Store, req: &RenameLibrary) -> Result<LibraryRecord> {
    let _lock = store.lock()?;
    let lib = find_library(store, &req.library)?;
    store.rename_library(lib.id, &req.name)?;
    store.library_by_id(lib.id)?.ok_or_else(|| Error::NotFound(format!("library {}", lib.id)))
}

/// A library by name or id, or an error naming the libraries there are.
pub(super) fn find_library(store: &Store, spec: &str) -> Result<LibraryRecord> {
    match store.find_library(spec.trim())? {
        Some(l) => Ok(l),
        None => Err(Error::NotFound(format!("library {:?} in the store ({})", spec.trim(), known_libraries(store)?))),
    }
}

/// "it has Main, Backup", for errors about a library name.
pub(super) fn known_libraries(store: &Store) -> Result<String> {
    let mut names: Vec<String> = store.libraries()?.into_iter().map(|l| format!("{:?}", l.name)).collect();
    names.sort_by_key(|n| n.to_lowercase());
    Ok(match names.is_empty() {
        true => "it has no libraries yet".into(),
        false => format!("it has {}", names.join(", ")),
    })
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct LabelRequest {
    pub label: String,
    /// The snapshot to label: an id prefix or another label. Not needed with `remove`.
    #[serde(default)]
    pub snapshot: Option<String>,
    /// Take the label off instead.
    #[serde(default)]
    pub remove: bool,
}

/// Label a snapshot, or take a label off: the label added or removed.
pub fn snapshot_label(store: &Store, req: &LabelRequest) -> Result<SnapshotLabel> {
    let _lock = store.lock()?;
    if req.remove {
        return store.remove_label(&req.label);
    }
    let spec = req.snapshot.as_deref().ok_or_else(|| Error::invalid("say which snapshot to label"))?;
    let id: Hash = store.resolve_snapshot(spec.strip_prefix("snapshot:").unwrap_or(spec))?;
    store.add_label(&id, &req.label)
}

pub use crate::offsite::{CopyOptions, CopyRecord, CopyReport, CopyStatus};
pub use crate::retention::{KeepReason, KeptSnapshot, LibraryPrune, PruneOptions, PruneReport};

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct StoreCopyRequest {
    /// Where the copy goes: a folder on a NAS, another disk, a cloud-synced folder.
    pub path: std::path::PathBuf,
    #[serde(default, flatten)]
    pub options: CopyOptions,
}

/// Copy the store to another folder, or bring a copy up to date (see [`crate::offsite`]).
pub fn store_copy(store: &Store, req: &StoreCopyRequest, ctx: &Ctx<'_>) -> Result<CopyReport> {
    crate::offsite::copy(store, &req.path, &req.options, ctx)
}

/// Where the store has been copied, and whether each copy is there and up to date.
pub fn store_copies(store: &Store) -> Result<Vec<CopyStatus>> {
    crate::offsite::copies(store)
}

/// What removing the snapshots a retention policy doesn't keep would do (see
/// [`crate::retention`]): which go, why the others stay, and the files that frees. Applying it
/// removes exactly the snapshots it lists, and is refused once a library, label, drive or
/// snapshot changed since.
pub fn store_prune_plan(store: &Store, req: &PruneOptions, ctx: &Ctx<'_>) -> Result<Planned<PruneReport>> {
    // Measured first, so whatever changes while the plan is made makes it stale.
    let files = ["libraries.json", "labels.json", "drives.json", "pruned.json"].map(String::from).to_vec();
    let touched = Touched::path(store.root(), true, vec![Probe::Content(files), Probe::Listing("snapshots".into())]);
    let report = crate::retention::plan(store, req, ctx)?;
    let line = format!(
        "remove {} snapshot(s), freeing {} in {} file(s)",
        report.removed_snapshots,
        bytes(report.freed_bytes),
        report.removed_objects
    );
    let noop = report.removed_snapshots == 0;
    let warnings = report.warnings.clone();
    Ok(Planned::new("store.prune", report, ())
        .line(line)
        .warnings(warnings)
        .destructive(!noop)
        .noop(noop)
        .touches(touched))
}

impl Operation for PruneReport {
    type Request = ();
    type Report = PruneReport;

    fn apply(planned: &Planned<Self>, store: Option<&Store>, _: &Ctx<'_>) -> Result<PruneReport> {
        let store = store.ok_or_else(|| Error::invalid("pruning needs the store"))?;
        crate::retention::apply(store, &planned.plan)
    }
}

/// Plan and prune in one go (with `dry_run`, only plan), for callers that don't confirm (see
/// [`store_prune_plan`]).
pub fn store_prune(store: &Store, req: &PruneOptions, ctx: &Ctx<'_>) -> Result<PruneReport> {
    let planned = store_prune_plan(store, req, ctx)?;
    match req.dry_run {
        true => Ok(planned.plan),
        false => planned.apply(Some(store), ctx),
    }
}

/// What collecting the store's garbage would free: snapshots of no library (see [`Store::gc`]),
/// and the files nothing uses.
pub fn store_gc_plan(store: &Store) -> Result<Planned<GcReport>> {
    let report = store.gc(true)?;
    let line = format!(
        "delete {} snapshot(s) and {} file(s) nothing uses ({})",
        report.removed_snapshots,
        report.removed_objects,
        bytes(report.freed_bytes)
    );
    let noop = report.removed_objects == 0 && report.removed_snapshots == 0;
    let probes = ["libraries.json", "labels.json", "pruned.json"].iter().map(|s| s.to_string()).collect();
    Ok(Planned::new("store.gc", report, ()).line(line).destructive(!noop).noop(noop).touches(Touched::path(
        store.root(),
        true,
        vec![Probe::Content(probes), Probe::Listing("snapshots".into())],
    )))
}

impl Operation for GcReport {
    type Request = ();
    type Report = GcReport;

    fn apply(_: &Planned<Self>, store: Option<&Store>, _: &Ctx<'_>) -> Result<GcReport> {
        let store = store.ok_or_else(|| Error::invalid("collecting garbage needs the store"))?;
        let _lock = store.lock()?;
        store.gc(false)
    }
}
