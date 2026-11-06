use hegel::TestCase;
use hegel::generators as gs;
use locallibrary::anlz::PhraseKind;
use locallibrary::theme::TOKENS;
use ratatui::style::Color;

use super::super::tier::{ANSI, ansi16};
use super::super::{Mode, ansi, luminance};
use super::*;

/// The roles text is drawn in, by name.
fn text_roles(t: &Theme) -> Vec<(&'static str, [u8; 3])> {
    vec![
        ("text", t.text),
        ("dim", t.dim),
        ("faint", t.faint),
        ("accent", t.accent),
        ("info", t.info),
        ("secondary", t.secondary),
        ("good", t.good),
        ("warning", t.warning),
        ("critical", t.critical),
    ]
}

/// The roles of waveforms and markers: read at a glance, not as text.
fn mark_roles(t: &Theme) -> Vec<(String, [u8; 3])> {
    let one = [
        ("rule", t.rule),
        ("playhead", t.playhead),
        ("hot", t.hot_cue),
        ("memory", t.memory_cue),
        ("loop", t.loop_cue),
    ];
    let lists = [
        ("band", &t.bands[..]),
        ("phrase", &t.phrases),
        ("track", &t.track_colors),
        ("spectrum", &t.spectrum),
        ("scope", &t.scope),
    ];
    let many =
        lists.into_iter().flat_map(|(name, all)| all.iter().enumerate().map(move |(i, c)| (format!("{name} {i}"), *c)));
    one.into_iter().map(|(n, c)| (n.to_string(), c)).chain(many).collect()
}

const DARK: Fit = Fit { mode: Mode::Dark, tier: Tier::Truecolor, page: DARK_PAGE, foreground: None };

fn fit(mode: Mode, tier: Tier, page: [u8; 3], foreground: Option<[u8; 3]>) -> Fit {
    Fit { mode, tier, page, foreground }
}

/// The colour a role comes out as on the screen, a scheme's own foreground as xterm's.
fn on_screen(c: [u8; 3], f: &Fit) -> [u8; 3] {
    if f.tier != Tier::Colors16 {
        return drawn(c, f.tier);
    }
    match ansi::readable(ansi16(c), f.mode) {
        Color::Indexed(i) => ANSI[usize::from(i)],
        _ if f.mode == Mode::Light => ANSI[0],
        _ => ANSI[7],
    }
}

/// With no answer from the terminal, every role of every tier and mode reads on the usual
/// background for the mode: 4.5:1 for text, 3:1 for what's a mark.
#[test]
fn every_role_reads_on_the_usual_background() {
    for mode in [Mode::Dark, Mode::Light] {
        for tier in Tier::ALL {
            let page = if mode == Mode::Light { LIGHT_PAGE } else { DARK_PAGE };
            let f = fit(mode, tier, page, None);
            let t = resolve(&Theme::DEFAULT, &f);
            for (role, c) in text_roles(&t) {
                let ratio = contrast(on_screen(c, &f), page);
                assert!(ratio >= TEXT, "{role} {c:?} is {ratio:.2}:1 on {mode:?} {tier:?}");
            }
            for (role, c) in mark_roles(&t) {
                let ratio = contrast(on_screen(c, &f), page);
                assert!(ratio >= MARK, "{role} {c:?} is {ratio:.2}:1 on {mode:?} {tier:?}");
            }
        }
    }
}

/// The light palette is as designed on its own background: nothing needed moving but rekordbox's
/// track colours, which are the one thing it doesn't design.
#[test]
fn the_light_palette_needs_no_fitting_on_white() {
    let t = resolve(&Theme::DEFAULT, &fit(Mode::Light, Tier::Truecolor, LIGHT_PAGE, None));
    let without_tracks = |t: &Theme| Theme { track_colors: [[0; 3]; 8], bands: [[0; 3]; 3], ..t.clone() };
    assert_eq!(without_tracks(&t), without_tracks(&Theme::LIGHT));
    assert_ne!(t.track_colors[3], Theme::LIGHT.track_colors[3], "yellow is taken down");
    assert_eq!(t.track_colors[6], Theme::LIGHT.track_colors[6], "blue reads as it is");
    assert_ne!(t.bands[2], Theme::LIGHT.bands[0], "the bright core stays apart from the lows");
}

