mod files;

use crate::common;

use std::fs;

use locallibrary::device::{Device, LineageOp};
use locallibrary::edit::{self, Edit, EditOptions};
use locallibrary::pack::{self, PackOptions};
use locallibrary::progress::Ctx;
use locallibrary::query::TrackSelector;
use locallibrary::{Library, paths, pdb};

fn lib(root: &std::path::Path) -> Library {
    pdb::read_library(&root.join(paths::EXPORT_PDB)).unwrap()
}

/// A source with two playlists in folders: "Sets/Friday" (3 tracks) and "Other" (2 tracks).
fn source(dir: &std::path::Path) -> std::path::PathBuf {
    let root = common::bigger_drive(dir, "Main");
    common::add_onelibrary(&root);
    common::add_history(&root, "HISTORY 001", &[1]);
    let mut d = Device::open_export(&root).unwrap();
    let l = lib(&root);
    let ids = |r: std::ops::Range<usize>| TrackSelector {
        ids: l.tracks[r].iter().map(|t| t.id).collect(),
        ..Default::default()
    };
    let edits = vec![
        Edit::SetTracks { playlist: "Sets/Friday".into(), tracks: ids(0..3), sort: None, limit: None, create: true },
        Edit::SetTracks { playlist: "Other".into(), tracks: ids(3..5), sort: None, limit: None, create: true },
    ];
    let plan = edit::plan(&d, &edits, &EditOptions::default(), &Ctx::silent()).unwrap();
    edit::apply(&plan, &mut d, &Ctx::silent()).unwrap();
    root
}

#[test]
fn packs_chosen_playlists_and_updates_incrementally() {
    let dir = tempfile::tempdir().unwrap();
    let src_root = source(dir.path());
    let club_root = dir.path().join("Club");
    fs::create_dir_all(&club_root).unwrap();
    let mut src = Device::open_export(&src_root).unwrap();
    src.adopt(None).unwrap();
    let mut club = Device::open(&club_root).unwrap();
    let opts = PackOptions { playlists: vec!["Sets".into()], ..Default::default() };
    let plan = pack::plan(&src, &club, &opts, &Ctx::silent()).unwrap();
    assert_eq!(plan.tracks, 3);
    assert!(!plan.replaces_library);
    pack::apply(&plan, &src, &mut club, None, &Ctx::silent()).unwrap();

    common::sqlcipher_reads(&club_root);
    for l in [lib(&club_root), common::plus_library(&club_root)] {
        assert_eq!(l.tracks.len(), 3);
        assert_eq!(l.playlists.iter().map(|p| l.playlist_path(p.id)).collect::<Vec<_>>(), ["Sets", "Sets/Friday"]);
        assert!(l.history.is_empty(), "history stays behind unless asked for");
        for t in &l.tracks {
            assert!(paths::to_fs(&club_root, &paths::key(&t.file_path)).is_file());
        }
    }
    let club = Device::open(&club_root).unwrap();
    assert_eq!(club.state().unwrap().lineage.last().unwrap().op, LineageOp::Pack);
    assert_eq!(pack::record(&club).unwrap().unwrap().playlists, ["Sets"]);

    // Again with the remembered selection: nothing to copy.
    let plan = pack::plan(&src, &club, &PackOptions::default(), &Ctx::silent()).unwrap();
    assert!(plan.copies.iter().all(|c| c.path.starts_with("PIONEER/rekordbox")), "{:?}", plan.copies);
    assert!(plan.deletes.is_empty());

    // Switch to the other playlist: its files come, Friday's go.
    let mut club = Device::open(&club_root).unwrap();
    let opts = PackOptions { playlists: vec!["Other".into()], history: true, ..Default::default() };
    let plan = pack::plan(&src, &club, &opts, &Ctx::silent()).unwrap();
    assert!(!plan.deletes.is_empty());
    pack::apply(&plan, &src, &mut club, None, &Ctx::silent()).unwrap();
    let l = lib(&club_root);
    assert_eq!(l.tracks.len(), 3, "Other's two tracks and the history's one");
    assert_eq!(l.history.len(), 1);
    assert_eq!(common::library_files(&club_root).iter().filter(|(k, _)| k.starts_with("Contents/")).count(), 3);
    // The source is untouched.
    assert_eq!(lib(&src_root).tracks.len(), 12);
}

