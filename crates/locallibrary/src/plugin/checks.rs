//! Plugins' checks (`check/run`, `plan/check`, the `checks` feature). What a check finds on a
//! drive joins the check before a set and doctor, each finding named `<plugin>.<check>` and with
//! the plugin's operation that fixes it, when it says. What a guard (a check run `when = "apply"`)
//! says about a plan that edits a library (edits, cues, a playlist restored, a contact's
//! playlist) is among the plan's warnings, and once the user makes the guard blocking, a warning
//! or a problem it finds keeps the plan from being applied. A check that fails, answers what it
//! may not, or takes longer than the `checks.timeout` setting is a warning, never a failure. Each
//! runs unless the user turns it off, or (`default_on = false`) once they turn it on. The app
//! hands the check before a set, doctor and those plans its host ([`set_hosted`]).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use indexmap::IndexMap;
use loclib_plugin::{Check, Context, Finding, Findings, Level, PlanCheck, RunCheck, When, method};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::process::{Slot, printable};
use super::{Plugins, host_settings, set_host_setting};
use crate::check::CheckItem;
use crate::device::Device;
use crate::edit::Edit;
use crate::op::Planned;
use crate::paths;
use crate::progress::{CancelToken, Ctx};
use crate::store::Store;
use crate::{Error, Result};

/// How long a check has to answer unless the `checks.timeout` setting says.
pub const TIMEOUT: Duration = Duration::from_secs(10);

/// The longest a check may be given to answer: a setting past it is read as it.
pub const MAX_TIMEOUT: Duration = Duration::from_secs(3600);

/// The shortest a check may be given to answer: a setting under it (but more than 0) is read as
/// it.
pub const MIN_TIMEOUT: Duration = Duration::from_millis(1);

/// The host's setting for how long a check has to answer, in seconds.
const TIMEOUT_SETTING: &str = "checks.timeout";

/// The most of one check's findings kept: past it, the rest are left out, saying so.
const MAX_FINDINGS: usize = 1000;

/// The most characters of what a finding says (its message, its playlist's name) kept: past it,
/// it's cut short.
const MAX_SAID: usize = 500;

/// How often a check under way looks for the time it has being up, or its caller cancelling.
const TICK: Duration = Duration::from_millis(50);

/// What a check found: each finding with what fixes it.
type Found = Vec<(Finding, Option<Fix>)>;

/// The plugin operation that fixes a finding, with its params: planned and confirmed as any is
/// (`plugins.op.plan`, with the drive it was found on).
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct Fix {
    pub plugin: String,
    pub operation: String,
    pub params: Value,
}

/// Whether a plugin's check runs, and whether a guard keeps a plan it finds something about from
/// being applied.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
pub struct CheckSettings {
    pub on: bool,
    /// Only a guard's can be on.
    pub blocking: bool,
}

/// A change to a check's settings.
#[derive(Debug, Clone, Copy, Default, Deserialize, JsonSchema)]
pub struct CheckChange {
    /// Run it, or not; left out, as it is.
    #[serde(default)]
    pub on: Option<bool>,
    /// For a guard: keep a plan it finds a warning or a problem about from being applied, or not;
    /// left out, as it is.
    #[serde(default)]
    pub blocking: Option<bool>,
    /// Put both back to their defaults first.
    #[serde(default)]
    pub reset: bool,
}

/// What the check before a set, doctor, edit plans and `refresh` find plugins in: the app's host.
static HOSTED: OnceLock<fn() -> Option<&'static Plugins>> = OnceLock::new();

/// Has the check before a set and doctor run the checks of the plugins `host` answers, plans
/// that edit a library their guards (see [`Plugins::run_checks`], [`Plugins::guard`]), and
/// `refresh` their operations (see [`crate::api::refresh_plan`]): set once, by the app. Until it
/// is, they run none, and a refresh asked to run an operation is refused.
pub fn set_hosted(host: fn() -> Option<&'static Plugins>) {
    let _ = HOSTED.set(host);
}

/// The plugins [`set_hosted`] gave, if any.
pub(crate) fn hosted() -> Option<&'static Plugins> {
    HOSTED.get().and_then(|host| host())
}

/// How long a check has to answer: the setting in `store`, from [`MIN_TIMEOUT`] to
/// [`MAX_TIMEOUT`] (one kept by hand may be past them: it's read as the nearest), else [`TIMEOUT`].
pub fn check_timeout(store: Option<&Store>) -> Duration {
    let secs = host_settings(store).get(TIMEOUT_SETTING).and_then(Value::as_f64).filter(|s| *s > 0.0);
    let kept = secs.map(|s| Duration::try_from_secs_f64(s).unwrap_or(MAX_TIMEOUT));
    kept.map_or(TIMEOUT, |d| d.clamp(MIN_TIMEOUT, MAX_TIMEOUT))
}

