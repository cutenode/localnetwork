#![doc = include_str!("../README.md")]
#![warn(missing_docs)]
#![cfg_attr(docsrs, feature(doc_cfg))]

mod cmd;
mod disk;
mod error;
pub mod fat32;
mod filesystem;
mod json;
#[cfg(any(test, feature = "test-util"))]
#[cfg_attr(docsrs, doc(cfg(feature = "test-util")))]
pub mod test_util;

#[cfg_attr(any(not(unix), target_os = "macos"), allow(dead_code))]
mod linux;
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
mod macos;
#[cfg_attr(not(windows), allow(dead_code))]
pub mod windows;

#[cfg(all(unix, not(target_os = "macos")))]
use linux as os;
#[cfg(target_os = "macos")]
use macos as os;
#[cfg(windows)]
use windows as os;

use std::path::{Path, PathBuf};

use loclib_progress::Ctx;

pub use cmd::Cmd;
pub use disk::{Disk, PARTITION_START, Scheme, Volume};
pub use error::{Error, Result};
pub use filesystem::Filesystem;

/// The disks the system's tools list: every disk on Linux and Windows, the external physical
/// ones on macOS. [`Disk::problem`] says which may be formatted.
pub fn list() -> Result<Vec<Disk>> {
    os::list()
}

/// The disk with this OS identifier (`disk4`, `/dev/sdb`, `PhysicalDrive2`), or holding the
/// partition with it. `None` if `spec` isn't shaped like one, and then the system isn't asked.
pub fn by_id(spec: &str) -> Result<Option<Disk>> {
    os::by_id(spec)
}

/// The disk holding the volume mounted at `root`.
pub fn by_mount(root: &Path) -> Result<Disk> {
    os::by_mount(root)
}

/// The speed `disk`'s USB link came up at, in bits a second, where the system says (macOS, Linux).
pub fn link_speed(disk: &Disk) -> Option<u64> {
    os::link_speed(disk)
}

/// What [`format()`] makes on a disk.
#[derive(Debug, Clone)]
pub struct Target {
    /// The file system.
    pub filesystem: Filesystem,
    /// Its volume label, one [`Filesystem::label`] made.
    pub label: String,
    /// Bytes per cluster.
    pub cluster_size: u32,
    /// The system's tools would pick this cluster size anyway.
    pub default_cluster: bool,
}

/// Refuse a file system this system's tools can't make (HFS+ on Windows).
pub fn supports(filesystem: Filesystem) -> Result<()> {
    os::supports(filesystem)
}

/// What formatting `disk` as `target` runs, shell-style, to show before it does.
pub fn steps(disk: &Disk, target: &Target) -> Result<Vec<String>> {
    os::steps(disk, target)
}

/// Erase `disk`: one MBR partition from [`PARTITION_START`] to the end, holding `target`'s file
/// system. Returns where the new volume is mounted, when the system mounts it
/// ([`MOUNTS_AFTER_FORMAT`]). Nothing checks that `disk` may be erased: [`Disk::problem`] and
/// [`Disk::identity`] are for the caller to check first.
pub fn format(disk: &Disk, target: &Target, ctx: &Ctx<'_>) -> Result<Option<PathBuf>> {
    os::format(disk, target, ctx)
}

/// Whether [`format()`] leaves the new volume mounted: on macOS and Windows, not on Linux (run as
/// root, there's no desktop session to mount it for).
pub const MOUNTS_AFTER_FORMAT: bool = os::MOUNTS_AFTER_FORMAT;

/// Fail with [`Error::Permission`] unless this process may write to the raw device.
pub(crate) fn require_write_access(device: &str, how: &str) -> Result<()> {
    match std::fs::OpenOptions::new().write(true).open(device) {
        Ok(_) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
            Err(Error::Permission(format!("formatting {device} needs administrator rights: {how}")))
        }
        // In use by a mounted volume: allowed, just not right now (the tools unmount first).
        Err(e) if e.kind() == std::io::ErrorKind::ResourceBusy => Ok(()),
        Err(e) => Err(Error::Io { path: device.into(), source: e }),
    }
}

#[cfg(test)]
mod tests;
