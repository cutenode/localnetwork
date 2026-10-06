//! Exports for other DJ software, read back with independent parsers: Traktor's NML with
//! quick-xml.

use crate::common;

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use locallibrary::anlz::{self, CueFile, CuePoint, TrackAnalysis};
use locallibrary::device::Device;
use locallibrary::interop::{self, InteropOptions, Software};
use locallibrary::paths;
use locallibrary::progress::{self, CancelToken, Ctx};
use quick_xml::events::{BytesStart, Event};
use rekordbox_pdb::PdbEditor;

/// The demo export (two tracks with real analysis files) with `Shows/Friday` (tracks 2, 1), a
/// top-level `All` (1, 2), and on track 1 a memory cue, a memory loop, a named hot cue A and a
/// looping hot cue E.
fn drive(dir: &Path) -> PathBuf {
    let root = common::demo_drive(dir, "Main");
    let pdb = root.join(paths::EXPORT_PDB);
    let mut ed = PdbEditor::from_file(&pdb).unwrap();
    let shows = ed.create_playlist("Shows", 0, true).unwrap();
    for (name, parent, tracks) in [("Friday", shows, [2, 1]), ("All", 0, [1, 2])] {
        let p = ed.create_playlist(name, parent, false).unwrap();
        for t in tracks {
            ed.add_to_playlist(p, t, None).unwrap();
        }
    }
    ed.save(&pdb).unwrap();
    let cues = vec![
        CuePoint { start_ms: 25, comment: "Intro".into(), color_id: 6, ..Default::default() },
        CuePoint { start_ms: 30_025, end_ms: Some(33_775), loop_beats: Some((8, 1)), ..Default::default() },
        CuePoint {
            hot_cue: Some(0),
            start_ms: 15_025,
            comment: "Vocal ♪".into(),
            color_code: 0x2a,
            color_rgb: [0xe6, 0x28, 0x28],
            ..Default::default()
        },
        CuePoint {
            hot_cue: Some(4),
            start_ms: 60_025,
            end_ms: Some(60_259),
            loop_beats: Some((1, 2)),
            color_rgb: anlz::DEFAULT_HOT_CUE_RGB,
            ..Default::default()
        },
    ];
    let d = Device::open(&root).unwrap();
    let t = d.library().unwrap().tracks.into_iter().find(|t| t.id == 1).unwrap();
    let dat = d.path(&paths::key(&t.analyze_path));
    let ext = dat.with_extension("EXT");
    for (path, kind) in [(dat, CueFile::Dat), (ext, CueFile::Ext)] {
        let bytes = std::fs::read(&path).unwrap();
        std::fs::write(&path, anlz::write_cue_points(&bytes, kind, &cues).unwrap()).unwrap();
    }
    root
}

fn attrs(e: &BytesStart<'_>) -> HashMap<String, String> {
    e.attributes()
        .map(|a| {
            let a = a.unwrap();
            (String::from_utf8(a.key.as_ref().to_vec()).unwrap(), a.unescape_value().unwrap().into_owned())
        })
        .collect()
}

/// An element's attributes.
type Attrs = HashMap<String, String>;

/// A Traktor collection as quick-xml reads it.
#[derive(Debug, Default)]
struct Nml {
    /// Entries by primary key (`VOLUME` + `DIR` + `FILE`): their element attributes by element
    /// name, and their `CUE_V2`s.
    entries: Vec<(String, HashMap<String, Attrs>, Vec<Attrs>)>,
    collection_count: usize,
    /// Playlist nodes as `path` -> primary keys; folders as `path/`.
    nodes: Vec<(String, Vec<String>)>,
}