/// Gives a check `secs` seconds to answer (from [`MIN_TIMEOUT`] to [`MAX_TIMEOUT`]), or with
/// `None` [`TIMEOUT`] again. Answers the setting then.
pub fn set_check_timeout(store: &Store, secs: Option<f64>) -> Result<Duration> {
    let (least, most) = (MIN_TIMEOUT.as_secs_f64(), MAX_TIMEOUT.as_secs_f64());
    if let Some(s) = secs.filter(|s| !(least..=most).contains(s)) {
        return Err(Error::invalid(format!("a check has {least} to {most} seconds to answer, not {s}")));
    }
    set_host_setting(store, TIMEOUT_SETTING, secs.map(Value::from))?;
    Ok(check_timeout(Some(store)))
}

/// The host's setting for one of a check's settings (`on`, `blocking`).
fn setting(plugin: &str, check: &str, which: &str) -> String {
    format!("checks.{plugin}.{check}.{which}")
}

/// `plugin`'s `check`'s settings, as `kept` (the host's settings) has them, else its defaults.
fn settings_of(kept: &IndexMap<String, Value>, plugin: &str, check: &Check) -> CheckSettings {
    let kept = |which: &str| kept.get(&setting(plugin, &check.name, which)).and_then(Value::as_bool);
    let blocking = check.when.contains(&When::Apply) && kept("blocking").unwrap_or(false);
    CheckSettings { on: kept("on").unwrap_or(check.default_on), blocking }
}

/// Runs `f` with a context cancelled once `timeout` has passed, or `ctx` is: `None` when the time
/// was up first.
fn within<T>(timeout: Duration, ctx: &Ctx<'_>, f: impl FnOnce(&Ctx<'_>) -> Result<T>) -> Result<Option<T>> {
    ctx.check()?;
    let (cancel, late) = (CancelToken::new(), AtomicBool::new(false));
    let (done, finished) = mpsc::channel::<()>();
    let caller = ctx.cancel.clone();
    let result = std::thread::scope(|s| {
        let (cancel, late) = (&cancel, &late);
        s.spawn(move || {
            // Past what the clock can say, it has no deadline.
            let until = Instant::now().checked_add(timeout);
            while let Err(RecvTimeoutError::Timeout) = finished.recv_timeout(TICK) {
                if caller.is_cancelled() || until.is_some_and(|until| Instant::now() >= until) {
                    late.store(!caller.is_cancelled(), Ordering::SeqCst);
                    return cancel.cancel();
                }
            }
        });
        let result = f(&Ctx::new(ctx.progress, cancel.clone()));
        drop(done);
        result
    });
    match result {
        Err(Error::Cancelled) if late.load(Ordering::SeqCst) => Ok(None),
        Err(_) if ctx.cancel.is_cancelled() => Err(Error::Cancelled),
        result => result.map(Some),
    }
}

impl Plugins {
    /// Every check, with its plugin's slot and whether it runs as `store` keeps it (its defaults
    /// without one), in the order the app gave the plugins.
    fn all_checks(&self, store: Option<&Store>) -> Vec<(&Arc<Slot>, &Check, CheckSettings)> {
        let kept = host_settings(store);
        let all = self.plugins.values().flat_map(|s| s.manifest.contributes.checks.iter().map(move |c| (s, c)));
        all.map(|(slot, c)| (slot, c, settings_of(&kept, &slot.manifest.id, c))).collect()
    }

    /// Every check, with its plugin's id and whether it runs as `store` keeps it (its defaults
    /// without one), in the order the app gave the plugins.
    pub fn checks(&self, store: Option<&Store>) -> Vec<(&str, &Check, CheckSettings)> {
        self.all_checks(store).into_iter().map(|(slot, c, settings)| (slot.manifest.id.as_str(), c, settings)).collect()
    }

    /// `plugin`'s check `check`.
    fn check(&self, plugin: &str, check: &str) -> Result<&Check> {
        let slot = self.slot(plugin)?;
        slot.declared("check", check, &slot.manifest.contributes.checks, |c| &c.name)
    }

