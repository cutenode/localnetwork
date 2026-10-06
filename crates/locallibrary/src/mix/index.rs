//! Fingerprints of a library's tracks, cached on the computer, and the hash index built from them.
//!
//! Fingerprinting decodes every track once, which takes minutes for a few thousand tracks. The
//! peaks are cached per audio file (keyed by name, size and modification time, so the same file
//! on two drives is fingerprinted once) and the index is rebuilt from the cache in seconds. A
//! file that doesn't decode is remembered too, so it isn't decoded again on every run.

use std::fs;
use std::path::{Path, PathBuf};

use schemars::JsonSchema;
use serde::Serialize;

use super::audio;
use super::fingerprint::{self, Analyzer, HASH_BITS, LIBRARY, Peak};
use crate::device::Device;
use crate::model::{Library, TrackRef};
use crate::progress::{Ctx, Phase, WarningLimiter};
use crate::store::Store;
use crate::{Error, Result, fsutil, paths, probe};

/// Landmarks per library peak.
const LIBRARY_FANOUT: usize = 3;

/// Library tracks longer than this (seconds) aren't indexed: they're recorded sets or
/// continuous-mix albums, which match a whole mix and swallow the tracks heard in it.
pub const LONGEST_TRACK_SECS: u32 = 20 * 60;

/// A track's fingerprint.
#[derive(Debug, Clone, PartialEq)]
pub struct TrackPrint {
    pub duration_secs: f32,
    pub peaks: Vec<Peak>,
}

const MAGIC: &[u8; 4] = b"LLFP";
/// Bump when anything that changes peaks changes.
const VERSION: u16 = 1;

impl TrackPrint {
    /// `LLFP`, version, duration, peak count, then per peak a varint frame delta and the bin.
    fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(16 + self.peaks.len() * 2);
        out.extend_from_slice(MAGIC);
        out.extend_from_slice(&VERSION.to_le_bytes());
        out.extend_from_slice(&self.duration_secs.to_le_bytes());
        out.extend_from_slice(&(self.peaks.len() as u32).to_le_bytes());
        let mut last = 0u32;
        for p in &self.peaks {
            let mut d = p.t - last;
            last = p.t;
            loop {
                let byte = (d & 0x7f) as u8;
                d >>= 7;
                if d == 0 {
                    out.push(byte);
                    break;
                }
                out.push(byte | 0x80);
            }
            out.push(p.f);
        }
        out
    }

    fn decode(bytes: &[u8]) -> Option<TrackPrint> {
        if bytes.get(..4)? != MAGIC || u16::from_le_bytes(bytes.get(4..6)?.try_into().ok()?) != VERSION {
            return None;
        }
        let duration_secs = f32::from_le_bytes(bytes.get(6..10)?.try_into().ok()?);
        let n = u32::from_le_bytes(bytes.get(10..14)?.try_into().ok()?) as usize;
        let mut peaks = Vec::with_capacity(n.min(bytes.len()));
        let mut i = 14;
        let mut t = 0u32;
        for _ in 0..n {
            let mut d = 0u32;
            let mut shift = 0;
            loop {
                let b = *bytes.get(i)?;
                i += 1;
                d |= ((b & 0x7f) as u32).checked_shl(shift)?;
                if b & 0x80 == 0 {
                    break;
                }
                shift += 7;
            }
            t = t.checked_add(d)?;
            peaks.push(Peak { t, f: *bytes.get(i)? });
            i += 1;
        }
        (i == bytes.len()).then_some(TrackPrint { duration_secs, peaks })
    }
}

/// Decode an audio file and fingerprint it.
pub fn fingerprint_file(path: &Path, ctx: &Ctx<'_>) -> Result<TrackPrint> {
    let mut a = Analyzer::new(LIBRARY, false);
    let format = audio::decode(path, fingerprint::RATE, ctx, &mut |s| a.push(s))?;
    let (peaks, _) = a.finish();
    Ok(TrackPrint { duration_secs: format.duration_secs as f32, peaks })
}

/// The name of the store's folder the fingerprint cache is in.
pub(crate) const CACHE_DIR: &str = "cache";

/// Fingerprints on disk, one small file per audio file.
#[derive(Debug, Clone)]
pub struct Cache {
    root: PathBuf,
}

impl Cache {
    pub fn new(root: impl Into<PathBuf>) -> Cache {
        Cache { root: root.into() }
    }

