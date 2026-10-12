//! Pulls refused or labelled as their requests say, and operations planned and applied in one
//! go doing what their plans said.

use crate::common::{self, path};
use crate::props_api_drives::saved_copy;
use crate::props_api_jobs::{SAMPLE, add_demo_sample};

use hegel::generators as gs;
use hegel::{HealthCheck, TestCase};
use locallibrary::api;
use locallibrary::convert::{ConvertOptions, Target};
use locallibrary::error::Reason;
use locallibrary::progress::Ctx;
use locallibrary::{Error, paths};

/// Main, saved in the library Main with a labelled snapshot, then exported to: a pull into a
/// library the store lacks is refused, naming the ones it has and offering to start it, unless
/// asked to; a blank label, or one already taken, is refused before anything is pulled; else the
/// new snapshot carries the label, trimmed. Pulled again, there's nothing new: the report names
/// the snapshot it matches, labels and all.
#[hegel::test(test_cases = 20, suppress_health_check = [HealthCheck::TooSlow])]
#[hegel::explicit_test_case(library = Some("Radio"), new_library = false, label = None::<&str>)]
#[hegel::explicit_test_case(library = Some("main"), new_library = false, label = Some("   "))]
#[hegel::explicit_test_case(library = None::<&str>, new_library = false, label = Some("Friday"))]
#[hegel::explicit_test_case(library = Some("Main"), new_library = true, label = Some(" after the gig "))]
fn a_pull_is_labelled_unless_its_library_or_label_is_wrong(tc: TestCase) {
    let library = tc.draw(gs::optional(gs::sampled_from(vec!["Main", "main", "Radio"])));
    let new_library = tc.draw(gs::booleans());
    let label = tc.draw(gs::optional(gs::sampled_from(vec!["", "   ", "Friday", "friday", " after the gig "])));

    let dir = tempfile::tempdir().unwrap();
    let ctx = Ctx::silent();
    let (store, main) = saved_copy(dir.path());
    let head = store.find_library("Main").unwrap().unwrap().head.unwrap();
    store.add_label(&head, "Friday").unwrap();
    common::export_new_track(&main, "Artist", "Fresh");
    let req = api::PullRequest {
        drive: path(&main),
        library: library.map(String::from),
        new_library,
        label: label.map(String::from),
        ..Default::default()
    };
    let pulled = api::pull(&store, &req, &ctx);
    let known = library.is_none_or(|l| l.eq_ignore_ascii_case("main"));
    let trimmed = label.map(str::trim);
    match () {
        _ if !known && !new_library => {
            let e = pulled.unwrap_err();
            assert_eq!(e.reason(), Some(Reason::UnknownLibrary), "{e}");
            assert!(e.to_string().contains("Main"), "names the libraries there are: {e}");
            assert_eq!(e.detail().actions[0].params["new_library"], true);
        }
        _ if trimmed == Some("") => assert!(matches!(pulled, Err(Error::Invalid(_))), "{pulled:?}"),
        _ if trimmed.is_some_and(|l| l.eq_ignore_ascii_case("friday")) => {
            let e = pulled.unwrap_err();
            assert!(matches!(e.inner(), Error::Conflict(_)) && e.to_string().contains("Friday"), "{e}");
        }
        _ => {
            let report = pulled.unwrap();
            assert!(!report.up_to_date);
            let snap = report.snapshot.expect("a new snapshot");
            assert_eq!(snap.labels, trimmed.into_iter().map(String::from).collect::<Vec<_>>());
            let again =
                api::pull(&store, &api::PullRequest { drive: path(&main), ..Default::default() }, &ctx).unwrap();
            // The snapshot it matches, with its labels.
            assert!(again.up_to_date, "{again:?}");
            let matched = again.snapshot.expect("the snapshot it matches");
            assert_eq!((matched.id, matched.labels), (snap.id, snap.labels));
            return;
        }
    }
    let after = store.find_library("Main").unwrap().unwrap().head.unwrap();
    assert_eq!(after, head, "nothing pulled");
}

/// rekordbox's demo sample on Main, or not; a track exported only to its export.pdb; no settings
/// files: cleaning removes the sample (planned, or in one go), once; converting to OneLibrary
/// adds what export.pdb has to the database there (or rebuilds it), after which OneLibrary
/// holds every track; importing settings from a drive with none is refused, and so is writing
/// settings with no drive mounted.
#[hegel::test(test_cases = 6, suppress_health_check = [HealthCheck::TooSlow])]
fn operations_applied_in_one_go_do_what_their_plans_say(tc: TestCase) {
    let sample = tc.draw(gs::booleans());
    let in_one_go = tc.draw(gs::booleans());
    let replace = tc.draw(gs::booleans());

    let dir = tempfile::tempdir().unwrap();
    let ctx = Ctx::silent();
    let (store, main) = saved_copy(dir.path());
    let drive = api::DriveRef { drive: path(&main) };
    if sample {
        add_demo_sample(&main);
    }
    let plan = api::clean_plan(&drive, &ctx).unwrap();
    assert_eq!(plan.noop, !sample);
    match in_one_go {
        true => drop(api::clean_apply(&drive, &ctx).unwrap()),
        false => drop(plan.apply(Some(&store), &ctx).unwrap()),
    }
    assert!(!paths::to_fs(&main, &paths::key(SAMPLE)).exists(), "the sample is gone");
    assert!(api::clean_plan(&drive, &ctx).unwrap().noop, "once");

    common::add_onelibrary(&main);
    common::export_new_track(&main, "Artist", "Only in export.pdb");
    let req = api::ConvertRequest { drive: path(&main), options: ConvertOptions { to: Target::OneLibrary, replace } };
    let plan = api::convert_plan(&req, &ctx).unwrap();
    assert_eq!((plan.adds, plan.replaces), (!replace, replace));
    let line = &plan.summary[0];
    match replace {
        true => assert!(
            line.starts_with("build exportLibrary.db from export.pdb on Main")
                && line.ends_with(", replacing the exportLibrary.db it has"),
            "{line}"
        ),
        false => {
            assert!(line.starts_with("add what export.pdb has to the exportLibrary.db on Main (1 tracks, "), "{line}")
        }
    }
    match in_one_go {
        true => drop(api::convert_apply(&req, &ctx).unwrap()),
        false => drop(plan.apply(None, &ctx).unwrap()),
    }
    let titles = |lib: locallibrary::Library| {
        let mut t: Vec<String> = lib.tracks.into_iter().map(|t| t.title).collect();
        t.sort();
        t
    };
    assert_eq!(titles(common::plus_library(&main)), titles(common::library(&main)));

    let none = api::ImportSettings { drive: Some(path(&main)) };
    for f in std::fs::read_dir(main.join(paths::PIONEER_DIR)).unwrap() {
        let f = f.unwrap().path();
        if f.extension().is_some_and(|e| e == "DAT") {
            std::fs::remove_file(f).unwrap();
        }
    }
    let e = api::settings_import_plan(&store, &none).map(|p| p.noop).unwrap_err();
    assert!(e.to_string().contains("no settings files found to import"), "{e}");
    std::fs::remove_dir_all(&main).unwrap();
    let e = api::settings_apply_plan(&store, &api::SettingsApplyRequest::default()).map(|p| p.noop).unwrap_err();
    assert!(e.to_string().contains("a mounted rekordbox drive"), "{e}");
}
