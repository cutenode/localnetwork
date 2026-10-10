//! Edits naming rows that aren't there, or tables they don't apply to, are refused without
//! changing a byte, and errors say what they're about.

use rekordbox_pdb::{Database, PdbEditor, PdbError, TableType};

const BIGGER: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/bigger-export.pdb");

#[test]
fn edits_of_what_isnt_there_change_nothing() {
    let original = std::fs::read(BIGGER).unwrap();
    let db = Database::from_bytes(original.clone()).unwrap();
    let playlist = db.playlist_tree.iter().find(|p| !p.is_folder).unwrap().id;
    let mut ed = PdbEditor::from_bytes(original.clone());
    let refusals: Vec<PdbError> = vec![
        ed.add_to_history(99, 1).unwrap_err(),
        ed.remove_history_playlists(&[99]).unwrap_err(),
        ed.add_to_playlist(playlist, 999, None).unwrap_err(),
        ed.repoint_track(1, 999).unwrap_err(),
        ed.remove_rows(TableType::Columns, &[1]).unwrap_err(),
    ];
    let said: Vec<String> = refusals.iter().map(ToString::to_string).collect();
    assert_eq!(
        said,
        [
            "no playlist with id 99",
            "no playlist with id 99",
            "no track with id 999",
            "no track with id 999",
            &format!("operation not supported on table type {}", TableType::Columns as u32),
        ]
    );
    assert_eq!(ed.to_bytes(), original);
}

#[test]
fn errors_name_what_they_are_about() {
    let Err(missing) = Database::from_file("no such export.pdb") else { panic!("read a file that isn't there") };
    assert!(missing.to_string().starts_with("io error: "), "{missing}");
    let mut ed = PdbEditor::from_bytes(std::fs::read(BIGGER).unwrap());
    let err = ed.set_track_field(1, "rating", 300).unwrap_err();
    assert_eq!(err.to_string(), "rating=300 does not fit in 1 byte(s)");
}
