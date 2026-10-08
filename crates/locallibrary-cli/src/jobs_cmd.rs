//! The commands that run several steps as one plan: `refresh` (rekordbox just exported to a
//! drive), `prep` (the night before a set) and `setup` (the first run); and the drive or drives a
//! command means when it's given none.
//!
//! Each takes its parsed arguments, so where the commands sit in the command line can change
//! without touching this.

use std::collections::BTreeMap;
use std::io::{IsTerminal, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{Result, bail};
use clap::Args;
use console::style;
use locallibrary::Store;
use locallibrary::api::{self, JobPlan, JobReport, StepKind};
use locallibrary::device::DeviceInfo;
use locallibrary::error::Reason;
use locallibrary::fleet::Role;
use locallibrary::op::{AnyPlanned, Planned};
use locallibrary::progress::{Ctx, Event, Progress, Scope};
use locallibrary::store::LibraryRecord;
use serde_json::{Value, json};

use crate::output::{self, Outcome, Output, dim};

// ---------------------------------------------------------------------------------------------
// Which drive
// ---------------------------------------------------------------------------------------------

/// A command line that doesn't say enough (which drive, which library). Exits with status 2, as
/// clap's own usage errors do.
#[derive(Debug)]
pub struct Usage(pub String);

impl std::fmt::Display for Usage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Usage {}

pub fn usage(message: impl Into<String>) -> anyhow::Error {
    anyhow::Error::new(Usage(message.into()))
}

/// Whether `e` is a [`Usage`] error.
pub fn is_usage(e: &anyhow::Error) -> bool {
    e.chain().any(|c| c.downcast_ref::<Usage>().is_some())
}

/// `drive`, or else the drive a command means when it's given none (the only mounted rekordbox
/// drive, or the primary of the one library whose drives are mounted). When that can't be told,
/// a usage error lists the mounted drives. `purpose` finishes "say which drive to ...".
pub fn drive_or_default(drive: Option<String>, store_path: &Option<PathBuf>, purpose: &str) -> Result<String> {
    drive_or(drive, store_path, |store| api::default_drive(store, purpose))
}

/// `drive`, or else the one `pick` chooses (a usage error when it can't).
fn drive_or(
    drive: Option<String>,
    store_path: &Option<PathBuf>,
    pick: impl FnOnce(Option<&Store>) -> locallibrary::Result<locallibrary::device::Device>,
) -> Result<String> {
    if let Some(d) = drive {
        return Ok(d);
    }
    let store = crate::diff_cmd::existing_store(store_path);
    match pick(store.as_ref()) {
        Ok(d) => Ok(d.root().display().to_string()),
        Err(e) => Err(usage(e.to_string())),
    }
}

/// `drives`, or else the mounted drives of one library (its drive group; archive and retired
/// drives are left out, and said so).
pub fn drives_or_group(
    drives: Vec<String>,
    store_path: &Option<PathBuf>,
    library: Option<&str>,
) -> Result<Vec<String>> {
    if !drives.is_empty() {
        return Ok(drives);
    }
    let store = crate::diff_cmd::existing_store(store_path);
    let group = api::drive_group(store.as_ref(), library).map_err(|e| usage(e.to_string()))?;
    if let Some(w) = api::left_out(&group.excluded) {
        output::warn(w);
    }
    Ok(group.specs())
}

/// When there's no store yet: one line on stderr suggesting `setup`.
pub fn suggest_setup(store_path: &Option<PathBuf>, out: Output) {
    if out.json || out.quiet {
        return;
    }
    let root = store_path.clone().unwrap_or_else(Store::default_root);
    if !root.join("store.json").exists() {
        eprintln!(
            "{} there's no locallibrary store yet: `locallibrary setup` names your drives and starts your library",
            style("tip:").cyan().bold()
        );
    }
}

/// `pull` with no drive: every mounted drive of one library, the newest export first, then
/// drives back from a set beside it.
pub fn pull_group(
    store: &Store,
    library: Option<String>,
    message: Option<String>,
    out: Output,
    ctx: &Ctx<'_>,
) -> Result<ExitCode> {
    let group = api::drive_group(Some(store), library.as_deref()).map_err(|e| usage(e.to_string()))?;
    let req = api::PullGroupRequest { library: group.library.clone().or(library), message };
    let report = api::pull_group(store, &req, ctx)?;
    if let Some(s) = &report.stopped
        && !report.did_anything()
    {
        bail!("{}", s.message);
    }
    let outcome = if report.stopped.is_some() { Outcome::Partial } else { Outcome::Applied };
    Ok(out.done_with(&report, outcome, |r| {
        for s in &r.steps {
            for l in &s.done {
                output::ok(l);
            }
            for w in &s.warnings {
                output::warn(w);
            }
        }
        if let Some(s) = &r.stopped {
            output::warn(format!("stopped: {}", s.message));
        }
        crate::hints::new_sessions(crate::hints::job_sessions(store, r));
    }))
}

// ---------------------------------------------------------------------------------------------
// Showing and running a job
// ---------------------------------------------------------------------------------------------

/// Prints each step's title as it begins (`[2/5] Make the recorded edits again`), and passes
/// everything else on to the progress bars.
struct Headings<'a> {
    inner: &'a dyn Progress,
    titles: Vec<(&'static str, String)>,
    show: bool,
}

impl Progress for Headings<'_> {
    fn on_event(&self, event: &Event) {
        self.inner.on_event(event);
    }

    fn on_scoped_event(&self, scope: &Scope, event: &Event) {
        self.inner.on_scoped_event(scope, event);
    }

    fn on_step(&self, scope: &Scope) {
        if let (true, Some(step)) = (self.show, scope.step) {
            let title = self.titles.iter().find(|(id, _)| *id == step.id).map_or(step.id, |(_, t)| t.as_str());
            eprintln!("{} {title}", style(format!("[{}/{}]", step.index, step.total)).cyan().bold());
        }
        self.inner.on_step(scope);
    }
}

