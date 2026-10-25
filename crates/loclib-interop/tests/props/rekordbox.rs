//! Properties of rekordbox XML, read back with quick-xml: every track, playlist, cue and tempo
//! comes back as the library has it, in rekordbox's units.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use hegel::TestCase;
use hegel::generators as gs;
use loclib_interop::{Content, Error, Export, Source, rekordbox};
use loclib_model::analysis::TrackAnalysis;
use loclib_model::{Library, Track, TrackColor};
use loclib_progress::{CancelToken, Ctx, Event, Phase};

use crate::common::{self, El};

/// rekordbox's colours, as its XML spells them.
const COLOURS: [(TrackColor, &str); 8] = [
    (TrackColor::Pink, "0xFF007F"),
    (TrackColor::Red, "0xFF0000"),
    (TrackColor::Orange, "0xFFA500"),
    (TrackColor::Yellow, "0xFFFF00"),
    (TrackColor::Green, "0x00FF00"),
    (TrackColor::Aqua, "0x25FDE9"),
    (TrackColor::Blue, "0x0000FF"),
    (TrackColor::Purple, "0x660099"),
];

/// The kind rekordbox calls a file by its extension (case aside); what it doesn't know is MP3.
fn kind(path: &str) -> &'static str {
    match path.rsplit('.').next().unwrap_or_default().to_ascii_lowercase().as_str() {
        "m4a" | "mp4" | "aac" => "M4A File",
        "wav" => "WAV File",
        "aif" | "aiff" => "AIFF File",
        "flac" => "FLAC File",
        _ => "MP3 File",
    }
}

/// `s` with its `%XX` escapes decoded.
fn unpercent(s: &str) -> String {
    let (mut bytes, mut rest) = (Vec::new(), s.as_bytes());
    while let Some((&b, tail)) = rest.split_first() {
        match b {
            b'%' => {
                bytes.push(u8::from_str_radix(std::str::from_utf8(&tail[..2]).unwrap(), 16).unwrap());
                rest = &tail[2..];
            }
            _ => {
                bytes.push(b);
                rest = tail;
            }
        }
    }
    String::from_utf8(bytes).unwrap()
}

/// The XML an export wrote to `out.xml`, as a tree.
fn xml(export: &Export) -> El {
    assert_eq!(export.files.len(), 1);
    let file = &export.files[0];
    assert_eq!(file.path, Path::new("out.xml"));
    let Content::Bytes(bytes) = &file.content else { panic!("the XML is written out") };
    assert_eq!(file.bytes, bytes.len() as u64);
    let text = std::str::from_utf8(bytes).expect("UTF-8");
    assert!(text.chars().all(|c| c >= ' ' || c == '\n'), "no raw control characters");
    let doc = common::parse_xml(text);
    assert_eq!((doc.name.as_str(), doc.get("Version")), ("DJ_PLAYLISTS", "1.0.0"));
    assert_eq!(doc.one("PRODUCT").get("Name"), "locallibrary");
    doc
}

/// `NODE`s below `parent` as `lib` has its children, each folder holding its own, each playlist
/// its tracks by id; a count of the playlists met.
fn assert_tree(lib: &Library, parent: Option<u32>, node: &El, playlists: &mut usize) {
    let kids = lib.children(parent);
    let nodes: Vec<&El> = node.all("NODE").collect();
    assert_eq!(nodes.len(), kids.len());
    assert_eq!(node.attr("Count").map(|c| c.parse::<usize>().unwrap()), Some(kids.len()));
    for (p, n) in kids.iter().zip(nodes) {
        assert_eq!(n.get("Name"), common::xml_text(&p.name));
        match p.is_folder {
            true => {
                assert_eq!(n.get("Type"), "0");
                assert_tree(lib, Some(p.id), n, playlists);
            }
            false => {
                *playlists += 1;
                assert_eq!((n.get("Type"), n.get("KeyType")), ("1", "0"));
                assert_eq!(n.get("Entries"), p.track_ids.len().to_string());
                let keys: Vec<u32> = n.all("TRACK").map(|t| t.get("Key").parse().unwrap()).collect();
                assert_eq!(keys, p.track_ids);
            }
        }
    }
}

