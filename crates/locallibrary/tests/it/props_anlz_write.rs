//! Beat grids, phrases and waveforms written into analysis files: rekordbox's own files rewrite
//! byte for byte with what they hold, and anything written reads back as given (with the reader,
//! and framed by rekordcrate's independent parser), leaving every other section as it was. New
//! files for a track without any hold its path and empty cue lists, and take the rest in
//! rekordbox's order.

use crate::common;

use std::io::Cursor;
use std::sync::OnceLock;

use binrw::BinRead;
use hegel::TestCase;
use hegel::generators as gs;
use locallibrary::anlz::{self, Bands, Beat, Mood, Phrase, PhraseKind, SongStructure, WaveformStyle};

const ANALYSED: [&str; 2] = ["P016/0000875E", "P053/0001D21F"];

/// One track's `.DAT`, `.EXT` and `.2EX`.
type Analysis = [Vec<u8>; 3];

/// The demo export's analysis files.
fn real_files() -> &'static [Analysis] {
    static FILES: OnceLock<Vec<Analysis>> = OnceLock::new();
    FILES.get_or_init(|| {
        ANALYSED
            .iter()
            .map(|dir| {
                let d = common::fixtures().join("rekordcrate/demo_tracks/PIONEER/USBANLZ").join(dir);
                ["DAT", "EXT", "2EX"].map(|ext| std::fs::read(d.join(format!("ANLZ0000.{ext}"))).unwrap())
            })
            .collect()
    })
}

/// Each section's tag and bytes, in order.
fn sections(bytes: &[u8]) -> Vec<([u8; 4], Vec<u8>)> {
    let be = |at: usize| u32::from_be_bytes(bytes[at..at + 4].try_into().unwrap()) as usize;
    let (mut pos, end) = (be(4), be(8));
    let mut out = Vec::new();
    while pos + 12 <= end {
        let total = be(pos + 8);
        out.push((bytes[pos..pos + 4].try_into().unwrap(), bytes[pos..pos + total].to_vec()));
        pos += total;
    }
    out
}

fn tags(bytes: &[u8]) -> Vec<String> {
    sections(bytes).iter().map(|(t, _)| String::from_utf8_lossy(t).into_owned()).collect()
}

/// The sections of `bytes` but those tagged `tag`.
fn others(bytes: &[u8], tag: &[u8; 4]) -> Vec<Vec<u8>> {
    sections(bytes).into_iter().filter(|(t, _)| t != tag).map(|(_, s)| s).collect()
}

/// rekordcrate frames the whole file: every section's header and the content it knows
/// (asserting each waveform's length and count, a song structure's entries, a beat grid's
/// count). Its beat grid's beats, if it has one.
fn rekordcrate_beats(bytes: &[u8]) -> Option<Vec<(u16, u16, u32)>> {
    use rekordcrate::anlz::{ANLZ, Content};
    let file = ANLZ::read_be(&mut Cursor::new(bytes)).expect("rekordcrate reads the file");
    file.sections.into_iter().find_map(|s| match s.content {
        Content::BeatGrid(g) => Some(g.beats.iter().map(|b| (b.beat_number, b.tempo, b.time)).collect()),
        _ => None,
    })
}

/// rekordbox's files hold what they hold: writing it again changes nothing, byte for byte.
#[test]
fn rekordbox_files_rewrite_byte_for_byte() {
    for [dat, ext, two] in real_files() {
        assert_eq!(anlz::write_beat_grid(dat, &anlz::beat_grid(dat).unwrap()).unwrap(), *dat);
        let phrases = anlz::song_structure(ext).unwrap();
        assert!(phrases.is_some(), "the demo tracks have phrases (masked, as rekordbox 6 writes them)");
        assert_eq!(anlz::write_song_structure(ext, phrases.as_ref()).unwrap(), *ext);
        let mut written = Vec::new();
        for file in [dat, ext, two] {
            for tag in anlz::waveform_tags() {
                let Some(columns) = anlz::waveform_entries(file, tag).unwrap() else { continue };
                assert_eq!(
                    anlz::write_waveform(file, tag, &columns).unwrap(),
                    *file,
                    "{}",
                    String::from_utf8_lossy(tag)
                );
                written.push(String::from_utf8_lossy(tag).into_owned());
            }
        }
        assert_eq!(written, ["PWAV", "PWV2", "PWV3", "PWV4", "PWV5", "PWV6", "PWV7"]);
        // The extended grid comes out alone; the cue lists and the rest stay.
        let without = anlz::without_extended_grid(ext).unwrap();
        assert_eq!(others(&without, b"PQT2"), others(ext, b"PQT2"));
        assert!(!tags(&without).contains(&"PQT2".to_string()));
        assert_eq!(anlz::without_extended_grid(&without).unwrap(), without);
    }
}

