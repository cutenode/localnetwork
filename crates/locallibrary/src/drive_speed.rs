//! How long a write to a drive will take, said before it starts.
//!
//! A port's speed predicts nothing: on the sticks and cards we've timed the port was never the
//! limit, and most of an export wasn't data at all. What fits is three costs, each measured on the
//! drive:
//!
//! ```text
//! time = bytes / sustained rate + files × create cost + folders × mkdir cost
//! ```
//!
//! A test finds them with a timed write of a scratch file and a few small files and folders,
//! all removed afterwards, and keeps them on the drive (`.locallibrary/speed.json`), so they go
//! with it from one computer to the next and a format forgets them. A real write of a gigabyte or
//! more corrects the rate afterwards: it has the numbers a short test can't get. The plan says it
//! ran a test, and a drive that had no `.locallibrary/` loses that folder if the plan isn't applied.
//!
//! What a short test can't see:
//!
//! * **A cache region.** Many sticks take the first gigabytes in a fast mode, then drop. A rate
//!   that fell during the test is the rate past it, good for any size. One that stayed flat is
//!   only good for as many bytes as the test wrote ([`DriveSpeed::holds_for`]): a longer write may
//!   run into a cache's end, so its estimate is a best case and says "at least".
//! * **Wear.** A card went from 47 to 16 MB/s over 320 GB of rewrites, so a result goes stale
//!   ([`Stale`]): with age, once a capacity's worth has been written since, or on a slower or
//!   faster port than it was measured on.
//!
//! The port is a hint and never enters the sum. A rate near what the port can carry may be the
//! port's and not the drive's, which is worth saying ([`WriteEstimate::lines`]).
//!
//! The estimate leaves out what isn't the drive's: reading a slow source, downloads and
//! transcoding (it says when there are any), and the databases. It charges every file the same
//! create cost, though FAT32 and exFAT folders are unsorted lists that a new name is checked
//! against, so a create costs more in a fuller folder, and a long name takes more directory
//! entries. Modelling that wouldn't pay here: on a FAT32 disk image the scan came to about 0.25 µs
//! per entry, so a 3,000-entry folder adds under a millisecond to creates of 5 ms and more, and
//! artwork (20 images a folder) and analysis files (3 a folder) never fill one. It would show in a
//! flat folder of tens of thousands of files, which only a library that keeps all its audio in one
//! folder has. The test's file names are a library's mix of short and long, so long names are
//! in the create cost it finds.

mod estimate;
mod probe;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::device::Device;
use crate::progress::Ctx;
use crate::{Result, fsutil};

pub use estimate::{Limit, Work, WriteEstimate};

const RECORD_FILE: &str = "speed.json";

/// A drive's result is checked again after this long.
const MAX_AGE_DAYS: i64 = 90;

/// The most a test writes.
pub const MAX_TEST_BYTES: u64 = probe::MAX_BYTES;

/// Below this a test tells nothing, and an estimate wouldn't matter.
pub const MIN_TEST_BYTES: u64 = 32 << 20;

/// What a real write has to be to say more than a test: the drive's rate dominates its time.
const MIN_LEARNED_BYTES: u64 = 1 << 30;

/// What a drive costs to write to, as measured on it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct DriveSpeed {
    /// Bytes a second, once the drive has settled.
    pub bytes_per_sec: u64,
    /// Microseconds to make a small file.
    pub create_us: u64,
    /// Microseconds to make a folder.
    pub mkdir_us: u64,
    /// How many bytes the rate has been seen to hold for. `None`: it fell during the test and this
    /// is the rate past the drive's cache region, good for any size.
    pub holds_for: Option<u64>,
    pub measured_at: DateTime<Utc>,
    pub measured_by: MeasuredBy,
    /// The USB link the drive was on then, in bits a second.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link_bps: Option<u64>,
    /// The drive's logged `bytes_written` then (see [`crate::fleet::DriveLog`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub logged_bytes: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MeasuredBy {
    /// A timed write of a scratch file.
    Test,
    /// A real write to the drive, whose rate replaced the test's.
    Write,
}