fn parse_nml(text: &str) -> Nml {
    let mut r = quick_xml::Reader::from_str(text);
    r.config_mut().trim_text(true);
    let mut nml = Nml::default();
    let mut path: Vec<String> = Vec::new();
    let mut in_entry = false;
    loop {
        match r.read_event().unwrap() {
            Event::Eof => break,
            Event::Start(e) | Event::Empty(e) => {
                let name = String::from_utf8(e.name().as_ref().to_vec()).unwrap();
                let a = attrs(&e);
                match name.as_str() {
                    "COLLECTION" => nml.collection_count = a["ENTRIES"].parse().unwrap(),
                    "ENTRY" if path.is_empty() => {
                        in_entry = true;
                        nml.entries.push((String::new(), HashMap::from([(name, a)]), Vec::new()));
                    }
                    "LOCATION" => {
                        let entry = nml.entries.last_mut().unwrap();
                        entry.0 = format!("{}{}{}", a["VOLUME"], a["DIR"], a["FILE"]);
                        entry.1.insert(name, a);
                    }
                    "CUE_V2" => nml.entries.last_mut().unwrap().2.push(a),
                    "ALBUM" | "INFO" | "TEMPO" | "MUSICAL_KEY" if in_entry => {
                        nml.entries.last_mut().unwrap().1.insert(name, a);
                    }
                    "NODE" => {
                        let full = path.iter().chain([&a["NAME"]]).cloned().collect::<Vec<_>>().join("/");
                        let full = full.trim_start_matches("$ROOT").trim_start_matches('/').to_string();
                        if a["TYPE"] == "FOLDER" {
                            if !full.is_empty() {
                                nml.nodes.push((format!("{full}/"), Vec::new()));
                            }
                        } else {
                            nml.nodes.push((full, Vec::new()));
                        }
                        path.push(a["NAME"].clone());
                    }
                    "PRIMARYKEY" => nml.nodes.last_mut().unwrap().1.push(a["KEY"].clone()),
                    _ => {}
                }
            }
            Event::End(e) => match e.name().as_ref() {
                b"ENTRY" if path.is_empty() => in_entry = false,
                b"NODE" => {
                    path.pop();
                }
                _ => {}
            },
            _ => {}
        }
    }
    nml
}

fn options() -> InteropOptions {
    InteropOptions::default()
}

#[test]
fn traktor_collection_reads_back() {
    let dir = tempfile::tempdir().unwrap();
    let root = drive(dir.path());
    let d = Device::open(&root).unwrap();
    let out = dir.path().join("for traktor");
    let report = interop::apply(&d, Software::Traktor, &out, &options(), &Ctx::silent()).unwrap();
    assert_eq!((report.tracks, report.playlists, report.folders), (2, 2, 1));
    assert_eq!(report.files.len(), 1);
    let nml = parse_nml(&std::fs::read_to_string(out.join("collection.nml")).unwrap());
    assert_eq!((nml.collection_count, nml.entries.len()), (2, 2));

    let lib = d.library().unwrap();
    let mut keys = HashMap::new();
    let mut grids = 0;
    let mut cues = 0;
    for t in &lib.tracks {
        let audio = paths::to_fs(d.root(), &paths::key(&t.file_path));
        let file = audio.file_name().unwrap().to_string_lossy().into_owned();
        let (key, el, markers) = nml.entries.iter().find(|e| e.1["LOCATION"]["FILE"] == file).expect("entry");
        keys.insert(t.id, key.clone());
        assert_eq!(el["ENTRY"]["TITLE"], t.title);
        assert_eq!(el["ENTRY"]["ARTIST"], t.artist);
        let dir_attr = &el["LOCATION"]["DIR"];
        assert!(
            dir_attr.starts_with("/:") && dir_attr.ends_with("/:Contents/:Loopmasters/:UnknownAlbum/:"),
            "{dir_attr}"
        );
        let info = &el["INFO"];
        assert_eq!(info["PLAYTIME"], t.duration_secs.to_string());
        assert_eq!(info["BITRATE"], (t.bitrate * 1000).to_string());
        assert_eq!(info["KEY"], t.key);
        assert_eq!(el["MUSICAL_KEY"]["VALUE"], "17", "F minor");

        let a = TrackAnalysis::load_grid(&d, t);
        let grid = markers.iter().find(|c| c["TYPE"] == "4").expect("a grid marker");
        let downbeat = a.beats.iter().find(|b| b.beat_number == 1).unwrap();
        assert_eq!(grid["START"].parse::<f64>().unwrap(), f64::from(downbeat.time_ms));
        assert_eq!(el["TEMPO"]["BPM"].parse::<f64>().unwrap(), downbeat.bpm);
        assert_eq!(el["ENTRY"]["LOCK"], "1");
        grids += 1;
        let others: Vec<&HashMap<String, String>> = markers.iter().filter(|c| c["TYPE"] != "4").collect();
        assert_eq!(others.len(), a.cues.len());
        for (m, c) in others.iter().zip(&a.cues) {
            assert_eq!(m["START"].parse::<f64>().unwrap(), f64::from(c.start_ms));
            assert_eq!(m["HOTCUE"], c.hot_cue.map_or(-1, i32::from).to_string());
            assert_eq!(m["TYPE"], if c.end_ms.is_some() { "5" } else { "0" });
            let len = c.end_ms.map_or(0, |e| e - c.start_ms);
            assert_eq!(m["LEN"].parse::<f64>().unwrap(), f64::from(len));
        }
        cues += a.cues.len();
    }
    assert_eq!((report.beat_grids, report.cues), (grids, 4));
    assert_eq!(cues, 4);
    let first = &nml.entries.iter().find(|e| e.0 == keys[&1]).unwrap().2;
    let named: Vec<(&str, &str)> = first.iter().map(|c| (c["NAME"].as_str(), c["HOTCUE"].as_str())).collect();
    assert_eq!(named, [("AutoGrid", "-1"), ("Intro", "-1"), ("n.n.", "-1"), ("Vocal ♪", "0"), ("n.n.", "4")]);
    let names: Vec<&str> = nml.nodes.iter().map(|n| n.0.as_str()).collect();
    assert_eq!(names, ["Shows/", "Shows/Friday", "All"]);
    assert_eq!(nml.nodes[1].1, [keys[&2].clone(), keys[&1].clone()]);
    assert_eq!(nml.nodes[2].1, [keys[&1].clone(), keys[&2].clone()]);
}

