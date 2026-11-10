//! The sum itself: what a write asks of a drive, and what's said about how long it takes.

use std::path::Path;

use rustc_hash::FxHashSet;
use schemars::JsonSchema;
use serde::Serialize;

use super::{DriveSpeed, Stale, TestTrace};
use crate::op::bytes;

/// Below both of these a write takes moments on anything, and nothing is said about it.
const SMALL_BYTES: u64 = 64 << 20;
const SMALL_FILES: u64 = 256;

/// What's left of a USB link's bits for data, in bytes for each bit a second: its encoding and
/// the storage protocol's overhead take the rest.
const LINK_BYTES_PER_BIT: f64 = 0.7 / 8.0;

/// A rate this close to what the port carries may be the port's.
const NEAR_LINK: f64 = 0.8;

/// What a write asks of a drive.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, JsonSchema)]
pub struct Work {
    pub bytes: u64,
    pub files: u64,
    /// Folders to make: those the files go in that the drive doesn't have.
    pub folders: u64,
    /// What else the time depends on that isn't the drive (downloads, transcoding), as a line
    /// says it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plus: Option<String>,
    /// Something other than this computer's disk is the source, or `plus`: the time of a real
    /// write says less about the drive.
    #[serde(skip)]
    pub(crate) slow_source: bool,
}

impl Work {
    /// The work of copying `copies` (paths below `root`, sizes) to the drive at `root`.
    pub(crate) fn copying<'a>(root: &Path, copies: impl IntoIterator<Item = (&'a str, u64)>) -> Work {
        let mut work = Work::default();
        let mut wanted: FxHashSet<&str> = FxHashSet::default();
        for (path, size) in copies {
            work.bytes += size;
            work.files += 1;
            let mut below = path;
            while let Some((parent, _)) = below.rsplit_once('/') {
                if !wanted.insert(parent) {
                    break;
                }
                below = parent;
            }
        }
        // Parents first: nothing below a folder that's missing needs looking for.
        let mut wanted: Vec<&str> = wanted.into_iter().collect();
        wanted.sort_unstable_by_key(|folder| folder.len());
        let mut missing: FxHashSet<&str> = FxHashSet::default();
        for folder in wanted {
            let parent_missing = folder.rsplit_once('/').is_some_and(|(parent, _)| missing.contains(parent));
            if parent_missing || !root.join(folder).is_dir() {
                missing.insert(folder);
            }
        }
        work.folders = missing.len() as u64;
        work
    }

    /// What else the time depends on, as a line says it (`downloads`).
    pub(crate) fn plus(self, what: &str) -> Work {
        Work { plus: Some(what.into()), slow_source: true, ..self }
    }

    /// The source isn't this computer's disk.
    pub(crate) fn read_elsewhere(self) -> Work {
        Work { slow_source: true, ..self }
    }

    pub(crate) fn matters(&self) -> bool {
        self.bytes >= SMALL_BYTES || self.files >= SMALL_FILES
    }

    /// The seconds going to files and folders rather than bytes.
    pub(crate) fn small_files_secs(&self, speed: &DriveSpeed) -> f64 {
        (self.files as f64 * speed.create_us as f64 + self.folders as f64 * speed.mkdir_us as f64) / 1e6
    }

    fn bytes_secs(&self, speed: &DriveSpeed) -> f64 {
        self.bytes as f64 / speed.bytes_per_sec.max(1) as f64
    }
}

/// Which of a write's costs is the bigger.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Limit {
    /// The drive's sustained rate: the bytes take longest.
    Rate,
    /// Making files and folders takes longest.
    Files,
}

/// How long a write to a drive will take.
#[derive(Debug, Serialize, JsonSchema)]
pub struct WriteEstimate {
    pub work: Work,
    /// The result it's worked out from. `None`: the drive hasn't been measured.
    pub speed: Option<DriveSpeed>,
    /// Seconds it will take, or at least take when `best_case`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seconds: Option<u64>,
    /// The write is longer than the rate has been seen to hold, so it could slow down past a cache
    /// region the test didn't reach.
    pub best_case: bool,
    /// Which cost is the bigger.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<Limit>,
    /// Why the result may no longer hold.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stale: Option<Stale>,
    /// The USB link the drive is on, in bits a second, when it's near or below the drive's rate.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port_limit_bps: Option<u64>,
    /// The drive was timed for this plan, with a short test whose files are gone.
    pub ran_test: bool,
    /// What the test left on a drive that had no `.locallibrary/`.
    #[serde(skip)]
    #[schemars(skip)]
    pub(crate) trace: Option<TestTrace>,
}

