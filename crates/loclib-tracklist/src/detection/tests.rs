use super::*;
use crate::detect;
use crate::testing::utf16le;

#[test]
fn detects_formats() {
    let d = |s: &str, hint: Option<&str>| detect(s.as_bytes(), hint);
    assert_eq!(d("<?xml version=\"1.0\"?><DJ_PLAYLISTS/>", None), Format::RekordboxXml);
    assert_eq!(d("#EXTM3U\n/a.mp3\n", Some("list.txt")), Format::M3u);
    assert_eq!(d("/Music/a.mp3\n/Music/b.mp3\n", Some("x.m3u8")), Format::M3u);
    assert_eq!(d("/Music/a.mp3\nC:\\Music\\b.flac\n", None), Format::M3u);
    assert_eq!(d("#\tTrack Title\tArtist\n1\tA\tB\n", Some("x.txt")), Format::RekordboxText);
    assert_eq!(detect(&utf16le("#\tTrack Title\tArtist\n"), None), Format::RekordboxText);
    assert_eq!(d("Track Name,Artist Name(s)\nx,y\n", None), Format::Csv);
    assert_eq!(d("anything", Some("CSV")), Format::Csv);
    assert_eq!(d("sep=;\nTitle;Artist\n", None), Format::Csv);
    assert_eq!(d("Daft Punk - One More Time\nAC/DC - Thunderstruck\n", Some("set.txt")), Format::Text);
    // A lone "Title" line isn't a table.
    assert_eq!(d("Title\nArtist - Song\n", None), Format::Text);
    assert_eq!(extension("/x/My.Playlist.M3U8"), "m3u8");
    assert_eq!(extension("xml"), "xml");
}

use hegel::generators as gs;
use hegel::{Generator, TestCase};
use percent_encoding::{NON_ALPHANUMERIC, utf8_percent_encode};

/// A file's path reads back from every way a list names it: as written (POSIX, Windows,
/// UNC), as a `file:` URL (percent-encoded, with `localhost`, a drive letter or a server),
/// or as a classic Mac OS `Disk:Folder:file` path; and every one of those looks like a path.
/// URLs of other schemes aren't files.
#[hegel::test(test_cases = 300)]
fn file_paths_read_back_from_every_spelling(tc: TestCase) {
    let segment = || gs::from_regex("[A-Za-z0-9é &'()_.-]{1,8}").fullmatch(true).filter(|s: &String| s.trim() == s);
    let dirs: Vec<String> = tc.draw(gs::vecs(segment()).max_size(3));
    let ext = tc.draw(gs::sampled_from(
        audio::PLAYER_EXTENSIONS.into_iter().chain(audio::OTHER_EXTENSIONS).map(|(e, _)| e).collect::<Vec<_>>(),
    ));
    let file = format!("{}.{ext}", tc.draw(segment()));
    let rel = dirs.iter().chain([&file]).cloned().collect::<Vec<_>>().join("/");
    let posix = format!("/{rel}");
    let url = |p: &str| utf8_percent_encode(p, NON_ALPHANUMERIC).to_string();
    let drive = tc.draw(gs::sampled_from(vec!['C', 'd']));
    let windows = format!("{drive}:\\{}", rel.replace('/', "\\"));
    let spellings = [
        (posix.clone(), posix.clone()),
        (windows.clone(), windows),
        (format!("\\\\nas\\{}", rel.replace('/', "\\")), format!("\\\\nas\\{}", rel.replace('/', "\\"))),
        (format!("file://{}", url(&posix)), posix.clone()),
        (format!("file:{posix}"), posix.clone()),
        (format!("FILE://localhost{}", url(&posix)), posix.clone()),
        (format!("file:///{drive}:{}", url(&posix)), format!("{drive}:{posix}")),
        (format!("file://LocalHost/{drive}:{posix}"), format!("{drive}:{posix}")),
        (format!("file://nas{}", url(&posix)), format!("//nas{posix}")),
    ];
    for (written, path) in spellings {
        assert_eq!(file_path(&written).as_deref(), Some(path.as_str()), "{written}");
        assert!(looks_like_path(&written), "{written}");
    }
    // The disk's name has a space, or it would be a URL scheme.
    let mac = format!("Macintosh HD:{}", rel.replace('/', ":"));
    if !rel.contains(':') && rel.contains('/') {
        assert_eq!(file_path(&mac), Some(format!("/Macintosh HD/{rel}")));
    }
    let scheme = tc.draw(gs::sampled_from(vec!["https", "spotify", "x-apple.music", "smb+tls"]));
    assert_eq!(file_path(&format!("{scheme}:{}", url(&posix))), None);
}
