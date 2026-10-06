//! Scanning the managed files of a drive into a [`FileTree`], with a persistent hash cache.
//!
//! Hashing hundreds of gigabytes of audio on every run would make the tool unusable, so hashes
//! are cached per path keyed by `(size, mtime)` (the same trick git's index uses). Library
//! database and analysis files are small and always hashed; audio files are hashed lazily, only
//! when a comparison or a store import actually needs their content identity.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use walkdir::WalkDir;

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
    pub entries: BTreeMap<String, FileEntry>,
    /// Actual on-disk relative path when it differs from the key (Unicode normalisation).
    actual: HashMap<String, PathBuf>,
    /// Lower-cased key -> key, for case-insensitive lookups.
    folded: BTreeMap<String, String>,
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
        self.folded
            .range(p.clone()..)
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
        let walker = WalkDir::new(&dir)
            .follow_links(false)
            .into_iter()
            .filter_entry(|e| e.file_name().to_str().is_none_or(|n| !paths::is_ignored_name(n)));
        for entry in walker {
            ctx.check()?;
            let entry = entry.map_err(|e| Error::Io {
                path: e.path().map(Path::to_path_buf).unwrap_or_else(|| dir.clone()),
                source: e.into_io_error().unwrap_or_else(|| std::io::Error::other("walk error")),
            })?;
            if !entry.file_type().is_file() {
                continue;
            }
            let rel = entry.path().strip_prefix(root).expect("walk stays below root");
            add_entry(&mut tree, rel, entry.path())?;
        }
    }
    for extra in &opts.extra_keys {
        let key = paths::key(extra);
        if tree.entries.contains_key(&key) {
            continue;
        }
        let p = paths::to_fs(root, &key);
        // Regular files only: never follow a symlink out of the drive.
        if std::fs::symlink_metadata(&p).is_ok_and(|m| m.file_type().is_file()) {
            let rel = p.strip_prefix(root).expect("joined below root").to_path_buf();
            add_entry_as(&mut tree, &rel, &p, FileClass::Audio)?;
        }
    }
    ctx.phase_end(Phase::Scan);
    Ok(tree)
}

fn add_entry(tree: &mut FileTree, rel: &Path, abs: &Path) -> Result<()> {
    let rel_str = rel.to_string_lossy();
    let Some(class) = paths::classify(&paths::key(&rel_str)) else { return Ok(()) };
    add_entry_as(tree, rel, abs, class)
}

fn add_entry_as(tree: &mut FileTree, rel: &Path, abs: &Path, class: FileClass) -> Result<()> {
    let meta = std::fs::metadata(abs).at(abs)?;
    let rel_str = rel.to_string_lossy().replace('\\', "/");
    let key = paths::key(&rel_str);
    if key != rel_str {
        tree.actual.insert(key.clone(), rel.to_path_buf());
    }
    tree.folded.insert(key.to_lowercase(), key.clone());
    tree.entries.insert(
        key.clone(),
        FileEntry { path: key, size: meta.len(), mtime: fsutil::mtime_secs(&meta), class, hash: None },
    );
    Ok(())
}

/// Persistent `path -> (size, mtime, hash)` cache.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct HashIndex {
    #[serde(default)]
    entries: HashMap<String, IndexEntry>,
    #[serde(skip)]
    dirty: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
struct IndexEntry {
    size: u64,
    mtime: i64,
    hash: Hash,
    /// When the hash was recorded (unix seconds).
    #[serde(default)]
    at: i64,
}

/// mtime granularity to distrust (FAT32 rounds to 2 s).
const RACY_SECS: i64 = 2;

impl HashIndex {
    pub fn load(path: &Path) -> HashIndex {
        // A broken cache is never fatal: it only costs re-hashing.
        fsutil::read_json_opt(path).ok().flatten().unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        if self.dirty { fsutil::write_json_atomic(path, self) } else { Ok(()) }
    }

    /// The cached hash, if the file's size and mtime are unchanged *and* the hash was recorded
    /// clearly after the file was last modified. A file hashed in the same (FAT: 2 s) window it
    /// was written in could change again without its mtime moving ("racy" entries, as in git).
    pub fn lookup(&self, e: &FileEntry) -> Option<Hash> {
        self.entries
            .get(&e.path)
            .filter(|i| i.size == e.size && i.mtime == e.mtime && i.at > i.mtime + RACY_SECS)
            .map(|i| i.hash)
    }

    pub fn record(&mut self, e: &FileEntry) {
        if let Some(hash) = e.hash {
            let at = chrono::Utc::now().timestamp();
            self.entries.insert(e.path.clone(), IndexEntry { size: e.size, mtime: e.mtime, hash, at });
            self.dirty = true;
        }
    }

    pub fn forget(&mut self, key: &str) {
        if self.entries.remove(key).is_some() {
            self.dirty = true;
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
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn scan_classifies_and_ignores_clutter() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        for (p, body) in [
            ("PIONEER/rekordbox/export.pdb", "db"),
            ("PIONEER/MYSETTING.DAT", "s"),
            ("PIONEER/USBANLZ/P001/00000001/ANLZ0000.DAT", "a"),
            ("Contents/A/x.mp3", "audio"),
            ("Contents/A/._x.mp3", "appledouble"),
            ("Contents/.DS_Store", "x"),
            ("Music/other.mp3", "unmanaged"),
        ] {
            let path = root.join(p);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, body).unwrap();
        }
        let tree = scan(root, &ScanOptions::default(), &Ctx::silent()).unwrap();
        let keys: Vec<&str> = tree.entries.keys().map(String::as_str).collect();
        assert_eq!(
            keys,
            [
                "Contents/A/x.mp3",
                "PIONEER/MYSETTING.DAT",
                "PIONEER/USBANLZ/P001/00000001/ANLZ0000.DAT",
                "PIONEER/rekordbox/export.pdb"
            ]
        );
        assert_eq!(tree.get("PIONEER/MYSETTING.DAT").unwrap().class, FileClass::Settings);
    }

    #[test]
    fn hash_index_avoids_rehashing() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join("Contents")).unwrap();
        fs::write(root.join("Contents/a.mp3"), b"hello").unwrap();
        // Old enough that the cache entry isn't "racy".
        filetime::set_file_mtime(root.join("Contents/a.mp3"), filetime::FileTime::from_unix_time(1_600_000_000, 0))
            .unwrap();
        let mut index = HashIndex::default();
        let mut tree = scan(root, &ScanOptions::default(), &Ctx::silent()).unwrap();
        tree.ensure_hashes(&mut index, |_| true, &Ctx::silent()).unwrap();
        let h = tree.get("Contents/a.mp3").unwrap().hash.unwrap();
        assert_eq!(h, Hash::of(b"hello"));

        // A rescan reuses the cached hash; a size change invalidates it.
        let mut tree2 = scan(root, &ScanOptions::default(), &Ctx::silent()).unwrap();
        let entry = tree2.entries.get_mut("Contents/a.mp3").unwrap();
        assert_eq!(index.lookup(entry), Some(h));
        entry.size += 1;
        assert_eq!(index.lookup(entry), None);
    }
}
