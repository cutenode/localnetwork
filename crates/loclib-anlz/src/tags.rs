//! Replacing, adding and removing one tagged section of an analysis file, for the writers of beat
//! grids, phrases and waveforms (the cue lists have their own, [`super::write_cue_points`]), and
//! making new files.
//!
//! A section is `tag[4] header_len:u32 total_len:u32`, the rest of its header and its body. A file
//! holding a tag twice (never seen, but for the cue lists) is refused rather than guessed at.

use super::write::put_u32;
use super::*;

/// The tags of rekordbox's analysis files in the order it writes them, all three kinds together
/// (`PPTH` and the cue lists are in each; every other tag in one kind): observed in rekordbox 6
/// exports (`.DAT`: `PPTH PVBR PQTZ PWAV PWV2 PCOB PCOB`; `.EXT`: `PPTH PWV3 PCOB PCOB PCO2 PCO2
/// PQT2 PWV5 PWV4 PSSI`; `.2EX`: `PPTH PWV7 PWV6 PWVC`). A section added goes right after the last
/// one before it here.
const ORDER: [&[u8; 4]; 15] = [
    b"PPTH", b"PVBR", b"PQTZ", b"PWAV", b"PWV2", b"PWV3", b"PCOB", b"PCO2", b"PQT2", b"PWV5", b"PWV4", b"PSSI",
    b"PWV7", b"PWV6", b"PWVC",
];

/// A section: `tag`, its lengths, `header` (the rest of its header) and `body`.
pub(super) fn section(tag: &[u8; 4], header: &[u8], body: &[u8]) -> Result<Vec<u8>> {
    let too_large = || malformed("section too large");
    let header_len = u32::try_from(12 + header.len()).map_err(|_| too_large())?;
    let total_len = u32::try_from(12 + header.len() + body.len()).map_err(|_| too_large())?;
    let mut s = Vec::with_capacity(total_len as usize);
    s.extend_from_slice(tag);
    s.extend_from_slice(&header_len.to_be_bytes());
    s.extend_from_slice(&total_len.to_be_bytes());
    s.extend_from_slice(header);
    s.extend_from_slice(body);
    Ok(s)
}

/// The one section tagged `tag`, if the file has it: refused when it has two.
pub(super) fn only<'a>(secs: &'a [SectionRef], tag: &[u8; 4]) -> Result<Option<&'a SectionRef>> {
    let mut found = secs.iter().filter(|s| &s.tag == tag);
    let first = found.next();
    if found.next().is_some() {
        let name = String::from_utf8_lossy(tag);
        return Err(Error::invalid(format!("the analysis file holds two {name} sections; left as it is")));
    }
    Ok(first)
}

/// A copy of an analysis file with its section `tag` replaced by `new` (a whole section, as
/// [`section`] makes), added where rekordbox puts it when the file has none, or removed when
/// `new` is `None`. Every other section comes through byte for byte, in its place (checked), and
/// so does anything after them.
pub(super) fn put(bytes: &[u8], tag: &[u8; 4], new: Option<&[u8]>) -> Result<Vec<u8>> {
    let secs = sections(bytes)?;
    let at = only(&secs, tag)?.map(|s| s.start);
    let header_len = be_u32(bytes, 4).expect("checked by sections") as usize;
    let file_len = be_u32(bytes, 8).expect("checked by sections") as usize;
    let rank = |t: &[u8; 4]| ORDER.iter().position(|o| *o == t);
    let mut out = bytes[..header_len].to_vec();
    let mut placed = false;
    let mut place = |out: &mut Vec<u8>| {
        if let Some(new) = new.filter(|_| !placed) {
            out.extend_from_slice(new);
        }
        placed = true;
    };
    for s in &secs {
        // Missing, it goes before the first section that comes after it in rekordbox's order.
        let after = rank(&s.tag).zip(rank(tag)).is_some_and(|(theirs, ours)| theirs > ours);
        if at.is_none() && after {
            place(&mut out);
        }
        if Some(s.start) == at {
            place(&mut out);
            continue;
        }
        out.extend_from_slice(&bytes[s.start..s.start + s.total_len]);
    }
    place(&mut out);
    let last_end = secs.last().map_or(header_len, |s| s.start + s.total_len);
    out.extend_from_slice(&bytes[last_end..file_len]);
    let len = u32::try_from(out.len()).map_err(|_| malformed("file too large"))?;
    put_u32(&mut out, 8, len);
    // Anything after the declared file length (never seen in practice) is preserved.
    out.extend_from_slice(&bytes[file_len..]);

    let others = |b: &[u8], secs: &[SectionRef]| -> Vec<Vec<u8>> {
        secs.iter().filter(|s| &s.tag != tag).map(|s| b[s.start..s.start + s.total_len].to_vec()).collect()
    };
    let written = sections(&out)?;
    let mine: Vec<&[u8]> =
        written.iter().filter(|s| &s.tag == tag).map(|s| &out[s.start..s.start + s.total_len]).collect();
    if others(&out, &written) != others(bytes, &secs) || mine != new.into_iter().collect::<Vec<_>>() {
        return Err(malformed("rewriting a section disturbed the others"));
    }
    Ok(out)
}

/// A new analysis file for the audio file at `audio` (a device path, `/Contents/...`): the
/// `PMAI` header and the `PPTH` path rekordbox 6 writes, and nothing else yet.
fn new_file(audio: &str) -> Result<Vec<u8>> {
    let mut path: Vec<u8> = audio.encode_utf16().flat_map(u16::to_be_bytes).collect();
    path.extend_from_slice(&[0, 0]);
    let len = u32::try_from(path.len()).map_err(|_| Error::invalid("the audio file's path is too long"))?;
    let ppth = section(b"PPTH", &len.to_be_bytes(), &path)?;
    // As every rekordbox 6 export's files have it: what the values after the lengths mean isn't
    // known.
    let mut f = b"PMAI".to_vec();
    for v in [0x1c, 0, 1, 0x0001_0000, 0x0001_0000, 0] {
        f.extend_from_slice(&u32::to_be_bytes(v));
    }
    f.extend_from_slice(&ppth);
    let total = f.len() as u32;
    put_u32(&mut f, 8, total);
    Ok(f)
}

/// New `.DAT` and `.EXT` files for a track without any, its audio file at `audio` (a device
/// path): each with its path (`PPTH`) and the empty cue lists rekordbox writes for a track
/// without cues, for the writers of grids, phrases and waveforms to add to.
pub fn new_analysis_files(audio: &str) -> Result<(Vec<u8>, Vec<u8>)> {
    let base = new_file(audio)?;
    let dat = write_cue_points(&base, CueFile::Dat, &[])?;
    let ext = write_cue_points(&base, CueFile::Ext, &[])?;
    Ok((dat, ext))
}
