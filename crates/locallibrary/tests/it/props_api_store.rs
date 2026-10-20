//! What the service layer does without the store, or with a store that isn't the drive's: plans
//! that keep something in the store are refused when applied without it, and lookups of a drive's
//! library in a store that doesn't hold it say so, each before anything is written.

use crate::common::{self, path};
use crate::props_api_drives::saved_copy;

use hegel::generators as gs;
use hegel::{HealthCheck, TestCase};
use locallibrary::api;
use locallibrary::device::{Device, dunce_canonicalize};
use locallibrary::progress::Ctx;
use locallibrary::store::Store;
use locallibrary::{Error, Result};

/// A plan applied without the store.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Plan {
    Provision,
    Push,
    AdoptAs,
    ImportSettings,
    Players,
    SetJob,
}

/// The saved demo's Main (with a track exported since it was saved), New (a blank drive), and
/// the store holding Main's library.
fn apply_without_store(store: &Store, main: &str, new: &str, plan: Plan) -> Result<String> {
    let ctx = Ctx::silent();
    let refused = |r: Result<()>| r.map(|()| "applied".to_string());
    match plan {
        Plan::Provision => {
            let req: api::ProvisionRequest = serde_json::from_value(serde_json::json!({ "target": new })).unwrap();
            refused(api::provision_plan(store, &req, &ctx)?.apply(None, &ctx).map(|_| ()))
        }
        Plan::Push => {
            let req: api::PushRequest = serde_json::from_value(serde_json::json!({ "drive": new })).unwrap();
            refused(api::push_plan(store, &req, &ctx)?.apply(None, &ctx).map(|_| ()))
        }
        Plan::AdoptAs => {
            let req = api::AdoptAs { drive: new.into(), known: "Main".into() };
            refused(api::drive_adopt_as_plan(store, &req)?.apply(None, &ctx).map(|_| ()))
        }
        Plan::ImportSettings => {
            let req = api::ImportSettings { drive: Some(main.into()) };
            refused(api::settings_import_plan(store, &req)?.apply(None, &ctx).map(|_| ()))
        }
        Plan::Players => {
            let req = api::SetPlayers { models: vec!["cdj-3000".into()], library: Some("Main".into()) };
            refused(api::players_set_plan(store, &req)?.apply(None, &ctx).map(|_| ()))
        }
        Plan::SetJob => {
            let req = api::SetJobRequest { drives: vec![main.into()], eject: false, ..Default::default() };
            let report = api::set_job_plan(store, &req, &ctx)?.apply(None, &ctx)?;
            // A job reports the step it stopped at instead of failing.
            assert!(report.steps.is_empty(), "nothing done before the pull: {:?}", report.steps);
            let stopped = report.stopped.expect("stopped at the pull");
            assert_eq!(stopped.step, "pull");
            Err(Error::Invalid(stopped.message))
        }
    }
}

/// Plans that keep something in the store (setting up a drive, pushing a snapshot, giving a drive
/// a known identity, importing My Settings, remembering player models, and a set job's saving of a
/// drive the store hasn't got yet), each planned with the store and applied without it in any
/// order, are refused saying what needs the store, and leave the store and both drives as they
/// were. Without a store, a library's drive group can't be found either.
#[hegel::test(test_cases = 4, suppress_health_check = [HealthCheck::TooSlow])]
#[hegel::explicit_test_case(plans = vec![Plan::Provision, Plan::Push, Plan::AdoptAs, Plan::ImportSettings, Plan::Players, Plan::SetJob])]
fn plans_that_keep_something_in_the_store_are_refused_without_it(tc: TestCase) {
    let all = vec![Plan::Provision, Plan::Push, Plan::AdoptAs, Plan::ImportSettings, Plan::Players, Plan::SetJob];
    let plans = tc.draw(gs::vecs(gs::sampled_from(all)).min_size(1).max_size(4));
    let dir = tempfile::tempdir().unwrap();
    let (store, main) = saved_copy(dir.path());
    common::export_new_track(&main, "Artist", "Since the save");
    let new = dunce_canonicalize(&common::blank(main.parent().unwrap(), "New"));
    let before = (common::drive_files(store.root()), common::drive_files(&main), common::drive_files(&new));

    for plan in plans {
        let why = match plan {
            Plan::Provision => "setting up a drive needs the store",
            Plan::Push => "writing a snapshot needs the store",
            Plan::AdoptAs => "a known drive's identity is in the store",
            Plan::ImportSettings => "importing settings needs the store",
            Plan::Players => "remembering player models needs the store",
            Plan::SetJob => "saving a drive needs the store",
        };
        let result = apply_without_store(&store, &path(&main), &path(&new), plan);
        assert!(matches!(&result, Err(Error::Invalid(m)) if m == why), "{plan:?}: {result:?}");
        let after = (common::drive_files(store.root()), common::drive_files(&main), common::drive_files(&new));
        assert!(after == before, "{plan:?} changed nothing");
    }
    assert!(Device::open(&new).is_ok_and(|d| d.id().is_none()), "New isn't given an identity");
    let group = api::drive_group(None, Some("Main"));
    assert!(matches!(&group, Err(Error::NotFound(m)) if m.contains("there's no store yet")), "{group:?}");
}

