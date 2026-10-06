//! Playlist diffs between drives and snapshots, and what a restore would change.

use std::collections::HashSet;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::{RestoreBackup, resolve_drive};
use crate::backup::{self, BackupInfo};
use crate::compare::Side;
use crate::device::Device;
use crate::diff::{self, PlaylistDiff};
use crate::doctor::Databases;
use crate::model::Library;
use crate::paths;
use crate::store::{Snapshot, Store};
use crate::{Error, Result};

/// A library to read: a drive, or a snapshot in the store.
pub(super) enum Endpoint<'s> {
    Drive(Device),
    Snapshot(&'s Store, Snapshot),
}

impl Endpoint<'_> {
    pub(super) fn label(&self) -> String {
        match self {
            Endpoint::Drive(d) => d.name(),
            Endpoint::Snapshot(_, s) => format!("snapshot {}", s.id.short()),
        }
    }

    /// The library as players see it.
    pub(super) fn library(&self) -> Result<Library> {
        match self {
            Endpoint::Drive(d) => d.library(),
            Endpoint::Snapshot(store, s) => Ok(Side::from_snapshot(store, s, self.label())?.library),
        }
    }

    /// Both databases' libraries.
    pub(super) fn databases(&self) -> Result<Databases> {
        match self {
            Endpoint::Drive(d) => Databases::read(d),
            Endpoint::Snapshot(store, s) => Databases::from_snapshot(store, s),
        }
    }
}

/// A snapshot by id prefix, with or without `snapshot:` in front.
pub(super) fn snapshot(store: &Store, spec: &str) -> Result<Snapshot> {
    let prefix = spec.strip_prefix("snapshot:").unwrap_or(spec);
    store.load_snapshot(&store.resolve_snapshot(prefix)?)
}

/// `snapshot:<id prefix>`, a drive (path, name or id), or a library in the store (its latest
/// snapshot).
pub(super) fn resolve_endpoint<'s>(store: Option<&'s Store>, spec: &str) -> Result<Endpoint<'s>> {
    let need_store = || Error::invalid(format!("{spec:?} is in the store, but there is no store"));
    if spec.starts_with("snapshot:") {
        let store = store.ok_or_else(need_store)?;
        return Ok(Endpoint::Snapshot(store, snapshot(store, spec)?));
    }
    match resolve_drive(spec) {
        Ok(d) => Ok(Endpoint::Drive(d)),
        Err(e @ Error::NotFound(_)) => {
            let Some((store, lib)) = store.and_then(|s| s.find_library(spec).ok().flatten().map(|l| (s, l))) else {
                return Err(e);
            };
            let head = lib.head.ok_or_else(|| Error::NotFound(format!("library {:?} has no snapshots", lib.name)))?;
            Ok(Endpoint::Snapshot(store, store.load_snapshot(&head)?))
        }
        Err(e) => Err(e),
    }
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct DiffRequest {
    /// A drive, `snapshot:<id prefix>`, or a library (its latest snapshot).
    pub left: String,
    pub right: String,
    /// Only this playlist, or the playlists in this folder.
    #[serde(default)]
    pub playlist: Option<String>,
}

/// How the playlists of `right` differ from `left`'s.
pub fn diff(store: Option<&Store>, req: &DiffRequest) -> Result<PlaylistDiff> {
    let left = resolve_endpoint(store, &req.left)?;
    let right = resolve_endpoint(store, &req.right)?;
    let (l, r) = (left.library()?, right.library()?);
    diff::check_playlist(&l, &r, req.playlist.as_deref())?;
    Ok(diff::between(&l, &r, (&left.label(), &right.label()), req.playlist.as_deref()))
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct RestorePreview {
    pub backup: BackupInfo,
    /// From the drive as it is to the drive restored.
    pub diff: PlaylistDiff,
    /// Tracks in the backup whose audio is neither on the drive nor in the backup: restoring
    /// refuses them unless forced.
    pub missing_audio: Vec<String>,
}

/// What restoring a backup would change, without changing anything.
pub fn restore_preview(req: &RestoreBackup) -> Result<RestorePreview> {
    let d = resolve_drive(&req.drive)?;
    let backups = backup::list(&d)?;
    let info = match &req.backup {
        Some(id) => backups.into_iter().find(|b| &b.id == id).ok_or_else(|| Error::NotFound(format!("backup {id}")))?,
        None => backups.into_iter().next().ok_or_else(|| Error::NotFound("no backups on this drive".into()))?,
    };
    let restored = Device::open(backup::dir(&d, &info.id))?.library()?;
    let saved: HashSet<String> = info.files.iter().map(|f| f.to_lowercase()).collect();
    let missing_audio = restored
        .tracks
        .iter()
        .filter(|t| {
            let key = paths::key(&t.file_path);
            !saved.contains(&key.to_lowercase()) && !d.checked_path(&key).is_ok_and(|p| p.exists())
        })
        .map(|t| t.display_name())
        .collect();
    let label = format!("backup {}", info.id);
    let diff = diff::between(&d.library()?, &restored, (&d.name(), &label), None);
    Ok(RestorePreview { backup: info, diff, missing_audio })
}
