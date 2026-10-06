//! `locallibrary plays ...`: the history lists players recorded (on the drive and archived in the
//! store), play counts, writing a list out and publishing one. `history` and `stats` are the old
//! names of `plays lists` and `plays stats`.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, Result, bail};
use clap::{Args, Subcommand};
use console::style;
use locallibrary::Store;
use locallibrary::api;
use locallibrary::export::{self, Format};
use locallibrary::model::Track;
use locallibrary::plays::{self, DriveHistory, HistoryArchive, HistoryList, PlayStats};
use locallibrary::progress::Ctx;
use serde::Serialize;

use crate::DRIVE_HELP;
use crate::each_drive::{self, Reads};
use crate::export_cmd::{FormatArg, PathsArg};
use crate::output::{Output, dim, duration, heading, ok, warn};

/// `plays`: with a drive and no subcommand, its history lists.
#[derive(Args)]
#[command(args_conflicts_with_subcommands = true)]
pub struct PlaysArgs {
    #[command(subcommand)]
    command: Option<PlaysCommand>,
    /// A drive whose history lists to list, as `plays lists` does [default: every mounted
    /// rekordbox drive]
    #[arg(value_name = "DRIVE")]
    drive: Option<String>,
}

#[derive(Subcommand)]
enum PlaysCommand {
    /// History lists players recorded on a drive (--all: also every one the store kept), or one
    /// of them
    #[command(visible_alias = "history", after_long_help = LISTS_LONG_HELP)]
    Lists(ListsArgs),
    /// Play counts: most played, never played, and what was played since a snapshot or a date
    Stats(StatsArgs),
    /// Write a history list to a file: text, CSV or M3U8
    Export(PlaysExportArgs),
    /// Publish a history list to your Atmosphere account as a set: its tracks in order, for
    /// anyone to read
    #[cfg(feature = "account")]
    Publish(HistoryPublishArgs),
}

const LISTS_LONG_HELP: &str = "Examples:
  locallibrary plays lists Main                     the lists on the drive
  locallibrary plays lists Main --all               and every one the store kept
  locallibrary plays lists Main \"HISTORY 012\"       one list's tracks
  locallibrary plays lists Main --delete --dry-run  what clearing the drive's lists would remove";

/// `history`: `plays lists`, with `history publish` for `plays publish`.
#[derive(Args)]
#[command(args_conflicts_with_subcommands = true)]
pub struct HistoryArgs {
    #[cfg(feature = "account")]
    #[command(subcommand)]
    command: Option<HistoryCommand>,
    #[command(flatten)]
    lists: ListsArgs,
}

/// `plays lists`.
#[derive(Args)]
pub struct ListsArgs {
    #[arg(help = crate::DEFAULT_DRIVE_HELP)]
    drive: Option<String>,
    /// A history list (name or path): print its tracks, or write them with -o
    list: Option<String>,
    /// Also every list archived in the store (rekordbox deletes lists from the drive when it
    /// imports them; `locallibrary pull` kept them)
    #[arg(long)]
    all: bool,
    /// Write the list to a file
    #[arg(long, short)]
    output: Option<PathBuf>,
    /// File format [default: from the file name, else text]
    #[arg(long, value_enum)]
    format: Option<FormatArg>,
    /// How M3U8 and CSV paths to the audio are written
    #[arg(long, value_enum, default_value_t = PathsArg::Relative)]
    paths: PathsArg,
    /// With -o: replace the file if it's there already
    #[arg(long, requires = "output")]
    replace: bool,
    /// Delete the list (every list on the drive without one), like a player's ALL DELETE; only
    /// lists the store has kept
    #[arg(long, conflicts_with_all = ["output", "all"])]
    delete: bool,
    /// With --delete: also lists the store hasn't kept
    #[arg(long, requires = "delete")]
    force: bool,
    /// With --delete: show the plan without changing anything
    #[arg(long, requires = "delete")]
    dry_run: bool,
}

#[cfg(feature = "account")]
#[derive(Subcommand)]
enum HistoryCommand {
    /// Publish a history list to your Atmosphere account as a set: its tracks in order, for
    /// anyone to read
    Publish(HistoryPublishArgs),
}

