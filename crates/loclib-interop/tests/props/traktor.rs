//! Properties of `collection.nml`, read back with quick-xml: every track, playlist, cue and grid
//! comes back as the library has it, or the export says what it left out.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use hegel::TestCase;
use hegel::generators as gs;
use loclib_interop::{Content, Error, Export, Source, traktor};
use loclib_model::{Library, Track, TrackColor};
use loclib_progress::{CancelToken, Ctx, Event, Phase};
use rustc_hash::FxHashMap;

use crate::common::{self, El, KEYS};

/// Where the audio is: a Mac volume, a Windows drive, or the startup disk, whose name Traktor
/// takes from this computer.
#[hegel::test_helper]
fn root(tc: &TestCase) -> (PathBuf, Option<&'static str>) {
    let (root, volume) =
        tc.draw(gs::sampled_from(vec![("/Volumes/USB", Some("USB")), ("e:\\", Some("E:")), ("/Users/me/Music", None)]));
    (PathBuf::from(root), volume)
}

/// The NML an export wrote, as a tree.
fn nml(export: &Export) -> El {
    assert_eq!(export.files.len(), 1);
    let file = &export.files[0];
    assert_eq!(file.path, Path::new("out/collection.nml"));
    let Content::Bytes(bytes) = &file.content else { panic!("the NML is written out") };
    assert_eq!(file.bytes, bytes.len() as u64);
    let text = std::str::from_utf8(bytes).expect("UTF-8");
    assert!(text.chars().all(|c| c >= ' ' || c == '\n'), "no raw control characters");
    let doc = common::parse_xml(text);
    assert_eq!((doc.name.as_str(), doc.get("VERSION")), ("NML", "19"));
    doc
}

/// A track's `LOCATION` as the module docs describe it: the volume, `/:name` per folder and a
/// final `/:`, the file; `volume` is `None` for the startup disk.
fn location(src: &Source<'_>, t: &Track, volume: Option<&str>) -> (Option<String>, String, String) {
    let audio = src.audio(t).to_string_lossy().replace('\\', "/");
    let rest = match volume {
        Some("USB") => audio.strip_prefix("/Volumes/USB").unwrap(),
        Some(_) => &audio[2..],
        None => &audio,
    };
    let mut parts: Vec<&str> = rest.split('/').filter(|p| !p.is_empty()).collect();
    let file = parts.pop().unwrap().to_string();
    let dir: String = parts.iter().map(|p| format!("/:{p}")).collect::<String>() + "/:";
    (volume.map(str::to_string), dir, file)
}

/// What playlists name a track by: `VOLUME` + `DIR` + `FILE`, with the startup disk's name as the
/// NML gives it.
fn key(src: &Source<'_>, t: &Track, volume: Option<&str>, startup: &str) -> String {
    let (v, dir, file) = location(src, t, volume);
    format!("{}{dir}{file}", v.as_deref().unwrap_or(startup))
}

/// Traktor's colours by rekordbox's: 1-7 from red round to magenta, aqua and blue both its blue.
const TRAKTOR_COLOURS: [(TrackColor, &str); 8] = [
    (TrackColor::Red, "1"),
    (TrackColor::Orange, "2"),
    (TrackColor::Yellow, "3"),
    (TrackColor::Green, "4"),
    (TrackColor::Aqua, "5"),
    (TrackColor::Blue, "5"),
    (TrackColor::Purple, "6"),
    (TrackColor::Pink, "7"),
];

/// A track's `INFO` attributes, sorted.
fn info_of(t: &Track) -> Vec<(String, String)> {
    let lib = Library { tracks: vec![t.clone()], ..Default::default() };
    let src = Source { library: &lib, root: PathBuf::from("/Volumes/USB"), analysis: None };
    let export = traktor::build(&src, Path::new("out"), &Ctx::silent()).unwrap();
    let mut got = nml(&export).one("COLLECTION").one("ENTRY").one("INFO").attrs.clone();
    got.sort();
    got
}

