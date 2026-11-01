//! One JSON message per line: reading lines with a cap on their length. The same framing as
//! locallibrary's RPC sidecar.

use std::io::{BufRead, Read};

/// The longest line read or written, far more than any message needs: a longer one is skipped,
/// so a peer that never ends a line can't make the other hold it all.
pub const MAX_LINE: usize = 16 << 20;

/// What [`read_line`] read.
#[derive(Debug, PartialEq, Eq)]
pub enum Line {
    Read,
    /// Longer than [`MAX_LINE`]: read to its end and dropped.
    TooLong,
    /// The input closed.
    End,
}

/// The next line of `input` into `buf`, holding at most [`MAX_LINE`] bytes of it.
pub fn read_line(input: &mut impl BufRead, buf: &mut Vec<u8>) -> std::io::Result<Line> {
    read_line_within(input, buf, MAX_LINE)
}

/// [`read_line`], with lines longer than `max` too long.
pub(crate) fn read_line_within(input: &mut impl BufRead, buf: &mut Vec<u8>, max: usize) -> std::io::Result<Line> {
    buf.clear();
    if input.by_ref().take(max as u64 + 1).read_until(b'\n', buf)? == 0 {
        return Ok(Line::End);
    }
    if buf.len() <= max || buf.ends_with(b"\n") {
        return Ok(Line::Read);
    }
    buf.clear();
    loop {
        let chunk = input.fill_buf()?;
        let (used, end) = match chunk.iter().position(|&b| b == b'\n') {
            Some(i) => (i + 1, true),
            None => (chunk.len(), chunk.is_empty()),
        };
        input.consume(used);
        if end {
            return Ok(Line::TooLong);
        }
    }
}

#[cfg(test)]
mod tests;
