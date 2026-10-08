//! The drives you own, through the service layer: the fleet view with copies and drives that
//! aren't mounted, roles, retiring and forgetting, renaming, giving a drive back its identity or
//! one of its own, and syncing a library's drive group.

use crate::common::{self, path};

use std::path::{Path, PathBuf};

use locallibrary::api::{self, DriveRef};
use locallibrary::device::{Device, dunce_canonicalize};
use locallibrary::error::Reason;
use locallibrary::fleet::Role;
use locallibrary::progress::Ctx;
use locallibrary::store::Store;
use locallibrary::{Error, Library};

fn drive(spec: &str) -> DriveRef {
    DriveRef { drive: spec.into() }
}

/// A store named for this thread, and drives looked for only in `volumes`.
struct Fleet {
    dir: tempfile::TempDir,
    store: Store,
    volumes: PathBuf,
}

impl Fleet {
    fn new() -> Fleet {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open_or_init(dir.path().join("store")).unwrap();
        locallibrary::testing::store_root_on_this_thread(Some(store.root().to_path_buf()));
        let volumes = dir.path().join("volumes");
        std::fs::create_dir(&volumes).unwrap();
        locallibrary::testing::volumes_on_this_thread(Some(vec![volumes.clone()]));
        Fleet { dir, store, volumes }
    }

    /// A demo drive in the volumes folder, adopted as `name` and saved in the store.
    fn pulled(&self, name: &str) -> PathBuf {
        let root = common::demo_drive(&self.volumes, name);
        self.adopt(&root, name);
        let req = api::PullRequest { drive: path(&root), ..Default::default() };
        api::pull(&self.store, &req, &Ctx::silent()).unwrap();
        root
    }

    fn adopt(&self, root: &Path, name: &str) {
        common::adopt(Some(&self.store), root, name);
    }
}

/// The fleet lists each mounted drive with the others sharing its identity, and with `all` the
/// registered drives that aren't mounted. Retired drives are listed only when asked for.
#[test]
fn the_fleet_shows_copies_and_drives_that_arent_mounted() {
    let f = Fleet::new();
    let main = f.pulled("Main");
    let copy = f.volumes.join("Copy");
    common::copy_dir(&main, &copy);
    // Registered, but outside the volumes folder: not mounted, as far as the API can tell.
    let away = common::demo_drive(f.dir.path(), "Away");
    f.adopt(&away, "Away");

    let status = api::fleet_status(&f.store, &api::FleetRequest::default(), &Ctx::silent()).unwrap();
    assert_eq!(status.mounted.len(), 2);
    assert_eq!(status.duplicates.len(), 1);
    let (main, copy) = (dunce_canonicalize(&main), dunce_canonicalize(&copy));
    let of = |root: &Path| status.mounted.iter().find(|d| d.status.drive.root == root).unwrap();
    assert_eq!(of(&main).same_identity_as, std::slice::from_ref(&copy));
    assert_eq!(of(&copy).same_identity_as, std::slice::from_ref(&main));
    assert_eq!(of(&main).log.as_ref().map(|l| l.writes), Some(0), "registered");
    assert!(!of(&main).low_space);
    assert!(status.offline.is_empty(), "only with all");

    let all = api::FleetRequest { all: true, retired: false };
    let offline = api::fleet_status(&f.store, &all, &Ctx::silent()).unwrap().offline;
    assert_eq!(offline.iter().map(|k| k.name.as_str()).collect::<Vec<_>>(), ["Away"]);

    let known = |retired| {
        let req = api::FleetRequest { all: true, retired };
        api::drives_known(&f.store, &req).unwrap().into_iter().map(|k| k.name).collect::<Vec<_>>()
    };
    assert_eq!(known(false), ["Main", "Away"], "mounted ones first");
    let retired = api::drive_retire(&f.store, &api::RetireDrive { drive: "Away".into(), undo: false }).unwrap();
    assert!(retired.log.retired.is_some());
    assert_eq!(known(false), ["Main"]);
    assert_eq!(known(true), ["Main", "Away"]);
    api::drive_retire(&f.store, &api::RetireDrive { drive: "Away".into(), undo: true }).unwrap();
    assert_eq!(known(false), ["Main", "Away"]);

    // A role for "Main" can't say which of the two it means; the drive away is shown as registered.
    let role = api::SetRole { drive: "Main".into(), role: Some("backup".into()) };
    assert!(api::drive_role(&f.store, &role).unwrap_err().to_string().contains("matches 2 drives"));
    assert_eq!(api::drive_to_change(&f.store, &drive("Away")).unwrap().last_mount, dunce_canonicalize(&away));

    let mounted: Vec<PathBuf> = api::drives_mounted().into_iter().map(|m| m.root).collect();
    assert!(mounted.contains(&main) && mounted.contains(&copy), "{mounted:?}");
}

