//! Looking after a drive: checking its files and repairing them, its space, clutter, update files
//! for the players, the name players show and a contact for whoever finds it. Mostly through the
//! service layer.

use crate::common::{self, path, rot};

use std::path::{Path, PathBuf};

use locallibrary::api::{self, DriveRef};
use locallibrary::device::Device;
use locallibrary::error::Reason;
use locallibrary::player::{self, CONTACT_FILE};
use locallibrary::progress::Ctx;
use locallibrary::space::{self, PLAYER_HEADROOM};
use locallibrary::store::Store;
use locallibrary::sync::{self, PullOptions};
use locallibrary::tidy::TidyOptions;
use locallibrary::{fleet, paths};

fn drive(root: &Path) -> DriveRef {
    DriveRef { drive: path(root) }
}

/// A demo drive, adopted and saved in a store named for this thread.
fn saved_drive(dir: &Path) -> (Store, PathBuf) {
    let store = Store::open_or_init(dir.join("store")).unwrap();
    locallibrary::testing::store_root_on_this_thread(Some(store.root().to_path_buf()));
    locallibrary::testing::volumes_on_this_thread(Some(vec![dir.to_path_buf()]));
    let root = common::demo_drive(dir, "Main");
    common::adopt(Some(&store), &root, "Main");
    api::pull(&store, &api::PullRequest { drive: path(&root), ..Default::default() }, &Ctx::silent()).unwrap();
    (store, root)
}

/// A damaged file is found against the store and noted in the drive's logbook; repairing it
/// rewrites it from a good copy, and the next check is clean.
#[test]
fn damage_is_logged_and_repaired_from_the_store() {
    let dir = tempfile::tempdir().unwrap();
    let (store, root) = saved_drive(dir.path());
    let audio = paths::key(&Device::open(&root).unwrap().library().unwrap().tracks[0].file_path);
    // A copy that's still intact, to check against; the drive itself among them is left out.
    let copy = dir.path().join("Copy");
    common::copy_dir(&root, &copy);
    rot(&paths::to_fs(&root, &audio));
    let against = vec![path(&root), path(&copy)];
    let verify = |store: Option<&Store>, snapshot: Option<&str>| {
        let req =
            api::VerifyRequest { drive: path(&root), snapshot: snapshot.map(Into::into), against: against.clone() };
        api::drive_verify(store, &req, &Ctx::silent())
    };

    let err = verify(None, Some("abcd")).unwrap_err();
    assert!(err.to_string().contains("needs the store"), "{err}");
    assert!(verify(Some(&store), Some("ffffffff")).is_err(), "no such snapshot");
    let report = verify(Some(&store), None).unwrap();
    assert_eq!(report.damaged.iter().map(|p| p.path.as_str()).collect::<Vec<_>>(), [audio.as_str()]);
    let log = api::drive_log(&store, &drive(&root)).unwrap().log;
    assert!(log.last_scrub.is_some_and(|c| !c.ok));
    assert_eq!(log.problems.iter().map(|p| p.kind.as_str()).collect::<Vec<_>>(), ["damaged_files"]);

    let req = api::RepairRequest { drive: path(&root), problems: report.damaged, against: Vec::new() };
    let repaired = api::drive_repair(Some(&store), &req, &Ctx::silent()).unwrap();
    assert_eq!(repaired.repaired, std::slice::from_ref(&audio));
    assert!(repaired.failed.is_empty());
    assert_eq!(api::drive_log(&store, &drive(&root)).unwrap().log.writes, 1, "the repair is a write");
    assert!(verify(Some(&store), None).unwrap().is_clean());
}

/// With the store, a drive's space report says whether its library fits on the other drives of
/// its library; without it, there's nothing to compare with.
#[test]
fn space_is_weighed_against_the_drives_of_the_library() {
    let dir = tempfile::tempdir().unwrap();
    let (store, root) = saved_drive(dir.path());
    let backup = common::blank(dir.path(), "Backup");
    api::sync_apply(Some(&store), &common::sync_pair(&root, &backup), &Ctx::silent()).unwrap();

    let with = api::drive_space(Some(&store), &drive(&root), &Ctx::silent()).unwrap();
    assert!(with.forecast.is_none(), "one snapshot is no trend");
    assert_eq!(with.fits.len(), 1, "{:?}", with.fits);
    assert!(with.fits[0].mounted);
    let without = api::drive_space(None, &drive(&root), &Ctx::silent()).unwrap();
    assert!(without.fits.is_empty());
    assert_eq!(without.categories.len(), with.categories.len());
}

