//! Mixcloud's upload (mixcloud.com/developers, "Uploading shows"): one `multipart/form-data` POST
//! holding the recording (as `mp3`, whatever its format), the show's fields and its picture,
//! answered `{"result": {"success", "message", "key"}}`, the key being the show's path,
//! `/<user>/<slug>/`. An error is `{"error": {"type", "message", "retry_after"?}}`, with `details`
//! (each field's problems) for a form Mixcloud refused: the shape the developer docs give its rate
//! limit in (`RateLimitException`, a `403` with `Retry-After`), and the one Mixcloud upload clients
//! read (github.com/Ruxton/mixcloud_uploader, `mixcloud/response.go`).
//!
//! The host streams the recording from the file as it sends the form. Mixcloud has no resumable
//! upload, so a POST that doesn't arrive, or that Mixcloud fails with a server error, is sent again
//! whole, a few times, waiting longer each time; one over Mixcloud's rate limit waits as long as
//! Mixcloud says, unless that's longer than publishing should wait.

use std::time::{Duration, Instant};

use loclib_plugin::{Fetch, Fetched, FormPart, Host, RpcError};
use loclib_progress::{Ctx, Phase};
use serde::Deserialize;
use serde_json::{Map, Value};

/// Where a show is uploaded, the account's token as the host fills it in.
pub(crate) const UPLOAD: &str = "https://api.mixcloud.com/upload/?access_token={account:mixcloud}";

/// Where Mixcloud shows a show, by its key.
pub(crate) const SHOWS: &str = "https://www.mixcloud.com";

/// How many times the form is sent before the upload fails.
pub(crate) const TRIES: u32 = 4;

/// A second of waiting. In this crate's tests it's a thousandth of one, so they wait out every
/// try without spending seconds on it.
const SECOND: Duration = if cfg!(test) { Duration::from_millis(1) } else { Duration::from_secs(1) };

/// The wait before the second try, in seconds, doubling for each try after.
const FIRST_WAIT: u64 = 5;

/// The longest Mixcloud's rate limit is waited out, in seconds: past it, the upload fails saying
/// when to publish again.
pub(crate) const MAX_RETRY_AFTER: u64 = 15 * 60;

/// How often a wait notices the hook being cancelled.
const CHECK_EVERY: Duration = Duration::from_millis(50);

/// Whether an upload Mixcloud refused is sent again: after the seconds Mixcloud asks for, when it
/// says.
enum Next {
    Again(Option<u64>),
    Stop,
}

/// Mixcloud's answer, as far as it's read.
#[derive(Deserialize, Default)]
struct Answer {
    result: Option<Done>,
    error: Option<Said>,
    /// Each field's problems, for a form Mixcloud refused, in its order.
    #[serde(default)]
    details: Map<String, Value>,
}

#[derive(Deserialize)]
struct Done {
    key: Option<String>,
}

#[derive(Deserialize, Default)]
struct Said {
    #[serde(rename = "type", default)]
    kind: String,
    #[serde(default)]
    message: String,
    retry_after: Option<u64>,
}

/// Uploads `form` (sending `bytes` of files): the show's page.
pub(crate) fn upload(host: &Host, form: Vec<FormPart>, bytes: u64) -> Result<String, RpcError> {
    let post = Fetch { method: "POST".into(), multipart: form, ..Fetch::get(UPLOAD) };
    let ctx = host.ctx();
    let (mut tried, mut wait) = (1, FIRST_WAIT);
    loop {
        ctx.phase_start(Phase::Upload, 1, bytes);
        let answer = host.fetch(&post);
        ctx.phase_end(Phase::Upload);
        let (failed, asked) = match answer {
            Ok(answer @ Fetched { status: 200..=299, .. }) => return page(&answer),
            Ok(answer) => match refused(&answer) {
                (Next::Again(asked), failed) => (failed, asked),
                (Next::Stop, failed) => return Err(failed),
            },
            Err(e) if e.kind() == Some("external") => {
                (RpcError { message: format!("couldn't send the show to Mixcloud: {}", e.message), ..e }, None)
            }
            // Cancelled, or not signed in: sending it again changes nothing.
            Err(e) => return Err(e),
        };
        if tried == TRIES {
            return Err(failed);
        }
        let secs = asked.unwrap_or(wait);
        // The host relays this as the plugin's log: a failure reaches the user, a retry not.
        eprintln!("{}: sending it again in {secs} s", failed.message);
        pause(&ctx, SECOND * secs.min(MAX_RETRY_AFTER) as u32)?;
        (tried, wait) = (tried + 1, wait * 2);
    }
}

