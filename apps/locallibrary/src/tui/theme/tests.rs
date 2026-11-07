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
fn without_colours_it_is_said_with_reverse_video_and_numbers() {
    set_test_mode(Some(Mode::Plain));
    assert_eq!(rgb3(current().accent), Color::Reset);
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

/// Phrases are coloured by role: builds and verses alike, breakdowns and bridges alike, and each
/// other kind (every phrase rekordbox doesn't name the same) its own; a track's colour shows as a
/// dot in it, or without colours as its number.
#[hegel::test(test_cases = 100)]
fn phrases_are_coloured_by_role_and_tracks_by_their_colour(tc: hegel::TestCase) {
    use hegel::generators as gs;
    use locallibrary::anlz::PhraseKind;
    let kind = |tc: &hegel::TestCase| match tc.draw(gs::integers::<u8>().max_value(7)) {
        0 => PhraseKind::Intro,
        1 => PhraseKind::Up,
        2 => PhraseKind::Down,
        3 => PhraseKind::Verse(tc.draw(gs::integers::<u8>().max_value(6))),
        4 => PhraseKind::Bridge,
        5 => PhraseKind::Chorus,
        6 => PhraseKind::Outro,
        _ => PhraseKind::Unknown(tc.draw(gs::integers::<u16>().max_value(9))),
    };
    let role = |k: PhraseKind| match k {
        PhraseKind::Intro => 0,
        PhraseKind::Up | PhraseKind::Verse(_) => 1,
        PhraseKind::Chorus => 2,
        PhraseKind::Down | PhraseKind::Bridge => 3,
        PhraseKind::Outro => 4,
        PhraseKind::Unknown(_) => 5,
    };
    let (a, b) = (kind(&tc), kind(&tc));
    assert_eq!(current().phrase(a) == current().phrase(b), role(a) == role(b), "{a:?} and {b:?}");
    let c = tc.draw(gs::sampled_from(&super::super::browse::COLOURS[..]));
    let mode = tc.draw(gs::sampled_from(&[Mode::Dark, Mode::Light, Mode::Plain][..]));
    set_test_mode(Some(mode));
    let dot = colour_dot(c);
    if mode == Mode::Plain {
        assert_eq!(dot.content, c.id().to_string());
    } else {
        assert_eq!(dot.content, "●");
    }
    set_test_mode(None);
}

/// The player draws in the theme chosen, kept readable on a light terminal, and in no colours
/// without them; its waveforms take the theme's colours too.
#[test]
fn a_theme_is_drawn_in_every_mode() {
    let mut t = Theme::DEFAULT;
    t.set("ui.accent", &[[10, 200, 90]]).unwrap();
    t.set("waveform.spectrum", &[[100, 200, 250], [200, 100, 250]]).unwrap();
    set_test_theme(Some(Box::leak(Box::new(t))));
    assert_eq!(key().fg, Some(exact([10, 200, 90])));
    assert_eq!(palette().spectrum, [[100, 200, 250], [200, 100, 250]]);
    set_test_mode(Some(Mode::Light));
    let light = current().accent;
    assert_eq!(key().fg, Some(exact(light)));
    assert!(light[1] > light[0] && light[1] > light[2], "still its green: {light:?}");
    assert!(contrast(tier::drawn(light, tier()), fit::LIGHT_PAGE) >= 4.5, "and it reads: {light:?}");
    set_test_mode(Some(Mode::Plain));
    assert_eq!(key().fg, Some(Color::Reset));
    set_test_mode(None);
    set_test_theme(None);
    assert_eq!(key().fg, Some(exact(Theme::DEFAULT.accent)), "the default's again");
}

/// The mode the environment asks for as the player starts is the one drawn in.
#[hegel::test(test_cases = 50)]
fn prop_the_mode_the_environment_asks_for_is_drawn_in(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let var = |name: &str, value: &Option<&str>| match value {
        // SAFETY: nextest runs each test in a process of its own, and nothing else in this one
        // reads the environment now.
        Some(v) => unsafe { std::env::set_var(name, v) },
        // SAFETY: as above.
        None => unsafe { std::env::remove_var(name) },
    };
    let no_color = tc.draw(gs::optional(gs::sampled_from(vec!["", "1"])));
    let theme = tc.draw(gs::optional(gs::sampled_from(vec!["light", "dark", "auto"])));
    let colorfgbg = tc.draw(gs::optional(gs::sampled_from(vec!["0;15", "15;0", "0;default;7"])));
    var("NO_COLOR", &no_color);
    var("LOCALLIBRARY_THEME", &theme);
    var("COLORFGBG", &colorfgbg);
    init();
    let text = |v: Option<&str>| v.map(String::from);
    assert_eq!(mode(), detect(text(no_color), text(theme), text(colorfgbg)));
}

/// Terminals that say they have 24-bit colour, or are known to, get it; others the nearest of the
/// xterm 256. It's read once a process, so each terminal here is a process of its own.
#[test]
fn truecolor_is_what_the_terminal_has() {
    const ALONE: &str = "LOCALLIBRARY_TEST_TRUECOLOR";
    if let Some(want) = std::env::var_os(ALONE) {
        assert_eq!(matches!(rgb(1, 2, 3), Color::Rgb(1, 2, 3)), want == "yes", "{:?}", rgb(1, 2, 3));
        return;
    }
    let terminals = [
        ("COLORTERM", "truecolor", "yes"),
        ("COLORTERM", "24BIT", "yes"),
        ("TERM_PROGRAM", "iTerm.app", "yes"),
        ("TERM_PROGRAM", "ghostty", "yes"),
        ("KITTY_WINDOW_ID", "1", "yes"),
        ("WT_SESSION", "1", "yes"),
        ("COLORTERM", "256color", "no"),
        ("TERM_PROGRAM", "Apple_Terminal", "no"),
    ];
    for (var, value, want) in terminals {
        let out = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["tui::theme::tests::truecolor_is_what_the_terminal_has", "--exact", "--nocapture"])
            .env_remove("COLORTERM")
            .env_remove("TERM_PROGRAM")
            .env_remove("KITTY_WINDOW_ID")
            .env_remove("WT_SESSION")
            .env(var, value)
            .env(ALONE, want)
            .output()
            .unwrap();
        let said = String::from_utf8_lossy(&out.stdout);
        assert!(out.status.success() && said.contains("1 passed"), "{var}={value}: {said}");
    }
}
