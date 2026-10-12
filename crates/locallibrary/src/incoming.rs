//! Audio files from this computer (or another drive) copied onto a drive along with an edit,
//! under its one backup: what [`crate::relocate`] and [`crate::upgrade`] share. Finding them in
//! folders, the space they need, and writing them.

use std::path::{Path, PathBuf};

use crate::commit::NewFile;
use crate::device::Device;
use crate::edit::{self, EditPlan, EditReport};
use crate::progress::Ctx;
use crate::{Error, Result, doctor, fsutil, paths};

/// An audio file in a folder (see [`audio_files`]).
pub(crate) struct Found {
    pub path: PathBuf,
    /// Its file name.
    pub name: String,
    pub size: u64,
}

/// The audio files in `folder` and below it. OS clutter, locallibrary's own folders (a drive's
/// backups hold audio too) and the files of `device`'s own library are left out: a folder that
/// is (or holds) the drive offers only what its library doesn't use.
pub(crate) fn audio_files(folder: &Path, device: &Device, ctx: &Ctx<'_>) -> Result<Vec<Found>> {
    if !folder.is_dir() {
        return Err(Error::NotFound(format!("folder {}", folder.display())));
    }
    let skipped = |name: &str| paths::is_ignored_name(name) || paths::is_state_dir(name);
    let root = crate::device::dunce_canonicalize(device.root());
    let mut out = Vec::new();
    for entry in walkdir::WalkDir::new(folder).into_iter().filter_entry(|e| !skipped(&e.file_name().to_string_lossy()))
    {
        ctx.check()?;
        let Ok(entry) = entry else { continue };
        let name = entry.file_name().to_string_lossy().into_owned();
        let audio = name
            .rsplit_once('.')
            .is_some_and(|(_, ext)| doctor::AUDIO_EXTENSIONS.contains(&ext.to_ascii_lowercase().as_str()));
        if !entry.file_type().is_file() || !audio {
            continue;
        }
        let in_library = |rel: &Path| paths::classify(&paths::key(&rel.to_string_lossy())).is_some();
        if crate::device::dunce_canonicalize(entry.path()).strip_prefix(&root).is_ok_and(in_library) {
            continue;
        }
        let size = entry.metadata().map_or(0, |m| m.len());
        out.push(Found { path: entry.path().to_path_buf(), name, size });
    }
    Ok(out)
}

/// A file to copy onto the drive.
#[derive(Debug, Clone)]
pub(crate) struct Incoming {
    pub from: PathBuf,
    /// Where it goes, as a key.
    pub to: String,
    pub size: u64,
}

/// The space an edit that copies files onto a drive needs there.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Space {
    /// The copies, the backup and the new databases.
    pub needed_bytes: u64,
    pub free_space: Option<u64>,
    pub fits: bool,
}

/// What copying `incoming` onto `device` with `edit` needs, `extra` bytes besides (more backed
/// up). A warning says so when it doesn't fit.
pub(crate) fn space(
    device: &Device,
    incoming: &[Incoming],
    edit: &EditPlan,
    extra: u64,
    warnings: &mut Vec<String>,
) -> Space {
    let cluster = fsutil::cluster_size(device.root());
    let copies: u64 = incoming.iter().map(|i| i.size.div_ceil(cluster) * cluster).sum();
    let needed_bytes = copies + edit.needed_bytes + extra;
    let free_space = fsutil::available_space(device.root());
    let fits = free_space.is_none_or(|f| needed_bytes <= f);
    if !fits {
        warnings.push(format!(
            "the copies, backup and new databases need about {needed_bytes} bytes but only {} are free on the drive",
            free_space.unwrap_or(0)
        ));
    }
    Space { needed_bytes, free_space, fits }
}

/// Make `edit` on `device`, copying `incoming` onto it (with their modification times) under the
/// same backup; refused when it doesn't fit.
pub(crate) fn apply(
    edit: &EditPlan,
    space: Space,
    incoming: &[Incoming],
    device: &mut Device,
    ctx: &Ctx<'_>,
) -> Result<EditReport> {
    if !space.fits {
        return Err(Error::NoSpace {
            path: device.root().to_path_buf(),
            needed: space.needed_bytes,
            available: space.free_space.unwrap_or(0),
        });
    }
    let mut files = Vec::new();
    for i in incoming {
        let meta = std::fs::metadata(&i.from).map_err(|source| Error::Io { path: i.from.clone(), source })?;
        let mtime = filetime::FileTime::from_last_modification_time(&meta).unix_seconds();
        files.push(NewFile::copy(i.to.clone(), i.from.clone(), i.size, Some(mtime)));
    }
    edit::apply_with(edit, device, files, ctx)
}

#[cfg(test)]
mod tests;