/// Main, linked to its library in the store it was saved in, read against another store that
/// holds no such library (or one, Empty, with no snapshots): restoring a playlist from the
/// library's snapshots and counting plays since a date (any of a year, a month or a day) are
/// refused naming the library that isn't there, and a difference from Empty is refused for its
/// having no snapshots; nothing on Main changes. Renaming Main there names the libraries it has.
#[hegel::test(test_cases = 6, suppress_health_check = [HealthCheck::TooSlow])]
fn a_drive_whose_library_another_store_lacks_is_refused_naming_it(tc: TestCase) {
    let since = tc.draw(gs::sampled_from(vec!["2025", "2025-09", "2025-09-30"]));
    let playlist = tc.draw(gs::sampled_from(vec!["Techno", "Folder/Name", "nothing like it"]));
    let empty_left = tc.draw(gs::booleans());
    let dir = tempfile::tempdir().unwrap();
    let (store, main) = saved_copy(dir.path());
    let library = store.find_library("Main").unwrap().expect("saved").id;
    let other = Store::open_or_init(dir.path().join("other")).unwrap();
    // Renaming a library names the ones there are when the one named isn't.
    let rename = |library: &str| {
        let req = api::RenameLibrary { library: library.into(), name: "Renamed".into() };
        api::library_rename(&other, &req).map(|l| l.name)
    };
    let none = rename("Main");
    assert!(matches!(&none, Err(Error::NotFound(m)) if m.ends_with("(it has no libraries yet)")), "{none:?}");
    other.create_library("Empty").unwrap();
    let one = rename("Main");
    assert!(matches!(&one, Err(Error::NotFound(m)) if m.ends_with("(it has \"Empty\")")), "{one:?}");
    let before = common::drive_files(&main);
    let ctx = Ctx::silent();

    let req =
        api::PlaylistRestoreRequest { drive: path(&main), playlist: playlist.into(), snapshot: None, backup: None };
    let restore = api::playlist_restore_plan(Some(&other), &req, &ctx).map(|p| p.noop);
    let missing = format!("library {library} in the store");
    assert!(matches!(&restore, Err(Error::NotFound(m)) if *m == missing), "{restore:?}");

    let req = api::PlayStatsRequest { source: path(&main), since: Some(since.into()), limit: None };
    let stats = api::play_stats(Some(&other), &req).map(|s| s.since.is_some());
    assert!(matches!(&stats, Err(Error::NotFound(m)) if m == "the source's library"), "{stats:?}");
    // In its own store, the library is there: it's the date that has no snapshot before it.
    let own = api::play_stats(Some(&store), &req).map(|s| s.since.is_some());
    assert!(matches!(&own, Err(Error::NotFound(m)) if m.starts_with("a snapshot of Main from before")), "{own:?}");

    let (left, right) = match empty_left {
        true => ("Empty".to_string(), path(&main)),
        false => (path(&main), "Empty".to_string()),
    };
    let diff = api::diff(Some(&other), &api::DiffRequest { left, right, playlist: None }).map(|_| ());
    assert!(matches!(&diff, Err(Error::NotFound(m)) if m == "library \"Empty\" has no snapshots"), "{diff:?}");
    assert_eq!(common::drive_files(&main), before);
}