fn render_plan(heading: &str, plan: &Planned<JobPlan>) {
    output::heading(heading);
    let mut n = 0;
    for s in &plan.steps {
        if !s.runs {
            say!("{}", dim(format!("-  {}: {}", s.title, s.reason.as_deref().unwrap_or("not needed"))));
            continue;
        }
        n += 1;
        say!("{} {}", style(format!("{n}.")).cyan().bold(), s.title);
        for l in &s.summary {
            say!("   {} {l}", style("•").cyan());
        }
    }
    for w in &plan.warnings {
        output::warn(w);
    }
}

fn title_of(plan: &JobPlan, step: &str) -> String {
    plan.steps.iter().find(|s| s.step.id() == step).map_or_else(|| step.to_string(), |s| s.title.clone())
}

fn render_report(plan: &JobPlan, r: &JobReport, store: &Store, verbose: bool) {
    for s in &r.steps {
        if s.step == StepKind::Check {
            continue;
        }
        for l in &s.done {
            output::ok(l);
        }
        for w in &s.warnings {
            output::warn(w);
        }
    }
    crate::hints::new_sessions(crate::hints::job_sessions(store, r));
    if let Some(c) = &r.check {
        say!();
        crate::check_cmd::render(c, verbose);
    }
    if let Some(s) = &r.stopped {
        output::warn(format!("stopped at \"{}\": {}", title_of(plan, &s.step), s.message));
        output::hint(
            "The steps before it stay done; `locallibrary undo <drive>` takes back the last change to a drive.",
        );
    } else if r.problems {
        output::hint("The drives stay mounted: sort out the problems above, then run this again.");
    }
}

/// How a job is shown.
struct Shown<'a> {
    /// Titles the plan.
    heading: &'a str,
    /// Said when no step runs.
    nothing: &'a str,
    dry_run: bool,
    /// The check lists the tracks a player won't load.
    verbose: bool,
}

/// Show the plan, ask, apply, report.
fn run_job(plan: Planned<JobPlan>, store: &Store, shown: Shown<'_>, out: Output, ctx: &Ctx<'_>) -> Result<ExitCode> {
    let Shown { heading, nothing, dry_run, verbose } = shown;
    if !out.json {
        render_plan(heading, &plan);
    }
    if plan.noop {
        return Ok(out.nothing(&plan, nothing));
    }
    if let Some(o) = out.decide(dry_run, "Go ahead?") {
        return Ok(out.not_applied(&plan, o));
    }
    let titles = plan.steps.iter().map(|s| (s.step.id(), s.title.clone())).collect();
    let headings = Headings { inner: ctx.progress, titles, show: !out.json && !out.quiet };
    let report = plan.apply(Some(store), &Ctx::new(&headings, ctx.cancel.clone()))?;
    if let Some(s) = &report.stopped
        && !report.did_anything()
    {
        bail!("{}: {}", title_of(&plan, &s.step), s.message);
    }
    let outcome = match (&report.stopped, report.problems) {
        (Some(_), _) => Outcome::Partial,
        (None, true) => Outcome::Problems,
        (None, false) => Outcome::Applied,
    };
    Ok(out.applied(&plan, &report, outcome, || render_report(&plan, &report, store, verbose)))
}

fn parse_step(s: &str) -> std::result::Result<StepKind, String> {
    StepKind::parse(s).map_err(|e| e.to_string())
}

// ---------------------------------------------------------------------------------------------
// refresh
// ---------------------------------------------------------------------------------------------

