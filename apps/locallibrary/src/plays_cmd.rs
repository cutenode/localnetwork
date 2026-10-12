//! `locallibrary plays ...`: the sessions players recorded (history lists, on the drive and
//! archived in the store), play counts, writing a session out and importing one from Beat Link
//! Trigger. `history` and `stats` are the old names of `plays lists` and `plays stats`.

use indexmap::IndexMap;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, Result, bail};
use clap::{Args, Subcommand};
use console::style;
use locallibrary::Store;
use locallibrary::api;
use locallibrary::export::{self, Format};
use locallibrary::local_time;
use locallibrary::model::Track;
use locallibrary::plays::{self, DriveHistory, HistoryArchive, HistoryList, PlayStats};
use locallibrary::progress::Ctx;
use locallibrary::sets::session::gather_from;
use locallibrary::sets::{Session, SetBook, describe};
use serde::Serialize;

use crate::DRIVE_HELP;
use crate::each_drive::{self, Reads};
use crate::export_cmd::{FormatArg, PathsArg};
use crate::output::{Output, dim, heading, ok, warn};

/// `plays`: with a drive and no subcommand, its sessions.
#[derive(Args)]
#[command(args_conflicts_with_subcommands = true)]
pub struct PlaysArgs {
    #[command(subcommand)]
    command: Option<PlaysCommand>,
    /// A drive whose sessions to list, as `plays lists` does [default: every mounted rekordbox
    /// drive]
    #[arg(value_name = "DRIVE", add = crate::completion::DriveOnly)]
    drive: Option<String>,
}

#[derive(Subcommand)]
enum PlaysCommand {
    /// Sessions: the history lists players recorded on a drive (--all: also every one the store
    /// kept), or one session's tracks
    #[command(visible_alias = "history", after_long_help = LISTS_LONG_HELP)]
    Lists(ListsArgs),
    /// Play counts: most played, never played, and what was played since a snapshot or a date
    Stats(StatsArgs),
    /// Write a session to a file: text, CSV or M3U8
    Export(PlaysExportArgs),
    /// Keep a play log from Beat Link Trigger's Playlist Writer as a session, with the time each
    /// track played
    Import(ImportArgs),
}

const LISTS_LONG_HELP: &str = "Players start a history list every time a drive goes in and log any track
that plays for about a minute, practice too: each is a session. A session's id is the same on
every drive and in the store; its date is a window, as players have no clock.

Examples:
  locallibrary plays lists SD                       the sessions on the drive
  locallibrary plays lists SD --all                 and every one the store kept
  locallibrary plays lists SD \"HISTORY 012\"         one session's tracks (or give its id)
  locallibrary plays lists SD --delete --dry-run    what clearing the drive's lists would remove";

/// `history`: `plays lists`.
#[derive(Args)]
pub struct HistoryArgs {
    #[command(flatten)]
    lists: ListsArgs,
}

