//! Edits on a drive whose two databases have drifted apart (rekordbox exported more to
//! export.pdb alone, and a player changed a playlist in OneLibrary alone): export.pdb ends up as
//! the same edits leave a drive without OneLibrary, OneLibrary gains nothing it lacked and keeps
//! no track the edits removed, and what it lacks is skipped there with a warning.

use crate::{common, drawn};

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use hegel::generators as gs;
use hegel::{HealthCheck, TestCase};
use locallibrary::device::Device;
use locallibrary::edit::{self, Edit, EditOptions, TrackFields};
use locallibrary::onelibrary::OneLibrary;
use locallibrary::progress::Ctx;
use locallibrary::query::TrackSelector;
use locallibrary::{Library, paths, pdb};
use rekordbox_pdb::PdbEditor;

/// The twelve-track export with a history list, OneLibrary beside it if `plus`, then exported to
/// export.pdb alone: the track "Bonus", a folder "Shows" holding "Only", a playlist "Solo" and the
/// history list "HISTORY 002".
fn drifted(dir: &Path, plus: bool) -> PathBuf {
    let root = common::bigger_drive(dir, if plus { "Both" } else { "Pdb" });
    common::add_history(&root, "HISTORY 001", &[1, 2]);
    if plus {
        common::add_onelibrary(&root);
    }
    common::export_new_track(&root, "Extra", "Bonus");
    let pdb_path = root.join(paths::EXPORT_PDB);
    let mut ed = PdbEditor::from_file(&pdb_path).unwrap();
    let shows = ed.create_playlist("Shows", 0, true).unwrap();
    ed.create_playlist("Only", shows, false).unwrap();
    ed.create_playlist("Solo", 0, false).unwrap();
    let h = ed.create_history_playlist("HISTORY 002").unwrap();
    ed.add_to_history(h, 3).unwrap();
    ed.save(&pdb_path).unwrap();
    root
}

/// A player adds `file` to the end of OneLibrary's `playlist`.
fn player_adds(root: &Path, playlist: &str, file: &str) {
    let mut one = OneLibrary::open_root(root).unwrap();
    let lib = one.library().unwrap();
    let id = lib.tracks.iter().find(|t| t.file_path == file).unwrap().id;
    one.add_to_playlist(lib.find_playlist(playlist).unwrap().id, id).unwrap();
    fs::write(root.join(paths::LIBRARY_PLUS_DB), one.to_encrypted().unwrap()).unwrap();
}

/// Tracks (file, title, genre) and playlists (path, entries' files).
type Shape = (Vec<(String, String, String)>, Vec<(String, Vec<String>)>);

/// What the edits can change, by file and path, comparable across drives.
fn shape(lib: &Library) -> Shape {
    let file = |id: &u32| paths::key(&lib.track(*id).unwrap().file_path);
    let mut tracks: Vec<_> =
        lib.tracks.iter().map(|t| (paths::key(&t.file_path), t.title.clone(), t.genre.clone())).collect();
    tracks.sort();
    let mut lists: Vec<_> = lib
        .playlists
        .iter()
        .chain(&lib.history)
        .map(|p| (lib.playlist_path(p.id), p.track_ids.iter().map(file).collect()))
        .collect();
    lists.sort();
    (tracks, lists)
}