/// A track's details come out as they do in the collection files Traktor writes: bits per
/// second, a rating of 0-255 (51 a star), whole kilobytes rounded up, dates as `2023/1/15`,
/// a release year alone as its first of January, and red as colour 1.
#[test]
fn info_is_written_in_traktors_units() {
    let t = Track {
        genre: "House".into(),
        key: "8A".into(),
        bitrate: 320,
        duration_secs: 372,
        play_count: 3,
        rating: 5,
        date_added: "2023-01-15".into(),
        year: 2019,
        file_size: 14_880_000,
        color: Some(TrackColor::Red),
        file_path: "/Contents/a.mp3".into(),
        ..Default::default()
    };
    let want = [
        ("BITRATE", "320000"),
        ("COLOR", "1"),
        ("FILESIZE", "14532"),
        ("GENRE", "House"),
        ("IMPORT_DATE", "2023/1/15"),
        ("KEY", "8A"),
        ("PLAYCOUNT", "3"),
        ("PLAYTIME", "372"),
        ("PLAYTIME_FLOAT", "372.000000"),
        ("RANKING", "255"),
        ("RELEASE_DATE", "2019/1/1"),
    ];
    assert_eq!(info_of(&t), want.map(|(k, v)| (k.to_string(), v.to_string())));
    let unrated = Track { rating: 0, release_date: "2021-06-04 10:00:00".into(), bitrate: 0, file_size: 1, ..t };
    let got = info_of(&unrated);
    let get = |k: &str| got.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str());
    assert_eq!(
        (get("RANKING"), get("RELEASE_DATE"), get("BITRATE"), get("FILESIZE")),
        (Some("0"), Some("2021/6/4"), None, Some("1"))
    );
}

/// A Traktor date read back: `2023/1/15` as its numbers, written without leading zeros.
fn traktor_day(d: &str) -> (i32, u32, u32) {
    let parts: Vec<&str> = d.split('/').collect();
    let [y, m, day] = parts[..] else { panic!("{d:?} isn't y/m/d") };
    let n = |p: &str| p.parse::<u32>().ok().filter(|v| v.to_string() == p).unwrap_or_else(|| panic!("{d:?}"));
    (n(y) as i32, n(m), n(day))
}

/// A rekordbox date that's a day on the calendar.
fn calendar_day(s: &str) -> Option<(i32, u32, u32)> {
    let (y, m, d) = common::ymd(s)?;
    (y > 0 && chrono::NaiveDate::from_ymd_opt(y, m, d).is_some()).then_some((y, m, d))
}

/// A track's `INFO` read back in Traktor's units matches the track: text tags when they're set,
/// numbers when they mean something, dates when they're a day Traktor can show.
fn assert_info_reads_back(t: &Track, info: &El) {
    let get = |k: &str| info.attr(k);
    for (name, value) in [
        ("GENRE", &t.genre),
        ("LABEL", &t.label),
        ("COMMENT", &t.comment),
        ("MIX", &t.mix_name),
        ("REMIXER", &t.remixer),
        ("KEY", &t.key),
    ] {
        assert_eq!(get(name).map(String::from), (!value.is_empty()).then(|| common::xml_text(value)), "{name}");
    }
    let bits = get("BITRATE").map(|b| b.parse::<u64>().unwrap());
    assert_eq!(bits.map(|b| (b / 1000, b % 1000)), (t.bitrate > 0).then_some((u64::from(t.bitrate), 0)));
    assert_eq!(get("PLAYCOUNT").map(|c| c.parse::<u16>().unwrap()), Some(t.play_count));
    let secs =
        (get("PLAYTIME").map(|p| p.parse::<u32>().unwrap()), get("PLAYTIME_FLOAT").map(|p| p.parse::<f64>().unwrap()));
    let played = (t.duration_secs > 0).then_some(t.duration_secs);
    assert_eq!(secs, (played, played.map(f64::from)));
    let ranking: u32 = get("RANKING").unwrap().parse().unwrap();
    assert_eq!((ranking / 51, ranking % 51), (u32::from(t.rating.min(5)), 0), "{} stars", t.rating);
    let kb = get("FILESIZE").map(|k| k.parse::<u64>().unwrap());
    assert_eq!(kb.is_some(), t.file_size > 0);
    if let Some(kb) = kb {
        assert!(kb * 1024 >= t.file_size && (kb - 1) * 1024 < t.file_size, "{} bytes as {kb} KB", t.file_size);
    }
    let colour = t.color.map(|c| TRAKTOR_COLOURS.iter().find(|(r, _)| *r == c).unwrap().1);
    assert_eq!(get("COLOR"), colour);

    let added = get("IMPORT_DATE").map(traktor_day);
    if let Some((y, m, d)) = added {
        assert!(y > 0 && (1..=12).contains(&m) && (1..=31).contains(&d), "{added:?}");
        assert_eq!(common::ymd(&t.date_added), added);
    }
    assert!(calendar_day(&t.date_added).is_none() || added.is_some(), "{:?}", t.date_added);
    let released = get("RELEASE_DATE").map(traktor_day);
    match calendar_day(&t.release_date) {
        Some(day) => assert_eq!(released, Some(day)),
        None if common::ymd(&t.release_date).is_none() => {
            assert_eq!(released, (t.year > 0).then_some((i32::from(t.year), 1, 1)));
        }
        // Shaped like a date but not a day: Traktor gets it as it is, or the year.
        None => {
            let year = (t.year > 0).then_some((i32::from(t.year), 1, 1));
            assert!(released == common::ymd(&t.release_date) || released == year, "{:?} {}", t.release_date, t.year);
        }
    }
    let known = [
        "BITRATE",
        "COLOR",
        "COMMENT",
        "FILESIZE",
        "GENRE",
        "IMPORT_DATE",
        "KEY",
        "LABEL",
        "MIX",
        "PLAYCOUNT",
        "PLAYTIME",
        "PLAYTIME_FLOAT",
        "RANKING",
        "RELEASE_DATE",
        "REMIXER",
    ];
    assert!(info.attrs.iter().all(|(k, _)| known.contains(&k.as_str())), "{:?}", info.attrs);
}

