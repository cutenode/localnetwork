//! Terminal output: JSON or human-readable rendering, progress bars, confirmation prompts.

use std::borrow::Cow;
use std::io::{IsTerminal, Write};
use std::process::ExitCode;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use console::style;
use indicatif::{ProgressBar, ProgressStyle};
use locallibrary::op::Planned;
use locallibrary::progress::{Event, Phase, Progress};
use serde::Serialize;

#[derive(Debug, Clone, Copy)]
pub struct Output {
    pub json: bool,
    pub quiet: bool,
    pub yes: bool,
    /// A long listing may go through a pager (not with `--no-pager`).
    pub pager: bool,
}

/// A confirmation's answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Answer {
    Yes,
    No,
    /// Not asked: `--json`, or no terminal to ask on, and no `--yes`.
    NotAsked,
}

/// What a command came to. It decides the exit status, and `--json` output reports it as
/// `outcome`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Applied,
    NothingToDo,
    DryRun,
    /// The user said no.
    Declined,
    /// Not asked (`--json`, or no terminal) and no `--yes`.
    NotConfirmed,
    /// A check found problems (`doctor`, `check`, `verify` without `--repair`).
    Problems,
    /// Some of several drives failed.
    Partial,
}

impl Outcome {
    pub fn name(self) -> &'static str {
        match self {
            Outcome::Applied => "applied",
            Outcome::NothingToDo => "nothing_to_do",
            Outcome::DryRun => "dry_run",
            Outcome::Declined => "declined",
            Outcome::NotConfirmed => "not_confirmed",
            Outcome::Problems => "problems",
            Outcome::Partial => "partial",
        }
    }

    /// The exit status (see `locallibrary --help`): 0 done (or nothing to do, or a dry run), 3
    /// not applied, 4 problems found, 5 some of several failed.
    pub fn exit(self) -> ExitCode {
        ExitCode::from(self.code())
    }

    pub fn code(self) -> u8 {
        match self {
            Outcome::Applied | Outcome::NothingToDo | Outcome::DryRun => 0,
            Outcome::Declined | Outcome::NotConfirmed => 3,
            Outcome::Problems => 4,
            Outcome::Partial => 5,
        }
    }
}

/// The version of the shapes `--json` prints, in every JSON object it prints as `"schema"`.
/// Raised when a change would break a script written for the previous one.
pub const SCHEMA: u32 = 1;

/// What every command that writes prints with `--json`: `{"schema", "plan", "report",
/// "applied", "outcome"}`. `plan` is what was planned (`{"summary": [...]}` for a change made in
/// one step, see [`Output::one_step`]; `null` for one made without asking, see [`Output::done`]),
/// `report` what applying it did (`null` when it wasn't applied).
pub fn envelope(plan: &impl Serialize, report: &impl Serialize, applied: bool, outcome: Outcome) -> serde_json::Value {
    serde_json::json!({
        "plan": plan,
        "report": report,
        "applied": applied,
        "outcome": outcome.name(),
    })
}

/// `v` with `"schema"` in it when it's an object (a list has none).
pub fn with_schema(mut v: serde_json::Value) -> serde_json::Value {
    if let Some(map) = v.as_object_mut() {
        map.insert("schema".into(), SCHEMA.into());
    }
    v
}

/// A JSON document on stdout, with `"schema"` in it when it's an object.
pub fn print_json(v: serde_json::Value) {
    println!("{}", serde_json::to_string_pretty(&with_schema(v)).expect("serialisable"));
}

impl Output {
    /// Print a result: JSON in `--json` mode, otherwise run `human`.
    pub fn emit<T: Serialize>(&self, value: &T, human: impl FnOnce(&T)) {
        if self.json {
            print_json(serde_json::to_value(value).expect("serialisable"));
        } else {
            human(value);
        }
    }

    /// [`Output::emit`] for a listing that may run longer than the screen: in a terminal, the
    /// human-readable output goes through a pager (`$PAGER`, else `less`, which shows a listing
    /// that fits on one screen and quits).
    pub fn emit_paged<T: Serialize>(&self, value: &T, human: impl FnOnce(&T)) {
        self.page();
        self.emit(value, human);
    }

