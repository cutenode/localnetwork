//! The timed test behind a [`super::DriveSpeed`]: a scratch file, then small files and folders, all
//! in a folder in the drive's `.locallibrary/`, removed afterwards. A test cut off leaves it for the
//! next one to remove.

use std::fs::File;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::error::IoContext;
use crate::format::capacity;
use crate::progress::{Ctx, Event, Phase};
use crate::{Error, Result, fsutil, library_write};

/// The most a test writes, and how long it writes for: a stick at 150 MB/s and a card at 16 both
/// get several chunks.
pub(super) const MAX_BYTES: u64 = 2 << 30;
const WRITE_BUDGET: Duration = Duration::from_secs(15);

/// Written and flushed at a go. Much smaller and the flushes are what's timed.
const CHUNK: usize = 32 << 20;

/// Small files and folders timed, and how long each kind gets: at least [`MIN_ITEMS`], for a drive
/// so slow the budget runs out first.
pub(super) const ITEMS: usize = 128;
pub(super) const MIN_ITEMS: usize = 16;
pub(super) const ITEM_BUDGET: Duration = Duration::from_secs(3);

/// As big as a typical analysis file.
const SMALL_FILE: [u8; 4096] = [0x4c; 4096];

/// One file in this many gets a long name, as one in five of a library's do (the audio; analysis
/// files and artwork have 8.3 names): a long name takes more directory entries.
const LONG_NAME_EVERY: usize = 5;

/// A rate has fallen, not just wandered, when the last third runs under this share of the first.
const FELL: f64 = 0.7;

/// What the timed write found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Rate {
    pub(super) bytes_per_sec: u64,
    /// See [`super::DriveSpeed::holds_for`].
    pub(super) holds_for: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Costs {
    pub(super) rate: Rate,
    pub(super) create_us: u64,
    pub(super) mkdir_us: u64,
}

/// The rate from each chunk's time: the chunks after a fall when the last third is well under the
/// first, and otherwise the mean, good for what was written.
pub(super) fn settle(chunk_bytes: u64, chunks: &[Duration]) -> Rate {
    let rate = |taken: &[Duration]| {
        let secs: f64 = taken.iter().map(Duration::as_secs_f64).sum();
        taken.len() as f64 * chunk_bytes as f64 / secs.max(1e-9)
    };
    let third = chunks.len() / 3;
    if third > 0 {
        let (first, last) = (rate(&chunks[..third]), rate(&chunks[chunks.len() - third..]));
        if last < FELL * first {
            // The last third can start before the fall.
            let is_fast = |taken: &Duration| rate(std::slice::from_ref(taken)) >= FELL * first;
            let after = chunks.iter().rposition(is_fast).map_or(0, |i| i + 1);
            let settled = if after < chunks.len() { rate(&chunks[after..]) } else { last };
            return Rate { bytes_per_sec: settled as u64, holds_for: None };
        }
    }
    Rate { bytes_per_sec: rate(chunks) as u64, holds_for: Some(chunks.len() as u64 * chunk_bytes) }
}

/// Write chunks with `write`, which gives the time each took, until `limit` bytes are written or
/// the budget is spent.
pub(super) fn timed_write(
    limit: u64,
    chunk: &mut [u8],
    write: &mut dyn FnMut(&[u8]) -> io::Result<Duration>,
    ctx: &Ctx<'_>,
) -> Result<Rate> {
    let size = chunk.len() as u64;
    let count = (limit / size).max(1);
    let nonce = u64::from(std::process::id());
    ctx.phase_start(Phase::Measure, count, count * size);
    let mut taken = Vec::new();
    for i in 0..count {
        ctx.check()?;
        // Fresh bytes each time: some controllers compress or skip what they've just seen.
        capacity::pattern(i, nonce, chunk);
        taken.push(write(chunk).map_err(|e| Error::Io { path: "drive".into(), source: e })?);
        ctx.emit(Event::Bytes { phase: Phase::Measure, bytes: size });
        if taken.iter().sum::<Duration>() >= WRITE_BUDGET {
            break;
        }
    }
    ctx.phase_end(Phase::Measure);
    Ok(settle(size, &taken))
}