/// `plays export`.
#[derive(Args)]
pub struct PlaysExportArgs {
    #[arg(help = DRIVE_HELP)]
    drive: String,
    /// The history list (name or path)
    list: String,
    /// The file to write
    #[arg(long, short)]
    output: PathBuf,
    /// Also look among the lists the store archived
    #[arg(long)]
    all: bool,
    /// File format [default: from the file name, else text]
    #[arg(long, value_enum)]
    format: Option<FormatArg>,
    /// How M3U8 and CSV paths to the audio are written
    #[arg(long, value_enum, default_value_t = PathsArg::Relative)]
    paths: PathsArg,
    /// Replace the file if it's there already
    #[arg(long)]
    replace: bool,
}

#[cfg(feature = "account")]
#[derive(Args)]
pub struct HistoryPublishArgs {
    #[arg(help = DRIVE_HELP)]
    drive: String,
    /// The history list (name or path)
    list: String,
    /// Also look among the lists the store archived
    #[arg(long)]
    all: bool,
    /// What the set is called (it has none otherwise)
    #[arg(long)]
    title: Option<String>,
    /// The day it was played [default: as published before, else the day the store first saw
    /// the list, else today]
    #[arg(long, value_name = "YYYY-MM-DD")]
    date: Option<String>,
    /// Where it was played
    #[arg(long)]
    venue: Option<String>,
    /// Publish the track at this position without naming it (repeat, or separate with commas);
    /// tracks your profile hides are never named
    #[arg(long, value_name = "N", value_delimiter = ',')]
    hide: Vec<usize>,
    /// Show what would be published without publishing
    #[arg(long)]
    dry_run: bool,
    #[command(flatten)]
    browser: crate::account_cmd::BrowserArgs,
}

#[derive(Args)]
pub struct StatsArgs {
    /// A drive (its mount path, name or id prefix), a library in the store, or snapshot:<id prefix>
    source: String,
    /// Also what was played since this snapshot (a label or id prefix) or this date (2026, 2026-09
    /// or 2026-09-30: since the newest snapshot from before it)
    #[arg(long, value_name = "SNAPSHOT|DATE")]
    since: Option<String>,
    /// How many of the most played tracks to show
    #[arg(long, short = 'n', default_value_t = 20)]
    limit: usize,
    /// List every track never played (not just the first 10)
    #[arg(long)]
    all: bool,
}

/// `plays ...`
pub fn run(args: PlaysArgs, store: &Option<PathBuf>, out: Output, ctx: &Ctx<'_>) -> Result<ExitCode> {
    let command = match (args.command, args.drive) {
        (Some(c), _) => c,
        (None, drive) => PlaysCommand::Lists(ListsArgs {
            drive: match drive {
                Some(d) => crate::help::check_drive_word("plays", &d).map(|_| Some(d))?,
                None => None,
            },
            list: None,
            all: false,
            output: None,
            format: None,
            paths: PathsArg::Relative,
            replace: false,
            delete: false,
            force: false,
            dry_run: false,
        }),
    };
    match command {
        PlaysCommand::Lists(a) => lists(a, store, out, ctx),
        PlaysCommand::Stats(a) => stats(a, store, out),
        PlaysCommand::Export(a) => {
            let PlaysExportArgs { drive, list, output, all, format, paths, replace } = a;
            let a = ListsArgs {
                drive: Some(drive),
                list: Some(list),
                all,
                output: Some(output),
                format,
                paths,
                replace,
                delete: false,
                force: false,
                dry_run: false,
            };
            lists(a, store, out, ctx)
        }
        #[cfg(feature = "account")]
        PlaysCommand::Publish(p) => publish(p, store, out, ctx),
    }
}

#[cfg(feature = "account")]
fn publish(p: HistoryPublishArgs, store: &Option<PathBuf>, out: Output, ctx: &Ctx<'_>) -> Result<ExitCode> {
    let req = api::HistoryPublishRequest {
        drive: p.drive,
        list: p.list,
        archived: p.all,
        title: p.title,
        date: p.date,
        venue: p.venue,
        hide: p.hide,
    };
    crate::publish_cmd::history_publish(store, &req, p.dry_run, p.browser, out, ctx)
}

