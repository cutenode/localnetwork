//! Jobs: commands that run several operations as one plan, for the moments a DJ has. rekordbox
//! just exported to a drive ([`refresh_plan`]); the night before a set ([`set_job_plan`]); and
//! which drive a command means when it's given none ([`default_drive`], [`drive_group`]).
//!
//! A job's plan ([`JobPlan`]) is a list of steps, each holding the plans of the operations it
//! runs, made by the same `*_plan` functions the single commands use, so one confirmation covers
//! them all. Applying runs the steps in order, each operation through its own apply and so
//! through the shared write routine. An operation whose drive an earlier step changed is planned
//! again on the drive as that step left it; the steps that only make sense after the others (the
//! pull and the sync) are planned as they run. The first step that fails stops the job, and the
//! report says what was done ([`JobReport::stopped`]).

use std::collections::BTreeSet;

use rustc_hash::FxHashMap;
use std::path::{Path, PathBuf};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use super::autopull::{PullNext, new_history, pull_next, save_order};
use super::{
    CheckRequest, Direction, DriveRef, EjectDrives, SetPlayerIdentity, SettingsApplyRequest, StepError, SyncRequest,
    check, clean_plan, drive_eject, edits_replay_plan, known_drives, player_set_plan, registry, resolve_drive,
    settings_apply_plan, smart_apply_plan, sync_plan,
};
use crate::check::{CheckOptions, CheckReport};
use crate::device::Device;
use crate::edit::journal::JournalEntry;
use crate::edit::{self, Edit, EditOptions};
use crate::eject::EjectReport;
use crate::error::{Action, Reason};
use crate::fleet::{self, Role};
use crate::local_time;
use crate::op::{self, AnyPlanned, CloneTarget, Operation, Planned, Touched};
use crate::progress::{Ctx, Steps};
use crate::store::Store;
use crate::sync::{self, PullOptions, PullReport};
use crate::{Error, Result};

// ---------------------------------------------------------------------------------------------
// Steps
// ---------------------------------------------------------------------------------------------

/// A step of a job.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum StepKind {
    /// Remove rekordbox's demo samples (`clean`).
    Clean,
    /// Make the recorded edits again (`edits replay`).
    Replay,
    /// Fill the rule-based playlists (`smart apply`).
    Smart,
    /// Put back the Device Name and colour players show (`drive player`).
    Player,
    /// Write My Settings (`settings apply`).
    Settings,
    /// Save drives in the store (`pull`).
    Pull,
    /// Copy the newest library onto the other drives (`sync`).
    Sync,
    /// Check the drives for the players (`check`).
    Check,
    /// Eject the drives (`eject`).
    Eject,
}

impl StepKind {
    pub const ALL: [StepKind; 9] = [
        StepKind::Clean,
        StepKind::Replay,
        StepKind::Smart,
        StepKind::Player,
        StepKind::Settings,
        StepKind::Pull,
        StepKind::Sync,
        StepKind::Check,
        StepKind::Eject,
    ];

    /// Its name in requests and progress events (`clean`, `replay`...).
    pub fn id(self) -> &'static str {
        match self {
            StepKind::Clean => "clean",
            StepKind::Replay => "replay",
            StepKind::Smart => "smart",
            StepKind::Player => "player",
            StepKind::Settings => "settings",
            StepKind::Pull => "pull",
            StepKind::Sync => "sync",
            StepKind::Check => "check",
            StepKind::Eject => "eject",
        }
    }

    pub fn parse(s: &str) -> Result<StepKind> {
        StepKind::ALL.into_iter().find(|k| k.id().eq_ignore_ascii_case(s.trim())).ok_or_else(|| {
            let ids: Vec<&str> = StepKind::ALL.iter().map(|k| k.id()).collect();
            Error::invalid(format!("unknown step {s:?} ({})", ids.join(", ")))
        })
    }

    /// What it does, as a plan's heading for it.
    pub fn title(self) -> &'static str {
        match self {
            StepKind::Clean => "Remove rekordbox's demo samples",
            StepKind::Replay => "Make the recorded edits again",
            StepKind::Smart => "Fill the rule-based playlists",
            StepKind::Player => "Put back the name and colour players show",
            StepKind::Settings => "Write My Settings",
            StepKind::Pull => "Save the drive in the store",
            StepKind::Sync => "Copy the newest library onto the other drives",
            StepKind::Check => "Check the drives for the players",
            StepKind::Eject => "Eject the drives",
        }
    }
}

/// Plans an operation again, on the drives as they are now.
type Replan = Box<dyn Fn(Option<&Store>, &Ctx<'_>) -> Result<Box<dyn AnyPlanned>> + Send + Sync>;

/// A step's operation, planned to write its own drive only: the job's sync step is what copies
/// onto the library's other clones (see [`op::alone`]).
fn replan<P: AnyPlanned + 'static>(
    f: impl Fn(Option<&Store>, &Ctx<'_>) -> Result<P> + Send + Sync + 'static,
) -> Replan {
    Box::new(move |store, ctx| Ok(Box::new(op::alone(|| f(store, ctx))?) as Box<dyn AnyPlanned>))
}

/// What a step does when it runs.
enum Work {
    /// An operation: its plan (`None`: planned when the step runs), and how to plan it again.
    Op {
        plan: Option<Box<dyn AnyPlanned>>,
        again: Replan,
    },
    /// Save a drive in the store, as `watch --pull` would (see [`pull_next`]).
    Pull {
        root: PathBuf,
        message: String,
    },
    Check {
        roots: Vec<PathBuf>,
        options: CheckOptions,
    },
    Eject {
        roots: Vec<PathBuf>,
    },
}

/// One step of a job's plan.
#[derive(Serialize, JsonSchema)]
pub struct JobStep {
    pub step: StepKind,
    /// What it does, in a few words.
    pub title: String,
    /// Whether it runs when the plan is applied; when it doesn't, `reason` says why.
    pub runs: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// What it will change, in a line each; the pull step also names the drives the store holds
    /// already, which it leaves alone. A step after one that writes to the same drive is planned
    /// on the drive as it is now, and planned again when it runs.
    pub summary: Vec<String>,
    pub warnings: Vec<String>,
    /// It deletes or overwrites something.
    pub destructive: bool,
    /// The plans of the operations it runs, as their `*.plan` methods return them.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub plans: Vec<Value>,
    #[serde(skip)]
    work: Vec<Work>,
}

impl JobStep {
    fn new(step: StepKind) -> JobStep {
        JobStep {
            step,
            title: step.title().to_string(),
            runs: false,
            reason: None,
            summary: Vec::new(),
            warnings: Vec::new(),
            destructive: false,
            plans: Vec::new(),
            work: Vec::new(),
        }
    }

    fn titled(mut self, title: impl Into<String>) -> JobStep {
        self.title = title.into();
        self
    }

    /// It doesn't run, because of `reason`.
    fn idle(mut self, reason: impl Into<String>) -> JobStep {
        self.runs = false;
        self.reason = Some(reason.into());
        self.work.clear();
        self
    }

    /// `reason` why it doesn't run, if nothing made it run.
    fn unless_runs(self, reason: impl Into<String>) -> JobStep {
        if self.runs { self } else { self.idle(reason) }
    }

    /// For the pull step: `held` (the store holds the drives' states) as why it doesn't run,
    /// unless a drive won't be saved, which a warning says.
    fn unless_pulls(self, held: &str) -> JobStep {
        let reason = if self.warnings.is_empty() { held } else { "not saved: see the warning" };
        self.unless_runs(reason)
    }

    fn warn(&mut self, w: impl Into<String>) {
        let w = w.into();
        if !self.warnings.contains(&w) {
            self.warnings.push(w);
        }
    }

    /// Run `plan` (as `again` makes it) in this step, unless it changes nothing.
    fn op(&mut self, plan: Box<dyn AnyPlanned>, again: Replan) {
        for w in plan.warnings() {
            self.warn(w.clone());
        }
        if plan.is_noop() {
            return;
        }
        self.runs = true;
        self.summary.extend(plan.summary().iter().cloned());
        self.destructive |= plan.destructive();
        self.plans.push(plan.to_json());
        self.work.push(Work::Op { plan: Some(plan), again });
    }
}

/// A drive a job is about.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct JobDrive {
    pub name: String,
    pub root: PathBuf,
}

