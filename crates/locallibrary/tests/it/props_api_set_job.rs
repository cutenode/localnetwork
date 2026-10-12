//! Getting drives the store can't all save, or put in order, ready for a set: what the job says
//! of each drive, and what it does, against a model of the drives.

use crate::common::{self, path};
use crate::props_api_drives::{saved_copy, saved_demo};
use crate::props_api_jobs::{bare_export, own_identity};

use hegel::generators as gs;
use hegel::{HealthCheck, TestCase};
use locallibrary::api::{self, StepKind};
use locallibrary::device::{Device, dunce_canonicalize};
use locallibrary::fleet;
use locallibrary::progress::{CancelToken, Ctx, Event, Progress, Scope};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};

/// Gives the drive at its path a new identity as the first step begins, once.
struct Swap(PathBuf, AtomicBool);

impl Progress for Swap {
    fn on_event(&self, _: &Event) {}
    fn on_step(&self, _: &Scope) {
        if !self.1.swap(true, Ordering::SeqCst) {
            fleet::reidentify(None, &mut Device::open(&self.0).unwrap(), Some("Impostor")).unwrap();
        }
    }
}

/// Whether `line` names the history lists the store lacks as a DJ reads them: none, each of up to
/// three, or past three how many and the first three.
fn names_lists(line: &str, lists: &[String]) -> bool {
    let Some((_, note)) = line.split_once(" (") else { return lists.is_empty() };
    match lists.len() {
        0 => false,
        1..=3 => note == format!("history lists the store lacks: {})", lists.join(", ")),
        n => {
            note.starts_with(&format!("{n} history lists the store lacks: {}", lists[..3].join(", ")))
                && note.ends_with("...)")
                && !note.contains(&lists[3])
        }
    }
}

