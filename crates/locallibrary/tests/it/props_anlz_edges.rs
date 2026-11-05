//! Analysis files at the edges of what the writers and readers take: grids, phrases and waveforms
//! the format can't hold, files holding a section twice or one cut short, cue lists repeated
//! within a file or across the `.DAT` and `.EXT`, and a track whose only waveforms are the
//! 3-band ones in its `.2EX`.

use crate::common;

use hegel::generators as gs;
use hegel::{HealthCheck, TestCase};
use locallibrary::anlz::{self, Beat, CueFile, CuePoint, Mood, Phrase, PhraseKind, SongStructure, WaveformStyle};
use locallibrary::device::Device;
use locallibrary::paths;

/// Each top-level section's tag and bytes, in order.
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

/// `bytes` with `extra` (whole sections) added after its last section, its length fixed.
fn appended(bytes: &[u8], extra: &[Vec<u8>]) -> Vec<u8> {
    let mut out = [bytes.to_vec(), extra.concat()].concat();
    let len = out.len() as u32;
    out[8..12].copy_from_slice(&len.to_be_bytes());
    out
}

/// `bytes` without its sections tagged one of `tags`, its length fixed.
fn without(bytes: &[u8], tags: &[&[u8; 4]]) -> Vec<u8> {
    let header = u32::from_be_bytes(bytes[4..8].try_into().unwrap()) as usize;
    let kept: Vec<Vec<u8>> = sections(bytes).into_iter().filter(|(t, _)| !tags.contains(&t)).map(|(_, s)| s).collect();
    appended(&bytes[..header], &kept)
}

/// A section of `tag` with a 12-byte header and `body`.
fn bare_section(tag: &[u8; 4], body: &[u8]) -> Vec<u8> {
    let mut s = tag.to_vec();
    s.extend_from_slice(&12u32.to_be_bytes());
    s.extend_from_slice(&(12 + body.len() as u32).to_be_bytes());
    s.extend_from_slice(body);
    s
}

/// A grid of `n` beats at `bpm`, numbered through their bars from `first`.
fn steady(n: usize, first: u16, bpm: f64) -> Vec<Beat> {
    (0..n)
        .map(|i| Beat {
            beat_number: ((usize::from(first) - 1 + i) % 4) as u16 + 1,
            bpm,
            time_ms: 100 + 500 * i as u32,
        })
        .collect()
}

/// A grid the format can't hold, whatever is wrong with it, is refused saying why, the file
/// untouched: no beats, a beat outside 1-4, a tempo outside 0.01-655.35 BPM, or a beat not after
/// the one before.
#[hegel::test(test_cases = 200)]
fn prop_grids_the_format_cant_hold_are_refused(tc: TestCase) {
    let dat = common::synthetic_anlz("/Contents/a.mp3");
    let mut beats = steady(tc.draw(gs::integers::<usize>().min_value(1).max_value(20)), 1, 120.0);
    let at = tc.draw(gs::integers::<usize>().max_value(beats.len() - 1));
    let n = at + 1;
    let why = match tc.draw(gs::integers::<u8>().max_value(3)) {
        0 => {
            beats.clear();
            "a beat grid needs a beat".to_string()
        }
        1 => {
            let b = tc.draw(hegel::one_of!(gs::just(0u16), gs::integers::<u16>().min_value(5)));
            beats[at].beat_number = b;
            format!("beat {n} is beat {b} of its bar: 1-4")
        }
        2 => {
            let bpm = tc.draw(hegel::one_of!(
                gs::just(f64::NAN),
                gs::floats::<f64>().max_value(0.004),
                gs::floats::<f64>().min_value(655.36)
            ));
            beats[at].bpm = bpm;
            format!("beat {n}'s tempo, {bpm} BPM, isn't 0.01-655.35")
        }
        _ => {
            let at = at.max(1);
            if at >= beats.len() {
                beats.push(beats[at - 1]);
            }
            beats[at].time_ms = tc.draw(gs::integers::<u32>().max_value(beats[at - 1].time_ms));
            format!("beat {} isn't after the one before it", at + 1)
        }
    };
    assert_eq!(anlz::write_beat_grid(&dat, &beats).unwrap_err().to_string(), why);
    assert_eq!(anlz::checked_grid(&beats).unwrap_err().to_string(), why);
}

