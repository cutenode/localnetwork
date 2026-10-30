//! Minimal reader/rewriter for rekordbox analysis files (`ANLZ0000.DAT/.EXT/.2EX`).
//!
//! Only what locallibrary needs: the audio path a file belongs to (`PPTH`, which must be
//! rewritten when a merge relocates a track), the beat grid (`PQTZ`) and cue points
//! (`PCOB`/`PCO2`) for XML export and editing (see [`write_cue_points`]), and the waveforms
//! (`PWAV`/`PWV3`/`PWV4`/`PWV5`, and `PWV6`/`PWV7` for a track with no others) and phrase analysis
//! (`PSSI`) for display. Everything else is passed through byte for byte. What they decode to is
//! [`loclib_model::analysis`]'s, re-exported here, and [`load`] reads it all for a track.
//!
//! The beat grid, phrases and waveforms can be written too ([`write_beat_grid`],
//! [`write_song_structure`], [`write_waveform`]), and new files made for a track without any
//! ([`new_analysis_files`]). Each writer reads what it wrote back with the reader here and fails
//! when that isn't what it was given; these layouts are checked against rekordbox's own files, but
//! only real players show what they make of them.
//!
//! All integers are big-endian. A file is a `PMAI` header followed by tagged sections, each
//! starting with `tag[4] header_len:u32 total_len:u32`. Layouts follow
//! [crate-digger](https://djl-analysis.deepsymmetry.org/rekordbox-export-analysis/anlz.html).

use crate::device::Device;
use crate::model::Track;
use crate::paths;
use crate::{Error, Result};

mod grid;
mod structure;
mod tags;
mod waves;
mod write;

pub use grid::{checked_grid, without_extended_grid, write_beat_grid};
pub use loclib_model::analysis::*;
pub use structure::{kind_number, write_song_structure};
pub use tags::new_analysis_files;
pub use waves::{DETAIL_RATE, at_detail_rate, waveform_entries, waveform_from_bands, waveform_tags, write_waveform};
pub(crate) use write::MAX_COMMENT;
pub use write::{CueFile, CuePoint, RawCue, read_cue_points, shift_times, write_cue_points};

#[derive(Debug, Clone, Copy)]
struct SectionRef {
    tag: [u8; 4],
    start: usize,
    header_len: usize,
    total_len: usize,
}

fn be_u16(b: &[u8], o: usize) -> Option<u16> {
    b.get(o..o + 2).map(|s| u16::from_be_bytes([s[0], s[1]]))
}

fn be_u32(b: &[u8], o: usize) -> Option<u32> {
    b.get(o..o + 4).map(|s| u32::from_be_bytes([s[0], s[1], s[2], s[3]]))
}

fn malformed(msg: &str) -> Error {
    Error::parse("ANLZ", msg)
}

/// Validate the `PMAI` header and list the top-level sections.
fn sections(bytes: &[u8]) -> Result<Vec<SectionRef>> {
    if bytes.get(0..4) != Some(b"PMAI") {
        return Err(malformed("missing PMAI header"));
    }
    let header_len = be_u32(bytes, 4).ok_or_else(|| malformed("truncated header"))? as usize;
    let file_len = be_u32(bytes, 8).ok_or_else(|| malformed("truncated header"))? as usize;
    if file_len > bytes.len() || header_len < 12 || header_len > file_len {
        return Err(malformed("inconsistent file length"));
    }
    let mut out = Vec::new();
    let mut pos = header_len;
    while pos + 12 <= file_len {
        let tag: [u8; 4] = bytes[pos..pos + 4].try_into().expect("4 bytes");
        let h = be_u32(bytes, pos + 4).unwrap() as usize;
        let t = be_u32(bytes, pos + 8).unwrap() as usize;
        if t < 12 || h < 12 || h > t || pos + t > file_len {
            return Err(malformed("section overruns file"));
        }
        out.push(SectionRef { tag, start: pos, header_len: h, total_len: t });
        pos += t;
    }
    Ok(out)
}

fn find<'a>(secs: &'a [SectionRef], tag: &[u8; 4]) -> Option<&'a SectionRef> {
    secs.iter().find(|s| &s.tag == tag)
}

