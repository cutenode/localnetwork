//! Checking an MP4's atoms before symphonia reads them. symphonia-format-isomp4 0.5 sizes its
//! sample tables from the counts they claim (`Vec::with_capacity(count)`) and a `data` tag from
//! its atom's size, before reading any of it: a file claiming 4 billion samples asks for 16 GiB,
//! which aborts the process wherever memory isn't overcommitted (Windows, Linux with
//! `vm.overcommit_memory=2`), and a tag claiming more asks for it anywhere. It reads each sample
//! into a buffer of the size the file gives it, so one claiming 4 GiB asks for that for every
//! packet. symphonia-codec-alac sizes its buffers from the frame length an ALAC setup claims, so
//! one claiming 4 billion samples asks for 16 GiB a channel too. An abort can't be caught, so such
//! a file is refused here.

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

const TOO_MANY: &str = "a sample table claims more entries than the file holds";
const TOO_BIG: &str = "a sample is larger than the file";
const TOO_LONG: &str = "an ALAC frame claims more samples than any encoder writes";

/// The most samples an ALAC frame may claim. Apple's encoder writes 4,096; this leaves room for
/// any other while keeping a frame's buffers to a few MiB.
const ALAC_FRAME: u32 = 1 << 16;

/// Deeper than any atom symphonia reads (`moov/udta/meta/ilst/<tag>/data` is 6).
const DEPTH: u8 = 8;

/// How far symphonia's probe looks for a format's marker (`Probe::PROBE_SEARCH_LIMIT`).
const PROBE_LIMIT: u64 = 1 << 20;

/// Refuse `path` when it's an MP4 whose atoms claim more than the file holds: a sample table
/// whose count wouldn't fit in it, a sample larger than the file, an ALAC frame longer than
/// [`ALAC_FRAME`], or an atom inside another running past it or the file's end.
/// Anything else, MP4 or not, is left for the decoder to make sense of. So is what isn't a regular
/// file (a FIFO): what this read of it took would be gone when the decoder opened it again, and
/// its atoms can't be walked without seeking.
pub fn vet_container(path: &Path) -> Result<()> {
    vet_file(&mut File::open(path).map_err(|source| Error::Io { path: path.to_path_buf(), source })?, path)
}

/// [`vet_container`] for the file `path` is open as, which is left at its start.
pub(super) fn vet_file(file: &mut File, path: &Path) -> Result<()> {
    let io = |source| Error::Io { path: path.to_path_buf(), source };
    let meta = file.metadata().map_err(io)?;
    if !meta.is_file() {
        return Ok(());
    }
    let len = meta.len();
    // Buffered only once it's an MP4: `check` vets every file it can't read, and most aren't.
    let vetted = match find_ftyp(file, path).map_err(io)? {
        Some(start) => {
            walk(&mut BufReader::new(&mut *file), start, len, len, Level::Top).or_else(eof_is_fine).map_err(io)?
        }
        None => None,
    };
    file.rewind().map_err(io)?;
    match vetted {
        Some(why) => Err(Error::parse(path, why)),
        None => Ok(()),
    }
}

/// Where the MP4 starts: symphonia reads one from the atom its `ftyp` marker is in, which every
/// MP4 starts with. A file named as MP4 is searched as far as symphonia's probe looks.
fn find_ftyp(file: &mut File, path: &Path) -> io::Result<Option<u64>> {
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

/// What the atoms are read from: the file, buffered, or in the tests a reader that fails partway.
/// Taken as `dyn`, not as a generic, so the tests' reader runs the same compiled walk the file does.
trait Source: Read + Seek {}

impl<T: Read + Seek> Source for T {}

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
fn walk(r: &mut dyn Source, mut at: u64, end: u64, len: u64, level: Level) -> io::Result<Option<&'static str>> {
    let depth = match level {
        Level::Top => 0,
        Level::In(n) | Level::Tags(n) => n,
    };
    while at.saturating_add(8) <= end {
        r.seek(SeekFrom::Start(at))?;
        let (size, kind) = (u64::from(read_u32(r)?), read_kind(r)?);
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
        } else if let Some(why) = table(r, &kind, body, body_end, len)? {
            return Ok(Some(why));
        } else if &kind == b"stsd"
            && let Some(why) = alac_frames(r, body, body_end)?
        {
            return Ok(Some(why));
        }
        at = atom_end;
    }
    Ok(None)
}

