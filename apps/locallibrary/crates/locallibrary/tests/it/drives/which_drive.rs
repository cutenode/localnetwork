//! Which drive a request means: a name, a path (or a folder inside a drive), a name that's also a
//! folder here, and the drive a command takes when it's given none: for a job, a drive group, a
//! mix or an undo. Refusals name the drives to choose from.

use crate::common::{self, path};

use std::path::{Path, PathBuf};

use locallibrary::api::{self, DriveRef};
use locallibrary::device::{Device, dunce_canonicalize};
use locallibrary::edit::Edit;
use locallibrary::error::Reason;
use locallibrary::progress::Ctx;
use locallibrary::store::Store;
use locallibrary::{Error, fleet};

/// A folder of drives, the only one drives are looked for in on this thread.
fn volumes() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    locallibrary::testing::volumes_on_this_thread(Some(vec![dir.path().to_path_buf()]));
    dir
}

/// A demo drive in `dir`, adopted as `name`.
fn adopted(dir: &Path, name: &str) -> PathBuf {
    let root = common::demo_drive(dir, name);
    common::adopt(None, &root, name);
    root
}

fn store_in(dir: &Path) -> Store {
    Store::open_or_init(dir.join("store")).unwrap()
}

fn pull(store: &Store, root: &Path, library: &str) {
    let req =
        api::PullRequest { drive: path(root), library: Some(library.into()), new_library: true, ..Default::default() };
    api::pull(store, &req, &Ctx::silent()).unwrap();
}

/// A folder inside a drive's export means the drive; a name that's also a folder where the
/// command runs is refused rather than guessed.
#[test]
fn a_drive_is_named_by_its_folders_and_a_name_that_is_also_a_folder_here_is_refused() {
    let dir = volumes();
    let main = adopted(dir.path(), "Main");
    let inside = api::drive_info(&DriveRef { drive: path(&main.join("Contents")) }).unwrap();
    assert_eq!(inside.root, dunce_canonicalize(&main));
    assert_eq!(inside.name.as_deref(), Some("Main"));

    // Tests run in the crate's folder, which has a `tests` folder.
    adopted(dir.path(), "tests");
    let err = api::resolve_drive("tests").unwrap_err().to_string();
    assert!(err.contains("both a folder here and the drive at"), "{err}");
    assert_eq!(api::resolve_drive("./tests").unwrap().root(), dunce_canonicalize(Path::new("tests")));
}

/// Listing drives leaves out folders without an export unless all are asked for, and a drive's
/// backups are listed and put back by name.
#[test]
fn drives_are_listed_and_their_backups_put_back() {
    let dir = volumes();
    let main = adopted(dir.path(), "Main");
    std::fs::create_dir(dir.path().join("Blank")).unwrap();
    let names = |all| -> Vec<String> {
        api::list_drives(&api::ListDrives { all }).into_iter().map(|d| d.volume_name).collect()
    };
    assert_eq!(names(false), ["Main"]);
    assert_eq!(names(true).len(), 2);

    let main_ref = DriveRef { drive: "Main".into() };
    assert!(api::list_backups(&main_ref).unwrap().is_empty());
    common::apply(&main, &[Edit::CreatePlaylist { path: "Friday".into(), folder: false }]);
    let backups = api::list_backups(&main_ref).unwrap();
    assert_eq!(backups.len(), 1);
    let req = api::RestoreBackup { drive: "Main".into(), backup: Some(backups[0].id.clone()), force: false };
    assert_eq!(api::restore_backup(&req).unwrap().id, backups[0].id);
    let playlists = Device::open(&main).unwrap().library().unwrap().playlists;
    assert!(playlists.iter().all(|p| p.name != "Friday"), "the playlist went with the backup");
}

