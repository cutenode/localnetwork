//! The soundcloud plugin: a set destination, `track`, that uploads a set's recording to SoundCloud
//! as a track of the signed-in account's (see `upload`), titled after the set and with its
//! tracklist as the description (see `track`), and answers the track's page, which locallibrary
//! keeps as the set's link. The host signs in to the account and fills its token into the
//! plugin's requests.

mod track;
mod upload;

use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use loclib_plugin::{FormPart, Hooks, Host, PublishSet, Published, RpcError};

/// What the set's link is, as its record names it.
const NAME: &str = "SoundCloud";

/// `locallibrary-plugin.toml`, which locallibrary compiles in and holds the plugin to.
pub const MANIFEST: &str = include_str!("../locallibrary-plugin.toml");

/// The recordings SoundCloud takes, by extension (its help centre's "Upload Requirements"; ALAC
/// is in `.m4a`).
const FORMATS: [&str; 15] =
    ["wav", "flac", "aif", "aiff", "mp3", "aac", "ogg", "mp4", "mp2", "m4a", "3gp", "3g2", "mj2", "wma", "amr"];

/// The biggest recording SoundCloud takes: 4 GB, read as the smaller of what that could mean.
const MAX_BYTES: u64 = 4_000_000_000;

/// The artwork SoundCloud takes, by extension, and the biggest: "a .jpg or .png ... under 2MB".
const PICTURES: [&str; 3] = ["jpg", "jpeg", "png"];
const MAX_PICTURE: u64 = 2_000_000;

/// Said with every dry run: what SoundCloud does with a mix, and how much a free account may
/// upload.
const RIGHTS: &str = "SoundCloud takes down a mix of other people's music unless every track's rights holders allow \
                      it, and closes an account after three copyright strikes";
const FREE_TIER: &str = "a free SoundCloud account may upload 2 hours of audio in all, a set too long for what's left \
                         won't go";

/// Said once a set has gone up.
const TRANSCODING: &str = "SoundCloud is still transcoding the track: it can take a while to play";

/// Serves the plugin over `input` and `output` (stdin and stdout, for a real plugin) until `input`
/// closes.
pub fn serve(input: impl Read + Send + 'static, output: impl Write + Send + 'static) {
    loclib_plugin::serve(MANIFEST, Hooks::new().on(publish_set), input, output);
}

/// `set/publish` to `track`: a recording or artwork SoundCloud wouldn't take is refused before
/// anything goes, and a dry run says what would go, sending nothing.
fn publish_set(req: PublishSet, host: &Host) -> Result<Published, RpcError> {
    let setting = |key: &str| req.context.settings.get(key).and_then(|v| v.as_str()).filter(|v| !v.is_empty());
    let sharing = setting("sharing").unwrap_or("private");
    let recording = &req.recording;
    let filename = name(recording);
    let format = extension(recording);
    if !FORMATS.contains(&format.as_str()) {
        let takes = "WAV, FLAC, AIFF, MP3, AAC, Ogg, MP4, MP2, M4A, 3GP, 3G2, MJ2, WMA and AMR";
        return Err(RpcError::failed(
            "invalid",
            format!("SoundCloud takes {takes} recordings, which {filename} isn't"),
        ));
    }
    let size = bytes(recording)?;
    if size == 0 {
        return Err(RpcError::failed("invalid", format!("{filename} is empty")));
    }
    if size > MAX_BYTES {
        let why = format!("SoundCloud takes recordings of up to {}, and {filename} is {}", mb(MAX_BYTES), mb(size));
        return Err(RpcError::failed("invalid", why));
    }
    let picture = setting("picture").map(PathBuf::from);
    let artwork = picture.as_deref().map(|p| artwork(p).map(|bytes| (p, bytes))).transpose()?;
    let track = track::of(&req.set, &req.tracklist);
    if req.dry_run {
        let mut what =
            format!("{filename} ({}) would go to SoundCloud as the {sharing} track \"{}\"", mb(size), track.title);
        if let Some((picture, _)) = artwork {
            what += &format!(" with {} as its artwork", name(picture));
        }
        if !req.tracklist.is_empty() {
            what += &format!(", its {} tracks in the description", req.tracklist.len());
        }
        let lines = [vec![what], track.notes, vec![RIGHTS.into(), FREE_TIER.into()]].concat();
        return Ok(Published { warnings: lines, ..Published::default() });
    }
    let text = |name: &str, value: &str| FormPart {
        name: format!("track[{name}]"),
        value: Some(value.into()),
        ..FormPart::default()
    };
    // SoundCloud makes a track downloadable unless it's told not to.
    let mut form = vec![
        text("title", &track.title),
        text("sharing", sharing),
        text("downloadable", "false"),
        text("tag_list", track::TAGS),
    ];
    if !track.description.is_empty() {
        form.push(text("description", &track.description));
    }
    if let Some(genre) = setting("genre") {
        form.push(text("genre", genre));
    }
    form.push(FormPart {
        name: "track[asset_data]".into(),
        file: Some(recording.clone()),
        filename: Some(filename),
        ..FormPart::default()
    });
    let mut sending = size;
    if let Some((picture, bytes)) = artwork {
        // The host sends only the recording it lent and files in the scratch folder.
        let scratch = req
            .context
            .scratch
            .as_deref()
            .ok_or_else(|| RpcError::invalid_params("no scratch folder to send the artwork from"))?;
        let copy = scratch.join(format!("artwork.{}", extension(picture)));
        let copied = std::fs::copy(picture, &copy);
        copied.map_err(|e| RpcError::failed("io", format!("{} to {}: {e}", picture.display(), copy.display())))?;
        form.push(FormPart {
            name: "track[artwork_data]".into(),
            filename: Some(name(&copy)),
            file: Some(copy),
            ..FormPart::default()
        });
        sending += bytes;
    }
    let uploaded = upload::upload(host, &track.title, form, sending)?;
    let warnings = [track.notes, vec![TRANSCODING.into()]].concat();
    Ok(Published { url: Some(uploaded.permalink_url), name: Some(NAME.into()), warnings, ..Published::default() })
}

/// The artwork setting's `picture`'s size, once it's one SoundCloud takes.
fn artwork(picture: &Path) -> Result<u64, RpcError> {
    let shown = picture.display();
    let refused = |why: String| {
        Err(RpcError::failed("invalid", format!("{why} (locallibrary plugin settings soundcloud picture)")))
    };
    if !picture.is_absolute() || !picture.is_file() {
        return refused(format!("the picture setting, {shown}, isn't a file"));
    }
    if !PICTURES.contains(&extension(picture).as_str()) {
        return refused(format!("SoundCloud takes JPEG and PNG artwork, which {shown} isn't"));
    }
    let size = bytes(picture)?;
    if size > MAX_PICTURE {
        return refused(format!("SoundCloud takes artwork of up to {}, and {shown} is {}", mb(MAX_PICTURE), mb(size)));
    }
    Ok(size)
}

/// `path`'s file name.
fn name(path: &Path) -> String {
    path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

/// `path`'s extension, in lowercase.
fn extension(path: &Path) -> String {
    path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default()
}

/// How big the file at `path` is.
fn bytes(path: &Path) -> Result<u64, RpcError> {
    let meta = std::fs::metadata(path).map_err(|e| RpcError::failed("io", format!("{}: {e}", path.display())))?;
    Ok(meta.len())
}

/// `bytes` in megabytes, as SoundCloud counts them.
fn mb(bytes: u64) -> String {
    format!("{:.1} MB", bytes as f64 / 1e6)
}

#[cfg(test)]
mod tests;
