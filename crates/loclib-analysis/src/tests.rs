use std::f64::consts::PI;

use loclib_mixid::test_util::{wav, wav_of};

use super::*;
use crate::tempo::TempoRelation;

/// A 16-bit stereo WAV of `signal` (full scale ±1.0, clamped) in both channels.
fn write(dir: &tempfile::TempDir, name: &str, rate: u32, signal: &[f64]) -> std::path::PathBuf {
    let path = dir.path().join(name);
    let pcm = signal.iter().map(|s| (s.clamp(-1.0, 1.0) * 32_767.0).round() as i32);
    std::fs::write(&path, wav_of(rate, 16, pcm.collect::<Vec<_>>().into_iter())).unwrap();
    path
}

fn sine(rate: u32, secs: f64, freq: f64, amplitude: f64, phase: f64) -> Vec<f64> {
    (0..(rate as f64 * secs) as usize)
        .map(|i| amplitude * (2.0 * PI * freq * i as f64 / rate as f64 + phase).sin())
        .collect()
}

/// A kick drum on every beat at `bpm` over a sustained A minor chord.
fn kick_track(bpm: f64, secs: f64) -> Vec<f64> {
    let rate = f64::from(RATE);
    let beat = 60.0 / bpm;
    (0..(rate * secs) as usize)
        .map(|i| {
            let t = i as f64 / rate;
            let since = t % beat;
            // A sine swept down from 150 to 50 Hz, decaying fast.
            let kick = 0.6 * (-since * 25.0).exp() * (2.0 * PI * (50.0 + 100.0 * (-since * 40.0).exp()) * since).sin();
            let chord: f64 = [220.0, 261.63, 329.63]
                .iter()
                .map(|f| 0.05 * (2.0 * PI * f * t).sin() + 0.02 * (2.0 * PI * 2.0 * f * t).sin())
                .sum();
            kick + chord
        })
        .collect()
}

/// A kick track is heard at its tempo, and a cached result reads back the same. Its chord, the
/// same all through, fits A minor and C major alike: keys that tie are no key.
#[test]
fn a_kick_track_is_estimated() {
    let dir = tempfile::tempdir().unwrap();
    let path = write(&dir, "kick.wav", RATE, &kick_track(128.0, 20.0));
    let analysis = analyze(&path, Want { estimate: true, loudness: false, waveform: false }, &Ctx::silent()).unwrap();
    assert_eq!(analysis.version, VERSION);
    assert!((analysis.duration_secs - 20.0).abs() < 0.01, "{}", analysis.duration_secs);
    assert_eq!(analysis.loudness, None);
    let e = analysis.estimate.clone().unwrap();
    assert_eq!(TempoRelation::between(e.bpm.into(), 128.0), Some(TempoRelation::Agrees), "{e:?}");
    assert_eq!((e.key, e.key_confidence), (None, 0.0), "{e:?}");
    let json = serde_json::to_string(&analysis).unwrap();
    assert_eq!(serde_json::from_str::<Analysis>(&json).unwrap(), analysis);
}

/// A low tone, then a high one: the lows are drawn where the low tone is and the highs where
/// the high one is, a column every 23 ms.
#[test]
fn a_waveform_draws_each_band() {
    let dir = tempfile::tempdir().unwrap();
    let tones = [sine(44_100, 2.0, 100.0, 0.5, 0.0), sine(44_100, 2.0, 5_000.0, 0.5, 0.0)].concat();
    let path = write(&dir, "tones.wav", 44_100, &tones);
    let want = Want { estimate: false, loudness: false, waveform: true };
    let analysis = analyze(&path, want, &Ctx::silent()).unwrap();
    let w = analysis.waveform.clone().unwrap();
    assert!((w.columns.len() as f64 - 4.0 * COLUMNS_PER_SEC).abs() <= 2.0, "{} columns", w.columns.len());
    let second = COLUMNS_PER_SEC as usize;
    assert!(w.columns[..second].iter().all(|[low, _, high]| low > high), "{:?}", &w.columns[..second]);
    let last = &w.columns[w.columns.len() - second..];
    assert!(last.iter().all(|[low, _, high]| high > low), "{last:?}");
    let json = serde_json::to_string(&analysis).unwrap();
    assert_eq!(serde_json::from_str::<Analysis>(&json).unwrap(), analysis);
}

/// A blip too short for a tempo is measured all the same: no tempo, and its loudness.
#[test]
fn a_blip_keeps_its_loudness() {
    let dir = tempfile::tempdir().unwrap();
    let path = write(&dir, "blip.wav", 44_100, &sine(44_100, 0.05, 440.0, 0.5, 0.0));
    let analysis = analyze(&path, Want::ALL, &Ctx::silent()).unwrap();
    assert_eq!(analysis.estimate.map(|e| e.bpm), Some(0.0));
    let l = analysis.loudness.unwrap();
    assert!((l.sample_peak_dbfs() + 6.0).abs() < 0.1, "{l:?}");
}