    /// Whether `plugin`'s check `check` runs, and keeps a plan it finds something about from
    /// being applied, as `store` keeps them (their defaults without one).
    pub fn check_settings(&self, plugin: &str, check: &str, store: Option<&Store>) -> Result<CheckSettings> {
        Ok(settings_of(&host_settings(store), plugin, self.check(plugin, check)?))
    }

    /// Refused when `plugin` has no check `check`, or `change` would have one that isn't a guard
    /// keep plans from being applied.
    pub fn check_change(&self, plugin: &str, check: &str, change: CheckChange) -> Result<()> {
        let declared = self.check(plugin, check)?;
        if change.blocking == Some(true) && !declared.when.contains(&When::Apply) {
            return Err(Error::invalid(format!(
                "{plugin}.{check} isn't run before a plan is applied, so it can't keep one from being applied"
            )));
        }
        Ok(())
    }

    /// Changes `plugin`'s check `check`'s settings as `change` says (see
    /// [`Plugins::check_change`]). Answers them then.
    pub fn set_check(&self, store: &Store, plugin: &str, check: &str, change: CheckChange) -> Result<CheckSettings> {
        self.check_change(plugin, check, change)?;
        let set = |which: &str, value: Option<bool>| {
            set_host_setting(store, &setting(plugin, check, which), value.map(Value::from))
        };
        if change.reset {
            set("on", None)?;
            set("blocking", None)?;
        }
        for (which, value) in [("on", change.on), ("blocking", change.blocking)] {
            if value.is_some() {
                set(which, value)?;
            }
        }
        self.check_settings(plugin, check, Some(store))
    }

    /// The checks run `when` that are on.
    fn running(&self, when: When, store: Option<&Store>) -> Vec<(&Arc<Slot>, &Check, CheckSettings)> {
        let all = self.all_checks(store).into_iter();
        all.filter(|(_, c, settings)| settings.on && c.when.contains(&when)).collect()
    }

    /// What the plugins' checks run with the check before a set (and on) find on `device`, in the
    /// order the plugins and their checks are; a check that failed, answered what it may not or
    /// took too long is a warning saying so. Fails only when cancelled.
    pub fn run_checks(&self, device: &Device, store: Option<&Store>, ctx: &Ctx<'_>) -> Result<Vec<CheckItem>> {
        let timeout = check_timeout(store);
        let mut items = Vec::new();
        for (slot, check, _) in self.running(When::Check, store) {
            let ran = within(timeout, ctx, |ctx| {
                let context = Context { drive: Some(device.root().display().to_string()), ..Context::default() };
                let (context, _) = slot.context(context, store)?;
                let req = RunCheck { check: check.name.clone(), context };
                slot.call_with(method::RUN_CHECK, &req, store, ctx, &|_| {}, |answer, _, _| slot.found(check, answer))
            });
            let item = |(finding, fix)| CheckItem { drive: device.name(), finding, fix };
            match slot.ran(check, timeout, ran)? {
                Ok(found) => items.extend(found.into_iter().map(item)),
                Err(why) => items.push(item((Finding::new(Level::Warning, slot.named(check), why), None))),
            }
        }
        Ok(items)
    }

    /// Runs the guards that are on (the plugins' checks run before a plan is applied) on
    /// `planned`, given its edits as `edits` has them (as the journal records them), unless it
    /// changes nothing or makes no edit: each finding among its warnings, as `<check's title>:
    /// <message>`, and a guard that failed or took too long one saying so. When a blocking guard
    /// found a warning or a problem, it's what keeps the plan from being applied
    /// ([`Planned::blocked`]); one that failed never does. Fails only when cancelled.
    pub fn guard<P, R>(
        &self,
        planned: &mut Planned<P, R>,
        edits: impl FnOnce(&P) -> Vec<Edit>,
        store: Option<&Store>,
        ctx: &Ctx<'_>,
    ) -> Result<()> {
        let guards = self.running(When::Apply, store);
        if planned.noop || guards.is_empty() {
            return Ok(());
        }
        let edits = edits(&planned.plan);
        if edits.is_empty() {
            return Ok(());
        }
        let edits = edits.iter().map(serde_json::to_value).collect::<serde_json::Result<Vec<_>>>();
        let edits = edits.map_err(|e| Error::invalid(e.to_string()))?;
        let drive = planned.drives.iter().find(|t| t.writes && t.drive).map(|t| t.root.display().to_string());
        let timeout = check_timeout(store);
        for (slot, check, settings) in guards {
            let ran = within(timeout, ctx, |ctx| {
                let (context, _) = slot.context(Context { drive: drive.clone(), ..Context::default() }, store)?;
                let operation = planned.operation.to_string();
                let req = PlanCheck { check: check.name.clone(), operation, edits: edits.clone(), context };
                slot.call_with(method::PLAN_CHECK, &req, store, ctx, &|_| {}, |answer, _, _| slot.found(check, answer))
            });
            let found = match slot.ran(check, timeout, ran)? {
                Ok(found) => found,
                Err(why) => vec![(Finding::new(Level::Info, slot.named(check), why), None)],
            };
            for (finding, _) in found {
                let said = format!("{}: {}", check.title, finding.message);
                if !planned.warnings.contains(&said) {
                    planned.warnings.push(said);
                }
                if settings.blocking && finding.level >= Level::Warning {
                    planned.blocked.get_or_insert(finding.check);
                }
            }
        }
        Ok(())
    }
}

