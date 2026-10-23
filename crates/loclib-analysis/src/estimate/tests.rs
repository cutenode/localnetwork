use super::*;

#[test]
fn keys_map_by_tonic_and_mode() {
    assert_eq!(camelot(Key::Minor(9)).code(), "8A");
    assert_eq!(camelot(Key::Major(0)).code(), "8B");
    assert_eq!(camelot(Key::Major(6)).note(), "F#");
}

/// stratum-dsp refuses what it finds no sound in (empty, or digital silence): no estimate. A
/// blip too short for a tempo has one, with no tempo.
#[test]
fn silence_has_no_estimate() {
    assert_eq!(estimate(vec![]), None);
    assert_eq!(estimate(vec![0.0; 44_100]), None);
    assert_eq!(estimate(vec![0.0, 0.5, -0.5]).map(|e| (e.bpm, e.key)), Some((0.0, None)));
}

/// Every key stratum-dsp names is the Camelot key on that tonic and in that mode, and a minor
/// key is the relative of the major three semitones above it.
#[hegel::test(test_cases = 50)]
fn every_key_lands_on_its_camelot_key(tc: hegel::TestCase) {
    let pitch = tc.draw(hegel::generators::integers::<u32>().max_value(11));
    let (major, minor) = (camelot(Key::Major(pitch)), camelot(Key::Minor(pitch)));
    assert_eq!((major.pitch(), major.major), (pitch, true));
    assert_eq!((minor.pitch(), minor.major), (pitch, false));
    assert_eq!(minor.relative(), camelot(Key::Major((pitch + 3) % 12)));
}

/// Under 12 seconds, stratum-dsp scores keys once over the whole track, and its best major and
/// best minor key always tie there (each mode's scores are scaled to its best): it picks one in
/// the order of a `HashMap`, which differs from process to process, A minor one run and A major
/// the next. A tie is no key, so the estimate is the same every time.
#[test]
fn tied_keys_are_none() {
    let a_minor: Vec<f32> = (0..RATE as usize * 6)
        .map(|i| {
            let t = i as f32 / RATE as f32;
            [220.0f32, 261.63, 329.63].iter().map(|f| (std::f32::consts::TAU * f * t).sin()).sum::<f32>() / 10.0
        })
        .collect();
    let e = estimate(a_minor).unwrap();
    assert_eq!((e.key, e.key_confidence), (None, 0.0), "{e:?}");
}
