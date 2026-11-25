//! Checking a drive's file system, and repairing it, with loclib-disk's [`fscheck`]: this adds
//! what makes it safe for a drive locallibrary looks after.

use std::path::PathBuf;

use loclib_disk::fscheck;
use schemars::JsonSchema;
use serde::Serialize;

use crate::Result;
use crate::device::Device;

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct FsCheckReport {
    pub drive: String,
    pub root: PathBuf,
    /// The volume's device (`disk4s1`, `/dev/sdb1`, `E:`).
    pub volume: Option<String>,
    pub filesystem: Option<String>,
    pub command: String,
    /// Whether the file system is consistent (after the repair, when repairing).
    pub ok: bool,
    pub repaired: bool,
    /// The tool's report, last lines.
    pub output: Vec<String>,
    /// What to be wary of: the check hasn't run on this platform before.
    pub warnings: Vec<String>,
}

impl FsCheckReport {
    /// One line for the drive's logbook.
    pub fn summary(&self) -> String {
        match (self.ok, self.repaired) {
            (true, true) => "file system repaired".into(),
            (true, false) => "file system consistent".into(),
            (false, true) => "file system errors the repair couldn't fix".into(),
            (false, false) => "file system errors found".into(),
        }
    }
}

/// Check a drive's file system (with `repair`, fix it). Nothing may be using the drive: it is
/// unmounted for the duration and mounted again (by diskutil on macOS, by udisks on Linux), and
/// on Windows a repair dismounts it. Refused while `$LOCALLIBRARY_VOLUMES` hides the machine's
/// drives (`device::disks_shown`): the volume holding a drive folder is the machine's.
pub fn check(device: &Device, repair: bool) -> Result<FsCheckReport> {
    crate::device::disks_shown()?;
    // No locallibrary operation may be running on it (and the lock file, on the drive itself,
    // must be closed again before the volume can be unmounted). Checking makes none.
    drop(device.read_lock()?);
    let root = device.root().to_path_buf();
    let disk = crate::format::disk_at(&root).ok();
    let volume =
        disk.as_ref().and_then(|d| d.volumes.iter().find(|v| v.mount.as_deref() == Some(root.as_path())).cloned());
    let partition = volume.as_ref().map(|v| v.id.as_str());
    let steps = fscheck::steps(&root, partition, volume.as_ref().and_then(|v| v.filesystem_name.as_deref()), repair)?;
    let found = fscheck::run(&steps, partition)?;
    Ok(FsCheckReport {
        drive: device.name(),
        root,
        volume: volume.as_ref().map(|v| v.id.clone()),
        filesystem: volume.and_then(|v| v.filesystem_name),
        command: steps.display(),
        ok: found.ok,
        repaired: repair,
        output: found.output,
        warnings: crate::format::untested_here("the file-system check").into_iter().collect(),
    })
}

#[cfg(test)]
mod tests;