    /// What's printed from here on is a long listing: in a terminal, it goes through a pager.
    pub fn page(&self) {
        if !self.json && self.pager {
            crate::pager::start();
        }
    }

    /// Ask before a destructive step. `--yes` skips the prompt. JSON mode and non-interactive
    /// stdin never prompt: without `--yes` they decline, so nothing is written by accident.
    pub fn confirm(&self, question: &str) -> bool {
        self.ask(question) == Answer::Yes
    }

    /// [`Output::confirm`], telling a no from not being asked.
    pub fn ask(&self, question: &str) -> Answer {
        if self.yes {
            return Answer::Yes;
        }
        if self.json || !std::io::stdin().is_terminal() {
            if !self.json {
                eprintln!("{} not confirmed (pass --yes to run non-interactively)", style("!").yellow());
            }
            return Answer::NotAsked;
        }
        eprint!("{} {question} [y/N] ", style("?").cyan().bold());
        let _ = std::io::stderr().flush();
        let mut line = String::new();
        match std::io::stdin().read_line(&mut line).is_ok() && matches!(line.trim(), "y" | "Y" | "yes" | "YES") {
            true => Answer::Yes,
            false => Answer::No,
        }
    }

    /// Whether to go ahead with a planned change: `None` to go ahead, otherwise what the command
    /// came to (a dry run, or not confirmed).
    pub fn decide(&self, dry_run: bool, question: &str) -> Option<Outcome> {
        if dry_run {
            return Some(Outcome::DryRun);
        }
        match self.ask(question) {
            Answer::Yes => None,
            Answer::No => Some(Outcome::Declined),
            Answer::NotAsked => Some(Outcome::NotConfirmed),
        }
    }

    /// [`Output::decide`] about `planned`, refused without asking when a plugin's guard the user
    /// made blocking found something about it (it can't be applied).
    pub fn decide_on<P, R>(
        &self,
        planned: &Planned<P, R>,
        dry_run: bool,
        question: &str,
    ) -> locallibrary::Result<Option<Outcome>> {
        if !dry_run {
            planned.unblocked()?;
        }
        Ok(self.decide(dry_run, question))
    }

    /// A plan that wasn't applied: in `--json` mode, the envelope with `"report": null` and
    /// `"applied": false` (see [`envelope`]).
    pub fn not_applied<T: Serialize>(&self, plan: &T, outcome: Outcome) -> ExitCode {
        if self.json {
            print_json(envelope(plan, &serde_json::Value::Null, false, outcome));
        }
        outcome.exit()
    }

    /// A report (a check's, say): `--json` prints it with the outcome (and the schema) added;
    /// otherwise `human` runs. Returns the exit status.
    pub fn report<T: Serialize>(&self, value: &T, outcome: Outcome, human: impl FnOnce(&T)) -> ExitCode {
        if self.json {
            let mut v = serde_json::to_value(value).expect("serialisable");
            if let Some(map) = v.as_object_mut() {
                map.insert("outcome".into(), outcome.name().into());
            }
            print_json(v);
        } else {
            human(value);
        }
        outcome.exit()
    }

    /// A plan with nothing to do: `ok` says so, or `--json` prints it as not applied.
    pub fn nothing<T: Serialize>(&self, plan: &T, ok_message: impl std::fmt::Display) -> ExitCode {
        if !self.json {
            ok(ok_message);
        }
        self.not_applied(plan, Outcome::NothingToDo)
    }

    /// A plan that was applied: `--json` prints the envelope of the plan and its report;
    /// otherwise `human` runs.
    pub fn applied<P: Serialize, R: Serialize>(
        &self,
        plan: &P,
        report: &R,
        outcome: Outcome,
        human: impl FnOnce(),
    ) -> ExitCode {
        if self.json {
            print_json(envelope(plan, report, true, outcome));
        } else {
            human();
        }
        outcome.exit()
    }

