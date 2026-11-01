//! The fetching end: copies what another computer's store has that this one lacks, then updates
//! the libraries (see the [module docs](super)).

use std::io::Write;

use iroh::endpoint::Connection;

use super::protocol::{self, ALPN, Catalog, JSON_LIMIT, OK, Request, ask};
use super::{FetchOptions, FetchReport, ShareCode, endpoint, runtime, short};
use crate::catalog;
use crate::error::IoContext;
use crate::fsutil;
use crate::hash::Hash;
use crate::progress::{Ctx, Event, Phase};
use crate::store::Store;
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
    let chosen = catalog::choose(&catalog, &opts.libraries)?;

    // The snapshots this store lacks, checked against their ids.
    let missing = catalog::lacking(store, &catalog, &chosen)?;
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
        manifests.push(catalog::checked(id, &bytes).map_err(Error::Peer)?);
    }
    ctx.phase_end(Phase::Scan);

    // The stored files they list that this store lacks.
    let (wanted, present) = catalog::needed(store, &manifests);
    report.snapshots = manifests.len();
    report.files = wanted.len();
    report.bytes = wanted.values().sum();
    report.present = present;
    if opts.dry_run {
        report.libraries = catalog::settle(store, &catalog, &chosen, manifests, true)?;
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
    report.libraries = catalog::settle(store, &catalog, &chosen, manifests, false)?;
    Ok(report)
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
    fsutil::replace(tmp.into_temp_path(), &dst)
}