impl JobDrive {
    fn of(d: &Device) -> JobDrive {
        JobDrive { name: d.name(), root: d.root().to_path_buf() }
    }
}

/// A job's plan: its steps, in the order they run (see the module docs).
#[derive(Serialize, JsonSchema)]
pub struct JobPlan {
    /// The drives it's about.
    pub targets: Vec<JobDrive>,
    /// Every step, including the ones that won't run (`runs: false`, with the reason).
    pub steps: Vec<JobStep>,
    /// Where each drive it touches was mounted, and its identity (drives that had one).
    #[serde(skip)]
    identities: Vec<(PathBuf, Uuid)>,
}

impl JobPlan {
    fn new<'a>(targets: Vec<JobDrive>, steps: Vec<JobStep>, touched: impl IntoIterator<Item = &'a Device>) -> JobPlan {
        let identities = touched.into_iter().filter_map(|d| Some((d.root().to_path_buf(), d.id()?))).collect();
        JobPlan { targets, steps, identities }
    }

    /// Refuse to go on once another drive is mounted where one the job is about was: its steps
    /// are planned again by mount path as they run.
    fn same_drives(&self) -> Result<()> {
        for (root, id) in &self.identities {
            match Device::open(root) {
                Ok(d) if d.id() == Some(*id) => {}
                Ok(_) => return Err(Error::stale("another drive", &format!("is at {}", root.display()))),
                Err(_) => return Err(Error::stale(root.display(), "is gone")),
            }
        }
        Ok(())
    }

    /// The steps that run.
    pub fn running(&self) -> impl Iterator<Item = &JobStep> {
        self.steps.iter().filter(|s| s.runs)
    }

    /// As a [`Planned`]: the steps' summaries and warnings together.
    fn planned(self, operation: &'static str) -> Planned<JobPlan> {
        let mut summary = Vec::new();
        let mut warnings = Vec::new();
        for s in &self.steps {
            warnings.extend(s.warnings.iter().cloned());
            if !s.runs {
                continue;
            }
            match s.summary.is_empty() {
                true => summary.push(s.title.clone()),
                false => summary.extend(s.summary.iter().map(|l| format!("{}: {l}", s.title))),
            }
        }
        let destructive = self.running().any(|s| s.destructive);
        let noop = self.running().next().is_none();
        Planned::new(operation, self, ()).summary(summary).warnings(warnings).destructive(destructive).noop(noop)
    }
}

/// What one step did.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct StepReport {
    pub step: StepKind,
    /// What it did, in a line each.
    pub done: Vec<String>,
    /// Warnings that came up as it ran (the plan's own aside).
    pub warnings: Vec<String>,
    /// Each operation's report, as its `*.apply` method returns it.
    pub reports: Vec<Value>,
}

/// What applying a job's plan did.
#[derive(Debug, Clone, Default, Serialize, JsonSchema)]
pub struct JobReport {
    /// The steps that ran, in order (a step that failed part of the way is here with what it did).
    pub steps: Vec<StepReport>,
    /// The step that failed, which stopped the job. The steps before it stay done.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stopped: Option<StepError>,
    /// The check found problems, so the job stopped before ejecting the drives.
    pub problems: bool,
    /// The check's report, when it ran.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub check: Option<CheckReport>,
}

impl JobReport {
    /// Whether any step changed something (a job that failed at its first step changed nothing).
    pub fn did_anything(&self) -> bool {
        self.steps.iter().any(|s| !s.done.is_empty() || !s.reports.is_empty())
    }
}

impl Operation for JobPlan {
    type Request = ();
    type Report = JobReport;

    fn apply(planned: &Planned<Self>, store: Option<&Store>, ctx: &Ctx<'_>) -> Result<JobReport> {
        run_job(&planned.plan, store, ctx)
    }
}

/// Run the steps that run, in order, stopping at the first that fails (or at a check that finds
/// problems). Progress events carry the step (see [`Steps`]).
fn run_job(plan: &JobPlan, store: Option<&Store>, ctx: &Ctx<'_>) -> Result<JobReport> {
    let steps = Steps::new(ctx, plan.running().map(|s| s.step.id()).collect());
    let ctx = &steps.ctx();
    let mut report = JobReport::default();
    for step in plan.running() {
        steps.begin(step.step.id());
        let mut done = StepReport { step: step.step, done: Vec::new(), warnings: Vec::new(), reports: Vec::new() };
        let mut outcome = Ok(true);
        for w in &step.work {
            outcome = plan.same_drives().and_then(|()| run_work(w, step, store, &mut done, &mut report, ctx));
            if !matches!(outcome, Ok(true)) {
                break;
            }
        }
        let did = !done.done.is_empty() || !done.reports.is_empty();
        match outcome {
            Ok(go_on) => {
                report.steps.push(done);
                if !go_on {
                    break;
                }
            }
            Err(Error::Cancelled) => return Err(Error::Cancelled),
            Err(e) => {
                if did {
                    report.steps.push(done);
                }
                report.stopped =
                    Some(StepError { step: step.step.id().into(), kind: e.code(), message: e.to_string() });
                break;
            }
        }
    }
    Ok(report)
}

/// Run one piece of a step. `Ok(false)` stops the job there (the check found problems).
fn run_work(
    w: &Work,
    step: &JobStep,
    store: Option<&Store>,
    done: &mut StepReport,
    report: &mut JobReport,
    ctx: &Ctx<'_>,
) -> Result<bool> {
    match w {
        Work::Op { plan: Some(plan), again } => match apply_op(plan.as_ref(), step, store, done, ctx) {
            // An earlier step changed the drive: the same operation, on the drive as it is now.
            Err(Error::StalePlan(_)) => apply_op(again(store, ctx)?.as_ref(), step, store, done, ctx)?,
            r => r?,
        },
        Work::Op { plan: None, again } => apply_op(again(store, ctx)?.as_ref(), step, store, done, ctx)?,
        Work::Pull { root, message } => {
            let store = store.ok_or_else(|| Error::invalid("saving a drive needs the store"))?;
            let mut d = Device::open(root)?;
            let pulled = pull_one(store, &mut d, message, ctx)?;
            done.done.extend(pulled.line);
            done.warnings.extend(pulled.warning);
            if let Some(r) = pulled.report {
                done.reports.push(serde_json::to_value(r).map_err(|e| Error::invalid(e.to_string()))?);
            }
        }
        Work::Check { roots, options } => {
            let drives = roots.iter().map(|r| r.display().to_string()).collect();
            let mut r = check(store, &CheckRequest { drives, options: options.clone() }, ctx)?;
            // The drives were to be saved first: history lists still not in the store (a pull
            // left out) would be lost to rekordbox's next import, so they keep the drives here.
            r.escalate("history");
            done.done.push(match (r.problems, r.warnings) {
                (0, 0) => "ready".to_string(),
                (0, w) => format!("ready, with {w} warning(s)"),
                (p, w) => format!("{p} problem(s) and {w} warning(s)"),
            });
            let ok = r.ok();
            report.problems = !ok;
            report.check = Some(r);
            return Ok(ok);
        }
        Work::Eject { roots } => eject_each(roots, done, |root| {
            let mut ejected = drive_eject(store, &EjectDrives { drives: vec![root.display().to_string()] })?;
            Ok(ejected.remove(0))
        })?,
    }
    Ok(true)
}

