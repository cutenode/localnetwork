//! Writing a beat grid (`PQTZ`, in `.DAT`).
//!
//! `PQTZ` ([DJL's "Beat Grid Tag"](https://djl-analysis.deepsymmetry.org/rekordbox-export-analysis/anlz.html#beat-grid-tag)):
//! header 0x18, two u32 nobody has worked out (0 and 0x00080000 in rekordbox 6 exports, kept as a
//! file has them), the beat count u32; then 8 bytes a beat: its number in its bar u16 (1-4), the
//! tempo there u16 (BPM x 100) and its time u32 (ms).
//!
//! The `.EXT`'s extended grid (`PQT2`) repeats the first and last beat in its header, but its
//! body (two bytes a beat: [pyrekordbox](https://github.com/dylanljones/pyrekordbox/blob/master/pyrekordbox/anlz/structs.py)
//! calls the first `beat` and leaves the second unexplained, and in rekordbox 6 exports the first
//! doesn't count beats in the bar) isn't understood, so it's never written: a new grid takes it out
//! of the `.EXT` ([`without_extended_grid`]) rather than leave one that contradicts it (how players
//! take an `.EXT` without one is for players to show). Moving a whole grid by some milliseconds
//! keeps it ([`super::shift_times`]).

use super::tags::{only, put, section};
use super::*;

/// The header fields `PQTZ` has before its beat count, as rekordbox 6 writes them.
const HEADER: [u8; 8] = [0, 0, 0, 0, 0, 0x08, 0, 0];

/// The beats a grid can hold: in order, each 1-4 in its bar, at a tempo of hundredths of a BPM
/// up to 655.35. The tempos as `PQTZ` keeps them (to hundredths), or why not.
pub fn checked_grid(beats: &[Beat]) -> Result<Vec<Beat>> {
    if beats.is_empty() {
        return Err(Error::invalid("a beat grid needs a beat"));
    }
    let mut out = Vec::with_capacity(beats.len());
    for (i, b) in beats.iter().enumerate() {
        let n = i + 1;
        if !(1..=4).contains(&b.beat_number) {
            return Err(Error::invalid(format!("beat {n} is beat {} of its bar: 1-4", b.beat_number)));
        }
        let hundredths = (b.bpm * 100.0).round();
        if !(1.0..=f64::from(u16::MAX)).contains(&hundredths) {
            return Err(Error::invalid(format!("beat {n}'s tempo, {} BPM, isn't 0.01-655.35", b.bpm)));
        }
        if out.last().is_some_and(|p: &Beat| p.time_ms >= b.time_ms) {
            return Err(Error::invalid(format!("beat {n} isn't after the one before it")));
        }
        out.push(Beat { bpm: hundredths / 100.0, ..*b });
    }
    u32::try_from(out.len()).map_err(|_| Error::invalid("too many beats"))?;
    Ok(out)
}

/// A copy of an analysis file (a `.DAT`) holding `beats` as its beat grid ([`checked_grid`]),
/// added when it has none. Unchanged beats keep the file as it was, byte for byte.
pub fn write_beat_grid(bytes: &[u8], beats: &[Beat]) -> Result<Vec<u8>> {
    let beats = checked_grid(beats)?;
    let secs = sections(bytes)?;
    let existing = only(&secs, b"PQTZ")?;
    let mut header = existing
        .filter(|s| s.header_len == 0x18)
        .map_or(HEADER.to_vec(), |s| bytes[s.start + 12..s.start + 20].to_vec());
    header.extend_from_slice(&(beats.len() as u32).to_be_bytes());
    let mut body = Vec::with_capacity(beats.len() * 8);
    for b in &beats {
        body.extend_from_slice(&b.beat_number.to_be_bytes());
        body.extend_from_slice(&((b.bpm * 100.0).round() as u16).to_be_bytes());
        body.extend_from_slice(&b.time_ms.to_be_bytes());
    }
    let out = put(bytes, b"PQTZ", Some(&section(b"PQTZ", &header, &body)?))?;
    // Never a grid the reader doesn't read back as given.
    if beat_grid(&out)? != beats {
        return Err(malformed("the beat grid written doesn't read back"));
    }
    Ok(out)
}

/// `beats` at twice the tempo: a beat halfway between each two, numbered on through the bar.
/// The first beat keeps its place, counted in the new beats (its old bar's beats 1 and 3 become
/// downbeats), and no beat goes after the last. Checked when written ([`checked_grid`]).
pub fn doubled_grid(beats: &[Beat]) -> Vec<Beat> {
    let Some(first) = beats.first() else { return Vec::new() };
    let start = 2 * (first.beat_number.clamp(1, 4) - 1);
    let mut out = Vec::with_capacity(beats.len() * 2);
    for (i, b) in beats.iter().enumerate() {
        out.push(Beat { bpm: b.bpm * 2.0, time_ms: b.time_ms, beat_number: 0 });
        if let Some(next) = beats.get(i + 1) {
            let half = b.time_ms + next.time_ms.saturating_sub(b.time_ms) / 2;
            out.push(Beat { bpm: b.bpm * 2.0, time_ms: half, beat_number: 0 });
        }
    }
    numbered(out, start)
}

/// `beats` at half the tempo: every other beat, from the first that's beat 1 or 3 of its bar
/// (so downbeats stay downbeats), numbered on through the bar from there. Checked when written
/// ([`checked_grid`]).
pub fn halved_grid(beats: &[Beat]) -> Vec<Beat> {
    let Some(from) = beats.iter().position(|b| b.beat_number % 2 == 1) else { return Vec::new() };
    let start = beats[from].beat_number / 2;
    let kept = beats[from..].iter().step_by(2).map(|b| Beat { bpm: b.bpm / 2.0, ..*b }).collect();
    numbered(kept, start)
}

/// `beats` numbered 1-4 through their bars, the first `start` beats (0-3) into its bar.
fn numbered(mut beats: Vec<Beat>, start: u16) -> Vec<Beat> {
    for (i, b) in beats.iter_mut().enumerate() {
        b.beat_number = ((usize::from(start) + i) % 4) as u16 + 1;
    }
    beats
}

/// A copy of an analysis file (an `.EXT`) without its extended beat grid (`PQT2`), which a new
/// grid would contradict (see the module's docs). One without stays as it is.
pub fn without_extended_grid(bytes: &[u8]) -> Result<Vec<u8>> {
    put(bytes, b"PQT2", None)
}
