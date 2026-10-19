//! The fetching end: copies what another computer's store has that this one lacks, then updates
//! the libraries (see the [module docs](super)).

use indexmap::IndexMap;
use rustc_hash::{FxHashMap, FxHashSet};
use std::io::Write;

use iroh::endpoint::Connection;
use uuid::Uuid;

use super::protocol::{self, ALPN, Catalog, JSON_LIMIT, OK, Request, ask};
use super::{FetchOptions, FetchReport, FetchedLibrary, LibraryOutcome, ShareCode, endpoint, runtime, short};
use crate::error::IoContext;
use crate::fsutil;
use crate::hash::Hash;
use crate::progress::{Ctx, Event, Phase};
use crate::store::{LibraryRecord, Snapshot, Store};
use crate::{Error, Result};

/// Stored files asked for at once.
const PARALLEL: usize = 8;

/// Fetch from the computer serving `code` into `store`.
pub fn fetch(store: &Store, code: &ShareCode, opts: &FetchOptions, ctx: &Ctx<'_>) -> Result<FetchReport> {
    let _lock = if opts.dry_run { None } else { Some(store.lock()?) };
    runtime()?.block_on(async {
        let endpoint = endpoint(opts.local_only, Vec::new()).await?;
        let conn = endpoint.connect(code.addr().clone(), ALPN).await.map_err(|e| {
            Error::Peer(format!(
                "couldn't reach the other computer (is `locallibrary store serve` still running there?): {e}"
            ))
        })?;
        let report = fetch_from(store, &conn, code, opts, ctx).await;
        conn.close(0u32.into(), b"done");
        endpoint.close().await;
        report
    })
}

async fn fetch_from(
    store: &Store,
    conn: &Connection,
    code: &ShareCode,
    opts: &FetchOptions,
    ctx: &Ctx<'_>,
) -> Result<FetchReport> {
    let mut report = FetchReport { peer: short(&conn.remote_id()), dry_run: opts.dry_run, ..Default::default() };
    let (status, mut recv) = ask(conn, &Request::Hello { token: code.token.clone() }).await?;
    match status {
        OK => {}
        protocol::REFUSED => {
            return Err(Error::Peer(
                "the other computer refused the share code: run `locallibrary store serve` there again, and use the \
                 code it prints"
                    .into(),
            ));
        }
        _ => {
            let why = String::from_utf8_lossy(&recv.read_to_end(4096).await.unwrap_or_default()).into_owned();
            return Err(Error::Peer(format!("the other computer couldn't read its store: {why}")));
        }
    }
    let catalog: Catalog = serde_json::from_slice(&recv.read_to_end(JSON_LIMIT).await.map_err(protocol::net)?)
        .map_err(|e| Error::Peer(format!("the other computer's catalog can't be read: {e}")))?;
    if catalog.store == store.id()? {
        return Err(Error::invalid("that share code is this store's own"));
    }
    let chosen = choose(&catalog, &opts.libraries)?;
    let ids: FxHashSet<Uuid> = chosen.iter().map(|l| l.id).collect();

    // The snapshots this store lacks, checked against their ids. Ones it pruned stay pruned.
    let pruned = store.pruned()?;
    let missing: Vec<Hash> = catalog
        .snapshots
        .iter()
        .filter(|s| ids.contains(&s.library) && !store.snapshot_path(&s.id).is_file() && !pruned.contains_key(&s.id))
        .map(|s| s.id)
        .collect();
    ctx.phase_start(Phase::Scan, missing.len() as u64, 0);
    let mut manifests = Vec::new();
    for id in &missing {
        ctx.check()?;
        ctx.emit(Event::Item { phase: Phase::Scan, path: id.short(), bytes: 0 });
        let (status, mut recv) = ask(conn, &Request::Snapshot { id: *id }).await?;
        if status != OK {
            // Pruned there since the catalog was sent.
            continue;
        }
        let bytes = recv.read_to_end(JSON_LIMIT).await.map_err(protocol::net)?;
        let snap: Snapshot = serde_json::from_slice(&bytes)
            .map_err(|e| Error::Peer(format!("snapshot {} can't be read: {e}", id.short())))?;
        if snap.id != *id || snap.compute_id() != *id {
            return Err(Error::Peer(format!("snapshot {} came back damaged", id.short())));
        }
        manifests.push(snap);
    }
    ctx.phase_end(Phase::Scan);

    // The stored files they list that this store lacks.
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
    report.snapshots = manifests.len();
    let fetched_snapshots: FxHashSet<Hash> = manifests.iter().map(|s| s.id).collect();
    report.files = wanted.len();
    report.bytes = wanted.values().sum();
    report.present = present.len();
    if opts.dry_run {
        report.libraries = outcomes(store, &catalog, &chosen, &fetched_snapshots, &mut |_| Ok(()))?;
        return Ok(report);
    }

    ctx.phase_start(Phase::Copy, wanted.len() as u64, report.bytes);
    let mut queue = wanted.iter().map(|(h, s)| (*h, *s));
    let mut fetching = tokio::task::JoinSet::new();
    let mut fetched = Vec::new();
    loop {
        while fetching.len() < PARALLEL
            && let Some((hash, size)) = queue.next()
        {
            let (conn, store) = (conn.clone(), store.clone());
            fetching.spawn(async move {
                get_object(&conn, &store, hash, size).await.map(|_| (hash, size)).map_err(|e| store.refusing(e))
            });
        }
        let Some(done) = fetching.join_next().await else { break };
        // What was copied before a failure is flushed, so the next fetch can trust it.
        let (hash, size) = match done.map_err(|e| Error::Peer(e.to_string())).and_then(|r| r) {
            Ok(got) => got,
            Err(e) => {
                store.flush_objects(&fetched)?;
                return Err(e);
            }
        };
        ctx.emit(Event::Item { phase: Phase::Copy, path: hash.short(), bytes: size });
        fetched.push(hash);
        if let Err(e) = ctx.check() {
            store.flush_objects(&fetched)?;
            return Err(e.into());
        }
    }
    store.flush_objects(&fetched)?;
    ctx.phase_end(Phase::Copy);

    // Manifests once every file they list is here, then the libraries and labels.
    for snap in manifests {
        store.save_snapshot(snap)?;
    }
    report.libraries = outcomes(store, &catalog, &chosen, &fetched_snapshots, &mut |lib| apply(store, lib))?;
    for label in &catalog.labels {
        if store.snapshot_path(&label.snapshot).is_file() && store.find_label(&label.name)?.is_none() {
            store.add_label(&label.snapshot, &label.name)?;
        }
    }
    Ok(report)
}

