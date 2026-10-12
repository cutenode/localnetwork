use super::*;

#[test]
fn xterm_mapping() {
    assert_eq!(xterm256(0, 0, 0), 16);
    assert_eq!(xterm256(255, 255, 255), 231);
    assert_eq!(xterm256(255, 0, 0), 196);
    assert_eq!(xterm256(128, 128, 128), 244);
}

#[test]
fn modes_from_the_environment() {
    let s = |v: &str| Some(v.to_string());
    assert_eq!(detect(None, None, None), Mode::Dark);
    assert_eq!(detect(s("1"), s("light"), None), Mode::Plain);
    assert_eq!(detect(s(""), None, None), Mode::Dark, "an empty NO_COLOR doesn't count");
    assert_eq!(detect(None, s("Light"), s("15;0")), Mode::Light);
    assert_eq!(detect(None, s("dark"), s("0;15")), Mode::Dark);
    assert_eq!(detect(None, None, s("0;15")), Mode::Light, "black on white");
    assert_eq!(detect(None, s("auto"), s("0;default;7")), Mode::Light);
    assert_eq!(detect(None, None, s("15;0")), Mode::Dark);
    assert_eq!(detect(None, None, s("7;8")), Mode::Dark);
    assert_eq!(detect(None, None, s("nonsense")), Mode::Dark);
}

#[test]
fn light_terminals_get_the_palette_turned_round() {
    let luma = |c: [u8; 3]| c.iter().map(|v| *v as u32).sum::<u32>() / 3;
    // Text goes dark, backgrounds light, faint stays faint against the page.
    assert!(luma(for_light(FG)) < 30);
    assert!(luma(for_light(SELECTION)) > 160);
    assert!(luma(for_light(FAINT)) > luma(for_light(DIM)));
    // Accents stay themselves, dark enough to read on white.
    for c in [PINK, CYAN, AMBER, VIOLET, GREEN, RED] {
        let l = for_light(c);
        assert!(luma(l) < 110, "{c:?} -> {l:?}");
        let top = |c: [u8; 3]| (0..3).max_by_key(|&i| c[i]).unwrap();
        assert_eq!(top(l), top(c), "{c:?} -> {l:?} keeps its hue");
    }
    assert_eq!(for_light([128, 128, 128]).iter().collect::<rustc_hash::FxHashSet<_>>().len(), 1, "greys stay grey");
    // Chips (light ink on the colour, turned round) stay readable, bright greens too.
    let contrast = |a: [u8; 3], b: [u8; 3]| {
        let (x, y) = (luminance(a), luminance(b));
        (x.max(y) + 0.05) / (x.min(y) + 0.05)
    };
    for c in [HOT_CUE, AMBER, CYAN, track_color(TrackColor::Yellow)] {
        assert!(contrast(for_light(c), for_light([14, 14, 20])) >= 4.5, "{c:?} -> {:?}", for_light(c));
    }
    set_test_mode(Some(Mode::Plain));
    assert_eq!(rgb3(PINK), Color::Reset);
    assert!(selected().add_modifier.contains(Modifier::REVERSED));
    assert_eq!(colour_dot(TrackColor::Aqua).content, "6");
    set_test_mode(None);
    assert_eq!(colour_dot(TrackColor::Aqua).content, "●");
}

/// Every colour has an xterm colour from the cube or the grey ramp (not the 16 the terminal
/// redefines), and it's no further off than the nearest in the cube.
#[hegel::test(test_cases = 300)]
fn every_colour_has_a_near_xterm_colour(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let c = [0; 3].map(|_| tc.draw(gs::integers::<u8>()));
    let i = xterm256(c[0], c[1], c[2]);
    assert!(i >= 16, "{c:?} is {i}");
    const LEVELS: [i32; 6] = [0, 95, 135, 175, 215, 255];
    let rgb = |i: u8| match i {
        232.. => [8 + 10 * (i as i32 - 232); 3],
        _ => {
            let i = i as usize - 16;
            [LEVELS[i / 36], LEVELS[i / 6 % 6], LEVELS[i % 6]]
        }
    };
    let dist = |x: [i32; 3]| x.iter().zip(c).map(|(a, b)| (a - b as i32).pow(2)).sum::<i32>();
    let nearest = |v: u8| *LEVELS.iter().min_by_key(|l| (*l - v as i32).abs()).unwrap();
    assert!(dist(rgb(i)) <= dist(c.map(nearest)), "{c:?} as {i}: {:?}", rgb(i));
}

