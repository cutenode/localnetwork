//! The Traktor importer served over pipes, as `locallibrary import` runs it, on collections in the
//! shape Traktor writes them.

use std::io::pipe;
use std::path::PathBuf;
use std::sync::mpsc::Receiver;
use std::thread::JoinHandle;

use hegel::generators as gs;
use hegel::{Generator, TestCase};
use loclib_plugin::{Incoming, PROTOCOL_VERSION, Peer, method};
use serde_json::{Value, json};
use tempfile::TempDir;

use super::*;

/// The plugin served on a thread, over pipes, past `hello`, and a folder for its files.
struct Session {
    host: Peer,
    served: JoinHandle<()>,
    /// What the plugin sends unasked: these hooks call nothing back.
    incoming: Receiver<Incoming>,
    dir: TempDir,
}

fn start() -> Session {
    let (plugin_in, to_plugin) = pipe().unwrap();
    let (from_plugin, plugin_out) = pipe().unwrap();
    let served = std::thread::spawn(move || crate::serve(plugin_in, plugin_out));
    let (host, incoming) = Peer::new(from_plugin, to_plugin);
    let hello = json!({ "protocol": PROTOCOL_VERSION, "host": { "name": "locallibrary", "version": "0.1.0" } });
    host.request::<_, Value>(method::HELLO, &hello).unwrap();
    Session { host, served, incoming, dir: tempfile::tempdir().unwrap() }
}

impl Session {
    /// `nml` as a file of the session's.
    fn file(&self, nml: &str) -> PathBuf {
        let file = self.dir.path().join("collection.nml");
        std::fs::write(&file, nml).unwrap();
        file
    }

    /// `import/read` of `file`, with `context`.
    fn import(&self, file: &Path, context: Value) -> Result<ImportAnswer, RpcError> {
        let read = json!({ "importer": "traktor", "file": file, "options": null, "context": context });
        self.host.request(method::READ_IMPORT, &read)
    }

    /// What `import/read` reads in `nml`, in a message.
    fn imported(&self, nml: &str) -> Imported {
        match self.import(&self.file(nml), json!({})).unwrap() {
            ImportAnswer::Read(read) => read,
            blob => panic!("in a message: {blob:?}"),
        }
    }

    fn stop(self) {
        self.host.close();
        self.served.join().unwrap();
        assert!(self.incoming.try_iter().all(|m| !matches!(m, Incoming::Request(_))), "nothing called back");
    }
}

