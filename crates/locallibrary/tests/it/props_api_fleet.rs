//! The registry's drives as their requests name them: renaming with the volume label, giving a
//! copy an identity of its own (and only a copy, unless forced), a role for a drive that isn't
//! plugged in, a name a mounted drive shares with a registered one, groups of two libraries, and
//! the fleet view of a library with no drive mounted.

use crate::common::{self, path};
use crate::props_api_drives::{saved_copy, saved_demo};
use crate::props_api_jobs::{bare_export, own_identity};

use hegel::generators as gs;
use hegel::{HealthCheck, TestCase};
use locallibrary::api;
use locallibrary::device::{Device, dunce_canonicalize};
use locallibrary::error::Reason;
use locallibrary::fleet::{self, Role};
use locallibrary::progress::Ctx;
use locallibrary::sync;

/// Main and Backup (clones of the library Main), Club A and Club B (drives of Club), and a copy
/// of Main made outside locallibrary, its identity and all, with Main's role drawn: renaming plans
/// the volume label too when asked, as the label FAT allows; Main, which nothing shows is a copy,
/// gets a new identity only when forced, and the copy gets one, its original keeping the role;
/// with two groups mounted a group sync must be told which; Club B, unplugged, still takes a role
/// by name; a mounted Stick the registry lacks is refused by a name it shares with a registered
/// Stick; a check against other drives leaves the drive itself out; and the fleet view leaves
/// out a library with no drive mounted.
#[hegel::test(test_cases = 4, suppress_health_check = [HealthCheck::TooSlow])]
#[hegel::explicit_test_case(name = "Main 2".to_string(), role = Some(Role::Primary), label = true)]
fn registry_requests_name_the_drives_they_mean(tc: TestCase) {
    let name = tc.draw(gs::sampled_from(vec!["Main 2".to_string(), "Gig stick".to_string(), "  Home  ".to_string()]));
    let role = tc.draw(gs::sampled_from(vec![None, Some(Role::Primary), Some(Role::Backup)]));
    let label = tc.draw(gs::booleans());

    let dir = tempfile::tempdir().unwrap();
    let ctx = Ctx::silent();
    let (store, main) = saved_copy(dir.path());
    let volumes = dunce_canonicalize(main.parent().unwrap());
    let backup = volumes.join("Backup");
    saved_demo().1.write(&backup);
    own_identity(&store, &mut Device::open(&backup).unwrap(), "Backup");
    let club = store.create_library("Club").unwrap().id;
    let clubs = ["Club A", "Club B"].map(|n| {
        let root = volumes.join(n);
        bare_export(&root);
        let mut d = Device::open(&root).unwrap();
        d.adopt(Some(n)).unwrap();
        d.state_mut().unwrap().library_id = Some(club);
        d.save_state().unwrap();
        sync::register_drive(&store, &d, None).unwrap();
        root
    });
    let main_id = Device::open(&main).unwrap().id().unwrap();
    fleet::set_role(&store, main_id, role).unwrap();

    // Renaming, with the volume label when asked.
    let req = api::RenameDrive { drive: path(&main), name: name.clone(), label };
    let plan = api::drive_rename_plan(&req).unwrap();
    assert_eq!(plan.summary[0], format!("rename Main to {:?}", name.trim()));
    let labelled = format!("change the volume label of {} to {:?}", main.display(), fleet::volume_label(name.trim()));
    assert_eq!(plan.summary.get(1), label.then_some(&labelled));
    assert_eq!(plan.plan.label, label.then(|| fleet::volume_label(name.trim())));
    assert!(fleet::volume_label(name.trim()).chars().all(|c| !c.is_ascii_lowercase()), "FAT labels are upper case");

    // An identity of its own: only for a copy, unless forced.
    let not_a_copy = api::Reidentify { drive: path(&main), name: None, force: false };
    let e = api::drive_reidentify_plan(Some(&store), &not_a_copy).map(|p| p.noop).unwrap_err();
    assert_eq!(e.reason(), Some(Reason::HasIdentity), "{e}");
    assert_eq!(e.detail().actions[0].params["force"], true);
    let copy = volumes.join("Main copy");
    common::copy_dir(&main, &copy);
    let req = api::Reidentify { drive: path(&copy), name: Some("Spare".into()), force: false };
    let plan = api::drive_reidentify_plan(Some(&store), &req).unwrap();
    let stays = role
        .map(|r| format!("its role ({}) stays with Main's identity: give the copy one with `drive role`", r.name()));
    assert_eq!(plan.warnings.first(), stays.as_ref());
    let info = api::drive_reidentify(Some(&store), &req).unwrap();
    assert_ne!(info.id, Some(main_id));
    assert_eq!(info.name.as_deref(), Some("Spare"));
    assert_eq!(store.drives().unwrap()[&main_id].role, role, "the role stays with the original");
    std::fs::remove_dir_all(&copy).unwrap();

    // Two groups mounted: which to sync is asked.
    let e = api::SyncSetRequest::default().resolve(&store).unwrap_err();
    assert_eq!(e.reason(), Some(Reason::ChooseLibrary), "{e}");
    assert!(e.to_string().contains("drives of 2 libraries are mounted"), "{e}");
    for lib in ["Club", "Main"] {
        assert!(e.to_string().contains(lib), "{e} names {lib}");
    }

    // Club B unplugged: a role by its name still reaches its record.
    let away = dir.path().join("away");
    std::fs::create_dir(&away).unwrap();
    std::fs::rename(&clubs[1], away.join("Club B")).unwrap();
    let rec = api::drive_role(&store, &api::SetRole { drive: "club b".into(), role: Some("archive".into()) }).unwrap();
    assert_eq!((rec.name.as_str(), rec.role), ("Club B", Some(Role::Archive)));

    // A Stick the registry has, unplugged, and another mounted that it lacks.
    let stick = volumes.join("Stick");
    std::fs::create_dir(&stick).unwrap();
    common::adopt(Some(&store), &stick, "Stick");
    std::fs::rename(&stick, away.join("Stick")).unwrap();
    std::fs::create_dir(&stick).unwrap();
    Device::open(&stick).unwrap().adopt(Some("Stick")).unwrap();
    let e = api::drive_find(&store, &api::DriveRef { drive: "Stick".into() }).unwrap_err();
    assert_eq!(e.reason(), Some(Reason::ChooseDrive), "{e}");
    assert!(
        e.to_string().contains("which the registry doesn't have") && e.to_string().contains("the registered Stick"),
        "{e}"
    );

    // Checked against Backup (and itself, which is left out).
    let req = api::VerifyRequest { drive: path(&main), snapshot: None, against: vec![path(&backup), path(&main)] };
    let report = api::drive_verify(Some(&store), &req, &ctx).unwrap();
    assert!(report.is_clean(), "{report:?}");

    // A library with no drive mounted isn't in the fleet view.
    store.create_library("Radio").unwrap();
    let fleet = api::fleet_status(Some(&store), &api::FleetRequest { all: true, retired: false }, &ctx).unwrap();
    let names: Vec<&str> = fleet.libraries.iter().map(|l| l.name.as_str()).collect();
    assert_eq!(names, ["Club", "Main"]);
}
