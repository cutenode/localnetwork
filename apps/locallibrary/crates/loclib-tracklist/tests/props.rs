//! Properties of reading tracklists.

use hegel::generators as gs;
use hegel::{Generator, TestCase};
use loclib_tracklist::Format;

const HINTS: &[Option<&str>] = &[None, Some("list.m3u8"), Some("list.CSV"), Some("rekordbox.xml"), Some("set.txt")];
const FORMATS: [Format; 5] = [Format::M3u, Format::Csv, Format::RekordboxText, Format::RekordboxXml, Format::Text];

/// Fragments that steer the parsers into their less travelled paths.
const FRAGMENTS: &[&str] = &[
    "#EXTM3U",
    "#EXTINF:",
    "#EXTINF:-1 a=\"x,y\",",
    "#EXTALB:",
    "<?xml ?>",
    "<DJ_PLAYLISTS>",
    "<COLLECTION>",
    "<TRACK ",
    "Name=\"",
    "Location=\"file://localhost/",
    "/>",
    "<!--",
    "&#x",
    "&amp;",
    "<PLAYLISTS><NODE Type=\"1\" ",
    "Key=\"",
    "#,Track Title\tArtist",
    "Title,Artist,Duration\n",
    "sep=;",
    "\"",
    "\t",
    "01. ",
    "[1:02:03] ",
    "(5:23)",
    " - ",
    " – ",
    " by ",
    "file:///C:/x.mp3",
    "C:\\Music\\",
    "\u{a0}",
    "\u{3000}",
    "\u{feff}",
    "•",
    "12:34",
    "Disk:Folder:",
];

fn messy_line() -> impl hegel::PrintableGenerator<String> {
    gs::vecs(hegel::one_of!(
        gs::sampled_from(FRAGMENTS.to_vec()).map(String::from),
        gs::text().max_size(6),
        gs::text().alphabet("0123456789:.-()[]# \u{a0}\u{2003}aé,;\t\"").max_size(8),
    ))
    .max_size(6)
    .map(|parts| parts.concat())
}

/// Reading never panics, whatever the bytes, file name or format.
#[hegel::test(test_cases = 500)]
#[hegel::explicit_test_case(bytes = "a\u{a0}b".as_bytes().to_vec(), hint = None::<&str>)]
#[hegel::explicit_test_case(bytes = "x\u{3000}1:23".as_bytes().to_vec(), hint = Some("x.txt"))]
fn parse_never_panics_on_bytes(tc: TestCase) {
    let bytes = tc.draw(hegel::one_of!(
        gs::binary().max_size(200),
        gs::vecs(messy_line()).max_size(6).map(|l| l.join("\n").into_bytes()),
        gs::vecs(messy_line()).max_size(4).map(|l| l
            .join("\r\n")
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<u8>>()),
    ));
    let hint = tc.draw(gs::sampled_from(HINTS.to_vec()));
    let _ = loclib_tracklist::parse(&bytes, hint);
    let _ = loclib_tracklist::detect(&bytes, hint);
    let _ = loclib_tracklist::xml_playlists(&bytes);
    for f in FORMATS {
        let _ = loclib_tracklist::parse_as(&bytes, f);
    }
}
