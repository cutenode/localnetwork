//! `locallibrary library ...`: the libraries in the store, their snapshot history, writing a
//! snapshot onto a drive (`push`) and pruning old snapshots.

use indexmap::IndexMap;
use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::Result;
use clap::{Args, Subcommand};
use console::style;
use locallibrary::Store;
use locallibrary::api::{self, LibraryListing};
use locallibrary::local_time;
use locallibrary::progress::Ctx;

use crate::output::{self, Outcome, Output, dim};
use crate::{DRIVE_HELP, SyncFlags, open_store, render};

/// `library`: with no subcommand, the libraries; with a library, its snapshot history.
#[derive(Args)]
#[command(args_conflicts_with_subcommands = true)]
pub struct LibraryArgs {
    #[command(subcommand)]
    command: Option<LibraryCommand>,
    /// A library whose snapshot history to show, as `library log` does [default: every library,
    /// as `library ls` lists them]
    #[arg(value_name = "LIBRARY")]
    library: Option<String>,
}

#[derive(Subcommand)]
pub enum LibraryCommand {
    /// Every library: its snapshots, latest snapshot and drives
    #[command(visible_alias = "list")]
    Ls,
    /// Rename a library (drives and snapshots follow it)
    Rename {
        /// The library's name or id
        library: String,
        /// Its new name
        name: String,
    },
    /// Snapshot history of a library (or of every library), with labels
    Log(LogArgs),
    /// Write a stored snapshot (the library's latest by default) onto a drive
    #[command(after_long_help = PUSH_LONG_HELP)]
    Push(PushArgs),
    /// Remove old snapshots, keeping each library's latest, labelled ones, drives' last states
    /// and history lists; then free the files nothing uses (as `store prune`)
    Prune(PruneArgs),
}

const PUSH_LONG_HELP: &str = "Examples:
  locallibrary library push BACKUP                          the latest snapshot of the drive's library
  locallibrary library push BACKUP --snapshot \"before NYE\"  a labelled snapshot
  locallibrary library push /Volumes/NEW -l Home --snapshot 3f9a2c";

/// `library log` (and `log`).
#[derive(Args)]
pub struct LogArgs {
    /// Library name [default: all libraries]
    library: Option<String>,
    /// Show at most this many snapshots of each library, newest first
    #[arg(long, short = 'n')]
    limit: Option<usize>,
}

/// `library push` (and `push`).
#[derive(Args)]
pub struct PushArgs {
    #[arg(help = DRIVE_HELP)]
    drive: String,
    /// The library to write [default: the drive's]
    #[arg(long, short)]
    library: Option<String>,
    /// Snapshot id (prefix) or label to restore instead of the latest
    #[arg(long)]
    snapshot: Option<String>,
    /// Overwrite even if the drive holds changes the store doesn't have
    #[arg(long)]
    force: bool,
    #[command(flatten)]
    opts: SyncFlags,
}

/// `library prune` and `store prune`.
#[derive(Args)]
pub struct PruneArgs {
    /// Keep each library's newest N snapshots
    #[arg(long, value_name = "N")]
    keep_last: Option<usize>,
    /// Keep the newest snapshot of each of the last N days that have one
    #[arg(long, value_name = "N")]
    keep_daily: Option<usize>,
    /// Keep the newest snapshot of each of the last N weeks that have one
    #[arg(long, value_name = "N")]
    keep_weekly: Option<usize>,
    /// Only this library
    #[arg(long, short)]
    library: Option<String>,
    /// Show what would go without removing anything
    #[arg(long)]
    dry_run: bool,
}

impl PruneArgs {
    pub fn options(self) -> api::PruneOptions {
        let PruneArgs { keep_last, keep_daily, keep_weekly, library, dry_run } = self;
        api::PruneOptions { keep_last, keep_daily, keep_weekly, library, dry_run }
    }
}