#[test]
fn exports_plan_first_never_touch_the_drive_and_replace_only_when_asked() {
    let dir = tempfile::tempdir().unwrap();
    let root = drive(dir.path());
    let d = Device::open(&root).unwrap();
    let before = common::drive_files(&root);
    let out = dir.path().join("out");

    let plan = interop::plan(&d, Software::Traktor, &out, &options(), &Ctx::silent()).unwrap();
    assert_eq!(plan.files[0].path, out.join("collection.nml"));
    assert!(!plan.files[0].exists && !out.exists(), "a plan writes nothing");

    let on_drive = interop::apply(&d, Software::Traktor, &root.join("exports"), &options(), &Ctx::silent());
    assert_eq!(on_drive.unwrap_err().code(), "invalid");

    interop::apply(&d, Software::Traktor, &out, &options(), &Ctx::silent()).unwrap();
    let again = interop::apply(&d, Software::Traktor, &out, &options(), &Ctx::silent());
    assert_eq!(again.unwrap_err().code(), "conflict");
    let replace = InteropOptions { replace: true, ..options() };
    assert!(interop::plan(&d, Software::Traktor, &out, &replace, &Ctx::silent()).unwrap().files[0].exists);
    interop::apply(&d, Software::Traktor, &out, &replace, &Ctx::silent()).unwrap();
    assert!(common::changed_files(&before, &common::drive_files(&root)).is_empty(), "the drive is only read");
}

#[test]
fn only_some_playlists_and_elsewhere() {
    let dir = tempfile::tempdir().unwrap();
    let root = drive(dir.path());
    let d = Device::open(&root).unwrap();
    let out = dir.path().join("out");
    // A copy of the drive's Contents/ on another volume.
    #[cfg(not(windows))]
    let (elsewhere, volume, prefix) = ("/Volumes/Music Copy", "Music Copy", "Music Copy/:Contents/:");
    #[cfg(windows)]
    let (elsewhere, volume, prefix) = (r"E:\Music Copy", "E:", "E:/:Music Copy/:Contents/:");
    let opts = InteropOptions {
        only: vec!["Shows".into()],
        location_root: Some(PathBuf::from(elsewhere)),
        skip_analysis: true,
        ..options()
    };
    let report = interop::apply(&d, Software::Traktor, &out, &opts, &Ctx::silent()).unwrap();
    assert_eq!((report.tracks, report.playlists, report.beat_grids, report.cues), (2, 1, 0, 0));
    assert!(report.warnings.iter().any(|w| w.contains("no audio under")), "{:?}", report.warnings);
    let nml = parse_nml(&std::fs::read_to_string(out.join("collection.nml")).unwrap());
    assert_eq!(nml.nodes.iter().map(|n| n.0.as_str()).collect::<Vec<_>>(), ["Shows/", "Shows/Friday"]);
    for (key, el, markers) in &nml.entries {
        assert_eq!(el["LOCATION"]["VOLUME"], volume);
        assert!(key.starts_with(prefix), "{key}");
        assert!(markers.is_empty());
    }
}