#[derive(Args)]
pub struct RefreshArgs {
    /// The drive rekordbox exported to [default: the only mounted rekordbox drive, else the one
    /// rekordbox exported over recorded edits on, else the primary of the one library whose
    /// drives are mounted]
    pub drive: Option<String>,
    /// Leave a step out: clean, replay, smart, player, settings, pull or sync (repeatable or
    /// comma-separated)
    #[arg(long, value_name = "STEP", value_delimiter = ',', value_parser = parse_step)]
    pub skip: Vec<StepKind>,
    /// Then copy the drive onto the other mounted drives of its library, when it's the newest
    #[arg(long)]
    pub sync: bool,
    /// Show the plan without changing anything
    #[arg(long)]
    pub dry_run: bool,
}

pub const REFRESH_HELP: &str = "Steps, each only when needed: remove rekordbox's demo samples (clean), make the \
recorded edits again (edits replay: playlists, tracks, My Tags, cues), fill the rule-based playlists (smart \
apply), put back the name and colour players show (as `drive player` last set them), write My Settings \
(settings apply), save the drive in the store (pull), and with --sync copy it onto the other mounted drives \
of its library. One plan lists them all and asks once; the first step that fails stops it (exit status 5 \
when some steps were done).";

/// `locallibrary refresh [DRIVE]`: rekordbox just exported to this drive.
pub fn refresh(args: RefreshArgs, store_path: &Option<PathBuf>, out: Output, ctx: &Ctx<'_>) -> Result<ExitCode> {
    let drive = drive_or(args.drive, store_path, api::refresh_drive)?;
    let store = crate::open_store(store_path)?;
    let req = api::RefreshRequest { drive: Some(drive), skip: args.skip, sync: args.sync };
    let plan = api::refresh_plan(&store, &req, ctx)?;
    let target = plan.targets.first().map(|t| format!("{} ({})", t.name, t.root.display())).unwrap_or_default();
    let heading = format!("Refresh {target} after rekordbox's export");
    let nothing = "nothing to do: the drive has everything rekordbox's export left out";
    run_job(plan, &store, Shown { heading: &heading, nothing, dry_run: args.dry_run, verbose: false }, out, ctx)
}

// ---------------------------------------------------------------------------------------------
// prep
// ---------------------------------------------------------------------------------------------

#[derive(Args)]
pub struct PrepArgs {
    /// The drives you're taking [default: the mounted drives of one library]
    pub drives: Vec<String>,
    /// The library whose mounted drives to take (with no drives given)
    #[arg(long, short)]
    pub library: Option<String>,
    /// Player models to check against, e.g. CDJ-3000X,CDJ-2000NXS2 [default: the ones `players set`
    /// remembered]
    #[arg(long, value_delimiter = ',')]
    pub players: Vec<String>,
    /// Also open every audio file and decode its end (slow on a big drive)
    #[arg(long)]
    pub audio: bool,
    /// Keep the drives mounted afterwards
    #[arg(long)]
    pub no_eject: bool,
    /// List the tracks each player won't load
    #[arg(long, short)]
    pub verbose: bool,
    /// Show the plan without changing anything
    #[arg(long)]
    pub dry_run: bool,
}

pub const PREP_HELP: &str = "Steps: save the drives in the store (history lists first, so the sync doesn't lose \
any), copy the newest library onto the others, check them against the player models (`players set`, or \
--players), and eject them when everything passed. One plan lists them all and asks once. Exit status 4 when \
the check finds problems (the drives stay mounted), 5 when a step failed after others were done.";

/// `locallibrary prep [DRIVES...]`: the night before a set.
pub fn prep(args: PrepArgs, store_path: &Option<PathBuf>, out: Output, ctx: &Ctx<'_>) -> Result<ExitCode> {
    let drives = drives_or_group(args.drives, store_path, args.library.as_deref())?;
    let store = crate::open_store(store_path)?;
    let req =
        api::SetJobRequest { drives, library: None, players: args.players, audio: args.audio, eject: !args.no_eject };
    let plan = api::set_job_plan(&store, &req, ctx)?;
    let names: Vec<&str> = plan.targets.iter().map(|t| t.name.as_str()).collect();
    let heading = format!("Get {} ready for a set", names.join(", "));
    let shown = Shown { heading: &heading, nothing: "nothing to do", dry_run: args.dry_run, verbose: args.verbose };
    run_job(plan, &store, shown, out, ctx)
}

// ---------------------------------------------------------------------------------------------
// setup
// ---------------------------------------------------------------------------------------------