/// Every track row comes back as a `TRACK`, in order, with its id, text tags, kind, colour,
/// rating (0-255, 51 a star), year (the release date's when it has none), tempo and location (a
/// `file://localhost` URL of its audio); with analysis, a `TEMPO` per tempo change and a
/// `POSITION_MARK` per cue (a loop with its end, a hot cue with its pad and colour). The playlist
/// tree comes back as the library has it, and the export counts what went in.
#[hegel::test(test_cases = 200)]
fn the_library_reads_back(tc: TestCase) {
    let lib = common::library(&tc, 6);
    let analysis = common::analysis(&tc, &lib);
    let load = common::loader(&analysis);
    let load: &dyn Fn(&Track) -> TrackAnalysis = &load;
    let analysed = tc.draw(gs::booleans());
    let root = PathBuf::from(tc.draw(gs::sampled_from(vec!["/Volumes/USB", "e:\\", "/Users/me/Music"])));
    let src = Source { library: &lib, root, analysis: analysed.then_some(load) };
    let export = rekordbox::build(&src, Path::new("out.xml"), &Ctx::silent()).unwrap();
    let doc = xml(&export);

    let collection = doc.one("COLLECTION");
    let tracks: Vec<&El> = collection.all("TRACK").collect();
    assert_eq!(collection.get("Entries"), lib.tracks.len().to_string());
    assert_eq!((tracks.len(), export.tracks), (lib.tracks.len(), lib.tracks.len()));
    let (mut grids, mut cues) = (0, 0);
    for (t, e) in lib.tracks.iter().zip(tracks) {
        assert_eq!(e.get("TrackID"), t.id.to_string());
        for (name, value) in [
            ("Name", &t.title),
            ("Artist", &t.artist),
            ("Composer", &t.composer),
            ("Album", &t.album),
            ("Genre", &t.genre),
            ("Comments", &t.comment),
            ("Remixer", &t.remixer),
            ("Tonality", &t.key),
            ("Label", &t.label),
            ("Mix", &t.mix_name),
        ] {
            assert_eq!(e.get(name), common::xml_text(value), "{name}");
        }
        assert_eq!(e.get("Kind"), kind(&t.file_path), "{}", t.file_path);
        assert_eq!(e.attr("Colour"), t.color.map(|c| COLOURS.iter().find(|(r, _)| *r == c).unwrap().1));
        let rating: u32 = e.get("Rating").parse().unwrap();
        assert_eq!((rating / 51, rating % 51), (u32::from(t.rating.min(5)), 0), "{} stars", t.rating);
        let released = t.release_date.get(..4).and_then(|y| y.parse::<u16>().ok()).unwrap_or(0);
        assert_eq!(e.get("Year"), if t.year > 0 { t.year } else { released }.to_string());
        assert!((e.get("AverageBpm").parse::<f64>().unwrap() - t.bpm).abs() <= 0.0051, "{}", t.bpm);
        let url = e.get("Location");
        let audio = src.audio(t).to_string_lossy().replace('\\', "/");
        let audio = if audio.starts_with('/') { audio } else { format!("/{audio}") };
        assert_eq!(unpercent(url.strip_prefix("file://localhost").unwrap()), audio);
        assert!(url.chars().all(|c| c.is_ascii_alphanumeric() || "/-_.~:%".contains(c)), "{url}");

        let (beats, marks) = match analysed {
            true => analysis[&t.id].clone(),
            false => Default::default(),
        };
        let mut changes: Vec<f64> = Vec::new();
        for b in &beats {
            if changes.last() != Some(&b.bpm) {
                changes.push(b.bpm);
            }
        }
        let tempos: Vec<f64> = e.all("TEMPO").map(|t| t.get("Bpm").parse().unwrap()).collect();
        assert_eq!(tempos.len(), changes.len());
        assert!(tempos.iter().zip(&changes).all(|(got, want)| (got - want).abs() <= 0.0051), "{tempos:?} {changes:?}");
        assert!(e.all("TEMPO").all(|t| (1..=4).contains(&t.get("Battito").parse::<u16>().unwrap())));
        grids += usize::from(!beats.is_empty());
        let positions: Vec<&El> = e.all("POSITION_MARK").collect();
        assert_eq!(positions.len(), marks.len());
        cues += marks.len();
        for (c, p) in marks.iter().zip(positions) {
            assert_eq!(p.get("Name"), common::xml_text(&c.comment));
            assert_eq!(p.get("Type"), if c.end_ms.is_some() { "4" } else { "0" });
            assert_eq!(p.get("Start"), format!("{:.3}", f64::from(c.start_ms) / 1000.0));
            assert_eq!(p.attr("End").map(String::from), c.end_ms.map(|end| format!("{:.3}", f64::from(end) / 1000.0)));
            assert_eq!(p.get("Num"), c.hot_cue.map_or(-1, i32::from).to_string());
            let rgb = c.color.filter(|_| c.hot_cue.is_some()).map(|[r, g, b]| [r, g, b].map(|v| v.to_string()));
            let got = p.attr("Red").map(|r| [r, p.get("Green"), p.get("Blue")].map(String::from));
            assert_eq!(got, rgb);
        }
    }
    assert_eq!((export.beat_grids, export.cues), (grids, cues));

    let mut playlists = 0;
    let root = doc.one("PLAYLISTS").one("NODE");
    assert_eq!((root.get("Type"), root.get("Name")), ("0", "ROOT"));
    assert_tree(&lib, None, root, &mut playlists);
    assert_eq!((export.playlists, playlists), (playlists, lib.playlists.iter().filter(|p| !p.is_folder).count()));
    assert_eq!(export.folders, lib.playlists.iter().filter(|p| p.is_folder).count());
}

/// An export reports one scan item per track row, in order, between the phase's start and end;
/// cancelled, it stops with [`Error::Cancelled`] before the first track.
#[hegel::test(test_cases = 40)]
fn progress_and_cancelling(tc: TestCase) {
    let lib = common::library(&tc, 4);
    let src = Source { library: &lib, root: PathBuf::from("/Volumes/USB"), analysis: None };
    let events = Mutex::new(Vec::new());
    let record = |e: &Event| events.lock().unwrap().push(e.clone());
    rekordbox::build(&src, Path::new("out.xml"), &Ctx::new(&record, CancelToken::new())).unwrap();
    let events = events.into_inner().unwrap();
    let n = lib.tracks.len();
    assert_eq!(events.len(), n + 2);
    assert!(matches!(events[0], Event::PhaseStart { phase: Phase::Scan, items, bytes: 0 } if items == n as u64));
    for (t, e) in lib.tracks.iter().zip(&events[1..]) {
        assert!(matches!(e, Event::Item { phase: Phase::Scan, path, bytes: 0 } if *path == t.file_path), "{e:?}");
    }
    assert!(matches!(events[n + 1], Event::PhaseEnd { phase: Phase::Scan }));

    let cancel = CancelToken::new();
    cancel.cancel();
    let ctx = Ctx { cancel, ..Ctx::silent() };
    match rekordbox::build(&src, Path::new("out.xml"), &ctx) {
        Err(e @ Error::Cancelled(_)) => assert!(n > 0, "{e}"),
        Ok(export) => assert_eq!((n, export.tracks), (0, 0)),
        Err(e) => panic!("{e}"),
    }
}
