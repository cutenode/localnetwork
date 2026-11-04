use std::path::Path;

use hegel::TestCase;
use hegel::generators as gs;
use loclib_model::analysis::Beat;
use loclib_model::model::Library;
use loclib_plugin::method;
use serde_json::{Value, json};

use super::*;
use crate::tests::{kind, start, write};
use crate::xml_import::tests::collection_with;

/// The track a cursor's page starts at (`i:len:mtime_ns`).
fn at(cursor: &Option<String>) -> Option<usize> {
    cursor.as_deref().map(|c| c.split(':').next().unwrap().parse().unwrap())
}

/// Each page of the source over `file` (with `scratch` as each request's folder, the blobs read
/// into their pages), from the first until one has no `next`, and the cursors asked for.
fn pages(file: &str, scratch: Option<&Path>) -> (Vec<LibraryPage>, Vec<Option<String>>) {
    let s = start(Library::default());
    let (mut pages, mut cursors) = (Vec::new(), vec![None]);
    loop {
        let cursor = cursors.last().cloned().flatten();
        let req = json!({ "source": "rekordbox-xml", "options": { "file": file }, "roots": ["/"], "cursor": cursor, "context": { "scratch": scratch } });
        let mut page: LibraryPage = s.host.request(method::READ_LIBRARY, &req).unwrap();
        if let (Some(blob), Some(scratch)) = (page.blob.take(), scratch) {
            let text = std::fs::read_to_string(scratch.join(blob)).unwrap();
            page = LibraryPage { next: page.next, ..serde_json::from_str::<LibraryPage>(&text).unwrap() };
        }
        let next = page.next.clone();
        pages.push(page);
        // The host refuses a cursor it was given before; this one starts with an index, so it
        // must grow.
        match next {
            Some(next) if at(&Some(next.clone())) <= at(&cursor) => panic!("the page after {cursor:?} is {next:?}"),
            Some(next) => cursors.push(Some(next)),
            None => break,
        }
    }
    assert!(s.stop().is_empty(), "it reads nothing through the host");
    (pages, cursors)
}

/// The library/read request's answer for `options` and `cursor`.
fn asked(options: Value, cursor: Option<&str>) -> Result<LibraryPage, RpcError> {
    let s = start(Library::default());
    let req = json!({ "source": "rekordbox-xml", "options": options, "roots": ["/"], "cursor": cursor });
    let page = s.host.request(method::READ_LIBRARY, &req);
    s.stop();
    page
}

