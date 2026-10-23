//! Jobs through the service layer: refreshing a drive after rekordbox exported over it, getting
//! drives ready for a set, and which drive a command means when it's given none. One test, with
//! the mounted volumes and the store the API consults named for its thread.

mod refresh_and_prep;

use crate::common::{self, path};

use indexmap::IndexMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use locallibrary::api::{self, StepKind};
use locallibrary::check::Level;
use locallibrary::compare::Relation;
use locallibrary::device::{Device, dunce_canonicalize};
use locallibrary::edit::Edit;
use locallibrary::progress::{CancelToken, Ctx, Event, Progress, Scope};
use locallibrary::query::TrackSelector;
use locallibrary::store::Store;
use locallibrary::{Error, fleet, paths, player, tags};
use rekordbox_pdb::{NewTrack, PdbEditor};

const SAMPLE: &str = "/Contents/rekordbox/GROOVE CIRCUIT FACTORY SAMPLES/House1.wav";

/// rekordbox's demo sample, as an export brings it back.
fn add_demo_sample(root: &Path) {
    let pdb = root.join(paths::EXPORT_PDB);
    let mut ed = PdbEditor::from_file(&pdb).unwrap();
    ed.add_track(&NewTrack { artist: Some("rekordbox".into()), ..NewTrack::new("House 1", SAMPLE) }).unwrap();
    ed.save(&pdb).unwrap();
    let audio = paths::to_fs(root, &paths::key(SAMPLE));
    std::fs::create_dir_all(audio.parent().unwrap()).unwrap();
    std::fs::write(audio, "demo sample").unwrap();
}

/// The database files of an export, to put back as rekordbox's next export would.
fn databases(root: &Path) -> IndexMap<&'static str, Vec<u8>> {
    [paths::EXPORT_PDB, paths::EXPORT_EXT_PDB, paths::LIBRARY_PLUS_DB]
        .into_iter()
        .filter_map(|k| Some((k, std::fs::read(root.join(k)).ok()?)))
        .collect()
}

/// rekordbox exports to the drive again: its own databases (one more track), a minute later.
fn export_again(root: &Path, export: &IndexMap<&'static str, Vec<u8>>) {
    for (k, bytes) in export {
        std::fs::write(root.join(k), bytes).unwrap();
    }
    if !paths::to_fs(root, &paths::key(SAMPLE)).exists() {
        add_demo_sample(root);
    }
    common::export_new_track(root, "Artist", "Fresh");
    common::add_onelibrary(root);
    let later = SystemTime::now() + Duration::from_secs(60);
    for k in [paths::EXPORT_PDB, paths::LIBRARY_PLUS_DB] {
        std::fs::File::options().write(true).open(root.join(k)).unwrap().set_modified(later).unwrap();
    }
}

fn playlist_names(root: &Path) -> Vec<String> {
    Device::open(root).unwrap().library().unwrap().playlists.iter().map(|p| p.name.clone()).collect()
}

fn status_of(store: &Store, root: &Path) -> api::FleetDrive {
    let st = api::fleet_status(Some(store), &api::FleetRequest::default(), &Ctx::silent()).unwrap();
    st.mounted.into_iter().find(|d| d.status.drive.root == dunce_canonicalize(root)).expect("mounted")
}

fn running(plan: &api::JobPlan) -> Vec<StepKind> {
    plan.running().map(|s| s.step).collect()
}

