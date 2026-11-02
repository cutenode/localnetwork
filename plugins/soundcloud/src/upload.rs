//! SoundCloud's upload: one `POST /tracks`, a `multipart/form-data` form of the track's fields,
//! its audio (`track[asset_data]`) and maybe its artwork (`track[artwork_data]`), answered with
//! the track. The host reads the files as it sends them, and reports the bytes it sends as the
//! upload's progress.
//!
//! SoundCloud has no resumable upload, so a form that doesn't arrive (or that SoundCloud answers
//! with a server error, or too many requests) is sent again whole, a few times at most, waiting
//! longer each time or as long as SoundCloud's `Retry-After` says. SoundCloud can make the track
//! even when it answers with an error, or the answer is lost (its guide says to check before
//! sending again, and `soundcloud/api#601`), so before sending it again after anything but too
//! many requests, the account's newest tracks are looked through for one of the same title made
//! since the upload began: that's the answer. Anything else SoundCloud refuses is said in its
//! words and not sent again.

use std::time::{Duration, Instant};

use chrono::{DateTime, TimeDelta, Utc};
use loclib_plugin::{Fetch, Fetched, FormPart, Host, RpcError};
use loclib_progress::{Ctx, Phase};
use serde::Deserialize;
use serde::de::DeserializeOwned;

/// Where a track is uploaded.
pub(crate) const TRACKS: &str = "https://api.soundcloud.com/tracks";

/// The account's newest tracks, newest first.
pub(crate) const NEWEST: &str = "https://api.soundcloud.com/me/tracks?limit=20&linked_partitioning=true";

/// The SoundCloud account, as the host fills its access token in (`OAuth <token>`).
pub(crate) const AUTHORIZATION: &str = "{account:soundcloud}";

/// How many times the form is sent before the upload fails.
pub(crate) const TRIES: u32 = 3;

/// A second, or a thousandth of one in this crate's tests, so they wait out every try (and every
/// `Retry-After`) without spending seconds on it.
const SECOND: Duration = if cfg!(test) { Duration::from_millis(1) } else { Duration::from_secs(1) };

/// The wait after the first failure, in seconds, doubling for each after.
const FIRST_WAIT: u64 = 2;

/// The longest `Retry-After` waited out, in seconds. SoundCloud asking for longer fails the
/// upload, saying how long.
pub(crate) const LONGEST_WAIT: u64 = 300;

/// How much earlier than the upload began, by this computer's clock, a track SoundCloud made of
/// it may say it was made: the two clocks needn't agree.
const CLOCKS_APART: TimeDelta = TimeDelta::minutes(5);

/// How often a wait notices the hook being cancelled.
const CHECK_EVERY: Duration = Duration::from_millis(50);

/// A track SoundCloud made, as far as it's read.
#[derive(Deserialize)]
pub(crate) struct Uploaded {
    /// The track's page.
    pub(crate) permalink_url: String,
}

/// A track of the account's, as its tracks are listed.
#[derive(Deserialize)]
struct Listed {
    title: String,
    /// `2026/10/03 21:40:49 +0000`.
    created_at: String,
    permalink_url: String,
}

#[derive(Deserialize)]
struct Tracks {
    collection: Vec<Listed>,
}

/// Uploads `form` (whose files come to `bytes`) as the track `title`: the track SoundCloud made
/// of it.
pub(crate) fn upload(host: &Host, title: &str, form: Vec<FormPart>, bytes: u64) -> Result<Uploaded, RpcError> {
    let began = Utc::now();
    let upload = Fetch { method: "POST".into(), multipart: form, ..request(TRACKS) };
    let ctx = host.ctx();
    let mut tried = 1;
    loop {
        // A phase for each try: the host counts every byte it sends, and each try sends them all.
        ctx.phase_start(Phase::Upload, 1, bytes);
        let answer = host.fetch(&upload);
        ctx.phase_end(Phase::Upload);
        let backoff = FIRST_WAIT << (tried - 1);
        // Why it failed, how long to wait, and whether SoundCloud may have made the track anyway.
        let (failure, wait, unknown) = match answer {
            Ok(answer) if (200..300).contains(&answer.status) => return read(&answer),
            Ok(answer) if answer.status == 429 => {
                let said = retry_after(&answer).unwrap_or(backoff);
                (refused(&answer, "the set"), said, false)
            }
            Ok(answer) if matches!(answer.status, 500 | 502 | 503 | 504) => {
                (refused(&answer, "the set"), backoff, true)
            }
            Ok(answer) => return Err(refused(&answer, "the set")),
            Err(e) if e.kind() == Some("external") => {
                let message = format!("couldn't send the set to SoundCloud: {}", e.message);
                (RpcError { message, ..e }, backoff, true)
            }
            // Cancelled, or not signed in: sending it again changes nothing.
            Err(e) => return Err(e),
        };
        if wait > LONGEST_WAIT {
            let message = format!("{}: publish again in {} minutes", failure.message, wait.div_ceil(60));
            return Err(RpcError { message, ..failure });
        }
        let last = tried == TRIES;
        if last && !unknown {
            return Err(failure);
        }
        // The host relays this as the plugin's log: a failure reaches the user, a retry not.
        eprintln!("{}: waiting {wait} s", failure.message);
        pause(&ctx, SECOND * u32::try_from(wait).unwrap_or(u32::MAX))?;
        if unknown {
            match made(host, title, began) {
                Ok(Some(track)) => {
                    eprintln!("SoundCloud made {} though the upload failed", track.permalink_url);
                    return Ok(track);
                }
                Ok(None) => {}
                Err(e) if e.kind() == Some("cancelled") => return Err(e),
                Err(e) => {
                    let message = format!(
                        "{}, and SoundCloud may have made the track anyway ({}): look at your tracks there before \
                         publishing again",
                        failure.message, e.message
                    );
                    return Err(RpcError { message, ..failure });
                }
            }
        }
        if last {
            return Err(failure);
        }
        tried += 1;
    }
}

