//! Scrubbing a drive: re-read every library file and compare it with a known-good copy, then
//! rewrite only the files that are damaged.
//!
//! USB flash fails without warning, and a damaged file shows up during a set as a track that won't
//! load. A scrub reads every byte under `PIONEER/` and `Contents/` (bypassing the hash cache,
//! which would vouch for a damaged file by its size and modification time) and compares each
//! file with the best reference available for it:
//!
//! 1. a stored snapshot of exactly this library state (found by fingerprint, or named), which
//!    has a hash for every file, audio included;
//! 2. the drive's hash cache, when the file's size and modification time are what they were
//!    when locallibrary last hashed or wrote it (a changed file has a new mtime; a damaged one
//!    doesn't);
//! 3. any snapshot holding the same path with the same size and modification time.
//!
//! Players and rekordbox rewrite database and analysis files legitimately, but they also change
//! the modification time, so such files simply have no reference and count as unverified.
//! Files that can't be read at all are reported too. [`repair`] rewrites damaged and missing
//! files from store objects or another mounted drive, verifying every byte on the way, then
//! reads them back.

use std::collections::{BTreeSet, HashMap};
use std::path::PathBuf;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::commit::{Changes, Content, NewFile, Options};
use crate::device::{self, Device, LineageOp};
use crate::fsutil;
use crate::hash::{self, Hash};
use crate::model::{Library, TrackRef};
use crate::paths::{self, FileClass};
use crate::progress::{Ctx, Event, Phase};
use crate::schema::{Failure, Skip};
use crate::store::{Snapshot, Store};
use crate::tree::{FileEntry, FileTree, HashIndex};
use crate::{Error, Result};

/// Where a file's expected content came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ReferenceKind {
    /// A snapshot of exactly this library state.
    Snapshot,
    /// The drive's hash cache.
    Cache,
    /// An older snapshot holding the same version of the file.
    History,
}

#[derive(Debug, Clone, Copy)]
struct Expected {
    hash: Hash,
    size: u64,
    mtime: i64,
    kind: ReferenceKind,
}

