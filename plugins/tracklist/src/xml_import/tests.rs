use std::path::Path;

use hegel::generators as gs;
use hegel::{Generator, TestCase};
use loclib_model::model::Library;
use loclib_plugin::method;
use serde_json::json;

use super::*;
use crate::tests::{kind, start, write};

/// A rekordbox XML file holding `tracks`, each a TRACK's attributes and what's inside it, and
/// the `playlists` tree (NODEs).
pub(crate) fn collection_with(tracks: &[(String, String)], playlists: &str) -> String {
    let tracks: String = tracks.iter().map(|(attrs, inside)| format!("<TRACK {attrs}>\n{inside}</TRACK>\n")).collect();
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<DJ_PLAYLISTS Version=\"1.0.0\">\n<COLLECTION>\n{tracks}</COLLECTION>\n<PLAYLISTS>{playlists}</PLAYLISTS>\n</DJ_PLAYLISTS>\n"
    )
}

fn collection(tracks: &[(String, String)]) -> String {
    collection_with(tracks, "")
}

/// What the importer answers for `file`, with `scratch` as the request's folder.
fn answer(file: &str, scratch: Option<&Path>) -> Result<ImportAnswer, RpcError> {
    let s = start(Library::default());
    let req = json!({ "importer": "rekordbox-xml", "file": file, "context": { "scratch": scratch } });
    let answer = s.host.request(method::READ_IMPORT, &req);
    assert!(s.stop().is_empty(), "it reads nothing through the host");
    answer
}

fn read(file: &str) -> Imported {
    match answer(file, None).unwrap() {
        ImportAnswer::Read(read) => read,
        ImportAnswer::Blob(b) => panic!("a blob, {b:?}"),
    }
}

