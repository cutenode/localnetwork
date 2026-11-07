//! The new files a write to a drive is making. They go straight under their own names (see
//! [`crate::fsutil::copy_for_drive`]), so a pulled plug can leave part of one where a scan sees it.
//! The write notes them on the drive before it starts and forgets them when it's done, so what a
//! cut-off write left is known by name: the next write to the drive and `drive tidy` remove it, as
//! they do temp files (see [`crate::tidy`]).

use std::path::PathBuf;

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

fn file(device: &Device) -> PathBuf {
    device.state_dir().join(FILE)
}

/// Note `files` (key, size, modification time), those not on `target` yet, before the first is
/// written. A key that isn't safe stays out: the write refuses it.
pub(crate) fn note(target: &Device, files: impl Iterator<Item = (String, u64, i64)>) -> Result<()> {
    let is_new =
        |(key, ..): &(String, u64, i64)| target.checked_path(key).is_ok_and(|p| std::fs::symlink_metadata(p).is_err());
    let new: Vec<NewFile> = files.filter(is_new).map(|(key, size, mtime)| NewFile { key, size, mtime }).collect();
    match new.is_empty() {
        true => forget(target),
        false => fsutil::write_json_atomic(&file(target), &new),
    }
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
    let noted: Vec<NewFile> = fsutil::read_json_opt(&file(device)).ok().flatten().unwrap_or_default();
    noted
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