/// A base `.DAT` or `.EXT` to write into: one of rekordbox's, a minimal synthetic one, or a new
/// one.
#[hegel::test_helper]
fn base(tc: &TestCase, ext: bool) -> Vec<u8> {
    let which = tc.draw(gs::integers::<usize>().max_value(ANALYSED.len() + 1));
    match real_files().get(which) {
        Some(files) => files[usize::from(ext)].clone(),
        None if which == ANALYSED.len() => common::synthetic_anlz("/Contents/a.mp3"),
        None => {
            let (dat, e) = anlz::new_analysis_files("/Contents/new.mp3").unwrap();
            if ext { e } else { dat }
        }
    }
}

/// What a file says besides grids, phrases and waveforms, as the readers see it.
fn content(bytes: &[u8]) -> String {
    format!(
        "{:?} {:?} {:?}",
        anlz::read_path(bytes).unwrap(),
        anlz::read_cue_points(bytes, None).unwrap().len(),
        anlz::cues(bytes).unwrap()
    )
}

/// A beat grid: in order, each beat 1-4 in its bar, at a tempo of hundredths of a BPM.
#[hegel::test_helper]
fn grid(tc: &TestCase) -> Vec<Beat> {
    let n = tc.draw(gs::integers::<usize>().min_value(1).max_value(300));
    let mut time = tc.draw(gs::integers::<u32>().max_value(10_000));
    (0..n)
        .map(|i| {
            time += tc.draw(gs::integers::<u32>().min_value(1).max_value(2_000));
            let hundredths = tc.draw(gs::integers::<u16>().min_value(1));
            Beat { beat_number: (i % 4) as u16 + 1, bpm: f64::from(hundredths) / 100.0, time_ms: time }
        })
        .collect()
}

/// A grid written reads back as given, as rekordcrate reads it too, and nothing else in the file
/// changes; written again it stays as it is.
#[hegel::test(test_cases = 200)]
fn beat_grids_read_back_as_written(tc: TestCase) {
    let dat = base(&tc, false);
    let beats = grid(&tc);
    let out = anlz::write_beat_grid(&dat, &beats).unwrap();
    assert_eq!(anlz::beat_grid(&out).unwrap(), beats);
    let crate_view: Vec<(u16, u16, u32)> =
        beats.iter().map(|b| (b.beat_number, (b.bpm * 100.0).round() as u16, b.time_ms)).collect();
    assert_eq!(rekordcrate_beats(&out), Some(crate_view));
    assert_eq!(others(&out, b"PQTZ"), others(&dat, b"PQTZ"));
    assert_eq!(content(&out), content(&dat));
    assert_eq!(anlz::write_beat_grid(&out, &beats).unwrap(), out);
    assert_eq!(u32::from_be_bytes(out[8..12].try_into().unwrap()) as usize, out.len());
}

/// A grid the format can't hold is refused, saying why.
#[test]
fn grids_that_cant_be_stored_are_refused() {
    let dat = common::synthetic_anlz("/Contents/a.mp3");
    let beat = |n: u16, bpm: f64, ms: u32| Beat { beat_number: n, bpm, time_ms: ms };
    let refused = |beats: &[Beat]| anlz::write_beat_grid(&dat, beats).unwrap_err().to_string();
    assert_eq!(refused(&[]), "a beat grid needs a beat");
    assert_eq!(refused(&[beat(5, 120.0, 0)]), "beat 1 is beat 5 of its bar: 1-4");
    assert_eq!(refused(&[beat(1, 700.0, 0)]), "beat 1's tempo, 700 BPM, isn't 0.01-655.35");
    assert_eq!(refused(&[beat(1, f64::NAN, 0)]), "beat 1's tempo, NaN BPM, isn't 0.01-655.35");
    assert_eq!(refused(&[beat(1, 120.0, 5), beat(2, 120.0, 5)]), "beat 2 isn't after the one before it");
    // Tempos are kept to hundredths.
    let out = anlz::write_beat_grid(&dat, &[beat(1, 128.004, 0)]).unwrap();
    assert_eq!(anlz::beat_grid(&out).unwrap()[0].bpm, 128.0);
}

