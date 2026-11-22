#![doc = include_str!("../README.md")]
#![warn(missing_docs)]
#![cfg_attr(docsrs, feature(doc_cfg))]

mod error;
mod estimate;
mod probe;
#[cfg(any(test, feature = "test-util"))]
#[doc(hidden)]
pub mod test_util;

use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use chrono::{DateTime, Utc};
use loclib_progress::Ctx;
use serde::{Deserialize, Serialize};

pub use error::{Error, Result};
pub use estimate::{Limit, Work, WriteEstimate, link_rate};

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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
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
    /// When it was measured.
    pub measured_at: DateTime<Utc>,
    /// What measured it.
    pub measured_by: MeasuredBy,
    /// The USB link the drive was on then, in bits a second.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link_bps: Option<u64>,
    /// How many bytes the caller had logged written to the drive then, if it keeps count.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub logged_bytes: Option<u64>,
}

/// What measured a drive's speed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum MeasuredBy {
    /// A timed write of a scratch file.
    Test,
    /// A real write to the drive, whose rate replaced the test's.
    Write,
}

/// Why a stored result may no longer hold.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case", tag = "reason")]
pub enum Stale {
    /// Measured this many days ago.
    Old {
        /// Days since it was measured.
        days: i64,
    },
    /// This many bytes written since, a capacity's worth or more.
    Worn {
        /// Bytes written since it was measured.
        bytes: u64,
    },
    /// Measured on a link of `then` bits a second, and now on `now`.
    Moved {
        /// The link it was measured on.
        then: u64,
        /// The link it's on now.
        now: u64,
    },
}

/// What's known about a drive now, to judge a stored result by.
#[derive(Debug, Clone, Copy)]
pub struct DriveNow {
    /// The time now.
    pub at: DateTime<Utc>,
    /// How many bytes the caller has logged written to the drive, if it keeps count.
    pub logged_bytes: Option<u64>,
    /// The drive's size in bytes, if it's known.
    pub capacity: Option<u64>,
    /// The USB link it's on, in bits a second, if it's known.
    pub link_bps: Option<u64>,
}

/// The drive a test runs on and its result is kept on, as the caller reaches it. The test's small
/// files, its lock and the result go through the caller's own writes, so what's timed is what a
/// real write costs.
pub trait Drive {
    /// What holds the drive while a test writes to it.
    type Lock;
    /// The folder the drive is mounted at.
    fn root(&self) -> &Path;
    /// The caller's own folder on the drive, where the test works and its result is kept.
    fn state_dir(&self) -> PathBuf;
    /// What the drive is called, as a warning names it.
    fn name(&self) -> String;
    /// Hold the drive against other writes until the lock is dropped, making
    /// [`Drive::state_dir`] if it's missing.
    fn lock(&self) -> io::Result<Self::Lock>;
    /// The bytes of the file at `path`.
    fn read(&self, path: &Path) -> io::Result<Vec<u8>>;
    /// Write `bytes` to `path` so that a crash leaves either the old file or the new one.
    fn write_atomic(&self, path: &Path, bytes: &[u8]) -> io::Result<()>;
    /// Write `bytes` to the file `path(n)` names, for each `n` in `0..count` it names one for
    /// (`None`: no file for that one), as the caller's copies write small files (several at a
    /// time, say), stopping at the first error. Then flush them to the drive. The files written.
    fn small_files(
        &self,
        count: usize,
        path: &(dyn Fn(usize) -> Option<PathBuf> + Sync),
        bytes: &[u8],
    ) -> io::Result<Vec<PathBuf>>;
}