// ---------------------------------------------------------------------------------------------
// Serato
// ---------------------------------------------------------------------------------------------

/// The records of a crate (or of an `o*` record): (tag, data).
fn records(mut b: &[u8]) -> Vec<(String, &[u8])> {
    let mut out = Vec::new();
    while b.len() >= 8 {
        let len = u32::from_be_bytes(b[4..8].try_into().unwrap()) as usize;
        out.push((String::from_utf8(b[..4].to_vec()).unwrap(), &b[8..8 + len]));
        b = &b[8 + len..];
    }
    assert!(b.is_empty(), "trailing bytes");
    out
}

fn utf16be(b: &[u8]) -> String {
    String::from_utf16(&b.chunks(2).map(|c| u16::from_be_bytes([c[0], c[1]])).collect::<Vec<_>>()).unwrap()
}

/// A crate's version and its tracks' paths.
fn read_crate(path: &Path) -> (String, Vec<String>) {
    let bytes = std::fs::read(path).unwrap();
    let recs = records(&bytes);
    let tracks = recs
        .iter()
        .filter(|(t, _)| t == "otrk")
        .map(|(_, d)| {
            let inner = records(d);
            assert_eq!(inner[0].0, "ptrk");
            utf16be(inner[0].1)
        })
        .collect();
    (utf16be(recs[0].1), tracks)
}

/// A path as a crate names it: below its volume's root, `/`-separated.
fn volume_relative(p: &Path) -> String {
    let s = p.to_string_lossy().replace('\\', "/");
    let s = match s.as_bytes() {
        [_, b':', ..] => s[2..].to_string(),
        _ => s.strip_prefix("/Volumes/").and_then(|r| r.split_once('/')).map_or(s.clone(), |(_, r)| r.to_string()),
    };
    s.trim_start_matches('/').to_string()
}

#[test]
fn serato_crates_read_back() {
    let dir = tempfile::tempdir().unwrap();
    let root = drive(dir.path());
    let d = Device::open(&root).unwrap();
    let out = dir.path().join("for serato");
    let report = interop::apply(&d, Software::Serato, &out, &options(), &Ctx::silent()).unwrap();
    assert_eq!((report.tracks, report.playlists, report.folders, report.cues, report.copy_bytes), (2, 2, 1, 0, 0));
    let crates = out.join("_Serato_/Subcrates");
    let mut names: Vec<String> =
        std::fs::read_dir(&crates).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    names.sort();
    assert_eq!(names, ["All.crate", "Shows%%Friday.crate", "Shows.crate"]);
    let lib = d.library().unwrap();
    let path = |id: u32| {
        let t = lib.track(id).unwrap();
        volume_relative(&d.root().join(paths::key(&t.file_path)))
    };
    assert_eq!(
        read_crate(&crates.join("Shows%%Friday.crate")),
        ("1.0/Serato ScratchLive Crate".into(), vec![path(2), path(1)])
    );
    assert_eq!(read_crate(&crates.join("All.crate")).1, [path(1), path(2)]);
    assert!(read_crate(&crates.join("Shows.crate")).1.is_empty());
    assert!(path(1).ends_with("Contents/Loopmasters/UnknownAlbum/Demo Track 1.mp3"), "{}", path(1));
    assert!(!path(1).starts_with('/'));
    assert!(report.notes.iter().any(|n| n.contains("--write-tags")), "{:?}", report.notes);
    assert!(!out.join("Contents").exists(), "no copies without --write-tags");
}

