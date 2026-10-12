//! `locallibrary watch`: save drives into the store as they're plugged in.

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use anyhow::Result;
use clap::Args;
use console::style;
use locallibrary::api::{WatchAction, WatchOptions, Watched, Watching};
use locallibrary::progress::Ctx;
use locallibrary::watch::{VolumeWatcher, WatchEvent};
use locallibrary::{Error, Store};
use serde_json::json;

use crate::autostart;
use crate::output::{Output, dim, ok, with_schema};

#[derive(Args)]
#[command(after_long_help = "Examples:
  locallibrary watch                       say which drives hold changes the store lacks
  locallibrary watch --pull --sync-group   pull them, then sync the rest of their group
  locallibrary watch --pull --once         look at what's plugged in now, then stop
  locallibrary watch --install             pull drives as they're plugged in and sync the rest of their
                                           group, whenever you're logged in")]
pub struct WatchArgs {
    /// Pull drives holding changes the store lacks (without it, only say which)
    #[arg(long)]
    pull: bool,
    /// Then sync the library's other mounted drives (its drive group) from the drive, when it's the
    /// newest with confidence
    #[arg(long, alias = "sync-set")]
    sync_group: bool,
    /// Look at the drives mounted now (once they've settled), then stop
    #[arg(long)]
    once: bool,
    /// Seconds a drive must go unchanged before it's looked at (it may still be being written)
    #[arg(long, value_name = "SECS", default_value_t = SETTLE)]
    settle: u64,
    /// Run `watch --pull --sync-group` (with --settle as given) whenever you log in, starting now
    /// (macOS and Linux)
    #[arg(long, conflicts_with_all = ["once", "uninstall"])]
    install: bool,
    /// With --install: only pull, without syncing the rest of the group
    #[arg(long, requires = "install", conflicts_with = "sync_group")]
    no_sync_group: bool,
    /// Stop running it at log in
    #[arg(long, conflicts_with_all = ["pull", "sync_group", "once"])]
    uninstall: bool,
}

pub const SETTLE: u64 = 5;

/// The arguments `watch --install` runs this executable with: `watch --pull --sync-group` (a
/// library's drives are clones of it, so one plugged in with changes brings the others up to
/// date), the store when it's not the default, and the options given.
pub fn agent_args(store: &Option<PathBuf>, no_sync_group: bool, settle: u64) -> Result<Vec<String>> {
    let mut args = Vec::new();
    if let Some(s) = store {
        args.extend(["--store".into(), std::path::absolute(s)?.display().to_string()]);
    }
    args.extend(["watch".into(), "--pull".into()]);
    if !no_sync_group {
        args.push("--sync-group".into());
    }
    if settle != SETTLE {
        args.extend(["--settle".into(), settle.to_string()]);
    }
    Ok(args)
}

/// `watch --install`, also offered by `setup`: says what it did. Where the agent was written.
pub fn install(args: &[String], out: Output) -> Result<PathBuf> {
    let path = autostart::install(args)?;
    if !out.json {
        ok("Drives are now saved into the store as they're plugged in, whenever you're logged in");
        say!("  {}", dim(format!("log: {}; `locallibrary watch --uninstall` stops it", autostart::log_location())));
    }
    Ok(path)
}

/// Runs until Ctrl-C (or, with `--once`, until the mounted drives have been looked at). With
/// `--json`, each event is one line of JSON.
pub fn run(args: WatchArgs, store: &Option<PathBuf>, out: Output, ctx: &Ctx<'_>) -> Result<ExitCode> {
    if args.install {
        let command = agent_args(store, args.no_sync_group, args.settle)?;
        let path = install(&command, out)?;
        return Ok(out.done(&json!({ "installed": path, "command": command }), |_| {}));
    }
    if args.uninstall {
        let path = autostart::uninstall()?;
        return Ok(out.done(&json!({ "uninstalled": path }), |_| match path {
            Some(_) => ok("Drives are no longer saved as they're plugged in"),
            None => say!("`watch` wasn't set to run at log in"),
        }));
    }
    let store_root = store.clone().unwrap_or_else(Store::default_root);
    let options = WatchOptions { pull: args.pull, sync_set: args.sync_group, settle: Duration::from_secs(args.settle) };
    let mut watching = Watching::new(VolumeWatcher::new(), options);
    if !out.json && !args.once {
        say!(
            "{}",
            dim(format!(
                "Watching for drives ({}). Ctrl-C to stop.",
                if args.pull { "pulling them into the store" } else { "--pull to save them in the store" }
            ))
        );
    }
    let mut on = |w: Watched<'_>| log(out, w);
    let done = if args.once {
        watching.until_idle(&store_root, ctx, &mut on)
    } else {
        watching.run(&store_root, ctx, &|| false, &mut on)
    };
    match done {
        Ok(()) | Err(Error::Cancelled) => Ok(ExitCode::SUCCESS),
        Err(e) => Err(e.into()),
    }
}

fn log(out: Output, w: Watched<'_>) {
    if out.json {
        let line = match w {
            Watched::Seen(e) => serde_json::to_value(e),
            Watched::Did(a) => serde_json::to_value(a),
        };
        say!("{}", with_schema(line.expect("serialisable")));
        return;
    }
    let Some(text) = describe(w) else { return };
    let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S");
    say!("{} {text}", dim(now));
    if let Watched::Did(WatchAction::Pulled { history_lists, .. }) = w {
        crate::hints::new_sessions(history_lists.len());
    }
}

fn describe(w: Watched<'_>) -> Option<String> {
    let text = match w {
        Watched::Seen(WatchEvent::Attached(d)) => format!("{} plugged in", d.name),
        Watched::Seen(WatchEvent::Detached(d)) => format!("{} unplugged", d.name),
        Watched::Seen(_) => return None,
        Watched::Did(a) => {
            let name = &a.drive().name;
            match a {
                WatchAction::UpToDate { library, .. } => format!("{name}: the store holds it ({library})"),
                WatchAction::Pending { library, history_lists, beside, .. } => format!(
                    "{name}: holds changes {library} lacks{}{}; --pull saves them",
                    lists(history_lists),
                    if *beside { " (back from a set: it would be saved beside the newest snapshot)" } else { "" }
                ),
                WatchAction::Pulled { library, snapshot, history_lists, beside, .. } => format!(
                    "{} {name}: pulled into {library} as snapshot {}{}{}",
                    style("✓").green().bold(),
                    snapshot.short_id,
                    lists(history_lists),
                    if *beside { ", beside the newest snapshot (back from a set)" } else { "" }
                ),
                WatchAction::Synced { targets, .. } => {
                    let each: Vec<String> = targets
                        .iter()
                        .map(|t| match &t.error {
                            None => t.name.clone(),
                            Some(e) => format!("{} (failed: {})", t.name, e.message),
                        })
                        .collect();
                    format!("{} {name}: synced to {}", style("✓").green().bold(), each.join(", "))
                }
                WatchAction::Waiting { reason, .. } => format!("{name}: waiting: {reason}"),
                WatchAction::Skipped { reason, .. } => format!("{name}: left alone: {reason}"),
                WatchAction::Failed { message, .. } => format!("{} {name}: {message}", style("error:").red().bold()),
            }
        }
    };
    Some(text)
}

fn lists(names: &[String]) -> String {
    match names.len() {
        0 => String::new(),
        1 => format!(" (new history list {})", names[0]),
        n => format!(" ({n} new history lists: {})", names.join(", ")),
    }
}

#[cfg(test)]
mod tests;
