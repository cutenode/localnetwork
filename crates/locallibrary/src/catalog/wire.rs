//! The JSON another computer's store sends over `locallibrary/store/1` (the `peers` feature's
//! protocol): the requests a serving computer reads, the catalog it answers `hello` with, and the
//! snapshot manifests it answers with. They're plain serde, so they build without the network code.

use serde::{Deserialize, Serialize};

use super::Catalog;
use crate::hash::Hash;
use crate::store::Snapshot;
use crate::{Error, Result};

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Request {
    Hello { token: String },
    Snapshot { id: Hash },
    Object { hash: Hash },
}

impl Request {
    /// The request another computer sent, as JSON.
    pub fn parse(bytes: &[u8]) -> Result<Request> {
        serde_json::from_slice(bytes).map_err(|e| Error::Peer(format!("a request that isn't one: {e}")))
    }
}

/// The catalog another computer answered `hello` with, as JSON.
pub(crate) fn catalog(bytes: &[u8]) -> Result<Catalog> {
    serde_json::from_slice(bytes).map_err(|e| Error::Peer(format!("the other computer's catalog can't be read: {e}")))
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
