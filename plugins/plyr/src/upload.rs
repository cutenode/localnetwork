//! plyr.fm's resumable upload (`backend/src/backend/api/tracks/upload_sessions.py`): a session
//! opened with the file's name and size, the file sent in parts of the size it says (each exactly
//! that long but the last, or it's refused), then the track's fields, which queue it to be
//! processed and published; and the upload's progress, read until it says how that went.
//!
//! The host answers a request whole, and the progress is a stream of server-sent events (one a
//! second) that plyr.fm ends once the upload is published or has failed. plyr.fm has no other way
//! to ask, so the stream is read as one answer, its last event saying how it went: the host waits
//! as long as events keep coming, giving up once one is 30 seconds late. A part has those 30
//! seconds to go and be answered, so a 10 MiB part needs about 3 Mbit/s up.
//!
//! A part, or the progress, that doesn't arrive (or that plyr.fm answers with a server error) is
//! asked for again, a few times, waiting longer each time: plyr.fm takes a part sent again in place
//! of the first. Nothing is kept between requests, so a publish that stops partway (failing for
//! good, cancelled, or the computer stopping) leaves its session open on plyr.fm, which fails it
//! once it has been idle for a day; publishing again starts afresh.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::time::{Duration, Instant};

use loclib_plugin::{Fetch, Fetched, Host, RpcError};
use loclib_progress::{Ctx, Event, Phase};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::json;

use crate::track::{TAGS, Track};

/// plyr.fm's API.
pub(crate) const API: &str = "https://api.plyr.fm";

/// The developer token, as the host fills it in.
pub(crate) const AUTHORIZATION: &str = "Bearer {secret:token}";

/// How many times a part, or the progress, is asked for before the upload fails.
pub(crate) const TRIES: u32 = 4;

/// The wait before the second try, doubling for each try after. In this crate's tests it's a
/// thousandth of a second, so they wait out every try without spending seconds on it.
const FIRST_WAIT: Duration = if cfg!(test) { Duration::from_millis(1) } else { Duration::from_secs(1) };

/// How often a wait notices the hook being cancelled.
const CHECK_EVERY: Duration = Duration::from_millis(50);

/// The biggest part taken from plyr.fm: the host reads a part into memory to send it, and plyr.fm
/// asks for 10 MiB.
const MAX_PART: u64 = 64 << 20;

/// What opening a session answers.
#[derive(Deserialize)]
struct Session {
    upload_id: String,
    part_size_bytes: u64,
    part_count: u64,
}

/// What finishing one answers: the upload whose progress to follow.
#[derive(Deserialize)]
struct Queued {
    upload_id: String,
}

/// An event of the upload's progress, as far as it's read.
#[derive(Deserialize)]
struct Progress {
    /// `pending`, `processing`, `completed` or `failed`.
    status: String,
    track_id: Option<u64>,
    message: Option<String>,
    error: Option<String>,
    #[serde(default)]
    warnings: Vec<String>,
}

/// The parts of a file of `size` bytes in parts of `part` bytes: each one's number (from 1), where
/// it starts and how long it is, every one `part` long but the last.
pub(crate) fn parts(size: u64, part: u64) -> impl Iterator<Item = (u64, u64, u64)> {
    (1..=size.div_ceil(part)).map(move |n| {
        let start = (n - 1) * part;
        (n, start, part.min(size - start))
    })
}

/// Uploads `recording` (`size` bytes, named `filename`) as `track`, each part through a file in
/// `scratch`: the track's id once plyr.fm has published it, and what plyr.fm warns of.
pub(crate) fn upload(
    host: &Host,
    recording: &Path,
    size: u64,
    filename: &str,
    track: &Track,
    scratch: &Path,
) -> Result<(u64, Vec<String>), RpcError> {
    let start = json!({ "filename": filename, "size_bytes": size }).to_string();
    let start = Fetch { body: Some(start), ..request("POST", "/tracks/uploads", Some("application/json")) };
    let session: Session = read(&ask(host, &start, "the upload", 1)?)?;
    let part = session.part_size_bytes;
    if !(1..=MAX_PART).contains(&part) || session.part_count != size.div_ceil(part) {
        return Err(RpcError::failed(
            "external",
            format!("plyr.fm asked for {} parts of {part} bytes", session.part_count),
        ));
    }
    let id = &session.upload_id;
    let io = |e: std::io::Error| RpcError::failed("io", format!("{}: {e}", recording.display()));
    let mut file = File::open(recording).map_err(io)?;
    let blob = scratch.join("part");
    let ctx = host.ctx();
    ctx.phase_start(Phase::Copy, session.part_count, size);
    for (n, at, len) in parts(size, part) {
        ctx.check()?;
        let mut bytes = vec![0; usize::try_from(len).expect("a part fits in memory")];
        file.seek(SeekFrom::Start(at)).and_then(|_| file.read_exact(&mut bytes)).map_err(io)?;
        std::fs::write(&blob, bytes).map_err(|e| RpcError::failed("io", format!("{}: {e}", blob.display())))?;
        let path = format!("/tracks/uploads/{id}/parts/{n}");
        let put = Fetch { body_blob: Some(blob.clone()), ..request("PUT", &path, Some("application/octet-stream")) };
        ask(host, &put, &format!("part {n} of {}", session.part_count), TRIES)?;
        ctx.emit(Event::Bytes { phase: Phase::Copy, bytes: len });
    }
    ctx.phase_end(Phase::Copy);
    let tags = serde_json::to_string(&TAGS).expect("tags are JSON");
    let mut form = form_urlencoded::Serializer::new(String::new());
    form.append_pair("title", &track.title).append_pair("tags", &tags);
    if !track.description.is_empty() {
        form.append_pair("description", &track.description);
    }
    let path = format!("/tracks/uploads/{id}/finish");
    let finish =
        Fetch { body: Some(form.finish()), ..request("POST", &path, Some("application/x-www-form-urlencoded")) };
    let queued: Queued = read(&ask(host, &finish, "the track", 1)?)?;
    published(host, &ctx, &queued.upload_id)
}

