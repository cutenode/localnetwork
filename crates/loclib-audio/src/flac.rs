//! Checking a FLAC's metadata before symphonia reads it. symphonia-metadata 0.5 reads a picture's
//! media type, description and data, and each Vorbis comment, into a buffer of the length the file
//! claims before reading any of it: a claim of 4 GiB asks for 4 GiB, though a metadata block holds
//! 16 MiB at most. A comment holding a picture (`METADATA_BLOCK_PICTURE`, in base64) is decoded and
//! read the same way. An abort can't be caught, so a FLAC claiming more than its block holds is
//! refused here.

use std::fs::File;
use std::io::{self, BufReader, ErrorKind, Read, Seek, SeekFrom, Take};
use std::path::Path;

use symphonia::core::probe::QueryDescriptor;
use symphonia::default::formats::{AdtsReader, AiffReader, IsoMp4Reader, MpaReader, WavReader};

use crate::{Error, PROBE_LIMIT, Result};

const PICTURE: &str = "a picture claims more than its metadata block holds";
const COMMENT: &str = "a Vorbis comment claims more than its metadata block holds";

/// Refuse `path` when it's a FLAC whose picture or Vorbis comment claims more than its metadata
/// block holds. Anything else is left for symphonia to report, as is what isn't a regular file
/// (see [`vet_container`](crate::vet_container)). `file` is `path`, open, and is left at its start.
pub(super) fn vet_file(file: &mut File, path: &Path) -> Result<()> {
    let io = |source| Error::Io { path: path.to_path_buf(), source };
    if !file.metadata().map_err(io)?.is_file() {
        return Ok(());
    }
    // Buffered only once it's a FLAC: `check` vets every file it can't read, and most aren't.
    let vetted = match find_flac(file).map_err(io)? {
        Some(start) => {
            file.seek(SeekFrom::Start(start)).map_err(io)?;
            match blocks(&mut BufReader::new(&mut *file)) {
                // A file cut short, or a block running out partway, is symphonia's to report.
                Err(e) if e.kind() == ErrorKind::UnexpectedEof => None,
                vetted => vetted.map_err(io)?,
            }
        }
        None => None,
    };
    file.rewind().map_err(io)?;
    match vetted {
        Some(why) => Err(Error::parse(path, why)),
        None => Ok(()),
    }
}

/// Where in `file` symphonia reads FLAC metadata from, if it reads any: just past the `fLaC`
/// marker, when that's the first marker its probe finds. The probe looks a byte at a time, from the
/// start and up to [`PROBE_LIMIT`] bytes on, for any format's marker; an ID3v2 tag's has it read
/// the tag and look again from its end. The formats are the ones every build of this crate has
/// symphonia read, so its probe stops at each of them; one a wider build adds (Ogg) is looked
/// past, which can only vet more.
fn find_flac(file: &mut File) -> io::Result<Option<u64>> {
    let formats =
        [AdtsReader::query(), AiffReader::query(), IsoMp4Reader::query(), MpaReader::query(), WavReader::query()];
    let other =
        |here: &[u8]| formats.iter().flat_map(|f| f.iter()).flat_map(|d| d.markers).any(|m| here.starts_with(m));
    let mut buf = [0; 8192];
    let mut from = 0;
    'search: loop {
        file.seek(SeekFrom::Start(from))?;
        // `buf[..have]` is the file from `at`.
        let (mut at, mut have) = (from, 0);
        loop {
            let read = file.read(&mut buf[have..])?;
            have += read;
            // Each place with an ID3v2 header's 10 bytes after it, or every place once the file ends.
            let done = if read == 0 { have } else { have.saturating_sub(9) };
            for (i, here) in (0..done).map(|i| (at + i as u64, &buf[i..have])) {
                // The probe compares once it has read 2 bytes of a marker, and stops past the limit.
                if i - from + 2 > PROBE_LIMIT {
                    return Ok(None);
                }
                if here.starts_with(b"fLaC") {
                    return Ok(Some(i + 4));
                }
                if here.starts_with(b"ID3") {
                    // A tag cut short in its header fails the probe.
                    let Some(size) = here.get(6..10) else { return Ok(None) };
                    // The header's 10 bytes, then the size: 7 bits a byte.
                    from = i + 10 + size.iter().fold(0, |n, b| n << 7 | u64::from(b & 0x7f));
                    continue 'search;
                }
                if other(here) {
                    return Ok(None);
                }
            }
            if read == 0 {
                return Ok(None);
            }
            buf.copy_within(done..have, 0);
            (at, have) = (at + done as u64, have - done);
        }
    }
}

