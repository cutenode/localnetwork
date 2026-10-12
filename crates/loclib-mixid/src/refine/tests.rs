use std::path::PathBuf;

use super::*;
use crate::test_util::{resample, synth, wav_of};

/// Rate the test signal is taken to be at: 44.1 kHz isn't a multiple of it, so a delay of
/// whole samples here is a fraction of one there.
const MASTER: u32 = 16_000;

/// `x` (at [`MASTER`] Hz) delayed by `delay` of its samples, or started that many in when
/// negative, saved as a 16-bit WAV at `rate`.
fn save(dir: &Path, name: &str, x: &[f32], delay: isize, rate: u32) -> PathBuf {
    let shifted: Vec<f32> = match usize::try_from(delay) {
        Ok(d) => std::iter::repeat_n(0.0, d).chain(x.iter().copied()).collect(),
        Err(_) => x[delay.unsigned_abs()..].to_vec(),
    };
    let pcm = resample(&shifted, MASTER, rate);
    let path = dir.join(name);
    std::fs::write(&path, wav_of(rate, 16, pcm.iter().map(|s| (s * 12_000.0) as i32))).unwrap();
    path
}

/// About 10 s at [`MASTER`] Hz.
fn track(seed: u32) -> Vec<f32> {
    synth(14.0, seed)
}

/// Copies shifted by fractions of a sample, either way, made at 48 kHz, or low-passed and
/// coarsely quantised like a lossy encode are lined up to within a microsecond, from an
/// estimate 4 ms out either way.
#[test]
fn copies_are_lined_up_to_a_fraction_of_a_sample() {
    let dir = tempfile::tempdir().unwrap();
    let x = track(7919);
    let original = save(dir.path(), "a.wav", &x, 0, 44_100);
    let lossy: Vec<f32> =
        resample(&resample(&x, MASTER, 8_000), 8_000, MASTER).iter().map(|s| (s * 16.0).round() / 16.0).collect();
    // (copy, its delay, how far out the estimate is). 401 samples is 25.06 ms, 1105.26 samples
    // at 44.1 kHz.
    let copies = [
        (save(dir.path(), "later.wav", &x, 401, 44_100), 401, -0.004),
        (save(dir.path(), "earlier.wav", &x, -173, 44_100), -173, 0.004),
        (save(dir.path(), "48k.wav", &x, 401, 48_000), 401, 0.004),
        (save(dir.path(), "lossy.wav", &lossy, 401, 44_100), 401, -0.004),
    ];
    for (copy, delay, miss) in &copies {
        let truth = *delay as f64 / MASTER as f64;
        let r = refine(&original, copy, truth + miss, &Ctx::silent()).unwrap();
        assert!((r.offset - truth).abs() < 1e-6, "{copy:?}: {r:?}");
        assert!(r.correlation > REFINE_MIN_CORRELATION, "{copy:?}: {r:?}");
    }
    // And the other way round.
    let back = refine(&copies[3].0, &original, -0.021, &Ctx::silent()).unwrap();
    assert!((back.offset + 401.0 / MASTER as f64).abs() < 1e-6, "{back:?}");
    // An estimate a sample over 10 ms out puts the truth past the search's edge, where the
    // correlation still climbs: the edge isn't taken for the peak.
    let far = refine(&original, &copies[3].0, 401.0 / MASTER as f64 + 0.01 + 1.0 / RATE as f64, &Ctx::silent());
    assert!(matches!(far, Err(Error::Invalid(_))), "{far:?}");
}

/// Another track, or an edit with 2 s cut from its first few, doesn't correlate at any
/// offset near the estimate.
#[test]
fn other_audio_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let x = track(7919);
    let original = save(dir.path(), "a.wav", &x, 0, 44_100);
    let m = MASTER as usize;
    let others = [
        save(dir.path(), "other.wav", &track(3 * 7919), 0, 44_100),
        save(dir.path(), "edit.wav", &[&x[..3 * m], &x[5 * m..]].concat(), 0, 44_100),
    ];
    for other in &others {
        let r = refine(&original, other, 0.0, &Ctx::silent()).unwrap();
        assert!(r.correlation < REFINE_MIN_CORRELATION, "{other:?}: {r:?}");
    }
}

/// Too little audio, or only silence, is an error; so is a missing file and cancelling.
#[test]
fn short_quiet_and_missing_files_and_cancelling_are_errors() {
    let dir = tempfile::tempdir().unwrap();
    let x = track(7919);
    let original = save(dir.path(), "a.wav", &x, 0, 44_100);
    let short = save(dir.path(), "short.wav", &x[..MASTER as usize], 0, 44_100);
    let silent = save(dir.path(), "silent.wav", &vec![0.0; x.len()], 0, 44_100);
    for (a, b) in [(&short, &original), (&original, &short), (&silent, &original)] {
        let err = refine(a, b, 0.0, &Ctx::silent()).unwrap_err();
        assert!(matches!(err, Error::Invalid(_)), "{a:?} {b:?}: {err}");
    }
    let gone = dir.path().join("gone.wav");
    assert!(matches!(refine(&original, &gone, 0.0, &Ctx::silent()), Err(Error::Io { path, .. }) if path == gone));
    let ctx = Ctx::silent();
    ctx.cancel.cancel();
    assert!(matches!(refine(&short, &short, 0.0, &ctx), Err(Error::Cancelled(_))));
}

use hegel::TestCase;
use hegel::generators as gs;

/// A copy at any delay either way, at any level and at either rate, is lined up to within a
/// microsecond from any estimate up to 4 ms out, and correlates as the same audio.
#[hegel::test(test_cases = 2, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn any_delay_is_measured_to_a_fraction_of_a_sample(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let x = synth(8.0, tc.draw(gs::integers::<u32>().min_value(1)));
    // Whole samples here, so a fraction of one at 44.1 kHz but every 160th.
    let delay =
        tc.draw(gs::integers::<isize>().min_value(1).max_value(600)) * if tc.draw(gs::booleans()) { 1 } else { -1 };
    tc.assume(delay % 160 != 0);
    let gain = tc.draw(gs::floats::<f32>().min_value(0.25).max_value(1.0));
    let rate = tc.draw(gs::sampled_from(vec![44_100, 48_000]));
    let miss = tc.draw(gs::floats::<f64>().min_value(-0.004).max_value(0.004));
    let original = save(dir.path(), "a.wav", &x, 0, 44_100);
    let quieter: Vec<f32> = x.iter().map(|s| s * gain).collect();
    let copy = save(dir.path(), "b.wav", &quieter, delay, rate);
    let truth = delay as f64 / MASTER as f64;
    let r = refine(&original, &copy, truth + miss, &Ctx::silent()).unwrap();
    assert!((r.offset - truth).abs() < 1e-6, "{truth}: {r:?}");
    assert!(r.correlation > REFINE_MIN_CORRELATION && r.correlation <= 1.0 + 1e-9, "{r:?}");
    // Under a window's worth of audio has nothing to line up.
    let short = save(dir.path(), "short.wav", &quieter[..MASTER as usize], 0, rate);
    assert!(matches!(refine(&original, &short, truth + miss, &Ctx::silent()), Err(Error::Invalid(_))));
}
