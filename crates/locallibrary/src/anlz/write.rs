//! Reading a track's full cue list from its analysis files and writing a new one into them.
//!
//! rekordbox keeps every cue three times over: a `PCOB` list of `PCPT` entries for hot cues and
//! one for memory cues in `.DAT` (read by every player), the same pair in `.EXT`, and a `PCO2`
//! list of `PCP2` entries for each in `.EXT` (nexus 2 and later: names, colours, loop lengths in
//! beats). In a rekordbox 7 export the `.DAT` hot cue list holds hot cues A-C and the `.EXT` one
//! D-H, the `.EXT` memory list stays empty, and `PCO2` holds them all (observed by baken, MIT,
//! whose byte-level fixtures the tests below reproduce). A track without an `.EXT` keeps every
//! cue in its `.DAT`.
//!
//! [`write_cue_points`] regenerates only the lists whose cues changed: a list holding exactly
//! the wanted cues is kept byte for byte, and an entry read from the file keeps every byte this
//! module doesn't model (status, unknown fields, padding) when it's written again. New entries
//! use the layout rekordbox 7 writes. Everything outside the cue lists is copied unchanged.
//!
//! `PCPT`: tag, header_len 0x1c, len 0x38, hot_cue u32 (0 = memory, 1 = A...), status u32 (4 =
//! active loop), u32 (0x00010000), previous u16, next u16 (memory lists chain their entries by
//! position; hot lists use 0xffff), type u8 (1 point, 2 loop), u8, u16 (0x03e8), time u32 (ms),
//! loop end u32 (ms, 0xffffffff for points), 16 bytes.
//!
//! `PCP2`: tag, header_len 0x10, len, hot_cue u32, type u8, u8, u16 (0x03e8), time u32, loop end
//! u32, colour id u8 (memory cues: rekordbox's colour table, 1 pink ... 8 purple), 7 bytes,
//! loop numerator u16, denominator u16, comment length u32 (0 when empty), UTF-16BE comment with
//! a NUL, hot cue colour code u8 (rekordbox's palette, 1-62) and RGB, 40 bytes.

use schemars::JsonSchema;
use serde::Serialize;

use super::*;
use crate::{Error, Result};

const PCOB: [u8; 4] = *b"PCOB";
const PCO2: [u8; 4] = *b"PCO2";
const HOT: u32 = 1;
const MEMORY: u32 = 0;
const NO_LOOP: u32 = u32::MAX;
const ACTIVE_LOOP: u32 = 4;
const PCPT_LEN: usize = 0x38;
/// `PCP2` up to the comment length field.
const PCP2_HEAD: usize = 0x2c;
/// Bytes rekordbox 7 writes after a `PCP2` entry's colour.
const PCP2_PADDING: usize = 40;

/// A cue point with everything the analysis files store about it.
#[derive(Debug, Clone, Default, PartialEq, Serialize, JsonSchema)]
pub struct CuePoint {
    /// `None` for memory cues; `Some(0)` = hot cue A, `Some(1)` = B, ...
    pub hot_cue: Option<u8>,
    pub start_ms: u32,
    /// End of the loop, for loops.
    pub end_ms: Option<u32>,
    /// A quantised loop's length in beats, as numerator and denominator (4/1, 1/2...).
    #[schemars(with = "Option<super::LoopBeats>")]
    pub loop_beats: Option<(u16, u16)>,
    pub comment: String,
    /// Memory cue colour: a row of rekordbox's colour table (1 pink ... 8 purple), 0 for none.
    pub color_id: u8,
    /// Hot cue colour: a code of rekordbox's palette ([`HOT_CUE_PALETTE`]), 0 for the default.
    pub color_code: u8,
    /// The RGB stored with the hot cue colour (what players light their pads with).
    pub color_rgb: [u8; 3],
    /// The loop is active when the player reaches it (`PCPT` status 4).
    pub active_loop: bool,
    /// The entries this cue was read from, so writing it back keeps what isn't modelled.
    #[serde(skip)]
    pub raw: RawCue,
}

/// A cue's entries as read from a file.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RawCue {
    pcpt: Option<Vec<u8>>,
    pcp2: Option<Vec<u8>>,
}

