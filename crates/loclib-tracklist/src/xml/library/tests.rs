use loclib_model::analysis::Beat;

use crate::{Mark, Tempo, read_xml_library};

/// A document with one collection track: `attrs` in its tag, `children` inside it.
fn one_track(attrs: &str, children: &str) -> String {
    format!(
        "<DJ_PLAYLISTS Version=\"1.0.0\"><COLLECTION Entries=\"1\">\n<TRACK TrackID=\"1\" Name=\"Glue\" {attrs}>\n{children}</TRACK>\n</COLLECTION></DJ_PLAYLISTS>"
    )
}

/// What rekordbox writes of a track: its tags, rating, key and tempo, a grid that changes tempo,
/// and a memory cue, a hot cue with its colour and a loop.
#[test]
fn a_collection_track_reads_with_its_tags_grid_and_cues() {
    let xml = one_track(
        r#"Artist="Bicep" Album="Bicep" Genre="Electronica" Label="Ninja Tune" Remixer="" Composer="M. McBriar"
           Comments="peak &amp; drop" Year="2017" Rating="204" Tonality="Fm" AverageBpm="128.00" TotalTime="245"
           Location="file://localhost/Users/me/Music/Glue.mp3""#,
        r#"<TEMPO Inizio="0.025" Bpm="128.00" Metro="4/4" Battito="1"/>
           <TEMPO Inizio="60.025" Bpm="130.00" Metro="4/4" Battito="3"/>
           <POSITION_MARK Name="" Type="0" Start="0.025" Num="-1"/>
           <POSITION_MARK Name="Drop" Type="0" Start="30.025" Num="2" Red="255" Green="140" Blue="0"/>
           <POSITION_MARK Name="Build" Type="4" Start="15.025" End="16.900" Num="-1"/>
"#,
    );
    let read = read_xml_library(xml.as_bytes()).unwrap();
    assert!(read.warnings.is_empty(), "{:?}", read.warnings);
    let t = &read.tracks[0];
    assert_eq!((t.entry.title.as_str(), t.entry.path.as_deref()), ("Glue", Some("/Users/me/Music/Glue.mp3")));
    let tags = [&t.genre, &t.label, &t.remixer, &t.composer, &t.comment].map(String::as_str);
    assert_eq!(tags, ["Electronica", "Ninja Tune", "", "M. McBriar", "peak & drop"]);
    assert_eq!((t.year, t.rating, t.key.as_deref(), t.bpm), (2017, 4, Some("Fm"), Some(128.0)));
    let tempo = |start_ms, bpm, beat| Tempo { start_ms, bpm, beats_per_bar: 4, beat };
    assert_eq!(t.tempos, [tempo(25.0, 128.0, 1), tempo(60_025.0, 130.0, 3)]);
    let mark = |hot_cue, start_ms, end_ms, name: &str, rgb| Mark { hot_cue, start_ms, end_ms, name: name.into(), rgb };
    assert_eq!(
        t.marks,
        [
            mark(None, 25, None, "", None),
            mark(Some(2), 30_025, None, "Drop", Some([255, 140, 0])),
            mark(None, 15_025, Some(16_900), "Build", None)
        ]
    );
}

