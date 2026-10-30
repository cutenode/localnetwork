//! YouTube's resumable upload (the YouTube Data API's `videos.insert`, as
//! `guides/using_resumable_upload_protocol` describes it): a session started with the video's
//! details, size and type, which answers where to send the file (`Location`); the file sent there
//! in chunks, each saying where it goes (`Content-Range`) and answered `308` with how much YouTube
//! has (`Range`) until the last, answered with the video. A chunk that doesn't arrive (or that
//! YouTube answers with a server error) is followed by asking YouTube how much it has, `PUT` with
//! `Content-Range: bytes */<size>`, and the upload goes on from there, a few times in a row at
//! most, waiting longer each time.
//!
//! The host reads each chunk from the video as it sends it (`body_range`), and reports the bytes
//! it sends as the upload's progress. Nothing is kept between requests, so a publish that stops
//! partway leaves a session YouTube drops on its own, and publishing again starts afresh.

use std::path::Path;
use std::time::{Duration, Instant};

use loclib_plugin::{ByteRange, Fetch, Fetched, Host, RpcError};
use loclib_progress::{Ctx, Phase};
use serde::Deserialize;
use serde_json::Value;

/// Where an upload's session is started: the video's details (`snippet`) and who can watch it
/// (`status`) go with it.
pub(crate) const START: &str =
    "https://www.googleapis.com/upload/youtube/v3/videos?uploadType=resumable&part=snippet,status";

/// The Google account, as the host fills its access token in.
pub(crate) const AUTHORIZATION: &str = "{account:google}";

/// What a chunk is a multiple of (but the last): 256 KiB, as YouTube asks. A 256th of that in
/// this crate's tests, so a few kilobytes make several chunks.
pub(crate) const QUANTUM: u64 = if cfg!(test) { 1 << 10 } else { 256 << 10 };

/// How much a chunk sends: 16 MiB, which the host gives two and a half minutes to go.
pub(crate) const CHUNK: u64 = 64 * QUANTUM;

/// How many times in a row a request may fail before the upload does.
pub(crate) const TRIES: u32 = 4;

/// The wait after the first failure, doubling for each after. In this crate's tests it's a
/// thousandth of a second, so they wait out every try without spending seconds on it.
const FIRST_WAIT: Duration = if cfg!(test) { Duration::from_millis(1) } else { Duration::from_secs(1) };

/// How often a wait notices the hook being cancelled.
const CHECK_EVERY: Duration = Duration::from_millis(50);

/// What YouTube answers the last chunk with, as far as it's read.
#[derive(Deserialize)]
pub(crate) struct Uploaded {
    pub(crate) id: String,
    pub(crate) status: Option<Status>,
}

/// Who can watch it, as YouTube made it.
#[derive(Deserialize)]
pub(crate) struct Status {
    #[serde(rename = "privacyStatus")]
    pub(crate) privacy: Option<String>,
}

/// Uploads `video` (an MP4 of `size` bytes) with `details` (the `videos` resource's `snippet`
/// and `status`): the video YouTube made of it.
pub(crate) fn upload(host: &Host, video: &Path, size: u64, details: &Value) -> Result<Uploaded, RpcError> {
    let start = Fetch {
        body: Some(details.to_string()),
        ..request(
            "POST",
            START,
            &[
                ("Content-Type", "application/json; charset=UTF-8"),
                ("X-Upload-Content-Length", &size.to_string()),
                ("X-Upload-Content-Type", "video/mp4"),
            ],
        )
    };
    let started = ask(host, &start, "the video's details")?;
    if !(200..300).contains(&started.status) {
        return Err(refused(&started, "the video's details"));
    }
    let session = header(&started, "location")
        .ok_or_else(|| RpcError::failed("external", "YouTube didn't say where to send the video"))?
        .to_string();
    let ctx = host.ctx();
    ctx.phase_start(Phase::Upload, 1, size);
    let sent = send(host, &ctx, &session, video, size);
    ctx.phase_end(Phase::Upload);
    sent
}

/// Sends `video` to the upload `session` in chunks. After a chunk that fails, YouTube is asked
/// how much it has, and the upload goes on from there.
fn send(host: &Host, ctx: &Ctx<'_>, session: &str, video: &Path, size: u64) -> Result<Uploaded, RpcError> {
    let what = "the video";
    // How much YouTube has, how many requests in a row have failed, and whether to ask YouTube
    // how much it has before sending more.
    let (mut at, mut failed, mut ask_where) = (0, 0, false);
    loop {
        ctx.check()?;
        let checking = ask_where || at == size;
        let fetch = match checking {
            true => {
                let range = format!("bytes */{size}");
                Fetch { body: Some(String::new()), ..request("PUT", session, &[("Content-Range", &range)]) }
            }
            false => {
                let length = CHUNK.min(size - at);
                let range = format!("bytes {at}-{}/{size}", at + length - 1);
                Fetch {
                    body_blob: Some(video.to_path_buf()),
                    body_range: Some(ByteRange { offset: at, length: Some(length) }),
                    ..request("PUT", session, &[("Content-Type", "video/mp4"), ("Content-Range", &range)])
                }
            }
        };
        let failure = match host.fetch(&fetch) {
            Ok(answer) if (200..300).contains(&answer.status) => return read(&answer),
            Ok(answer) if answer.status == 308 => {
                let had = received(&answer).min(size);
                let moved = had > at;
                at = had;
                if moved {
                    failed = 0;
                }
                if moved || checking && at < size {
                    ask_where = false;
                    continue;
                }
                RpcError::failed("external", format!("YouTube kept no more than {at} bytes of {what}"))
            }
            Ok(answer) if matches!(answer.status, 500 | 502 | 503 | 504) => refused(&answer, what),
            Ok(answer) => return Err(refused(&answer, what)),
            Err(e) if e.kind() == Some("external") => e,
            // Cancelled, or not signed in: asking again changes nothing.
            Err(e) => return Err(e),
        };
        failed += 1;
        if failed == TRIES {
            return Err(failure);
        }
        ask_where = true;
        let wait = FIRST_WAIT * 2u32.pow(failed - 1);
        // The host relays this as the plugin's log: a failure reaches the user, a retry not.
        eprintln!("{}: asking YouTube how much it has in {} ms", failure.message, wait.as_millis());
        pause(ctx, wait)?;
    }
}

