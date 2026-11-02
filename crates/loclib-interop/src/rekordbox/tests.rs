use std::path::PathBuf;

use loclib_model::analysis::{Beat, Cue, TrackAnalysis};

use super::*;

#[test]
fn locations_are_file_urls() {
    let audio = Path::new("/Volumes/USB/Contents/Daft Punk/Ärm & Co/01 x.mp3");
    assert_eq!(location(audio), "file://localhost/Volumes/USB/Contents/Daft%20Punk/%C3%84rm%20%26%20Co/01%20x.mp3");
    assert_eq!(location(Path::new("E:\\Contents\\a b.mp3")), "file://localhost/E:/Contents/a%20b.mp3");
}

/// Each element's attributes, by element name, in order, as quick-xml reads them.
fn elements(xml: &str) -> Vec<(String, rustc_hash::FxHashMap<String, String>)> {
    let mut reader = quick_xml::Reader::from_str(xml);
    let mut out = Vec::new();
    loop {
        match reader.read_event().unwrap() {
            quick_xml::events::Event::Start(e) | quick_xml::events::Event::Empty(e) => {
                let attrs = e
                    .attributes()
                    .map(|a| a.unwrap())
                    .map(|a| {
                        (
                            a.key.as_ref().to_owned(),
                            a.normalized_value(quick_xml::XmlVersion::Implicit1_0).unwrap().into(),
                        )
                    })
                    .collect();
                out.push((e.name().as_ref().to_owned(), attrs));
            }
            quick_xml::events::Event::Eof => return out,
            _ => {}
        }
    }
}

/// A track's colour, kind and year, a TEMPO for each tempo change, and its cues: loops with
/// their ends, hot cues with their numbers and colours, memory cues without either.
#[test]
fn tracks_carry_their_colour_kind_tempos_and_cues() {
    let t = Track {
        id: 7,
        artist: "A & B".into(),
        title: "Title".into(),
        file_path: "/Contents/a.flac".into(),
        color: Some(TrackColor::Aqua),
        release_date: "2019-05-01".into(),
        rating: 9,
        ..Default::default()
    };
    let beat = |time_ms, bpm, beat_number| Beat { beat_number, bpm, time_ms };
    let cue = |hot_cue, start_ms, end_ms, color| Cue {
        hot_cue,
        start_ms,
        end_ms,
        comment: "Drop".into(),
        color,
        color_id: 0,
        loop_beats: None,
        active_loop: false,
    };
    let analysis = TrackAnalysis {
        beats: vec![beat(0, 120.0, 1), beat(500, 120.0, 2), beat(1000, 122.0, 7)],
        cues: vec![cue(Some(0), 1500, Some(3500), Some([255, 0, 0])), cue(None, 250, None, Some([0, 255, 0]))],
        ..Default::default()
    };
    let load = |_: &Track| analysis.clone();
    let library = Library::default();
    let src = Source { library: &library, root: PathBuf::from("/Volumes/USB"), analysis: Some(&load) };
    let (mut x, mut export) = (String::new(), Export::default());
    track(&mut x, &t, &src, &mut export);
    let e = elements(&x);
    let names: Vec<&str> = e.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(names, ["TRACK", "TEMPO", "TEMPO", "POSITION_MARK", "POSITION_MARK"]);
    let track = &e[0].1;
    let said = |k: &str| track[k].as_str();
    assert_eq!(
        [said("Kind"), said("Colour"), said("Year"), said("Rating"), said("Artist")],
        ["FLAC File", "0x25FDE9", "2019", "255", "A & B"]
    );
    assert_eq!(
        (e[2].1["Bpm"].as_str(), e[2].1["Inizio"].as_str(), e[2].1["Battito"].as_str()),
        ("122.00", "1.000", "4")
    );
    let (hot, memory) = (&e[3].1, &e[4].1);
    let hot: Vec<&str> = ["Type", "Start", "End", "Num", "Red", "Green"].iter().map(|k| hot[*k].as_str()).collect();
    assert_eq!(hot, ["4", "1.500", "3.500", "0", "255", "0"]);
    assert_eq!((memory["Type"].as_str(), memory["Num"].as_str(), memory.get("Red")), ("0", "-1", None));
    assert_eq!((export.beat_grids, export.cues), (1, 2));
}

