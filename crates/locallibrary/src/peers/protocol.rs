//! `locallibrary/store/1`: each request is a stream of its own, the request as JSON and then a
//! response of a status byte and its body.
//!
//! - `hello` with the share code's token, first on a connection: the [`Catalog`] as JSON. A wrong
//!   token is refused and the connection closed; nothing else is answered before a right one.
//! - `snapshot` with an id: the manifest as it is in the store (`snapshots/<id>.json`).
//! - `object` with a hash: the stored file's bytes.

use serde::{Deserialize, Serialize};
use tokio::io::AsyncWriteExt;
use uuid::Uuid;

use crate::hash::Hash;
use crate::store::{LibraryRecord, SnapshotLabel};
use crate::{Error, Result};

pub(super) const ALPN: &[u8] = b"locallibrary/store/1";

pub(super) const OK: u8 = 0;
pub(super) const NOT_FOUND: u8 = 1;
pub(super) const REFUSED: u8 = 2;

/// The largest request: a hash or a token in a little JSON.
const REQUEST_LIMIT: usize = 4096;
/// The largest catalog or manifest (a manifest lists every file of a library).
pub(super) const JSON_LIMIT: usize = 512 << 20;

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(super) enum Request {
    Hello { token: String },
    Snapshot { id: Hash },
    Object { hash: Hash },
}

/// What a store holds, as the other computer sees it.
#[derive(Debug, Serialize, Deserialize)]
pub(super) struct Catalog {
    pub(super) store: Uuid,
    pub(super) libraries: Vec<LibraryRecord>,
    /// Every snapshot of those libraries, with its library and parents.
    pub(super) snapshots: Vec<CatalogSnapshot>,
    pub(super) labels: Vec<SnapshotLabel>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct CatalogSnapshot {
    pub(super) id: Hash,
    pub(super) library: Uuid,
    pub(super) parents: Vec<Hash>,
}

pub(super) fn net(e: impl std::fmt::Display) -> Error {
    Error::Peer(format!("the connection to the other computer failed: {e}"))
}

pub(super) async fn read_request(recv: &mut iroh::endpoint::RecvStream) -> Result<Request> {
    let bytes = recv.read_to_end(REQUEST_LIMIT).await.map_err(net)?;
    serde_json::from_slice(&bytes).map_err(|e| Error::Peer(format!("a request that isn't one: {e}")))
}

/// Send `request`, and read its status (the body follows on `recv`).
pub(super) async fn ask(
    conn: &iroh::endpoint::Connection,
    request: &Request,
) -> Result<(u8, iroh::endpoint::RecvStream)> {
    let (mut send, mut recv) = conn.open_bi().await.map_err(net)?;
    send.write_all(&serde_json::to_vec(request).expect("serializes")).await.map_err(net)?;
    send.finish().map_err(net)?;
    let mut status = [0u8; 1];
    recv.read_exact(&mut status).await.map_err(net)?;
    Ok((status[0], recv))
}

/// Answer with `status` and `body`.
pub(super) async fn answer(send: &mut iroh::endpoint::SendStream, status: u8, body: &[u8]) -> Result<()> {
    send.write_all(&[status]).await.map_err(net)?;
    send.write_all(body).await.map_err(net)?;
    send.flush().await.map_err(net)?;
    send.finish().map_err(net)
}

#[cfg(test)]
mod tests;
