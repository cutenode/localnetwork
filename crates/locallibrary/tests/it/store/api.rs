//! Requests about what the store keeps: libraries by name, the history lists it archived for a
//! drive's library, copies of the store elsewhere and its garbage, comparing against a library,
//! and the player models remembered for the check.

use crate::common::{self, path};

use std::path::Path;

use locallibrary::Error;
use locallibrary::api::{self, DriveRef};
use locallibrary::error::Reason;
use locallibrary::progress::Ctx;
use locallibrary::store::Store;

/// A store with Main saved in the library Home, after a player recorded a history list on it.
fn saved(dir: &Path) -> (Store, std::path::PathBuf) {
    let store = Store::open_or_init(dir.join("store")).unwrap();
    let main = common::demo_drive(dir, "Main");
    common::add_history(&main, "HISTORY 001", &[1]);
    let pull =
        api::PullRequest { drive: path(&main), library: Some("Home".into()), new_library: true, ..Default::default() };
    api::pull(&store, &pull, &Ctx::silent()).unwrap();
    (store, main)
}

/// A library that isn't there is refused naming the ones that are, or saying there are none.
#[test]
fn an_unknown_library_names_the_ones_there_are() {
    let dir = tempfile::tempdir().unwrap();
    let empty = Store::open_or_init(dir.path().join("empty")).unwrap();
    let rename = |store: &Store| {
        api::library_rename(store, &api::RenameLibrary { library: "Club".into(), name: "Clubs".into() }).unwrap_err()
    };
    assert!(rename(&empty).to_string().contains("it has no libraries yet"), "{}", rename(&empty));
    let (store, _) = saved(dir.path());
    assert!(rename(&store).to_string().contains("it has \"Home\""), "{}", rename(&store));
    let renamed = api::library_rename(&store, &api::RenameLibrary { library: "home".into(), name: "House".into() });
    assert_eq!(renamed.unwrap().name, "House");
}

/// The history lists on a drive, and those the store archived for the drive's library.
#[test]
fn history_lists_on_the_drive_and_in_the_store() {
    let dir = tempfile::tempdir().unwrap();
    let (store, main) = saved(dir.path());
    let on_drive = api::drive_history(&DriveRef { drive: path(&main) }).unwrap();
    assert_eq!(on_drive.lists.iter().map(|l| l.name.as_str()).collect::<Vec<_>>(), ["HISTORY 001"]);
    let by_drive = api::HistoryArchiveRequest { library: None, drive: Some(path(&main)) };
    let archive = api::history_archive(&store, &by_drive, &Ctx::silent()).unwrap();
    assert_eq!(archive.lists.iter().map(|a| a.list.name.as_str()).collect::<Vec<_>>(), ["HISTORY 001"]);
    let unknown = api::HistoryArchiveRequest { library: Some("Club".into()), drive: None };
    assert!(matches!(api::history_archive(&store, &unknown, &Ctx::silent()), Err(Error::NotFound(_))));
}

/// A copy of the store is made and listed as up to date; collecting garbage is planned first,
/// needs the store to apply, and leaves nothing to collect.
#[test]
fn the_store_is_copied_and_its_garbage_collected() {
    let dir = tempfile::tempdir().unwrap();
    let (store, _) = saved(dir.path());
    let copy = dir.path().join("copy");
    let req = api::StoreCopyRequest { path: copy.clone(), options: Default::default() };
    let report = api::store_copy(&store, &req, &Ctx::silent()).unwrap();
    assert_eq!(report.snapshots, 1);
    let copies = api::store_copies(&store).unwrap();
    assert_eq!(copies.len(), 1);
    assert!(copies[0].available && copies[0].up_to_date == Some(true));

    let plan = api::store_gc_plan(&store).unwrap();
    assert!(plan.dry_run);
    let err = plan.apply(None, &Ctx::silent()).unwrap_err();
    assert!(err.to_string().contains("needs the store"), "{err}");
    plan.apply(Some(&store), &Ctx::silent()).unwrap();
    assert!(api::store_gc_plan(&store).unwrap().noop);
}

/// A drive is compared against a library by its name, which must have a snapshot; a snapshot
/// needs the store.
#[test]
fn a_drive_is_compared_against_a_library_by_name() {
    let dir = tempfile::tempdir().unwrap();
    locallibrary::testing::volumes_on_this_thread(Some(vec![dir.path().join("volumes")]));
    let (store, main) = saved(dir.path());
    let diff = |store: Option<&Store>, left: &str| {
        api::diff(store, &api::DiffRequest { left: left.into(), right: path(&main), playlist: None })
    };
    assert!(diff(Some(&store), "Home").unwrap().is_empty());
    let err = diff(None, "snapshot:abcd").unwrap_err();
    assert!(err.to_string().contains("there is no store"), "{err}");
    let err = diff(Some(&store), "Nowhere").unwrap_err();
    assert!(matches!(err, Error::NotFound(_)), "{err}");
}

/// The check needs a drive, each named once; player models are remembered for a library that's
/// in the store, and at least one is given.
#[test]
fn the_check_and_its_player_models() {
    let dir = tempfile::tempdir().unwrap();
    let (store, main) = saved(dir.path());
    let check = |drives: Vec<String>| {
        api::check(Some(&store), &api::CheckRequest { drives, options: Default::default() }, &Ctx::silent())
    };
    assert!(check(Vec::new()).is_err());
    assert!(check(vec![path(&main), path(&main)]).unwrap_err().to_string().contains("listed twice"));

    let set = |models: &[&str], library: Option<&str>| api::SetPlayers {
        models: models.iter().map(|m| m.to_string()).collect(),
        library: library.map(Into::into),
    };
    let err = api::players_set_plan(Some(&store), &set(&["CDJ-3000"], Some("Club"))).unwrap_err();
    assert_eq!(err.reason(), Some(Reason::UnknownLibrary), "{err}");
    assert!(
        api::players_set_plan(Some(&store), &set(&[], None)).unwrap_err().to_string().contains("at least one model")
    );
    api::players_set(&store, &set(&["cdj-3000"], Some("Home"))).unwrap();
    let shown = api::players_show(&store).unwrap();
    assert_eq!(shown.libraries[0].models, ["CDJ-3000"]);
    assert_eq!(shown.libraries[0].name.as_deref(), Some("Home"));
}