/// What the XML writer writes of a track, loclib-tracklist's reader (which the tracklist plugin
/// imports and builds drives from rekordbox XML with) reads back: its tags, key, tempo and
/// rating, a TEMPO for each tempo change that expands back into the same grid, and its cues.
#[hegel::test(test_cases = 200)]
fn the_xml_reader_reads_back_what_the_writer_writes(tc: hegel::TestCase) {
    use hegel::Generator;
    use hegel::generators as gs;
    // What XML holds, which is what the writer keeps.
    let holds = |c: char| (c >= ' ' || matches!(c, '\t' | '\n' | '\r')) && !matches!(c, '\u{FFFE}' | '\u{FFFF}');
    let text = move || gs::text().max_size(12).map(move |s| s.chars().filter(|&c| holds(c)).collect::<String>());
    let mut t = loclib_model::test_util::track(1, &tc.draw(text()), &tc.draw(text()), "/Contents/a.mp3");
    [t.genre, t.label, t.remixer, t.composer, t.comment, t.key] = std::array::from_fn(|_| tc.draw(text()));
    t.year = tc.draw(gs::integers::<u16>());
    t.rating = tc.draw(gs::integers::<u8>().max_value(5));
    t.bpm = f64::from(tc.draw(gs::integers::<u16>())) / 100.0;
    // A grid of tempos that change, each a few beats long but the last, which runs to the end.
    let mut beats = Vec::new();
    let mut tempos = Vec::new();
    let mut start = f64::from(tc.draw(gs::integers::<u16>().max_value(2_000)));
    for last in (0..tc.draw(gs::integers::<usize>().min_value(1).max_value(4))).rev().map(|i| i == 0) {
        let before = tempos.last().map(|(_, bpm, _)| *bpm);
        let bpm = f64::from(tc.draw(gs::integers::<u16>().min_value(6_000).max_value(20_000))) / 100.0;
        tc.assume(before != Some(bpm));
        let first = tc.draw(gs::integers::<u16>().min_value(1).max_value(4));
        let interval = 60_000.0 / bpm;
        tempos.push((start, bpm, first as u8));
        let count = match last {
            false => tc.draw(gs::integers::<u32>().min_value(1).max_value(16)),
            true => {
                t.duration_secs = (start / 1000.0) as u32 + tc.draw(gs::integers::<u32>().min_value(1).max_value(60));
                // Each beat whose millisecond is before the end.
                ((f64::from(t.duration_secs) * 1000.0 - 0.5 - start) / interval).ceil() as u32
            }
        };
        for k in 0..count {
            let time_ms = (start + f64::from(k) * interval).round() as u32;
            beats.push(Beat { beat_number: (first - 1 + k as u16) % 4 + 1, bpm, time_ms });
        }
        // The next tempo starts on the millisecond the next beat would have been in.
        start = (start + f64::from(count) * interval).floor();
    }
    let cues: Vec<Cue> = (0..tc.draw(gs::integers::<usize>().max_value(4)))
        .map(|_| {
            let start_ms = tc.draw(gs::integers::<u32>().max_value(3_600_000));
            Cue {
                hot_cue: tc.draw(gs::optional(gs::integers::<u8>().max_value(7))),
                start_ms,
                end_ms: tc
                    .draw(gs::optional(gs::integers::<u32>().min_value(1).max_value(60_000)))
                    .map(|n| start_ms + n),
                comment: tc.draw(text()),
                color: tc
                    .draw(gs::optional(hegel::tuples!(gs::integers(), gs::integers(), gs::integers())))
                    .map(|(r, g, b)| [r, g, b]),
                color_id: 0,
                loop_beats: None,
                active_loop: false,
            }
        })
        .collect();
    let analysis = TrackAnalysis { beats: beats.clone(), cues: cues.clone(), ..Default::default() };
    let load = |_: &Track| analysis.clone();
    let library = Library::default();
    let src = Source { library: &library, root: PathBuf::from("/Volumes/USB"), analysis: Some(&load) };
    let mut x = String::from("<DJ_PLAYLISTS Version=\"1.0.0\">\n  <COLLECTION Entries=\"1\">\n");
    track(&mut x, &t, &src, &mut Export::default());
    x.push_str("  </COLLECTION>\n</DJ_PLAYLISTS>\n");
    tc.note(&x);

    let read = loclib_tracklist::read_xml_library(x.as_bytes()).unwrap();
    assert!(read.warnings.is_empty(), "{:?}", read.warnings);
    let r = &read.tracks[0];
    let said = [&r.entry.title, &r.entry.artist, &r.genre, &r.label, &r.remixer, &r.composer, &r.comment];
    let written = [&t.title, &t.artist, &t.genre, &t.label, &t.remixer, &t.composer, &t.comment];
    assert_eq!(said.map(|s| s.as_str()), written.map(|s| s.trim()));
    assert_eq!(r.key.as_deref(), Some(t.key.trim()).filter(|k| !k.is_empty()));
    assert_eq!((r.bpm, r.year, r.rating), ((t.bpm > 0.0).then_some(t.bpm), t.year, t.rating));
    let read_tempos: Vec<(f64, f64, u8)> = r.tempos.iter().map(|x| (x.start_ms, x.bpm, x.beat)).collect();
    assert_eq!(read_tempos, tempos);
    assert_eq!(r.beats(), (beats, false));
    let marks: Vec<loclib_tracklist::Mark> = cues
        .into_iter()
        .map(|c| loclib_tracklist::Mark {
            hot_cue: c.hot_cue,
            start_ms: c.start_ms,
            end_ms: c.end_ms,
            name: c.comment,
            // Only a hot cue's colour is written.
            rgb: c.hot_cue.and(c.color),
        })
        .collect();
    assert_eq!(r.marks, marks);
}

#[test]
fn kinds_and_colours_are_rekordboxs_own() {
    let kinds: Vec<&str> = ["a.MP3", "a.m4a", "a.aac", "a.wav", "a.aif", "a.aiff", "a.flac"].map(kind).into();
    assert_eq!(kinds, ["MP3 File", "M4A File", "M4A File", "WAV File", "AIFF File", "AIFF File", "FLAC File"]);
    use TrackColor::*;
    let colours = [Pink, Red, Orange, Yellow, Green, Aqua, Blue, Purple].map(colour);
    assert_eq!(
        colours,
        ["0xFF007F", "0xFF0000", "0xFFA500", "0xFFFF00", "0x00FF00", "0x25FDE9", "0x0000FF", "0x660099"]
    );
}
