//! The commands that, given no drive, take every mounted rekordbox drive: a read reads each one,
//! a write plans each, asks once and writes each.

use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{Result, bail};
use console::style;
use locallibrary::Store;
use locallibrary::api::{self, TargetError};
use locallibrary::op::{Operation, Planned};
use locallibrary::progress::Ctx;
use serde::Serialize;

use crate::output::{self, Outcome, Output, dim};

/// What a read found: on the drive it was given (or the only one mounted), or on each mounted
/// rekordbox drive.
pub enum Reads<T> {
    One(T),
    Each(Vec<DriveRead<T>>),
}

/// One drive's part of a read of every mounted drive: what was read, or why it couldn't be.
#[derive(Serialize)]
pub struct DriveRead<T> {
    pub drive: String,
    pub root: PathBuf,
    pub result: Option<T>,
    pub error: Option<TargetError>,
}

/// Read `drive`, or else every mounted rekordbox drive (only one mounted reads as if it were
/// named). A drive that can't be read doesn't stop the others: it's said on stderr, before any
/// pager starts.
pub fn read<T>(drive: Option<String>, read: impl Fn(String) -> locallibrary::Result<T>) -> Result<Reads<T>> {
    let mut mounted = match drive {
        Some(drive) => return Ok(Reads::One(read(drive)?)),
        None => api::drive_candidates(None),
    };
    if mounted.len() < 2 {
        // None mounted is a usage error, as `drive_or_default` makes it.
        let only = mounted.pop().ok_or_else(|| {
            let none =
                locallibrary::Error::NotFound("a mounted rekordbox drive (plug one in, or give its mount path)".into());
            crate::jobs_cmd::usage(none.to_string())
        })?;
        return Ok(Reads::One(read(only.root.display().to_string())?));
    }
    let mut reads = Vec::new();
    for d in mounted {
        let (result, error) = match read(d.root.display().to_string()) {
            Ok(r) => (Some(r), None),
            Err(locallibrary::Error::Cancelled) => return Err(locallibrary::Error::Cancelled.into()),
            Err(e) => {
                output::diag(format!("{} left out: {e}", d.name));
                (None, Some(e.into()))
            }
        };
        reads.push(DriveRead { drive: d.name, root: d.root, result, error });
    }
    Ok(Reads::Each(reads))
}

impl<T: Serialize> Reads<T> {
    /// Print what was read with `human`, which names the drive: one drive as reading it by name
    /// prints it, several one after another (`--json`: a list of [`DriveRead`]).
    pub fn emit(&self, out: Output, human: impl Fn(&T)) -> Outcome {
        self.print(out, false, human)
    }

    /// [`Reads::emit`] for a `human` that doesn't name the drive: of several, each is printed
    /// under its name.
    pub fn emit_headed(&self, out: Output, human: impl Fn(&T)) -> Outcome {
        self.print(out, true, human)
    }

    fn print(&self, out: Output, headed: bool, human: impl Fn(&T)) -> Outcome {
        let reads = match self {
            Reads::One(v) => {
                out.emit(v, human);
                return Outcome::Applied;
            }
            Reads::Each(reads) => reads,
        };
        out.emit(reads, |reads| {
            for (i, (r, v)) in reads.iter().filter_map(|r| Some((r, r.result.as_ref()?))).enumerate() {
                if i > 0 {
                    say!();
                }
                if headed {
                    say!("{}  {}", style(&r.drive).bold(), dim(r.root.display()));
                }
                human(v);
            }
        });
        self.outcome()
    }

    /// `Partial` when a drive couldn't be read.
    pub fn outcome(&self) -> Outcome {
        match self {
            Reads::Each(reads) if reads.iter().any(|r| r.error.is_some()) => Outcome::Partial,
            _ => Outcome::Applied,
        }
    }
}

/// The drives a write given none goes to: every mounted rekordbox drive but archive and retired
/// ones (a warning names those).
pub fn targets(store: Option<&Store>) -> Result<Vec<String>> {
    let (targets, left) = api::unnamed_targets(store);
    if let Some(w) = api::left_out(&left) {
        output::warn(w);
    }
    if targets.is_empty() {
        bail!("no rekordbox drives mounted");
    }
    Ok(targets.iter().map(|d| d.root().display().to_string()).collect())
}

/// Several drives' plans: each shown with `show`, one question ("`question` on N drive(s)?"), then
/// each drive with something to do written, `done` saying what it did. A drive that fails doesn't
/// stop the others. `--json` prints one document: the plans, and as the report a
/// `{"drive", "report"}` (or `"error"`) for each drive written.
pub fn apply_each<P: Operation + Serialize>(
    plans: Vec<(String, Planned<P, P::Request>)>,
    dry_run: bool,
    question: &str,
    out: Output,
    ctx: &Ctx<'_>,
    show: impl Fn(&str, &Planned<P, P::Request>),
    done: impl Fn(&str, &P::Report),
) -> Result<ExitCode> {
    if !out.json {
        for (drive, p) in &plans {
            show(drive, p);
        }
    }
    let shown: Vec<&Planned<P, P::Request>> = plans.iter().map(|(_, p)| p).collect();
    let pending = shown.iter().filter(|p| !p.noop).count();
    if pending == 0 {
        return Ok(out.not_applied(&shown, Outcome::NothingToDo));
    }
    if let Some(o) = out.decide(dry_run, &format!("{question} on {pending} drive(s)?")) {
        return Ok(out.not_applied(&shown, o));
    }
    let mut reports = Vec::new();
    for (drive, p) in plans.iter().filter(|(_, p)| !p.noop) {
        let r = p.apply(None, ctx);
        let json = match &r {
            Ok(r) => serde_json::json!({ "drive": drive, "report": r }),
            Err(e) => serde_json::json!({ "drive": drive, "error": { "kind": e.code(), "message": e.to_string() } }),
        };
        reports.push((drive, r, json));
    }
    let outcome = if reports.iter().any(|(_, r, _)| r.is_err()) { Outcome::Partial } else { Outcome::Applied };
    let json: Vec<&serde_json::Value> = reports.iter().map(|(_, _, j)| j).collect();
    Ok(out.applied(&shown, &json, outcome, || {
        for (drive, r, _) in &reports {
            match r {
                Ok(r) => done(drive, r),
                Err(e) => output::warn(format!("{drive}: {e}")),
            }
        }
    }))
}
