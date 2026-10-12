//! Fingerprints of a library's tracks, cached on the computer, and the hash index built from them
//! (loclib-mixid's [`TrackPrint`] and [`LibraryIndex`], re-exported here).
//!
//! Fingerprinting decodes every track once, which takes minutes for a few thousand tracks. The
//! peaks are cached per audio file (see [`crate::audio_cache`]) and the index is rebuilt from the
//! cache in seconds.

use schemars::JsonSchema;
use serde::Serialize;

pub use loclib_mixid::index::*;

pub use crate::audio_cache::TrackFailure;
use crate::audio_cache::{self, Kind};
use crate::device::Device;
use crate::model::{Library, Track, TrackRef};
use crate::progress::{Ctx, Phase, WarningLimiter};
use crate::{Error, Result};

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
}

/// Fingerprints on disk, one small file per audio file.
pub type Cache = audio_cache::Cache<Prints>;

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