/// On a light terminal any colour of the palette turns round: no brighter than it was dark
/// (so light text goes dark), greys stay grey and a lighter grey goes no lighter, and a
/// colour that's clearly one hue keeps it.
#[hegel::test(test_cases = 300)]
fn any_colour_turns_round_for_a_light_terminal(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let c = [0; 3].map(|_| tc.draw(gs::integers::<u8>()));
    let out = for_light(c);
    assert!(luminance(out) <= (1.0 - luminance(c)).powi(3) + 1e-6, "{c:?} -> {out:?}");
    let (a, b) = (tc.draw(gs::integers::<u8>()), tc.draw(gs::integers::<u8>()));
    let (ga, gb) = (for_light([a; 3]), for_light([b; 3]));
    assert!(ga[0] == ga[1] && ga[1] == ga[2], "grey {a} -> {ga:?}");
    assert!(a > b || ga[0] >= gb[0], "grey {a} -> {ga:?}, {b} -> {gb:?}");
    let mut sorted = c;
    sorted.sort_unstable();
    // Clearly one hue: its strongest channel leads the next by 96 (in u16, where 237 + 96
    // doesn't saturate to 255 and let a yellow like [255, 237, 0] in).
    if sorted[2] as u16 >= sorted[1] as u16 + 96 && out != [0; 3] {
        let top = |c: [u8; 3]| (0..3).max_by_key(|&i| c[i]).unwrap();
        assert_eq!(top(out), top(c), "{c:?} -> {out:?} keeps its hue");
    }
}

/// The environment's say, in order. NO_COLOR set to anything means no colours, whatever
/// else is set, and an empty one is as good as none. A theme named `light` or `dark`, in any
/// case with spaces around, is that theme whatever COLORFGBG says; any other is as good as
/// none. Then only COLORFGBG's last field counts: light grey and the bright colours (7,
/// 9-15) are a light background, and anything else, or nothing, is dark.
#[hegel::test(test_cases = 300)]
fn the_mode_is_what_the_environment_asks_for(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let maybe = |s: String| tc.draw(gs::booleans()).then_some(s);
    let no_color = maybe(tc.draw(gs::sampled_from(&["", "1", "0"][..])).to_string());
    let named = tc.draw(gs::sampled_from(&["light", "dark", "solarized", "auto", ""][..]));
    let pad = || tc.draw(gs::sampled_from(&["", " ", "\t"][..]));
    let cased: String =
        named.chars().map(|c| if tc.draw(gs::booleans()) { c.to_ascii_uppercase() } else { c }).collect();
    let theme = maybe(format!("{}{cased}{}", pad(), pad()));
    let bg = tc.draw(gs::sampled_from(&["0", "6", "7", "8", "9", "15", "16", "default", ""][..]));
    let prefix = tc.draw(gs::sampled_from(&["", "15;", "0;default;", "x;"][..]));
    let colorfgbg = maybe(format!("{prefix}{bg}"));

    let got = detect(no_color.clone(), theme.clone(), colorfgbg.clone());
    let why = format!("{no_color:?} {theme:?} {colorfgbg:?}");
    if no_color.as_deref().is_some_and(|n| !n.is_empty()) {
        assert_eq!(got, Mode::Plain, "{why}");
        return;
    }
    assert_eq!(got, detect(None, theme.clone(), colorfgbg.clone()), "{why}");
    let background = detect(None, None, colorfgbg.clone());
    match (theme.is_some(), named) {
        (true, "light" | "dark") => {
            assert_eq!(got, detect(None, Some(named.into()), Some("0;15".into())), "{why}");
            assert_eq!(got, detect(None, Some(named.into()), Some("15;0".into())), "{why}");
            assert_eq!(got, if named == "light" { Mode::Light } else { Mode::Dark }, "{why}");
        }
        _ => assert_eq!(got, background, "{why}"),
    }
    let light = ["7", "9", "15"].contains(&bg);
    assert_eq!(background == Mode::Light, colorfgbg.is_some() && light, "{why}");
}
