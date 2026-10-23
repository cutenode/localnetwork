use super::*;
use crate::test_util::synth;

fn analyse(x: &[f32], d: Density) -> Vec<Peak> {
    let mut a = Analyzer::new(d, false);
    for c in x.chunks(777) {
        a.push(c);
    }
    a.finish().0
}

#[test]
fn peak_density_is_in_range() {
    let x = synth(20.0, 1);
    let lib = analyse(&x, LIBRARY).len() as f64 / 20.0;
    let query = analyse(&x, QUERY).len() as f64 / 20.0;
    assert!((5.0..60.0).contains(&lib), "library {lib}/s");
    assert!(query > lib, "query {query}/s vs library {lib}/s");
}

#[test]
fn same_audio_shares_hashes() {
    let x = synth(15.0, 7);
    let a: rustc_hash::FxHashSet<u32> = landmarks(&analyse(&x, LIBRARY), 3).iter().map(Landmark::hash).collect();
    // Same audio, quieter and offset by a few samples: most hashes survive.
    let y: Vec<f32> = std::iter::repeat_n(0.0, 37).chain(x.iter().map(|s| s * 0.5)).collect();
    let b = landmarks(&analyse(&y, QUERY), 8);
    let hits = b.iter().filter(|l| a.contains(&l.hash())).count();
    assert!(hits * 3 > a.len(), "{hits} of {} hashes", a.len());
}

#[test]
fn variants_undo_speed_changes() {
    let lm = Landmark { t: 100, f1: 100, f2: 120, dt: 20 };
    let faster = Landmark { t: 95, f1: 105, f2: 126, dt: 19 };
    let v = Variant { ratio: 1.05, pitch: true };
    assert_eq!(v.original_hash(&faster), Some(lm.hash()));
    let stretched = Landmark { dt: 19, ..lm };
    assert_eq!(Variant { ratio: 1.05, pitch: false }.original_hash(&stretched), Some(lm.hash()));
    let vs = variants(0.08);
    assert!(vs.iter().any(|v| (v.ratio - 1.08).abs() < 0.004 && v.pitch));
    assert!(vs.iter().all(|v| (0.9..1.1).contains(&v.ratio)));
}

use crate::index::tests::sorted_peaks;
use hegel::TestCase;
use hegel::generators as gs;

/// Each peak is paired with the first `fanout` later peaks in its target zone, and nothing
/// else: every landmark joins two peaks 2 to 62 frames and at most 31 bins apart.
#[hegel::test(test_cases = 300)]
fn landmarks_pair_peaks_in_their_target_zone(tc: TestCase) {
    let peaks = sorted_peaks(&tc, 8, 80);
    let fanout = tc.draw(gs::integers::<usize>().min_value(1).max_value(8));
    let expected: Vec<Landmark> = peaks
        .iter()
        .enumerate()
        .flat_map(|(i, a)| {
            peaks[i + 1..]
                .iter()
                .take_while(|b| b.t - a.t < MAX_DT)
                .filter(|b| b.t - a.t >= MIN_DT && (b.f as i32 - a.f as i32).abs() <= MAX_DF)
                .take(fanout)
                .map(|b| Landmark { t: a.t, f1: a.f, f2: b.f, dt: (b.t - a.t) as u8 })
        })
        .collect();
    let found = landmarks(&peaks, fanout);
    assert_eq!(found, expected);
    assert!(found.iter().all(|lm| lm.hash() < 1 << HASH_BITS));
}