/// Tidying plans the clutter it removes (the trash only when asked) and removes just that.
#[test]
fn clutter_is_planned_then_removed() {
    let dir = tempfile::tempdir().unwrap();
    let root = common::demo_drive(dir.path(), "Main");
    std::fs::write(root.join(".DS_Store"), b"finder").unwrap();
    std::fs::create_dir(root.join(".Trashes")).unwrap();
    std::fs::write(root.join(".Trashes/old.mp3"), b"binned").unwrap();
    let tidy = |empty_trash| api::TidyRequest { drive: path(&root), options: TidyOptions { empty_trash } };

    let plan = api::tidy_plan(&tidy(false), &Ctx::silent()).unwrap();
    assert_eq!(plan.items.iter().map(|i| i.path.as_str()).collect::<Vec<_>>(), [".DS_Store"]);
    assert!(plan.destructive && !plan.noop);
    let report = api::tidy_apply(&tidy(false), &Ctx::silent()).unwrap();
    assert_eq!(report.removed, 1);
    assert!(!root.join(".DS_Store").exists() && root.join(".Trashes/old.mp3").exists());
    assert!(api::tidy_apply(&tidy(true), &Ctx::silent()).unwrap().removed > 0);
    assert!(api::tidy_plan(&tidy(true), &Ctx::silent()).unwrap().noop);
}

/// An update file the players wouldn't recognise is refused unless forced; one already on the
/// drive is replaced only when asked, and removing them is planned first.
#[test]
fn update_files_are_checked_copied_and_removed() {
    let dir = tempfile::tempdir().unwrap();
    let root = common::demo_drive(dir.path(), "Main");
    let downloads = dir.path().join("downloads");
    std::fs::create_dir(&downloads).unwrap();
    let file = |name: &str| {
        let f = downloads.join(name);
        std::fs::write(&f, b"firmware").unwrap();
        f
    };
    let update = |file: PathBuf, replace, force| api::FirmwareRequest { drive: path(&root), file, replace, force };

    let odd = file("notes.UPD");
    let plan = api::firmware_plan(&update(odd.clone(), false, false)).unwrap();
    assert!(plan.warnings.iter().any(|w| w.contains("isn't named like")), "{:?}", plan.warnings);
    let err = api::firmware_apply(&update(odd.clone(), false, false), &Ctx::silent()).unwrap_err();
    assert_eq!(err.reason(), Some(Reason::FirmwareProblems), "{err}");
    api::firmware_apply(&update(odd, false, true), &Ctx::silent()).unwrap();
    assert!(root.join("notes.UPD").is_file());

    let cdj = file("CDJ3Kv321.UPD");
    let plan = api::firmware_plan(&update(cdj.clone(), false, false)).unwrap();
    assert!(plan.warnings.iter().any(|w| w.contains("only one update file")), "{:?}", plan.warnings);
    let plan = api::firmware_plan(&update(cdj.clone(), true, false)).unwrap();
    assert!(plan.destructive);
    assert!(plan.summary.iter().any(|s| s == "remove notes.UPD"), "{:?}", plan.summary);
    let copied = api::firmware_apply(&update(cdj, true, true), &Ctx::silent()).unwrap();
    assert_eq!(copied.removed, ["notes.UPD"]);

    let remove = api::firmware_remove_plan(&drive(&root)).unwrap();
    assert_eq!(remove.files, ["CDJ3Kv321.UPD"]);
    assert_eq!(remove.bytes, 8);
    assert_eq!(api::firmware_remove(&drive(&root), &Ctx::silent()).unwrap(), ["CDJ3Kv321.UPD"]);
    assert!(api::firmware_remove_plan(&drive(&root)).unwrap().noop);
}