/// A request to `url` with the account's token and `headers`.
fn request(method: &str, url: &str, headers: &[(&str, &str)]) -> Fetch {
    let headers = [("Authorization", AUTHORIZATION)].iter().chain(headers);
    Fetch {
        method: method.into(),
        headers: headers.map(|(k, v)| ((*k).into(), (*v).into())).collect(),
        ..Fetch::get(url)
    }
}

/// What YouTube answers `fetch` (which is `what`) with, asked again a few times, waiting longer
/// each time, while it can't be reached or answers a server error.
fn ask(host: &Host, fetch: &Fetch, what: &str) -> Result<Fetched, RpcError> {
    let (mut tried, mut wait) = (1, FIRST_WAIT);
    loop {
        let failure = match host.fetch(fetch) {
            Ok(answer) if matches!(answer.status, 500 | 502 | 503 | 504) => refused(&answer, what),
            Ok(answer) => return Ok(answer),
            Err(e) if e.kind() == Some("external") => e,
            Err(e) => return Err(e),
        };
        if tried == TRIES {
            return Err(failure);
        }
        eprintln!("{}: asking again in {} ms", failure.message, wait.as_millis());
        pause(&host.ctx(), wait)?;
        (tried, wait) = (tried + 1, wait * 2);
    }
}

/// The header `name` of `answer`, whatever its case.
fn header<'a>(answer: &'a Fetched, name: &str) -> Option<&'a str> {
    answer.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
}

/// How many bytes YouTube has, from a `308`'s `Range: bytes=0-<last>` (none without one).
fn received(answer: &Fetched) -> u64 {
    let last =
        header(answer, "range").and_then(|r| r.strip_prefix("bytes=0-")).and_then(|l| l.trim().parse::<u64>().ok());
    last.map_or(0, |l| l + 1)
}

/// The video YouTube answers with.
fn read(answer: &Fetched) -> Result<Uploaded, RpcError> {
    serde_json::from_str(answer.body.as_deref().unwrap_or_default())
        .map_err(|e| RpcError::failed("external", format!("YouTube's answer can't be read: {e}")))
}

/// Google's error, as far as it's read: `{"error": {"message", "errors": [{"reason"}]}}`.
#[derive(Deserialize)]
struct Refusal {
    error: Explained,
}

#[derive(Deserialize)]
struct Explained {
    #[serde(default)]
    message: String,
    #[serde(default)]
    errors: Vec<Reason>,
}

#[derive(Deserialize)]
struct Reason {
    reason: String,
}

/// YouTube refusing `what`, in its words, and what to do about it where there's something to do.
fn refused(answer: &Fetched, what: &str) -> RpcError {
    let status = answer.status;
    let said = answer.body.as_deref().and_then(|b| serde_json::from_str::<Refusal>(b).ok()).map(|r| r.error);
    let reason = said.as_ref().and_then(|s| s.errors.first()).map(|r| r.reason.as_str()).unwrap_or_default();
    let words = match &said {
        Some(s) if !reason.is_empty() => format!("{status} {reason}: {}", s.message),
        Some(s) if !s.message.is_empty() => format!("{status}: {}", s.message),
        _ => status.to_string(),
    };
    let (kind, why) = match (status, reason) {
        (_, "youtubeSignupRequired") => {
            ("permission", "the Google account has no YouTube channel: make one and publish again")
        }
        (_, "uploadLimitExceeded") => ("external", "the channel has uploaded as many videos as it may today"),
        (_, "quotaExceeded" | "dailyLimitExceeded") => {
            ("external", "Local Network's share of the YouTube API is used up for today: publish again tomorrow")
        }
        (429, _) | (_, "rateLimitExceeded" | "userRateLimitExceeded") => {
            ("external", "YouTube is taking too many requests: publish again in a while")
        }
        (401 | 403, _) => ("permission", ""),
        (404, _) => ("external", "the upload's session expired: publish again"),
        (400..=499, _) => ("invalid", ""),
        _ => ("external", ""),
    };
    let verb = if kind == "external" && status >= 500 { "failed" } else { "refused" };
    let why = if why.is_empty() { String::new() } else { format!(": {why}") };
    RpcError::failed(kind, format!("YouTube {verb} {what} ({words}){why}"))
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
