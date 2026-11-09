//! What a drive's speed is, and what a plan says from it (see [`crate::drive_speed`]).

use std::path::Path;
use std::time::Duration;

use chrono::Utc;
use schemars::JsonSchema;
use serde::Serialize;

use super::{DriveRef, registry, resolve_one_drive};
use crate::device::Device;
use crate::drive_speed::{self, DriveNow, DriveSpeed, Stale, Work, WriteEstimate};
use crate::progress::Ctx;
use crate::{Result, device, fleet, format, fsutil, op};

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
    drive_speed::estimate(drive, work, &now(drive, link_bps), op::measuring_allowed(), ctx)
}

/// A write of `estimate`'s work to the drive at `root` took `elapsed` (see [`drive_speed::learn`]).
pub(crate) fn learn_write(root: &Path, estimate: &WriteEstimate, elapsed: Duration, ctx: &Ctx<'_>) {
    let Ok(device) = Device::open(root) else { return };
    let link_bps = estimate.speed.as_ref().and_then(|s| s.link_bps);
    drive_speed::learn(root, estimate, elapsed, &now(&device, link_bps), ctx);
}