/// Phrases a `PSSI` can't hold are refused saying why: none, a gap or overlap between two, a
/// first one on beat 0, or a fill-in on beat 0.
#[hegel::test(test_cases = 200)]
fn prop_phrases_the_format_cant_hold_are_refused(tc: TestCase) {
    let ext = common::synthetic_anlz("/Contents/a.mp3");
    let mood = tc.draw(gs::sampled_from(vec![Mood::High, Mood::Mid, Mood::Low]));
    let n = tc.draw(gs::integers::<u16>().min_value(1).max_value(8));
    let mut phrases: Vec<Phrase> = (0..n)
        .map(|i| Phrase { kind: PhraseKind::Intro, start_beat: 1 + 8 * i, end_beat: 9 + 8 * i, fill_beat: None })
        .collect();
    let at = tc.draw(gs::integers::<usize>().max_value(phrases.len() - 1));
    let why = match tc.draw(gs::integers::<u8>().max_value(2)) {
        0 => {
            phrases.clear();
            "phrases need a phrase".to_string()
        }
        1 => {
            phrases[at].fill_beat = Some(0);
            format!("phrase {}'s fill-in starts on beat 0: beats count from 1", at + 1)
        }
        _ => {
            let p = &mut phrases[at];
            p.end_beat = tc.draw(gs::integers::<u16>().max_value(p.start_beat));
            format!("phrase {} runs from beat {} to {}", at + 1, p.start_beat, p.end_beat)
        }
    };
    let err = anlz::write_song_structure(&ext, Some(&SongStructure { mood, phrases })).unwrap_err().to_string();
    assert!(err.starts_with(&why), "{err} (wanted {why})");
}

/// Only the waveform tags take waveforms: any other tag is refused for writing and reading, and a
/// waveform's columns must be whole columns of its size.
#[hegel::test(test_cases = 200)]
fn prop_only_waveform_tags_take_waveforms(tc: TestCase) {
    let file = common::synthetic_anlz("/Contents/a.mp3");
    let tags: Vec<[u8; 4]> = anlz::waveform_tags().copied().collect();
    let tag = tc.draw(hegel::one_of!(gs::sampled_from(tags.clone()), gs::arrays::<_, u8, 4>(gs::integers::<u8>())));
    let columns = tc.draw(gs::vecs(gs::integers::<u8>()).max_size(12));
    if !tags.contains(&tag) {
        let why = format!("{} isn't a waveform tag", String::from_utf8_lossy(&tag));
        assert_eq!(anlz::write_waveform(&file, &tag, &columns).unwrap_err().to_string(), why);
        assert_eq!(anlz::waveform_entries(&file, &tag).unwrap_err().to_string(), why);
        return;
    }
    let size = match &tag {
        b"PWV4" => 6,
        b"PWV5" => 2,
        b"PWV6" | b"PWV7" => 3,
        _ => 1,
    };
    let written = anlz::write_waveform(&file, &tag, &columns);
    let whole = !columns.is_empty() && columns.len().is_multiple_of(size);
    assert_eq!(written.is_ok(), whole, "{} columns of {}", columns.len(), String::from_utf8_lossy(&tag));
}

/// A file holding the section a writer replaces twice is left as it is, saying so: a grid's
/// `PQTZ`, phrases' `PSSI` or any waveform's tag.
#[hegel::test(test_cases = 100)]
fn prop_a_section_held_twice_is_left_alone(tc: TestCase) {
    let file = common::synthetic_anlz("/Contents/a.mp3");
    let structure = SongStructure {
        mood: Mood::Mid,
        phrases: vec![Phrase { kind: PhraseKind::Intro, start_beat: 1, end_beat: 9, fill_beat: None }],
    };
    let waveforms: Vec<[u8; 4]> = anlz::waveform_tags().copied().collect();
    let tag = tc.draw(gs::sampled_from([vec![*b"PQTZ", *b"PSSI"], waveforms].concat()));
    let size = |t: &[u8; 4]| match t {
        b"PWV4" => 6,
        b"PWV5" => 2,
        b"PWV6" | b"PWV7" => 3,
        _ => 1,
    };
    let write = |bytes: &[u8]| match &tag {
        b"PQTZ" => anlz::write_beat_grid(bytes, &steady(4, 1, 120.0)),
        b"PSSI" => anlz::write_song_structure(bytes, Some(&structure)),
        t => anlz::write_waveform(bytes, t, &vec![1; size(t)]),
    };
    let once = write(&file).unwrap();
    let section = sections(&once).into_iter().find(|(t, _)| *t == tag).unwrap().1;
    let twice = appended(&once, &[section]);
    let why = format!("the analysis file holds two {} sections; left as it is", String::from_utf8_lossy(&tag));
    assert_eq!(write(&twice).unwrap_err().to_string(), why);
}

