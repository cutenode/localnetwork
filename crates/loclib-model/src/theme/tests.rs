use hegel::TestCase;
use hegel::generators as gs;

use super::*;

#[test]
fn the_default_is_the_shared_colours() {
    let t = Theme::default();
    assert_eq!(t.get("cues.hot"), Some(vec![tokens::cues::HOT]));
    assert_eq!(t.get("trackColors.purple"), Some(vec![tokens::track_colors::PURPLE]));
    assert_eq!(t.get("status.dark.warning"), Some(vec![tokens::status::dark::WARNING]));
    assert_eq!(t.get("waveform.scope"), Some(SCOPE.to_vec()));
    assert_eq!(t.phrase(PhraseKind::Verse(3)), tokens::phrases::VERSE);
    assert_eq!(t.track_color(TrackColor::Aqua), TrackColor::Aqua.rgb());
}

#[test]
fn a_token_takes_its_number_of_colours() {
    let mut t = Theme::default();
    assert_eq!(t.set("playhead", &[[1, 2, 3], [4, 5, 6]]), Err("playhead is one colour".into()));
    assert_eq!(
        t.set("waveform.spectrum", &[[1, 2, 3]]),
        Err("waveform.spectrum is a gradient: two colours or more".into())
    );
    assert_eq!(t.set("ui.colour", &[[1, 2, 3]]), Err("\"ui.colour\" isn't a colour a theme sets".into()));
    assert_eq!(
        t.set("status.light.good", &[[1, 2, 3]]),
        Err("\"status.light.good\" isn't a colour a theme sets".into())
    );
    assert_eq!(t, Theme::default(), "nothing set");
    t.set("ui.accent", &[[1, 2, 3]]).unwrap();
    assert_eq!(t.accent, [1, 2, 3]);
}

#[test]
fn hex_colours_are_lowercase_rrggbb() {
    assert_eq!(hex("#ff8c1e"), Some([255, 140, 30]));
    for bad in ["ff8c1e", "#FF8C1E", "#ff8c1", "#ff8c1e0", "#gg0000", "", "#"] {
        assert_eq!(hex(bad), None, "{bad}");
    }
}

/// Setting a token to colours it takes changes that token alone, to those colours; one it doesn't
/// take (a gradient of one, a colour of two, a name it doesn't have) changes nothing.
#[hegel::test(test_cases = 300)]
fn setting_a_token_changes_it_alone(tc: TestCase) {
    let name = tc.draw(gs::sampled_from(TOKENS.to_vec()));
    let colors = tc.draw(gs::vecs(gs::arrays(gs::integers::<u8>())).max_size(4));
    let gradient = name.starts_with("waveform.");
    let mut t = Theme::default();
    let fits = if gradient { colors.len() >= 2 } else { colors.len() == 1 };
    assert_eq!(t.set(name, &colors).is_ok(), fits, "{name} {colors:?}");
    let expected = if fits { colors } else { Theme::default().get(name).unwrap() };
    assert_eq!(t.get(name), Some(expected));
    for other in TOKENS.iter().filter(|o| **o != name) {
        assert_eq!(t.get(other), Theme::default().get(other), "{other} after {name}");
    }
    let unknown = tc.draw(gs::text().max_size(12));
    assert_eq!(t.clone().set(&unknown, &[[0, 0, 0]]).is_ok(), TOKENS.contains(&unknown.as_str()));
}

/// Any colour written `#rrggbb` reads back as itself.
#[hegel::test(test_cases = 200)]
fn hex_reads_back_what_it_writes(tc: TestCase) {
    let [r, g, b] = tc.draw(gs::arrays(gs::integers::<u8>()));
    assert_eq!(hex(&format!("#{r:02x}{g:02x}{b:02x}")), Some([r, g, b]));
}