/// A stereo 1 kHz sine at -20 dBFS measures -20 LUFS (EBU Tech 3341's first test, at another
/// level), with no loudness range and peaks at its level.
#[test]
fn a_sine_measures_its_level() {
    let dir = tempfile::tempdir().unwrap();
    let path = write(&dir, "sine.wav", 48_000, &sine(48_000, 10.0, 1_000.0, 0.1, 0.0));
    let analysis = analyze(&path, Want { estimate: false, loudness: true, waveform: false }, &Ctx::silent()).unwrap();
    assert_eq!(analysis.estimate, None);
    let l = analysis.loudness.unwrap();
    assert!((l.integrated_lufs.unwrap() + 20.0).abs() < 0.1, "{l:?}");
    assert!(l.range_lu < 0.1, "{l:?}");
    assert!((l.sample_peak_dbfs() + 20.0).abs() < 0.05, "{l:?}");
    assert!((l.true_peak_dbtp() + 20.0).abs() < 0.1, "{l:?}");
    assert_eq!((l.clipped_runs, l.clipped_samples), (0, 0));
}

/// A sine at a quarter of the sample rate, sampled 45 degrees off its peaks, peaks 3 dB above
/// its highest sample between samples.
#[test]
fn true_peak_finds_peaks_between_samples() {
    let dir = tempfile::tempdir().unwrap();
    let path = write(&dir, "quarter.wav", 44_100, &sine(44_100, 5.0, 44_100.0 / 4.0, 0.5, PI / 4.0));
    let l = analyze(&path, Want { estimate: false, loudness: true, waveform: false }, &Ctx::silent())
        .unwrap()
        .loudness
        .unwrap();
    assert!((l.true_peak_dbtp() - l.sample_peak_dbfs() - 3.01).abs() < 0.3, "{l:?}");
    assert!((l.true_peak_dbtp() + 6.02).abs() < 0.3, "{l:?}");
}

/// A 100 Hz sine driven to twice full scale clips flat on every half cycle, in both channels.
#[test]
fn clipping_is_counted_in_runs() {
    let dir = tempfile::tempdir().unwrap();
    let path = write(&dir, "clipped.wav", 44_100, &sine(44_100, 1.0, 100.0, 2.0, 0.0));
    let l = analyze(&path, Want::ALL, &Ctx::silent()).unwrap().loudness.unwrap();
    assert_eq!(l.clipped_runs, 100 * 2 * 2, "{l:?}");
    // Twice a sine reaches full scale for two thirds of every cycle.
    let expected = 44_100.0 * 2.0 / 3.0 * 2.0;
    assert!((l.clipped_samples as f64 / expected - 1.0).abs() < 0.01, "{l:?}");
}

/// Silence has no estimate and no integrated loudness, and serialises without infinities.
#[test]
fn silence_measures_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let path = write(&dir, "silence.wav", 44_100, &vec![0.0; 44_100 * 3]);
    let analysis = analyze(&path, Want::ALL, &Ctx::silent()).unwrap();
    assert_eq!(analysis.estimate, None);
    let l = analysis.loudness.unwrap();
    assert_eq!((l.integrated_lufs, l.sample_peak, l.true_peak), (None, 0.0, 0.0));
    assert_eq!(l.true_peak_dbtp(), f64::NEG_INFINITY);
    let json = serde_json::to_string(&analysis).unwrap();
    assert_eq!(serde_json::from_str::<Analysis>(&json).unwrap(), analysis);
}

#[test]
fn unreadable_files_and_cancelling_are_errors() {
    let dir = tempfile::tempdir().unwrap();
    let gone = dir.path().join("gone.wav");
    let err = analyze(&gone, Want::ALL, &Ctx::silent()).unwrap_err();
    assert!(matches!(&err, Error::Audio(loclib_mixid::Error::Io { path, .. }) if *path == gone), "{err}");
    let zero = dir.path().join("zero.wav");
    std::fs::write(&zero, wav(0, 16, 0.0)).unwrap();
    assert!(matches!(analyze(&zero, Want::ALL, &Ctx::silent()), Err(Error::Audio(loclib_mixid::Error::Parse { .. }))));
    let path = write(&dir, "sine.wav", 44_100, &sine(44_100, 30.0, 440.0, 0.5, 0.0));
    let ctx = Ctx::silent();
    ctx.cancel.cancel();
    assert!(matches!(analyze(&path, Want::ALL, &ctx), Err(Error::Cancelled(_))));
}
