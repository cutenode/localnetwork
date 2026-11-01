//! What a store holds, as one fetching from it sees it (its libraries, their snapshots and labels:
//! a catalog), and fetching from it: which snapshots this store lacks, the stored files they
//! list that it lacks, and then saving them and what becomes of each library. Another computer's
//! store ([`crate::peers`]) and a plugin's copy of this one ([`crate::offsite::restore`]) are
//! fetched from alike.
//!
//! Manifests are saved once every file they list is here (and flushed), then libraries are
//! updated: one this store doesn't have is added with the other store's id (renamed if the name
//! is taken here), and one it has moves to the other's latest snapshot when that descends from its
//! own. Libraries that changed on both keep their own latest; the other's is saved beside it, for
//! `library push --snapshot`. Labels come along unless the name is taken.

use indexmap::IndexMap;
use rustc_hash::{FxHashMap, FxHashSet};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::hash::Hash;
use crate::store::{LibraryRecord, Snapshot, SnapshotLabel, Store};
use crate::{Error, Result};

/// What a store holds, as another sees it.
#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct Catalog {
    pub(crate) store: Uuid,
    pub(crate) libraries: Vec<LibraryRecord>,
    /// Every snapshot of those libraries, with its library and parents.
    pub(crate) snapshots: Vec<CatalogSnapshot>,
    pub(crate) labels: Vec<SnapshotLabel>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct CatalogSnapshot {
    pub(crate) id: Hash,
    pub(crate) library: Uuid,
    pub(crate) parents: Vec<Hash>,
}

/// The snapshots of `store`'s libraries, each with its library and parents.
pub(crate) fn snapshots(store: &Store) -> Result<Vec<CatalogSnapshot>> {
    let ids: FxHashSet<Uuid> = store.libraries()?.iter().map(|l| l.id).collect();
    let mut out = Vec::new();
    for id in store.snapshot_ids()? {
        let info = store.load_info(&id)?;
        if ids.contains(&info.library_id) {
            out.push(CatalogSnapshot { id, library: info.library_id, parents: info.parents });
        }
    }
    Ok(out)
}

/// What another sees of `store`.
#[cfg(feature = "peers")]
pub(crate) fn of(store: &Store) -> Result<Catalog> {
    Ok(Catalog {
        store: store.id()?,
        libraries: store.libraries()?,
        snapshots: snapshots(store)?,
        labels: store.labels()?,
    })
}

