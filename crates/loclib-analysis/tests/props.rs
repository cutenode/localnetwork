//! Properties of whole analyses, from synthesised audio written as WAV files.

use std::f64::consts::PI;
use std::path::{Path, PathBuf};

use hegel::TestCase;
use hegel::generators as gs;
use loclib_analysis::tempo::TempoRelation;
use loclib_analysis::{COLUMNS_PER_SEC, Error, VERSION, Want, analyze};
use loclib_mixid::test_util::wav_of;
use loclib_progress::Ctx;

/// A 16-bit stereo WAV of `signal` (full scale ±1.0, clamped) in both channels.
fn write(dir: &Path, rate: u32, signal: &[f64]) -> PathBuf {
    let path = dir.join("x.wav");
    let pcm: Vec<i32> = signal.iter().map(|s| (s.clamp(-1.0, 1.0) * 32_767.0).round() as i32).collect();
    std::fs::write(&path, wav_of(rate, 16, pcm.into_iter())).unwrap();
    path
}

/// A click on every beat at `bpm`: a burst of a 2 kHz tone, decaying fast.
fn clicks(rate: u32, bpm: f64, secs: f64) -> Vec<f64> {
    let beat = 60.0 / bpm;
    (0..(rate as f64 * secs) as usize)
        .map(|i| {
            let since = (i as f64 / rate as f64) % beat;
            0.8 * (-since * 60.0).exp() * (2.0 * PI * 2_000.0 * since).sin()
        })
        .collect()
}

/// A click track at any tempo DJs play is heard at that tempo, or at half or double it (the
/// octave errors every tempo estimator makes).
#[hegel::test(test_cases = 2, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn click_tracks_are_heard_at_their_tempo(tc: TestCase) {
    let bpm = tc.draw(gs::floats::<f64>().min_value(90.0).max_value(150.0));
    let dir = tempfile::tempdir().unwrap();
    let path = write(dir.path(), 44_100, &clicks(44_100, bpm, 8.0));
    let analysis = analyze(&path, Want { estimate: true, loudness: false, waveform: false }, &Ctx::silent()).unwrap();
    let e = analysis.estimate.unwrap();
    let heard = TempoRelation::between(e.bpm.into(), bpm);
    assert!(
        matches!(heard, Some(TempoRelation::Agrees | TempoRelation::Half | TempoRelation::Double)),
        "{bpm} BPM heard as {e:?}"
    );
    assert!((0.0..=1.0).contains(&e.bpm_confidence) && (0.0..=1.0).contains(&e.key_confidence), "{e:?}");
}

/// A click track at any tempo DJs play, after any silence, has a grid in its own time: beats in
/// order between the silence and the end, counted in fours from 1, each at the tempo heard, and,
/// when the grid is stable, a beat at that tempo apart.
#[hegel::test(test_cases = 3, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn grids_fall_after_the_silence(tc: TestCase) {
    let bpm = tc.draw(gs::floats::<f64>().min_value(90.0).max_value(150.0));
    let lead = tc.draw(gs::floats::<f64>().min_value(0.0).max_value(3.0));
    let signal: Vec<f64> =
        std::iter::repeat_n(0.0, (44_100.0 * lead) as usize).chain(clicks(44_100, bpm, 6.0)).collect();
    let dir = tempfile::tempdir().unwrap();
    let path = write(dir.path(), 44_100, &signal);
    let want = Want { estimate: true, loudness: false, waveform: false };
    let e = analyze(&path, want, &Ctx::silent()).unwrap().estimate.unwrap();
    assert!((0.0..=1.0).contains(&e.grid_stability), "{e:?}");
    let end = signal.len() as f64 / 44.1;
    for (i, b) in e.beats.iter().enumerate() {
        assert_eq!((usize::from(b.beat_number), b.bpm), (i % 4 + 1, f64::from(e.bpm)), "{e:?}");
        // Onsets are framed 46 ms at a time.
        assert!((lead * 1000.0 - 100.0..=end).contains(&f64::from(b.time_ms)), "{lead} s of silence: {e:?}");
    }
    assert!(e.beats.windows(2).all(|w| w[0].time_ms < w[1].time_ms), "{e:?}");
    let apart = 60_000.0 / f64::from(e.bpm);
    if e.grid_stability >= 0.95 {
        assert!(e.beats.windows(2).all(|w| (f64::from(w[1].time_ms - w[0].time_ms) - apart).abs() <= 1.0), "{e:?}");
    }
}