    /// A change made in one step (a drive's role, a rule, a setting), which `change` says in a
    /// line: shown and asked about first, like every write (not made when declined, nor under
    /// `--json` or without a terminal unless `--yes`). Then `make` makes it and `human` shows
    /// what it came to; `--json` prints the line as the plan's `summary`.
    pub fn one_step<R: Serialize>(
        &self,
        change: &str,
        question: &str,
        make: impl FnOnce() -> anyhow::Result<R>,
        human: impl FnOnce(&R),
    ) -> anyhow::Result<ExitCode> {
        let plan = serde_json::json!({ "summary": [change] });
        if !self.json {
            summary(&[change.to_string()], &[]);
        }
        if let Some(o) = self.decide(false, question) {
            return Ok(self.not_applied(&plan, o));
        }
        let report = make()?;
        Ok(self.applied(&plan, &report, Outcome::Applied, || human(&report)))
    }

    /// A change made without a plan step (a snapshot pulled, a file written): `--json` prints
    /// the envelope with `"plan": null`; otherwise `human` runs.
    pub fn done<R: Serialize>(&self, report: &R, human: impl FnOnce(&R)) -> ExitCode {
        self.done_with(report, Outcome::Applied, human)
    }

    /// [`Output::done`], with how it came out (some of several failed, say).
    pub fn done_with<R: Serialize>(&self, report: &R, outcome: Outcome, human: impl FnOnce(&R)) -> ExitCode {
        if self.json {
            print_json(envelope(&serde_json::Value::Null, report, true, outcome));
        } else {
            human(report);
        }
        outcome.exit()
    }

    /// For erasing a disk: the user types `expected` (any case), or gave it as `--erase`. `--yes`
    /// doesn't skip it. `None` to go ahead, otherwise what the command came to.
    pub fn confirm_typed(&self, prompt: &str, expected: &str, given: Option<&str>) -> Option<Outcome> {
        if let Some(given) = given {
            if given.trim().eq_ignore_ascii_case(expected.trim()) {
                return None;
            }
            if !self.json {
                eprintln!("{} --erase {given:?} isn't {expected:?}; nothing was changed", style("!").yellow());
            }
            return Some(Outcome::Declined);
        }
        if self.json || !std::io::stdin().is_terminal() {
            if !self.json {
                eprintln!(
                    "{} not confirmed: type the name in a terminal, or pass --erase {expected:?}",
                    style("!").yellow()
                );
            }
            return Some(Outcome::NotConfirmed);
        }
        eprint!("{} {prompt} ", style("?").red().bold());
        let _ = std::io::stderr().flush();
        let mut line = String::new();
        let ok = std::io::stdin().read_line(&mut line).is_ok() && line.trim().eq_ignore_ascii_case(expected.trim());
        if !ok {
            eprintln!("{} not confirmed; nothing was changed", style("!").yellow());
            return Some(Outcome::Declined);
        }
        None
    }
    pub fn progress(&self) -> CliProgress {
        CliProgress { bar: Mutex::new(None), enabled: !self.json && !self.quiet && std::io::stderr().is_terminal() }
    }
}

// How this run prints, set once from the command line (see [`configure`]): every human-readable
// line goes through the helpers below, which read these.
static JSON: AtomicBool = AtomicBool::new(false);
static QUIET: AtomicBool = AtomicBool::new(false);
static ASCII: AtomicBool = AtomicBool::new(false);
static ALL: AtomicBool = AtomicBool::new(false);

/// When to colour the output (`--color`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum ColorWhen {
    /// When writing to a terminal, unless NO_COLOR is set
    Auto,
    Always,
    Never,
}

/// Set how this run prints: JSON or not, quiet (no progress, no hints), ASCII symbols instead of
/// ✓ ✗ ★ ↕ → (`--ascii`, or a locale that isn't UTF-8), and colour.
pub fn configure(json: bool, quiet: bool, ascii: bool, color: ColorWhen) {
    JSON.store(json, Ordering::Relaxed);
    QUIET.store(quiet, Ordering::Relaxed);
    ASCII.store(!json && (ascii || !utf8_locale()), Ordering::Relaxed);
    match color {
        ColorWhen::Auto => {}
        ColorWhen::Always | ColorWhen::Never => {
            console::set_colors_enabled(color == ColorWhen::Always);
            console::set_colors_enabled_stderr(color == ColorWhen::Always);
        }
    }
}

