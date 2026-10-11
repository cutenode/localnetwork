//! A health report for one drive: duplicate tracks, missing and broken files, files nothing
//! references any more, tracks in no playlist, drift between the two databases, files FAT32 can't
//! hold, and how players get on with the drive's file system. On request ([`Section::Audio`]),
//! also audio files that are cut short or don't decode (see [`crate::probe::check_tracks`]), and
//! ([`Section::Analysis`]) stored tempos and keys an analysis of the audio disagrees with, and
//! tracks whose level may be a problem (see [`crate::sound`]).
//!
//! The report never writes to the drive (with `hash_audio`, the hash cache is kept on adopted
//! drives, as every scan does). [`plan_orphans`] / [`apply_orphans`] remove orphaned files: audio
//! under `Contents/`, analysis under `PIONEER/USBANLZ/` and images under `PIONEER/Artwork/` that no
//! track in either database references. They are backed up first (see [`crate::backup`]), so
//! `locallibrary undo` takes it back.

use indexmap::IndexMap;

use rustc_hash::{FxHashMap, FxHashSet};
use std::path::{Path, PathBuf};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::commit::{self, Changes, Options};
use crate::device::{Device, LibraryDrift, LineageOp};
use crate::equipment::{Codec, FromPath};
use crate::error::Reason;
use crate::format::{self, Compatibility, Disk, Volume, players};
use crate::fsutil;
use crate::hash::Hash;
use crate::mix::index::{Cache, fingerprint_library};
use crate::model::{Library, Track, TrackRef, normalize};
use crate::paths::{self, FileClass};
use crate::pdb;
use crate::progress::{Ctx, Phase};
use crate::store::{Snapshot, Store};
use crate::timeline;
use crate::tree::FileTree;
use crate::{Error, Result};
use loclib_mixid::matcher;

/// FAT32 stores file sizes in 32 bits: a file of 4 GiB or more can't be stored on it (or copied
/// to it).
pub const FAT32_MAX_FILE: u64 = 1 << 32;