/// A collection as Traktor writes one: a track with a key, a tempo, a grid marker and cues of
/// every kind (some the formats can't take), one with a key as text alone, and one with nothing to
/// import.
const NML: &str = r##"<?xml version="1.0" encoding="UTF-8" standalone="no" ?>
<NML VERSION="19"><HEAD COMPANY="www.native-instruments.com" PROGRAM="Traktor"></HEAD>
<COLLECTION ENTRIES="3">
<ENTRY MODIFIED_DATE="2024/3/1" TITLE="Glue" ARTIST="Bicep"><LOCATION DIR="/:Contents/:Bicep/:" FILE="Glue.mp3" VOLUME="USB" VOLUMEID="USB"></LOCATION>
<INFO KEY="8A" PLAYTIME="269" PLAYTIME_FLOAT="268.538000"></INFO><TEMPO BPM="128.000000" BPM_QUALITY="100.000000"></TEMPO><MUSICAL_KEY VALUE="21"></MUSICAL_KEY>
<CUE_V2 NAME="AutoGrid" DISPL_ORDER="0" TYPE="4" START="49.5" LEN="0.000000" REPEATS="-1" HOTCUE="-1"><GRID BPM="128.000000"></GRID></CUE_V2>
<CUE_V2 NAME="Drop" DISPL_ORDER="0" TYPE="0" START="29588.6" LEN="0.000000" REPEATS="-1" HOTCUE="0"></CUE_V2>
<CUE_V2 NAME="n.n." DISPL_ORDER="0" TYPE="5" START="59127.7" LEN="1875.000000" REPEATS="-1" HOTCUE="1"></CUE_V2>
<CUE_V2 NAME="Beat" DISPL_ORDER="0" TYPE="4" START="1000.000000" LEN="0.000000" REPEATS="-1" HOTCUE="2"></CUE_V2>
<CUE_V2 NAME="n.n." DISPL_ORDER="0" TYPE="3" START="-20.000000" LEN="0.000000" REPEATS="-1" HOTCUE="-1" COLOR="#FF8800"></CUE_V2>
<CUE_V2 NAME="In" DISPL_ORDER="0" TYPE="1" START="500.000000" LEN="0.000000" REPEATS="-1" HOTCUE="-1"></CUE_V2>
<CUE_V2 NAME="Half" DISPL_ORDER="0" TYPE="5" START="2000.000000" LEN="234.375000" REPEATS="-1" HOTCUE="-1"></CUE_V2>
<CUE_V2 NAME="Odd" DISPL_ORDER="0" TYPE="5" START="3000.000000" LEN="1000.000000" REPEATS="-1" HOTCUE="3"></CUE_V2>
<CUE_V2 NAME="Far" DISPL_ORDER="0" TYPE="0" START="4000.000000" LEN="0.000000" REPEATS="-1" HOTCUE="9"></CUE_V2>
<CUE_V2 NAME="New" DISPL_ORDER="0" TYPE="7" START="0.000000" LEN="0.000000" REPEATS="-1" HOTCUE="-1"></CUE_V2>
</ENTRY>
<ENTRY TITLE="b"><LOCATION DIR="/:" FILE="b.wav" VOLUME="E:"></LOCATION><INFO KEY="Fm"></INFO>
<CUE_V2 NAME="n.n." TYPE="5" START="0" LEN="500" HOTCUE="-1"></CUE_V2></ENTRY>
<ENTRY TITLE="Nothing"><LOCATION DIR="/:" FILE="c.mp3" VOLUME="USB"></LOCATION></ENTRY>
</COLLECTION>
<PLAYLISTS></PLAYLISTS></NML>
"##;

fn cue(
    hot_cue: Option<&str>,
    start_ms: u32,
    loop_beats: Option<f64>,
    name: Option<&str>,
    color: Option<&str>,
) -> ImportedCue {
    ImportedCue {
        hot_cue: hot_cue.map(String::from),
        start_ms,
        loop_beats,
        name: name.map(String::from),
        color: color.map(String::from),
    }
}

/// Each track's file, artist, title and length to find it by, its key and tempo, and its cues on
/// their pads, loops in beats, in the colours Traktor shows them; grid markers off the pads,
/// hot cues past H and loops that aren't a length in beats left out, saying so.
#[test]
fn a_collection_is_imported() {
    let s = start();
    let read = s.imported(NML);
    let glue = ImportedTrack {
        find: TrackMatch {
            path: Some(Path::new("/Volumes/USB/Contents/Bicep/Glue.mp3").to_string_lossy().into_owned()),
            isrc: None,
            artist: Some("Bicep".into()),
            title: Some("Glue".into()),
            duration: Some(269),
        },
        key: Some("8A".into()),
        bpm: Some(128.0),
        cues: vec![
            cue(Some("A"), 29_589, None, Some("Drop"), Some("blue")),
            cue(Some("B"), 59_128, Some(4.0), None, Some("green")),
            cue(Some("C"), 1000, None, Some("Beat"), None),
            cue(None, 0, None, None, Some("#ff8800")),
            cue(None, 500, None, Some("In"), Some("orange")),
            cue(None, 2000, Some(0.5), Some("Half"), Some("green")),
        ],
        my_tags: Vec::new(),
    };
    let b = ImportedTrack {
        find: TrackMatch {
            path: Some(format!("E:{}b.wav", std::path::MAIN_SEPARATOR)),
            title: Some("b".into()),
            ..TrackMatch::default()
        },
        key: Some("Fm".into()),
        ..ImportedTrack::default()
    };
    assert_eq!(read.tracks, [glue, b]);
    assert_eq!(
        read.warnings,
        [
            "1 marker(s) of a type Traktor doesn't document left out",
            "1 hot cue(s) past H left out",
            "2 loop(s) left out: not a whole number of beats (or 1/2 to 1/32 of one) at the track's tempo",
        ]
    );
    assert!(read.sessions.is_empty());
    s.stop();
}

