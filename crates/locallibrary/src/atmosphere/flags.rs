//! Asking Local Network's website which feature flags are on, for [`crate::flags`].
//!
//! The public request goes first, with a short limit. Besides the flags' public gates, it proves
//! the network works before the session is touched: a session refresh spends a single-use token,
//! so that step must never be cancelled by a deadline, and running it only after the website has
//! just answered keeps a dead network from hanging a command. Then, when the session may, the same
//! request with a service token says which flags target this account.

use std::path::Path;
use std::time::Duration;

use chrono::{DateTime, Utc};
use jacquard_oauth::scopes::Scope;
use reqwest::StatusCode;
use serde::Deserialize;

use super::directory::{service_did, service_token, website};
use super::{AccountInfo, block_on, grants, net, status, store};
use crate::error::Reason;
use crate::flags::{CACHE_FILE, Cache, Flag, PUBLIC_TIMEOUT};

/// The website's method that says which flags are on. It takes a token, and answers without one.
pub const GET_FLAGS: &str = "computer.localnetwork.flags.getFlags";

/// Longer than [`PUBLIC_TIMEOUT`]: the website looks up the account's signing key to check a token.
const TOKEN_TIMEOUT: Duration = Duration::from_secs(5);

/// Whether the session may ask about targeting: one signed in before [`super::FLAGS_SCOPE`]
/// wasn't given it, and those last up to two weeks.
pub fn may_ask(info: &AccountInfo) -> bool {
    grants(info, [Scope::rpc(GET_FLAGS)])
}

/// Why the website gave no answer.
enum NoAnswer {
    /// It didn't take the token.
    TokenRefused,
    /// Anything else: it can't be reached, it's failing, its answer doesn't read.
    Failed,
}

/// What the request with a token came to.
enum Targeting {
    /// What the website says for this account.
    Answered(Vec<Flag>),
    /// The session or its token is no good: nothing targets this account as far as we know.
    Refused,
    /// There's no telling this time (the account lock is taken, a server didn't answer), which
    /// says nothing about the account: what was said of it last still stands.
    Unknown,
}

#[derive(Deserialize)]
struct Answer {
    flags: Vec<Flag>,
}

/// An answer's flags. A percentage outside 0 to 100 means the website and this code disagree on
/// what the numbers are, so the whole answer goes instead of a guess at what was meant.
fn parse(body: &[u8]) -> Option<Vec<Flag>> {
    let answer: Answer = serde_json::from_slice(body).ok()?;
    answer.flags.iter().all(|flag| flag.gates.percentage <= 100).then_some(answer.flags)
}

fn fetch(website: &str, token: Option<&str>, limit: Duration) -> Result<Vec<Flag>, NoAnswer> {
    block_on(async {
        let client = net::client().map_err(|_| NoAnswer::Failed)?;
        let mut request = client.get(format!("{website}/xrpc/{GET_FLAGS}")).timeout(limit);
        if let Some(token) = token {
            request = request.bearer_auth(token);
        }
        let response = request.send().await.map_err(|_| NoAnswer::Failed)?;
        match response.status() {
            StatusCode::UNAUTHORIZED => return Err(NoAnswer::TokenRefused),
            status if !status.is_success() => return Err(NoAnswer::Failed),
            _ => {}
        }
        let body = net::body(response).await.map_err(|_| NoAnswer::Failed)?;
        parse(&body).ok_or(NoAnswer::Failed)
    })
}

/// Ask the website at `website` as the signed-in account. The lock isn't waited for: a flag check
/// isn't worth holding a command up for whoever has it. No limit goes on the token: getting one
/// may refresh the session (see the module's docs).
fn targeting(root: &Path, website: &str) -> Targeting {
    let Some(aud) = service_did(website) else { return Targeting::Refused };
    let token = {
        let Ok(Some(lock)) = store::try_lock(root) else { return Targeting::Unknown };
        service_token(&lock, root, &aud, GET_FLAGS)
    };
    match token.map(|token| fetch(website, Some(&token), TOKEN_TIMEOUT)) {
        Ok(Ok(flags)) => Targeting::Answered(flags),
        Ok(Err(NoAnswer::TokenRefused)) => Targeting::Refused,
        Ok(Err(NoAnswer::Failed)) => Targeting::Unknown,
        Err(e) if e.reason() == Some(Reason::SessionEnded) => Targeting::Refused,
        Err(_) => Targeting::Unknown,
    }
}

/// `public`, with `targeted` as `previous` had it.
fn with_targeted(previous: &[Flag], mut public: Vec<Flag>) -> Vec<Flag> {
    for flag in &mut public {
        flag.gates.targeted = previous.iter().any(|old| old.key == flag.key && old.gates.targeted);
    }
    public
}

/// Keep the answer in the store. When it can't be, the next check just asks again, which is also
/// what happens with no store yet: asking about a flag doesn't make one, and writing would.
fn save(root: &Path, cache: &Cache) {
    if root.is_dir() {
        let _ = crate::fsutil::write_json_atomic(&root.join(CACHE_FILE), cache);
    }
}

/// Ask the website and keep what it says. With no answer, `cache` stands, and the attempt is
/// remembered for the back-off.
pub(crate) fn refresh(root: &Path, mut cache: Cache, now: DateTime<Utc>) -> Cache {
    cache.last_attempt_at = Some(now);
    let website = website();
    let Ok(public) = fetch(&website, None, PUBLIC_TIMEOUT) else {
        save(root, &cache);
        return cache;
    };
    cache.fetched_at = Some(now);
    let account = status(root).ok().flatten().filter(may_ask);
    match account.map(|info| (targeting(root, &website), info.did)) {
        Some((Targeting::Answered(flags), did)) => {
            cache.did = Some(did);
            cache.flags = flags;
        }
        Some((Targeting::Unknown, did)) if cache.did.as_deref() == Some(did.as_str()) => {
            cache.flags = with_targeted(&cache.flags, public);
        }
        _ => {
            cache.did = None;
            cache.flags = public;
        }
    }
    save(root, &cache);
    cache
}

#[cfg(test)]
mod tests;
