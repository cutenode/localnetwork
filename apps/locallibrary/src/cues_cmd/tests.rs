use super::*;

#[test]
fn times() {
    assert_eq!(parse_time("62.5"), Ok(62_500));
    assert_eq!(parse_time("1:02.5"), Ok(62_500));
    assert_eq!(parse_time("1:02:03.5"), Ok(3_723_500));
    assert_eq!(parse_time("0:00.0004"), Ok(0));
    assert!(parse_time("1:75").is_err());
    assert!(parse_time("a:10").is_err());
    assert!(parse_time("1::2").is_err());
    assert!(parse_time("-3").is_err());
}

/// A cue's position is milliseconds in 32 bits: about 1193 hours.
#[test]
fn a_time_past_what_a_cue_holds_is_refused() {
    assert_eq!(parse_time("1193:02:47.295"), Ok(u32::MAX));
    assert!(parse_time("1193:02:47.296").is_err());
}
