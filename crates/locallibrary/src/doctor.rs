//! A health report for one drive: duplicate tracks, missing and broken files, files nothing
//! references any more, tracks in no playlist, drift between the two databases, files FAT32 can't
//! hold, and how players get on with the drive's file system. On request ([`Section::Audio`]),
//! also audio files that are cut short or don't decode (see [`crate::probe::check_tracks`]).
//!
//! The report never writes to the drive (with `hash_audio`, the hash cache is kept on adopted
//! drives, as every scan does). [`plan_orphans`] / [`apply_orphans`] remove orphaned files: audio
//! under `Contents/`, analysis under `PIONEER/USBANLZ/` and images under `PIONEER/Artwork/` that no
//! track in either database references. They are backed up first (see [`crate::backup`]), so
//! `locallibrary undo` takes it back.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::commit::{self, Changes, Options};
use crate::device::{Device, LibraryDrift, LineageOp};
use crate::error::Reason;
use crate::format::{self, Compatibility, Disk, Volume, players};
use crate::fsutil;
use crate::hash::Hash;
use crate::model::{Library, Track, TrackRef, normalize};
use crate::paths::{self, FileClass};
use crate::pdb;
use crate::progress::Ctx;
use crate::store::{Snapshot, Store};
use crate::tree::FileTree;
use crate::{Error, Result};

/// FAT32 stores file sizes in 32 bits: a file of 4 GiB or more can't be stored on it (or copied
/// to it).
pub const FAT32_MAX_FILE: u64 = 1 << 32;

/// Extensions of the audio files rekordbox exports. Anything else under `Contents/` (a
/// `cover.jpg`, notes) is never reported as orphaned.
const AUDIO_EXTENSIONS: &[&str] = &["mp3", "m4a", "mp4", "aac", "wav", "aif", "aiff", "flac", "ogg", "alac"];

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
}

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct DoctorOptions {
    /// Also find duplicates by audio content. Only files whose size matches another track's are
    /// hashed, through the drive's hash cache.
    #[serde(default)]
    pub hash_audio: bool,
    /// Sections to run (all but [`Section::Audio`] when empty).
    #[serde(default)]
    pub only: Vec<Section>,
    /// Also run [`Section::Audio`].
    #[serde(default)]
    pub audio: bool,
    /// Duplicates only by file, audio or Sync Manager copy: leave out tracks that only share
    /// artist, title and length (they may be different edits), also from groups the others join.
    #[serde(default)]
    pub sure_only: bool,
}