/// A collection reads as its tracks, each its audio file with its tags, key, tempo, cues (a
/// loop's end, a cue's colour as `#rrggbb`) and the grid its tempos make to its end, and its
/// playlists by folder path, their tracks by file. A track without a file, and playlist entries
/// that aren't the collection's tracks with one, are left out, counted with what the reader left
/// out.
#[test]
fn a_collection_is_read_with_tags_cues_grids_and_playlists() {
    let dir = tempfile::tempdir().unwrap();
    let glue = (
        r#"TrackID="1" Name="Glue" Artist="Bicep" Album="Bicep" Genre="Electronica" Label="Ninja Tune" Remixer="R"
           Composer="C" Comments="peak" Year="2017" Rating="255" Tonality="Fm" AverageBpm="120.00" TotalTime="2"
           Location="file://localhost/Users/me/Music/Bicep%20-%20Glue.mp3""#
            .to_string(),
        r#"<TEMPO Inizio="0.000" Bpm="120.00" Metro="4/4" Battito="2"/>
           <POSITION_MARK Name="Drop" Type="4" Start="0.500" End="1.500" Num="0" Red="255" Green="0" Blue="0"/>
           <POSITION_MARK Name="" Type="0" Start="1.000" Num="-1"/>
           <TEMPO Inizio="0" Bpm="0"/>
"#
        .to_string(),
    );
    let opus = (r#"TrackID="2" Name="Opus" Location="file://localhost/C:/Music/opus.mp3""#.to_string(), String::new());
    let nowhere = (r#"TrackID="3" Name="Stream" Location="https://example.com/x""#.to_string(), String::new());
    let tree = r#"<NODE Type="0" Name="ROOT"><NODE Type="0" Name="Sets">
        <NODE Type="1" Name="Friday" KeyType="0"><TRACK Key="1"/><TRACK Key="3"/><TRACK Key="99"/></NODE></NODE>
        <NODE Type="1" Name="By file" KeyType="1"><TRACK Key="file://localhost/C:/Music/opus.mp3"/><TRACK Key="file://localhost/gone/x.mp3"/></NODE>
        </NODE>"#;
    let file = write(dir.path(), "rekordbox.xml", &collection_with(&[glue, opus, nowhere], tree));
    let (pages, _) = pages(&file, None);
    let [page] = &pages[..] else { panic!("{pages:?}") };
    let beat = |beat_number, time_ms| Beat { beat_number, bpm: 120.0, time_ms };
    let glue = SourceTrack {
        path: "/Users/me/Music/Bicep - Glue.mp3".into(),
        title: "Glue".into(),
        artist: "Bicep".into(),
        album: "Bicep".into(),
        genre: "Electronica".into(),
        label: "Ninja Tune".into(),
        remixer: "R".into(),
        composer: "C".into(),
        comment: "peak".into(),
        year: 2017,
        rating: 5,
        key: Some("Fm".into()),
        bpm: Some(120.0),
        cues: vec![
            SourceCue {
                hot_cue: Some(0),
                start_ms: 500,
                end_ms: Some(1500),
                name: "Drop".into(),
                color: Some("#ff0000".into()),
            },
            SourceCue { start_ms: 1000, ..SourceCue::default() },
        ],
        beats: vec![beat(2, 0), beat(3, 500), beat(4, 1000), beat(1, 1500)],
        ..SourceTrack::default()
    };
    let opus = SourceTrack { path: "C:/Music/opus.mp3".into(), title: "Opus".into(), ..SourceTrack::default() };
    assert_eq!(page.tracks, [glue, opus]);
    let playlist = |path: &str, tracks: &[&str]| SourcePlaylist {
        path: path.into(),
        tracks: tracks.iter().map(PathBuf::from).collect(),
    };
    assert_eq!(
        page.playlists,
        [
            playlist("Sets/Friday", &["/Users/me/Music/Bicep - Glue.mp3"]),
            playlist("By file", &["C:/Music/opus.mp3", "/gone/x.mp3"])
        ]
    );
    assert_eq!(
        page.warnings,
        [
            "1 beat grid tempo(s) (TEMPO) left out: a time, tempo or beat out of range",
            "1 track(s) left out: no audio file (Location)",
            "2 playlist entr(ies) left out: not a collection track with an audio file",
        ]
    );
}

/// A collection comes a page at a time, each at most PER_PAGE tracks, or fewer once their beats
/// and cues come to PAGE_ITEMS, its playlists with the last; a page bigger than a quarter of a
/// message is a file in the request's scratch folder. A grid past MAX_BEATS is cut, and one with
/// no length to end at left out: what's left out over all the pages is said once, with the last.
#[test]
fn a_big_collection_comes_in_pages() {
    let dir = tempfile::tempdir().unwrap();
    let fastest = r#"<TEMPO Inizio="0" Bpm="655.35"/>"#.to_string();
    let mut tracks =
        vec![(r#"TrackID="0" Name="Long" TotalTime="36000" Location="/m/long.mp3""#.to_string(), fastest.clone())];
    tracks.push((r#"TrackID="1" Name="No length" Location="/m/x.mp3""#.into(), fastest));
    tracks
        .extend((2..1_200).map(|id| (format!(r#"TrackID="{id}" Name="t{id}" Location="/m/{id}.mp3""#), String::new())));
    tracks.push((r#"TrackID="9" Name="Bad" Inizio="x""#.into(), r#"<TEMPO Inizio="x"/>"#.into()));
    let lists = r#"<NODE Type="1" Name="All" KeyType="0"><TRACK Key="0"/></NODE>"#;
    let file = write(dir.path(), "rekordbox.xml", &collection_with(&tracks, lists));
    let scratch = tempfile::tempdir().unwrap();
    let (paged, cursors) = pages(&file, Some(scratch.path()));
    assert_eq!(cursors.iter().map(at).collect::<Vec<_>>(), [None, Some(1), Some(501), Some(1001)]);
    let sizes: Vec<usize> = paged.iter().map(|p| p.tracks.len()).collect();
    assert_eq!(sizes, [1, 500, 500, 199]);
    assert_eq!(paged[0].tracks[0].beats.len(), MAX_BEATS);
    assert!(paged[1].tracks[0].beats.is_empty());
    let said: Vec<&[String]> = paged.iter().map(|p| p.warnings.as_slice()).collect();
    let last = [
        "1 beat grid tempo(s) (TEMPO) left out: a time, tempo or beat out of range".to_string(),
        "1 track(s) left out: no audio file (Location)".into(),
        "1 beat grid(s) left out: no length (TotalTime) to end at".into(),
        format!("1 beat grid(s) cut at {MAX_BEATS} beats"),
    ];
    assert_eq!(said, [&[][..], &[][..], &[][..], &last[..]]);
    let lists: Vec<usize> = paged.iter().map(|p| p.playlists.len()).collect();
    assert_eq!(lists, [0, 0, 0, 1]);
    let (inline, _) = pages(&file, None);
    assert_eq!(inline[0].tracks, paged[0].tracks, "the same in a blob and not");
    assert!(scratch.path().join("page.json").is_file(), "the first page, with its grid, is a blob");
}

/// Playlists that name a track with a long path many times name it only up to PLAYLIST_BYTES of
/// files: the entries past that are left out, counted.
#[test]
fn playlists_name_files_up_to_a_bound() {
    let dir = tempfile::tempdir().unwrap();
    let long = format!("/m/{}.mp3", "x".repeat((1 << 20) - 7));
    let track = (format!(r#"TrackID="1" Name="A" Location="{long}""#), String::new());
    let list = format!("<NODE Type=\"1\" Name=\"All\" KeyType=\"0\">{}</NODE>", "<TRACK Key=\"1\"/>".repeat(100));
    let file = write(dir.path(), "rekordbox.xml", &collection_with(&[track], &list));
    let (pages, _) = pages(&file, Some(dir.path()));
    let kept = pages[0].playlists[0].tracks.len();
    assert_eq!(
        (kept, pages[0].warnings.as_slice()),
        (64, &["36 playlist entr(ies) left out: the playlists name more than 64 MiB of files".to_string()][..])
    );
}

/// A file saved between pages is refused (`conflict`): its tracks may have moved, so the pages
/// after the first could skip or repeat some. Kept as it was, the file is read once: the pages
/// after the first come from what the first read, even with its bytes since changed.
#[test]
fn a_file_saved_between_pages_is_refused_and_the_rest_read_once() {
    let dir = tempfile::tempdir().unwrap();
    let tracks: Vec<(String, String)> =
        (0..600).map(|id| (format!(r#"TrackID="{id}" Name="t{id}" Location="/m/{id}.mp3""#), String::new())).collect();
    let file = write(dir.path(), "rekordbox.xml", &collection_with(&tracks, ""));
    let s = start(Library::default());
    let read = |cursor: Option<&str>| {
        let req = json!({ "source": "rekordbox-xml", "options": { "file": file }, "roots": ["/"], "cursor": cursor });
        s.host.request::<_, LibraryPage>(method::READ_LIBRARY, &req)
    };
    let first = read(None).unwrap();
    let next = first.next.clone().unwrap();
    assert_eq!(at(&Some(next.clone())), Some(500));

    // The same length and time, other bytes: it isn't read again.
    let mtime = std::fs::metadata(&file).unwrap().modified().unwrap();
    let len = std::fs::metadata(&file).unwrap().len() as usize;
    std::fs::write(&file, " ".repeat(len)).unwrap();
    std::fs::File::options().write(true).open(&file).unwrap().set_modified(mtime).unwrap();
    let second = read(Some(&next)).unwrap();
    assert_eq!((second.tracks.len(), second.next.is_none()), (100, true));

    // Saved again after its first page, with fewer tracks: refused.
    std::fs::write(&file, collection_with(&tracks, "")).unwrap();
    let next = read(None).unwrap().next.unwrap();
    std::fs::write(&file, collection_with(&tracks[..10], "")).unwrap();
    let e = read(Some(&next)).unwrap_err();
    assert_eq!(kind(&e), ("conflict", format!("{file} changed while it was read: build again").as_str()));
    s.stop();
}

/// A cursor this source didn't give, or options without a file, are refused; a file that isn't
/// there fails naming it.
#[test]
fn what_cant_be_read_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let file = write(dir.path(), "rekordbox.xml", &collection_with(&[], ""));
    let meta = std::fs::metadata(&file).unwrap();
    let mtime_ns = meta.modified().unwrap().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    let past_the_end = format!("1:{}:{mtime_ns}", meta.len());
    for cursor in ["x", "1", "-1", "1:2", "1:2:3:4", past_the_end.as_str()] {
        let e = asked(json!({ "file": file }), Some(cursor)).unwrap_err();
        assert_eq!(e.code, loclib_plugin::code::INVALID_PARAMS, "{cursor}: {e:?}");
    }
    assert_eq!(asked(json!({}), None).unwrap_err().code, loclib_plugin::code::INVALID_PARAMS);
    let gone = dir.path().join("gone.xml").to_string_lossy().into_owned();
    let e = asked(json!({ "file": gone }), None).unwrap_err();
    assert!(kind(&e).0 == "io" && kind(&e).1.contains("gone.xml"), "{e:?}");
}

/// How many tracks a collection has: a few, or about a page's worth (PER_PAGE).
fn sizes() -> impl hegel::PrintableGenerator<usize> {
    hegel::one_of!(gs::integers::<usize>().max_value(20), gs::integers::<usize>().min_value(480).max_value(1_020))
}

/// For any collection, paging through it from the first page to the last answers each track
/// with a file once, in order, each grid in order and before the track's end, and the
/// playlists once; every page holds at most PER_PAGE tracks (and [`pages`] checks the cursors only go up).
#[hegel::test(test_cases = 50)]
fn any_collection_pages_through_each_track_once(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let mut tracks = Vec::new();
    // Each track with a file: its file and its length in ms (0 for none).
    let mut files = Vec::new();
    for id in 0..tc.draw(sizes()) {
        let length = tc.draw(gs::optional(gs::integers::<u32>().max_value(3_000)));
        let mut attrs = format!("TrackID=\"{id}\" Name=\"t{id}\"");
        attrs.extend(length.map(|s| format!(" TotalTime=\"{s}\"")));
        if tc.draw(gs::booleans()) {
            attrs += &format!(" Location=\"/m/{id}.mp3\"");
            files.push((PathBuf::from(format!("/m/{id}.mp3")), length.unwrap_or(0) * 1000));
        }
        let start = f64::from(tc.draw(gs::integers::<i32>().min_value(-5_000).max_value(5_000))) / 1000.0;
        let bpm = f64::from(tc.draw(gs::integers::<u16>().min_value(1).max_value(65_535))) / 100.0;
        let tempo = format!("<TEMPO Inizio=\"{start}\" Bpm=\"{bpm}\"/>");
        tracks.push((attrs, if tc.draw(gs::booleans()) { tempo } else { String::new() }));
    }
    let file = write(dir.path(), "rekordbox.xml", &collection_with(&tracks, r#"<NODE Type="1" Name="A"/>"#));
    let (paged, _) = pages(&file, Some(dir.path()));
    let read: Vec<&SourceTrack> = paged.iter().flat_map(|p| &p.tracks).collect();
    assert_eq!(
        read.iter().map(|t| t.path.clone()).collect::<Vec<_>>(),
        files.iter().map(|(f, _)| f.clone()).collect::<Vec<_>>()
    );
    assert!(paged.iter().all(|p| p.tracks.len() <= PER_PAGE));
    assert_eq!(paged.iter().map(|p| p.playlists.len()).sum::<usize>(), 1);
    for (t, (_, end)) in read.into_iter().zip(&files) {
        assert!(t.beats.windows(2).all(|w| w[0].time_ms < w[1].time_ms));
        assert!(t.beats.iter().all(|b| b.time_ms < *end), "{:?}", t.beats.last());
    }
}

/// What the source can't read is refused, saying why: a cursor that isn't three numbers between
/// colons (`i:len:mtime_ns`), whichever part of it isn't a number or is missing, or one past the
/// collection's end; options without a file; a file that isn't there; and a cursor of the file
/// as it was before it was saved (`conflict`).
#[hegel::test(test_cases = 10)]
fn prop_what_cant_be_read_is_refused(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let file = write(dir.path(), "rekordbox.xml", &collection_with(&[], ""));
    let meta = std::fs::metadata(&file).unwrap();
    let mtime_ns = meta.modified().unwrap().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    let invalid =
        |said: String| (loclib_plugin::code::INVALID_PARAMS, Some("invalid_params"), format!("invalid params: {said}"));
    // Each part missing or not a number in turn, and one too many.
    let cursors = ["", "x", "-1", "7", "7:", "7:x", "7:7", "7:7:", "7:7:x", "x:7:7", "7:7:7:7"];
    let mut cases: Vec<_> = cursors
        .into_iter()
        .map(|cursor| {
            let said = format!("{cursor:?} isn't a cursor of {file}'s collection");
            (json!({ "file": file }), Some(cursor.to_string()), invalid(said))
        })
        .collect();
    let from = tc.draw(gs::integers::<u32>().min_value(1));
    let past_the_end = format!("{from}:{}:{mtime_ns}", meta.len());
    cases.push((
        json!({ "file": file }),
        Some(past_the_end),
        invalid(format!("{file}'s collection has no track {from}")),
    ));
    cases.push((json!({}), None, invalid("missing field `file`".into())));
    // The system's words for it after the file's name.
    let gone = dir.path().join("gone.xml");
    cases.push((
        json!({ "file": gone }),
        None,
        (loclib_plugin::code::FAILED, Some("io"), format!("{}: ", gone.display())),
    ));
    let saved = format!("0:{}:{mtime_ns}", meta.len() + 1);
    let said = format!("{file} changed while it was read: build again");
    cases.push((json!({ "file": file }), Some(saved), (loclib_plugin::code::FAILED, Some("conflict"), said)));
    for (options, cursor, (code, kind, said)) in cases {
        let e = asked(options, cursor.as_deref()).unwrap_err();
        assert_eq!((e.code, e.kind()), (code, kind), "{e:?}");
        assert!(e.message.starts_with(&said), "{e:?}, not {said:?}");
    }
}

/// A track's hot cue reads with its colour as `#rrggbb` (none without one), and a playlist names
/// the files of the collection's tracks it holds, in order: an entry for a track the collection
/// hasn't, or one without a file, is left out, counted with the last page.
#[hegel::test(test_cases = 10)]
fn prop_cues_and_playlists_read_as_the_file_says(tc: TestCase) {
    cues_and_playlists(&tc, false);
}

/// Playlists naming a file a mebibyte long more times than PLAYLIST_BYTES of files hold leave
/// the entries past that out, counted with the last page. Once: the page is 64 MiB.
#[hegel::test(test_cases = 1)]
fn prop_playlists_past_their_bound_leave_the_rest_out(tc: TestCase) {
    cues_and_playlists(&tc, true);
}

/// [`prop_cues_and_playlists_read_as_the_file_says`], with track 1's file `long` or not.
#[hegel::test_helper]
fn cues_and_playlists(tc: &TestCase, long: bool) {
    let dir = tempfile::tempdir().unwrap();
    let path = if long { format!("/m/{}.mp3", "x".repeat((1 << 20) - 7)) } else { "/m/1.mp3".into() };
    let rgb = tc.draw(gs::optional(gs::arrays::<_, _, 3>(gs::integers::<u8>())));
    let colour = rgb.map_or(String::new(), |[r, g, b]| format!(r#" Red="{r}" Green="{g}" Blue="{b}""#));
    let mark = format!(r#"<POSITION_MARK Name="" Type="0" Start="1.000" Num="0"{colour}/>"#);
    let tracks = [
        (format!(r#"TrackID="1" Name="A" Location="{path}""#), mark),
        (r#"TrackID="2" Name="B""#.to_string(), String::new()),
    ];
    let mut keys = tc.draw(gs::vecs(gs::sampled_from(vec![1, 2, 99])).max_size(6));
    // A track the collection hasn't, always.
    keys.push(99);
    if long {
        // More than PLAYLIST_BYTES of it.
        keys.extend([1; 66]);
    }
    let entries: String = keys.iter().map(|k| format!(r#"<TRACK Key="{k}"/>"#)).collect();
    let list = format!(r#"<NODE Type="1" Name="Set" KeyType="0">{entries}</NODE>"#);
    let file = write(dir.path(), "rekordbox.xml", &collection_with(&tracks, &list));
    let (pages, _) = pages(&file, Some(dir.path()));
    let [page] = &pages[..] else { panic!("{} pages", pages.len()) };

    let color = rgb.map(|[r, g, b]| format!("#{r:02x}{g:02x}{b:02x}"));
    let cue = SourceCue { hot_cue: Some(0), start_ms: 1000, color, ..SourceCue::default() };
    assert_eq!(page.tracks.iter().map(|t| &t.cues).collect::<Vec<_>>(), [&vec![cue]]);
    let ones = keys.iter().filter(|k| **k == 1).count();
    let kept = ones.min(PLAYLIST_BYTES / path.len());
    let [playlist] = &page.playlists[..] else { panic!("{:?}", page.playlists.len()) };
    assert_eq!((playlist.path.as_str(), playlist.tracks.len()), ("Set", kept));
    assert!(playlist.tracks.iter().all(|t| *t == Path::new(&path)));
    let mut warnings = vec!["1 track(s) left out: no audio file (Location)".to_string()];
    let (absent, past) = (keys.len() - ones, ones - kept);
    if absent > 0 {
        warnings.push(format!("{absent} playlist entr(ies) left out: not a collection track with an audio file"));
    }
    if past > 0 {
        warnings.push(format!("{past} playlist entr(ies) left out: the playlists name more than 64 MiB of files"));
    }
    assert_eq!(page.warnings, warnings);
}

/// However many pages a collection takes, the file is read once for them all: the pages after the
/// first come from what the first read, even with the file's bytes changed since (its length and
/// time kept), every track once.
#[hegel::test(test_cases = 10)]
fn prop_a_collection_is_read_once_for_all_its_pages(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let n = tc.draw(gs::integers::<usize>().min_value(PER_PAGE + 1).max_value(3 * PER_PAGE));
    let tracks: Vec<(String, String)> =
        (0..n).map(|id| (format!(r#"TrackID="{id}" Name="t{id}" Location="/m/{id}.mp3""#), String::new())).collect();
    let file = write(dir.path(), "rekordbox.xml", &collection_with(&tracks, ""));
    let s = start(Library::default());
    let read = |cursor: Option<&str>| {
        let req = json!({ "source": "rekordbox-xml", "options": { "file": file }, "roots": ["/"], "cursor": cursor });
        s.host.request::<_, LibraryPage>(method::READ_LIBRARY, &req).unwrap()
    };
    let mut page = read(None);
    let mut count = page.tracks.len();
    // The same length and time, other bytes.
    let meta = std::fs::metadata(&file).unwrap();
    std::fs::write(&file, " ".repeat(meta.len() as usize)).unwrap();
    std::fs::File::options().write(true).open(&file).unwrap().set_modified(meta.modified().unwrap()).unwrap();
    while let Some(next) = page.next.clone() {
        page = read(Some(&next));
        count += page.tracks.len();
    }
    s.stop();
    assert_eq!(count, n);
}

/// What stops a read of the first page fails it as what it is: a file that isn't rekordbox XML
/// is refused (`invalid`), naming it; cancelled while it reads the file, it answers `cancelled`;
/// and a page too big for a message, without the scratch folder it goes in, fails as `io`,
/// naming the file it couldn't write.
#[hegel::test(test_cases = 5)]
fn prop_a_first_page_stops_as_what_stopped_it(tc: TestCase) {
    // Cancelled while it reads a FIFO, which only Unix has.
    let stops = [Stop::NotXml, Stop::NoFolder].into_iter().chain(cfg!(unix).then_some(Stop::Cancelled));
    let name = tc.draw(gs::sampled_from(vec!["rekordbox.xml", "Library Export.xml"]));
    for stop in stops {
        first_page_stops(stop, name);
    }
}

/// What stops [`prop_a_first_page_stops_as_what_stopped_it`].
#[derive(Clone, Copy, Debug)]
enum Stop {
    NotXml,
    Cancelled,
    NoFolder,
}

/// [`prop_a_first_page_stops_as_what_stopped_it`] for the file `name` and `stop`.
fn first_page_stops(stop: Stop, name: &str) {
    let dir = tempfile::tempdir().unwrap();
    // A grid as long as it goes makes the first page too big for a message.
    let long = r#"<TEMPO Inizio="0" Bpm="655.35"/>"#.to_string();
    let tracks = [(r#"TrackID="0" Name="Long" TotalTime="36000" Location="/m/long.mp3""#.to_string(), long)];
    let text = match stop {
        Stop::NotXml => "#EXTM3U\n/Music/a.mp3\n".to_string(),
        _ => collection_with(&tracks, ""),
    };
    let file = match stop {
        #[cfg(unix)]
        Stop::Cancelled => crate::xml_import::tests::fifo(dir.path(), name),
        _ => write(dir.path(), name, &text),
    };
    let s = start(Library::default());
    let scratch = dir.path().join("gone");
    let req = json!({ "source": "rekordbox-xml", "options": { "file": file }, "roots": ["/"], "context": { "scratch": scratch } });
    let pending = s.host.send_request(method::READ_LIBRARY, &req).unwrap();
    #[cfg(unix)]
    if let Stop::Cancelled = stop {
        crate::xml_import::tests::cancel_while_reading(&s.host, &pending, &file, &text);
    }
    let e = pending.wait_timeout::<LibraryPage>(std::time::Duration::from_secs(60)).expect("an answer").unwrap_err();
    assert!(s.stop().is_empty(), "it reads nothing through the host");
    match stop {
        Stop::NotXml => {
            let said = format!("{file}: not a rekordbox XML file (no DJ_PLAYLISTS element)");
            assert_eq!(kind(&e), ("invalid", said.as_str()));
        }
        Stop::Cancelled => assert_eq!(e, RpcError::cancelled()),
        Stop::NoFolder => assert!(kind(&e).0 == "io" && kind(&e).1.starts_with("page.json: "), "{e:?}"),
    }
}
