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