/// The name and colour players show are planned before they're written, and so is the contact
/// for whoever finds the drive, with its playlist on a drive holding an export.
#[test]
fn player_name_and_contact_are_planned_then_written() {
    let dir = tempfile::tempdir().unwrap();
    let root = common::demo_drive(dir.path(), "Main");
    common::add_onelibrary(&root);
    let set =
        |name: Option<&str>, colour| api::SetPlayerIdentity { drive: path(&root), name: name.map(Into::into), colour };

    assert!(api::player_set_plan(&set(None, None)).is_err());
    let plan = api::player_set_plan(&set(Some(" MAIN "), Some(3))).unwrap();
    assert_eq!(plan.summary, ["players show Main as \"MAIN\"", "in colour 3"]);
    api::player_set(&set(Some("MAIN"), Some(3))).unwrap();
    let shown = api::player_show(&drive(&root)).unwrap().onelibrary.unwrap();
    assert_eq!((shown.name.as_deref(), shown.colour), (Some("MAIN"), Some(3)));

    let contact = |contact: &str| api::SetContact { drive: path(&root), contact: contact.into(), playlist: true };
    assert!(api::drive_contact_plan(&contact(" ")).is_err());
    let plan = api::drive_contact_plan(&contact("dj@example.com")).unwrap();
    assert!(!plan.replaces);
    assert_eq!(plan.playlist.as_deref(), Some("IF FOUND: dj@example.com"));
    let report = api::drive_contact(&contact("dj@example.com"), &Ctx::silent()).unwrap();
    assert!(report.file.is_file());
    let names: Vec<String> =
        Device::open(&root).unwrap().library().unwrap().playlists.into_iter().map(|p| p.name).collect();
    assert!(names.contains(&"IF FOUND: dj@example.com".to_string()), "{names:?}");
    let again = api::drive_contact_plan(&contact("dj@example.com")).unwrap();
    assert!(again.replaces && again.summary[0].starts_with("replace "), "{:?}", again.summary);
}

/// With several drives in a library, whether it fits on the others: a retired one is left out,
/// and one too small says so.
#[test]
fn a_library_fits_on_the_drives_with_room_for_it() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    let mut drives = Vec::new();
    for name in ["Main", "Big", "Small", "Retired"] {
        let mut d = Device::open(common::demo_drive(dir.path(), name)).unwrap();
        d.adopt(Some(name)).unwrap();
        let opts = PullOptions { library: Some("Main".into()), message: None, force: false };
        sync::pull(&store, &mut d, &opts, &Ctx::silent()).unwrap();
        drives.push(d);
    }
    let gb = 1_000_000_000;
    for (d, capacity) in drives.iter().zip([64 * gb, 128 * gb, gb]) {
        fleet::update(&store, d.id().unwrap(), |r| r.capacity = Some(capacity)).unwrap();
    }
    fleet::retire(&store, drives[3].id().unwrap(), false).unwrap();

    // 1 GB less the file system's 3% is too small for 1 GB of library and the players' headroom.
    let fits = space::fits_on_set(&store, &drives[0], gb, &drives[..2]).unwrap();
    let shown: Vec<(&str, bool, bool)> = fits.iter().map(|f| (f.name.as_str(), f.fits, f.mounted)).collect();
    assert_eq!(shown, [("Big", true, true), ("Small", false, false)]);
    assert_eq!((fits[1].capacity, fits[1].needed), (gb / 100 * 97, gb + PLAYER_HEADROOM));

    // A drive outside any library has no group.
    let loose = Device::open(common::demo_drive(dir.path(), "Loose")).unwrap();
    assert!(space::fits_on_set(&store, &loose, gb, &[]).unwrap().is_empty());
}

/// A forecast needs a linked library and snapshots spread over a week or more.
#[test]
fn no_forecast_without_a_history_to_go_on() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    let mut main = Device::open(common::demo_drive(dir.path(), "Main")).unwrap();
    assert!(space::forecast(&store, &main, Some(1 << 30)).unwrap().is_none(), "not linked to a library");
    main.adopt(Some("Main")).unwrap();
    let opts = PullOptions { library: None, message: None, force: false };
    sync::pull(&store, &mut main, &opts, &Ctx::silent()).unwrap();
    assert!(space::forecast(&store, &main, Some(1 << 30)).unwrap().is_none(), "one snapshot");
}

/// The contact goes in a playlist players show first, its name made one players can show.
#[test]
fn the_contact_is_a_playlist_too() {
    let dir = tempfile::tempdir().unwrap();
    let mut d = Device::open(common::demo_drive(dir.path(), "Main")).unwrap();
    let r = player::set_contact(&mut d, "sam@example.com / 555 0100", true, &Ctx::silent()).unwrap();
    assert_eq!(r.playlist.as_deref(), Some("IF FOUND: sam@example.com - 555 0100"));
    assert!(d.root().join(CONTACT_FILE).is_file());
    let lib = Device::open(d.root()).unwrap().library().unwrap();
    assert!(lib.playlists.iter().any(|p| p.name == "IF FOUND: sam@example.com - 555 0100" && p.parent_id.is_none()));
    // Without asking for one, there's no playlist.
    let r = player::set_contact(&mut d, "sam@example.com", false, &Ctx::silent()).unwrap();
    assert_eq!(r.playlist, None);
}
