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
mod tests {
    use super::*;

    #[test]
    fn prints_round_trip() {
        use loclib_mixid::fingerprint::Peak;
        let p = TrackPrint {
            duration_secs: 312.5,
            peaks: vec![Peak { t: 0, f: 3 }, Peak { t: 0, f: 200 }, Peak { t: 130, f: 7 }, Peak { t: 70_000, f: 255 }],
        };
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
        use crate::probe::tests::wav;
        use crate::progress::{CancelToken, Event};
        use loclib_model::test_util::track;
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
        use crate::probe::tests::wav;
        use crate::progress::{CancelToken, Event};
        use loclib_model::test_util::track;
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
}
