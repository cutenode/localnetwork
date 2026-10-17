use std::io::Cursor;

use super::*;

#[test]
fn lines_are_read_one_at_a_time_and_long_ones_skipped() {
    let mut input = Vec::from(&b"{}\n"[..]);
    input.extend(vec![b'a'; MAX_LINE + 1]);
    input.extend(b"\nlast\n");
    // A last line too long that the input ends inside of.
    input.extend(vec![b'b'; MAX_LINE + 2]);
    let mut input = Cursor::new(input);
    let mut buf = Vec::new();
    let mut read = || (read_line(&mut input, &mut buf).unwrap(), String::from_utf8(buf.clone()).unwrap());
    assert_eq!(read(), (Line::Read, "{}\n".into()));
    assert_eq!(read(), (Line::TooLong, String::new()));
    assert_eq!(read(), (Line::Read, "last\n".into()));
    assert_eq!(read(), (Line::TooLong, String::new()));
    assert_eq!(read(), (Line::End, String::new()));
}
