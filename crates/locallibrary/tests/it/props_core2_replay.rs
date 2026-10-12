//! Replaying edits after rekordbox exported again without the playlist an edit named: the edit
//! is skipped, never made to another playlist of that name in some folder, which a lookup by
//! name alone would find once the one the journal recorded is gone.

use crate::common;

use std::fs;

use hegel::generators as gs;
use hegel::{HealthCheck, TestCase};
use locallibrary::edit::Edit;
use locallibrary::paths;
use rekordbox_pdb::PdbEditor;

#[hegel::test(test_cases = 6, suppress_health_check = [HealthCheck::TooSlow])]
#[hegel::explicit_test_case(kind = 0u8, name = "Renamed Crate".to_string())]
#[hegel::explicit_test_case(kind = 1u8, name = "Renamed Crate".to_string())]
#[hegel::explicit_test_case(kind = 2u8, name = "Renamed Crate".to_string())]
fn a_replayed_edit_never_lands_on_another_playlist_of_its_name(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let root = common::bigger_drive(dir.path(), "A");
    let pdb = root.join(paths::EXPORT_PDB);
    // As rekordbox exported it: a playlist Crate at the top, and one in folder F.
    let mut ed = PdbEditor::from_file(&pdb).unwrap();
    let top = ed.create_playlist("Crate", 0, false).unwrap();
    let folder = ed.create_playlist("F", 0, true).unwrap();
    ed.create_playlist("Crate", folder, false).unwrap();
    ed.create_playlist("Elsewhere", 0, true).unwrap();
    ed.save(&pdb).unwrap();
    let exported = fs::read(&pdb).unwrap();

    let kind = tc.draw(gs::integers::<u8>().max_value(2));
    let name = tc.draw(gs::sampled_from(vec!["Renamed Crate".to_string(), "Ünï".into()]));
    let edit = match kind {
        0 => Edit::RenamePlaylist { playlist: "Crate".into(), name: name.clone() },
        1 => Edit::DeletePlaylist { playlist: "Crate".into(), with_tracks: false },
        _ => Edit::MovePlaylist { playlist: "Crate".into(), into: Some("Elsewhere".into()), position: None },
    };
    common::apply(&root, &[edit]);

    // rekordbox exports again, without the top-level Crate.
    let mut ed = PdbEditor::from_bytes(exported);
    ed.remove_playlists(&[top]).unwrap();
    fs::write(&pdb, ed.to_bytes()).unwrap();
    let (_, lib) = common::replay(&root);
    let paths: Vec<String> = lib.playlists.iter().map(|p| lib.playlist_path(p.id)).collect();
    assert!(paths.contains(&"F/Crate".to_string()), "{paths:?}");
    assert!(!paths.iter().any(|p| p.ends_with(&name) || p == "Elsewhere/Crate"), "{paths:?}");
}