/// Eject the drives one at a time, noting each as it goes, so a failure part way still reports
/// the ones already ejected.
fn eject_each(
    roots: &[PathBuf],
    done: &mut StepReport,
    mut eject: impl FnMut(&Path) -> Result<EjectReport>,
) -> Result<()> {
    for root in roots {
        let e = eject(root)?;
        done.done.push(match e.powered_off {
            true => format!("{} ejected; it can be pulled out", e.name),
            false => format!("{} ejected (unmounted; power stays on)", e.name),
        });
        done.reports.push(serde_json::to_value(e).map_err(|e| Error::invalid(e.to_string()))?);
    }
    Ok(())
}

/// Apply one operation's plan, unless it changes nothing (any more).
fn apply_op(
    plan: &dyn AnyPlanned,
    step: &JobStep,
    store: Option<&Store>,
    done: &mut StepReport,
    ctx: &Ctx<'_>,
) -> Result<()> {
    // Planned again: what's new since the plan that was shown.
    for w in plan.warnings() {
        if !step.warnings.contains(w) && !done.warnings.contains(w) {
            done.warnings.push(w.clone());
        }
    }
    if plan.is_noop() {
        return Ok(());
    }
    let value = plan.apply_json(store, ctx)?;
    // Reports of several drives say which failed, rather than failing.
    let failed = value.as_array().into_iter().flatten().find_map(|r| {
        let e = r.get("error").filter(|e| !e.is_null())?;
        Some(e.get("message").and_then(Value::as_str).unwrap_or("failed").to_string())
    });
    done.reports.push(value);
    if let Some(message) = failed {
        return Err(Error::Conflict(message));
    }
    done.done.extend(plan.summary().iter().cloned());
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// Saving drives in the store
// ---------------------------------------------------------------------------------------------

/// How a drive would be saved in the store.
enum Decision {
    /// In no library the store has: a pull starts one, named after the drive.
    Start,
    /// Left alone (retired), and why.
    Skip(String),
    /// In this library, as [`pull_next`] decides.
    In(String, PullNext),
}

fn decide(store: &Store, d: &Device, ctx: &Ctx<'_>) -> Result<Decision> {
    if let Some(id) = d.id()
        && store.drives()?.get(&id).is_some_and(|r| r.log.retired.is_some())
    {
        return Ok(Decision::Skip("it's retired".into()));
    }
    let lib = d.state().and_then(|s| s.library_id).map(|id| store.library_by_id(id)).transpose()?.flatten();
    let Some(lib) = lib else { return Ok(Decision::Start) };
    Ok(Decision::In(lib.name.clone(), pull_next(store, &lib, d, ctx)?))
}

fn history_note(lists: &[String]) -> String {
    match lists.len() {
        0 => String::new(),
        1..=3 => format!(" (history lists the store lacks: {})", lists.join(", ")),
        n => format!(" ({n} history lists the store lacks: {}...)", lists[..3].join(", ")),
    }
}

struct Pulled {
    line: Option<String>,
    warning: Option<String>,
    report: Option<PullReport>,
}

/// Save `d` in the store as `watch --pull` would: on top of its library's newest snapshot,
/// beside it (back from a set with only the players' changes), or not at all (saved already, or
/// left alone, which comes back as a warning). A drive in no library of the store starts one.
fn pull_one(store: &Store, d: &mut Device, message: &str, ctx: &Ctx<'_>) -> Result<Pulled> {
    let name = d.name();
    let opts = PullOptions { library: None, message: Some(format!("{name}: {message}")), force: false };
    let short = |r: &PullReport| r.snapshot.as_ref().map(|s| format!(", snapshot {}", s.short_id)).unwrap_or_default();
    let (line, report) = match decide(store, d, ctx)? {
        Decision::Skip(why) | Decision::In(_, PullNext::Skip(why)) => {
            return Ok(Pulled { line: None, warning: Some(format!("{name} wasn't saved: {why}")), report: None });
        }
        Decision::In(library, PullNext::Saved { .. }) => (format!("the store holds {name} already ({library})"), None),
        Decision::Start => {
            let r = sync::pull(store, d, &opts, ctx)?;
            (format!("saved {name} in a new library, {}{}", r.library, short(&r)), Some(r))
        }
        Decision::In(_, PullNext::Pull { ancestor }) => {
            let lists = new_history(store, d, ancestor.as_deref())?;
            let r = sync::pull(store, d, &opts, ctx)?;
            (format!("saved {name} in {}{}{}", r.library, short(&r), history_note(&lists)), Some(r))
        }
        Decision::In(library, PullNext::Beside { shared }) => {
            let lists = new_history(store, d, Some(&*shared))?;
            let r = sync::pull_beside(store, d, &opts, shared.id, ctx)?;
            let line = format!("saved {name} beside {library}'s newest snapshot{}{}", short(&r), history_note(&lists));
            (line, Some(r))
        }
    };
    Ok(Pulled { line: Some(line), warning: None, report })
}

/// The pull step over `drives`, in the order [`save_order`] gives (the newest export first,
/// drives back from a set beside it last), and the drives that go first (rank 0).
fn pull_step(store: &Store, drives: &[Device], message: &str, ctx: &Ctx<'_>) -> Result<(JobStep, Vec<usize>)> {
    let mut step = JobStep::new(StepKind::Pull);
    let decisions: Vec<Decision> = drives.iter().map(|d| decide(store, d, ctx)).collect::<Result<_>>()?;
    let mut pending = Vec::new();
    let mut held = Vec::new();
    for (i, (d, decision)) in drives.iter().zip(&decisions).enumerate() {
        match decision {
            Decision::Skip(why) | Decision::In(_, PullNext::Skip(why)) => {
                step.warn(format!("{} won't be saved: {why}", d.name()))
            }
            Decision::In(library, PullNext::Saved { .. }) => {
                held.push(format!("the store holds {} already ({library})", d.name()))
            }
            Decision::Start => pending.push((i, d, None)),
            Decision::In(_, next) => pending.push((i, d, Some(next))),
        }
    }
    let order = save_order(store, pending, ctx)?;
    for &(_, i) in &order {
        let (d, name) = (&drives[i], drives[i].name());
        step.summary.push(match &decisions[i] {
            Decision::In(library, PullNext::Pull { ancestor }) => {
                format!("save {name} in {library}{}", history_note(&new_history(store, d, ancestor.as_deref())?))
            }
            Decision::In(library, PullNext::Beside { shared }) => format!(
                "save {name} beside {library}'s newest snapshot: it's back from a set{}",
                history_note(&new_history(store, d, Some(shared))?)
            ),
            // A drive starting a library (`save_order` leaves out the ones not saved).
            _ => format!("save {name} in the store, starting a library for it"),
        });
        step.work.push(Work::Pull { root: d.root().to_path_buf(), message: message.to_string() });
    }
    step.summary.extend(held);
    step.runs = !step.work.is_empty();
    let newest_first = order.iter().filter(|o| o.0 == 0).map(|o| o.1).collect();
    Ok((step, newest_first))
}

/// A sync from `source` onto `target`, planned as the step runs: refused if `target` looks
/// newer by then.
fn sync_again(source: PathBuf, target: PathBuf) -> Replan {
    replan(move |_, ctx| {
        let p = sync_plan(&pair(&source, &target), ctx)?;
        if p.source.as_deref().is_some_and(|s| s != source) {
            return Err(Error::Conflict(format!(
                "{} looks newer than {} now; plan again",
                target.display(),
                source.display()
            )));
        }
        Ok(p)
    })
}

fn pair(source: &Path, target: &Path) -> SyncRequest {
    SyncRequest {
        left: source.display().to_string(),
        right: target.display().to_string(),
        direction: Direction::Auto,
        allow_low_confidence: false,
        verify: false,
        keep_extra: false,
        delete_first: false,
        overwrite_archive: false,
        keep_player_changes: false,
    }
}

/// Why a drive can't be copied onto the others: the reason, and a warning to show.
struct Blocked {
    reason: String,
    warning: String,
}

/// The sync step: `source` onto each of `others`, when `source` is the newest of them. `after`
/// says an earlier step changes `source`, so even a target that matches it now gets a copy.
fn sync_step(
    source: &Device,
    others: &[&Device],
    after: bool,
    ctx: &Ctx<'_>,
) -> Result<std::result::Result<JobStep, Blocked>> {
    let mut step = JobStep::new(StepKind::Sync);
    let name = source.name();
    let mut matching = Vec::new();
    for o in others {
        let p = match sync_plan(&pair(source.root(), o.root()), ctx) {
            Ok(p) => p,
            Err(Error::Cancelled) => return Err(Error::Cancelled),
            Err(e) => {
                return Ok(Err(Blocked {
                    reason: format!("{name} and {} can't be synced", o.name()),
                    warning: format!("not copying {name} onto {}: {e}", o.name()),
                }));
            }
        };
        if p.source.as_deref().is_some_and(|s| s != source.root()) {
            return Ok(Err(Blocked {
                reason: format!("{} is newer than {name}", o.name()),
                warning: format!("{} is newer than {name}: it isn't copied over", o.name()),
            }));
        }
        if !p.noop {
            step.summary.extend(p.summary.iter().cloned());
            step.destructive |= p.destructive;
        } else if after {
            step.summary.push(format!("copy {name}'s changes onto {}", o.name()));
        } else {
            matching.push(o.name());
            continue;
        }
        for w in &p.warnings {
            step.warn(w.clone());
        }
        step.plans.push(p.to_json());
        step.work.push(Work::Op { plan: None, again: sync_again(source.root().into(), o.root().into()) });
    }
    if !matching.is_empty() && !step.work.is_empty() {
        let verb = if matching.len() == 1 { "matches" } else { "match" };
        step.summary.push(format!("{} already {verb} {name}", matching.join(", ")));
    }
    step.runs = !step.work.is_empty();
    Ok(Ok(step.unless_runs("the drives hold the same library already")))
}

// ---------------------------------------------------------------------------------------------
// refresh
// ---------------------------------------------------------------------------------------------

/// After rekordbox exported to a drive: put back what the export replaced, then save it.
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct RefreshRequest {
    /// The drive rekordbox exported to [default: see [`refresh_drive`]].
    #[serde(default)]
    pub drive: Option<String>,
    /// Steps to leave out (`clean`, `replay`, `smart`, `player`, `settings`, `pull`, `sync`).
    #[serde(default)]
    pub skip: Vec<StepKind>,
    /// Then copy the drive onto the other mounted drives of its library, when it's the newest.
    #[serde(default)]
    pub sync: bool,
}

/// What refreshing a drive after a rekordbox export would do, step by step, each only when
/// needed: remove rekordbox's demo samples, make the recorded edits again (playlists, tracks,
/// My Tags, cues), fill the rule-based playlists, put back the name and colour players show
/// (as `drive player` last set them), write My Settings, save the drive in the store, and (with
/// `sync`) copy it onto the other mounted drives of its library.
pub fn refresh_plan(store: &Store, req: &RefreshRequest, ctx: &Ctx<'_>) -> Result<Planned<JobPlan>> {
    let d = match &req.drive {
        Some(spec) => resolve_drive(spec)?,
        None => refresh_drive(Some(store))?,
    };
    if !d.has_export() {
        return Err(Error::NotAnExport(d.root().to_path_buf()));
    }
    let spec = d.root().display().to_string();
    let name = d.name();
    let skipped = |k: StepKind| req.skip.contains(&k);
    let drive_ref = {
        let spec = spec.clone();
        move || DriveRef { drive: spec.clone() }
    };
    let mut steps = Vec::new();

    // rekordbox's demo samples, which come back with every export.
    let mut s = JobStep::new(StepKind::Clean);
    if !skipped(StepKind::Clean) {
        let dr = drive_ref.clone();
        let again = replan(move |_, ctx| clean_plan(&dr(), ctx));
        s.op(again(Some(store), ctx)?, again);
    }
    steps.push(s.unless_runs(if skipped(StepKind::Clean) { "skipped" } else { "no demo samples on the drive" }));

    // The edits the export replaced.
    let mut s = JobStep::new(StepKind::Replay);
    let journal = edit::journal::list(&d);
    let s = match journal {
        _ if skipped(StepKind::Replay) => s.idle("skipped"),
        Err(e) => {
            s.warn(format!("the recorded edits can't be read: {e}"));
            s.idle("the recorded edits can't be read")
        }
        Ok(entries) if entries.is_empty() => s.idle("no recorded edits"),
        Ok(entries) if !edits_lost(&d, &entries, ctx) => s.idle("the recorded edits are in place"),
        Ok(_) => {
            let dr = drive_ref.clone();
            let again = replan(move |_, ctx| edits_replay_plan(&dr(), ctx));
            s.op(again(Some(store), ctx)?, again);
            s.unless_runs("the recorded edits are in place")
        }
    };
    steps.push(s);

    // Rule-based playlists, which the export leaves out.
    let mut s = JobStep::new(StepKind::Smart);
    let s = if skipped(StepKind::Smart) {
        s.idle("skipped")
    } else {
        let dr = drive_ref.clone();
        let again = replan(move |store, ctx| {
            let store = store.ok_or_else(|| Error::invalid("rule-based playlists need the store"))?;
            smart_apply_plan(store, &dr(), ctx)
        });
        match again(Some(store), ctx) {
            Err(Error::NotFound(_)) => s.idle("no rule-based playlists for this drive"),
            Err(e) => return Err(e),
            Ok(plan) => {
                s.op(plan, again);
                s.unless_runs("the rule-based playlists are up to date")
            }
        }
    };
    steps.push(s);

    // The name and colour players show, which the export sets to rekordbox's.
    let s = JobStep::new(StepKind::Player);
    let s = match d.state().and_then(|st| st.player.clone()) {
        _ if skipped(StepKind::Player) => s.idle("skipped"),
        None => s.idle("no name or colour remembered for it (setting the name players show remembers them)"),
        Some(_) if !d.has_library_plus() => s.idle("it has no OneLibrary database, where players read the name"),
        Some(want) if crate::player::db_identity(d.root()).as_ref() == Some(&want) => {
            s.idle(format!("players show {} already", want.name.as_deref().unwrap_or("its name")))
        }
        Some(want) => {
            let spec = spec.clone();
            let again = replan(move |_, _| {
                player_set_plan(&SetPlayerIdentity {
                    drive: spec.clone(),
                    name: want.name.clone(),
                    colour: want.colour,
                })
            });
            let mut s = s;
            s.op(again(Some(store), ctx)?, again);
            s.unless_runs("players show its name already")
        }
    };
    steps.push(s);

    // My Settings.
    let mut s = JobStep::new(StepKind::Settings);
    let profile = store.settings_profile()?;
    let s = if skipped(StepKind::Settings) {
        s.idle("skipped")
    } else if profile.base.is_empty() && profile.drives.is_empty() {
        s.idle("the My Settings profile is empty (`settings import` starts one)")
    } else {
        let spec = spec.clone();
        let again = replan(move |store, _| {
            let store = store.ok_or_else(|| Error::invalid("writing My Settings needs the store's profile"))?;
            settings_apply_plan(store, &SettingsApplyRequest { drives: vec![spec.clone()], create_missing: false })
        });
        s.op(again(Some(store), ctx)?, again);
        s.unless_runs("its settings are as the profile has them")
    };
    steps.push(s);

    // Save it.
    let writes = steps.iter().any(|s| s.runs);
    let s = if skipped(StepKind::Pull) {
        JobStep::new(StepKind::Pull).idle("skipped")
    } else {
        let (mut s, _) = pull_step(store, std::slice::from_ref(&d), "refreshed after a rekordbox export", ctx)?;
        if !s.runs && writes && s.warnings.is_empty() {
            s.summary = vec![format!("save {name} in the store after the steps above")];
            s.work.push(Work::Pull { root: d.root().into(), message: "refreshed after a rekordbox export".into() });
            s.runs = true;
        }
        s.unless_pulls("the store holds it already")
    };
    let pulls = s.runs;
    steps.push(s);

    // Its library's other drives.
    let mut others = Vec::new();
    if req.sync {
        let s = if skipped(StepKind::Sync) {
            JobStep::new(StepKind::Sync).idle("skipped")
        } else {
            let lib = d.state().and_then(|s| s.library_id);
            let mounted = known_drives(false);
            let set = fleet::sets(store, &mounted)?.into_iter().find(|s| lib.is_some() && s.library_id == lib);
            others = set
                .map(|s| s.members)
                .unwrap_or_default()
                .into_iter()
                .filter(|m| m != d.root())
                .filter_map(|m| Device::open(&m).ok())
                .collect::<Vec<_>>();
            let refs: Vec<&Device> = others.iter().collect();
            match refs.is_empty() {
                true => JobStep::new(StepKind::Sync).idle("no other drive of its library is mounted"),
                false => match sync_step(&d, &refs, writes || pulls, ctx)? {
                    Ok(step) => step,
                    Err(blocked) => {
                        let mut step = JobStep::new(StepKind::Sync);
                        step.warn(blocked.warning);
                        step.idle(blocked.reason)
                    }
                },
            }
        };
        if !s.runs {
            others.clear();
        }
        steps.push(s);
    }

    let mut planned =
        JobPlan::new(vec![JobDrive::of(&d)], steps, std::iter::once(&d).chain(&others)).planned("refresh");
    planned = planned.touches(Touched::library(&d, true));
    for o in &others {
        planned = planned.touches(Touched::library(o, true));
    }
    planned.guard(Some(store))
}

/// Plan and refresh a drive in one go, for callers that don't confirm (see [`refresh_plan`]).
pub fn refresh_apply(store: &Store, req: &RefreshRequest, ctx: &Ctx<'_>) -> Result<JobReport> {
    refresh_plan(store, req, ctx)?.apply(Some(store), ctx)
}

/// Whether making the recorded edits again would change something. My Tag and cue edits can't
/// be told apart from their effect without making them, so they count once rekordbox exported
/// to the drive after them.
fn edits_lost(d: &Device, entries: &[JournalEntry], ctx: &Ctx<'_>) -> bool {
    let (tags_and_cues, rest): (Vec<Edit>, Vec<Edit>) = entries
        .iter()
        .flat_map(|e| e.edits.iter().cloned())
        .partition(|e| matches!(e, Edit::MyTag(_) | Edit::Cues { .. }));
    if !tags_and_cues.is_empty() && exported_since(d, entries) {
        return true;
    }
    !rest.is_empty()
        && edit::plan(d, &rest, &EditOptions { lenient: true, no_journal: true }, ctx).is_ok_and(|p| !p.is_noop())
}

/// Whether rekordbox exported to the drive after its last recorded edit (see
/// [`Device::exported_at`]; allowing for FAT's 2-second timestamps).
fn exported_since(d: &Device, entries: &[JournalEntry]) -> bool {
    let Some(last) = entries.iter().map(|e| e.at).max() else { return false };
    d.exported_at().is_some_and(|at| at > last + chrono::Duration::seconds(2))
}

/// How many recorded edits a rekordbox export replaced: the entries in the drive's journal, when
/// rekordbox exported after the last of them and making them again would change something; 0
/// otherwise. `status` flags such a drive; `refresh` makes the edits again.
pub fn exported_over_edits(d: &Device) -> usize {
    let Ok(entries) = edit::journal::list(d) else { return 0 };
    match exported_since(d, &entries) && edits_lost(d, &entries, &Ctx::silent()) {
        true => entries.len(),
        false => 0,
    }
}

// ---------------------------------------------------------------------------------------------
// set: the night before a set
// ---------------------------------------------------------------------------------------------

fn yes() -> bool {
    true
}

/// Getting drives ready for a set.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct SetJobRequest {
    /// The drives you're taking, or a library's name for its mounted clones [default: the mounted
    /// drives of one library, see [`drive_group`]].
    #[serde(default)]
    pub drives: Vec<String>,
    /// The library whose mounted drives to take, when `drives` is empty.
    #[serde(default)]
    pub library: Option<String>,
    /// Player models to check against [default: the ones remembered, see `players set`].
    #[serde(default)]
    pub players: Vec<String>,
    /// Also decode the end of every audio file (slow).
    #[serde(default)]
    pub audio: bool,
    /// Eject the drives once everything passed [default: yes].
    #[serde(default = "yes")]
    pub eject: bool,
}

