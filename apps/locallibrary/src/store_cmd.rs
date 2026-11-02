//! `pull` and the store's upkeep: `store label|unlabel|prune|copy|copies|restore|serve|fetch`.

use std::io::IsTerminal;
use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::Result;
use console::style;
use locallibrary::Store;
use locallibrary::api;
use locallibrary::catalog::{FetchReport, LibraryOutcome};
use locallibrary::local_time;
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
    Ok(out.done(&r, |r| {
        render::pull(r);
        if let Some(s) = r.snapshot.as_ref().filter(|_| !r.up_to_date) {
            let parent = s.parents.first().map(ToString::to_string);
            crate::hints::new_sessions(crate::hints::saved_sessions(store, [(s.id.to_string(), parent)]));
        }
    }))
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
        local_time(s.created, "%Y-%m-%d %H:%M"),
        s.summary.tracks,
        s.message.as_ref().map(|m| format!("  {}", output::dim(m))).unwrap_or_default()
    )
}

/// `store copy DIR`: with `--mirror`, what would be removed from the copy is asked about first.
pub fn copy(store: &Store, req: api::StoreCopyRequest, out: Output, ctx: &Ctx<'_>) -> Result<ExitCode> {
    let copy = |options: &api::CopyOptions| {
        let req = api::StoreCopyRequest { path: req.path.clone(), options: options.clone() };
        Ok(api::store_copy(store, &req, ctx)?)
    };
    copied(req.options, copy, out)
}

/// `store copy --to PLUGIN:DESTINATION`.
pub fn copy_to(store: &Store, spec: &str, options: api::CopyOptions, out: Output, ctx: &Ctx<'_>) -> Result<ExitCode> {
    crate::account_cmd::access::ask(spec, store, out, ctx)?;
    let dest = crate::plugins::host()?.store_destination(spec, Some(store))?;
    copied(options, |options| Ok(locallibrary::offsite::copy_to(store, &dest, options, ctx)?), out)
}

/// A copy made with `copy`: with `--mirror` (or `--dry-run`), what would be copied and removed is
/// worked out first, and removing anything asked about.
fn copied(
    mut options: api::CopyOptions,
    copy: impl Fn(&api::CopyOptions) -> Result<api::CopyReport>,
    out: Output,
) -> Result<ExitCode> {
    let dry_run = options.dry_run;
    let mut planned = None;
    if options.mirror || dry_run {
        options.dry_run = true;
        let plan = copy(&options)?;
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
        options.dry_run = false;
        planned = Some(plan);
    }
    let r = copy(&options)?;
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
pub fn copies(store: &Option<PathBuf>, out: Output) -> Result<ExitCode> {
    let copies = crate::diff_cmd::read_or_empty(store, api::store_copies)?;
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
                t.map(|t| local_time(t, "%Y-%m-%d %H:%M").to_string()).unwrap_or_else(|| "never".into())
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

/// `store serve`: the code to give, then a line for each computer that fetches (`--json`: one JSON
/// object per event), until Ctrl-C.
#[cfg(feature = "peers")]
pub fn serve(store: &Store, opts: &locallibrary::peers::ServeOptions, out: Output, ctx: &Ctx<'_>) -> Result<ExitCode> {
    use locallibrary::peers::ServeEvent;
    let report = locallibrary::peers::serve(store, opts, ctx, &mut |e| {
        if out.json {
            say!("{}", output::with_schema(serde_json::to_value(e).expect("serialisable")));
            return;
        }
        match e {
            ServeEvent::Ready { code } => {
                say!("On the other computer, run:\n\n  locallibrary store fetch {code}\n");
                output::hint("Serving until Ctrl-C. Anyone with the code can read the store while it's served.");
            }
            ServeEvent::Connected { peer } => output::ok(format!("{peer} connected")),
            ServeEvent::Refused { peer } => output::warn(format!("{peer} gave the wrong code and was turned away")),
            ServeEvent::Done { peer, snapshots, files, bytes } => output::ok(format!(
                "{peer} fetched {snapshots} snapshot(s) and {files} file(s) ({})",
                output::bytes(*bytes)
            )),
        }
    })?;
    Ok(out.done(&report, |_| {}))
}

/// `store fetch`.
#[cfg(feature = "peers")]
pub fn fetch(
    store: &Store,
    code: &str,
    opts: &locallibrary::peers::FetchOptions,
    out: Output,
    ctx: &Ctx<'_>,
) -> Result<ExitCode> {
    let r = locallibrary::peers::fetch(store, &code.parse()?, opts, ctx)?;
    Ok(fetched(&r, "the other computer", "on both computers", out))
}

/// `store restore --from PLUGIN:DESTINATION`.
pub fn restore(
    store: &Store,
    spec: &str,
    opts: &locallibrary::offsite::RestoreOptions,
    out: Output,
    ctx: &Ctx<'_>,
) -> Result<ExitCode> {
    crate::account_cmd::access::ask(spec, store, out, ctx)?;
    let dest = crate::plugins::host()?.store_destination(spec, Some(store))?;
    let r = locallibrary::offsite::restore(store, &dest, opts, ctx)?;
    Ok(fetched(&r, "the copy", "here and in the copy", out))
}

/// What `store fetch` and `store restore` brought in, from `there` (`the other computer`), and
/// each library that changed `both` places.
fn fetched(r: &FetchReport, there: &str, both: &str, out: Output) -> ExitCode {
    let outcome = if r.dry_run { Outcome::DryRun } else { Outcome::Applied };
    out.report(r, outcome, |r| {
        let verb = if r.dry_run { "would copy" } else { "copied" };
        output::ok(format!(
            "{verb} {} snapshot(s) and {} file(s) ({}) from {}; {} file(s) were here already",
            r.snapshots,
            r.files,
            output::bytes(r.bytes),
            r.peer,
            r.present
        ));
        for l in &r.libraries {
            let name = style(&l.name).bold();
            match (l.outcome, &l.was) {
                (LibraryOutcome::Added, Some(was)) => {
                    say!("  {name}: new here (called {was:?} there, a name this store uses)")
                }
                (LibraryOutcome::Added, None) => say!("  {name}: new here"),
                (LibraryOutcome::Updated, _) => say!("  {name}: now at {there}'s latest snapshot"),
                (LibraryOutcome::UpToDate, _) => say!("  {name}: up to date"),
                (LibraryOutcome::Unavailable, _) => output::warn(format!(
                    "{}: {there}'s latest snapshot couldn't be had, so it's left as it was",
                    l.name
                )),
                (LibraryOutcome::Diverged, _) => output::warn(format!(
                    "{} changed {both}: it keeps this store's latest; push the other's with \
                     `locallibrary library push --snapshot {}`",
                    l.name,
                    l.head.map(|h| h.short()).unwrap_or_default()
                )),
            }
        }
    })
}
