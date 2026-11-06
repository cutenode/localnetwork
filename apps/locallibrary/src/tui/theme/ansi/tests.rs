use hegel::TestCase;
use hegel::generators as gs;

use super::super::tier::{Tier, ansi16, drawn};
use super::*;

/// Text is the terminal's own foreground, so it reads on any scheme.
#[test]
fn text_is_the_terminals_own_foreground() {
    for mode in [Mode::Dark, Mode::Light] {
        assert_eq!(readable(ansi16(theme(mode).text), mode), Color::Reset, "{mode:?}");
    }
    assert_eq!(readable(ansi16(LIGHT.ink), Mode::Light), Color::Indexed(7), "white stays white for ink");
    assert_eq!(readable(ansi16(DARK.ink), Mode::Dark), Color::Indexed(0), "and black");
}

/// Whatever the colour, what's drawn is the terminal's foreground or one of the 16, and never one
/// of the hues that read badly on that mode's background.
#[hegel::test(test_cases = 300)]
fn prop_any_colour_comes_out_as_one_of_the_16(tc: TestCase) {
    let c = [0; 3].map(|_| tc.draw(gs::integers::<u8>()));
    let mode = tc.draw(gs::sampled_from(&[Mode::Dark, Mode::Light][..]));
    match readable(ansi16(c), mode) {
        Color::Reset => assert!(matches!(drawn(c, Tier::Colors16), [0, 0, 0] | [229, 229, 229]), "{c:?}"),
        Color::Indexed(i) => {
            assert!(i < 16, "{c:?} is {i}");
            let bad: &[u8] = if mode == Mode::Light { &[9, 10, 11, 12, 13, 14] } else { &[1, 4, 5] };
            assert!(!bad.contains(&i), "{c:?} is {i} on {mode:?}");
        }
        other => panic!("{c:?} is {other:?}"),
    }
}
