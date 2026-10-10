//! A drive back from a set, while the library moved on at home: what players did to it is told
//! apart from everything else, and syncing it keeps what they recorded in the store first, so it
//! needs the store, and is done as a pair before several drives are synced at once.

use crate::common::{self, path, sync_pair};

use std::path::{Path, PathBuf};

use locallibrary::after_set;
use locallibrary::api::{self, Direction, SyncRequest};
use locallibrary::compare::{self, Relation};
use locallibrary::device::Device;
use locallibrary::edit::Edit;
use locallibrary::error::Reason;
use locallibrary::progress::Ctx;
use locallibrary::store::Store;
use locallibrary::sync::{self, PullOptions, SyncOptions};
use locallibrary::{paths, pdb};
use rekordbox_pdb::PdbEditor;

fn create(path: &str) -> Edit {
    Edit::CreatePlaylist { path: path.into(), folder: false }
}

fn editor(root: &Path) -> PdbEditor {
    PdbEditor::from_file(root.join(paths::EXPORT_PDB)).unwrap()
}

/// Club, synced from Main, which is then saved in the store.
fn synced(store: &Store, main: &mut Device, club: &Path) -> Device {
    std::fs::create_dir(club).unwrap();
    let mut club = Device::open(club).unwrap();
    sync::sync_drive_to_drive(main, &mut club, &SyncOptions::default(), &Ctx::silent()).unwrap();
    let opts = PullOptions { library: None, message: Some("before the set".into()), force: false };
    sync::pull(store, main, &opts, &Ctx::silent()).unwrap();
    club
}

/// A player records a history list on Club; Main gets a new export at home. Only a change of
/// Club's own makes it more than a drive back from a set.
#[test]
fn a_drive_back_from_a_set_is_recognised() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    let mut main = Device::open(common::demo_drive(dir.path(), "Main")).unwrap();
    main.adopt(Some("Main")).unwrap();
    let club_root = synced(&store, &mut main, &dir.path().join("Club")).root().to_path_buf();

    let ids: Vec<u32> = Device::open(&club_root).unwrap().library().unwrap().tracks.iter().map(|t| t.id).collect();
    common::add_history(&club_root, "HISTORY 001", &ids);
    common::export_new_track(main.root(), "Artist", "Fresh");
    let (main, club) = (Device::open(main.root()).unwrap(), Device::open(&club_root).unwrap());

    let c = compare::compare_devices(&main, &club, &Ctx::silent()).unwrap();
    assert_eq!(c.relation, Relation::Diverged);
    let g =
        after_set::resolve(&store, &main, &club, &Ctx::silent()).unwrap().expect("the club drive's are the players'");
    assert!(!g.left_is_back_from_set);
    assert_eq!(g.back_from_set.history_lists, ["HISTORY 001"]);
    assert!(g.newer.other.iter().any(|o| o.contains("track added")), "{:?}", g.newer.other);

    common::export_new_track(&club_root, "Other", "Edit");
    let club = Device::open(&club_root).unwrap();
    assert!(after_set::resolve(&store, &main, &club, &Ctx::silent()).unwrap().is_none());
}

/// Every kind of change on each side: the drive back from the set has a longer history list, a
/// TAG LIST playlist and memory cues in an analysis file; at home a track and a history list went,
/// a playlist was added and one removed, and audio files changed and went.
#[test]
fn players_changes_are_told_from_everything_else() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    let home = common::demo_drive(dir.path(), "Main");
    common::apply(&home, &[create("Old")]);
    let lib = pdb::read_library(&home.join(paths::EXPORT_PDB)).unwrap();
    let (first, second) = (&lib.tracks[0], &lib.tracks[1]);
    common::add_history(&home, "HISTORY 001", &[first.id]);
    let history = pdb::read_library(&home.join(paths::EXPORT_PDB)).unwrap().history[0].id;
    let mut main = Device::open(&home).unwrap();
    main.adopt(Some("Main")).unwrap();
    let club_root = synced(&store, &mut main, &dir.path().join("Club")).root().to_path_buf();

    // At the set.
    let mut ed = editor(&club_root);
    ed.add_to_history(history, second.id).unwrap();
    ed.save(club_root.join(paths::EXPORT_PDB)).unwrap();
    common::apply(&club_root, &[create("TAG LIST 001")]);
    let anlz = paths::to_fs(&club_root, &paths::key(&first.analyze_path));
    let mut cues = std::fs::read(&anlz).unwrap();
    cues.extend_from_slice(b"PCOB");
    std::fs::write(&anlz, cues).unwrap();

    // At home.
    let mut ed = editor(&home);
    ed.remove_tracks(&[second.id]).unwrap();
    ed.remove_history_playlists(&[history]).unwrap();
    ed.save(home.join(paths::EXPORT_PDB)).unwrap();
    std::fs::remove_file(paths::to_fs(&home, &paths::key(&second.file_path))).unwrap();
    std::fs::write(paths::to_fs(&home, &paths::key(&first.file_path)), b"re-encoded").unwrap();
    common::apply(&home, &[Edit::DeletePlaylist { playlist: "Old".into(), with_tracks: false }, create("Friday")]);

    let (main, club) = (Device::open(&home).unwrap(), Device::open(&club_root).unwrap());
    let set =
        after_set::resolve(&store, &club, &main, &Ctx::silent()).unwrap().expect("the club drive's are the players'");
    assert!(set.left_is_back_from_set);
    let back = &set.back_from_set;
    assert_eq!(
        (back.history_lists.as_slice(), back.tag_lists.as_slice(), back.cue_files),
        (&["HISTORY 001".to_string()][..], &["TAG LIST 001".to_string()][..], 1)
    );
    assert_eq!(back.describe(), "1 history list(s), cues in 1 analysis file(s), 1 TAG LIST playlist(s)");
    let other = &set.newer.other;
    for change in [
        format!("track removed: {}", second.display_name()),
        "playlist removed: Old".into(),
        "playlist added: Friday".into(),
        "history list removed: HISTORY 001".into(),
        format!("file removed: {}", paths::key(&second.file_path)),
        format!("file changed: {}", paths::key(&first.file_path)),
    ] {
        assert!(other.contains(&change), "{change:?} isn't in {other:?}");
    }
    assert_eq!(set.newer.describe(), "nothing", "no player made those");
}

