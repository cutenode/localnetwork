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
    assert_eq!(estimate(&[]), None);
    assert_eq!(estimate(&[0.0; 44_100]), None);
    assert_eq!(estimate(&[0.0, 0.5, -0.5]).map(|e| e.bpm), Some(0.0));
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