/// A 1 kHz sine at any level, rate and length measures that level: its integrated loudness in
/// LUFS and its peak in dBFS are its level in dB, it has no loudness range and doesn't clip.
/// Only what's asked for is measured, and the length is the file's either way.
#[hegel::test(test_cases = 6)]
fn a_sine_measures_its_level_whatever_it_is(tc: TestCase) {
    let level = tc.draw(gs::floats::<f64>().min_value(0.05).max_value(0.95));
    let rate = tc.draw(gs::sampled_from(vec![44_100, 48_000]));
    let secs = tc.draw(gs::floats::<f64>().min_value(0.5).max_value(1.5));
    let loudness = tc.draw(gs::booleans());
    let dir = tempfile::tempdir().unwrap();
    let n = (rate as f64 * secs) as usize;
    let sine: Vec<f64> = (0..n).map(|i| level * (2.0 * PI * 1_000.0 * i as f64 / rate as f64).sin()).collect();
    let path = write(dir.path(), rate, &sine);
    let analysis = analyze(&path, Want { estimate: false, loudness, waveform: false }, &Ctx::silent()).unwrap();
    assert_eq!((analysis.version, analysis.estimate.is_some()), (VERSION, false));
    assert!((analysis.duration_secs - n as f64 / rate as f64).abs() < 1e-9, "{analysis:?}");
    let Some(l) = analysis.loudness else { return assert!(!loudness) };
    let db = 20.0 * level.log10();
    assert!((l.integrated_lufs.unwrap() - db).abs() < 0.2, "{db} dB: {l:?}");
    assert!(
        (l.sample_peak_dbfs() - db).abs() < 0.05 && l.true_peak_dbtp() >= l.sample_peak_dbfs() - 0.05,
        "{db}: {l:?}"
    );
    assert!(l.range_lu < 0.5 && (l.clipped_runs, l.clipped_samples) == (0, 0), "{l:?}");
}

/// Any WAV, at any rate a file can say and any length up to a moment, is analysed for what's
/// asked of it: as long as the file, its loudness and waveform when asked for, and its tempo and
/// key when asked for and there's sound to estimate. Only the loudness meter refuses a rate, one
/// outside the 16 Hz to 2.8 MHz it takes, as that file's analyzer error. A cancelled analysis is
/// cancelled, and a file that isn't there is that file's error.
#[hegel::test(test_cases = 6, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn files_are_analysed_for_what_is_asked(tc: TestCase) {
    let rate = tc.draw(gs::sampled_from(vec![8, 15, 16, 22_050, 44_100, 3_000_000]));
    // Up to a quarter of a second, and a few frames at least.
    let frames = tc.draw(gs::integers::<usize>().min_value(1).max_value((rate as usize / 4).clamp(3, 4_000)));
    let level = tc.draw(gs::sampled_from(vec![0.0, 0.5]));
    let signal: Vec<f64> = (0..frames).map(|i| level * ((i * 7919 % 2001) as f64 / 1000.0 - 1.0)).collect();
    let sound = signal.iter().any(|s| (s * 32_767.0).round() != 0.0);
    let dir = tempfile::tempdir().unwrap();
    let path = write(dir.path(), rate, &signal);
    let gone = dir.path().join("gone.wav");
    for want in (0..8).map(|i| Want { estimate: i & 1 != 0, loudness: i & 2 != 0, waveform: i & 4 != 0 }) {
        let Want { estimate, loudness, waveform } = want;
        let refused = loudness && !(16..=2_822_400).contains(&rate);
        match analyze(&path, want, &Ctx::silent()) {
            Err(Error::Analyzer { path: at, .. }) if refused => assert_eq!(at, path),
            Ok(a) if !refused => {
                assert!((a.duration_secs - frames as f64 / f64::from(rate)).abs() < 1e-9, "{a:?}");
                let measured = (a.loudness.is_some(), a.estimate.is_some(), a.waveform.is_some());
                assert_eq!(measured, (loudness, estimate && sound, waveform), "{a:?}");
                // Between samples a signal only peaks higher, and silence has no peak at all.
                if let Some(l) = &a.loudness {
                    assert!(l.true_peak_dbtp() >= l.sample_peak_dbfs() - 0.05, "{l:?}");
                    assert_eq!(l.sample_peak_dbfs() == f64::NEG_INFINITY, !sound, "{l:?}");
                }
                // A column for each of the analyser's frames, the last padded out, and silence
                // draws none.
                if let Some(w) = &a.waveform {
                    assert!(w.columns.len() <= (a.duration_secs * COLUMNS_PER_SEC).ceil() as usize + 2, "{w:?}");
                    assert!(sound || w.columns.iter().all(|c| *c == [0; 3]), "{w:?}");
                }
            }
            other => panic!("{rate} Hz, {frames} frames, {want:?}: {other:?}"),
        }
        let ctx = Ctx::silent();
        ctx.cancel.cancel();
        let cancelled = analyze(&path, want, &ctx);
        // The meter refuses the rate at the first packet, before the cancel is seen.
        let stopped = match &cancelled {
            Err(Error::Cancelled(_)) => true,
            Err(Error::Analyzer { .. }) => refused,
            _ => false,
        };
        assert!(stopped, "{want:?}: {cancelled:?}");
        let err = analyze(&gone, want, &Ctx::silent()).unwrap_err();
        assert!(matches!(&err, Error::Audio(loclib_mixid::Error::Io { path, .. }) if *path == gone), "{err}");
    }
}
