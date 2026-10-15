//! Talking to MusicBrainz and the Cover Art Archive as they ask: every request says what's asking
//! and how to reach whoever runs it (the User-Agent), MusicBrainz's are a second apart (its limit
//! for each address), and redirects (the Cover Art Archive's, to archive.org) are followed only to
//! the hosts the manifest names.

use std::io::Read;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use loclib_progress::{Cancelled, Ctx};
use reqwest::blocking::Client;
use reqwest::redirect::Policy;

/// Where the services are.
pub(crate) struct Services {
    pub(crate) musicbrainz: String,
    pub(crate) coverartarchive: String,
}

impl Default for Services {
    fn default() -> Self {
        Services {
            musicbrainz: "https://musicbrainz.org".into(),
            coverartarchive: "https://coverartarchive.org".into(),
        }
    }
}

/// Time, for spacing requests out: tests pass one that only pretends to sleep.
pub(crate) trait Clock: Send + Sync {
    fn now(&self) -> Instant;
    fn sleep(&self, d: Duration);
}

pub(crate) struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Instant {
        Instant::now()
    }

    fn sleep(&self, d: Duration) {
        std::thread::sleep(d);
    }
}

/// MusicBrainz's limit: a request a second from each address, on average.
const MUSICBRAINZ_EVERY: Duration = Duration::from_secs(1);
/// How often a request waiting for its turn notices being cancelled.
const CHECK_EVERY: Duration = Duration::from_millis(100);

/// Spaces MusicBrainz requests [`MUSICBRAINZ_EVERY`] apart, across every operation under way.
pub(crate) struct Limiter {
    clock: Arc<dyn Clock>,
    /// When the last request went.
    last: Mutex<Option<Instant>>,
}

impl Limiter {
    pub(crate) fn new(clock: Arc<dyn Clock>) -> Limiter {
        Limiter { clock, last: Mutex::new(None) }
    }

    /// Waits for the next request's turn, which is then taken, unless the operation is cancelled
    /// first. Operations under way wait their turns one after another.
    pub(crate) fn wait(&self, ctx: &Ctx<'_>) -> Result<(), Cancelled> {
        let mut last = self.last.lock().unwrap_or_else(PoisonError::into_inner);
        loop {
            ctx.check()?;
            let left =
                last.map_or(Duration::ZERO, |at| (at + MUSICBRAINZ_EVERY).saturating_duration_since(self.clock.now()));
            if left.is_zero() {
                *last = Some(self.clock.now());
                return Ok(());
            }
            self.clock.sleep(left.min(CHECK_EVERY));
        }
    }
}

/// The longest a request may take.
const TIMEOUT: Duration = Duration::from_secs(30);
/// The most redirects followed: the Cover Art Archive's take two.
const MAX_REDIRECTS: usize = 5;
/// The most of an answer read: far more than a search's or a 500 px cover's.
const MAX_BODY: u64 = 8 << 20;

/// An HTTP client for one operation.
pub(crate) struct Web {
    client: Client,
}

impl Web {
    /// Says it's this plugin, reachable at `contact`, and follows redirects only to `hosts` and
    /// their subdomains.
    pub(crate) fn new(contact: &str, hosts: Vec<String>) -> Result<Web, String> {
        let agent = format!("{}/{} ( {contact} )", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"));
        let policy = Policy::custom(move |attempt| {
            let host = attempt.url().host_str().unwrap_or_default().to_string();
            if attempt.previous().len() > MAX_REDIRECTS {
                return attempt.error("too many redirects");
            }
            match hosts.iter().any(|h| host == *h || host.ends_with(&format!(".{h}"))) {
                true => attempt.follow(),
                false => attempt.error(format!("redirected to {host}, which the plugin doesn't talk to")),
            }
        });
        let client = Client::builder().user_agent(agent).redirect(policy).timeout(TIMEOUT).build();
        client.map(|client| Web { client }).map_err(|e| message(&e))
    }

    /// GETs `url` with `query`: the answer's status and body, or why there's none.
    pub(crate) fn get(&self, url: &str, query: &[(&str, &str)]) -> Result<(u16, Vec<u8>), String> {
        let response = self.client.get(url).query(query).send().map_err(|e| message(&e))?;
        let status = response.status().as_u16();
        let mut body = Vec::new();
        response.take(MAX_BODY).read_to_end(&mut body).map_err(|e| message(&e))?;
        Ok((status, body))
    }
}

/// `e` and what caused it, which reqwest keeps apart (the redirect policy's reason, say).
fn message(e: &dyn std::error::Error) -> String {
    let causes = std::iter::successors(Some(e), |e| e.source());
    causes.map(ToString::to_string).collect::<Vec<_>>().join(": ")
}
