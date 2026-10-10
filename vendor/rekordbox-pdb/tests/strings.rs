//! DeviceSQL strings through a track's string slots: each encoding reads back, text too long for
//! any encoding is refused before anything is written (a track's own text never is), and a string
//! of a kind nobody writes is an error naming where it is.

use rekordbox_pdb::{Database, PdbEditor, PdbError};

const ONE_SONG: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/data/one-song-export.pdb"
);

/// The one-song export with track 1's strings set to `values`, as bytes.
fn with_strings(values: &[(&str, &str)]) -> Vec<u8> {
    let mut ed = PdbEditor::from_bytes(std::fs::read(ONE_SONG).unwrap());
    ed.set_track_strings(1, values).unwrap();
    ed.to_bytes()
}

#[test]
fn every_encoding_reads_back() {
    let long = "a long ascii title ".repeat(10);
    let bytes = with_strings(&[
        ("isrc", "GBUM71029604"),
        ("title", &long),
        ("comment", "Ärm ♪ Co"),
    ]);
    let t = &Database::from_bytes(bytes).unwrap().tracks[0];
    assert_eq!(t.strings[0], "GBUM71029604");
    assert_eq!(t.title(), long);
    assert_eq!(t.comment(), "Ärm ♪ Co");
}

#[test]
fn text_too_long_for_any_encoding_is_refused() {
    let db = Database::from_file(ONE_SONG).unwrap();
    let track = &db.tracks[0];
    let huge = "x".repeat(40_000);
    for slot in ["isrc", "title"] {
        let err = db.check_track_strings(track, &[(slot, &huge)]).unwrap_err();
        assert!(matches!(err, PdbError::StringTooLong), "{slot}: {err}");
        assert_eq!(err.to_string(), "string too long for DeviceSQL encoding");
    }
    let err = db.check_track_strings(track, &[("mood", "dark")]).unwrap_err();
    assert_eq!(err.to_string(), "unknown or non-fixed track field \"mood\"");
    let err = PdbEditor::from_bytes(std::fs::read(ONE_SONG).unwrap())
        .set_track_strings(99, &[("title", "x")])
        .unwrap_err();
    assert_eq!(err.to_string(), "no track with id 99");
}

#[test]
fn a_tracks_own_strings_need_no_room() {
    let db = Database::from_file(ONE_SONG).unwrap();
    let t = &db.tracks[0];
    db.check_track_strings(t, &[("title", t.title()), ("comment", t.comment())]).unwrap();
}

#[test]
fn a_string_of_an_unknown_kind_is_an_error_saying_where() {
    let mut bytes = with_strings(&[("title", "ZZMARKERZZ")]);
    let at = bytes
        .windows(10)
        .position(|w| w == b"ZZMARKERZZ")
        .unwrap()
        - 1;
    // A short ASCII string's kind byte is odd; 0x42 is no kind rekordbox writes.
    bytes[at] = 0x42;
    let Err(err) = Database::from_bytes(bytes) else { panic!("read a string of an unknown kind") };
    assert!(
        matches!(err, PdbError::UnknownString { kind: 0x42, offset } if offset == at),
        "{err:?}"
    );
    assert!(
        err.to_string()
            .starts_with("unknown DeviceSQL string kind 0x42 at offset 0x"),
        "{err}"
    );
}
