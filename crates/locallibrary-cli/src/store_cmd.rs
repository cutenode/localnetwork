//! `pull` and the store's upkeep: `store label|unlabel|prune|copy|copies`.

use std::io::IsTerminal;
use std::process::ExitCode;

use anyhow::Result;
use console::style;
use locallibrary::Store;
use locallibrary::api;
use locallibrary::progress::Ctx;

use crate::output::{self, Outcome, Output};
use crate::render;

/// `pull`: an unknown `--library` name is asked about in a terminal, and refused elsewhere (and
/// with `--yes`, so a script's typo doesn't start a library) unless `--new-library`.
pub fn pull(store: &Store, mut req: api::PullRequest, out: Output, ctx: &Ctx<'_>) -> Result<ExitCode> {
    if let Some(name) = &req.library
        && !req.new_library
        && store.find_library(name)?.is_none()
        && !out.json
        && !out.yes
        && std::io::stdin().is_terminal()
    {
        let known: Vec<String> = store.libraries()?.into_iter().map(|l| format!("{:?}", l.name)).collect();
        let others = match known.is_empty() {
            true => String::new(),
            false => format!(" (the store has {})", known.join(", ")),
        };
        req.new_library = out.confirm(&format!("There's no library named {name:?}{others}. Start a new one?"));
    }
    let r = api::pull(store, &req, ctx)?;
    Ok(out.done(&r, render::pull))
}

/// `store prune`: the plan, then (confirmed) exactly that prune.
pub fn prune(store: &Store, opts: api::PruneOptions, out: Output, ctx: &Ctx<'_>) -> Result<ExitCode> {
    let plan = api::store_prune_plan(store, &opts, ctx)?;
    if plan.noop {
        if !out.json {
            for w in &plan.warnings {
                output::warn(w);
            }
        }
        return Ok(out.nothing(&plan, "nothing to prune: every snapshot is kept"));
    }
    if !out.json {
        render_prune(&plan);
    }
    if let Some(o) = out.decide(opts.dry_run, &format!("Remove {} snapshot(s)?", plan.removed_snapshots)) {
        return Ok(out.not_applied(&plan, o));
    }
    let report = plan.apply(Some(store), ctx)?;
    Ok(out.applied(&plan, &report, Outcome::Applied, || {
        output::ok(format!(
            "removed {} snapshot(s); freed {} ({} file(s))",
            report.removed_snapshots,
            output::bytes(report.freed_bytes),
            report.removed_objects
        ))
    }))
}

fn render_prune(p: &api::PruneReport) {
    use api::KeepReason as R;
    for lib in &p.libraries {
        output::heading(&format!("{}: keeping {}, removing {}", lib.library, lib.kept.len(), lib.removed.len()));
        let mut lines: Vec<(chrono::DateTime<chrono::Utc>, String)> = Vec::new();
        for k in &lib.kept {
            let why: Vec<String> = k
                .reasons
                .iter()
                .map(|r| match r {
                    R::Latest => "latest".into(),
                    R::Label => format!("labelled {}", k.snapshot.labels.join(", ")),
                    R::Drive => format!("{} last seen holding it", k.drives.join(", ")),
                    R::History => format!("holds {}", k.history.join(", ")),
                    R::Unreadable => "couldn't be read".into(),
                    R::Last => "among the newest".into(),
                    R::Daily => "newest that day".into(),
                    R::Weekly => "newest that week".into(),
                })
                .collect();
            lines.push((
                k.snapshot.created,
                format!("  {} {}  {}", style("+").green(), line(&k.snapshot), output::dim(why.join("; "))),
            ));
        }
        for s in &lib.removed {
            lines.push((s.created, format!("  {} {}", style("-").red(), line(s))));
        }
        lines.sort_by_key(|(created, _)| std::cmp::Reverse(*created));
        for (_, l) in lines {
            say!("{l}");
        }
    }
    for w in &p.warnings {
        output::warn(w);
    }
    say!(
        "{}",
        output::dim(format!(
            "Frees {} ({} file(s) no snapshot left uses).",
            output::bytes(p.freed_bytes),
            p.removed_objects
        ))
    );
}

fn line(s: &locallibrary::store::SnapshotInfo) -> String {
    format!(
        "{}  {}  {:>5} tracks{}",
        style(&s.short_id).yellow(),
        s.created.with_timezone(&chrono::Local).format("%Y-%m-%d %H:%M"),
        s.summary.tracks,
        s.message.as_ref().map(|m| format!("  {}", output::dim(m))).unwrap_or_default()
    )
}

