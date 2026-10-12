use super::*;

/// 44.1 kHz as AIFF writers spell it (Apple's own files hold these bytes).
#[test]
fn sample_rates_as_extended_floats() {
    assert_eq!(extended(44_100), [0x40, 0x0E, 0xAC, 0x44, 0, 0, 0, 0, 0, 0]);
    assert_eq!(extended(48_000), [0x40, 0x0E, 0xBB, 0x80, 0, 0, 0, 0, 0, 0]);
}
