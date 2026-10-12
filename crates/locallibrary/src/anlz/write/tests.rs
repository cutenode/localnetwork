use super::*;

fn hex(s: &str) -> Vec<u8> {
    let s: String = s.split_whitespace().collect();
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect()
}

/// PMAI + PPTH + the given sections.
fn file(sections: &[Vec<u8>]) -> Vec<u8> {
    let mut f = crate::anlz::tests::synthetic("/Contents/a.mp3");
    for s in sections {
        f.extend_from_slice(s);
    }
    let len = f.len() as u32;
    put_u32(&mut f, 8, len);
    f
}

fn hot(slot: u8, ms: u32) -> CuePoint {
    CuePoint { hot_cue: Some(slot), start_ms: ms, color_rgb: DEFAULT_HOT_CUE_RGB, ..Default::default() }
}

fn memory(ms: u32) -> CuePoint {
    CuePoint { start_ms: ms, ..Default::default() }
}

fn cue_sections(bytes: &[u8]) -> Vec<Vec<u8>> {
    sections(bytes)
        .unwrap()
        .iter()
        .filter(|s| matches!(&s.tag, b"PCOB" | b"PCO2"))
        .map(|s| bytes[s.start..s.start + s.total_len].to_vec())
        .collect()
}

/// A section: tag, `header` (header length, total length and `fields`), then `body`.
fn section(tag: &[u8; 4], fields: &[u32], header: usize, body: &[u8]) -> Vec<u8> {
    let mut s = tag.to_vec();
    s.extend((header as u32).to_be_bytes());
    s.extend(((header + body.len()) as u32).to_be_bytes());
    for f in fields {
        s.extend(f.to_be_bytes());
    }
    s.resize(header, 0);
    s.extend(body);
    s
}

/// Shifting a file (PPTH and beats 1 and 2 at 100 and 569 ms) moves its beat grid, `PQT2`'s
/// first and last beat, every cue's position and loop end in each list, and its detail
/// waveform by whole columns; times before the start go to the start.
#[test]
fn shifting_moves_grid_cues_and_waveforms() {
    let mut lp = hot(1, 2000);
    lp.end_ms = Some(4000);
    let cues = vec![memory(10), hot(0, 1000), lp];
    let mut pqt2 = section(b"PQT2", &[0, 0x0100_0002, 0], 0x38, &[0, 7, 0, 9]);
    pqt2[0x18..0x28].copy_from_slice(&[0, 1, 0x2e, 0xe0, 0, 0, 0, 100, 0, 2, 0x2e, 0xe0, 0, 0, 2, 0x39]);
    // One byte a column, 150 columns a second: column 3 is loud.
    let pwv3 = section(b"PWV3", &[1, 6, 0x0096_0000], 0x18, &[0, 0, 0, 31, 0, 0]);
    let others = [pqt2, pwv3];
    for kind in [CueFile::DatAlone, CueFile::Ext] {
        let f = write_cue_points(&file(&others), kind, &cues).unwrap();
        let later = shift_times(&f, 14).unwrap();
        let read = |b: &[u8]| -> Vec<(u32, Option<u32>)> {
            read_cue_points(b, None).unwrap().iter().map(|c| (c.start_ms, c.end_ms)).collect()
        };
        assert_eq!(read(&later), [(24, None), (1014, None), (2014, Some(4014))], "{kind:?}");
        let times = |b: &[u8]| beat_grid(b).unwrap().iter().map(|b| b.time_ms).collect::<Vec<_>>();
        assert_eq!(times(&later), [114, 583]);
        let tagged = |b: &[u8], tag: &[u8; 4]| {
            let s = *sections(b).unwrap().iter().find(|s| &s.tag == tag).unwrap();
            b[s.start..s.start + s.total_len].to_vec()
        };
        let p = tagged(&later, b"PQT2");
        assert_eq!((be_u32(&p, 0x1c), be_u32(&p, 0x24)), (Some(114), Some(583)));
        // 14 ms is 2.1 columns: two silent columns in front.
        assert_eq!(&tagged(&later, b"PWV3")[0x18..], &[0, 0, 0, 0, 0, 31]);
        let earlier = shift_times(&f, -150).unwrap();
        assert_eq!(read(&earlier), [(0, None), (850, None), (1850, Some(3850))], "{kind:?}");
        assert_eq!(times(&earlier), [0, 419], "a beat isn't dropped");
        assert_eq!(&tagged(&earlier, b"PWV3")[0x18..], &[0; 6]);
        assert_eq!(be_u32(&earlier, 8).unwrap() as usize, earlier.len());
        assert_eq!(read_path(&earlier).unwrap().as_deref(), Some("/Contents/a.mp3"));
        assert_eq!(shift_times(&f, 0).unwrap(), f);
    }
    // A beat count past the section's end (a damaged file) stops at its end.
    let mut lying = file(&[]);
    let at = sections(&lying).unwrap().iter().find(|s| &s.tag == b"PQTZ").unwrap().start;
    put_u32(&mut lying, at + 20, u32::MAX);
    let started = std::time::Instant::now();
    let shifted = shift_times(&lying, 5).unwrap();
    assert!(started.elapsed() < std::time::Duration::from_secs(1), "{:?}", started.elapsed());
    assert_eq!(be_u32(&shifted, at + 24 + 4), Some(105));
}

