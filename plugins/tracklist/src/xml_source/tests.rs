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