    /// The store's fingerprint cache, `<store>/cache/fingerprints`.
    pub fn in_store(store: &Store) -> Cache {
        Cache::new(store.root().join(CACHE_DIR).join("fingerprints"))
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Key for an audio file as it is now, or `None` if it can't be read.
    pub fn key(path: &Path) -> Option<String> {
        let meta = fs::metadata(path).ok()?;
        let mtime =
            meta.modified().ok().and_then(|m| m.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_secs());
        let name = path.file_name()?.to_string_lossy().to_lowercase();
        let mut h = blake3::Hasher::new();
        h.update(format!("{VERSION}\u{1f}{name}\u{1f}{}\u{1f}{mtime}", meta.len()).as_bytes());
        Some(h.finalize().to_hex()[..32].to_string())
    }

    fn file(&self, key: &str, ext: &str) -> PathBuf {
        self.root.join(&key[..2]).join(format!("{key}.{ext}"))
    }

    pub fn get(&self, key: &str) -> Option<TrackPrint> {
        TrackPrint::decode(&fs::read(self.file(key, "fp")).ok()?)
    }

    pub fn put(&self, key: &str, print: &TrackPrint) -> Result<()> {
        fsutil::write_deferred(&self.file(key, "fp"), &print.encode())
    }

    /// Why the file couldn't be fingerprinted, when this version of locallibrary tried (a newer
    /// one may decode it).
    pub fn failure(&self, key: &str) -> Option<String> {
        let text = fs::read_to_string(self.file(key, "err")).ok()?;
        let (version, error) = text.split_once('\n')?;
        (version == env!("CARGO_PKG_VERSION")).then(|| error.to_string())
    }

    pub fn put_failure(&self, key: &str, error: &str) -> Result<()> {
        fsutil::write_deferred(&self.file(key, "err"), format!("{}\n{error}", env!("CARGO_PKG_VERSION")).as_bytes())
    }
}

/// Inverted index from landmark hash to (track, frame) occurrences.
pub struct LibraryIndex {
    /// Library track id of each indexed track.
    pub track_ids: Vec<u32>,
    pub durations: Vec<f32>,
    offsets: Vec<u32>,
    /// `(slot << 32) | frame`, grouped by hash.
    entries: Vec<u64>,
}

impl LibraryIndex {
    pub fn bucket(&self, hash: u32) -> &[u64] {
        let h = hash as usize;
        &self.entries[self.offsets[h] as usize..self.offsets[h + 1] as usize]
    }

    pub fn hashes(&self) -> usize {
        self.entries.len()
    }