/// Extensions of the audio files rekordbox exports. Anything else under `Contents/` (a
/// `cover.jpg`, notes) is never reported as orphaned.
pub(crate) const AUDIO_EXTENSIONS: &[&str] = &["mp3", "m4a", "mp4", "aac", "wav", "aif", "aiff", "flac", "ogg", "alac"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[schemars(rename = "DoctorSection")]
#[serde(rename_all = "snake_case")]
pub enum Section {
    Duplicates,
    Missing,
    Orphans,
    Unfiled,
    Drift,
    LargeFiles,
    Compatibility,
    /// Audio files cut short or that don't decode. Only when asked for (`audio`, or `only`
    /// naming it): it reads every file's last seconds.
    Audio,
    /// Stored tempos and keys an analysis of the audio disagrees with, and tracks too loud,
    /// clipped or far from the library's level. Only when asked for (`analyze`, or `only` naming
    /// it): it decodes every track not analysed before.
    Analysis,
}

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct DoctorOptions {
    /// Also find duplicates by audio content. Only files whose size matches another track's are
    /// hashed, through the drive's hash cache.
    #[serde(default)]
    pub hash_audio: bool,
    /// Also find the same recording in other files and encodings (a FLAC and an MP3 of it) by its
    /// sound: every track is fingerprinted (once: the store keeps the prints) and tracks of about
    /// the same length are compared. Decoding a library's audio the first time takes a while.
    #[serde(default)]
    pub by_sound: bool,
    /// Sections to run (all but [`Section::Audio`] when empty).
    #[serde(default)]
    pub only: Vec<Section>,
    /// Also run [`Section::Audio`].
    #[serde(default)]
    pub audio: bool,
    /// Also run [`Section::Analysis`].
    #[serde(default)]
    pub analyze: bool,
    /// Duplicates only by file, audio, sound or Sync Manager copy: leave out tracks that only share
    /// artist, title and length (they may be different edits), also from groups the others join.
    #[serde(default)]
    pub sure_only: bool,
}

impl DoctorOptions {
    fn wants(&self, s: Section) -> bool {
        match s {
            Section::Audio => self.audio || self.only.contains(&s),
            Section::Analysis => self.analyze || self.only.contains(&s),
            _ => self.only.is_empty() || self.only.contains(&s),
        }
    }
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct DoctorReport {
    pub drive: String,
    pub root: PathBuf,
    pub tracks: usize,
    /// Each section is `None` when it wasn't run (see [`DoctorOptions::only`]).
    pub duplicates: Option<Vec<DuplicateGroup>>,
    pub missing: Option<Vec<BrokenTrack>>,
    /// `None` also when OneLibrary can't be read (then what it references is unknown).
    pub orphans: Option<Vec<OrphanFile>>,
    /// Tracks in no playlist.
    pub unfiled: Option<Vec<TrackRef>>,
    /// How `export.pdb` and OneLibrary differ (`None` also on a drive with only one of them).
    pub drift: Option<LibraryDrift>,
    pub large_files: Option<Vec<LargeFile>>,
    /// Audio files cut short or that don't decode (with [`DoctorOptions::audio`]).
    pub audio: Option<AudioReport>,
    /// What an analysis of the audio says about stored tempos, keys and levels (with
    /// [`DoctorOptions::analyze`]). Opinions to look into, not problems.
    pub analysis: Option<crate::sound::AnalysisReport>,
    /// How players get on with the drive's disk, when it is a mounted volume's root.
    pub compatibility: Option<Compatibility>,
    /// The volume's file system as the OS names it.
    pub file_system: Option<String>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct AudioReport {
    /// Audio files opened.
    pub checked: usize,
    pub problems: Vec<crate::probe::AudioFinding>,
}

impl DoctorReport {
    /// Whether it found something to fix: duplicates, missing or orphaned files, drift between
    /// the databases, files too big for FAT32, or audio files cut short (with
    /// [`DoctorOptions::audio`]). (Tracks in no playlist are only listed.)
    pub fn has_problems(&self) -> bool {
        self.duplicates.as_ref().is_some_and(|v| !v.is_empty())
            || self.missing.as_ref().is_some_and(|v| !v.is_empty())
            || self.orphans.as_ref().is_some_and(|v| !v.is_empty())
            || self.drift.as_ref().is_some_and(|d| !d.is_empty())
            || self.large_files.as_ref().is_some_and(|v| !v.is_empty())
            || self.audio.as_ref().is_some_and(|a| !a.problems.is_empty())
    }
}

/// Why tracks are considered the same recording.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DuplicateReason {
    /// Several database rows for one audio file.
    SameFile,
    /// Different files with identical audio (with `hash_audio`).
    SameAudio,
    /// The same recording in another file or encoding (with `by_sound`): their fingerprints line
    /// up over most of both, and their lengths agree (an extended mix and its radio edit share
    /// most of their audio, but not their length).
    SameRecording,
    /// rekordbox Sync Manager copies: `Name-1.mp3` or `Name (1).mp3` next to `Name.mp3`, same size.
    SyncManagerCopy,
    /// Same artist, title and duration.
    SameMetadata,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct DuplicateGroup {
    pub reasons: Vec<DuplicateReason>,
    /// Suggested keeper first.
    pub tracks: Vec<DuplicateTrack>,
    /// Id of the suggested keeper: the best audio (lossless over lossy, then the higher bitrate,
    /// sample rate and bit depth), then the most cues, playlists and history lists, then the
    /// earliest date added.
    pub keep: u32,
    /// What put the keeper ahead of the next copy (`lossless over lossy`, `320 kbps over 192`).
    pub keep_reason: String,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct DuplicateTrack {
    pub track: TrackRef,
    /// Playlists the track is in.
    pub playlists: usize,
    /// History lists (what players recorded) the track is in.
    pub history: usize,
    /// Hot cues, memory cues and loops in its analysis files.
    pub cues: usize,
    pub play_count: u16,
    /// FLAC, WAV, AIFF or Apple Lossless.
    pub lossless: bool,
    pub bitrate: u32,
    pub sample_rate: u32,
    pub bit_depth: u16,
    pub file_size: u64,
    pub date_added: String,
}

impl DuplicateTrack {
    /// What decides between copies, best first: the audio (for a lossless file its bitrate says
    /// how well it compressed, not how it sounds), then what the DJ put into the track.
    fn rank(&self) -> impl Ord {
        let bitrate = if self.lossless { 0 } else { self.bitrate };
        (self.lossless, bitrate, self.sample_rate, self.bit_depth, self.cues, self.playlists, self.history)
    }

    /// Why `self` ranks ahead of `next`.
    fn reason_over(&self, next: &DuplicateTrack) -> String {
        if self.lossless != next.lossless {
            return "lossless over lossy".into();
        }
        if !self.lossless && self.bitrate != next.bitrate {
            return format!("{} kbps over {}", self.bitrate, next.bitrate);
        }
        if self.sample_rate != next.sample_rate {
            return format!("{} Hz over {}", self.sample_rate, next.sample_rate);
        }
        if self.bit_depth != next.bit_depth {
            return format!("{}-bit over {}-bit", self.bit_depth, next.bit_depth);
        }
        for (a, b, what) in [
            (self.cues, next.cues, "cue(s)"),
            (self.playlists, next.playlists, "playlist(s)"),
            (self.history, next.history, "history list(s)"),
        ] {
            if a != b {
                return format!("{a} {what} over {b}");
            }
        }
        match self.date_added.is_empty() || self.date_added == next.date_added {
            true => "no other difference: the lowest id".into(),
            false => format!("added first ({})", self.date_added),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, JsonSchema)]
#[schemars(rename = "DoctorProblem")]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum Problem {
    /// The audio file isn't on the drive.
    MissingAudio,
    /// The database's path for it leaves the drive.
    UnsafePath,
    /// The audio file is smaller than the database recorded, and its audio stops before its
    /// length: a copy cut short.
    Truncated,
    /// The analysis file (`.DAT`: beat grid, cues, waveform) isn't on the drive.
    MissingAnalysis,
    /// The artwork image isn't on the drive.
    MissingArtwork,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct BrokenTrack {
    pub track: TrackRef,
    pub problem: Problem,
    /// The file concerned, as a drive key.
    pub path: String,
    pub detail: Option<String>,
    /// The database listing the track: `device_library` (`export.pdb`) or `onelibrary`.
    pub database: &'static str,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct OrphanFile {
    /// Drive key.
    pub path: String,
    pub size: u64,
    pub class: FileClass,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct LargeFile {
    pub path: String,
    pub size: u64,
    /// The track using it, if any.
    pub track: Option<TrackRef>,
}

// ---------------------------------------------------------------------------------------------
// Reading both databases
// ---------------------------------------------------------------------------------------------

/// A drive's (or snapshot's) libraries from both databases, read once.
pub(crate) struct Databases {
    /// `export.pdb`'s library.
    pub pdb: Option<Library>,
    /// OneLibrary's library.
    pub plus: Option<Library>,
    /// OneLibrary exists but couldn't be read (on a drive that also has `export.pdb`).
    pub plus_error: Option<Error>,
    /// Image paths every artwork / image row points at, used by tracks or not (read from drives
    /// only, for finding orphaned images).
    pub images: Vec<String>,
}

impl Databases {
    /// Read a drive's databases. An unreadable OneLibrary is an error only on a drive without
    /// `export.pdb`.
    pub fn read(device: &Device) -> Result<Databases> {
        if !device.has_export() {
            return Err(Error::NotAnExport(device.root().to_path_buf()));
        }
        let mut images = Vec::new();
        let pdb = match device.has_device_library() {
            true => {
                let db = pdb::parse_database(device.pdb_bytes()?, &device.path(paths::EXPORT_PDB))?;
                images.extend(db.artwork.iter().map(|a| a.path.clone()));
                Some(pdb::library_from_database(&db))
            }
            false => None,
        };
        let (plus, plus_error) = match device.has_library_plus() {
            false => (None, None),
            true => match device.library_plus().and_then(|one| Ok((one.library()?, image_paths(&one)?))) {
                Ok((lib, paths)) => {
                    images.extend(paths);
                    (Some(lib), None)
                }
                Err(e) if pdb.is_some() => (None, Some(e)),
                Err(e) => return Err(e),
            },
        };
        Ok(Databases { pdb, plus, plus_error, images })
    }

    /// Read the databases stored in a snapshot.
    pub fn from_snapshot(store: &Store, snapshot: &Snapshot) -> Result<Databases> {
        let object = |key: &str| crate::compare::snapshot_object(store, snapshot, key);
        let pdb = match object(paths::EXPORT_PDB)? {
            Some((path, bytes)) => Some(pdb::library_from_database(&pdb::parse_database(bytes, &path)?)),
            None => None,
        };
        let (plus, plus_error) = match object(paths::LIBRARY_PLUS_DB)? {
            None => (None, None),
            Some((_, db)) => {
                let wal = object(paths::LIBRARY_PLUS_WAL)?.map(|(_, b)| b);
                match crate::onelibrary::OneLibrary::from_encrypted(&db, wal.as_deref()).and_then(|one| one.library()) {
                    Ok(lib) => (Some(lib), None),
                    Err(e) if pdb.is_some() => (None, Some(e)),
                    Err(e) => return Err(e),
                }
            }
        };
        if pdb.is_none() && plus.is_none() {
            return Err(Error::NotFound(format!("export.pdb or exportLibrary.db in snapshot {}", snapshot.id.short())));
        }
        Ok(Databases { pdb, plus, plus_error, images: Vec::new() })
    }

    /// The library players see: `export.pdb`'s when there is one, otherwise OneLibrary's.
    pub fn primary(&self) -> &Library {
        self.pdb.as_ref().or(self.plus.as_ref()).expect("one database was read")
    }

    /// OneLibrary's library when it's beside `export.pdb`'s (the primary): a CDJ-3000X writes
    /// only to OneLibrary, so what only it holds is read from here.
    pub fn beside(&self) -> Option<&Library> {
        self.pdb.as_ref().and(self.plus.as_ref())
    }

    /// Each readable database's library, primary first, with its name (`device_library` or
    /// `onelibrary`).
    pub fn all(&self) -> impl Iterator<Item = (&'static str, &Library)> {
        self.pdb.iter().map(|l| ("device_library", l)).chain(self.plus.iter().map(|l| ("onelibrary", l)))
    }

    /// Every track either database lists (audio outside `Contents/` included in a scan).
    pub fn everything(&self) -> Library {
        Library { tracks: self.all().flat_map(|(_, l)| l.tracks.iter().cloned()).collect(), ..Default::default() }
    }

    /// How the two databases differ, when both were read.
    pub fn drift(&self) -> Option<LibraryDrift> {
        match (&self.pdb, &self.plus, &self.plus_error) {
            (Some(pdb), Some(plus), _) => Some(LibraryDrift::between(pdb, plus)),
            (Some(_), None, Some(e)) => Some(LibraryDrift { error: Some(e.to_string()), ..Default::default() }),
            _ => None,
        }
    }
}

/// Every image row's path in a OneLibrary database.
fn image_paths(one: &crate::onelibrary::OneLibrary) -> Result<Vec<String>> {
    let mut stmt = one.connection().prepare("SELECT path FROM image")?;
    let rows = stmt.query_map([], |r| r.get::<_, Option<String>>(0))?;
    let mut out = Vec::new();
    for r in rows {
        out.extend(r?.filter(|p| !p.is_empty()));
    }
    Ok(out)
}

pub(crate) fn fold(s: &str) -> String {
    s.to_lowercase()
}

// ---------------------------------------------------------------------------------------------
// The report
// ---------------------------------------------------------------------------------------------

pub fn run(device: &Device, opts: &DoctorOptions, ctx: &Ctx<'_>) -> Result<DoctorReport> {
    let dbs = Databases::read(device)?;
    let mut tree = device.scan(Some(&dbs.everything()), ctx)?;
    let library = dbs.primary();
    let mut warnings = Vec::new();
    if let Some(e) = &dbs.plus_error {
        warnings.push(format!("{e}; only export.pdb was checked"));
    }
    let duplicates = match opts.wants(Section::Duplicates) {
        true => Some(duplicates(device, library, &mut tree, opts, &mut warnings, ctx)?),
        false => None,
    };
    let orphans = match (opts.wants(Section::Orphans), &dbs.plus_error) {
        (false, _) => None,
        (true, Some(_)) => {
            warnings.push("orphaned files weren't looked for: what OneLibrary references is unknown".into());
            None
        }
        (true, None) => {
            if device.state().is_some_and(|s| s.pending.is_some()) {
                warnings.push(
                    "an interrupted operation may have left files the database doesn't reference yet; \
                     finish it before removing orphans"
                        .into(),
                );
            }
            Some(orphans(&dbs, &tree))
        }
    };
    let audio = match opts.wants(Section::Audio) {
        true => {
            let (checked, problems) = crate::probe::check_tracks(device, &dbs.everything(), &tree, ctx)?;
            Some(AudioReport { checked, problems })
        }
        false => None,
    };
    let analysis =
        opts.wants(Section::Analysis).then(|| crate::sound::analyze_library(device, library, ctx)).transpose()?;
    let volume = opts.wants(Section::Compatibility).then(|| volume_of(device.root())).flatten();
    Ok(DoctorReport {
        drive: device.name(),
        root: device.root().to_path_buf(),
        tracks: library.tracks.len(),
        duplicates,
        missing: opts.wants(Section::Missing).then(|| missing(device, &dbs, &tree, ctx)).transpose()?,
        orphans,
        unfiled: opts.wants(Section::Unfiled).then(|| unfiled(library)),
        drift: opts.wants(Section::Drift).then(|| dbs.drift()).flatten(),
        large_files: opts.wants(Section::LargeFiles).then(|| large_files(library, &tree)),
        audio,
        analysis,
        file_system: volume.as_ref().and_then(|(_, v)| v.filesystem_name.clone()),
        compatibility: volume.as_ref().map(|(disk, _)| players::assess_disk(disk)),
        warnings,
    })
}

/// The disk a drive is mounted from and its volume, when `root` is the root of a mounted volume
/// (asks the OS: `diskutil`, `lsblk` or PowerShell).
pub fn volume_of(root: &Path) -> Option<(Disk, Volume)> {
    let disk = format::disk_at(root).ok()?;
    let canonical = |p: &Path| p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
    let root = canonical(root);
    let volume = disk.volumes.iter().find(|v| v.mount.as_deref().is_some_and(|m| canonical(m) == root))?.clone();
    Some((disk, volume))
}

/// Tracks in no playlist.
pub fn unfiled(library: &Library) -> Vec<TrackRef> {
    let filed: FxHashSet<u32> =
        library.playlists.iter().filter(|p| !p.is_folder).flat_map(|p| p.track_ids.iter().copied()).collect();
    library.tracks.iter().filter(|t| !filed.contains(&t.id)).map(TrackRef::from).collect()
}

/// Library files of 4 GiB or more, which FAT32 can't hold.
pub fn large_files(library: &Library, tree: &FileTree) -> Vec<LargeFile> {
    let by_key: FxHashMap<String, &Track> =
        library.tracks.iter().map(|t| (fold(&paths::key(&t.file_path)), t)).collect();
    tree.library_entries()
        .filter(|e| e.size >= FAT32_MAX_FILE)
        .map(|e| LargeFile {
            path: e.path.clone(),
            size: e.size,
            track: by_key.get(&fold(&e.path)).map(|t| TrackRef::from(*t)),
        })
        .collect()
}

/// Tracks whose audio, analysis or artwork is missing, or whose audio is cut short. A track both
/// databases list is reported once; tracks sharing a missing file (an album's cover) each are.
///
/// rekordbox records a file's size when it imports it and keeps it when a tag editor (Mixed In
/// Key, Serato, Mp3tag) later rewrites the file's tags, so a size that differs says little. A cut
/// copy is always smaller, though: only files smaller than recorded have their end decoded
/// (see [`crate::probe::check_tracks`]), and only those whose audio stops early are reported.
pub(crate) fn missing(device: &Device, dbs: &Databases, tree: &FileTree, ctx: &Ctx<'_>) -> Result<Vec<BrokenTrack>> {
    let exists = |key: &str| present(device, tree, key);
    let size = |key: &str| {
        tree.resolve(key)
            .map(|e| e.size)
            .or_else(|| device.checked_path(key).ok().and_then(|p| std::fs::metadata(p).ok()).map(|m| m.len()))
    };
    let mut out = Vec::new();
    // (problem, file, the track's audio file)
    let mut seen: FxHashSet<(Problem, String, String)> = FxHashSet::default();
    let mut smaller = Library::default();
    for (database, library) in dbs.all() {
        for t in &library.tracks {
            let audio = paths::key(&t.file_path);
            let track_audio = fold(&audio);
            let mut report = |problem: Problem, path: String, detail: Option<String>| {
                if seen.insert((problem, fold(&path), track_audio.clone())) {
                    out.push(BrokenTrack { track: t.into(), problem, path, detail, database });
                }
            };
            if !paths::is_safe_key(&audio) {
                report(
                    Problem::UnsafePath,
                    audio,
                    Some(format!("the database's path {:?} leaves the drive", t.file_path)),
                );
            } else if !exists(&audio) {
                report(Problem::MissingAudio, audio, None);
            } else if size(&audio).is_some_and(|n| n < t.file_size) {
                // export.pdb keeps sizes modulo 2^32, so a file of 4 GiB or more never looks smaller.
                smaller.tracks.push(t.clone());
            }
            let present = |key: &str| paths::is_safe_key(key) && exists(key);
            let analysis = paths::key(&t.analyze_path);
            if !analysis.is_empty() && !present(&analysis) {
                report(Problem::MissingAnalysis, analysis, None);
            }
            if let Some(art) = t.artwork_path.as_deref().map(paths::key).filter(|k| !k.is_empty())
                && !present(&art)
            {
                report(Problem::MissingArtwork, art, None);
            }
        }
    }
    let (_, findings) = crate::probe::check_tracks(device, &smaller, tree, ctx)?;
    for f in findings.into_iter().filter(|f| matches!(f.problem, crate::probe::AudioProblem::Truncated { .. })) {
        let database = dbs.all().find(|(_, l)| l.tracks.iter().any(|t| t.file_path == f.track.file_path));
        out.push(BrokenTrack {
            path: paths::key(&f.track.file_path),
            detail: Some(f.problem.to_string()),
            database: database.map_or("device_library", |(d, _)| d),
            track: f.track,
            problem: Problem::Truncated,
        });
    }
    Ok(out)
}

/// Whether the file `key` is on the drive: in the scan, or outside the folders it covers.
pub(crate) fn present(device: &Device, tree: &FileTree, key: &str) -> bool {
    tree.resolve(key).is_some()
        || (paths::classify(key).is_none() && device.checked_path(key).is_ok_and(|p| p.is_file()))
}

/// Audio under `Contents/`, analysis under `PIONEER/USBANLZ/` and images under
/// `PIONEER/Artwork/` that nothing in either database references. Analysis files go by their
/// stem (a track's `.DAT` keeps its `.EXT` and `.2EX`), artwork with its `_m` variant.
pub(crate) fn orphans(dbs: &Databases, tree: &FileTree) -> Vec<OrphanFile> {
    fn use_image(key: &str, in_use: &mut FxHashSet<String>) {
        in_use.insert(fold(key));
        in_use.insert(fold(&crate::artwork::medium_variant(key)));
    }
    let mut in_use: FxHashSet<String> = FxHashSet::default();
    let mut analysis_stems: FxHashSet<String> = FxHashSet::default();
    for (_, library) in dbs.all() {
        for t in &library.tracks {
            in_use.insert(fold(&paths::key(&t.file_path)));
            let dat = paths::key(&t.analyze_path);
            if let Some((stem, _)) = dat.rsplit_once('.') {
                analysis_stems.insert(fold(stem));
            }
            if let Some(a) = &t.artwork_path {
                use_image(&paths::key(a), &mut in_use);
            }
        }
    }
    for image in &dbs.images {
        use_image(&paths::key(image), &mut in_use);
    }
    tree.entries
        .values()
        .filter(|e| paths::classify(&e.path) == Some(e.class))
        .filter(|e| match e.class {
            FileClass::Audio => {
                let ext = e.path.rsplit_once('.').map(|(_, x)| x.to_ascii_lowercase()).unwrap_or_default();
                e.path.starts_with("Contents/")
                    && AUDIO_EXTENSIONS.contains(&ext.as_str())
                    && !in_use.contains(&fold(&e.path))
            }
            FileClass::Analysis => {
                let stem = e.path.rsplit_once('.').map_or(e.path.as_str(), |(s, _)| s);
                !analysis_stems.contains(&fold(stem))
            }
            FileClass::Artwork => !in_use.contains(&fold(&e.path)),
            _ => false,
        })
        .map(|e| OrphanFile { path: e.path.clone(), size: e.size, class: e.class })
        .collect()
}

// ---------------------------------------------------------------------------------------------
// Duplicates
// ---------------------------------------------------------------------------------------------

/// `Name-1` / `Name (1)` -> `Name`: the file names rekordbox's Sync Manager gives a second copy.
fn sync_manager_base(stem: &str) -> &str {
    let digits_before = |s: &str, end: usize| {
        let d = &s[..end];
        let start = d.trim_end_matches(|c: char| c.is_ascii_digit()).len();
        (start < end).then_some(start)
    };
    if let Some(start) = digits_before(stem, stem.len())
        && start > 1
        && stem[..start].ends_with('-')
    {
        return &stem[..start - 1];
    }
    if let Some(inner) = stem.strip_suffix(')')
        && let Some(start) = digits_before(inner, inner.len())
        && inner[..start].ends_with(" (")
        && start > 2
    {
        return &inner[..start - 2];
    }
    stem
}

struct UnionFind(Vec<usize>);

impl UnionFind {
    fn find(&mut self, i: usize) -> usize {
        let mut root = i;
        while self.0[root] != root {
            root = self.0[root];
        }
        let mut cur = i;
        while self.0[cur] != root {
            let next = self.0[cur];
            self.0[cur] = root;
            cur = next;
        }
        root
    }

    fn union(&mut self, a: usize, b: usize) {
        let (ra, rb) = (self.find(a), self.find(b));
        if ra != rb {
            self.0[ra.max(rb)] = ra.min(rb);
        }
    }
}

/// Tracks whose lengths differ by more than this (seconds) aren't compared by sound. Two
/// encodings of one recording differ by an encoder's padding (tens of milliseconds) or silence a
/// rip trimmed or added (a second or so); an edit differs by whole bars, over 5 s even for 4 bars
/// at 174 BPM. So an extended mix and its radio edit, which share most of their audio, never meet.
pub(crate) const SAME_LENGTH_SECS: f32 = 2.0;
/// Of each track's stretches with sound in them, the share heard at one offset for two tracks to
/// be one recording ([`matcher::align`]'s coverage). A re-encoded or shifted copy is heard in
/// all of them, and quiet passages keep a few from counting; an edit cut or moved a stretch, so
/// only part of it lines up at any one offset (about half of each, with 16 s cut from the middle).
pub(crate) const SAME_RECORDING_COVERAGE: f64 = 0.8;

/// Pairs of `library`'s tracks (indexes) that are one recording by their sound: every track is
/// fingerprinted (through the store's cache, so only new files are decoded) and those of about
/// the same length compared. Tracks that couldn't be fingerprinted are left out with a warning.
fn same_recordings(
    device: &Device,
    library: &Library,
    warnings: &mut Vec<String>,
    ctx: &Ctx<'_>,
) -> Result<Vec<(usize, usize)>> {
    let (cache, _temp) = Cache::of_default_store()?;
    let (mut prints, report) = fingerprint_library(device, library, &cache, ctx)?;
    if let Some(f) = report.failed.first() {
        warnings.push(format!(
            "{} track(s) couldn't be decoded, so they weren't compared by sound (e.g. {}: {})",
            report.failed.len(),
            f.track.name,
            f.error
        ));
    }
    prints.sort_by(|a, b| a.print.duration_secs.total_cmp(&b.print.duration_secs));
    let index: FxHashMap<u32, usize> = library.tracks.iter().enumerate().map(|(i, t)| (t.id, i)).collect();
    // Rows for one file are duplicates by file already. (Two files can share a cache key: one
    // file in two folders, with its name and time.)
    let file = |id: u32| fold(&paths::key(&library.tracks[index[&id]].file_path));
    let mut pairs = Vec::new();
    for (i, a) in prints.iter().enumerate() {
        for b in
            prints[i + 1..].iter().take_while(|b| b.print.duration_secs - a.print.duration_secs <= SAME_LENGTH_SECS)
        {
            if file(a.id) != file(b.id) {
                pairs.push((a, b));
            }
        }
    }
    let path = |id: u32| library.tracks[index[&id]].file_path.clone();
    let aligned = crate::probe::parallel(
        &pairs,
        Phase::Match,
        |(a, _)| (path(a.id), 0),
        |(a, b)| matcher::align(&a.print, &b.print),
        ctx,
    )?;
    Ok(pairs
        .iter()
        .zip(aligned)
        .filter(|(_, al)| {
            al.is_some_and(|al| al.coverage.0 >= SAME_RECORDING_COVERAGE && al.coverage.1 >= SAME_RECORDING_COVERAGE)
        })
        .map(|((a, b), _)| (index[&a.id], index[&b.id]))
        .collect())
}

/// Seconds to add to a time in `a` for the same moment in `b`, when the two are one recording by
/// their sound and the offset between them was measured to the sample (see [`measured_offset`]);
/// otherwise why not.
pub(crate) fn offset_between(
    device: &Device,
    a: &Track,
    b: &Track,
    ctx: &Ctx<'_>,
) -> Result<std::result::Result<f64, &'static str>> {
    let (cache, _temp) = Cache::of_default_store()?;
    let both = Library { tracks: vec![a.clone(), b.clone()], ..Default::default() };
    let (prints, _) = fingerprint_library(device, &both, &cache, ctx)?;
    let [pa, pb] = prints.as_slice() else { return Ok(Err("one of them couldn't be decoded")) };
    let file = |t: &Track| device.checked_path(&paths::key(&t.file_path));
    match matcher::align(&pa.print, &pb.print) {
        Some(al) if al.coverage.0 >= SAME_RECORDING_COVERAGE && al.coverage.1 >= SAME_RECORDING_COVERAGE => {
            // Refining searches a few milliseconds around the prints' estimate, so it needs one.
            if !al.is_precise() {
                return Ok(Err("the offset between them couldn't be estimated"));
            }
            // From one decode to the other, then from each decode to rekordbox's timeline.
            let (Some(lead_a), Some(lead_b)) = (timeline::lead(device, a, ctx)?, timeline::lead(device, b, ctx)?)
            else {
                return Ok(Err(timeline::UNTOLD));
            };
            Ok(measured_offset(&file(a)?, &file(b)?, al.offset, ctx)?.map(|secs| secs + lead_a - lead_b))
        }
        _ => Ok(Err("they don't sound like one recording")),
    }
}

/// Seconds to add to a time in file `a` for the same moment in file `b`, two files of one
/// recording whose prints line up at `coarse` ([`matcher::Alignment::offset`], good to a few
/// milliseconds): measured to a fraction of a sample from their audio
/// ([`loclib_mixid::refine`]). Cues and beat grids are moved by it, so audio that doesn't line up
/// closely there (a different edit or master) is why not, rather than an estimate.
pub(crate) fn measured_offset(
    a: &Path,
    b: &Path,
    coarse: f64,
    ctx: &Ctx<'_>,
) -> Result<std::result::Result<f64, &'static str>> {
    match loclib_mixid::refine(a, b, coarse, ctx) {
        Ok(r) if r.correlation >= loclib_mixid::REFINE_MIN_CORRELATION => Ok(Ok(r.offset)),
        Ok(_) => Ok(Err("their audio doesn't line up closely enough to move cues by")),
        Err(loclib_mixid::Error::Cancelled(_)) => Err(Error::Cancelled),
        Err(_) => Ok(Err("their audio couldn't be compared")),
    }
}

/// Whether the audio of `t` is lossless: by its extension, or for an MPEG-4 file (AAC or Apple
/// Lossless), by its header.
pub(crate) fn lossless(device: &Device, t: &Track) -> bool {
    match Codec::from_path(&t.file_path) {
        FromPath::Codec(c) => c.lossless(),
        FromPath::AacOrAlac => device
            .checked_path(&paths::key(&t.file_path))
            .ok()
            .and_then(|p| crate::probe::format(&p).ok())
            .is_some_and(|f| f.codec == Some(Codec::Alac)),
        FromPath::Unknown => false,
    }
}

/// Groups of tracks that are the same recording, each with a suggested keeper.
fn duplicates(
    device: &Device,
    library: &Library,
    tree: &mut FileTree,
    opts: &DoctorOptions,
    warnings: &mut Vec<String>,
    ctx: &Ctx<'_>,
) -> Result<Vec<DuplicateGroup>> {
    let tracks = &library.tracks;
    let keys: Vec<String> = tracks.iter().map(|t| fold(&paths::key(&t.file_path))).collect();
    // Each track's audio file on the drive (its tree key), if present.
    let files: Vec<Option<String>> =
        tracks.iter().map(|t| tree.resolve(&paths::key(&t.file_path)).map(|e| e.path.clone())).collect();
    let sizes: Vec<u64> = (0..tracks.len())
        .map(|i| files[i].as_ref().and_then(|k| tree.get(k)).map_or(tracks[i].file_size, |e| e.size))
        .collect();

    let mut edges: Vec<(usize, usize, DuplicateReason)> = Vec::new();
    let mut link = |groups: IndexMap<_, Vec<usize>>, reason: DuplicateReason, distinct_files: bool| {
        for members in groups.into_values() {
            for pair in members.windows(2) {
                if !distinct_files || keys[pair[0]] != keys[pair[1]] {
                    edges.push((pair[0], pair[1], reason));
                }
            }
        }
    };
    let group_by = |f: &dyn Fn(usize) -> Option<String>| {
        let mut groups: IndexMap<String, Vec<usize>> = IndexMap::new();
        for i in 0..tracks.len() {
            if let Some(k) = f(i) {
                groups.entry(k).or_default().push(i);
            }
        }
        groups
    };
    link(group_by(&|i| Some(keys[i].clone())), DuplicateReason::SameFile, false);
    if !opts.sure_only {
        link(
            group_by(&|i| (!normalize(&tracks[i].title).is_empty()).then(|| tracks[i].fingerprint_key())),
            DuplicateReason::SameMetadata,
            true,
        );
    }
    link(
        group_by(&|i| {
            let (dir, name) = keys[i].rsplit_once('/').unwrap_or(("", &keys[i]));
            let (stem, ext) = name.rsplit_once('.')?;
            Some(format!("{dir}/{}.{ext}\u{1f}{}", sync_manager_base(stem), sizes[i]))
        }),
        DuplicateReason::SyncManagerCopy,
        true,
    );
    if opts.hash_audio {
        // Only files whose size matches another file's can have identical content.
        let mut by_size: FxHashMap<u64, FxHashSet<&str>> = FxHashMap::default();
        for k in files.iter().flatten() {
            if let Some(e) = tree.get(k) {
                by_size.entry(e.size).or_default().insert(k);
            }
        }
        let candidates: FxHashSet<String> =
            by_size.into_values().filter(|s| s.len() > 1).flatten().map(str::to_string).collect();
        let mut index = device.load_index();
        tree.ensure_hashes(&mut index, |e| candidates.contains(&e.path), ctx)?;
        device.save_index(&index)?;
        let hashes: Vec<Option<Hash>> =
            files.iter().map(|k| k.as_ref().and_then(|k| tree.get(k)).and_then(|e| e.hash)).collect();
        link(group_by(&|i| hashes[i].map(|h| h.to_hex())), DuplicateReason::SameAudio, true);
    }
    if opts.by_sound {
        let pairs = same_recordings(device, library, warnings, ctx)?;
        edges.extend(pairs.into_iter().map(|(a, b)| (a, b, DuplicateReason::SameRecording)));
    }

    let mut uf = UnionFind((0..tracks.len()).collect());
    for (a, b, _) in &edges {
        uf.union(*a, *b);
    }
    let mut groups: IndexMap<usize, (Vec<usize>, Vec<DuplicateReason>)> = IndexMap::new();
    for i in 0..tracks.len() {
        groups.entry(uf.find(i)).or_default().0.push(i);
    }
    for (a, _, reason) in &edges {
        let reasons = &mut groups.get_mut(&uf.find(*a)).expect("every track has a group").1;
        if !reasons.contains(reason) {
            reasons.push(*reason);
        }
    }

    let count_lists = |lists: &[crate::model::Playlist]| {
        let mut n: FxHashMap<u32, usize> = FxHashMap::default();
        for p in lists.iter().filter(|p| !p.is_folder) {
            for id in p.track_ids.iter().collect::<FxHashSet<_>>() {
                *n.entry(*id).or_default() += 1;
            }
        }
        n
    };
    let (memberships, history) = (count_lists(&library.playlists), count_lists(&library.history));
    let mut out: Vec<DuplicateGroup> = groups
        .into_values()
        .filter(|(members, _)| members.len() > 1)
        .map(|(members, mut reasons)| {
            reasons.sort();
            let mut tracks: Vec<DuplicateTrack> = members
                .iter()
                .map(|&i| {
                    let t = &tracks[i];
                    DuplicateTrack {
                        track: t.into(),
                        playlists: memberships.get(&t.id).copied().unwrap_or(0),
                        history: history.get(&t.id).copied().unwrap_or(0),
                        cues: crate::anlz::load_grid(device, t).cues.len(),
                        play_count: t.play_count,
                        lossless: lossless(device, t),
                        bitrate: t.bitrate,
                        sample_rate: t.sample_rate,
                        bit_depth: t.sample_depth,
                        file_size: sizes[i],
                        date_added: t.date_added.clone(),
                    }
                })
                .collect();
            // Keeper first (see `DuplicateTrack::rank`), then earliest date added (tracks without
            // a date last).
            tracks.sort_by(|a, b| {
                b.rank()
                    .cmp(&a.rank())
                    .then_with(|| match (a.date_added.is_empty(), b.date_added.is_empty()) {
                        (false, true) => std::cmp::Ordering::Less,
                        (true, false) => std::cmp::Ordering::Greater,
                        _ => a.date_added.cmp(&b.date_added),
                    })
                    .then(a.track.id.cmp(&b.track.id))
            });
            DuplicateGroup { reasons, keep: tracks[0].track.id, keep_reason: tracks[0].reason_over(&tracks[1]), tracks }
        })
        .collect();
    out.sort_by(|a, b| a.tracks[0].track.name.to_lowercase().cmp(&b.tracks[0].track.name.to_lowercase()));
    Ok(out)
}

// ---------------------------------------------------------------------------------------------
// Removing orphaned files
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct OrphanPlan {
    pub drive: String,
    pub root: PathBuf,
    pub files: Vec<OrphanFile>,
    /// Total size of `files`.
    pub bytes: u64,
    /// Space needed for the backup of the database and the deleted files.
    pub needed_bytes: u64,
    pub free_space: Option<u64>,
    pub fits: bool,
    pub warnings: Vec<String>,
}

impl OrphanPlan {
    pub fn is_noop(&self) -> bool {
        self.files.is_empty()
    }
}

#[derive(Debug, Clone, Default, Serialize, JsonSchema)]
pub struct OrphanCleanReport {
    pub files_deleted: usize,
    pub freed_bytes: u64,
    pub backup: Option<String>,
    pub fingerprint: Option<Hash>,
}

/// The drive's orphaned files. Refuses when what a database references can't be known (an
/// unreadable OneLibrary) or an interrupted operation may have left files the database will
/// reference once it is finished.
fn find_orphans(device: &Device, ctx: &Ctx<'_>) -> Result<Vec<OrphanFile>> {
    if let Some(p) = device.state().and_then(|s| s.pending.as_ref()) {
        return Err(Error::conflict(
            Reason::Interrupted,
            format!(
                "an interrupted operation ({}) may have left files the database doesn't reference yet; re-run it first",
                p.description
            ),
        )
        .with("operation", &p.description));
    }
    let dbs = Databases::read(device)?;
    if let Some(e) = dbs.plus_error {
        let reason = match e {
            Error::OneLibrary(m) => m,
            other => other.to_string(),
        };
        return Err(Error::OneLibrary(format!("{reason}; without it, which files are orphaned can't be known")));
    }
    let tree = device.scan(Some(&dbs.everything()), ctx)?;
    Ok(orphans(&dbs, &tree))
}

pub fn plan_orphans(device: &Device, ctx: &Ctx<'_>) -> Result<OrphanPlan> {
    let files = find_orphans(device, ctx)?;
    let bytes = files.iter().map(|f| f.size).sum();
    let cluster = fsutil::cluster_size(device.root());
    let round = |n: u64| n.div_ceil(cluster) * cluster;
    let size = |key| std::fs::metadata(device.path(key)).map(|m| m.len()).unwrap_or(0);
    let needed_bytes = match files.is_empty() {
        true => 0,
        false => {
            files.iter().map(|f| round(f.size)).sum::<u64>()
                + paths::DATABASE_FILES.iter().map(|k| round(size(k))).sum::<u64>()
        }
    };
    let free_space = fsutil::available_space(device.root());
    let fits = free_space.is_none_or(|f| needed_bytes <= f);
    let mut warnings = Vec::new();
    if !fits {
        warnings.push(format!(
            "the backup needs about {needed_bytes} bytes but only {} are free on the drive",
            free_space.unwrap_or(0)
        ));
    }
    Ok(OrphanPlan {
        drive: device.name(),
        root: device.root().to_path_buf(),
        files,
        bytes,
        needed_bytes,
        free_space,
        fits,
        warnings,
    })
}

pub fn apply_orphans(plan: &OrphanPlan, device: &mut Device, ctx: &Ctx<'_>) -> Result<OrphanCleanReport> {
    if plan.is_noop() {
        return Ok(OrphanCleanReport::default());
    }
    if !plan.fits {
        return Err(Error::NoSpace {
            path: plan.root.clone(),
            needed: plan.needed_bytes,
            available: plan.free_space.unwrap_or(0),
        });
    }
    let _lock = commit::lock(device, None)?;
    // Under the lock, every planned file must still be orphaned (rekordbox may have exported a
    // track that uses it since).
    let still: FxHashSet<String> = find_orphans(device, ctx)?.iter().map(|f| fold(&f.path)).collect();
    if plan.files.iter().any(|f| !still.contains(&fold(&f.path)) && device.path(&f.path).exists()) {
        return Err(Error::stale(device.name(), "changed"));
    }
    ctx.check()?;
    // Only files locallibrary manages, inside Contents/ and PIONEER/.
    let deletes: Vec<String> = plan
        .files
        .iter()
        .map(|f| f.path.clone())
        .filter(|k| matches!(paths::classify(k), Some(FileClass::Audio | FileClass::Analysis | FileClass::Artwork)))
        .collect();
    let opts = Options::new(LineageOp::Edit, "remove orphaned files").backup("before removing orphaned files");
    let done = commit::commit(device, Changes { deletes, ..Default::default() }, opts, ctx)?;
    Ok(OrphanCleanReport {
        files_deleted: done.deleted,
        freed_bytes: done.freed,
        backup: done.backup,
        fingerprint: done.fingerprint,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sync_manager_names() {
        assert_eq!(sync_manager_base("House1-1"), "House1");
        assert_eq!(sync_manager_base("House1-12"), "House1");
        assert_eq!(sync_manager_base("Track (2)"), "Track");
        assert_eq!(sync_manager_base("Track"), "Track");
        assert_eq!(sync_manager_base("-1"), "-1");
        assert_eq!(sync_manager_base("(1)"), "(1)");
        assert_eq!(sync_manager_base("Track (live)"), "Track (live)");
        assert_eq!(sync_manager_base("Track 2"), "Track 2");
    }

    /// Regression: tracks sharing a missing cover were reported as one, so `check` counted one
    /// track without artwork for a whole album.
    #[test]
    fn each_track_missing_a_shared_file_is_reported_once() {
        let dir = tempfile::tempdir().unwrap();
        let d = Device::open(dir.path()).unwrap();
        let track = |id, path: &str| Track {
            artwork_path: Some("/PIONEER/Artwork/00001/a1.jpg".into()),
            ..loclib_model::test_util::track(id, "A", "T", path)
        };
        let lib =
            Library { tracks: vec![track(1, "/Contents/a.mp3"), track(2, "/Contents/b.mp3")], ..Default::default() };
        // Both databases list both tracks.
        let dbs = Databases { pdb: Some(lib.clone()), plus: Some(lib), plus_error: None, images: Vec::new() };
        let found = missing(&d, &dbs, &FileTree::default(), &Ctx::silent()).unwrap();
        let said: Vec<(Problem, u32, &str)> = found.iter().map(|b| (b.problem, b.track.id, b.database)).collect();
        assert_eq!(
            said,
            [
                (Problem::MissingAudio, 1, "device_library"),
                (Problem::MissingArtwork, 1, "device_library"),
                (Problem::MissingAudio, 2, "device_library"),
                (Problem::MissingArtwork, 2, "device_library"),
            ]
        );
    }
}