#[derive(Debug, Clone, Default, Serialize, JsonSchema)]
pub struct FetchReport {
    /// Where it was fetched from: the other computer's key, shortened, or a plugin's store
    /// destination (`<plugin>:<destination>`).
    pub peer: String,
    pub libraries: Vec<FetchedLibrary>,
    pub snapshots: usize,
    /// Stored files copied, and the store had already.
    pub files: usize,
    pub bytes: u64,
    pub present: usize,
    pub dry_run: bool,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct FetchedLibrary {
    /// Its name in this store.
    pub name: String,
    /// Its name on the other computer, when that was taken here.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub was: Option<String>,
    pub id: Uuid,
    /// The other computer's latest snapshot.
    pub head: Option<Hash>,
    pub outcome: LibraryOutcome,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LibraryOutcome {
    /// This store didn't have it.
    Added,
    /// It moved to the other computer's newer latest snapshot.
    Updated,
    /// This store was as new, or newer.
    UpToDate,
    /// Both changed: this store keeps its latest, and has the other's beside it.
    Diverged,
    /// The other computer's latest snapshot couldn't be had (removed there during the fetch, or
    /// missing from its store): the library is left as it was.
    Unavailable,
}

/// The libraries `names` choose (all when empty), by name or id.
pub(crate) fn choose(catalog: &Catalog, names: &[String]) -> Result<Vec<LibraryRecord>> {
    if names.is_empty() {
        return Ok(catalog.libraries.clone());
    }
    names
        .iter()
        .map(|n| {
            catalog
                .libraries
                .iter()
                .find(|l| l.name.eq_ignore_ascii_case(n) || l.id.to_string() == *n)
                .cloned()
                .ok_or_else(|| {
                    let known: Vec<&str> = catalog.libraries.iter().map(|l| l.name.as_str()).collect();
                    Error::NotFound(format!("library {n:?} on the other computer (it has {})", known.join(", ")))
                })
        })
        .collect()
}

/// The snapshots of `chosen` libraries `store` lacks. Ones it pruned stay pruned.
pub(crate) fn lacking(store: &Store, catalog: &Catalog, chosen: &[LibraryRecord]) -> Result<Vec<Hash>> {
    let ids: FxHashSet<Uuid> = chosen.iter().map(|l| l.id).collect();
    let pruned = store.pruned()?;
    let lacks = |s: &&CatalogSnapshot| {
        ids.contains(&s.library) && !store.snapshot_path(&s.id).is_file() && !pruned.contains_key(&s.id)
    };
    Ok(catalog.snapshots.iter().filter(lacks).map(|s| s.id).collect())
}

/// A snapshot's manifest as it came, once it reads and is the snapshot `id`; otherwise why not.
pub(crate) fn checked(id: &Hash, bytes: &[u8]) -> std::result::Result<Snapshot, String> {
    let snap: Snapshot =
        serde_json::from_slice(bytes).map_err(|e| format!("snapshot {} can't be read: {e}", id.short()))?;
    match snap.id == *id && snap.compute_id() == *id {
        true => Ok(snap),
        false => Err(format!("snapshot {} came back damaged", id.short())),
    }
}

/// The stored files `manifests` list that `store` lacks (with their sizes), and how many it has.
pub(crate) fn needed(store: &Store, manifests: &[Snapshot]) -> (IndexMap<Hash, u64>, usize) {
    let mut wanted: IndexMap<Hash, u64> = IndexMap::new();
    let mut present: FxHashSet<Hash> = FxHashSet::default();
    for f in manifests.iter().flat_map(|s| &s.files) {
        let Some(h) = f.hash else { continue };
        match store.holds(&h, f.size) {
            true => {
                present.insert(h);
            }
            false => {
                wanted.insert(h, f.size);
            }
        }
    }
    (wanted, present.len())
}

/// With every file `manifests` list in `store` and flushed: saves them, updates the libraries and
/// brings the labels (see the module docs). With `dry_run`, only says what becomes of each
/// library.
pub(crate) fn settle(
    store: &Store,
    catalog: &Catalog,
    chosen: &[LibraryRecord],
    manifests: Vec<Snapshot>,
    dry_run: bool,
) -> Result<Vec<FetchedLibrary>> {
    let fetched: FxHashSet<Hash> = manifests.iter().map(|s| s.id).collect();
    if dry_run {
        return outcomes(store, catalog, chosen, &fetched, &mut |_| Ok(()));
    }
    for snap in manifests {
        store.save_snapshot(snap)?;
    }
    let libraries = outcomes(store, catalog, chosen, &fetched, &mut |lib| apply(store, lib))?;
    for label in &catalog.labels {
        if store.snapshot_path(&label.snapshot).is_file() && store.find_label(&label.name)?.is_none() {
            store.add_label(&label.snapshot, &label.name)?;
        }
    }
    Ok(libraries)
}

/// What becomes of each library fetched, with `apply` called for those that change.
fn outcomes(
    store: &Store,
    catalog: &Catalog,
    chosen: &[LibraryRecord],
    fetched: &FxHashSet<Hash>,
    apply: &mut dyn FnMut(&FetchedLibrary) -> Result<()>,
) -> Result<Vec<FetchedLibrary>> {
    let parents: FxHashMap<Hash, Vec<Hash>> = catalog.snapshots.iter().map(|s| (s.id, s.parents.clone())).collect();
    let mut out = Vec::new();
    for remote in chosen {
        let local = store.library_by_id(remote.id)?;
        let had = |h: &Hash| store.snapshot_path(h).is_file() || fetched.contains(h);
        let outcome = match (local.as_ref().and_then(|l| l.head), remote.head) {
            (_, Some(r)) if !had(&r) => LibraryOutcome::Unavailable,
            _ if local.is_none() => LibraryOutcome::Added,
            (_, None) => LibraryOutcome::UpToDate,
            (None, Some(_)) => LibraryOutcome::Updated,
            (Some(l), Some(r)) if l == r || descends(store, &parents, l, r)? => LibraryOutcome::UpToDate,
            (Some(l), Some(r)) if descends(store, &parents, r, l)? => LibraryOutcome::Updated,
            _ => LibraryOutcome::Diverged,
        };
        let name = match (&local, outcome) {
            (Some(l), _) => l.name.clone(),
            (None, _) => store.unused_library_name(&remote.name)?,
        };
        let lib = FetchedLibrary {
            was: (local.is_none() && name != remote.name).then(|| remote.name.clone()),
            name,
            id: remote.id,
            head: remote.head,
            outcome,
        };
        if matches!(outcome, LibraryOutcome::Added | LibraryOutcome::Updated) {
            apply(&lib)?;
        }
        out.push(lib);
    }
    Ok(out)
}

/// Record a library that's added, or moves to the other computer's latest snapshot.
fn apply(store: &Store, lib: &FetchedLibrary) -> Result<()> {
    match lib.outcome {
        LibraryOutcome::Added => store.add_library(lib.id, &lib.name, lib.head),
        _ => match lib.head {
            Some(head) => store.set_head(lib.id, head),
            None => Ok(()),
        },
    }
}

/// Whether `descendant` is `ancestor` or has it among its parents, through both stores' snapshots
/// (this store's first, then the catalog's), and those this store pruned.
fn descends(store: &Store, catalog: &FxHashMap<Hash, Vec<Hash>>, descendant: Hash, ancestor: Hash) -> Result<bool> {
    let pruned = store.pruned()?;
    let mut seen = FxHashSet::default();
    let mut queue = vec![descendant];
    while let Some(id) = queue.pop() {
        if id == ancestor {
            return Ok(true);
        }
        if !seen.insert(id) {
            continue;
        }
        let parents = match store.load_info(&id) {
            Ok(info) => info.parents,
            Err(Error::NotFound(_)) => match (catalog.get(&id), pruned.get(&id)) {
                (Some(p), _) => p.clone(),
                (None, Some(p)) => p.parents.clone(),
                (None, None) => Vec::new(),
            },
            Err(e) => return Err(e),
        };
        queue.extend(parents);
    }
    Ok(false)
}

#[cfg(test)]
mod tests;
