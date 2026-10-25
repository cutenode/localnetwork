//! Refreshing a drive after rekordbox exported to it, against a model of what the DJ did to it
//! since it was saved and what the export put back: which steps run, why the others don't, and
//! what applying them leaves.

use crate::common::{self, Template, path};

use std::path::Path;
use std::time::{Duration, SystemTime};

use hegel::generators as gs;
use hegel::{HealthCheck, TestCase};
use indexmap::IndexMap;
use locallibrary::api::{self, JobPlan, StepKind};
use locallibrary::device::Device;
use locallibrary::edit::Edit;
use locallibrary::progress::Ctx;
use locallibrary::store::Store;
use locallibrary::{paths, player};

fn reason(plan: &JobPlan, kind: StepKind) -> Option<&str> {
    plan.steps.iter().find(|s| s.step == kind).and_then(|s| s.reason.as_deref())
}

fn playlists(root: &Path) -> Vec<String> {
    common::library(root).playlists.into_iter().map(|p| p.name).collect()
}

/// rekordbox exports to the drive again: its own files back as they were (settings too), with
/// one more track, a minute later; OneLibrary's database only when the drive has one.
fn export_again(root: &Path, export: &Template, onelibrary: bool) {
    export.write(&root.join(paths::PIONEER_DIR));
    common::export_new_track(root, "Artist", "Fresh");
    match onelibrary {
        true => common::add_onelibrary(root),
        false => std::fs::remove_file(root.join(paths::LIBRARY_PLUS_DB)).unwrap(),
    }
    let later = SystemTime::now() + Duration::from_secs(60);
    std::fs::File::options().write(true).open(root.join(paths::EXPORT_PDB)).unwrap().set_modified(later).unwrap();
}

