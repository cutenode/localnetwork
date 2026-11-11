use hegel::TestCase;
use hegel::generators as gs;

use super::*;
use crate::contents;
use crate::tests::{ACTIVATIONS_STRIPPED, ACTIVATIONS_XML, SETTINGS_STRIPPED, SETTINGS_XML};

#[test]
fn tokens_and_accounts_go_with_their_lines_and_nothing_else() {
    assert_eq!(strip_tokens(SETTINGS_XML.as_bytes()).unwrap(), SETTINGS_STRIPPED.as_bytes());
    assert_eq!(strip_accounts(ACTIVATIONS_XML.as_bytes()).unwrap(), ACTIVATIONS_STRIPPED.as_bytes());
    // A file without them is left as it is, to the byte.
    assert_eq!(strip_tokens(SETTINGS_STRIPPED.as_bytes()).unwrap(), SETTINGS_STRIPPED.as_bytes());
}

/// An element with others on its line goes alone; one with children goes whole; a value JUCE
/// stored as XML (a child element) goes with its `VALUE`.
#[test]
fn elements_go_whole_wherever_they_are() {
    let one_line = br#"<A><VALUE name="DropboxToken" val="s"/><VALUE name="lang" val="en"/></A>"#;
    assert_eq!(strip_tokens(one_line).unwrap(), br#"<A><VALUE name="lang" val="en"/></A>"#);
    let nested = "<ACTIVATIONS>\n\t<rekordbox_account account=\"a\">\n\t\t<enc>secret</enc>\n\t</rekordbox_account>\n</ACTIVATIONS>";
    assert_eq!(strip_accounts(nested.as_bytes()).unwrap(), b"<ACTIVATIONS>\n</ACTIVATIONS>");
    let stored_xml =
        "<P>\r\n  <VALUE name=\"AuthToken\"><x a=\"1\"/></VALUE>\r\n  <VALUE name=\"b\" val=\"2\"/>\r\n</P>";
    assert_eq!(strip_tokens(stored_xml.as_bytes()).unwrap(), b"<P>\r\n  <VALUE name=\"b\" val=\"2\"/>\r\n</P>");
    // Escaped in the name, a token is still one.
    assert_eq!(strip_tokens(br#"<P><VALUE name="&#84;oken" val="s"/></P>"#).unwrap(), b"<P></P>");
}

#[test]
fn a_file_that_doesnt_parse_isnt_stripped() {
    let odd: [&[u8]; 5] = [
        b"<P><VALUE name=\"Token\" val=\"s\"/>",
        b"<P><VALUE name=\"Token\" val=\"s/></P>",
        b"<P></Q>",
        b"RekordboxToken=secret",
        b"",
    ];
    for odd in odd {
        assert!(strip_tokens(odd).is_err(), "{}", String::from_utf8_lossy(odd));
    }
}

/// Whatever the settings, stripping leaves the file it would have been without the token
/// values, every other value where it was.
#[hegel::test(test_cases = 100)]
fn prop_stripping_leaves_every_other_value(tc: TestCase) {
    let n = tc.draw(gs::integers::<usize>().max_value(8));
    let values: Vec<(String, String)> = (0..n)
        .map(|_| {
            let token = tc.draw(gs::booleans());
            let name = tc.draw(gs::from_regex("[A-Za-z]{0,8}").fullmatch(true));
            let name = if token { format!("{name}ToKeN") } else { name.replace(['t', 'T'], "") };
            (name, tc.draw(gs::text().max_size(20)).replace('\0', ""))
        })
        .collect();
    let others: Vec<(String, String)> =
        values.iter().filter(|(n, _)| !n.to_lowercase().contains("token")).cloned().collect();
    let stripped = strip_tokens(&contents::write(&values)).unwrap();
    assert_eq!(stripped, contents::write(&others));
}
