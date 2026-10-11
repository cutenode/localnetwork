//! Edits on a drive whose two databases have drifted apart: what only export.pdb has is edited
//! there, and skipped in OneLibrary with a warning.

use crate::common;

use std::path::{Path, PathBuf};

use locallibrary::edit::{Edit, TrackFields};
use locallibrary::query::TrackSelector;
use locallibrary::{Library, paths, pdb};

/// Both databases, then a folder "Shows" holding "Only", a playlist "Solo" and the track "Bonus"
/// exported to export.pdb alone. Returns the root and Bonus's id.
fn drifted(dir: &Path) -> (PathBuf, u32) {
    let root = common::bigger_drive(dir, "Main");
    common::add_onelibrary(&root);
    let bonus = common::export_new_track(&root, "Extra", "Bonus");
    let pdb_path = root.join(paths::EXPORT_PDB);
    let mut ed = rekordbox_pdb::PdbEditor::from_file(&pdb_path).unwrap();
    let shows = ed.create_playlist("Shows", 0, true).unwrap();
    ed.create_playlist("Only", shows, false).unwrap();
    ed.create_playlist("Solo", 0, false).unwrap();
    ed.save(&pdb_path).unwrap();
    (root, bonus)
}

fn names(lib: &Library) -> Vec<String> {
    let mut v: Vec<String> = lib.playlists.iter().map(|p| lib.playlist_path(p.id)).collect();
    v.sort();
    v
}

#[test]
fn what_onelibrary_lacks_is_edited_in_export_pdb_and_skipped_there() {
    let dir = tempfile::tempdir().unwrap();
    let (root, bonus) = drifted(dir.path());
    let plus_before = names(&common::plus_library(&root));
    let ids = |ids: Vec<u32>| TrackSelector { ids, ..Default::default() };
    let edits = [
        Edit::RenamePlaylist { playlist: "Solo".into(), name: "Renamed".into() },
        Edit::MovePlaylist { playlist: "Renamed".into(), into: Some("Shows".into()), position: None },
        Edit::SetTracks {
            playlist: "Shows/Only".into(),
            tracks: ids(vec![1, 2]),
            sort: None,
            limit: None,
            create: false,
        },
        Edit::DeletePlaylist { playlist: "Shows/Renamed".into(), with_tracks: false },
        Edit::EditTracks {
            tracks: ids(vec![bonus]),
            fields: TrackFields { genre: Some("Techno".into()), ..Default::default() },
        },
    ];
    for edit in edits {
        let plan = common::apply(&root, std::slice::from_ref(&edit));
        let skipped: Vec<&String> = plan
            .warnings
            .iter()
            .filter(|w| w.starts_with("exportLibrary.db: ") && w.ends_with("; skipped there"))
            .collect();
        assert_eq!(skipped.len(), 1, "{edit:?}: {:?}", plan.warnings);
    }

    let lib = pdb::read_library(&root.join(paths::EXPORT_PDB)).unwrap();
    assert_eq!(names(&lib), ["Shows", "Shows/Only", "aab"]);
    assert_eq!(lib.find_playlist("Shows/Only").unwrap().track_ids, [1, 2]);
    assert_eq!(lib.track(bonus).unwrap().genre, "Techno");
    assert_eq!(names(&common::plus_library(&root)), plus_before, "OneLibrary is as it was");
    common::sqlcipher_reads(&root);
}

