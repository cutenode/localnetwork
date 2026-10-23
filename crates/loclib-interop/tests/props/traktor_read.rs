//! Properties of reading `collection.nml` back: what `build` writes reads as the library it was
//! written from, grid markers lay down the grid they say, and nothing makes the reader panic.

use std::path::{MAIN_SEPARATOR, MAIN_SEPARATOR_STR, Path, PathBuf};

use hegel::TestCase;
use hegel::generators as gs;
use loclib_interop::traktor::{self, Entry, List, Marker, MarkerKind};
use loclib_interop::{Content, Error, Source};
use loclib_model::analysis::Beat;
use loclib_model::harmony::Camelot;
use loclib_model::{Library, Track};
use loclib_progress::Ctx;
use rustc_hash::FxHashMap;

use crate::common::{self, KEYS};
use crate::traktor::{calendar_day, location, root};

/// Every file's entry reads back in library order, at its location (a path on this computer
/// again), with its tags, key, tempo, length, rating and year, its grid marker and cues; every
/// playlist reads back with its folders and the files it lists.
#[hegel::test(test_cases = 200)]
fn a_collection_reads_back(tc: TestCase) {
    let lib = common::library(&tc, 5);
    let analysis = common::analysis(&tc, &lib);
    let load = common::loader(&analysis);
    let (root, volume) = root(&tc);
    let src = Source { library: &lib, root, analysis: Some(&load) };
    let export = traktor::build(&src, Path::new("out"), &Ctx::silent()).unwrap();
    let Content::Bytes(nml) = &export.files[0].content else { panic!("the NML is written out") };
    let read = traktor::read(nml).unwrap();
    assert_eq!(read.warnings, Vec::<String>::new());
    let startup = read.entries.first().map_or(String::new(), |e| e.location.volume.clone());
    let written = common::first_per_file(&lib, |t| {
        let (v, dir, file) = location(&src, t, volume);
        format!("{}{dir}{file}", v.as_deref().unwrap_or(&startup)).to_lowercase()
    });
    assert_eq!(read.entries.len(), written.len());
    for (t, e) in written.iter().zip(&read.entries) {
        let (v, dir, file) = location(&src, t, volume);
        assert_eq!((&e.location.dir, &e.location.file), (&dir, &file));
        assert_eq!(e.location.volume, v.unwrap_or_else(|| startup.clone()));
        located_again(e, volume);
        let text = [&e.title, &e.artist, &e.album, &e.genre, &e.label, &e.comment, &e.remixer, &e.key_text];
        let tags = [&t.title, &t.artist, &t.album, &t.genre, &t.label, &t.comment, &t.remixer, &t.key];
        assert_eq!(text.map(String::clone), tags.map(|s| common::xml_text(s)));
        let numbered = KEYS.iter().find(|k| k.0 == t.key).unwrap().1;
        assert_eq!(e.key, numbered.map(|_| Camelot::parse(&t.key).unwrap()), "{}", t.key);
        assert_eq!(e.duration_secs, (t.duration_secs > 0).then(|| f64::from(t.duration_secs)));
        assert_eq!(e.rating, t.rating.min(5));
        assert_year(t, e.year);
        let (beats, cues) = &analysis[&t.id];
        assert_markers(t, e, beats, cues);
    }
    // A track's file is named by its first row's entry, whatever the case of its own path.
    let by_file: FxHashMap<String, String> =
        read.entries.iter().map(|e| (e.location.key().to_lowercase(), e.location.key())).collect();
    let file_of = |t: &Track| {
        let (v, dir, file) = location(&src, t, volume);
        by_file[&format!("{}{dir}{file}", v.as_deref().unwrap_or(&startup)).to_lowercase()].clone()
    };
    let want: Vec<List> = lists(&lib, None, &[])
        .into_iter()
        .map(|(path, ids)| List { path, keys: ids.iter().filter_map(|id| lib.track(*id)).map(file_of).collect() })
        .collect();
    assert_eq!(read.lists, want);
}

/// The path an entry's location reads as is where the library's file was: below `/Volumes/USB`,
/// a drive letter, or `/` on the startup disk.
fn located_again(e: &Entry, volume: Option<&str>) {
    let parts: Vec<&str> =
        e.location.dir.split("/:").filter(|p| !p.is_empty()).chain([e.location.file.as_str()]).collect();
    let below = parts.join(MAIN_SEPARATOR_STR);
    let want = match volume {
        Some("USB") => Path::new("/Volumes/USB").join(parts.join("/")),
        Some(letter) => PathBuf::from(format!("{letter}{MAIN_SEPARATOR}{below}")),
        None => Path::new("/").join(parts.join("/")),
    };
    // The startup disk is the one the collection was written on.
    let startup = volume.is_none().then_some(e.location.volume.as_str());
    assert_eq!(e.location.path(startup), want);
}

