// paths: fuzz/fuzz_targets/peers.rs
// paths: fuzz/seeds/peers/**
// paths: crates/locallibrary/src/catalog/wire.rs
// paths: crates/locallibrary/src/catalog.rs
// paths: crates/locallibrary/src/store.rs
// paths: crates/locallibrary/src/hash.rs
// coverage: crates/locallibrary/src/catalog/wire.rs
//! What another computer sends over the store protocol: the request a serving computer reads, and
//! the catalog and snapshot manifests a fetching one reads.

#![no_main]

use libfuzzer_sys::fuzz_target;
use locallibrary::store::Snapshot;
use locallibrary::testing::{PeerRequest, peer_catalog, peer_snapshot, snapshot_id};

fuzz_target!(|data: &[u8]| {
    let _ = PeerRequest::parse(data);
    let _ = peer_catalog(data);
    let Ok(mut snap) = serde_json::from_slice::<Snapshot>(data) else { return };
    // A manifest checks only as the snapshot its content hashes to, whatever id it claims.
    if let Ok(read) = peer_snapshot(&snap.id, data) {
        assert_eq!(snapshot_id(&read), snap.id);
    }
    // The hash check refuses nearly every input, so the manifest goes on as a store writes it,
    // with its own id: what a fetch then reads must check.
    snap.id = snapshot_id(&snap);
    let written = serde_json::to_vec(&snap).expect("serializes");
    peer_snapshot(&snap.id, &written).expect("a manifest with its own id checks");
});
