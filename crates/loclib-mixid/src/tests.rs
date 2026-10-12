use super::*;
use crate::test_util::{resample, synth, wav, wav_of};

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
    let pcm = resample(&[&a[..], &c[..]].concat(), fingerprint::RATE, 44_100);
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

use hegel::generators as gs;

/// A WAV at the fingerprints' own rate is heard from the file as from its samples in
/// memory: the same peaks as a library track and as a recording (with its levels and frames
/// as the analyzer counts them), and as long. A file that isn't there is that file's error.
#[hegel::test(test_cases = 8)]
fn files_are_heard_as_their_samples(tc: hegel::TestCase) {
    let secs = tc.draw(gs::floats::<f64>().min_value(0.2).max_value(3.0));
    let pcm: Vec<i32> =
        synth(secs, tc.draw(gs::integers::<u32>().min_value(1))).iter().map(|s| (s * 12_000.0) as i32).collect();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("x.wav");
    std::fs::write(&path, wav_of(fingerprint::RATE, 16, pcm.iter().copied())).unwrap();
    let samples: Vec<f32> = pcm.iter().map(|&s| s as f32 / 32_768.0).collect();

    let file = fingerprint_file(&path, &Ctx::silent()).unwrap();
    assert_eq!(
        file,
        TrackPrint { duration_secs: (samples.len() as f64 / fingerprint::RATE as f64) as f32, ..print(&samples) }
    );
    let (format, heard) = listen(&path, &Ctx::silent()).unwrap();
    let mut a = Analyzer::new(fingerprint::QUERY, true);
    a.push(&samples);
    let frames = a.frames();
    let (peaks, levels) = a.finish();
    assert_eq!((heard.peaks, heard.levels.len(), heard.frames), (peaks, levels.len(), frames));
    assert_eq!(format.duration_secs, samples.len() as f64 / fingerprint::RATE as f64);

    let gone = dir.path().join("gone.wav");
    assert!(matches!(fingerprint_file(&gone, &Ctx::silent()), Err(Error::Io { path, .. }) if path == gone));
    assert!(matches!(listen(&gone, &Ctx::silent()), Err(Error::Io { path, .. }) if path == gone));
}
