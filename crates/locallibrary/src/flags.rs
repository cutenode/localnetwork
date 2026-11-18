//! Feature flags, the same ones the website has (`flags/README.md` in the repository).
//!
//! [`check`] is the only gate: commands that don't call it run none of this, and nothing asks the
//! website but a check that finds the cache due, or `account flags`. The website's answer is kept
//! in [`CACHE_FILE`] in the store folder, and every flag is off without one. Asking is
//! `atmosphere::flags`' part, so a build without the account never asks and has every flag off.
//!
//! A flag here is for rolling something out. Anyone can edit the cache, so anything that has to be
//! enforced belongs on the server.

use std::path::Path;
use std::time::Duration;

use chrono::{DateTime, TimeDelta, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// The flags in the repository's `flags/catalog/`: a removed flag fails to compile where it's used.
pub mod keys {
    include!(concat!(env!("OUT_DIR"), "/flag_keys.rs"));
}

/// The website's last answer, in the store folder.
pub const CACHE_FILE: &str = "flags.json";

/// How long an answer lasts before a check asks again.
pub const FRESH: TimeDelta = TimeDelta::hours(1);

/// How long a failed attempt waits before the next: a check shouldn't spend [`PUBLIC_TIMEOUT`] on
/// every run while the network is down.
pub const RETRY_AFTER_FAILURE: TimeDelta = TimeDelta::minutes(15);

/// How long an answer is gone by with no newer one: as long as a session lasts, so a DJ offline at
/// a gig keeps what they had, and a flag turned off for a bug stops working within two weeks at
/// worst. It's `SESSION_LIMIT_DAYS`, stated here because that's only in builds with the account.
pub const LAST_KNOWN_GOOD: TimeDelta = TimeDelta::days(14);

/// The most a check waits for the website.
pub const PUBLIC_TIMEOUT: Duration = Duration::from_secs(2);

/// A flag's gates, as the website answers them for its caller.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Gates {
    /// Off means off for everyone, whatever the rest says. A record from before staff could turn a
    /// flag off is on, and so is an answer from a website that predates it.
    #[serde(default = "enabled_by_default")]
    pub enabled: bool,
    pub everyone: bool,
    /// How much of the accounts it's on for, 0 to 100.
    pub percentage: u32,
    /// The caller is one of the flag's accounts or in one of its groups.
    pub targeted: bool,
}

fn enabled_by_default() -> bool {
    true
}

/// One flag in an answer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Flag {
    pub key: String,
    #[serde(flatten)]
    pub gates: Gates,
}

/// Where the account `did` falls in `key`'s rollout, under 100: the first 4 bytes of
/// sha256(key + did), big-endian, mod 100. The website runs the same sum (`flags/vectors.json`).
pub fn bucket(key: &str, did: &str) -> u32 {
    let hash = Sha256::new().chain_update(key).chain_update(did).finalize();
    u32::from_be_bytes([hash[0], hash[1], hash[2], hash[3]]) % 100
}

/// Whether `key` is on for `did`, given its gates. Signed out, there's no DID to bucket.
pub fn decide(gates: &Gates, key: &str, did: Option<&str>) -> bool {
    gates.enabled && (gates.everyone || gates.targeted || did.is_some_and(|did| bucket(key, did) < gates.percentage))
}

/// How much an answer can be gone by.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Freshness {
    /// Younger than [`FRESH`].
    Fresh,
    /// The website hasn't answered since, but it's younger than [`LAST_KNOWN_GOOD`].
    Stale,
    /// No answer, or one older than that: every flag is off.
    None,
}

/// [`CACHE_FILE`].
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub(crate) struct Cache {
    /// Who the flags' `targeted` was said of. `None`: nobody, the answer was public.
    pub(crate) did: Option<String>,
    pub(crate) fetched_at: Option<DateTime<Utc>>,
    pub(crate) last_attempt_at: Option<DateTime<Utc>>,
    pub(crate) flags: Vec<Flag>,
}

impl Cache {
    /// What's in the store at `root`. One that's damaged or can't be read is no answer, never an
    /// error: a flag check has nothing to say to whoever ran the command.
    #[cfg(feature = "account")]
    pub(crate) fn load(root: &Path) -> Cache {
        crate::fsutil::read_json_opt(&root.join(CACHE_FILE)).ok().flatten().unwrap_or_default()
    }

    fn freshness(&self, now: DateTime<Utc>) -> Freshness {
        match self.fetched_at.map(|at| now - at) {
            Some(age) if age < FRESH => Freshness::Fresh,
            Some(age) if age <= LAST_KNOWN_GOOD => Freshness::Stale,
            _ => Freshness::None,
        }
    }