/// A CDJ-3000X saves its playlist edits to OneLibrary alone. An edit planned on export.pdb's
/// entries used to give OneLibrary export.pdb's entries plus the edit, dropping the player's; they
/// stay, after the edit's, with a warning.
#[test]
fn entries_only_onelibrary_has_survive_an_edit_of_the_playlist() {
    use locallibrary::onelibrary::OneLibrary;
    let dir = tempfile::tempdir().unwrap();
    let root = common::bigger_drive(dir.path(), "Main");
    common::add_onelibrary(&root);
    let ids = |ids: Vec<u32>| TrackSelector { ids, ..Default::default() };
    let add = |tracks: Vec<u32>| Edit::AddTracks {
        playlist: "Set".into(),
        tracks: ids(tracks),
        position: None,
        sort: None,
        allow_repeats: false,
    };
    common::apply(&root, &[Edit::CreatePlaylist { path: "Set".into(), folder: false }, add(vec![1, 2])]);
    let pdb = pdb::read_library(&root.join(paths::EXPORT_PDB)).unwrap();
    let file = |id: u32| pdb.track(id).unwrap().file_path.clone();
    // The player adds track 3, in OneLibrary.
    let mut one = OneLibrary::open_root(&root).unwrap();
    let plus = one.library().unwrap();
    let in_plus = plus.tracks.iter().find(|t| t.file_path == file(3)).unwrap().id;
    one.add_to_playlist(plus.find_playlist("Set").unwrap().id, in_plus).unwrap();
    std::fs::write(root.join(paths::LIBRARY_PLUS_DB), one.to_encrypted().unwrap()).unwrap();

    let plan = common::apply(&root, &[add(vec![4])]);
    assert!(plan.warnings.iter().any(|w| w.contains("it's made in its own")), "{:?}", plan.warnings);
    let pdb_after = pdb::read_library(&root.join(paths::EXPORT_PDB)).unwrap();
    assert_eq!(super::files(&pdb_after, "Set"), [file(1), file(2), file(4)]);
    assert_eq!(super::files(&common::plus_library(&root), "Set"), [file(1), file(2), file(4), file(3)]);

    let remove = Edit::RemoveTracks { playlist: "Set".into(), tracks: None, positions: vec![1] };
    common::apply(&root, &[remove]);
    let plus = common::plus_library(&root);
    assert_eq!(super::files(&plus, "Set"), [file(2), file(4), file(3)], "still there after another edit");
    common::sqlcipher_reads(&root);
}

/// An edit planned on export.pdb's entries is made in OneLibrary's own: entries a CDJ-3000X took
/// out there stay out, its order stays, and the edit's own changes land next to the entries they
/// follow.
#[test]
fn an_edit_is_made_in_onelibrarys_own_entries() {
    use locallibrary::onelibrary::OneLibrary;
    let dir = tempfile::tempdir().unwrap();
    let root = common::bigger_drive(dir.path(), "Main");
    common::add_onelibrary(&root);
    let ids = |ids: Vec<u32>| TrackSelector { ids, ..Default::default() };
    let add = |tracks: Vec<u32>, position: Option<usize>| Edit::AddTracks {
        playlist: "Set".into(),
        tracks: ids(tracks),
        position,
        sort: None,
        allow_repeats: false,
    };
    common::apply(&root, &[Edit::CreatePlaylist { path: "Set".into(), folder: false }, add(vec![1, 2, 3], None)]);
    let pdb = pdb::read_library(&root.join(paths::EXPORT_PDB)).unwrap();
    let file = |id: u32| pdb.track(id).unwrap().file_path.clone();
    // The player takes 2 out and puts 3 first, in OneLibrary.
    let mut one = OneLibrary::open_root(&root).unwrap();
    let plus = one.library().unwrap();
    let in_plus = |id: u32| plus.tracks.iter().find(|t| t.file_path == file(id)).unwrap().id;
    let set = plus.find_playlist("Set").unwrap().id;
    one.connection().execute("DELETE FROM playlist_content WHERE playlist_id = ?1", [set]).unwrap();
    for id in [3, 1] {
        one.add_to_playlist(set, in_plus(id)).unwrap();
    }
    std::fs::write(root.join(paths::LIBRARY_PLUS_DB), one.to_encrypted().unwrap()).unwrap();

    common::apply(&root, &[add(vec![4], None)]);
    let entries = |root: &Path| super::files(&common::plus_library(root), "Set");
    assert_eq!(super::files(&pdb::read_library(&root.join(paths::EXPORT_PDB)).unwrap(), "Set"), [1, 2, 3, 4].map(file));
    assert_eq!(entries(&root), [3, 4, 1].map(file), "4 follows 3, as in export.pdb; 2 stays out");
    common::apply(&root, &[add(vec![5], Some(1))]);
    assert_eq!(entries(&root), [5, 3, 4, 1].map(file));
    common::apply(&root, &[Edit::SortPlaylist { playlist: "Set".into(), by: "-title".into() }]);
    let sorted = super::files(&pdb::read_library(&root.join(paths::EXPORT_PDB)).unwrap(), "Set");
    let want: Vec<String> = sorted.iter().filter(|f| **f != file(2)).cloned().collect();
    assert_eq!(entries(&root), want, "a sort orders them as export.pdb's");
    common::sqlcipher_reads(&root);
}
