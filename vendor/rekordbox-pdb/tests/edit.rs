//! Write-path tests: every edit must read back correctly and leave the rest
//! of the file intact.

use rekordbox_pdb::{Database, NewTrack, PdbEditor, PdbError, TableType};

const ONE_SONG: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/data/one-song-export.pdb"
);
const BIGGER: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/bigger-export.pdb");

fn byte_diff(a: &[u8], b: &[u8]) -> usize {
    assert_eq!(a.len(), b.len());
    a.iter().zip(b).filter(|(x, y)| x != y).count()
}

#[test]
fn set_rating_in_place() {
    let orig = std::fs::read(ONE_SONG).unwrap();
    let mut ed = PdbEditor::from_bytes(orig.clone());
    ed.set_track_field(1, "rating", 5).unwrap();
    let out = ed.to_bytes();

    let db = Database::from_bytes(out.clone()).unwrap();
    assert_eq!(db.tracks[0].rating, 5);
    assert_eq!(db.tracks[0].title(), "Super Smash Bros.");
    assert_eq!(byte_diff(&orig, &out), 1); // exactly one byte changed
}

/// The format code, for a track pointed at a file of another format: one-song's MP3 (code 1)
/// becomes an AIFF (12), and only that byte changes.
#[test]
fn set_file_type_in_place() {
    let orig = std::fs::read(ONE_SONG).unwrap();
    let mut ed = PdbEditor::from_bytes(orig.clone());
    ed.set_track_field(1, "file_type", 0x0c).unwrap();
    let out = ed.to_bytes();
    let changed: Vec<usize> = (0..orig.len()).filter(|&i| orig[i] != out[i]).collect();
    assert_eq!(changed.len(), 1);
    assert_eq!((orig[changed[0]], out[changed[0]]), (0x01, 0x0c));
}

#[test]
fn set_field_rejects_bad_input() {
    let mut ed = PdbEditor::from_file(ONE_SONG).unwrap();
    assert!(ed.set_track_field(1, "title", 5).is_err()); // not a fixed field
    assert!(ed.set_track_field(999, "rating", 5).is_err()); // no such track
    assert!(ed.set_track_field(1, "rating", 300).is_err()); // out of u8 range
                                                            // original value intact after the failed range check
    assert_eq!(ed.database().unwrap().tracks[0].rating, 4);
}

#[test]
fn add_track_minimal() {
    let mut ed = PdbEditor::from_file(ONE_SONG).unwrap();
    let tid = ed
        .add_track(&NewTrack::new(
            "Test Song",
            "/Contents/Test Artist/Test Album/Test Song.mp3",
        ))
        .unwrap();
    let db = Database::from_bytes(ed.to_bytes()).unwrap();
    assert_eq!(db.tracks.len(), 2);
    let t = db.tracks.iter().find(|t| t.id == tid).unwrap();
    assert_eq!(t.title(), "Test Song");
    assert_eq!(
        t.file_path(),
        "/Contents/Test Artist/Test Album/Test Song.mp3"
    );
    assert_eq!(t.filename(), "Test Song.mp3");
}

#[test]
fn add_track_full_metadata() {
    let mut ed = PdbEditor::from_file(ONE_SONG).unwrap();
    let spec = NewTrack {
        artist: Some("Test Artist".into()),
        album: Some("Test Album".into()),
        genre: Some("House".into()),
        key: Some("Am".into()),
        tempo: 12800,
        duration: 200,
        year: 2026,
        bitrate: 320,
        sample_rate: 44100,
        file_size: 8_000_000,
        track_number: 3,
        ..NewTrack::new("Test Song", "/Contents/A/B/Test Song.mp3")
    };
    let tid = ed.add_track(&spec).unwrap();
    let db = Database::from_bytes(ed.to_bytes()).unwrap();
    let t = db.tracks.iter().find(|t| t.id == tid).unwrap();
    assert_eq!((t.tempo, t.duration, t.year), (12800, 200, 2026));
    assert_eq!((t.bitrate, t.sample_rate), (320, 44100));

    let artist = db.artists.iter().find(|a| a.id == t.artist_id).unwrap();
    assert_eq!(artist.name, "Test Artist");
    let album = db.albums.iter().find(|a| a.id == t.album_id).unwrap();
    assert_eq!(album.name, "Test Album");
    let genre = db.genres.iter().find(|g| g.id == t.genre_id).unwrap();
    assert_eq!(genre.name, "House");
    let key = db.keys.iter().find(|k| k.id == t.key_id).unwrap();
    assert_eq!(key.name, "Am");
}