/// A grid with no beat to start from doubles to nothing, and one with no beat 1 or 3 to keep
/// halves to nothing.
#[hegel::test(test_cases = 100)]
fn prop_grids_without_a_downbeat_to_keep_double_and_halve_to_nothing(tc: TestCase) {
    assert!(anlz::doubled_grid(&[]).is_empty());
    let n = tc.draw(gs::integers::<usize>().max_value(10));
    let even: Vec<Beat> = (0..n)
        .map(|i| Beat { beat_number: tc.draw(gs::sampled_from(vec![2u16, 4])), bpm: 120.0, time_ms: 500 * i as u32 })
        .collect();
    assert!(anlz::halved_grid(&even).is_empty());
}

/// Shifting a file's times leaves an extended grid (`PQT2`) or a detail waveform too short to
/// hold the fields it would move as it is, and shifts the rest.
#[hegel::test(test_cases = 100)]
fn prop_sections_too_short_to_shift_are_kept(tc: TestCase) {
    let (dat, _) = anlz::new_analysis_files("/Contents/a.mp3").unwrap();
    let dat = anlz::write_beat_grid(&dat, &steady(8, 1, 120.0)).unwrap();
    let tag = tc.draw(gs::sampled_from(vec![*b"PQT2", *b"PWV3", *b"PWV5", *b"PWV7"]));
    let len = tc.draw(gs::integers::<usize>().max_value(match &tag {
        b"PQT2" => 0x1c - 12,
        _ => 7,
    }));
    let short = bare_section(&tag, &tc.draw(gs::vecs(gs::integers::<u8>()).min_size(len).max_size(len)));
    let file = appended(&dat, std::slice::from_ref(&short));
    let ms = tc.draw(gs::integers::<i32>().min_value(-5_000).max_value(5_000));
    let out = anlz::shift_times(&file, ms).unwrap();
    assert_eq!(sections(&out).last().unwrap().1, short);
    let moved = |b: &Beat| Beat { time_ms: (i64::from(b.time_ms) + i64::from(ms)).max(0) as u32, ..*b };
    let want: Vec<Beat> = anlz::beat_grid(&file).unwrap().iter().map(moved).collect();
    assert_eq!(anlz::beat_grid(&out).unwrap(), want);
}

/// Cues of a track without an `.EXT`, by position: some memory cues, hot cues and loops.
#[hegel::test_helper]
fn some_cues(tc: &TestCase) -> Vec<CuePoint> {
    let n = tc.draw(gs::integers::<usize>().max_value(6));
    (0..n)
        .map(|_| {
            let start_ms = tc.draw(gs::integers::<u32>().max_value(600_000));
            let end_ms =
                tc.draw(gs::optional(gs::integers::<u32>().min_value(1).max_value(8_000))).map(|l| start_ms + l);
            CuePoint {
                hot_cue: tc.draw(gs::optional(gs::integers::<u8>().max_value(7))),
                start_ms,
                end_ms,
                active_loop: end_ms.is_some() && tc.draw(gs::booleans()),
                ..Default::default()
            }
        })
        .collect()
}

/// A `.DAT` whose cue list of a kind appears twice gets the cues in the first and an empty
/// second, so no reader finds stale cues there: it reads back as given.
#[hegel::test(test_cases = 100)]
fn prop_a_second_cue_list_of_a_kind_is_emptied(tc: TestCase) {
    let (dat, _) = anlz::new_analysis_files("/Contents/a.mp3").unwrap();
    let old = anlz::write_cue_points(&dat, CueFile::DatAlone, &some_cues(&tc)).unwrap();
    let lists: Vec<Vec<u8>> = sections(&old).into_iter().filter(|(t, _)| t == b"PCOB").map(|(_, s)| s).collect();
    let twice = appended(&old, &[lists[tc.draw(gs::integers::<usize>().max_value(lists.len() - 1))].clone()]);
    let cues = some_cues(&tc);
    let out = anlz::write_cue_points(&twice, CueFile::DatAlone, &cues).unwrap();
    let key = |c: &CuePoint| (c.hot_cue, c.start_ms, c.end_ms, c.active_loop);
    let mut want: Vec<_> = cues.iter().map(key).collect();
    let mut got: Vec<_> = anlz::read_cue_points(&out, None).unwrap().iter().map(key).collect();
    want.sort();
    got.sort();
    assert_eq!(got, want);
    let repeated = sections(&out).into_iter().filter(|(t, _)| t == b"PCOB").nth(2).unwrap().1;
    assert_eq!(
        u32::from_be_bytes(repeated[8..12].try_into().unwrap()),
        u32::from_be_bytes(repeated[4..8].try_into().unwrap())
    );
}