#[derive(Args)]
pub struct SetupArgs {
    /// The drives to set up (names, mount paths or volume names) [default: asked in a terminal;
    /// with --yes, every mounted rekordbox drive but those of another library in the store]
    pub drives: Vec<String>,
    /// The library's name: a new one, or one the store has (asked in a terminal; needed with
    /// --yes)
    #[arg(long, short)]
    pub library: Option<String>,
    /// The drive rekordbox exports to [default: asked, or the first drive named or found]
    #[arg(long, value_name = "DRIVE")]
    pub primary: Option<String>,
    /// A drive's name, as DRIVE=NAME (DRIVE: its mount path or volume name; repeatable)
    #[arg(long = "name", value_name = "DRIVE=NAME")]
    pub names: Vec<String>,
    /// A drive's role, as DRIVE=ROLE: backup, set or archive (repeatable) [default: the role it has,
    /// else backup]
    #[arg(long = "role", value_name = "DRIVE=ROLE")]
    pub roles: Vec<String>,
    /// The Device Name players show for a drive, as DRIVE=NAME (repeatable)
    #[arg(long = "player-name", value_name = "DRIVE=NAME")]
    pub player_names: Vec<String>,
    /// Contact details (email, phone or name) for whoever finds a drive, put on every drive
    #[arg(long)]
    pub contact: Option<String>,
    /// With --contact: also a top-level playlist "IF FOUND: <contact>", the first thing a player shows
    #[arg(long, requires = "contact")]
    pub contact_playlist: bool,
    /// Player models to remember for `check` and `prep`, e.g. CDJ-3000,CDJ-2000NXS2
    #[arg(long, value_delimiter = ',')]
    pub players: Vec<String>,
    /// Don't import My Settings from the primary drive
    #[arg(long)]
    pub no_settings: bool,
}

pub const SETUP_HELP: &str = "Finds your rekordbox drives, names them, chooses their roles, starts your library \
with a first pull (or saves the drives in the library they're in), imports My Settings from the primary drive, \
and (if you like) sets the name players show, your contact details and the player models you play on. Each \
change shows its plan first. It asks its questions in a terminal; without one, pass --yes and --library NAME \
(the other flags answer the rest). Drives of another library in the store are left out unless named (or, in a \
terminal, said yes to), and archive drives are kept as they are.";

/// Asking (in a terminal) or taking the answers from flags (with `--yes`).
struct Setup<'a> {
    out: Output,
    asking: bool,
    store: &'a Store,
    ctx: &'a Ctx<'a>,
    /// What was done, for `--json`.
    log: Vec<Value>,
    notes: Vec<String>,
}

impl Setup<'_> {
    fn ask(&self, question: &str, default: &str) -> Result<String> {
        if !self.asking {
            return Ok(default.to_string());
        }
        match default.is_empty() {
            true => eprint!("{} {question} ", style("?").cyan().bold()),
            false => eprint!("{} {question} [{default}] ", style("?").cyan().bold()),
        }
        let _ = std::io::stderr().flush();
        let mut line = String::new();
        if std::io::stdin().read_line(&mut line)? == 0 {
            bail!("setup stopped (no more input)");
        }
        let line = line.trim();
        Ok(if line.is_empty() { default.to_string() } else { line.to_string() })
    }

    fn ask_yes(&self, question: &str, default: bool) -> Result<bool> {
        if !self.asking {
            return Ok(default);
        }
        let hint = if default { "[Y/n]" } else { "[y/N]" };
        eprint!("{} {question} {hint} ", style("?").cyan().bold());
        let _ = std::io::stderr().flush();
        let mut line = String::new();
        if std::io::stdin().read_line(&mut line)? == 0 {
            bail!("setup stopped (no more input)");
        }
        Ok(match line.trim().to_lowercase().as_str() {
            "" => default,
            a => a.starts_with('y'),
        })
    }

    fn note(&mut self, line: impl Into<String>) {
        let line = line.into();
        if !self.out.json {
            say!("  {}", dim(&line));
        }
        self.notes.push(line);
    }

    /// Show the plans, ask once (in a terminal), apply each. Whether they were applied.
    fn apply(&mut self, what: &str, question: &str, plans: Vec<Box<dyn AnyPlanned>>) -> Result<bool> {
        let plans: Vec<Box<dyn AnyPlanned>> = plans.into_iter().filter(|p| !p.is_noop()).collect();
        if plans.is_empty() {
            return Ok(true);
        }
        if !self.out.json {
            for p in &plans {
                output::summary(p.summary(), p.warnings());
            }
        }
        if self.asking && !self.out.confirm(question) {
            self.note("left as it is");
            return Ok(false);
        }
        for p in plans {
            let report = p.apply_json(Some(self.store), self.ctx)?;
            self.log.push(json!({ "step": what, "plan": p.to_json(), "report": report }));
        }
        Ok(true)
    }
}

/// `DRIVE=VALUE` flags, by the drive they name (a mount path, volume name or drive name).
fn by_drive(flags: &[String], drives: &[DeviceInfo], flag: &str) -> Result<BTreeMap<PathBuf, String>> {
    let mut out = BTreeMap::new();
    for f in flags {
        let Some((key, value)) = f.split_once('=') else {
            return Err(usage(format!("{flag} takes DRIVE=VALUE, not {f:?}")));
        };
        out.insert(being_set_up(&format!("{flag} {f:?}"), key, drives)?.root.clone(), value.trim().to_string());
    }
    Ok(out)
}