/// A crate's file name holds its folders' names too: past 255 bytes the file couldn't be
/// written, so the plan was fine and the export failed with the crates written so far.
#[test]
fn long_nested_crate_names_are_cut_to_fit() {
    let dir = tempfile::tempdir().unwrap();
    let root = common::demo_drive(dir.path(), "Main");
    let pdb = root.join(paths::EXPORT_PDB);
    let mut ed = PdbEditor::from_file(&pdb).unwrap();
    let folder = ed.create_playlist(&"A".repeat(130), 0, true).unwrap();
    let list = ed.create_playlist(&"é".repeat(130), folder, false).unwrap();
    ed.add_to_playlist(list, 1, None).unwrap();
    ed.save(&pdb).unwrap();
    let d = Device::open(&root).unwrap();
    let out = dir.path().join("for serato");
    interop::apply(&d, Software::Serato, &out, &options(), &Ctx::silent()).unwrap();
    let mut names: Vec<String> = std::fs::read_dir(out.join("_Serato_/Subcrates"))
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    let nested = format!("{}%%{}.crate", "A".repeat(130), "é".repeat(34));
    assert_eq!(names, [nested.clone(), format!("{}.crate", "A".repeat(130))]);
    assert_eq!(read_crate(&out.join("_Serato_/Subcrates").join(&nested)).1.len(), 1);
}

/// An MP3 lofty reads: an ID3v2.3 tag with a title, then MPEG-1 layer III frames of silence.
fn mp3(title: &str) -> Vec<u8> {
    let mut frame = b"TIT2".to_vec();
    frame.extend_from_slice(&(1 + title.len() as u32).to_be_bytes());
    frame.extend_from_slice(&[0, 0, 0]);
    frame.extend_from_slice(title.as_bytes());
    let size = frame.len() as u32;
    let mut out = b"ID3\x03\x00\x00".to_vec();
    out.extend((0..4).rev().map(|i| ((size >> (7 * i)) & 0x7f) as u8));
    out.extend(frame);
    for _ in 0..8 {
        let mut f = vec![0u8; 417];
        f[..4].copy_from_slice(&[0xff, 0xfb, 0x90, 0x00]);
        out.extend(f);
    }
    out
}

fn base64_decode(text: &[u8]) -> Vec<u8> {
    let value = |c: u8| match c {
        b'A'..=b'Z' => c - b'A',
        b'a'..=b'z' => c - b'a' + 26,
        b'0'..=b'9' => c - b'0' + 52,
        b'+' => 62,
        _ => 63,
    };
    let (mut bits, mut n, mut out) = (0u32, 0, Vec::new());
    for &c in text.iter().filter(|c| **c != b'\n' && **c != b'=') {
        bits = (bits << 6) | u32::from(value(c));
        n += 6;
        if n >= 8 {
            n -= 8;
            out.push((bits >> n) as u8);
        }
    }
    out
}

/// The entries of a `Serato Markers2` frame: (name, data).
fn markers2(data: &[u8]) -> Vec<(String, Vec<u8>)> {
    assert_eq!(&data[..2], &[1, 1]);
    assert!(data.len() >= 470);
    let end = data[2..].iter().position(|b| *b == 0).map_or(data.len(), |p| p + 2);
    let payload = base64_decode(&data[2..end]);
    assert_eq!(&payload[..2], &[1, 1]);
    let mut b = &payload[2..];
    let mut out = Vec::new();
    while let Some(nul) = b.iter().position(|c| *c == 0).filter(|&p| p > 0) {
        let name = String::from_utf8(b[..nul].to_vec()).unwrap();
        let len = u32::from_be_bytes(b[nul + 1..nul + 5].try_into().unwrap()) as usize;
        out.push((name, b[nul + 5..nul + 5 + len].to_vec()));
        b = &b[nul + 5 + len..];
    }
    out
}