#[test]
fn jobs_through_the_api() {
    let dir = tempfile::tempdir().unwrap();
    let volumes = dir.path().join("volumes");
    std::fs::create_dir_all(&volumes).unwrap();
    locallibrary::testing::volumes_on_this_thread(Some(vec![volumes.clone()]));
    let store_root = dir.path().join("store");
    let store = Store::open_or_init(&store_root).unwrap();
    locallibrary::testing::store_root_on_this_thread(Some(store_root.clone()));
    let ctx = Ctx::silent();

    // Nothing mounted: nothing to default to.
    let err = api::default_drive(Some(&store), "refresh").unwrap_err();
    assert!(matches!(err, Error::NotFound(_)), "{err}");

    // Main: rekordbox's export (with a demo sample), saved in the store.
    let main = common::demo_drive(&volumes, "Main");
    add_demo_sample(&main);
    common::add_onelibrary(&main);
    let export = databases(&main);
    common::adopt(Some(&store), &main, "Main");
    let pull =
        api::PullRequest { drive: path(&main), library: Some("Home".into()), new_library: true, ..Default::default() };
    api::pull(&store, &pull, &ctx).unwrap();
    assert_eq!(api::default_drive(Some(&store), "refresh").unwrap().root(), dunce_canonicalize(&main));

    // What the DJ did since: a playlist, the name players show, My Settings, a rule.
    let first = Device::open(&main).unwrap().library().unwrap().tracks[0].id;
    common::apply(
        &main,
        &[
            Edit::CreatePlaylist { path: "Shows/Friday".into(), folder: false },
            Edit::AddTracks {
                playlist: "Shows/Friday".into(),
                tracks: TrackSelector { ids: vec![first], ..Default::default() },
                position: None,
                sort: None,
                allow_repeats: false,
            },
        ],
    );
    api::player_set(&api::SetPlayerIdentity { drive: path(&main), name: Some("MAIN A".into()), colour: Some(3) })
        .unwrap();
    let remembered = Device::open(&main).unwrap().state().unwrap().player.clone().expect("remembered on the drive");
    assert_eq!((remembered.name.as_deref(), remembered.colour), (Some("MAIN A"), Some(3)));
    let values = IndexMap::from([("quantize".to_string(), "off".to_string())]);
    api::settings_set(&store, &api::SetSettings { values, drive: None }).unwrap();
    let rule = api::SetSmartPlaylist {
        path: "Smart/Everything".into(),
        filter: "bpm:>=1".into(),
        sort: None,
        limit: None,
        drives: Vec::new(),
    };
    api::smart_set(&store, &rule).unwrap();
    api::pull(&store, &api::PullRequest { drive: path(&main), ..Default::default() }, &ctx).unwrap();
    assert_eq!(status_of(&store, &main).exported_over_edits, 0, "the edits are in place");

    // rekordbox exports over all of it.
    export_again(&main, &export);
    assert!(!playlist_names(&main).contains(&"Friday".to_string()));
    assert_eq!(player::db_identity(&main).unwrap().name, None);
    assert_eq!(status_of(&store, &main).exported_over_edits, 1, "one recorded edit was exported over");

    // One plan puts it all back, then saves the drive.
    let plan = api::refresh_plan(&store, &api::RefreshRequest::default(), &ctx).unwrap();
    assert_eq!(
        running(&plan),
        [StepKind::Replay, StepKind::Smart, StepKind::Player, StepKind::Settings, StepKind::Pull]
    );
    let v = serde_json::to_value(&plan).unwrap();
    assert_eq!(v["operation"], "refresh");
    // The demo sample is the demos plugin's to remove, which the app hosts.
    let first = (v["steps"][0]["step"].as_str(), v["steps"][0]["reason"].as_str());
    assert_eq!(first, (Some("clean"), Some("the demos plugin isn't there")));
    assert!(v["summary"].as_array().unwrap().iter().any(|l| l.as_str().unwrap().starts_with("Make the recorded")));
    let report = plan.apply(Some(&store), &ctx).unwrap();
    assert!(report.stopped.is_none(), "{:?}", report.stopped);
    assert_eq!(report.steps.len(), 5);
    let names = playlist_names(&main);
    assert!(names.contains(&"Friday".to_string()) && names.contains(&"Everything".to_string()), "{names:?}");
    assert!(paths::to_fs(&main, &paths::key(SAMPLE)).exists(), "the demo sample stays");
    assert_eq!(player::db_identity(&main), Some(remembered.clone()));
    let st = status_of(&store, &main);
    assert_eq!(
        (st.status.relation, st.status.settings_drift, st.exported_over_edits),
        (Some(Relation::Identical), 0, 0)
    );

    // Again: nothing to do, and each step says why.
    let plan = api::refresh_plan(&store, &api::RefreshRequest::default(), &ctx).unwrap();
    assert!(plan.noop, "{:?}", plan.summary);
    assert!(plan.steps.iter().all(|s| s.reason.is_some()));

    // A step that fails stops the job, and the report says what was done before it.
    export_again(&main, &export);
    let skip = vec![StepKind::Smart, StepKind::Player, StepKind::Settings];
    let plan = api::refresh_plan(&store, &api::RefreshRequest { skip, ..Default::default() }, &ctx).unwrap();
    assert_eq!(running(&plan), [StepKind::Replay, StepKind::Pull]);
    assert_eq!(plan.steps.iter().find(|s| s.step == StepKind::Smart).unwrap().reason.as_deref(), Some("skipped"));
    let held = store.lock().unwrap();
    let report = plan.apply(Some(&store), &ctx).unwrap();
    drop(held);
    let stopped = report.stopped.as_ref().expect("the store was locked");
    assert_eq!((stopped.step.as_str(), stopped.kind), ("pull", "locked"));
    assert!(report.did_anything());
    assert_eq!(report.steps.iter().map(|s| s.step).collect::<Vec<_>>(), [StepKind::Replay]);
    assert!(playlist_names(&main).contains(&"Friday".to_string()));
    // The rest, with the drive as the first run left it (the replay made the rule's playlist too,
    // as `smart apply` recorded it).
    let plan = api::refresh_plan(&store, &api::RefreshRequest::default(), &ctx).unwrap();
    let steps = running(&plan);
    assert!(steps.contains(&StepKind::Player) && steps.ends_with(&[StepKind::Pull]), "{steps:?}");
    assert!(!steps.contains(&StepKind::Clean) && !steps.contains(&StepKind::Replay), "{steps:?}");
    let report = plan.apply(Some(&store), &ctx).unwrap();
    assert!(report.stopped.is_none(), "{:?}", report.stopped);

    // Backup: a copy of Main, in the same library.
    let backup = volumes.join("Backup");
    std::fs::create_dir(&backup).unwrap();
    api::sync_apply(Some(&store), &common::sync_pair(&main, &backup), &ctx).unwrap();
    common::adopt(Some(&store), &backup, "Backup");
    api::pull(&store, &api::PullRequest { drive: path(&backup), ..Default::default() }, &ctx).unwrap();

    // Two clones of one library: either will do while they're identical, so neither is asked about.
    let either = api::default_drive(Some(&store), "refresh").unwrap();
    assert!([dunce_canonicalize(&main), dunce_canonicalize(&backup)].contains(&either.root().to_path_buf()));
    // A library's name means the library.
    let lead = api::resolve_drive("home").unwrap();
    assert!([dunce_canonicalize(&main), dunce_canonicalize(&backup)].contains(&lead.root().to_path_buf()));
    assert!(api::resolve_one_drive("Home").is_err(), "a drive's own commands take a drive");
    let group = api::drive_group(Some(&store), None).unwrap();
    assert_eq!((group.library.as_deref(), group.drives.len()), (Some("Home"), 2));
    let main_id = Device::open(&main).unwrap().id().unwrap();
    fleet::set_role(&store, main_id, Some(fleet::Role::Primary)).unwrap();
    assert_eq!(api::default_drive(Some(&store), "refresh").unwrap().root(), dunce_canonicalize(&main));
    assert_eq!(api::drive_group(Some(&store), None).unwrap().drives[0], dunce_canonicalize(&main));
    let candidates = api::drive_candidates(Some(&store));
    assert!(candidates.iter().any(|c| c.name == "Main" && c.role == Some(fleet::Role::Primary)));

    // A known drive that isn't plugged in, and a name nobody has.
    std::fs::rename(&backup, dir.path().join("unplugged")).unwrap();
    let err = api::resolve_drive("backup").unwrap_err().to_string();
    assert!(err.contains("Backup is known but not mounted; last seen"), "{err}");
    assert!(err.contains("mounted: Main ("), "{err}");
    let err = api::resolve_drive("Nobody").unwrap_err().to_string();
    assert!(err.contains("no mounted drive matches \"Nobody\""), "{err}");
    std::fs::rename(dir.path().join("unplugged"), &backup).unwrap();

    // The night before a set: Backup came back from the last one with a history list, and Main
    // has a new export since.
    let ids: Vec<u32> = Device::open(&backup).unwrap().library().unwrap().tracks.iter().map(|t| t.id).take(2).collect();
    common::add_history(&backup, "HISTORY 001", &ids);
    common::export_new_track(&main, "Artist", "Newer");
    common::add_onelibrary(&main);
    let req = api::SetJobRequest { eject: false, ..Default::default() };
    let plan = api::set_job_plan(&store, &req, &ctx).unwrap();
    assert_eq!(running(&plan), [StepKind::Pull, StepKind::Sync, StepKind::Check]);
    let pull = &plan.steps[0];
    assert!(pull.summary[0].starts_with("save Main in Home"), "{:?}", pull.summary);
    assert!(pull.summary[1].contains("Backup") && pull.summary[1].contains("HISTORY 001"), "{:?}", pull.summary);
    assert_eq!(plan.steps[1].summary[0], "Main holds the newest library");
    assert_eq!(plan.steps[3].reason.as_deref(), Some("kept mounted, as asked"));
    let report = plan.apply(Some(&store), &ctx).unwrap();
    assert!(report.stopped.is_none(), "{:?}", report.stopped);
    assert!(!report.problems, "{:?}", report.check);
    assert!(report.steps[0].done[1].contains("beside Home's newest snapshot"), "{:?}", report.steps[0].done);
    // Backup's history list is in the store, and both drives hold Main's library.
    let lib = store.find_library("Home").unwrap().unwrap();
    let kept = store.history(&lib, None).unwrap().iter().any(|s| {
        let snap = store.load_snapshot(&s.id).unwrap();
        locallibrary::compare::Side::from_snapshot(&store, &snap, "s")
            .unwrap()
            .library
            .history
            .iter()
            .any(|h| h.name == "HISTORY 001")
    });
    assert!(kept);
    let c = api::compare(&api::ComparePair { left: path(&main), right: path(&backup) }, &ctx).unwrap();
    assert_eq!(c.relation, Relation::Identical);

    // A check that finds problems (placeholder audio doesn't decode) stops before ejecting.
    let req = api::SetJobRequest { audio: true, ..Default::default() };
    let plan = api::set_job_plan(&store, &req, &ctx).unwrap();
    assert_eq!(running(&plan), [StepKind::Check, StepKind::Eject], "everything is saved and synced already");
    let report = plan.apply(Some(&store), &ctx).unwrap();
    assert!(report.problems);
    assert!(report.steps.iter().all(|s| s.step != StepKind::Eject), "not ejected");
    assert!(report.check.as_ref().is_some_and(|c| c.problems > 0));

    // Pulling the group: each drive as `watch --pull` would.
    let report = api::pull_group(&store, &api::PullGroupRequest::default(), &ctx).unwrap();
    assert_eq!(report.steps.len(), 1);

    // A model `set` doesn't know is refused, as `players set` refuses it, rather than left
    // unchecked.
    let req = api::SetJobRequest { players: vec!["CDJ-9000X".into()], eject: false, ..Default::default() };
    assert!(matches!(api::set_job_plan(&store, &req, &ctx), Err(Error::Invalid(_))));

    // Backup is retired, and players recorded a history list on it: `set` doesn't save it, says
    // why, and keeps it mounted, the list not being in the store.
    let backup_id = Device::open(&backup).unwrap().id().unwrap();
    fleet::retire(&store, backup_id, false).unwrap();
    let ids: Vec<u32> = Device::open(&backup).unwrap().library().unwrap().tracks.iter().map(|t| t.id).take(1).collect();
    common::add_history(&backup, "HISTORY 002", &ids);
    let req = api::SetJobRequest { drives: vec![path(&backup)], eject: false, ..Default::default() };
    let plan = api::set_job_plan(&store, &req, &ctx).unwrap();
    let pull = &plan.steps[0];
    assert_eq!((pull.runs, pull.reason.as_deref()), (false, Some("not saved: see the warning")));
    assert!(pull.warnings[0].contains("it's retired"), "{:?}", pull.warnings);
    let report = plan.apply(Some(&store), &ctx).unwrap();
    let check = report.check.as_ref().unwrap();
    assert!(report.problems, "{:?}", check.items);
    assert!(check.items.iter().any(|i| i.check == "history" && i.level == Level::Problem), "{:?}", check.items);
    fleet::retire(&store, backup_id, true).unwrap();

    // rekordbox exported over an edit recorded on Backup: `refresh` with no drive means Backup.
    let before = databases(&backup);
    common::apply(&backup, &[Edit::CreatePlaylist { path: "Backup only".into(), folder: false }]);
    export_again(&backup, &before);
    assert_eq!(status_of(&store, &main).exported_over_edits, 0);
    // Backup's journal came with Main's library: Main's two edits, and its own.
    assert_eq!(status_of(&store, &backup).exported_over_edits, 3);
    // The library's lead is the newest clone, Backup now, primary or not.
    assert_eq!(api::default_drive(Some(&store), "refresh").unwrap().root(), dunce_canonicalize(&backup));
    let plan = api::refresh_plan(&store, &api::RefreshRequest::default(), &ctx).unwrap();
    assert_eq!(plan.targets[0].root, dunce_canonicalize(&backup));

    // Another drive at Main's mount path once the job is under way: the steps after are refused,
    // rather than planned again on that drive.
    export_again(&main, &export);
    struct Swap(PathBuf);
    impl Progress for Swap {
        fn on_event(&self, _: &Event) {}
        fn on_step(&self, scope: &Scope) {
            if scope.step.is_some_and(|s| s.id == "smart") {
                let mut d = Device::open(&self.0).unwrap();
                fleet::reidentify(None, &mut d, Some("Impostor")).unwrap();
            }
        }
    }
    let req = api::RefreshRequest { drive: Some(path(&main)), ..Default::default() };
    let plan = api::refresh_plan(&store, &req, &ctx).unwrap();
    assert_eq!(running(&plan)[..2], [StepKind::Replay, StepKind::Smart]);
    let swap = Swap(main.clone());
    let report = plan.apply(Some(&store), &Ctx::new(&swap, CancelToken::new())).unwrap();
    let stopped = report.stopped.as_ref().expect("refused");
    assert_eq!((stopped.step.as_str(), stopped.kind), ("smart", "stale_plan"), "{}", stopped.message);
    assert_eq!(report.steps.iter().map(|s| s.step).collect::<Vec<_>>(), [StepKind::Replay]);
}