/// Forgetting a drive that isn't mounted takes it out of the registry by its name; a mounted,
/// adopted drive the registry doesn't have can't be forgotten, and says where it is.
#[test]
fn forgetting_a_drive_takes_it_out_of_the_registry() {
    let f = Fleet::new();
    let away = common::demo_drive(f.dir.path(), "Away");
    f.adopt(&away, "Away");
    let forgotten = api::drive_forget(&f.store, &drive("away")).unwrap();
    assert_eq!(forgotten.name, "Away");
    assert!(matches!(api::drive_log(&f.store, &drive("Away")), Err(Error::NotFound(_))));

    let loose = common::demo_drive(&f.volumes, "Loose");
    common::adopt(None, &loose, "Loose");
    let err = api::drive_forget(&f.store, &drive("Loose")).unwrap_err();
    assert!(matches!(&err, Error::NotFound(m) if m.contains("Loose (at ")), "{err}");
}

/// A role is set on a mounted drive the registry doesn't have yet, registering it, and cleared
/// again; one that isn't a role is refused before anything is written.
#[test]
fn roles_are_set_and_cleared_and_unknown_ones_refused() {
    let f = Fleet::new();
    let stick = common::demo_drive(&f.volumes, "Stick");
    common::adopt(None, &stick, "Stick");
    let set = |role: Option<&str>| {
        api::drive_role(&f.store, &api::SetRole { drive: "Stick".into(), role: role.map(Into::into) })
    };

    let err = set(Some("headliner")).unwrap_err();
    assert!(err.to_string().contains("unknown role"), "{err}");
    assert!(f.store.drives().unwrap().is_empty(), "nothing is registered");

    assert_eq!(set(Some("gig")).unwrap().role, Some(Role::Set), "the old name still works");
    assert_eq!(f.store.drives().unwrap().values().next().unwrap().role, Some(Role::Set));
    assert_eq!(set(None).unwrap().role, None);
    let shown = api::drive_to_change(&f.store, &drive("Stick")).unwrap();
    assert_eq!((shown.name.as_str(), shown.role), ("Stick", None));
}

/// A rename is planned first: an empty name is refused, the same name is nothing to do, and
/// with `label` the plan names the volume label it would write.
#[test]
fn renaming_is_planned_and_applied() {
    let f = Fleet::new();
    let main = f.pulled("Main");
    let rename = |name: &str, label: bool| api::RenameDrive { drive: path(&main), name: name.into(), label };

    assert!(api::drive_rename_plan(&rename("  ", false)).unwrap_err().to_string().contains("empty"));
    assert!(api::drive_rename_plan(&rename("Main", false)).unwrap().noop);
    let labelled = api::drive_rename_plan(&rename("Club night", true)).unwrap();
    assert_eq!(labelled.label.as_deref(), Some("CLUB NIGHT"));
    assert_eq!(labelled.summary.len(), 2, "{:?}", labelled.summary);

    let report = api::drive_rename(Some(&f.store), &rename("Club", false)).unwrap();
    assert_eq!((report.old_name.as_str(), report.name.as_str(), report.label), ("Main", "Club", None));
    assert_eq!(api::drive_log(&f.store, &drive("Club")).unwrap().name, "Club", "the registry follows");
}

/// A drive whose identity was lost is recognised by its library, and given that identity back
/// as planned. Without the store the plan can't be applied.
#[test]
fn a_drive_that_lost_its_identity_gets_it_back() {
    let f = Fleet::new();
    let main = f.pulled("Main");
    let id = Device::open(&main).unwrap().id().unwrap();
    std::fs::remove_dir_all(main.join(".locallibrary")).unwrap();

    let seen = api::drive_recognise(&f.store, &drive(&path(&main)), &Ctx::silent()).unwrap();
    assert_eq!(seen.best().unwrap().id, id);
    let req = api::AdoptAs { drive: path(&main), known: "Main".into() };
    let plan = api::drive_adopt_as_plan(&f.store, &req).unwrap();
    assert_eq!(plan.known.id, id);
    assert!(plan.summary[0].contains(&id.to_string()), "{:?}", plan.summary);
    let err = plan.apply(None, &Ctx::silent()).unwrap_err();
    assert!(err.to_string().contains("store"), "{err}");

    let info = api::drive_adopt_as(&f.store, &req, &Ctx::silent()).unwrap();
    assert_eq!((info.id, info.name.as_deref()), (Some(id), Some("Main")));
}

