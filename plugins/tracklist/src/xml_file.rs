//! The rekordbox XML file the importer and the library source read, and their answers too big
//! for a message.

use std::path::Path;

use loclib_plugin::RpcError;
use loclib_plugin::framing::MAX_LINE;
use loclib_tracklist::{self as tracklist, XmlLibrary};
use serde::Serialize;

/// The rekordbox XML file at `file`, read with `read` ([`tracklist::read_xml_library`], or
/// [`tracklist::read_xml_collection`] without the playlists).
pub(crate) fn read(file: &Path, read: fn(&[u8]) -> tracklist::Result<XmlLibrary>) -> Result<XmlLibrary, RpcError> {
    let shown = file.display();
    let bytes = std::fs::read(file).map_err(|e| RpcError::failed("io", format!("{shown}: {e}")))?;
    read(&bytes).map_err(|e| RpcError::failed("invalid", format!("{shown}: {e}")))
}

/// `answer` written to `name` in the request's `scratch` folder when its JSON is more than a
/// quarter of what a message holds: its name there. None when it fits, or without a folder.
pub(crate) fn spilled(answer: &impl Serialize, scratch: Option<&Path>, name: &str) -> Result<Option<String>, RpcError> {
    let Some(scratch) = scratch else { return Ok(None) };
    let json = serde_json::to_vec(answer).map_err(|e| RpcError::internal(e.to_string()))?;
    if json.len() <= MAX_LINE / 4 {
        return Ok(None);
    }
    std::fs::write(scratch.join(name), json).map_err(|e| RpcError::failed("io", format!("{name}: {e}")))?;
    Ok(Some(name.into()))
}

/// A cue's colour as cue edits take one.
pub(crate) fn hex([r, g, b]: [u8; 3]) -> String {
    format!("#{r:02x}{g:02x}{b:02x}")
}
