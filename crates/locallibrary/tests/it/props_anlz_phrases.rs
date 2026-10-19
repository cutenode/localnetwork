//! What an analysis file says of a track's phrases and its scrolling waveform reads back as
//! written: the mood, each phrase's kind as rekordbox names it for that mood, its first beat and
//! where it ends (where the next starts, the last at the track's end), and its fill; masked as
//! rekordbox 6 writes them or not, alike. A detail waveform's columns keep their heights at the
//! rate the file gives (150 a second when it gives none).

use hegel::TestCase;
use hegel::generators as gs;
use locallibrary::anlz::{self, Mood, PhraseKind};

/// The XOR mask rekordbox 6 puts over a `PSSI` body (per crate-digger), offset by the count.
const MASK: [u8; 19] =
    [0xCB, 0xE1, 0xEE, 0xFA, 0xE5, 0xEE, 0xAD, 0xEE, 0xE9, 0xD2, 0xE9, 0xEB, 0xE1, 0xE9, 0xF3, 0xE8, 0xE9, 0xF4, 0xE1];

/// An analysis file of `sections`.
fn file(sections: &[Vec<u8>]) -> Vec<u8> {
    let len: usize = 28 + sections.iter().map(Vec::len).sum::<usize>();
    let mut out = b"PMAI".to_vec();
    out.extend(28u32.to_be_bytes());
    out.extend((len as u32).to_be_bytes());
    out.extend([0; 16]);
    for s in sections {
        out.extend(s);
    }
    out
}

/// A `PSSI` section: `mood` (1 high, 2 mid, 3 low), phrases as (first beat, kind, fill beat),
/// the track ending at beat `end`.
fn pssi(mood: u16, phrases: &[(u16, u16, Option<u16>)], end: u16, masked: bool) -> Vec<u8> {
    let count = phrases.len() as u16;
    let mut body = vec![0u8; 14];
    body[0..2].copy_from_slice(&mood.to_be_bytes());
    body[8..10].copy_from_slice(&end.to_be_bytes());
    for (i, (beat, kind, fill)) in phrases.iter().enumerate() {
        let mut e = vec![0u8; 24];
        e[0..2].copy_from_slice(&(i as u16 + 1).to_be_bytes());
        e[2..4].copy_from_slice(&beat.to_be_bytes());
        e[4..6].copy_from_slice(&kind.to_be_bytes());
        if let Some(f) = fill {
            e[21] = 1;
            e[22..24].copy_from_slice(&f.to_be_bytes());
        }
        body.extend(e);
    }
    if masked {
        for (i, b) in body.iter_mut().enumerate() {
            *b ^= MASK[i % MASK.len()].wrapping_add(count as u8);
        }
    }
    let mut s = b"PSSI".to_vec();
    s.extend(32u32.to_be_bytes());
    s.extend((18 + body.len() as u32).to_be_bytes());
    s.extend(24u32.to_be_bytes());
    s.extend(count.to_be_bytes());
    s.extend(body);
    s
}

