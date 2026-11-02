//! Scanning the managed files of a drive into a [`FileTree`], with a persistent hash cache.
//!
//! Hashing hundreds of gigabytes of audio on every run would make the tool unusable, so hashes
//! are cached per path keyed by `(size, mtime)` (the same trick git's index uses). Library
//! database and analysis files are small and always hashed; audio files are hashed lazily, only
//! when a comparison or a store import actually needs their content identity.

use indexmap::IndexMap;

use rustc_hash::FxHashMap;
use std::path::{Path, PathBuf};

use rayon::prelude::*;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::error::IoContext;
use crate::fsutil;
use crate::hash::{self, Hash};
use crate::paths::{self, FileClass};
use crate::progress::{Ctx, Event, Phase};
use crate::{Error, Result};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct FileEntry {
    /// Normalised key (see [`paths::key`]).
    pub path: String,
    pub size: u64,
    /// Unix seconds.
    pub mtime: i64,
    pub class: FileClass,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hash: Option<Hash>,
}

impl FileEntry {
    /// Whether it's library data. Its path decides where it can: a snapshot taken by an earlier
    /// version may list what players keep for the drive as library data.
    pub fn is_library(&self) -> bool {
        paths::classify(&self.path).unwrap_or(self.class).is_library()
    }
}

/// The managed files below a drive root.
#[derive(Debug, Clone, Default)]
pub struct FileTree {
    pub root: PathBuf,
    /// Sorted by key.
    pub entries: IndexMap<String, FileEntry>,
    /// Actual on-disk relative path when it differs from the key (Unicode normalisation).
    actual: FxHashMap<String, PathBuf>,
    /// Lower-cased key -> key, for case-insensitive lookups. Sorted, so a prefix is a binary search.
    folded: IndexMap<String, String>,
}

impl FileTree {
    pub fn get(&self, key: &str) -> Option<&FileEntry> {
        self.entries.get(key)
    }

    /// Look up a key ignoring letter case. DJ drives are FAT32/exFAT/HFS+, where
    /// `Contents/[A]rtist/x.mp3` in the database and `Contents/[a]rtist/x.mp3` on disk are the
    /// same file.
    pub fn resolve(&self, key: &str) -> Option<&FileEntry> {
        self.entries.get(key).or_else(|| self.folded.get(&key.to_lowercase()).and_then(|k| self.entries.get(k)))
    }

    /// Entries whose key starts with `prefix`, ignoring case.
    pub fn with_prefix(&self, prefix: &str) -> Vec<&FileEntry> {
        let p = prefix.to_lowercase();
        let start = self.folded.partition_point(|f, _| *f < p);
        self.folded[start..]
            .iter()
            .take_while(|(f, _)| f.starts_with(&p))
            .filter_map(|(_, k)| self.entries.get(k))
            .collect()
    }

    /// Absolute filesystem path for a key in this tree.
    pub fn fs_path(&self, key: &str) -> PathBuf {
        match self.actual.get(key) {
            Some(rel) => self.root.join(rel),
            None => paths::to_fs(&self.root, key),
        }
    }

    pub fn total_bytes(&self) -> u64 {
        self.entries.values().map(|e| e.size).sum()
    }

    pub fn library_entries(&self) -> impl Iterator<Item = &FileEntry> {
        self.entries.values().filter(|e| e.class.is_library())
    }

    /// Fill in missing hashes for entries matching `want`, using and updating `index`.
    pub fn ensure_hashes(
        &mut self,
        index: &mut HashIndex,
        want: impl Fn(&FileEntry) -> bool,
        ctx: &Ctx<'_>,
    ) -> Result<()> {
        let todo: Vec<String> = self
            .entries
            .values_mut()
            .filter(|e| e.hash.is_none() && want(e))
            .filter_map(|e| match index.lookup(e) {
                Some(h) => {
                    e.hash = Some(h);
                    None
                }
                None => Some(e.path.clone()),
            })
            .collect();
        if todo.is_empty() {
            return Ok(());
        }
        let bytes: u64 = todo.iter().map(|k| self.entries[k].size).sum();
        ctx.phase_start(Phase::Hash, todo.len() as u64, bytes);
        for key in todo {
            ctx.check()?;
            let path = self.fs_path(&key);
            let entry = self.entries.get_mut(&key).expect("key from entries");
            ctx.emit(Event::Item { phase: Phase::Hash, path: key.clone(), bytes: entry.size });
            let h = hash::hash_file(&path, ctx, Phase::Hash)?;
            entry.hash = Some(h);
            index.record(entry);
        }
        ctx.phase_end(Phase::Hash);
        Ok(())
    }
}

/// Which files a scan should pick up.
#[derive(Debug, Clone, Default)]
pub struct ScanOptions {
    /// Extra keys to include even outside `Contents/` (audio referenced by the database at
    /// unusual locations).
    pub extra_keys: Vec<String>,
}