impl DriveSpeed {
    /// Why this result may no longer hold, if it may not.
    pub fn stale(&self, now: &DriveNow) -> Option<Stale> {
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

/// What a test made in a drive's [`Drive::state_dir`] when the drive had none. Dropped with its
/// plan, which covers every way of saying no, it leaves the drive as it found it, unless a write
/// went ahead.
#[derive(Debug)]
pub(crate) struct TestTrace {
    dir: PathBuf,
    /// What was in the folder when the test ended, listed before it let go of the drive's lock:
    /// the test's alone.
    made: Vec<OsString>,
    kept: AtomicBool,
}

impl TestTrace {
    fn new(dir: PathBuf) -> TestTrace {
        TestTrace { dir, made: Vec::new(), kept: AtomicBool::new(false) }
    }

    /// The test is over and still holds the drive's lock: what's in the folder now is what it
    /// made.
    fn note(&mut self) {
        let entries = std::fs::read_dir(&self.dir).into_iter().flatten().flatten();
        self.made = entries.map(|e| e.file_name()).collect();
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
        for made in &self.made {
            let _ = std::fs::remove_file(self.dir.join(made));
        }
        // Only if it's empty: anything else in it isn't the test's.
        let _ = std::fs::remove_dir(&self.dir);
    }
}

/// The stored result for a drive. One that can't be read counts as none: it's measured again.
pub fn stored<D: Drive>(drive: &D) -> Option<DriveSpeed> {
    let bytes = drive.read(&record_path(drive)).ok()?;
    let found = serde_json::from_slice::<DriveSpeed>(&bytes).ok();
    found.filter(|s| s.bytes_per_sec > 0)
}

fn record_path<D: Drive>(drive: &D) -> PathBuf {
    drive.state_dir().join(RECORD_FILE)
}

fn save<D: Drive>(drive: &D, speed: &DriveSpeed) -> Result<()> {
    let path = record_path(drive);
    let mut bytes = serde_json::to_vec_pretty(speed).map_err(|e| Error::Io { path: path.clone(), source: e.into() })?;
    bytes.push(b'\n');
    drive.write_atomic(&path, &bytes).map_err(Error::Drive)
}

/// Measure `drive` and keep the result. It writes up to `limit` bytes of scratch data, never more
/// than the free space can spare, under the drive's lock. `None` when there's too little free
/// space to tell anything.
pub fn measure<D: Drive>(drive: &D, limit: u64, now: &DriveNow, ctx: &Ctx<'_>) -> Result<Option<DriveSpeed>> {
    measure_noting(drive, limit, now, ctx, None)
}

/// [`measure`], noting in `trace` what's in the state folder when the test ends, before it lets go
/// of the drive's lock: whatever lands there once it has is another write's.
fn measure_noting<D: Drive>(
    drive: &D,
    limit: u64,
    now: &DriveNow,
    ctx: &Ctx<'_>,
    trace: Option<&mut TestTrace>,
) -> Result<Option<DriveSpeed>> {
    let free = fs4::available_space(drive.root()).unwrap_or(0);
    let limit = limit.min(free / 2);
    if limit < MIN_TEST_BYTES {
        return Ok(None);
    }
    let _lock = drive.lock().map_err(Error::Drive)?;
    let measured = test_and_keep(drive, limit, now, ctx);
    if let Some(trace) = trace {
        trace.note();
    }
    measured.map(Some)
}

/// Time `drive`, writing up to `limit` bytes, and keep the result. The caller holds its lock.
fn test_and_keep<D: Drive>(drive: &D, limit: u64, now: &DriveNow, ctx: &Ctx<'_>) -> Result<DriveSpeed> {
    let costs = probe::run(drive, limit, ctx)?;
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
    save(drive, &speed)?;
    Ok(speed)
}

/// What's said about a write of `work` to `drive` before it starts: from the stored result, or,
/// when `may_measure` and that's missing or stale, a new one. Nothing for a write too small to
/// matter. Only cancelling is an error: an estimate that can't be made never stops a write.
pub fn estimate<D: Drive>(
    drive: &D,
    work: Work,
    now: &DriveNow,
    may_measure: bool,
    ctx: &Ctx<'_>,
) -> Result<Option<WriteEstimate>> {
    if !work.matters() {
        return Ok(None);
    }
    let found = stored(drive);
    let stale = found.as_ref().and_then(|s| s.stale(now));
    let mut ran_test = false;
    // Before the test: one cut short leaves nothing either.
    let state_dir = drive.state_dir();
    let mut trace = (!state_dir.exists()).then(|| TestTrace::new(state_dir));
    let speed = match (found, stale.is_some()) {
        (Some(speed), false) => Some(speed),
        (found, _) if may_measure => {
            match measure_noting(drive, work.bytes.min(MAX_TEST_BYTES), now, ctx, trace.as_mut()) {
                Ok(Some(measured)) => {
                    ran_test = true;
                    Some(measured)
                }
                Ok(None) => found,
                Err(Error::Cancelled(c)) => return Err(Error::Cancelled(c)),
                Err(e) => {
                    ctx.warn(format!("couldn't measure {}'s speed: {e}", drive.name()));
                    found
                }
            }
        }
        (found, _) => found,
    };
    let stale = speed.as_ref().and_then(|s| s.stale(now));
    let mut estimate = WriteEstimate::new(work, speed, stale, now.link_bps);
    estimate.ran_test = ran_test;
    estimate.trace = trace.take().filter(|_| ran_test);
    Ok(Some(estimate))
}

/// Correct `drive`'s stored rate from a real write of `estimate`'s work that took `elapsed`. Only
/// for writes big enough to tell and with nothing else in their time. A correction that can't be
/// kept is a warning: the write itself is done.
pub fn learn<D: Drive>(drive: &D, estimate: &WriteEstimate, elapsed: Duration, now: &DriveNow, ctx: &Ctx<'_>) {
    let Some(speed) = estimate.speed.as_ref().filter(|_| estimate.drive_alone()) else { return };
    if estimate.work.bytes < MIN_LEARNED_BYTES {
        return;
    }
    let Some(corrected) = speed.corrected(&estimate.work, elapsed, now) else { return };
    if let Err(e) = save(drive, &corrected) {
        ctx.warn(format!("couldn't keep the drive's speed: {e}"));
    }
}

#[cfg(test)]
mod tests;
