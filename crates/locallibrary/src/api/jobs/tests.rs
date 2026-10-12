use super::*;

/// Steps are named as requests and progress events name them, in any case; an unknown one is
/// refused with the names it could have been.
#[test]
fn steps_are_parsed_by_their_ids() {
    assert_eq!(StepKind::parse(" Sync ").unwrap(), StepKind::Sync);
    assert!(StepKind::ALL.iter().all(|k| StepKind::parse(k.id()).ok() == Some(*k)));
    let err = StepKind::parse("dance").unwrap_err().to_string();
    assert!(
        err.contains("unknown step \"dance\" (clean, replay, smart, player, settings, pull, sync, check, eject)"),
        "{err}"
    );
}

/// Regression: when ejecting the drives failed part way, the report dropped the ones already
/// ejected.
#[test]
fn a_failed_eject_still_reports_the_drives_ejected() {
    let roots = [PathBuf::from("/drives/A"), PathBuf::from("/drives/B")];
    let mut done = StepReport { step: StepKind::Eject, done: vec![], warnings: vec![], reports: vec![] };
    let result = eject_each(&roots, &mut done, |root| match root.ends_with("A") {
        true => Ok(EjectReport { name: "A".into(), root: root.into(), commands: vec![], powered_off: true }),
        false => Err(Error::invalid("B is busy")),
    });
    assert!(result.is_err());
    assert_eq!(done.done, ["A ejected; it can be pulled out"]);
    assert_eq!(done.reports.len(), 1);
}

/// How a pretend operation's plan goes when it's applied.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Goes {
    /// Applied, saying what it did.
    Done,
    /// Refused as stale: planned again, which goes through.
    Stale,
    /// Planned only as its step runs, when it turns out to have nothing to do.
    Later,
    /// Fails.
    Fails,
    /// Applied to several drives, one of which failed.
    Partly,
    Cancelled,
}

/// A pretend operation, for running jobs without drives.
#[derive(Clone)]
struct Fake {
    summary: Vec<String>,
    warnings: Vec<String>,
    noop: bool,
    goes: Goes,
}

