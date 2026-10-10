#![doc = include_str!("../README.md")]

pub mod audio;
mod error;
pub mod fingerprint;
pub mod index;
pub mod matcher;
#[cfg(any(test, feature = "test-util"))]
pub mod test_util;

use std::path::Path;

use loclib_progress::Ctx;

pub use audio::SourceFormat;
pub use error::{Error, Result};
pub use index::{LibraryIndex, TrackPrint, fingerprint_file};
pub use matcher::Found;

use fingerprint::{Analyzer, FrameLevels, Peak};

/// A recording decoded and fingerprinted, ready for [`matcher::find`].
pub struct Heard {
    pub peaks: Vec<Peak>,
    /// Each frame's loudness, for drawing the recording.
    pub levels: Vec<FrameLevels>,
    pub frames: u32,
}

impl From<Analyzer> for Heard {
    fn from(analyzer: Analyzer) -> Heard {
        let frames = analyzer.frames();
        let (peaks, levels) = analyzer.finish();
        Heard { peaks, levels, frames }
    }
}

/// Decode and fingerprint the recording at `path`. Another source (an editor's project) is heard
/// by pushing it into an [`Analyzer`] made with [`fingerprint::QUERY`] and levels, then taking
/// [`Heard::from`] it.
pub fn listen(path: &Path, ctx: &Ctx<'_>) -> Result<(SourceFormat, Heard)> {
    let mut analyzer = Analyzer::new(fingerprint::QUERY, true);
    let format = audio::decode(path, fingerprint::RATE, ctx, &mut |s| analyzer.push(s))?;
    Ok((format, analyzer.into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::{synth, wav, wav_of};

    /// Fingerprint `x` as a library track would be.
    fn print(x: &[f32]) -> TrackPrint {
        let mut a = Analyzer::new(fingerprint::LIBRARY, false);
        a.push(x);
        TrackPrint { duration_secs: x.len() as f32 / fingerprint::RATE as f32, peaks: a.finish().0 }
    }

    /// A recording on disk is found to play the tracks it was made of, in order, whatever rate
    /// it was saved at.
    #[test]
    fn a_recording_is_heard_and_its_tracks_found() {
        let (a, b, c) = (synth(40.0, 7919), synth(40.0, 3 * 7919), synth(40.0, 5 * 7919));
        let index = LibraryIndex::build(vec![(10, print(&a)), (20, print(&b)), (30, print(&c))]);
        let dir = tempfile::tempdir().unwrap();
        let set = dir.path().join("set.wav");
        let mut resampler = audio::Resampler::new(fingerprint::RATE, 44_100);
        let mut pcm = Vec::new();
        resampler.push(&[&a[..], &c[..]].concat(), &mut |s| pcm.extend_from_slice(s));
        resampler.finish(&mut |s| pcm.extend_from_slice(s));
        std::fs::write(&set, wav_of(44_100, 16, pcm.iter().map(|s| (s * 12_000.0) as i32))).unwrap();

        let (format, heard) = listen(&set, &Ctx::silent()).unwrap();
        assert_eq!((format.sample_rate, format.channels), (44_100, 2));
        assert!((format.duration_secs - 80.0).abs() < 0.1, "{format:?}");
        assert!(heard.levels.len() >= heard.frames as usize);
        let found = matcher::find(&index, &heard.peaks, heard.frames, 0.02, None, &Ctx::silent()).unwrap();
        let ids: Vec<u32> = found.iter().map(|f| index.track_ids[f.slot as usize]).collect();
        assert_eq!(ids, [10, 30], "{found:#?}");
        assert!(found.iter().all(|f| f.confidence() > 0.5 && f.semitones().abs() < 0.1), "{found:#?}");
    }

    /// A file the decoder panics on (a WAV saying 0 Hz) is that file's error; a missing one says
    /// where it was looked for.
    #[test]
    fn unreadable_files_are_errors() {
        let dir = tempfile::tempdir().unwrap();
        let zero = dir.path().join("zero.wav");
        std::fs::write(&zero, wav(0, 16, 0.0)).unwrap();
        let err = fingerprint_file(&zero, &Ctx::silent()).unwrap_err();
        assert!(matches!(&err, Error::Parse { message, .. } if message == audio::CRASHED), "{err}");
        let gone = dir.path().join("gone.wav");
        assert!(matches!(listen(&gone, &Ctx::silent()), Err(Error::Io { path, .. }) if path == gone));
    }
}