#[test]
fn serato_tags_go_into_copies_only() {
    use lofty::config::ParseOptions;
    use lofty::file::AudioFile;
    use lofty::id3::v2::{Frame, FrameFlags, FrameId, GeneralEncapsulatedObject};

    let dir = tempfile::tempdir().unwrap();
    let root = drive(dir.path());
    let d = Device::open(&root).unwrap();
    let lib = d.library().unwrap();
    for t in &lib.tracks {
        std::fs::write(d.path(&paths::key(&t.file_path)), mp3(&t.title)).unwrap();
    }
    let before = common::drive_files(&root);
    let out = dir.path().join("for serato");
    let opts = InteropOptions { write_tags: true, ..options() };
    let plan = interop::plan(&d, Software::Serato, &out, &opts, &Ctx::silent()).unwrap();
    assert_eq!(plan.copy_bytes, 2 * mp3("Demo Track 1").len() as u64);
    assert!(!out.exists());
    // Tags are Serato's business only, and need the copies under the output folder.
    let traktor = interop::plan(&d, Software::Traktor, &out, &opts, &Ctx::silent());
    assert_eq!(traktor.unwrap_err().code(), "invalid");
    let elsewhere = InteropOptions { location_root: Some(dir.path().join("x")), ..opts.clone() };
    assert_eq!(interop::plan(&d, Software::Serato, &out, &elsewhere, &Ctx::silent()).unwrap_err().code(), "invalid");

    let report = interop::apply(&d, Software::Serato, &out, &opts, &Ctx::silent()).unwrap();
    assert!(common::changed_files(&before, &common::drive_files(&root)).is_empty(), "the drive is only read");
    assert_eq!(report.cues, 3, "hot cue A, the hot loop on E and the memory loop; the memory cue has no place");
    assert!(report.warnings.iter().any(|w| w.contains("1 cue(s) left out")), "{:?}", report.warnings);

    let t1 = lib.track(1).unwrap();
    let copy = out.join(paths::key(&t1.file_path));
    let crate_paths = read_crate(&out.join("_Serato_/Subcrates/All.crate")).1;
    let canonical_copy = out.canonicalize().unwrap().join(paths::key(&t1.file_path));
    assert!(
        crate_paths[0] == volume_relative(&copy) || crate_paths[0] == volume_relative(&canonical_copy),
        "the crates list the copies: {}",
        crate_paths[0]
    );
    let file = lofty::mpeg::MpegFile::read_from(
        &mut std::fs::File::open(&copy).unwrap(),
        ParseOptions::new().read_properties(false),
    )
    .unwrap();
    let tag = file.id3v2().unwrap();
    assert_eq!(tag.get_text(&FrameId::Valid("TIT2".into())), Some("Demo Track 1"), "other frames stay");
    let geob: Vec<GeneralEncapsulatedObject> = tag
        .into_iter()
        .filter_map(|f| match f {
            Frame::Binary(b) if b.id().as_str() == "GEOB" => {
                GeneralEncapsulatedObject::parse(&b.data, FrameFlags::default()).ok()
            }
            _ => None,
        })
        .collect();
    assert_eq!(geob.len(), 1);
    assert_eq!(geob[0].descriptor.as_deref(), Some("Serato Markers2"));
    assert_eq!(geob[0].mime_type.as_deref(), Some("application/octet-stream"));
    let entries = markers2(&geob[0].data);
    let names: Vec<&str> = entries.iter().map(|e| e.0.as_str()).collect();
    assert_eq!(names, ["COLOR", "CUE", "LOOP", "LOOP", "BPMLOCK"]);
    let cue = &entries[1].1;
    assert_eq!((cue[1], u32::from_be_bytes(cue[2..6].try_into().unwrap())), (0, 15_025));
    assert_eq!(&cue[7..10], &[0xcc, 0, 0]);
    assert_eq!(&cue[12..cue.len() - 1], "Vocal ♪".as_bytes());
    let loop_at = |e: &[u8]| {
        (e[1], u32::from_be_bytes(e[2..6].try_into().unwrap()), u32::from_be_bytes(e[6..10].try_into().unwrap()))
    };
    assert_eq!(loop_at(&entries[2].1), (0, 30_025, 33_775), "the memory loop takes the first free slot");
    assert_eq!(loop_at(&entries[3].1), (4, 60_025, 60_259), "the hot loop keeps its pad");
    let t2 = lib.track(2).unwrap();
    assert!(out.join(paths::key(&t2.file_path)).is_file());
}

// ---------------------------------------------------------------------------------------------
// Engine DJ
// ---------------------------------------------------------------------------------------------

/// A playlist's entries, following the `nextEntityId` links from the one no other points at.
fn entity_chain(db: &rusqlite::Connection, list: i64) -> Vec<i64> {
    let mut stmt = db.prepare("SELECT id, trackId, nextEntityId FROM PlaylistEntity WHERE listId = ?1").unwrap();
    let rows: Vec<(i64, i64, i64)> =
        stmt.query_map([list], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).unwrap().map(Result::unwrap).collect();
    let pointed: std::collections::HashSet<i64> = rows.iter().map(|r| r.2).collect();
    let mut cur = rows.iter().find(|r| !pointed.contains(&r.0)).map(|r| r.0);
    let mut out = Vec::new();
    while let Some(id) = cur.filter(|id| *id != 0) {
        let r = rows.iter().find(|r| r.0 == id).unwrap();
        out.push(r.1);
        cur = Some(r.2);
    }
    assert_eq!(out.len(), rows.len(), "one chain through every entry");
    out
}