type PcptKey = (Option<u8>, u32, Option<u32>, bool);
type Pcp2Key<'a> = (Option<u8>, u32, Option<u32>, Option<(u16, u16)>, &'a str, u8, u8, [u8; 3]);

impl CuePoint {
    pub fn is_loop(&self) -> bool {
        self.end_ms.is_some()
    }

    /// Whether two cues are the same apart from how they're stored.
    pub fn same_as(&self, other: &CuePoint) -> bool {
        self.pcp2_key() == other.pcp2_key() && self.active_loop == other.active_loop
    }

    /// What a `PCPT` entry stores.
    fn pcpt_key(&self) -> PcptKey {
        (self.hot_cue, self.start_ms, self.end_ms, self.active_loop && self.is_loop())
    }

    /// What a `PCP2` entry stores.
    fn pcp2_key(&self) -> Pcp2Key<'_> {
        (
            self.hot_cue,
            self.start_ms,
            self.end_ms,
            self.loop_beats,
            &self.comment,
            self.color_id,
            self.color_code,
            self.color_rgb,
        )
    }

    /// Memory cues by position, then hot cues A-H.
    fn order(&self) -> (u32, u32) {
        (self.hot_cue.map_or(0, |h| u32::from(h) + 1), self.start_ms)
    }
}

/// Which analysis file is being written: it decides which cues each of its lists holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CueFile {
    /// The `.DAT` of a track that also has an `.EXT`: hot cues A-C and the memory cues.
    Dat,
    /// A `.DAT` with no `.EXT` next to it: every cue.
    DatAlone,
    /// The `.EXT`: hot cues D-H in `PCOB` (its memory list stays empty), and every cue in `PCO2`.
    Ext,
}

impl CueFile {
    /// The lists the file has, in rekordbox's order.
    fn lists(self) -> &'static [([u8; 4], u32)] {
        match self {
            CueFile::Dat | CueFile::DatAlone => &[(PCOB, HOT), (PCOB, MEMORY)],
            CueFile::Ext => &[(PCOB, HOT), (PCOB, MEMORY), (PCO2, HOT), (PCO2, MEMORY)],
        }
    }

    /// The cues list `(tag, list)` of this file holds.
    fn wanted<'a>(self, tag: &[u8; 4], list: u32, hot: &[&'a CuePoint], memory: &[&'a CuePoint]) -> Vec<&'a CuePoint> {
        let in_dat = |c: &&&CuePoint| c.hot_cue.is_some_and(|h| h < 3);
        match (tag, list, self) {
            (b"PCOB", HOT, CueFile::Dat) => hot.iter().filter(in_dat).copied().collect(),
            (b"PCOB", HOT, CueFile::Ext) => hot.iter().filter(|c| !in_dat(c)).copied().collect(),
            (b"PCOB", MEMORY, CueFile::Ext) => Vec::new(),
            (_, HOT, _) => hot.to_vec(),
            (_, MEMORY, _) => memory.to_vec(),
            _ => Vec::new(),
        }
    }
}

pub(super) fn put_u16(b: &mut [u8], o: usize, v: u16) {
    b[o..o + 2].copy_from_slice(&v.to_be_bytes());
}

pub(super) fn put_u32(b: &mut [u8], o: usize, v: u32) {
    b[o..o + 4].copy_from_slice(&v.to_be_bytes());
}

/// A cue list section's entries (whatever their tag), as slices.
fn list_entries<'a>(bytes: &'a [u8], s: &SectionRef) -> Vec<&'a [u8]> {
    let mut out = Vec::new();
    let mut pos = s.start + s.header_len;
    let end = s.start + s.total_len;
    while pos + 12 <= end {
        let Some(t) = be_u32(bytes, pos + 8).map(|t| t as usize) else { break };
        if t < 12 || pos + t > end {
            break;
        }
        out.push(&bytes[pos..pos + t]);
        pos += t;
    }
    out
}

