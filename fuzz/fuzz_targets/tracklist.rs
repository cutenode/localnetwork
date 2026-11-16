//! Tracklists a DJ imports (M3U, CSV and other tables, rekordbox's text and XML, plain text):
//! format detection and each format's parser, whatever the detection would pick.

#![no_main]

use libfuzzer_sys::fuzz_target;
use loclib_tracklist::{Format, parse, parse_as, read_xml_collection, read_xml_library, xml_playlist};

fuzz_target!(|data: &[u8]| {
    let _ = parse(data, None);
    for format in [Format::M3u, Format::Csv, Format::RekordboxText, Format::RekordboxXml, Format::Text] {
        let _ = parse_as(data, format);
    }
    let _ = xml_playlist(data, "Sets/Friday");
    let library = read_xml_library(data);
    // Beat grids are sized by the file's numbers.
    for track in library.as_ref().map(|l| l.tracks.as_slice()).unwrap_or_default() {
        let _ = track.beats();
    }
    // The collection alone is the library without its playlists.
    let collection = read_xml_collection(data);
    if let Ok(library) = library {
        let collection = collection.expect("a file that reads as a library reads as a collection");
        assert_eq!(collection.tracks, library.tracks);
        assert!(collection.playlists.is_empty());
    }
});