/// The libraries `names` choose (all when empty), by name or id.
fn choose(catalog: &Catalog, names: &[String]) -> Result<Vec<LibraryRecord>> {
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

/// Copy one stored file into the store, hashed on the way and refused unless it's `hash`.
async fn get_object(conn: &Connection, store: &Store, hash: Hash, size: u64) -> Result<()> {
    let (status, mut recv) = ask(conn, &Request::Object { hash }).await?;
    if status != OK {
        return Err(Error::Peer(format!(
            "the other computer is missing a stored file a snapshot lists ({})",
            hash.short()
        )));
    }
    let dir = store.root().join("objects").join("tmp");
    std::fs::create_dir_all(&dir).at(&dir)?;
    let mut tmp = fsutil::temp_in(tempfile::Builder::new().prefix("fetch-"), &dir)?;
    let mut hasher = blake3::Hasher::new();
    let (mut n, mut buf) = (0u64, vec![0u8; 1 << 16]);
    while let Some(read) = recv.read(&mut buf).await.map_err(protocol::net)? {
        n += read as u64;
        if n > size {
            break;
        }
        hasher.update(&buf[..read]);
        tmp.write_all(&buf[..read]).at(tmp.path())?;
    }
    let got = Hash::from(hasher.finalize());
    if n != size || got != hash {
        return Err(Error::Peer(format!("a stored file came back damaged ({})", hash.short())));
    }
    let dst = store.object_path(&hash);
    let parent = dst.parent().expect("object path has parent");
    std::fs::create_dir_all(parent).at(parent)?;
    tmp.persist(&dst).map_err(|e| Error::Io { path: dst.clone(), source: e.error })?;
    Ok(())
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