#[test]
fn add_track_reuses_lookups() {
    let mut ed = PdbEditor::from_file(ONE_SONG).unwrap();
    let spec = NewTrack {
        artist: Some("BABY GRAVY".into()), // exists with id 2
        key: Some("Em".into()),            // exists with id 1
        ..NewTrack::new("Another Gravy Song", "/Contents/BABY GRAVY/x/y.mp3")
    };
    let tid = ed.add_track(&spec).unwrap();
    let db = Database::from_bytes(ed.to_bytes()).unwrap();
    let t = db.tracks.iter().find(|t| t.id == tid).unwrap();
    assert_eq!(t.artist_id, 2);
    assert_eq!(t.key_id, 1);
    assert_eq!(db.artists.len(), 2); // nothing new created
    assert_eq!(db.keys.len(), 1);
}

#[test]
fn add_track_unicode_title() {
    let mut ed = PdbEditor::from_file(ONE_SONG).unwrap();
    let tid = ed
        .add_track(&NewTrack::new(
            "Stavöstranos — épreuve",
            "/Contents/a/b/c.mp3",
        ))
        .unwrap();
    let db = Database::from_bytes(ed.to_bytes()).unwrap();
    let t = db.tracks.iter().find(|t| t.id == tid).unwrap();
    assert_eq!(t.title(), "Stavöstranos — épreuve");
}

#[test]
fn add_track_leaves_existing_data_intact() {
    let before = Database::from_file(BIGGER).unwrap();
    let mut ed = PdbEditor::from_file(BIGGER).unwrap();
    ed.add_track(&NewTrack::new("New", "/Contents/a/b/New.mp3"))
        .unwrap();
    let after = Database::from_bytes(ed.to_bytes()).unwrap();

    for t in &before.tracks {
        let same = after.tracks.iter().find(|x| x.id == t.id).unwrap();
        assert_eq!(same.title(), t.title());
    }
    let a1: Vec<_> = before.artists.iter().map(|a| (a.id, &a.name)).collect();
    let a2: Vec<_> = after.artists.iter().map(|a| (a.id, &a.name)).collect();
    assert_eq!(a1, a2);
}

#[test]
fn oversized_row_rejected_cleanly() {
    let orig = std::fs::read(ONE_SONG).unwrap();
    let mut ed = PdbEditor::from_bytes(orig.clone());
    let huge = "x".repeat(5000);
    assert!(ed
        .add_track(&NewTrack::new(huge, "/Contents/a/b/c.mp3"))
        .is_err());
    // still usable
    let tid = ed
        .add_track(&NewTrack::new("fine", "/Contents/a/b/fine.mp3"))
        .unwrap();
    let db = Database::from_bytes(ed.to_bytes()).unwrap();
    assert!(db.tracks.iter().any(|t| t.id == tid));
}

#[test]
fn playlists() {
    let mut ed = PdbEditor::from_file(ONE_SONG).unwrap();
    let tid = ed
        .add_track(&NewTrack::new("New", "/Contents/a/b/New.mp3"))
        .unwrap();
    ed.add_to_playlist(1, tid, None).unwrap(); // existing "aac"
    let db = Database::from_bytes(ed.to_bytes()).unwrap();
    let mut entries: Vec<_> = db
        .playlist_entries
        .iter()
        .filter(|e| e.playlist_id == 1)
        .collect();
    entries.sort_by_key(|e| e.entry_index);
    assert_eq!(
        entries.iter().map(|e| e.track_id).collect::<Vec<_>>(),
        vec![1, tid]
    );
    assert_eq!(
        entries.iter().map(|e| e.entry_index).collect::<Vec<_>>(),
        vec![1, 2]
    );
}