/// Byte-level fixtures from a real rekordbox 7 export, as published in baken's tests (MIT):
/// track "Unreal", hot cues B at 15.280 s and C at 30.280 s with the default colour.
#[test]
fn matches_rekordbox_7_layout_for_hot_cues() {
    let cues = [hot(1, 15_280), hot(2, 30_280)];
    let dat = write_cue_points(&file(&[]), CueFile::DatAlone, &cues).unwrap();
    let s = &cue_sections(&dat)[0];
    assert_eq!(s[..24], hex("50434f4200000018000000880000000100000002ffffffff"));
    // rekordbox listed C before B; ours go by slot.
    assert_eq!(
        s[24 + 56..],
        hex("504350540000001c00000038000000030000000000010000ffffffff010003e800007648ffffffff
                 00000000000000000000000000000000")
    );
    let ext = write_cue_points(&file(&[]), CueFile::Ext, &cues).unwrap();
    let s = &cue_sections(&ext)[2];
    assert_eq!(s[..20], hex("50434f3200000014000000c40000000100020000"));
    assert_eq!(
        s[20 + 88..],
        hex("50435032000000100000005800000003010003e800007648ffffffff0001000000000000000000000000
                 0000001aff00 00000000000000000000000000000000000000000000000000000000000000000000
                 000000000000")
    );
}

/// The same export: hot cue A at 0.048 s, a memory cue "1.1Bars" at 1.547 s and hot cue G at
/// 185.805 s. Hot cues past C go into the .EXT's PCOB.
#[test]
fn splits_hot_cues_between_dat_and_ext_like_rekordbox() {
    let named = CuePoint { comment: "1.1Bars".into(), ..memory(1_547) };
    let cues = [hot(0, 48), named, hot(6, 185_805)];
    let dat = cue_sections(&write_cue_points(&file(&[]), CueFile::Dat, &cues).unwrap());
    assert_eq!(
        dat[0],
        hex("50434f4200000018000000500000000100000001ffffffff504350540000001c00000038000000010000000000010000
                 ffffffff010003e800000030ffffffff00000000000000000000000000000000")
    );
    assert_eq!(
        dat[1],
        hex("50434f420000001800000050000000000000000100000000504350540000001c00000038000000000000000000010000
                 ffffffff010003e80000060bffffffff00000000000000000000000000000000")
    );
    let ext = cue_sections(&write_cue_points(&file(&[]), CueFile::Ext, &cues).unwrap());
    assert_eq!(
        ext[0],
        hex("50434f4200000018000000500000000100000001ffffffff504350540000001c00000038000000070000000000010000
                 ffffffff010003e80002d5cdffffffff00000000000000000000000000000000")
    );
    assert_eq!(ext[1], hex("50434f4200000018000000180000000000000000ffffffff"));
    assert_eq!(ext[2][16..18], [0, 2]);
    // The named memory cue: 104 bytes with its comment.
    assert_eq!(ext[3].len(), 20 + 104);
    assert_eq!(ext[3][20 + 40..20 + 64], hex("000000100031002e00310042006100720073000000000000"));
}

#[test]
fn active_loop_and_memory_chain() {
    // From the same export (track "inner universe"): a memory cue at 0.253 s and an active
    // loop from 121.155 s to 126.904 s; rekordbox wrote the loop first.
    let lp = CuePoint { end_ms: Some(126_904), active_loop: true, ..memory(121_155) };
    let dat = write_cue_points(&file(&[]), CueFile::Dat, &[lp.clone(), memory(253)]).unwrap();
    let s = &cue_sections(&dat)[1];
    assert_eq!(s[..24], hex("50434f420000001800000088000000000000000200000001"));
    assert_eq!(
        s[24..80],
        hex("504350540000001c00000038000000000000000000010000ffff0001010003e8000000fdffffffff
                 00000000000000000000000000000000")
    );
    assert_eq!(
        s[80..],
        hex("504350540000001c000000380000000000000004000100000000ffff020003e80001d9430001efb8
                 00000000000000000000000000000000")
    );
    let back = read_cue_points(&dat, None).unwrap();
    assert_eq!(back.len(), 2);
    assert!(back[1].same_as(&lp));
}

#[test]
fn round_trips_and_keeps_unmodelled_bytes() {
    let cues = vec![
        CuePoint { comment: "Intro".into(), color_id: 3, ..memory(500) },
        CuePoint { end_ms: Some(16_000), loop_beats: Some((8, 1)), ..memory(8_000) },
        CuePoint { comment: "Drop ♫".into(), color_code: 0x2a, color_rgb: [0xe6, 0x28, 0x28], ..hot(0, 30_000) },
        CuePoint { end_ms: Some(64_000), loop_beats: Some((1, 2)), ..hot(4, 60_000) },
    ];
    let base = file(&[]);
    let dat = write_cue_points(&base, CueFile::Dat, &cues).unwrap();
    let mut ext = write_cue_points(&base, CueFile::Ext, &cues).unwrap();
    // Unknown bytes in an entry (here the last padding byte of the first PCP2) survive.
    let secs = sections(&ext).unwrap();
    let pco2 = secs.iter().find(|s| &s.tag == b"PCO2").unwrap();
    let entry_end = pco2.start + pco2.header_len + be_u32(&ext, pco2.start + pco2.header_len + 8).unwrap() as usize;
    ext[entry_end - 1] = 0x5a;

    let read = read_cue_points(&dat, Some(&ext)).unwrap();
    assert_eq!(read.len(), 4);
    for (a, b) in read.iter().zip(&cues) {
        assert!(a.same_as(b), "{a:?} vs {b:?}");
    }
    assert_eq!(write_cue_points(&dat, CueFile::Dat, &read).unwrap(), dat);
    assert_eq!(write_cue_points(&ext, CueFile::Ext, &read).unwrap(), ext);

    // Renaming one cue rewrites its lists only, and the unknown byte stays.
    let mut renamed = read.clone();
    renamed[2].comment = "Breakdown".into();
    assert_eq!(write_cue_points(&dat, CueFile::Dat, &renamed).unwrap(), dat);
    let ext2 = write_cue_points(&ext, CueFile::Ext, &renamed).unwrap();
    let again = read_cue_points(&dat, Some(&ext2)).unwrap();
    assert_eq!(again[2].comment, "Breakdown");
    assert!(again[3].same_as(&renamed[3]));
    assert_eq!(cue_sections(&ext2)[3], cue_sections(&ext)[3]);
    let hot_pco2 = &cue_sections(&ext2)[2];
    let first_len = be_u32(hot_pco2, 20 + 8).unwrap() as usize;
    assert_eq!(hot_pco2[20 + first_len - 1], 0x5a);
}

#[test]
fn keeps_old_layouts_it_reads() {
    // An entry that ends right after an empty comment stored as just a NUL (len 2).
    let mut e = hex(
        "5043503200000010 00000000 00000002 01 0003e8 00001000 ffffffff 00 000000 00000000 0000 0000 00000002 0000",
    );
    let len = e.len() as u32;
    put_u32(&mut e, 8, len);
    let mut list = hex("50434f3200000014 00000000 00000001 0001 0000");
    list.extend_from_slice(&e);
    let n = list.len() as u32;
    put_u32(&mut list, 8, n);
    let ext = file(&[list.clone()]);
    let mut read = read_cue_points(&ext, None).unwrap();
    assert_eq!(read.len(), 1);
    assert_eq!((read[0].hot_cue, read[0].start_ms, read[0].comment.as_str()), (Some(1), 4096, ""));
    // The PCO2 list is kept byte for byte; the missing lists are added around it.
    let out = write_cue_points(&ext, CueFile::Ext, &read).unwrap();
    let tags: Vec<[u8; 4]> = cue_sections(&out).iter().map(|s| s[..4].try_into().unwrap()).collect();
    assert_eq!(tags, [*b"PCOB", *b"PCOB", *b"PCO2", *b"PCO2"]);
    assert_eq!(cue_sections(&out)[2], list);
    // A colour makes the entry longer, but the old comment encoding stays.
    read[0].color_code = 1;
    let out = write_cue_points(&ext, CueFile::Ext, &read).unwrap();
    assert_eq!(cue_sections(&out)[2].len(), list.len() + 4);
    assert_eq!(cue_sections(&out)[2][20 + 0x28..20 + 0x2e], hex("00000002 0000"));
    assert_eq!(read_cue_points(&out, None).unwrap()[0].color_code, 1);
}

#[test]
fn leaves_lists_of_unknown_types_alone() {
    let mut odd = hex("50434f4200000018 00000000 00000007 0000 0001 ffffffff");
    odd.extend_from_slice(&pcpt_entry(&hot(0, 500), 0xffff, 0xffff));
    let n = odd.len() as u32;
    put_u32(&mut odd, 8, n);
    let f = file(&[odd.clone()]);
    assert!(read_cue_points(&f, None).unwrap().is_empty());
    let out = write_cue_points(&f, CueFile::DatAlone, &[memory(1_000)]).unwrap();
    assert_eq!(cue_sections(&out)[0], odd);
    assert_eq!(read_cue_points(&out, None).unwrap().len(), 1);
}

/// More cues than a list can count, or a name longer than an entry can hold, are refused
/// before anything is written.
#[test]
fn cues_that_cant_be_stored_are_refused() {
    let f = file(&[]);
    let many = vec![memory(1); usize::from(u16::MAX) + 1];
    let err = write_cue_points(&f, CueFile::Ext, &many).unwrap_err();
    assert_eq!(err.to_string(), "too many cues");
    let named = CuePoint { comment: "x".repeat(0x10001), ..hot(0, 1) };
    let err = write_cue_points(&f, CueFile::Ext, &[named]).unwrap_err();
    assert_eq!(err.to_string(), format!("cue name is too long: {:?}...", "x".repeat(20)));
}

#[test]
fn adds_lists_to_files_without_them() {
    let base = crate::anlz::tests::synthetic("/Contents/a.mp3");
    let out = write_cue_points(&base, CueFile::Ext, &[hot(3, 1_000)]).unwrap();
    let tags: Vec<[u8; 4]> = sections(&out).unwrap().iter().map(|s| s.tag).collect();
    assert_eq!(tags, [*b"PPTH", *b"PQTZ", *b"PCOB", *b"PCOB", *b"PCO2", *b"PCO2"]);
    assert_eq!(crate::anlz::beat_grid(&out).unwrap(), crate::anlz::beat_grid(&base).unwrap());
    assert_eq!(crate::anlz::cues(&out).unwrap()[0].hot_cue, Some(3));
    // Clearing them leaves empty lists behind, as rekordbox writes for a track without cues.
    let cleared = write_cue_points(&out, CueFile::Ext, &[]).unwrap();
    assert!(read_cue_points(&cleared, None).unwrap().is_empty());
    assert_eq!(cleared.len(), base.len() + 24 * 2 + 20 * 2);
}

/// Only hot cue and memory cue lists hold cues, and a file's lists hold each cue once: a `.DAT`
/// alone holds every cue in its `PCOB` lists, a `.DAT` and `.EXT` pair share them out between
/// their `PCOB` lists, and the `.EXT`'s `PCO2` lists hold every cue. A list of any other type
/// holds none, whichever file and tag it's in.
#[hegel::test]
fn each_cue_goes_in_one_list_of_its_type(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let n = tc.draw(gs::integers::<usize>().max_value(12));
    let cues: Vec<CuePoint> = (0..n)
        .map(|i| CuePoint {
            hot_cue: tc.draw(gs::optional(gs::integers::<u8>().max_value(15))),
            start_ms: i as u32,
            ..Default::default()
        })
        .collect();
    let (hot, memory): (Vec<&CuePoint>, Vec<&CuePoint>) = cues.iter().partition(|c| c.hot_cue.is_some());
    let held = |lists: &[(CueFile, [u8; 4])]| -> Vec<u32> {
        let mut ms: Vec<u32> = lists
            .iter()
            .flat_map(|(file, tag)| [HOT, MEMORY].map(|list| file.wanted(tag, list, &hot, &memory)))
            .flatten()
            .map(|c| c.start_ms)
            .collect();
        ms.sort_unstable();
        ms
    };
    let every: Vec<u32> = (0..n as u32).collect();
    assert_eq!(held(&[(CueFile::DatAlone, PCOB)]), every);
    assert_eq!(held(&[(CueFile::Dat, PCOB), (CueFile::Ext, PCOB)]), every);
    assert_eq!(held(&[(CueFile::Ext, PCO2)]), every);

    let file = [CueFile::Dat, CueFile::DatAlone, CueFile::Ext][tc.draw(gs::integers::<usize>().max_value(2))];
    let tag = [PCOB, PCO2][tc.draw(gs::integers::<usize>().max_value(1))];
    let other = tc.draw(gs::integers::<u32>().min_value(2));
    assert!(file.wanted(&tag, other, &hot, &memory).is_empty());
}

/// A cue list is read entry by entry up to the first whose length can't be an entry's (under
/// its own 12-byte header) or runs past the list, or until too little is left for a header:
/// every entry before that comes back whole, and nothing from there on.
#[hegel::test(test_cases = 200)]
fn cue_list_entries_end_at_the_first_that_cant_be_one(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let header_len = tc.draw(gs::sampled_from(vec![16usize, 24]));
    let lens: Vec<usize> = tc.draw(gs::vecs(gs::integers::<usize>().min_value(12).max_value(40)).max_size(4));
    let mut bytes = vec![0u8; header_len];
    let mut want = Vec::new();
    for (i, len) in lens.iter().enumerate() {
        let start = bytes.len();
        bytes.extend(std::iter::repeat_n(i as u8 + 1, *len));
        bytes[start + 8..start + 12].copy_from_slice(&(*len as u32).to_be_bytes());
        want.push(start..start + len);
    }
    // Then: nothing, a few bytes too short for a header, or an entry claiming a length it can't have.
    match tc.draw(gs::integers::<u8>().max_value(2)) {
        0 => {}
        1 => bytes.extend(vec![0xEE; tc.draw(gs::integers::<usize>().min_value(1).max_value(11))]),
        _ => {
            let tail = tc.draw(gs::integers::<usize>().min_value(12).max_value(30));
            let bad = tc.draw(hegel::one_of!(
                gs::integers::<u32>().max_value(11),
                gs::integers::<u32>().min_value(tail as u32 + 1).max_value(1 << 20),
            ));
            let start = bytes.len();
            bytes.extend(vec![0xEE; tail]);
            bytes[start + 8..start + 12].copy_from_slice(&bad.to_be_bytes());
        }
    }
    let s = SectionRef { tag: *b"PCOB", start: 0, header_len, total_len: bytes.len() };
    let got = list_entries(&bytes, &s);
    assert_eq!(got, want.into_iter().map(|r| &bytes[r]).collect::<Vec<_>>());
}