/// A file that isn't there, or isn't a collection, fails as the host shows: about the file, or
/// as what can't be done.
#[test]
fn what_cant_be_read_fails() {
    let s = start();
    let missing = s.dir.path().join("gone.nml");
    let e = s.import(&missing, json!({})).unwrap_err();
    assert_eq!(e.kind(), Some("io"));
    assert!(e.message.starts_with(&missing.display().to_string()), "{e:?}");
    let e = s.import(&s.file("<plist/>"), json!({})).unwrap_err();
    assert_eq!(
        (e.kind(), e.message.as_str()),
        (Some("invalid"), "not a Traktor collection: its root element is plist, not NML")
    );
    s.stop();
}

/// An answer past what a message holds goes in a blob in the scratch folder, holding what it
/// would have said; without a folder it's said in the message all the same.
#[test]
fn a_big_answer_is_a_blob() {
    let s = start();
    let title = "x".repeat(MESSAGE);
    let file = s.file(&format!(
        r#"<NML><COLLECTION><ENTRY TITLE="{title}"><LOCATION DIR="/:" FILE="a.mp3" VOLUME="USB"/><TEMPO BPM="120"/></ENTRY></COLLECTION></NML>"#
    ));
    let scratch = s.dir.path().join("scratch");
    std::fs::create_dir(&scratch).unwrap();
    let blob = s.import(&file, json!({ "scratch": scratch })).unwrap();
    assert_eq!(blob, ImportAnswer::Blob(Blob { blob: "read.json".into() }));
    let held: Imported = serde_json::from_slice(&std::fs::read(scratch.join("read.json")).unwrap()).unwrap();
    let ImportAnswer::Read(said) = s.import(&file, json!({})).unwrap() else { panic!("no folder, no blob") };
    assert_eq!((held.tracks[0].find.title.as_ref().map(String::len), &held), (Some(MESSAGE), &said));
    let e = s.import(&file, json!({ "scratch": s.dir.path().join("gone") })).unwrap_err();
    assert_eq!(e.kind(), Some("io"));
    s.stop();
}

/// A marker as a collection might hold one: of any type (one nobody documents too), anywhere,
/// on a pad (one past H too) or none, any length, named or not.
#[hegel::composite]
fn marker(tc: &TestCase) -> Drawn {
    Drawn {
        kind: tc.draw(gs::integers::<u8>().max_value(6)),
        start: f64::from(tc.draw(gs::integers::<i32>().min_value(-100).max_value(600_000))),
        len: tc.draw(hegel::one_of!(
            gs::just(0.0),
            // A loop of some beats at 120 BPM, a little off, or any length.
            gs::sampled_from(vec![0.5_f64, 1.0, 4.0, 32.0, 0.03125, 3.3, 513.0]).map(|beats| beats * 500.0 * 1.01),
            gs::floats::<f64>().min_value(1.0).max_value(100_000.0),
        )),
        hot: tc.draw(gs::integers::<i32>().min_value(-1).max_value(9)),
        name: tc.draw(gs::sampled_from(vec!["n.n.", "", "Drop", "a & b"])),
    }
}

/// A `CUE_V2` as drawn.
#[derive(Debug, Clone)]
struct Drawn {
    kind: u8,
    start: f64,
    len: f64,
    hot: i32,
    name: &'static str,
}

