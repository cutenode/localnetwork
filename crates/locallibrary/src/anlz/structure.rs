//! Writing phrases (`PSSI`, the song structure, in `.EXT`).
//!
//! `PSSI` ([DJL's "Song Structure Tag"](https://djl-analysis.deepsymmetry.org/rekordbox-export-analysis/anlz.html#song-structure-tag)):
//! header 0x20: the entry length u32 (24), the phrase count u16, the mood u16 (1 high, 2 mid, 3
//! low), 6 bytes, the beat the last phrase ends on u16, 2 bytes, the lighting bank u8 and a byte.
//! Then 24 bytes a phrase: its number u16 (from 1), the beat it starts on u16, its kind u16 (named
//! by the mood), 15 bytes of variation flags and beats, a fill-in flag u8 at 21 and the beat the
//! fill-in starts on u16 at 22. rekordbox 6 and later mask everything after the count with an XOR
//! mask ([`PSSI_MASK`] plus the count); the reader takes a mood past 20 for masked.
//!
//! Rewriting keeps a file's header bytes and every byte of a phrase entry whose start, kind and
//! fill-in stay the same; a new entry has its flags at 0 (the kind's first variation) and new
//! headers bank 0. It's masked as the file's was (new ones as rekordbox 6 writes them), unless
//! masking would make it read as unmasked (a mask byte of 0 over a mood), when it isn't.

use super::tags::{only, put, section};
use super::write::{put_u16, put_u32};
use super::*;

const ENTRY: usize = 24;

/// rekordbox's kind number for a phrase of `kind` in a track of `mood`: the first of those named
/// so (low verses have three numbers each). `None` for one the mood has no name for.
pub fn kind_number(mood: Mood, kind: PhraseKind) -> Option<u16> {
    let n = match (mood, kind) {
        (_, PhraseKind::Intro) => 1,
        (Mood::High, PhraseKind::Up) => 2,
        (Mood::High, PhraseKind::Down) => 3,
        (Mood::High, PhraseKind::Chorus) => 5,
        (Mood::High, PhraseKind::Outro) => 6,
        (Mood::Mid, PhraseKind::Verse(n @ 1..=6)) => u16::from(n) + 1,
        (Mood::Low, PhraseKind::Verse(1)) => 2,
        (Mood::Low, PhraseKind::Verse(2)) => 5,
        (Mood::Mid | Mood::Low, PhraseKind::Bridge) => 8,
        (Mood::Mid | Mood::Low, PhraseKind::Chorus) => 9,
        (Mood::Mid | Mood::Low, PhraseKind::Outro) => 10,
        (_, PhraseKind::Unknown(k)) if phrase_kind(mood, k) == PhraseKind::Unknown(k) => k,
        _ => return None,
    };
    Some(n)
}

fn mood_number(mood: Mood) -> u16 {
    match mood {
        Mood::High => 1,
        Mood::Mid => 2,
        Mood::Low => 3,
    }
}

/// Phrases a `PSSI` can hold: at least one, each starting after the one before on beat 1 or
/// later, ending where the next starts (the last after it starts), of a kind the mood names, with
/// a fill-in (if any) on beat 1 or later.
fn check(s: &SongStructure) -> Result<()> {
    if s.phrases.is_empty() {
        return Err(Error::invalid("phrases need a phrase"));
    }
    if s.phrases.len() > usize::from(u16::MAX) {
        return Err(Error::invalid("too many phrases"));
    }
    for (i, p) in s.phrases.iter().enumerate() {
        let n = i + 1;
        let next = s.phrases.get(i + 1).map_or(p.end_beat, |q| q.start_beat);
        if p.start_beat == 0 || p.end_beat <= p.start_beat || p.end_beat != next {
            return Err(Error::invalid(format!(
                "phrase {n} runs from beat {} to {}: phrases follow one another from beat 1",
                p.start_beat, p.end_beat
            )));
        }
        if kind_number(s.mood, p.kind).is_none() {
            return Err(Error::invalid(format!("phrase {n}: a {:?} track has no {} phrases", s.mood, p.kind.name())));
        }
        if p.fill_beat == Some(0) {
            return Err(Error::invalid(format!("phrase {n}'s fill-in starts on beat 0: beats count from 1")));
        }
    }
    Ok(())
}

