//! Drive management through the service layer: finding drives (and hiding disks) while tests
//! name the folders they're in, syncing after a set, archive drives, parallel multi-drive sync,
//! and provisioning a new drive. The store the API consults is named for each test's thread.

use crate::common::{self, path, sync_pair};

use locallibrary::api::{self, Direction};
use locallibrary::compare::{Relation, Side};
use locallibrary::device::{Device, dunce_canonicalize};
use locallibrary::progress::Ctx;
use locallibrary::store::Store;
use locallibrary::{Error, fleet};

/// While the test switch names folders, a drive the store knows is found only inside them: never
/// the machine's own, wherever the store last saw them. Inside, one mounted deeper than the
/// folders' subfolders is still found where the store last saw it. The machine's disks are hidden
/// too.
#[test]
fn the_store_finds_drives_only_inside_the_folders_named() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    locallibrary::testing::store_root_on_this_thread(Some(store.root().to_path_buf()));
    let volumes = dir.path().join("volumes");
    let deep = common::demo_drive(&volumes.join("mnt"), "Deep");
    let outside = common::demo_drive(dir.path(), "Outside");
    for (d, name) in [(&deep, "Deep"), (&outside, "Probestick")] {
        common::adopt(Some(&store), d, name);
    }
    locallibrary::testing::volumes_on_this_thread(Some(vec![volumes.clone()]));
    let names: Vec<_> = api::list_drives(&api::ListDrives::default()).into_iter().map(|d| d.name).collect();
    assert_eq!(names, [Some("Deep".to_string())]);
    assert_eq!(api::resolve_drive("deep").unwrap().root(), dunce_canonicalize(&deep));
    assert!(api::resolve_drive("Probestick").is_err());
    // Disks are hidden with the drives: they can't be listed, nor a format planned or applied.
    let hidden = |e: Error| assert!(e.to_string().contains("LOCALLIBRARY_VOLUMES"), "{e}");
    hidden(api::list_disks(store.root()).unwrap_err());
    let format = api::FormatRequest { drive: path(&deep), options: Default::default() };
    hidden(api::format_plan(store.root(), &format, &Ctx::silent()).unwrap_err());
    let apply = api::FormatApply { drive: path(&deep), identity: "disk9".into(), options: Default::default() };
    hidden(api::format_apply(store.root(), &apply, &Ctx::silent()).unwrap_err());
    assert!(Device::open(&deep).unwrap().has_export(), "untouched");
}

/// A mounted drive is looked up by its identity: the registry's record of another drive with its
/// name is never taken for it. That name is refused, naming both, and so is a name two mounted
/// drives, or two registered ones, share.
#[test]
fn a_drive_is_found_by_its_identity_and_a_shared_name_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    locallibrary::testing::store_root_on_this_thread(Some(store.root().to_path_buf()));
    let volumes = dir.path().join("volumes");
    let main = common::demo_drive(dir.path(), "Main");
    let stick = common::demo_drive(&volumes, "Stick");
    common::adopt(Some(&store), &main, "Main");
    common::adopt(None, &stick, "Main");
    locallibrary::testing::volumes_on_this_thread(Some(vec![volumes.clone()]));
    let main_id = Device::open(&main).unwrap().id().unwrap().to_string();
    let find = |drive: &str| api::drive_find(&store, &api::DriveRef { drive: drive.into() });
    let choose = |e: Error| {
        assert_eq!(e.reason(), Some(locallibrary::error::Reason::ChooseDrive), "{e}");
        e.to_string()
    };

    let refused = choose(find("Main").unwrap_err());
    assert!(refused.contains(&main_id) && refused.contains("Stick"), "{refused}");
    assert!(api::drive_log(&store, &api::DriveRef { drive: "Main".into() }).is_err());
    assert!(matches!(find(&path(&stick)), Err(Error::NotFound(_))), "the stick isn't registered");
    assert_eq!(find(&main_id).unwrap().id.to_string(), main_id);

    let spare = common::demo_drive(&volumes, "Spare");
    common::adopt(None, &spare, "Main");
    let refused = find("Main").unwrap_err().to_string();
    assert!(refused.contains("Spare") && refused.contains("Stick"), "{refused}");
    // Regression: a role went to the registry's Main, which neither of them is.
    let role = api::drive_role(&store, &api::SetRole { drive: "Main".into(), role: Some("backup".into()) });
    assert!(role.unwrap_err().to_string().contains("Spare"));
    assert!(api::drive_to_change(&store, &api::DriveRef { drive: "Main".into() }).is_err());
    assert!(store.drives().unwrap().values().all(|r| r.role.is_none()), "no role was set");
    std::fs::remove_dir_all(&spare).unwrap();

    // The stick, by its path: what a role would change, before it's registered.
    let shown = api::drive_to_change(&store, &api::DriveRef { drive: path(&stick) }).unwrap();
    assert_eq!((shown.name.as_str(), Some(shown.id)), ("Main", Device::open(&stick).unwrap().id()));
    assert!(!store.drives().unwrap().contains_key(&shown.id), "nothing is written");

    // Registered too, the stick is found by its identity; unmounted, the name is two drives'.
    let opened = Device::open(&stick).unwrap();
    locallibrary::sync::register_drive(&store, &opened, None).unwrap();
    let stick_id = opened.id().unwrap();
    assert_eq!(find("Main").unwrap().id, stick_id);
    let refused = choose(fleet::find_record(&store, "Main").unwrap_err());
    assert!(refused.contains(&main_id) && refused.contains(&stick_id.to_string()), "{refused}");
}