#[test]
fn packing_over_another_library_says_so() {
    let dir = tempfile::tempdir().unwrap();
    let src_root = source(dir.path());
    let other = common::demo_drive(dir.path(), "Other");
    let src = Device::open_export(&src_root).unwrap();
    let opts = PackOptions { playlists: vec!["Sets/Friday".into()], ..Default::default() };
    let plan = pack::plan(&src, &Device::open(&other).unwrap(), &opts, &Ctx::silent()).unwrap();
    assert!(plan.replaces_library);
    assert!(plan.warnings.iter().any(|w| w.contains("replaces")));
    assert!(pack::plan(&src, &src, &opts, &Ctx::silent()).is_err());
    let none = PackOptions::default();
    assert!(pack::plan(&src, &Device::open(&other).unwrap(), &none, &Ctx::silent()).is_err());
}

/// Regression: packing again replaced the history lists and memory cues players recorded on the
/// club drive, without a backup. They're now saved in the store first (refused without one), the
/// replaced databases are backed up, and compare and check know a pack from its source.
#[test]
fn packing_again_saves_what_players_recorded_first() {
    use locallibrary::check::{self, CheckOptions, Level};
    use locallibrary::compare::{self, Relation};
    use locallibrary::store::Store;
    use locallibrary::{Error, backup, plays};

    let dir = tempfile::tempdir().unwrap();
    let src_root = source(dir.path());
    let club_root = dir.path().join("Club");
    fs::create_dir_all(&club_root).unwrap();
    let mut src = Device::open_export(&src_root).unwrap();
    src.adopt(None).unwrap();
    let opts = PackOptions { playlists: vec!["Other".into()], ..Default::default() };
    let mut club = Device::open(&club_root).unwrap();
    let plan = pack::plan(&src, &club, &opts, &Ctx::silent()).unwrap();
    assert!(plan.unsaved.is_none());
    pack::apply(&plan, &src, &mut club, None, &Ctx::silent()).unwrap();

    // The drive is a pack of Main: not the same library, but up to date.
    let club = Device::open(&club_root).unwrap();
    let c = compare::compare_devices(&src, &club, &Ctx::silent()).unwrap();
    assert_eq!(c.relation, Relation::Unrelated);
    assert!(c.evidence.iter().any(|e| e.detail.contains("packed from")), "{:?}", c.evidence);
    let report = check::check(&[src, club], None, &CheckOptions::default(), &Ctx::silent()).unwrap();
    let item = report.items.iter().find(|i| i.drive == "Club" && i.check == "same_library").unwrap();
    assert_eq!(item.level, Level::Ok, "{}", item.message);
    assert!(item.message.contains("up to date"), "{}", item.message);

    // A set: the player records a history list.
    let ids: Vec<u32> = lib(&club_root).tracks.iter().map(|t| t.id).collect();
    common::add_history(&club_root, "HISTORY 001", &ids);
    let pdb_after_set = fs::read(club_root.join(paths::EXPORT_PDB)).unwrap();

    // Without a store, packing again is refused and changes nothing.
    let src = Device::open_export(&src_root).unwrap();
    let mut club = Device::open(&club_root).unwrap();
    let plan = pack::plan(&src, &club, &PackOptions::default(), &Ctx::silent()).unwrap();
    let unsaved = plan.unsaved.clone().expect("the set's history list is at risk");
    assert_eq!(unsaved.history_lists, ["HISTORY 001"]);
    assert!(unsaved.changed);
    let err = pack::apply(&plan, &src, &mut club, None, &Ctx::silent()).unwrap_err();
    assert!(matches!(err.inner(), Error::Conflict(_)), "{err}");
    assert_eq!(fs::read(club_root.join(paths::EXPORT_PDB)).unwrap(), pdb_after_set);

    // With one, the drive is saved first and its databases backed up.
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    let report = pack::apply(&plan, &src, &mut club, Some(&store), &Ctx::silent()).unwrap();
    assert!(report.saved.is_some());
    let archive = plays::archive(&store, None, &Ctx::silent()).unwrap();
    assert!(archive.lists.iter().any(|a| a.list.name == "HISTORY 001"));
    assert!(lib(&club_root).history.is_empty(), "the pack itself has no history lists");
    let club = Device::open(&club_root).unwrap();
    let saved = backup::dir(&club, report.backup.as_deref().unwrap());
    assert_eq!(fs::read(saved.join(paths::EXPORT_PDB)).unwrap(), pdb_after_set);

    // Main moves on: check says the pack is out of date.
    common::export_new_track(&src_root, "New Artist", "Fresh Track");
    let src = Device::open_export(&src_root).unwrap();
    let report = check::check(&[src, club], None, &CheckOptions::default(), &Ctx::silent()).unwrap();
    let item = report.items.iter().find(|i| i.drive == "Club" && i.check == "same_library").unwrap();
    assert_eq!(item.level, Level::Warning, "{}", item.message);
    assert!(item.message.contains("locallibrary pack Main Club"), "{}", item.message);
}

