//! Fingerprints of a library's tracks and of the recordings identified against it, cached on the
//! computer, and the hash index built from a library's (loclib-mixid's [`TrackPrint`] and
//! [`LibraryIndex`], re-exported here).
//!
//! Fingerprinting decodes every track once, which takes minutes for a few thousand tracks. The
//! peaks are cached per audio file (see [`crate::audio_cache`]), and so is what's heard in a
//! recording ([`Recordings`]). The index is built from the cache in seconds, and kept beside it,
//! one per drive, until the prints it was built from change ([`library_index`]).

use std::path::{Path, PathBuf};

use schemars::JsonSchema;
use serde::Serialize;

use loclib_mixid::Heard;
use loclib_mixid::audio::SourceFormat;
use loclib_mixid::fingerprint::{FrameLevels, Peak};
pub use loclib_mixid::index::*;

pub use crate::audio_cache::TrackFailure;
use crate::audio_cache::{self, Kind};
use crate::device::Device;
use crate::model::{Library, Track, TrackRef};
use crate::progress::{Ctx, Event, Phase, WarningLimiter};
use crate::{Error, Result, fsutil};

/// Library tracks longer than this (seconds) aren't indexed: they're recorded sets or
/// continuous-mix albums, which match a whole mix and swallow the tracks heard in it.
pub const LONGEST_TRACK_SECS: u32 = 20 * 60;

/// Fingerprints, in `<store>/cache/fingerprints`.
#[derive(Debug, Clone, Copy)]
pub struct Prints;

impl Kind for Prints {
    const DIR: &'static str = "fingerprints";
    const EXT: &'static str = "fp";
    const WHAT: &'static str = "a fingerprint";
    const VERSION: u32 = VERSION as u32;
    type Value = TrackPrint;

    fn encode(print: &TrackPrint) -> Vec<u8> {
        print.encode()
    }

    fn decode(bytes: &[u8]) -> Option<TrackPrint> {
        TrackPrint::decode(bytes)
    }

    /// `LLFP` and the version, as every [`VERSION`] has written them.
    fn written_by(head: &[u8]) -> Option<u32> {
        let version = head.strip_prefix(b"LLFP")?.get(..2)?;
        Some(u16::from_le_bytes(version.try_into().ok()?).into())
    }
}

/// Fingerprints on disk, one small file per audio file.
pub type Cache = audio_cache::Cache<Prints>;

/// What's heard in recordings ([`super::listen`]), beside the library's prints in
/// `<store>/cache/fingerprints`.
#[derive(Debug, Clone, Copy)]
pub struct Recordings;

/// The version of [`Recordings`]' format, and of what's heard beside the peaks (the levels the
/// mix's waveform is drawn from). Bump when either changes; [`VERSION`] covers the peaks.
const HEARD_VERSION: u32 = 1;

const HEARD_MAGIC: &[u8; 4] = b"LLMX";

impl Kind for Recordings {
    const DIR: &'static str = Prints::DIR;
    const EXT: &'static str = "heard";
    const WHAT: &'static str = "a recording's fingerprint";
    const VERSION: u32 = ((VERSION as u32) << 16) | HEARD_VERSION;
    type Value = (SourceFormat, Heard);

    /// `LLMX`, [`Self::VERSION`], the format, the frame count, then each peak and each frame's
    /// levels, little-endian.
    fn encode((format, heard): &(SourceFormat, Heard)) -> Vec<u8> {
        let mut out = Vec::with_capacity(34 + heard.peaks.len() * 5 + heard.levels.len() * 16);
        out.extend_from_slice(HEARD_MAGIC);
        out.extend_from_slice(&Self::VERSION.to_le_bytes());
        out.extend_from_slice(&format.sample_rate.to_le_bytes());
        out.extend_from_slice(&format.channels.to_le_bytes());
        out.extend_from_slice(&format.duration_secs.to_le_bytes());
        out.extend_from_slice(&heard.frames.to_le_bytes());
        out.extend_from_slice(&(heard.peaks.len() as u32).to_le_bytes());
        for p in &heard.peaks {
            out.extend_from_slice(&p.t.to_le_bytes());
            out.push(p.f);
        }
        out.extend_from_slice(&(heard.levels.len() as u32).to_le_bytes());
        for l in &heard.levels {
            for v in [l.peak, l.bands[0], l.bands[1], l.bands[2]] {
                out.extend_from_slice(&v.to_le_bytes());
            }
        }
        out
    }

