//! The mixcloud plugin: a set destination, `show`, that uploads a set's recording to Mixcloud as a
//! show of the signed-in account (see `upload`), titled after the set with its tracklist as the
//! show's sections (see `show`), and answers the show's page, which locallibrary keeps as the set's
//! link. The host signs in to the account and fills its token into the plugin's requests.

mod show;
mod upload;

use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use loclib_plugin::{FormPart, Hooks, Host, PublishSet, Published, RpcError};
use serde_json::Value;

/// `locallibrary-plugin.toml`, which locallibrary compiles in and holds the plugin to.
pub const MANIFEST: &str = include_str!("../locallibrary-plugin.toml");

/// What the set's link is, as its record names it.
const NAME: &str = "Mixcloud";

/// The recordings Mixcloud takes, by extension (help.mixcloud.com/hc/en-us/articles/360004031000:
/// "WAV, AIFF, FLAC or AAC, OGG, M4A, MP2, MP3 or WMAs").
const FORMATS: [&str; 10] = ["mp3", "m4a", "aac", "wav", "aiff", "aif", "flac", "ogg", "mp2", "wma"];

/// The biggest recording Mixcloud takes: "No larger than 4.0GB", read as decimal gigabytes, the
/// smaller of the two it could mean.
const MAX_BYTES: u64 = 4_000_000_000;

/// The biggest picture: "No larger than 10.0MB", read the same way.
const MAX_PICTURE: u64 = 10_000_000;

/// Said with every dry run: what Mixcloud does with a set once it's there.
const MIXCLOUD: &str = "Mixcloud licenses the music in DJ mixes. A free account keeps 10 shows published, and \
                        listeners without Premium can rewind a show only a few times an hour";

/// Serves the plugin over `input` and `output` (stdin and stdout, for a real plugin) until `input`
/// closes.
pub fn serve(input: impl Read + Send + 'static, output: impl Write + Send + 'static) {
    loclib_plugin::serve(MANIFEST, Hooks::new().on(publish_set), input, output);
}

/// `set/publish` to `show`: a recording or a picture Mixcloud wouldn't take is refused before
/// anything goes, and a dry run says what would go, sending nothing.
fn publish_set(req: PublishSet, host: &Host) -> Result<Published, RpcError> {
    let settings = &req.context.settings;
    let tags = settings.get("tags").and_then(Value::as_str).unwrap_or_default();
    let unlisted = settings.get("unlisted").and_then(Value::as_bool).unwrap_or(false);
    let picture = settings.get("picture").and_then(Value::as_str).filter(|p| !p.is_empty()).map(PathBuf::from);
    let recording = &req.recording;
    let format = recording.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    if !FORMATS.contains(&format.as_str()) {
        let takes = "MP3, M4A, AAC, WAV, AIFF, FLAC, OGG, MP2 and WMA";
        return Err(RpcError::failed(
            "invalid",
            format!("Mixcloud takes {takes} recordings, which {} isn't", name(recording)),
        ));
    }
    let size = size_of(recording)?;
    if size == 0 {
        return Err(RpcError::failed("invalid", format!("{} is empty", name(recording))));
    }
    if size > MAX_BYTES {
        let why = format!(
            "Mixcloud takes recordings of up to {}, and {} is {}",
            amount(MAX_BYTES),
            name(recording),
            amount(size)
        );
        return Err(RpcError::failed("invalid", why));
    }
    let picture_size = match &picture {
        Some(p) if !p.is_absolute() || !p.is_file() => {
            let why = format!(
                "the picture setting, {}, isn't a file (locallibrary plugin settings mixcloud picture)",
                p.display()
            );
            return Err(RpcError::failed("invalid", why));
        }
        Some(p) => match size_of(p)? {
            big if big > MAX_PICTURE => {
                let why = format!(
                    "Mixcloud takes pictures of up to {}, and {} is {}",
                    amount(MAX_PICTURE),
                    name(p),
                    amount(big)
                );
                return Err(RpcError::failed("invalid", why));
            }
            fits => fits,
        },
        None => 0,
    };
    let show = show::of(&req.set, &req.tracklist, tags, unlisted);
    if req.dry_run {
        return Ok(Published { warnings: dry_run(&req, &show, picture.as_deref(), size), ..Published::default() });
    }
    let mp3 = FormPart {
        name: "mp3".into(),
        file: Some(recording.clone()),
        filename: Some(name(recording)),
        ..FormPart::default()
    };
    let mut form = vec![mp3];
    form.extend(show.fields());
    if let Some(picture) = &picture {
        // The host sends only the recording and files in the request's scratch folder.
        let scratch = req
            .context
            .scratch
            .as_deref()
            .ok_or_else(|| RpcError::invalid_params("no scratch folder to send the picture from"))?;
        let copy = scratch.join(name(picture));
        std::fs::copy(picture, &copy).map_err(|e| RpcError::failed("io", format!("{}: {e}", picture.display())))?;
        form.push(FormPart {
            name: "picture".into(),
            file: Some(copy),
            filename: Some(name(picture)),
            ..FormPart::default()
        });
    }
    let url = upload::upload(host, form, size + picture_size)?;
    Ok(Published { url: Some(url), name: Some(NAME.into()), warnings: show.notes, ..Published::default() })
}

/// What a dry run says: what would go where, what was cut or left out, and what Mixcloud does
/// with it.
fn dry_run(req: &PublishSet, show: &show::Show, picture: Option<&Path>, size: u64) -> Vec<String> {
    let listed = if show.unlisted { "unlisted show" } else { "show" };
    let mut what =
        format!("{} ({}) would go to Mixcloud as the {listed} \"{}\"", name(&req.recording), amount(size), show.name);
    let timed = show.sections.iter().filter(|s| s.start.is_some()).count();
    match show.sections.len() {
        0 => {}
        n => what += &format!(", its {n} tracks as the tracklist ({timed} timed)"),
    }
    if !show.tags.is_empty() {
        what += &format!(", tagged {}", show.tags.join(", "));
    }
    if let Some(picture) = picture {
        what += &format!(", {} as its picture", name(picture));
    }
    [vec![what], show.notes.clone(), vec![MIXCLOUD.into()]].concat()
}

/// A file's name, as Mixcloud is told it.
fn name(path: &Path) -> String {
    path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

fn size_of(path: &Path) -> Result<u64, RpcError> {
    std::fs::metadata(path).map(|m| m.len()).map_err(|e| RpcError::failed("io", format!("{}: {e}", path.display())))
}

/// `bytes` in decimal megabytes, or gigabytes from one, as Mixcloud gives its limits.
fn amount(bytes: u64) -> String {
    match bytes as f64 {
        b if b >= 1e9 => format!("{:.2} GB", b / 1e9),
        b => format!("{:.1} MB", b / 1e6),
    }
}

#[cfg(test)]
mod tests;