/// Why a stored result may no longer hold.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case", tag = "reason")]
pub enum Stale {
    /// Measured this many days ago.
    Old { days: i64 },
    /// This many bytes written since, a capacity's worth or more.
    Worn { bytes: u64 },
    /// Measured on a link of `then` bits a second, and now on `now`.
    Moved { then: u64, now: u64 },
}

/// What's known about a drive now, to judge a stored result by.
#[derive(Debug, Clone, Copy)]
pub(crate) struct DriveNow {
    pub(crate) at: DateTime<Utc>,
    /// Its logged `bytes_written`.
    pub(crate) logged_bytes: Option<u64>,
    pub(crate) capacity: Option<u64>,
    pub(crate) link_bps: Option<u64>,
}

impl DriveSpeed {
    /// Why this result may no longer hold, if it may not.
    pub(crate) fn stale(&self, now: &DriveNow) -> Option<Stale> {
        if let (Some(then), Some(now)) = (self.link_bps, now.link_bps)
            && then != now
        {
            return Some(Stale::Moved { then, now });
        }
        let written = now.logged_bytes.unwrap_or(0).saturating_sub(self.logged_bytes.unwrap_or(0));
        if now.capacity.is_some_and(|capacity| written >= capacity) {
            return Some(Stale::Worn { bytes: written });
        }
        let days = (now.at - self.measured_at).num_days();
        (days > MAX_AGE_DAYS).then_some(Stale::Old { days })
    }

    /// `elapsed`, a real write's time for `work`, tells the rate better than a test: what's left
    /// of it once the files and folders are paid for. `None` when nothing is left, which only a
    /// result gone badly wrong gives.
    fn corrected(&self, work: &Work, elapsed: Duration, now: &DriveNow) -> Option<DriveSpeed> {
        let small = work.small_files_secs(self);
        let writing = elapsed.as_secs_f64() - small;
        (writing > 0.0).then(|| DriveSpeed {
            bytes_per_sec: (work.bytes as f64 / writing) as u64,
            holds_for: self.holds_for.map(|held| held.max(work.bytes)),
            measured_at: now.at,
            measured_by: MeasuredBy::Write,
            link_bps: now.link_bps.or(self.link_bps),
            logged_bytes: now.logged_bytes,
            ..*self
        })
    }
}

/// What a test made on a drive that had no `.locallibrary/`. Dropped with its plan, which covers
/// every way of saying no, it leaves the drive as it found it, unless a write went ahead.
#[derive(Debug)]
pub(crate) struct TestTrace {
    root: PathBuf,
    kept: AtomicBool,
}

impl TestTrace {
    fn new(root: &Path) -> TestTrace {
        TestTrace { root: root.to_path_buf(), kept: AtomicBool::new(false) }
    }

    /// A write is going ahead, so the folder stays.
    pub(crate) fn keep(&self) {
        self.kept.store(true, Ordering::SeqCst);
    }
}

impl Drop for TestTrace {
    fn drop(&mut self) {
        if self.kept.load(Ordering::SeqCst) {
            return;
        }
        let dir = self.root.join(crate::paths::STATE_DIR);
        for made in [RECORD_FILE, "lock"] {
            let _ = std::fs::remove_file(dir.join(made));
        }
        // Only if it's empty: anything else in it isn't the test's.
        let _ = std::fs::remove_dir(dir);
    }
}

/// The stored result for a drive. One that can't be read counts as none: it's measured again.
pub fn stored(device: &Device) -> Option<DriveSpeed> {
    let found = fsutil::read_json_opt::<DriveSpeed>(&record_path(device.root())).ok().flatten();
    found.filter(|s| s.bytes_per_sec > 0)
}

