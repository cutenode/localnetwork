use super::*;

#[test]
fn escaping() {
    assert_eq!(xml_escape("a&b<\"c\">'\u{1}"), "a&amp;b&lt;&quot;c&quot;&gt;&apos;");
    assert_eq!(xml_escape("t\tl\nc\r\u{FFFE}\u{FFFF}\u{FFFD}"), "t&#9;l&#10;c&#13;\u{FFFD}");
}