/// The kinds a mood names, each as rekordbox names it.
fn kinds(mood: Mood) -> Vec<PhraseKind> {
    use PhraseKind::*;
    let mut k = vec![Intro];
    match mood {
        Mood::High => k.extend([Up, Down, Chorus, Outro, Unknown(4), Unknown(7)]),
        Mood::Mid => k.extend((1..=6).map(Verse).chain([Bridge, Chorus, Outro, Unknown(11)])),
        Mood::Low => k.extend([Verse(1), Verse(2), Bridge, Chorus, Outro, Unknown(0)]),
    }
    k
}

/// Phrases following one another from beat 1, each of a kind the mood names, some with fill-ins.
#[hegel::test_helper]
fn structure(tc: &TestCase) -> SongStructure {
    let mood = tc.draw(gs::sampled_from(vec![Mood::High, Mood::Mid, Mood::Low]));
    let n = tc.draw(gs::integers::<usize>().min_value(1).max_value(60));
    let mut start = tc.draw(gs::integers::<u16>().min_value(1).max_value(40));
    let phrases = (0..n)
        .map(|_| {
            let len = tc.draw(gs::integers::<u16>().min_value(1).max_value(64));
            let end = start + len;
            let fill = tc.draw(gs::optional(gs::integers::<u16>().min_value(1).max_value(end)));
            let p = Phrase {
                kind: tc.draw(gs::sampled_from(kinds(mood))),
                start_beat: start,
                end_beat: end,
                fill_beat: fill,
            };
            start = end;
            p
        })
        .collect();
    SongStructure { mood, phrases }
}

/// Phrases written read back as given, masked as the file's were (new ones too, as rekordbox 6
/// writes them) unless that would read as unmasked, framed as rekordcrate expects, and nothing
/// else changes; written again they stay; taken out, the file is as it was without them.
#[hegel::test(test_cases = 200)]
fn phrases_read_back_as_written(tc: TestCase) {
    let ext = base(&tc, true);
    let s = structure(&tc);
    let out = anlz::write_song_structure(&ext, Some(&s)).unwrap();
    assert_eq!(anlz::song_structure(&out).unwrap(), Some(s.clone()));
    rekordcrate_beats(&out);
    assert_eq!(others(&out, b"PSSI"), others(&ext, b"PSSI"));
    assert_eq!(content(&out), content(&ext));
    assert_eq!(anlz::write_song_structure(&out, Some(&s)).unwrap(), out);
    let pssi = sections(&out).into_iter().find(|(t, _)| t == b"PSSI").unwrap().1;
    // Masking a mid track's mood with a count of 53 makes 20, which reads as unmasked.
    let reads_unmasked = s.mood == Mood::Mid && (s.phrases.len() as u8).wrapping_add(0xCB) == 0;
    assert_eq!(u16::from_be_bytes([pssi[18], pssi[19]]) > 20, !reads_unmasked);
    let gone = anlz::write_song_structure(&out, None).unwrap();
    assert_eq!(others(&gone, b"PSSI"), others(&ext, b"PSSI"));
    assert_eq!(anlz::song_structure(&gone).unwrap(), None);
}

