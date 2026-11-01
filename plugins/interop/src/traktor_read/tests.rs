//! The Traktor importer and source served over pipes, as `locallibrary import` and `collection
//! export --from interop:traktor` run them, on collections in the shape Traktor writes them.

use std::io::pipe;
use std::sync::mpsc::Receiver;
use std::thread::JoinHandle;

use hegel::generators as gs;
use hegel::{Generator, TestCase};
use loclib_model::analysis::{Beat, Cue, TrackAnalysis};
use loclib_model::test_util::track;
use loclib_model::{Library, Playlist, Track};
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

    /// `library/read` of `file`'s page after `cursor`, granted `roots`, with `context`.
    fn read_library(
        &self,
        file: &Path,
        roots: &[PathBuf],
        cursor: Option<&str>,
        context: Value,
    ) -> Result<LibraryPage, RpcError> {
        let read = json!({
            "source": "traktor",
            "options": { "collection": file },
            "roots": roots,
            "cursor": cursor,
            "context": context,
        });
        self.host.request(method::READ_LIBRARY, &read)
    }

    /// `library`'s collection as loclib-interop writes it, its audio below the session's folder,
    /// with `analysis`.
    fn built(&self, library: &Library, analysis: &dyn Fn(&Track) -> TrackAnalysis) -> PathBuf {
        let src = loclib_interop::Source { library, root: self.dir.path().into(), analysis: Some(analysis) };
        let export = traktor::build(&src, Path::new("out"), &Ctx::silent()).unwrap();
        let loclib_interop::Content::Bytes(nml) = &export.files[0].content else { panic!("the NML is bytes") };
        self.file(std::str::from_utf8(nml).unwrap())
    }

    /// A folder of the session's, made, as the host grants it: its links resolved.
    fn granted(&self, name: &str) -> PathBuf {
        let dir = self.dir.path().join(name);
        std::fs::create_dir_all(&dir).unwrap();
        dunce::canonicalize(dir).unwrap()
    }

    fn stop(self) {
        self.host.close();
        self.served.join().unwrap();
        assert!(self.incoming.try_iter().all(|m| !matches!(m, Incoming::Request(_))), "nothing called back");
    }
}

/// A collection as Traktor writes one: a track with a key, a tempo, a grid marker and cues of
/// every kind (some the formats can't take), one with a key as text alone, and one with nothing to
/// import, and a smart playlist (which has nothing to do with an import).
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
<PLAYLISTS><NODE TYPE="SMARTLIST" NAME="Recent"/></PLAYLISTS></NML>
"##;

/// A path written with `/`, as this computer writes it (`\` on Windows).
fn native(path: &str) -> String {
    path.replace('/', std::path::MAIN_SEPARATOR_STR)
}

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
            path: Some(native("/Volumes/USB/Contents/Bicep/Glue.mp3")),
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
        find: TrackMatch { path: Some(native("E:/b.wav")), title: Some("b".into()), ..TrackMatch::default() },
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
                path: Some(native(&format!("/Volumes/USB/{i}.mp3"))),
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

fn list(id: u32, parent_id: Option<u32>, name: &str, track_ids: Vec<u32>, is_folder: bool) -> Playlist {
    Playlist { id, parent_id, name: name.into(), is_folder, sort_order: id, track_ids }
}

/// A grid at `bpm` from `from_ms`, a beat a step, `n` beats.
fn grid(bpm: f64, from_ms: u32, n: u16) -> Vec<Beat> {
    let step = 60_000.0 / bpm;
    (0..n)
        .map(|i| Beat { beat_number: i % 4 + 1, bpm, time_ms: from_ms + (f64::from(i) * step).round() as u32 })
        .collect()
}