impl Drawn {
    fn nml(&self) -> String {
        let Drawn { kind, start, len, hot, name } = self;
        let name = name.replace('&', "&amp;");
        format!(r#"<CUE_V2 NAME="{name}" TYPE="{kind}" START="{start}" LEN="{len}" HOTCUE="{hot}"/>"#)
    }
}

/// What a marker the importer takes says, as the format describes it: its pad, where it is (a
/// marker before the start at the start), a loop's length in beats at 120 BPM when it's one cue
/// edits take, its name and the colour Traktor shows its kind in. `None` for a grid marker off
/// the pads; an error for what's left out.
fn expected(m: &Drawn, bpm: Option<f64>) -> Option<Result<ImportedCue, &'static str>> {
    let Drawn { kind, start, len, hot, name } = *m;
    let colours = [Some("blue"), Some("orange"), Some("orange"), Some("yellow"), None, Some("green")];
    let colour = *colours.get(usize::from(kind))?;
    if kind == 4 && hot < 0 {
        return None;
    }
    if hot > 7 {
        return Some(Err("past H"));
    }
    let loop_beats = match kind == 5 && len > 0.0 {
        true => {
            let beats = len * bpm.unwrap_or(f64::NAN) / 60_000.0;
            let lengths = [0.03125, 0.0625, 0.125, 0.25, 0.5].into_iter().chain((1..=512).map(f64::from));
            match lengths.into_iter().find(|n| ((beats - n) / n).abs() <= 0.02) {
                Some(n) => Some(n),
                None => return Some(Err("unsized")),
            }
        }
        false => None,
    };
    Some(Ok(ImportedCue {
        hot_cue: (hot >= 0).then(|| ["A", "B", "C", "D", "E", "F", "G", "H"][hot as usize].to_string()),
        start_ms: start.max(0.0).round() as u32,
        loop_beats,
        name: (!name.is_empty() && name != "n.n.").then(|| name.to_string()),
        color: colour.map(String::from),
    }))
}

/// Whatever markers a track has, the importer answers those the formats take as they say (a
/// grid marker off the pads being the grid), and counts the hot cues past H and loops that
/// aren't a length in beats; a track with nothing to import isn't answered.
#[hegel::test(test_cases = 60)]
fn markers_are_imported_as_cues(tc: TestCase) {
    let s = start();
    let tracks = tc.draw(gs::integers::<usize>().max_value(3));
    let mut nml = String::from("<NML><COLLECTION>");
    let mut want = Vec::new();
    let (mut past_h, mut unsized_loops) = (0, 0);
    for i in 0..tracks {
        let bpm = tc.draw(gs::optional(gs::just(120.0)));
        let key = tc.draw(gs::optional(gs::integers::<u8>().max_value(25)));
        let markers: Vec<Drawn> = tc.draw(gs::vecs(marker()).max_size(6));
        let tempo = bpm.map_or(String::new(), |b| format!(r#"<TEMPO BPM="{b}"/>"#));
        let musical = key.map_or(String::new(), |k| format!(r#"<MUSICAL_KEY VALUE="{k}"/>"#));
        let cues: String = markers.iter().map(Drawn::nml).collect();
        nml += &format!(
            r#"<ENTRY TITLE="t{i}"><LOCATION DIR="/:" FILE="{i}.mp3" VOLUME="USB"/>{tempo}{musical}{cues}</ENTRY>"#
        );
        let mut cues = Vec::new();
        for m in &markers {
            match expected(m, bpm) {
                Some(Ok(c)) => cues.push(c),
                Some(Err("past H")) => past_h += 1,
                Some(Err(_)) => unsized_loops += 1,
                None => {}
            }
        }
        let key = key.filter(|k| *k < 24).map(|k| Camelot::from_pitch(u32::from(k % 12), k < 12).code());
        if key.is_some() || bpm.is_some() || !cues.is_empty() {
            let find = TrackMatch {
                path: Some(Path::new("/Volumes/USB").join(format!("{i}.mp3")).to_string_lossy().into_owned()),
                title: Some(format!("t{i}")),
                ..TrackMatch::default()
            };
            want.push(ImportedTrack { find, key, bpm, cues, my_tags: Vec::new() });
        }
    }
    nml += "</COLLECTION></NML>";
    let read = s.imported(&nml);
    assert_eq!(read.tracks, want);
    let warned = |what: &str| {
        read.warnings.iter().find(|w| w.contains(what)).map(|w| w.split(' ').next().unwrap().parse::<usize>().unwrap())
    };
    assert_eq!(
        (warned("past H"), warned("loop(s)")),
        ((past_h > 0).then_some(past_h), (unsized_loops > 0).then_some(unsized_loops))
    );
    s.stop();
}