    fn decode(bytes: &[u8]) -> Option<(SourceFormat, Heard)> {
        let mut at = 0;
        let mut take = |n: usize| {
            let b = bytes.get(at..at + n)?;
            at += n;
            Some(b)
        };
        let u32_ = |b: &[u8]| u32::from_le_bytes(b.try_into().expect("4 bytes"));
        let f32_ = |b: &[u8]| f32::from_le_bytes(b.try_into().expect("4 bytes"));
        if take(4)? != HEARD_MAGIC || u32_(take(4)?) != Self::VERSION {
            return None;
        }
        let sample_rate = u32_(take(4)?);
        let channels = u16::from_le_bytes(take(2)?.try_into().ok()?);
        let duration_secs = f64::from_le_bytes(take(8)?.try_into().ok()?);
        let frames = u32_(take(4)?);
        let n = u32_(take(4)?) as usize;
        let peaks = take(n.checked_mul(5)?)?.chunks_exact(5).map(|c| Peak { t: u32_(&c[..4]), f: c[4] }).collect();
        let n = u32_(take(4)?) as usize;
        let levels = take(n.checked_mul(16)?)?
            .chunks_exact(16)
            .map(|c| FrameLevels { peak: f32_(&c[..4]), bands: [f32_(&c[4..8]), f32_(&c[8..12]), f32_(&c[12..])] })
            .collect();
        take(1)
            .is_none()
            .then_some((SourceFormat { sample_rate, channels, duration_secs }, Heard { peaks, levels, frames }))
    }

    fn written_by(head: &[u8]) -> Option<u32> {
        let version = head.strip_prefix(HEARD_MAGIC)?.get(..4)?;
        Some(u32::from_le_bytes(version.try_into().ok()?))
    }
}

/// What's heard in recordings, kept beside `prints`.
pub fn recordings(prints: &Cache) -> audio_cache::Cache<Recordings> {
    audio_cache::Cache::new(prints.root())
}

/// A recording's key: its file's, and for an Audacity project, its write-ahead log's too (what
/// Audacity hasn't written into the project yet).
fn recording_key(path: &Path) -> Option<String> {
    let key = audio_cache::Cache::<Recordings>::key(path)?;
    let mut wal = path.as_os_str().to_owned();
    wal.push("-wal");
    Some(match audio_cache::Cache::<Recordings>::key(Path::new(&wal)) {
        Some(log) => audio_cache::Cache::<Recordings>::key_of(&format!("{key}\u{1f}{log}")),
        None => key,
    })
}

/// What `hear` hears in the recording at `path`, from `cache` when it was heard before (and kept
/// there when it wasn't), and whether it came from the cache.
pub(super) fn heard(
    cache: Option<&audio_cache::Cache<Recordings>>,
    path: &Path,
    hear: impl FnOnce() -> Result<(SourceFormat, Heard)>,
    ctx: &Ctx<'_>,
) -> Result<((SourceFormat, Heard), bool)> {
    let key = cache.and_then(|_| recording_key(path));
    let Some((cache, key)) = cache.zip(key) else { return Ok((hear()?, false)) };
    if let Some(kept) = cache.get(&key) {
        ctx.emit(Event::Item { phase: Phase::Listen, path: "heard before: from the cache".into(), bytes: 0 });
        return Ok((kept, true));
    }
    let heard = hear()?;
    if let Err(e) = cache.put(&key, &heard) {
        ctx.warn(format!("couldn't cache {}: {e}", Recordings::WHAT));
    }
    Ok((heard, false))
}

/// The name of the folder in a fingerprint cache that indexes are kept in, one per drive.
const INDEX_DIR: &str = "index";