impl<P, R> Planned<P, R> {
    /// With what the guards of the plugins [`set_hosted`] gave make of it, its edits as `edits`
    /// has them (see [`Plugins::guard`]), before it's confirmed.
    pub(crate) fn guarded(mut self, edits: impl FnOnce(&P) -> Vec<Edit>, ctx: &Ctx<'_>) -> Result<Self> {
        if let Some(plugins) = hosted() {
            plugins.guard(&mut self, edits, crate::api::registry().as_ref(), ctx)?;
        }
        Ok(self)
    }
}

impl Slot {
    /// What `check` answered: each finding named `<plugin>.<check>` and saying something (as
    /// [`Slot::said`] has it), its files on the drive, and its fix one of the plugin's operations.
    /// Past [`MAX_FINDINGS`], the rest are left out, an `info` saying so.
    fn found(&self, check: &Check, answer: Findings) -> Result<Found> {
        let (id, name) = (&self.manifest.id, self.named(check));
        let mut found = Vec::new();
        let total = answer.findings.len();
        for (i, f) in answer.findings.into_iter().take(MAX_FINDINGS).enumerate() {
            let message = self.said(&f.finding.message);
            if message.trim().is_empty() {
                return Err(self.misbehaved(format!("its finding {i} says nothing")));
            }
            let on_the_drive = |p: &String| paths::is_safe_key(p) && !p.chars().any(char::is_control);
            if let Some(path) = f.finding.paths.iter().find(|p| !on_the_drive(p)) {
                return Err(self.misbehaved(format!("its finding {i} names {path:?}, which isn't a file on the drive")));
            }
            let fix = match f.fix {
                Some(fix) if !self.manifest.contributes.operations.iter().any(|o| o.name == fix.operation) => {
                    let what = fix.operation;
                    return Err(
                        self.misbehaved(format!("its finding {i} is fixed by {what:?}, which isn't its operation"))
                    );
                }
                fix => fix.map(|f| Fix { plugin: id.clone(), operation: f.operation, params: f.params }),
            };
            let playlist = f.finding.playlist.as_deref().map(|p| self.said(p));
            found.push((Finding { check: name.clone(), message, playlist, ..f.finding }, fix));
        }
        if total > MAX_FINDINGS {
            let said = format!("more than {MAX_FINDINGS} found, the rest left out");
            found.push((Finding::new(Level::Info, name, said), None));
        }
        Ok(found)
    }

    /// `text` as a finding says it: without control characters (see [`printable`]) or the
    /// secrets the host filled in for the plugin, and at most [`MAX_SAID`] characters.
    fn said(&self, text: &str) -> String {
        let text = self.net.scrub(&printable(text));
        match text.char_indices().nth(MAX_SAID) {
            Some((at, _)) => format!("{}...", &text[..at]),
            None => text,
        }
    }

    /// How its findings name `check`: `<plugin>.<check>`.
    fn named(&self, check: &Check) -> String {
        format!("{}.{}", self.manifest.id, check.name)
    }

    /// What running `check` within `timeout` came to: what it found, or why it found nothing
    /// (logged). Fails only when cancelled.
    fn ran(
        &self,
        check: &Check,
        timeout: Duration,
        ran: Result<Option<Found>>,
    ) -> Result<std::result::Result<Found, String>> {
        let why = match ran {
            Ok(Some(found)) => return Ok(Ok(found)),
            Err(Error::Cancelled) => return Err(Error::Cancelled),
            Ok(None) => format!("took longer than {}s, so it was stopped", timeout.as_secs_f64()),
            Err(e) => self.said(&e.to_string()),
        };
        self.host.log(&self.manifest.id, &format!("its check {}: {why}", check.name));
        Ok(Err(why))
    }
}

#[cfg(test)]
mod tests;