/// Whether an analysis file has a section tagged `tag`.
pub fn has_section(bytes: &[u8], tag: &[u8; 4]) -> Result<bool> {
    Ok(find(&sections(bytes)?, tag).is_some())
}

fn utf16be(bytes: &[u8]) -> String {
    let units: Vec<u16> =
        bytes.chunks_exact(2).map(|c| u16::from_be_bytes([c[0], c[1]])).take_while(|&u| u != 0).collect();
    String::from_utf16_lossy(&units)
}

/// The audio file path recorded in the analysis file (device path, e.g. `/Contents/a.mp3`).
pub fn read_path(bytes: &[u8]) -> Result<Option<String>> {
    let secs = sections(bytes)?;
    let Some(s) = find(&secs, b"PPTH") else { return Ok(None) };
    let len = be_u32(bytes, s.start + 12).ok_or_else(|| malformed("truncated PPTH"))? as usize;
    let body = s.start + s.header_len;
    let path = bytes.get(body..body + len).ok_or_else(|| malformed("truncated PPTH"))?;
    Ok(Some(utf16be(path)))
}

/// Return a copy of the file with its `PPTH` path replaced (lengths fixed up). Files without a
/// `PPTH` section are returned unchanged.
pub fn rewrite_path(bytes: &[u8], new_path: &str) -> Result<Vec<u8>> {
    let secs = sections(bytes)?;
    let Some(s) = find(&secs, b"PPTH").copied() else { return Ok(bytes.to_vec()) };
    let mut encoded: Vec<u8> = new_path.encode_utf16().flat_map(u16::to_be_bytes).collect();
    encoded.extend_from_slice(&[0, 0]);
    let header_len = 16usize;
    let total = header_len + encoded.len();

    let file_len = be_u32(bytes, 8).unwrap() as usize;
    let mut out = Vec::with_capacity(bytes.len() + encoded.len());
    out.extend_from_slice(&bytes[..s.start]);
    out.extend_from_slice(b"PPTH");
    out.extend_from_slice(&(header_len as u32).to_be_bytes());
    out.extend_from_slice(&(total as u32).to_be_bytes());
    out.extend_from_slice(&(encoded.len() as u32).to_be_bytes());
    out.extend_from_slice(&encoded);
    out.extend_from_slice(&bytes[s.start + s.total_len..file_len]);
    let new_len = out.len() as u32;
    out[8..12].copy_from_slice(&new_len.to_be_bytes());
    // Anything after the declared file length (never seen in practice) is preserved.
    out.extend_from_slice(&bytes[file_len..]);
    Ok(out)
}

pub fn beat_grid(bytes: &[u8]) -> Result<Vec<Beat>> {
    let secs = sections(bytes)?;
    let Some(s) = find(&secs, b"PQTZ") else { return Ok(Vec::new()) };
    let count = be_u32(bytes, s.start + 20).ok_or_else(|| malformed("truncated PQTZ"))? as usize;
    let body = s.start + s.header_len;
    let end = s.start + s.total_len;
    // The count comes from the file; never trust it for allocation.
    let mut beats = Vec::with_capacity(count.min((end - body) / 8));
    for i in 0..count {
        let o = body + i * 8;
        if o + 8 > end {
            break;
        }
        beats.push(Beat {
            beat_number: be_u16(bytes, o).unwrap(),
            bpm: be_u16(bytes, o + 2).unwrap() as f64 / 100.0,
            time_ms: be_u32(bytes, o + 4).unwrap(),
        });
    }
    Ok(beats)
}

/// Cue points. Uses the extended list (`PCO2`, in `.EXT`) when present, which carries names and
/// colours; otherwise the basic list (`PCOB`). Only the basic list says which loops are active,
/// so its say goes for the extended list's loops too.
pub fn cues(bytes: &[u8]) -> Result<Vec<Cue>> {
    let secs = sections(bytes)?;
    let mut basic = Vec::new();
    for s in secs.iter().filter(|s| &s.tag == b"PCOB") {
        parse_entries(bytes, s, b"PCPT", &mut basic, parse_pcpt);
    }
    let extended: Vec<&SectionRef> = secs.iter().filter(|s| &s.tag == b"PCO2").collect();
    let mut out = match extended.is_empty() {
        true => basic,
        false => {
            let mut out = Vec::new();
            for s in extended {
                parse_entries(bytes, s, b"PCP2", &mut out, parse_pcp2);
            }
            mark_active_loops(&mut out, &basic);
            out
        }
    };
    out.sort_by_key(|c| (c.hot_cue.map_or(0, |h| h as u32 + 1), c.start_ms));
    Ok(out)
}

