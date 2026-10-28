//! The archive plugin: a set destination, `item`, that uploads a set's recording to the Internet
//! Archive as an item in Community Audio (see `upload`), titled after the set and with its
//! tracklist as the description (see `item`), and answers the item's page, which locallibrary
//! keeps as the set's link. The Archive's S3 API takes an account's pair of keys, the secret
//! settings `access` and `secret`, which the host fills into the plugin's requests.

mod item;
mod upload;

use std::io::{Read, Write};

use loclib_plugin::{Hooks, Host, PublishSet, Published, RpcError};
use serde_json::Value;

/// `locallibrary-plugin.toml`, which locallibrary compiles in and holds the plugin to.
pub const MANIFEST: &str = include_str!("../locallibrary-plugin.toml");

/// Where the Archive shows an item, by its identifier.
const DETAILS: &str = "https://archive.org/details/";

/// Serves the plugin over `input` and `output` (stdin and stdout, for a real plugin) until `input`
/// closes.
pub fn serve(input: impl Read + Send + 'static, output: impl Write + Send + 'static) {
    loclib_plugin::serve(MANIFEST, Hooks::new().on(publish_set), input, output);
}

/// `set/publish` to `item`: an empty recording is refused before anything goes, and a dry run
/// says what would go.
fn publish_set(req: PublishSet, host: &Host) -> Result<Published, RpcError> {
    let recording = &req.recording;
    let filename = recording.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let size = std::fs::metadata(recording)
        .map_err(|e| RpcError::failed("io", format!("{}: {e}", recording.display())))?
        .len();
    if size == 0 {
        return Err(RpcError::failed("invalid", format!("{filename} is empty")));
    }
    let creator = req.context.settings.get("creator").and_then(Value::as_str);
    let item = item::of(&req.set, creator, &req.tracklist);
    if req.dry_run {
        let mib = format!("{:.1} MiB", size as f64 / f64::from(1 << 20));
        let what = format!(
            "{filename} ({mib}) would go to the Internet Archive as {DETAILS}{} (or that with -2 and on, if it's taken)",
            item.identifier
        );
        let by = item.creator.as_ref().map(|c| format!(" by {c},")).unwrap_or_default();
        let listed = match req.tracklist.len() {
            0 => String::new(),
            n => format!(", its {n} tracks in the description"),
        };
        let about = format!(
            "titled \"{}\",{by} dated {}, in Community Audio, tagged {}{listed}",
            item.title,
            item.date,
            item.subjects.join(", ")
        );
        let rights = "the Archive is for what you have the rights to share: it takes down mixes of others' records";
        return Ok(Published {
            warnings: [vec![what, about, rights.into()], item.notes].concat(),
            ..Published::default()
        });
    }
    let id = upload::free(host, &item.identifier)?;
    upload::upload(host, recording, size, &filename, &id, &item)?;
    let mut warnings = item.notes;
    if id != item.identifier {
        warnings.push(format!("{DETAILS}{} is taken, so it's {id}", item.identifier));
    }
    warnings.push("the Archive can take a few minutes to show it, while it processes the upload".into());
    Ok(Published { url: Some(format!("{DETAILS}{id}")), warnings, ..Published::default() })
}

#[cfg(test)]
mod tests;
