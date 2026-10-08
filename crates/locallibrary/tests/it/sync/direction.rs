//! Which way a sync goes, through the service layer: what's refused (and what the refusal offers
//! instead) when the drives can't be put in order, a blank drive on either side, drives that
//! already match, and several drives at once with a target that went away after planning.

use crate::common::{self, blank, path, sync_many, sync_pair};

use std::path::Path;

use locallibrary::api::{self, Direction, SyncRequest};
use locallibrary::compare::Relation;
use locallibrary::device::{Device, dunce_canonicalize};
use locallibrary::error::Reason;
use locallibrary::progress::Ctx;
use locallibrary::store::Store;
use locallibrary::{Error, Result};

fn plan(req: &SyncRequest) -> Result<locallibrary::op::Planned<api::SyncPreview, SyncRequest>> {
    api::sync_plan(req, &Ctx::silent())
}

/// The labels of the actions a refusal offers.
fn offered(e: &Error) -> Vec<String> {
    e.detail().actions.into_iter().map(|a| a.label).collect()
}

/// A folder for the drives, the only one drives are looked for in on this thread.
fn drives_folder() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    locallibrary::testing::volumes_on_this_thread(Some(vec![dir.path().to_path_buf()]));
    dir
}

/// Two drives that can't be synced on their own are refused, saying why and offering the ways
/// on: copying either way, and merging where that would make sense.
#[test]
fn drives_that_cant_be_put_in_order_are_refused_with_the_ways_on() {
    let dir = drives_folder();
    let main = common::demo_drive(dir.path(), "Main");

    let err = plan(&sync_pair(&main, &main)).unwrap_err();
    assert!(err.to_string().contains("same drive"), "{err}");
    let (a, b) = (blank(dir.path(), "A"), blank(dir.path(), "B"));
    assert!(plan(&sync_pair(&a, &b)).unwrap_err().to_string().contains("neither drive"));

    let club = common::bigger_drive(dir.path(), "Club");
    let err = plan(&sync_pair(&main, &club)).unwrap_err();
    assert_eq!(err.reason(), Some(Reason::Unrelated), "{err}");
    assert_eq!(
        offered(&err),
        ["Copy Main over Club", "Copy Club over Main", "Merge Main into Club", "Merge Club into Main"]
    );

    // Both changed since they were copies of each other.
    let copy = dir.path().join("Copy");
    common::copy_dir(&main, &copy);
    common::export_new_track(&main, "Artist", "Here");
    common::export_new_track(&copy, "Artist", "There");
    let err = plan(&sync_pair(&main, &copy)).unwrap_err();
    assert_eq!(err.reason(), Some(Reason::Diverged), "{err}");
    assert_eq!(offered(&err).len(), 4);

    // Keeping the players' changes is only for a drive back from a set.
    let pinned =
        SyncRequest { direction: Direction::LeftToRight, keep_player_changes: true, ..sync_pair(&main, &copy) };
    assert!(matches!(plan(&pinned), Err(Error::StalePlan(_))));
}

/// A guess at which drive is newer is refused unless taken; taken, it copies the way it guessed.
#[test]
fn a_guess_at_the_newer_drive_is_taken_only_when_asked() {
    let dir = drives_folder();
    let main = common::demo_drive(dir.path(), "Main");
    let copy = dir.path().join("Copy");
    common::copy_dir(&main, &copy);
    // A player saved history on the copy: its database moved on, nothing else tells them apart.
    let ids: Vec<u32> = Device::open(&copy).unwrap().library().unwrap().tracks.iter().map(|t| t.id).collect();
    common::add_history(&copy, "HISTORY 001", &ids);

    let err = plan(&sync_pair(&main, &copy)).unwrap_err();
    assert_eq!(err.reason(), Some(Reason::Uncertain), "{err}");
    assert_eq!(offered(&err), ["Copy Main over Copy", "Copy Copy over Main", "Take the guess"]);
    let err = api::sync_many_plan(&sync_many(&[&main, &copy]), &Ctx::silent()).unwrap_err();
    assert_eq!(err.reason(), Some(Reason::Uncertain), "{err}");

    let guess = SyncRequest { allow_low_confidence: true, ..sync_pair(&main, &copy) };
    let planned = plan(&guess).unwrap();
    assert_eq!(planned.comparison.relation, Relation::RightNewer);
    assert_eq!(planned.source.as_deref(), Some(dunce_canonicalize(&copy).as_path()));
}

