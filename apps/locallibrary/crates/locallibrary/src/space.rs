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
use schemars::JsonSchema;
use serde::Serialize;
use walkdir::WalkDir;

use crate::Result;
use crate::device::Device;
use crate::fsutil;
use crate::paths::{self, FileClass};
use crate::progress::{Ctx, Phase};
use crate::store::Store;

/// Space players need for history lists and cues; plans warn before leaving less.
pub const PLAYER_HEADROOM: u64 = 200 * 1000 * 1000;

/// Audio file extensions players can list.
pub const AUDIO_EXTENSIONS: [&str; 9] = ["mp3", "m4a", "aac", "mp4", "flac", "wav", "aif", "aiff", "alac"];

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
    name == ".Trashes" || name.eq_ignore_ascii_case("$RECYCLE.BIN")
}

pub fn is_audio_name(name: &str) -> bool {
    !name.starts_with("._")
        && name.rsplit_once('.').is_some_and(|(_, ext)| AUDIO_EXTENSIONS.iter().any(|a| a.eq_ignore_ascii_case(ext)))
}

/// Sort every file on the volume into a [`SpaceKind`].
pub fn breakdown(device: &Device, ctx: &Ctx<'_>) -> Result<SpaceReport> {
    let root = device.root();
    let library = device.library().ok();
    let referenced: FxHashSet<String> =
        library.iter().flat_map(|l| l.tracks.iter()).map(|t| paths::key(&t.file_path).to_lowercase()).collect();
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
            } else if name == "System Volume Information" {
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
mod tests {
    use super::*;

    fn at(days: i64) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-01-01T00:00:00Z").unwrap().with_timezone(&Utc) + Duration::days(days)
    }

    #[test]
    fn fits_a_growth_line() {
        let gb = 1_000_000_000u64;
        let points = [(at(0), 50 * gb), (at(30), 53 * gb), (at(60), 56 * gb)];
        let f = fit("Main", 56 * gb, &points, Some(9 * gb + PLAYER_HEADROOM), at(60)).unwrap();
        assert_eq!(f.per_month, 3 * gb as i64);
        // 9 GB of room at 0.1 GB a day.
        assert_eq!(f.full_around, Some(at(150).date_naive()));
        assert!(fit("Main", 0, &points[..1], None, at(0)).is_none());
        assert!(fit("Main", 0, &[(at(0), 1), (at(2), 2)], None, at(2)).is_none());
    }

    #[test]
    fn shrinking_libraries_never_fill() {
        let points = [(at(0), 100), (at(30), 50)];
        let f = fit("Main", 50, &points, Some(1_000_000_000), at(30)).unwrap();
        assert!(f.per_month < 0);
        assert!(f.full_around.is_none());
    }

    #[test]
    fn classifies_files() {
        let referenced: FxHashSet<String> = ["music/a.mp3".to_string()].into_iter().collect();
        assert_eq!(classify("Contents/x.mp3", "x.mp3", &referenced), SpaceKind::Audio);
        assert_eq!(classify("Music/A.mp3", "A.mp3", &referenced), SpaceKind::Audio);
        assert_eq!(classify("Music/b.mp3", "b.mp3", &referenced), SpaceKind::LooseAudio);
        assert_eq!(classify("Contents/._x.mp3", "._x.mp3", &referenced), SpaceKind::OsClutter);
        assert_eq!(
            classify(".locallibrary/backups/1/export.pdb", "export.pdb", &referenced),
            SpaceKind::LocallibraryBackups
        );
        assert_eq!(classify("PIONEER/rekordbox/export.pdb", "export.pdb", &referenced), SpaceKind::Databases);
        assert_eq!(classify("PIONEER/MYSETTING.DAT", "MYSETTING.DAT", &referenced), SpaceKind::Settings);
        assert_eq!(classify("notes.txt", "notes.txt", &referenced), SpaceKind::Other);
    }

    #[test]
    fn classifies_what_rekordbox_keeps_besides_the_library() {
        let none = FxHashSet::default();
        assert_eq!(classify("PIONEER/Artwork/00001/a1.jpg", "a1.jpg", &none), SpaceKind::Artwork);
        assert_eq!(
            classify("PIONEER/rekordbox/exportLibrary.db-shm", "exportLibrary.db-shm", &none),
            SpaceKind::Databases
        );
        let backup = "PIONEER/DeviceLibBackup/rbDevLibBaInfo_815904346.json";
        assert_eq!(classify(backup, "rbDevLibBaInfo_815904346.json", &none), SpaceKind::RekordboxOther);
        assert_eq!(classify(".locallibrary/state.json", "state.json", &none), SpaceKind::Locallibrary);
    }

    /// Whole folders the OS keeps are measured as one kind, wherever they are; links aren't
    /// followed or counted.
    #[test]
    fn system_folders_are_measured_whole() {
        let dir = tempfile::tempdir().unwrap();
        let put = |key: &str, len: usize| {
            let p = dir.path().join(key);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, vec![0u8; len]).unwrap();
        };
        put("System Volume Information/IndexerVolumeGuid", 76);
        put(".Spotlight-V100/Store-V2/store.db", 4096);
        put("Music/.fseventsd/fseventsd-uuid", 36);
        put("PIONEER/Artwork/00001/a1.jpg", 900);
        put("notes.txt", 24);
        #[cfg(unix)]
        std::os::unix::fs::symlink(dir.path().join("notes.txt"), dir.path().join("link.txt")).unwrap();
        let r = breakdown(&Device::open(dir.path()).unwrap(), &Ctx::silent()).unwrap();
        assert_eq!(r.bytes(SpaceKind::Other), 76 + 24);
        assert_eq!(r.bytes(SpaceKind::OsClutter), 4096 + 36);
        assert_eq!(r.bytes(SpaceKind::Artwork), 900);
        let files: usize = r.categories.iter().map(|c| c.files).sum();
        assert_eq!((files, r.unreadable), (5, 0));
        assert_eq!(r.categories[0].kind, SpaceKind::OsClutter, "largest first");
        assert_eq!(dir_size(&dir.path().join("Music")), (1, 36, 0));
    }

    #[test]
    fn warns_when_little_room_is_left() {
        assert!(headroom_warning(Some(1_000_000_000), 900_000_000, 0).is_some());
        assert!(headroom_warning(Some(1_000_000_000), 100_000_000, 0).is_none());
        assert!(headroom_warning(Some(1_000_000_000), 900_000_000, 500_000_000).is_none());
        assert!(headroom_warning(None, 1, 0).is_none());
    }
}