/// rekordbox writes both databases, a player only the one it reads: on a drive with both, a set
/// isn't taken for an export over recorded edits (whose My Tag edits would then count as lost).
#[test]
fn a_set_is_not_taken_for_an_export_over_recorded_edits() {
    let dir = tempfile::tempdir().unwrap();
    let root = common::demo_drive(dir.path(), "Main");
    common::add_onelibrary(&root);
    let export = databases(&root);
    let mut d = Device::open_export(&root).unwrap();
    let first = d.library().unwrap().tracks[0].id;
    let tag = tags::Edit::Add { tag: "Genre/Deep".into(), tracks: vec![first] };
    tags::apply(&tags::plan(&d, &tag).unwrap(), &mut d, &Ctx::silent()).unwrap();
    let written = |key: &str, at: SystemTime| {
        std::fs::File::options().write(true).open(root.join(key)).unwrap().set_modified(at).unwrap();
    };
    let later = SystemTime::now() + Duration::from_secs(60);
    let over = || api::exported_over_edits(&Device::open(&root).unwrap());

    // A CDJ-3000X writes OneLibrary only.
    written(paths::LIBRARY_PLUS_DB, later);
    assert_eq!(over(), 0, "a player wrote OneLibrary");
    written(paths::LIBRARY_PLUS_DB, SystemTime::now());

    // A CDJ-3000 records a history list in export.pdb only.
    common::add_history(&root, "HISTORY 001", &[first]);
    written(paths::EXPORT_PDB, later);
    assert_eq!(over(), 0, "a player wrote export.pdb");

    // rekordbox writes both.
    export_again(&root, &export);
    assert_eq!(over(), 1);
}