/// Where `cache` keeps `drive`'s index: by a hash of the drive's identity (or name).
fn index_file(cache: &Cache, drive: &str) -> PathBuf {
    cache.root().join(INDEX_DIR).join(format!("{}.idx", &blake3::hash(drive.as_bytes()).to_hex()[..16]))
}

/// What an index is built from: its tracks and their prints' keys, in order, and the versions of
/// what makes prints and indexes.
fn index_key(prints: &[Fingerprinted]) -> [u8; 32] {
    let mut h = blake3::Hasher::new();
    h.update(&VERSION.to_le_bytes());
    h.update(&INDEX_VERSION.to_le_bytes());
    for p in prints {
        h.update(&p.id.to_le_bytes());
        h.update(p.key.as_bytes());
    }
    *h.finalize().as_bytes()
}

/// What older versions kept in the fingerprint cache `prints` that nothing reads any more, for
/// `store gc`: prints and recordings heard by an older [`VERSION`], and indexes in an older
/// [`INDEX_VERSION`] or of older prints.
pub(crate) fn older(prints: &Cache) -> Vec<PathBuf> {
    let mut older = prints.older();
    older.extend(recordings(prints).older());
    let indexes = std::fs::read_dir(prints.root().join(INDEX_DIR)).into_iter().flatten().flatten().map(|e| e.path());
    older.extend(indexes.filter(|f| {
        let mut head = [0u8; 40];
        let read = std::fs::File::open(f).and_then(|mut file| std::io::Read::read_exact(&mut file, &mut head));
        // After the key: `LLIX`, then the versions of prints and of the index.
        let version = |at: usize| u16::from_le_bytes([head[at], head[at + 1]]);
        read.is_ok() && &head[32..36] == b"LLIX" && (version(36), version(38)) < (VERSION, INDEX_VERSION)
    }));
    older
}

/// The index of `prints`: the one kept for `drive` beside `cache` when it was built from the same
/// prints, else built now and kept in its place. Says whether it was the one kept.
pub fn library_index(cache: &Cache, drive: &str, prints: Vec<Fingerprinted>, ctx: &Ctx<'_>) -> (LibraryIndex, bool) {
    let key = index_key(&prints);
    let file = index_file(cache, drive);
    let kept = std::fs::read(&file).ok().and_then(|b| {
        let index = b.strip_prefix(key.as_slice())?;
        LibraryIndex::decode(index)
    });
    if let Some(index) = kept {
        ctx.emit(Event::Item { phase: Phase::Index, path: "from the cache".into(), bytes: 0 });
        return (index, true);
    }
    let index = LibraryIndex::build(prints.into_iter().map(|f| (f.id, f.print)).collect());
    let mut bytes = key.to_vec();
    bytes.extend(index.encode());
    if let Err(e) = fsutil::write_deferred(&file, &bytes) {
        ctx.warn(format!("couldn't cache the library's index: {e}"));
    }
    (index, false)
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
    let (long, tracks): (Vec<&Track>, Vec<&Track>) =
        library.tracks.iter().partition(|t| t.duration_secs > LONGEST_TRACK_SECS);
    let run = audio_cache::for_tracks(
        device,
        &tracks,
        cache,
        Phase::Fingerprint,
        |file, ctx| fingerprint_file(file, ctx).map_err(Error::from),
        ctx,
    )?;
    let mut warn = WarningLimiter::new(ctx, 3);
    for f in &run.failed {
        warn.warn("unreadable tracks", format!("couldn't fingerprint {}: {}", f.track.name, f.error));
    }
    warn.finish();
    let report = IndexReport {
        tracks: library.tracks.len(),
        indexed: run.done.len(),
        fingerprinted: run.computed,
        missing: run.missing.iter().filter_map(|id| library.track(*id)).map(TrackRef::from).collect(),
        failed: run.failed,
        long: long.into_iter().map(TrackRef::from).collect(),
        hashes: 0,
    };
    let prints = run.done.into_iter().map(|d| Fingerprinted { id: d.id, key: d.key, print: d.value }).collect();
    Ok((prints, report))
}

#[cfg(test)]
mod tests;