/// Walk `PIONEER/` and `Contents/` below `root` (plus `extra_keys`).
pub fn scan(root: &Path, opts: &ScanOptions, ctx: &Ctx<'_>) -> Result<FileTree> {
    let mut tree = FileTree { root: root.to_path_buf(), ..Default::default() };
    ctx.phase_start(Phase::Scan, 0, 0);
    for top in [paths::PIONEER_DIR, paths::CONTENTS_DIR] {
        let dir = root.join(top);
        if !dir.is_dir() {
            continue;
        }
        for (rel, class, meta) in walk(root, &dir, ctx)? {
            insert(&mut tree, &rel, class, &meta);
        }
    }
    add_extra(&mut tree, &opts.extra_keys)?;
    ctx.phase_end(Phase::Scan);
    Ok(tree)
}

/// A managed file a walk found: its path below the root, its class and its metadata.
type Found = (PathBuf, FileClass, std::fs::Metadata);

/// What a directory holds, in the order it lists them: a managed file (or the error that ends
/// the walk there), or its `n`th subdirectory.
enum Item {
    File(Result<Found>),
    Dir(usize),
}

/// The managed files below `top` (below `root`), depth first in the order each directory lists
/// its entries, OS clutter and symlinks left out, or the first error on the way: what a
/// [`walkdir::WalkDir`] of `top` gives in that order. Opening directories is most of a scan's
/// time, and a drive has one per analysed track, so each depth's directories are listed in
/// parallel. Neither step recurses, so however deep a drive's folders go, the walk's stack stays
/// the same.
fn walk(root: &Path, top: &Path, ctx: &Ctx<'_>) -> Result<Vec<Found>> {
    // Each directory's items, and where its subdirectories' own listings start.
    let mut listings: Vec<(std::vec::IntoIter<Item>, usize)> = Vec::new();
    let mut depth = vec![top.to_path_buf()];
    while !depth.is_empty() {
        let mut next = listings.len() + depth.len();
        let listed: Vec<(Vec<Item>, Vec<PathBuf>)> = depth.par_iter().map(|dir| list(root, top, dir, ctx)).collect();
        depth = Vec::new();
        for (items, subdirs) in listed {
            listings.push((items.into_iter(), next));
            next += subdirs.len();
            depth.extend(subdirs);
        }
    }
    let mut found = Vec::new();
    let mut open = vec![0];
    while let Some(&at) = open.last() {
        let (items, first) = &mut listings[at];
        match items.next() {
            Some(Item::File(file)) => found.push(file?),
            Some(Item::Dir(n)) => open.push(*first + n),
            None => {
                open.pop();
            }
        }
    }
    Ok(found)
}

/// The items `dir` (in `top`, below `root`) holds, and its subdirectories. Listing stops at an
/// error, which is the last item.
fn list(root: &Path, top: &Path, dir: &Path, ctx: &Ctx<'_>) -> (Vec<Item>, Vec<PathBuf>) {
    let io = |path: &Path, source| Error::Io { path: path.to_path_buf(), source };
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) => return (vec![Item::File(Err(io(dir, e)))], Vec::new()),
    };
    let (mut items, mut subdirs) = (Vec::new(), Vec::new());
    for entry in entries {
        if let Err(e) = ctx.check() {
            items.push(Item::File(Err(e.into())));
            break;
        }
        // An entry that can't be read is put down to the walk, as walkdir does.
        let entry = match entry {
            Ok(entry) => entry,
            Err(e) => {
                items.push(Item::File(Err(io(top, e))));
                break;
            }
        };
        if entry.file_name().to_str().is_some_and(paths::is_ignored_name) {
            continue;
        }
        let path = entry.path();
        match entry.file_type() {
            Ok(t) if t.is_dir() => {
                items.push(Item::Dir(subdirs.len()));
                subdirs.push(path);
            }
            Ok(t) if t.is_file() => {
                let rel = path.strip_prefix(root).expect("walk stays below root");
                if let Some(class) = paths::classify(&paths::key(&rel.to_string_lossy())) {
                    let found = std::fs::metadata(&path).at(&path).map(|meta| (rel.to_path_buf(), class, meta));
                    items.push(Item::File(found));
                }
            }
            Ok(_) => {}
            Err(e) => {
                items.push(Item::File(Err(io(&path, e))));
                break;
            }
        }
    }
    (items, subdirs)
}