/// `history` (`plays lists`, and `history publish`).
pub fn history(args: HistoryArgs, store: &Option<PathBuf>, out: Output, ctx: &Ctx<'_>) -> Result<ExitCode> {
    #[cfg(feature = "account")]
    if let Some(HistoryCommand::Publish(p)) = args.command {
        return publish(p, store, out, ctx);
    }
    lists(args.lists, store, out, ctx)
}

/// `plays lists`.
pub fn lists(args: ListsArgs, store: &Option<PathBuf>, out: Output, ctx: &Ctx<'_>) -> Result<ExitCode> {
    let store = crate::diff_cmd::existing_store(store);
    let drive = || args.drive.clone().context("name a drive (`locallibrary drive ls` lists them)");
    if args.delete {
        let drive = drive()?;
        let req = api::DeleteHistoryRequest {
            drive: drive.clone(),
            names: args.list.into_iter().collect(),
            force: args.force,
        };
        let plan = api::delete_history_plan(store.as_ref(), &req, ctx)?;
        return crate::edit_cmd::confirm_and_apply(&drive, plan, args.dry_run, out, ctx);
    }
    if let Some(output) = args.output {
        let drive = drive()?;
        let Some(list) = args.list else {
            bail!("name the history list to write (`locallibrary plays lists {:?}` lists them)", drive)
        };
        let format = args.format.map(Into::into).or_else(|| Format::from_path(&output)).unwrap_or(Format::Txt);
        let req = api::ExportPlaylist {
            drive: drive.clone(),
            playlist: None,
            history: Some(list),
            archived: args.all,
            format: Some(format),
            output,
            paths: args.paths.into(),
            split: false,
            replace: args.replace,
        };
        let report = api::export_playlist(store.as_ref(), &req, ctx)?;
        out.done(&report, |r| {
            for f in &r.files {
                ok(format!("wrote {} ({}, {} tracks)", f.path.display(), f.name, f.tracks));
            }
        });
        return Ok(ExitCode::SUCCESS);
    }

    if args.all && store.is_none() && !out.json {
        warn("there's no store yet, so nothing is archived (locallibrary pull <drive> starts one)");
    }
    let reads = each_drive::read(args.drive.clone(), |drive| histories(drive, args.all, store.as_ref(), ctx))?;
    // A list comes after its drive on the command line, so it's looked for on that drive.
    if let (Reads::One(h), Some(query)) = (&reads, &args.list) {
        let list = plays::find_list(&h.drive.lists, query)
            .or_else(|| h.archive.as_ref().and_then(|a| plays::find_list(a.lists.iter().map(|a| &a.list), query)));
        let Some(list) = list else {
            let hint = if args.all { "" } else { " (--all also looks in the store)" };
            bail!("no history list {query:?} on {}{hint}", h.drive.drive)
        };
        let format = args.format.map(Into::into).unwrap_or(Format::Txt);
        out.emit(list, |l| print_list(l, format));
        return Ok(ExitCode::SUCCESS);
    }
    out.page();
    let outcome = reads.emit(out, |h| {
        list_histories(&h.drive.drive, &h.drive.lists, h.archive.as_ref());
        for w in &h.drive.warnings {
            warn(w);
        }
    });
    Ok(outcome.exit())
}

/// A drive's history lists, and with `all` every one the store archived from it.
#[derive(Serialize)]
struct Histories {
    drive: DriveHistory,
    archive: Option<HistoryArchive>,
}

fn histories(drive: String, all: bool, store: Option<&Store>, ctx: &Ctx<'_>) -> locallibrary::Result<Histories> {
    let on_drive = api::drive_history(&api::DriveRef { drive: drive.clone() })?;
    let archive = match store.filter(|_| all) {
        Some(store) => {
            Some(api::history_archive(store, &api::HistoryArchiveRequest { library: None, drive: Some(drive) }, ctx)?)
        }
        None => None,
    };
    Ok(Histories { drive: on_drive, archive })
}