/// `store copy`: with `--mirror`, what would be removed from the copy is asked about first.
pub fn copy(store: &Store, mut req: api::StoreCopyRequest, out: Output, ctx: &Ctx<'_>) -> Result<ExitCode> {
    let dry_run = req.options.dry_run;
    // With --mirror (or --dry-run), what would be copied and removed is worked out first.
    let mut planned = None;
    if req.options.mirror || dry_run {
        req.options.dry_run = true;
        let plan = api::store_copy(store, &req, ctx)?;
        let removes = plan.removed_snapshots + plan.removed_objects > 0;
        if !out.json && (dry_run || removes) {
            render_copy_plan(&plan, removes);
        }
        if dry_run {
            return Ok(out.not_applied(&plan, Outcome::DryRun));
        }
        if removes && let Some(o) = out.decide(false, "Remove them from the copy?") {
            return Ok(out.not_applied(&plan, o));
        }
        req.options.dry_run = false;
        planned = Some(plan);
    }
    let r = api::store_copy(store, &req, ctx)?;
    let outcome = if r.complete { Outcome::Applied } else { Outcome::Partial };
    Ok(out.applied(&planned, &r, outcome, || {
        output::ok(format!(
            "copied {} file(s) ({}) and {} snapshot(s) to {}; {} file(s) were there already",
            r.objects_copied,
            output::bytes(r.bytes_copied),
            r.snapshots_copied,
            r.path.display(),
            r.objects_present
        ));
        if r.objects_repaired > 0 {
            output::warn(format!("{} file(s) in the copy had changed and were copied again", r.objects_repaired));
        }
        if r.removed_snapshots + r.removed_objects > 0 {
            say!(
                "{}",
                output::dim(format!(
                    "removed {} snapshot(s) and {} file(s) from the copy",
                    r.removed_snapshots, r.removed_objects
                ))
            );
        }
        for p in &r.problems {
            output::warn(p);
        }
    }))
}

fn render_copy_plan(p: &api::CopyReport, removes: bool) {
    output::heading(&format!("Copying the store to {}", p.path.display()));
    say!(
        "  {} new file(s), {}; {} snapshot(s) of {}; {} file(s) there already",
        p.objects_copied,
        output::bytes(p.bytes_copied),
        p.snapshots_copied,
        p.snapshots,
        p.objects_present
    );
    if removes {
        say!(
            "  {} {} snapshot(s) and {} file(s) the store no longer has",
            style("-").red(),
            p.removed_snapshots,
            p.removed_objects
        );
    }
}

/// `store copies`.
pub fn copies(store: &Store, out: Output) -> Result<ExitCode> {
    let copies = api::store_copies(store)?;
    out.emit(&copies, |copies| {
        if copies.is_empty() {
            say!("No copies yet. Make one with `locallibrary store copy <dir>`.");
        }
        for c in copies {
            let r = &c.record;
            let state = match (c.available, c.up_to_date, r.finished) {
                (false, _, _) => style("not there (unmounted?)".to_string()).yellow(),
                (true, _, None) => style("stopped part way: run `store copy` again".to_string()).yellow(),
                (true, Some(true), _) if r.problems.is_empty() => style("up to date".to_string()).green(),
                (true, _, _) => style("behind".to_string()).yellow(),
            };
            say!("{}  {state}", style(r.path.display()).bold());
            let when = |t: Option<chrono::DateTime<chrono::Utc>>| {
                t.map(|t| t.format("%Y-%m-%d %H:%M").to_string()).unwrap_or_else(|| "never".into())
            };
            say!(
                "  {}",
                output::dim(format!(
                    "last complete {}, {} snapshot(s), {} in {} file(s)",
                    when(r.last_complete),
                    r.snapshots,
                    output::bytes(r.bytes),
                    r.objects
                ))
            );
            for p in &r.problems {
                output::warn(p);
            }
        }
    });
    Ok(ExitCode::SUCCESS)
}

/// `store label` and `store unlabel`.
pub fn label(store: &Store, req: api::LabelRequest, out: Output) -> Result<ExitCode> {
    let change = match &req.snapshot {
        Some(s) if !req.remove => format!("label snapshot {s} {:?}", req.label),
        _ => format!("take the label {:?} off its snapshot", req.label),
    };
    out.one_step(
        &change,
        "Go ahead?",
        || Ok(api::snapshot_label(store, &req)?),
        |l| match req.remove {
            true => output::ok(format!("took {:?} off snapshot {}", l.name, l.snapshot.short())),
            false => output::ok(format!("snapshot {} is {:?}", l.snapshot.short(), l.name)),
        },
    )
}