/// Add the files at `extra_keys` (see [`ScanOptions::extra_keys`]) that `tree` lacks, as audio.
pub(crate) fn add_extra(tree: &mut FileTree, extra_keys: &[String]) -> Result<()> {
    for extra in extra_keys {
        let key = paths::key(extra);
        if tree.entries.contains_key(&key) {
            continue;
        }
        let p = paths::to_fs(&tree.root, &key);
        // Regular files only: never follow a symlink out of the drive.
        if std::fs::symlink_metadata(&p).is_ok_and(|m| m.file_type().is_file()) {
            let rel = p.strip_prefix(&tree.root).expect("joined below root").to_path_buf();
            let meta = std::fs::metadata(&p).at(&p)?;
            insert(tree, &rel, FileClass::Audio, &meta);
        }
    }
    tree.entries.sort_unstable_keys();
    tree.folded.sort_unstable_keys();
    Ok(())
}

fn insert(tree: &mut FileTree, rel: &Path, class: FileClass, meta: &std::fs::Metadata) {
    let rel_str = rel.to_string_lossy().replace('\\', "/");
    let key = paths::key(&rel_str);
    if key != rel_str {
        tree.actual.insert(key.clone(), rel.to_path_buf());
    }
    tree.folded.insert(key.to_lowercase(), key.clone());
    tree.entries.insert(
        key.clone(),
        FileEntry { path: key, size: meta.len(), mtime: fsutil::mtime_secs(meta), class, hash: None },
    );
}

/// Persistent `path -> (size, mtime, hash)` cache.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct HashIndex {
    #[serde(default)]
    entries: FxHashMap<String, IndexEntry>,
    /// Entries [`HashIndex::forget`] took out. One recorded again as it was goes back unchanged,
    /// so a rescan that finds nothing new leaves the file alone.
    #[serde(skip)]
    forgotten: FxHashMap<String, IndexEntry>,
    #[serde(skip)]
    dirty: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema)]
struct IndexEntry {
    size: u64,
    mtime: i64,
    hash: Hash,
    /// When the hash was recorded (unix seconds).
    #[serde(default)]
    at: i64,
}

impl IndexEntry {
    /// Hashed clearly after the file was last modified (see [`HashIndex::lookup`]).
    fn settled(&self) -> bool {
        fsutil::settled(self.mtime, self.at)
    }

    /// `other` answers lookups as this entry does: only when it was recorded differs.
    fn answers_as(&self, other: &IndexEntry) -> bool {
        (self.size, self.mtime, self.hash, self.settled()) == (other.size, other.mtime, other.hash, other.settled())
    }
}

impl HashIndex {
    pub fn load(path: &Path) -> HashIndex {
        // A broken cache is never fatal: it only costs re-hashing.
        fsutil::read_json_opt(path).ok().flatten().unwrap_or_default()
    }

    /// Write the cache (atomically, synced) if lookups would now answer differently from the
    /// last one saved or loaded. Recording a hash it holds already doesn't count.
    pub fn save(&self, path: &Path) -> Result<()> {
        if self.dirty || !self.forgotten.is_empty() { fsutil::write_json_atomic(path, self) } else { Ok(()) }
    }

    /// The cached hash, if the file's size and mtime are unchanged *and* the hash was recorded
    /// clearly after the file was last modified (see [`fsutil::settled`]). A file hashed in the
    /// same (FAT: 2 s) window it was written in could change again without its mtime moving
    /// ("racy" entries, as in git).
    pub fn lookup(&self, e: &FileEntry) -> Option<Hash> {
        self.entries.get(&e.path).filter(|i| i.size == e.size && i.mtime == e.mtime && i.settled()).map(|i| i.hash)
    }

    /// Whether `e` looks like the file recorded at its path, by size and mtime, but was recorded
    /// too soon after it was written for that to say it's unchanged (see [`fsutil::settled`]):
    /// a file copied moments after its source was written may have missed a rewrite of it.
    pub fn racy(&self, e: &FileEntry) -> bool {
        self.entries
            .get(&e.path)
            .is_some_and(|i| i.size == e.size && fsutil::mtime_eq(i.mtime, e.mtime) && !i.settled())
    }

    pub fn record(&mut self, e: &FileEntry) {
        let Some(hash) = e.hash else { return };
        let fresh = IndexEntry { size: e.size, mtime: e.mtime, hash, at: chrono::Utc::now().timestamp() };
        let old = self.forgotten.remove(&e.path).or_else(|| self.entries.get(&e.path).copied());
        let entry = match old {
            Some(old) if old.answers_as(&fresh) => old,
            _ => {
                self.dirty = true;
                fresh
            }
        };
        self.entries.insert(e.path.clone(), entry);
    }

    /// Stop trusting `key`'s entry: lookups miss it until it's recorded again.
    pub fn forget(&mut self, key: &str) {
        if let Some(old) = self.entries.remove(key) {
            self.forgotten.insert(key.to_string(), old);
        }
    }

    /// Drop cache entries for files that no longer exist.
    pub fn retain_keys(&mut self, tree: &FileTree) {
        let before = self.entries.len();
        self.entries.retain(|k, _| tree.entries.contains_key(k));
        self.dirty |= self.entries.len() != before;
    }
}

#[cfg(test)]
mod tests;
