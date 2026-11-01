//! Copying snapshots from another computer's store over the network, so a library on one
//! computer can be had on another without a drive in between.
//!
//! One computer runs [`serve`], which prints a share code and answers until it's stopped; the
//! other runs [`fetch`] with that code. The code holds the serving computer's iroh address (its
//! public key, and where it can be reached) and a random token made for that run: only someone
//! with the code can read the store, and only while it's served. Connections are iroh's QUIC
//! ones, encrypted and authenticated by that key. They go directly between the computers when they
//! can (on the same network, or through a NAT), and through n0's public relays otherwise, unless
//! `local_only` keeps them on the local network.
//!
//! The protocol (`locallibrary/store/1`) is small: the first request names the
//! token and gets the store's catalog (its libraries, their snapshots and labels), then each
//! snapshot manifest and stored file is asked for by hash, one stream each. Serving only reads
//! the store, and only those: not the account session, settings or anything else in it.
//!
//! Fetching copies the snapshots the store lacks and the files they list that it lacks, each
//! hashed as it arrives and refused unless its BLAKE3 hash is the one asked for, so a fetch
//! stopped part way keeps what it finished and the next one copies the rest. Then manifests,
//! libraries and labels are saved as [`crate::catalog`] says.

mod fetch;
mod protocol;
mod serve;

use std::fmt;
use std::str::FromStr;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{Error, Result};

pub use crate::catalog::{FetchReport, FetchedLibrary, LibraryOutcome};
pub use fetch::fetch;
pub use serve::serve;

/// What [`serve`] prints for the other computer: its iroh ticket and the run's token, as
/// `<ticket>-<token>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShareCode {
    ticket: iroh_tickets::endpoint::EndpointTicket,
    token: String,
}

impl ShareCode {
    pub(crate) fn new(addr: iroh::EndpointAddr) -> ShareCode {
        ShareCode { ticket: addr.into(), token: Uuid::new_v4().simple().to_string() }
    }

    fn addr(&self) -> &iroh::EndpointAddr {
        self.ticket.endpoint_addr()
    }
}

impl fmt::Display for ShareCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}-{}", self.ticket, self.token)
    }
}

impl FromStr for ShareCode {
    type Err = Error;

    fn from_str(s: &str) -> Result<ShareCode> {
        let bad = || Error::invalid("that isn't a share code from `locallibrary store serve`");
        let (ticket, token) = s.trim().rsplit_once('-').ok_or_else(bad)?;
        if token.len() != 32 || !token.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(bad());
        }
        Ok(ShareCode { ticket: ticket.parse().map_err(|_| bad())?, token: token.to_string() })
    }
}

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct ServeOptions {
    /// Stay on the local network: no relays, so only computers that can reach this one directly
    /// can fetch.
    #[serde(default)]
    pub local_only: bool,
}

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct FetchOptions {
    /// Libraries to fetch, by name or id on the other computer. Empty: all of them.
    #[serde(default)]
    pub libraries: Vec<String>,
    /// Stay on the local network: no relays.
    #[serde(default)]
    pub local_only: bool,
    /// Say what would be copied, and copy nothing.
    #[serde(default)]
    pub dry_run: bool,
}

/// `store.fetch`: the code to fetch with, and what to fetch.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct FetchRequest {
    /// The share code the other computer's `store.serve` gave (its `ready` event).
    pub code: String,
    #[serde(flatten)]
    pub options: FetchOptions,
}

/// What serving is doing, as it happens.
#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum ServeEvent {
    /// Serving: give `code` to the other computer.
    Ready { code: String },
    /// A computer with the code connected (`peer`: its key, shortened).
    Connected { peer: String },
    /// A computer without the right code was turned away.
    Refused { peer: String },
    /// A computer that fetched disconnected, having been sent this much.
    Done { peer: String, snapshots: usize, files: usize, bytes: u64 },
}

#[derive(Debug, Clone, Default, Serialize, JsonSchema)]
pub struct ServeReport {
    /// Computers that fetched.
    pub peers: usize,
    pub files: usize,
    pub bytes: u64,
}

/// An endpoint for this end: on the local network only, or reaching out through n0's relays and
/// address lookup.
async fn endpoint(local_only: bool, alpns: Vec<Vec<u8>>) -> Result<iroh::Endpoint> {
    use iroh::endpoint::presets;
    let bound = match local_only {
        true => iroh::Endpoint::builder(presets::Minimal).alpns(alpns).bind().await,
        false => iroh::Endpoint::builder(presets::N0).alpns(alpns).bind().await,
    };
    bound.map_err(|e| Error::Peer(format!("couldn't start networking: {e}")))
}

/// A runtime for the network side; the rest of locallibrary is synchronous.
fn runtime() -> Result<tokio::runtime::Runtime> {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|e| Error::Peer(format!("couldn't start networking: {e}")))
}

fn short(id: &iroh::EndpointId) -> String {
    id.fmt_short().to_string()
}

#[cfg(test)]
mod tests;
