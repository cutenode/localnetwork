//! Properties of whole analyses, from synthesised audio written as WAV files.

use std::f64::consts::PI;
use std::path::{Path, PathBuf};

use hegel::TestCase;
use hegel::generators as gs;
use loclib_analysis::tempo::TempoRelation;
use loclib_analysis::{VERSION, Want, analyze};
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
    let analysis = analyze(&path, Want { estimate: true, loudness: false }, &Ctx::silent()).unwrap();
    let e = analysis.estimate.unwrap();
    let heard = TempoRelation::between(e.bpm.into(), bpm);
    assert!(
        matches!(heard, Some(TempoRelation::Agrees | TempoRelation::Half | TempoRelation::Double)),
        "{bpm} BPM heard as {e:?}"
    );
    assert!((0.0..=1.0).contains(&e.bpm_confidence) && (0.0..=1.0).contains(&e.key_confidence), "{e:?}");
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
    let analysis = analyze(&path, Want { estimate: false, loudness }, &Ctx::silent()).unwrap();
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
