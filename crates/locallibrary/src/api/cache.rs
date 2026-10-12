//! Parsed libraries kept between requests, for a long-running caller (the sidecar).
//!
//! Reading a drive's library means parsing `export.pdb` (or decrypting OneLibrary), which takes
//! a while on a big drive; a GUI paging through tracks or showing one track after another would
//! pay it on every request. [`LibraryCache`] keeps the parsed library of the last few drives,
//! keyed by the drive's root and the [`LibraryStamp`] of its database files: as soon as any of
//! them is rewritten (an export, an edit, a sync, a player recording history), the next request
//! reads the library again. Callers that write a drive, or hear from a
//! [`crate::watch::VolumeWatcher`] that it changed, can also [`LibraryCache::invalidate`] it,
//! which covers file systems whose timestamps are too coarse to tell two quick writes apart.

use rustc_hash::FxHashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use super::resolve_drive;
use crate::device::Device;
use crate::mix::Identifier;
use crate::model::Library;
use crate::progress::Ctx;
use crate::tags::MyTags;
use crate::watch::LibraryStamp;
use crate::{Result, tags};

/// The libraries of the drives read last, most recently used last.
pub struct LibraryCache {
    capacity: usize,
    inner: Mutex<Inner>,
}

struct Inner {
    /// Bumped by every invalidation: a library read before one isn't kept.
    generation: u64,
    entries: Vec<Arc<CachedLibrary>>,
}

/// One drive's parsed library, and what's derived from it on demand, valid while the drive's
/// databases are as they were when it was read.
pub struct CachedLibrary {
    pub root: PathBuf,
    pub stamp: LibraryStamp,
    pub library: Library,
    tags: Mutex<Option<Arc<MyTags>>>,
    tags_by_track: Mutex<Option<Arc<TagsByTrack>>>,
    identifier: Mutex<IdentifierSlot>,
    /// Signalled when a build of the identifier ends.
    built: Condvar,
}

/// The mix identifier, by the store whose fingerprint cache built it.
#[derive(Default)]
enum IdentifierSlot {
    #[default]
    Empty,
    /// A request is building it: others wait for that rather than build it again.
    Building,
    Built(PathBuf, Arc<Identifier>),
}

/// Each track's My Tags (`Category/Tag`) by its file key, as [`tags::by_track`] gives them.
pub type TagsByTrack = FxHashMap<String, Vec<String>>;

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

impl CachedLibrary {
    /// Reads the drive's library now.
    pub fn read(device: &Device) -> Result<CachedLibrary> {
        // Stamped before reading: a write while it's read shows as a change next time.
        let stamp = LibraryStamp::of(device.root());
        Ok(CachedLibrary {
            root: device.root().to_path_buf(),
            stamp,
            library: device.library()?,
            tags: Mutex::default(),
            tags_by_track: Mutex::default(),
            identifier: Mutex::default(),
            built: Condvar::new(),
        })
    }

    /// The drive's My Tags ([`tags::read`]), read once.
    pub fn tags(&self, device: &Device) -> Result<Arc<MyTags>> {
        let mut slot = lock(&self.tags);
        if let Some(t) = &*slot {
            return Ok(t.clone());
        }
        let t = Arc::new(tags::read(device)?);
        *slot = Some(t.clone());
        Ok(t)
    }

    /// Each track's My Tags by file key ([`tags::by_track`]), read once.
    pub fn tags_by_track(&self, device: &Device) -> Result<Arc<TagsByTrack>> {
        let mut slot = lock(&self.tags_by_track);
        if let Some(t) = &*slot {
            return Ok(t.clone());
        }
        let t = Arc::new(tags::by_track(device)?);
        *slot = Some(t.clone());
        Ok(t)
    }