/// Changing one phrase keeps every other entry's bytes, flags rekordbox set included.
#[test]
fn changing_a_phrase_keeps_the_others_entries() {
    let ext = &real_files()[1][1];
    let mut s = anlz::song_structure(ext).unwrap().unwrap();
    let unmasked = |b: &[u8]| {
        let pssi = sections(b).into_iter().find(|(t, _)| t == b"PSSI").unwrap().1;
        let count = u16::from_be_bytes([pssi[16], pssi[17]]);
        let mask = [
            0xCB, 0xE1, 0xEE, 0xFA, 0xE5, 0xEE, 0xAD, 0xEE, 0xE9, 0xD2, 0xE9, 0xEB, 0xE1, 0xE9, 0xF3, 0xE8, 0xE9, 0xF4,
            0xE1u8,
        ];
        let body: Vec<u8> =
            pssi[18..].iter().enumerate().map(|(i, b)| b ^ mask[i % 19].wrapping_add(count as u8)).collect();
        body[14..].chunks(24).map(<[u8]>::to_vec).collect::<Vec<_>>()
    };
    let before = unmasked(ext);
    s.phrases[3].kind = PhraseKind::Chorus;
    let after = unmasked(&anlz::write_song_structure(ext, Some(&s)).unwrap());
    for (i, (a, b)) in before.iter().zip(&after).enumerate() {
        assert_eq!(a == b, i != 3, "entry {i}");
    }
    assert_eq!(u16::from_be_bytes([after[3][4], after[3][5]]), 9, "a mid track's chorus");
}

/// Phrases a `PSSI` can't hold are refused, saying why.
#[test]
fn phrases_that_cant_be_stored_are_refused() {
    let ext = common::synthetic_anlz("/Contents/a.mp3");
    let phrase = |kind, start_beat, end_beat| Phrase { kind, start_beat, end_beat, fill_beat: None };
    let refused = |mood, phrases: Vec<Phrase>| {
        anlz::write_song_structure(&ext, Some(&SongStructure { mood, phrases })).unwrap_err().to_string()
    };
    assert_eq!(refused(Mood::Mid, vec![]), "phrases need a phrase");
    let gap = vec![phrase(PhraseKind::Intro, 1, 17), phrase(PhraseKind::Outro, 20, 30)];
    assert_eq!(refused(Mood::Mid, gap), "phrase 1 runs from beat 1 to 17: phrases follow one another from beat 1");
    assert_eq!(
        refused(Mood::Low, vec![phrase(PhraseKind::Intro, 0, 4)]),
        "phrase 1 runs from beat 0 to 4: phrases follow one another from beat 1"
    );
    assert_eq!(
        refused(Mood::High, vec![phrase(PhraseKind::Verse(1), 1, 4)]),
        "phrase 1: a High track has no Verse 1 phrases"
    );
    assert_eq!(refused(Mood::Mid, vec![phrase(PhraseKind::Up, 1, 4)]), "phrase 1: a Mid track has no Up phrases");
    assert_eq!(
        refused(Mood::Low, vec![phrase(PhraseKind::Unknown(3), 1, 4)]),
        "phrase 1: a Low track has no Phrase 3 phrases"
    );
    let fill = Phrase { fill_beat: Some(0), ..phrase(PhraseKind::Intro, 1, 4) };
    assert_eq!(refused(Mood::Mid, vec![fill]), "phrase 1's fill-in starts on beat 0: beats count from 1");
}

/// A waveform tag's stored columns: whole columns of its size.
#[hegel::test_helper]
fn columns(tc: &TestCase, size: usize) -> Vec<u8> {
    let n = tc.draw(gs::integers::<usize>().min_value(1).max_value(500));
    tc.draw(gs::vecs(gs::integers::<u8>()).min_size(n * size).max_size(n * size))
}

/// Any tag's columns written read back as given, framed as rekordcrate expects (each count is
/// its length), and nothing else changes; written again they stay as they are.
#[hegel::test(test_cases = 200)]
fn waveforms_read_back_as_written(tc: TestCase) {
    let tag = *tc.draw(gs::sampled_from(anlz::waveform_tags().collect::<Vec<_>>()));
    let size = match &tag {
        b"PWV4" => 6,
        b"PWV5" => 2,
        b"PWV6" | b"PWV7" => 3,
        _ => 1,
    };
    let file = base(&tc, tc.draw(gs::booleans()));
    let cols = columns(&tc, size);
    let out = anlz::write_waveform(&file, &tag, &cols).unwrap();
    assert_eq!(anlz::waveform_entries(&out, &tag).unwrap(), Some(cols.clone()));
    rekordcrate_beats(&out);
    assert_eq!(others(&out, &tag), others(&file, &tag));
    assert_eq!(anlz::write_waveform(&out, &tag, &cols).unwrap(), out);
    let short = anlz::write_waveform(&file, &tag, &cols[..cols.len() - 1]);
    assert_eq!(short.is_err(), size > 1 || cols.len() == 1);
}

