//! Converting into a drive that already has the database: what only that database holds (a
//! CDJ-3000X's history lists and playlist edits in OneLibrary, an older player's history lists in
//! export.pdb, hot cue bank lists, the player name) stays, unless rebuilding it is asked for, and
//! then the plan names what goes.

use crate::common;

use std::fs;
use std::path::{Path, PathBuf};

use locallibrary::convert::{self, ConvertOptions, Target};
use locallibrary::device::Device;
use locallibrary::onelibrary::OneLibrary;
use locallibrary::progress::Ctx;
use locallibrary::{Library, backup, paths, player};

fn opts(to: Target, replace: bool) -> ConvertOptions {
    ConvertOptions { to, replace }
}

/// The twelve-track drive with both databases after a set on mixed players: an older player
/// recorded "HISTORY 001" in export.pdb; a CDJ-3000X recorded "HISTORY 002", made the playlist
/// "Booth" (tracks 7 and 2) and a hot cue bank list in OneLibrary, which has also lost track 9.
/// The drive's player name is "BOOTH". Returns the drive and track 9's file.
fn after_a_set(dir: &Path) -> (PathBuf, String) {
    let root = common::bigger_drive(dir, "A");
    common::add_onelibrary(&root);
    common::add_history(&root, "HISTORY 001", &[3, 1, 4]);
    let mut d = Device::open_export(&root).unwrap();
    player::set(&mut d, Some("BOOTH"), Some(3)).unwrap();
    let mut one = OneLibrary::open_root(&root).unwrap();
    let history = one.create_history("HISTORY 002", 0, false).unwrap();
    for t in [5, 6] {
        one.add_to_history(history, t).unwrap();
    }
    let booth = one.create_playlist("Booth", 0, false).unwrap();
    for t in [7, 2] {
        one.add_to_playlist(booth, t).unwrap();
    }
    one.create_hot_cue_bank_list("Drops", 0, false).unwrap();
    let lost = one.library().unwrap().track(9).unwrap().file_path.clone();
    one.remove_tracks(&[9]).unwrap();
    fs::write(root.join(paths::LIBRARY_PLUS_DB), one.to_encrypted().unwrap()).unwrap();
    (root, lost)
}

fn history(lib: &Library) -> Vec<String> {
    let mut names: Vec<String> = lib.history.iter().filter(|h| !h.is_folder).map(|h| h.name.clone()).collect();
    names.sort();
    names
}

fn titles(lib: &Library, playlist: &str) -> Vec<String> {
    let p = lib.find_playlist(playlist).unwrap();
    p.track_ids.iter().map(|id| lib.track(*id).unwrap().title.clone()).collect()
}

/// The titles of export.pdb's tracks `ids` (OneLibrary's were the same before it lost track 9).
fn titles_of(root: &Path, ids: &[u32]) -> Vec<String> {
    let lib = Device::open_export(root).unwrap().library().unwrap();
    ids.iter().map(|id| lib.track(*id).unwrap().title.clone()).collect()
}

fn hot_cue_bank_lists(root: &Path) -> Vec<String> {
    OneLibrary::open_root(root).unwrap().hot_cue_bank_lists().unwrap().into_iter().map(|l| l.name).collect()
}

fn player_name(root: &Path) -> Option<String> {
    player::db_identity(root).and_then(|p| p.name)
}

#[test]
fn converting_into_onelibrary_adds_what_it_lacks_and_keeps_what_only_it_has() {
    let dir = tempfile::tempdir().unwrap();
    let (root, lost) = after_a_set(dir.path());
    let booth = titles_of(&root, &[7, 2]);
    let mut d = Device::open_export(&root).unwrap();
    let to = opts(Target::OneLibrary, false);

    let plan = convert::plan(&d, &to, &Ctx::silent()).unwrap();
    assert!(plan.adds && !plan.replaces);
    // Track 9, the playlist it was in, and the older player's history list.
    assert_eq!((plan.tracks, plan.playlists, plan.history_playlists), (1, 1, 1), "{plan:?}");
    let report = convert::apply(&plan, &mut d, &to, &Ctx::silent()).unwrap();
    assert_eq!((report.tracks, report.playlists, report.history_playlists), (1, 1, 1));

    let plus = common::plus_library(&root);
    assert_eq!(plus.tracks.len(), 12);
    assert!(plus.tracks.iter().any(|t| t.file_path == lost));
    assert_eq!(history(&plus), ["HISTORY 001", "HISTORY 002"]);
    assert_eq!(titles(&plus, "Booth"), booth);
    assert_eq!(hot_cue_bank_lists(&root), ["Drops"]);
    assert_eq!(player_name(&root).as_deref(), Some("BOOTH"));
    assert!(d.info().drift.unwrap().only_in_device_library.is_empty());
    common::sqlcipher_reads(&root);

    // Nothing is left to add.
    let again = convert::plan(&d, &to, &Ctx::silent()).unwrap();
    assert_eq!((again.tracks, again.playlists, again.history_playlists, again.tagged_tracks), (0, 0, 0, 0));

    // Undo takes the additions back.
    backup::restore(&mut d, report.backup.as_deref(), false).unwrap();
    let plus = common::plus_library(&root);
    assert_eq!((plus.tracks.len(), history(&plus)), (11, vec!["HISTORY 002".to_string()]));
}