/// The drive among `drives` that `key` (given as `given`, a flag) names, or the usage error
/// saying it isn't one of them.
fn being_set_up<'d>(given: &str, key: &str, drives: &'d [DeviceInfo]) -> Result<&'d DeviceInfo> {
    find_drive(key, drives)?
        .ok_or_else(|| usage(format!("{given}: {:?} isn't a mounted rekordbox drive being set up", key.trim())))
}

/// The drive `key` names, found as every command finds one (a mount path, a drive's name or
/// volume name, an id; a key two drives answer to is refused), when it's one of `drives`.
fn find_drive<'d>(key: &str, drives: &'d [DeviceInfo]) -> Result<Option<&'d DeviceInfo>> {
    match api::resolve_drive(key.trim()) {
        Ok(d) => Ok(drives.iter().find(|x| x.root == d.root())),
        Err(locallibrary::Error::NotFound(_)) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

fn label(d: &DeviceInfo) -> String {
    d.name.clone().unwrap_or_else(|| d.volume_name.clone())
}

/// A role as `drive role` takes it.
fn role_arg(s: &str) -> Result<String> {
    match Role::parse(s) {
        Ok(r) => Ok(r.name().to_string()),
        Err(_) => Err(usage(format!("unknown role {:?} (primary, backup, set or archive)", s.trim()))),
    }
}

fn mounted_exports() -> Vec<DeviceInfo> {
    api::list_drives(&api::ListDrives { all: false }).into_iter().filter(|d| d.has_export).collect()
}

/// The drives to set up: the ones named; else each mounted one the user says yes to (in a
/// terminal), or (with --yes) all of them. A drive of a library the store has, other than
/// `library` (or the store's only library), is left out unless named or said yes to.
fn choose_drives(
    s: &mut Setup,
    named: &[String],
    library: Option<&str>,
    mounted: Vec<DeviceInfo>,
) -> Result<Vec<DeviceInfo>> {
    if !named.is_empty() {
        let mut out: Vec<DeviceInfo> = Vec::new();
        for n in named {
            let d = find_drive(n, &mounted)?.ok_or_else(|| usage(format!("no rekordbox drive {n:?} is mounted")))?;
            if !out.iter().any(|o| o.root == d.root) {
                out.push(d.clone());
            }
        }
        return Ok(out);
    }
    let libraries = s.store.libraries()?;
    let target = match library {
        Some(l) => s.store.find_library(l.trim())?,
        None if libraries.len() == 1 => libraries.first().cloned(),
        None => None,
    };
    let several = mounted.len() > 1;
    let mut out = Vec::new();
    for d in mounted {
        let other = d.library_id.filter(|id| target.as_ref().is_none_or(|t| t.id != *id));
        let other = other.and_then(|id| libraries.iter().find(|l| l.id == id));
        let take = match other {
            None if !several => true,
            None => s.ask_yes(&format!("Set up {}?", label(&d)), true)?,
            Some(l) => s.ask_yes(&format!("Set up {}? It's a drive of library {:?}", label(&d), l.name), false)?,
        };
        match (take, other) {
            (true, _) => out.push(d),
            (false, Some(l)) => {
                s.note(format!("left out {}, a drive of library {:?} (name it to set it up)", label(&d), l.name))
            }
            (false, None) => s.note(format!("left out {}", label(&d))),
        }
    }
    if out.is_empty() {
        bail!("no drive to set up: name the drives (`locallibrary setup DRIVE...`)");
    }
    Ok(out)
}

/// The drive `--primary` names among `drives`, if it's given.
fn given_primary<'d>(primary: Option<&str>, drives: &'d [DeviceInfo]) -> Result<Option<&'d DeviceInfo>> {
    primary.map(|p| being_set_up(&format!("--primary {p:?}"), p, drives)).transpose()
}

/// The library the store has that `primary` is a drive of.
fn linked_library(store: &Store, primary: &DeviceInfo) -> Result<Option<LibraryRecord>> {
    Ok(primary.library_id.map(|id| store.library_by_id(id)).transpose()?.flatten())
}

/// The library the primary is a drive of (`linked`), when that isn't the one it's set up in
/// (`existing`, the store's library of the name given): setting it up there would leave `linked`
/// without it.
fn left_behind<'l>(linked: &'l Option<LibraryRecord>, existing: &Option<LibraryRecord>) -> Option<&'l LibraryRecord> {
    linked.as_ref().filter(|l| existing.as_ref().is_none_or(|e| e.id != l.id))
}