/// Mark the loops of `cues` that `basic` (cues from a `PCOB` list, the only one with the flag)
/// has as active loops.
fn mark_active_loops(cues: &mut [Cue], basic: &[Cue]) {
    let key = |c: &Cue| (c.hot_cue, c.start_ms, c.end_ms);
    for c in cues.iter_mut().filter(|c| c.end_ms.is_some()) {
        c.active_loop |= basic.iter().any(|b| b.active_loop && key(b) == key(c));
    }
}

fn parse_entries(
    bytes: &[u8],
    list: &SectionRef,
    tag: &[u8; 4],
    out: &mut Vec<Cue>,
    parse: fn(&[u8], usize, usize) -> Option<Cue>,
) {
    let mut pos = list.start + list.header_len;
    let end = list.start + list.total_len;
    while pos + 12 <= end {
        let Some(t) = be_u32(bytes, pos + 8).map(|t| t as usize) else { break };
        if t < 12 || pos + t > end {
            break;
        }
        if &bytes[pos..pos + 4] == tag
            && let Some(c) = parse(bytes, pos, t)
        {
            out.push(c);
        }
        pos += t;
    }
}

fn loop_end(start: u32, loop_time: u32, is_loop: bool) -> Option<u32> {
    (is_loop && loop_time != u32::MAX && loop_time > start).then_some(loop_time)
}

/// `PCPT`: hot_cue u32, status u32 (4: an active loop), u32, order u16 x2, type u8, u8, u16,
/// time u32, loop u32. A hot cue number past 256 isn't a cue, as for [`read_cue_points`].
fn parse_pcpt(b: &[u8], o: usize, _len: usize) -> Option<Cue> {
    let hot = be_u32(b, o + 12)?;
    let status = be_u32(b, o + 16)?;
    let kind = *b.get(o + 28)?;
    let time = be_u32(b, o + 32)?;
    let loop_time = be_u32(b, o + 36)?;
    let end_ms = loop_end(time, loop_time, kind == 2);
    Some(Cue {
        hot_cue: write::slot(hot)?,
        start_ms: time,
        end_ms,
        comment: String::new(),
        color: None,
        color_id: 0,
        loop_beats: None,
        active_loop: end_ms.is_some() && status == write::ACTIVE_LOOP,
    })
}

/// `PCP2`: hot_cue u32, type u8, u8, u16, time u32, loop u32, color id u8, 7 bytes,
/// loop num/den u16 x2, comment_len u32, comment (UTF-16BE), color code u8, r, g, b.
fn parse_pcp2(b: &[u8], o: usize, len: usize) -> Option<Cue> {
    let hot = be_u32(b, o + 12)?;
    let kind = *b.get(o + 16)?;
    let time = be_u32(b, o + 20)?;
    let loop_time = be_u32(b, o + 24)?;
    let color_id = b.get(o + 28).copied().filter(|id| (1..=8).contains(id)).unwrap_or(0);
    let end_ms = loop_end(time, loop_time, kind == 2);
    let loop_beats = match (be_u16(b, o + 36), be_u16(b, o + 38)) {
        (Some(n), Some(d)) if n > 0 && d > 0 && end_ms.is_some() => Some((n, d)),
        _ => None,
    };
    let comment_len = be_u32(b, o + 40)? as usize;
    let comment_start = o + 44;
    let comment = b.get(comment_start..comment_start + comment_len).map(utf16be).unwrap_or_default();
    let rgb_at = comment_start + comment_len + 1;
    let color = (rgb_at + 3 <= o + len).then(|| [b[rgb_at], b[rgb_at + 1], b[rgb_at + 2]]).filter(|c| c != &[0, 0, 0]);
    let hot_cue = write::slot(hot)?;
    Some(Cue { hot_cue, start_ms: time, end_ms, comment, color, color_id, loop_beats, active_loop: false })
}

