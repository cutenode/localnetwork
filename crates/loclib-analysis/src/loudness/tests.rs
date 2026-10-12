use super::*;

/// Single full-scale samples and pairs aren't clipping; runs of three or more are, counted
/// once each whatever their length, across packets, in each channel apart.
#[test]
fn clipped_runs_need_three_samples_in_a_row() {
    let full = 1.0;
    let left = [0.0, full, 0.0, full, -full, 0.0, full, full, full, full, -full, 0.0];
    let right = [0.0, 0.0, -full, -full, -full, 0.0, 0.0, 0.0, full, 0.0, 0.0, 0.0];
    let interleaved: Vec<f32> = left.iter().zip(&right).flat_map(|(l, r)| [*l, *r]).collect();
    let mut meter = Meter::default();
    // Split inside the left channel's run of five.
    for packet in [&interleaved[..16], &interleaved[16..]] {
        meter.push(packet, 44_100, 2).unwrap();
    }
    let l = meter.finish().unwrap();
    assert_eq!((l.clipped_runs, l.clipped_samples), (2, 8));
    assert_eq!(l.sample_peak, 1.0);
}

#[test]
fn nothing_decoded_measures_nothing() {
    let l = Meter::default().finish().unwrap();
    assert_eq!((l.integrated_lufs, l.true_peak, l.clipped_runs), (None, 0.0, 0));
}

use hegel::TestCase;
use hegel::generators as gs;

/// However a signal is split into packets, in any number of channels: each channel's runs of
/// three or more full-scale samples are counted once each with all their samples, shorter
/// ones not at all, and the peak is the loudest sample.
#[hegel::test(test_cases = 200)]
fn clipping_is_counted_run_by_run(tc: TestCase) {
    let channels = tc.draw(gs::integers::<usize>().min_value(1).max_value(3));
    let levels = gs::sampled_from(vec![0.0f32, 0.5, -0.25, 1.0, -1.0, FULL_SCALE, 0.999]);
    let frames = tc.draw(gs::vecs(gs::vecs(levels).min_size(channels).max_size(channels)).max_size(200));
    let interleaved: Vec<f32> = frames.concat();
    let mut meter = Meter::default();
    let mut rest = &interleaved[..];
    while !rest.is_empty() {
        let n = tc.draw(gs::integers::<usize>().min_value(1).max_value(64)).min(rest.len() / channels) * channels;
        meter.push(&rest[..n], 44_100, channels).unwrap();
        rest = &rest[n..];
    }
    let l = meter.finish().unwrap();
    let (mut runs, mut samples) = (0, 0);
    for c in 0..channels {
        let column: Vec<bool> = frames.iter().map(|f| f[c].abs() >= FULL_SCALE).collect();
        for run in column.split(|clipped| !clipped).map(<[bool]>::len).filter(|n| *n >= CLIPPED_RUN as usize) {
            runs += 1;
            samples += run as u64;
        }
    }
    assert_eq!((l.clipped_runs, l.clipped_samples), (runs, samples));
    let loudest = interleaved.iter().fold(0.0f32, |m, s| m.max(s.abs()));
    assert_eq!(l.sample_peak, f64::from(loudest));
    assert_eq!(l.sample_peak_dbfs(), 20.0 * f64::from(loudest).log10());
}