/// Older exports list a cue in both the `.DAT`'s and the `.EXT`'s `PCOB`: read together, each
/// is one cue, as the `.DAT` alone has them (two alike in one file stay two).
#[hegel::test(test_cases = 100)]
fn prop_a_cue_both_files_list_reads_once(tc: TestCase) {
    let (dat, _) = anlz::new_analysis_files("/Contents/a.mp3").unwrap();
    let mut cues = some_cues(&tc);
    if let Some(c) = cues.first().cloned().filter(|_| tc.draw(gs::booleans())) {
        cues.push(c);
    }
    let dat = anlz::write_cue_points(&dat, CueFile::DatAlone, &cues).unwrap();
    assert_eq!(anlz::read_cue_points(&dat, Some(&dat)).unwrap(), anlz::read_cue_points(&dat, None).unwrap());
}

/// A loop read from a file, its end taken away, is written and read back as a cue at the same
/// place (stored as a point, not a loop).
#[hegel::test(test_cases = 100)]
fn prop_a_loop_cut_to_a_cue_reads_back_as_a_cue(tc: TestCase) {
    let (dat, _) = anlz::new_analysis_files("/Contents/a.mp3").unwrap();
    let start_ms = tc.draw(gs::integers::<u32>().max_value(600_000));
    let hot_cue = tc.draw(gs::optional(gs::integers::<u8>().max_value(7)));
    let looped = CuePoint { hot_cue, start_ms, end_ms: Some(start_ms + 4_000), ..Default::default() };
    let dat = anlz::write_cue_points(&dat, CueFile::DatAlone, &[looped]).unwrap();
    let mut read = anlz::read_cue_points(&dat, None).unwrap();
    assert!(read[0].is_loop());
    read[0].end_ms = None;
    let out = anlz::write_cue_points(&dat, CueFile::DatAlone, &read).unwrap();
    let back = anlz::read_cue_points(&out, None).unwrap();
    assert_eq!((back.len(), back[0].hot_cue, back[0].start_ms, back[0].end_ms), (1, hot_cue, start_ms, None));
    let cue = anlz::cues(&out).unwrap();
    assert_eq!(cue.len(), 1);
    assert_eq!((cue[0].start_ms, cue[0].end_ms), (start_ms, None));
}

/// A track whose `.DAT` and `.EXT` lack an overview or detail waveform shows the 3-band ones of
/// its `.2EX` in their place; the ones it has come first (colour, then blue).
#[hegel::test(test_cases = 12, suppress_health_check = [HealthCheck::TooSlow])]
fn prop_a_track_without_other_waveforms_shows_its_3_band_ones(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let root = common::demo_drive(dir.path(), "USB");
    let d = Device::open_export(&root).unwrap();
    let lib = d.library().unwrap();
    let analysed = |t: &&locallibrary::model::Track| {
        paths::to_fs(&root, &paths::key(&t.analyze_path)).with_extension("2EX").is_file()
    };
    let t = lib.tracks.iter().find(analysed).unwrap().clone();
    let dat = paths::to_fs(&root, &paths::key(&t.analyze_path));
    let (no_blue, no_colour, no_detail) = (tc.draw(gs::booleans()), tc.draw(gs::booleans()), tc.draw(gs::booleans()));
    let strip = |path: &std::path::Path, tags: &[&[u8; 4]]| {
        std::fs::write(path, without(&std::fs::read(path).unwrap(), tags)).unwrap();
    };
    if no_blue {
        strip(&dat, &[b"PWAV"]);
    }
    let ext = dat.with_extension("EXT");
    let mut gone: Vec<&[u8; 4]> = Vec::new();
    if no_colour {
        gone.push(b"PWV4");
    }
    if no_detail {
        gone.extend([b"PWV3", b"PWV5"]);
    }
    strip(&ext, &gone);
    let a = anlz::load(&d, &t);
    let preview = match (no_colour, no_blue) {
        (false, _) => WaveformStyle::Rgb,
        (true, false) => WaveformStyle::Blue,
        (true, true) => WaveformStyle::ThreeBand,
    };
    assert_eq!(a.preview.map(|w| w.style), Some(preview));
    let detail = if no_detail { WaveformStyle::ThreeBand } else { WaveformStyle::Rgb };
    assert_eq!(a.detail.map(|w| w.style), Some(detail));
}

/// Asking whether something that isn't an analysis file has a section is an error, not an answer.
#[hegel::test(test_cases = 100)]
fn prop_has_section_refuses_what_isnt_an_analysis_file(tc: TestCase) {
    let bytes = tc.draw(gs::binary().max_size(64));
    if !bytes.starts_with(b"PMAI") {
        assert!(anlz::has_section(&bytes, b"PQTZ").is_err());
    }
}
