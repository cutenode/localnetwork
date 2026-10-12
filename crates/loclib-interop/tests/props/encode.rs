//! Properties of the XML escaping.

use hegel::TestCase;
use hegel::generators as gs;
use loclib_interop::encode::xml_escape;

use crate::common;

/// Escaped text reads back as it was in a double- or single-quoted attribute, tabs and line
/// breaks included, less only what XML 1.0 can't hold; nothing XML treats specially is left raw.
#[hegel::test(test_cases = 300)]
fn escaped_text_reads_back(tc: TestCase) {
    let s = tc.draw(hegel::one_of!(
        gs::text().max_size(40),
        gs::text().alphabet("<>&\"';#x9\t\n\r \u{0}\u{1}\u{1f}\u{7f}\u{fffd}\u{fffe}\u{ffff}é").max_size(40),
    ));
    let escaped = xml_escape(&s);
    assert!(!escaped.contains(['<', '>', '"', '\'']), "{escaped:?}");
    assert!(escaped.chars().all(|c| c >= ' '), "{escaped:?}");
    assert_eq!(xml_escape(&common::xml_text(&s)), escaped, "only what XML can't hold is dropped");
    let doc = common::parse_xml(&format!("<a double=\"{escaped}\" single='{escaped}'/>"));
    let want = common::xml_text(&s);
    assert_eq!((doc.get("double"), doc.get("single")), (want.as_str(), want.as_str()));
}
