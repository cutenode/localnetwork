//! My Settings through the service layer: the profile's base and per-drive overrides (for a drive
//! mounted or only registered), what a drive holds against it, writing it to drives (tried first,
//! or planned for several), and importing a drive's settings into it.

use crate::common::{self, path};

use indexmap::IndexMap;
use std::path::{Path, PathBuf};

use locallibrary::Error;
use locallibrary::api::{self, DriveRef};
use locallibrary::device::Device;
use locallibrary::progress::Ctx;
use locallibrary::settings::SettingValue;
use locallibrary::store::Store;

const MYSETTING: &str = "PIONEER/MYSETTING.DAT";

/// A store named for this thread, drives looked for in `volumes` only, and an adopted demo drive
/// `Main` there.
struct Setup {
    dir: tempfile::TempDir,
    store: Store,
    volumes: PathBuf,
    main: PathBuf,
}

fn setup() -> Setup {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    locallibrary::testing::store_root_on_this_thread(Some(store.root().to_path_buf()));
    let volumes = dir.path().join("volumes");
    let main = common::demo_drive(&volumes, "Main");
    locallibrary::testing::volumes_on_this_thread(Some(vec![volumes.clone()]));
    common::adopt(None, &main, "Main");
    Setup { dir, store, volumes, main }
}

fn values(pairs: &[(&str, &str)]) -> IndexMap<String, String> {
    pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
}

/// An override for a mounted drive registers it; one for a drive that isn't mounted is found in
/// the registry by its name; one for a drive known nowhere is refused. Unsetting takes it off.
#[test]
fn overrides_are_kept_by_the_drives_identity() {
    let s = setup();
    let set = |drive: &str, pairs| {
        api::settings_set(&s.store, &api::SetSettings { values: values(pairs), drive: Some(drive.into()) })
    };
    let profile = set("Main", &[("language", "japanese")]).unwrap();
    let id = Device::open(&s.main).unwrap().id().unwrap();
    assert_eq!(profile.drives[&id].values, values(&[("language", "japanese")]));
    assert!(s.store.drives().unwrap().contains_key(&id), "registered on the way");

    let away = common::demo_drive(s.dir.path(), "Away");
    common::adopt(Some(&s.store), &away, "Away");
    let away_id = Device::open(&away).unwrap().id().unwrap();
    assert!(set("away", &[("quantize", "off")]).unwrap().drives.contains_key(&away_id));
    let err = set("Nowhere", &[("quantize", "off")]).unwrap_err();
    assert!(matches!(&err, Error::NotFound(m) if m.contains("Nowhere")), "{err}");

    let unset = api::UnsetSettings { keys: vec!["language".into()], drive: Some("Main".into()) };
    let profile = api::settings_unset(&s.store, &unset).unwrap();
    assert!(profile.drives.get(&id).is_none_or(|o| o.values.is_empty()), "{profile:?}");
}

/// A drive's settings are shown against the profile; a dry run says what writing would change
/// and leaves the file alone, and writing changes just that.
#[test]
fn a_drive_is_shown_against_the_profile_and_written_after_a_dry_run() {
    let s = setup();
    api::settings_set(&s.store, &api::SetSettings { values: values(&[("quantize", "off")]), drive: None }).unwrap();
    let main = DriveRef { drive: "Main".into() };
    let shown = api::settings_show(Some(&s.store), &main).unwrap();
    assert_eq!(shown.drift.iter().map(|d| d.key.as_str()).collect::<Vec<_>>(), ["quantize"]);

    let before = std::fs::read(s.main.join(MYSETTING)).unwrap();
    let apply = |dry_run| api::ApplySettings { drive: "Main".into(), dry_run, create_missing: false };
    let dry = api::settings_apply(&s.store, &apply(true)).unwrap();
    assert!(dry.dry_run);
    assert_eq!(dry.changes.len(), 1);
    assert_eq!(std::fs::read(s.main.join(MYSETTING)).unwrap(), before);
    assert_eq!(api::settings_apply(&s.store, &apply(false)).unwrap().changes.len(), 1);
    let shown = api::settings_show(Some(&s.store), &main).unwrap();
    assert!(shown.drift.is_empty());
    assert_eq!(shown.current.values["quantize"], SettingValue::Known("off".into()));
}

/// Writing the profile to several drives is planned per drive, says which already match, and
/// needs the store to apply; with no drive mounted there's nothing to plan.
#[test]
fn writing_to_several_drives_is_planned_per_drive() {
    let s = setup();
    api::settings_set(&s.store, &api::SetSettings { values: values(&[("quantize", "off")]), drive: None }).unwrap();
    let other = common::demo_drive(&s.volumes, "Other");
    let both = api::SettingsApplyRequest { drives: vec!["Main".into(), path(&other)], create_missing: false };
    api::settings_apply(&s.store, &api::ApplySettings { drive: path(&other), dry_run: false, create_missing: false })
        .unwrap();

    let plan = api::settings_apply_plan(&s.store, &both).unwrap();
    assert_eq!(plan.summary, ["Main: 1 setting(s) in 1 file(s)", "Other: already as the profile has it"]);
    assert!(!plan.noop);
    let err = plan.apply(None, &Ctx::silent()).unwrap_err();
    assert!(err.to_string().contains("needs the store"), "{err}");
    let results = plan.apply(Some(&s.store), &Ctx::silent()).unwrap();
    assert_eq!(results.iter().map(|r| r.drive.as_str()).collect::<Vec<_>>(), ["Main"], "Other needed nothing");
    assert!(results[0].error.is_none());
    assert!(api::settings_apply_plan(&s.store, &both).unwrap().noop);

    locallibrary::testing::volumes_on_this_thread(Some(vec![s.dir.path().join("nothing")]));
    let none = api::settings_apply_plan(&s.store, &api::SettingsApplyRequest::default());
    assert!(matches!(none, Err(Error::NotFound(_))));
}

/// A drive's settings become the profile's base, planned first; a drive without settings files
/// has none to give.
#[test]
fn a_drives_settings_are_imported_into_the_base() {
    let s = setup();
    let from = |drive: &Path| api::ImportSettings { drive: Some(path(drive)) };
    let plan = api::settings_import_plan(&s.store, &from(&s.main)).unwrap();
    assert!(
        plan.values > 0 && plan.changes.len() == plan.values,
        "{} values, {} changes",
        plan.values,
        plan.changes.len()
    );
    let err = plan.apply(None, &Ctx::silent()).unwrap_err();
    assert!(err.to_string().contains("needs the store"), "{err}");
    let profile = plan.apply(Some(&s.store), &Ctx::silent()).unwrap();
    assert_eq!(profile.base.len(), plan.values);
    assert!(api::settings_import_plan(&s.store, &from(&s.main)).unwrap().noop);

    let blank = s.volumes.join("Blank");
    std::fs::create_dir(&blank).unwrap();
    let err = api::settings_import_plan(&s.store, &from(&blank)).unwrap_err();
    assert!(matches!(&err, Error::NotFound(m) if m.contains("no settings files")), "{err}");
}
