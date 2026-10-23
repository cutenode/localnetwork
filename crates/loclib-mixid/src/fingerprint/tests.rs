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

/// Frames pool into the columns their middles fall in, each column the loudest of its frames,
/// and each series is scaled on its own: its loudest column is full height whatever the others',
/// bands on a curve that lifts quiet ones.
#[test]
fn columns_pool_frames_and_scale_each_series() {
    let frame = |peak, bands| FrameLevels { peak, bands };
    let levels =
        [frame(0.1, [0.2, 0.0, 1.0]), frame(0.5, [0.4, 0.0, 1.0]), frame(0.25, [0.1, 0.0, 1.0]), frame(0.2, [0.0; 3])];
    let two_frames_a_column = columns(&levels, FRAMES_PER_SEC / 2.0);
    let expected =
        LevelColumns { peak: vec![51, 255, 102], bands: [vec![146, 255, 0], vec![0, 0, 0], vec![255, 255, 0]] };
    assert_eq!(two_frames_a_column, expected);
    assert_eq!(columns(&[], 4.0), LevelColumns::default());
    // Frame 10's hop is centred 255 ms in: the second quarter second.
    let mut late = [FrameLevels::default(); 11];
    late[10].peak = 1.0;
    assert_eq!(columns(&late, 4.0).peak, [0, 255]);
}

/// Any frames at any rate up to two frames a column: a column for each stretch the frames'
/// middles reach, each the loudest of the frames whose middle falls in it, each series' loudest
/// full height (or none, for silence), louder never drawn lower, and the same frames at another
/// level drawn the same, give or take rounding.
#[hegel::test(test_cases = 200)]
fn columns_scale_each_series_to_its_loudest(tc: TestCase) {
    let level = || hegel::one_of!(gs::just(0.0f32), gs::floats::<f32>().min_value(1e-3).max_value(1.0));
    let frames: Vec<(f32, [f32; 3])> =
        tc.draw(gs::vecs(gs::tuples!(level(), gs::arrays::<_, _, 3>(level()))).max_size(400));
    let levels: Vec<FrameLevels> = frames.iter().map(|&(peak, bands)| FrameLevels { peak, bands }).collect();
    let per_sec = tc.draw(gs::floats::<f64>().min_value(0.5).max_value(FRAMES_PER_SEC / 2.0));
    let of = |l: &FrameLevels| [l.peak, l.bands[0], l.bands[1], l.bands[2]];
    // Frame i's middle is (i + 1) frames in.
    let at = |i: usize| ((i + 1) as f64 / FRAMES_PER_SEC * per_sec) as usize;
    let n = levels.len().checked_sub(1).map_or(0, |last| at(last) + 1);
    let mut loudest = vec![[0f32; 4]; n];
    for (i, l) in levels.iter().enumerate() {
        for (m, v) in loudest[at(i)].iter_mut().zip(of(l)) {
            *m = m.max(v);
        }
    }
    let c = columns(&levels, per_sec);
    let drawn = [&c.peak, &c.bands[0], &c.bands[1], &c.bands[2]];
    for (s, series) in drawn.iter().enumerate() {
        assert_eq!(series.len(), n, "series {s}");
        let top = loudest.iter().map(|m| m[s]).fold(0.0f32, f32::max);
        assert_eq!(series.iter().copied().max().unwrap_or(0), if top > 0.0 { 255 } else { 0 }, "series {s}");
        for (a, b) in (0..n).flat_map(|a| (0..n).map(move |b| (a, b))) {
            assert!(loudest[a][s] > loudest[b][s] || series[a] <= series[b], "series {s}: {a} drawn above {b}");
        }
    }
    let k = tc.draw(gs::floats::<f32>().min_value(0.01).max_value(100.0));
    let louder: Vec<FrameLevels> =
        levels.iter().map(|l| FrameLevels { peak: l.peak * k, bands: l.bands.map(|b| b * k) }).collect();
    let again = columns(&louder, per_sec);
    for (a, b) in drawn.iter().zip([&again.peak, &again.bands[0], &again.bands[1], &again.bands[2]]) {
        assert!(a.iter().zip(b).all(|(x, y)| x.abs_diff(*y) <= 1), "{a:?} at {k}x is {b:?}");
    }
}

/// A rate of no columns, a negative or not-a-number one, or one above two frames a column
/// (which would leave columns no frame falls in) is refused.
#[hegel::test(test_cases = 50)]
fn columns_refuse_rates_that_leave_columns_empty(tc: TestCase) {
    let per_sec = tc.draw(hegel::one_of!(
        gs::just(0.0f64),
        gs::just(f64::NAN),
        gs::floats::<f64>().max_value(-1e-9),
        gs::floats::<f64>().min_value(FRAMES_PER_SEC / 2.0 + 1e-9),
    ));
    let levels = [FrameLevels::default(); 4];
    assert!(std::panic::catch_unwind(|| columns(&levels, per_sec)).is_err(), "{per_sec}");
}

/// Scaled together, a recording's bands keep their ratios (one 20 dB under another draws a
/// tenth as high) and its loudest is full height; a near-silent one stays near 0.
#[test]
fn bands_scaled_together_keep_their_ratios() {
    let (lows, highs) = ([0.5f32; 8], [0.05f32; 8]);
    let [low, high] = scaled([&lows, &highs], Scale::Together { floor: 0.01 }, [1.0; 2]);
    assert_eq!((low[0], high[0]), (255, 26));
    let dither = [3e-5f32; 8];
    assert_eq!(scaled([&dither], Scale::Together { floor: 0.01 }, [1.0]), [vec![1; 8]]);
}

/// Any levels scaled together: louder is never drawn lower, whichever series it's in; the
/// loudest is full height when it's over the floor, and under it as high as it is against it.
#[hegel::test(test_cases = 200)]
fn levels_scaled_together_are_drawn_against_one_another(tc: TestCase) {
    let n = tc.draw(gs::integers::<usize>().max_value(200));
    let level = || gs::vecs(gs::floats::<f32>().min_value(0.0).max_value(1.0)).min_size(n).max_size(n);
    let series: [Vec<f32>; 3] = [tc.draw(level()), tc.draw(level()), tc.draw(level())];
    let floor = tc.draw(gs::floats::<f32>().min_value(1e-4).max_value(0.5));
    let drawn = scaled(series.each_ref().map(Vec::as_slice), Scale::Together { floor }, [1.0; 3]);
    let all: Vec<(f32, u8)> =
        series.iter().zip(&drawn).flat_map(|(s, d)| s.iter().copied().zip(d.iter().copied())).collect();
    for (a, b) in all.iter().flat_map(|a| all.iter().map(move |b| (a, b))) {
        assert!(a.0 > b.0 || a.1 <= b.1, "{a:?} drawn above {b:?}");
    }
    let loudest = all.iter().map(|(v, _)| *v).fold(0.0f32, f32::max);
    let highest = all.iter().map(|(_, d)| *d).max().unwrap_or(0);
    match loudest <= floor {
        true => assert_eq!(highest, (loudest / floor * 255.0).round() as u8, "under the floor"),
        false => assert_eq!(highest, 255),
    }
}
