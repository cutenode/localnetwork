use super::*;

#[test]
fn gradients() {
    let stops = crate::Palette::DARK.spectrum;
    assert_eq!(gradient(stops, 0.0), stops[0]);
    assert_eq!(gradient(stops, 1.0), [255, 196, 86]);
    assert_eq!(mix([0, 0, 0], [200, 100, 50], 0.5), [100, 50, 25]);
}

#[test]
fn muting_goes_towards_the_grey_of_the_same_lightness() {
    assert_eq!(mute([200, 40, 90], 0.0), [200, 40, 90]);
    assert_eq!(mute([200, 40, 90], 1.0), [78, 78, 78]);
    assert_eq!(mute([50, 50, 50], 0.5), [50, 50, 50]);
}
