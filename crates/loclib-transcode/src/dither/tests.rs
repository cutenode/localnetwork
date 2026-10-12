use super::*;

/// A quarter of a 16-bit step, held for a second.
fn quarter_step(dither: Dither) -> Vec<i32> {
    let mut q = Quantizer::new(16, dither);
    (0..96_000).map(|_| q.quantize(0.25 / 32_768.0)).collect()
}

fn share(values: &[i32], v: i32) -> f64 {
    values.iter().filter(|&&x| x == v).count() as f64 / values.len() as f64
}

/// Rounding alone loses a signal under half a step; dithered, it survives as the average of
/// steps of -1, 0 and 1 in the proportions a ±1 LSB triangle gives (P(1) = 0.75²/2,
/// P(-1) = 0.25²/2), and the same run gives the same samples.
#[test]
fn tpdf_keeps_what_rounding_loses() {
    assert!(quarter_step(Dither::None).iter().all(|&v| v == 0));
    let dithered = quarter_step(Dither::Tpdf);
    assert!(dithered.iter().all(|v| (-1..=1).contains(v)));
    let mean = dithered.iter().map(|&v| f64::from(v)).sum::<f64>() / dithered.len() as f64;
    assert!((mean - 0.25).abs() < 0.01, "mean {mean}");
    assert!((share(&dithered, 1) - 0.28125).abs() < 0.01, "{}", share(&dithered, 1));
    assert!((share(&dithered, -1) - 0.03125).abs() < 0.005, "{}", share(&dithered, -1));
    assert_eq!(dithered, quarter_step(Dither::Tpdf));
}

/// Full scale clips to the depth's range instead of wrapping.
#[test]
fn clips_at_full_scale() {
    let mut q = Quantizer::new(24, Dither::None);
    assert_eq!([q.quantize(1.5), q.quantize(-1.5), q.quantize(0.5)], [8_388_607, -8_388_608, 4_194_304]);
}

use hegel::TestCase;
use hegel::generators as gs;

/// Any sample, at either depth: rounding alone is the nearest step, dither lands within a
/// step and a half of it, both stay in the depth's range, and the same samples always round
/// the same way.
#[hegel::test(test_cases = 300)]
fn samples_round_to_a_step_near_them(tc: TestCase) {
    let depth = tc.draw(gs::sampled_from(vec![16u16, 24]));
    let samples = tc.draw(gs::vecs(gs::floats::<f64>().min_value(-1.5).max_value(1.5)).min_size(1).max_size(50));
    let scale = f64::from(1u32 << (depth - 1));
    let (mut plain, mut dithered, mut again) =
        (Quantizer::new(depth, Dither::None), Quantizer::new(depth, Dither::Tpdf), Quantizer::new(depth, Dither::Tpdf));
    for s in samples {
        let exact = (s * scale).clamp(-scale, scale - 1.0);
        let q = plain.quantize(s);
        assert_eq!(f64::from(q), (s * scale).round().clamp(-scale, scale - 1.0), "{s}");
        let d = dithered.quantize(s);
        assert!((f64::from(d) - exact).abs() <= 1.5 && (-scale..scale).contains(&f64::from(d)), "{s}: {d}");
        assert_eq!(again.quantize(s), d);
    }
}