/// The speeds searched start at the original, come in reciprocal pairs, cover the range and
/// stay within a step of it, for key-locked and pitched playback alike.
#[hegel::test(test_cases = 200)]
fn variants_cover_the_range(tc: TestCase) {
    let range = tc.draw(gs::floats::<f64>().min_value(0.0).max_value(0.25));
    let vs = variants(range);
    assert_eq!(vs[0], Variant { ratio: 1.0, pitch: false });
    for (pitch, step) in [(false, 0.012f64), (true, 0.0038)] {
        let grid: Vec<f64> = vs.iter().filter(|v| v.pitch == pitch && v.ratio != 1.0).map(|v| v.ratio).collect();
        for r in &grid {
            assert!(grid.iter().any(|q| (q * r - 1.0).abs() < 1e-9), "{r} without its reciprocal");
            assert!(r.ln().abs() <= (1.0 + range).ln() + step.ln_1p() + 1e-9, "{r} beyond ±{range}");
        }
        let speed = tc.draw(gs::floats::<f64>().min_value(-1.0).max_value(1.0)) * (1.0 + range).ln();
        let nearest = grid.iter().chain([&1.0]).map(|r| (r.ln() - speed).abs()).fold(f64::MAX, f64::min);
        assert!(nearest <= step.ln_1p() / 2.0 + 1e-9, "{} is {nearest} from the grid", speed.exp());
    }
}

/// A landmark heard at any speed within ±8% with key lock (its gap stretched, then rounded to
/// a frame) maps back to the original's hash through one of the key-locked speeds searched;
/// at the original speed, any landmark is its own hash.
#[hegel::test(test_cases = 500)]
fn key_locked_speeds_undo_a_stretch(tc: TestCase) {
    let f1 = tc.draw(gs::integers::<u8>().min_value(MIN_BIN as u8));
    let df = tc.draw(gs::integers::<i32>().min_value(-MAX_DF).max_value(MAX_DF));
    let f2 = (f1 as i32 + df).clamp(0, 255) as u8;
    let dt = tc.draw(gs::integers::<u8>().min_value(MIN_DT as u8).max_value(MAX_DT as u8 - 1));
    let original = Landmark { t: 0, f1, f2, dt };
    assert_eq!(Variant { ratio: 1.0, pitch: tc.draw(gs::booleans()) }.original_hash(&original), Some(original.hash()));
    let speed = tc.draw(gs::floats::<f64>().min_value(1.0 / 1.08).max_value(1.08));
    let heard = Landmark { dt: (dt as f64 / speed).round() as u8, ..original };
    let undone = variants(0.08).iter().filter(|v| !v.pitch).any(|v| v.original_hash(&heard) == Some(original.hash()));
    assert!(undone, "{heard:?} at {speed}");
}

/// Peaks don't depend on how the samples arrive: pushed all at once or in any chunks, the
/// same peaks (in frame and bin order, in the bins searched) and one level per frame.
#[hegel::test(test_cases = 40)]
fn peaks_dont_depend_on_how_samples_arrive(tc: TestCase) {
    let secs = tc.draw(gs::floats::<f64>().min_value(0.0).max_value(2.5));
    let x = crate::test_util::synth(secs, tc.draw(gs::integers::<u32>()));
    let whole = {
        let mut a = Analyzer::new(QUERY, true);
        a.push(&x);
        a.finish()
    };
    let sizes = tc.draw(gs::vecs(gs::integers::<usize>().min_value(1).max_value(3000)).min_size(1).max_size(8));
    let mut a = Analyzer::new(QUERY, true);
    let (mut at, mut i) = (0, 0);
    while at < x.len() {
        let n = sizes[i % sizes.len()].min(x.len() - at);
        a.push(&x[at..at + n]);
        (at, i) = (at + n, i + 1);
    }
    assert_eq!(a.frames() as usize, x.len().saturating_sub(FFT_LEN) / HOP + usize::from(x.len() >= FFT_LEN));
    let (peaks, levels) = a.finish();
    assert_eq!(peaks, whole.0);
    assert!(peaks.is_sorted());
    assert!(peaks.iter().all(|p| (MIN_BIN..BINS - 1).contains(&(p.f as usize)) && (p.t as usize) < levels.len()));
    let loudest = x.iter().fold(0.0f32, |m, s| m.max(s.abs()));
    assert_eq!(levels.len(), whole.1.len());
    assert!(levels.iter().all(|l| l.peak <= loudest && l.bands.iter().all(|b| *b >= 0.0)));
}