impl Default for SetJobRequest {
    fn default() -> Self {
        SetJobRequest { drives: Vec::new(), library: None, players: Vec::new(), audio: false, eject: true }
    }
}

/// What getting drives ready for a set would do: save them in the store (history lists first,
/// so none is lost to the sync), copy the newest library onto the others, check them against the
/// player models, and eject them once everything passed.
pub fn set_job_plan(store: &Store, req: &SetJobRequest, ctx: &Ctx<'_>) -> Result<Planned<JobPlan>> {
    let devices: Vec<Device> = match req.drives.is_empty() {
        true => {
            drive_group(Some(store), req.library.as_deref())?.drives.iter().map(Device::open).collect::<Result<_>>()?
        }
        false => {
            let mut out: Vec<Device> = Vec::new();
            for spec in &super::expand_libraries(&req.drives)?.0 {
                let d = resolve_drive(spec)?;
                if out.iter().any(|x| x.root() == d.root()) {
                    return Err(Error::invalid(format!("{spec:?} is listed twice")));
                }
                out.push(d);
            }
            out
        }
    };
    if let Some(d) = devices.iter().find(|d| !d.has_export()) {
        return Err(Error::NotAnExport(d.root().to_path_buf()));
    }
    let names: Vec<String> = devices.iter().map(Device::name).collect();
    // As `players set` takes them: a model it doesn't know would go unchecked, and still eject.
    let players = crate::equipment::canonical(&req.players)?;
    let mut steps = Vec::new();

    // The store first: history lists players recorded, before a sync replaces them.
    let (pull, newest) = pull_step(store, &devices, "before a set", ctx)?;
    let title = match devices.len() {
        1 => "Save the drive in the store",
        _ => "Save the drives in the store, history lists first",
    };
    steps.push(pull.titled(title).unless_pulls("the store holds every drive's state already"));

    // The newest library onto the others.
    let mut targets = BTreeSet::new();
    let sync = if devices.len() < 2 {
        JobStep::new(StepKind::Sync).idle("one drive: nothing to copy")
    } else {
        // The drive with the newest export is the likeliest source; then the order given (the
        // primary first, for a drive group).
        let mut order: Vec<usize> = newest.clone();
        order.extend((0..devices.len()).filter(|i| !newest.contains(i)));
        let mut chosen = None;
        let mut blocked = Vec::new();
        for &c in &order {
            let others: Vec<&Device> = devices.iter().enumerate().filter(|(i, _)| *i != c).map(|(_, d)| d).collect();
            match sync_step(&devices[c], &others, false, ctx)? {
                Ok(step) => {
                    chosen = Some((c, step));
                    break;
                }
                Err(b) => blocked.push(b.warning),
            }
        }
        match chosen {
            Some((c, mut step)) => {
                if step.runs {
                    step.summary.insert(0, format!("{} holds the newest library", devices[c].name()));
                    targets.extend((0..devices.len()).filter(|i| *i != c));
                }
                step
            }
            None => {
                let mut step = JobStep::new(StepKind::Sync);
                step.warn(format!(
                    "can't tell which drive holds the newest library: {}",
                    blocked.first().map(String::as_str).unwrap_or("")
                ));
                step.idle("no drive is newer than all the others")
            }
        }
    };
    steps.push(sync);

    // The players.
    let mut check_step = JobStep::new(StepKind::Check);
    let models = match players.is_empty() {
        false => players.clone(),
        true => {
            let r = store.player_models()?;
            let lib = devices.iter().find_map(|d| d.state().and_then(|s| s.library_id));
            lib.and_then(|id| r.libraries.get(&id).cloned()).filter(|m| !m.is_empty()).unwrap_or(r.default)
        }
    };
    check_step.summary.push(match models.is_empty() {
        true => format!("check {} (no player models remembered: `players set` adds them)", names.join(", ")),
        false => format!("check {} for the {}", names.join(", "), models.join(", ")),
    });
    check_step.runs = true;
    let roots: Vec<PathBuf> = devices.iter().map(|d| d.root().to_path_buf()).collect();
    check_step.work.push(Work::Check { roots: roots.clone(), options: CheckOptions { players, audio: req.audio } });
    steps.push(check_step);

    // Out of the computer.
    let mut eject = JobStep::new(StepKind::Eject);
    let eject = match req.eject {
        false => eject.idle("kept mounted, as asked"),
        true => {
            // Each with the other volumes of its disk, which go too. A drive that can't be ejected
            // fails when it's tried, after the check.
            let also = |root: &PathBuf| {
                super::tui::eject_target(&root.display().to_string(), Some(store.root())).map(|(_, t)| t.also())
            };
            let named: Vec<String> = names
                .iter()
                .zip(&roots)
                .map(|(name, root)| format!("{name}{}", also(root).unwrap_or_default()))
                .collect();
            eject.summary.push(format!("eject {} once the check passes", named.join(", ")));
            if let Some(w) = crate::format::untested_here("ejecting") {
                eject.warn(w);
            }
            eject.runs = true;
            eject.work.push(Work::Eject { roots });
            eject
        }
    };
    steps.push(eject);

    let targets_list = devices.iter().map(JobDrive::of).collect();
    let mut planned = JobPlan::new(targets_list, steps, &devices).planned("set");
    for (i, d) in devices.iter().enumerate() {
        planned = planned.touches(Touched::library(d, targets.contains(&i)));
    }
    Ok(planned)
}

