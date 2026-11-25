//! What a drive's speed is, and what a plan says from it (see [`loclib_drive_speed`]), which
//! measures a [`Device`] through the drive writes everything else here goes through.

mod words;

use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use chrono::Utc;
use loclib_drive_speed::{self as drive_speed, Drive, DriveNow, DriveSpeed, Stale, Work, WriteEstimate};
use schemars::JsonSchema;
use serde::Serialize;

use super::{DriveRef, registry, resolve_one_drive};
use crate::device::{Device, DriveLock};
use crate::progress::Ctx;
use crate::{Result, device, fleet, format, fsutil, library_write, op};

pub use words::estimate_lines;

/// A drive as a test times it: written the way every other write here writes. Its errors are
/// core's own, carried in an [`io::Error`] for [`crate::Error`]'s `From` to give back.
impl Drive for Device {
    type Lock = DriveLock;

    fn root(&self) -> &Path {
        Device::root(self)
    }

    fn state_dir(&self) -> PathBuf {
        Device::state_dir(self)
    }

    fn name(&self) -> String {
        Device::name(self)
    }

    fn lock(&self) -> io::Result<DriveLock> {
        Device::lock(self).map_err(io::Error::other)
    }

    fn read(&self, path: &Path) -> io::Result<Vec<u8>> {
        fsutil::read(path)
    }

    fn write_atomic(&self, path: &Path, bytes: &[u8]) -> io::Result<()> {
        fsutil::write_atomic(path, bytes).map_err(io::Error::other)
    }

    fn small_files(
        &self,
        count: usize,
        path: &(dyn Fn(usize) -> Option<PathBuf> + Sync),
        bytes: &[u8],
    ) -> io::Result<Vec<PathBuf>> {
        let items: Vec<usize> = (0..count).collect();
        let (made, failed) = library_write::copy_each(
            &items,
            |_| bytes.len() as u64,
            |&i| {
                let Some(path) = path(i) else { return Ok(None) };
                fsutil::write_for_drive(&path, bytes)?;
                Ok(Some(path))
            },
        );
        if let Some(e) = failed {
            return Err(io::Error::other(e));
        }
        let written: Vec<PathBuf> = made.into_iter().flatten().collect();
        fsutil::flush(&written).map_err(io::Error::other)?;
        Ok(written)
    }
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct DriveSpeedReport {
    pub drive: String,
    /// The stored result. `None`: it hasn't been measured.
    pub speed: Option<DriveSpeed>,
    /// Why that may no longer hold.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stale: Option<Stale>,
    /// The USB link it's on now, in bits a second.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub link_bps: Option<u64>,
}

/// The disk a device is on, as far as its speed goes: whether it's external at all (an internal
/// disk or a folder isn't worth an estimate) and its USB link. A test's folders stand for drives.
fn disk_facts(device: &Device) -> (bool, Option<u64>) {
    match format::disk_at(device.root()) {
        Ok(disk) => (!disk.internal, format::link_speed(&disk)),
        Err(_) => (device::disks_shown().is_err(), None),
    }
}

fn now(device: &Device, link_bps: Option<u64>) -> DriveNow {
    let logged = registry().zip(device.id()).and_then(|(store, id)| {
        let drives = store.drives().ok()?;
        drives.get(&id).map(|record| record.log.bytes_written)
    });
    DriveNow { at: Utc::now(), logged_bytes: logged, capacity: fsutil::total_space(device.root()), link_bps }
}

fn report(device: &Device, speed: Option<DriveSpeed>, facts: &DriveNow) -> DriveSpeedReport {
    let stale = speed.as_ref().and_then(|s| s.stale(facts));
    DriveSpeedReport { drive: device.name(), speed, stale, link_bps: facts.link_bps }
}

/// What's stored about a drive's speed. Nothing is written.
pub fn drive_speed(req: &DriveRef) -> Result<DriveSpeedReport> {
    let device = resolve_one_drive(&req.drive)?;
    let facts = now(&device, disk_facts(&device).1);
    Ok(report(&device, drive_speed::stored(&device), &facts))
}

/// Measure a drive again. It writes a scratch file and removes it.
pub fn drive_speed_measure(req: &DriveRef, ctx: &Ctx<'_>) -> Result<DriveSpeedReport> {
    let device = resolve_one_drive(&req.drive)?;
    fleet::check_target(registry().as_ref(), &device, false)?;
    let facts = now(&device, disk_facts(&device).1);
    // A folder standing in for a drive in a test has nothing to find: the smallest test will do.
    let limit = if device::disks_shown().is_ok() { drive_speed::MAX_TEST_BYTES } else { drive_speed::MIN_TEST_BYTES };
    let measured = drive_speed::measure(&device, limit, &facts, ctx)?;
    let measured =
        measured.ok_or_else(|| crate::Error::invalid(format!("{} has too little free space", device.name())))?;
    Ok(report(&device, Some(measured), &facts))
}

/// What a plan says about writing `work` to `drive`: nothing for an internal disk, a folder or a
/// small write. It measures the drive first only where [`op::allow_measuring`] says it may.
pub(crate) fn write_estimate(drive: &Device, work: Work, ctx: &Ctx<'_>) -> Result<Option<WriteEstimate>> {
    if !work.matters() {
        return Ok(None);
    }
    let (external, link_bps) = disk_facts(drive);
    if !external {
        return Ok(None);
    }
    Ok(drive_speed::estimate(drive, work, &now(drive, link_bps), op::measuring_allowed(), ctx)?)
}

/// A write of `estimate`'s work to the drive at `root` took `elapsed` (see [`drive_speed::learn`]).
pub(crate) fn learn_write(root: &Path, estimate: &WriteEstimate, elapsed: Duration, ctx: &Ctx<'_>) {
    let Ok(device) = Device::open(root) else { return };
    let link_bps = estimate.speed.as_ref().and_then(|s| s.link_bps);
    drive_speed::learn(&device, estimate, elapsed, &now(&device, link_bps), ctx);
}

#[cfg(test)]
mod tests;
