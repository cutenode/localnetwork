//! Audio on this computer that a drive's audio is a copy of. A track on a drive usually came from
//! rekordbox's collection, so a pull often imports a file this computer already holds: when the
//! collection's file is on the store's volume and has the same content, the store's object is
//! cloned from it (see [`crate::fsutil::copy_into`]) and takes no space of its own.
//!
//! An object cloned this way shares its blocks with the collection's file until either is written,
//! so the store stops being a second physical copy of that track: a failing disk loses both. A
//! store copy elsewhere (`store copy`) is the backup.

use std::path::{Path, PathBuf};

use rustc_hash::FxHashMap;

use crate::audio_cache::{Cache, Kind};
use crate::hash::{self, Hash};
use crate::masterdb::{self, CloudFolders};
use crate::progress::{Ctx, NoProgress, Phase};
use crate::store::Store;
use crate::tree::FileEntry;
use crate::{Result, fsutil};

/// What's in an audio file, kept by the file's name, size and modification time so the
/// collection's audio is read once.
pub(crate) struct Contents;

impl Kind for Contents {
    const DIR: &'static str = "contents";
    const EXT: &'static str = "hash";
    const WHAT: &'static str = "a content hash";
    const VERSION: u32 = 1;
    type Value = Hash;

    fn encode(hash: &Hash) -> Vec<u8> {
        hash.to_hex().into_bytes()
    }

    fn decode(bytes: &[u8]) -> Option<Hash> {
        std::str::from_utf8(bytes).ok()?.parse().ok()
    }
}

/// The audio this computer holds that an object can be cloned from, by size: nothing else is
/// read to tell whether a file on a drive might be one of them.
pub(crate) struct Twins {
    by_size: FxHashMap<u64, Vec<PathBuf>>,
    contents: Cache<Contents>,
}

impl Twins {
    /// The audio of rekordbox's collection on this computer. None when it has no collection, or
    /// one that can't be read: a pull doesn't need it.
    pub(crate) fn of_collection(store: &Store) -> Twins {
        // A test's pulls stay away from this computer's collection unless it names one.
        let path = match cfg!(debug_assertions) {
            true => std::env::var_os(masterdb::PATH_ENV).map(PathBuf::from),
            false => masterdb::default_path(),
        };
        let collection =
            path.filter(|p| p.is_file()).and_then(|p| masterdb::open(&p, &masterdb::resolve_key(None)).ok());
        let Some(collection) = collection else { return Twins::among(store, Vec::new()) };
        let cloud = CloudFolders::this_computer();
        let files = collection.library.tracks.iter().filter_map(|t| collection.audio_file(t, &cloud)).collect();
        Twins::among(store, files)
    }

    /// Those of `files` an object can be cloned from: on the store's volume, and on this computer
    /// (reading an online-only file would download it, to save none of the space that takes).
    fn among(store: &Store, files: Vec<PathBuf>) -> Twins {
        let mut by_size: FxHashMap<u64, Vec<PathBuf>> = FxHashMap::default();
        for file in files {
            let Ok(meta) = std::fs::metadata(&file) else { continue };
            if meta.is_file() && !fsutil::is_online_only(&meta) && fsutil::same_volume(&file, store.root()) {
                by_size.entry(meta.len()).or_default().push(file);
            }
        }
        Twins { by_size, contents: Cache::in_store(store) }
    }

    /// The file holding what the drive's file at `path` (`entry`) does, with that hash, which
    /// reads the drive's file if there are any of its size and `entry` doesn't have it yet.
    pub(crate) fn of(&self, path: &Path, entry: &mut FileEntry, ctx: &Ctx<'_>) -> Result<Option<&Path>> {
        let Some(same_size) = self.by_size.get(&entry.size) else { return Ok(None) };
        // Not reported as progress: the copy that follows reports these bytes.
        let quiet = Ctx::new(&NoProgress, ctx.cancel.clone());
        let wanted = match entry.hash {
            Some(hash) => hash,
            None => *entry.hash.insert(hash::hash_file(path, &quiet, Phase::Hash)?),
        };
        for file in same_size {
            if self.content(file, &quiet)? == Some(wanted) {
                return Ok(Some(file));
            }
        }
        Ok(None)
    }

    /// What's in `file`, `None` if it can't be read any more.
    fn content(&self, file: &Path, ctx: &Ctx<'_>) -> Result<Option<Hash>> {
        let Some(key) = Cache::<Contents>::key(file) else { return Ok(None) };
        if let Some(hash) = self.contents.get(&key) {
            return Ok(Some(hash));
        }
        let hash = hash::hash_file(file, ctx, Phase::Hash)?;
        self.contents.put(&key, &hash)?;
        Ok(Some(hash))
    }
}

#[cfg(test)]
mod tests;