/// A copy of a drive is given an identity of its own only by its path, and only when something
/// shows it's a copy (unless forced). The role stays with the original's identity, and the plan
/// says so.
#[test]
fn a_copy_gets_an_identity_of_its_own() {
    let f = Fleet::new();
    let main = f.pulled("Main");
    api::drive_role(&f.store, &api::SetRole { drive: path(&main), role: Some("primary".into()) }).unwrap();
    let reidentify = |drive: &str, force: bool| api::Reidentify { drive: drive.into(), name: None, force };

    let err = api::drive_reidentify_plan(Some(&f.store), &reidentify("Main", false)).unwrap_err();
    assert!(err.to_string().contains("mount path"), "{err}");
    let err = api::drive_reidentify_plan(Some(&f.store), &reidentify(&path(&main), false)).unwrap_err();
    assert_eq!(err.reason(), Some(Reason::HasIdentity), "{err}");
    let blank = f.volumes.join("Blank");
    std::fs::create_dir(&blank).unwrap();
    let err = api::drive_reidentify_plan(Some(&f.store), &reidentify(&path(&blank), true)).unwrap_err();
    assert!(err.to_string().contains("no identity"), "{err}");

    let copy = f.volumes.join("Copy");
    common::copy_dir(&main, &copy);
    let plan = api::drive_reidentify_plan(Some(&f.store), &reidentify(&path(&copy), false)).unwrap();
    assert_eq!(plan.name, "Main (copy)");
    assert!(plan.warnings.iter().any(|w| w.contains("primary")), "{:?}", plan.warnings);
    let info = api::drive_reidentify(Some(&f.store), &reidentify(&path(&copy), false)).unwrap();
    let original = Device::open(&main).unwrap().id();
    assert!(info.id.is_some() && info.id != original);
    assert_eq!(info.name.as_deref(), Some("Main (copy)"));
}

/// A library's drive group is synced from its primary: with one group mounted it needn't be
/// named; with none, or with an unknown library, the request is refused.
#[test]
fn a_drive_group_is_synced_from_its_primary() {
    let f = Fleet::new();
    let none = api::SyncSetRequest::default();
    let err = api::sync_set_plan(&f.store, &none, &Ctx::silent()).unwrap_err();
    assert!(matches!(&err, Error::NotFound(m) if m.contains("two or more")), "{err}");

    let main = f.pulled("Main");
    let backup = common::blank(&f.volumes, "Backup");
    api::sync_apply(Some(&f.store), &common::sync_pair(&main, &backup), &Ctx::silent()).unwrap();
    f.adopt(&backup, "Backup");
    api::drive_role(&f.store, &api::SetRole { drive: "Backup".into(), role: Some("primary".into()) }).unwrap();
    assert_eq!(api::drive_sets(&f.store).unwrap()[0].members.len(), 2);

    let unknown = api::SyncSetRequest { library: Some("Nope".into()), ..Default::default() };
    assert!(matches!(api::sync_set_plan(&f.store, &unknown, &Ctx::silent()), Err(Error::NotFound(_))));
    let resolved = none.resolve(&f.store).unwrap();
    assert_eq!(resolved.drives[0], path(&dunce_canonicalize(&backup)), "the primary first");

    // The primary moves on; syncing the group brings Main up to it.
    common::export_new_track(&backup, "Artist", "Fresh");
    let named = api::SyncSetRequest { library: Some("Main".into()), ..Default::default() };
    let plan = api::sync_set_plan(&f.store, &named, &Ctx::silent()).unwrap();
    assert_eq!(plan.operation, "sync.group");
    assert_eq!(plan.source, dunce_canonicalize(&backup));
    let results = api::sync_set_apply(&f.store, &named, &Ctx::silent()).unwrap();
    assert!(results.iter().all(|r| r.error.is_none()), "{results:?}");
    let titles = |root: &Path| -> Vec<String> {
        let lib: Library = Device::open(root).unwrap().library().unwrap();
        lib.tracks.into_iter().map(|t| t.title).collect()
    };
    assert!(titles(&main).contains(&"Fresh".to_string()));
}

/// Drives of two libraries mounted: the group to sync has to be chosen, and the refusal names
/// the libraries.
#[test]
fn a_drive_group_must_be_chosen_among_several() {
    let f = Fleet::new();
    for name in ["One", "Two"] {
        let first = f.pulled(name);
        let second = f.volumes.join(format!("{name} copy"));
        common::copy_dir(&first, &second);
        api::drive_reidentify(Some(&f.store), &api::Reidentify { drive: path(&second), name: None, force: false })
            .unwrap();
    }
    let err = api::sync_set_plan(&f.store, &api::SyncSetRequest::default(), &Ctx::silent()).unwrap_err();
    assert_eq!(err.reason(), Some(Reason::ChooseLibrary), "{err}");
    assert!(err.to_string().contains("One") && err.to_string().contains("Two"), "{err}");
}
