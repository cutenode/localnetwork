//! Playlist diffs between drives and snapshots, and what a restore would change.

use rustc_hash::FxHashSet;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::{RestoreBackup, resolve_drive};
use crate::backup::{self, BackupInfo};
use crate::device::Device;
use crate::diff::{self, PlaylistDiff};
use crate::doctor::Databases;
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

/// `snapshot:<id prefix>`, a drive (path, name or id), or a library in the store: its lead clone
/// when one of its drives is plugged in (see [`resolve_drive`]), else its latest snapshot.
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
    /// A library (its newest drive plugged in, else its latest snapshot), a drive, or
    /// `snapshot:<id prefix>`.
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
    let (l, r) = (left.databases()?, right.databases()?);
    let playlist = req.playlist.as_deref();
    // A playlist only OneLibrary has (a CDJ-3000X makes them there) is found there.
    let in_onelibrary = l.beside().zip(r.beside()).is_some_and(|(a, b)| diff::check_playlist(a, b, playlist).is_ok());
    if !in_onelibrary {
        diff::check_playlist(l.primary(), r.primary(), playlist)?;
    }
    Ok(both_between(&l, &r, (&left.label(), &right.label()), playlist))
}

/// [`diff::between`] the libraries players see, with the differences of the OneLibrary databases
/// beside them added where both sides have one (a CDJ-3000X writes only to OneLibrary).
fn both_between(left: &Databases, right: &Databases, labels: (&str, &str), playlist: Option<&str>) -> PlaylistDiff {
    let mut out = diff::between(left.primary(), right.primary(), labels, playlist);
    if let (Some(a), Some(b)) = (left.beside(), right.beside()) {
        out.add(diff::between(a, b, labels, playlist));
    }
    out
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
    let backed_up = Device::open(backup::dir(&d, &info.id))?;
    let restored = backed_up.library()?;
    let saved: FxHashSet<String> = info.files.iter().map(|f| f.to_lowercase()).collect();
    let missing_audio = restored
        .tracks
        .iter()
        .filter(|t| {
            let key = paths::key(&t.file_path);
            !saved.contains(&key.to_lowercase()) && !d.checked_path(&key).is_ok_and(|p| p.exists())
        })
        .map(|t| t.display_name())
        .collect();
    let diff = to_backup(&d, &backed_up, &format!("backup {}", info.id));
    Ok(RestorePreview { backup: info, diff, missing_audio })
}

/// How `d` would differ with the backup `backed_up` put back (a database that can't be read
/// counts as empty).
pub(super) fn to_backup(d: &Device, backed_up: &Device, label: &str) -> PlaylistDiff {
    match (Databases::read(d), Databases::read(backed_up)) {
        (Ok(now), Ok(then)) => both_between(&now, &then, (&d.name(), label), None),
        (now, then) => {
            let library = |dbs: Result<Databases>| dbs.map(|d| d.primary().clone()).unwrap_or_default();
            diff::between(&library(now), &library(then), (&d.name(), label), None)
        }
    }
}
