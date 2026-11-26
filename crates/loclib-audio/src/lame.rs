//! The LAME tag at the head of an MP3, and the frame it sits in.
//!
//! An encoder writes its Xing or Info tag in a frame of its own before the audio, which decoders
//! (symphonia among them) skip. rekordbox doesn't skip it when the tag is LAME's own (`LAME` in
//! its encoder field), only when another encoder wrote it (ffmpeg's `Lavc` and `Lavf`, or LAME
//! versions old enough to sign `Lame`): its beat grids for those files sit one frame later than
//! the audio decoded here. Measured against 489 MP3s rekordbox analysed: the 70 signed `LAME`
//! sat 26.3 ms (a frame at 44.1 kHz is 26.1) later than the rest.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

/// How far past an ID3v2 tag the first frame is looked for.
const SEARCH: usize = 4096;

/// The largest ID3v2 tag skipped over (cover art makes them big): more and the file is left be.
const MAX_ID3: u64 = 64 << 20;

/// Seconds of audio in the frame holding the MP3's Xing or Info tag, when that tag is LAME's
/// (see the module's docs); `None` for anything else, an MP3 without one or a file that isn't an
/// MP3. Reads a few kilobytes past the file's ID3v2 tag.
pub fn lame_frame_secs(path: &Path) -> Option<f64> {
    let mut file = File::open(path).ok()?;
    let mut id3 = [0u8; 10];
    file.read_exact(&mut id3).ok()?;
    let skip = if &id3[..3] == b"ID3" {
        let size = id3[6..10].iter().fold(0u64, |n, b| (n << 7) | u64::from(b & 0x7f));
        let footer = if id3[5] & 0x10 != 0 { 10 } else { 0 };
        10 + size + footer
    } else {
        0
    };
    if skip > MAX_ID3 {
        return None;
    }
    file.seek(SeekFrom::Start(skip)).ok()?;
    let mut head = Vec::with_capacity(SEARCH + 512);
    file.take((SEARCH + 512) as u64).read_to_end(&mut head).ok()?;
    lame_frame_in(&head)
}

/// [`lame_frame_secs`] of the bytes after the ID3v2 tag.
pub(crate) fn lame_frame_in(bytes: &[u8]) -> Option<f64> {
    let start = (0..bytes.len().min(SEARCH)).find(|i| bytes.get(*i..*i + 4).and_then(header).is_some())?;
    let (samples, rate, side) = header(&bytes[start..start + 4])?;
    let tag = start + 4 + side;
    let id = bytes.get(tag..tag + 4)?;
    if id != b"Xing" && id != b"Info" {
        return None;
    }
    let flags = u32::from_be_bytes(bytes.get(tag + 4..tag + 8)?.try_into().ok()?);
    // The fields the flags say are there: frames, bytes, the table of contents, quality.
    let fields =
        [(1, 4), (2, 4), (4, 100), (8, 4)].iter().filter(|(f, _)| flags & f != 0).map(|(_, n)| n).sum::<usize>();
    let lame = tag + 8 + fields;
    (bytes.get(lame..lame + 4)? == b"LAME").then(|| f64::from(samples) / f64::from(rate))
}

/// An MPEG audio layer III frame header's samples per frame, sample rate, and side information
/// length.
fn header(b: &[u8]) -> Option<(u32, u32, usize)> {
    if b.len() < 4 || b[0] != 0xff || b[1] & 0xe0 != 0xe0 {
        return None;
    }
    let version = (b[1] >> 3) & 3; // 0: MPEG 2.5, 2: MPEG 2, 3: MPEG 1.
    let layer = (b[1] >> 1) & 3; // 1: layer III.
    let rate_index = usize::from((b[2] >> 2) & 3);
    let bitrate_index = b[2] >> 4;
    if version == 1 || layer != 1 || rate_index == 3 || bitrate_index == 0 || bitrate_index == 15 {
        return None;
    }
    let mono = b[3] >> 6 == 3;
    let rates = match version {
        3 => [44_100, 48_000, 32_000],
        2 => [22_050, 24_000, 16_000],
        _ => [11_025, 12_000, 8_000],
    };
    let (samples, side) = match (version == 3, mono) {
        (true, false) => (1152, 32),
        (true, true) => (1152, 17),
        (false, false) => (576, 17),
        (false, true) => (576, 9),
    };
    Some((samples, rates[rate_index], side))
}

#[cfg(test)]
mod tests;
