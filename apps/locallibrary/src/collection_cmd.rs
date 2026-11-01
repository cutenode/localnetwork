//! `locallibrary collection`: rekordbox's own collection (`master.db`) on this computer: what's in
//! it, its tracks and playlists, what a drive lacks against it, and exporting it (or a plugin's
//! library source, `--from`) to a drive or folder.

use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{Context, Result, bail};
use clap::{Args, Subcommand};
use console::style;
use locallibrary::api;
use locallibrary::collection_export::CollectionExportOptions;
use locallibrary::collection_export::CollectionExportPlan;
use locallibrary::masterdb::CollectionInfo;
use locallibrary::op::Planned;
use locallibrary::plugin::SourceExport;
use locallibrary::progress::Ctx;
use locallibrary::query::{self, Filter};

use crate::output::{self, Outcome, Output, bytes, dim, duration, heading, ok, warn};

#[derive(Args)]
#[command(
    after_help = "Reads rekordbox 6/7's master.db (~/Library/Pioneer/rekordbox on macOS, %APPDATA%\\Pioneer\\rekordbox \
on Windows) from a decrypted copy in a temporary folder: nothing of rekordbox's is changed or locked, and rekordbox can \
stay open. LOCALLIBRARY_MASTERDB names another master.db and LOCALLIBRARY_MASTERDB_KEY another key."
)]
pub struct CollectionArgs {
    /// The master.db to read [default: rekordbox's on this computer]
    #[arg(long, global = true, value_name = "PATH")]
    db: Option<PathBuf>,
    /// The database's SQLCipher key, should rekordbox change it [default: the one rekordbox 6 and 7 use]
    #[arg(long, global = true, hide = true)]
    key: Option<String>,
    #[command(subcommand)]
    command: Option<CollectionCommand>,
}

#[derive(Subcommand)]
pub enum CollectionCommand {
    /// What the collection holds: tracks, playlists, cues, My Tags, history (the default)
    Info,
    /// List the collection's tracks
    #[command(after_help = crate::FILTER_HELP)]
    Tracks {
        /// Filter by "artist - title" or path
        #[arg(long, short)]
        search: Option<String>,
        /// Only tracks in this playlist (path like Folder/Name, or a unique name)
        #[arg(long, short)]
        playlist: Option<String>,
        /// Only tracks matching a filter, e.g. "bpm:120-128 key:8A~ rating:>=4"
        #[arg(long, short, allow_hyphen_values = true)]
        filter: Option<String>,
        /// Order, e.g. key,bpm or -rating
        #[arg(long, allow_hyphen_values = true)]
        sort: Option<String>,
        /// Show at most this many
        #[arg(long, short = 'n')]
        limit: Option<usize>,
    },
    /// The collection's playlist tree (intelligent playlists show empty)
    Playlists,
    /// Write the collection (or a plugin's library source) to a drive or folder, as rekordbox's
    /// Export does
    #[command(
        after_help = "Tracks Cloud Library Sync keeps in Dropbox or Google Drive are read from there (online-only \
files are downloaded). Exporting again copies only what changed. A folder exported to is a drive like any other: \
`locallibrary pull FOLDER` saves it in the store, and `library push` writes it to a USB drive.

With --from, a plugin reads the library instead (`locallibrary plugin info` lists their sources): only audio files in \
the folders you --grant it are copied, and cues and beat grids it brings are written to new analysis files.",
        after_long_help = "Examples:
  locallibrary collection export /Volumes/USB
  locallibrary collection export /Volumes/USB --from PLUGIN:SOURCE --grant ~/Music"
    )]
    Export {
        /// The drive or folder to write to (a folder: its path)
        drive: String,
        /// Playlist or folder to export (repeatable) [default: everything, or what it was exported with last]
        #[arg(long = "playlist", value_name = "PLAYLIST", conflicts_with = "all")]
        playlists: Vec<String>,
        /// Export everything, whatever it was exported with last
        #[arg(long)]
        all: bool,
        /// Replace a library on it that wasn't exported from the collection, or files on a drive with no
        /// library
        #[arg(long)]
        replace: bool,
        /// Show the plan without changing anything
        #[arg(long)]
        dry_run: bool,
        /// Build it from a plugin's library source instead, as PLUGIN:SOURCE
        #[arg(long, value_name = "PLUGIN:SOURCE")]
        from: Option<String>,
        /// With --from: a folder the plugin may read files in (repeatable)
        #[arg(long = "grant", value_name = "FOLDER", requires = "from")]
        grants: Vec<PathBuf>,
        /// With --from: an option for the source, like key=value (repeatable; `locallibrary plugin
        /// info` shows them)
        #[arg(long = "option", value_name = "KEY=VALUE", requires = "from")]
        options: Vec<String>,
    },
    /// What a drive lacks against the collection: tracks, playlists and playlist entries
    Diff {
        #[arg(help = crate::LIBRARY_HELP)]
        drive: String,
        /// Only this playlist, or the playlists in this folder
        #[arg(long, short)]
        playlist: Option<String>,
    },
}