/// The track titled `title` SoundCloud made since `began`, if it's among the account's newest.
fn made(host: &Host, title: &str, began: DateTime<Utc>) -> Result<Option<Uploaded>, RpcError> {
    let what = "the account's tracks";
    let answer = host.fetch(&request(NEWEST))?;
    if !(200..300).contains(&answer.status) {
        return Err(refused(&answer, what));
    }
    let tracks: Tracks = read(&answer)?;
    let since = |t: &Listed| {
        let made = DateTime::parse_from_str(&t.created_at, "%Y/%m/%d %H:%M:%S %z");
        made.is_ok_and(|made| made >= began - CLOCKS_APART)
    };
    let found = tracks.collection.into_iter().find(|t| t.title == title && since(t));
    Ok(found.map(|t| Uploaded { permalink_url: t.permalink_url }))
}

/// A request to `url` with the account's token, asking for JSON.
fn request(url: &str) -> Fetch {
    let headers = [("Authorization", AUTHORIZATION), ("Accept", "application/json; charset=utf-8")];
    Fetch { headers: headers.into_iter().map(|(k, v)| (k.into(), v.into())).collect(), ..Fetch::get(url) }
}

/// How many seconds SoundCloud asks to wait (`Retry-After`, a number of them), when it says.
pub(crate) fn retry_after(answer: &Fetched) -> Option<u64> {
    let said = answer.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case("retry-after"))?.1.trim();
    let digits = !said.is_empty() && said.bytes().all(|b| b.is_ascii_digit());
    // One too big to count is longer than any wait.
    digits.then(|| said.parse().unwrap_or(u64::MAX))
}

/// SoundCloud's answer as `T`.
fn read<T: DeserializeOwned>(answer: &Fetched) -> Result<T, RpcError> {
    serde_json::from_str(answer.body.as_deref().unwrap_or_default())
        .map_err(|e| RpcError::failed("external", format!("SoundCloud's answer can't be read: {e}")))
}

/// SoundCloud's error, as far as it's read: `{"message", "errors": [{"error_message"}]}`.
#[derive(Deserialize)]
struct Refusal {
    #[serde(default)]
    message: Option<String>,
    #[serde(default)]
    errors: Vec<Explained>,
}

#[derive(Deserialize)]
struct Explained {
    error_message: Option<String>,
}

/// SoundCloud refusing `what`, in its words, and what to do about it where there's something to
/// do.
fn refused(answer: &Fetched, what: &str) -> RpcError {
    let status = answer.status;
    let said = answer.body.as_deref().and_then(|b| serde_json::from_str::<Refusal>(b).ok());
    let words = said.into_iter().flat_map(|r| r.errors.into_iter().filter_map(|e| e.error_message).chain(r.message));
    let words = words.map(|w| w.trim().to_string()).find(|w| !w.is_empty());
    let words = words.map_or(status.to_string(), |w| format!("{status}: {w}"));
    let (kind, why) = match status {
        401 | 403 => ("permission", ""),
        413 => ("invalid", "the recording is bigger than SoundCloud takes"),
        429 => ("external", "SoundCloud is taking too many requests"),
        400..=499 => ("invalid", ""),
        _ => ("external", ""),
    };
    let verb = if status >= 500 { "failed" } else { "refused" };
    let why = if why.is_empty() { String::new() } else { format!(": {why}") };
    RpcError::failed(kind, format!("SoundCloud {verb} {what} ({words}){why}"))
}

/// Waits `wait`, failing with `cancelled` once `ctx` is.
fn pause(ctx: &Ctx<'_>, wait: Duration) -> Result<(), RpcError> {
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