/// A cue list section's list type: 0 memory, 1 hot. Other sections, and lists of a type never
/// seen, are `None` (and left as they are).
fn list_type(bytes: &[u8], s: &SectionRef) -> Option<u32> {
    let cue_list = matches!(&s.tag, b"PCOB" | b"PCO2") && s.header_len >= 16;
    cue_list.then(|| be_u32(bytes, s.start + 12)).flatten().filter(|t| *t == HOT || *t == MEMORY)
}

/// The hot cue field (0 = memory cue, 1 = A...) as a slot.
pub(super) fn slot(hot: u32) -> Option<Option<u8>> {
    match hot {
        0 => Some(None),
        1..=256 => Some(Some((hot - 1) as u8)),
        _ => None,
    }
}

fn hot_number(c: &CuePoint) -> u32 {
    c.hot_cue.map_or(0, |h| u32::from(h) + 1)
}

fn pcpt_point(e: &[u8]) -> Option<CuePoint> {
    if !e.starts_with(b"PCPT") {
        return None;
    }
    let time = be_u32(e, 0x20)?;
    let is_loop = *e.get(0x1c)? == 2;
    Some(CuePoint {
        hot_cue: slot(be_u32(e, 0x0c)?)?,
        start_ms: time,
        end_ms: loop_end(time, be_u32(e, 0x24)?, is_loop),
        active_loop: be_u32(e, 0x10)? == ACTIVE_LOOP,
        raw: RawCue { pcpt: Some(e.to_vec()), pcp2: None },
        ..Default::default()
    })
}

/// A `PCP2` entry's comment bytes (without the length field) and everything after them.
fn pcp2_comment(e: &[u8]) -> (&[u8], &[u8]) {
    let len = be_u32(e, 0x28).map_or(0, |n| n as usize);
    let start = PCP2_HEAD.min(e.len());
    let end = start.saturating_add(len).min(e.len());
    (&e[start..end], &e[end..])
}

fn pcp2_point(e: &[u8]) -> Option<CuePoint> {
    if !e.starts_with(b"PCP2") {
        return None;
    }
    let time = be_u32(e, 0x14)?;
    let is_loop = *e.get(0x10)? == 2;
    let (comment, tail) = pcp2_comment(e);
    let color = tail.get(..4).unwrap_or(&[0; 4]);
    Some(CuePoint {
        hot_cue: slot(be_u32(e, 0x0c)?)?,
        start_ms: time,
        end_ms: loop_end(time, be_u32(e, 0x18)?, is_loop),
        loop_beats: match (be_u16(e, 0x24), be_u16(e, 0x26)) {
            (Some(n), Some(d)) if n > 0 && d > 0 => Some((n, d)),
            _ => None,
        },
        comment: utf16be(comment),
        color_id: e.get(0x1c).copied().unwrap_or(0),
        color_code: color[0],
        color_rgb: [color[1], color[2], color[3]],
        active_loop: false,
        raw: RawCue { pcpt: None, pcp2: Some(e.to_vec()) },
    })
}

/// Every cue in a track's analysis files: the `.EXT`'s `PCO2` lists, with the `PCOB` entries
/// for the same cues attached, plus any cue only a `PCOB` list has. Memory cues by position come
/// first, then hot cues A-H.
pub fn read_cue_points(dat: &[u8], ext: Option<&[u8]>) -> Result<Vec<CuePoint>> {
    let mut extended = Vec::new();
    // `PCPT` entries with the file (0 `.DAT`, 1 `.EXT`) they came from.
    let mut basic = Vec::new();
    for (file, bytes) in std::iter::once(dat).chain(ext).enumerate() {
        for s in sections(bytes)? {
            match (&s.tag, list_type(bytes, &s)) {
                (_, None) => {}
                (b"PCO2", _) => extended.extend(list_entries(bytes, &s).into_iter().filter_map(pcp2_point)),
                _ => basic.extend(list_entries(bytes, &s).into_iter().filter_map(pcpt_point).map(|c| (file, c))),
            }
        }
    }
    let named = extended.len();
    let mut out = extended;
    // The file each cue's `PCPT` entry came from.
    let mut origin: Vec<Option<usize>> = vec![None; named];
    for (file, c) in basic {
        // The `PCP2` entry of the same position and loop end, or failing that any at the same
        // position: a memory cue and a loop can start together, and the two files' lists need
        // not be in the same order (an unchanged list is kept as it was).
        let unpaired = |o: &CuePoint| o.raw.pcpt.is_none() && o.hot_cue == c.hot_cue && o.start_ms == c.start_ms;
        let pair = out[..named]
            .iter()
            .position(|o| unpaired(o) && o.end_ms == c.end_ms)
            .or_else(|| out[..named].iter().position(unpaired));
        if let Some(i) = pair {
            out[i].active_loop = c.active_loop;
            out[i].raw.pcpt = c.raw.pcpt;
            origin[i] = Some(file);
        } else if !out.iter().zip(&origin).any(|(o, f)| f.is_some_and(|f| f != file) && o.pcpt_key() == c.pcpt_key()) {
            // Older exports list a cue in both files' `PCOB`; keep one. (Two alike in one file
            // are two cues.)
            out.push(c);
            origin.push(Some(file));
        }
    }
    out.sort_by_key(CuePoint::order);
    Ok(out)
}

