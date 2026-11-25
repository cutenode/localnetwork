use super::*;

/// `secs` of a four-to-the-floor loop at `bpm`: a kick (a falling sine under 2 kHz) on every beat
/// from `first` seconds, and a hat (noise differenced thrice, so mostly over 5 kHz) on every
/// off-beat, sharper than the kick.
fn kicks_and_hats(bpm: f64, first: f64, secs: f64) -> Vec<f32> {
    let n = (secs * f64::from(RATE)) as usize;
    let mut out = vec![0f32; n];
    let period = 60.0 / bpm;
    let mut noise = 0x2545_f491_u32;
    let mut beat = first;
    while beat < secs {
        let start = (beat * f64::from(RATE)) as usize;
        for (j, s) in out.iter_mut().skip(start).take(RATE as usize / 8).enumerate() {
            let t = j as f32 / RATE as f32;
            *s += 0.5 * (std::f32::consts::TAU * (55.0 + 100.0 * (-t * 40.0).exp()) * t).sin() * (-t * 20.0).exp();
        }
        let hat = ((beat + period / 2.0) * f64::from(RATE)) as usize;
        let mut diffs = [0f32; 3];
        for (j, s) in out.iter_mut().skip(hat).take(RATE as usize / 40).enumerate() {
            noise ^= noise << 13;
            noise ^= noise >> 17;
            noise ^= noise << 5;
            let mut x = noise as f32 / u32::MAX as f32 - 0.5;
            for d in &mut diffs {
                (x, *d) = (x - *d, x);
            }
            *s += 0.4 * x * (-(j as f32) / 200.0).exp();
        }
        beat += period;
    }
    out
}

#[test]
fn a_loop_gets_its_tempo_and_its_kicks() {
    for (bpm, first) in [(128.0, 0.3), (174.0, 1.1), (123.45, 0.05)] {
        let grid = grid(&kicks_and_hats(bpm, first, 30.0), 0.0).unwrap();
        assert!((grid.bpm - bpm).abs() < 0.015, "{bpm}: {grid:?}");
        // On the kicks, not the hats, give or take the window and the median shift.
        let off = (grid.first_beat_secs - first).rem_euclid(grid.period_secs());
        let off = off.min(grid.period_secs() - off);
        assert!(off < 0.025, "{bpm}: {grid:?} is {off} s off the kicks");
    }
}

#[test]
fn a_lead_in_moves_the_beats_later() {
    let audio = kicks_and_hats(128.0, 0.3, 12.0);
    let (plain, later) = (grid(&audio, 0.0).unwrap(), grid(&audio, 0.026).unwrap());
    let moved = (later.first_beat_secs - plain.first_beat_secs).rem_euclid(plain.period_secs());
    assert!((moved - 0.026).abs() < 1e-9);
}

#[test]
fn silence_and_blips_have_none() {
    assert_eq!(grid(&vec![0.0; RATE as usize * 10], 0.0), None);
    assert_eq!(grid(&kicks_and_hats(128.0, 0.0, 2.0), 0.0), None);
    assert_eq!(grid(&[], 0.0), None);
}

#[test]
fn beats_count_bars_from_the_first() {
    let grid = Grid { bpm: 120.0, first_beat_secs: 0.25 };
    let beats = grid.beats(3.0);
    assert_eq!(beats.len(), 6);
    assert_eq!(beats.iter().map(|b| b.beat_number).collect::<Vec<_>>(), [1, 2, 3, 4, 1, 2]);
    assert_eq!(beats.iter().map(|b| b.time_ms).collect::<Vec<_>>(), [250, 750, 1250, 1750, 2250, 2750]);
    assert!(beats.iter().all(|b| b.bpm == 120.0));
}
