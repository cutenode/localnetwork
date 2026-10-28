//! The youtube plugin: a set destination, `video`, that publishes a set's recording to YouTube as
//! a video on the signed-in Google account's channel. YouTube takes videos, not audio, so FFmpeg
//! makes one first, a still picture over the recording (see `render`), in the request's scratch
//! folder; then it goes up through YouTube's resumable upload (see `upload`), titled after the set
//! with its tracklist as chapters in the description (see `video`), and the plugin answers the
//! video's page, which locallibrary keeps as the set's link. The host signs in to the account and
//! fills its token into the plugin's requests.

mod render;
mod upload;
mod video;

use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use loclib_plugin::{Hooks, Host, PublishSet, Published, RpcError};
use serde_json::json;

/// `locallibrary-plugin.toml`, which locallibrary compiles in and holds the plugin to.
pub const MANIFEST: &str = include_str!("../locallibrary-plugin.toml");

/// Where YouTube shows a video, by its id.
const WATCH: &str = "https://www.youtube.com/watch?v=";

/// YouTube's category for music.
const MUSIC: &str = "10";

/// How long a video may be before YouTube wants the account verified, in seconds.
const UNVERIFIED_LENGTH: f64 = 15.0 * 60.0;

/// Said with every dry run: what YouTube does with a set once it's there.
const CONTENT_ID: &str = "YouTube's Content ID checks every video for the records it plays: a label can block the \
                          set or put ads on it and take what they earn";

/// Said once a set has gone up.
const PROCESSING: &str = "YouTube is still processing the video: it can take a while to play, and longer in HD";

/// Serves the plugin over `input` and `output` (stdin and stdout, for a real plugin) until `input`
/// closes.
pub fn serve(input: impl Read + Send + 'static, output: impl Write + Send + 'static) {
    loclib_plugin::serve(MANIFEST, Hooks::new().on(publish_set), input, output);
}

/// `set/publish` to `video`: a dry run reads the recording and says what would go, making and
/// sending nothing.
fn publish_set(req: PublishSet, host: &Host) -> Result<Published, RpcError> {
    let setting = |key: &str| req.context.settings.get(key).and_then(|v| v.as_str()).filter(|v| !v.is_empty());
    let privacy = setting("privacy").unwrap_or("unlisted");
    let picture = setting("picture").map(PathBuf::from);
    if let Some(picture) = picture.as_deref().filter(|p| !p.is_absolute() || !p.is_file()) {
        let why = format!(
            "the picture setting, {}, isn't a file (locallibrary plugin settings youtube picture)",
            picture.display()
        );
        return Err(RpcError::failed("invalid", why));
    }
    let recording = &req.recording;
    let audio = render::probe(recording)?;
    let length = audio.seconds as u32;
    let details = video::of(&req.set, &req.tracklist, length);
    if req.dry_run {
        return Ok(Published {
            url: None,
            warnings: dry_run(&req, &details, privacy, picture.as_deref(), audio.seconds),
            ..Published::default()
        });
    }
    let scratch = req
        .context
        .scratch
        .as_deref()
        .ok_or_else(|| RpcError::invalid_params("no scratch folder to make the video in"))?;
    let bytes = std::fs::metadata(recording)
        .map_err(|e| RpcError::failed("io", format!("{}: {e}", recording.display())))?
        .len();
    let mp4 = scratch.join("video.mp4");
    let ctx = host.ctx();
    let resource = json!({
        "snippet": { "title": details.title, "description": details.description, "categoryId": MUSIC },
        "status": { "privacyStatus": privacy },
    });
    let uploaded =
        render::render(recording, &audio, bytes, picture.as_deref(), &mp4, &ctx).and_then(
            |()| match std::fs::metadata(&mp4).map_or(0, |m| m.len()) {
                0 => Err(RpcError::failed("external", "ffmpeg made no video")),
                size => upload::upload(host, &mp4, size, &resource),
            },
        );
    // Gone before answering: the host refuses a scratch folder holding more than 512 MiB.
    let _ = std::fs::remove_file(&mp4);
    let uploaded = uploaded?;
    let mut warnings = details.notes;
    let made = uploaded.status.and_then(|s| s.privacy).filter(|p| p != privacy);
    if let Some(made) = made {
        warnings.push(format!("YouTube made the video {made} rather than {privacy}: change it in YouTube Studio"));
    }
    warnings.push(PROCESSING.into());
    Ok(Published { url: Some(format!("{WATCH}{}", uploaded.id)), warnings, ..Published::default() })
}

/// What a dry run says: what would go where, what was cut, and what YouTube does with it.
fn dry_run(
    req: &PublishSet,
    details: &video::Details,
    privacy: &str,
    picture: Option<&Path>,
    seconds: f64,
) -> Vec<String> {
    let name = |p: &Path| p.file_name().unwrap_or_default().to_string_lossy().into_owned();
    let shown = picture.map_or("a black frame".into(), name);
    let length = video::clock(seconds as u32);
    let mut what = format!(
        "{} ({length}) would go to YouTube as the {privacy} video \"{}\", {shown} over the audio",
        name(&req.recording),
        details.title
    );
    match (req.tracklist.len(), details.chapters) {
        (0, _) => {}
        (n, 0) => what += &format!(", its {n} tracks in the description"),
        (n, c) => what += &format!(", its {n} tracks in the description ({c} chapters)"),
    }
    let mut lines = vec![what];
    lines.extend(details.notes.iter().cloned());
    if seconds > UNVERIFIED_LENGTH {
        lines.push("YouTube takes videos over 15 minutes from verified accounts only (youtube.com/verify)".into());
    }
    lines.push(CONTENT_ID.into());
    lines
}

#[cfg(test)]
mod tests;