/// What `make` makes of [`ITEMS`], stopping once the budget from `start` is spent, though not
/// before [`MIN_ITEMS`].
fn make_items(start: Instant, make: impl Fn(usize) -> Result<PathBuf>, ctx: &Ctx<'_>) -> Result<Vec<PathBuf>> {
    let mut made = Vec::with_capacity(ITEMS);
    while more(start, made.len()) {
        ctx.check()?;
        made.push(make(made.len())?);
    }
    Ok(made)
}

/// Whether a test that started at `start` and has made `made` items makes another: up to
/// [`ITEMS`], and past [`MIN_ITEMS`] only within [`ITEM_BUDGET`].
pub(super) fn more(start: Instant, made: usize) -> bool {
    made < ITEMS && (made < MIN_ITEMS || start.elapsed() < ITEM_BUDGET)
}

/// The microseconds each of `items` took since `start`.
fn each_us(start: Instant, items: usize) -> u64 {
    (start.elapsed().as_micros() / items.max(1) as u128) as u64
}

pub(super) fn scratch_path(root: &Path) -> PathBuf {
    root.join(crate::paths::STATE_DIR).join("speed-test")
}

/// The scratch folder, removed when it's dropped however the test ends.
struct Scratch(PathBuf);

impl Scratch {
    /// A new one on the drive at `root`, after what a test cut off left.
    fn new(root: &Path) -> Result<Scratch> {
        let dir = scratch_path(root);
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir(&dir).at(&dir)?;
        Ok(Scratch(dir))
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn file_name(i: usize) -> String {
    match i % LONG_NAME_EVERY {
        0 => format!("{i:05} - An Artist With A Long Name - A Track Title (Extended Mix).mp3"),
        _ => format!("{i:05}.DAT"),
    }
}

/// Time the test on the drive at `root`, writing `limit` bytes at most.
pub(super) fn run(root: &Path, limit: u64, ctx: &Ctx<'_>) -> Result<Costs> {
    let scratch = Scratch::new(root)?;
    let rate = {
        let path = scratch.0.join("rate");
        let mut file = File::create(&path).at(&path)?;
        let mut chunk = vec![0u8; CHUNK.min(limit as usize)];
        timed_write(
            limit,
            &mut chunk,
            &mut |bytes| {
                let start = Instant::now();
                // A megabyte at a go, as a copy writes.
                for piece in bytes.chunks(1 << 20) {
                    file.write_all(piece)?;
                }
                file.sync_data()?;
                Ok(start.elapsed())
            },
            ctx,
        )?
    };
    let files = scratch.0.join("files");
    std::fs::create_dir(&files).at(&files)?;
    let start = Instant::now();
    let items: Vec<usize> = (0..ITEMS).collect();
    // As a copy makes them: under their own names, several at a time. Past the budget the rest
    // are left unmade, as `make_items` leaves them, and the time is shared by those made.
    let (made, failed) = library_write::copy_each(
        &items,
        |_| SMALL_FILE.len() as u64,
        |&i| {
            ctx.check()?;
            if !more(start, i) {
                return Ok(None);
            }
            let path = files.join(file_name(i));
            fsutil::write_for_drive(&path, &SMALL_FILE)?;
            Ok(Some(path))
        },
    );
    if let Some(e) = failed {
        return Err(e);
    }
    let written: Vec<PathBuf> = made.into_iter().flatten().collect();
    fsutil::flush(&written)?;
    let create_us = each_us(start, written.len());
    let folders = scratch.0.join("folders");
    std::fs::create_dir(&folders).at(&folders)?;
    let start = Instant::now();
    let made = make_items(
        start,
        |i| {
            let path = folders.join(format!("{i:05}"));
            std::fs::create_dir(&path).at(&path)?;
            Ok(path)
        },
        ctx,
    )?;
    let mkdir_us = each_us(start, made.len());
    Ok(Costs { rate, create_us, mkdir_us })
}