pub fn run(args: CollectionArgs, store: &Option<PathBuf>, out: Output, ctx: &Ctx<'_>) -> Result<ExitCode> {
    let req = api::CollectionRef { db: args.db, key: args.key };
    match args.command.unwrap_or(CollectionCommand::Info) {
        CollectionCommand::Info => {
            let info = api::collection_info(&req)?;
            out.emit(&info, render_info);
        }
        CollectionCommand::Tracks { search, playlist, filter, sort, limit } => {
            let c = api::open_collection(&req)?;
            let lib = &c.library;
            let mut tracks: Vec<&locallibrary::Track> = match &search {
                Some(q) => lib.search(q),
                None => lib.tracks.iter().collect(),
            };
            if let Some(p) = &playlist {
                let pl = lib.find_playlist(p).with_context(|| format!("no playlist {p:?} in the collection"))?;
                let order = pl.track_ids.clone();
                tracks.retain(|t| order.contains(&t.id));
                tracks.sort_by_key(|t| order.iter().position(|id| *id == t.id));
            }
            if let Some(f) = &filter {
                let f = Filter::parse(f)?;
                let tags = f.uses_tags().then(|| c.tags_by_file());
                let ctx = api::filter_context(lib, &f, tags.as_ref(), None)?;
                tracks.retain(|t| f.matches(t, &ctx));
            }
            if let Some(s) = &sort {
                query::sort_tracks(&mut tracks, &query::parse_sort(s)?);
            }
            let (total, secs) = (tracks.len(), tracks.iter().map(|t| u64::from(t.duration_secs)).sum());
            if let Some(n) = limit {
                tracks.truncate(n);
            }
            out.emit_paged(&tracks, |t| crate::render::track_rows(t, total, secs));
        }
        CollectionCommand::Playlists => {
            let c = api::open_collection(&req)?;
            out.emit_paged(&c.library.playlists, |_| crate::render::playlist_tree(&c.library));
        }
        CollectionCommand::Export { drive, playlists, all, replace, dry_run, from, grants, options } => {
            let export = CollectionExportOptions { playlists, all, replace, overwrite_archive: false };
            let plan = match from {
                Some(_) if req.db.is_some() || req.key.is_some() => {
                    bail!("--db and --key are for rekordbox's collection, not --from")
                }
                Some(from) => {
                    let existing = crate::diff_cmd::existing_store(store)?;
                    let req = from_source(&from, &grants, &options, drive.clone(), export)?;
                    crate::plugins::host()?.plan_source_export(&req, existing.as_ref(), ctx)?
                }
                None => {
                    let req = api::CollectionExportRequest { collection: req, drive: drive.clone(), options: export };
                    api::collection_export_plan(&req, ctx)?
                }
            };
            return write(plan, &drive, dry_run, store, out, ctx);
        }
        CollectionCommand::Diff { drive, playlist } => {
            let d = api::collection_diff(&api::CollectionDiffRequest { collection: req, drive, playlist })?;
            out.emit_paged(&d, |d| {
                say!(
                    "{}",
                    dim(format!(
                        "+ is in the collection but not on {}, - only on the drive; {} track(s) found on the drive by file, \
                         {} by their details",
                        d.diff.left, d.matched_by_file, d.matched_by_details
                    ))
                );
                crate::diff_cmd::render(&d.diff);
                if !d.uncertain.is_empty() {
                    warn(format!(
                        "{} collection track(s) only loosely match a track on the drive and count as missing, e.g. {}",
                        d.uncertain.len(),
                        d.uncertain[0].name
                    ));
                }
            });
        }
    }
    Ok(ExitCode::SUCCESS)
}

