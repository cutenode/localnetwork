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
           <TEMPO Inizio="0" Bpm="128" Metro="5/4"/>
           <TEMPO Inizio="0" Bpm="128" Metro="0/4"/>
           <TEMPO Inizio="0" Bpm="128" Battito="5"/>
           <TEMPO Inizio="-1.5" Bpm="128"/>
           <POSITION_MARK Type="0" Start="-1" Num="-1"/>
           <POSITION_MARK Type="0" Start="1" Num="8"/>
           <POSITION_MARK Type="4" Start="2" End="2" Num="-1"/>
           <POSITION_MARK Type="1" Start="2" Num="-1"/>
           <POSITION_MARK Type="2" Start="2" Num="-1"/>
           <POSITION_MARK Type="3" Start="nope" Num="-1"/>
           <POSITION_MARK Type="9" Start="2" Num="-1"/>
           <POSITION_MARK Type="0" Start="1e12" Num="-1"/>
           <POSITION_MARK Type="0" Start="1" Num="1"/>
"#,
    );
    let read = read_xml_library(xml.as_bytes()).unwrap();
    let t = &read.tracks[0];
    assert_eq!((t.bpm, t.tempos.len(), t.marks.len()), (None, 1, 1));
    assert_eq!(t.tempos[0].start_ms, -1500.0, "a grid may start before the audio");
    // Fades, load points and other meters are values rekordbox writes, which aren't taken: not
    // supported, rather than out of range.
    assert_eq!(
        read.warnings,
        [
            "1 track tempo(s) (AverageBpm) left out: not 0.01-655.35",
            "4 beat grid tempo(s) (TEMPO) left out: a time, tempo or beat out of range",
            "2 beat grid tempo(s) (TEMPO) left out: only 1-4 beats to the bar (Metro) are supported",
            "5 cue(s) (POSITION_MARK) left out: a type, time or hot cue out of range",
            "3 cue(s) (POSITION_MARK) left out: fade-in, fade-out and load marks (Type 1-3) aren't supported",
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

/// rekordbox writes a grid that moves as a TEMPO on each beat, its time rounded to the
/// millisecond, so a tempo's next beat can land just before the next TEMPO: each beat comes once.
/// The markers are rekordbox 5.8.6's for its Demo Track 2, from rekordcrate's sample
/// (https://github.com/Holzhaus/rekordcrate/blob/14d54eded7b0d2f6fe67c7871533dca35a73e5fa/data/xml/database.xml#L47-L56).
#[test]
fn a_tempo_on_each_beat_gives_each_beat_once() {
    let xml = one_track(
        r#"TotalTime="128""#,
        r#"<TEMPO Inizio="0.025" Bpm="120.00" Metro="4/4" Battito="1"/>
      <TEMPO Inizio="48.026" Bpm="120.00" Metro="4/4" Battito="1"/>
      <TEMPO Inizio="48.525" Bpm="120.00" Metro="4/4" Battito="2"/>
      <TEMPO Inizio="49.026" Bpm="120.00" Metro="4/4" Battito="3"/>
      <TEMPO Inizio="49.525" Bpm="120.00" Metro="4/4" Battito="4"/>
      <TEMPO Inizio="50.026" Bpm="120.00" Metro="4/4" Battito="1"/>
      <TEMPO Inizio="50.525" Bpm="120.00" Metro="4/4" Battito="2"/>
      <TEMPO Inizio="51.026" Bpm="120.00" Metro="4/4" Battito="3"/>
      <TEMPO Inizio="51.525" Bpm="120.00" Metro="4/4" Battito="4"/>
      <TEMPO Inizio="52.026" Bpm="120.00" Metro="4/4" Battito="1"/>
"#,
    );
    let (beats, _) = read_xml_library(xml.as_bytes()).unwrap().tracks[0].beats();
    let around: Vec<(u32, u16)> =
        beats.iter().filter(|b| (47_000..=50_100).contains(&b.time_ms)).map(|b| (b.time_ms, b.beat_number)).collect();
    let want = [(47_025, 3), (47_525, 4), (48_026, 1), (48_525, 2), (49_026, 3), (49_525, 4), (50_026, 1)];
    assert_eq!(around, want);
    // One beat every half second, from 25 ms to the end.
    assert_eq!(beats.len(), 256);
}

/// A beat less than half a millisecond before the track's end would round to the end itself:
/// the grid stops before it.
#[test]
fn no_beat_rounds_to_the_tracks_end() {
    let xml = one_track(r#"TotalTime="1""#, r#"<TEMPO Inizio="0.4996" Bpm="120"/>"#);
    let (beats, _) = read_xml_library(xml.as_bytes()).unwrap().tracks[0].beats();
    assert_eq!(beats.iter().map(|b| b.time_ms).collect::<Vec<_>>(), [500]);
}

/// Playlists below a folder with a 1 MiB name each cost that much of their folder paths: 15 fit
/// in MAX_PATH_BYTES (16 MiB), and the rest are left out, counted.
#[test]
fn playlists_paths_are_bounded() {
    let folder = "f".repeat(1 << 20);
    let lists: String = (0..20).map(|i| format!("<NODE Type=\"1\" Name=\"p{i}\" KeyType=\"0\"/>")).collect();
    let xml = format!(
        "<DJ_PLAYLISTS><PLAYLISTS><NODE Type=\"0\" Name=\"{folder}\">{lists}</NODE></PLAYLISTS></DJ_PLAYLISTS>"
    );
    let read = read_xml_library(xml.as_bytes()).unwrap();
    assert_eq!(read.playlists.len(), 15);
    assert_eq!(read.warnings, ["5 playlist(s) left out: their folder paths come to more than 16 MiB"]);
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
    let names: Vec<&str> = read.playlists.iter().map(|p| p.path.as_str()).collect();
    assert_eq!(names, ["kept"]);
    assert_eq!(read.warnings, ["1 element(s) nested more than 64 deep left out, with what's in them"]);
}
