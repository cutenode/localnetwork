use super::*;

/// Feed a sine through the analyser's spectrum and return its bands' levels.
fn levels(freq: f32, bands: usize) -> Vec<f32> {
    let mut a = Analyzer::default();
    let sr = 44_100.0;
    for (i, s) in a.samples.iter_mut().enumerate() {
        *s = 0.5 * (2.0 * std::f32::consts::PI * freq * i as f32 / sr).sin();
    }
    a.transform();
    a.band_levels(bands)
}

/// Feed a sine through the analyser's spectrum and return the loudest band.
fn loudest_band(freq: f32, bands: usize) -> (usize, f32) {
    let out = levels(freq, bands);
    out.iter().copied().enumerate().fold((0, 0.0), |m, (i, v)| if v > m.1 { (i, v) } else { m })
}

#[test]
fn sines_land_in_the_right_band() {
    let bands = 32;
    let ratio = (MAX_HZ / MIN_HZ).powf(1.0 / bands as f32);
    for freq in [60.0, 440.0, 1000.0, 5000.0] {
        let (band, level) = loudest_band(freq, bands);
        let expected = ((freq / MIN_HZ).ln() / ratio.ln()).floor() as usize;
        assert!(band.abs_diff(expected) <= 1, "{freq} Hz -> band {band}, expected {expected}");
        assert!(level > 0.5, "{freq} Hz too quiet: {level}");
    }
}

#[test]
fn silence_decays_to_idle() {
    let full = Bands { levels: vec![1.0; 8], peaks: vec![(1.0, 0.0); 8] };
    let mut a = Analyzer { sets: vec![full], level: 1.0, ..Default::default() };
    let tap = Tap::default();
    for _ in 0..200 {
        a.update(&tap, false, &[8], 1.0 / 30.0);
    }
    assert!(a.is_idle());
}

#[test]
fn keeps_one_set_of_bands_per_spectrum_on_screen() {
    let mut a = Analyzer::default();
    let tap = Tap::default();
    a.update(&tap, false, &[24, 48, 24], 0.016);
    assert_eq!(a.sets.len(), 2);
    assert_eq!(a.bands(48).map(|b| b.levels.len()), Some(48));
    a.update(&tap, false, &[12], 0.016);
    assert!(a.bands(48).is_none() && a.bands(12).is_some());
}

/// With many bands, those at the bass end are narrower than one bin of the transform: the
/// band a low note is in still shows it.
#[test]
fn bass_notes_show_in_bands_narrower_than_a_bin() {
    let bands = 160;
    let ratio = (MAX_HZ / MIN_HZ).powf(1.0 / bands as f32);
    let band = ((45.0 / MIN_HZ).ln() / ratio.ln()).floor() as usize;
    let (lo, hi) = (MIN_HZ * ratio.powi(band as i32), MIN_HZ * ratio.powi(band as i32 + 1));
    let bin_hz = 44_100.0 / FFT_SIZE as f32;
    assert_eq!((lo / bin_hz).floor() + 1.0, (hi / bin_hz).ceil(), "within one bin");
    let level = levels(45.0, bands)[band];
    assert!(level > 0.5, "45 Hz in band {band} ({lo}-{hi} Hz): {level}");
}

/// A band jumps most of the way to a louder level at once, and its peak cap goes with it.
#[test]
fn bands_rise_fast() {
    let mut b = Bands::new(1);
    b.approach(&[1.0], 1.0 / 30.0);
    assert!(b.levels[0] > 0.5 && b.levels[0] < 1.0, "{}", b.levels[0]);
    assert_eq!(b.peaks[0], (b.levels[0], 0.0));
}