/// A write that names no drive goes to every mounted rekordbox drive but archive ones, and says
/// which it left out.
#[test]
fn unnamed_writes_leave_archive_drives_out() {
    let dir = volumes();
    let store = store_in(dir.path());
    let (main, vault) = (adopted(dir.path(), "Main"), adopted(dir.path(), "Vault"));
    for root in [&main, &vault] {
        locallibrary::sync::register_drive(&store, &Device::open(root).unwrap(), None).unwrap();
    }
    fleet::set_role(&store, Device::open(&vault).unwrap().id().unwrap(), Some(fleet::Role::Archive)).unwrap();

    let (targets, left) = api::unnamed_targets(Some(&store));
    assert_eq!(targets.iter().map(Device::name).collect::<Vec<_>>(), ["Main"]);
    assert_eq!(left, ["Vault"]);
    assert_eq!(
        api::left_out(&left).as_deref(),
        Some("left out Vault: archive and retired drives aren't taken unless named")
    );
    assert_eq!(api::left_out(&[]), None);
}

/// With several drives and nothing to choose by, the drive a command means is refused, listing
/// them; so is a drive group of drives that belong to no library, or to several.
#[test]
fn the_default_drive_and_group_are_refused_when_ambiguous() {
    let dir = volumes();
    let store = store_in(dir.path());
    let lonely = adopted(dir.path(), "Lonely");
    let group = api::drive_group(Some(&store), None).unwrap();
    assert_eq!((group.library, group.drives), (None, vec![dunce_canonicalize(&lonely)]), "a group of its own");
    let err = api::drive_group(None, Some("Home")).unwrap_err();
    assert!(err.to_string().contains("there's no store yet"), "{err}");
    assert!(matches!(api::drive_group(Some(&store), Some("Home")), Err(Error::NotFound(_))));

    let other = adopted(dir.path(), "Other");
    let err = api::default_drive(None, "check").unwrap_err();
    assert_eq!(err.reason(), Some(Reason::ChooseDrive), "{err}");
    let listing = err.to_string();
    assert!(listing.contains("say which drive to check") && listing.contains("(no library)"), "{listing}");
    let err = api::drive_group(Some(&store), None).unwrap_err().to_string();
    assert!(err.contains("none belongs to a library"), "{err}");

    pull(&store, &lonely, "Home");
    pull(&store, &other, "Away");
    let err = api::drive_group(Some(&store), None).unwrap_err().to_string();
    assert!(err.contains("drives of 2 libraries are mounted"), "{err}");
    let home = api::drive_group(Some(&store), Some("Home")).unwrap();
    assert_eq!(home.specs(), [path(&dunce_canonicalize(&lonely))]);
    std::fs::remove_dir_all(&other).unwrap();
    let err = api::drive_group(Some(&store), Some("Away")).unwrap_err();
    assert!(matches!(&err, Error::NotFound(m) if m.contains("a mounted drive of library \"Away\"")), "{err}");
}

/// A mix is matched against the only mounted rekordbox drive, or the one named.
#[test]
fn the_drive_a_mix_is_matched_against() {
    let dir = volumes();
    assert!(matches!(api::mix_drive(None), Err(Error::NotFound(_))));
    let main = adopted(dir.path(), "Main");
    assert_eq!(api::mix_drive(None).unwrap().root(), dunce_canonicalize(&main));
    adopted(dir.path(), "Other");
    let err = api::mix_drive(None).unwrap_err();
    assert_eq!(err.reason(), Some(Reason::ChooseDrive), "{err}");
    assert_eq!(api::mix_drive(Some("Other")).unwrap().name(), "Other");
}

/// An undo that names no drive takes the only mounted drive with something to undo, and with
/// two asks which, offering the list of what can be undone.
#[test]
fn the_drive_an_undo_means() {
    let dir = volumes();
    let undo = || api::undo_plan(&api::UndoRequest::default());
    assert!(matches!(undo(), Err(Error::NotFound(_))));
    let edit = [Edit::CreatePlaylist { path: "Friday".into(), folder: false }];
    let main = adopted(dir.path(), "Main");
    common::apply(&main, &edit);
    assert_eq!(undo().unwrap().drive, "Main");
    common::apply(&adopted(dir.path(), "Other"), &edit);
    let err = undo().unwrap_err();
    assert_eq!(err.reason(), Some(Reason::ChooseDrive), "{err}");
    let offered: Vec<Option<String>> = err.detail().actions.into_iter().map(|a| a.method).collect();
    assert_eq!(offered, [Some("undo.list".to_string())]);
}