impl AnyPlanned for Fake {
    fn id(&self) -> &str {
        "fake"
    }
    fn operation(&self) -> &'static str {
        "fake"
    }
    fn summary(&self) -> &[String] {
        &self.summary
    }
    fn warnings(&self) -> &[String] {
        &self.warnings
    }
    fn destructive(&self) -> bool {
        false
    }
    fn is_noop(&self) -> bool {
        self.noop
    }
    fn drives(&self) -> &[Touched] {
        &[]
    }
    fn planned_at(&self) -> chrono::DateTime<chrono::Utc> {
        chrono::Utc::now()
    }
    fn to_json(&self) -> Value {
        serde_json::json!({ "summary": self.summary })
    }
    fn check(&self, _: Option<&Store>) -> Result<()> {
        Ok(())
    }
    fn apply_json(&self, _: Option<&Store>, _: &Ctx<'_>) -> Result<Value> {
        match self.goes {
            Goes::Done | Goes::Later => Ok(serde_json::json!({ "done": self.summary })),
            Goes::Stale => Err(Error::StalePlan("the drive changed".into())),
            Goes::Fails => Err(Error::invalid("it broke")),
            Goes::Partly => Ok(serde_json::json!([{ "error": null }, { "error": { "message": "B is busy" } }])),
            Goes::Cancelled => Err(Error::Cancelled),
        }
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// A job of pretend operations runs its steps in order, each operation on the plan that was
/// shown or, refused as stale or planned as the step runs, on one planned again (whose new
/// warnings the step reports once, and which may have nothing left to do): a step's report says
/// what its operations did; the first step to fail stops the job, kept in the report when it did
/// something first, and named with its error; cancelling stops it at once. The plan sums up the
/// steps that run (by title when they say nothing more) with every step's warnings.
#[hegel::test(test_cases = 200)]
fn a_job_runs_its_steps_until_one_fails(tc: hegel::TestCase) {
    use hegel::Generator;
    use hegel::generators as gs;
    let goes = || {
        gs::sampled_from(vec![Goes::Done, Goes::Stale, Goes::Later, Goes::Fails, Goes::Partly, Goes::Cancelled])
            .print_as_debug()
    };
    let kinds = tc.draw(gs::vecs(gs::sampled_from(StepKind::ALL.to_vec()).print_as_debug()).max_size(4).unique(true));
    let mut steps = Vec::new();
    let mut model: Vec<(StepKind, Vec<Goes>, Vec<String>)> = Vec::new();
    for (s, kind) in kinds.iter().enumerate() {
        let mut step = JobStep::new(*kind);
        let ops = tc.draw(gs::vecs(goes()).max_size(3));
        let mut shown = Vec::new();
        for (o, g) in ops.iter().enumerate() {
            let line = format!("op {s}.{o}");
            let warned = tc.draw(gs::booleans()).then(|| format!("careful with {s}.{o}"));
            if *g != Goes::Later {
                shown.extend(warned.clone());
            }
            let again = Fake {
                summary: vec![format!("{line} again")],
                warnings: vec![format!("{line} was planned again")],
                noop: *g == Goes::Later,
                goes: Goes::Done,
            };
            let again: Replan = Box::new(move |_, _| Ok(Box::new(again.clone()) as Box<dyn AnyPlanned>));
            match g {
                Goes::Later => {
                    step.runs = true;
                    step.work.push(Work::Op { plan: None, again });
                }
                _ => {
                    let plan =
                        Fake { summary: vec![line], warnings: warned.into_iter().collect(), noop: false, goes: *g };
                    step.op(Box::new(plan), again);
                }
            }
        }
        assert_eq!(step.warnings, shown, "the step warns what its plans do");
        assert_eq!(step.runs, !ops.is_empty());
        let step = step.unless_runs("nothing to do");
        model.push((*kind, ops, shown));
        steps.push(step);
    }
    let plan = JobPlan::new(Vec::new(), steps, std::iter::empty::<&Device>());
    let running: Vec<StepKind> = plan.running().map(|s| s.step).collect();
    let ran = run_job(&plan, None, &Ctx::silent());
    // What the model says.
    let mut want = Vec::new();
    let mut stopped = None;
    let mut cancelled = false;
    'steps: for (s, (kind, ops, _)) in model.iter().enumerate() {
        if ops.is_empty() {
            continue;
        }
        let (mut done, mut warnings, mut reports) = (Vec::<String>::new(), Vec::<String>::new(), 0);
        for (o, g) in ops.iter().enumerate() {
            let line = format!("op {s}.{o}");
            match g {
                Goes::Done => {
                    done.push(line);
                    reports += 1;
                }
                Goes::Stale => {
                    warnings.push(format!("{line} was planned again"));
                    done.push(format!("{line} again"));
                    reports += 1;
                }
                Goes::Later => warnings.push(format!("{line} was planned again")),
                Goes::Cancelled => {
                    cancelled = true;
                    break 'steps;
                }
                Goes::Fails | Goes::Partly => {
                    reports += usize::from(*g == Goes::Partly);
                    let message = match g {
                        Goes::Fails => Error::invalid("it broke"),
                        _ => Error::Conflict("B is busy".into()),
                    };
                    if !done.is_empty() || reports > 0 {
                        want.push((*kind, done, warnings, reports));
                    }
                    stopped = Some((kind.id().to_string(), message.to_string(), message.code()));
                    break 'steps;
                }
            }
        }
        want.push((*kind, done, warnings, reports));
    }
    match ran {
        Err(e) => assert!(cancelled && matches!(e, Error::Cancelled), "{e}"),
        Ok(report) => {
            assert!(!cancelled, "a cancelled job went on");
            let got: Vec<(StepKind, Vec<String>, Vec<String>, usize)> =
                report.steps.iter().map(|s| (s.step, s.done.clone(), s.warnings.clone(), s.reports.len())).collect();
            assert_eq!(got, want);
            let stop = report.stopped.as_ref().map(|s| (s.step.clone(), s.message.clone(), s.kind));
            assert_eq!(stop, stopped);
            assert_eq!(report.did_anything(), want.iter().any(|w| !w.1.is_empty() || w.3 > 0));
        }
    }
    // The plan's own summary and warnings.
    let planned = plan.planned("job");
    let mut lines = Vec::new();
    for (kind, ops, _) in &model {
        if ops.is_empty() {
            continue;
        }
        let shown: Vec<usize> = (0..ops.len()).filter(|o| ops[*o] != Goes::Later).collect();
        let s = model.iter().position(|m| m.0 == *kind).unwrap();
        match shown.is_empty() {
            true => lines.push(kind.title().to_string()),
            false => lines.extend(shown.iter().map(|o| format!("{}: op {s}.{o}", kind.title()))),
        }
    }
    assert_eq!(planned.summary, lines);
    assert_eq!(planned.warnings, model.iter().flat_map(|m| m.2.clone()).collect::<Vec<_>>());
    assert_eq!(planned.noop, running.is_empty());
}

/// Ejecting drives one at a time says, for each ejected before the first that fails, whether it
/// can be pulled out or only unmounted, with its report; the failure is the result.
#[hegel::test(test_cases = 100)]
fn ejected_drives_are_reported_up_to_the_first_failure(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let outcomes: Vec<Option<bool>> = tc.draw(gs::vecs(gs::optional(gs::booleans())).max_size(4));
    let roots: Vec<PathBuf> = (0..outcomes.len()).map(|i| PathBuf::from(format!("/drives/D{i}"))).collect();
    let mut done = StepReport { step: StepKind::Eject, done: vec![], warnings: vec![], reports: vec![] };
    let result = eject_each(&roots, &mut done, |root| {
        let i: usize = root.to_string_lossy()[9..].parse().unwrap();
        match outcomes[i] {
            Some(powered_off) => {
                Ok(EjectReport { name: format!("D{i}"), root: root.into(), commands: vec![], powered_off })
            }
            None => Err(Error::invalid(format!("D{i} is busy"))),
        }
    });
    let ejected: Vec<String> = outcomes
        .iter()
        .enumerate()
        .map_while(|(i, o)| {
            o.map(|off| match off {
                true => format!("D{i} ejected; it can be pulled out"),
                false => format!("D{i} ejected (unmounted; power stays on)"),
            })
        })
        .collect();
    assert_eq!(done.done, ejected);
    assert_eq!(done.reports.len(), ejected.len());
    match outcomes.iter().position(Option::is_none) {
        Some(i) => assert_eq!(result.unwrap_err().to_string(), Error::invalid(format!("D{i} is busy")).to_string()),
        None => result.unwrap(),
    }
}
