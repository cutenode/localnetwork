//! `track relocate`: finding the audio of tracks whose file is missing from a drive.

use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::Result;
use console::style;
use locallibrary::api;
use locallibrary::progress::Ctx;
use locallibrary::relocate::{Candidate, RelocateOptions, RelocatePlan};

use crate::output::{self, Outcome, Output, bytes, cap, dim, warn};

pub fn run(drive: String, folders: Vec<PathBuf>, dry_run: bool, out: Output, ctx: &Ctx<'_>) -> Result<ExitCode> {
    let folders = folders.iter().map(std::path::absolute).collect::<std::io::Result<_>>()?;
    let req = api::RelocateRequest { drive: drive.clone(), options: RelocateOptions { folders } };
    let plan = api::relocate_plan(&req, ctx)?;
    if !out.json {
        render(&plan);
        output::clones(&plan);
    }
    if plan.noop {
        return Ok(out.nothing(&plan, "no track can be relocated"));
    }
    if let Some(o) = out.decide(dry_run, "Relocate them?") {
        return Ok(out.not_applied(&plan, o));
    }
    let report = plan.apply(None, ctx)?;
    Ok(out.applied(&plan, &report, Outcome::Applied, || {
        output::ok(format!("relocated {} track(s)", plan.repointed.len() + plan.copied.len()));
        output::hint(format!("Undo with `locallibrary undo {drive:?}`."));
    }))
}

/// `Contents/a.mp3  8.1 MB, 5:02`
fn candidate(c: &Candidate) -> String {
    let length = c.duration_secs.map(|s| format!(", {}", locallibrary::probe::minutes(s))).unwrap_or_default();
    format!("{}  {}", c.path, dim(format!("{}{length}", bytes(c.size))))
}

fn render(p: &RelocatePlan) {
    say!("{}: {}", style(&p.drive).bold(), dim(p.root.display()));
    for r in &p.repointed {
        say!("    {} {}  {}", style("~").yellow(), r.track.name, dim(format!("now {}", r.file.path)));
    }
    for r in p.copied.iter().take(cap(25)) {
        say!("    {} {}  {}", style("+").green(), r.track.name, dim(format!("copied from {}", candidate(&r.file))));
    }
    if p.copied.len() > cap(25) {
        say!("    {}", dim(format!("... and {} more copied", p.copied.len() - 25)));
    }
    for (list, why) in [
        (&p.ambiguous, "more than one file fits, so it's left as it is:"),
        (&p.unfit, "no file of its name is as long as it, so it's left as it is:"),
    ] {
        for a in list {
            say!("    {} {}  {}", style("?").cyan(), a.track.name, dim(why));
            for c in &a.candidates {
                say!("        {}", candidate(c));
            }
        }
    }
    if let Some(t) = p.not_found.first() {
        say!("    {} {} track(s) with no file of their name, e.g. {}", style("-").red(), p.not_found.len(), t.name);
    }
    for w in &p.warnings {
        warn(w);
    }
}