pub fn run(args: LibraryArgs, store_path: &Option<PathBuf>, out: Output, ctx: &Ctx<'_>) -> Result<ExitCode> {
    let command = match (args.command, args.library) {
        (Some(c), _) => c,
        (None, Some(library)) => {
            if open_store(store_path)?.find_library(&library)?.is_none() {
                crate::help::mistyped_subcommand("library", &library, "library")?;
            }
            LibraryCommand::Log(LogArgs { library: Some(library), limit: None })
        }
        (None, None) => LibraryCommand::Ls,
    };
    match command {
        LibraryCommand::Ls => {
            let libs = api::libraries_list(&open_store(store_path)?)?;
            out.emit(&libs, |l| render_list(l));
        }
        LibraryCommand::Rename { library, name } => {
            let store = open_store(store_path)?;
            let change = format!("rename the library {library:?} to {name:?}");
            let req = api::RenameLibrary { library: library.clone(), name };
            return out.one_step(
                &change,
                "Rename it?",
                || Ok(api::library_rename(&store, &req)?),
                |l| output::ok(format!("{library} is now {}", l.name)),
            );
        }
        LibraryCommand::Log(a) => return log(a, &open_store(store_path)?, out),
        LibraryCommand::Push(a) => return push(a, store_path, out, ctx),
        LibraryCommand::Prune(a) => return crate::store_cmd::prune(&open_store(store_path)?, a.options(), out, ctx),
    }
    Ok(ExitCode::SUCCESS)
}

/// `library log`: snapshot history.
pub fn log(a: LogArgs, store: &Store, out: Output) -> Result<ExitCode> {
    let libs = match a.library {
        Some(l) => vec![l],
        None => store.libraries()?.into_iter().map(|l| l.name).collect(),
    };
    let mut all = IndexMap::new();
    for l in libs {
        all.insert(l.clone(), api::history(store, &api::HistoryRequest { library: l, limit: a.limit })?);
    }
    all.sort_unstable_keys();
    // Under a key of its own: a library's name can be anything, `schema` too.
    out.emit_paged(&serde_json::json!({ "libraries": all }), |_| render::log(&all));
    Ok(ExitCode::SUCCESS)
}

/// `library push`: a stored snapshot onto a drive.
pub fn push(a: PushArgs, store_path: &Option<PathBuf>, out: Output, ctx: &Ctx<'_>) -> Result<ExitCode> {
    let PushArgs { drive, library, snapshot, force, opts } = a;
    let store = open_store(store_path)?;
    let req = api::PushRequest {
        drive,
        library,
        snapshot,
        force,
        verify: opts.verify,
        keep_extra: opts.keep_extra,
        delete_first: opts.delete_first,
    };
    let preview = api::push_plan(&store, &req, ctx)?;
    if preview.noop {
        let message = format!("drive already matches snapshot {}", preview.snapshot.short_id);
        return Ok(out.nothing(&preview, message));
    }
    if !out.json {
        say!(
            "Snapshot {} ({} tracks, {})",
            preview.snapshot.short_id,
            preview.snapshot.summary.tracks,
            local_time(preview.snapshot.created, "%Y-%m-%d %H:%M")
        );
        render::sync_plan(&preview.plan.plan);
    }
    if let Some(o) = out.decide(opts.dry_run, "Write this to the drive?") {
        return Ok(out.not_applied(&preview, o));
    }
    let report = preview.apply(Some(&store), ctx)?;
    let code = out.applied(&preview, &report, Outcome::Applied, || render::sync_report(&report));
    if opts.eject {
        crate::drive_cmd::eject_after(store_path, vec![req.drive], out);
    }
    Ok(code)
}

fn render_list(libs: &[LibraryListing]) {
    if libs.is_empty() {
        say!("The store has no libraries yet. Snapshot a drive with `locallibrary pull <drive>`.");
    }
    for l in libs {
        let head = match &l.head {
            Some(h) => format!("latest {} ({})", h.short_id, local_time(h.created, "%Y-%m-%d %H:%M")),
            None => "no snapshots".into(),
        };
        say!("{}  {}", style(&l.name).bold(), dim(format!("{} snapshot(s), {head}", l.snapshots)));
        if !l.drives.is_empty() {
            say!("  {}", dim(format!("drives: {}", l.drives.join(", "))));
        }
    }
}