/// Plan and get the drives ready in one go, for callers that don't confirm (see
/// [`set_job_plan`]).
pub fn set_job_apply(store: &Store, req: &SetJobRequest, ctx: &Ctx<'_>) -> Result<JobReport> {
    set_job_plan(store, req, ctx)?.apply(Some(store), ctx)
}

/// Saving the mounted drives of one library in the store (`pull` with no drive).
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct PullGroupRequest {
    /// The library whose drives to save [default: see [`drive_group`]].
    #[serde(default)]
    pub library: Option<String>,
    #[serde(default)]
    pub message: Option<String>,
}

/// Save the mounted drives of one library in the store: the newest export first, then drives
/// back from a set beside it, as `watch --pull` does (see [`drive_group`]).
pub fn pull_group(store: &Store, req: &PullGroupRequest, ctx: &Ctx<'_>) -> Result<JobReport> {
    let group = drive_group(Some(store), req.library.as_deref())?;
    let devices: Vec<Device> = group.drives.iter().map(Device::open).collect::<Result<_>>()?;
    let message = req.message.clone().unwrap_or_else(|| "pulled with its drive group".into());
    let (step, _) = pull_step(store, &devices, &message, ctx)?;
    let mut report = JobReport::default();
    let warnings = step.warnings.clone();
    if step.runs {
        report =
            run_job(&JobPlan::new(devices.iter().map(JobDrive::of).collect(), vec![step], &devices), Some(store), ctx)?;
    } else {
        let done = step.summary.clone();
        report.steps.push(StepReport { step: StepKind::Pull, done, warnings: Vec::new(), reports: Vec::new() });
    }
    if let Some(s) = report.steps.first_mut() {
        s.warnings.splice(0..0, warnings);
    }
    Ok(report)
}

