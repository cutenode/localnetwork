//! What's worked out from tracks' audio, cached on the computer: fingerprints
//! ([`crate::mix::index`]) and tempo, key and loudness estimates ([`crate::sound`]).
//!
//! Decoding a library's audio takes minutes for a few thousand tracks, so each result is kept in
//! a small file per audio file, keyed by the file's name, size and modification time (the same
//! file on two drives is decoded once) and by the version of what made it (a new version's
//! results are new files; old ones are never read). A file that doesn't decode is remembered
//! too, so it isn't decoded again on every run.

use std::fs;
use std::marker::PhantomData;
use std::path::{Path, PathBuf};

use rustc_hash::FxHashMap;

use crate::device::Device;
use crate::model::{Track, TrackRef};
use crate::progress::{Ctx, Phase};
use crate::store::Store;
use crate::{Error, Result, fsutil, paths, probe};

/// The name of the store's folder the caches are in.
pub(crate) const CACHE_DIR: &str = "cache";

/// What one cache holds.
pub trait Kind {
    /// Its folder in the store's `cache/`.
    const DIR: &'static str;
    /// Its files' extension.
    const EXT: &'static str;
    /// One value, as warnings name it: `a fingerprint`.
    const WHAT: &'static str;
    /// The version of what makes the values, part of every key.
    const VERSION: u32;
    type Value: Send;
    fn encode(value: &Self::Value) -> Vec<u8>;
    /// `None` for bytes it can't read (a file cut short), which count as not cached.
    fn decode(bytes: &[u8]) -> Option<Self::Value>;
}

/// One kind of result on disk, one small file per audio file.
#[derive(Debug)]
pub struct Cache<K> {
    root: PathBuf,
    kind: PhantomData<fn() -> K>,
}

impl<K> Clone for Cache<K> {
    fn clone(&self) -> Self {
        Cache { root: self.root.clone(), kind: PhantomData }
    }
}

impl<K: Kind> Cache<K> {
    pub fn new(root: impl Into<PathBuf>) -> Cache<K> {
        Cache { root: root.into(), kind: PhantomData }
    }

    /// The store's cache of this kind, `<store>/cache/<kind>`.
    pub fn in_store(store: &Store) -> Cache<K> {
        Cache::new(store.root().join(CACHE_DIR).join(K::DIR))
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The cache of the store drive lookups consult ([`crate::api::set_store_root`]), or without
    /// one, a cache in a temporary folder that lasts as long as the folder returned with it.
    pub(crate) fn of_default_store() -> Result<(Cache<K>, Option<tempfile::TempDir>)> {
        if let Some(store) = crate::api::registry() {
            return Ok((Cache::in_store(&store), None));
        }
        let dir = tempfile::tempdir().map_err(|e| Error::Io { path: std::env::temp_dir(), source: e })?;
        Ok((Cache::new(dir.path()), Some(dir)))
    }

    /// Key for an audio file as it is now, or `None` if it can't be read.
    pub fn key(path: &Path) -> Option<String> {
        let meta = fs::metadata(path).ok()?;
        let mtime =
            meta.modified().ok().and_then(|m| m.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_secs());
        let name = path.file_name()?.to_string_lossy().to_lowercase();
        let mut h = blake3::Hasher::new();
        h.update(format!("{}\u{1f}{name}\u{1f}{}\u{1f}{mtime}", K::VERSION, meta.len()).as_bytes());
        Some(h.finalize().to_hex()[..32].to_string())
    }

    pub(crate) fn file(&self, key: &str, ext: &str) -> PathBuf {
        self.root.join(&key[..2]).join(format!("{key}.{ext}"))
    }

    pub fn get(&self, key: &str) -> Option<K::Value> {
        K::decode(&fs::read(self.file(key, K::EXT)).ok()?)
    }

    pub fn put(&self, key: &str, value: &K::Value) -> Result<()> {
        fsutil::write_deferred(&self.file(key, K::EXT), &K::encode(value))
    }

    /// Why the file couldn't be decoded, when this version of locallibrary tried (a newer one
    /// may decode it).
    pub fn failure(&self, key: &str) -> Option<String> {
        let text = fs::read_to_string(self.file(key, "err")).ok()?;
        let (version, error) = text.split_once('\n')?;
        (version == env!("CARGO_PKG_VERSION")).then(|| error.to_string())
    }

    pub fn put_failure(&self, key: &str, error: &str) -> Result<()> {
        fsutil::write_deferred(&self.file(key, "err"), format!("{}\n{error}", env!("CARGO_PKG_VERSION")).as_bytes())
    }

    /// What's cached for each of `tracks` whose audio is on the drive, by track id. Nothing is
    /// decoded.
    pub fn cached<'t>(&self, device: &Device, tracks: impl IntoIterator<Item = &'t Track>) -> FxHashMap<u32, K::Value> {
        tracks
            .into_iter()
            .filter_map(|t| {
                let file = device.checked_path(&paths::key(&t.file_path)).ok()?;
                Some((t.id, self.get(&Self::key(&file)?)?))
            })
            .collect()
    }
}

crate::schema::labelled_tuple! {
    /// A track whose audio couldn't be decoded, and why: `[track, error]`.
    #[derive(Debug, Clone)]
    pub struct TrackFailure {
        pub track: TrackRef,
        pub error: String,
    }
}

