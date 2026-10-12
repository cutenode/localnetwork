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

/// An analysis file's `.EXT` is the same path ending `.EXT` instead of `.DAT` (in any case); a
/// path that doesn't end `.DAT` has none.
#[hegel::test(test_cases = 300)]
#[hegel::explicit_test_case(stem = "aé".to_string(), end = "xyz")]
#[hegel::explicit_test_case(stem = "é".to_string(), end = "")]
#[hegel::explicit_test_case(stem = "PIONEER/USBANLZ/P001/ANLZ0000".to_string(), end = ".dAt")]
fn an_analysis_files_ext_is_its_dat_renamed(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let stem = tc.draw(gs::text().max_size(8));
    let end = tc.draw(gs::sampled_from(vec![".DAT", ".dat", ".EXT", "DAT", ".DA", "", "é.DAT", "xyz"]));
    let path = format!("{stem}{end}");
    let want = path.to_ascii_uppercase().ends_with(".DAT").then(|| format!("{}.EXT", &path[..path.len() - 4]));
    assert_eq!(ext_key(&path), want, "{path:?}");
}

/// `#rrggbb` gives a cue a colour exactly when all six are hex digits, for hot and memory cues.
#[hegel::test(test_cases = 300)]
fn hex_colours_need_six_hex_digits(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let digits = tc.draw(gs::text().alphabet("0123456789abcdefABCDEFgz ").min_size(6).max_size(6));
    let spec = format!("#{digits}");
    let hex = digits.chars().all(|c| c.is_ascii_hexdigit());
    for hot in [Some(0), None] {
        let mut c = CuePoint { hot_cue: hot, ..Default::default() };
        assert_eq!(set_color(&mut c, &spec).is_ok(), hex, "{spec:?} on {hot:?}");
    }
}

/// A hot cue shows its palette colour when its code is in the palette, otherwise the colour it
/// stores unless that's none (black) or the default; it shows as an active loop only when it's a
/// loop.
#[hegel::test(test_cases = 300)]
#[hegel::explicit_test_case(code = 200u8, r = 1u8, g = 2u8, b = 3u8, end = Some(5_000u32), active = true)]
fn a_hot_cues_colour_is_its_palettes_or_its_own(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let code = tc.draw(gs::integers::<u8>());
    let r = tc.draw(gs::integers::<u8>());
    let g = tc.draw(gs::integers::<u8>());
    let b = tc.draw(gs::integers::<u8>());
    let rgb = [r, g, b];
    let end = tc.draw(gs::optional(gs::integers::<u32>().min_value(2_000).max_value(9_000)));
    let active = tc.draw(gs::booleans());
    let c = CuePoint {
        hot_cue: Some(0),
        start_ms: 1_000,
        end_ms: end,
        color_code: code,
        color_rgb: rgb,
        active_loop: active,
        ..Default::default()
    };
    let shown = info(&c, &grid());
    let hex = |[r, g, b]: [u8; 3]| format!("#{r:02x}{g:02x}{b:02x}");
    let want = match anlz::hot_cue_palette(code) {
        Some(p) => Some(hex(p)),
        None if rgb != [0; 3] && rgb != DEFAULT_HOT_CUE_RGB => Some(hex(rgb)),
        None => None,
    };
    assert_eq!(shown.color, want);
    assert_eq!(shown.active_loop, active && end.is_some());
    assert_eq!(shown.beat, Some(1));
}
