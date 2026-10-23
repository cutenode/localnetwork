//! A refresh running plugins' operations, a step of its own: planned with the job, applied on the
//! drive as the steps before left it, and refused up front when one can't be planned.

use std::cell::Cell;

use hegel::TestCase;
use hegel::generators as gs;
use locallibrary::api::{DriveRef, JobPlan, OperationStep, RefreshRequest, StepKind};
use locallibrary::op::Planned;
use loclib_plugin::{Choice, ChoiceItem};

use super::*;

thread_local! {
    /// The plugins a refresh on this thread finds (see `locallibrary::plugin::set_hosted`).
    static HOSTED: Cell<Option<&'static Plugins>> = const { Cell::new(None) };
}

/// A fixture whose plugins are the app's host on this thread, as `refresh` finds them.
struct Hosted {
    _dir: tempfile::TempDir,
    store: Store,
    drive: PathBuf,
    plugins: &'static Plugins,
}

impl Hosted {
    /// `f`'s plugins, hosted. The host lives as long as the test process does (a plan holds it),
    /// its plugins stopped when the test ends.
    fn new(f: Fixture) -> Hosted {
        let Fixture { _dir, store, drive, plugins, .. } = f;
        let plugins: &'static Plugins = Box::leak(Box::new(plugins));
        locallibrary::plugin::set_hosted(|| HOSTED.with(Cell::get));
        HOSTED.with(|h| h.set(Some(plugins)));
        Hosted { _dir, store, drive, plugins }
    }

    fn refresh(&self, operations: Vec<OperationStep>, skip: Vec<StepKind>) -> Result<Planned<JobPlan>> {
        let req = RefreshRequest { drive: Some(common::path(&self.drive)), skip, sync: false, operations };
        locallibrary::api::refresh_plan(&self.store, &req, &Ctx::silent())
    }

    fn playlists(&self) -> Vec<String> {
        common::library(&self.drive).playlists.into_iter().map(|p| p.name).collect()
    }
}

impl Drop for Hosted {
    fn drop(&mut self) {
        HOSTED.with(|h| h.set(None));
        self.plugins.shutdown();
    }
}

fn op(operation: &str, params: Value) -> OperationStep {
    OperationStep { operation: operation.into(), params }
}

/// The steps that put the drive right, but cleaning it, and saving it: what's under test is the
/// operations.
const ONLY_OPERATIONS: [StepKind; 6] =
    [StepKind::Clean, StepKind::Replay, StepKind::Smart, StepKind::Player, StepKind::Settings, StepKind::Pull];

/// Operations run after the steps that put the drive right, each planned again on the drive as the
/// step before left it (the clean, then the operation before), then the drive is saved; each is
/// an edit, journaled.
#[test]
fn a_refresh_runs_operations_on_the_drive_the_steps_before_left() {
    let f = Hosted::new(Fixture::echo());
    crate::jobs::add_demo_sample(&f.drive);
    let ops = vec![op("echo.playlist", json!({ "name": "First" })), op("echo.playlist", json!({ "name": "Second" }))];
    let plan = f.refresh(ops, Vec::new()).unwrap();
    let kinds: Vec<StepKind> = plan.running().map(|s| s.step).collect();
    assert_eq!(kinds, [StepKind::Clean, StepKind::Operations, StepKind::Pull]);
    let step = plan.steps.iter().find(|s| s.step == StepKind::Operations).unwrap();
    assert_eq!(step.title, "Run the plugins' operations");
    assert_eq!(step.plans.len(), 2);
    assert_eq!(step.plans[1]["operation"], "echo.playlist");
    assert!(step.summary.contains(&"make the playlist Second".to_string()), "{:?}", step.summary);
    assert!(!f.playlists().contains(&"First".to_string()), "nothing done before it's applied");

    // Each operation's plan is stale once the step before it wrote the drive: planned again.
    let report = plan.apply(Some(&f.store), &Ctx::silent()).unwrap();
    assert!(report.stopped.is_none(), "{:?}", report.stopped);
    let ran: Vec<(StepKind, usize)> = report.steps.iter().map(|s| (s.step, s.reports.len())).collect();
    assert_eq!(ran, [(StepKind::Clean, 1), (StepKind::Operations, 2), (StepKind::Pull, 1)]);
    let playlists = f.playlists();
    assert!(playlists.ends_with(&["First".to_string(), "Second".to_string()]), "{playlists:?}");
    let journal = locallibrary::api::edits_list(&DriveRef { drive: common::path(&f.drive) }).unwrap();
    assert_eq!(journal.len(), 2, "each journaled, as any operation");

    // Skipped, they aren't even planned.
    let skipped = f.refresh(vec![op("nope.playlist", Value::Null)], vec![StepKind::Operations]).unwrap();
    let step = skipped.steps.iter().find(|s| s.step == StepKind::Operations).unwrap();
    assert_eq!((step.runs, step.reason.as_deref()), (false, Some("skipped")));
}