#[test]
fn drive_management_through_the_api() {
    let dir = tempfile::tempdir().unwrap();
    let store_root = dir.path().join("store");
    let store = Store::open_or_init(&store_root).unwrap();
    locallibrary::testing::store_root_on_this_thread(Some(store_root.clone()));
    // The drives below are the folders in `dir`: found there whatever $LOCALLIBRARY_VOLUMES says,
    // which hides drives found elsewhere, through the store, while it's set.
    locallibrary::testing::volumes_on_this_thread(Some(vec![dir.path().to_path_buf()]));
    let ctx = Ctx::silent();

    // Main, and Club synced from it, both saved in the store.
    let main = common::demo_drive(dir.path(), "Main");
    common::adopt(Some(&store), &main, "Main");
    std::fs::create_dir(dir.path().join("Club")).unwrap();
    let club = dir.path().join("Club");
    let r = api::sync_apply(Some(&store), &sync_pair(&main, &club), &ctx).unwrap();
    assert!(r.report.is_some());
    common::adopt(Some(&store), &club, "Club");
    api::pull(&store, &api::PullRequest { drive: path(&main), ..Default::default() }, &ctx).unwrap();

    // The club: a player records history on Club; at home, Main gets a new export.
    let ids: Vec<u32> = Device::open(&club).unwrap().library().unwrap().tracks.iter().map(|t| t.id).collect();
    common::add_history(&club, "HISTORY 001", &ids);
    common::export_new_track(&main, "Artist", "Fresh");

    let preview = api::sync_plan(&sync_pair(&main, &club), &ctx).unwrap();
    assert_eq!(preview.comparison.relation, Relation::Diverged);
    let g = preview.after_set.as_ref().expect("Club came back from a set");
    assert_eq!(g.back_from_set.drive, "Club");
    assert_eq!(preview.target.as_deref(), Some(dunce_canonicalize(&club).as_path()));

    // Applying with the confirmed direction pinned, as the CLI does.
    let pinned = api::SyncRequest {
        left: path(preview.source.as_ref().unwrap()),
        right: path(preview.target.as_ref().unwrap()),
        direction: Direction::LeftToRight,
        keep_player_changes: true,
        ..sync_pair(&main, &club)
    };
    let r = api::sync_apply(Some(&store), &pinned, &ctx).unwrap();
    assert_eq!(r.saved.len(), 2, "the club's state, then the result");
    let lib = &store.libraries().unwrap()[0];
    let history = store.history(lib, None).unwrap();
    assert_eq!(history.len(), 3);
    // The club's history list is kept in the store...
    let club_snap = store.load_snapshot(&r.saved[0].id).unwrap();
    let kept = Side::from_snapshot(&store, &club_snap, "club").unwrap().library;
    assert!(kept.history.iter().any(|h| h.name == "HISTORY 001"));
    // ...and both drives now hold the newest library, which is the store's head.
    let status = api::status(Some(&store), &ctx).unwrap();
    for d in [&main, &club] {
        let s = status.iter().find(|s| s.drive.root == dunce_canonicalize(d)).expect("registered");
        assert_eq!(s.relation, Some(Relation::Identical), "{s:?}");
    }

    // An archive drive is never overwritten without being told to.
    let club_id = Device::open(&club).unwrap().id().unwrap();
    fleet::set_role(&store, club_id, Some(fleet::Role::Archive)).unwrap();
    common::export_new_track(&main, "Artist", "Later");
    let err = api::sync_plan(&sync_pair(&main, &club), &ctx).unwrap_err();
    assert!(matches!(err.inner(), Error::Conflict(m) if m.contains("archive")), "{err}");
    let ok = api::sync_plan(&api::SyncRequest { overwrite_archive: true, ..sync_pair(&main, &club) }, &ctx);
    assert!(ok.is_ok());
    fleet::set_role(&store, club_id, None).unwrap();

    // Several drives at once.
    for n in ["X", "Y"] {
        std::fs::create_dir(dir.path().join(n)).unwrap();
    }
    let many = common::sync_many(&[&main, &club, &dir.path().join("X"), &dir.path().join("Y")]);
    let preview = api::sync_many_plan(&many, &ctx).unwrap();
    let results = api::sync_many_apply(Some(&store), &many, &preview, &ctx).unwrap();
    assert_eq!(results.len(), 3, "{results:?}");
    assert!(results.iter().all(|r| r.error.is_none() && r.report.is_some()), "{results:?}");
    assert!(store.drives().unwrap().values().filter(|d| d.log.writes > 0).count() >= 3);

    // A new drive, set up from the latest snapshot, checked on the way.
    api::pull(&store, &api::PullRequest { drive: path(&main), ..Default::default() }, &ctx).unwrap();
    std::fs::create_dir(dir.path().join("New")).unwrap();
    let req = api::ProvisionRequest {
        target: path(&dir.path().join("New")),
        like: None,
        library: None,
        name: Some("Spare".into()),
        format: false,
        format_options: Default::default(),
        role: Some("backup".into()),
        player_name: None,
        player_colour: None,
        settings: true,
        verify: true,
        eject: false,
    };
    let plan = api::provision_plan(&store, &req, &ctx).unwrap();
    assert!(plan.steps.iter().any(|s| s.contains("latest snapshot")), "{:?}", plan.steps);
    let r = api::provision_apply(&store, &req, None, &ctx).unwrap();
    assert!(r.stopped.is_none(), "{:?}", r.stopped);
    assert!(r.verify.as_ref().unwrap().is_clean());
    assert_eq!(r.drive.as_ref().unwrap().name.as_deref(), Some("Spare"));
    let spare = store.drives().unwrap().into_values().find(|d| d.name == "Spare").unwrap();
    assert_eq!(spare.role, Some(fleet::Role::Backup));
    assert!(spare.log.last_scrub.as_ref().is_some_and(|c| c.ok));
}