#[test]
fn create_playlist_and_folder() {
    let mut ed = PdbEditor::from_file(BIGGER).unwrap();
    let pid = ed.create_playlist("claude mix", 0, false).unwrap();
    for tid in [1, 5, 9] {
        ed.add_to_playlist(pid, tid, None).unwrap();
    }
    let folder = ed.create_playlist("2026 Gigs", 0, true).unwrap();
    let nested = ed.create_playlist("Berghain", folder, false).unwrap();

    let db = Database::from_bytes(ed.to_bytes()).unwrap();
    let node = db.playlist_tree.iter().find(|n| n.id == pid).unwrap();
    assert_eq!(node.name, "claude mix");
    assert!(!node.is_folder);
    let mut entries: Vec<_> = db
        .playlist_entries
        .iter()
        .filter(|e| e.playlist_id == pid)
        .collect();
    entries.sort_by_key(|e| e.entry_index);
    assert_eq!(
        entries.iter().map(|e| e.track_id).collect::<Vec<_>>(),
        vec![1, 5, 9]
    );

    let f = db.playlist_tree.iter().find(|n| n.id == folder).unwrap();
    assert!(f.is_folder);
    let nn = db.playlist_tree.iter().find(|n| n.id == nested).unwrap();
    assert_eq!(nn.parent_id, folder);
}

#[test]
fn add_to_unknown_playlist_rejected() {
    let mut ed = PdbEditor::from_file(ONE_SONG).unwrap();
    assert!(ed.add_to_playlist(99, 1, None).is_err());
}

#[test]
fn bulk_tracks_spill_to_new_pages() {
    let mut ed = PdbEditor::from_file(ONE_SONG).unwrap();
    let mut ids = Vec::new();
    for i in 0..50 {
        let spec = NewTrack {
            artist: Some(format!("Bulk Artist {}", i % 7)),
            ..NewTrack::new(
                format!("Bulk Track {i:03}"),
                format!("/Contents/Bulk/Album/{i:03}.mp3"),
            )
        };
        ids.push(ed.add_track(&spec).unwrap());
    }
    let db = Database::from_bytes(ed.to_bytes()).unwrap();
    assert_eq!(db.tracks.len(), 51);
    let unique: std::collections::HashSet<_> = ids.iter().collect();
    assert_eq!(unique.len(), ids.len());
    for (i, tid) in ids.iter().enumerate() {
        let t = db.tracks.iter().find(|t| t.id == *tid).unwrap();
        assert_eq!(t.title(), format!("Bulk Track {i:03}"));
    }
}

#[test]
fn playlist_with_hundreds_of_entries() {
    let mut ed = PdbEditor::from_file(BIGGER).unwrap();
    let pid = ed
        .create_playlist("everything, many times", 0, false)
        .unwrap();
    for n in 0..400u32 {
        ed.add_to_playlist(pid, (n % 12) + 1, None).unwrap();
    }
    let db = Database::from_bytes(ed.to_bytes()).unwrap();
    let entries: Vec<_> = db
        .playlist_entries
        .iter()
        .filter(|e| e.playlist_id == pid)
        .collect();
    assert_eq!(entries.len(), 400);
    let mut idx: Vec<u32> = entries.iter().map(|e| e.entry_index).collect();
    idx.sort();
    assert_eq!(idx, (1..=400).collect::<Vec<_>>());
}

#[test]
fn add_artwork_row_and_link_track() {
    let mut ed = PdbEditor::from_file(ONE_SONG).unwrap();
    let before = ed.database().unwrap().artwork.len();
    let id = ed.add_artwork("/PIONEER/Artwork/00001/a900.jpg").unwrap();
    ed.set_track_field(1, "artwork_id", id).unwrap();
    let db = Database::from_bytes(ed.to_bytes()).unwrap();
    assert_eq!(db.artwork.len(), before + 1);
    let art = db.artwork.iter().find(|a| a.id == id).unwrap();
    assert_eq!(art.path, "/PIONEER/Artwork/00001/a900.jpg");
    assert_eq!(db.tracks[0].artwork_id, id);
}

// locallibrary patch: row deletion.