/// A tempo, cue or loop with a value missing or out of range is left out, and so is a track
/// tempo that isn't one, each kind counted in a warning; a track rekordbox hasn't analysed
/// (`AverageBpm="0.00"`) has no tempo and no warning.
#[test]
fn values_out_of_range_are_left_out_and_counted() {
    let xml = one_track(
        r#"AverageBpm="NaN""#,
        r#"<TEMPO Inizio="inf" Bpm="128"/>
           <TEMPO Inizio="0" Bpm="700"/>
           <TEMPO Inizio="0" Bpm="128" Metro="7/8"/>
           <TEMPO Inizio="0" Bpm="128" Battito="5"/>
           <TEMPO Inizio="-1.5" Bpm="128"/>
           <POSITION_MARK Type="0" Start="-1" Num="-1"/>
           <POSITION_MARK Type="0" Start="1" Num="8"/>
           <POSITION_MARK Type="4" Start="2" End="2" Num="-1"/>
           <POSITION_MARK Type="1" Start="2" Num="-1"/>
           <POSITION_MARK Type="0" Start="1e12" Num="-1"/>
           <POSITION_MARK Type="0" Start="1" Num="1"/>
"#,
    );
    let read = read_xml_library(xml.as_bytes()).unwrap();
    let t = &read.tracks[0];
    assert_eq!((t.bpm, t.tempos.len(), t.marks.len()), (None, 1, 1));
    assert_eq!(t.tempos[0].start_ms, -1500.0, "a grid may start before the audio");
    assert_eq!(
        read.warnings,
        [
            "1 track tempo(s) (AverageBpm) left out: not 0.01-655.35",
            "4 beat grid tempo(s) (TEMPO) left out: a time, tempo or beat out of range",
            "5 cue(s) (POSITION_MARK) left out: a type, time or hot cue out of range",
        ]
    );
    let unanalysed = read_xml_library(one_track(r#"AverageBpm="0.00""#, "").as_bytes()).unwrap();
    assert_eq!((unanalysed.tracks[0].bpm, unanalysed.warnings.len()), (None, 0));
}

/// The grid runs from the first beat at or after 0 to the track's end, changing tempo where a
/// `TEMPO` says, counting beats in the bar from each one's `Battito`.
#[test]
fn a_grid_expands_to_the_tracks_end() {
    let xml = one_track(
        r#"TotalTime="3""#,
        r#"<TEMPO Inizio="1.000" Bpm="120.00" Battito="2"/>
           <TEMPO Inizio="-0.800" Bpm="60.00" Metro="3/4" Battito="1"/>
"#,
    );
    let read = read_xml_library(xml.as_bytes()).unwrap();
    let (beats, cut) = read.tracks[0].beats();
    let beat = |beat_number, bpm, time_ms| Beat { beat_number, bpm, time_ms };
    // The 60 BPM tempo's first beat is at -800 ms: its next, at 200 ms, is the grid's first.
    let want =
        [beat(2, 60.0, 200), beat(2, 120.0, 1000), beat(3, 120.0, 1500), beat(4, 120.0, 2000), beat(1, 120.0, 2500)];
    assert_eq!((beats.as_slice(), cut), (&want[..], false));
    let no_length = read_xml_library(one_track("", r#"<TEMPO Inizio="0" Bpm="120"/>"#).as_bytes()).unwrap();
    assert_eq!(no_length.tracks[0].beats(), (Vec::new(), false), "no end to stop at");
}

/// A grid is cut at MAX_BEATS, saying so: a ten-hour track at the fastest tempo has more.
#[test]
fn a_grid_is_cut_at_the_most_beats() {
    let xml = one_track(r#"TotalTime="36000""#, r#"<TEMPO Inizio="0" Bpm="655.35"/>"#);
    let (beats, cut) = read_xml_library(xml.as_bytes()).unwrap().tracks[0].beats();
    assert_eq!((beats.len(), cut), (crate::MAX_BEATS, true));
}

/// Elements nested deeper than MAX_DEPTH are left out with what's in them, counted, and what
/// comes after them reads as ever.
#[test]
fn elements_nested_too_deep_are_left_out() {
    let deep = 70;
    let open: String = (0..deep).map(|i| format!("<NODE Type=\"0\" Name=\"f{i}\">")).collect();
    let close = "</NODE>".repeat(deep);
    let xml = format!(
        "<DJ_PLAYLISTS><COLLECTION><TRACK TrackID=\"1\" Name=\"A\"/></COLLECTION><PLAYLISTS>{open}<NODE Type=\"1\" Name=\"lost\"/>{close}<NODE Type=\"1\" Name=\"kept\" KeyType=\"0\"><TRACK Key=\"1\"/></NODE></PLAYLISTS></DJ_PLAYLISTS>"
    );
    let read = read_xml_library(xml.as_bytes()).unwrap();
    let names: Vec<&str> = read.playlists.iter().map(|(p, _)| p.as_str()).collect();
    assert_eq!(names, ["kept"]);
    assert_eq!(read.warnings, ["1 element(s) nested more than 64 deep left out, with what's in them"]);
}