fn number(e: &El, name: &str) -> f64 {
    e.get(name).parse().unwrap_or_else(|_| panic!("{name} is a number: {:?}", e.get(name)))
}

/// Every file comes back as one `ENTRY`, in library order, with its title, artist, location,
/// album, details, tempo and key as the module docs say Traktor reads them; track rows sharing a
/// file, in any case, are one entry, counted in a warning.
#[hegel::test(test_cases = 200)]
fn entries_read_back(tc: TestCase) {
    let lib = common::library(&tc, 6);
    let (root, volume) = root(&tc);
    let src = Source { library: &lib, root, analysis: None };
    let export = traktor::build(&src, Path::new("out"), &Ctx::silent()).unwrap();
    let doc = nml(&export);
    let collection = doc.one("COLLECTION");
    let entries: Vec<&El> = collection.all("ENTRY").collect();
    let startup = entries.first().map_or(String::new(), |e| e.one("LOCATION").get("VOLUME").to_string());
    // One entry per file, whatever the case of its path.
    let written = common::first_per_file(&lib, |t| key(&src, t, volume, &startup).to_lowercase());

    assert_eq!(collection.get("ENTRIES"), written.len().to_string());
    assert_eq!((entries.len(), export.tracks), (written.len(), written.len()));
    for (t, e) in written.iter().zip(&entries) {
        assert_eq!(e.get("TITLE"), common::xml_text(&t.title));
        assert_eq!(e.get("ARTIST"), common::xml_text(&t.artist));
        assert_eq!(e.attr("LOCK"), None, "no grid to lock");
        let loc = e.one("LOCATION");
        let (v, dir, file) = location(&src, t, volume);
        assert_eq!((loc.get("DIR"), loc.get("FILE")), (dir.as_str(), file.as_str()));
        assert_eq!(loc.get("VOLUME"), v.as_deref().unwrap_or(&startup));
        assert_eq!(loc.get("VOLUMEID"), loc.get("VOLUME"));
        assert!(!loc.get("VOLUME").is_empty());

        let album: Vec<&El> = e.all("ALBUM").collect();
        assert_eq!(album.len(), usize::from(!t.album.is_empty() || t.track_number > 0));
        if let [album] = album[..] {
            assert_eq!(album.get("TITLE"), common::xml_text(&t.album));
            assert_eq!(album.attr("TRACK"), (t.track_number > 0).then(|| t.track_number.to_string()).as_deref());
        }

        assert_info_reads_back(t, e.one("INFO"));

        let tempo: Vec<&El> = e.all("TEMPO").collect();
        assert_eq!(tempo.len(), usize::from(t.bpm > 0.0));
        if let [tempo] = tempo[..] {
            assert!((number(tempo, "BPM") - t.bpm).abs() < 1e-6);
        }
        let key = KEYS.iter().find(|k| k.0 == t.key).unwrap().1;
        assert_eq!(
            e.all("MUSICAL_KEY").map(|k| k.get("VALUE").parse::<u8>().unwrap()).collect::<Vec<_>>(),
            Vec::from_iter(key)
        );
        assert_eq!(e.all("CUE_V2").count(), 0, "no analysis, no cues");
    }

    let shared = lib.tracks.len() - written.len();
    let want: Vec<String> = (shared > 0)
        .then(|| format!("{shared} track row(s) share a file with another and are one track in Traktor"))
        .into_iter()
        .collect();
    assert_eq!(export.warnings, want);
    assert_eq!((export.beat_grids, export.cues), (0, 0));
}