#[test]
fn packing_again_keeps_what_the_drive_keeps_for_itself_and_forgets_its_edits() {
    let dir = tempfile::tempdir().unwrap();
    let src_root = source(dir.path());
    let club_root = dir.path().join("Club");
    fs::create_dir_all(&club_root).unwrap();
    let mut src = Device::open_export(&src_root).unwrap();
    src.adopt(None).unwrap();
    let mut club = Device::open(&club_root).unwrap();
    let opts = PackOptions { playlists: vec!["Sets".into()], ..Default::default() };
    pack::apply(&pack::plan(&src, &club, &opts, &Ctx::silent()).unwrap(), &src, &mut club, None, &Ctx::silent())
        .unwrap();
    // A player's track filter and rekordbox's sync records turn up; a playlist is made on it.
    let kept = ["PIONEER/rekordbox/RBFLTR.DAT", paths::PLAYLIST_SYNC, "PIONEER/LOG/0001.LOG"];
    for key in kept {
        let p = paths::to_fs(&club_root, key);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, b"the drive's own").unwrap();
    }
    common::apply(&club_root, &[Edit::CreatePlaylist { path: "Mine".into(), folder: false }]);

    let store = locallibrary::Store::open_or_init(dir.path().join("store")).unwrap();
    let mut club = Device::open(&club_root).unwrap();
    let plan = pack::plan(&src, &club, &PackOptions::default(), &Ctx::silent()).unwrap();
    assert!(plan.deletes.is_empty(), "{:?}", plan.deletes.iter().map(|d| &d.path).collect::<Vec<_>>());
    assert!(plan.warnings.iter().any(|w| w.contains("1 recorded edit(s)")), "{:?}", plan.warnings);
    pack::apply(&plan, &src, &mut club, Some(&store), &Ctx::silent()).unwrap();
    for key in kept {
        assert!(paths::to_fs(&club_root, key).is_file(), "{key}");
    }
    assert!(edit::journal::list(&Device::open(&club_root).unwrap()).unwrap().is_empty());
}

#[test]
fn a_cancel_once_the_pack_is_copied_finishes_recording_it() {
    let dir = tempfile::tempdir().unwrap();
    let src_root = source(dir.path());
    let club_root = dir.path().join("Club");
    fs::create_dir_all(&club_root).unwrap();
    let mut src = Device::open_export(&src_root).unwrap();
    src.adopt(None).unwrap();
    let mut club = Device::open(&club_root).unwrap();
    let opts = PackOptions { playlists: vec!["Sets".into()], ..Default::default() };
    let plan = pack::plan(&src, &club, &opts, &Ctx::silent()).unwrap();
    let token = locallibrary::CancelToken::new();
    let progress = |e: &locallibrary::Event| {
        if matches!(e, locallibrary::Event::PhaseEnd { phase: locallibrary::progress::Phase::Copy }) {
            token.cancel();
        }
    };
    pack::apply(&plan, &src, &mut club, None, &Ctx::new(&progress, token.clone())).unwrap();
    assert!(token.is_cancelled());
    let club = Device::open(&club_root).unwrap();
    assert!(club.state().unwrap().pending.is_none());
    assert!(pack::record(&club).unwrap().is_some(), "the pack record is written");
}