#[test]
fn remove_tracks_drops_rows_and_renumbers_playlist_entries() {
    let orig = std::fs::read(BIGGER).unwrap();
    let before = Database::from_bytes(orig.clone()).unwrap();
    let mut ed = PdbEditor::from_bytes(orig.clone());
    let pid = ed.create_playlist("mix", 0, false).unwrap();
    for tid in [3, 4, 5, 4] {
        ed.add_to_playlist(pid, tid, None).unwrap();
    }
    ed.remove_tracks(&[4, 7]).unwrap();
    let db = Database::from_bytes(ed.to_bytes()).unwrap();

    assert_eq!(db.tracks.len(), before.tracks.len() - 2);
    assert!(db.tracks.iter().all(|t| t.id != 4 && t.id != 7));
    for t in &db.tracks {
        let old = before.tracks.iter().find(|o| o.id == t.id).unwrap();
        assert_eq!(t.strings, old.strings);
    }
    assert!(db.playlist_entries.iter().all(|e| e.track_id != 4 && e.track_id != 7));
    let mut mix: Vec<_> = db
        .playlist_entries
        .iter()
        .filter(|e| e.playlist_id == pid)
        .map(|e| (e.entry_index, e.track_id))
        .collect();
    mix.sort();
    assert_eq!(mix, vec![(1, 3), (2, 5)]);
    // Untouched playlists keep their entries.
    let other = before.playlist_entries.iter().filter(|e| e.track_id != 4 && e.track_id != 7).count();
    assert_eq!(db.playlist_entries.len(), other + 2);
    // A deletion is a save: the sequence number moves on.
    assert_eq!(u32::from_le_bytes(ed.to_bytes()[0x14..0x18].try_into().unwrap()), {
        u32::from_le_bytes(orig[0x14..0x18].try_into().unwrap()) + 1
    });
}

#[test]
fn rows_can_be_added_after_removals() {
    let mut ed = PdbEditor::from_file(BIGGER).unwrap();
    ed.remove_tracks(&[1, 2, 3]).unwrap();
    let tid = ed.add_track(&NewTrack::new("After", "/Contents/a/After.mp3")).unwrap();
    let db = Database::from_bytes(ed.to_bytes()).unwrap();
    assert_eq!(db.tracks.len(), 10);
    assert_eq!(db.tracks.iter().find(|t| t.id == tid).unwrap().title(), "After");
}

#[test]
fn remove_lookup_rows() {
    let mut ed = PdbEditor::from_file(BIGGER).unwrap();
    let db = ed.database().unwrap();
    let artist = db.artists[0].id;
    let others = db.artists.len() - 1;
    assert_eq!(ed.remove_rows(TableType::Artists, &[artist, 99_999]).unwrap(), 1);
    let db = Database::from_bytes(ed.to_bytes()).unwrap();
    assert_eq!(db.artists.len(), others);
    assert!(db.artists.iter().all(|a| a.id != artist));
    assert!(ed.remove_rows(TableType::Tracks, &[1]).is_err());
}

#[test]
fn remove_unknown_track_rejected() {
    let mut ed = PdbEditor::from_file(BIGGER).unwrap();
    let orig = ed.to_bytes();
    assert!(ed.remove_tracks(&[1, 999]).is_err());
    assert_eq!(ed.to_bytes(), orig);
}

// locallibrary patch: playlist and track editing primitives.

fn entries(db: &Database, playlist: u32) -> Vec<u32> {
    let mut e: Vec<_> = db.playlist_entries.iter().filter(|e| e.playlist_id == playlist).collect();
    e.sort_by_key(|e| e.entry_index);
    assert_eq!(e.iter().map(|e| e.entry_index).collect::<Vec<_>>(), (1..=e.len() as u32).collect::<Vec<_>>());
    e.iter().map(|e| e.track_id).collect()
}

#[test]
fn set_playlist_entries_reuses_removes_and_appends_rows() {
    let mut ed = PdbEditor::from_file(BIGGER).unwrap();
    let pid = ed.create_playlist("set", 0, false).unwrap();
    ed.set_playlist_entries(pid, &[3, 1, 2]).unwrap();
    assert_eq!(entries(&ed.database().unwrap(), pid), [3, 1, 2]);
    ed.set_playlist_entries(pid, &[2, 2, 9, 1, 5]).unwrap();
    assert_eq!(entries(&ed.database().unwrap(), pid), [2, 2, 9, 1, 5]);
    ed.set_playlist_entries(pid, &[5]).unwrap();
    let db = Database::from_bytes(ed.to_bytes()).unwrap();
    assert_eq!(entries(&db, pid), [5]);
    ed.set_playlist_entries(pid, &[]).unwrap();
    assert!(entries(&ed.database().unwrap(), pid).is_empty());
    assert!(ed.set_playlist_entries(pid, &[999]).is_err());
    let folder = ed.create_playlist("folder", 0, true).unwrap();
    assert!(ed.set_playlist_entries(folder, &[1]).is_err());
}