/// The playlist tree comes back node for node: folders hold their children in display order,
/// each playlist lists the files of its tracks in order (repeats kept, missing tracks left out, a
/// file named by its entry's location in any case), and a playlist's `UUID` follows its path.
#[hegel::test(test_cases = 200)]
fn playlists_read_back(tc: TestCase) {
    let lib = common::library(&tc, 4);
    let (root, volume) = root(&tc);
    let src = Source { library: &lib, root, analysis: None };
    let export = traktor::build(&src, Path::new("out"), &Ctx::silent()).unwrap();
    let doc = nml(&export);
    let startup = doc
        .one("COLLECTION")
        .all("ENTRY")
        .next()
        .map_or(String::new(), |e| e.one("LOCATION").get("VOLUME").to_string());
    // A track is named by its file's entry: the first row's location, whatever the case of its own.
    let first: FxHashMap<String, String> =
        lib.tracks.iter().rev().map(|t| key(&src, t, volume, &startup)).map(|k| (k.to_lowercase(), k)).collect();
    let keys: FxHashMap<u32, String> =
        lib.tracks.iter().map(|t| (t.id, first[&key(&src, t, volume, &startup).to_lowercase()].clone())).collect();

    let top = doc.one("PLAYLISTS").one("NODE");
    assert_eq!((top.get("TYPE"), top.get("NAME")), ("FOLDER", "$ROOT"));
    let mut uuids: Vec<(String, String)> = Vec::new();
    same_tree(&lib, None, top.one("SUBNODES"), &keys, &mut uuids);
    assert_eq!(uuids.len(), export.playlists);
    for (path, uuid) in &uuids {
        assert!(uuid.len() == 32 && uuid.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()), "{uuid}");
        for (other_path, other) in &uuids {
            assert_eq!(path == other_path, uuid == other, "{path:?} and {other_path:?}");
        }
    }
    assert_eq!(export.playlists, lib.playlists.iter().filter(|p| !p.is_folder).count());
    assert_eq!(export.folders, lib.playlists.iter().filter(|p| p.is_folder).count());
}

fn same_tree(
    lib: &Library,
    parent: Option<u32>,
    subnodes: &El,
    keys: &FxHashMap<u32, String>,
    uuids: &mut Vec<(String, String)>,
) {
    let children = lib.children(parent);
    let nodes: Vec<&El> = subnodes.all("NODE").collect();
    assert_eq!(subnodes.get("COUNT"), children.len().to_string());
    assert_eq!(nodes.len(), children.len());
    for (p, node) in children.iter().zip(nodes) {
        assert_eq!(node.get("NAME"), common::xml_text(&p.name));
        if p.is_folder {
            assert_eq!(node.get("TYPE"), "FOLDER");
            same_tree(lib, Some(p.id), node.one("SUBNODES"), keys, uuids);
            continue;
        }
        assert_eq!(node.get("TYPE"), "PLAYLIST");
        let list = node.one("PLAYLIST");
        let want: Vec<&str> = p.track_ids.iter().filter_map(|id| keys.get(id)).map(String::as_str).collect();
        let got: Vec<&str> = list.all("ENTRY").map(|e| e.one("PRIMARYKEY")).map(|k| k.get("KEY")).collect();
        assert_eq!(got, want);
        assert_eq!(list.get("ENTRIES"), want.len().to_string());
        assert!(list.all("ENTRY").all(|e| e.one("PRIMARYKEY").get("TYPE") == "TRACK"));
        uuids.push((lib.playlist_path(p.id), list.get("UUID").to_string()));
    }
}

