//! Setting up a new drive from what the store holds, or from another drive: what it's refused
//! for, what its plan says, and a check afterwards that finds what went wrong.

use crate::common::{self, path};

use std::path::PathBuf;

use hegel::generators as gs;
use hegel::{HealthCheck, TestCase};
use locallibrary::api;
use locallibrary::device::{Device, dunce_canonicalize};
use locallibrary::progress::{CancelToken, Ctx, Event, Progress, Scope};
use locallibrary::store::Store;
use locallibrary::{Error, paths, player};

/// Damages a library file on the new drive as its check begins.
struct Damage(PathBuf);

impl Progress for Damage {
    fn on_event(&self, _: &Event) {}
    fn on_step(&self, scope: &Scope) {
        if scope.step.is_some_and(|s| s.id == "verify") {
            common::rot(&self.0.join(paths::EXPORT_PDB));
        }
    }
}

fn provision(target: &std::path::Path) -> api::ProvisionRequest {
    serde_json::from_value(serde_json::json!({ "target": path(target) })).unwrap()
}

/// A store with no libraries, or two, can't say which library a new drive gets unless it's
/// copied from a drive, and formatting is refused while the machine's disks are hidden. Copied
/// from Loose, a drive with both databases in no library, with a name and colour for players, it
/// gets Loose's library and players show the name; a file damaged before the check afterwards
/// stops it there, failed, with everything before it done.
#[hegel::test(test_cases = 4, suppress_health_check = [HealthCheck::TooSlow])]
#[hegel::explicit_test_case(name = Some("SPARE"), colour = Some(2i64), damaged = true)]
fn a_new_drive_is_set_up_from_a_drive_or_the_one_library(tc: TestCase) {
    let name = tc.draw(gs::optional(gs::sampled_from(vec!["SPARE", "B2"])));
    let colour = tc.draw(gs::optional(gs::integers::<i64>().min_value(1).max_value(8)));
    let damaged = tc.draw(gs::booleans());

    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    let volumes = common::isolate(dir.path(), Some(store.root()));
    let ctx = Ctx::silent();
    let target = dunce_canonicalize(&common::blank(&volumes, "New"));
    let req = provision(&target);
    let e = api::provision_plan(&store, &req, &ctx).map(|p| p.noop).unwrap_err();
    assert!(matches!(&e, Error::NotFound(m) if m.contains("no libraries yet")), "{e}");
    let formatted = api::ProvisionRequest { format: true, ..req.clone() };
    let e = api::provision_plan(&store, &formatted, &ctx).map(|p| p.noop).unwrap_err();
    assert!(e.to_string().contains("LOCALLIBRARY_VOLUMES"), "{e}");
    store.create_library("Home").unwrap();
    store.create_library("Club").unwrap();
    let e = api::provision_plan(&store, &req, &ctx).map(|p| p.noop).unwrap_err();
    assert!(e.to_string().contains("the store holds several libraries"), "{e}");

    let loose = common::demo_drive(&volumes, "Loose");
    common::add_onelibrary(&loose);
    let req = api::ProvisionRequest {
        like: Some(path(&loose)),
        player_name: name.map(String::from),
        player_colour: colour,
        ..provision(&target)
    };
    let plan = api::provision_plan(&store, &req, &ctx).unwrap();
    assert_eq!(plan.plan.steps[1], "copy Loose's library onto it");
    let players = name.is_some() || colour.is_some();
    assert_eq!(plan.plan.steps.contains(&"set the name and colour players show".to_string()), players);
    let damage = Damage(target.clone());
    let ctx = match damaged {
        true => Ctx::new(&damage, CancelToken::new()),
        false => Ctx::silent(),
    };
    let report = api::provision_apply(&store, &req, None, &ctx).unwrap();
    assert!(report.copy.is_some());
    assert_eq!(report.player.is_some(), players);
    if players {
        let shown = player::db_identity(&target).expect("a OneLibrary database");
        if name.is_some() {
            assert_eq!(shown.name.as_deref(), name);
        }
        if colour.is_some() {
            assert_eq!(shown.colour, colour);
        }
    }
    match damaged {
        true => {
            let stopped = report.stopped.as_ref().expect("the check failed");
            assert_eq!(stopped.step, "check it");
            assert!(report.verify.as_ref().is_some_and(|v| !v.is_clean()));
            assert!(report.drive.is_none(), "not handed over");
            assert_eq!(stopped.kind, "conflict");
            assert!(stopped.message.contains("some files didn't read back correctly"), "{}", stopped.message);
        }
        false => {
            assert!(report.stopped.is_none(), "{:?}", report.stopped);
            assert!(report.verify.as_ref().is_some_and(|v| v.is_clean()));
            assert_eq!(common::library(&target).tracks, common::library(&loose).tracks);
        }
    }
    assert!(Device::open(&target).unwrap().id().is_some(), "adopted first");
}

