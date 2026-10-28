//! Which recordings MusicBrainz has an ISRC as: its ISRC lookup, with the artists each credits and
//! the releases each is on. The lookup is read in XML, as JSON's answer to it leaves out the
//! releases. The host makes the request (`net/fetch`), in turns with every other request to
//! MusicBrainz as the manifest asks, saying it's locallibrary and how to reach whoever runs it.

use std::time::{Duration, Instant, SystemTime};

use loclib_plugin::{Fetch, Fetched, Host, RpcError};
use loclib_progress::Ctx;
use serde::Deserialize;
use uuid::Uuid;

/// Where MusicBrainz is.
const MUSICBRAINZ: &str = "https://musicbrainz.org";

/// How many times a lookup is sent while MusicBrainz answers `503`, which it does when too many
/// requests come at once (from everyone, at busy times).
pub(crate) const TRIES: usize = 3;

/// The longest `Retry-After` waited for, in seconds: a longer one fails the lookup, to be tried
/// another time.
pub(crate) const LONGEST_WAIT: u32 = 30;

/// A second of `Retry-After`. In this crate's tests it's a hundredth of one, so they wait out
/// MusicBrainz's waits, in proportion, without spending seconds on each.
pub(crate) const SECOND: Duration = if cfg!(test) { Duration::from_millis(10) } else { Duration::from_secs(1) };

/// How often a wait notices the hook being cancelled.
const CHECK_EVERY: Duration = Duration::from_millis(50);

/// An ISRC lookup's answer, as far as it's read.
#[derive(Deserialize)]
struct Metadata {
    isrc: Isrc,
}

#[derive(Deserialize)]
struct Isrc {
    #[serde(rename = "recording-list")]
    list: RecordingList,
}

#[derive(Deserialize)]
struct RecordingList {
    #[serde(default)]
    recording: Vec<Recording>,
}

/// A recording an ISRC is.
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct Recording {
    #[serde(rename = "@id")]
    pub(crate) id: Uuid,
    #[serde(default)]
    pub(crate) title: String,
    /// In milliseconds.
    pub(crate) length: Option<u64>,
    #[serde(rename = "artist-credit", default)]
    pub(crate) credit: Credit,
    #[serde(rename = "release-list", default)]
    pub(crate) releases: ReleaseList,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub(crate) struct Credit {
    #[serde(rename = "name-credit", default)]
    pub(crate) names: Vec<NameCredit>,
}

/// An artist a recording credits.
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct NameCredit {
    /// The name it's credited under, when that isn't the artist's own.
    pub(crate) name: Option<String>,
    pub(crate) artist: Artist,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct Artist {
    #[serde(rename = "@id")]
    pub(crate) id: Uuid,
    pub(crate) name: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub(crate) struct ReleaseList {
    #[serde(default)]
    pub(crate) release: Vec<Release>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct Release {
    #[serde(rename = "@id")]
    pub(crate) id: Uuid,
    #[serde(default)]
    pub(crate) title: String,
    /// `2018`, `2018-03` or `2018-03-09`.
    pub(crate) date: Option<String>,
}

/// The recordings MusicBrainz has `isrc` (a valid one) as: none when it doesn't know it (`404`).
/// Fails when MusicBrainz can't be reached, stays busy, refuses the lookup (`400` included: every
/// ISRC asked about is one) or answers what can't be read, and when the hook is cancelled: none of
/// that is an answer to keep.
pub(crate) fn recordings(host: &Host, isrc: &str) -> Result<Vec<Recording>, RpcError> {
    let fetch = Fetch::get(format!("{MUSICBRAINZ}/ws/2/isrc/{isrc}?inc=artist-credits+releases"));
    let mut tries = 1;
    loop {
        let fetched = host.fetch(&fetch).map_err(|e| match e.kind() {
            Some("cancelled") => e,
            _ => RpcError { message: format!("couldn't reach MusicBrainz: {}", e.message), ..e },
        })?;
        let wait = retry_after(&fetched, SystemTime::now());
        match fetched.status {
            200 => {
                let xml = fetched.body.as_deref().unwrap_or_default();
                let answer = quick_xml::de::from_str::<Metadata>(xml).map_err(|e| {
                    RpcError::failed("external", format!("MusicBrainz's answer about {isrc} isn't readable: {e}"))
                });
                return answer.map(|m| m.isrc.list.recording);
            }
            404 => return Ok(Vec::new()),
            503 if tries < TRIES && wait <= LONGEST_WAIT => {
                // The host relays this as the plugin's: a failure reaches the user as the track's
                // warning, but a retry would otherwise go unseen.
                eprintln!("MusicBrainz is busy (503) for {isrc}: asking again in {wait} s");
                pause(&host.ctx(), SECOND * wait)?;
                tries += 1;
            }
            status => return Err(RpcError::failed("external", format!("MusicBrainz answered {status} about {isrc}"))),
        }
    }
}

/// How many seconds after `now` a `503` asks to wait (`Retry-After`): a number of them, or until
/// an HTTP-date (rounded up). One too big to count is longer than any wait; none, one past, or
/// one that can't be read is no wait past the host's turns.
pub(crate) fn retry_after(fetched: &Fetched, now: SystemTime) -> u32 {
    let said = fetched.headers.iter().find(|(name, _)| name.eq_ignore_ascii_case("retry-after"));
    let Some(said) = said.map(|(_, v)| v.trim()) else { return 0 };
    if !said.is_empty() && said.bytes().all(|b| b.is_ascii_digit()) {
        return said.parse().unwrap_or(u32::MAX);
    }
    let until = httpdate::parse_http_date(said).ok().and_then(|at| at.duration_since(now).ok());
    until.map_or(0, |d| u32::try_from(d.as_secs() + u64::from(d.subsec_nanos() > 0)).unwrap_or(u32::MAX))
}

/// Waits `wait`, failing with `cancelled` once `ctx` is.
pub(crate) fn pause(ctx: &Ctx<'_>, wait: Duration) -> Result<(), RpcError> {
    let until = Instant::now() + wait;
    loop {
        ctx.check()?;
        let left = until.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Ok(());
        }
        std::thread::sleep(left.min(CHECK_EVERY));
    }
}
