//! `locallibrary tags`: My Tags on a drive (see [`locallibrary::tags`]).

use std::process::ExitCode;

use anyhow::Result;
use clap::{Args, Subcommand};
use console::style;
use locallibrary::api::{self, DriveRef, TagAction, TagsEdit};
use locallibrary::progress::Ctx;
use locallibrary::tags::{MyTags, Source, TagPlan, TagReport};

use crate::SelectArgs;
use crate::output::{Outcome, Output, dim, ok, warn};

#[derive(Args)]
#[command(args_conflicts_with_subcommands = true)]
pub struct TagsArgs {
    #[command(subcommand)]
    command: Option<TagsCommand>,
    /// Library or drive whose My Tags to list (same as `tags list`) [default: each library plugged
    /// in, and each rekordbox drive of none]
    drive: Option<String>,
    /// Also list the tracks carrying each tag
    #[arg(long)]
    tracks: bool,
}

#[derive(Subcommand)]
enum TagsCommand {
    /// Categories and tags, how many tracks carry each, and where the two databases disagree
    List {
        #[arg(help = crate::DEFAULT_LIBRARY_HELP)]
        drive: Option<String>,
        /// Also list the tracks carrying each tag
        #[arg(long)]
        tracks: bool,
    },
    /// Give tracks a tag, creating it (and its category) where it's missing
    #[command(after_help = crate::FILTER_HELP)]
    Add {
        #[arg(help = crate::LIBRARY_HELP)]
        drive: String,
        /// Category/Tag, like "Genre/Deep House"
        tag: String,
        #[command(flatten)]
        select: SelectArgs,
        /// Show the plan without changing anything
        #[arg(long)]
        dry_run: bool,
    },
    /// Take a tag off tracks (`delete` deletes the tag itself)
    #[command(alias = "remove", after_help = crate::FILTER_HELP)]
    Untag {
        #[arg(help = crate::LIBRARY_HELP)]
        drive: String,
        /// Category/Tag
        tag: String,
        #[command(flatten)]
        select: SelectArgs,
        /// Show the plan without changing anything
        #[arg(long)]
        dry_run: bool,
    },
    /// Rename a tag ("Genre/Deep" "Deep House") or a category ("Untitled Column" "Mood")
    Rename {
        #[arg(help = crate::LIBRARY_HELP)]
        drive: String,
        /// Category/Tag, or Category
        tag: String,
        /// The new name
        to: String,
        /// Show the plan without changing anything
        #[arg(long)]
        dry_run: bool,
    },
    /// Delete a tag, taking it off every track
    Delete {
        #[arg(help = crate::LIBRARY_HELP)]
        drive: String,
        /// Category/Tag
        tag: String,
        /// Show the plan without changing anything
        #[arg(long)]
        dry_run: bool,
    },
}

pub fn run(args: TagsArgs, out: Output, ctx: &Ctx<'_>) -> Result<ExitCode> {
    let (drive, action, dry_run) = match args.command {
        None => {
            if let Some(drive) = &args.drive {
                crate::help::check_drive_word("tags", drive)?;
            }
            return list(args.drive, args.tracks, out);
        }
        Some(TagsCommand::List { drive, tracks }) => return list(drive, tracks, out),
        Some(TagsCommand::Add { drive, tag, select, dry_run }) => {
            (drive, TagAction::Add { tag, tracks: select.selector() }, dry_run)
        }
        Some(TagsCommand::Untag { drive, tag, select, dry_run }) => {
            (drive, TagAction::Remove { tag, tracks: select.selector() }, dry_run)
        }
        Some(TagsCommand::Rename { drive, tag, to, dry_run }) => (drive, TagAction::Rename { tag, to }, dry_run),
        Some(TagsCommand::Delete { drive, tag, dry_run }) => (drive, TagAction::Delete { tag }, dry_run),
    };
    let req = TagsEdit { drive, action };
    let plan = api::tags_plan(&req)?;
    if plan.noop {
        if !out.json {
            for w in &plan.warnings {
                warn(w);
            }
        }
        return Ok(out.nothing(&plan, nothing_to_do(&plan)));
    }
    if !out.json {
        render_plan(&plan);
        crate::output::clones(&plan);
    }
    if let Some(o) = out.decide(dry_run, "Apply?") {
        return Ok(out.not_applied(&plan, o));
    }
    let report = plan.apply(None, ctx)?;
    Ok(out.applied(&plan, &report, Outcome::Applied, || render_report(&report, &req.drive)))
}