#[test]
fn converting_into_export_pdb_adds_what_it_lacks_and_keeps_what_only_it_has() {
    let dir = tempfile::tempdir().unwrap();
    let (root, _) = after_a_set(dir.path());
    let booth = titles_of(&root, &[7, 2]);
    let mut d = Device::open_export(&root).unwrap();
    let ids = |d: &Device| -> Vec<(u32, String)> {
        d.library().unwrap().tracks.into_iter().map(|t| (t.id, t.file_path)).collect()
    };
    let before = ids(&d);
    let to = opts(Target::DeviceLibrary, false);

    let plan = convert::plan(&d, &to, &Ctx::silent()).unwrap();
    assert!(plan.adds && !plan.replaces);
    // "Booth" and the CDJ-3000X's history list; export.pdb has every track already.
    assert_eq!((plan.tracks, plan.playlists, plan.history_playlists), (0, 1, 1), "{plan:?}");
    let report = convert::apply(&plan, &mut d, &to, &Ctx::silent()).unwrap();
    assert_eq!((report.tracks, report.playlists, report.history_playlists), (0, 1, 1));

    let pdb = d.library().unwrap();
    assert_eq!(ids(&d), before, "its tracks keep their ids");
    assert_eq!(history(&pdb), ["HISTORY 001", "HISTORY 002"]);
    assert_eq!(titles(&pdb, "Booth"), booth);
    let again = convert::plan(&d, &to, &Ctx::silent()).unwrap();
    assert_eq!((again.tracks, again.playlists, again.history_playlists), (0, 0, 0));
}

#[test]
fn rebuilding_names_what_only_the_database_had() {
    let dir = tempfile::tempdir().unwrap();
    let (root, _) = after_a_set(dir.path());
    let mut d = Device::open_export(&root).unwrap();

    let to = opts(Target::DeviceLibrary, true);
    let plan = convert::plan(&d, &to, &Ctx::silent()).unwrap();
    let gone = plan.warnings.iter().find(|w| w.contains("HISTORY 001")).unwrap_or_else(|| panic!("{plan:?}"));
    assert!(!gone.contains("HISTORY 002"), "{gone}");

    let to = opts(Target::OneLibrary, true);
    let plan = convert::plan(&d, &to, &Ctx::silent()).unwrap();
    assert!(plan.replaces && !plan.adds);
    let gone = plan.warnings.iter().find(|w| w.contains("HISTORY 002")).unwrap_or_else(|| panic!("{plan:?}"));
    assert!(gone.contains("Booth") && !gone.contains("HISTORY 001"), "{gone}");
    assert!(plan.warnings.iter().any(|w| w.contains("1 hot cue bank list(s)")), "{:?}", plan.warnings);
    convert::apply(&plan, &mut d, &to, &Ctx::silent()).unwrap();
    // The player name belongs to the drive, not the library.
    assert_eq!(player_name(&root).as_deref(), Some("BOOTH"));
    common::sqlcipher_reads(&root);
}

#[test]
fn a_database_that_cant_be_read_is_rebuilt_only_when_asked() {
    let dir = tempfile::tempdir().unwrap();
    let root = common::bigger_drive(dir.path(), "A");
    common::add_onelibrary(&root);
    fs::write(root.join(paths::LIBRARY_PLUS_DB), b"not a database").unwrap();
    let d = Device::open_export(&root).unwrap();
    let err = convert::plan(&d, &opts(Target::OneLibrary, false), &Ctx::silent()).unwrap_err();
    assert!(err.to_string().contains("exportLibrary.db can't be read"), "{err}");
    let retry = err.detail().actions.first().map(|a| serde_json::Value::Object(a.params.clone()));
    assert_eq!(retry, Some(serde_json::json!({ "replace": true })));
}

/// Adding to a database writes a new copy of it beside the old one, so the plan counts the space
/// of what it holds, not only what's added.
#[test]
fn adding_counts_the_space_of_the_database_it_adds_to() {
    let dir = tempfile::tempdir().unwrap();
    let root = common::bigger_drive(dir.path(), "A");
    common::add_onelibrary(&root);
    let one = OneLibrary::open_root(&root).unwrap();
    // Comments OneLibrary keeps (export.pdb couldn't hold them) make it far bigger than export.pdb.
    let comment = "x".repeat(1 << 18);
    one.connection().execute("UPDATE content SET djComment = ?1", [&comment]).unwrap();
    fs::write(root.join(paths::LIBRARY_PLUS_DB), one.to_encrypted().unwrap()).unwrap();
    let size = fs::metadata(root.join(paths::LIBRARY_PLUS_DB)).unwrap().len();
    let d = Device::open_export(&root).unwrap();
    let plan = convert::plan(&d, &opts(Target::OneLibrary, false), &Ctx::silent()).unwrap();
    assert!(plan.adds);
    assert!(plan.needed_bytes >= 2 * size, "{} for a {size}-byte database and its backup", plan.needed_bytes);
}