/// Building `drive` from `from`, a plugin's library source (`PLUGIN:SOURCE`), reading the folders
/// `grants` (made absolute) with the source's `--option`s.
fn from_source(
    from: &str,
    grants: &[PathBuf],
    options: &[String],
    drive: String,
    export: CollectionExportOptions,
) -> Result<SourceExport> {
    let (plugin, source) = from.split_once(':').with_context(|| format!("--from takes PLUGIN:SOURCE, not {from:?}"))?;
    let declared = crate::plugins::host()?.manifest(plugin)?.contributes.sources.iter().find(|s| s.name == source);
    let declared = declared.with_context(|| format!("the {plugin} plugin has no library source {source:?}"))?;
    let options = crate::plugin_cmd::options("source", declared.options.as_ref(), options)?;
    let grants = grants.iter().map(std::path::absolute).collect::<std::io::Result<_>>()?;
    Ok(SourceExport { plugin: plugin.into(), source: source.into(), options, grants, drive, export })
}

/// Shows what writing `plan` to its drive (`drive`, as it was given) does, asks first, and writes it.
fn write(
    plan: Planned<CollectionExportPlan, CollectionExportOptions>,
    drive: &str,
    dry_run: bool,
    store: &Option<PathBuf>,
    out: Output,
    ctx: &Ctx<'_>,
) -> Result<ExitCode> {
    if !out.json {
        let what = match plan.playlists.as_slice() {
            [] => plan.what(),
            p => p.join(", "),
        };
        say!(
            "{} -> {}: {} track(s), {} playlist(s), {}",
            style(what).bold(),
            style(&plan.target).bold(),
            plan.tracks,
            plan.playlist_count,
            bytes(plan.library_bytes)
        );
        say!(
            "  copy {} file(s), {}; remove {} file(s), {}; {} unchanged",
            plan.copies.len(),
            bytes(plan.copy_bytes),
            plan.deletes.len(),
            bytes(plan.delete_bytes),
            plan.unchanged
        );
        if plan.downloads > 0 {
            say!(
                "  {}",
                dim(format!(
                    "{} of them ({}) are online only in cloud storage and download as they're copied",
                    plan.downloads,
                    bytes(plan.download_bytes)
                ))
            );
        }
        for s in plan.summary.iter().filter(|s| s.starts_with("read the files") || s.starts_with("keep a smart rule")) {
            say!("  {}", dim(s));
        }
        for w in &plan.warnings {
            warn(w);
        }
    }
    let question = match (plan.replaces_library, &plan.source) {
        (true, None) => format!("Replace the library on {} with the collection?", plan.target),
        (true, Some(source)) => format!("Replace the library on {} with {source}?", plan.target),
        (false, _) => format!("Export to {}?", plan.target),
    };
    if let Some(o) = out.decide(dry_run, &question) {
        // A folder made for the export goes again, as nothing was written to it.
        if plan.made_folder {
            let _ = std::fs::remove_dir(&plan.target_root);
        }
        return Ok(out.not_applied(&plan, o));
    }
    let report = plan.apply(crate::open_store(store).ok().as_ref(), ctx)?;
    Ok(out.applied(&plan, &report, Outcome::Applied, || {
        if let Some(s) = &report.saved {
            ok(format!("saved snapshot {} ({})", s.short_id, s.message.as_deref().unwrap_or("")));
        }
        ok(format!(
            "exported {} track(s) to {}: {} file(s) copied ({}), {} removed",
            report.tracks,
            plan.target,
            report.copied,
            bytes(report.copied_bytes),
            report.deleted
        ));
        output::hint(format!("Save it in the store with: locallibrary pull {:?}", plan.target_root));
        if let Some(b) = &report.backup {
            output::hint(format!("undo with: locallibrary undo {drive:?} (backup {b})"));
        }
    }))
}

fn render_info(i: &CollectionInfo) {
    heading("rekordbox collection");
    say!(
        "  {}  {}{}",
        i.path.display(),
        bytes(i.bytes),
        i.wal_bytes.map(|w| format!(" (+ {} not yet checkpointed)", bytes(w))).unwrap_or_default()
    );
    if let Some(v) = &i.db_version {
        say!("  {}", dim(format!("database version {v}")));
    }
    let s = &i.summary;
    ok(format!(
        "{} tracks by {} artists, {} ({} of audio)",
        s.tracks,
        s.artists,
        duration(s.total_duration_secs),
        bytes(s.audio_bytes)
    ));
    say!("  {} playlists in {} folders, {} history lists", s.playlists, s.folders, i.history_lists);
    say!("  {} hot cues, {} memory cues, {} loops", i.hot_cues, i.memory_cues, i.loops);
    say!("  {} My Tags in {} categories on {} tracks", i.tags, i.tag_categories, i.tagged_tracks);
    if !i.smart_playlists.is_empty() {
        say!(
            "  {}",
            dim(format!(
                "{} intelligent playlist(s) aren't evaluated and show empty: {}",
                i.smart_playlists.len(),
                i.smart_playlists.join(", ")
            ))
        );
    }
}
