//! `locallibrary diff` (playlist entries added, removed and moved between two libraries; with
//! `--summary`, which of two drives is newer) and the playlist changes `undo` and `restore` show.

use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::Result;
use clap::Args;
use console::style;
use locallibrary::Store;
use locallibrary::api;
use locallibrary::diff::{Entry, PlaylistDiff};
use locallibrary::progress::Ctx;

use crate::output::{Output, cap, dim, ok};
use crate::render;

#[derive(Args)]
#[command(after_long_help = "Examples:\n  locallibrary diff REKORDBOX BACKUP --playlist Sets/Friday\n  \
locallibrary diff snapshot:3f9a2c Home\n  \
locallibrary diff REKORDBOX BACKUP --summary    # which is newer, the evidence, and the differences")]
pub struct DiffArgs {
    /// A library (its newest drive plugged in, else its latest snapshot), a drive, or
    /// snapshot:<id prefix>; with --summary, a drive (a library's name: its newest)
    left: String,
    /// Same, compared against the first
    right: String,
    /// Only this playlist, or the playlists in this folder
    #[arg(long, short, conflicts_with = "summary")]
    playlist: Option<String>,
    /// Which of two drives is newer, the evidence, and how their libraries differ (what `sync`
    /// goes by)
    #[arg(long)]
    summary: bool,
    /// List every difference (not just the first of each kind)
    #[arg(long)]
    all: bool,
}

/// The store, if there is one: read-only commands never create it.
pub fn existing_store(path: &Option<PathBuf>) -> Option<Store> {
    Store::open(path.clone().unwrap_or_else(Store::default_root)).ok()
}

pub fn run(args: DiffArgs, store: &Option<PathBuf>, out: Output, ctx: &Ctx<'_>) -> Result<ExitCode> {
    crate::output::show_all(args.all);
    if args.summary {
        if args.playlist.is_some() {
            return Err(crate::jobs_cmd::usage("--playlist doesn't go with --summary"));
        }
        return compare(args.left, args.right, out, ctx);
    }
    let store = existing_store(store);
    let req = api::DiffRequest { left: args.left, right: args.right, playlist: args.playlist };
    let d = api::diff(store.as_ref(), &req)?;
    out.emit_paged(&d, render);
    Ok(ExitCode::SUCCESS)
}

/// `compare`, the old name of `diff --summary`.
pub fn compare_alias(args: DiffArgs, store: &Option<PathBuf>, out: Output, ctx: &Ctx<'_>) -> Result<ExitCode> {
    run(DiffArgs { summary: true, ..args }, store, out, ctx)
}

/// `diff --summary`: which of two drives is newer, and why.
fn compare(left: String, right: String, out: Output, ctx: &Ctx<'_>) -> Result<ExitCode> {
    let c = api::compare(&api::ComparePair { left, right }, ctx)?;
    out.emit_paged(&c, render::comparison);
    Ok(ExitCode::SUCCESS)
}

fn entries(sign: &str, list: &[Entry]) {
    for e in list.iter().take(cap(15)) {
        say!("      {sign} {:>4}. {}", e.position, e.track.name);
    }
    if list.len() > cap(15) {
        say!("      {}", dim(format!("... and {} more", list.len() - 15)));
    }
}

pub fn render(d: &PlaylistDiff) {
    say!("{} → {}", style(&d.left).bold(), style(&d.right).bold());
    if d.is_empty() {
        ok(format!("no differences ({} playlist(s) the same)", d.unchanged_playlists));
        return;
    }
    let names = |label: &str, items: Vec<&str>| {
        if items.is_empty() {
            return;
        }
        say!("  {label} ({}):", items.len());
        for i in items.iter().take(cap(10)) {
            say!("    {i}");
        }
        if items.len() > cap(10) {
            say!("    {}", dim(format!("... and {} more", items.len() - 10)));
        }
    };
    names("+ tracks", d.added_tracks.iter().map(|t| t.name.as_str()).collect());
    names("- tracks", d.removed_tracks.iter().map(|t| t.name.as_str()).collect());
    names("~ tracks", d.changed_tracks.iter().map(|t| t.track.name.as_str()).collect());
    names("+ playlists", d.added_playlists.iter().map(String::as_str).collect());
    names("- playlists", d.removed_playlists.iter().map(String::as_str).collect());
    for p in &d.changed_playlists {
        say!(
            "  ~ {}  {}",
            style(&p.path).bold(),
            dim(format!("+{} -{} moved {}", p.added.len(), p.removed.len(), p.moved.len()))
        );
        entries(&style("+").green().to_string(), &p.added);
        entries(&style("-").red().to_string(), &p.removed);
        for m in p.moved.iter().take(cap(15)) {
            say!("      {} {:>4} → {}. {}", style("↕").yellow(), m.from, m.to, m.track.name);
        }
        if p.moved.len() > cap(15) {
            say!("      {}", dim(format!("... and {} more", p.moved.len() - 15)));
        }
    }
    if d.unchanged_playlists > 0 {
        say!("  {}", dim(format!("{} playlist(s) the same", d.unchanged_playlists)));
    }
}