/// A blank drive is only ever a target, on either side; drives holding the same library already
/// have nothing to sync, planned or applied.
#[test]
fn a_blank_drive_is_a_target_and_matching_drives_need_nothing() {
    let dir = drives_folder();
    let main = common::demo_drive(dir.path(), "Main");
    let fresh = blank(dir.path(), "Fresh");

    let planned = plan(&sync_pair(&fresh, &main)).unwrap();
    assert_eq!(planned.comparison.relation, Relation::RightNewer);
    assert_eq!(planned.target.as_deref(), Some(dunce_canonicalize(&fresh).as_path()));
    let done = api::sync_apply(None, &sync_pair(&fresh, &main), &Ctx::silent()).unwrap();
    assert!(done.report.is_some_and(|r| r.copied > 0));

    let same = plan(&sync_pair(&main, &fresh)).unwrap();
    assert!(same.noop && same.plan.plan.is_none(), "{:?}", same.summary);
    let nothing = api::sync_apply(None, &sync_pair(&main, &fresh), &Ctx::silent()).unwrap();
    assert!(nothing.report.is_none() && nothing.source.is_none());
    let compared = api::compare(&api::ComparePair { left: path(&main), right: path(&fresh) }, &Ctx::silent()).unwrap();
    assert_eq!(compared.relation, Relation::Identical);
}

/// Several drives at once need two different drives, one holding an export, all of one library.
#[test]
fn several_drives_must_be_two_of_one_library() {
    let dir = drives_folder();
    let main = common::demo_drive(dir.path(), "Main");
    let fresh = blank(dir.path(), "Fresh");
    let refused = |drives: &[&Path]| api::sync_many_plan(&sync_many(drives), &Ctx::silent()).unwrap_err();

    assert!(refused(&[&main]).to_string().contains("at least two"));
    assert!(refused(&[&main, &main]).to_string().contains("listed twice"));
    let other = blank(dir.path(), "Other");
    assert!(refused(&[&fresh, &other]).to_string().contains("none of the drives"));
    let club = common::bigger_drive(dir.path(), "Club");
    let err = refused(&[&main, &fresh, &club]);
    assert_eq!(err.reason(), Some(Reason::Unrelated), "{err}");

    let copy = dir.path().join("Copy");
    common::copy_dir(&main, &copy);
    common::export_new_track(&main, "Artist", "Here");
    common::export_new_track(&copy, "Artist", "There");
    assert_eq!(refused(&[&main, &copy]).reason(), Some(Reason::Diverged));
}

/// Applying a plan for several drives copies onto each target that's still there; one that went
/// away since is reported as failed, and the store registers the drives written.
#[test]
fn a_target_gone_since_planning_fails_alone() {
    let dir = drives_folder();
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    let main = common::demo_drive(dir.path(), "Main");
    Device::open(&main).unwrap().adopt(Some("Main")).unwrap();
    let (a, b) = (blank(dir.path(), "A"), blank(dir.path(), "B"));
    let req = sync_many(&[&a, &main, &b]);

    let planned = api::sync_many_plan(&req, &Ctx::silent()).unwrap();
    assert_eq!(planned.source_name, "Main");
    std::fs::remove_dir(&b).unwrap();
    // Applied as planned, without the plan's own check that every drive is still there.
    let results = api::sync_many_apply(Some(&store), &req, &planned.plan, &Ctx::silent()).unwrap();
    let outcome: Vec<(String, bool)> = results.iter().map(|r| (r.name.clone(), r.error.is_none())).collect();
    assert_eq!(outcome, [("B".to_string(), false), ("A".to_string(), true)]);
    assert_eq!(results[0].error.as_ref().map(|e| e.kind), Some("not_found"));
    let registered: Vec<String> = store.drives().unwrap().into_values().map(|d| d.name).collect();
    assert!(registered.contains(&"Main".to_string()) && registered.len() == 2, "{registered:?}");
}
