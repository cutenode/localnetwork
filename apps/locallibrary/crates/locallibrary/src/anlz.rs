//! Minimal reader/rewriter for rekordbox analysis files (`ANLZ0000.DAT/.EXT/.2EX`).
//!
//! Only what locallibrary needs: the audio path a file belongs to (`PPTH`, which must be
//! rewritten when a merge relocates a track), the beat grid (`PQTZ`) and cue points
//! (`PCOB`/`PCO2`) for XML export and editing (see [`write_cue_points`]), and the waveforms
//! (`PWAV`/`PWV3`/`PWV4`/`PWV5`) and phrase analysis (`PSSI`) for display. Everything else is
//! passed through byte for byte. What they decode to is [`loclib_model::analysis`]'s, re-exported
//! here, and [`load`] reads it all for a track.
//!
//! All integers are big-endian. A file is a `PMAI` header followed by tagged sections, each
//! starting with `tag[4] header_len:u32 total_len:u32`. Layouts follow
//! [crate-digger](https://djl-analysis.deepsymmetry.org/rekordbox-export-analysis/anlz.html).

use crate::device::Device;
use crate::model::Track;
use crate::paths;
use crate::{Error, Result};

mod write;

pub use loclib_model::analysis::*;
pub use write::{CueFile, CuePoint, RawCue, read_cue_points, write_cue_points};

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
/// colours; otherwise the basic list (`PCOB`).
pub fn cues(bytes: &[u8]) -> Result<Vec<Cue>> {
    let secs = sections(bytes)?;
    let extended: Vec<&SectionRef> = secs.iter().filter(|s| &s.tag == b"PCO2").collect();
    let mut out = Vec::new();
    if !extended.is_empty() {
        for s in extended {
            parse_entries(bytes, s, b"PCP2", &mut out, parse_pcp2);
        }
    } else {
        for s in secs.iter().filter(|s| &s.tag == b"PCOB") {
            parse_entries(bytes, s, b"PCPT", &mut out, parse_pcpt);
        }
    }
    out.sort_by_key(|c| (c.hot_cue.map_or(0, |h| h as u32 + 1), c.start_ms));
    Ok(out)
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

/// `PCPT`: hot_cue u32, status u32, u32, order u16 x2, type u8, u8, u16, time u32, loop u32.
/// (status is not interpreted; its meaning is not fully understood.) A hot cue number past 256
/// isn't a cue, as for [`read_cue_points`].
fn parse_pcpt(b: &[u8], o: usize, _len: usize) -> Option<Cue> {
    let hot = be_u32(b, o + 12)?;
    let kind = *b.get(o + 28)?;
    let time = be_u32(b, o + 32)?;
    let loop_time = be_u32(b, o + 36)?;
    Some(Cue {
        hot_cue: write::slot(hot)?,
        start_ms: time,
        end_ms: loop_end(time, loop_time, kind == 2),
        comment: String::new(),
        color: None,
        color_id: 0,
        loop_beats: None,
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
    Some(Cue { hot_cue: write::slot(hot)?, start_ms: time, end_ms, comment, color, color_id, loop_beats })
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
/// else blue (`PWAV`, in `.DAT`).
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
    Ok(None)
}

/// The scrolling detail waveform in an analysis file (`.EXT`): colour (`PWV5`) if present, else
/// blue (`PWV3`).
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
    let read = |key: &str| device.checked_path(key).ok().and_then(|p| std::fs::read(p).ok());
    let ext = dat.strip_suffix(".DAT").map(|s| format!("{s}.EXT")).and_then(|k| read(&k));
    if let Some(bytes) = read(&dat) {
        a.beats = beat_grid(&bytes).unwrap_or_default();
        a.cues = cues(&bytes).unwrap_or_default();
        if waveforms {
            a.preview = preview(&bytes).ok().flatten();
        }
    }
    if let Some(bytes) = ext {
        // The .EXT file's extended cue list has names and colours; prefer it when present.
        if let Ok(cues) = cues(&bytes)
            && !cues.is_empty()
        {
            a.cues = cues;
        }
        if waveforms {
            // Colour waveforms live in .EXT; keep the .DAT's blue preview otherwise.
            if let Some(p) = preview(&bytes).ok().flatten() {
                a.preview = Some(p);
            }
            a.detail = detail(&bytes).ok().flatten();
            a.structure = song_structure(&bytes).ok().flatten();
        }
    }
    a
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A tiny synthetic ANLZ file: PMAI + PPTH + PQTZ with two beats.
    pub(crate) fn synthetic(path: &str) -> Vec<u8> {
        let mut path_bytes: Vec<u8> = path.encode_utf16().flat_map(u16::to_be_bytes).collect();
        path_bytes.extend_from_slice(&[0, 0]);
        let mut ppth = Vec::new();
        ppth.extend_from_slice(b"PPTH");
        ppth.extend_from_slice(&16u32.to_be_bytes());
        ppth.extend_from_slice(&((16 + path_bytes.len()) as u32).to_be_bytes());
        ppth.extend_from_slice(&(path_bytes.len() as u32).to_be_bytes());
        ppth.extend_from_slice(&path_bytes);
        let mut pqtz = Vec::new();
        pqtz.extend_from_slice(b"PQTZ");
        pqtz.extend_from_slice(&24u32.to_be_bytes());
        pqtz.extend_from_slice(&(24u32 + 16).to_be_bytes());
        pqtz.extend_from_slice(&0u32.to_be_bytes());
        pqtz.extend_from_slice(&0x80000u32.to_be_bytes());
        pqtz.extend_from_slice(&2u32.to_be_bytes());
        for (n, t) in [(1u16, 100u32), (2, 569)] {
            pqtz.extend_from_slice(&n.to_be_bytes());
            pqtz.extend_from_slice(&12800u16.to_be_bytes());
            pqtz.extend_from_slice(&t.to_be_bytes());
        }
        let mut f = Vec::new();
        f.extend_from_slice(b"PMAI");
        f.extend_from_slice(&0x1cu32.to_be_bytes());
        let total = 0x1c + ppth.len() + pqtz.len();
        f.extend_from_slice(&(total as u32).to_be_bytes());
        f.extend_from_slice(&[0u8; 0x1c - 12]);
        f.extend_from_slice(&ppth);
        f.extend_from_slice(&pqtz);
        f
    }

    #[test]
    fn read_and_rewrite_path() {
        let f = synthetic("/Contents/a.mp3");
        assert_eq!(read_path(&f).unwrap().as_deref(), Some("/Contents/a.mp3"));
        let g = rewrite_path(&f, "/Contents/Ärtist/longer name (2).mp3").unwrap();
        assert_eq!(read_path(&g).unwrap().as_deref(), Some("/Contents/Ärtist/longer name (2).mp3"));
        assert_eq!(be_u32(&g, 8).unwrap() as usize, g.len());
        // Other sections survive.
        assert_eq!(beat_grid(&g).unwrap(), beat_grid(&f).unwrap());
        assert_eq!(beat_grid(&g).unwrap().len(), 2);
        assert_eq!(beat_grid(&g).unwrap()[0].bpm, 128.0);
    }

    fn section(tag: &[u8; 4], header: &[u8], body: &[u8]) -> Vec<u8> {
        let mut s = Vec::new();
        s.extend_from_slice(tag);
        s.extend_from_slice(&((12 + header.len()) as u32).to_be_bytes());
        s.extend_from_slice(&((12 + header.len() + body.len()) as u32).to_be_bytes());
        s.extend_from_slice(header);
        s.extend_from_slice(body);
        s
    }

    fn file(sections: &[Vec<u8>]) -> Vec<u8> {
        let body: Vec<u8> = sections.concat();
        let mut f = Vec::new();
        f.extend_from_slice(b"PMAI");
        f.extend_from_slice(&0x1cu32.to_be_bytes());
        f.extend_from_slice(&((0x1c + body.len()) as u32).to_be_bytes());
        f.extend_from_slice(&[0u8; 0x1c - 12]);
        f.extend_from_slice(&body);
        f
    }

    /// `PCPT` per the documented layout: hot_cue, status, u32, order u16 x2, type, u8, u16,
    /// time, loop_time, 16 bytes padding.
    fn pcpt(hot: u32, kind: u8, time: u32, loop_time: u32) -> Vec<u8> {
        let mut h = Vec::new();
        h.extend_from_slice(&hot.to_be_bytes());
        h.extend_from_slice(&4u32.to_be_bytes());
        h.extend_from_slice(&0x10000u32.to_be_bytes());
        h.extend_from_slice(&0xffffu16.to_be_bytes());
        h.extend_from_slice(&0xffffu16.to_be_bytes());
        let mut b = vec![kind, 0];
        b.extend_from_slice(&0x3e8u16.to_be_bytes());
        b.extend_from_slice(&time.to_be_bytes());
        b.extend_from_slice(&loop_time.to_be_bytes());
        b.extend_from_slice(&[0u8; 16]);
        section(b"PCPT", &h, &b)
    }

    /// `PCP2`: hot_cue, type, u8, u16, time, loop_time, color u8, u8, u16, u32, num/den u16 x2,
    /// comment length, UTF-16BE comment, colour code, r, g, b, padding.
    fn pcp2(hot: u32, kind: u8, time: u32, loop_time: u32, comment: &str, rgb: [u8; 3]) -> Vec<u8> {
        pcp2_with(hot, kind, time, loop_time, comment, rgb, 0, (0, 0))
    }

    /// `pcp2` with a colour table row and a loop length in beats.
    #[allow(clippy::too_many_arguments)]
    fn pcp2_with(
        hot: u32,
        kind: u8,
        time: u32,
        loop_time: u32,
        comment: &str,
        rgb: [u8; 3],
        color_id: u8,
        beats: (u16, u16),
    ) -> Vec<u8> {
        let mut c: Vec<u8> = comment.encode_utf16().flat_map(u16::to_be_bytes).collect();
        c.extend_from_slice(&[0, 0]);
        let mut b = vec![kind, 0, 0, 0];
        b.extend_from_slice(&time.to_be_bytes());
        b.extend_from_slice(&loop_time.to_be_bytes());
        b.extend_from_slice(&[color_id, 0, 0, 0, 0, 0, 0, 0]);
        b.extend_from_slice(&beats.0.to_be_bytes());
        b.extend_from_slice(&beats.1.to_be_bytes());
        b.extend_from_slice(&(c.len() as u32).to_be_bytes());
        b.extend_from_slice(&c);
        b.extend_from_slice(&[0x22, rgb[0], rgb[1], rgb[2]]);
        b.extend_from_slice(&[0u8; 4]);
        section(b"PCP2", &hot.to_be_bytes(), &b)
    }

    #[test]
    fn basic_cue_list() {
        let mut hot_list = 1u32.to_be_bytes().to_vec();
        hot_list.extend_from_slice(&[0, 0, 0, 1, 0xff, 0xff, 0xff, 0xff]);
        let mut mem_list = 0u32.to_be_bytes().to_vec();
        mem_list.extend_from_slice(&[0, 0, 0, 2, 0xff, 0xff, 0xff, 0xff]);
        let f = file(&[
            section(b"PCOB", &hot_list, &pcpt(1, 1, 1500, u32::MAX)),
            section(b"PCOB", &mem_list, &[pcpt(0, 1, 500, u32::MAX), pcpt(0, 2, 8000, 16000)].concat()),
        ]);
        let cues = cues(&f).unwrap();
        assert_eq!(cues.len(), 3);
        let memory = Cue {
            hot_cue: None,
            start_ms: 500,
            end_ms: None,
            comment: String::new(),
            color: None,
            color_id: 0,
            loop_beats: None,
        };
        assert_eq!(cues[0], memory);
        assert_eq!(cues[1].end_ms, Some(16000));
        assert_eq!((cues[2].hot_cue, cues[2].start_ms), (Some(0), 1500));
    }

    #[test]
    fn extended_cue_list_wins_and_carries_names() {
        let mut hot_list = 1u32.to_be_bytes().to_vec();
        hot_list.extend_from_slice(&[0, 2, 0, 0]);
        let f = file(&[
            section(b"PCOB", &[0, 0, 0, 1, 0, 0, 0, 1, 0xff, 0xff, 0xff, 0xff], &pcpt(1, 1, 1, u32::MAX)),
            section(
                b"PCO2",
                &hot_list,
                &[pcp2(1, 1, 30_000, u32::MAX, "Drop", [255, 0, 64]), pcp2(3, 2, 60_000, 64_000, "", [0, 0, 0])]
                    .concat(),
            ),
        ]);
        let cues = cues(&f).unwrap();
        assert_eq!(cues.len(), 2);
        assert_eq!(cues[0].comment, "Drop");
        assert_eq!(cues[0].color, Some([255, 0, 64]));
        assert_eq!((cues[1].hot_cue, cues[1].end_ms, cues[1].color), (Some(2), Some(64_000), None));
    }

    #[test]
    fn extended_cues_carry_memory_colours_and_loop_lengths() {
        let f = file(&[section(
            b"PCO2",
            &[0, 0, 0, 0, 0, 3, 0, 0],
            &[
                pcp2_with(0, 1, 1_000, u32::MAX, "", [0, 0, 0], 3, (0, 0)),
                pcp2_with(0, 2, 8_000, 10_000, "", [0, 0, 0], 6, (4, 1)),
                pcp2_with(0, 2, 20_000, 20_250, "", [0, 0, 0], 0, (1, 2)),
            ]
            .concat(),
        )]);
        let cues = cues(&f).unwrap();
        let got: Vec<_> = cues.iter().map(|c| (c.color_id, c.loop_beats)).collect();
        assert_eq!(got, [(3, None), (6, Some((4, 1))), (0, Some((1, 2)))]);
    }

    /// An unmasked `PSSI` (crate-digger's layout) of `mood`, phrases of `(start beat, kind)`
    /// up to beat `end`, the last with a fill-in from beat `end - 4`.
    fn pssi(mood: u16, end: u16, phrases: &[(u16, u16)]) -> Vec<u8> {
        let mut h = 24u32.to_be_bytes().to_vec();
        h.extend_from_slice(&(phrases.len() as u16).to_be_bytes());
        h.extend_from_slice(&mood.to_be_bytes());
        h.extend_from_slice(&[0; 6]);
        h.extend_from_slice(&end.to_be_bytes());
        h.extend_from_slice(&[0; 4]);
        let mut b = Vec::new();
        for (i, &(beat, kind)) in phrases.iter().enumerate() {
            let mut e = vec![0u8; 24];
            e[0..2].copy_from_slice(&(i as u16 + 1).to_be_bytes());
            e[2..4].copy_from_slice(&beat.to_be_bytes());
            e[4..6].copy_from_slice(&kind.to_be_bytes());
            if i == phrases.len() - 1 {
                e[21] = 1;
                e[22..24].copy_from_slice(&(end - 4).to_be_bytes());
            }
            b.extend(e);
        }
        section(b"PSSI", &h, &b)
    }

    /// Phrase kinds are named by the track's mood, as rekordbox names them.
    #[test]
    fn phrases_are_named_by_mood() {
        let read = |mood, phrases: &[(u16, u16)]| song_structure(&file(&[pssi(mood, 97, phrases)])).unwrap();
        let mid = read(2, &[(1, 1), (17, 2), (33, 5), (49, 8), (65, 9), (81, 10)]).unwrap();
        assert_eq!(mid.mood, Mood::Mid);
        let names: Vec<String> = mid.phrases.iter().map(|p| p.kind.name()).collect();
        assert_eq!(names, ["Intro", "Verse 1", "Verse 4", "Bridge", "Chorus", "Outro"]);
        let spans: Vec<(u16, u16)> = mid.phrases.iter().map(|p| (p.start_beat, p.end_beat)).collect();
        assert_eq!(spans, [(1, 17), (17, 33), (33, 49), (49, 65), (65, 81), (81, 97)]);
        assert_eq!(mid.phrases.iter().map(|p| p.fill_beat).collect::<Vec<_>>()[4..], [None, Some(93)]);

        let low = read(3, &[(1, 3), (9, 6), (17, 8), (25, 42)]).unwrap();
        assert_eq!(low.mood, Mood::Low);
        let names: Vec<String> = low.phrases.iter().map(|p| p.kind.name()).collect();
        assert_eq!(names, ["Verse 1", "Verse 2", "Bridge", "Phrase 42"]);
        let high = read(1, &[(1, 2), (9, 3), (17, 5), (25, 6)]).unwrap();
        let names: Vec<String> = high.phrases.iter().map(|p| p.kind.name()).collect();
        assert_eq!(
            (high.mood, names),
            (Mood::High, vec!["Up".to_string(), "Down".into(), "Chorus".into(), "Outro".into()])
        );
        // Not a mood rekordbox writes: no structure, rather than a wrong one.
        assert_eq!(read(7, &[(1, 1)]), None);
        // Cut short before its entry count.
        let truncated = file(&[section(b"PSSI", &[], &[])]);
        assert!(song_structure(&truncated).unwrap_err().to_string().contains("truncated PSSI"));
    }

    /// The blue detail waveform (`PWV3`), for files without a colour one: as many columns as the
    /// header says (no more than are there), at the rate it says, or 150 a second.
    #[test]
    fn the_blue_detail_waveform_is_read_when_theres_no_colour_one() {
        let pwv3 = |count: u32, rate: u16, body: &[u8]| {
            let mut h = 1u32.to_be_bytes().to_vec();
            h.extend_from_slice(&count.to_be_bytes());
            h.extend_from_slice(&rate.to_be_bytes());
            h.extend_from_slice(&[0, 0]);
            file(&[section(b"PWV3", &h, body)])
        };
        let w = detail(&pwv3(2, 0, &[0x1f, 0xe0, 0x1f])).unwrap().unwrap();
        assert_eq!((w.style, w.columns_per_sec, w.columns.len()), (WaveformStyle::Blue, Some(150.0), 2));
        assert_eq!((w.columns[0].back.height, w.columns[1].back.height), (1.0, 0.0));
        assert_eq!(w.columns[1].back.color, [200, 230, 255], "all white");
        assert_eq!(detail(&pwv3(9, 300, &[0x1f])).unwrap().unwrap().columns_per_sec, Some(300.0));
        assert_eq!(detail(&file(&[])).unwrap(), None);
    }

    /// An entry claiming a length shorter than its own header ends the list there.
    #[test]
    fn a_cue_entry_of_impossible_length_ends_the_list() {
        let mut entries = [pcpt(0, 1, 500, u32::MAX), pcpt(0, 1, 900, u32::MAX)].concat();
        let second = entries.len() / 2;
        entries[second + 8..second + 12].copy_from_slice(&4u32.to_be_bytes());
        let list = section(b"PCOB", &[0, 0, 0, 0, 0, 0, 0, 2, 0xff, 0xff, 0xff, 0xff], &entries);
        assert_eq!(cues(&file(&[list])).unwrap().iter().map(|c| c.start_ms).collect::<Vec<_>>(), [500]);
    }

    #[test]
    fn rejects_garbage() {
        assert!(read_path(b"nope").is_err());
        let mut f = synthetic("/x");
        f[8..12].copy_from_slice(&9999u32.to_be_bytes());
        assert!(read_path(&f).is_err());
    }
}
