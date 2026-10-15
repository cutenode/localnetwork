//! A library's drives are clones of it: a write to the library is made on its lead clone and
//! copied onto the others, one plan for all of them, and a sync brings the edits recorded on a
//! drive along with its library.

use std::path::{Path, PathBuf};

use crate::common::{self, path};

use locallibrary::api::{self, StepKind};
use locallibrary::device::{Device, dunce_canonicalize};
use locallibrary::edit::{Edit, EditOptions, journal};
use locallibrary::fleet::{self, Role};
use locallibrary::progress::Ctx;
use locallibrary::store::Store;

/// A store whose library Home has three clones mounted: Main (the primary), Backup and SD; and
/// Archive, a fourth, kept as an archive.
pub(crate) struct Home {
    pub(crate) _dir: tempfile::TempDir,
    pub(crate) store: Store,
    pub(crate) main: PathBuf,
    pub(crate) backup: PathBuf,
    pub(crate) sd: PathBuf,
    pub(crate) archive: PathBuf,
}

pub(crate) fn home() -> Home {
    let dir = tempfile::tempdir().unwrap();
    let volumes = dir.path().join("volumes");
    std::fs::create_dir_all(&volumes).unwrap();
    locallibrary::testing::volumes_on_this_thread(Some(vec![volumes.clone()]));
    let store_root = dir.path().join("store");
    let store = Store::open_or_init(&store_root).unwrap();
    locallibrary::testing::store_root_on_this_thread(Some(store_root));
    let main = common::demo_drive(&volumes, "Main");
    common::adopt(Some(&store), &main, "Main");
    let pull =
        api::PullRequest { drive: path(&main), library: Some("Home".into()), new_library: true, ..Default::default() };
    api::pull(&store, &pull, &Ctx::silent()).unwrap();
    let clone = |name: &str| {
        let root = volumes.join(name);
        std::fs::create_dir(&root).unwrap();
        api::sync_apply(Some(&store), &common::sync_pair(&main, &root), &Ctx::silent()).unwrap();
        common::adopt(Some(&store), &root, name);
        dunce_canonicalize(&root)
    };
    let (backup, sd, archive) = (clone("Backup"), clone("SD"), clone("Archive"));
    let id = |root: &Path| Device::open(root).unwrap().id().unwrap();
    fleet::set_role(&store, id(&main), Some(Role::Primary)).unwrap();
    fleet::set_role(&store, id(&archive), Some(Role::Archive)).unwrap();
    Home { _dir: dir, store, main: dunce_canonicalize(&main), backup, sd, archive }
}

fn playlists(root: &Path) -> Vec<String> {
    Device::open(root).unwrap().library().unwrap().playlists.iter().map(|p| p.name.clone()).collect()
}

fn new_playlist(drive: &str, name: &str) -> api::EditRequest {
    let edits = vec![Edit::CreatePlaylist { path: name.into(), folder: false }];
    api::EditRequest { drive: drive.into(), edits, options: EditOptions::default() }
}

/// Edit the drive at `root` alone, as rekordbox or a player would: no clone hears of it.
fn edit_alone(root: &Path, playlist: &str) {
    common::apply(root, &[Edit::CreatePlaylist { path: playlist.into(), folder: false }]);
}

#[test]
fn a_write_to_a_library_reaches_every_clone() {
    let h = home();
    let ctx = Ctx::silent();
    let plan = api::edit_plan(&new_playlist("Home", "Friday"), &ctx).unwrap();
    // One plan: made on the lead (the primary, as the clones are identical), then copied onto
    // the others, but not the archive.
    assert_eq!(plan.drives[0].root, h.main);
    assert_eq!(plan.summary.last().map(String::as_str), Some("then copied onto Backup and SD"));
    let v = serde_json::to_value(&plan).unwrap();
    assert_eq!(
        v["clones"],
        serde_json::json!([{ "name": "Backup", "root": h.backup }, { "name": "SD", "root": h.sd }])
    );
    let written: Vec<&Path> = plan.drives.iter().filter(|t| t.writes).map(|t| t.root.as_path()).collect();
    assert_eq!(written, [&h.main, &h.backup, &h.sd]);
    plan.apply(Some(&h.store), &ctx).unwrap();
    for root in [&h.main, &h.backup, &h.sd] {
        assert!(playlists(root).contains(&"Friday".to_string()), "{}", root.display());
        // The edit is recorded on each, as the journal goes with the library.
        assert_eq!(journal::list(&Device::open(root).unwrap()).unwrap().len(), 1, "{}", root.display());
    }
    assert!(!playlists(&h.archive).contains(&"Friday".to_string()), "archive drives are left alone");

    // A drive named, of a library with other clones, reaches them too.
    let plan = api::edit_plan(&new_playlist(&path(&h.sd), "Saturday"), &ctx).unwrap();
    assert_eq!(plan.copied_onto().as_deref(), Some("then copied onto Backup and Main"));

    // A job's own steps write their drive only: its sync step copies onto the others.
    let rule = api::SetSmartPlaylist {
        path: "Smart/Everything".into(),
        filter: "bpm:>=1".into(),
        sort: None,
        limit: None,
        drives: Vec::new(),
    };
    api::smart_set(&h.store, &rule).unwrap();
    let req = api::RefreshRequest { drive: Some(path(&h.main)), ..Default::default() };
    let refresh = api::refresh_plan(&h.store, &req, &ctx).unwrap();
    let smart = refresh.steps.iter().find(|s| s.step == StepKind::Smart).unwrap();
    assert!(smart.runs && smart.plans[0].get("clones").is_none(), "{:?}", smart.plans);
    assert!(!refresh.summary.iter().any(|l| l.contains("then copied onto")), "{:?}", refresh.summary);

    // A write to the archive, allowed, stays on it.
    locallibrary::op::allow_archive_writes(true);
    let plan = api::edit_plan(&new_playlist("Archive", "Old"), &ctx);
    locallibrary::op::allow_archive_writes(false);
    assert!(plan.unwrap().clones.is_empty());
}

