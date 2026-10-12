use super::*;

#[test]
fn gradients() {
    let stops = crate::Palette::DARK.spectrum;
    assert_eq!(gradient(stops, 0.0), stops[0]);
    assert_eq!(gradient(stops, 1.0), [255, 196, 86]);
    assert_eq!(mix([0, 0, 0], [200, 100, 50], 0.5), [100, 50, 25]);
}