impl DoctorOptions {
    fn wants(&self, s: Section) -> bool {
        match s {
            Section::Audio => self.audio || self.only.contains(&s),
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
    /// Id of the suggested keeper: most playlist memberships, then the higher bitrate, then the
    /// earliest date added.
    pub keep: u32,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct DuplicateTrack {
    pub track: TrackRef,
    /// Playlists the track is in.
    pub playlists: usize,
    pub play_count: u16,
    pub bitrate: u32,
    pub file_size: u64,
    pub date_added: String,
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
    /// The audio file's size differs from what the database recorded (truncated or replaced).
    SizeMismatch,
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
        let object = |key: &str| -> Result<Option<(PathBuf, Vec<u8>)>> {
            let Some(entry) = snapshot.files.iter().find(|f| f.path == key) else { return Ok(None) };
            let hash = entry.hash.ok_or_else(|| Error::invalid(format!("{} has no hash", entry.path)))?;
            let path = store.object_path(&hash);
            let bytes = std::fs::read(&path).map_err(|e| Error::Io { path: path.clone(), source: e })?;
            Ok(Some((path, bytes)))
        };
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

fn fold(s: &str) -> String {
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
        true => Some(duplicates(device, library, &mut tree, opts, ctx)?),
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
    let volume = opts.wants(Section::Compatibility).then(|| volume_of(device.root())).flatten();
    Ok(DoctorReport {
        drive: device.name(),
        root: device.root().to_path_buf(),
        tracks: library.tracks.len(),
        duplicates,
        missing: opts.wants(Section::Missing).then(|| missing(device, &dbs, &tree)),
        orphans,
        unfiled: opts.wants(Section::Unfiled).then(|| unfiled(library)),
        drift: opts.wants(Section::Drift).then(|| dbs.drift()).flatten(),
        large_files: opts.wants(Section::LargeFiles).then(|| large_files(library, &tree)),
        audio,
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
    let filed: HashSet<u32> =
        library.playlists.iter().filter(|p| !p.is_folder).flat_map(|p| p.track_ids.iter().copied()).collect();
    library.tracks.iter().filter(|t| !filed.contains(&t.id)).map(TrackRef::from).collect()
}

/// Library files of 4 GiB or more, which FAT32 can't hold.
pub fn large_files(library: &Library, tree: &FileTree) -> Vec<LargeFile> {
    let by_key: HashMap<String, &Track> = library.tracks.iter().map(|t| (fold(&paths::key(&t.file_path)), t)).collect();
    tree.library_entries()
        .filter(|e| e.size >= FAT32_MAX_FILE)
        .map(|e| LargeFile {
            path: e.path.clone(),
            size: e.size,
            track: by_key.get(&fold(&e.path)).map(|t| TrackRef::from(*t)),
        })
        .collect()
}

/// Whether a file's size on the drive matches the database's. `export.pdb` stores sizes in 32
/// bits, so a file of 4 GiB or more is recorded modulo 2^32.
fn size_matches(on_disk: u64, recorded: u64) -> bool {
    on_disk == recorded || (recorded < FAT32_MAX_FILE && on_disk % FAT32_MAX_FILE == recorded)
}

/// Tracks whose audio, analysis or artwork is missing, or whose audio isn't the size the
/// database recorded. A file both databases reference is reported once.
pub(crate) fn missing(device: &Device, dbs: &Databases, tree: &FileTree) -> Vec<BrokenTrack> {
    let exists = |key: &str| {
        tree.resolve(key).is_some()
            || (paths::classify(key).is_none() && device.checked_path(key).is_ok_and(|p| p.is_file()))
    };
    let size = |key: &str| {
        tree.resolve(key)
            .map(|e| e.size)
            .or_else(|| device.checked_path(key).ok().and_then(|p| std::fs::metadata(p).ok()).map(|m| m.len()))
    };
    let mut out = Vec::new();
    let mut seen: HashSet<(Problem, String)> = HashSet::new();
    for (database, library) in dbs.all() {
        for t in &library.tracks {
            let mut report = |problem: Problem, path: String, detail: Option<String>| {
                if seen.insert((problem, fold(&path))) {
                    out.push(BrokenTrack { track: t.into(), problem, path, detail, database });
                }
            };
            let audio = paths::key(&t.file_path);
            if !paths::is_safe_key(&audio) {
                report(
                    Problem::UnsafePath,
                    audio,
                    Some(format!("the database's path {:?} leaves the drive", t.file_path)),
                );
            } else if !exists(&audio) {
                report(Problem::MissingAudio, audio, None);
            } else if let Some(n) = size(&audio).filter(|&n| t.file_size > 0 && !size_matches(n, t.file_size)) {
                let detail = format!("{n} bytes on the drive, {} in the database", t.file_size);
                report(Problem::SizeMismatch, audio, Some(detail));
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
    out
}

/// Audio under `Contents/`, analysis under `PIONEER/USBANLZ/` and images under
/// `PIONEER/Artwork/` that nothing in either database references. Analysis files go by their
/// stem (a track's `.DAT` keeps its `.EXT` and `.2EX`), artwork with its `_m` variant.
fn orphans(dbs: &Databases, tree: &FileTree) -> Vec<OrphanFile> {
    fn use_image(key: &str, in_use: &mut HashSet<String>) {
        in_use.insert(fold(key));
        in_use.insert(fold(&crate::artwork::medium_variant(key)));
    }
    let mut in_use: HashSet<String> = HashSet::new();
    let mut analysis_stems: HashSet<String> = HashSet::new();
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

/// Groups of tracks that are the same recording, each with a suggested keeper.
fn duplicates(
    device: &Device,
    library: &Library,
    tree: &mut FileTree,
    opts: &DoctorOptions,
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
    let mut link = |groups: BTreeMap<_, Vec<usize>>, reason: DuplicateReason, distinct_files: bool| {
        for members in groups.into_values() {
            for pair in members.windows(2) {
                if !distinct_files || keys[pair[0]] != keys[pair[1]] {
                    edges.push((pair[0], pair[1], reason));
                }
            }
        }
    };
    let group_by = |f: &dyn Fn(usize) -> Option<String>| {
        let mut groups: BTreeMap<String, Vec<usize>> = BTreeMap::new();
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
        let mut by_size: HashMap<u64, HashSet<&str>> = HashMap::new();
        for k in files.iter().flatten() {
            if let Some(e) = tree.get(k) {
                by_size.entry(e.size).or_default().insert(k);
            }
        }
        let candidates: HashSet<String> =
            by_size.into_values().filter(|s| s.len() > 1).flatten().map(str::to_string).collect();
        let mut index = device.load_index();
        tree.ensure_hashes(&mut index, |e| candidates.contains(&e.path), ctx)?;
        device.save_index(&index)?;
        let hashes: Vec<Option<Hash>> =
            files.iter().map(|k| k.as_ref().and_then(|k| tree.get(k)).and_then(|e| e.hash)).collect();
        link(group_by(&|i| hashes[i].map(|h| h.to_hex())), DuplicateReason::SameAudio, true);
    }

    let mut uf = UnionFind((0..tracks.len()).collect());
    for (a, b, _) in &edges {
        uf.union(*a, *b);
    }
    let mut groups: BTreeMap<usize, (Vec<usize>, Vec<DuplicateReason>)> = BTreeMap::new();
    for i in 0..tracks.len() {
        groups.entry(uf.find(i)).or_default().0.push(i);
    }
    for (a, _, reason) in &edges {
        let reasons = &mut groups.get_mut(&uf.find(*a)).expect("every track has a group").1;
        if !reasons.contains(reason) {
            reasons.push(*reason);
        }
    }

    let mut memberships: HashMap<u32, usize> = HashMap::new();
    for p in library.playlists.iter().filter(|p| !p.is_folder) {
        for id in p.track_ids.iter().collect::<HashSet<_>>() {
            *memberships.entry(*id).or_default() += 1;
        }
    }
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
                        play_count: t.play_count,
                        bitrate: t.bitrate,
                        file_size: sizes[i],
                        date_added: t.date_added.clone(),
                    }
                })
                .collect();
            // Keeper first: most playlists, then higher bitrate, then earliest date added (tracks
            // without a date last).
            tracks.sort_by(|a, b| {
                b.playlists
                    .cmp(&a.playlists)
                    .then(b.bitrate.cmp(&a.bitrate))
                    .then_with(|| match (a.date_added.is_empty(), b.date_added.is_empty()) {
                        (false, true) => std::cmp::Ordering::Less,
                        (true, false) => std::cmp::Ordering::Greater,
                        _ => a.date_added.cmp(&b.date_added),
                    })
                    .then(a.track.id.cmp(&b.track.id))
            });
            DuplicateGroup { reasons, keep: tracks[0].track.id, tracks }
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
    let still: HashSet<String> = find_orphans(device, ctx)?.iter().map(|f| fold(&f.path)).collect();
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

    #[test]
    fn sizes_wrap_at_4_gib_in_export_pdb() {
        assert!(size_matches(10, 10));
        assert!(!size_matches(10, 11));
        assert!(size_matches(FAT32_MAX_FILE + 5, 5));
        assert!(size_matches(FAT32_MAX_FILE + 5, FAT32_MAX_FILE + 5));
        assert!(!size_matches(5, FAT32_MAX_FILE + 5));
    }
}
