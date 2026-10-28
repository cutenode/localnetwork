//! The upload: an identifier no item has yet (archive.org's `check_identifier.php`, as the
//! `internetarchive` library's `identifier_available` asks), then the recording in one PUT to the
//! Archive's S3 API, which makes the item with the metadata its headers carry
//! (archive.org/developers/ias3.html). The Archive processes it afterwards; nothing here waits.
//!
//! One PUT, as the `internetarchive` library and rclone send a file of any size: the host streams
//! the recording with its exact length (the Archive's S3 takes no chunked body), a 2 GiB one in
//! the 4.5 hours it gives a request sending that much, and the Archive documents nothing of its
//! multipart upload but that it has one. A PUT that doesn't arrive, or that the Archive fails, is
//! sent again whole, a few times, waiting longer each time. The Archive answers `503 SlowDown`
//! when its queue is full, often after the whole file has gone, so before each PUT its S3 is asked
//! whether it's over its limit (`check_limit`), and while it is the PUT waits its turn instead.
//! The Archive's derive (the MP3s it plays the item from, its waveform) is left to run.

use std::path::Path;
use std::time::{Duration, Instant};

use loclib_plugin::{Fetch, Fetched, Host, RpcError};
use loclib_progress::{Ctx, Phase};
use percent_encoding::utf8_percent_encode;
use serde::Deserialize;

use crate::item::{IDENTIFIERS, Item, KEPT, suffixed};

/// The Archive's S3 API.
pub(crate) const S3: &str = "https://s3.us.archive.org";

/// Where the Archive says whether an identifier is free.
pub(crate) const CHECK: &str = "https://archive.org/services/check_identifier.php";

/// The keys, as the host fills them in.
pub(crate) const AUTHORIZATION: &str = "LOW {secret:access}:{secret:secret}";

/// How many times the recording is sent before the upload fails.
pub(crate) const TRIES: u32 = 4;

/// The wait before the second try, doubling for each try after: the `internetarchive` library
/// waits 30 seconds after a `503 SlowDown`. In this crate's tests it's a thousandth of a second,
/// so they wait out every try without spending minutes on it.
const FIRST_WAIT: Duration = if cfg!(test) { Duration::from_millis(1) } else { Duration::from_secs(30) };

/// How often a wait notices the hook being cancelled.
const CHECK_EVERY: Duration = Duration::from_millis(50);

/// The first of `base`, `base-2`... up to `base-10` that no item has, asking the Archive.
pub(crate) fn free(host: &Host, base: &str) -> Result<String, RpcError> {
    #[derive(Deserialize)]
    struct Checked {
        /// `available` or `not_available`.
        code: String,
    }
    for n in 1..=IDENTIFIERS {
        let id = suffixed(base, n);
        let check = Fetch::get(format!("{CHECK}?output=json&identifier={id}"));
        let answer = host.fetch(&check).map_err(|e| reaching(e, "the identifier check"))?;
        if !(200..=299).contains(&answer.status) {
            return Err(refused(&answer, "the identifier check"));
        }
        let checked: Checked = serde_json::from_str(answer.body.as_deref().unwrap_or_default())
            .map_err(|e| RpcError::failed("external", format!("the Archive's identifier check can't be read: {e}")))?;
        match checked.code.as_str() {
            "available" => return Ok(id),
            "not_available" => eprintln!("archive.org/details/{id} is taken"),
            code => {
                return Err(RpcError::failed(
                    "external",
                    format!("the Archive's identifier check said {code} of {id}"),
                ));
            }
        }
    }
    let last = suffixed(base, IDENTIFIERS);
    Err(RpcError::failed("external", format!("archive.org/details/{base} and the next ones up to {last} are taken")))
}