/// On its usual background the dark default is as made, but for the phrase rekordbox couldn't
/// name, a shared token a little under 3:1.
#[test]
fn the_dark_default_is_left_alone_on_its_own_background() {
    let t = resolve(&Theme::DEFAULT, &DARK);
    assert!(contrast(t.phrases[7], DARK_PAGE) >= MARK);
    for name in TOKENS.into_iter().filter(|n| *n != "phrases.unknown") {
        assert_eq!(t.get(name), Theme::DEFAULT.get(name), "{name}");
    }
}

/// Text stays what it means on a colour, too: every role a chip is made of has an ink that reads.
#[test]
fn chips_read_in_the_palettes_ink() {
    for t in [&Theme::DEFAULT, &Theme::LIGHT] {
        for (role, paper) in [("accent", t.accent), ("info", t.info), ("warning", t.warning), ("good", t.good)] {
            let ink = super::super::better_ink(t.ink, t.text, paper);
            let ratio = contrast(ink, paper);
            assert!(ratio >= TEXT, "ink on {role}: {ratio:.2}:1");
        }
    }
}

/// A terminal's real background can sit anywhere. Whatever it is, whatever its foreground and
/// whichever palette was chosen, every text role reaches 4.5:1 on it and every mark 3:1, as the
/// terminal shows them; a colour that reads already isn't touched; and what's behind a row leaves
/// dimmed text readable on top.
#[hegel::test(test_cases = 300)]
fn prop_text_roles_keep_contrast_on_any_background(tc: TestCase) {
    let page = tc.draw(gs::arrays(gs::integers::<u8>()));
    let foreground = tc.draw(gs::optional(gs::arrays(gs::integers::<u8>())));
    let mode = tc.draw(gs::sampled_from(&[Mode::Dark, Mode::Light][..]));
    let tier = tc.draw(gs::sampled_from(&[Tier::Truecolor, Tier::Colors256][..]));
    let f = fit(mode, tier, page, foreground);
    let mut theme = Theme::DEFAULT;
    if tc.draw(gs::booleans()) {
        theme.set("ui.accent", &[tc.draw(gs::arrays(gs::integers::<u8>()))]).unwrap();
    }
    let t = resolve(&theme, &f);
    for (role, c) in text_roles(&t) {
        let ratio = contrast(drawn(c, tier), page);
        assert!(ratio >= TEXT, "{role} {c:?} is {ratio:.2}:1 on {page:?}");
    }
    for (role, c) in mark_roles(&t) {
        let ratio = contrast(drawn(c, tier), page);
        assert!(ratio >= MARK, "{role} {c:?} is {ratio:.2}:1 on {page:?}");
    }
    let row = drawn(t.selection, tier);
    assert!(contrast(t.dim, row) >= TEXT || row == drawn(page, tier), "dim on {row:?}");
    let c = tc.draw(gs::arrays(gs::integers::<u8>()));
    if contrast(drawn(c, tier), page) >= TEXT {
        assert_eq!(legible(c, &f, t.text, TEXT), c);
    }
}

/// What's behind a row keeps its distance from the page wherever the page is.
#[test]
fn a_panel_follows_the_real_background() {
    let night = [20, 20, 24];
    let f = fit(Mode::Dark, Tier::Truecolor, night, None);
    let t = resolve(&Theme::DEFAULT, &f);
    assert_eq!(t.idle, [43, 43, 59], "as far from the page as it was made to be");
    assert_ne!(t.selection, [51, 51, 75], "nearer it where dimmed text has to read on top");
    assert!(luminance(t.selection) > luminance(night));
    assert!(contrast(t.dim, t.selection) >= TEXT);
    let paper = fit(Mode::Light, Tier::Truecolor, [239, 241, 245], None);
    let t = resolve(&Theme::DEFAULT, &paper);
    assert_eq!(t.selection, [203, 220, 244], "the wash on a real page");
}

/// Plain and 16-colour themes aren't fitted: plain has no colours and the 16 are the scheme's.
#[test]
fn only_real_colours_are_fitted() {
    let plain = fit(Mode::Plain, Tier::Truecolor, [128; 3], None);
    assert_eq!(resolve(&Theme::DEFAULT, &plain), Theme::DEFAULT);
    let sixteen = fit(Mode::Dark, Tier::Colors16, [128; 3], None);
    assert_eq!(resolve(&Theme::DEFAULT, &sixteen).phrase(PhraseKind::Intro), ANSI[12]);
    let mut own = Theme::DEFAULT;
    own.set("ui.accent", &[[10, 200, 90]]).unwrap();
    assert_eq!(resolve(&own, &sixteen).accent, [10, 200, 90], "a theme's own colour stays, for the mapping");
}