    /// Whether to ask the website now: the answer has aged, or it was for someone else and `caller`
    /// may hear what targets them (a sign-in shouldn't wait an hour for that). Either way only once
    /// the last attempt is [`RETRY_AFTER_FAILURE`] old, so a token that keeps being refused doesn't
    /// mean a request on every run.
    #[cfg(feature = "account")]
    fn due(&self, caller: &Caller, now: DateTime<Utc>) -> bool {
        let for_someone_else = caller.did.is_some() && !caller.lacks_scope && caller.did != self.did;
        (self.freshness(now) != Freshness::Fresh || for_someone_else)
            && self.last_attempt_at.is_none_or(|at| now - at >= RETRY_AFTER_FAILURE)
    }

    /// Whether `key` is on for `did`, signed in now. `targeted` only counts for the account it was
    /// said of; the public gates apply to whoever is asking.
    fn on(&self, key: &str, did: Option<&str>, now: DateTime<Utc>) -> bool {
        if self.freshness(now) == Freshness::None {
            return false;
        }
        let Some(flag) = self.flags.iter().find(|flag| flag.key == key) else { return false };
        let targeted = flag.gates.targeted && did.is_some() && did == self.did.as_deref();
        decide(&Gates { targeted, ..flag.gates }, key, did)
    }
}

/// Who's asking.
#[derive(Default)]
struct Caller {
    did: Option<String>,
    /// Signed in, with a session from before it could ask about targeting.
    lacks_scope: bool,
}

#[cfg(feature = "account")]
fn caller(root: &Path) -> Caller {
    // An account file that can't be read is nobody signed in, as far as a flag goes.
    let Some(info) = crate::atmosphere::status(root).ok().flatten() else { return Caller::default() };
    let lacks_scope = !crate::atmosphere::flags::may_ask(&info);
    Caller { did: Some(info.did), lacks_scope }
}

/// The website's latest answer and who's asking: the cache's, after asking when that's `due` or
/// with `force`.
#[cfg(feature = "account")]
fn answer(root: &Path, force: bool, now: DateTime<Utc>) -> (Cache, Caller) {
    let cache = Cache::load(root);
    let asking = caller(root);
    if !force && !cache.due(&asking, now) {
        return (cache, asking);
    }
    // Asking can end a session, so who's signed in comes after it.
    (crate::atmosphere::flags::refresh(root, cache, now), caller(root))
}

/// Without the account there's nothing to ask with, and every flag is off, whatever a build with
/// it left in the same store.
#[cfg(not(feature = "account"))]
fn answer(_: &Path, _: bool, _: DateTime<Utc>) -> (Cache, Caller) {
    Default::default()
}

/// Whether the flag `key` (one of [`keys`]) is on for whoever's signed in. The one gate: it reads
/// the cache and only goes to the network when that's due, for at most [`PUBLIC_TIMEOUT`] when
/// the website doesn't answer. Asking can take longer once it has, as the session may be
/// refreshed. Call it outside async code.
pub fn check(root: &Path, key: &str) -> bool {
    check_at(root, key, Utc::now())
}

pub(crate) fn check_at(root: &Path, key: &str, now: DateTime<Utc>) -> bool {
    let (cache, caller) = answer(root, false, now);
    cache.on(key, caller.did.as_deref(), now)
}

/// A flag in [`FlagReport`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct FlagState {
    pub key: String,
    pub on: bool,
}

/// What `account flags` shows.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct FlagReport {
    /// Every flag this version knows.
    pub flags: Vec<FlagState>,
    pub freshness: Freshness,
    /// When the website last answered, however long ago.
    pub fetched_at: Option<DateTime<Utc>>,
    /// The session can't ask about flags that target an account or group: only a new sign-in can.
    pub needs_sign_in: bool,
}

/// Every flag, on or off for whoever's signed in. Asks the website as [`check`] does, and with
/// `refresh` whatever the cache's age.
pub fn all(root: &Path, refresh: bool) -> FlagReport {
    all_at(root, refresh, Utc::now())
}

pub(crate) fn all_at(root: &Path, refresh: bool, now: DateTime<Utc>) -> FlagReport {
    let (cache, caller) = answer(root, refresh, now);
    let on = |key: &&str| FlagState { key: key.to_string(), on: cache.on(key, caller.did.as_deref(), now) };
    FlagReport {
        flags: keys::ALL.iter().map(on).collect(),
        freshness: cache.freshness(now),
        fetched_at: cache.fetched_at,
        needs_sign_in: caller.lacks_scope,
    }
}

#[cfg(test)]
mod tests;