// ---------------------------------------------------------------------------------------------
// Waveforms
// ---------------------------------------------------------------------------------------------

/// Fixed-size entries of a waveform section: `entry_len` bytes each, after the section header.
/// Uses the declared count when the header has one (at `count_at`), bounded by the section.
fn entries<'a>(bytes: &'a [u8], s: &SectionRef, entry_len: usize, count_at: Option<usize>) -> Option<&'a [u8]> {
    let body = bytes.get(s.start + s.header_len..s.start + s.total_len)?;
    let available = body.len() / entry_len;
    let declared = count_at.and_then(|o| be_u32(bytes, s.start + o)).map_or(available, |n| n as usize);
    Some(&body[..declared.min(available) * entry_len])
}

/// Blue waveform byte: height in the low 5 bits, "whiteness" in the top 3.
fn blue_column(b: u8) -> WaveColumn {
    let height = (b & 0x1f) as f32 / 31.0;
    let w = (b >> 5) as f32 / 7.0;
    let lerp = |a: f32, b: f32| (a + (b - a) * w) as u8;
    let color = [lerp(0.0, 200.0), lerp(104.0, 230.0), lerp(232.0, 255.0)];
    WaveColumn { back: WaveLayer { height, color }, front: WaveLayer::default() }
}

/// Scale an RGB triple so its brightest channel is `max`.
fn normalized(rgb: [u8; 3], max: u32) -> [u8; 3] {
    let peak = rgb.into_iter().max().unwrap_or(0) as u32;
    if peak == 0 {
        return [0, 0, 0];
    }
    rgb.map(|c| (c as u32 * max / peak) as u8)
}

/// The scrolling detail waveform's resolution: rekordbox stores it per half-frame (150/s). The
/// rate is also recorded in the section header.
fn detail_rate(bytes: &[u8], s: &SectionRef) -> f64 {
    match be_u16(bytes, s.start + 20) {
        Some(r) if r > 0 => r as f64,
        _ => 150.0,
    }
}

/// The whole-track overview waveform in an analysis file: colour (`PWV4`, in `.EXT`) if present,
/// else blue (`PWAV`, in `.DAT`), else 3-band (`PWV6`, in `.2EX`).
pub fn preview(bytes: &[u8]) -> Result<Option<Waveform>> {
    let secs = sections(bytes)?;
    if let Some(s) = find(&secs, b"PWV4")
        && let Some(body) = entries(bytes, s, 6, Some(16))
    {
        // Bytes 3-5 are the red/green/blue (low/mid/high) amplitudes, up to 127. The back layer
        // is as tall as the loudest band; the front one is the high band.
        let columns = body
            .chunks_exact(6)
            .map(|e| {
                let rgb = [e[3], e[4], e[5]];
                let back = e[3].max(e[4]).max(e[5]);
                WaveColumn {
                    back: WaveLayer { height: (back as f32 / 127.0).min(1.0), color: normalized(rgb, 191) },
                    front: WaveLayer { height: (e[5] as f32 / 127.0).min(1.0), color: normalized(rgb, 255) },
                }
            })
            .collect();
        return Ok(Some(Waveform { style: WaveformStyle::Rgb, columns_per_sec: None, columns }));
    }
    if let Some(s) = find(&secs, b"PWAV")
        && let Some(body) = entries(bytes, s, 1, Some(12))
    {
        let columns = body.iter().copied().map(blue_column).collect();
        return Ok(Some(Waveform { style: WaveformStyle::Blue, columns_per_sec: None, columns }));
    }
    if let Some(s) = find(&secs, b"PWV6")
        && let Some(body) = entries(bytes, s, 3, Some(16))
    {
        return Ok(Some(waves::three_band(body, b"PWV6").waveform(None)));
    }
    Ok(None)
}

