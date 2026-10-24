//! Properties of reading back what `export` writes, and rekordbox XML from a real XML writer,
//! through `import`.

use std::path::Path;

use hegel::generators as gs;
use hegel::{Generator, PrintableGenerator, TestCase};
use locallibrary::export::{self, Locator, PathStyle};
use locallibrary::import::{self, Format, MatchKind, Status};
use locallibrary::{Library, Track, paths};
use loclib_model::test_util::{self, tracks_like};

/// Text without control characters, trimmed (readers trim fields), as track metadata.
fn field() -> impl hegel::PrintableGenerator<String> {
    hegel::one_of!(gs::text().exclude_characters("\0\r").max_size(12), gs::text().alphabet("a, \"\n;'é").max_size(6))
        .map(|s| s.trim().to_string())
}

/// A file name part: no separators, drive colons or control characters, and not `.`/`..`.
fn path_part() -> impl hegel::PrintableGenerator<String> {
    hegel::one_of!(
        gs::text().exclude_characters("/\\:").exclude_categories(&["Cc", "Cs"]).min_size(1).max_size(8),
        gs::text().alphabet("ab %#?&e\u{301}é,'\"").min_size(1).max_size(6),
    )
    .filter(|s: &String| s != "." && s != ".." && !s.ends_with(char::is_whitespace))
}

/// A track with any metadata and a unique path (`/Contents/<id>/...`).
fn track(id: u32) -> impl PrintableGenerator<Track> {
    tracks_like(test_util::track(id, "", "", ""))
        .title(field().filter(|s: &String| !s.is_empty()))
        .artist(field())
        .album(field())
        .genre(field())
        .comment(field())
        .bpm(gs::floats::<f64>().min_value(0.0).max_value(300.0))
        .duration_secs(gs::integers::<u32>().min_value(1).max_value(59_999))
        .file_path(hegel::tuples!(path_part(), path_part()).map(move |(a, b)| format!("/Contents/{id}/{a}/{b}.mp3")))
}

fn library() -> impl PrintableGenerator<Library> {
    test_util::libraries(1, 6, track)
}

fn drive_locator() -> Locator {
    Locator::new(PathStyle::Drive, Path::new("/"), Path::new("/list.m3u8"))
}

/// CSV export reads back with the same title, artist, album, duration and path.
#[hegel::test(test_cases = 300)]
fn csv_export_reads_back(tc: TestCase) {
    let lib = tc.draw(library());
    let refs: Vec<&Track> = lib.tracks.iter().collect();
    let csv = export::csv(&refs, &drive_locator());
    tc.note(&csv);
    assert_eq!(import::detect(csv.as_bytes(), Some("export.csv")), Format::Csv);
    let entries = import::parse_as(csv.as_bytes(), Format::Csv).unwrap();
    assert_eq!(entries.len(), lib.tracks.len());
    for (e, t) in entries.iter().zip(&lib.tracks) {
        assert_eq!(e.title, t.title);
        assert_eq!(e.artist, t.artist);
        assert_eq!(e.album.as_deref().unwrap_or(""), t.album);
        assert_eq!(e.duration_secs, Some(t.duration_secs));
        assert_eq!(e.path.as_deref(), Some(paths::device_path(&paths::key(&t.file_path)).as_str()));
    }
}

/// M3U8 export reads back with the same durations and paths, and the artist and title when
/// neither contains a dash separator.
#[hegel::test(test_cases = 300)]
fn m3u8_export_reads_back(tc: TestCase) {
    let lib = tc.draw(library());
    let name = tc.draw(field());
    let refs: Vec<&Track> = lib.tracks.iter().collect();
    let m3u = export::m3u8(&name, &refs, &drive_locator());
    tc.note(&m3u);
    let entries = import::parse(m3u.as_bytes(), Some("list.m3u8")).unwrap();
    assert_eq!(entries.len(), lib.tracks.len());
    for (e, t) in entries.iter().zip(&lib.tracks) {
        assert_eq!(e.duration_secs, Some(t.duration_secs));
        assert_eq!(e.path.as_deref(), Some(paths::device_path(&paths::key(&t.file_path)).as_str()));
        let plain = |s: &str| !s.contains(['-', '–', '—', '‒', '―']) && !s.chars().any(char::is_control);
        if !t.artist.is_empty() && plain(&t.artist) && plain(&t.title) {
            assert_eq!((e.artist.as_str(), e.title.as_str()), (t.artist.as_str(), t.title.as_str()));
        }
    }
}

/// Every entry of an exported list, CSV or M3U8, with any path style, is found again by its path
/// as the track it came from.
#[hegel::test(test_cases = 100)]
fn exported_lists_match_their_tracks(tc: TestCase) {
    let lib = tc.draw(library());
    let style = tc.draw(gs::sampled_from(vec![PathStyle::Drive, PathStyle::Relative, PathStyle::Absolute]));
    let csv = tc.draw(gs::booleans());
    let dir = tempfile::tempdir().unwrap();
    let locator = Locator::new(style, &dir.path().join("USB"), &dir.path().join("lists/set.m3u8"));
    let refs: Vec<&Track> = lib.tracks.iter().collect();
    let text = if csv { export::csv(&refs, &locator) } else { export::m3u8("Set", &refs, &locator) };
    tc.note(&text);
    let entries = import::parse(text.as_bytes(), Some(if csv { "set.csv" } else { "set.m3u8" })).unwrap();
    let matched = import::match_entries(&lib, &entries);
    assert_eq!(matched.len(), lib.tracks.len());
    for (m, t) in matched.iter().zip(&lib.tracks) {
        assert_eq!(m.status, Status::Found { track_id: t.id, how: MatchKind::Path }, "{:?}", m.entry);
    }
}