fn cue_type(c: &CuePoint, stored: u8) -> u8 {
    match (c.is_loop(), stored) {
        (true, _) => 2,
        (false, 2) => 1,
        (false, t) => t,
    }
}

/// A `PCPT` entry for `c`: the one it was read from with the modelled fields replaced, or
/// rekordbox 7's layout.
fn pcpt_entry(c: &CuePoint, previous: u16, next: u16) -> Vec<u8> {
    let mut e = match &c.raw.pcpt {
        Some(raw) if raw.len() >= 0x28 => raw.clone(),
        _ => {
            let mut e = vec![0u8; PCPT_LEN];
            e[..4].copy_from_slice(b"PCPT");
            put_u32(&mut e, 0x04, 0x1c);
            put_u32(&mut e, 0x14, 0x0001_0000);
            e[0x1c] = 1;
            put_u16(&mut e, 0x1e, 0x03e8);
            e
        }
    };
    put_u32(&mut e, 0x0c, hot_number(c));
    let status = match be_u32(&e, 0x10).unwrap_or(0) {
        _ if c.active_loop && c.is_loop() => ACTIVE_LOOP,
        ACTIVE_LOOP => 0,
        s => s,
    };
    put_u32(&mut e, 0x10, status);
    put_u16(&mut e, 0x18, previous);
    put_u16(&mut e, 0x1a, next);
    e[0x1c] = cue_type(c, e[0x1c]);
    put_u32(&mut e, 0x20, c.start_ms);
    put_u32(&mut e, 0x24, c.end_ms.unwrap_or(NO_LOOP));
    let len = e.len() as u32;
    put_u32(&mut e, 0x08, len);
    e
}

fn encode_comment(s: &str) -> Vec<u8> {
    if s.is_empty() {
        return Vec::new();
    }
    s.encode_utf16().chain(std::iter::once(0)).flat_map(u16::to_be_bytes).collect()
}

/// A `PCP2` entry for `c`, like [`pcpt_entry`].
fn pcp2_entry(c: &CuePoint) -> Vec<u8> {
    let raw = c.raw.pcp2.as_deref().filter(|r| r.len() >= PCP2_HEAD);
    let mut e = match raw {
        Some(r) => r[..PCP2_HEAD].to_vec(),
        None => {
            let mut e = vec![0u8; PCP2_HEAD];
            e[..4].copy_from_slice(b"PCP2");
            put_u32(&mut e, 0x04, 0x10);
            e[0x10] = 1;
            put_u16(&mut e, 0x12, 0x03e8);
            e[0x1d] = 1;
            e
        }
    };
    put_u32(&mut e, 0x0c, hot_number(c));
    e[0x10] = cue_type(c, e[0x10]);
    put_u32(&mut e, 0x14, c.start_ms);
    put_u32(&mut e, 0x18, c.end_ms.unwrap_or(NO_LOOP));
    e[0x1c] = c.color_id;
    let (numerator, denominator) = c.loop_beats.unwrap_or((0, 0));
    put_u16(&mut e, 0x24, numerator);
    put_u16(&mut e, 0x26, denominator);
    let stored = raw.map(pcp2_comment);
    // An unchanged comment keeps its exact encoding (some files store an empty one as a NUL).
    let comment = match stored {
        Some((bytes, _)) if utf16be(bytes) == c.comment => bytes.to_vec(),
        _ => encode_comment(&c.comment),
    };
    put_u32(&mut e, 0x28, comment.len() as u32);
    e.extend_from_slice(&comment);
    let color = [c.color_code, c.color_rgb[0], c.color_rgb[1], c.color_rgb[2]];
    match stored.map(|(_, tail)| tail) {
        Some(tail) if tail.len() >= 4 => {
            e.extend_from_slice(&color);
            e.extend_from_slice(&tail[4..]);
        }
        // The entry ended before the colour (older exports): keep it short unless one is set.
        Some(tail) if color == [0; 4] => e.extend_from_slice(tail),
        Some(_) => e.extend_from_slice(&color),
        None => {
            e.extend_from_slice(&color);
            e.extend_from_slice(&[0; PCP2_PADDING]);
        }
    }
    let len = e.len() as u32;
    put_u32(&mut e, 0x08, len);
    e
}