fn record_path(root: &Path) -> PathBuf {
    root.join(crate::paths::STATE_DIR).join(RECORD_FILE)
}

fn save(root: &Path, speed: &DriveSpeed) -> Result<()> {
    fsutil::write_json_atomic(&record_path(root), speed)
}

/// Measure `device` and keep the result. It writes up to `limit` bytes of scratch data, never more
/// than the free space can spare, under the drive's lock. `None` when there's too little free
/// space to tell anything.
pub(crate) fn measure(device: &Device, limit: u64, now: &DriveNow, ctx: &Ctx<'_>) -> Result<Option<DriveSpeed>> {
    let free = fsutil::available_space(device.root()).unwrap_or(0);
    let limit = limit.min(free / 2);
    if limit < MIN_TEST_BYTES {
        return Ok(None);
    }
    let _lock = device.lock()?;
    let costs = probe::run(device.root(), limit, ctx)?;
    let speed = DriveSpeed {
        bytes_per_sec: costs.rate.bytes_per_sec,
        create_us: costs.create_us,
        mkdir_us: costs.mkdir_us,
        holds_for: costs.rate.holds_for,
        measured_at: now.at,
        measured_by: MeasuredBy::Test,
        link_bps: now.link_bps,
        logged_bytes: now.logged_bytes,
    };
    save(device.root(), &speed)?;
    Ok(Some(speed))
}

/// What's said about a write of `work` to `device` before it starts: from the stored result, or,
/// when `may_measure` and that's missing or stale, a new one. Nothing for a write too small to
/// matter. Only cancelling is an error: an estimate that can't be made never stops a write.
pub(crate) fn estimate(
    device: &Device,
    work: Work,
    now: &DriveNow,
    may_measure: bool,
    ctx: &Ctx<'_>,
) -> Result<Option<WriteEstimate>> {
    if !work.matters() {
        return Ok(None);
    }
    let found = stored(device);
    let stale = found.as_ref().and_then(|s| s.stale(now));
    let mut ran_test = false;
    // Before the test: one cut short leaves nothing either.
    let mut trace = (!device.state_dir().exists()).then(|| TestTrace::new(device.root()));
    let speed = match (found, stale.is_some()) {
        (Some(speed), false) => Some(speed),
        (found, _) if may_measure => match measure(device, work.bytes.min(MAX_TEST_BYTES), now, ctx) {
            Ok(Some(measured)) => {
                ran_test = true;
                Some(measured)
            }
            Ok(None) => found,
            Err(crate::Error::Cancelled) => return Err(crate::Error::Cancelled),
            Err(e) => {
                ctx.warn(format!("couldn't measure {}'s speed: {e}", device.name()));
                found
            }
        },
        (found, _) => found,
    };
    let stale = speed.as_ref().and_then(|s| s.stale(now));
    let mut estimate = WriteEstimate::new(work, speed, stale, now.link_bps);
    estimate.ran_test = ran_test;
    estimate.trace = trace.take().filter(|_| ran_test);
    Ok(Some(estimate))
}

/// Correct `root`'s stored rate from a real write of `estimate`'s work that took `elapsed`. Only
/// for writes big enough to tell and with nothing else in their time. A correction that can't be
/// kept is a warning: the write itself is done.
pub(crate) fn learn(root: &Path, estimate: &WriteEstimate, elapsed: Duration, now: &DriveNow, ctx: &Ctx<'_>) {
    let Some(speed) = estimate.speed.as_ref().filter(|_| estimate.drive_alone()) else { return };
    if estimate.work.bytes < MIN_LEARNED_BYTES {
        return;
    }
    let Some(corrected) = speed.corrected(&estimate.work, elapsed, now) else { return };
    if let Err(e) = save(root, &corrected) {
        ctx.warn(format!("couldn't keep the drive's speed: {e}"));
    }
}

#[cfg(test)]
mod tests;