#[test]
fn rename_move_and_remove_playlists() {
    let mut ed = PdbEditor::from_file(BIGGER).unwrap();
    let folder = ed.create_playlist("Gigs", 0, true).unwrap();
    let inner = ed.create_playlist("Inner", folder, true).unwrap();
    let a = ed.create_playlist("A", 0, false).unwrap();
    ed.add_to_playlist(a, 4, None).unwrap();
    ed.rename_playlist(a, "A longer name with ünïcode").unwrap();
    ed.move_playlist(a, folder, 7).unwrap();
    // Not into itself or its own descendants, and only into folders.
    assert!(ed.move_playlist(folder, inner, 0).is_err());
    assert!(ed.move_playlist(inner, a, 0).is_err());
    let db = Database::from_bytes(ed.to_bytes()).unwrap();
    let node = db.playlist_tree.iter().find(|n| n.id == a).unwrap();
    assert_eq!((node.name.as_str(), node.parent_id, node.sort_order), ("A longer name with ünïcode", folder, 7));
    assert_eq!(entries(&db, a), [4]);
    assert_eq!(db.playlist_tree.iter().filter(|n| n.id == a).count(), 1);

    ed.remove_playlists(&[a, inner]).unwrap();
    let db = Database::from_bytes(ed.to_bytes()).unwrap();
    assert!(db.playlist_tree.iter().all(|n| n.id != a && n.id != inner));
    assert!(db.playlist_entries.iter().all(|e| e.playlist_id != a));
    assert!(ed.remove_playlists(&[a]).is_err());
    assert!(ed.create_playlist("orphan", a, false).is_err(), "parent must exist");
}

#[test]
fn set_track_strings_rewrites_the_row_with_the_same_id() {
    let orig = Database::from_file(BIGGER).unwrap();
    let mut ed = PdbEditor::from_file(BIGGER).unwrap();
    let pid = ed.create_playlist("p", 0, false).unwrap();
    ed.add_to_playlist(pid, 6, None).unwrap();
    ed.set_track_strings(6, &[("title", "A new title, much longer than before"), ("comment", "fire")]).unwrap();
    ed.set_track_field(6, "rating", 3).unwrap();
    assert!(ed.set_track_strings(6, &[("nope", "x")]).is_err());
    let db = Database::from_bytes(ed.to_bytes()).unwrap();
    assert_eq!(db.tracks.len(), orig.tracks.len());
    let t = db.tracks.iter().find(|t| t.id == 6).unwrap();
    let o = orig.tracks.iter().find(|t| t.id == 6).unwrap();
    assert_eq!(t.title(), "A new title, much longer than before");
    assert_eq!(t.comment(), "fire");
    assert_eq!(t.rating, 3);
    assert_eq!((t.file_path(), t.artist_id, t.tempo), (o.file_path(), o.artist_id, o.tempo));
    assert_eq!(entries(&db, pid), [6]);
}

#[test]
fn repoint_and_history_renumbering() {
    let mut ed = PdbEditor::from_file(BIGGER).unwrap();
    let pid = ed.create_playlist("p", 0, false).unwrap();
    ed.set_playlist_entries(pid, &[1, 2, 3]).unwrap();
    let h = ed.create_history_playlist("HISTORY 001").unwrap();
    for t in [2, 5, 3] {
        ed.add_to_history(h, t).unwrap();
    }
    let before = ed.database().unwrap().playlist_entries.iter().filter(|e| e.track_id == 2).count();
    assert_eq!(ed.repoint_track(2, 7).unwrap(), before + 1); // playlists plus one history entry
    ed.remove_tracks(&[5]).unwrap();
    let db = Database::from_bytes(ed.to_bytes()).unwrap();
    assert_eq!(entries(&db, pid), [1, 7, 3]);
    let mut hist: Vec<_> = db.history_entries.iter().filter(|e| e.playlist_id == h).map(|e| (e.entry_index, e.track_id)).collect();
    hist.sort();
    assert_eq!(hist, [(1, 7), (2, 3)]);
    ed.remove_history_playlists(&[h]).unwrap();
    let db = Database::from_bytes(ed.to_bytes()).unwrap();
    assert!(db.history_playlists.is_empty() && db.history_entries.is_empty());
}

