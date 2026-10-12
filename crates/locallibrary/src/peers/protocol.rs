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
mod tests {
    use super::*;
    use hegel::TestCase;
    use hegel::generators as gs;

    fn hash(tc: &TestCase) -> Hash {
        Hash::of(&tc.draw(gs::binary().max_size(8)))
    }

    /// Every request reads back from the JSON it's sent as, tagged with its type, and fits in the
    /// request limit; text that isn't one of the three is refused.
    #[hegel::test(test_cases = 300)]
    fn requests_read_back(tc: TestCase) {
        let request = match tc.draw(gs::integers::<u8>().max_value(2)) {
            0 => Request::Hello { token: tc.draw(gs::text().max_size(64)) },
            1 => Request::Snapshot { id: hash(&tc) },
            _ => Request::Object { hash: hash(&tc) },
        };
        let bytes = serde_json::to_vec(&request).unwrap();
        assert!(bytes.len() <= REQUEST_LIMIT);
        let back: Request = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(serde_json::to_vec(&back).unwrap(), bytes);
        let tag = serde_json::from_slice::<serde_json::Value>(&bytes).unwrap()["type"].clone();
        let want = match &request {
            Request::Hello { .. } => "hello",
            Request::Snapshot { .. } => "snapshot",
            Request::Object { .. } => "object",
        };
        assert_eq!(tag, want);
        // Only a request of a known type with what it names reads as one.
        let h = hash(&tc).to_hex();
        let (junk, ok) = tc.draw(gs::sampled_from(vec![
            (r#"{"type":"hello","token":"x"}"#.to_string(), true),
            (r#"{"type":"hello"}"#.to_string(), false),
            (format!(r#"{{"type":"object","hash":"{h}"}}"#), true),
            (format!(r#"{{"type":"snapshot","id":"{h}"}}"#), true),
            (format!(r#"{{"type":"snapshot","hash":"{h}"}}"#), false),
            (format!(r#"{{"type":"object","hash":"{}"}}"#, &h[1..]), false),
            (format!(r#"{{"type":"files","hash":"{h}"}}"#), false),
            ("hello".to_string(), false),
        ]));
        assert_eq!(serde_json::from_str::<Request>(&junk).is_ok(), ok, "{junk}");
    }

    /// A catalog reads back from its JSON: the same store, libraries, snapshots and labels.
    #[hegel::test(test_cases = 100)]
    fn catalogs_read_back(tc: TestCase) {
        let at =
            chrono::DateTime::from_timestamp(tc.draw(gs::integers::<i64>().min_value(0).max_value(4_000_000_000)), 0)
                .unwrap();
        let n = tc.draw(gs::integers::<usize>().max_value(3));
        let libraries: Vec<LibraryRecord> = (0..n)
            .map(|i| LibraryRecord {
                id: Uuid::from_u128(tc.draw(gs::integers::<u128>())),
                name: format!("Library {i}"),
                head: tc.draw(gs::booleans()).then(|| hash(&tc)),
                created: at,
                updated: at,
            })
            .collect();
        let snapshots: Vec<CatalogSnapshot> = libraries
            .iter()
            .filter_map(|l| l.head.map(|id| CatalogSnapshot { id, library: l.id, parents: vec![hash(&tc)] }))
            .collect();
        let labels: Vec<SnapshotLabel> =
            snapshots.iter().map(|s| SnapshotLabel { name: "gig".into(), snapshot: s.id, created: at }).collect();
        let catalog = Catalog { store: Uuid::from_u128(tc.draw(gs::integers::<u128>())), libraries, snapshots, labels };
        let bytes = serde_json::to_vec(&catalog).unwrap();
        let back: Catalog = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(back.store, catalog.store);
        assert_eq!(serde_json::to_vec(&back).unwrap(), bytes);
        assert!(bytes.len() < JSON_LIMIT);
    }
}