/// Frames pool into columns, each the loudest of its frames, and each series is scaled on its
/// own: its loudest column is full height whatever the others', bands on a curve that lifts
/// quiet ones.
#[test]
fn columns_pool_frames_and_scale_each_series() {
    let frame = |peak, bands| FrameLevels { peak, bands };
    let levels =
        [frame(0.1, [0.2, 0.0, 1.0]), frame(0.5, [0.4, 0.0, 1.0]), frame(0.25, [0.1, 0.0, 1.0]), frame(0.2, [0.0; 3])];
    let two_frames_a_column = columns(&levels, FRAMES_PER_SEC / 2.0);
    let expected = LevelColumns { peak: vec![255, 128], bands: [vec![255, 84], vec![0, 0], vec![255, 255]] };
    assert_eq!(two_frames_a_column, expected);
    assert_eq!(columns(&levels, FRAMES_PER_SEC).peak, [51, 255, 128, 102], "a column a frame");
    assert_eq!(columns(&[], FRAMES_PER_SEC), LevelColumns::default());
}

/// Any frames at any rate up to a column a frame: as many columns as the frames last, each
/// series' loudest full height (or none, for silence), louder never drawn lower, and the same
/// frames at another level drawn the same, give or take rounding.
#[hegel::test(test_cases = 200)]
fn columns_scale_each_series_to_its_loudest(tc: TestCase) {
    let level = || hegel::one_of!(gs::just(0.0f32), gs::floats::<f32>().min_value(1e-3).max_value(1.0));
    let frames: Vec<(f32, [f32; 3])> =
        tc.draw(gs::vecs(gs::tuples!(level(), gs::arrays::<_, _, 3>(level()))).max_size(400));
    let levels: Vec<FrameLevels> = frames.iter().map(|&(peak, bands)| FrameLevels { peak, bands }).collect();
    let per_sec = tc.draw(gs::floats::<f64>().min_value(0.5).max_value(FRAMES_PER_SEC));
    let pooled = columns(&levels, per_sec);
    let n = levels.len() as f64 * per_sec / FRAMES_PER_SEC;
    let series = |c: &LevelColumns| [c.peak.clone(), c.bands[0].clone(), c.bands[1].clone(), c.bands[2].clone()];
    let of = |l: &FrameLevels| [l.peak, l.bands[0], l.bands[1], l.bands[2]];
    for (s, drawn) in series(&pooled).iter().enumerate() {
        assert!((drawn.len() as f64 - n).abs() < 1.0 + 1e-9, "{} columns for {n}", drawn.len());
        let loudest = levels.iter().map(|l| of(l)[s]).fold(0.0f32, f32::max);
        assert_eq!(drawn.iter().copied().max().unwrap_or(0), if loudest > 0.0 { 255 } else { 0 }, "series {s}");
    }
    for (s, drawn) in series(&columns(&levels, FRAMES_PER_SEC)).iter().enumerate() {
        assert_eq!(drawn.len(), levels.len(), "a column a frame");
        let frames: Vec<(f32, u8)> = levels.iter().map(|l| of(l)[s]).zip(drawn.iter().copied()).collect();
        for (a, b) in frames.iter().flat_map(|a| frames.iter().map(move |b| (a, b))) {
            assert!(a.0 > b.0 || a.1 <= b.1, "series {s}: {a:?} drawn above {b:?}");
        }
    }
    let k = tc.draw(gs::floats::<f32>().min_value(0.01).max_value(100.0));
    let scaled: Vec<FrameLevels> =
        levels.iter().map(|l| FrameLevels { peak: l.peak * k, bands: l.bands.map(|b| b * k) }).collect();
    for (a, b) in series(&pooled).iter().zip(series(&columns(&scaled, per_sec))) {
        assert!(a.iter().zip(&b).all(|(x, y)| x.abs_diff(*y) <= 1), "{a:?} at {k}x is {b:?}");
    }
}
