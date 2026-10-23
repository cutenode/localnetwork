//! Tracklists a DJ imports (M3U, CSV and other tables, rekordbox's text and XML, plain text):
//! format detection and each format's parser, whatever the detection would pick.

#![no_main]

use libfuzzer_sys::fuzz_target;
use loclib_tracklist::{Format, parse, parse_as, read_xml_library, xml_playlists};

fuzz_target!(|data: &[u8]| {
    let _ = parse(data, None);
    for format in [Format::M3u, Format::Csv, Format::RekordboxText, Format::RekordboxXml, Format::Text] {
        let _ = parse_as(data, format);
    }
    let _ = xml_playlists(data);
    // Beat grids are sized by the file's numbers.
    for track in read_xml_library(data).map(|l| l.tracks).unwrap_or_default() {
        let _ = track.beats();
    }
});