/// A cue list section holding `cues`, with the header of `existing` (if it has a full one).
fn list_section(tag: &[u8; 4], list: u32, existing: Option<&[u8]>, cues: &[&CuePoint]) -> Vec<u8> {
    let pcob = tag == b"PCOB";
    let header_len = if pcob { 0x18 } else { 0x14 };
    let mut out = match existing {
        Some(h) if h.len() >= header_len => h.to_vec(),
        _ => {
            let mut h = vec![0u8; header_len];
            h[..4].copy_from_slice(tag);
            put_u32(&mut h, 4, header_len as u32);
            h
        }
    };
    put_u32(&mut out, 0x0c, list);
    let n = cues.len() as u16;
    if pcob {
        put_u16(&mut out, 0x12, n);
        // Observed: u32::MAX for hot cue lists and empty memory lists, else one less than the count.
        let memory_count = if list == HOT || cues.is_empty() { u32::MAX } else { cues.len() as u32 - 1 };
        put_u32(&mut out, 0x14, memory_count);
    } else {
        put_u16(&mut out, 0x10, n);
    }
    for (i, c) in cues.iter().enumerate() {
        if pcob {
            // Memory cues are chained by position: the previous and next entry, 0xffff for none.
            let (previous, next) = match list {
                HOT => (0xffff, 0xffff),
                _ => (
                    if i == 0 { 0xffff } else { (i - 1) as u16 },
                    if i + 1 == cues.len() { 0xffff } else { (i + 1) as u16 },
                ),
            };
            out.extend_from_slice(&pcpt_entry(c, previous, next));
        } else {
            out.extend_from_slice(&pcp2_entry(c));
        }
    }
    let len = out.len() as u32;
    put_u32(&mut out, 8, len);
    out
}

/// Cues by slot, position and loop end, keeping the order of those alike.
fn by_position(mut cues: Vec<&CuePoint>) -> Vec<&CuePoint> {
    cues.sort_by_key(|c| (c.order(), c.end_ms));
    cues
}

/// The list section `s` holding `cues`: unchanged if it already holds exactly them.
fn rewrite_list(bytes: &[u8], s: &SectionRef, list: u32, cues: &[&CuePoint]) -> Vec<u8> {
    let section = &bytes[s.start..s.start + s.total_len];
    let entries = list_entries(bytes, s);
    // The same cues, in any order but one: cues of the same slot, position and loop end are
    // paired with their entries in the other file's lists by order (see `read_cue_points`), so
    // those must stay in the wanted order.
    let unchanged = if &s.tag == b"PCOB" {
        let found: Vec<CuePoint> = entries.iter().filter_map(|e| pcpt_point(e)).collect();
        let have: Vec<PcptKey> = by_position(found.iter().collect()).iter().map(|c| c.pcpt_key()).collect();
        let want: Vec<PcptKey> = by_position(cues.to_vec()).iter().map(|c| c.pcpt_key()).collect();
        found.len() == entries.len() && have == want
    } else {
        let found: Vec<CuePoint> = entries.iter().filter_map(|e| pcp2_point(e)).collect();
        let have: Vec<Pcp2Key<'_>> = by_position(found.iter().collect()).iter().map(|c| c.pcp2_key()).collect();
        let want: Vec<Pcp2Key<'_>> = by_position(cues.to_vec()).iter().map(|c| c.pcp2_key()).collect();
        found.len() == entries.len() && have == want
    };
    if unchanged {
        return section.to_vec();
    }
    list_section(&s.tag, list, Some(&section[..s.header_len]), cues)
}