/// Bands, at 150 columns a second.
#[hegel::test_helper]
fn bands(tc: &TestCase) -> Bands {
    let n = tc.draw(gs::integers::<usize>().min_value(1).max_value(3_000));
    let loud = tc.draw(gs::integers::<u8>());
    let band = || gs::integers::<u8>().max_value(loud);
    Bands((0..n).map(|_| [tc.draw(band()), tc.draw(band()), tc.draw(band())]).collect())
}

/// Every tag but `PWV4` is worked out from bands: previews of rekordbox's widths, details as
/// long as the bands; heights from the loudest band, whiteness and colours from the bands'
/// shares, 3-band heights from each band. Written and read for drawing, the 3-band ones give
/// the bands back (to their precision).
#[hegel::test(test_cases = 100)]
fn waveforms_are_worked_out_from_bands(tc: TestCase) {
    let b = bands(&tc);
    assert_eq!(anlz::waveform_from_bands(b"PWV4", &b).unwrap(), None);
    let loudest = |c: &[u8; 3]| u32::from(c[0].max(c[1]).max(c[2]));
    for (tag, n) in [(b"PWAV", 400), (b"PWV2", 100), (b"PWV6", 1200)] {
        let made = anlz::waveform_from_bands(tag, &b).unwrap().unwrap();
        let size = if tag == b"PWV6" { 3 } else { 1 };
        assert_eq!(made.len(), n * size, "{}", String::from_utf8_lossy(tag));
        let pooled = b.pooled(n);
        if tag == b"PWV2" {
            assert!(made.iter().zip(&pooled.0).all(|(m, c)| u32::from(*m) == loudest(c) * 15 / 255));
        }
    }
    let blue = anlz::waveform_from_bands(b"PWV3", &b).unwrap().unwrap();
    let color = anlz::waveform_from_bands(b"PWV5", &b).unwrap().unwrap();
    assert_eq!((blue.len(), color.len()), (b.0.len(), 2 * b.0.len()));
    for (i, c) in b.0.iter().enumerate() {
        let top = loudest(c);
        assert_eq!(u32::from(blue[i] & 0x1f), top * 31 / 255);
        assert_eq!(u32::from(blue[i] >> 5), u32::from(c[2]) * 7 / top.max(1));
        let v = u16::from_be_bytes([color[2 * i], color[2 * i + 1]]);
        let rgb = [v >> 13, (v >> 10) & 7, (v >> 7) & 7];
        assert_eq!(rgb.map(u32::from), c.map(|x| u32::from(x) * 7 / top.max(1)));
        assert_eq!(u32::from((v >> 2) & 0x1f), top * 31 / 255);
        assert_eq!(v & 3, 0);
    }
    let two = &real_files()[0][2];
    let mut file = two.clone();
    for tag in [b"PWV6", b"PWV7"] {
        file = anlz::write_waveform(&file, tag, &anlz::waveform_from_bands(tag, &b).unwrap().unwrap()).unwrap();
    }
    let (preview, detail) = (anlz::preview(&file).unwrap().unwrap(), anlz::detail(&file).unwrap().unwrap());
    assert_eq!((preview.style, detail.style), (WaveformStyle::ThreeBand, WaveformStyle::ThreeBand));
    assert_eq!((preview.columns.len(), detail.columns.len(), detail.columns_per_sec), (1200, b.0.len(), Some(150.0)));
    // Each band to 1/128 of its height: the detail keeps the top 7 bits of each.
    let back = |c: &[u8; 3]| Bands(vec![c.map(|v| v & !1)]).waveform(None).columns[0];
    for (drawn, c) in detail.columns.iter().zip(&b.0) {
        assert_eq!(*drawn, back(c));
    }
}