/// Each collection track is found by its file, artist, title and length, with its key, tempo and
/// cues: a hot cue's slot as a letter and its colour, a loop's length in the beats its tempo makes
/// it. A loop that isn't a length cue edits take comes as a cue, and what the reader left out is
/// said, both counted.
#[test]
fn a_collection_answers_each_tracks_match_key_tempo_and_cues() {
    let dir = tempfile::tempdir().unwrap();
    let glue = (
        r#"TrackID="1" Name="Glue" Artist="Bicep" TotalTime="269" Tonality="Fm" AverageBpm="128.00" Location="file://localhost/Users/me/Music/Glue.mp3""#.to_string(),
        r#"<TEMPO Inizio="0.000" Bpm="128.00" Metro="4/4" Battito="1"/>
           <TEMPO Inizio="60.000" Bpm="120.00" Metro="4/4" Battito="1"/>
           <POSITION_MARK Name="" Type="0" Start="0.000" Num="-1"/>
           <POSITION_MARK Name="Drop" Type="0" Start="30.000" Num="2" Red="255" Green="140" Blue="0"/>
           <POSITION_MARK Name="" Type="4" Start="15.000" End="16.875" Num="-1"/>
           <POSITION_MARK Name="" Type="4" Start="61.000" End="61.250" Num="7"/>
           <POSITION_MARK Name="" Type="4" Start="20.000" End="21.000" Num="-1"/>
           <POSITION_MARK Name="" Type="0" Start="nope" Num="-1"/>
"#
        .to_string(),
    );
    let bare = (r#"TrackID="2" Name="Untitled""#.to_string(), String::new());
    let file = write(dir.path(), "rekordbox.xml", &collection(&[glue, bare]));
    let read = read(&file);
    let cue = |hot_cue: Option<&str>, start_ms, loop_beats, name: Option<&str>, color: Option<&str>| ImportedCue {
        hot_cue: hot_cue.map(String::from),
        start_ms,
        loop_beats,
        name: name.map(String::from),
        color: color.map(String::from),
    };
    let glue = ImportedTrack {
        find: TrackMatch {
            path: Some("/Users/me/Music/Glue.mp3".into()),
            artist: Some("Bicep".into()),
            title: Some("Glue".into()),
            duration: Some(269),
            isrc: None,
        },
        key: Some("Fm".into()),
        bpm: Some(128.0),
        cues: vec![
            cue(None, 0, None, None, None),
            cue(Some("C"), 30_000, None, Some("Drop"), Some("#ff8c00")),
            cue(None, 15_000, Some(4.0), None, None),
            cue(Some("H"), 61_000, Some(0.5), None, None),
            cue(None, 20_000, None, None, None),
        ],
        my_tags: Vec::new(),
    };
    let bare = ImportedTrack {
        find: TrackMatch { title: Some("Untitled".into()), ..TrackMatch::default() },
        ..ImportedTrack::default()
    };
    assert_eq!(read.tracks, [glue, bare]);
    assert_eq!(
        read.warnings,
        [
            "1 cue(s) (POSITION_MARK) left out: a type, time or hot cue out of range",
            "1 loop(s) imported as cues: not 1-512 beats or 1/2 to 1/32 of one at the track's tempo",
        ]
    );
    let manifest = loclib_plugin::Manifest::parse(crate::MANIFEST).unwrap();
    let importer = &manifest.contributes.importers[0];
    assert_eq!((importer.format.as_str(), importer.extension.as_deref()), ("rekordbox-xml", Some("xml")));
}

/// An answer bigger than a quarter of a message is a file in the request's scratch folder.
#[test]
fn a_big_answer_comes_in_a_blob() {
    let dir = tempfile::tempdir().unwrap();
    let name = "n".repeat(500);
    let tracks: Vec<(String, String)> =
        (0..10_000).map(|id| (format!(r#"TrackID="{id}" Name="{name}" Artist="{id}""#), String::new())).collect();
    let file = write(dir.path(), "big.xml", &collection(&tracks));
    let scratch = tempfile::tempdir().unwrap();
    let answer = answer(&file, Some(scratch.path())).unwrap();
    assert_eq!(answer, ImportAnswer::Blob(Blob { blob: "read.json".into() }));
    let text = std::fs::read_to_string(scratch.path().join("read.json")).unwrap();
    let read: Imported = serde_json::from_str(&text).unwrap();
    assert_eq!((read.tracks.len(), read.tracks[9_999].find.artist.as_deref()), (10_000, Some("9999")));
}

/// A file that isn't there, or isn't rekordbox XML, is refused, naming it.
#[test]
fn what_cant_be_read_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let gone = dir.path().join("gone.xml").to_string_lossy().into_owned();
    let e = answer(&gone, None).unwrap_err();
    assert!(kind(&e).0 == "io" && kind(&e).1.contains("gone.xml"), "{e:?}");
    let m3u = write(dir.path(), "list.xml", "#EXTM3U\n/Music/a.mp3\n");
    let e = answer(&m3u, None).unwrap_err();
    assert_eq!(kind(&e), ("invalid", format!("{m3u}: not a rekordbox XML file (no DJ_PLAYLISTS element)").as_str()));
}

/// The loop lengths cue edits take, in beats.
fn lengths() -> Vec<f64> {
    (1..=512).map(f64::from).chain((1..=5).map(|i| 0.5f64.powi(i))).collect()
}

/// For any collection, each track's cues come back in order, a hot cue A-H by its slot; a loop
/// a whole number of beats long at its tempo (to the millisecond) comes back with that length,
/// any other with a length cue edits take within 1% of it, or none, counted in a warning.
#[hegel::test(test_cases = 50)]
fn any_collections_loops_come_back_in_beats_or_counted(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let mut tracks = Vec::new();
    // Each track's cues as written: the hot cue, the start and, for a loop, its length in beats
    // when it's one of `lengths`.
    type Written = (Option<u8>, u32, Option<Option<f64>>);
    let mut wanted: Vec<Vec<Written>> = Vec::new();
    for id in 0..tc.draw(gs::integers::<usize>().max_value(4)) {
        let bpm = f64::from(tc.draw(gs::integers::<u16>().min_value(6_000).max_value(20_000))) / 100.0;
        let mut inside = format!("<TEMPO Inizio=\"0.000\" Bpm=\"{bpm:.2}\" Metro=\"4/4\" Battito=\"1\"/>\n");
        let mut cues = Vec::new();
        for _ in 0..tc.draw(gs::integers::<usize>().max_value(4)) {
            let hot_cue = tc.draw(gs::optional(gs::integers::<u8>().max_value(7)));
            let start_ms = tc.draw(gs::integers::<u32>().max_value(600_000));
            let length = tc.draw(gs::optional(hegel::one_of!(
                gs::sampled_from(lengths()).map(Ok::<f64, u32>),
                gs::integers::<u32>().min_value(1).max_value(600_000).map(Err::<f64, u32>)
            )));
            let len_ms = length.map(|l| match l {
                Ok(beats) => (beats * 60_000.0 / bpm).round() as u32,
                Err(ms) => ms,
            });
            let num = hot_cue.map_or(-1, i32::from);
            let mut mark =
                format!("<POSITION_MARK Name=\"\" Start=\"{:.3}\" Num=\"{num}\"", f64::from(start_ms) / 1000.0);
            match len_ms {
                Some(len) => mark += &format!(" Type=\"4\" End=\"{:.3}\"/>\n", f64::from(start_ms + len) / 1000.0),
                None => mark += " Type=\"0\"/>\n",
            }
            inside += &mark;
            cues.push((hot_cue, start_ms, length.map(|l| l.ok())));
        }
        tracks.push((format!("TrackID=\"{id}\" Name=\"t{id}\" TotalTime=\"700\""), inside));
        wanted.push(cues);
    }
    let file = write(dir.path(), "rekordbox.xml", &collection(&tracks));
    tc.note(&std::fs::read_to_string(&file).unwrap());
    let read = read(&file);
    let mut unsized_loops = 0;
    for (track, cues) in read.tracks.iter().zip(&wanted) {
        assert_eq!(track.cues.len(), cues.len());
        for (got, (hot_cue, start_ms, length)) in track.cues.iter().zip(cues) {
            let letter = hot_cue.map(|h| ["A", "B", "C", "D", "E", "F", "G", "H"][usize::from(h)].to_string());
            assert_eq!((&got.hot_cue, got.start_ms), (&letter, *start_ms));
            match (length, got.loop_beats) {
                (None, beats) => assert_eq!(beats, None, "a cue isn't a loop"),
                (Some(Some(exact)), beats) => assert_eq!(beats, Some(*exact)),
                (Some(None), Some(beats)) => assert!(lengths().contains(&beats), "{beats}"),
                (Some(None), None) => unsized_loops += 1,
            }
        }
    }
    let said = read.warnings.iter().find_map(|w| {
        w.strip_suffix(" loop(s) imported as cues: not 1-512 beats or 1/2 to 1/32 of one at the track's tempo")
    });
    assert_eq!(said.map_or(0, |n| n.parse().unwrap()), unsized_loops);
}

/// A loop takes the length in beats it's within 1% (or 2 ms) of, and none past 512 beats.
#[test]
fn loops_within_a_percent_of_a_length_take_it() {
    // 4 beats at 128 BPM is 1875 ms; 1% of that is 18.75 ms.
    let at = |ms| loop_beats(ms, 128.0);
    assert_eq!([at(1875), at(1893), at(1857), at(1895), at(1854)], [Some(4.0), Some(4.0), Some(4.0), None, None]);
    // 1/32 of a beat at 128 BPM is 14.6 ms: within 2 ms.
    assert_eq!([at(15), at(16), at(12)], [Some(1.0 / 32.0), Some(1.0 / 32.0), None]);
    assert_eq!([at(240_000), at(300_000)], [Some(512.0), None], "512 beats at most");
}
