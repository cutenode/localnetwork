//! The new files a write to a drive is making. On macOS and Linux they go straight under their own
//! names (see [`crate::fsutil::copy_for_drive`]), so a pulled plug can leave part of one where a
//! scan sees it. The write notes them on the drive before it starts and forgets them when it's
//! done, so what a cut-off write left is known by name: the next write to the drive and
//! `drive tidy` remove it, as they do temp files (see [`crate::tidy`]).

use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::device::Device;
use crate::{Result, fsutil};

const FILE: &str = "writing.json";

/// A file as it'll be once it's finished: a write stamps it with its source's time last.
#[derive(Debug, Serialize, Deserialize)]
struct NewFile {
    key: String,
    size: u64,
    mtime: i64,
}

/// What a write noted, and when the operation it's part of started (see
/// [`crate::device::PendingOp`]): a note from an earlier operation that finished is stale, even if
/// removing it never reached the disk.
#[derive(Debug, Serialize, Deserialize)]
struct Note {
    started: Option<DateTime<Utc>>,
    files: Vec<NewFile>,
}

fn file(device: &Device) -> PathBuf {
    device.state_dir().join(FILE)
}

fn started(device: &Device) -> Option<DateTime<Utc>> {
    device.state()?.pending.as_ref().map(|p| p.started)
}

/// Note `files` (key, size, modification time), those not on `target` yet, before the first is
/// written. A key that isn't safe stays out: the write refuses it.
pub(crate) fn note(target: &Device, files: impl Iterator<Item = (String, u64, i64)>) -> Result<()> {
    note_with_sync(target, files, sync_directory)
}

fn sync_directory(dir: &Path) -> Result<()> {
    #[cfg(not(windows))]
    {
        use crate::error::IoContext;
        std::fs::File::open(dir).and_then(|f| f.sync_all()).at(dir)
    }
    #[cfg(windows)]
    {
        // No directory durability barrier here. Temp names cover writes before rename; after
        // rename, retry repair depends on the pending marker and flush-before-stamp ordering.
        let _ = dir;
        Ok(())
    }
}

fn note_with_sync(
    target: &Device,
    files: impl Iterator<Item = (String, u64, i64)>,
    mut sync: impl FnMut(&Path) -> Result<()>,
) -> Result<()> {
    let is_new =
        |(key, ..): &(String, u64, i64)| target.checked_path(key).is_ok_and(|p| std::fs::symlink_metadata(p).is_err());
    let new: Vec<NewFile> = files.filter(is_new).map(|(key, size, mtime)| NewFile { key, size, mtime }).collect();
    if new.is_empty() {
        return forget(target);
    }
    fsutil::write_json_atomic(&file(target), &Note { started: started(target), files: new })?;
    sync(&target.state_dir())?;
    // lock/begin may have just created the state directory, even if it existed on entry here.
    sync(target.root())
}

/// The write finished, or stopped having finished or removed each file.
pub(crate) fn forget(target: &Device) -> Result<()> {
    fsutil::remove_if_exists(&file(target)).map(|_| ())
}

/// The keys of the files a cut-off write left unfinished: those that are there and aren't what it
/// noted, or that it wrote after it started, as a plan takes them (see
/// [`crate::sync::cut_off_since`]). None unless the drive is marked as cut off, which nothing else
/// can change first.
pub(crate) fn unfinished(device: &Device) -> Vec<String> {
    let Some(since) = crate::sync::cut_off_since(device) else { return Vec::new() };
    let noted: Option<Note> = fsutil::read_json_opt(&file(device)).ok().flatten();
    noted
        .filter(|n| n.started == started(device))
        .map_or_else(Vec::new, |n| n.files)
        .into_iter()
        .filter(|f| {
            let Some(meta) = device.checked_path(&f.key).ok().and_then(|p| std::fs::metadata(p).ok()) else {
                return false;
            };
            let mtime = fsutil::mtime_secs(&meta);
            meta.is_file() && !(meta.len() == f.size && fsutil::mtime_eq(mtime, f.mtime) && mtime < since)
        })
        .map(|f| f.key)
        .collect()
}

#[cfg(test)]
mod tests;