#[hegel::test_helper]
fn draw_edit(tc: &TestCase, kind: u8, files: &[String], bonus: &str) -> Edit {
    let some = |tc: &TestCase| {
        let mut picked = drawn::some(tc, files);
        if picked.is_empty() || tc.draw(gs::booleans()) {
            picked.push(bonus.to_string());
        }
        TrackSelector::paths(picked)
    };
    match kind {
        0 => Edit::RenamePlaylist { playlist: "Solo".into(), name: "Renamed".into() },
        1 => Edit::MovePlaylist { playlist: "Solo".into(), into: Some("Shows".into()), position: None },
        2 => {
            Edit::SetTracks { playlist: "Shows/Only".into(), tracks: some(tc), sort: None, limit: None, create: false }
        }
        3 => Edit::DeletePlaylist { playlist: "Shows".into(), with_tracks: tc.draw(gs::booleans()) },
        4 => Edit::EditTracks {
            tracks: some(tc),
            fields: TrackFields { genre: Some("Techno".into()), ..Default::default() },
        },
        5 => Edit::AddTracks {
            playlist: "aab".into(),
            tracks: some(tc),
            position: tc.draw(gs::optional(gs::integers::<usize>().min_value(1).max_value(14))),
            sort: None,
            allow_repeats: tc.draw(gs::booleans()),
        },
        6 => Edit::RemoveTracks {
            playlist: "aab".into(),
            tracks: None,
            positions: vec![tc.draw(gs::integers::<usize>().min_value(1).max_value(12))],
        },
        7 => Edit::MoveTracks {
            playlist: "aab".into(),
            positions: vec![tc.draw(gs::integers::<usize>().min_value(1).max_value(12))],
            to: tc.draw(gs::integers::<usize>().min_value(1).max_value(12)),
        },
        8 => Edit::DeleteHistory { names: vec!["HISTORY 002".into()], all: false },
        9 => Edit::DeleteTracks {
            tracks: TrackSelector::paths([bonus.to_string()]),
            keep_files: tc.draw(gs::booleans()),
        },
        _ => Edit::SortPlaylist { playlist: "aab".into(), by: "-title".into() },
    }
}

#[hegel::test(test_cases = 3, suppress_health_check = [HealthCheck::TooSlow])]
fn edits_on_drifted_databases_change_export_pdb_alike_and_onelibrary_only_where_it_can(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let (alone, both) = (drifted(dir.path(), false), drifted(dir.path(), true));
    let lib = pdb::read_library(&both.join(paths::EXPORT_PDB)).unwrap();
    let files: Vec<String> = lib.tracks.iter().map(|t| t.file_path.clone()).collect();
    let bonus = lib.tracks.iter().find(|t| t.title == "Bonus").unwrap().file_path.clone();
    // The player puts one more entry in aab, in OneLibrary: one of the twelve tracks it holds (not
    // Bonus, which is in export.pdb alone).
    if tc.draw(gs::booleans()) {
        player_adds(&both, "aab", &tc.draw(gs::sampled_from(&files[..12])));
    }
    let plus_before = common::plus_library(&both);
    let plus_files = |l: &Library| -> BTreeSet<String> { l.tracks.iter().map(|t| paths::key(&t.file_path)).collect() };

    // Every kind of edit, one at a time, in a drawn order.
    for kind in tc.draw(gs::permutations((0..=10).collect::<Vec<u8>>())) {
        let edits = vec![draw_edit(&tc, kind, &files[..12], &bonus)];
        let plan = |root: &Path| {
            let d = Device::open_export(root).unwrap();
            edit::plan(&d, &edits, &EditOptions::default(), &Ctx::silent()).map(|p| (d, p))
        };
        let (a, b) = (plan(&alone), plan(&both));
        assert_eq!(a.is_ok(), b.is_ok(), "{edits:?}: {:?} / {:?}", a.as_ref().err(), b.as_ref().err());
        let (Ok((mut da, pa)), Ok((mut db, pb))) = (a, b) else { continue };
        edit::apply(&pa, &mut da, &Ctx::silent()).unwrap();
        edit::apply(&pb, &mut db, &Ctx::silent()).unwrap();
        tc.note(&format!("{edits:?}: {:?}", pb.warnings));
        let read = |root: &Path| pdb::read_library(&root.join(paths::EXPORT_PDB)).unwrap();
        assert_eq!(shape(&read(&both)), shape(&read(&alone)), "export.pdb after {edits:?}");
        let plus = common::plus_library(&both);
        assert!(plus_files(&plus).is_subset(&plus_files(&plus_before)), "OneLibrary gains no track");
        let pdb_files: BTreeSet<String> = read(&both).tracks.iter().map(|t| paths::key(&t.file_path)).collect();
        let removed: BTreeSet<String> = plus_files(&plus_before).difference(&pdb_files).cloned().collect();
        assert!(plus_files(&plus).is_disjoint(&removed), "a track the edits removed stays in OneLibrary");
    }
    common::sqlcipher_reads(&both);
}