/// Return a copy of an analysis file whose cue lists hold `cues` (see the module docs for
/// which list holds what). Files without cue lists get them after their other sections.
pub fn write_cue_points(bytes: &[u8], file: CueFile, cues: &[CuePoint]) -> Result<Vec<u8>> {
    let secs = sections(bytes)?;
    if cues.len() > usize::from(u16::MAX) {
        return Err(Error::invalid("too many cues"));
    }
    if let Some(c) = cues.iter().find(|c| c.comment.len() > 0x10000) {
        return Err(Error::invalid(format!(
            "cue name is too long: {:?}...",
            c.comment.chars().take(20).collect::<String>()
        )));
    }
    let mut hot: Vec<&CuePoint> = cues.iter().filter(|c| c.hot_cue.is_some()).collect();
    let mut memory: Vec<&CuePoint> = cues.iter().filter(|c| c.hot_cue.is_none()).collect();
    hot.sort_by_key(|c| c.order());
    memory.sort_by_key(|c| c.order());

    let header_len = be_u32(bytes, 4).expect("checked by sections") as usize;
    let file_len = be_u32(bytes, 8).expect("checked by sections") as usize;
    // Each section's bytes, and the tag of the cue lists among them.
    let mut chunks: Vec<(Option<[u8; 4]>, Vec<u8>)> = Vec::new();
    let mut seen: Vec<([u8; 4], u32)> = Vec::new();
    for s in &secs {
        let Some(list) = list_type(bytes, s) else {
            chunks.push((None, bytes[s.start..s.start + s.total_len].to_vec()));
            continue;
        };
        // A second list of the same kind (never seen in practice) is emptied, so no reader
        // finds stale cues in it.
        let first = !seen.contains(&(s.tag, list));
        seen.push((s.tag, list));
        let wanted = if first { file.wanted(&s.tag, list, &hot, &memory) } else { Vec::new() };
        chunks.push((Some(s.tag), rewrite_list(bytes, s, list, &wanted)));
    }
    // Missing lists go where rekordbox puts them: after the other lists, `PCOB` before `PCO2`.
    for (tag, list) in file.lists().iter().filter(|l| !seen.contains(l)) {
        let after_lists = chunks.iter().rposition(|(t, _)| t.is_some()).map_or(chunks.len(), |i| i + 1);
        let at = match tag {
            &PCOB => chunks.iter().position(|(t, _)| t == &Some(PCO2)).unwrap_or(after_lists),
            _ => after_lists,
        };
        chunks.insert(at, (Some(*tag), list_section(tag, *list, None, &file.wanted(tag, *list, &hot, &memory))));
    }

    let last_end = secs.last().map_or(header_len, |s| s.start + s.total_len);
    let mut out = bytes[..header_len].to_vec();
    for (_, c) in &chunks {
        out.extend_from_slice(c);
    }
    out.extend_from_slice(&bytes[last_end..file_len]);
    let len = u32::try_from(out.len()).map_err(|_| malformed("file too large"))?;
    put_u32(&mut out, 8, len);
    // Anything after the declared file length (never seen in practice) is preserved.
    out.extend_from_slice(&bytes[file_len..]);

    // Every other section must have come through unchanged.
    let others = |b: &[u8], secs: &[SectionRef]| -> Vec<Vec<u8>> {
        secs.iter().filter(|s| list_type(b, s).is_none()).map(|s| b[s.start..s.start + s.total_len].to_vec()).collect()
    };
    if others(&out, &sections(&out)?) != others(bytes, &secs) {
        return Err(malformed("rewritten cue lists disturbed other sections"));
    }
    Ok(out)
}