fn leaving(primary: &DeviceInfo, from: &LibraryRecord) -> String {
    format!("{} is a drive of library {:?}, which would be left without it", label(primary), from.name)
}

/// A drive's plan for setup's last steps, or `None` (said) when the drive is kept as it is: an
/// archive drive, or a retired one. `what` is what's left off it.
fn plan_for<P: AnyPlanned + 'static>(
    s: &mut Setup,
    d: &DeviceInfo,
    what: &str,
    plan: locallibrary::Result<P>,
) -> Result<Option<Box<dyn AnyPlanned>>> {
    match plan {
        Ok(p) => Ok(Some(Box::new(p))),
        Err(e) if e.reason() == Some(Reason::Archive) => {
            s.note(format!("{} is an archive drive, kept as it is: no {what}", label(d)));
            Ok(None)
        }
        Err(e) if e.reason() == Some(Reason::Retired) => {
            s.note(format!("{} is retired: no {what}", label(d)));
            Ok(None)
        }
        Err(e) => Err(e.into()),
    }
}

/// `locallibrary setup`: the first run.
pub fn setup(args: SetupArgs, store_path: &Option<PathBuf>, out: Output, ctx: &Ctx<'_>) -> Result<ExitCode> {
    let asking = !out.json && !out.yes && std::io::stdin().is_terminal();
    if !asking && !out.yes {
        return Err(usage(
            "setup asks questions, so it needs a terminal; without one, pass --yes with --library NAME \
             (and --primary, --name, --role, --player-name, --contact, --players as you like)",
        ));
    }
    if !asking && args.library.is_none() {
        return Err(usage("without a terminal to ask in, setup needs --library NAME: what to call your library"));
    }
    let mounted = mounted_exports();
    if mounted.is_empty() {
        bail!(
            "no rekordbox drive is mounted: plug in a drive rekordbox exported to, then run `locallibrary setup` again"
        );
    }
    let store = crate::open_store(store_path)?;
    let mut s = Setup { out, asking, store: &store, ctx, log: Vec::new(), notes: Vec::new() };
    if !out.json {
        let libraries = store.libraries()?;
        output::heading(&format!("{} rekordbox drive(s) mounted", mounted.len()));
        for (i, d) in mounted.iter().enumerate() {
            let tracks = d.summary.as_ref().map(|s| format!("{} tracks", s.tracks)).unwrap_or_default();
            let library = d.library_id.and_then(|id| libraries.iter().find(|l| l.id == id));
            let library = library.map(|l| format!("library {:?}", l.name)).unwrap_or_default();
            say!("  {}. {}  {}  {}  {}", i + 1, label(d), dim(d.root.display()), dim(tracks), dim(library));
        }
    }
    // A drive mounted now is another drive, not one the store knew before it was erased.
    let held: Vec<_> = mounted.iter().filter_map(|d| d.id).collect();
    let drives = choose_drives(&mut s, &args.drives, args.library.as_deref(), mounted)?;
    let names = by_drive(&args.names, &drives, "--name")?;
    let roles = by_drive(&args.roles, &drives, "--role")?;
    let player_names = by_drive(&args.player_names, &drives, "--player-name")?;
    let roots: Vec<PathBuf> = drives.iter().map(|d| d.root.clone()).collect();
    // Without a terminal, a primary that would leave its library behind is refused before
    // anything is written.
    if let (false, Some(library)) = (asking, &args.library) {
        let primary = given_primary(args.primary.as_deref(), &drives)?.unwrap_or(&drives[0]);
        let linked = linked_library(&store, primary)?;
        if let Some(from) = left_behind(&linked, &store.find_library(library.trim())?) {
            let why = leaving(primary, from);
            return Err(usage(format!(
                "{why}: set that library up with --library {:?}, or choose another primary",
                from.name
            )));
        }
    }

    // 1. Names: a known drive's identity back, or a new one.
    if !out.json {
        say!();
        output::heading("Names");
    }
    let mut plans: Vec<Box<dyn AnyPlanned>> = Vec::new();
    for d in &drives {
        let spec = d.root.display().to_string();
        if d.id.is_none()
            && !names.contains_key(&d.root)
            && let Ok(r) = api::drive_recognise(&store, &api::DriveRef { drive: spec.clone() }, ctx)
            && let Some(best) = r.best().filter(|b| !held.contains(&b.id))
            && s.ask_yes(
                &format!("{} looks like {}, a drive you had before. Give it that identity back?", label(d), best.name),
                true,
            )?
        {
            let req = api::AdoptAs { drive: spec, known: best.id.to_string() };
            plans.push(Box::new(api::drive_adopt_as_plan(&store, &req)?));
            continue;
        }
        let default = names.get(&d.root).cloned().unwrap_or_else(|| label(d));
        let name = s.ask(&format!("Name for {}", d.root.display()), &default)?;
        plans.push(Box::new(api::adopt_plan(&api::AdoptDrive { drive: spec, name: Some(name) })?));
    }
    s.apply("names", "Name them?", plans)?;
    // Read again (named now), in the order chosen: the first is the primary unless one is given.
    let listed = mounted_exports();
    let drives: Vec<DeviceInfo> = roots.iter().filter_map(|r| listed.iter().find(|d| d.root == *r).cloned()).collect();

    // 2. The primary, the other drives' roles, and the library.
    if !out.json {
        say!();
        output::heading("Your library");
    }
    let primary = match given_primary(args.primary.as_deref(), &drives)? {
        Some(d) => d,
        None if drives.len() == 1 || !asking => &drives[0],
        None => {
            for (i, d) in drives.iter().enumerate() {
                say!("  {}. {}", i + 1, label(d));
            }
            let mut pick = None;
            while pick.is_none() {
                let a = s.ask(&format!("Which one does rekordbox export to (1-{})?", drives.len()), "1")?;
                pick = a.parse::<usize>().ok().filter(|n| (1..=drives.len()).contains(n)).map(|n| &drives[n - 1]);
            }
            pick.expect("chosen")
        }
    };
    let mut chosen: Vec<(&DeviceInfo, String)> = vec![(primary, "primary".into())];
    // Run again, setup keeps the roles drives have unless told otherwise.
    let registry = store.drives()?;
    for d in drives.iter().filter(|d| d.root != primary.root) {
        let had = d.id.and_then(|id| registry.get(&id)).and_then(|r| r.role).filter(|r| *r != Role::Primary);
        let default = roles.get(&d.root).cloned().unwrap_or_else(|| had.map_or("backup", Role::name).to_string());
        let role = loop {
            let a = s.ask(&format!("Role for {} (backup, set or archive)", label(d)), &default)?;
            match role_arg(&a) {
                Ok(r) => break r,
                Err(e) if s.asking => output::warn(e),
                Err(e) => return Err(e),
            }
        };
        chosen.push((d, role));
    }
    // The library the primary is in already, if the store has it: setting up again joins it.
    let linked = linked_library(&store, primary)?;
    let library = match &args.library {
        Some(l) => l.clone(),
        None => s.ask("Name for your library", &linked.as_ref().map_or_else(|| label(primary), |l| l.name.clone()))?,
    };
    let existing = store.find_library(library.trim())?;
    if let Some(from) = left_behind(&linked, &existing).filter(|_| s.asking)
        && !out.confirm(&format!("{}. Set it up in {library:?} anyway?", leaving(primary, from)))
    {
        bail!("left as it is: run setup again with --library {:?}", from.name);
    }
    let first = match &existing {
        None => format!("save {} in the store as the first snapshot of a new library, {library:?}", label(primary)),
        Some(l) if linked.as_ref().is_some_and(|x| x.id == l.id) => {
            format!(
                "save {} in {:?}, the library it's in already (`locallibrary pull` does this from now on)",
                label(primary),
                l.name
            )
        }
        Some(l) => format!("save {} in {:?}, a library the store has", label(primary), l.name),
    };
    if !out.json {
        output::summary(
            &[
                first,
                "then the other drives in it too, where they hold the same library".into(),
                format!(
                    "roles: {}",
                    chosen.iter().map(|(d, r)| format!("{} {r}", label(d))).collect::<Vec<_>>().join(", ")
                ),
            ],
            &[],
        );
    }
    let question = if existing.is_some() { "Save them in it?" } else { "Start the library?" };
    if !s.asking || out.confirm(question) {
        let req = api::PullRequest {
            drive: primary.root.display().to_string(),
            library: Some(existing.as_ref().map_or_else(|| library.clone(), |l| l.name.clone())),
            new_library: existing.is_none(),
            message: Some("the first pull (setup)".into()),
            ..Default::default()
        };
        let r = api::pull(&store, &req, ctx)?;
        s.log.push(json!({ "step": "pull", "report": r }));
        if !out.json {
            output::ok(format!("saved {} in {}", label(primary), r.library));
        }
        for (d, role) in &chosen {
            if d.root != primary.root && role != "archive" {
                let req = api::PullRequest {
                    drive: d.root.display().to_string(),
                    library: Some(r.library.clone()),
                    message: Some("the first pull (setup)".into()),
                    ..Default::default()
                };
                match api::pull(&store, &req, ctx) {
                    Ok(p) => {
                        s.log.push(json!({ "step": "pull", "report": p }));
                        if !out.json {
                            output::ok(format!("saved {} in {}", label(d), p.library));
                        }
                    }
                    Err(locallibrary::Error::Cancelled) => return Err(locallibrary::Error::Cancelled.into()),
                    Err(e) => s.note(format!(
                        "{} isn't in {} yet ({e}); `locallibrary sync {} {}` brings it up to date and adds it",
                        label(d),
                        r.library,
                        label(primary),
                        label(d)
                    )),
                }
            }
            let rec = api::drive_role(
                &store,
                &api::SetRole { drive: d.root.display().to_string(), role: Some(role.clone()) },
            )?;
            s.log.push(json!({ "step": "role", "report": rec }));
        }
    } else {
        s.note("no library yet: `locallibrary pull <drive>` starts one");
    }

    // 3. My Settings, from the primary drive.
    if !args.no_settings {
        if !out.json {
            say!();
            output::heading("My Settings");
        }
        let req = api::ImportSettings { drive: Some(primary.root.display().to_string()) };
        match api::settings_import_plan(&store, &req) {
            Err(locallibrary::Error::NotFound(_)) => {
                s.note(format!("{} has no My Settings files to import", label(primary)));
            }
            Err(e) => return Err(e.into()),
            Ok(plan) if plan.noop => s.note("the profile has those settings already"),
            Ok(plan) => {
                let question = format!(
                    "Make {}'s My Settings your profile (`settings apply` writes it to drives)?",
                    label(primary)
                );
                s.apply("settings", &question, vec![Box::new(plan)])?;
            }
        }
    }

    // 4. What players show, and who to call.
    let with_onelibrary: Vec<&DeviceInfo> = drives.iter().filter(|d| d.has_library_plus).collect();
    let wanted = !player_names.is_empty()
        || (!with_onelibrary.is_empty()
            && s.ask_yes("Set the name players show for each drive (its Device Name)?", false)?);
    if wanted {
        if !out.json {
            say!();
            output::heading("Device Names");
        }
        let mut plans: Vec<Box<dyn AnyPlanned>> = Vec::new();
        for d in &drives {
            let given = player_names.get(&d.root).cloned();
            if !d.has_library_plus {
                if given.is_some() || asking {
                    s.note(format!(
                        "{} has no OneLibrary database, where newer players read the name: set it in rekordbox",
                        label(d)
                    ));
                }
                continue;
            }
            let name = match (given, asking) {
                (Some(n), _) => n,
                (None, true) => s.ask(&format!("Device Name for {} (empty to leave it)", label(d)), "")?,
                (None, false) => continue,
            };
            if name.is_empty() {
                continue;
            }
            let req = api::SetPlayerIdentity { drive: d.root.display().to_string(), name: Some(name), colour: None };
            plans.extend(plan_for(&mut s, d, "Device Name", api::player_set_plan(&req))?);
        }
        s.apply("player", "Set them?", plans)?;
    }
    let contact = match &args.contact {
        Some(c) => c.clone(),
        None => s.ask(
            "Contact details to put on the drives for whoever finds one (email, phone or name; empty to skip)",
            "",
        )?,
    };
    if !contact.trim().is_empty() {
        let playlist = args.contact_playlist
            || s.ask_yes(&format!("Also a playlist named \"IF FOUND: {contact}\" at the top?"), false)?;
        let mut plans: Vec<Box<dyn AnyPlanned>> = Vec::new();
        for d in &drives {
            let req = api::SetContact { drive: d.root.display().to_string(), contact: contact.clone(), playlist };
            plans.extend(plan_for(&mut s, d, "contact details", api::drive_contact_plan(&req))?);
        }
        s.apply("contact", "Write them?", plans)?;
    }

    // 5. The players `check` tests against.
    let models = match args.players.is_empty() {
        false => args.players.join(","),
        true => s.ask(
            "Which player models do you play on (comma-separated, e.g. CDJ-3000,CDJ-2000NXS2; empty to skip)?",
            "",
        )?,
    };
    let models: Vec<String> = models.split(',').map(|m| m.trim().to_string()).filter(|m| !m.is_empty()).collect();
    if !models.is_empty() {
        match api::players_set_plan(&store, &api::SetPlayers { models, library: None }) {
            Ok(plan) => {
                s.apply("players", "Remember them?", vec![Box::new(plan)])?;
            }
            Err(e) if asking => output::warn(format!(
                "{e} (`locallibrary players models` lists them; `players set` remembers them later)"
            )),
            Err(e) => return Err(e.into()),
        }
    }

    let report = json!({ "steps": s.log, "notes": s.notes });
    Ok(out.done(&report, |_| {
        say!();
        output::heading("You're set up. The commands you'll use most:");
        for (c, what) in [
            ("locallibrary status", "every drive at a glance"),
            ("locallibrary refresh", "after rekordbox exports to a drive: your edits, settings and names back"),
            ("locallibrary sync", "copy the newest drive onto the others"),
            ("locallibrary prep", "the night before a set: save, sync, check and eject"),
            ("locallibrary", "browse your drives and listen"),
        ] {
            say!("  {:<22} {}", style(c).bold(), dim(what));
        }
    }))
}
