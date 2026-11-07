use hegel::TestCase;
use hegel::generators as gs;
use rustc_hash::FxHashMap;

use super::*;

fn env(vars: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
    let vars: FxHashMap<String, String> = vars.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
    move |k| vars.get(k).cloned()
}

#[test]
fn the_environment_says_what_a_terminal_that_did_not_answer_can_draw() {
    let tier = |vars: &[(&str, &str)]| from_env(env(vars));
    assert_eq!(tier(&[("TERM", "xterm"), ("COLORTERM", "truecolor")]), Tier::Truecolor);
    assert_eq!(tier(&[("TERM", "xterm"), ("COLORTERM", "24bit")]), Tier::Truecolor);
    assert_eq!(tier(&[("TERM", "xterm"), ("TERM_PROGRAM", "iTerm.app")]), Tier::Truecolor);
    assert_eq!(tier(&[("TERM", "xterm"), ("KITTY_WINDOW_ID", "1")]), Tier::Truecolor);
    assert_eq!(tier(&[("TERM", "xterm"), ("WT_SESSION", "x")]), Tier::Truecolor);
    assert_eq!(tier(&[("TERM", "xterm-256color")]), Tier::Colors256);
    assert_eq!(tier(&[("TERM", "tmux-256color")]), Tier::Colors256);
    assert_eq!(tier(&[]), Tier::Colors256, "no TERM: Windows");
    assert_eq!(tier(&[("TERM", "xterm")]), Tier::Colors16);
    assert_eq!(tier(&[("TERM", "linux")]), Tier::Colors16);
    assert_eq!(tier(&[("TERM", "vt100")]), Tier::Colors16);
}

/// Terminal.app sets no COLORTERM: its build says which macOS it came with.
#[test]
fn terminal_app_draws_truecolor_from_macos_26() {
    let app = |build: &str| {
        from_env(env(&[
            ("TERM", "xterm-256color"),
            ("TERM_PROGRAM", "Apple_Terminal"),
            ("TERM_PROGRAM_VERSION", build),
        ]))
    };
    assert_eq!(app("455.1"), Tier::Colors256, "macOS 15");
    assert_eq!(app("460"), Tier::Truecolor, "the first build counted as macOS 26");
    assert_eq!(app("488"), Tier::Truecolor, "macOS 27");
    assert_eq!(app("nonsense"), Tier::Colors256);
    assert_eq!(from_env(env(&[("TERM", "xterm-256color"), ("TERM_PROGRAM", "Apple_Terminal")])), Tier::Colors256);
}

#[test]
fn each_of_the_16_is_itself() {
    for (i, c) in ANSI.iter().enumerate() {
        assert_eq!(usize::from(ansi16(*c)), i, "{c:?}");
    }
}

#[test]
fn colours_of_a_kind_stay_that_kind_in_16() {
    assert_eq!(ansi16([250, 40, 40]), 9, "a bright red");
    assert_eq!(ansi16([120, 30, 30]), 1, "a deep one");
    assert_eq!(ansi16([40, 200, 220]), 6, "aqua");
    assert_eq!(ansi16([255, 92, 160]), 9, "pink's strongest channel");
    assert_eq!(ansi16([40, 42, 48]), 0, "almost black");
    assert_eq!(ansi16([120, 120, 142]), 8, "a mid grey");
    assert_eq!(ansi16([210, 210, 215]), 7, "a light grey");
    assert_eq!(ansi16([250, 250, 250]), 15, "white");
}

#[hegel::test(test_cases = 100)]
fn prop_a_tier_names_itself(tc: TestCase) {
    let tier = tc.draw(gs::sampled_from(&Tier::ALL[..]));
    assert_eq!(Tier::parse(tier.name()), Some(tier));
    assert_eq!(Tier::parse(&format!("  {} ", tier.name().to_uppercase())), Some(tier));
    assert_eq!(Tier::ALL[tier as usize], tier);
    let other = tc.draw(gs::text().max_size(6));
    let known = ["truecolor", "24bit", "256", "16"].contains(&other.trim().to_ascii_lowercase().as_str());
    assert_eq!(Tier::parse(&other).is_some(), known, "{other:?}");
}

/// Any colour stays in its tier's range: an index of 16 or more in 256 colours, since the 16 below
/// are the terminal's to redefine, and one of the 16 else, where a grey stays a grey.
#[hegel::test(test_cases = 300)]
fn prop_every_colour_stays_in_its_tiers_range(tc: TestCase) {
    let c = [0; 3].map(|_| tc.draw(gs::integers::<u8>()));
    assert!(xterm256(c[0], c[1], c[2]) >= 16, "{c:?}");
    assert!(ansi16(c) < 16, "{c:?}");
    assert_eq!(drawn(c, Tier::Truecolor), c);
    assert!(ANSI.contains(&drawn(c, Tier::Colors16)), "{c:?}");
    let grey = [c[0]; 3];
    assert!(matches!(ansi16(grey), 0 | 7 | 8 | 15), "{grey:?}");
}

/// In any environment: `COLORTERM`'s word for 24-bit colour, or a terminal known to draw it, is
/// 24-bit; without that, TERM decides between 256 and 16.
#[hegel::test(test_cases = 300)]
fn prop_the_environment_decides_a_tier(tc: TestCase) {
    let colorterm = tc.draw(gs::sampled_from(&["", "truecolor", "24BIT", "yes"][..]));
    let program = tc.draw(gs::sampled_from(&["", "iTerm.app", "ghostty", "Apple_Terminal", "tmux"][..]));
    let build = tc.draw(gs::sampled_from(&["", "455", "460", "470.1", "x"][..]));
    let term = tc.draw(gs::sampled_from(&["", "xterm", "xterm-256color", "screen", "linux"][..]));
    let vars = [("COLORTERM", colorterm), ("TERM_PROGRAM", program), ("TERM_PROGRAM_VERSION", build), ("TERM", term)];
    let set: Vec<(&str, &str)> = vars.into_iter().filter(|(_, v)| !v.is_empty()).collect();
    let apple_true = program == "Apple_Terminal" && matches!(build, "460" | "470.1");
    let truecolor =
        matches!(colorterm, "truecolor" | "24BIT") || matches!(program, "iTerm.app" | "ghostty") || apple_true;
    let expected = match (truecolor, term) {
        (true, _) => Tier::Truecolor,
        (_, "" | "xterm-256color") => Tier::Colors256,
        _ => Tier::Colors16,
    };
    assert_eq!(from_env(env(&set)), expected, "{set:?}");
}