/// What's wrong with the atom `kind` whose body runs from `body` to `body_end` in a file of `len`
/// bytes, if it's one symphonia reads a table or sample sizes from: a count of entries the atom
/// doesn't hold, or a sample larger than the file. Sizes come from `stsz` (one for every sample,
/// or each sample's), a fragmented MP4's defaults (`trex`, `tfhd`) and its runs (`trun`).
fn table(r: &mut dyn Source, kind: &[u8; 4], body: u64, body_end: u64, len: u64) -> io::Result<Option<&'static str>> {
    if !matches!(kind, b"trex" | b"tfhd" | b"trun") && !TABLES.iter().any(|(t, _)| *t == kind) {
        return Ok(None);
    }
    r.seek(SeekFrom::Start(body))?;
    let flags = read_u32(r)?;
    let flagged = |bit: u32| if flags & bit == 0 { 0 } else { 4 };
    let too_big = |size: u32| (u64::from(size) > len).then_some(TOO_BIG);
    // Where the entries start, the bytes each takes, and where in one a sample's size is.
    let (entries, entry, size_at) = match kind {
        b"stsz" => match read_u32(r)? {
            0 => (body + 12, 4, Some(0)),
            size => return Ok(too_big(size)),
        },
        // Past the track id, and the default sample description and duration.
        b"trex" => {
            r.seek(SeekFrom::Start(body + 16))?;
            return Ok(too_big(read_u32(r)?));
        }
        // Past the track id, then the base data offset (8 bytes), sample description and duration
        // when the flags say they're there.
        b"tfhd" if flags & 0x10 != 0 => {
            r.seek(SeekFrom::Start(body + 8 + 2 * flagged(0x1) + flagged(0x2) + flagged(0x8)))?;
            return Ok(too_big(read_u32(r)?));
        }
        // Past the count, data offset and first sample's flags, each sample's duration, size,
        // flags and composition offset, as the flags say.
        b"trun" => (
            body + 8 + flagged(0x1) + flagged(0x4),
            u64::from((flags & 0xf00).count_ones()) * 4,
            (flags & 0x200 != 0).then_some(flagged(0x100) as usize),
        ),
        _ => match TABLES.iter().find(|(t, _)| *t == kind) {
            Some(&(_, entry)) => (body + 8, entry, None),
            // A `tfhd` without a default sample size.
            None => return Ok(None),
        },
    };
    let count = u64::from(read_u32(r)?);
    if count * entry > body_end.saturating_sub(entries) {
        return Ok(Some(TOO_MANY));
    }
    let Some(size_at) = size_at else { return Ok(None) };
    r.seek(SeekFrom::Start(entries))?;
    let mut read = [0; 16];
    for _ in 0..count {
        let read = &mut read[..entry as usize];
        r.read_exact(read)?;
        let size = u32::from_be_bytes(read[size_at..size_at + 4].try_into().expect("4 bytes"));
        if let Some(why) = too_big(size) {
            return Ok(Some(why));
        }
    }
    Ok(None)
}

/// What's wrong with the ALAC setups among the sample entries in the `stsd` body from `body` to
/// `body_end`, if anything: a frame longer than [`ALAC_FRAME`]. Each setup is an `alac` atom inside
/// an `alac` entry, after the entry's fields (28 bytes, 16 more in version 1 and 36 in version 2).
/// Entries symphonia can't read are left for it to report.
fn alac_frames(r: &mut dyn Source, body: u64, body_end: u64) -> io::Result<Option<&'static str>> {
    // Past the version, flags and entry count.
    let mut at = body + 8;
    while at.saturating_add(8) <= body_end {
        r.seek(SeekFrom::Start(at))?;
        let (size, kind) = (u64::from(read_u32(r)?), read_kind(r)?);
        if size < 8 {
            return Ok(None);
        }
        let entry_end = at.saturating_add(size).min(body_end);
        if &kind == b"alac" {
            r.seek(SeekFrom::Start(at + 16))?;
            let fields = match read_u16(r)? {
                0 => 28,
                1 => 44,
                2 => 64,
                _ => return Ok(None),
            };
            let mut child = at + 8 + fields;
            // A header, the version and flags, and the frame length.
            while child.saturating_add(16) <= entry_end {
                r.seek(SeekFrom::Start(child))?;
                let (size, kind) = (u64::from(read_u32(r)?), read_kind(r)?);
                if &kind == b"alac" {
                    read_u32(r)?;
                    if read_u32(r)? > ALAC_FRAME {
                        return Ok(Some(TOO_LONG));
                    }
                    break;
                }
                if size < 8 {
                    break;
                }
                child = child.saturating_add(size);
            }
        }
        at = entry_end;
    }
    Ok(None)
}

fn read_kind(r: &mut (impl Read + ?Sized)) -> io::Result<[u8; 4]> {
    let mut kind = [0; 4];
    r.read_exact(&mut kind)?;
    Ok(kind)
}

fn read_u16(r: &mut (impl Read + ?Sized)) -> io::Result<u16> {
    let mut b = [0; 2];
    r.read_exact(&mut b)?;
    Ok(u16::from_be_bytes(b))
}

fn read_u32(r: &mut (impl Read + ?Sized)) -> io::Result<u32> {
    let mut b = [0; 4];
    r.read_exact(&mut b)?;
    Ok(u32::from_be_bytes(b))
}

fn read_u64(r: &mut (impl Read + ?Sized)) -> io::Result<u64> {
    let mut b = [0; 8];
    r.read_exact(&mut b)?;
    Ok(u64::from_be_bytes(b))
}

#[cfg(test)]
mod tests;
