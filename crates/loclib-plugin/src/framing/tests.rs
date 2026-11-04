use std::io::{BufReader, Cursor};

use hegel::TestCase;
use hegel::generators as gs;

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

/// Any lines, the last perhaps unended, read through any buffer: each at most `max` long is read
/// whole, the rest are skipped to their end, then the input ends; and with the longest line
/// there is, every one of them is read.
#[hegel::test(test_cases = 100)]
fn prop_lines_are_read_whole_up_to_the_longest_and_skipped_past_it(tc: TestCase) {
    let max = tc.draw(gs::integers::<usize>().min_value(1).max_value(8));
    let lines: Vec<Vec<u8>> = tc.draw(gs::vecs(gs::vecs(gs::integers::<u8>().max_value(9)).max_size(20)).max_size(6));
    let mut lines: Vec<Vec<u8>> = lines.into_iter().map(|l| l.into_iter().map(|b| b'a' + b).collect()).collect();
    let capacity = tc.draw(gs::integers::<usize>().min_value(1).max_value(16));
    // One too long, by more than the buffer holds: skipped a buffer at a time.
    let at = tc.draw(gs::integers::<usize>().max_value(lines.len()));
    lines.insert(at, vec![b'z'; max + 1 + 2 * capacity]);
    let ended = tc.draw(gs::booleans()) || lines.last().is_none_or(Vec::is_empty);
    let mut input = lines.join(&b'\n');
    if ended {
        input.push(b'\n');
    }
    let mut reader = input_of(input.clone(), capacity, false);
    let mut buf = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        let unended = i + 1 == lines.len() && !ended;
        let mut whole = line.clone();
        if !unended {
            whole.push(b'\n');
        }
        match line.len() <= max {
            true => assert_eq!((read_line_within(&mut reader, &mut buf, max).unwrap(), &buf), (Line::Read, &whole)),
            false => assert_eq!(read_line_within(&mut reader, &mut buf, max).unwrap(), Line::TooLong),
        }
    }
    assert_eq!(read_line_within(&mut reader, &mut buf, max).unwrap(), Line::End);
    let mut reader = input_of(input, 64, false);
    for line in &lines {
        assert_eq!(read_line(&mut reader, &mut buf).unwrap(), Line::Read);
        assert_eq!(buf.strip_suffix(b"\n").unwrap_or(&buf), &line[..]);
    }
    assert_eq!(read_line(&mut reader, &mut buf).unwrap(), Line::End);
}

/// What comes after an input's bytes: its end, or (`true`) a failure.
struct Unplugged(bool);

impl Read for Unplugged {
    fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
        match self.0 {
            true => Err(std::io::Error::other("unplugged")),
            false => Ok(0),
        }
    }
}

/// `bytes`, read through a buffer of `capacity`, then the end or (`fails`) a failure. The
/// properties all read one of these, so one reader's code is run every way.
fn input_of(bytes: Vec<u8>, capacity: usize, fails: bool) -> BufReader<std::io::Chain<Cursor<Vec<u8>>, Unplugged>> {
    BufReader::with_capacity(capacity, Cursor::new(bytes).chain(Unplugged(fails)))
}

/// An input that fails, before a line ends or while a line too long is skipped, fails the read.
#[hegel::test(test_cases = 50)]
fn prop_an_input_that_fails_fails_the_read(tc: TestCase) {
    let max = tc.draw(gs::integers::<usize>().min_value(1).max_value(8));
    let short = tc.draw(gs::integers::<usize>().max_value(max));
    let long = tc.draw(gs::integers::<usize>().min_value(max + 1).max_value(20));
    for given in [short, long] {
        let capacity = tc.draw(gs::integers::<usize>().min_value(1).max_value(16));
        let mut reader = input_of(vec![b'a'; given], capacity, true);
        let e = read_line_within(&mut reader, &mut Vec::new(), max).unwrap_err();
        assert_eq!(e.to_string(), "unplugged");
    }
}
