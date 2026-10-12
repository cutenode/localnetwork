use super::*;
use crate::onelibrary::ContentRecord;

fn names(one: &OneLibrary, table: &str) -> Vec<String> {
    let mut stmt = one.conn.prepare(&format!("SELECT name FROM \"{table}\" ORDER BY name")).unwrap();
    stmt.query_map([], |r| r.get(0)).unwrap().collect::<rusqlite::Result<_>>().unwrap()
}

#[test]
fn lookup_rows_go_once_no_track_uses_them() {
    let mut one = OneLibrary::create().unwrap();
    let mut t = loclib_model::test_util::track(1, "Artist", "Title", "/Contents/a.mp3");
    (t.album, t.album_artist, t.genre, t.label, t.key) =
        ("Album".into(), "Band".into(), "House".into(), "Label".into(), "Am".into());
    let rec = ContentRecord::from_track(&t, false, &one.content_defaults().unwrap());
    let id = one.add_track(&rec, &t.file_path, "", None).unwrap();
    let keys = names(&one, "key");
    assert!(keys.contains(&"Am".to_string()), "{keys:?}");

    let f = TrackFields {
        artist: Some("Someone".into()),
        album: Some(String::new()),
        genre: Some("Techno".into()),
        label: Some(" ".into()),
        key: Some(String::new()),
        ..Default::default()
    };
    one.set_track_fields(&[id], &f).unwrap();
    // The album went, and with it the band only the album named.
    assert_eq!(names(&one, "artist"), ["Someone"]);
    assert!(names(&one, "album").is_empty());
    assert_eq!(names(&one, "genre"), ["Techno"]);
    assert!(names(&one, "label").is_empty());
    assert_eq!(names(&one, "key").len(), keys.len() - 1);
}

#[test]
fn nodes_and_tracks_that_arent_there_are_not_found() {
    let mut one = OneLibrary::create().unwrap();
    let not_found = |r: Result<()>| matches!(r, Err(Error::NotFound(m)) if m.ends_with("#99 in exportLibrary.db"));
    assert!(not_found(one.rename_node(Tree::History, 99, "x")));
    assert!(not_found(one.place_node(Tree::Playlist, 99, 0, 1)));
    assert!(not_found(one.remove_nodes(Tree::Playlist, &[99])));
    assert!(not_found(one.set_track_fields(&[99], &TrackFields { title: Some("x".into()), ..Default::default() })));
}