/// A copy of an analysis file with every time in it `ms` later (earlier when negative), for a
/// track pointed at another file of its recording whose audio starts elsewhere: the beat grid
/// (`PQTZ`, and the first and last beat `PQT2` repeats), each cue's position and loop end (`PCPT`,
/// `PCP2`), and the detail waveforms (`PWV3`, `PWV5`, `PWV7`, 150 columns a second, moved by
/// whole columns and kept as long). Times that would come before the start go to the start: a
/// beat isn't dropped, since phrases (`PSSI`) and `PQT2`'s body count beats. Everything else is
/// copied unchanged; whole-track previews move by under a column.
pub fn shift_times(bytes: &[u8], ms: i32) -> Result<Vec<u8>> {
    let secs = sections(bytes)?;
    let shifted = |t: u32| (i64::from(t) + i64::from(ms)).clamp(0, i64::from(u32::MAX)) as u32;
    let shift_at = |section: &mut Vec<u8>, at: usize| {
        if let Some(t) = be_u32(section, at) {
            put_u32(section, at, shifted(t));
        }
    };
    let header_len = be_u32(bytes, 4).expect("checked by sections") as usize;
    let file_len = be_u32(bytes, 8).expect("checked by sections") as usize;
    let mut out = bytes[..header_len].to_vec();
    for s in &secs {
        let mut section = bytes[s.start..s.start + s.total_len].to_vec();
        match &s.tag {
            b"PQTZ" => {
                // The count comes from the file: never trust it past the section's end.
                let count = (be_u32(&section, 20).unwrap_or(0) as usize).min((s.total_len - s.header_len) / 8);
                for i in 0..count {
                    shift_at(&mut section, s.header_len + i * 8 + 4);
                }
            }
            b"PQT2" => {
                shift_at(&mut section, 0x1c);
                shift_at(&mut section, 0x24);
            }
            b"PWV3" | b"PWV5" | b"PWV7" => shift_waveform(&mut section, s.header_len, ms, detail_rate(bytes, s)),
            _ if list_type(bytes, s).is_some() => {
                let mut at = s.header_len;
                for e in list_entries(bytes, s) {
                    // Where the type (2 for a loop), position and loop end are.
                    let fields = match &e[..4] {
                        b"PCPT" => Some((0x1c, 0x20, 0x24)),
                        b"PCP2" => Some((0x10, 0x14, 0x18)),
                        _ => None,
                    };
                    if let Some((kind, time, end)) = fields.filter(|(_, _, end)| e.len() >= end + 4) {
                        shift_at(&mut section, at + time);
                        if e[kind] == 2 {
                            shift_at(&mut section, at + end);
                        }
                    }
                    at += e.len();
                }
            }
            _ => {}
        }
        out.extend_from_slice(&section);
    }
    let last_end = secs.last().map_or(header_len, |s| s.start + s.total_len);
    out.extend_from_slice(&bytes[last_end..file_len]);
    let len = u32::try_from(out.len()).map_err(|_| malformed("file too large"))?;
    put_u32(&mut out, 8, len);
    out.extend_from_slice(&bytes[file_len..]);
    Ok(out)
}

/// A detail waveform section's columns moved `ms` later at `rate` columns a second: silent
/// columns in front and as many dropped from the end, or the reverse, so its length stays the
/// one the other waveforms have.
fn shift_waveform(section: &mut [u8], header_len: usize, ms: i32, rate: f64) {
    let (Some(width), Some(count)) = (be_u32(section, 12), be_u32(section, 16)) else { return };
    let body = &mut section[header_len..];
    let len = (width as usize * count as usize).min(body.len());
    let body = &mut body[..len];
    // Width and rate come from the file, so the product can be anything: it saturates, and a
    // shift past either end empties the waveform.
    let columns = (f64::from(ms) * rate / 1000.0).round() as isize;
    let by = columns.saturating_mul(width as isize).clamp(-(len as isize), len as isize);
    if by > 0 {
        body.rotate_right(by as usize);
        body[..by as usize].fill(0);
    } else if by < 0 {
        body.rotate_left(by.unsigned_abs());
        body[len - by.unsigned_abs()..].fill(0);
    }
}

#[cfg(test)]
mod tests;
