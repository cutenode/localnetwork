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
            &[pcp2(1, 1, 30_000, u32::MAX, "Drop", [255, 0, 64]), pcp2(3, 2, 60_000, 64_000, "", [0, 0, 0])].concat(),
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

/// A shift past either end of a detail waveform empties it, whatever column width and rate its
/// header claims: their product with the shift used to overflow.
#[test]
fn a_huge_shift_empties_the_detail_waveform() {
    let mut h = u32::MAX.to_be_bytes().to_vec(); // bytes per column
    h.extend_from_slice(&1u32.to_be_bytes()); // columns
    h.extend_from_slice(&u16::MAX.to_be_bytes()); // columns a second
    h.extend_from_slice(&[0, 0]);
    let f = file(&[section(b"PWV5", &h, &[1, 2, 3, 4])]);
    for ms in [i32::MAX, i32::MIN] {
        let shifted = shift_times(&f, ms).unwrap();
        assert_eq!(shifted[shifted.len() - 4..], [0; 4], "shifted by {ms} ms");
    }
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
