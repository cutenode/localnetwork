//! Checking an MP4's atoms before symphonia reads them. symphonia-format-isomp4 0.5 sizes its
//! sample tables from the counts they claim (`Vec::with_capacity(count)`) and a `data` tag from
//! its atom's size, before reading any of it: a file claiming 4 billion samples asks for 16 GiB,
//! which aborts the process wherever memory isn't overcommitted (Windows, Linux with
//! `vm.overcommit_memory=2`), and a tag claiming more asks for it anywhere. An abort can't be
//! caught, so such a file is refused here.

use std::fs::File;
use std::io::{self, BufReader, ErrorKind, Read, Seek, SeekFrom};
use std::path::Path;

use loclib_model::audio::Format;

use crate::{Error, Result};

/// The atoms symphonia reads other atoms out of. `meta` starts with a version and flags.
const CONTAINERS: [&[u8; 4]; 12] =
    [b"moov", b"trak", b"mdia", b"minf", b"stbl", b"edts", b"udta", b"mvex", b"moof", b"traf", b"meta", b"ilst"];

/// The tables symphonia sizes from their count, with the bytes each entry takes. `stsz` has a
/// sample size before its count, and entries only when that is 0.
const TABLES: [(&[u8; 4], u64); 5] = [(b"stsz", 4), (b"stco", 4), (b"co64", 8), (b"stts", 8), (b"stsc", 12)];

/// Deeper than any atom symphonia reads (`moov/udta/meta/ilst/<tag>/data` is 6).
const DEPTH: u8 = 8;

/// How far symphonia's probe looks for a format's marker (`Probe::PROBE_SEARCH_LIMIT`).
const PROBE_LIMIT: u64 = 1 << 20;

/// Refuse `path` when it's an MP4 whose atoms claim more than the file holds: a sample table
/// whose count wouldn't fit in it, or an atom inside another running past it or the file's end.
/// Anything else, MP4 or not, is left for the decoder to make sense of.
pub fn vet_container(path: &Path) -> Result<()> {
    let io = |source| Error::Io { path: path.to_path_buf(), source };
    let mut file = BufReader::new(File::open(path).map_err(io)?);
    let len = file.get_ref().metadata().map_err(io)?.len();
    let Some(start) = find_ftyp(&mut file, path).map_err(io)? else {
        return Ok(());
    };
    match walk(&mut file, start, len, len, Level::Top).or_else(eof_is_fine).map_err(io)? {
        Some(why) => Err(Error::parse(path, why)),
        None => Ok(()),
    }
}

/// Where the MP4 starts: symphonia reads one from the atom its `ftyp` marker is in, which every
/// MP4 starts with. A file named as MP4 is searched as far as symphonia's probe looks.
fn find_ftyp(file: &mut BufReader<File>, path: &Path) -> io::Result<Option<u64>> {
    let mut head = [0; 8];
    match file.read_exact(&mut head) {
        Ok(()) if &head[4..] == b"ftyp" => return Ok(Some(0)),
        Ok(()) => {}
        Err(e) if e.kind() == ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e),
    }
    let format = path.extension().and_then(|e| e.to_str()).and_then(Format::from_extension);
    if !matches!(format, Some(Format::Mpeg4 | Format::Alac)) {
        return Ok(None);
    }
    let mut start = head.to_vec();
    file.by_ref().take(PROBE_LIMIT).read_to_end(&mut start)?;
    // The atom starts 4 bytes before its marker, which is where the 4 windows skipped put 0.
    Ok(start.windows(4).skip(4).position(|w| w == b"ftyp").map(|at| at as u64))
}

/// A file cut short inside an atom's header is the decoder's to report.
fn eof_is_fine(e: io::Error) -> io::Result<Option<&'static str>> {
    if e.kind() == ErrorKind::UnexpectedEof { Ok(None) } else { Err(e) }
}

#[derive(Clone, Copy, PartialEq)]
enum Level {
    /// The file's own atoms: one may run past the end of a file cut short (its `mdat`), for the
    /// decoder to report.
    Top,
    /// Inside another atom, reading at depth `n`.
    In(u8),
    /// Inside `ilst`, whose atoms (one per tag) each hold more.
    Tags(u8),
}

/// What's wrong with the atoms from `at` to `end` in a file of `len` bytes, if anything.
fn walk<R: Read + Seek>(r: &mut R, mut at: u64, end: u64, len: u64, level: Level) -> io::Result<Option<&'static str>> {
    let depth = match level {
        Level::Top => 0,
        Level::In(n) | Level::Tags(n) => n,
    };
    while at.saturating_add(8) <= end {
        r.seek(SeekFrom::Start(at))?;
        let size = u64::from(read_u32(r)?);
        let mut kind = [0; 4];
        r.read_exact(&mut kind)?;
        let (size, header) = match size {
            0 => (end - at, 8),
            1 => (read_u64(r)?, 16),
            size => (size, 8),
        };
        if size < header {
            return Ok(Some("an atom is smaller than its header"));
        }
        let atom_end = at.saturating_add(size);
        if level != Level::Top && atom_end > end {
            return Ok(Some("an atom runs past the one holding it"));
        }
        if level != Level::Top && atom_end > len {
            return Ok(Some("it ends inside an atom (cut short, or the atom claims more than it holds)"));
        }
        let (body, body_end) = (at + header, atom_end.min(len));
        let inner = match level {
            Level::Tags(_) => Some(Level::In(depth + 1)),
            _ if !CONTAINERS.contains(&&kind) => None,
            _ if &kind == b"ilst" => Some(Level::Tags(depth + 1)),
            _ => Some(Level::In(depth + 1)),
        };
        if let Some(inner) = inner.filter(|_| depth < DEPTH) {
            let first = if &kind == b"meta" { body + 4 } else { body };
            if let Some(why) = walk(r, first, body_end, len, inner)? {
                return Ok(Some(why));
            }
        } else if let Some(&(_, entry)) = TABLES.iter().find(|(t, _)| **t == kind) {
            // Past the version and flags.
            r.seek(SeekFrom::Start(body + 4))?;
            let sized = &kind == b"stsz";
            if sized && read_u32(r)? != 0 {
                at = atom_end;
                continue;
            }
            let count = u64::from(read_u32(r)?);
            let entries = body + 8 + if sized { 4 } else { 0 };
            if count * entry > body_end.saturating_sub(entries) {
                return Ok(Some("a sample table claims more entries than the file holds"));
            }
        }
        at = atom_end;
    }
    Ok(None)
}

fn read_u32(r: &mut impl Read) -> io::Result<u32> {
    let mut b = [0; 4];
    r.read_exact(&mut b)?;
    Ok(u32::from_be_bytes(b))
}

fn read_u64(r: &mut impl Read) -> io::Result<u64> {
    let mut b = [0; 8];
    r.read_exact(&mut b)?;
    Ok(u64::from_be_bytes(b))
}

#[cfg(test)]
mod tests;