/// Whether the locale says the terminal takes UTF-8: the first of LC_ALL, LC_CTYPE and LANG
/// that's set; with none set, it's taken to (as macOS and Windows terminals do).
fn utf8_locale() -> bool {
    for var in ["LC_ALL", "LC_CTYPE", "LANG"] {
        if let Ok(v) = std::env::var(var)
            && !v.is_empty()
        {
            let v = v.to_ascii_lowercase();
            return v.contains("utf-8") || v.contains("utf8");
        }
    }
    true
}

/// `--quiet`: no progress bars and no hints.
pub fn quiet() -> bool {
    QUIET.load(Ordering::Relaxed)
}

/// List everything: lift the "... and N more" caps (`--all` on `doctor`, `drive verify`, `diff`).
pub fn show_all(all: bool) {
    ALL.store(all, Ordering::Relaxed);
}

/// How many of a list to show: `n`, or all of them with `--all`.
pub fn cap(n: usize) -> usize {
    if ALL.load(Ordering::Relaxed) { usize::MAX } else { n }
}

/// The symbols output uses, and what an ASCII terminal gets instead.
const SYMBOLS: &[(char, &str)] = &[
    ('✓', "v"),
    ('✗', "x"),
    ('✖', "x"),
    ('★', "*"),
    ('☆', "."),
    ('↕', "~"),
    ('→', "->"),
    ('←', "<-"),
    ('⇄', "<>"),
    ('•', "-"),
    ('…', "..."),
    ('×', "x"),
    ('·', "-"),
    ('–', "-"),
    ('—', "--"),
    ('±', "+/-"),
    ('≈', "~"),
    ('◆', "<>"),
    ('●', "*"),
    ('○', "o"),
    ('▁', "_"),
    ('▂', "."),
    ('▃', ":"),
    ('▄', "-"),
    ('▅', "="),
    ('▆', "+"),
    ('▇', "*"),
    ('█', "#"),
    ('▏', "|"),
    ('▎', "|"),
    ('▍', "|"),
    ('▌', "|"),
    ('▋', "|"),
    ('▊', "|"),
    ('▉', "|"),
];

/// `s` as the terminal can show it: with ASCII stand-ins for the symbols above when it isn't
/// UTF-8 (or `--ascii`). Letters in names are left as they are.
pub fn sym(s: &str) -> Cow<'_, str> {
    if !ASCII.load(Ordering::Relaxed) { Cow::Borrowed(s) } else { ascii(s) }
}

/// `s` with ASCII stand-ins for the symbols output uses.
fn ascii(s: &str) -> Cow<'_, str> {
    if s.is_ascii() {
        return Cow::Borrowed(s);
    }
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match SYMBOLS.iter().find(|(u, _)| *u == c) {
            Some((_, a)) => out.push_str(a),
            None => out.push(c),
        }
    }
    Cow::Owned(out)
}

/// `println!` for human-readable output: symbols as the terminal can show them.
macro_rules! say {
    () => {
        println!()
    };
    ($($arg:tt)*) => {
        println!("{}", $crate::output::sym(&format!($($arg)*)))
    };
}

/// A plan's summary lines and warnings.
pub fn summary(lines: &[String], warnings: &[String]) {
    for l in lines {
        say!("{} {l}", style("•").cyan());
    }
    for w in warnings {
        warn(w);
    }
}

/// For a plan shown its own way: the warnings it has besides `shown` (its own, said with it) and
/// those about its clones (see [`clones`]), such as what a plugin's guard says.
pub fn more_warnings<P, R>(plan: &Planned<P, R>, shown: &[String]) {
    let about_clones = |w: &String| plan.clones.iter().any(|c| c.left_alone.as_ref() == Some(w));
    for w in plan.warnings.iter().filter(|w| !shown.contains(w) && !about_clones(w)) {
        warn(w);
    }
}

