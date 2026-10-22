use hegel::TestCase;
use hegel::generators as gs;

use super::*;

/// `<plugin>:<destination>` splits at its first colon into a plugin and a destination, neither
/// empty; anything else names none.
#[hegel::test(test_cases = 100)]
fn a_destination_is_named_by_its_plugin_and_its_name(tc: TestCase) {
    let plugin: String = tc.draw(gs::from_regex("[a-z0-9_-]{0,6}"));
    let name: String = tc.draw(gs::from_regex("[a-z0-9:_-]{0,6}"));
    let spec = match tc.draw(gs::booleans()) {
        true => format!("{plugin}:{name}"),
        false => format!("{plugin}{name}").replace(':', ""),
    };
    let split = destination_spec(&spec).ok();
    let expected = spec.split_once(':').filter(|(p, n)| !p.is_empty() && !n.is_empty());
    assert_eq!(split, expected, "{spec:?}");
    if split.is_none() {
        let e = destination_spec(&spec).unwrap_err().to_string();
        assert_eq!(e, format!("{spec:?} names no destination: <plugin>:<destination>"));
    }
}
