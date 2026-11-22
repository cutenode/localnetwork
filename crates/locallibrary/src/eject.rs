//! Ejecting a drive with loclib-disk's [`eject`](loclib_disk::eject), refused at once while a
//! locallibrary operation is writing to it (it takes the drive lock without waiting for it).

use std::path::{Path, PathBuf};

use loclib_format::Cmd;
use schemars::JsonSchema;
use serde::Serialize;

use crate::Result;
use crate::device::Device;

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct EjectReport {
    pub name: String,
    pub root: PathBuf,
    /// What was run.
    pub commands: Vec<String>,
    /// Whether the disk was also powered off (otherwise only unmounted).
    pub powered_off: bool,
}

/// The commands that eject the volume mounted at `root` (for plans and tests).
pub(crate) fn commands(root: &Path) -> Result<Vec<Cmd>> {
    // Only Linux asks for the disk: it unmounts each of its partitions.
    let disk = cfg!(all(unix, not(target_os = "macos"))).then(|| crate::format::disk_at(root)).transpose()?;
    Ok(loclib_disk::eject::commands(root, disk.as_ref())?)
}

/// Eject a drive. Fails with [`crate::Error::Locked`] while a locallibrary operation is writing
/// to it.
pub fn eject(device: &Device) -> Result<EjectReport> {
    // Wait for nothing: a running sync must finish (or be cancelled) first. Checking makes no
    // lock file on a drive that has none.
    drop(device.read_lock()?);
    let root = device.root().to_path_buf();
    let ejected = loclib_disk::eject::run(&root, &commands(&root)?)?;
    Ok(EjectReport { name: device.name(), root, commands: ejected.commands, powered_off: ejected.powered_off })
}

#[cfg(test)]
mod tests;