/// A new drive set up from the store: a role that isn't one, or a library the store lacks, is
/// refused before anything is written. Otherwise the plan writes the library's newest snapshot
/// (the only library's, or the one named in any case), then the My Settings profile when it holds
/// any, the role, and a check and an eject when asked; applied, the drive holds the library with
/// the settings and role, and an eject, which a folder can't take, stops it at the end.
#[hegel::test(test_cases = 6, suppress_health_check = [HealthCheck::TooSlow])]
#[hegel::explicit_test_case(role = Some("backup"), library = Some("main"), profile = true, verify = true, eject = true)]
fn prop_a_new_drive_gets_the_newest_snapshot_pushed(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let (store, main) = crate::props_api_drives::saved_copy(dir.path());
    let target = dunce_canonicalize(&common::blank(main.parent().unwrap(), "New"));
    let role = tc.draw(gs::sampled_from(vec![None, Some("backup"), Some("headliner")]));
    let library = tc.draw(gs::sampled_from(vec![None, Some("main"), Some("Radio")]));
    let profile = tc.draw(gs::booleans());
    let verify = tc.draw(gs::booleans());
    let eject = tc.draw(gs::booleans());
    if profile {
        let values = indexmap::IndexMap::from([("quantize".to_string(), "off".to_string())]);
        api::settings_set(&store, &api::SetSettings { values, drive: None }).unwrap();
    }
    let req = api::ProvisionRequest {
        library: library.map(String::from),
        role: role.map(String::from),
        verify,
        eject,
        ..provision(&target)
    };
    let ctx = Ctx::silent();
    let plan = api::provision_plan(&store, &req, &ctx);
    if role == Some("headliner") {
        assert!(matches!(plan, Err(Error::Invalid(_))), "not a role");
        return;
    }
    if library == Some("Radio") {
        assert!(matches!(&plan, Err(Error::NotFound(m)) if m.contains("\"Radio\"")), "{:?}", plan.map(|p| p.noop));
        return;
    }
    let steps = plan.unwrap().plan.steps;
    assert!(steps[1].starts_with("write \"Main\"'s latest snapshot ("), "{steps:?}");
    let has = |s: &str| steps.iter().any(|x| x == s);
    assert_eq!(has("write the My Settings profile"), profile, "{steps:?}");
    assert_eq!(has("mark it as a backup drive"), role.is_some(), "{steps:?}");
    assert_eq!((has("read every file back and check it"), has("eject it")), (verify, eject), "{steps:?}");

    let report = api::provision_apply(&store, &req, None, &ctx).unwrap();
    assert_eq!(report.settings.is_some(), profile);
    assert_eq!(report.verify.as_ref().map(|v| v.is_clean()), verify.then_some(true));
    assert_eq!(report.stopped.as_ref().map(|s| s.step.as_str()), eject.then_some("eject it"), "{:?}", report.stopped);
    assert_eq!(common::library(&target).tracks, common::library(&main).tracks);
    let id = Device::open(&target).unwrap().id().unwrap();
    let recorded = store.drives().unwrap().shift_remove(&id).and_then(|r| r.role);
    assert_eq!(recorded, role.map(|_| locallibrary::fleet::Role::Backup));
}