    /// The mix identifier built from this library with the fingerprint cache at `store_root`,
    /// built by `build` the first time. Building takes minutes on a big drive, so it's done
    /// outside the lock: a second request for it waits for the first (and can be cancelled while
    /// it does), then takes its result, or builds it when the first didn't finish.
    pub fn identifier(
        &self,
        store_root: &Path,
        ctx: &Ctx<'_>,
        build: impl FnOnce(Library) -> Result<Identifier>,
    ) -> Result<Arc<Identifier>> {
        let mut slot = lock(&self.identifier);
        loop {
            match &*slot {
                IdentifierSlot::Built(root, id) if root == store_root => return Ok(id.clone()),
                IdentifierSlot::Building => {
                    ctx.check()?;
                    // Woken when the build ends; the timeout only lets a cancel be seen.
                    slot = self
                        .built
                        .wait_timeout(slot, Duration::from_millis(200))
                        .unwrap_or_else(PoisonError::into_inner)
                        .0;
                }
                _ => break,
            }
        }
        *slot = IdentifierSlot::Building;
        drop(slot);
        // Empty again if the build fails or panics, so waiters don't wait for ever.
        struct Ended<'a>(&'a CachedLibrary);
        impl Drop for Ended<'_> {
            fn drop(&mut self) {
                let mut slot = lock(&self.0.identifier);
                if matches!(*slot, IdentifierSlot::Building) {
                    *slot = IdentifierSlot::Empty;
                }
                self.0.built.notify_all();
            }
        }
        let _ended = Ended(self);
        let id = Arc::new(build(self.library.clone())?);
        *lock(&self.identifier) = IdentifierSlot::Built(store_root.to_path_buf(), id.clone());
        Ok(id)
    }
}

impl LibraryCache {
    /// Keeps the libraries of the last `capacity` drives read.
    pub const fn new(capacity: usize) -> LibraryCache {
        LibraryCache { capacity, inner: Mutex::new(Inner { generation: 0, entries: Vec::new() }) }
    }

    /// Keeps nothing: every request reads the library again.
    pub fn none() -> &'static LibraryCache {
        static NONE: LibraryCache = LibraryCache::new(0);
        &NONE
    }

    /// The drive `spec` names ([`resolve_drive`]) and its library, kept from before when its
    /// databases haven't changed since.
    pub fn open(&self, spec: &str) -> Result<(Device, Arc<CachedLibrary>)> {
        let d = resolve_drive(spec)?;
        let c = self.get(&d)?;
        Ok((d, c))
    }

    /// The drive's library, kept from before when its databases haven't changed since.
    pub fn get(&self, device: &Device) -> Result<Arc<CachedLibrary>> {
        let root = device.root();
        let generation = {
            let mut inner = lock(&self.inner);
            if let Some(i) = inner.entries.iter().position(|e| e.root == root) {
                if inner.entries[i].stamp == LibraryStamp::of(root) {
                    let e = inner.entries.remove(i);
                    inner.entries.push(e.clone());
                    return Ok(e);
                }
                inner.entries.remove(i);
            }
            inner.generation
        };
        let read = Arc::new(CachedLibrary::read(device)?);
        let mut inner = lock(&self.inner);
        if self.capacity > 0 && inner.generation == generation {
            inner.entries.retain(|e| e.root != root);
            inner.entries.push(read.clone());
            let excess = inner.entries.len().saturating_sub(self.capacity);
            inner.entries.drain(..excess);
        }
        Ok(read)
    }

    /// Forgets the library of the drive at `root`.
    pub fn invalidate(&self, root: &Path) {
        let mut inner = lock(&self.inner);
        inner.generation += 1;
        inner.entries.retain(|e| e.root != root);
    }

    /// Forgets every library.
    pub fn clear(&self) {
        let mut inner = lock(&self.inner);
        inner.generation += 1;
        inner.entries.clear();
    }

    /// The roots of the drives whose libraries are kept, least recently used first.
    pub fn roots(&self) -> Vec<PathBuf> {
        lock(&self.inner).entries.iter().map(|e| e.root.clone()).collect()
    }
}

#[cfg(test)]
mod tests;