/// Without the app's plugins, a refresh asked to run one's operation is refused.
#[test]
fn a_refresh_without_plugins_refuses_their_operations() {
    let f = Fixture::echo();
    let req = RefreshRequest {
        drive: Some(common::path(&f.drive)),
        operations: vec![op("echo.playlist", json!({ "name": "x" }))],
        ..RefreshRequest::default()
    };
    let e = locallibrary::api::refresh_plan(&f.store, &req, &Ctx::silent()).err().unwrap();
    assert_eq!((e.code(), e.to_string()), ("not_found", "not found: plugin \"echo\"".into()));
    assert_eq!(e.detail().fields["operation"], "echo.playlist");
}

/// An operation that asks a question first is refused: a refresh can't ask it.
#[test]
fn a_refresh_refuses_an_operation_that_asks() {
    // Its name not required, so it may be asked for.
    let manifest = ECHO.replace(", required = [\"name\"] }", " }");
    let f = Hosted::new(Fixture::new(|dir| vec![spec(dir, &manifest, &[])], Options::default()));
    let item = |v: &str| ChoiceItem { label: v.into(), detail: None, value: json!(v) };
    let ask = Choice { title: "Which one?".into(), param: "name".into(), multi: false, items: vec![item("Peak")] };
    let e = f.refresh(vec![op("echo.playlist", json!({ "ask": ask }))], ONLY_OPERATIONS.to_vec()).err().unwrap();
    assert_eq!(e.code(), "invalid");
    assert_eq!(e.to_string(), "Which one?: it asks which first, as `name` in its params");
    assert_eq!(e.detail().fields["operation"], "echo.playlist");
}

/// An operation a refresh is asked to run.
#[derive(Debug, Clone, Copy)]
enum Asked {
    /// Echo's, making a playlist of its own.
    Make,
    MissingPlugin,
    MissingOperation,
    /// A name that isn't `PLUGIN.OPERATION`.
    Unnamed,
    /// Echo's, without the name it needs.
    NoName,
}

/// For any operations a refresh is asked to run, the plan is refused up front at the first that
/// can't be planned (its plugin or the operation is missing, it isn't named `PLUGIN.OPERATION`,
/// or the params don't fit), with that error's kind and naming it, and nothing
/// is written; otherwise it's one step, a plan for each, and applying it makes each in order.
/// Skipped, none is planned.
#[hegel::test(test_cases = 6, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn a_refreshs_operations_are_planned_up_front(tc: TestCase) {
    let f = Hosted::new(Fixture::echo());
    let all = vec![Asked::Make, Asked::MissingPlugin, Asked::MissingOperation, Asked::Unnamed, Asked::NoName];
    let asked: Vec<Asked> = tc.draw(gs::vecs(gs::sampled_from(all)).min_size(1).max_size(4));
    let skip: bool = tc.draw(gs::booleans());
    let ops: Vec<OperationStep> = asked
        .iter()
        .enumerate()
        .map(|(i, a)| match a {
            Asked::Make => op("echo.playlist", json!({ "name": format!("P{i}") })),
            Asked::MissingPlugin => op("nope.playlist", json!({ "name": "x" })),
            Asked::MissingOperation => op("echo.nope", Value::Null),
            Asked::Unnamed => op("echo", Value::Null),
            Asked::NoName => op("echo.playlist", json!({})),
        })
        .collect();
    let before = f.playlists();
    let mut skipping = ONLY_OPERATIONS.to_vec();
    skipping.extend(skip.then_some(StepKind::Operations));
    let planned = f.refresh(ops.clone(), skipping);
    let refused = asked.iter().zip(&ops).find(|(a, _)| !matches!(a, Asked::Make)).filter(|_| !skip);
    if let Some((a, o)) = refused {
        let e = planned.err().unwrap();
        let kind = match a {
            Asked::MissingPlugin | Asked::MissingOperation => "not_found",
            _ => "invalid",
        };
        assert_eq!(e.code(), kind, "{a:?}: {e}");
        assert_eq!(e.detail().fields["operation"], json!(o.operation), "{a:?}");
        assert_eq!(f.playlists(), before);
        return;
    }
    let plan = planned.unwrap();
    let step = plan.steps.iter().find(|s| s.step == StepKind::Operations).unwrap();
    if skip {
        assert_eq!((step.runs, step.reason.as_deref()), (false, Some("skipped")));
        assert!(plan.noop);
        return;
    }
    assert_eq!(step.plans.len(), ops.len());
    let report = plan.apply(Some(&f.store), &Ctx::silent()).unwrap();
    assert!(report.stopped.is_none(), "{:?}", report.stopped);
    let made: Vec<String> = (0..ops.len()).map(|i| format!("P{i}")).collect();
    assert_eq!(f.playlists(), [before, made].concat());
}