// ---------------------------------------------------------------------------------------------
// Which drive
// ---------------------------------------------------------------------------------------------

/// A mounted rekordbox drive a command could mean.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct DriveCandidate {
    pub name: String,
    pub root: PathBuf,
    pub role: Option<Role>,
    pub library: Option<String>,
}

fn mounted_exports() -> Vec<Device> {
    known_drives(false).into_iter().filter(Device::has_export).collect()
}

fn candidates_of(store: Option<&Store>, devices: &[Device]) -> Vec<DriveCandidate> {
    let registry = store.and_then(|s| s.drives().ok()).unwrap_or_default();
    let libraries: FxHashMap<Uuid, String> =
        store.and_then(|s| s.libraries().ok()).unwrap_or_default().into_iter().map(|l| (l.id, l.name)).collect();
    devices
        .iter()
        .map(|d| DriveCandidate {
            name: d.name(),
            root: d.root().to_path_buf(),
            role: d.id().and_then(|id| registry.get(&id)).and_then(|r| r.role),
            library: d.state().and_then(|s| s.library_id).and_then(|id| libraries.get(&id).cloned()),
        })
        .collect()
}

/// Every mounted rekordbox drive, with its role and library.
pub fn drive_candidates(store: Option<&Store>) -> Vec<DriveCandidate> {
    candidates_of(store, &mounted_exports())
}