/// Regression: `provision.apply` with a request ran its steps without the plan's checks, so an
/// archive drive was renamed and registered before writing its library was refused. Without a
/// format, the steps also never said which drive they were for.
#[test]
fn provisioning_an_archive_drive_is_refused_before_anything_is_written() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    locallibrary::testing::store_root_on_this_thread(Some(store.root().to_path_buf()));
    locallibrary::testing::volumes_on_this_thread(Some(vec![dir.path().to_path_buf()]));
    let ctx = Ctx::silent();
    let main = common::demo_drive(dir.path(), "Main");
    api::pull(&store, &api::PullRequest { drive: path(&main), ..Default::default() }, &ctx).unwrap();
    let vault = common::demo_drive(dir.path(), "Vault");
    common::adopt(Some(&store), &vault, "Vault");
    let req = api::ProvisionRequest {
        target: path(&vault),
        like: None,
        library: None,
        name: Some("Spare".into()),
        format: false,
        format_options: Default::default(),
        role: None,
        player_name: None,
        player_colour: None,
        settings: true,
        verify: true,
        eject: false,
    };
    let plan = api::provision_plan(&store, &req, &ctx).unwrap();
    let named = format!("give the drive at {} the name \"Spare\"", dunce_canonicalize(&vault).display());
    assert_eq!(plan.steps[0], named, "{:?}", plan.steps);

    fleet::set_role(&store, Device::open(&vault).unwrap().id().unwrap(), Some(fleet::Role::Archive)).unwrap();
    let err = api::provision_apply(&store, &req, None, &ctx).unwrap_err();
    assert!(matches!(err.inner(), Error::Conflict(m) if m.contains("archive")), "{err}");
    assert_eq!(Device::open(&vault).unwrap().name(), "Vault", "not renamed");
}