/// The show's page, from Mixcloud's answer to an upload that went.
fn page(answer: &Fetched) -> Result<String, RpcError> {
    let read: Answer = serde_json::from_str(answer.body.as_deref().unwrap_or_default())
        .map_err(|e| RpcError::failed("external", format!("Mixcloud's answer can't be read: {e}")))?;
    let key = read.result.and_then(|r| r.key).unwrap_or_default();
    // `/<user>/<slug>/`: two names, nothing an address would read otherwise.
    let parts: Vec<&str> =
        key.strip_prefix('/').and_then(|k| k.strip_suffix('/')).unwrap_or_default().split('/').collect();
    let named = |p: &&str| !p.is_empty() && !p.contains(|c: char| c.is_whitespace() || matches!(c, '?' | '#' | '\\'));
    match parts[..] {
        [_, _] if parts.iter().all(named) => Ok(format!("{SHOWS}{key}")),
        _ => Err(RpcError::failed("external", format!("Mixcloud's answer named no show: {key:?}"))),
    }
}

/// Mixcloud refusing the upload, in its words, and whether to send it again.
fn refused(answer: &Fetched) -> (Next, RpcError) {
    let status = answer.status;
    let read: Answer = answer.body.as_deref().and_then(|b| serde_json::from_str(b).ok()).unwrap_or_default();
    let said = read.error.unwrap_or_default();
    let mut words = match (said.kind.as_str(), said.message.as_str()) {
        ("", "") => status.to_string(),
        ("", message) => format!("{status}: {message}"),
        (kind, message) => format!("{status} {kind}: {message}"),
    };
    let details: Vec<String> = read.details.iter().map(|(field, why)| format!("{field}: {}", problems(why))).collect();
    if !details.is_empty() {
        words += &format!("; {}", details.join("; "));
    }
    let failed = |kind: &str, why: &str| {
        let verb = if status >= 500 { "failed" } else { "refused" };
        let why = if why.is_empty() { String::new() } else { format!(": {why}") };
        RpcError::failed(kind, format!("Mixcloud {verb} the show ({words}){why}"))
    };
    if said.kind == "RateLimitException" || status == 429 {
        let header = answer.headers.get("retry-after").and_then(|s| s.trim().parse::<u64>().ok());
        return match header.or(said.retry_after) {
            Some(secs) if secs > MAX_RETRY_AFTER => {
                let why = format!("it's over its rate limit: publish again in {} minutes", secs.div_ceil(60));
                (Next::Stop, failed("external", &why))
            }
            secs => (Next::Again(secs), failed("external", "it's over its rate limit")),
        };
    }
    let limit = format!("{} {}", said.kind, said.message).to_lowercase().contains("limit");
    match status {
        408 | 500.. => (Next::Again(None), failed("external", "")),
        401 => {
            (Next::Stop, failed("permission", "sign in again: locallibrary account mixcloud sign-in --for mixcloud"))
        }
        400..=499 if limit => {
            let why = "a free Mixcloud account keeps 10 shows published: move one to your drafts, or go Pro";
            (Next::Stop, failed("invalid", why))
        }
        403 => (Next::Stop, failed("permission", "")),
        _ => (Next::Stop, failed("invalid", "")),
    }
}

/// A field's problems as Mixcloud lists them, in a line.
fn problems(why: &Value) -> String {
    match why {
        Value::Array(each) => {
            each.iter().map(|w| w.as_str().map_or_else(|| w.to_string(), String::from)).collect::<Vec<_>>().join(" ")
        }
        Value::String(why) => why.clone(),
        other => other.to_string(),
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
