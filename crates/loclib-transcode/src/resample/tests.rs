use super::*;

/// Feeds `input` in uneven pieces, as decoded packets come.
fn resample(input: &[f64], from: u32, to: u32, channels: usize) -> Vec<f64> {
    let mut r = Resampler::new(from, to, channels);
    let mut out = Vec::new();
    let mut sink = |s: &[f64]| -> Result<(), ()> {
        out.extend_from_slice(s);
        Ok(())
    };
    for piece in input.chunks(577 * channels) {
        r.push(piece, &mut sink).unwrap();
    }
    r.finish(&mut sink).unwrap();
    out
}

/// A click comes out at the same time it went in, to the nearest frame: on the frame itself
/// when one rate is a multiple of the other.
#[test]
fn clicks_stay_in_time() {
    for (from, to) in [(96_000, 48_000), (88_200, 44_100), (44_100, 88_200), (44_100, 48_000), (48_000, 44_100)] {
        let mut click = vec![0.0; from as usize / 5];
        click[from as usize / 10] = 1.0;
        let out = resample(&click, from, to, 1);
        let peak = (0..out.len()).max_by(|a, b| out[*a].total_cmp(&out[*b])).unwrap();
        assert_eq!(peak, to as usize / 10, "{from} -> {to}");
        // rubato's kernel sits 1/128 of an input sample early (its table step), which tips
        // these by about 0.02; a frame or half a frame out would by far more.
        if from % to == 0 || to % from == 0 {
            let (before, after) = (out[peak - 1], out[peak + 1]);
            assert!((before - after).abs() < 0.05, "{from} -> {to}: {before} and {after} around the click");
        }
    }
}

/// Lengths follow the input to the nearest frame, and a short clip (under one chunk) or none
/// at all comes out right too.
#[test]
fn length_follows_the_input() {
    for (frames, from, to) in
        [(96_000, 96_000, 48_000), (44_101, 44_100, 48_000), (300, 88_200, 44_100), (0, 48_000, 44_100)]
    {
        let out = resample(&vec![0.1; frames * 2], from, to, 2);
        let expected = (frames as u64 * u64::from(to) + u64::from(from) / 2) / u64::from(from);
        assert_eq!(out.len() as u64, expected * 2, "{frames} frames {from} -> {to}");
    }
}

use hegel::TestCase;
use hegel::generators as gs;

/// Between any two of the rates players take, in any number of channels, however the input
/// comes: the output is the input's length at the new rate to the nearest frame, the same
/// whatever the pieces, and a click comes out within a frame of when it went in.
#[hegel::test(test_cases = 5)]
fn clicks_keep_their_time_between_any_rates(tc: TestCase) {
    let rates = vec![32_000, 44_100, 48_000, 88_200, 96_000];
    let (from, to) = (tc.draw(gs::sampled_from(rates.clone())), tc.draw(gs::sampled_from(rates)));
    let channels = tc.draw(gs::integers::<usize>().min_value(1).max_value(3));
    let frames = tc.draw(gs::integers::<usize>().min_value(800).max_value(2500));
    let at = tc.draw(gs::integers::<usize>().min_value(frames / 4).max_value(3 * frames / 4));
    let mut input = vec![0.0; frames * channels];
    input[at * channels..(at + 1) * channels].fill(0.9);
    let piece = tc.draw(gs::integers::<usize>().min_value(1).max_value(3000));
    let mut r = Resampler::new(from, to, channels);
    let mut out = Vec::new();
    let mut sink = |s: &[f64]| -> Result<(), ()> {
        out.extend_from_slice(s);
        Ok(())
    };
    for p in input.chunks(piece * channels) {
        r.push(p, &mut sink).unwrap();
    }
    r.finish(&mut sink).unwrap();
    let expected = (frames as u64 * u64::from(to) + u64::from(from) / 2) / u64::from(from);
    assert_eq!(out.len() as u64, expected * channels as u64);
    assert_eq!(out, resample(&input, from, to, channels), "pieces of {piece} change it");
    for c in 0..channels {
        let channel: Vec<f64> = out.iter().skip(c).step_by(channels).copied().collect();
        let peak = (0..channel.len()).max_by(|a, b| channel[*a].total_cmp(&channel[*b])).unwrap();
        let truth = at as f64 * f64::from(to) / f64::from(from);
        assert!((peak as f64 - truth).abs() <= 1.0, "{from} -> {to}: the click at {at} came out at {peak}");
    }
}