/// The scrolling detail waveform in an analysis file: colour (`PWV5`, in `.EXT`) if present, else
/// blue (`PWV3`, `.EXT`), else 3-band (`PWV7`, `.2EX`).
pub fn detail(bytes: &[u8]) -> Result<Option<Waveform>> {
    let secs = sections(bytes)?;
    if let Some(s) = find(&secs, b"PWV5")
        && let Some(body) = entries(bytes, s, 2, Some(16))
    {
        // u16: red 3 bits, green 3 bits, blue 3 bits, height 5 bits, 2 unused.
        let columns = body
            .chunks_exact(2)
            .map(|e| {
                let v = u16::from_be_bytes([e[0], e[1]]);
                let rgb = [(v >> 13) & 7, (v >> 10) & 7, (v >> 7) & 7].map(|c| (c * 36) as u8);
                let height = ((v >> 2) & 0x1f) as f32 / 31.0;
                WaveColumn { back: WaveLayer { height, color: normalized(rgb, 255) }, front: WaveLayer::default() }
            })
            .collect();
        return Ok(Some(Waveform { style: WaveformStyle::Rgb, columns_per_sec: Some(detail_rate(bytes, s)), columns }));
    }
    if let Some(s) = find(&secs, b"PWV3")
        && let Some(body) = entries(bytes, s, 1, Some(16))
    {
        let columns = body.iter().copied().map(blue_column).collect();
        return Ok(Some(Waveform {
            style: WaveformStyle::Blue,
            columns_per_sec: Some(detail_rate(bytes, s)),
            columns,
        }));
    }
    if let Some(s) = find(&secs, b"PWV7")
        && let Some(body) = entries(bytes, s, 3, Some(16))
    {
        return Ok(Some(waves::three_band(body, b"PWV7").waveform(Some(detail_rate(bytes, s)))));
    }
    Ok(None)
}

// ---------------------------------------------------------------------------------------------
// Phrases
// ---------------------------------------------------------------------------------------------

/// What rekordbox's phrase kind number means in a track of `mood`.
fn phrase_kind(mood: Mood, kind: u16) -> PhraseKind {
    match (mood, kind) {
        (_, 1) => PhraseKind::Intro,
        (Mood::High, 2) => PhraseKind::Up,
        (Mood::High, 3) => PhraseKind::Down,
        (Mood::High, 5) => PhraseKind::Chorus,
        (Mood::High, 6) => PhraseKind::Outro,
        (Mood::Mid, 2..=7) => PhraseKind::Verse(kind as u8 - 1),
        (Mood::Low, 2..=4) => PhraseKind::Verse(1),
        (Mood::Low, 5..=7) => PhraseKind::Verse(2),
        (Mood::Mid | Mood::Low, 8) => PhraseKind::Bridge,
        (Mood::Mid | Mood::Low, 9) => PhraseKind::Chorus,
        (Mood::Mid | Mood::Low, 10) => PhraseKind::Outro,
        _ => PhraseKind::Unknown(kind),
    }
}

/// rekordbox 6 obfuscates `PSSI` bodies with this XOR mask (offset by the entry count), per
/// crate-digger. Unmasked files have a mood of 1-3 where masked ones have garbage.
const PSSI_MASK: [u8; 19] =
    [0xCB, 0xE1, 0xEE, 0xFA, 0xE5, 0xEE, 0xAD, 0xEE, 0xE9, 0xD2, 0xE9, 0xEB, 0xE1, 0xE9, 0xF3, 0xE8, 0xE9, 0xF4, 0xE1];

