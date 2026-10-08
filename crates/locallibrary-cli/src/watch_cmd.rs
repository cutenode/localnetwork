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

use crate::output::{Output, dim, with_schema};

#[derive(Args)]
#[command(after_long_help = "Examples:
  locallibrary watch                       say which drives hold changes the store lacks
  locallibrary watch --pull --sync-group   pull them, then sync the rest of their group
  locallibrary watch --pull --once         look at what's plugged in now, then stop")]
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
    #[arg(long, value_name = "SECS", default_value_t = 5)]
    settle: u64,
}

/// Runs until Ctrl-C (or, with `--once`, until the mounted drives have been looked at). With
/// `--json`, each event is one line of JSON.
pub fn run(args: WatchArgs, store: &Option<PathBuf>, out: Output, ctx: &Ctx<'_>) -> Result<ExitCode> {
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
