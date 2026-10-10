//! Setting up a new drive through the service layer: copied from another drive or pushed from the
//! store, with My Settings, a role and the name players show; where the steps can't run here
//! (formatting, ejecting), the plan and the report say so.

use crate::common::{self, path};

use std::path::{Path, PathBuf};

use locallibrary::Error;
use locallibrary::api::{self, ProvisionRequest};
use locallibrary::device::Device;
use locallibrary::fleet::Role;
use locallibrary::progress::Ctx;
use locallibrary::store::Store;

/// A store named for this thread, drives looked for in `dir`, and an empty folder `New` to set up.
fn setup(dir: &Path) -> (Store, PathBuf) {
    let store = Store::open_or_init(dir.join("store")).unwrap();
    locallibrary::testing::store_root_on_this_thread(Some(store.root().to_path_buf()));
    locallibrary::testing::volumes_on_this_thread(Some(vec![dir.to_path_buf()]));
    let new = dir.join("New");
    std::fs::create_dir(&new).unwrap();
    (store, new)
}

fn request(target: &Path) -> ProvisionRequest {
    ProvisionRequest {
        target: path(target),
        like: None,
        library: None,
        name: Some("Spare".into()),
        format: false,
        format_options: Default::default(),
        role: None,
        player_name: None,
        player_colour: None,
        settings: true,
        verify: false,
        eject: false,
    }
}

fn pull(store: &Store, root: &Path, library: &str) {
    let req =
        api::PullRequest { drive: path(root), library: Some(library.into()), new_library: true, ..Default::default() };
    api::pull(store, &req, &Ctx::silent()).unwrap();
}

/// A drive copied from another one gets My Settings, its role and the name players show, each
/// planned as a step and reported.
#[test]
fn a_drive_is_set_up_like_another_with_settings_role_and_player_name() {
    let dir = tempfile::tempdir().unwrap();
    let (store, new) = setup(dir.path());
    let main = common::demo_drive(dir.path(), "Main");
    common::add_onelibrary(&main);
    let quantize = [("quantize".to_string(), "off".to_string())].into();
    api::settings_set(&store, &api::SetSettings { values: quantize, drive: None }).unwrap();
    let req = ProvisionRequest {
        like: Some(path(&main)),
        role: Some("set".into()),
        player_name: Some("SPARE".into()),
        player_colour: Some(2),
        ..request(&new)
    };

    let plan = api::provision_plan(&store, &req, &Ctx::silent()).unwrap();
    let steps = &plan.steps[1..];
    assert_eq!(
        steps,
        [
            "copy Main's library onto it",
            "write the My Settings profile",
            "mark it as a set drive",
            "set the name and colour players show",
        ]
    );
    assert!(!plan.destructive, "the folder held no library");

    let report = api::provision_apply(&store, &req, None, &Ctx::silent()).unwrap();
    assert!(report.stopped.is_none(), "{:?}", report.stopped);
    assert!(report.copy.is_some_and(|c| c.copied > 0));
    assert!(report.settings.is_some_and(|s| !s.changes.is_empty()));
    assert_eq!(report.player.and_then(|p| p.onelibrary).and_then(|o| o.name).as_deref(), Some("SPARE"));
    assert_eq!(report.drive.and_then(|d| d.name).as_deref(), Some("Spare"));
    let id = Device::open(&new).unwrap().id().unwrap();
    assert_eq!(store.drives().unwrap()[&id].role, Some(Role::Set));
}

/// Pushing from the store needs a library to push: one there is, or the one named. With none,
/// several, or an unknown name, the plan says what to do instead. A role that isn't one is
/// refused too, and a drive holding a library is warned about.
#[test]
fn the_library_to_push_must_be_clear() {
    let dir = tempfile::tempdir().unwrap();
    let (store, new) = setup(dir.path());
    let plan = |req: &ProvisionRequest| api::provision_plan(&store, req, &Ctx::silent());

    let err = plan(&request(&new)).unwrap_err();
    assert!(matches!(&err, Error::NotFound(m) if m.contains("no libraries yet")), "{err}");
    let err = plan(&ProvisionRequest { role: Some("dj".into()), ..request(&new) }).unwrap_err();
    assert!(err.to_string().contains("unknown role"), "{err}");

    let home = common::demo_drive(dir.path(), "Home");
    pull(&store, &home, "Home");
    let over = plan(&request(&home)).unwrap();
    assert!(over.destructive && over.warnings[0].contains("already holds a rekordbox library"), "{:?}", over.warnings);
    pull(&store, &common::bigger_drive(dir.path(), "Club"), "Club");
    let err = plan(&request(&new)).unwrap_err();
    assert!(err.to_string().contains("several libraries"), "{err}");
    let unknown = ProvisionRequest { library: Some("Radio".into()), ..request(&new) };
    assert!(matches!(plan(&unknown), Err(Error::NotFound(m)) if m.contains("Radio")));
    let named = ProvisionRequest { library: Some("Club".into()), role: Some("archive".into()), ..request(&new) };
    let named = plan(&named).unwrap();
    assert!(named.steps[1].starts_with("write \"Club\"'s latest snapshot"), "{:?}", named.steps);
    assert!(named.steps[1].contains("12 tracks"), "{:?}", named.steps);
    // Regression: the step read "mark it as a archive drive".
    assert!(named.steps.contains(&"mark it as an archive drive".to_string()), "{:?}", named.steps);
}

/// Formatting can't be planned while drives are looked for only in test folders, and ejecting a
/// folder can't be done: the plan warns about ejecting, and the report says the setup stopped
/// there, with the steps before it done.
#[test]
fn a_setup_that_cant_eject_stops_there_with_the_rest_done() {
    let dir = tempfile::tempdir().unwrap();
    let (store, new) = setup(dir.path());
    pull(&store, &common::demo_drive(dir.path(), "Home"), "Home");

    let err =
        api::provision_plan(&store, &ProvisionRequest { format: true, ..request(&new) }, &Ctx::silent()).unwrap_err();
    assert!(err.to_string().contains("LOCALLIBRARY_VOLUMES"), "{err}");

    let req = ProvisionRequest { eject: true, verify: true, ..request(&new) };
    let plan = api::provision_plan(&store, &req, &Ctx::silent()).unwrap();
    assert_eq!(plan.steps.last().map(String::as_str), Some("eject it"));
    let report = api::provision_apply(&store, &req, None, &Ctx::silent()).unwrap();
    let stopped = report.stopped.expect("a folder can't be ejected");
    assert_eq!((stopped.step.as_str(), stopped.kind), ("eject it", "invalid"));
    assert!(report.verify.is_some_and(|v| v.is_clean()));
    assert_eq!(report.drive.and_then(|d| d.name).as_deref(), Some("Spare"));
}

/// A request that doesn't say writes My Settings and checks the drive afterwards, as the RPC
/// contract promises.
#[test]
fn settings_and_the_check_are_on_unless_turned_off() {
    let req: ProvisionRequest = serde_json::from_value(serde_json::json!({ "target": "/Volumes/NEW" })).unwrap();
    assert!(req.settings && req.verify);
    assert!(!req.format && !req.eject);
}