fn print_list(l: &HistoryList, format: Format) {
    let tracks: Vec<&Track> = l.tracks.iter().collect();
    match format {
        Format::Txt => {
            heading(&l.path);
            print!("{}", export::text(&tracks));
        }
        other => {
            // Paths as the drive has them: there's no file to be relative to.
            let locator = export::Locator::new(export::PathStyle::Drive, Path::new("/"), Path::new("/"));
            print!("{}", export::render(other, &l.path, &tracks, &locator));
        }
    }
    if l.missing > 0 {
        warn(format!("{} entr(ies) refer to tracks no longer in the library", l.missing));
    }
}

fn length(tracks: &[Track]) -> String {
    duration(tracks.iter().map(|t| u64::from(t.duration_secs)).sum())
}

fn list_histories(drive: &str, lists: &[HistoryList], archive: Option<&HistoryArchive>) {
    heading(&format!("On {drive}"));
    if lists.is_empty() {
        say!("  {}", dim("no history lists"));
    }
    for l in lists {
        let stored = match archive {
            Some(a) if plays::is_archived(l, a) => style("in the store").green().to_string(),
            Some(_) => style("not in the store yet").yellow().to_string(),
            None => String::new(),
        };
        say!("  {:<28} {:>4} tracks  {:>8}  {stored}", l.path, l.tracks.len(), length(&l.tracks));
    }
    let Some(archive) = archive else { return };
    let only_stored: Vec<_> = archive.lists.iter().filter(|a| !lists.iter().any(|l| l.same_as(&a.list))).collect();
    heading(&format!("Only in the store ({} of {} archived)", only_stored.len(), archive.lists.len()));
    if only_stored.is_empty() {
        say!("  {}", dim("none"));
    }
    for a in only_stored {
        say!(
            "  {:<28} {:>4} tracks  {:>8}  {}",
            a.list.path,
            a.list.tracks.len(),
            length(&a.list.tracks),
            dim(format!(
                "first seen {} in snapshot {}{}",
                a.first_seen.created.format("%Y-%m-%d %H:%M"),
                a.first_seen.short_id,
                a.first_seen.drive.as_ref().map(|d| format!(" from {d}")).unwrap_or_default()
            ))
        );
    }
    for w in &archive.warnings {
        warn(w);
    }
}

pub fn stats(args: StatsArgs, store: &Option<PathBuf>, out: Output) -> Result<ExitCode> {
    crate::output::show_all(args.all);
    let store = crate::diff_cmd::existing_store(store);
    let req = api::PlayStatsRequest { source: args.source, since: args.since, limit: Some(args.limit) };
    let s = api::play_stats(store.as_ref(), &req)?;
    out.emit_paged(&s, render_stats);
    Ok(ExitCode::SUCCESS)
}

fn render_stats(s: &PlayStats) {
    heading(&s.source);
    say!(
        "  {} tracks, {} played, {} never played, {} history list(s)",
        s.tracks,
        s.played,
        s.never_played.len(),
        s.history_lists
    );
    if !s.never_played.is_empty() {
        heading(&format!("Never played ({})", s.never_played.len()));
        for t in s.never_played.iter().take(crate::output::cap(10)) {
            say!("  {}", t.name);
        }
        if s.never_played.len() > crate::output::cap(10) {
            say!("  {}", dim(format!("... and {} more (--all lists them all)", s.never_played.len() - 10)));
        }
    }
    if !s.most_played.is_empty() {
        heading("Most played");
        for p in &s.most_played {
            let lists = if p.history_lists > 0 { format!("in {} list(s)", p.history_lists) } else { String::new() };
            say!("  {:>4}×  {:<12} {}", p.play_count, dim(lists), p.track.name);
        }
    }
    if let Some(since) = &s.since {
        heading(&format!("Since snapshot {} ({})", since.short_id, since.created.format("%Y-%m-%d %H:%M")));
        if !since.new_histories.is_empty() {
            say!("  {} new history list(s): {}", since.new_histories.len(), since.new_histories.join(", "));
        }
        if since.tracks.is_empty() {
            say!("  {}", dim("nothing played"));
        }
        for t in &since.tracks {
            let plus = t.after.saturating_sub(t.before);
            let lists = if t.history_lists > 0 { format!("in {} list(s)", t.history_lists) } else { String::new() };
            say!(
                "  {:>4}  {:<12} {}",
                if plus > 0 { format!("+{plus}") } else { String::new() },
                dim(lists),
                t.track.name
            );
        }
    }
}