/// Through the service layer: Main at home and Club, synced from it and both saved; then a player
/// records history on Club, and Main gets a new export. The store is named for this thread.
fn back_from_a_set(dir: &Path) -> (Store, PathBuf, PathBuf) {
    let store = Store::open_or_init(dir.join("store")).unwrap();
    locallibrary::testing::store_root_on_this_thread(Some(store.root().to_path_buf()));
    locallibrary::testing::volumes_on_this_thread(Some(vec![dir.to_path_buf()]));
    let ctx = Ctx::silent();
    let main = common::demo_drive(dir, "Main");
    common::adopt(Some(&store), &main, "Main");
    let club = common::blank(dir, "Club");
    api::sync_apply(Some(&store), &sync_pair(&main, &club), &ctx).unwrap();
    common::adopt(Some(&store), &club, "Club");
    api::pull(&store, &api::PullRequest { drive: path(&main), ..Default::default() }, &ctx).unwrap();
    let ids: Vec<u32> = Device::open(&club).unwrap().library().unwrap().tracks.iter().map(|t| t.id).collect();
    common::add_history(&club, "HISTORY 001", &ids);
    common::export_new_track(&main, "Artist", "Fresh");
    (store, main, club)
}

/// Copying over a drive back from a set keeps its players' changes in the store first: without
/// the store that's refused, offering to pull the drive.
#[test]
fn keeping_a_sets_changes_needs_the_store() {
    let dir = tempfile::tempdir().unwrap();
    let (_store, main, club) = back_from_a_set(dir.path());
    let planned = api::sync_plan(&sync_pair(&main, &club), &Ctx::silent()).unwrap();
    assert!(planned.after_set.is_some());
    assert!(planned.summary[0].starts_with("Club came back from a set"), "{:?}", planned.summary);

    let pinned =
        SyncRequest { direction: Direction::LeftToRight, keep_player_changes: true, ..sync_pair(&main, &club) };
    let err = api::sync_apply(None, &pinned, &Ctx::silent()).unwrap_err();
    assert_eq!(err.reason(), Some(Reason::NotSaved), "{err}");
    let offered: Vec<Option<String>> = err.detail().actions.into_iter().map(|a| a.method).collect();
    assert_eq!(offered, [Some("pull".to_string())]);
    assert!(Device::open(&club).unwrap().library().unwrap().history.iter().any(|h| h.name == "HISTORY 001"));
}

/// Several drives at once won't take a drive back from a set: it's synced as a pair first.
#[test]
fn a_drive_back_from_a_set_is_synced_as_a_pair_first() {
    let dir = tempfile::tempdir().unwrap();
    let (_store, main, club) = back_from_a_set(dir.path());
    let err = api::sync_many_plan(&common::sync_many(&[&main, &club]), &Ctx::silent()).unwrap_err();
    assert_eq!(err.reason(), Some(Reason::BackFromSet), "{err}");
    assert!(err.to_string().starts_with("Club came back from a set"), "{err}");
    let offered: Vec<String> = err.detail().actions.into_iter().map(|a| a.label).collect();
    assert_eq!(offered, ["Sync Main and Club first"]);
}