/// `plays lists`.
#[derive(Args)]
pub struct ListsArgs {
    #[arg(help = crate::DEFAULT_DRIVE_HELP)]
    drive: Option<String>,
    /// A session (its id, or its history list's name or path): print its tracks, or write them
    /// with -o
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

/// `plays export`.
#[derive(Args)]
pub struct PlaysExportArgs {
    #[arg(help = DRIVE_HELP)]
    drive: String,
    /// The session: its id, or its history list's name or path
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

/// `plays import`.
#[derive(Args)]
pub struct ImportArgs {
    /// The CSV file Beat Link Trigger's Playlist Writer wrote
    file: PathBuf,
    /// The drive whose library the tracks are looked up in [default: the only mounted rekordbox
    /// drive, else none]
    #[arg(long)]
    drive: Option<String>,
    /// The session's name [default: the file's]
    #[arg(long)]
    name: Option<String>,
}

#[derive(Args)]
pub struct StatsArgs {
    /// A library (its newest drive plugged in, else its latest snapshot), a drive (its mount
    /// path, name or id prefix), or snapshot:<id prefix>
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
        PlaysCommand::Import(a) => {
            let req = api::ImportSessionLog { file: a.file, drive: a.drive, name: a.name };
            let log = api::sessions_import(&crate::open_store(store)?, &req)?;
            Ok(out.done(&log, |l| {
                let id = &l.id[..l.id.len().min(8)];
                let when = |t: chrono::DateTime<chrono::Utc>| local_time(t, "%Y-%m-%d %H:%M");
                // An imported log has plays: one without is refused.
                let (first, last) = (&l.plays[0], &l.plays[l.plays.len() - 1]);
                let span = format!(", {} to {}", when(first.started), when(last.stopped.unwrap_or(last.started)));
                ok(format!("kept {} as session {id}: {}{span}", l.name, crate::output::plural(l.plays.len(), "play")));
                crate::output::hint(format!("`locallibrary sets new --from {id}` makes a set of it"));
            }))
        }
    }
}

/// `history` (`plays lists`).
pub fn history(args: HistoryArgs, store: &Option<PathBuf>, out: Output, ctx: &Ctx<'_>) -> Result<ExitCode> {
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
    let reads =
        each_drive::read_each_drive(args.drive.clone(), |drive| histories(drive, args.all, store.as_ref(), ctx))?;
    // A list comes after its drive on the command line, so it's looked for on that drive.
    if let (Reads::One(h), Some(query)) = (&reads, &args.list) {
        let list = plays::find_list(&h.drive.lists, query)
            .or_else(|| h.archive.as_ref().and_then(|a| plays::find_list(a.lists.iter().map(|a| &a.list), query)));
        let Some(list) = list else {
            let hint = if args.all { "" } else { " (--all also looks in the store)" };
            bail!("no session {query:?} on {}{hint}", h.drive.drive)
        };
        let format = args.format.map(Into::into).unwrap_or(Format::Txt);
        out.emit(list, |l| print_list(l, format));
        return Ok(ExitCode::SUCCESS);
    }
    out.page();
    let outcome = reads.emit(out, |h| list_sessions(h, store.is_some()));
    Ok(outcome.exit())
}

/// A drive's history lists, and with `all` every one the store archived from its library; and the
/// sessions they are.
#[derive(Serialize)]
struct Histories {
    drive: DriveHistory,
    archive: Option<HistoryArchive>,
    /// The drive's sessions, newest first, with the window each was recorded in and the sets made
    /// from it (with `all`, also its library's that only the store has, and imported logs).
    sessions: Vec<Session>,
    /// The sets the sessions are in, by id, as they're named.
    #[serde(skip)]
    sets: IndexMap<String, String>,
}

fn histories(drive: String, all: bool, store: Option<&Store>, ctx: &Ctx<'_>) -> locallibrary::Result<Histories> {
    let d = api::resolve_drive(&drive)?;
    let on_drive = plays::drive_history(&d)?;
    // The store's lists date the drive's too, so they're read whenever there's a store.
    let req = api::HistoryArchiveRequest { library: None, drive: Some(drive) };
    let archive = store.map(|s| api::history_archive(s, &req, ctx)).transpose()?;
    let book = store.map(SetBook::load).transpose()?.unwrap_or_default();
    let mut sessions = gather_from(archive.as_ref(), std::slice::from_ref(&d), &book).sessions;
    if !all {
        sessions.retain(|s| !s.drives.is_empty());
    }
    let sets = book.sets.iter().map(|s| (s.id.clone(), describe(s))).collect();
    Ok(Histories { drive: on_drive, archive: archive.filter(|_| all), sessions, sets })
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

/// A drive's sessions, then (with `--all`) the ones only the store has: each in a line, with
/// whether the store has it (when there's a store) and the set it's in.
fn list_sessions(h: &Histories, with_store: bool) {
    let (on_drive, stored): (Vec<&Session>, Vec<&Session>) = h.sessions.iter().partition(|s| !s.drives.is_empty());
    let line = |s: &Session, with_drive: bool| {
        let kept = match (with_store, s.drives.is_empty() || s.first_seen_on.is_some()) {
            (false, _) => None,
            (true, true) => Some(style("in the store").green().to_string()),
            (true, false) => Some(style("not in the store yet").yellow().to_string()),
        };
        let decided = match s.sets.first() {
            Some(id) => Some(format!("in {}", h.sets.get(id).unwrap_or(id))),
            None => s.dismissed.then(|| "dismissed".to_string()),
        };
        let status: Vec<String> = [kept, decided].into_iter().flatten().collect();
        say!("  {}  {}", crate::sets_cmd::session_line(s, with_drive), dim(status.join(" · ")));
    };
    heading(&format!("On {}", h.drive.drive));
    if on_drive.is_empty() {
        say!("  {}", dim("no sessions"));
    }
    // The heading names the drive.
    on_drive.into_iter().for_each(|s| line(s, false));
    for w in &h.drive.warnings {
        warn(w);
    }
    let Some(archive) = &h.archive else { return };
    heading(&format!("Only in the store ({})", stored.len()));
    if stored.is_empty() {
        say!("  {}", dim("none"));
    }
    stored.into_iter().for_each(|s| line(s, true));
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
        heading(&format!("Since snapshot {} ({})", since.short_id, local_time(since.created, "%Y-%m-%d %H:%M")));
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
