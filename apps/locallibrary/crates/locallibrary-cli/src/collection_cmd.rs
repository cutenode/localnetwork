//! `locallibrary collection`: rekordbox's own collection (`master.db`) on this computer, read
//! only: what's in it, its tracks and playlists, and what a drive lacks against it.

use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{Context, Result};
use clap::{Args, Subcommand};
use locallibrary::api;
use locallibrary::masterdb::CollectionInfo;
use locallibrary::query::{self, Filter, QueryContext};

use crate::output::{Output, bytes, dim, duration, heading, ok, warn};

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
    /// What a drive lacks against the collection: tracks, playlists and playlist entries
    Diff {
        #[arg(help = crate::LIBRARY_HELP)]
        drive: String,
        /// Only this playlist, or the playlists in this folder
        #[arg(long, short)]
        playlist: Option<String>,
    },
}

pub fn run(args: CollectionArgs, out: Output) -> Result<ExitCode> {
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
                let mut ctx = QueryContext::new(lib);
                if let Some(t) = &tags {
                    ctx = ctx.with_tags(t);
                }
                f.check(&ctx)?;
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