/// Regression: pack wrote its databases one after another, each staged and flushed just before
/// its rename, with nothing for an exit at once (a second Ctrl-C) to wait for: it could leave the
/// drive with databases from two libraries. They're all staged first, and an exit waits for them
/// to go in.
#[test]
fn pack_stages_the_databases_and_an_exit_waits_for_the_swap() {
    let dir = tempfile::tempdir().unwrap();
    let src_root = source(dir.path());
    let club_root = dir.path().join("Club");
    fs::create_dir_all(&club_root).unwrap();
    let mut src = Device::open_export(&src_root).unwrap();
    src.adopt(None).unwrap();
    let mut club = Device::open(&club_root).unwrap();
    let opts = PackOptions { playlists: vec!["Sets".into()], ..Default::default() };
    let plan = pack::plan(&src, &club, &opts, &Ctx::silent()).unwrap();
    let databases: Vec<&str> = [paths::EXPORT_PDB, paths::EXPORT_EXT_PDB, paths::LIBRARY_PLUS_DB]
        .into_iter()
        .filter(|k| paths::to_fs(&src_root, k).is_file())
        .collect();
    let folder = paths::to_fs(&club_root, paths::EXPORT_PDB).parent().unwrap().to_path_buf();
    let staged = || {
        fs::read_dir(&folder).map_or(0, |rd| {
            rd.flatten().filter(|e| e.file_name().to_string_lossy().starts_with(paths::TEMP_PREFIX)).count()
        })
    };
    std::thread::scope(|s| {
        let exiting = locallibrary::progress::wait_for_critical();
        let packing = s.spawn(|| pack::apply(&plan, &src, &mut club, None, &Ctx::silent()));
        let none_in = || databases.iter().all(|k| !paths::to_fs(&club_root, k).exists());
        while staged() < databases.len() {
            assert!(none_in() && !packing.is_finished(), "a database went in while an exit was waiting");
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(none_in(), "a database went in while an exit was waiting");
        drop(exiting);
        packing.join().unwrap().unwrap();
    });
    assert_eq!(staged(), 0);
    assert!(databases.iter().all(|k| paths::to_fs(&club_root, k).is_file()));
}

/// Regression: packing put the source's player name and colour on the target, the way players
/// tell sticks apart. The target keeps its own.
#[test]
fn packing_keeps_the_targets_player_name() {
    let dir = tempfile::tempdir().unwrap();
    let src_root = source(dir.path());
    let club_root = dir.path().join("Club");
    fs::create_dir_all(&club_root).unwrap();
    let mut src = Device::open_export(&src_root).unwrap();
    src.adopt(None).unwrap();
    locallibrary::player::set(&mut src, Some("MAIN"), Some(3)).unwrap();
    let mut club = Device::open(&club_root).unwrap();
    let opts = PackOptions { playlists: vec!["Sets".into()], ..Default::default() };
    let plan = pack::plan(&src, &club, &opts, &Ctx::silent()).unwrap();
    pack::apply(&plan, &src, &mut club, None, &Ctx::silent()).unwrap();
    locallibrary::player::set(&mut club, Some("CLUB"), Some(6)).unwrap();

    let opts = PackOptions { playlists: vec!["Other".into()], ..Default::default() };
    let plan = pack::plan(&src, &club, &opts, &Ctx::silent()).unwrap();
    pack::apply(&plan, &src, &mut club, None, &Ctx::silent()).unwrap();
    let id = locallibrary::player::db_identity(&club_root).unwrap();
    assert_eq!((id.name.as_deref(), id.colour), (Some("CLUB"), Some(6)));
    assert_eq!(common::plus_library(&club_root).tracks.len(), 2, "the library is the new pack's");
}

/// Regression: pack's space check left out the backup it takes of the target, so a stick with room
/// for the copies but not the backup failed part way instead of in the plan.
#[test]
fn packs_space_check_counts_the_backup() {
    let dir = tempfile::tempdir().unwrap();
    let src_root = source(dir.path());
    let club_root = dir.path().join("Club");
    fs::create_dir_all(&club_root).unwrap();
    let mut src = Device::open_export(&src_root).unwrap();
    src.adopt(None).unwrap();
    let mut club = Device::open(&club_root).unwrap();
    let opts = PackOptions { playlists: vec!["Sets".into()], ..Default::default() };
    let first = pack::plan(&src, &club, &opts, &Ctx::silent()).unwrap();
    pack::apply(&first, &src, &mut club, None, &Ctx::silent()).unwrap();

    // Packed again with the same selection: the new databases are the size of the ones on the
    // stick, and the backup copies those too.
    let again = pack::plan(&src, &club, &opts, &Ctx::silent()).unwrap();
    let size = |key: &str| fs::metadata(paths::to_fs(&club_root, key)).map(|m| m.len()).unwrap_or(0);
    let databases = size(paths::EXPORT_PDB) + size(paths::LIBRARY_PLUS_DB);
    assert!(
        again.needed_bytes >= again.copy_bytes + 2 * databases,
        "{} needed, {} copied, {databases} in databases",
        again.needed_bytes,
        again.copy_bytes
    );
}

/// A target whose OneLibrary database can't be read may hold history lists a CDJ-3000X recorded
/// there, which the check for unsaved history can't see: packing over it saves the drive in the
/// store first (refused without one), as it does history lists it can read.
#[test]
fn a_onelibrary_that_cant_be_read_is_saved_before_packing_over_it() {
    use locallibrary::Error;
    let dir = tempfile::tempdir().unwrap();
    let src_root = source(dir.path());
    let mut src = Device::open_export(&src_root).unwrap();
    src.adopt(None).unwrap();
    let club_root = common::bigger_drive(dir.path(), "Club");
    common::add_onelibrary(&club_root);
    fs::write(club_root.join(paths::LIBRARY_PLUS_DB), b"not a database").unwrap();
    let mut club = Device::open(&club_root).unwrap();
    let opts = PackOptions { playlists: vec!["Other".into()], ..Default::default() };
    let plan = pack::plan(&src, &club, &opts, &Ctx::silent()).unwrap();
    let unsaved = plan.unsaved.clone().expect("what OneLibrary holds can't be checked");
    assert!(unsaved.history_lists.is_empty() && unsaved.describe().contains("exportLibrary.db"), "{unsaved:?}");
    assert!(plan.warnings.iter().any(|w| w.contains("an exportLibrary.db that can't be read")), "{:?}", plan.warnings);
    let err = pack::apply(&plan, &src, &mut club, None, &Ctx::silent()).unwrap_err();
    assert!(matches!(err.inner(), Error::Conflict(_)), "{err}");
}

/// A source whose OneLibrary database can't be read used to pack with export.pdb alone, taking
/// the target's exportLibrary.db away with only a warning, so players that read only OneLibrary
/// couldn't browse the packed drive. It's refused, naming them, and the target is left as it was.
#[test]
fn a_source_whose_onelibrary_cant_be_read_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let src_root = source(dir.path());
    let club_root = dir.path().join("Club");
    fs::create_dir_all(&club_root).unwrap();
    let mut src = Device::open_export(&src_root).unwrap();
    src.adopt(None).unwrap();
    let mut club = Device::open(&club_root).unwrap();
    let opts = PackOptions { playlists: vec!["Other".into()], ..Default::default() };
    let plan = pack::plan(&src, &club, &opts, &Ctx::silent()).unwrap();
    pack::apply(&plan, &src, &mut club, None, &Ctx::silent()).unwrap();
    let packed = fs::read(club_root.join(paths::LIBRARY_PLUS_DB)).unwrap();

    fs::write(src_root.join(paths::LIBRARY_PLUS_DB), b"not a database").unwrap();
    let src = Device::open_export(&src_root).unwrap();
    let err = pack::plan(&src, &Device::open(&club_root).unwrap(), &opts, &Ctx::silent()).unwrap_err();
    let message = err.to_string();
    assert!(message.contains("exportLibrary.db can't be read") && message.contains("CDJ-3000X"), "{message}");
    let offered =
        err.detail().actions.into_iter().map(|a| (a.method, a.params.get("replace").cloned())).collect::<Vec<_>>();
    assert_eq!(offered, [(Some("convert.plan".to_string()), Some(serde_json::Value::Bool(true)))]);
    assert_eq!(fs::read(club_root.join(paths::LIBRARY_PLUS_DB)).unwrap(), packed);
}
