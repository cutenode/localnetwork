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
//! stopped part way keeps what it finished and the next one copies the rest. Manifests are saved
//! once every file they list is there (and flushed), then libraries are updated: one this store
//! doesn't have is added with the other store's id (renamed if the name is taken here), and one it
//! has moves to the other's latest snapshot when that descends from its own. Libraries that
//! changed on both computers keep their own latest; the other's is saved beside it, for `library
//! push --snapshot`. Labels come along unless the name is taken.

mod fetch;
mod protocol;
mod serve;

use std::fmt;
use std::str::FromStr;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::hash::Hash;
use crate::{Error, Result};

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

#[derive(Debug, Clone, Default, Serialize, JsonSchema)]
pub struct FetchReport {
    /// The other computer's key, shortened.
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
mod tests {
    use super::*;
    use hegel::generators as gs;
    use hegel::{Generator, TestCase};

    /// A share code for a random key and address, with `token`.
    fn code(tc: &TestCase, token: String) -> ShareCode {
        let key: Vec<u8> = tc.draw(gs::vecs(gs::integers::<u8>()).min_size(32).max_size(32));
        let key = iroh::SecretKey::from_bytes(&key.try_into().expect("32 bytes")).public();
        let ip = std::net::Ipv4Addr::from(tc.draw(gs::integers::<u32>()));
        let port = tc.draw(gs::integers::<u16>());
        let addr = iroh::EndpointAddr::new(key).with_ip_addr(std::net::SocketAddr::from((ip, port)));
        ShareCode { ticket: addr.into(), token }
    }

    /// A share code reads back from what it prints, around any whitespace, and names the same
    /// computer; a fresh one's token is 32 hex digits.
    #[hegel::test(test_cases = 200)]
    fn share_codes_read_back(tc: TestCase) {
        let token: String = tc.draw(gs::from_regex("[0-9a-fA-F]{32}").fullmatch(true));
        let c = code(&tc, token);
        let printed = c.to_string();
        assert_eq!(format!(" {printed}\n").parse::<ShareCode>().unwrap(), c);
        assert_eq!(printed.parse::<ShareCode>().unwrap().addr(), c.addr());
        let fresh = ShareCode::new(c.addr().clone());
        assert!(fresh.token.len() == 32 && fresh.token.bytes().all(|b| b.is_ascii_hexdigit()));
        assert_eq!(fresh.to_string().parse::<ShareCode>().unwrap(), fresh);
    }

    /// A code with its token cut short, made longer, or with a letter that isn't hex, or with
    /// its ticket mangled, isn't read as one; whatever text does read as a code prints back as
    /// a code with the same token.
    #[hegel::test(test_cases = 300)]
    fn mangled_share_codes_are_refused(tc: TestCase) {
        let c = code(&tc, Uuid::new_v4().simple().to_string());
        let printed = c.to_string();
        let (ticket, token) = printed.rsplit_once('-').unwrap();
        let cut = tc.draw(gs::integers::<usize>().min_value(1).max_value(31));
        let junk = tc.draw(gs::sampled_from(vec!['g', 'z', '-', ' ', 'é']));
        let at = tc.draw(gs::integers::<usize>().max_value(31));
        let mut bad_letter: Vec<char> = token.chars().collect();
        bad_letter[at] = junk;
        let bad_letter: String = bad_letter.into_iter().collect();
        for bad in [
            format!("{ticket}-{}", &token[..cut]),
            format!("{ticket}-{token}0"),
            format!("{ticket}-{bad_letter}"),
            format!("{ticket}{token}"),
            format!("-{token}"),
            format!("x{ticket}-{token}"),
        ] {
            assert!(bad.parse::<ShareCode>().is_err(), "{bad:?} was read as a code");
        }
        let padded = format!("{}{printed}{}", " ".repeat(cut % 3), "\n".repeat(at % 2));
        let text: String = tc.draw(hegel::one_of!(
            gs::text().max_size(80),
            gs::just(padded),
            gs::integers::<usize>().max_value(printed.len()).map(move |n| printed[..n].to_string()),
        ));
        if let Ok(read) = text.parse::<ShareCode>() {
            let token = text.trim().rsplit_once('-').unwrap().1;
            assert_eq!(read.token, token);
            assert_eq!(read.to_string().parse::<ShareCode>().unwrap(), read);
        }
    }
}
