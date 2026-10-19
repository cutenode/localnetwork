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

/// Any time a cue can hold, written as seconds, minutes:seconds or hours:minutes:seconds (with
/// up to three decimals, and spaces around), reads back as its milliseconds; seconds past 59 after
/// a colon, an empty part, more than three parts, a part that isn't a number or a time past what a
/// cue holds don't read, and the error quotes what was given.
#[hegel::test(test_cases = 300)]
fn times_read_back_however_they_are_written(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let ms = tc.draw(gs::integers::<u32>());
    let (h, m, s, frac) = (ms / 3_600_000, ms / 60_000 % 60, ms / 1000 % 60, ms % 1000);
    let written = match tc.draw(gs::integers::<u8>().max_value(2)) {
        0 => format!("{}.{frac:03}", ms / 1000),
        1 => format!("{}:{s:02}.{frac:03}", ms / 60_000),
        _ => format!("{h}:{m:02}:{s:02}.{frac:03}"),
    };
    let padded = if tc.draw(gs::booleans()) { format!(" {written} ") } else { written.clone() };
    assert_eq!(parse_time(&padded), Ok(ms), "{padded:?}");
    let bad = match tc.draw(gs::integers::<u8>().max_value(5)) {
        0 => format!("{}:{}", ms / 60_000, 60 + s),
        1 => format!("{written}:"),
        2 => format!("1:{written}:00:00"),
        3 => format!("{}.5", u64::from(u32::MAX) / 1000 + 1),
        // Seconds, or minutes, that aren't a number.
        4 => format!("{written}s"),
        _ => format!("{}m:{s:02}", ms / 60_000),
    };
    let refused = parse_time(&bad).expect_err(&bad);
    assert!(refused.starts_with("not a time: ") && refused.contains(&format!("{bad:?}")), "{refused}");
}

/// A cue's line shows its slot (or a memory cue's mark) and time, then each thing it has and
/// nothing it hasn't: the beat, a loop's end and length (and that it's the active loop), its
/// name and its colour. Which cue an edit names is the hot cue if one is given, else the memory
/// cue at a time, else the one on a beat. A track's cues are listed with a warning when the
/// OneLibrary database has a different number of them.
#[hegel::test(test_cases = 200)]
fn a_cues_line_shows_what_it_has(tc: hegel::TestCase) {
    use hegel::generators as gs;
    use locallibrary::model::TrackRef;
    let maybe = |tc: &hegel::TestCase| tc.draw(gs::booleans());
    let c = CueInfo {
        hot_cue: maybe(&tc).then(|| HotCue(tc.draw(gs::integers::<u8>().max_value(7))).to_string()),
        start_ms: tc.draw(gs::integers::<u32>().max_value(3_600_000)),
        end_ms: maybe(&tc).then(|| tc.draw(gs::integers::<u32>().max_value(3_600_000))),
        beat: maybe(&tc).then(|| tc.draw(gs::integers::<u32>().max_value(2000))),
        loop_beats: maybe(&tc).then(|| tc.draw(gs::sampled_from(&["1", "8", "1/2"][..])).to_string()),
        name: tc.draw(gs::sampled_from(&["", "Drop", "Ünïcödé"][..])).to_string(),
        color: maybe(&tc).then(|| "#e62828".to_string()),
        active_loop: maybe(&tc),
    };
    let line = console::strip_ansi_codes(&cue_line(&c)).into_owned();
    let start = match &c.hot_cue {
        Some(h) => format!("{h}  {}", clock(c.start_ms)),
        None => format!("◆  {}", clock(c.start_ms)),
    };
    assert!(line.starts_with(&start), "{line:?}");
    assert_eq!(line.contains("  beat "), c.beat.is_some(), "{line:?}");
    assert_eq!(line.contains("  loop "), c.end_ms.is_some(), "{line:?}");
    if let (Some(end), Some(n)) = (c.end_ms, &c.loop_beats) {
        let unit = if n == "1" { "beat" } else { "beats" };
        assert!(line.contains(&format!("loop {n} {unit} to {}", clock(end))), "{line:?}");
    }
    assert_eq!(line.contains("(active)"), c.end_ms.is_some() && c.active_loop, "{line:?}");
    assert_eq!(line.contains(&format!("{:?}", c.name)), !c.name.is_empty(), "{line:?}");
    assert_eq!(line.ends_with("#e62828"), c.color.is_some(), "{line:?}");

    let hot = maybe(&tc).then_some(HotCue(1));
    let memory_at = maybe(&tc).then_some(62_500);
    let memory_beat = maybe(&tc).then_some(33);
    let which = Which { hot, memory_at, memory_beat, all: true };
    let expected = match (hot, memory_at, memory_beat) {
        (Some(h), _, _) => CueRef::Hot(h),
        (None, Some(ms), _) => CueRef::Memory(CuePosition::Ms(ms)),
        (None, None, Some(b)) => CueRef::Memory(CuePosition::Beat(b)),
        (None, None, None) => CueRef::All,
    };
    assert_eq!(which.get(), expected);

    let cues = tc.draw(gs::integers::<usize>().max_value(2));
    let list = [TrackCues {
        track: TrackRef { id: 1, name: "Artist - Title".into(), file_path: "/Contents/1.mp3".into() },
        bpm: 128.0,
        cues: vec![c; cues],
        onelibrary_cues: maybe(&tc).then(|| tc.draw(gs::integers::<usize>().max_value(2))),
        warnings: tc.draw(gs::subsequences(vec!["no beat grid".to_string()])),
    }];
    render_list(&list);
}