/// Main, saved in the store after the DJ changed it (a playlist, the name players show, My
/// Settings written or only set, a rule-based playlist filled, each or not), then maybe exported over by
/// rekordbox, maybe with no OneLibrary database, maybe with its recorded edits unreadable:
/// refreshing it runs exactly the steps the model says, each other step saying why not (skipped,
/// nothing to do, players show the name already, no OneLibrary database to show it in, edits that
/// can't be read, no other drive of its library to copy onto), and saves the drive when the
/// export moved it on or a step wrote to it. Applied, each step's change is back.
///
/// The explicit cases are those each step's branches need: everything put back and saved, a
/// drive whose only change is My Settings set since, a name with nowhere to show it, and the
/// recorded edits unreadable with a rule-based playlist (up to date or dropped) and without, and
/// the steps that write skipped.
#[hegel::test(test_cases = 8, suppress_health_check = [HealthCheck::TooSlow])]
#[hegel::explicit_test_case(edit = true, name = Some("MAIN A"), settings = 1u8, smart = true, onelibrary = true, exported = true, unreadable = false, sync = true, skip = Vec::<StepKind>::new(), applied_by_plan = true)]
#[hegel::explicit_test_case(edit = false, name = Some("Booth"), settings = 2u8, smart = true, onelibrary = true, exported = false, unreadable = false, sync = false, skip = Vec::<StepKind>::new(), applied_by_plan = false)]
#[hegel::explicit_test_case(edit = true, name = Some("Booth"), settings = 0u8, smart = false, onelibrary = false, exported = true, unreadable = false, sync = true, skip = vec![StepKind::Pull, StepKind::Clean, StepKind::Sync], applied_by_plan = false)]
#[hegel::explicit_test_case(edit = true, name = None::<&str>, settings = 0u8, smart = false, onelibrary = true, exported = true, unreadable = true, sync = false, skip = Vec::<StepKind>::new(), applied_by_plan = true)]
#[hegel::explicit_test_case(edit = false, name = None::<&str>, settings = 0u8, smart = true, onelibrary = true, exported = false, unreadable = true, sync = false, skip = Vec::<StepKind>::new(), applied_by_plan = true)]
#[hegel::explicit_test_case(edit = true, name = None::<&str>, settings = 0u8, smart = true, onelibrary = true, exported = true, unreadable = true, sync = false, skip = Vec::<StepKind>::new(), applied_by_plan = false)]
#[hegel::explicit_test_case(edit = true, name = Some("Booth"), settings = 1u8, smart = true, onelibrary = true, exported = true, unreadable = false, sync = true, skip = vec![StepKind::Replay, StepKind::Smart, StepKind::Player, StepKind::Settings], applied_by_plan = true)]
fn a_refresh_puts_back_what_the_export_replaced(tc: TestCase) {
    let edit = tc.draw(gs::booleans());
    let name = tc.draw(gs::optional(gs::sampled_from(vec!["MAIN A", "Booth"])));
    // My Settings: none, written to the drive before it was saved, or set after (not written).
    let settings = tc.draw(gs::integers::<u8>().max_value(2));
    let smart = tc.draw(gs::booleans());
    let onelibrary = tc.draw(gs::weighted_booleans(0.7));
    let exported = tc.draw(gs::booleans());
    let unreadable = tc.draw(gs::weighted_booleans(0.2));
    let sync = tc.draw(gs::booleans());
    let skip = tc.draw(
        gs::vecs(gs::sampled_from(vec![
            StepKind::Clean,
            StepKind::Replay,
            StepKind::Smart,
            StepKind::Player,
            StepKind::Settings,
            StepKind::Pull,
            StepKind::Sync,
        ]))
        .max_size(3)
        .unique(true),
    );
    let applied_by_plan = tc.draw(gs::booleans());

    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    let volumes = common::isolate(dir.path(), Some(store.root()));
    let ctx = Ctx::silent();
    let main = Device::open(common::demo_drive(&volumes, "Main")).unwrap().root().to_path_buf();
    common::add_onelibrary(&main);
    let export = Template::of(&main.join(paths::PIONEER_DIR));
    common::adopt(Some(&store), &main, "Main");
    let pull =
        api::PullRequest { drive: path(&main), library: Some("Home".into()), new_library: true, ..Default::default() };
    api::pull(&store, &pull, &ctx).unwrap();

    // What the DJ did since, saved.
    if edit {
        common::apply(&main, &[Edit::CreatePlaylist { path: "Friday".into(), folder: false }]);
    }
    if let Some(n) = name {
        api::player_set(&api::SetPlayerIdentity { drive: path(&main), name: Some(n.into()), colour: Some(3) }).unwrap();
    }
    let set_settings = || {
        let values = IndexMap::from([("quantize".to_string(), "off".to_string())]);
        api::settings_set(&store, &api::SetSettings { values, drive: None }).unwrap();
    };
    if settings == 1 {
        set_settings();
        api::settings_apply(&store, &api::ApplySettings { drive: path(&main), dry_run: false, create_missing: false })
            .unwrap();
    }
    if smart {
        let rule = api::SetSmartPlaylist {
            path: "Smart/Everything".into(),
            filter: "bpm:>=1".into(),
            sort: None,
            limit: None,
            drives: Vec::new(),
        };
        api::smart_set(&store, &rule).unwrap();
        api::smart_apply(&store, &api::DriveRef { drive: path(&main) }, &ctx).unwrap();
    }
    if !onelibrary {
        std::fs::remove_file(main.join(paths::LIBRARY_PLUS_DB)).unwrap();
    }
    api::pull(&store, &api::PullRequest { drive: path(&main), ..Default::default() }, &ctx).unwrap();
    if settings == 2 {
        set_settings();
    }
    let remembered = Device::open(&main).unwrap().state().unwrap().player.clone();
    assert_eq!(remembered.as_ref().and_then(|p| p.name.as_deref()), name, "remembered on the drive");
    if exported {
        export_again(&main, &export, onelibrary);
    }
    let journal = main.join(paths::STATE_DIR).join("edits.json");
    let kept = std::fs::read(&journal).ok();
    if unreadable {
        std::fs::write(&journal, "\"not a journal\"").unwrap();
    }

    let req = api::RefreshRequest { drive: Some(path(&main)), skip: skip.clone(), sync, ..Default::default() };
    let skipped = |k: StepKind| skip.contains(&k);
    let plan = api::refresh_plan(&store, &req, &ctx).unwrap();
    let mut want = Vec::new();
    let clean = if skipped(StepKind::Clean) { "skipped" } else { "no demo samples on the drive" };
    assert_eq!(reason(&plan, StepKind::Clean), Some(clean));
    // The recorded edits (the playlist, the rule's playlist) are lost to an export.
    match () {
        _ if skipped(StepKind::Replay) => assert_eq!(reason(&plan, StepKind::Replay), Some("skipped")),
        _ if unreadable => {
            assert_eq!(reason(&plan, StepKind::Replay), Some("the recorded edits can't be read"));
            let step = plan.steps.iter().find(|s| s.step == StepKind::Replay).unwrap();
            assert!(step.warnings[0].starts_with("the recorded edits can't be read: "), "{:?}", step.warnings);
        }
        _ if exported && (edit || smart) => want.push(StepKind::Replay),
        _ => assert!(reason(&plan, StepKind::Replay).is_some()),
    }
    // A rule-based playlist the export dropped.
    match () {
        _ if skipped(StepKind::Smart) => assert_eq!(reason(&plan, StepKind::Smart), Some("skipped")),
        _ if !smart => assert_eq!(reason(&plan, StepKind::Smart), Some("no rule-based playlists for this drive")),
        _ if exported => want.push(StepKind::Smart),
        _ => assert_eq!(reason(&plan, StepKind::Smart), Some("the rule-based playlists are up to date")),
    }
    // Filling a rule's playlist records an edit, which an unreadable journal can't take: the
    // playlists are filled all the same, unrecorded.
    let smart_warnings = &plan.steps.iter().find(|s| s.step == StepKind::Smart).unwrap().warnings;
    match want.contains(&StepKind::Smart) && unreadable {
        true => assert_eq!(smart_warnings, &["filling them isn't recorded: the recorded edits can't be read"]),
        false => assert!(smart_warnings.is_empty(), "{smart_warnings:?}"),
    }
    match (name, skipped(StepKind::Player)) {
        (_, true) => assert_eq!(reason(&plan, StepKind::Player), Some("skipped")),
        (None, _) => assert_eq!(
            reason(&plan, StepKind::Player),
            Some("no name or colour remembered for it (setting the name players show remembers them)")
        ),
        (Some(_), _) if !onelibrary => assert_eq!(
            reason(&plan, StepKind::Player),
            Some("it has no OneLibrary database, where players read the name")
        ),
        (Some(n), _) if !exported => {
            assert_eq!(reason(&plan, StepKind::Player).map(String::from), Some(format!("players show {n} already")))
        }
        (Some(_), _) => want.push(StepKind::Player),
    }
    match () {
        _ if skipped(StepKind::Settings) => assert_eq!(reason(&plan, StepKind::Settings), Some("skipped")),
        _ if settings == 0 => assert_eq!(
            reason(&plan, StepKind::Settings),
            Some("the My Settings profile is empty (`settings import` starts one)")
        ),
        _ if exported || settings == 2 => want.push(StepKind::Settings),
        _ => assert_eq!(reason(&plan, StepKind::Settings), Some("its settings are as the profile has them")),
    }
    let writes = !want.is_empty();
    let pull = plan.steps.iter().find(|s| s.step == StepKind::Pull).unwrap();
    match () {
        _ if skipped(StepKind::Pull) => assert_eq!(pull.reason.as_deref(), Some("skipped")),
        _ if exported => {
            assert_eq!(pull.summary, ["save Main in Home"]);
            want.push(StepKind::Pull);
        }
        _ if writes => {
            assert_eq!(pull.summary, ["save Main in the store after the steps above"]);
            want.push(StepKind::Pull);
        }
        _ => assert_eq!(pull.reason.as_deref(), Some("the store holds it already")),
    }
    if sync {
        let why = if skipped(StepKind::Sync) { "skipped" } else { "no other drive of its library is mounted" };
        assert_eq!(reason(&plan, StepKind::Sync), Some(why));
    }
    let running: Vec<StepKind> = plan.running().map(|s| s.step).collect();
    assert_eq!(running, want, "{:?}", plan.steps.iter().map(|s| (s.step, &s.reason)).collect::<Vec<_>>());

    if unreadable {
        // Put back, so the steps that record edits can.
        match &kept {
            Some(bytes) => std::fs::write(&journal, bytes).unwrap(),
            None => std::fs::remove_file(&journal).unwrap(),
        }
    }
    let report = match applied_by_plan {
        true => api::refresh_plan(&store, &req, &ctx).unwrap().apply(Some(&store), &ctx).unwrap(),
        false => api::refresh_apply(&store, &req, &ctx).unwrap(),
    };
    assert!(report.stopped.is_none(), "{:?}", report.stopped);
    let lists = playlists(&main);
    if want.contains(&StepKind::Player) {
        assert_eq!(player::db_identity(&main), remembered, "players show the name again");
    }
    if edit && want.contains(&StepKind::Replay) {
        assert!(lists.contains(&"Friday".to_string()), "{lists:?}");
    }
    if want.contains(&StepKind::Smart) {
        assert!(lists.contains(&"Everything".to_string()), "{lists:?}");
    }
    if want.contains(&StepKind::Pull) {
        // The same refresh again: what the steps that ran wrote is saved.
        let again = api::refresh_plan(&store, &req, &ctx).unwrap();
        assert_eq!(
            again.steps.iter().find(|s| s.step == StepKind::Pull).unwrap().reason.as_deref(),
            Some("the store holds it already")
        );
    }
}