#[test]
fn clones_that_cant_take_the_copy_are_left_alone() {
    let h = home();
    let ctx = Ctx::silent();
    // SD and Main both changed on their own since they were last in sync; Backup is behind.
    edit_alone(&h.sd, "SD only");
    edit_alone(&h.main, "Main only");
    let plan = api::edit_plan(&new_playlist("Main", "Friday"), &ctx).unwrap();
    assert_eq!(plan.copied_onto().as_deref(), Some("then copied onto Backup"));
    let sd = plan.clones.iter().find(|c| c.name == "SD").unwrap();
    let why = sd.left_alone.as_deref().expect("left alone");
    assert!(why.starts_with("not copying Main onto SD: "), "{why}");
    assert!(plan.warnings.iter().any(|w| w == why), "{:?}", plan.warnings);
    plan.apply(Some(&h.store), &ctx).unwrap();
    assert!(playlists(&h.backup).contains(&"Main only".to_string()), "Backup was behind, so it gets both");
    assert!(!playlists(&h.sd).contains(&"Friday".to_string()));

    // A clone newer than the drive written isn't copied over.
    edit_alone(&h.backup, "Backup only");
    let plan = api::edit_plan(&new_playlist("Main", "Saturday"), &ctx).unwrap();
    assert!(plan.clones.iter().all(|c| c.left_alone.is_some()), "{:?}", plan.clones);
    assert!(
        plan.warnings.iter().any(|w| w == "Backup is newer than Main: it isn't copied over"),
        "{:?}",
        plan.warnings
    );
    assert!(plan.copied_onto().is_none());
}

#[test]
fn a_sync_brings_the_recorded_edits_with_the_library() {
    let h = home();
    let ctx = Ctx::silent();
    let export = std::fs::read(h.main.join(locallibrary::paths::EXPORT_PDB)).unwrap();
    edit_alone(&h.main, "Friday");
    // Backup's own recorded edit, which copying Main over it forgets: the plan says so.
    edit_alone(&h.backup, "Mine");
    let mut req = common::sync_pair(&h.main, &h.backup);
    req.direction = api::Direction::LeftToRight;
    let plan = api::sync_plan(&req, &ctx).unwrap();
    let forgotten = "Backup has 1 recorded edit(s) Main lacks, made to the library this replaces: they're forgotten";
    assert!(plan.warnings.iter().any(|w| w == forgotten), "{:?}", plan.warnings);
    plan.apply(Some(&h.store), &ctx).unwrap();
    let recorded = |root: &Path| -> Vec<Vec<String>> {
        journal::list(&Device::open(root).unwrap()).unwrap().into_iter().map(|e| e.changes).collect()
    };
    assert_eq!(recorded(&h.backup), recorded(&h.main));
    assert_eq!(recorded(&h.backup), [["create playlist Friday"]]);
    // Edits the source has too aren't forgotten: nothing to warn about.
    edit_alone(&h.main, "Saturday");
    let plan = api::sync_plan(&common::sync_pair(&h.main, &h.backup), &ctx).unwrap();
    assert!(!plan.warnings.iter().any(|w| w.contains("recorded edit")), "{:?}", plan.warnings);

    // rekordbox exports to Backup over the edit made on Main: refreshing Backup makes it again.
    std::fs::write(h.backup.join(locallibrary::paths::EXPORT_PDB), &export).unwrap();
    common::export_new_track(&h.backup, "Artist", "Fresh");
    let later = std::time::SystemTime::now() + std::time::Duration::from_secs(60);
    // Closed straight away: an open handle would keep Windows from replacing the file.
    let pdb = h.backup.join(locallibrary::paths::EXPORT_PDB);
    std::fs::File::options().write(true).open(pdb).unwrap().set_modified(later).unwrap();
    assert!(!playlists(&h.backup).contains(&"Friday".to_string()));
    let req = api::RefreshRequest { drive: Some(path(&h.backup)), ..Default::default() };
    let report = api::refresh_apply(&h.store, &req, &ctx).unwrap();
    assert!(report.stopped.is_none(), "{:?}", report.stopped);
    assert!(playlists(&h.backup).contains(&"Friday".to_string()));
}