/// The year `RELEASE_DATE` reads as: the release date's when it's a day on the calendar, else the
/// track's year.
fn assert_year(t: &Track, year: u16) {
    match calendar_day(&t.release_date) {
        Some((y, _, _)) => assert_eq!(i32::from(year), y),
        None if common::ymd(&t.release_date).is_none() => assert_eq!(year, t.year),
        // Shaped like a date but not a day: written as it is, or as the year.
        None => {
            let shaped = common::ymd(&t.release_date).map(|(y, _, _)| y);
            assert!(shaped == Some(i32::from(year)) || year == t.year, "{:?} {}", t.release_date, t.year);
        }
    }
}

/// The grid marker on the first downbeat (or first beat) and the cues in order, as `build`
/// writes them: point cues and loops on their pads (none past the eighth) or none, named, an
/// unnamed one as Traktor writes it.
fn assert_markers(t: &Track, e: &Entry, beats: &[Beat], cues: &[loclib_model::analysis::Cue]) {
    let first = beats.iter().find(|b| b.beat_number == 1).or(beats.first());
    let grid_bpm = first.map(|b| if b.bpm > 0.0 { b.bpm } else { t.bpm }).filter(|&bpm| bpm > 0.0);
    let tempo = grid_bpm.unwrap_or(t.bpm);
    match e.bpm {
        Some(bpm) => assert!((bpm - tempo).abs() < 1e-6, "{bpm} for {tempo}"),
        None => assert_eq!(tempo, 0.0),
    }
    let mut want = Vec::new();
    if let (Some(_), Some(first)) = (grid_bpm, first) {
        want.push(Marker {
            kind: MarkerKind::Grid,
            name: "AutoGrid".into(),
            start_ms: f64::from(first.time_ms),
            len_ms: 0.0,
            hot_cue: None,
            color: None,
            bpm: None,
        });
    }
    for c in cues.iter().filter(|c| c.hot_cue.is_none_or(|h| h <= 7)) {
        let name = common::xml_text(&c.comment);
        want.push(Marker {
            kind: if c.end_ms.is_some() { MarkerKind::Loop } else { MarkerKind::Cue },
            name: if name == "n.n." { String::new() } else { name },
            start_ms: f64::from(c.start_ms),
            len_ms: f64::from(c.end_ms.map_or(0, |end| end.saturating_sub(c.start_ms))),
            hot_cue: c.hot_cue,
            color: None,
            bpm: None,
        });
    }
    assert_eq!(e.markers, want);
}

/// The playlists below `parent`, depth first in display order: each one's folders and name, and
/// its track ids.
fn lists(lib: &Library, parent: Option<u32>, above: &[String]) -> Vec<(Vec<String>, Vec<u32>)> {
    let mut out = Vec::new();
    for p in lib.children(parent) {
        let path: Vec<String> = above.iter().cloned().chain([common::xml_text(&p.name)]).collect();
        match p.is_folder {
            true => out.extend(lists(lib, Some(p.id), &path)),
            false => out.push((path, p.track_ids.clone())),
        }
    }
    out
}

/// Grid markers anywhere (before the track, past its end, out of order), with or without their
/// own tempos, on tracks with or without a tempo and a length.
#[hegel::composite]
fn gridded(tc: &TestCase) -> Entry {
    let tempo = || {
        hegel::one_of!(
            gs::floats::<f64>().min_value(40.0).max_value(220.0),
            gs::sampled_from(vec![0.0, -1.0, 655.35, 700.0])
        )
    };
    let n = tc.draw(gs::integers::<usize>().max_value(4));
    let markers = (0..n)
        .map(|_| Marker {
            kind: MarkerKind::Grid,
            name: String::new(),
            start_ms: f64::from(tc.draw(gs::integers::<i32>().min_value(-3_000).max_value(200_000))) / 10.0 * 3.0,
            len_ms: 0.0,
            hot_cue: None,
            color: None,
            bpm: tc.draw(gs::optional(tempo())),
        })
        .collect();
    Entry {
        bpm: tc.draw(gs::optional(tempo())),
        duration_secs: tc.draw(gs::optional(gs::floats::<f64>().min_value(-1.0).max_value(400.0))),
        markers,
        ..Entry::default()
    }
}