#[test]
fn albums_are_per_artist_and_lookups_are_public() {
    let mut ed = PdbEditor::from_file(ONE_SONG).unwrap();
    let a = ed.artist_id_for("Artist A").unwrap();
    let b = ed.artist_id_for("Artist B").unwrap();
    assert_eq!(ed.artist_id_for("Artist A").unwrap(), a);
    let x = ed.album_id_for("Greatest Hits", a).unwrap();
    let y = ed.album_id_for("Greatest Hits", b).unwrap();
    assert_ne!(x, y);
    assert_eq!(ed.album_id_for("Greatest Hits", a).unwrap(), x);
    let g = ed.genre_id_for("Techno").unwrap();
    let k = ed.key_id_for("8A").unwrap();
    let l = ed.label_id_for("Label").unwrap();
    ed.set_track_field(1, "genre_id", g).unwrap();
    ed.set_track_field(1, "composer_id", b).unwrap();
    let db = Database::from_bytes(ed.to_bytes()).unwrap();
    assert_eq!(db.albums.iter().filter(|al| al.name == "Greatest Hits").count(), 2);
    assert!(db.keys.iter().any(|kk| kk.id == k && kk.name == "8A"));
    assert!(db.labels.iter().any(|ll| ll.id == l));
    assert_eq!((db.tracks[0].genre_id, db.tracks[0].composer_id), (g, b));
}

#[test]
fn thousands_of_entries_stay_fast_and_spill_past_511_slots() {
    let mut ed = PdbEditor::from_file(BIGGER).unwrap();
    let pid = ed.create_playlist("long", 0, false).unwrap();
    let start = std::time::Instant::now();
    let tracks: Vec<u32> = (0..3000u32).map(|n| n % 12 + 1).collect();
    for &t in &tracks {
        ed.add_to_playlist(pid, t, None).unwrap();
    }
    assert!(start.elapsed() < std::time::Duration::from_secs(5), "{:?}", start.elapsed());
    let db = Database::from_bytes(ed.to_bytes()).unwrap();
    assert_eq!(entries(&db, pid), tracks);
}

/// Patch 19: an artist or album name in a long string (UTF-16 here) starts 4-byte aligned, at
/// 0x0c and 0x18, as in rekordbox's own rows; short ASCII names stay at 0x0a and 0x16.
#[test]
fn long_artist_and_album_names_are_aligned() {
    let mut ed = PdbEditor::from_file(ONE_SONG).unwrap();
    let long = ed.artist_id_for("Röyksopp").unwrap();
    let short = ed.artist_id_for("Bicep").unwrap();
    let album = ed.album_id_for("Melody A.M. – Ü", long).unwrap();
    let data = ed.to_bytes();
    let db = Database::from_bytes(data.clone()).unwrap();
    let row = |table: TableType, id: u32| -> usize {
        let i = match table {
            TableType::Artists => db.artists.iter().position(|a| a.id == id),
            _ => db.albums.iter().position(|a| a.id == id),
        };
        db.row_locations(table)[i.unwrap()]
    };
    let artist = row(TableType::Artists, long);
    assert_eq!(&data[artist + 8..artist + 13], [0x03, 0x0c, 0, 0, 0x90]);
    assert_eq!(&data[row(TableType::Artists, short) + 8..][..3], [0x03, 0x0a, 0x0d]);
    let album_row = row(TableType::Albums, album);
    assert_eq!(&data[album_row + 0x14..album_row + 0x19], [0x03, 0x18, 0, 0, 0x90]);
    assert!(db.artists.iter().any(|a| a.id == long && a.name == "Röyksopp"));
    assert!(db.albums.iter().any(|a| a.id == album && a.name == "Melody A.M. – Ü"));
}

/// Patch 20: a table's next free page that's corrupt (far past the end of the file, or not the
/// page its last page leads to) is refused instead of growing the file to it.
#[test]
fn a_corrupt_free_page_is_refused() {
    let orig = std::fs::read(ONE_SONG).unwrap();
    // History playlists: empty, so the first one needs a page. 11th directory entry.
    let candidate = 0x1c + 11 * 16 + 4;
    for page in [0x0004_0000u32, 25] {
        let mut data = orig.clone();
        data[candidate..candidate + 4].copy_from_slice(&page.to_le_bytes());
        let mut ed = PdbEditor::from_bytes(data.clone());
        assert!(matches!(ed.create_history_playlist("HISTORY 001"), Err(PdbError::Malformed(_))), "{page}");
        assert_eq!(ed.to_bytes().len(), orig.len());
    }
    let mut ed = PdbEditor::from_bytes(orig);
    assert!(ed.create_history_playlist("HISTORY 001").is_ok());
}