#[test]
fn an_undo_reaches_every_clone() {
    let h = home();
    let ctx = Ctx::silent();
    api::edit_plan(&new_playlist("Home", "Friday"), &ctx).unwrap().apply(Some(&h.store), &ctx).unwrap();
    let plan = api::undo_plan(&api::UndoRequest { drive: Some("Home".into()), ..Default::default() }).unwrap();
    assert_eq!(plan.copied_onto().as_deref(), Some("then copied onto Backup and SD"));
    let warnings = common::Warnings::default();
    plan.apply(Some(&h.store), &warnings.ctx()).unwrap();
    assert_eq!(warnings.said(), Vec::<String>::new());
    for root in [&h.main, &h.backup, &h.sd] {
        assert!(!playlists(root).contains(&"Friday".to_string()), "{}", root.display());
    }
}

#[test]
fn a_clone_without_an_export_isnt_written() {
    let h = home();
    let ctx = Ctx::silent();
    std::fs::remove_dir_all(h.sd.join("PIONEER")).unwrap();
    let plan = api::edit_plan(&new_playlist("Home", "Friday"), &ctx).unwrap();
    assert_eq!(plan.copied_onto().as_deref(), Some("then copied onto Backup"));
    plan.apply(Some(&h.store), &ctx).unwrap();
    assert!(!Device::open(&h.sd).unwrap().has_export());
}

/// As the sidecar takes them too: a library's name is its clones, each once.
#[test]
fn requests_for_several_drives_take_a_library_for_its_clones() {
    let h = home();
    let req = api::CheckRequest { drives: vec!["Home".into(), path(&h.backup)], options: Default::default() };
    let report = api::check(Some(&h.store), &req, &Ctx::silent()).unwrap();
    assert_eq!(report.drives, ["Main", "Backup", "SD"]);
    let req = api::SyncManyRequest { drives: vec!["Home".into()], ..Default::default() };
    let plan = api::sync_many_plan(&req, &Ctx::silent()).unwrap();
    assert_eq!(plan.targets.len(), 2, "Main is the source, its clones the targets");
    // Ejecting one: each clone is a drive to eject (folders here, which aren't volumes to eject).
    let err = api::drive_eject_plan(None, &api::EjectDrives { drives: vec!["Home".into()] }).unwrap_err();
    assert!(err.to_string().starts_with("Main isn't the root of a volume"), "{err}");
}

#[test]
fn several_clones_named_are_their_library_once() {
    let h = home();
    let named = |specs: &[&Path]| api::libraries_once(specs.iter().map(|p| path(p)).collect()).unwrap();
    assert_eq!(named(&[&h.sd, &h.archive, &h.backup]), [path(&h.main), path(&h.archive)]);
    assert_eq!(named(&[&h.sd]), [path(&h.sd)], "one clone named is that drive");
    // The newest clone is the one written.
    edit_alone(&h.backup, "Backup only");
    assert_eq!(named(&[&h.main, &h.backup]), [path(&h.backup)]);
}

#[test]
fn a_library_with_only_its_archive_plugged_in_has_no_clone_to_act_on() {
    let h = home();
    let away = h._dir.path().join("away");
    std::fs::create_dir(&away).unwrap();
    for root in [&h.main, &h.backup, &h.sd] {
        std::fs::rename(root, away.join(root.file_name().unwrap())).unwrap();
    }
    let err = api::resolve_drive("Home").unwrap_err();
    assert!(matches!(err, locallibrary::Error::NotFound(_)), "{err}");
    assert!(err.to_string().contains("library \"Home\""), "{err}");
}

/// The TUI opens a library by its id: the lead is among its clones, never its archive.
#[test]
fn a_library_by_its_id_is_read_from_its_lead_clone() {
    let h = home();
    let id = h.store.find_library("Home").unwrap().unwrap().id.to_string();
    assert_eq!(api::resolve_drive(&id).unwrap().root(), h.main, "identical clones: the primary");
    edit_alone(&h.archive, "Archive only");
    assert_eq!(api::resolve_drive(&id).unwrap().root(), h.main);
    edit_alone(&h.sd, "SD only");
    assert_eq!(api::resolve_drive(&id).unwrap().root(), h.sd, "the newest clone");
}