/// For a plan shown its own way: the library's other clones applying it copies onto, and those
/// left alone (see `Planned::reaching_clones`).
pub fn clones<P, R>(plan: &Planned<P, R>) {
    if let Some(line) = plan.copied_onto() {
        say!("  {line}");
    }
    for why in plan.clones.iter().filter_map(|c| c.left_alone.as_ref()) {
        warn(why);
    }
}

pub fn heading(s: &str) {
    say!("{}", style(s).bold());
}

pub fn ok(s: impl std::fmt::Display) {
    say!("{} {s}", style("✓").green().bold());
}

/// A problem a report found: part of the report, so on stdout with it (on stderr with `--json`,
/// which keeps stdout for the JSON).
pub fn warn(s: impl std::fmt::Display) {
    let line = format!("{} {s}", style("!").yellow().bold());
    if JSON.load(Ordering::Relaxed) {
        eprintln!("{}", sym(&line));
    } else {
        say!("{line}");
    }
}

/// Something about the run rather than its result (a warning while working, a platform caveat):
/// on stderr, with the progress bars.
pub fn diag(s: impl std::fmt::Display) {
    eprintln!("{}", sym(&format!("{} {s}", style("!").yellow().bold())));
}

/// A tip: what to run next, how to undo. Left out with `--quiet`.
pub fn hint(s: impl std::fmt::Display) {
    hint_at("", s);
}

/// [`hint`], indented under what it's about.
pub fn hint_at(indent: &str, s: impl std::fmt::Display) {
    if !quiet() {
        say!("{indent}{}", dim(s));
    }
}

/// `s` in `width` columns of the terminal: cut short with an ellipsis, or padded with spaces.
/// Wide characters (CJK) take two columns, combining marks none.
pub fn fit(s: &str, width: usize) -> String {
    fit_with(s, width, if ASCII.load(Ordering::Relaxed) { "..." } else { "…" })
}

/// [`fit`], with `ellipsis` marking a cut.
pub fn fit_with(s: &str, width: usize, ellipsis: &str) -> String {
    use unicode_width::UnicodeWidthStr;
    let s = clip(s, width, ellipsis);
    let pad = width.saturating_sub(s.width());
    format!("{s}{}", " ".repeat(pad))
}

/// At most `max` columns of `s`, measured as a terminal (and ratatui) measures them: CJK and
/// emoji take two. Cut short, it ends in `ellipsis`; nothing is left when even that won't fit.
pub fn clip(s: &str, max: usize, ellipsis: &str) -> String {
    use unicode_width::UnicodeWidthStr;
    if s.width() <= max {
        return s.to_string();
    }
    let Some(room) = max.checked_sub(ellipsis.width()) else { return String::new() };
    // As much as fits beside the ellipsis, measured whole: some sequences (an emoji and its
    // presentation selector, say) are wider or narrower together than apart.
    let mut end = 0;
    for (i, c) in s.char_indices() {
        let next = i + c.len_utf8();
        if s[..next].width() > room {
            break;
        }
        end = next;
    }
    format!("{}{ellipsis}", &s[..end])
}

/// A cell filled from the bottom, an eighth at a time (`▁` to `█`).
pub const BLOCKS: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

/// A horizontal bar `frac` of `width` cells long, in eighths.
pub fn bar(frac: f64, width: usize) -> String {
    let eighths = (frac.clamp(0.0, 1.0) * width as f64 * 8.0).round() as usize;
    let mut s = "█".repeat(eighths / 8);
    if !eighths.is_multiple_of(8) {
        s.push(['▏', '▎', '▍', '▌', '▋', '▊', '▉'][eighths % 8 - 1]);
    }
    s
}

/// A share as a whole percentage: `0.42` is `42%`.
pub fn pct(x: f64) -> String {
    format!("{:.0}%", x * 100.0)
}

/// [`pct`], or `–` when there's no share to give.
pub fn opt_pct(x: Option<f64>) -> String {
    x.map_or_else(|| "–".to_string(), pct)
}

pub fn dim(s: impl std::fmt::Display) -> console::StyledObject<String> {
    style(s.to_string()).dim()
}