/// The phrase structure in an analysis file, if it has one.
pub fn song_structure(bytes: &[u8]) -> Result<Option<SongStructure>> {
    let secs = sections(bytes)?;
    let Some(s) = find(&secs, b"PSSI") else { return Ok(None) };
    let (Some(entry_len), Some(count)) = (be_u32(bytes, s.start + 12), be_u16(bytes, s.start + 16)) else {
        return Err(malformed("truncated PSSI"));
    };
    let entry_len = entry_len as usize;
    // Everything after the entry count (mood onwards) is what the mask covers.
    let mut body = bytes.get(s.start + 18..s.start + s.total_len).ok_or_else(|| malformed("truncated PSSI"))?.to_vec();
    if be_u16(&body, 0).is_some_and(|m| m > 20) {
        for (i, b) in body.iter_mut().enumerate() {
            *b ^= PSSI_MASK[i % PSSI_MASK.len()].wrapping_add(count as u8);
        }
    }
    let mood = match be_u16(&body, 0) {
        Some(1) => Mood::High,
        Some(2) => Mood::Mid,
        Some(3) => Mood::Low,
        _ => return Ok(None),
    };
    let end = be_u16(&body, 8).ok_or_else(|| malformed("truncated PSSI"))?;
    let first = s.header_len.saturating_sub(18);
    let mut phrases: Vec<Phrase> = Vec::new();
    for i in 0..count as usize {
        let o = first + i * entry_len;
        let (Some(beat), Some(kind)) = (be_u16(&body, o + 2), be_u16(&body, o + 4)) else { break };
        let fill = body.get(o + 21).copied().unwrap_or(0) != 0;
        let fill_beat = be_u16(&body, o + 22).filter(|b| fill && *b > 0);
        if let Some(prev) = phrases.last_mut() {
            prev.end_beat = beat;
        }
        phrases.push(Phrase { kind: phrase_kind(mood, kind), start_beat: beat, end_beat: end, fill_beat });
    }
    // Entries come in order; anything else is corrupt.
    phrases.retain(|p| p.end_beat > p.start_beat);
    Ok(Some(SongStructure { mood, phrases }))
}

/// A track's cues, from its `.DAT` and `.EXT` (unreadable ones leave none). The `.EXT`'s extended
/// list has names and colours, so it's preferred when present; which loops are active the `.DAT`'s
/// basic lists say too (a rekordbox 7 `.EXT` keeps no memory cues in its own).
pub fn track_cues(dat: Option<&[u8]>, ext: Option<&[u8]>) -> Vec<Cue> {
    let basic = dat.and_then(|b| cues(b).ok()).unwrap_or_default();
    match ext.and_then(|b| cues(b).ok()).filter(|c| !c.is_empty()) {
        Some(mut extended) => {
            mark_active_loops(&mut extended, &basic);
            extended
        }
        None => basic,
    }
}

/// A track's beat grid, cues, waveforms and phrases. Missing or unreadable files leave fields
/// empty.
pub fn load(device: &Device, track: &Track) -> TrackAnalysis {
    read_track(device, track, true)
}

/// A track's beat grid and cues only, for bulk work such as XML export.
pub fn load_grid(device: &Device, track: &Track) -> TrackAnalysis {
    read_track(device, track, false)
}

fn read_track(device: &Device, track: &Track, waveforms: bool) -> TrackAnalysis {
    let mut a = TrackAnalysis::default();
    if track.analyze_path.is_empty() {
        return a;
    }
    let dat = paths::key(&track.analyze_path);
    let read = |key: &str| device.checked_path(key).ok().and_then(|p| crate::fsutil::read(&p).ok());
    let sibling = |ext: &str| dat.strip_suffix(".DAT").map(|s| format!("{s}.{ext}")).and_then(|k| read(&k));
    let ext = sibling("EXT");
    let dat = read(&dat);
    a.cues = track_cues(dat.as_deref(), ext.as_deref());
    if let Some(bytes) = dat {
        a.beats = beat_grid(&bytes).unwrap_or_default();
        if waveforms {
            a.preview = preview(&bytes).ok().flatten();
        }
    }
    if waveforms && let Some(bytes) = ext {
        // Colour waveforms live in .EXT; keep the .DAT's blue preview otherwise.
        if let Some(p) = preview(&bytes).ok().flatten() {
            a.preview = Some(p);
        }
        a.detail = detail(&bytes).ok().flatten();
        a.structure = song_structure(&bytes).ok().flatten();
    }
    // The 3-band waveforms (`.2EX`), only for a track with no others.
    if waveforms
        && (a.preview.is_none() || a.detail.is_none())
        && let Some(bytes) = sibling("2EX")
    {
        a.preview = a.preview.or_else(|| preview(&bytes).ok().flatten());
        a.detail = a.detail.or_else(|| detail(&bytes).ok().flatten());
    }
    a
}

#[cfg(test)]
mod tests;