impl WriteEstimate {
    pub(crate) fn new(work: Work, speed: Option<DriveSpeed>, stale: Option<Stale>, link_bps: Option<u64>) -> Self {
        let port_limit_bps = speed.as_ref().and_then(|s| link_bps.filter(|&link| near_link(s, link)));
        let timing = speed.as_ref().map(|s| {
            let (data, small) = (work.bytes_secs(s), work.small_files_secs(s));
            let limit = if data >= small { Limit::Rate } else { Limit::Files };
            ((data + small).ceil() as u64, limit, s.holds_for.is_some_and(|held| held < work.bytes))
        });
        WriteEstimate {
            seconds: timing.map(|(seconds, _, _)| seconds),
            limit: timing.map(|(_, limit, _)| limit),
            best_case: timing.is_some_and(|(_, _, best_case)| best_case),
            work,
            speed,
            stale,
            port_limit_bps,
            ran_test: false,
            trace: None,
        }
    }

    /// A write is going ahead, so what the test left on the drive stays.
    pub(crate) fn keep(&self) {
        if let Some(trace) = &self.trace {
            trace.keep();
        }
    }

    /// Nothing but the drive went into a real write's time.
    pub(crate) fn drive_alone(&self) -> bool {
        !self.work.slow_source
    }

    /// What a plan says about it: the estimate, then a line about the port if there's one to say.
    pub fn lines(&self) -> Vec<String> {
        let (Some(speed), Some(seconds)) = (&self.speed, self.seconds) else {
            return vec!["time unknown: this drive hasn't been measured".into()];
        };
        let mostly = if self.limit == Some(Limit::Files) { ", mostly making files" } else { "" };
        let plus = self.work.plus.as_deref().map(|p| format!(", plus {p}")).unwrap_or_default();
        let rate = format!("drive writes ~{:.0} MB/s", speed.bytes_per_sec as f64 / 1e6);
        let create = format!("{:.0} ms a file", speed.create_us as f64 / 1e3);
        let stale = self.stale.map(|s| format!(", {}", stale_words(s))).unwrap_or_default();
        let time = time_words(seconds, self.best_case);
        let mut lines = vec![format!("{time}{mostly}{plus} ({rate}, {create}{stale})")];
        if self.ran_test {
            lines.push("ran a short test on this drive to time it, and removed its files".into());
        }
        if let Some(link) = self.port_limit_bps {
            lines.push(format!(
                "this port ({}) tops out near {:.0} MB/s, so a faster one may help",
                link_name(link),
                link_rate(link) / 1e6
            ));
        }
        lines
    }
}

fn near_link(speed: &DriveSpeed, link_bps: u64) -> bool {
    speed.bytes_per_sec as f64 >= NEAR_LINK * link_rate(link_bps)
}

fn link_rate(link_bps: u64) -> f64 {
    link_bps as f64 * LINK_BYTES_PER_BIT
}

fn stale_words(stale: Stale) -> String {
    match stale {
        Stale::Old { days } => format!("measured {days} days ago"),
        Stale::Worn { bytes: written } => format!("measured before {} of writes", bytes(written)),
        Stale::Moved { then, .. } => format!("measured on a {} port", link_name(then)),
    }
}

/// A USB link by what people call it.
pub(crate) fn link_name(bps: u64) -> String {
    match bps {
        0..=12_000_000 => "USB 1".into(),
        480_000_000 => "USB 2.0".into(),
        bps => format!("USB 3, {} Gb/s", bps / 1_000_000_000),
    }
}

/// A time as a plan says it, a best case as the least it could be.
fn time_words(seconds: u64, best_case: bool) -> String {
    match (seconds, best_case) {
        (0..60, false) => "under a minute".into(),
        (0..60, true) => "a minute or more".into(),
        (_, false) => format!("about {}", duration(seconds)),
        (_, true) => format!("at least {}", duration(seconds)),
    }
}

/// `7 min`, `1 h 25 min`: as exact as a guess deserves.
pub(crate) fn duration(seconds: u64) -> String {
    match seconds {
        0..3600 => format!("{} min", (seconds + 30) / 60),
        _ => {
            let fives = (seconds + 150) / 300;
            match (fives / 12, fives % 12 * 5) {
                (hours, 0) => format!("{hours} h"),
                (hours, minutes) => format!("{hours} h {minutes} min"),
            }
        }
    }
}