/// The collection's tracks, each by its audio file, with their tags, key, tempo, cues and the
/// grid its grid marker lays down to the end of the track; one in a folder not granted by its path
/// alone; its playlists by their folders, listing tracks by path, on the last page.
#[test]
fn a_collection_is_a_library_source() {
    let s = start();
    let music = s.granted("music");
    let a = Track {
        key: "Am".into(),
        genre: "House".into(),
        duration_secs: 4,
        rating: 3,
        year: 2019,
        ..track(1, "Art", "A", "/music/a.mp3")
    };
    let b = Track { duration_secs: 0, ..track(2, "", "B", "/music/sub/b.mp3") };
    let c = track(3, "", "C", "/elsewhere/c.mp3");
    let library = Library {
        tracks: vec![a, b, c],
        playlists: vec![
            list(1, None, "Sets", vec![], true),
            list(2, Some(1), "Friday", vec![1, 3], false),
            list(3, None, "All", vec![2], false),
        ],
        ..Library::default()
    };
    let cue = |hot_cue, start_ms, end_ms, comment: &str| Cue {
        hot_cue,
        start_ms,
        end_ms,
        comment: comment.into(),
        color: None,
        color_id: 0,
        loop_beats: None,
        active_loop: false,
    };
    let analysis = |t: &Track| match t.id {
        1 => TrackAnalysis {
            beats: grid(128.0, 100, 2),
            cues: vec![cue(Some(0), 1000, None, "Drop"), cue(None, 2000, Some(3875), "")],
            ..TrackAnalysis::default()
        },
        // A grid but no length to lay it down to.
        _ => TrackAnalysis { beats: grid(120.0, 0, 1), ..TrackAnalysis::default() },
    };
    let file = s.built(&library, &analysis);
    let page = s.read_library(&file, std::slice::from_ref(&music), None, json!({})).unwrap();
    let at = |p: &str| s.dir.path().join(p);
    let paths: Vec<&Path> = page.tracks.iter().map(|t| t.path.as_path()).collect();
    assert_eq!(paths, [at("music/a.mp3"), at("music/sub/b.mp3"), at("elsewhere/c.mp3")]);
    let [a, b, c] = &page.tracks[..] else { panic!("three tracks") };
    assert_eq!(
        (a.title.as_str(), a.artist.as_str(), a.genre.as_str(), a.key.as_deref(), a.bpm, a.rating, a.year),
        ("A", "Art", "House", Some("8A"), Some(128.0), 3, 2019)
    );
    let cues = [
        SourceCue { hot_cue: Some(0), start_ms: 1000, end_ms: None, name: "Drop".into(), color: Some("blue".into()) },
        SourceCue {
            hot_cue: None,
            start_ms: 2000,
            end_ms: Some(3875),
            name: String::new(),
            color: Some("green".into()),
        },
    ];
    assert_eq!(a.cues, cues);
    // 4 s at 128 BPM from the downbeat at 100 ms: nine beats.
    assert_eq!(a.beats, grid(128.0, 100, 9));
    assert_eq!((b.title.as_str(), b.bpm, b.beats.len()), ("B", Some(120.0), 0));
    assert_eq!(c, &SourceTrack { path: at("elsewhere/c.mp3"), ..SourceTrack::default() });
    let playlists = [
        SourcePlaylist { path: "Sets/Friday".into(), tracks: vec![at("music/a.mp3"), at("elsewhere/c.mp3")] },
        SourcePlaylist { path: "All".into(), tracks: vec![at("music/sub/b.mp3")] },
    ];
    assert_eq!((page.playlists.as_slice(), page.next, page.blob), (&playlists[..], None, None));
    assert_eq!(page.warnings, ["1 beat grid(s) left out: the track's length isn't known, or is over a day"]);
    s.stop();
}