/// What rekordbox calls phrase `kind` in a track of `mood`.
fn named(mood: Mood, kind: u16) -> PhraseKind {
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

#[hegel::test(test_cases = 300)]
fn phrases_and_the_detail_waveform_read_back_as_written(tc: TestCase) {
    let mood_code = tc.draw(gs::integers::<u16>().min_value(1).max_value(3));
    let mood = [Mood::High, Mood::Mid, Mood::Low][usize::from(mood_code - 1)];
    // Phrases in order, each at least a beat long.
    let mut beat = 1u16;
    let mut phrases = Vec::new();
    for _ in 0..tc.draw(gs::integers::<usize>().max_value(6)) {
        let kind = tc.draw(gs::integers::<u16>().min_value(1).max_value(12));
        let fill = tc.draw(gs::optional(gs::integers::<u16>().min_value(1).max_value(500)));
        phrases.push((beat, kind, fill));
        beat += tc.draw(gs::integers::<u16>().min_value(1).max_value(64));
    }
    let end = beat;
    let plain = anlz::song_structure(&file(&[pssi(mood_code, &phrases, end, false)])).unwrap().unwrap();
    let masked = anlz::song_structure(&file(&[pssi(mood_code, &phrases, end, true)])).unwrap().unwrap();
    assert_eq!(plain, masked, "masked or not, alike");
    assert_eq!(plain.mood, mood);
    let ends: Vec<u16> = phrases.iter().skip(1).map(|p| p.0).chain([end]).collect();
    let want: Vec<_> =
        phrases.iter().zip(ends).map(|((start, kind, fill), end)| (named(mood, *kind), *start, end, *fill)).collect();
    let got: Vec<_> = plain.phrases.iter().map(|p| (p.kind, p.start_beat, p.end_beat, p.fill_beat)).collect();
    assert_eq!(got, want);

    // A mood that isn't one is no structure; a section cut short is refused.
    let other = tc.draw(gs::sampled_from(vec![0u16, 4, 20]));
    assert_eq!(anlz::song_structure(&file(&[pssi(other, &phrases, end, false)])).unwrap(), None);
    let mut short = b"PSSI".to_vec();
    short.extend(12u32.to_be_bytes());
    short.extend(12u32.to_be_bytes());
    assert!(anlz::song_structure(&file(&[short])).is_err());

    // A blue detail waveform: heights in the low five bits, at the rate the header gives.
    let columns: Vec<u8> = tc.draw(gs::vecs(gs::integers::<u8>()).max_size(40));
    let rate = tc.draw(gs::sampled_from(vec![0u16, 150, 75]));
    let mut pwv3 = b"PWV3".to_vec();
    pwv3.extend(24u32.to_be_bytes());
    pwv3.extend((24 + columns.len() as u32).to_be_bytes());
    pwv3.extend(1u32.to_be_bytes());
    pwv3.extend((columns.len() as u32).to_be_bytes());
    pwv3.extend(rate.to_be_bytes());
    pwv3.extend([0, 0]);
    pwv3.extend(&columns);
    let w = anlz::detail(&file(&[pwv3])).unwrap().unwrap();
    assert_eq!(w.columns_per_sec, Some(if rate == 0 { 150.0 } else { f64::from(rate) }));
    let heights: Vec<f32> = w.columns.iter().map(|c| c.back.height).collect();
    assert_eq!(heights, columns.iter().map(|b| f32::from(b & 0x1f) / 31.0).collect::<Vec<_>>());
}

fn be16(b: &[u8], o: usize) -> Option<u16> {
    b.get(o..o + 2).map(|s| u16::from_be_bytes([s[0], s[1]]))
}

fn be32(b: &[u8], o: usize) -> Option<u32> {
    b.get(o..o + 4).map(|s| u32::from_be_bytes([s[0], s[1], s[2], s[3]]))
}

/// The path, beat grid and phrase readers take each field from where the section's layout puts
/// it, and refuse a section whose fields lie past the end of the file (or, for the phrases' body,
/// past the section's own end) as truncated: a short section is never read out of bytes that
/// aren't there, whatever follows it.
#[hegel::test(test_cases = 400)]
fn sections_cut_short_are_refused_as_truncated(tc: TestCase) {
    let tag = tc.draw(gs::sampled_from(vec![*b"PPTH", *b"PQTZ", *b"PSSI"]));
    let total = tc.draw(gs::integers::<usize>().min_value(12).max_value(48));
    let header = tc.draw(gs::integers::<usize>().min_value(12).max_value(total));
    let mut section = tag.to_vec();
    section.extend((header as u32).to_be_bytes());
    section.extend((total as u32).to_be_bytes());
    // Small numbers, so lengths and counts sometimes fit and sometimes don't.
    section.extend(tc.draw(gs::vecs(gs::integers::<u8>().max_value(40)).min_size(total - 12).max_size(total - 12)));
    // A phrase body's mood stays unmasked (rekordbox masks bodies whose mood reads above 20).
    if tag == *b"PSSI" && total >= 20 {
        section[18..20].copy_from_slice(&tc.draw(gs::integers::<u16>().max_value(4)).to_be_bytes());
    }
    let mut sections = vec![section];
    if tc.draw(gs::booleans()) {
        let len = tc.draw(gs::integers::<usize>().min_value(12).max_value(24));
        let mut other = b"PVBR".to_vec();
        other.extend(12u32.to_be_bytes());
        other.extend((len as u32).to_be_bytes());
        other.extend(vec![7u8; len - 12]);
        sections.push(other);
    }
    let bytes = file(&sections);
    // The section and everything after it, as the readers see it.
    let f = &bytes[28..];
    let truncated = |e: locallibrary::Error| {
        let name = String::from_utf8_lossy(&tag).into_owned();
        assert!(e.to_string().contains(&format!("truncated {name}")), "{e}");
    };
    match &tag {
        b"PPTH" => {
            let want = be32(f, 12).and_then(|len| f.get(header..header + len as usize)).map(|path| {
                let units: Vec<u16> =
                    path.chunks_exact(2).map(|c| u16::from_be_bytes([c[0], c[1]])).take_while(|&u| u != 0).collect();
                String::from_utf16_lossy(&units)
            });
            match (anlz::read_path(&bytes), want) {
                (Ok(got), Some(want)) => assert_eq!(got, Some(want)),
                (Err(e), None) => truncated(e),
                (got, want) => panic!("read {got:?}, expected {want:?}"),
            }
        }
        b"PQTZ" => match (anlz::beat_grid(&bytes), be32(f, 20)) {
            (Ok(beats), Some(count)) => {
                // Only the whole beats inside the section, however many the count says.
                let fit = (total - header) / 8;
                assert_eq!(beats.len(), (count as usize).min(fit));
                let numbers: Vec<u16> = (0..beats.len()).map(|i| be16(f, header + i * 8).unwrap()).collect();
                assert_eq!(beats.iter().map(|b| b.beat_number).collect::<Vec<_>>(), numbers);
            }
            (Err(e), None) => truncated(e),
            (got, count) => panic!("read {got:?}, count {count:?}"),
        },
        _ => {
            let fields = be32(f, 12).zip(be16(f, 16)).and_then(|_| f.get(18..total));
            let mood = fields.and_then(|body| be16(body, 0)).filter(|m| (1..=3).contains(m));
            let refused = fields.is_none() || (mood.is_some() && fields.is_some_and(|body| be16(body, 8).is_none()));
            match anlz::song_structure(&bytes) {
                Err(e) => {
                    assert!(refused, "refused a whole section: {e}");
                    truncated(e);
                }
                Ok(s) => {
                    assert!(!refused, "read a section cut short: {s:?}");
                    assert_eq!(s.is_some(), mood.is_some(), "{s:?}");
                }
            }
        }
    }
}