pub use locallibrary::op::bytes;

/// `m:ss`, or `h:mm:ss` past an hour, to the nearest second (59.6 s is `1:00`).
pub fn clock(secs: f64) -> String {
    let s = secs.max(0.0).round() as u64;
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, (s % 3600) / 60, s % 60)
    } else {
        format!("{}:{:02}", s / 60, s % 60)
    }
}

/// `1 track`, `3,886 tracks`.
pub fn plural(n: usize, one: &str) -> String {
    format!("{} {one}{}", thousands(n), if n == 1 { "" } else { "s" })
}

/// `3,886`.
pub fn thousands(n: usize) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

pub fn duration(secs: u64) -> String {
    let (h, m) = (secs / 3600, (secs % 3600) / 60);
    if h > 0 { format!("{h}h {m:02}m") } else { format!("{m}m {:02}s", secs % 60) }
}

/// Progress bars on stderr, driven by library events.
pub struct CliProgress {
    /// The current bar, and whether it counts items (true) or bytes (false).
    bar: Mutex<Option<(ProgressBar, bool)>>,
    enabled: bool,
}

fn phase_label(p: Phase) -> &'static str {
    match p {
        Phase::Scan => "Scanning",
        Phase::Hash => "Hashing",
        Phase::Copy => "Copying",
        Phase::Write => "Writing",
        Phase::Delete => "Deleting",
        Phase::Verify => "Verifying",
        Phase::Format => "Formatting",
        Phase::Fingerprint => "Fingerprinting",
        Phase::Index => "Indexing",
        Phase::Listen => "Listening",
        Phase::Match => "Matching",
        Phase::Analyze => "Analysing",
        _ => "Working",
    }
}

impl Progress for CliProgress {
    fn on_event(&self, event: &Event) {
        if let Event::Warning { message } = event {
            match &*self.bar.lock().unwrap() {
                Some((bar, _)) => bar.println(format!("{} {message}", style("!").yellow().bold())),
                None => diag(message),
            }
            return;
        }
        if !self.enabled {
            return;
        }
        let mut guard = self.bar.lock().unwrap();
        match event {
            Event::PhaseStart { phase, items, bytes } => {
                if let Some((old, _)) = guard.take() {
                    old.finish_and_clear();
                }
                let bar = if *bytes > 0 {
                    let b = ProgressBar::new(*bytes);
                    b.set_style(
                        ProgressStyle::with_template("{prefix:>9.cyan.bold} [{bar:30}] {decimal_bytes}/{decimal_total_bytes} {decimal_bytes_per_sec} {eta} {wide_msg:.dim}")
                            .unwrap()
                            .progress_chars("=> "),
                    );
                    b
                } else if *items > 0 {
                    let b = ProgressBar::new(*items);
                    b.set_style(
                        ProgressStyle::with_template(
                            "{prefix:>9.cyan.bold} [{bar:30}] {pos}/{len} {eta} {wide_msg:.dim}",
                        )
                        .unwrap()
                        .progress_chars("=> "),
                    );
                    b
                } else {
                    let b = ProgressBar::new_spinner();
                    b.set_style(
                        ProgressStyle::with_template("{prefix:>9.cyan.bold} {spinner} {wide_msg:.dim}").unwrap(),
                    );
                    b.enable_steady_tick(Duration::from_millis(100));
                    b
                };
                bar.set_prefix(phase_label(*phase));
                *guard = Some((bar, *bytes == 0 && *items > 0));
            }
            Event::Item { path, .. } => {
                if let Some((bar, counts_items)) = &*guard {
                    bar.set_message(path.rsplit('/').next().unwrap_or(path).to_string());
                    if *counts_items {
                        bar.inc(1);
                    }
                }
            }
            Event::Bytes { bytes, .. } => {
                if let Some((bar, false)) = &*guard {
                    bar.inc(*bytes);
                }
            }
            Event::PhaseEnd { .. } => {
                if let Some((bar, _)) = guard.take() {
                    bar.finish_and_clear();
                }
            }
            Event::Warning { .. } => unreachable!(),
            // Newer events than this front end knows.
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests;