/// A plain-text set list (`01. Artist - Title`) reads back as the artists and titles it lists,
/// for names without dashes, colons (cue times) or a leading bullet.
#[hegel::test(test_cases = 300)]
fn text_export_reads_back(tc: TestCase) {
    let name = || gs::from_regex("[A-Za-z0-9é][A-Za-z0-9é&'.,()! \u{a0}]{0,10}[A-Za-z0-9é)!]").fullmatch(true);
    let n = tc.draw(gs::integers::<u32>().min_value(1).max_value(12));
    let tracks: Vec<Track> = (1..=n).map(|id| test_util::track(id, &tc.draw(name()), &tc.draw(name()), "")).collect();
    let refs: Vec<&Track> = tracks.iter().collect();
    let text = export::text(&refs);
    tc.note(&text);
    let entries = import::parse(text.as_bytes(), Some("set.txt")).unwrap();
    let got: Vec<(&str, &str)> = entries.iter().map(|e| (e.artist.as_str(), e.title.as_str())).collect();
    let want: Vec<(&str, &str)> = tracks.iter().map(|t| (t.artist.as_str(), t.title.as_str())).collect();
    assert_eq!(got, want);
}

/// rekordbox XML written by a real XML writer (`quick-xml` escaping) reads back: the collection's
/// names, artists, albums, durations and file locations, and each playlist's tracks in order.
#[hegel::test(test_cases = 300)]
fn rekordbox_xml_reads_back(tc: TestCase) {
    let lib = tc.draw(library());
    let lists: Vec<Vec<u32>> = tc.draw(
        gs::vecs(gs::vecs(gs::integers::<u32>().min_value(1).max_value(lib.tracks.len() as u32)).max_size(5))
            .max_size(3),
    );
    let attr = |s: &str| quick_xml::escape::escape(s).into_owned();
    let mut xml = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<DJ_PLAYLISTS Version=\"1.0.0\">\n");
    xml.push_str(&format!("  <COLLECTION Entries=\"{}\">\n", lib.tracks.len()));
    for t in &lib.tracks {
        let location = format!(
            "file://localhost{}",
            percent_encoding::utf8_percent_encode(&t.file_path, percent_encoding::NON_ALPHANUMERIC)
        );
        xml.push_str(&format!(
            "    <TRACK TrackID=\"{}\" Name=\"{}\" Artist=\"{}\" Album=\"{}\" TotalTime=\"{}\" Location=\"{}\"/>\n",
            t.id,
            attr(&t.title),
            attr(&t.artist),
            attr(&t.album),
            t.duration_secs.min(36_000),
            attr(&location)
        ));
    }
    xml.push_str("  </COLLECTION>\n  <PLAYLISTS>\n    <NODE Type=\"0\" Name=\"ROOT\" Count=\"1\">\n");
    xml.push_str("      <NODE Type=\"0\" Name=\"Shows\" Count=\"0\">\n");
    for (i, ids) in lists.iter().enumerate() {
        xml.push_str(&format!("        <NODE Name=\"List {i}\" Type=\"1\" KeyType=\"0\" Entries=\"{}\">\n", ids.len()));
        for id in ids {
            xml.push_str(&format!("          <TRACK Key=\"{id}\"/>\n"));
        }
        xml.push_str("        </NODE>\n");
    }
    xml.push_str("      </NODE>\n    </NODE>\n  </PLAYLISTS>\n</DJ_PLAYLISTS>\n");
    tc.note(&xml);

    let entries = import::parse(xml.as_bytes(), None).unwrap();
    assert_eq!(entries.len(), lib.tracks.len());
    for (e, t) in entries.iter().zip(&lib.tracks) {
        assert_eq!((e.title.as_str(), e.artist.as_str()), (t.title.as_str(), t.artist.as_str()));
        assert_eq!(e.album.as_deref().unwrap_or(""), t.album);
        assert_eq!(e.duration_secs, Some(t.duration_secs.min(36_000)));
        assert_eq!(e.path.as_deref(), Some(t.file_path.as_str()));
    }
    let library = import::read_xml_library(xml.as_bytes()).unwrap();
    assert_eq!(library.playlists.len(), lists.len());
    for (i, (p, ids)) in library.playlists.iter().zip(&lists).enumerate() {
        assert_eq!(p.path, format!("Shows/List {i}"));
        let got: Vec<import::Entry> = p.entries.iter().map(|e| library.entry(e)).collect();
        let titles: Vec<&str> = got.iter().map(|e| e.title.as_str()).collect();
        let want: Vec<&str> = ids.iter().map(|id| lib.tracks[*id as usize - 1].title.as_str()).collect();
        assert_eq!(titles, want);
    }
}