/// The bytes after the count XORed with the mask (it undoes itself).
fn masked(body: &mut [u8], count: u16) {
    for (i, b) in body.iter_mut().enumerate() {
        *b ^= PSSI_MASK[i % PSSI_MASK.len()].wrapping_add(count as u8);
    }
}

/// A copy of an analysis file (an `.EXT`) holding `phrases` as its song structure, added when it
/// has none, or without one (`None`). Phrases as they are keep the file as it was, byte for byte.
pub fn write_song_structure(bytes: &[u8], phrases: Option<&SongStructure>) -> Result<Vec<u8>> {
    let Some(s) = phrases else { return put(bytes, b"PSSI", None) };
    check(s)?;
    let secs = sections(bytes)?;
    let existing = only(&secs, b"PSSI")?.filter(|e| e.header_len == 0x20);
    // The existing section unmasked, from the entry length on, with whether it was masked.
    let old = existing.map(|e| {
        let mut b = bytes[e.start + 12..e.start + e.total_len].to_vec();
        let count = be_u16(&b, 4).unwrap_or(0);
        let was = be_u16(&b, 6).is_some_and(|m| m > 20);
        if was {
            masked(&mut b[6..], count);
        }
        (b, was)
    });
    let old_mood = old.as_ref().and_then(|(b, _)| be_u16(b, 6));
    let old_entries: Vec<&[u8]> = match &old {
        Some((b, _)) if old_mood == Some(mood_number(s.mood)) => b[20..].chunks_exact(ENTRY).collect(),
        _ => Vec::new(),
    };
    let count = s.phrases.len() as u16;
    let mut head = old.as_ref().map_or_else(|| vec![0; 20], |(b, _)| b[..20].to_vec());
    put_u32(&mut head, 0, ENTRY as u32);
    put_u16(&mut head, 4, count);
    put_u16(&mut head, 6, mood_number(s.mood));
    put_u16(&mut head, 14, s.phrases.last().expect("checked").end_beat);
    let mut plain = head;
    for (i, p) in s.phrases.iter().enumerate() {
        let kept = old_entries.get(i).filter(|e| {
            let fill = (e[21] != 0).then(|| be_u16(e, 22).unwrap_or(0)).filter(|b| *b > 0);
            be_u16(e, 2) == Some(p.start_beat)
                && be_u16(e, 4).map(|k| phrase_kind(s.mood, k)) == Some(p.kind)
                && fill == p.fill_beat
        });
        let mut e = kept.map_or_else(|| vec![0; ENTRY], |e| e.to_vec());
        put_u16(&mut e, 0, i as u16 + 1);
        if kept.is_none() {
            put_u16(&mut e, 2, p.start_beat);
            put_u16(&mut e, 4, kind_number(s.mood, p.kind).expect("checked"));
            e[21] = u8::from(p.fill_beat.is_some());
            put_u16(&mut e, 22, p.fill_beat.unwrap_or(0));
        }
        plain.extend_from_slice(&e);
    }
    let want = Some(SongStructure { mood: s.mood, phrases: s.phrases.clone() });
    let mask = old.as_ref().is_none_or(|(_, was)| *was);
    for mask in [mask, false] {
        let mut body = plain.clone();
        if mask {
            masked(&mut body[6..], count);
        }
        let out = put(bytes, b"PSSI", Some(&section(b"PSSI", &body[..20], &body[20..])?))?;
        // Never phrases the reader doesn't read back as given.
        if song_structure(&out)? == want {
            return Ok(out);
        }
    }
    Err(malformed("the phrases written don't read back"))
}