/// Where a damaged or missing file can be rewritten from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "from", rename_all = "snake_case")]
pub enum RepairSource {
    Store,
    Drive { name: String, root: PathBuf },
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct FileProblem {
    pub path: String,
    pub class: FileClass,
    pub size: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expected: Option<Hash>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub actual: Option<Hash>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reference: Option<ReferenceKind>,
    /// The modification time the good copy had (restored on repair, so syncs and caches see the
    /// file they knew).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mtime: Option<i64>,
    /// Why it couldn't be read.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// `None`: no known-good copy is available.
    pub repair: Option<RepairSource>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct ScrubReport {
    pub drive: String,
    pub root: PathBuf,
    /// The snapshot of exactly this state, when there is one.
    pub snapshot: Option<Hash>,
    pub files: usize,
    pub bytes: u64,
    /// Files compared with a reference and found intact.
    pub verified: usize,
    /// Files read without error but with no reference (changed since locallibrary last saw
    /// them, or never seen).
    pub unverified: usize,
    pub damaged: Vec<FileProblem>,
    pub unreadable: Vec<FileProblem>,
    pub missing: Vec<FileProblem>,
    /// Tracks whose audio, analysis or artwork is affected.
    pub tracks: Vec<TrackRef>,
    pub playlists: Vec<String>,
}

impl ScrubReport {
    pub fn is_clean(&self) -> bool {
        self.damaged.is_empty() && self.unreadable.is_empty() && self.missing.is_empty()
    }

    pub fn problems(&self) -> impl Iterator<Item = &FileProblem> {
        self.damaged.iter().chain(&self.unreadable).chain(&self.missing)
    }

    /// One line for the drive's logbook.
    pub fn summary(&self) -> String {
        if self.is_clean() {
            format!("{} files intact ({} unverified)", self.verified, self.unverified)
        } else {
            format!(
                "{} damaged, {} unreadable, {} missing of {} files",
                self.damaged.len(),
                self.unreadable.len(),
                self.missing.len(),
                self.files
            )
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct ScrubOptions {
    /// Compare with this snapshot instead of finding one by fingerprint.
    pub snapshot: Option<Hash>,
}

/// Expected contents, per file key (folded to lower case).
struct References<'a> {
    exact: Option<HashMap<String, &'a FileEntry>>,
    history: Vec<&'a Snapshot>,
    index: HashIndex,
}

impl<'a> References<'a> {
    fn new(exact: Option<&'a Snapshot>, history: Vec<&'a Snapshot>, index: HashIndex) -> Self {
        References {
            exact: exact.map(|s| s.files.iter().map(|f| (f.path.to_lowercase(), f)).collect()),
            history,
            index,
        }
    }

    fn expected(&self, e: &FileEntry) -> Option<Expected> {
        let key = e.path.to_lowercase();
        if let Some(f) = self.exact.as_ref().and_then(|exact| exact.get(&key))
            && let Some(hash) = f.hash
        {
            return Some(Expected { hash, size: f.size, mtime: f.mtime, kind: ReferenceKind::Snapshot });
        }
        if let Some(hash) = self.index.lookup(e) {
            return Some(Expected { hash, size: e.size, mtime: e.mtime, kind: ReferenceKind::Cache });
        }
        for s in &self.history {
            if let Some(f) = s.files.iter().find(|f| f.path.to_lowercase() == key)
                && f.size == e.size
                && fsutil::mtime_eq(f.mtime, e.mtime)
                && let Some(hash) = f.hash
            {
                return Some(Expected { hash, size: f.size, mtime: f.mtime, kind: ReferenceKind::History });
            }
        }
        None
    }

    /// What a file that is gone should hold: the exact snapshot's copy, else the newest
    /// consulted snapshot listing that path.
    fn expected_missing(&self, key: &str) -> Option<Expected> {
        let key = key.to_lowercase();
        let from = |f: &FileEntry, kind| f.hash.map(|hash| Expected { hash, size: f.size, mtime: f.mtime, kind });
        if let Some(f) = self.exact.as_ref().and_then(|e| e.get(&key)) {
            return from(f, ReferenceKind::Snapshot);
        }
        self.history
            .iter()
            .find_map(|s| s.files.iter().find(|f| f.path.to_lowercase() == key))
            .and_then(|f| from(f, ReferenceKind::History))
    }

    /// Library files the exact snapshot has that `tree` lacks (by path: a snapshot an earlier
    /// version took lists what players keep for the drive as library files).
    fn missing_from(&self, tree: &FileTree) -> Vec<(&'a FileEntry, Expected)> {
        let Some(exact) = &self.exact else { return Vec::new() };
        let mut out: Vec<_> = exact
            .values()
            .filter(|f| f.is_library() && tree.resolve(&f.path).is_none())
            .filter_map(|f| {
                f.hash.map(|hash| (*f, Expected { hash, size: f.size, mtime: f.mtime, kind: ReferenceKind::Snapshot }))
            })
            .collect();
        out.sort_by(|a, b| a.0.path.cmp(&b.0.path));
        out
    }
}

/// Snapshots worth consulting for a drive: its library's head, and the snapshot its lineage
/// says it last held.
fn history_snapshots(store: &Store, device: &Device) -> Vec<Snapshot> {
    let mut ids = Vec::new();
    if let Some(state) = device.state() {
        if let Some(id) = state.library_id.and_then(|id| store.library_by_id(id).ok().flatten()).and_then(|l| l.head) {
            ids.push(id);
        }
        if let Some(id) = state.lineage.iter().rev().find_map(|e| e.snapshot) {
            ids.push(id);
        }
    }
    ids.dedup();
    ids.into_iter().filter_map(|id| store.load_snapshot(&id).ok()).collect()
}

/// A snapshot (in any library) whose fingerprint is `fp`.
fn snapshot_by_fingerprint(store: &Store, device: &Device, fp: &Hash) -> Result<Option<Snapshot>> {
    let mut libs = store.libraries()?;
    // The drive's own library first.
    let own = device.state().and_then(|s| s.library_id);
    libs.sort_by_key(|l| Some(l.id) != own);
    for lib in libs {
        if let Some(info) = store.find_by_fingerprint(&lib, fp)? {
            return Ok(Some(store.load_snapshot(&info.id)?));
        }
    }
    Ok(None)
}

/// Read every library file on `device` and compare it with what it should hold. `siblings` are
/// other mounted drives that may supply good copies.
pub fn scrub(
    device: &Device,
    store: Option<&Store>,
    siblings: &[Device],
    opts: &ScrubOptions,
    ctx: &Ctx<'_>,
) -> Result<ScrubReport> {
    let _lock = device.lock()?;
    let library = device.library().ok();
    let mut tree = device.scan(library.as_ref(), ctx)?;
    let keys: Vec<String> = tree.library_entries().map(|e| e.path.clone()).collect();
    let total: u64 = keys.iter().map(|k| tree.entries[k].size).sum();

    // 1. Read everything.
    let mut unreadable_errors: HashMap<String, String> = HashMap::new();
    ctx.phase_start(Phase::Verify, keys.len() as u64, total);
    for key in &keys {
        ctx.check()?;
        let path = tree.fs_path(key);
        let size = tree.entries[key].size;
        ctx.emit(Event::Item { phase: Phase::Verify, path: key.clone(), bytes: size });
        match hash::hash_file(&path, ctx, Phase::Verify) {
            Ok(h) => tree.entries.get_mut(key).expect("key from tree").hash = Some(h),
            Err(Error::Cancelled) => return Err(Error::Cancelled),
            Err(e) => {
                unreadable_errors.insert(key.clone(), e.to_string());
            }
        }
    }
    ctx.phase_end(Phase::Verify);

    // 2. Find the references.
    // The state's fingerprints, current and (for snapshots taken by earlier versions) legacy.
    let fps: Vec<Hash> = if unreadable_errors.is_empty() {
        device.fingerprints(&tree).map(|(a, b)| a.into_iter().chain(b).collect()).unwrap_or_default()
    } else {
        Vec::new()
    };
    let exact = match (store, opts.snapshot) {
        (Some(store), Some(id)) => Some(store.load_snapshot(&id)?),
        (Some(store), None) => {
            let mut found = None;
            for fp in &fps {
                found = snapshot_by_fingerprint(store, device, fp)?;
                if found.is_some() {
                    break;
                }
            }
            found
        }
        _ => None,
    };
    let history = store.map(|s| history_snapshots(s, device)).unwrap_or_default();
    let mut index = device.load_index();
    let refs = References::new(exact.as_ref(), history.iter().collect(), index.clone());

    // 3. Compare.
    let sources = RepairSources::new(store, siblings, ctx)?;
    let mut report = ScrubReport {
        drive: device.name(),
        root: device.root().to_path_buf(),
        snapshot: exact.as_ref().map(|s| s.id),
        files: keys.len(),
        bytes: total,
        verified: 0,
        unverified: 0,
        damaged: Vec::new(),
        unreadable: Vec::new(),
        missing: Vec::new(),
        tracks: Vec::new(),
        playlists: Vec::new(),
    };
    for key in &keys {
        let e = &tree.entries[key];
        let expected = refs.expected(e);
        let problem = |actual: Option<Hash>, error: Option<String>| FileProblem {
            path: e.path.clone(),
            class: e.class,
            size: e.size,
            expected: expected.map(|x| x.hash),
            actual,
            reference: expected.map(|x| x.kind),
            mtime: expected.map(|x| x.mtime),
            error,
            repair: expected.and_then(|x| sources.find(&e.path, &x)),
        };
        match (e.hash, unreadable_errors.remove(key)) {
            (_, Some(err)) => report.unreadable.push(problem(None, Some(err))),
            (Some(h), None) => match expected {
                Some(x) if x.hash != h => report.damaged.push(problem(Some(h), None)),
                Some(_) => {
                    report.verified += 1;
                    index.record(e);
                }
                None => {
                    report.unverified += 1;
                    // Now it's a reference for the next scrub.
                    index.record(e);
                }
            },
            (None, None) => unreachable!("every readable file was hashed"),
        }
    }
    for (f, x) in refs.missing_from(&tree) {
        report.missing.push(FileProblem {
            path: f.path.clone(),
            class: f.class,
            size: f.size,
            expected: Some(x.hash),
            actual: None,
            reference: Some(x.kind),
            mtime: Some(x.mtime),
            error: None,
            repair: sources.find(&f.path, &x),
        });
    }
    // Audio the database lists that isn't on the drive at all.
    if let Some(lib) = &library {
        let listed: BTreeSet<String> = report.missing.iter().map(|m| m.path.to_lowercase()).collect();
        for t in &lib.tracks {
            let key = paths::key(&t.file_path);
            if paths::is_safe_key(&key) && tree.resolve(&key).is_none() && !listed.contains(&key.to_lowercase()) {
                let x = refs.expected_missing(&key);
                report.missing.push(FileProblem {
                    size: x.map_or(t.file_size, |x| x.size),
                    class: FileClass::Audio,
                    expected: x.map(|x| x.hash),
                    actual: None,
                    reference: x.map(|x| x.kind),
                    mtime: x.map(|x| x.mtime),
                    error: None,
                    repair: x.and_then(|x| sources.find(&key, &x)),
                    path: key,
                });
            }
        }
    }
    if let Some(lib) = &library {
        let (tracks, playlists) = affected(lib, report.problems().map(|p| p.path.as_str()));
        report.tracks = tracks;
        report.playlists = playlists;
    }
    device.save_index(&index)?;
    Ok(report)
}

/// Tracks whose audio, analysis or artwork is among `keys`, and the playlists holding them.
pub fn affected<'k>(lib: &Library, keys: impl Iterator<Item = &'k str>) -> (Vec<TrackRef>, Vec<String>) {
    let keys: BTreeSet<String> = keys.map(str::to_lowercase).collect();
    let dir = |p: &str| p.rsplit_once('/').map(|(d, _)| d.to_string()).unwrap_or_default();
    let analysis_dirs: BTreeSet<String> =
        keys.iter().filter(|k| k.starts_with("pioneer/usbanlz/")).map(|k| dir(k)).collect();
    let mut ids = BTreeSet::new();
    let mut tracks = Vec::new();
    for t in &lib.tracks {
        let audio = paths::key(&t.file_path).to_lowercase();
        let anlz = paths::key(&t.analyze_path).to_lowercase();
        let art = t.artwork_path.as_deref().map(|p| paths::key(p).to_lowercase());
        let hit = keys.contains(&audio)
            || (!t.analyze_path.is_empty() && analysis_dirs.contains(&dir(&anlz)))
            || art.is_some_and(|a| keys.contains(&a));
        if hit {
            ids.insert(t.id);
            tracks.push(TrackRef { id: t.id, name: t.display_name(), file_path: t.file_path.clone() });
        }
    }
    let playlists = lib
        .playlists
        .iter()
        .filter(|p| !p.is_folder && p.track_ids.iter().any(|id| ids.contains(id)))
        .map(|p| lib.playlist_path(p.id))
        .collect();
    (tracks, playlists)
}

/// Known-good copies: store objects, and files on sibling drives with the right size (whose
/// content is checked while copying).
struct RepairSources<'a> {
    store: Option<&'a Store>,
    siblings: Vec<(&'a Device, FileTree)>,
}

impl<'a> RepairSources<'a> {
    fn new(store: Option<&'a Store>, siblings: &'a [Device], ctx: &Ctx<'_>) -> Result<Self> {
        let mut out = Vec::new();
        for d in siblings {
            let lib = d.library().ok();
            out.push((d, d.scan(lib.as_ref(), ctx)?));
        }
        Ok(RepairSources { store, siblings: out })
    }

    fn find(&self, key: &str, x: &Expected) -> Option<RepairSource> {
        if self.store.is_some_and(|s| s.has_object(&x.hash)) {
            return Some(RepairSource::Store);
        }
        self.siblings
            .iter()
            .find(|(_, tree)| tree.resolve(key).is_some_and(|e| e.size == x.size))
            .map(|(d, _)| RepairSource::Drive { name: d.name(), root: d.root().to_path_buf() })
    }
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct RepairReport {
    pub drive: String,
    pub repaired: Vec<String>,
    pub bytes: u64,
    /// Files that still differ after being rewritten: the flash itself is failing.
    pub failed: Vec<Failure>,
    /// Files that were fine by the time of the repair, or had no good copy.
    pub skipped: Vec<Skip>,
}

/// Rewrite the files a scrub found damaged or missing. Each file is checked again first (only
/// files still wrong are written), copied with every byte verified against the expected hash,
/// flushed, and read back.
pub fn repair(
    device: &mut Device,
    store: Option<&Store>,
    siblings: &[Device],
    problems: &[FileProblem],
    ctx: &Ctx<'_>,
) -> Result<RepairReport> {
    let _lock = crate::commit::lock(device, None)?;
    let mut report =
        RepairReport { drive: device.name(), repaired: Vec::new(), bytes: 0, failed: Vec::new(), skipped: Vec::new() };
    let library = device.library().ok();
    let tree = device.scan(library.as_ref(), ctx)?;
    let sibling_trees: Vec<(&Device, FileTree)> =
        siblings.iter().map(|d| Ok((d, d.scan(d.library().ok().as_ref(), ctx)?))).collect::<Result<_>>()?;
    let mut index = device.load_index();

    // What still needs writing, and from where.
    let mut todo: Vec<(&FileProblem, Hash, PathBuf)> = Vec::new();
    for p in problems {
        let Some(expected) = p.expected else {
            report.skipped.push(Skip::new(p.path.clone(), "no known-good copy"));
            continue;
        };
        if !paths::is_safe_key(&p.path) || paths::classify(&p.path).is_none_or(|c| !c.is_library()) {
            report.skipped.push(Skip::new(p.path.clone(), "not a library file"));
            continue;
        }
        if let Some(e) = tree.resolve(&p.path)
            && let Ok(h) = hash::hash_file(&tree.fs_path(&e.path), ctx, Phase::Verify)
            && h == expected
        {
            report.skipped.push(Skip::new(p.path.clone(), "already intact"));
            continue;
        }
        let src = match &p.repair {
            Some(RepairSource::Store) => store.filter(|s| s.has_object(&expected)).map(|s| s.object_path(&expected)),
            Some(RepairSource::Drive { root, .. }) => sibling_trees
                .iter()
                .find(|(d, _)| d.root() == root)
                .and_then(|(_, t)| t.resolve(&p.path).map(|e| t.fs_path(&e.path))),
            None => None,
        };
        match src {
            Some(src) => todo.push((p, expected, src)),
            None => report.skipped.push(Skip::new(p.path.clone(), "no known-good copy is available now")),
        }
    }
    if todo.is_empty() {
        return Ok(report);
    }

    // Each copy is verified against the expected hash as it's staged; one that can't be made
    // is reported and the others go in. There's no backup: what's written back is a good copy of
    // the same state.
    let mut expected: HashMap<String, (FileClass, Hash, u64)> = HashMap::new();
    let mut files = Vec::new();
    for (p, hash, src) in &todo {
        let dst_key = tree.resolve(&p.path).map(|e| e.path.clone()).unwrap_or_else(|| p.path.clone());
        let mtime = p.mtime.or_else(|| std::fs::metadata(src).ok().map(|m| fsutil::mtime_secs(&m)));
        expected.insert(dst_key.clone(), (p.class, *hash, p.size));
        files.push(NewFile {
            key: dst_key,
            content: Content::Copy { path: src.clone(), expected: Some(*hash) },
            mtime,
            size: p.size,
            optional: true,
        });
    }
    let mut opts = Options::new(LineageOp::Repair, format!("rewrite {} damaged file(s)", todo.len()));
    opts.lineage = false;
    let done = crate::commit::commit(device, Changes { files, ..Default::default() }, opts, ctx)?;
    for (key, e) in done.failed {
        let why = match e {
            Error::Corrupt { .. } => "the copy it would come from is damaged too".into(),
            e => e.to_string(),
        };
        report.failed.push(Failure::new(key, why));
    }

    // Read back what was written.
    for key in done.written {
        let (class, want, size) = expected[&key];
        let dst = device.checked_path(&key)?;
        match hash::hash_file(&dst, ctx, Phase::Verify) {
            Ok(h) if h == want => {
                let meta = std::fs::metadata(&dst).map_err(|e| Error::Io { path: dst.clone(), source: e })?;
                index.record(&FileEntry {
                    path: key.clone(),
                    size: meta.len(),
                    mtime: fsutil::mtime_secs(&meta),
                    class,
                    hash: Some(h),
                });
                report.repaired.push(key);
                report.bytes += size;
            }
            Ok(_) => report.failed.push(Failure::new(key, "reads back differently after being rewritten")),
            Err(e) => report.failed.push(Failure::new(key, e)),
        }
    }
    device.save_index(&index)?;
    Ok(report)
}

/// Whether a file a scrub flags would change the drive's fingerprint when repaired (database,
/// analysis and artwork files are fingerprinted by content).
pub fn changes_fingerprint(p: &FileProblem) -> bool {
    device::needs_content_hash(&FileEntry { path: p.path.clone(), size: p.size, mtime: 0, class: p.class, hash: None })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repairing_files_fingerprinted_by_content_changes_the_fingerprint() {
        let problem = |path: &str| FileProblem {
            path: path.into(),
            class: paths::classify(path).unwrap(),
            size: 10,
            expected: None,
            actual: None,
            reference: None,
            mtime: None,
            error: None,
            repair: None,
        };
        assert!(changes_fingerprint(&problem("PIONEER/rekordbox/export.pdb")));
        assert!(changes_fingerprint(&problem("PIONEER/USBANLZ/P000/00000001/ANLZ0000.DAT")));
        assert!(changes_fingerprint(&problem("PIONEER/Artwork/00001/a1.jpg")));
        // Audio goes by its size and path.
        assert!(!changes_fingerprint(&problem("Contents/a.mp3")));
    }
}