/// Without a grid marker there's no grid; without a length or with a tempo a grid can't hold it
/// says so; otherwise the grid is every beat at or after the start of the track and before its
/// end: from each marker at its tempo (or the track's) up to the next, the first reaching back to
/// the start, each marker a downbeat, and a beat less than a millisecond after the one before
/// left out.
#[hegel::test(test_cases = 300)]
fn grid_markers_lay_down_the_grid(tc: TestCase) {
    let e = tc.draw(gridded());
    let mut grid: Vec<&Marker> = e.markers.iter().collect();
    grid.sort_by(|a, b| a.start_ms.total_cmp(&b.start_ms));
    let end = e.duration_secs.filter(|s| *s > 0.0).map(|s| s * 1000.0);
    let tempos: Vec<Option<f64>> = grid.iter().map(|m| m.bpm.or(e.bpm).filter(|b| *b > 0.0 && *b <= 655.35)).collect();
    let got = e.beats();
    let Some(end) = end.filter(|_| !grid.is_empty()) else {
        assert_eq!(got, if grid.is_empty() { Ok(Vec::new()) } else { Err("the track's length isn't known") });
        return;
    };
    if tempos.contains(&None) {
        assert_eq!(got, Err("its tempo isn't more than 0 and at most 655.35 BPM"));
        return;
    }
    let mut want: Vec<Beat> = Vec::new();
    for (i, m) in grid.iter().enumerate() {
        let bpm = tempos[i].unwrap();
        let step = 60_000.0 / bpm;
        let from = if i == 0 { 0.0 } else { m.start_ms.max(0.0) };
        let until = grid.get(i + 1).map_or(end, |next| next.start_ms.min(end));
        // Every beat of the marker's grid in the window, by counting from the marker.
        let mut k = ((from - m.start_ms) / step).floor() as i64 - 1;
        loop {
            let at = m.start_ms + k as f64 * step;
            if at >= until {
                break;
            }
            let time_ms = at.round() as u32;
            if at >= from && want.last().is_none_or(|b| b.time_ms < time_ms) {
                want.push(Beat { beat_number: (k.rem_euclid(4) + 1) as u16, bpm, time_ms });
            }
            k += 1;
        }
    }
    assert_eq!(got, Ok(want));
}

/// Text that's anything from a collection to junk reads as a collection or is refused as not
/// being one, never anything else; what reads has a file for every track.
#[hegel::test(test_cases = 300)]
fn anything_reads_or_is_refused(tc: TestCase) {
    let pieces = vec![
        "<NML VERSION=\"19\">",
        "</NML>",
        "<COLLECTION>",
        "</COLLECTION>",
        "<ENTRY TITLE=\"a\">",
        "</ENTRY>",
        "<LOCATION VOLUME=\"USB\" DIR=\"/:a/:\" FILE=\"x.mp3\"/>",
        "<LOCATION FILE=\"\"/>",
        "<INFO PLAYTIME=\"9\" RANKING=\"x\"/>",
        "<TEMPO BPM=\"nan\"/>",
        "<MUSICAL_KEY VALUE=\"-1\"/>",
        "<CUE_V2 TYPE=\"4\" START=\"1e400\" HOTCUE=\"300\">",
        "</CUE_V2>",
        "<GRID BPM=\"128\"/>",
        "<PLAYLISTS>",
        "</PLAYLISTS>",
        "<NODE TYPE=\"PLAYLIST\" NAME=\"p\">",
        "<NODE TYPE=\"FOLDER\" NAME=\"$ROOT\">",
        "</NODE>",
        "<PLAYLIST>",
        "</PLAYLIST>",
        "<ENTRY>",
        "<PRIMARYKEY TYPE=\"TRACK\" KEY=\"k\"/>",
        "<plist>",
        "&amp;",
        "&bogus;",
        "<!-- note -->",
        "<?xml version=\"1.0\"?>",
        "<a b=\"1\" b=\"2\"/>",
        "\u{0}",
        "é",
        "<ENTRY TITLE=\"b\"><LOCATION VOLUME=\"C:\" DIR=\"/:\" FILE=\"y.wav\"/><CUE_V2 TYPE=\"4\"><GRID BPM=\"1\"/></CUE_V2></ENTRY>",
        "<NODE TYPE=\"PLAYLIST\" NAME=\"q\"><PLAYLIST><ENTRY><PRIMARYKEY TYPE=\"TRACK\" KEY=\"C:/:y.wav\"/></ENTRY></PLAYLIST></NODE>",
    ];
    let inside = tc.draw(gs::vecs(gs::sampled_from(pieces)).max_size(24)).concat();
    // Often in a collection's own elements, so some of it reads.
    let text = match tc.draw(gs::booleans()) {
        true => format!("<NML><COLLECTION>{inside}</COLLECTION><PLAYLISTS>{inside}</PLAYLISTS></NML>"),
        false => inside,
    };
    match traktor::read(text.as_bytes()) {
        Ok(c) => assert!(c.entries.iter().all(|e| !e.location.file.is_empty()), "{c:?}"),
        Err(Error::Invalid(why)) => assert!(why.starts_with("not a Traktor collection: "), "{why}"),
        Err(e) => panic!("{e}"),
    }
}