/// Children of an Engine playlist (0: the top), following `nextListId` from the one no sibling
/// points at.
fn list_chain(db: &rusqlite::Connection, parent: i64) -> Vec<(i64, String)> {
    let mut stmt = db.prepare("SELECT id, title, nextListId FROM Playlist WHERE parentListId = ?1").unwrap();
    let rows: Vec<(i64, String, i64)> =
        stmt.query_map([parent], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).unwrap().map(Result::unwrap).collect();
    let pointed: std::collections::HashSet<i64> = rows.iter().map(|r| r.2).collect();
    let mut cur = rows.iter().find(|r| !pointed.contains(&r.0)).map(|r| r.0);
    let mut out = Vec::new();
    while let Some(id) = cur.filter(|id| *id != 0) {
        let r = rows.iter().find(|r| r.0 == id).unwrap();
        out.push((r.0, r.1.clone()));
        cur = Some(r.2);
    }
    assert_eq!(out.len(), rows.len());
    out
}

#[test]
fn engine_library_reads_back() {
    let dir = tempfile::tempdir().unwrap();
    let root = drive(dir.path());
    // A playlist naming a track twice: Engine keeps it once.
    let pdb = root.join(paths::EXPORT_PDB);
    let mut ed = PdbEditor::from_file(&pdb).unwrap();
    let twice = ed.create_playlist("Twice", 0, false).unwrap();
    for t in [1, 1, 2] {
        ed.add_to_playlist(twice, t, None).unwrap();
    }
    ed.save(&pdb).unwrap();
    let d = Device::open(&root).unwrap();
    let lib = d.library().unwrap();
    let out = dir.path().join("for engine");
    let report = interop::apply(&d, Software::Engine, &out, &options(), &Ctx::silent()).unwrap();
    assert_eq!((report.tracks, report.playlists, report.folders, report.cues), (2, 3, 1, 0));
    assert!(report.warnings.iter().any(|w| w.contains("1 repeated playlist")), "{:?}", report.warnings);

    let path = out.join("Engine Library/Database2/m.db");
    let db = rusqlite::Connection::open_with_flags(&path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    let check: String = db.query_row("PRAGMA integrity_check", [], |r| r.get(0)).unwrap();
    assert_eq!(check, "ok");
    let (uuid, version): (String, (i64, i64, i64)) = db
        .query_row(
            "SELECT uuid, schemaVersionMajor, schemaVersionMinor, schemaVersionPatch FROM Information",
            [],
            |r| Ok((r.get(0)?, (r.get(1)?, r.get(2)?, r.get(3)?))),
        )
        .unwrap();
    assert_eq!(version, (3, 0, 2));
    assert_eq!(uuid.len(), 36);
    let mut names: Vec<String> = db
        .prepare("SELECT name FROM sqlite_master WHERE type IN ('table', 'view') AND name NOT LIKE 'sqlite_%'")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    names.sort();
    assert_eq!(
        names,
        [
            "AlbumArt",
            "ChangeLog",
            "Information",
            "Pack",
            "PerformanceData",
            "Playlist",
            "PlaylistAllChildren",
            "PlaylistAllParent",
            "PlaylistEntity",
            "PlaylistPath",
            "PreparelistEntity",
            "Smartlist",
            "Track"
        ]
    );

    let library_dir = out.canonicalize().unwrap().join("Engine Library");
    let mut ids = HashMap::new();
    for t in &lib.tracks {
        let file = paths::key(&t.file_path).rsplit('/').next().unwrap().to_string();
        let (id, rel, bpm, key, length, kind, analysed, origin, art): (
            i64,
            String,
            f64,
            i64,
            i64,
            String,
            bool,
            i64,
            i64,
        ) = db
            .query_row(
                "SELECT id, path, bpmAnalyzed, key, length, fileType, isAnalyzed, originTrackId, albumArtId
                 FROM Track WHERE filename = ?1",
                [&file],
                |r| {
                    Ok((
                        r.get(0)?,
                        r.get(1)?,
                        r.get(2)?,
                        r.get(3)?,
                        r.get(4)?,
                        r.get(5)?,
                        r.get(6)?,
                        r.get(7)?,
                        r.get(8)?,
                    ))
                },
            )
            .unwrap();
        ids.insert(t.id, id);
        // The path leads from the Engine Library folder to the audio on the drive.
        let resolved = library_dir.join(&rel).canonicalize().unwrap();
        assert_eq!(resolved, d.root().join(paths::key(&t.file_path)).canonicalize().unwrap(), "{rel}");
        assert_eq!((bpm, key, length as u32, kind.as_str()), (t.bpm, 17, t.duration_secs, "mp3"), "F minor");
        assert!(!analysed, "Engine analyses the tracks itself");
        assert_eq!((origin, art), (id, 1));
        let blobs: (Option<Vec<u8>>, Option<Vec<u8>>) = db
            .query_row("SELECT beatData, quickCues FROM PerformanceData WHERE trackId = ?1", [id], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .unwrap();
        assert_eq!(blobs, (None, None));
    }

    let top = list_chain(&db, 0);
    assert_eq!(top.iter().map(|l| l.1.as_str()).collect::<Vec<_>>(), ["Shows", "All", "Twice"]);
    let shows = list_chain(&db, top[0].0);
    assert_eq!(shows.iter().map(|l| l.1.as_str()).collect::<Vec<_>>(), ["Friday"]);
    assert_eq!(entity_chain(&db, shows[0].0), [ids[&2], ids[&1]]);
    assert_eq!(entity_chain(&db, top[1].0), [ids[&1], ids[&2]]);
    assert_eq!(entity_chain(&db, top[2].0), [ids[&1], ids[&2]]);
    assert!(entity_chain(&db, top[0].0).is_empty(), "a folder holds playlists, not tracks");
    let uuids: i64 =
        db.query_row("SELECT count(*) FROM PlaylistEntity WHERE databaseUuid = ?1", [&uuid], |r| r.get(0)).unwrap();
    assert_eq!(uuids, 6);
}

#[test]
fn engine_library_for_the_drive_and_replacing_one() {
    let dir = tempfile::tempdir().unwrap();
    let root = drive(dir.path());
    let d = Device::open(&root).unwrap();
    let out = dir.path().join("for engine");
    let opts = InteropOptions { paths: locallibrary::export::PathStyle::Drive, ..options() };
    interop::apply(&d, Software::Engine, &out, &opts, &Ctx::silent()).unwrap();
    let path = out.join("Engine Library/Database2/m.db");
    let paths_in = |p: &Path| -> Vec<String> {
        let db = rusqlite::Connection::open_with_flags(p, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
        let mut stmt = db.prepare("SELECT path FROM Track ORDER BY id").unwrap();
        stmt.query_map([], |r| r.get(0)).unwrap().map(Result::unwrap).collect()
    };
    assert_eq!(
        paths_in(&path),
        [
            "../Contents/Loopmasters/UnknownAlbum/Demo Track 1.mp3",
            "../Contents/Loopmasters/UnknownAlbum/Demo Track 2.mp3"
        ]
    );

    // An existing library isn't replaced without asking; with --replace, a stale WAL goes too.
    assert_eq!(interop::apply(&d, Software::Engine, &out, &opts, &Ctx::silent()).unwrap_err().code(), "conflict");
    let wal = path.with_file_name("m.db-wal");
    std::fs::write(&wal, b"stale").unwrap();
    let replace = InteropOptions { replace: true, ..opts };
    // Stopped before the new database is written, the old one keeps its WAL.
    let cancel = CancelToken::new();
    let progress = |e: &progress::Event| {
        if let progress::Event::PhaseEnd { .. } = e {
            cancel.cancel();
        }
    };
    let ctx = Ctx::new(&progress, cancel.clone());
    assert_eq!(interop::apply(&d, Software::Engine, &out, &replace, &ctx).unwrap_err().code(), "cancelled");
    assert_eq!(std::fs::read(&wal).unwrap(), b"stale");
    interop::apply(&d, Software::Engine, &out, &replace, &Ctx::silent()).unwrap();
    assert!(!wal.exists());
    assert_eq!(paths_in(&path).len(), 2);
    // Other software locates tracks by their full path.
    let traktor = InteropOptions { paths: locallibrary::export::PathStyle::Drive, ..options() };
    assert_eq!(interop::plan(&d, Software::Traktor, &out, &traktor, &Ctx::silent()).unwrap_err().code(), "invalid");
}
