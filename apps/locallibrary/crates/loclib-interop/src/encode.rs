//! The XML escaping the formats are written with.

use std::fmt::Write as _;

/// `s` as XML text or a quoted attribute value. Characters XML 1.0 can't hold (other control
/// characters, U+FFFE, U+FFFF) are dropped: one would make the whole file unreadable. Tab, LF
/// and CR are written as references, since a reader turns them into spaces when they stand as
/// they are in an attribute value.
pub fn xml_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            '\t' | '\n' | '\r' => {
                let _ = write!(out, "&#{};", c as u32);
            }
            c if (c as u32) < 0x20 || c == '\u{FFFE}' || c == '\u{FFFF}' => {}
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escaping() {
        assert_eq!(xml_escape("a&b<\"c\">'\u{1}"), "a&amp;b&lt;&quot;c&quot;&gt;&apos;");
        assert_eq!(xml_escape("t\tl\nc\r\u{FFFE}\u{FFFF}\u{FFFD}"), "t&#9;l&#10;c&#13;\u{FFFD}");
    }
}