/// Bands at another rate are pooled to 150 columns a second, as long in time.
#[test]
fn bands_go_to_the_detail_rate() {
    let b = Bands(vec![[1, 2, 3]; 100]);
    assert_eq!(anlz::at_detail_rate(&b, 75.0).0.len(), 200);
    assert_eq!(anlz::at_detail_rate(&b, 300.0).0.len(), 50);
    assert_eq!(anlz::at_detail_rate(&Bands::default(), 150.0).0.len(), 1);
}

/// A waveform the format can't hold, or a tag that isn't a waveform's, is refused.
#[test]
fn waveforms_that_cant_be_stored_are_refused() {
    let file = common::synthetic_anlz("/Contents/a.mp3");
    let refused = |tag: &[u8; 4], cols: &[u8]| anlz::write_waveform(&file, tag, cols).unwrap_err().to_string();
    assert_eq!(refused(b"PWV5", &[1, 2, 3]), "PWV5 takes whole columns of 2 bytes");
    assert_eq!(refused(b"PWAV", &[]), "PWAV takes whole columns of 1 bytes");
    assert_eq!(refused(b"PQTZ", &[1]), "PQTZ isn't a waveform tag");
    assert!(anlz::waveform_entries(&file, b"PCOB").is_err());
    // A file holding a tag twice is left alone.
    let pwav = sections(&anlz::write_waveform(&file, b"PWAV", &[1]).unwrap()).pop().unwrap().1;
    let mut twice = [file.clone(), pwav.clone(), pwav].concat();
    let len = twice.len() as u32;
    twice[8..12].copy_from_slice(&len.to_be_bytes());
    let err = anlz::write_waveform(&twice, b"PWAV", &[2]).unwrap_err().to_string();
    assert_eq!(err, "the analysis file holds two PWAV sections; left as it is");
}

/// New files hold the track's path and empty cue lists, as rekordbox writes for a track without
/// cues, with the header rekordbox 6 writes; what's written into them goes in rekordbox's order.
#[test]
fn new_files_take_what_is_written_in_rekordbox_order() {
    let (dat, ext) = anlz::new_analysis_files("/Contents/Ärtist/a track.mp3").unwrap();
    for f in [&dat, &ext] {
        assert_eq!(anlz::read_path(f).unwrap().as_deref(), Some("/Contents/Ärtist/a track.mp3"));
        assert_eq!(f[12..28], [0, 0, 0, 1, 0, 1, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0]);
        assert!(anlz::read_cue_points(f, None).unwrap().is_empty());
        rekordcrate_beats(f);
    }
    assert_eq!(tags(&dat), ["PPTH", "PCOB", "PCOB"]);
    assert_eq!(tags(&ext), ["PPTH", "PCOB", "PCOB", "PCO2", "PCO2"]);
    let b = Bands(vec![[200, 100, 50]; 600]);
    let wave = |f: Vec<u8>, tag: &[u8; 4]| {
        anlz::write_waveform(&f, tag, &anlz::waveform_from_bands(tag, &b).unwrap().unwrap()).unwrap()
    };
    let beats = [Beat { beat_number: 1, bpm: 120.0, time_ms: 0 }];
    let dat = wave(wave(anlz::write_beat_grid(&dat, &beats).unwrap(), b"PWV2"), b"PWAV");
    let phrases = SongStructure {
        mood: Mood::Low,
        phrases: vec![Phrase { kind: PhraseKind::Intro, start_beat: 1, end_beat: 2, fill_beat: None }],
    };
    let ext = anlz::write_song_structure(&wave(wave(ext, b"PWV5"), b"PWV3"), Some(&phrases)).unwrap();
    assert_eq!(tags(&dat), ["PPTH", "PQTZ", "PWAV", "PWV2", "PCOB", "PCOB"]);
    assert_eq!(tags(&ext), ["PPTH", "PWV3", "PCOB", "PCOB", "PCO2", "PCO2", "PWV5", "PSSI"]);
    assert_eq!(rekordcrate_beats(&dat), Some(vec![(1, 12000, 0)]));
    rekordcrate_beats(&ext);
}