/// The candidates as an aligned table, a line each: name, mount path, role, library.
fn table(candidates: &[DriveCandidate]) -> String {
    let width = |f: &dyn Fn(&DriveCandidate) -> usize| candidates.iter().map(f).max().unwrap_or(0);
    let (n, p) = (width(&|c| c.name.chars().count()), width(&|c| c.root.display().to_string().chars().count()));
    let r = width(&|c| c.role.map_or(1, |r| r.name().len()));
    candidates
        .iter()
        .map(|c| {
            let root = c.root.display().to_string();
            let role = c.role.map_or("-", Role::name);
            let library = c.library.as_deref().unwrap_or("(no library)");
            format!("  {:<n$}  {root:<p$}  {role:<r$}  {library}", c.name)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The drive a command means when it's given none: the only mounted rekordbox drive; otherwise,
/// when the mounted drives that belong to a library all belong to one, that library's lead clone
/// (see [`lead_clone`]). Anything else is refused, listing the candidates (name, mount path, role,
/// library). `purpose` finishes "say which library to ...".
pub fn default_drive(store: Option<&Store>, purpose: &str) -> Result<Device> {
    let mut mounted = mounted_exports();
    match mounted.len() {
        0 => return Err(Error::NotFound("a mounted rekordbox drive (plug one in, or give its mount path)".into())),
        1 => return Ok(mounted.remove(0)),
        _ => {}
    }
    let libraries: BTreeSet<Uuid> = mounted.iter().filter_map(|d| d.state().and_then(|s| s.library_id)).collect();
    if libraries.len() == 1
        && let Ok(group) = drive_group(store, None)
        && group.library_id.is_some()
    {
        return lead_clone(store, &group, &Ctx::silent());
    }
    let names: Vec<String> = candidates_of(store, &mounted).into_iter().filter_map(|c| c.library).collect();
    let which = match names.is_empty() {
        true => "say which drive".to_string(),
        false => format!(
            "say which library ({}) or drive",
            names.into_iter().collect::<BTreeSet<_>>().into_iter().collect::<Vec<_>>().join(", ")
        ),
    };
    Err(Error::invalid(format!(
        "{which} to {purpose} (a library's name, or a drive's name, mount path or id): {} rekordbox drives are \
         mounted\n{}",
        mounted.len(),
        table(&candidates_of(store, &mounted))
    ))
    .because(Reason::ChooseDrive)
    .with("drives", mounted.iter().map(|d| d.root().display().to_string()).collect::<Vec<_>>())
    .action(Action::retry(serde_json::json!({ "drive": null }), "Choose the library or drive")))
}

/// The clone of a library that commands act on, among its mounted clones (`group`): the newest,
/// as `sync` finds it, which is the primary when they're identical. When they can't be ordered
/// (they diverged, or one came back from a set), the primary if it's mounted, else `sync`'s
/// refusal. Refused when the library's only mounted drives are archive or retired ones.
pub fn lead_clone(store: Option<&Store>, group: &DriveGroup, ctx: &Ctx<'_>) -> Result<Device> {
    match group.drives.as_slice() {
        [] => {
            let library = group.library.as_deref().unwrap_or_default();
            return Err(Error::NotFound(format!(
                "a mounted drive of library {library:?} but archive and retired ones"
            )));
        }
        [only] => return Device::open(only),
        _ => {}
    }
    let req = super::SyncManyRequest { drives: group.specs(), ..Default::default() };
    match super::newest_of(&req, ctx) {
        Ok((lead, _)) => Ok(lead),
        Err(e) if matches!(e.inner(), Error::Conflict(_)) => match primary_mounted(store, group) {
            Some(primary) => Device::open(&primary),
            None => Err(e),
        },
        Err(e) => Err(e),
    }
}

/// The other mounted clones of `lead`'s library, which a write to it is copied onto (see
/// `Planned::reaching_clones`; archive and retired ones aren't clones here, see [`fleet::sets`],
/// and a write to one stays on it) but the drives the write touches already (`touched`), each
/// with why it's left alone, if it is: it's newer than `lead`, diverged from it or back from a
/// set, which a sync of the two sorts out first.
pub(crate) fn clones_of(lead: &Device, touched: &[&Path], ctx: &Ctx<'_>) -> Result<Vec<(Device, Option<String>)>> {
    let mut clones = Vec::new();
    let (Some(library), Some(store)) = (lead.state().and_then(|s| s.library_id), registry()) else {
        return Ok(clones);
    };
    let set = fleet::sets(&store, &known_drives(false))?
        .into_iter()
        .find(|s| s.library_id == Some(library) && s.members.iter().any(|m| m == lead.root()));
    let others = set.map(|s| s.members).unwrap_or_default().into_iter().filter(|m| !touched.contains(&m.as_path()));
    let name = lead.name();
    for root in others {
        let clone = Device::open(&root)?;
        // A drive of the library holding no export (emptied, or set up and not written yet) is
        // filled by a sync or `provision`, not by a change to the library.
        if !clone.has_export() {
            continue;
        }
        let left_alone = match super::sync_endpoints(&pair(lead.root(), &root), ctx) {
            Ok(e) if e.after_set.is_some() => Some(format!(
                "not copying {name} onto {}: one of them came back from a set (sync the two first, which keeps \
                 the players' changes in the store)",
                clone.name()
            )),
            Ok(e) if e.drives.as_ref().is_none_or(|(source, _)| source.root() == lead.root()) => None,
            Ok(_) => Some(format!("{} is newer than {name}: it isn't copied over", clone.name())),
            Err(Error::Cancelled) => return Err(Error::Cancelled),
            Err(e) => Some(format!("not copying {name} onto {}: {e}", clone.name())),
        };
        clones.push((clone, left_alone));
    }
    Ok(clones)
}

/// Copy `lead` onto its `clones` once a write to it is done (see `Planned::reaching_clones`),
/// as a group sync does, crash-safe. The copy goes from `lead` whichever is newer: an undo leaves
/// it older than its clones, which an automatic sync would copy back over it. The plan found each
/// clone no newer than `lead`, applying it checked none changed since, and each clone's lock
/// refuses one that changed after that. The write is done by then, so what isn't copied is a
/// warning.
pub(crate) fn copy_onto_clones(lead: &Path, clones: &[&CloneTarget], store: Option<&Store>, ctx: &Ctx<'_>) {
    let owned = store.is_none().then(registry).flatten();
    let store = store.or(owned.as_ref());
    let req = super::SyncManyRequest::default();
    let copy = || {
        let source = Device::open(lead)?;
        let mut targets = Vec::new();
        for c in clones {
            targets.push(sync_plan(&req.pair(lead, &c.root, Direction::LeftToRight), ctx)?.plan);
        }
        let preview = super::SyncManyPreview { source_name: source.name(), source: lead.to_path_buf(), targets };
        super::sync_many_apply(store, &req, &preview, ctx)
    };
    match copy() {
        Ok(results) => {
            for r in results {
                if let Some(e) = r.error {
                    ctx.warn(format!("the change wasn't copied onto {}: {}", r.name, e.message));
                }
            }
        }
        Err(e) => {
            let names: Vec<&str> = clones.iter().map(|c| c.name.as_str()).collect();
            ctx.warn(format!("the change wasn't copied onto {}: {e}", names.join(", ")));
        }
    }
}

/// The drives a write to each of `specs` goes to, with several clones of one library (see
/// [`fleet::sets`]) taken once, as the library's lead clone: a write to one reaches the others
/// (see `Planned::reaching_clones`). When the clones can't be ordered and none is the primary,
/// the first named, which the write's plan says it can't copy onto the rest.
pub fn libraries_once(specs: Vec<String>) -> Result<Vec<String>> {
    let Some(store) = registry() else { return Ok(specs) };
    let devices: Vec<Device> = specs.iter().map(|s| resolve_drive(s)).collect::<Result<_>>()?;
    let sets = fleet::sets(&store, &known_drives(false))?;
    let set_of = |d: &Device| sets.iter().position(|s| s.members.iter().any(|m| m == d.root()));
    let mut taken = Vec::new();
    let mut out = Vec::new();
    for (spec, d) in specs.iter().zip(&devices) {
        let Some(set) = set_of(d) else {
            out.push(spec.clone());
            continue;
        };
        if taken.contains(&set) {
            continue;
        }
        taken.push(set);
        if devices.iter().filter(|x| set_of(x) == Some(set)).count() == 1 {
            out.push(spec.clone());
            continue;
        }
        match lead_clone(Some(&store), &group_of(sets[set].clone()), &Ctx::silent()) {
            Ok(lead) => out.push(lead.root().display().to_string()),
            Err(e) if matches!(e.inner(), Error::Conflict(_)) => out.push(spec.clone()),
            Err(e) => return Err(e),
        }
    }
    Ok(out)
}

/// The library's primary, when it's one of the mounted clones in `group`.
fn primary_mounted(store: Option<&Store>, group: &DriveGroup) -> Option<PathBuf> {
    let registry = store?.drives().ok()?;
    group
        .drives
        .iter()
        .find(|p| {
            Device::open(p)
                .ok()
                .and_then(|d| d.id())
                .and_then(|id| registry.get(&id))
                .is_some_and(|r| r.role == Some(Role::Primary))
        })
        .cloned()
}

/// What a read that's given no drive reads: each library with a clone mounted once, from its lead
/// clone (named after the library), and each mounted rekordbox drive that belongs to no library in
/// the store.
pub fn library_reads(store: Option<&Store>) -> Vec<DriveCandidate> {
    let owned = store.is_none().then(super::registry).flatten();
    let store = store.or(owned.as_ref());
    let mounted = mounted_exports();
    let candidates = candidates_of(store, &mounted);
    let sets = store.and_then(|s| fleet::sets(s, &mounted).ok()).unwrap_or_default();
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for c in &candidates {
        let Some(library) = &c.library else {
            out.push(c.clone());
            continue;
        };
        if !seen.insert(library.clone()) {
            continue;
        }
        // A library whose only mounted clones are archive or retired ones is read from the first.
        let lead = sets
            .iter()
            .find(|s| s.library.as_ref() == Some(library) && !s.members.is_empty())
            .and_then(|s| lead_clone(store, &group_of(s.clone()), &Ctx::silent()).ok())
            .map(|d| d.root().to_path_buf());
        let lead = candidates.iter().find(|x| Some(&x.root) == lead.as_ref()).unwrap_or(c);
        out.push(DriveCandidate { name: library.clone(), ..lead.clone() });
    }
    out
}

/// The drive `refresh` means when it's given none: the one mounted drive rekordbox exported over
/// recorded edits on (see [`exported_over_edits`]), else [`default_drive`].
pub fn refresh_drive(store: Option<&Store>) -> Result<Device> {
    let mut over: Vec<Device> = mounted_exports().into_iter().filter(|d| exported_over_edits(d) > 0).collect();
    match over.len() {
        1 => Ok(over.remove(0)),
        _ => default_drive(store, "refresh"),
    }
}

/// The mounted drives of one library, as `check`, `eject`, `pull` and `prep` take them when given
/// no drives (the library's drive group: archive and retired drives are left out).
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct DriveGroup {
    pub library: Option<String>,
    pub library_id: Option<Uuid>,
    /// Mount paths, the library's primary first.
    pub drives: Vec<PathBuf>,
    /// The library's mounted drives left out: archive and retired ones.
    pub excluded: Vec<String>,
}

impl DriveGroup {
    /// The drives, as requests take them.
    pub fn specs(&self) -> Vec<String> {
        self.drives.iter().map(|p| p.display().to_string()).collect()
    }
}

/// The mounted drives of `library`, or (without one) of the only library whose drives are
/// mounted. With no library's drives mounted, a single mounted rekordbox drive is a group of its
/// own. Anything else is refused, listing the candidates.
pub fn drive_group(store: Option<&Store>, library: Option<&str>) -> Result<DriveGroup> {
    let mounted = mounted_exports();
    if mounted.is_empty() {
        return Err(Error::NotFound("a mounted rekordbox drive (plug one in, or give its mount path)".into()));
    }
    let sets = match store {
        Some(s) => fleet::sets(s, &mounted)?,
        None => Vec::new(),
    };
    let set = match library {
        Some(name) => {
            let store = store.ok_or_else(|| Error::NotFound(format!("library {name:?}: there's no store yet")))?;
            let lib = store.find_library(name)?.ok_or_else(|| Error::NotFound(format!("library {name:?}")))?;
            let set = sets.into_iter().find(|s| s.library_id == Some(lib.id));
            Some(set.ok_or_else(|| Error::NotFound(format!("a mounted drive of library {:?}", lib.name)))?)
        }
        None => {
            let mut with_members: Vec<_> = sets.into_iter().filter(|s| !s.members.is_empty()).collect();
            match (with_members.len(), mounted.len()) {
                (1, _) => with_members.pop(),
                (0, 1) => None,
                (0, n) => {
                    return Err(Error::invalid(format!(
                        "say which drives (a name, a mount path or an id): {n} rekordbox drives are mounted and none \
                         belongs to a library in the store yet (pull them, or sync them, to link them to one)\n{}",
                        table(&candidates_of(store, &mounted))
                    )));
                }
                (n, _) => {
                    let names: Vec<String> = with_members.iter().filter_map(|s| s.library.clone()).collect();
                    return Err(Error::invalid(format!(
                        "say which library ({}) or which drives: drives of {n} libraries are mounted\n{}",
                        names.join(", "),
                        table(&candidates_of(store, &mounted))
                    )));
                }
            }
        }
    };
    Ok(match set {
        Some(set) => group_of(set),
        None => DriveGroup {
            library: None,
            library_id: None,
            drives: vec![mounted[0].root().to_path_buf()],
            excluded: Vec::new(),
        },
    })
}

/// A library's mounted clones as a group, its primary first.
fn group_of(set: fleet::DriveSet) -> DriveGroup {
    let mut drives: Vec<PathBuf> = set.primary.iter().cloned().collect();
    drives.extend(set.members.iter().filter(|m| Some(*m) != set.primary.as_ref()).cloned());
    DriveGroup { library: set.library, library_id: set.library_id, drives, excluded: set.excluded }
}

/// Why `spec` names no mounted drive: a known drive that isn't plugged in, or no match at all,
/// with the drives that are mounted.
pub(crate) fn unmatched_drive(spec: &str, mounted: &[Device]) -> Error {
    let ours: Vec<String> = mounted
        .iter()
        .filter(|d| d.has_export() || d.is_adopted())
        .map(|d| format!("{} ({})", d.name(), d.root().display()))
        .collect();
    let listing = match ours.is_empty() {
        true => "; no rekordbox drive is mounted".to_string(),
        false => format!("; mounted: {}", ours.join(", ")),
    };
    if let Some(store) = registry()
        && let Ok(rec) = fleet::find_record(&store, spec)
    {
        return Error::NotFound(format!(
            "{} is known but not mounted; last seen {} at {}{listing}",
            rec.name,
            local_time(rec.last_seen, "%Y-%m-%d %H:%M"),
            rec.last_mount.display()
        ));
    }
    Error::NotFound(format!("no mounted drive matches {spec:?} (use a path, a drive name, or an id){listing}"))
}

#[cfg(test)]
mod tests {
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
}