/// An audio file to work something out from.
pub struct Audio {
    /// The caller's id for it (a track id).
    pub id: u32,
    pub path: PathBuf,
    /// What progress calls it.
    pub name: String,
    /// Its size, for progress.
    pub bytes: u64,
}

/// A result, cached or worked out just now.
pub struct Done<V> {
    pub id: u32,
    /// Its file's key ([`Cache::key`]).
    pub key: String,
    pub value: V,
}

/// What [`for_files`] found.
pub struct Run<V, F = TrackFailure> {
    /// In the order the files came.
    pub done: Vec<Done<V>>,
    /// Worked out just now (the rest came from the cache).
    pub computed: usize,
    /// Files that couldn't be read ([`for_tracks`]: tracks whose audio is missing from the drive).
    pub missing: Vec<u32>,
    /// Files that couldn't be decoded, by id, and why.
    pub failed: Vec<F>,
}

/// Each file's result: from the cache where it has one, otherwise from `compute` on the file,
/// decoding several at once (one [`Event::Item`](crate::Event) in `phase` each) and caching each as
/// it finishes, so a cancelled run keeps what it did.
pub fn for_files<K: Kind>(
    files: &[Audio],
    cache: &Cache<K>,
    phase: Phase,
    compute: impl Fn(&Path, &Ctx<'_>) -> Result<K::Value> + Sync,
    ctx: &Ctx<'_>,
) -> Result<Run<K::Value, (u32, String)>> {
    let mut run = Run { done: Vec::new(), computed: 0, missing: Vec::new(), failed: Vec::new() };
    let mut todo = Vec::new();
    ctx.phase_start(Phase::Scan, files.len() as u64, 0);
    for f in files {
        ctx.check()?;
        let Some(key) = Cache::<K>::key(&f.path) else {
            run.missing.push(f.id);
            continue;
        };
        if let Some(value) = cache.get(&key) {
            run.done.push(Done { id: f.id, key, value });
        } else if let Some(error) = cache.failure(&key) {
            run.failed.push((f.id, error));
        } else {
            todo.push((f, key));
        }
    }
    ctx.phase_end(Phase::Scan);

    let results = probe::parallel(
        &todo,
        phase,
        |(f, _)| (f.name.clone(), f.bytes),
        |(f, key)| {
            // Guarded: a decoder panicking on one file is that file's error.
            let r = probe::guarded(&f.path, || compute(&f.path, ctx));
            let cached = match &r {
                Ok(v) => cache.put(key, v),
                // What a file holds doesn't change until the file does (and its key with it). One
                // that couldn't be read, or went away or changed while it was (a drive pulled
                // out), is tried again next time: a failed read can look like a file that doesn't
                // decode.
                Err(e)
                    if matches!(e.inner(), Error::Parse { .. }) && Cache::<K>::key(&f.path).as_ref() == Some(key) =>
                {
                    cache.put_failure(key, &e.to_string())
                }
                Err(_) => Ok(()),
            };
            if let Err(e) = cached {
                ctx.warn(format!("couldn't cache {}: {e}", K::WHAT));
            }
            r
        },
        ctx,
    )?;
    for ((f, key), r) in todo.into_iter().zip(results) {
        match r {
            Ok(value) => {
                run.done.push(Done { id: f.id, key, value });
                run.computed += 1;
            }
            Err(e) if matches!(e.inner(), Error::Cancelled) => return Err(e),
            Err(e) => run.failed.push((f.id, e.to_string())),
        }
    }
    // Back in the order the files came, whatever order the workers finished in.
    let order: FxHashMap<u32, usize> = files.iter().enumerate().map(|(i, f)| (f.id, i)).collect();
    run.done.sort_by_key(|d| order.get(&d.id).copied().unwrap_or(usize::MAX));
    Ok(run)
}

/// [`for_files`] on `tracks`' audio files on `device`, with failures by track.
pub fn for_tracks<K: Kind>(
    device: &Device,
    tracks: &[&Track],
    cache: &Cache<K>,
    phase: Phase,
    compute: impl Fn(&Path, &Ctx<'_>) -> Result<K::Value> + Sync,
    ctx: &Ctx<'_>,
) -> Result<Run<K::Value>> {
    let mut missing = Vec::new();
    let files: Vec<Audio> = tracks
        .iter()
        .filter_map(|t| match device.checked_path(&paths::key(&t.file_path)).ok().filter(|p| p.is_file()) {
            Some(path) => Some(Audio { id: t.id, path, name: t.file_path.clone(), bytes: t.file_size }),
            None => {
                missing.push(t.id);
                None
            }
        })
        .collect();
    let run = for_files(&files, cache, phase, compute, ctx)?;
    let by_id: FxHashMap<u32, &Track> = tracks.iter().map(|t| (t.id, *t)).collect();
    missing.extend(run.missing);
    Ok(Run {
        done: run.done,
        computed: run.computed,
        missing,
        failed: run.failed.into_iter().map(|(id, error)| TrackFailure { track: by_id[&id].into(), error }).collect(),
    })
}

#[cfg(test)]
mod tests;