    /// Build from fingerprints. Hashes so common they say nothing (a bare kick pattern shared by
    /// thousands of tracks) are dropped. Each track's landmarks are worked out twice, to count
    /// and then to fill: keeping them all between the two would double the memory it takes.
    pub fn build(prints: Vec<(u32, TrackPrint)>) -> LibraryIndex {
        let buckets = 1usize << HASH_BITS;
        let mut counts = vec![0u32; buckets + 1];
        for (_, p) in &prints {
            for lm in fingerprint::landmarks(&p.peaks, LIBRARY_FANOUT) {
                counts[lm.hash() as usize] += 1;
            }
        }
        let total: u64 = counts.iter().map(|&c| c as u64).sum();
        let cap = (total as f64 / buckets as f64 * 64.0).max(512.0) as u32;
        let mut offsets = vec![0u32; buckets + 1];
        let mut sum = 0u32;
        for h in 0..buckets {
            offsets[h] = sum;
            if counts[h] <= cap {
                sum += counts[h];
            }
        }
        offsets[buckets] = sum;
        let mut fill = offsets.clone();
        let mut entries = vec![0u64; sum as usize];
        for (slot, (_, p)) in prints.iter().enumerate() {
            for lm in fingerprint::landmarks(&p.peaks, LIBRARY_FANOUT) {
                let h = lm.hash() as usize;
                if counts[h] <= cap {
                    entries[fill[h] as usize] = ((slot as u64) << 32) | lm.t as u64;
                    fill[h] += 1;
                }
            }
        }
        let (track_ids, durations) = prints.iter().map(|(id, p)| (*id, p.duration_secs)).unzip();
        LibraryIndex { track_ids, durations, offsets, entries }
    }
}

#[derive(Debug, Clone, Default, Serialize, serde::Deserialize, JsonSchema)]
pub struct IndexReport {
    /// Tracks in the library.
    pub tracks: usize,
    /// Tracks in the index.
    pub indexed: usize,
    /// Fingerprinted just now (the rest came from the cache).
    pub fingerprinted: usize,
    /// Audio files missing from the drive.
    pub missing: Vec<TrackRef>,
    /// Audio files that couldn't be decoded.
    pub failed: Vec<TrackFailure>,
    /// Tracks over 20 minutes ([`LONGEST_TRACK_SECS`]), left out: recorded sets, not tracks.
    #[serde(default)]
    pub long: Vec<TrackRef>,
    pub hashes: usize,
}

crate::schema::labelled_tuple! {
    /// A track whose audio couldn't be fingerprinted, and why: `[track, error]`.
    #[derive(Debug, Clone)]
    pub struct TrackFailure {
        pub track: TrackRef,
        pub error: String,
    }
}

/// A library track's fingerprint.
pub struct Fingerprinted {
    pub id: u32,
    /// Its file's key ([`Cache::key`]).
    pub key: String,
    pub print: TrackPrint,
}

/// Fingerprint every track of `library` (from the cache where possible), in library order.
pub fn fingerprint_library(
    device: &Device,
    library: &Library,
    cache: &Cache,
    ctx: &Ctx<'_>,
) -> Result<(Vec<Fingerprinted>, IndexReport)> {
    let mut report = IndexReport { tracks: library.tracks.len(), ..Default::default() };
    let mut prints = Vec::with_capacity(library.tracks.len());
    let mut todo = Vec::new();
    ctx.phase_start(Phase::Scan, library.tracks.len() as u64, 0);
    for t in &library.tracks {
        ctx.check()?;
        if t.duration_secs > LONGEST_TRACK_SECS {
            report.long.push(t.into());
            continue;
        }
        let file = device.checked_path(&paths::key(&t.file_path)).ok().filter(|p| p.is_file());
        let Some((file, key)) = file.and_then(|f| Cache::key(&f).map(|k| (f, k))) else {
            report.missing.push(t.into());
            continue;
        };
        if let Some(p) = cache.get(&key) {
            prints.push(Fingerprinted { id: t.id, key, print: p });
        } else if let Some(error) = cache.failure(&key) {
            report.failed.push(TrackFailure { track: t.into(), error });
        } else {
            todo.push((t, file, key));
        }
    }
    ctx.phase_end(Phase::Scan);

    // Cached as each finishes, so a cancelled run keeps what it did.
    let results = probe::parallel(
        &todo,
        Phase::Fingerprint,
        |(t, ..)| (t.file_path.clone(), t.file_size),
        |(_, file, key)| {
            let r = fingerprint_file(file, ctx);
            let cached = match &r {
                Ok(p) => cache.put(key, p),
                // What a file holds doesn't change until the file does (and its key with it). One
                // that couldn't be read, or went away or changed while it was (a drive pulled
                // out), is tried again next time: a failed read can look like a file that doesn't
                // decode.
                Err(e) if matches!(e.inner(), Error::Parse { .. }) && Cache::key(file).as_ref() == Some(key) => {
                    cache.put_failure(key, &e.to_string())
                }
                Err(_) => Ok(()),
            };
            if let Err(e) = cached {
                ctx.warn(format!("couldn't cache a fingerprint: {e}"));
            }
            r
        },
        ctx,
    )?;
    for ((t, _, key), r) in todo.into_iter().zip(results) {
        match r {
            Ok(p) => {
                prints.push(Fingerprinted { id: t.id, key, print: p });
                report.fingerprinted += 1;
            }
            Err(e) if matches!(e.inner(), Error::Cancelled) => return Err(e),
            Err(e) => report.failed.push(TrackFailure { track: t.into(), error: e.to_string() }),
        }
    }
    let mut warn = WarningLimiter::new(ctx, 3);
    for f in &report.failed {
        warn.warn("unreadable tracks", format!("couldn't fingerprint {}: {}", f.track.name, f.error));
    }
    warn.finish();
    // Keep the index in library order, whatever order the workers finished in.
    let order: std::collections::HashMap<u32, usize> =
        library.tracks.iter().enumerate().map(|(i, t)| (t.id, i)).collect();
    prints.sort_by_key(|f| order.get(&f.id).copied().unwrap_or(usize::MAX));
    report.indexed = prints.len();
    Ok((prints, report))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prints_round_trip() {
        let p = TrackPrint {
            duration_secs: 312.5,
            peaks: vec![Peak { t: 0, f: 3 }, Peak { t: 0, f: 200 }, Peak { t: 130, f: 7 }, Peak { t: 70_000, f: 255 }],
        };
        let bytes = p.encode();
        assert_eq!(TrackPrint::decode(&bytes), Some(p.clone()));
        assert_eq!(TrackPrint::decode(&bytes[..bytes.len() - 1]), None);

        let dir = tempfile::tempdir().unwrap();
        let cache = Cache::new(dir.path());
        cache.put("abcdef", &p).unwrap();
        assert_eq!(cache.get("abcdef"), Some(p));
        assert_eq!(cache.get("abcdeg"), None);
    }

    #[test]
    fn failures_are_remembered_by_the_version_that_failed() {
        let dir = tempfile::tempdir().unwrap();
        let cache = Cache::new(dir.path());
        cache.put_failure("abcdef", "could not parse x.wav: no audio track").unwrap();
        assert_eq!(cache.failure("abcdef").as_deref(), Some("could not parse x.wav: no audio track"));
        assert_eq!(cache.get("abcdef"), None);
        std::fs::write(cache.file("abcdef", "err"), "0.0.1\nan older decoder's").unwrap();
        assert_eq!(cache.failure("abcdef"), None, "a newer version tries again");
    }

    /// A file the decoder can't make sense of (a WAV saying 0 Hz panics symphonia) is reported
    /// and remembered, so the next run neither crashes nor decodes it again. Tracks as long as a
    /// recorded set aren't indexed.
    #[test]
    fn unreadable_and_long_tracks_are_left_out() {
        use crate::model::tests::track;
        use crate::probe::tests::wav;
        use crate::progress::{CancelToken, Event};
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("drive");
        std::fs::create_dir_all(root.join("Contents")).unwrap();
        std::fs::write(root.join("Contents/zero.wav"), wav(0, 16, 0.0)).unwrap();
        std::fs::write(root.join("Contents/set.wav"), wav(8_000, 16, 1.0)).unwrap();
        let device = Device::open(&root).unwrap();
        let mut set = track(2, "DJ", "Live", "/Contents/set.wav");
        set.duration_secs = LONGEST_TRACK_SECS + 1;
        let library = Library { tracks: vec![track(1, "A", "Zero", "/Contents/zero.wav"), set], ..Default::default() };
        let cache = Cache::new(dir.path().join("cache"));

        let decoded = std::sync::atomic::AtomicUsize::new(0);
        let count = |e: &Event| {
            if matches!(e, Event::Item { phase: Phase::Fingerprint, .. }) {
                decoded.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }
        };
        let ctx = Ctx::new(&count, CancelToken::new());
        for run in 0..2 {
            let (prints, report) = fingerprint_library(&device, &library, &cache, &ctx).unwrap();
            assert!(prints.is_empty());
            assert_eq!(report.long.iter().map(|t| t.id).collect::<Vec<_>>(), [2]);
            assert_eq!(report.failed.len(), 1, "run {run}");
            assert!(report.failed[0].error.contains("crashed"), "{}", report.failed[0].error);
        }
        assert_eq!(decoded.into_inner(), 1, "decoded once, on the first run");
    }

    /// A failure is remembered only for the file it came from: one that went away or changed
    /// while it was read (a drive pulled out) is tried again when it's back as it was.
    #[test]
    fn a_failure_from_a_file_changed_mid_read_is_not_remembered() {
        use crate::model::tests::track;
        use crate::probe::tests::wav;
        use crate::progress::{CancelToken, Event};
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("drive");
        let file = root.join("Contents/a.wav");
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, wav(8_000, 16, 1.0)).unwrap();
        let modified = std::fs::metadata(&file).unwrap().modified().unwrap();
        let device = Device::open(&root).unwrap();
        let library = Library { tracks: vec![track(1, "A", "One", "/Contents/a.wav")], ..Default::default() };
        let cache = Cache::new(dir.path().join("cache"));

        // Swapped for one the decoder crashes on as it's about to be read.
        let swap = |e: &Event| {
            if matches!(e, Event::Item { phase: Phase::Fingerprint, .. }) {
                std::fs::write(&file, wav(0, 16, 0.0)).unwrap();
            }
        };
        let (_, report) = fingerprint_library(&device, &library, &cache, &Ctx::new(&swap, CancelToken::new())).unwrap();
        assert_eq!(report.failed.len(), 1);

        std::fs::write(&file, wav(8_000, 16, 1.0)).unwrap();
        std::fs::File::options().write(true).open(&file).unwrap().set_modified(modified).unwrap();
        let (prints, report) = fingerprint_library(&device, &library, &cache, &Ctx::silent()).unwrap();
        assert!(report.failed.is_empty() && prints.len() == 1, "{:?}", report.failed);
    }

    #[test]
    fn index_finds_hashes() {
        let peaks: Vec<Peak> = (0..50).map(|i| Peak { t: i * 5, f: (10 + (i * 37) % 200) as u8 }).collect();
        let idx = LibraryIndex::build(vec![(42, TrackPrint { duration_secs: 6.0, peaks: peaks.clone() })]);
        let lm = fingerprint::landmarks(&peaks, LIBRARY_FANOUT)[5];
        let hits = idx.bucket(lm.hash());
        assert!(hits.iter().any(|&e| (e >> 32) == 0 && (e as u32) == lm.t));
        assert_eq!(idx.track_ids, vec![42]);
    }
}
