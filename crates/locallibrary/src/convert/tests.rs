use super::*;

#[test]
fn empty_template_keeps_colours() {
    let mut editor = empty_device_library().unwrap();
    let db = editor.database().unwrap();
    assert!(db.tracks.is_empty() && db.playlist_tree.is_empty() && db.playlist_entries.is_empty());
    assert_eq!(db.colors.len(), 8);
    let bytes = editor.to_bytes();
    let lib = pdb::library_from_database(&pdb::parse_database(bytes, std::path::Path::new("x")).unwrap());
    assert!(lib.tracks.is_empty() && lib.playlists.is_empty());
}
