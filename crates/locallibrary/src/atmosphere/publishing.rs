//! Whether publishing is open yet: whether the permission set publishing asks for
//! ([`super::PUBLISH_SET`]) can be found by the servers that have to grant it.
//!
//! A server looks a permission set up by its NSID's authority, the DNS TXT record
//! `_lexicon.<authority>` (`_lexicon.localnetwork.computer` for locallibrary's), which names the
//! account the Lexicon is published in. Until Local Network publishes it, every server refuses a
//! sign-in that asks for it (`invalid_scope`, "Unable to resolve Lexicon schema"). Asking DNS
//! first lets locallibrary say so before the DJ is sent to the browser, and once the record is
//! there publishing opens without a new release.
//!
//! Only a definite answer closes it: DNS saying there's no such record. When DNS can't be asked
//! (no network, a timeout), it isn't known, and signing in goes ahead; the server's refusal is
//! then explained the same way ([`refused`]).
//!
//! `LOCALLIBRARY_PERMISSION_SET` set to `published` or `unpublished` answers instead of DNS, for
//! tests and for trying either state; `testing::publishing_on_this_thread` answers for one thread.
//! With `LOCALLIBRARY_TEST_OFFLINE` set and no answer given, DNS isn't asked and it isn't known.

use std::time::Duration;

use hickory_resolver::TokioResolver;
use hickory_resolver::proto::rr::RData;

use super::{PUBLISH_SET, block_on, identity};

/// The environment variable that answers in place of DNS: `published` or `unpublished`.
pub const ANSWER_ENV: &str = "LOCALLIBRARY_PERMISSION_SET";

/// How long DNS gets: it's asked before a sign-in, so a slow answer only costs this.
const TIMEOUT: Duration = Duration::from_secs(3);

/// Why publishing isn't open, for a front end to say (it's the same everywhere).
pub const NOT_OPEN: &str = "publishing isn't open yet: Local Network hasn't published the permission locallibrary \
                            asks your account's server for, so no server can grant it. Nothing on your side \
                            changes this; it opens by itself once it's published";

/// Whether publishing is open, as far as can be told.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Availability {
    /// The permission set's DNS record is there.
    Open,
    /// DNS says there's no record: no server can grant publishing.
    NotOpen,
    /// DNS couldn't be asked: try, and explain a refusal if it comes.
    Unknown,
}

impl Availability {
    /// Why publishing can't happen, when it can't.
    pub fn why_not(self) -> Option<&'static str> {
        (self == Availability::NotOpen).then_some(NOT_OPEN)
    }
}

/// The DNS name a server asks for the account `nsid`'s Lexicon is published in: `_lexicon.` and
/// the NSID's authority (all but its last segment) backwards.
pub fn lexicon_dns_name(nsid: &str) -> String {
    let mut authority: Vec<&str> = nsid.split('.').collect();
    authority.pop();
    authority.reverse();
    format!("_lexicon.{}", authority.join("."))
}

thread_local! {
    /// [`availability`]'s answer for one thread (`testing::publishing_on_this_thread`), so a test
    /// neither asks real DNS nor sets the variable for every test in its process.
    pub(crate) static THREAD_ANSWER: std::cell::Cell<Option<Availability>> = const { std::cell::Cell::new(None) };
}

/// Whether publishing is open now. Asks DNS (for at most a few seconds), so call it outside async
/// code, and only when about to ask for publishing.
pub fn availability() -> Availability {
    if let Some(answer) = THREAD_ANSWER.get() {
        return answer;
    }
    match std::env::var(ANSWER_ENV).as_deref().map(str::trim) {
        Ok("published") => return Availability::Open,
        Ok("unpublished") => return Availability::NotOpen,
        _ => {}
    }
    let offline = std::env::var_os("LOCALLIBRARY_TEST_OFFLINE").is_some();
    asked((!offline).then(identity::system_dns).flatten())
}

/// What `dns` says: not known without any to ask.
fn asked(dns: Option<TokioResolver>) -> Availability {
    let Some(dns) = dns else { return Availability::Unknown };
    let name = format!("{}.", lexicon_dns_name(PUBLISH_SET));
    block_on(async {
        match tokio::time::timeout(TIMEOUT, dns.txt_lookup(name)).await {
            Ok(Ok(answer)) => {
                let names_an_account = answer.answers().iter().any(|record| {
                    matches!(&record.data, RData::TXT(txt) if txt.txt_data.iter().any(|s| s.starts_with(b"did=")))
                });
                if names_an_account { Availability::Open } else { Availability::NotOpen }
            }
            Ok(Err(e)) if e.is_no_records_found() => Availability::NotOpen,
            _ => Availability::Unknown,
        }
    })
}

/// Whether a server's refusal of a sign-in (an OAuth error's `error` and `error_description`) is
/// the one it gives while the permission set can't be found.
pub fn refused(error: &str, description: &str) -> bool {
    error == "invalid_scope" && description.contains(PUBLISH_SET)
}

#[cfg(test)]
mod tests;
