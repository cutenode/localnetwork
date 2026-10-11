//! `track upgrade`: pointing lossy tracks at lossless files of their recording.

use std::process::ExitCode;

use anyhow::Result;
use console::style;
use locallibrary::api;
use locallibrary::progress::Ctx;
use locallibrary::upgrade::{UpgradeOptions, UpgradePlan};

use crate::output::{self, Outcome, Output, bytes, dim, warn};

pub fn run(drive: String, options: UpgradeOptions, dry_run: bool, out: Output, ctx: &Ctx<'_>) -> Result<ExitCode> {
    let folders = options.folders.iter().map(std::path::absolute).collect::<std::io::Result<_>>()?;
    let req = api::UpgradeRequest { drive: drive.clone(), options: UpgradeOptions { folders, ..options } };
    let plan = api::upgrade_plan(&req, ctx)?;
    if !out.json {
        render(&plan);
        output::clones(&plan);
    }
    if plan.noop {
        return Ok(out.nothing(&plan, "no track has a lossless file of its recording in those folders"));
    }
    if let Some(o) = out.decide(dry_run, "Upgrade them?") {
        return Ok(out.not_applied(&plan, o));
    }
    let report = plan.apply(None, ctx)?;
    Ok(out.applied(&plan, &report, Outcome::Applied, || {
        output::ok(format!("upgraded {} track(s)", plan.upgrades.len()));
        output::hint(format!("Undo with `locallibrary undo {drive:?}`; `locallibrary edits replay` makes it again after rekordbox exports."));
    }))
}

fn render(p: &UpgradePlan) {
    say!("{}: {}", style(&p.drive).bold(), dim(p.root.display()));
    for u in &p.upgrades {
        let copied = if u.on_drive { "already on the drive".to_string() } else { bytes(u.size) };
        let shift = format!("cues and grid {:+} ms, {copied}", u.shift_ms);
        say!("    {} {}  {}  {}", style("~").yellow(), u.track.name, u.to, dim(shift));
    }
    for n in &p.not_upgraded {
        say!("    {} {}  {}", style("-").red(), n.track.name, dim(format!("{}: {}", n.file.display(), n.why)));
    }
    if p.unmatched > 0 {
        say!("    {}", dim(format!("{} lossy track(s) have no lossless file of their recording there", p.unmatched)));
    }
    for k in &p.removed {
        say!("    {} {}", style("x").red(), dim(k));
    }
    for w in &p.warnings {
        warn(w);
    }
}