/// The track the upload `id` became, once plyr.fm has published it, and what it warns of.
fn published(host: &Host, ctx: &Ctx<'_>, id: &str) -> Result<(u64, Vec<String>), RpcError> {
    let progress = request("GET", &format!("/tracks/uploads/{id}/progress"), None);
    for _ in 0..TRIES {
        let stream = ask(host, &progress, "the upload's progress", TRIES)?;
        let last = stream.lines().rev().find_map(|l| l.strip_prefix("data:"));
        match last.map(|event| serde_json::from_str::<Progress>(event.trim())) {
            Some(Ok(Progress { status, track_id: Some(track), warnings, .. })) if status == "completed" => {
                return Ok((track, warnings));
            }
            Some(Ok(Progress { status, message, error, .. })) if status == "failed" => {
                let why = error.or(message).unwrap_or_default();
                return Err(RpcError::failed("external", format!("plyr.fm couldn't publish the track: {why}")));
            }
            // The stream stopped before the upload did: asked again, it goes on from where it is.
            _ => ctx.check()?,
        }
    }
    Err(RpcError::failed("external", format!("plyr.fm's progress kept stopping before upload {id} was published")))
}

/// A request to plyr.fm's `path`, with the token, saying what its body is when it has one.
fn request(method: &str, path: &str, content_type: Option<&str>) -> Fetch {
    let headers = [("Authorization", AUTHORIZATION)].into_iter().chain(content_type.map(|t| ("Content-Type", t)));
    Fetch {
        method: method.into(),
        headers: headers.map(|(k, v)| (k.into(), v.into())).collect(),
        ..Fetch::get(format!("{API}{path}"))
    }
}

/// What plyr.fm answers `fetch` with, which is `what`, asked for up to `tries` times while it
/// can't be reached or answers a server error.
fn ask(host: &Host, fetch: &Fetch, what: &str, tries: u32) -> Result<String, RpcError> {
    let (mut tried, mut wait) = (1, FIRST_WAIT);
    loop {
        let failed = match host.fetch(fetch) {
            Ok(Fetched { status: 200..=299, body, .. }) => return Ok(body.unwrap_or_default()),
            Ok(answer) if matches!(answer.status, 408 | 429 | 500..) => refused(&answer, what),
            Ok(answer) => return Err(refused(&answer, what)),
            Err(e) if e.kind() == Some("external") => {
                RpcError { message: format!("couldn't send {what} to plyr.fm: {}", e.message), ..e }
            }
            // Cancelled, or no token: asking again changes nothing.
            Err(e) => return Err(e),
        };
        if tried == tries {
            return Err(failed);
        }
        // The host relays this as the plugin's log: a failure reaches the user, a retry not.
        eprintln!("{}: asking again in {} ms", failed.message, wait.as_millis());
        pause(&host.ctx(), wait)?;
        (tried, wait) = (tried + 1, wait * 2);
    }
}

/// plyr.fm refusing `what`, in its words: the token when it's `401` or `403`.
fn refused(answer: &Fetched, what: &str) -> RpcError {
    #[derive(Deserialize)]
    struct Refusal {
        detail: String,
    }
    let status = answer.status;
    let detail = answer.body.as_deref().and_then(|b| serde_json::from_str::<Refusal>(b).ok()).map(|r| r.detail);
    let detail = detail.map(|d| format!(": {d}")).unwrap_or_default();
    match status {
        401 | 403 => RpcError::failed(
            "permission",
            format!("plyr.fm refused the token ({status}{detail}): make a new one at plyr.fm/settings#developer"),
        ),
        400..=499 => RpcError::failed("invalid", format!("plyr.fm refused {what} ({status}{detail})")),
        _ => RpcError::failed("external", format!("plyr.fm failed {what} ({status}{detail})")),
    }
}

/// plyr.fm's answer as `T`.
fn read<T: DeserializeOwned>(body: &str) -> Result<T, RpcError> {
    serde_json::from_str(body).map_err(|e| RpcError::failed("external", format!("plyr.fm's answer can't be read: {e}")))
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
