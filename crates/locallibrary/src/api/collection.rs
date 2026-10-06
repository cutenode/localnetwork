//! rekordbox's own collection (`master.db`) on this computer, read only, and what a drive lacks
//! against it.

use std::path::PathBuf;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::resolve_drive;
use crate::masterdb::{self, Collection, CollectionInfo, DriveDiff, TagCategory};
use crate::model::Library;
use crate::{Error, Result};

/// Which `master.db` to read, and how.
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct CollectionRef {
    /// Default: `$LOCALLIBRARY_MASTERDB`, else rekordbox's own (see [`masterdb::default_path`]).
    #[serde(default)]
    pub db: Option<PathBuf>,
    /// SQLCipher passphrase. Default: `$LOCALLIBRARY_MASTERDB_KEY`, else the one every rekordbox
    /// 6/7 installation uses.
    #[serde(default)]
    pub key: Option<String>,
}

/// Read the collection `req` names.
pub fn open_collection(req: &CollectionRef) -> Result<Collection> {
    let path = match &req.db {
        Some(p) => p.clone(),
        None => masterdb::default_path().ok_or_else(|| {
            Error::NotFound(
                "rekordbox's collection: give the master.db to read (rekordbox runs on macOS and Windows)".into(),
            )
        })?,
    };
    if !path.is_file() {
        return Err(Error::NotFound(format!(
            "rekordbox collection {} (is rekordbox 6 or 7 installed?)",
            path.display()
        )));
    }
    masterdb::open(&path, &masterdb::resolve_key(req.key.as_deref()))
}

pub fn collection_info(req: &CollectionRef) -> Result<CollectionInfo> {
    Ok(open_collection(req)?.info())
}

/// Everything `collection.library` returns: counts, the library (tracks, playlists, history), cues
/// by track and My Tags.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct CollectionView {
    pub info: CollectionInfo,
    pub library: Library,
    pub cues: std::collections::BTreeMap<u32, Vec<crate::anlz::Cue>>,
    pub tags: Vec<TagCategory>,
}

pub fn collection_library(req: &CollectionRef) -> Result<CollectionView> {
    let c = open_collection(req)?;
    Ok(CollectionView { info: c.info(), library: c.library, cues: c.cues, tags: c.tags })
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct CollectionDiffRequest {
    #[serde(flatten)]
    pub collection: CollectionRef,
    pub drive: String,
    /// Only this playlist, or the playlists in this folder (and no track-level changes).
    #[serde(default)]
    pub playlist: Option<String>,
}

/// What the drive lacks against the collection (`diff.added_*`), and what only it has.
pub fn collection_diff(req: &CollectionDiffRequest) -> Result<DriveDiff> {
    let d = resolve_drive(&req.drive)?;
    let drive = d.library()?;
    let collection = open_collection(&req.collection)?;
    collection.diff_drive(&drive, &d.name(), req.playlist.as_deref())
}
