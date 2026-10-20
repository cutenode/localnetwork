//! What is using a drive's space, and when it will be full.
//!
//! [`breakdown`] walks the whole volume (not just `PIONEER/` and `Contents/`) and sorts every
//! file into what a DJ would recognise: audio, analysis, artwork, databases, locallibrary's own
//! undo backups, recordings made on the player, OS clutter, the Finder or Windows trash, and
//! audio copied onto the drive by hand (players list it when browsing folders, with no
//! analysis). [`forecast`] fits the library's growth across its stored snapshots.

use indexmap::IndexMap;

use rustc_hash::FxHashSet;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Duration, NaiveDate, Utc};
use loclib_model::audio::Format;
use schemars::JsonSchema;
use serde::Serialize;
use walkdir::WalkDir;

use crate::Result;
use crate::device::Device;
use crate::fsutil;
use crate::paths::{self, FileClass};
use crate::progress::{Ctx, Phase};
use crate::store::Store;
use crate::tidy::TidyKind;

/// Space players need for history lists and cues; plans warn before leaving less.
pub const PLAYER_HEADROOM: u64 = 200 * 1000 * 1000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum SpaceKind {
    Audio,
    Analysis,
    Artwork,
    Databases,
    /// Player and mixer My Settings.
    Settings,
    /// Everything else rekordbox keeps under `PIONEER/`.
    RekordboxOther,
    /// `.locallibrary/backups`: undo copies taken before merges, cleans and edits.
    LocallibraryBackups,
    /// The rest of `.locallibrary/` (identity, hash cache).
    Locallibrary,
    /// WAV files recorded on the player (`PIONEER REC`, `ALPHATHETA REC`).
    Recordings,
    /// `._*`, `.DS_Store`, `.Spotlight-V100`, `.fseventsd`, `Thumbs.db`...
    OsClutter,
    /// `.Trashes`, `$RECYCLE.BIN`: deleted files still using space.
    Trash,
    /// Audio files outside the library.
    LooseAudio,
    Other,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct SpaceCategory {
    pub kind: SpaceKind,
    pub files: usize,
    pub bytes: u64,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct LooseFile {
    pub path: String,
    pub size: u64,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct SpaceReport {
    pub drive: String,
    pub root: PathBuf,
    pub total: Option<u64>,
    pub free: Option<u64>,
    pub cluster_size: u64,
    /// Largest first.
    pub categories: Vec<SpaceCategory>,
    /// Largest first, at most 50.
    pub loose_audio: Vec<LooseFile>,
    /// Files that couldn't be measured (no permission).
    pub unreadable: usize,
    pub forecast: Option<Forecast>,
    /// Whether this drive's library fits on the other drives of its library.
    pub fits: Vec<DriveFit>,
}

impl SpaceReport {
    pub fn bytes(&self, kind: SpaceKind) -> u64 {
        self.categories.iter().find(|c| c.kind == kind).map_or(0, |c| c.bytes)
    }
}

/// The REC folder names players use.
pub fn is_recordings_dir(name: &str) -> bool {
    let n = name.trim().to_ascii_uppercase();
    n == "PIONEER REC" || n == "ALPHATHETA REC"
}

fn is_trash_dir(name: &str) -> bool {
    paths::clutter_kind(name) == Some(TidyKind::Trash)
}

pub fn is_audio_name(name: &str) -> bool {
    !name.starts_with("._") && Format::for_players(name).is_some()
}

/// Sort every file on the volume into a [`SpaceKind`].
pub fn breakdown(device: &Device, ctx: &Ctx<'_>) -> Result<SpaceReport> {
    let root = device.root();
    let library = device.library().ok();
    let referenced: FxHashSet<String> =
        library.iter().flat_map(|l| l.tracks.iter()).map(|t| paths::file_key(&t.file_path)).collect();
    let mut by_kind: IndexMap<SpaceKind, (usize, u64)> = IndexMap::new();
    let mut loose = Vec::new();
    let mut unreadable = 0;
    ctx.phase_start(Phase::Scan, 0, 0);
    let mut walker = WalkDir::new(root).follow_links(false).min_depth(1).into_iter();
    while let Some(entry) = walker.next() {
        ctx.check()?;
        let entry = match entry {
            Ok(e) => e,
            Err(_) => {
                unreadable += 1;
                continue;
            }
        };
        let rel = entry.path().strip_prefix(root).expect("walk stays below root");
        let key = paths::key(&rel.to_string_lossy().replace('\\', "/"));
        let name = entry.file_name().to_string_lossy();
        if entry.file_type().is_dir() {
            // Whole folders of one kind are measured without descending further.
            let kind = if entry.depth() == 1 && is_recordings_dir(&name) {
                Some(SpaceKind::Recordings)
            } else if is_trash_dir(&name) {
                Some(SpaceKind::Trash)
            } else if name == paths::SYSTEM_VOLUME_INFORMATION {
                Some(SpaceKind::Other)
            } else if paths::is_ignored_name(&name) && !name.starts_with("._") {
                Some(SpaceKind::OsClutter)
            } else {
                None
            };
            if let Some(kind) = kind {
                let (files, bytes, bad) = dir_size(entry.path());
                unreadable += bad;
                let slot = by_kind.entry(kind).or_default();
                slot.0 += files;
                slot.1 += bytes;
                walker.skip_current_dir();
            }
            continue;
        }
        if !entry.file_type().is_file() {
            continue;
        }
        let size = match entry.metadata() {
            Ok(m) => m.len(),
            Err(_) => {
                unreadable += 1;
                continue;
            }
        };
        let kind = classify(&key, &name, &referenced);
        if kind == SpaceKind::LooseAudio {
            loose.push(LooseFile { path: key.clone(), size });
        }
        let slot = by_kind.entry(kind).or_default();
        slot.0 += 1;
        slot.1 += size;
    }
    ctx.phase_end(Phase::Scan);
    let mut categories: Vec<SpaceCategory> =
        by_kind.into_iter().map(|(kind, (files, bytes))| SpaceCategory { kind, files, bytes }).collect();
    categories.sort_by_key(|c| (std::cmp::Reverse(c.bytes), c.kind));
    loose.sort_by_key(|f| std::cmp::Reverse(f.size));
    loose.truncate(50);
    Ok(SpaceReport {
        drive: device.name(),
        root: root.to_path_buf(),
        total: fsutil::total_space(root),
        free: fsutil::available_space(root),
        cluster_size: fsutil::cluster_size(root),
        categories,
        loose_audio: loose,
        unreadable,
        forecast: None,
        fits: Vec::new(),
    })
}

fn classify(key: &str, name: &str, referenced: &FxHashSet<String>) -> SpaceKind {
    if key.split('/').any(|c| c.starts_with("._") || paths::is_ignored_name(c)) {
        return SpaceKind::OsClutter;
    }
    if let Some(rest) = key.strip_prefix(".locallibrary/") {
        return if rest.starts_with("backups/") { SpaceKind::LocallibraryBackups } else { SpaceKind::Locallibrary };
    }
    if referenced.contains(&key.to_lowercase()) {
        return SpaceKind::Audio;
    }
    match paths::classify(key) {
        Some(FileClass::Audio) => SpaceKind::Audio,
        Some(FileClass::Analysis) => SpaceKind::Analysis,
        Some(FileClass::Artwork) => SpaceKind::Artwork,
        Some(FileClass::Database | FileClass::Transient) => SpaceKind::Databases,
        Some(FileClass::Settings) => SpaceKind::Settings,
        Some(_) => SpaceKind::RekordboxOther,
        None if is_audio_name(name) => SpaceKind::LooseAudio,
        None => SpaceKind::Other,
    }
}

/// Files, bytes and unreadable entries below `dir`.
pub fn dir_size(dir: &Path) -> (usize, u64, usize) {
    let (mut files, mut bytes, mut bad) = (0, 0, 0);
    for e in WalkDir::new(dir).follow_links(false) {
        match e {
            Ok(e) if e.file_type().is_file() => match e.metadata() {
                Ok(m) => {
                    files += 1;
                    bytes += m.len();
                }
                Err(_) => bad += 1,
            },
            Ok(_) => {}
            Err(_) => bad += 1,
        }
    }
    (files, bytes, bad)
}

// ---------------------------------------------------------------------------------------------
// Growth
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Forecast {
    pub library: String,
    /// Library size in the newest snapshot.
    pub library_bytes: u64,
    /// Average growth, bytes per 30 days (negative when it shrinks).
    pub per_month: i64,
    pub snapshots: usize,
    pub span_days: i64,
    /// When the drive runs out of space at that rate (counting the players' headroom).
    pub full_around: Option<NaiveDate>,
}

/// Fit a line through the library's size over its snapshots from the last year. Needs at least
/// two snapshots a week apart.
pub fn forecast(store: &Store, device: &Device, free: Option<u64>) -> Result<Option<Forecast>> {
    let Some(lib_id) = device.state().and_then(|s| s.library_id) else { return Ok(None) };
    let Some(lib) = store.library_by_id(lib_id)? else { return Ok(None) };
    let history = store.history(&lib, None)?;
    let cutoff = Utc::now() - Duration::days(365);
    let points: Vec<(DateTime<Utc>, u64)> =
        history.iter().filter(|s| s.created >= cutoff).map(|s| (s.created, s.bytes)).collect();
    let Some(newest) = history.first() else { return Ok(None) };
    Ok(fit(&lib.name, newest.bytes, &points, free, Utc::now()))
}

fn fit(
    library: &str,
    library_bytes: u64,
    points: &[(DateTime<Utc>, u64)],
    free: Option<u64>,
    now: DateTime<Utc>,
) -> Option<Forecast> {
    let first = points.iter().map(|p| p.0).min()?;
    let last = points.iter().map(|p| p.0).max()?;
    let span_days = (last - first).num_days();
    if points.len() < 2 || span_days < 7 {
        return None;
    }
    // Least squares: bytes against days since the first snapshot.
    let xs: Vec<f64> = points.iter().map(|p| (p.0 - first).num_seconds() as f64 / 86_400.0).collect();
    let ys: Vec<f64> = points.iter().map(|p| p.1 as f64).collect();
    let n = xs.len() as f64;
    let (mx, my) = (xs.iter().sum::<f64>() / n, ys.iter().sum::<f64>() / n);
    let sxx: f64 = xs.iter().map(|x| (x - mx).powi(2)).sum();
    let sxy: f64 = xs.iter().zip(&ys).map(|(x, y)| (x - mx) * (y - my)).sum();
    // A week or more between the first and last snapshots: `sxx` isn't 0.
    let per_day = sxy / sxx;
    let full_around = match free {
        Some(free) if per_day > 0.0 => {
            let room = free.saturating_sub(PLAYER_HEADROOM) as f64;
            let days = (room / per_day).ceil();
            (days < 365.0 * 20.0).then(|| (now + Duration::days(days as i64)).date_naive())
        }
        _ => None,
    };
    Some(Forecast {
        library: library.to_string(),
        library_bytes,
        per_month: (per_day * 30.0).round() as i64,
        snapshots: points.len(),
        span_days,
        full_around,
    })
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct DriveFit {
    pub name: String,
    pub capacity: u64,
    /// The library plus the players' headroom.
    pub needed: u64,
    pub fits: bool,
    pub mounted: bool,
}

/// Whether `library_bytes` fits on each other registered drive of the same library. Capacity
/// is the volume size last seen, less 3% for the file system's own structures.
pub fn fits_on_set(store: &Store, device: &Device, library_bytes: u64, mounted: &[Device]) -> Result<Vec<DriveFit>> {
    let Some(lib) = device.state().and_then(|s| s.library_id) else { return Ok(Vec::new()) };
    let needed = library_bytes + PLAYER_HEADROOM;
    let mut out: Vec<DriveFit> = store
        .drives()?
        .into_values()
        .filter(|r| r.library_id == Some(lib) && Some(r.id) != device.id() && r.log.retired.is_none())
        .filter_map(|r| {
            let capacity = r.capacity? / 100 * 97;
            Some(DriveFit {
                name: r.name.clone(),
                capacity,
                needed,
                fits: needed <= capacity,
                mounted: mounted.iter().any(|d| d.id() == Some(r.id)),
            })
        })
        .collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}

/// A warning when a plan would leave a drive with less room than players need, if it would.
pub fn headroom_warning(free: Option<u64>, needed: u64, freed: u64) -> Option<String> {
    let after = free?.saturating_add(freed).checked_sub(needed)?;
    (after < PLAYER_HEADROOM).then(|| {
        format!(
            "leaves only {} MB free: players need room for history lists and cues (a full drive shows USB FULL and can't save cue points)",
            after / 1_000_000
        )
    })
}

#[cfg(test)]
mod tests;
