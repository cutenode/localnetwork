use super::*;

fn grid() -> Vec<Beat> {
    (0..8).map(|i| Beat { beat_number: (i % 4 + 1) as u16, bpm: 120.0, time_ms: 1000 + i * 500 }).collect()
}

#[test]
fn positions_snap_to_the_grid() {
    let beats = grid();
    assert_eq!(resolve(CuePosition::Beat(3), false, &beats).unwrap(), (2000, Some(2)));
    assert_eq!(resolve(CuePosition::Ms(2240), true, &beats).unwrap(), (2000, Some(2)));
    assert_eq!(resolve(CuePosition::Ms(2240), false, &beats).unwrap(), (2240, None));
    assert!(resolve(CuePosition::Beat(9), false, &beats).is_err());
    assert!(resolve(CuePosition::Beat(1), false, &[]).is_err());
}

#[test]
fn loops_end_on_the_grid_or_at_the_tempo() {
    let beats = grid();
    assert_eq!(loop_end(1000, Some(0), beat_fraction(4.0).unwrap(), &beats, 0.0).unwrap(), 3000);
    // Past the end of the grid, and off it: 120 BPM is 500 ms a beat.
    assert_eq!(loop_end(4000, Some(6), (4, 1), &beats, 0.0).unwrap(), 6000);
    assert_eq!(loop_end(1100, None, beat_fraction(0.5).unwrap(), &beats, 0.0).unwrap(), 1350);
    assert_eq!(beat_fraction(0.25).unwrap(), (1, 4));
    assert!(beat_fraction(3.5).is_err() && beat_fraction(0.3).is_err() && beat_fraction(0.0).is_err());
}

#[test]
fn colours() {
    let mut hot = CuePoint { hot_cue: Some(0), ..Default::default() };
    set_color(&mut hot, "red").unwrap();
    assert_eq!((hot.color_code, hot.color_rgb), (0x2a, [0xe6, 0x28, 0x28]));
    set_color(&mut hot, "#00e1fe").unwrap();
    assert_eq!(hot.color_code, 0x09);
    assert_eq!(color_label(&hot).as_deref(), Some("#00e0ff"));
    set_color(&mut hot, "none").unwrap();
    assert_eq!((hot.color_code, hot.color_rgb, color_label(&hot)), (0, DEFAULT_HOT_CUE_RGB, None));
    let mut memory = CuePoint::default();
    set_color(&mut memory, "cyan").unwrap();
    assert_eq!(memory.color_id, TrackColor::Aqua.id());
    set_color(&mut memory, "#ff1010").unwrap();
    assert_eq!(color_label(&memory).as_deref(), Some("red"));
    assert!(set_color(&mut memory, "plaid").is_err());
    assert_eq!("b".parse::<HotCue>().unwrap(), HotCue(1));
    assert!("I".parse::<HotCue>().is_err());
    assert_eq!(clock(62_500), "1:02.500");
}
