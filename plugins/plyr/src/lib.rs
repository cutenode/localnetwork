//! The plyr plugin: a set destination, `track`, that uploads a set's recording to plyr.fm (an AT
//! Protocol audio host) as a track of the user's (see `upload`), titled after the set and with its
//! tracklist as the description (see `track`), and answers the track's page, which locallibrary
//! keeps as the set's link, and the track's `fm.plyr.track` record, which the set's record points
//! to. plyr.fm takes a developer token, the secret setting `token`, which the host fills into the
//! plugin's requests.

mod track;
mod upload;

use std::io::{Read, Write};

use loclib_plugin::{Hooks, Host, PublishSet, Published, RpcError};

/// `locallibrary-plugin.toml`, which locallibrary compiles in and holds the plugin to.
pub const MANIFEST: &str = include_str!("../locallibrary-plugin.toml");

/// What the set's link is, as its record names it.
const NAME: &str = "plyr.fm";

/// Where plyr.fm shows a track, by its id.
const TRACK_PAGE: &str = "https://plyr.fm/track/";

/// The recordings plyr.fm takes, by extension (`backend/src/backend/utilities/audio_formats.py`,
/// `.aif` as `.aiff`).
const FORMATS: [&str; 8] = ["mp3", "wav", "m4a", "aiff", "aif", "flac", "webm", "ogg"];

/// The biggest recording plyr.fm takes, 1536 MiB (`backend/src/backend/upload_contract.json`).
const MAX_BYTES: u64 = 1536 << 20;

/// Serves the plugin over `input` and `output` (stdin and stdout, for a real plugin) until `input`
/// closes.
pub fn serve(input: impl Read + Send + 'static, output: impl Write + Send + 'static) {
    loclib_plugin::serve(MANIFEST, Hooks::new().on(publish_set), input, output);
}

/// `set/publish` to `track`: a recording plyr.fm wouldn't take is refused before anything goes,
/// and a dry run says what would go.
fn publish_set(req: PublishSet, host: &Host) -> Result<Published, RpcError> {
    let recording = &req.recording;
    let filename = recording.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let format = recording.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    if !FORMATS.contains(&format.as_str()) {
        let takes = "mp3, wav, m4a, aiff, flac, webm and ogg";
        return Err(RpcError::failed("invalid", format!("plyr.fm takes {takes} recordings, which {filename} isn't")));
    }
    let size = std::fs::metadata(recording)
        .map_err(|e| RpcError::failed("io", format!("{}: {e}", recording.display())))?
        .len();
    let mib = |bytes: u64| format!("{:.1} MiB", bytes as f64 / f64::from(1 << 20));
    if size == 0 {
        return Err(RpcError::failed("invalid", format!("{filename} is empty")));
    }
    if size > MAX_BYTES {
        let why = format!("plyr.fm takes recordings of up to {}, and {filename} is {}", mib(MAX_BYTES), mib(size));
        return Err(RpcError::failed("invalid", why));
    }
    let track = track::of(&req.set, &req.tracklist);
    if req.dry_run {
        let what = format!("{filename} ({}) would go to plyr.fm as \"{}\"", mib(size), track.title);
        let listed = match req.tracklist.len() {
            0 => String::new(),
            n => format!(", its {n} tracks in the description"),
        };
        let line = format!("{what}, tagged {}{listed}", track::TAGS.join(", "));
        return Ok(Published { warnings: [vec![line], track.notes].concat(), ..Published::default() });
    }
    let uploaded = upload::upload(host, recording, size, &filename, &track)?;
    Ok(Published {
        url: Some(format!("{TRACK_PAGE}{}", uploaded.id)),
        name: Some(NAME.into()),
        record: uploaded.record,
        warnings: [track.notes, uploaded.warnings].concat(),
    })
}

#[cfg(test)]
mod tests;