/// Main with history lists players recorded (none to five), Old (a retired clone) and Loose (an
/// export in no library, with no audio), in any order: the job saves Main with its new lists
/// named (the first three, past three), starts a library for Loose and leaves Old, saying why;
/// it can't tell which drive holds the newest library, so copies none; it checks the three for
/// the models asked, or none remembered, and stops there, as Loose's audio is missing. Planned
/// again with Backup, a clone given a new identity before the job runs, it's refused as another
/// drive; once Backup moved on and was saved, it's the one copied from. Club, back from a set
/// while Main moved on, is saved beside Main's newest snapshot,
/// unless it was retired once the plan was made, which the job then says.
///
/// The explicit case takes them in that order, with more than three lists, so Old is tried as
/// the source after Main and finds Main newer.
#[hegel::test(test_cases = 3, suppress_health_check = [HealthCheck::TooSlow])]
#[hegel::explicit_test_case(n = 4usize, order = [0usize, 1, 2], players = Vec::<String>::new(), eject = true, by_plan = true)]
fn a_set_job_saves_what_it_can_and_says_why_not_the_rest(tc: TestCase) {
    let n = tc.draw(gs::integers::<usize>().max_value(5));
    // The order Main, Old and Loose are named in.
    let order = tc.draw(gs::sampled_from(vec![[0, 1, 2], [0, 2, 1], [1, 0, 2], [1, 2, 0], [2, 0, 1], [2, 1, 0]]));
    let players: Vec<String> = tc.draw(gs::sampled_from(vec![vec![], vec!["cdj-3000".to_string()]]));
    let eject = tc.draw(gs::booleans());
    let by_plan = tc.draw(gs::booleans());

    let dir = tempfile::tempdir().unwrap();
    let ctx = Ctx::silent();
    let (store, main) = saved_copy(dir.path());
    let volumes = dunce_canonicalize(main.parent().unwrap());
    let old = volumes.join("Old");
    saved_demo().1.write(&old);
    own_identity(&store, &mut Device::open(&old).unwrap(), "Old");
    fleet::retire(&store, Device::open(&old).unwrap().id().unwrap(), false).unwrap();
    let loose = volumes.join("Loose");
    bare_export(&loose);

    let ids: Vec<u32> = common::library(&main).tracks.iter().map(|t| t.id).collect();
    let lists: Vec<String> = (1..=n).map(|i| format!("HISTORY {i:03}")).collect();
    for l in &lists {
        common::add_history(&main, l, &ids[..1]);
    }
    let drives: Vec<std::path::PathBuf> = order.iter().map(|i| [&main, &old, &loose][*i].clone()).collect();
    let names: Vec<String> = drives.iter().map(|d| Device::open(d).unwrap().name()).collect();
    let req = api::SetJobRequest {
        drives: drives.iter().map(|d| path(d)).collect(),
        players: players.clone(),
        eject,
        ..Default::default()
    };
    let plan = api::set_job_plan(&store, &req, &ctx).unwrap();
    // Each drive once, and every one holding an export.
    let twice = api::SetJobRequest { drives: vec![path(&main), path(&main)], ..Default::default() };
    assert!(api::set_job_plan(&store, &twice, &ctx).is_err_and(|e| e.to_string().contains("is listed twice")));
    let blank = common::blank(&volumes, "Blank");
    let with_blank = api::SetJobRequest { drives: vec![path(&main), path(&blank)], ..Default::default() };
    assert!(matches!(api::set_job_plan(&store, &with_blank, &ctx), Err(locallibrary::Error::NotAnExport(_))));
    let refresh = api::RefreshRequest { drive: Some(path(&blank)), ..Default::default() };
    assert!(matches!(
        api::refresh_plan(&store, &refresh, &ctx).map(|p| p.noop),
        Err(locallibrary::Error::NotAnExport(_))
    ));
    std::fs::remove_dir(&blank).unwrap();
    let pull = &plan.steps[0];
    assert_eq!(pull.title, "Save the drives in the store, history lists first");
    assert_eq!(pull.warnings, ["Old won't be saved: it's retired"]);
    let saves_main = |l: &String| match n {
        0 => l == "the store holds Main already (Main)",
        _ => l.starts_with("save Main in Main") && names_lists(l, &lists),
    };
    assert!(pull.summary.iter().any(saves_main), "{:?}", pull.summary);
    assert!(pull.summary.contains(&"save Loose in the store, starting a library for it".to_string()));
    let sync = &plan.steps[1];
    assert_eq!(sync.reason.as_deref(), Some("no drive is newer than all the others"));
    assert!(sync.warnings[0].starts_with("can't tell which drive holds the newest library: "), "{:?}", sync.warnings);
    let check = &plan.steps[2].summary[0];
    let models = match players.is_empty() {
        true => "(no player models remembered: `players set` adds them)".to_string(),
        false => "for the CDJ-3000".to_string(),
    };
    assert_eq!(check, &format!("check {} {models}", names.join(", ")));

    let report = match by_plan {
        true => plan.apply(Some(&store), &ctx).unwrap(),
        false => api::set_job_apply(&store, &req, &ctx).unwrap(),
    };
    let done: Vec<StepKind> = report.steps.iter().map(|s| s.step).collect();
    assert_eq!(done, [StepKind::Pull, StepKind::Check], "stopped at the check: {:?}", report.stopped);
    assert!(report.problems && report.stopped.is_none());
    let saved = &report.steps[0];
    assert!(saved.warnings.is_empty(), "said when planned: {:?}", saved.warnings);
    let starts = |prefix: &str| saved.done.iter().any(|l| l.starts_with(prefix));
    assert!(starts("saved Loose in a new library, Loose, snapshot "), "{:?}", saved.done);
    match n {
        0 => assert!(!saved.done.iter().any(|l| l.contains("Main")), "nothing to save: {:?}", saved.done),
        _ => assert!(
            saved.done.iter().any(|l| l.starts_with("saved Main in Main, snapshot ") && names_lists(l, &lists)),
            "{:?}",
            saved.done
        ),
    }
    assert_eq!(saved.reports.len(), 1 + usize::from(n > 0), "a report for each drive saved");
    let c = report.check.as_ref().unwrap();
    assert_eq!(report.steps[1].done, [format!("{} problem(s) and {} warning(s)", c.problems, c.warnings)]);
    std::fs::remove_dir_all(&loose).unwrap();
    std::fs::remove_dir_all(&old).unwrap();

    // Another drive where Backup was, once the job is under way: the job stops rather than go on
    // with it.
    let backup = common::blank(&volumes, "Backup");
    api::sync_apply(Some(&store), &common::sync_pair(&main, &backup), &ctx).unwrap();
    common::adopt(Some(&store), &backup, "Backup");
    let req = api::SetJobRequest { drives: vec![path(&main), path(&backup)], eject: false, ..Default::default() };
    let plan = api::set_job_plan(&store, &req, &ctx).unwrap();
    let swap = Swap(backup.clone(), AtomicBool::new(false));
    let report = plan.apply(Some(&store), &Ctx::new(&swap, CancelToken::new())).unwrap();
    let stopped = report.stopped.as_ref().expect("refused");
    assert_eq!(stopped.kind, "stale_plan");
    assert!(stopped.message.contains(&format!("another drive is at {}", backup.display())), "{}", stopped.message);
    assert!(report.steps.is_empty(), "{:?}", report.steps);
    // Backup moved on and was saved: the job can't copy Main over it, so copies Backup onto Main.
    own_identity(&store, &mut Device::open(&backup).unwrap(), "Backup");
    common::export_new_track(&backup, "Artist", "At the club");
    api::pull(&store, &api::PullRequest { drive: path(&backup), ..Default::default() }, &ctx).unwrap();
    let plan = api::set_job_plan(&store, &req, &ctx).unwrap();
    let sync = plan.steps.iter().find(|s| s.step == StepKind::Sync).unwrap();
    assert_eq!(sync.summary.first().map(String::as_str), Some("Backup holds the newest library"), "{:?}", sync.reason);
    plan.apply(Some(&store), &ctx).unwrap();
    assert_eq!(common::library(&main).tracks, common::library(&backup).tracks, "Main holds Backup's library");
    std::fs::remove_dir_all(&backup).unwrap();

    // Club, synced from Main and played at a set, while Main moved on and was saved.
    let club = common::blank(&volumes, "Club");
    api::sync_apply(Some(&store), &common::sync_pair(&main, &club), &ctx).unwrap();
    common::adopt(Some(&store), &club, "Club");
    common::add_history(&club, "CLUB 001", &ids[..1]);
    common::export_new_track(&main, "Artist", "Fresh");
    api::pull(&store, &api::PullRequest { drive: path(&main), ..Default::default() }, &ctx).unwrap();
    // Retired once the plan was made (taken alone, nothing is written to it), it isn't saved
    // after all, and the job says so.
    let alone = api::SetJobRequest { drives: vec![path(&club)], eject: false, ..Default::default() };
    let plan = api::set_job_plan(&store, &alone, &ctx).unwrap();
    let beside =
        "save Club beside Main's newest snapshot: it's back from a set (history lists the store lacks: CLUB 001)";
    assert_eq!(plan.steps[0].summary, [beside]);
    let id = Device::open(&club).unwrap().id().unwrap();
    fleet::retire(&store, id, false).unwrap();
    let report = plan.apply(Some(&store), &ctx).unwrap();
    assert_eq!(report.steps[0].warnings, ["Club wasn't saved: it's retired"]);
    fleet::retire(&store, id, true).unwrap();
    // Taken with Main, and ejected once checked: a folder can't be ejected, which stops the job.
    let req = api::SetJobRequest { drives: vec![path(&main), path(&club)], eject: true, ..Default::default() };
    let plan = api::set_job_plan(&store, &req, &ctx).unwrap();
    assert_eq!(plan.steps[0].summary, [beside, "the store holds Main already (Main)"]);
    let report = plan.apply(Some(&store), &ctx).unwrap();
    if !report.problems {
        assert_eq!(report.stopped.as_ref().map(|s| s.step.as_str()), Some("eject"), "{:?}", report.stopped);
    }
    let saved = &report.steps[0].done;
    assert!(
        saved[0].starts_with("saved Club beside Main's newest snapshot, snapshot ")
            && saved[0].ends_with(" (history lists the store lacks: CLUB 001)"),
        "{saved:?}"
    );
    // Saving the group again has nothing to save, and says what the store holds.
    let group =
        api::pull_group(&store, &api::PullGroupRequest { library: Some("Main".into()), ..Default::default() }, &ctx)
            .unwrap();
    assert_eq!(group.steps.len(), 1);
    assert!(group.steps[0].reports.is_empty());
    assert!(group.steps[0].done.iter().all(|l| l.starts_with("the store holds ")), "{:?}", group.steps[0].done);
}