fn list(drive: Option<String>, tracks: bool, out: Output) -> Result<ExitCode> {
    let reads = crate::each_drive::read(drive, |drive| api::tags_list(&DriveRef { drive }))?;
    out.page();
    Ok(reads.emit(out, |t| render_list(t, tracks)).exit())
}

fn files(sources: &[Source]) -> String {
    sources.iter().map(|s| s.file_name()).collect::<Vec<_>>().join(" and ")
}

/// " (exportExt.pdb only)" for something one of the drive's two databases lacks.
fn only_in(sources: &[Source], all: &[Source]) -> String {
    if all.len() > 1 && sources.len() == 1 { format!(" ({} only)", sources[0].file_name()) } else { String::new() }
}

fn render_list(t: &MyTags, tracks: bool) {
    if t.sources.is_empty() {
        say!("{}: no My Tags", style(&t.drive).bold());
    } else {
        say!("{}  {}", style(&t.drive).bold(), dim(format!("My Tags in {}", files(&t.sources))));
    }
    let width = t.categories.iter().flat_map(|c| &c.tags).map(|t| t.name.chars().count()).max().unwrap_or(0);
    for c in &t.categories {
        say!("{}{}", style(&c.name).bold(), dim(only_in(&c.sources, &t.sources)));
        if c.tags.is_empty() {
            say!("  {}", dim("no tags"));
        }
        for tag in &c.tags {
            // Padded before it's dimmed: the colour's codes would count towards the width.
            let count = match tag.tracks.len() {
                0 => dim(format!("{:>4}", "-")).to_string(),
                n => format!("{n:>4}"),
            };
            say!("  {:<width$}  {count}{}", tag.name, dim(only_in(&tag.sources, &t.sources)));
            if tracks {
                for r in &tag.tracks {
                    say!("      {}  {}", r.name, dim(format!("#{}", r.id)));
                }
            }
        }
    }
    for w in &t.warnings {
        warn(w);
    }
    if !t.differences.is_empty() {
        warn(format!("{} disagree:", files(&t.sources)));
        for d in &t.differences {
            say!("    {d}");
        }
    }
}

fn nothing_to_do(p: &TagPlan) -> String {
    match p.action.as_str() {
        "add" => format!("nothing to change: the selected track(s) already have {}", p.tag),
        "remove" => format!("nothing to change: none of the selected tracks has {}", p.tag),
        "rename" => format!("nothing to change: it's already called {}", p.to.as_deref().unwrap_or_default()),
        _ => "nothing to change".to_string(),
    }
}

fn render_plan(p: &TagPlan) {
    let drive = style(&p.drive).bold();
    let n = p.tracks.len();
    match p.action.as_str() {
        "add" => say!("{drive}: add {} to {n} track(s)", p.tag),
        "remove" => say!("{drive}: remove {} from {n} track(s)", p.tag),
        "rename" => say!("{drive}: rename {} to {}", p.tag, p.to.as_deref().unwrap_or_default()),
        _ => say!("{drive}: delete {} (from {n} track(s))", p.tag),
    }
    let sign = if p.action == "add" { "+" } else { "-" };
    for t in p.tracks.iter().take(15) {
        say!("    {sign} {}", t.name);
    }
    if n > 15 {
        say!("    {}", dim(format!("... and {} more", n - 15)));
    }
    for c in &p.created {
        say!("  create {} in {}", c.path, c.database.file_name());
    }
    if p.unchanged > 0 {
        let state = if p.action == "add" { "already have it" } else { "don't have it" };
        say!("  {}", dim(format!("{} selected track(s) {state}", p.unchanged)));
    }
    say!("  {}", dim(format!("changes {}", files(&p.databases))));
    for w in &p.warnings {
        warn(w);
    }
}

fn render_report(r: &TagReport, drive: &str) {
    let tracks = if r.tracks_changed > 0 { format!(" ({} track(s))", r.tracks_changed) } else { String::new() };
    ok(format!("updated My Tags in {}{tracks}", files(&r.databases)));
    if let Some(b) = &r.backup {
        crate::output::hint(format!("undo with: locallibrary undo {drive:?} (backup {b})"));
    }
}