/// A collection is read a thousand tracks a page, its playlists on the last; a cursor that isn't
/// one of its pages, options without the collection and a collection that isn't there are
/// refused.
#[test]
fn a_collection_is_read_in_pages() {
    let s = start();
    let entry = |i: usize| format!(r#"<ENTRY TITLE="{i}"><LOCATION DIR="/:" FILE="{i}.mp3" VOLUME="USB"/></ENTRY>"#);
    let entries: String = (0..1001).map(entry).collect();
    let list = r#"<NODE TYPE="PLAYLIST" NAME="Last"><PLAYLIST><ENTRY><PRIMARYKEY TYPE="TRACK" KEY="USB/:1000.mp3"/></ENTRY></PLAYLIST></NODE><NODE TYPE="SMARTLIST" NAME="Recent"/>"#;
    let file = s.file(&format!("<NML><COLLECTION>{entries}</COLLECTION><PLAYLISTS>{list}</PLAYLISTS></NML>"));
    let first = s.read_library(&file, &[], None, json!({})).unwrap();
    assert_eq!((first.tracks.len(), first.playlists.len(), first.warnings.len()), (1000, 0, 0));
    let next = first.next.unwrap();
    let (index, stamp) = next.split_once(' ').unwrap();
    let meta = std::fs::metadata(&file).unwrap();
    assert_eq!((index, stamp.split(':').next()), ("1000", Some(meta.len().to_string().as_str())));
    for cursor in [format!("1002 {stamp}"), "x".into(), "y z".into()] {
        let e = s.read_library(&file, &[], Some(&cursor), json!({})).unwrap_err();
        assert_eq!(e.kind(), Some("invalid"), "{e:?}");
        assert_eq!(e.message, format!("{cursor:?} isn't a page of {}", file.display()));
    }
    // What's read stays read between pages while the file stays as it is (its size and when it
    // changed): here as it was, though its bytes aren't.
    let nml = std::fs::read(&file).unwrap();
    std::fs::write(&file, vec![b' '; nml.len()]).unwrap();
    std::fs::File::options().write(true).open(&file).unwrap().set_modified(meta.modified().unwrap()).unwrap();
    let last = s.read_library(&file, &[], Some(&next), json!({})).unwrap();
    assert_eq!((last.tracks.len(), last.next.as_deref()), (1, None));
    assert_eq!(last.playlists[0].tracks, [last.tracks[0].path.clone()]);
    // What the read left out, said once, with the last page.
    assert_eq!(last.warnings, ["1 smart playlist(s), whose rules aren't read, left out"]);
    // Saved between pages, it's read again from the start.
    std::fs::write(&file, [nml.as_slice(), b"\n"].concat()).unwrap();
    let e = s.read_library(&file, &[], Some(&next), json!({})).unwrap_err();
    assert_eq!(e.kind(), Some("conflict"), "{e:?}");
    assert_eq!(e.message, format!("{} changed since its first page was read: read it again", file.display()));
    assert_eq!(s.read_library(&file, &[], None, json!({})).unwrap().tracks.len(), 1000);
    let read = json!({ "source": "traktor", "options": {}, "roots": [] });
    let e = s.host.request::<_, LibraryPage>(method::READ_LIBRARY, &read).unwrap_err();
    assert_eq!(e.kind(), Some("invalid_params"), "{e:?}");
    let e = s.read_library(&s.dir.path().join("gone.nml"), &[], None, json!({})).unwrap_err();
    assert_eq!(e.kind(), Some("io"), "{e:?}");
    s.stop();
}

/// A page past what a message holds (a day-long track's grid at a fast tempo) goes in a blob in
/// the scratch folder, holding the page.
#[test]
fn a_big_page_is_a_blob() {
    let s = start();
    let music = s.granted("music");
    let long = Track { duration_secs: 86_400, bpm: 600.0, ..track(1, "", "Long", "/music/long.mp3") };
    let library = Library { tracks: vec![long], ..Library::default() };
    let file = s.built(&library, &|_| TrackAnalysis { beats: grid(600.0, 0, 1), ..TrackAnalysis::default() });
    let scratch = s.granted("scratch");
    let page = s.read_library(&file, std::slice::from_ref(&music), None, json!({ "scratch": scratch })).unwrap();
    assert_eq!(page, LibraryPage { blob: Some("page.json".into()), ..LibraryPage::default() });
    let held: LibraryPage = serde_json::from_slice(&std::fs::read(scratch.join("page.json")).unwrap()).unwrap();
    assert_eq!(held.tracks[0].beats.len(), 864_000);
    s.stop();
}

/// A library a collection could be written from: tracks in a folder granted or not, with a key,
/// a tempo (or none), a length, cues of every kind (past H too) and a grid (or none); playlists in
/// a folder or not, listing tracks twice and tracks it hasn't.
#[hegel::composite]
fn library(tc: &TestCase) -> (Library, FxHashMap<u32, TrackAnalysis>) {
    let n = tc.draw(gs::integers::<u32>().max_value(5));
    let mut tracks = Vec::new();
    let mut analysis = FxHashMap::default();
    for id in 1..=n {
        let folder = tc.draw(gs::sampled_from(vec!["music", "music/sub", "elsewhere"]));
        let t = Track {
            key: tc.draw(gs::sampled_from(vec!["", "Am", "8B", "F#m"])).into(),
            bpm: tc.draw(gs::sampled_from(vec![0.0, 120.0, 128.5])),
            duration_secs: tc.draw(gs::integers::<u32>().max_value(30)),
            rating: tc.draw(gs::integers::<u8>().max_value(5)),
            ..track(id, "Art", &format!("T{id}"), &format!("/{folder}/{id}.mp3"))
        };
        let cues = tc.draw(gs::vecs(hegel::tuples!(
            gs::optional(gs::integers::<u8>().max_value(9)),
            gs::integers::<u32>().max_value(30_000),
            gs::optional(gs::integers::<u32>().max_value(30_000)),
        )));
        let cues = cues.into_iter().map(|(hot_cue, start_ms, end_ms)| Cue {
            hot_cue,
            start_ms,
            end_ms,
            comment: String::new(),
            color: None,
            color_id: 0,
            loop_beats: None,
            active_loop: false,
        });
        let beats = match tc.draw(gs::booleans()) {
            true => grid(t.bpm.max(90.0), tc.draw(gs::integers::<u32>().max_value(2000)), 4),
            false => Vec::new(),
        };
        analysis.insert(id, TrackAnalysis { beats, cues: cues.collect(), ..TrackAnalysis::default() });
        tracks.push(t);
    }
    let mut ids: Vec<u32> = (1..=n).collect();
    ids.push(99);
    let in_folder = tc.draw(gs::booleans());
    let mut playlists = vec![list(1, None, "Sets", vec![], true)];
    for id in 2..tc.draw(gs::integers::<u32>().min_value(2).max_value(4)) {
        let parent = in_folder.then_some(1);
        playlists.push(list(
            id,
            parent,
            &format!("P{id}"),
            tc.draw(gs::vecs(gs::sampled_from(ids.clone())).max_size(4)),
            false,
        ));
    }
    (Library { tracks, playlists, ..Library::default() }, analysis)
}

/// Whatever library a collection is written from, the source answers each of its tracks by its
/// audio file: those in the folder granted with their tags, key, tempo, cues on their pads (the
/// host leaves out those past H) and the grid laid down from the first downbeat to the end of
/// the track; the others by their path alone. Playlists come by their folders, listing the
/// tracks they have by path.
#[hegel::test(test_cases = 40)]
fn a_collection_reads_as_its_library(tc: TestCase) {
    let s = start();
    let music = s.granted("music");
    let (library, analysis) = tc.draw(library());
    let file = s.built(&library, &|t: &Track| analysis[&t.id].clone());
    let page = s.read_library(&file, std::slice::from_ref(&music), None, json!({})).unwrap();
    let at = |t: &Track| s.dir.path().join(t.file_path.trim_start_matches('/'));
    assert_eq!(
        page.tracks.iter().map(|t| t.path.clone()).collect::<Vec<_>>(),
        library.tracks.iter().map(at).collect::<Vec<_>>()
    );
    for (t, read) in library.tracks.iter().zip(&page.tracks) {
        if t.file_path.starts_with("/elsewhere") {
            assert_eq!(read, &SourceTrack { path: at(t), ..SourceTrack::default() });
            continue;
        }
        let a = &analysis[&t.id];
        let downbeat = a.beats.first();
        let tempo = downbeat.map_or(t.bpm, |b| b.bpm);
        assert_eq!(
            (read.title.as_str(), read.rating, read.bpm),
            (t.title.as_str(), t.rating, (tempo > 0.0).then_some(tempo))
        );
        let key = Camelot::parse(&t.key).map(Camelot::code);
        assert_eq!(read.key, key);
        // Loops in green, a loop that doesn't end after it starts as long as nothing; Traktor has no
        // pad past H.
        let cues: Vec<SourceCue> = a
            .cues
            .iter()
            .filter(|c| c.hot_cue.is_none_or(|h| h <= 7))
            .map(|c| SourceCue {
                hot_cue: c.hot_cue,
                start_ms: c.start_ms,
                end_ms: c.end_ms.filter(|end| *end > c.start_ms),
                name: String::new(),
                color: Some(if c.end_ms.is_some() { "green" } else { "blue" }.into()),
            })
            .collect();
        assert_eq!(read.cues, cues);
        let end_ms = t.duration_secs * 1000;
        match downbeat.filter(|_| t.duration_secs > 0) {
            Some(first) => {
                assert!(read.beats.iter().all(|b| b.bpm == first.bpm && b.time_ms < end_ms), "{:?}", read.beats);
                assert_eq!(
                    read.beats.iter().find(|b| b.time_ms == first.time_ms).map(|b| b.beat_number),
                    (first.time_ms < end_ms).then_some(1)
                );
            }
            None => assert!(read.beats.is_empty()),
        }
    }
    let lists: Vec<(String, Vec<PathBuf>)> = library
        .playlists
        .iter()
        .filter(|p| !p.is_folder)
        .map(|p| {
            let name = if p.parent_id.is_some() { format!("Sets/{}", p.name) } else { p.name.clone() };
            (name, p.track_ids.iter().filter_map(|id| library.track(*id)).map(at).collect())
        })
        .collect();
    let read: Vec<(String, Vec<PathBuf>)> = page.playlists.into_iter().map(|p| (p.path, p.tracks)).collect();
    assert_eq!(read, lists);
    // A grid on a track granted without a length to lay it down to is left out, and said.
    let unlaid = library
        .tracks
        .iter()
        .filter(|t| !t.file_path.starts_with("/elsewhere") && t.duration_secs == 0 && !analysis[&t.id].beats.is_empty())
        .count();
    let said = (unlaid > 0)
        .then(|| format!("{unlaid} beat grid(s) left out: the track's length isn't known, or is over a day"));
    assert_eq!(page.warnings, Vec::from_iter(said));
    s.stop();
}

/// A track a page could hold: of some length (or none), with a grid at a tempo up to the fastest
/// one holds, and some cues.
#[hegel::composite]
fn paged(tc: &TestCase) -> Entry {
    let marker = |kind, start_ms, bpm| Marker {
        kind,
        name: String::new(),
        start_ms,
        len_ms: 0.0,
        hot_cue: None,
        color: None,
        bpm,
    };
    let bpm = tc.draw(gs::sampled_from(vec![60.0, 120.0, 655.35]));
    let mut markers = vec![marker(MarkerKind::Grid, 0.0, Some(bpm))];
    let cues = tc.draw(gs::integers::<u32>().max_value(4));
    markers.extend((0..cues).map(|i| marker(MarkerKind::Cue, f64::from(i) * 1000.0, None)));
    Entry {
        location: traktor::Location {
            volume: "USB".into(),
            dir: "/:".into(),
            file: format!("{}.mp3", tc.draw(gs::integers::<u32>())),
        },
        duration_secs: tc.draw(gs::optional(gs::integers::<u32>().min_value(1).max_value(120))).map(f64::from),
        markers,
        ..Entry::default()
    }
}

/// Whatever the tracks and a page's limits (as small as one track, or a beat), pages hold at
/// most their tracks, fewer beats and cues than their budget before their last track, and end
/// early only for the budget; every track comes once, in order, each grid laid down to its
/// track's end, and grids with no length to end at are counted over every page.
#[hegel::test(test_cases = 200)]
fn pages_keep_to_their_limits(tc: TestCase) {
    let entries = tc.draw(gs::vecs(paged()).max_size(12));
    let limits = Limits {
        tracks: tc.draw(gs::integers::<usize>().min_value(1).max_value(5)),
        marks: tc.draw(gs::integers::<usize>().min_value(1).max_value(20_000)),
    };
    let (mut from, mut unlaid, mut paths) = (0, Vec::new(), Vec::new());
    while from < entries.len() {
        let page = page_tracks(&entries[from..], "Macintosh HD", |_| true, limits, &mut unlaid);
        let marks: Vec<usize> = page.iter().map(|t| t.beats.len() + t.cues.len()).collect();
        let (_, before_last) = marks.split_last().expect("a track a page");
        assert!(page.len() <= limits.tracks, "{limits:?}");
        assert!(before_last.iter().sum::<usize>() < limits.marks, "{marks:?} {limits:?}");
        let ended_early = page.len() < limits.tracks && from + page.len() < entries.len();
        assert!(!ended_early || marks.iter().sum::<usize>() >= limits.marks, "{marks:?} {limits:?}");
        for (t, e) in page.iter().zip(&entries[from..]) {
            let fastest = e.markers[0].bpm.unwrap();
            let most = e.duration_secs.map_or(0.0, |s| s * fastest / 60.0) + 1.0;
            assert!(t.beats.len() as f64 <= most, "{} beats in {:?}s at {fastest}", t.beats.len(), e.duration_secs);
        }
        from += page.len();
        paths.extend(page.into_iter().map(|t| t.path));
    }
    let want: Vec<PathBuf> = entries.iter().map(|e| e.location.path("Macintosh HD")).collect();
    assert_eq!(paths, want);
    let no_length = entries.iter().filter(|e| e.duration_secs.is_none()).count();
    let counted: usize = unlaid.iter().map(|(_, n)| n).sum();
    assert_eq!(counted, no_length);
}

/// With the real limits, a page of long tracks at fast tempos ends after the track that brings
/// it to 100,000 beats and cues, and what was left out over every page comes with the last.
#[test]
fn a_page_ends_once_its_beats_and_cues_reach_100_000() {
    let s = start();
    let music = s.granted("music");
    // About 60,000 beats each at 655.35 BPM, and one with no length to lay its grid down to.
    let long = |id| Track {
        duration_secs: 5500,
        bpm: 655.35,
        ..track(id, "", &format!("T{id}"), &format!("/music/{id}.mp3"))
    };
    let library = Library {
        tracks: vec![long(1), long(2), long(3), Track { duration_secs: 0, ..long(4) }],
        ..Library::default()
    };
    let file = s.built(&library, &|_| TrackAnalysis { beats: grid(655.35, 0, 1), ..TrackAnalysis::default() });
    let first = s.read_library(&file, std::slice::from_ref(&music), None, json!({})).unwrap();
    let beats: Vec<usize> = first.tracks.iter().map(|t| t.beats.len()).collect();
    assert_eq!((beats.len(), first.warnings.len()), (2, 0), "{beats:?}");
    assert!(beats[0] < 100_000 && beats.iter().sum::<usize>() >= 100_000, "{beats:?}");
    let last = s.read_library(&file, std::slice::from_ref(&music), first.next.as_deref(), json!({})).unwrap();
    assert_eq!((last.tracks.len(), last.next), (2, None));
    assert_eq!(last.warnings, ["1 beat grid(s) left out: the track's length isn't known, or is over a day"]);
    s.stop();
}
