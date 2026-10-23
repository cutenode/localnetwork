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
