//! The reads of rekordbox's files a backup needs (`collection:files`, see
//! [`crate::masterdb::files`]). The host answers them itself rather than through the app's reads
//! ([`super::callbacks::Reads`]): two name files in the request's scratch folder, which only the
//! host knows. `collection.copy` writes a new file there, as far as the folder has room, and
//! `collection.tracks` reads one there (or rekordbox's own `master.db`). Neither writes anywhere
//! else, rekordbox's folders least of all.

use std::path::{Path, PathBuf};

use loclib_plugin::code;
use serde::Deserialize;
use serde_json::Value;

use super::blobs;
use super::callbacks::invalid_params;
use super::process::Live;
use crate::api::{RpcFailure, collection_path};
use crate::masterdb::{self, files};
use crate::{Error, op};

/// The reads answered here.
const METHODS: [&str; 4] = ["collection.locations", "collection.status", "collection.copy", "collection.tracks"];

/// Whether the host answers the read `method` itself, here.
pub(crate) fn answers(method: &str) -> bool {
    METHODS.contains(&method)
}

/// `collection.copy`'s params.
#[derive(Deserialize)]
struct ToCopy {
    /// A new file in the request's scratch folder, absolute.
    into: PathBuf,
    /// Keep the cloud agent's sign-in rows, which are left out unless asked for.
    #[serde(default)]
    with_credentials: bool,
}

/// `collection.tracks`'s params.
#[derive(Deserialize)]
struct Tracks {
    /// A file in the request's scratch folder, absolute; rekordbox's own `master.db` when not given.
    db: Option<PathBuf>,
    #[serde(default)]
    offset: u64,
    /// Bounded by the host's check (see [`super::callbacks`]).
    limit: u64,
}

/// Answers the read `method` (one [`answers`]) with `params`, once the host has checked them, for
/// a plugin process whose requests' scratch folders `live` has, each holding at most `most` bytes.
pub(crate) fn answer(live: &Live, most: u64, method: &str, params: Value) -> Result<Value, RpcFailure> {
    let read = |e: serde_json::Error| invalid_params(e);
    let key = masterdb::resolve_key(None);
    let answer = match method {
        "collection.locations" => serde_json::to_value(files::locations()),
        "collection.status" => serde_json::to_value(files::status()),
        "collection.copy" => {
            let p: ToCopy = serde_json::from_value(params).map_err(read)?;
            let (dir, into) = in_scratch(live, &p.into)?;
            let made = files::export(&collection_path(None)?, &key, p.with_credentials, &into)?;
            if blobs::size(&dir) > most {
                let _ = std::fs::remove_file(&into);
                let why = format!("the copy is more than the scratch folder has room for ({})", op::bytes(most));
                return Err(RpcFailure::new(code::FAILED, "too_big", why));
            }
            serde_json::to_value(made)
        }
        // `collection.tracks`, the last of them.
        _ => {
            let p: Tracks = serde_json::from_value(params).map_err(read)?;
            let db = match &p.db {
                Some(db) => scratch_file(live, db)?,
                None => collection_path(None)?,
            };
            serde_json::to_value(files::tracks(&db, &key, p.offset, p.limit)?)
        }
    };
    Ok(answer.expect("an answer is JSON"))
}

/// The scratch folder under way that `path` (absolute) names a file in, and the file.
fn in_scratch(live: &Live, path: &Path) -> Result<(PathBuf, PathBuf), Error> {
    let (dir, name) = live.scratches.holding(path).map_err(Error::invalid)?;
    let file = dir.join(name);
    Ok((dir, file))
}

/// The file `path` (absolute) names in a scratch folder under way, as it's read: itself, not a
/// link or a file with another name (see [`blobs`]).
fn scratch_file(live: &Live, path: &Path) -> Result<PathBuf, Error> {
    let (_, file) = in_scratch(live, path)?;
    live.scratches.open(&file).map_err(Error::invalid)?;
    Ok(file)
}

#[cfg(test)]
mod tests;