/// What's wrong with the metadata blocks from the one `r` is at, if anything. Each starts with
/// whether it's the last (a bit), its type (7 bits: 4 is Vorbis comments, 6 a picture) and its
/// length (24 bits).
fn blocks(r: &mut BufReader<&mut File>) -> io::Result<Option<&'static str>> {
    loop {
        let mut header = [0; 4];
        r.read_exact(&mut header)?;
        let mut block = r.by_ref().take(u64::from(u32::from_be_bytes(header) & 0xff_ffff));
        let why = match header[0] & 0x7f {
            4 => comments(&mut block)?,
            6 => picture(&mut block)?,
            _ => None,
        };
        if why.is_some() || header[0] & 0x80 != 0 {
            return Ok(why);
        }
        let left = block.limit();
        r.seek_relative(left as i64)?;
    }
}

/// What's wrong with the picture `r` holds, if anything: a media type, description or data
/// claiming more than is left of it. After the picture's type, each follows its length, the data
/// after the width, height, colour depth and palette size too; all are 32 bits, big-endian.
fn picture(r: &mut Take<impl Read>) -> io::Result<Option<&'static str>> {
    skip(r, 4)?;
    let Some(media) = claim(r, u32::from_be_bytes)? else { return Ok(Some(PICTURE)) };
    skip(r, media)?;
    let Some(description) = claim(r, u32::from_be_bytes)? else { return Ok(Some(PICTURE)) };
    skip(r, description + 16)?;
    Ok(claim(r, u32::from_be_bytes)?.is_none().then_some(PICTURE))
}

/// What's wrong with the Vorbis comments `r` holds, if anything: the vendor's name or a comment
/// claiming more than is left of them, or a picture in a comment claiming more than it holds. The
/// lengths, and the count of comments after the vendor's name, are 32 bits, little-endian.
fn comments(r: &mut Take<impl Read>) -> io::Result<Option<&'static str>> {
    let Some(vendor) = claim(r, u32::from_le_bytes)? else { return Ok(Some(COMMENT)) };
    skip(r, vendor)?;
    let mut count = [0; 4];
    r.read_exact(&mut count)?;
    for _ in 0..u32::from_le_bytes(count) {
        let Some(len) = claim(r, u32::from_le_bytes)? else { return Ok(Some(COMMENT)) };
        let mut comment = vec![0; len as usize];
        r.read_exact(&mut comment)?;
        // Told as symphonia tells it. One it can't decode or read is a tag it skips: a picture
        // running out of bytes is left to it too.
        let comment = String::from_utf8_lossy(&comment);
        if let Some((key, value)) = comment.split_once('=')
            && key.to_lowercase() == "metadata_block_picture"
            && let Some(data) = base64(value)
            && let Ok(Some(why)) = picture(&mut data.as_slice().take(data.len() as u64))
        {
            return Ok(Some(why));
        }
    }
    Ok(None)
}

/// The length `r` claims next, 32 bits read by `from`, when what's left of it holds that many.
fn claim(r: &mut Take<impl Read>, from: fn([u8; 4]) -> u32) -> io::Result<Option<u64>> {
    let mut len = [0; 4];
    r.read_exact(&mut len)?;
    let len = u64::from(from(len));
    Ok((len <= r.limit()).then_some(len))
}

/// Read past `n` bytes of `r`, or to its end, for the next read to find.
fn skip(r: &mut impl Read, n: u64) -> io::Result<()> {
    io::copy(&mut r.by_ref().take(n), &mut io::sink()).map(drop)
}

/// `text` decoded as symphonia decodes a picture comment, or `None` where it gives up: padding is
/// trimmed, a lone last symbol dropped unread, and any other symbol outside RFC 4648's alphabet
/// fails it. A crate's decoder differs at those edges, and what's vetted must be what symphonia
/// reads.
fn base64(text: &str) -> Option<Vec<u8>> {
    let text = text.trim_end_matches('=').as_bytes();
    let text = &text[..text.len() - usize::from(text.len() % 4 == 1)];
    let mut out = Vec::with_capacity(text.len() / 4 * 3 + 2);
    for group in text.chunks(4) {
        let mut bits = 0;
        for &c in group {
            let value = match c {
                b'A'..=b'Z' => c - b'A',
                b'a'..=b'z' => c - b'a' + 26,
                b'0'..=b'9' => c - b'0' + 52,
                b'+' => 62,
                b'/' => 63,
                _ => return None,
            };
            bits = bits << 6 | u32::from(value);
        }
        // 6 bits a symbol, from the top of 24: each whole 8 is a byte.
        bits <<= 6 * (4 - group.len());
        out.extend_from_slice(&bits.to_be_bytes()[1..group.len()]);
    }
    Some(out)
}

#[cfg(test)]
mod tests;