/// Uploads `recording` (`size` bytes, named `filename`) as the item `id`, with `item`'s metadata.
pub(crate) fn upload(
    host: &Host,
    recording: &Path,
    size: u64,
    filename: &str,
    id: &str,
    item: &Item,
) -> Result<(), RpcError> {
    let url = format!("{S3}/{id}/{}", utf8_percent_encode(filename, KEPT));
    let size_hint = size.to_string();
    let headers = [
        ("authorization", AUTHORIZATION),
        ("x-archive-auto-make-bucket", "1"),
        ("x-archive-size-hint", size_hint.as_str()),
    ];
    let headers = headers.into_iter().map(|(k, v)| (k.to_string(), v.to_string())).chain(item.headers()).collect();
    let put = Fetch { method: "PUT".into(), headers, body_blob: Some(recording.to_path_buf()), ..Fetch::get(url) };
    let ctx = host.ctx();
    let (mut tried, mut wait) = (1, FIRST_WAIT);
    loop {
        let failed = if busy(host, id)? {
            RpcError::failed("external", "the Archive's S3 is over its limit for now")
        } else {
            ctx.phase_start(Phase::Upload, 1, size);
            let answer = host.fetch(&put);
            ctx.phase_end(Phase::Upload);
            match answer {
                Ok(Fetched { status: 200..=299, .. }) => return Ok(()),
                Ok(answer) if spam(&answer) => return Err(refused(&answer, filename)),
                Ok(answer) if matches!(answer.status, 408 | 429 | 500..) => refused(&answer, filename),
                Ok(answer) => return Err(refused(&answer, filename)),
                Err(e) if e.kind() == Some("external") => reaching(e, filename),
                // Cancelled, or no keys: sending it again changes nothing.
                Err(e) => return Err(e),
            }
        };
        if tried == TRIES {
            return Err(failed);
        }
        // The host relays this as the plugin's log: a failure reaches the user, a retry not.
        eprintln!("{}: sending it again in {} ms", failed.message, wait.as_millis());
        pause(&ctx, wait)?;
        (tried, wait) = (tried + 1, wait * 2);
    }
}

/// Whether the Archive's S3 says it's over its limit for `id`'s uploads with these keys. An
/// answer that can't be read says nothing, and the PUT goes: its own answer says.
fn busy(host: &Host, id: &str) -> Result<bool, RpcError> {
    #[derive(Deserialize)]
    struct Limit {
        over_limit: u8,
    }
    let check = Fetch::get(format!("{S3}/?check_limit=1&accesskey={{secret:access}}&bucket={id}"));
    match host.fetch(&check) {
        Ok(Fetched { status: 200..=299, body, .. }) => {
            Ok(serde_json::from_str::<Limit>(body.as_deref().unwrap_or_default()).is_ok_and(|l| l.over_limit != 0))
        }
        Ok(_) => Ok(false),
        Err(e) if e.kind() == Some("external") => Ok(false),
        Err(e) => Err(e),
    }
}

/// The host not reaching the Archive with `what`.
fn reaching(e: RpcError, what: &str) -> RpcError {
    match e.kind() {
        Some("external") => RpcError { message: format!("couldn't send {what} to the Archive: {}", e.message), ..e },
        _ => e,
    }
}

/// Whether the Archive took the upload for spam, which sending again won't change.
fn spam(answer: &Fetched) -> bool {
    answer.status == 503 && answer.body.as_deref().is_some_and(|b| b.contains("appears to be spam"))
}

/// The Archive refusing `what`, in its words (its S3's `<Error>`): the keys when it's `401` or
/// `403`.
fn refused(answer: &Fetched, what: &str) -> RpcError {
    #[derive(Deserialize)]
    struct S3Error {
        #[serde(rename = "Code")]
        code: String,
        #[serde(rename = "Message", default)]
        message: String,
    }
    let status = answer.status;
    let said = answer.body.as_deref().and_then(|b| quick_xml::de::from_str::<S3Error>(b).ok());
    let said = match said {
        Some(S3Error { code, message }) if message.is_empty() => format!(" {code}"),
        Some(S3Error { code, message }) => format!(" {code}: {message}"),
        None => String::new(),
    };
    match status {
        401 | 403 => RpcError::failed(
            "permission",
            format!("the Archive refused the keys ({status}{said}): check them at archive.org/account/s3.php"),
        ),
        _ if spam(answer) => RpcError::failed("invalid", format!("the Archive took {what} for spam ({status}{said})")),
        400..=499 => RpcError::failed("invalid", format!("the Archive refused {what} ({status}{said})")),
        _ => RpcError::failed("external", format!("the Archive failed {what} ({status}{said})")),
    }
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