/// With analysis, each file's grid is one locked marker on its first downbeat (or first beat) at
/// that beat's tempo, then its cues in order: hot cues on their pads, memory cues on none, loops
/// as long as they are; hot cues past the eighth pad, and tempo changes, are counted in warnings.
#[hegel::test(test_cases = 200)]
fn cues_and_grids_read_back(tc: TestCase) {
    let lib = common::library(&tc, 4);
    let analysis = common::analysis(&tc, &lib);
    let load = common::loader(&analysis);
    let src = Source { library: &lib, root: PathBuf::from("/Volumes/USB"), analysis: Some(&load) };
    let export = traktor::build(&src, Path::new("out"), &Ctx::silent()).unwrap();
    let doc = nml(&export);
    let written = common::first_per_file(&lib, |t| key(&src, t, Some("USB"), "").to_lowercase());
    let (mut grids, mut cues, mut varying, mut extra) = (0, 0, 0, 0);
    for (t, e) in written.iter().zip(doc.one("COLLECTION").all("ENTRY")) {
        let (beats, track_cues) = &analysis[&t.id];
        let markers: Vec<&El> = e.all("CUE_V2").collect();
        let mut markers = markers.iter();
        let first = beats.iter().find(|b| b.beat_number == 1).or(beats.first());
        let grid_bpm = first.map(|b| if b.bpm > 0.0 { b.bpm } else { t.bpm }).filter(|&bpm| bpm > 0.0);
        assert_eq!(e.attr("LOCK"), grid_bpm.map(|_| "1"));
        let tempo = grid_bpm.unwrap_or(t.bpm);
        let tempos: Vec<f64> = e.all("TEMPO").map(|x| number(x, "BPM")).collect();
        assert_eq!(tempos.len(), usize::from(tempo > 0.0));
        assert!(tempos.iter().all(|got| (got - tempo).abs() < 1e-6), "{tempos:?} for {tempo}");
        if let (Some(bpm), Some(first)) = (grid_bpm, first) {
            grids += 1;
            varying += usize::from(beats.iter().any(|b| (b.bpm - bpm).abs() > 0.005));
            let m = markers.next().expect("a grid marker");
            assert_eq!((m.get("NAME"), m.get("TYPE"), m.get("HOTCUE")), ("AutoGrid", "4", "-1"));
            assert_eq!((number(m, "START"), number(m, "LEN")), (f64::from(first.time_ms), 0.0));
        }
        for c in track_cues {
            if c.hot_cue.is_some_and(|h| h > 7) {
                extra += 1;
                continue;
            }
            cues += 1;
            let m = markers.next().expect("a marker per cue");
            let name = if c.comment.is_empty() { "n.n.".to_string() } else { common::xml_text(&c.comment) };
            assert_eq!(m.get("NAME"), name);
            assert_eq!(m.get("HOTCUE"), c.hot_cue.map_or(-1, i32::from).to_string());
            assert_eq!(m.get("TYPE"), if c.end_ms.is_some() { "5" } else { "0" });
            assert_eq!(number(m, "START"), f64::from(c.start_ms));
            assert_eq!(number(m, "LEN"), f64::from(c.end_ms.map_or(0, |end| end.saturating_sub(c.start_ms))));
        }
        assert!(markers.next().is_none(), "nothing else is marked");
    }
    assert_eq!((export.beat_grids, export.cues), (grids, cues));
    let mut want = Vec::new();
    if varying > 0 {
        want.push(format!("{varying} track(s) change tempo; their grid keeps the tempo of the first downbeat"));
    }
    if extra > 0 {
        want.push(format!("{extra} hot cue(s) past H left out (Traktor has 8 pads)"));
    }
    let shared = lib.tracks.len() - written.len();
    if shared > 0 {
        want.push(format!("{shared} track row(s) share a file with another and are one track in Traktor"));
    }
    assert_eq!(export.warnings, want);
}

/// An export reports one scan item per track row, in order, between the phase's start and end;
/// cancelled, it stops with [`Error::Cancelled`] before the first track.
#[hegel::test(test_cases = 40)]
fn progress_and_cancelling(tc: TestCase) {
    let lib = common::library(&tc, 4);
    let src = Source { library: &lib, root: PathBuf::from("/Volumes/USB"), analysis: None };
    let events = Mutex::new(Vec::new());
    let record = |e: &Event| events.lock().unwrap().push(e.clone());
    traktor::build(&src, Path::new("out"), &Ctx::new(&record, CancelToken::new())).unwrap();
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
    match traktor::build(&src, Path::new("out"), &ctx) {
        Err(e @ Error::Cancelled(_)) => {
            assert!(n > 0);
            assert_eq!(e.to_string(), "operation cancelled");
        }
        Ok(export) => assert_eq!((n, export.tracks), (0, 0)),
        Err(e) => panic!("{e}"),
    }
}