/// Given no drive, a mix (and a set's tracks) is matched against the one library plugged in, on
/// its lead clone, as other commands take it.
#[test]
fn a_mix_is_matched_against_the_one_library_plugged_in() {
    let h = home();
    assert_eq!(api::mix_drive(None).unwrap().root(), h.main, "identical clones: the primary");
    edit_alone(&h.sd, "SD only");
    assert_eq!(api::mix_drive(None).unwrap().root(), h.sd, "the newest clone");
}

/// A smart playlist limited to a drive is limited to its library: every clone takes it, so
/// keeping them identical doesn't undo it. A library's name limits it the same way.
#[test]
fn a_smart_playlist_limited_to_a_drive_applies_to_its_library() {
    let h = home();
    let ctx = Ctx::silent();
    let rule = |path: &str, drive: &str| api::SetSmartPlaylist {
        path: path.into(),
        filter: "bpm:>=1".into(),
        sort: None,
        limit: None,
        drives: vec![drive.into()],
    };
    api::smart_set(&h.store, &rule("Smart/Main", "Main")).unwrap();
    api::smart_set(&h.store, &rule("Smart/Home", "Home")).unwrap();
    let library = h.store.find_library("Home").unwrap().unwrap().id;
    assert_eq!(api::smart_list(&h.store).unwrap().get("Smart/Home").unwrap().drives, [library]);
    let lonely = common::demo_drive(h.main.parent().unwrap(), "Lonely");
    let fills = |root: &Path| -> Vec<String> {
        match api::smart_apply_plan(&h.store, &api::DriveRef { drive: path(root) }, &ctx) {
            Ok(plan) => plan.changes.clone(),
            Err(e) => vec![e.to_string()],
        }
    };
    for root in [&h.main, &h.backup, &h.sd] {
        let fills = fills(root);
        assert!(fills.len() == 2 && fills.iter().any(|e| e.contains("Smart/Home")), "{}: {fills:?}", root.display());
    }
    let none = fills(&lonely);
    assert!(none.len() == 1 && none[0].contains("no smart playlists apply"), "{none:?}");
}

/// Removing orphaned files is a write to the library's drive: it reaches its clones too.
#[test]
fn orphaned_files_are_removed_from_every_clone() {
    let h = home();
    let ctx = Ctx::silent();
    let orphan = "Contents/Old/gone.mp3";
    for root in [&h.main, &h.backup, &h.sd, &h.archive] {
        std::fs::create_dir_all(root.join("Contents/Old")).unwrap();
        std::fs::write(root.join(orphan), b"old audio").unwrap();
    }
    let plan = api::doctor_orphans_plan(&api::DriveRef { drive: "Home".into() }, &ctx).unwrap();
    assert_eq!(plan.copied_onto().as_deref(), Some("then copied onto Backup and SD"));
    plan.apply(Some(&h.store), &ctx).unwrap();
    for root in [&h.main, &h.backup, &h.sd] {
        assert!(!root.join(orphan).exists(), "{}", root.display());
    }
    assert!(h.archive.join(orphan).exists(), "archive drives are left alone");
}

/// A group sync is one plan, of one library: given none with two plugged in, it asks which (as
/// the sidecar's `sync.group.plan` documents; `locallibrary sync` plans each in turn).
#[test]
fn a_group_sync_given_no_library_asks_which_when_two_are_plugged_in() {
    let h = home();
    let ctx = Ctx::silent();
    let volumes = h.main.parent().unwrap();
    let other = common::demo_drive(volumes, "Other");
    common::adopt(Some(&h.store), &other, "Other");
    let pull =
        api::PullRequest { drive: path(&other), library: Some("Away".into()), new_library: true, ..Default::default() };
    api::pull(&h.store, &pull, &ctx).unwrap();
    let spare = volumes.join("Spare");
    std::fs::create_dir(&spare).unwrap();
    api::sync_apply(Some(&h.store), &common::sync_pair(&other, &spare), &ctx).unwrap();
    common::adopt(Some(&h.store), &spare, "Spare");

    let err = api::sync_set_plan(&h.store, &api::SyncSetRequest::default(), &ctx).unwrap_err();
    assert_eq!(err.reason(), Some(locallibrary::error::Reason::ChooseLibrary), "{err}");
    let mut libraries = err.detail().fields["libraries"].clone();
    libraries.as_array_mut().unwrap().sort_by_key(|l| l.to_string());
    assert_eq!(libraries, serde_json::json!(["Away", "Home"]));
}
