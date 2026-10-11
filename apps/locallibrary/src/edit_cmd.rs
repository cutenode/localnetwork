//! `playlist`, `track`, `smart` and `edits`: editing a drive's playlists and tracks in place.

use std::process::ExitCode;

use anyhow::{Result, bail};
use clap::{Args, Subcommand};
use console::style;
use locallibrary::Store;
use locallibrary::api::{self, DriveRef, EditRequest};
use locallibrary::edit::{Edit, EditOptions, EditPlan, EditReport, TrackFields};
use locallibrary::model::Library;
use locallibrary::op::Planned;
use locallibrary::progress::Ctx;
use locallibrary::query::TrackSelector;

use crate::output::{self, Outcome, Output, bytes, dim, warn};
use crate::{LIBRARY_HELP, SelectArgs, render};

#[derive(Args)]
pub struct DryRun {
    /// Show the plan without changing anything
    #[arg(long)]
    dry_run: bool,
}

/// `playlist`: with a library or drive and no subcommand, its playlist tree.
#[derive(Args)]
#[command(args_conflicts_with_subcommands = true)]
pub struct PlaylistArgs {
    #[command(subcommand)]
    command: Option<PlaylistCommand>,
    /// A library (or drive) whose playlist tree to show, as `playlist ls` does [default: each
    /// library plugged in, and each rekordbox drive of none]
    #[arg(value_name = "DRIVE")]
    drive: Option<String>,
}

#[derive(Subcommand)]
pub enum PlaylistCommand {
    /// A library's playlist tree: folders, playlists and how many tracks each holds
    #[command(visible_alias = "tree", alias = "list")]
    Ls {
        #[arg(help = crate::DEFAULT_LIBRARY_HELP)]
        drive: Option<String>,
    },
    /// Create a playlist (or a folder), creating the folders above it
    #[command(
        after_long_help = "Examples:\n  locallibrary playlist new Home \"Sets/2026/Friday\" --filter \"bpm:122-128 key:8A~\"\n  \
locallibrary playlist new Home Archive --folder"
    )]
    New {
        #[arg(help = LIBRARY_HELP)]
        drive: String,
        /// Path like Sets/2026/Friday
        path: String,
        /// Make a folder rather than a playlist
        #[arg(long)]
        folder: bool,
        /// Fill the new playlist with these tracks
        #[command(flatten)]
        select: SelectArgs,
        #[command(flatten)]
        dry: DryRun,
    },
    /// Rename a playlist or folder
    Rename {
        #[arg(help = LIBRARY_HELP)]
        drive: String,
        /// The playlist or folder (path like Folder/Name, or a unique name)
        playlist: String,
        /// Its new name
        name: String,
        #[command(flatten)]
        dry: DryRun,
    },
    /// Move a playlist or folder into another folder, or among its siblings (`reorder` moves
    /// the tracks in a playlist)
    #[command(alias = "mv")]
    Move {
        #[arg(help = LIBRARY_HELP)]
        drive: String,
        /// The playlist or folder (path like Folder/Name, or a unique name)
        playlist: String,
        /// Folder to move into [default: where it is]
        #[arg(long, conflicts_with = "top")]
        into: Option<String>,
        /// Move to the top level
        #[arg(long)]
        top: bool,
        /// Position among the folder's contents, from 1 [default: last]
        #[arg(long)]
        position: Option<usize>,
        /// `playlist reorder`'s --at (this command's old meaning: moving tracks in the playlist)
        #[arg(long = "at", value_delimiter = ',', value_name = "POSITIONS", hide = true, requires = "to")]
        #[arg(conflicts_with_all = ["into", "top", "position"])]
        at: Vec<usize>,
        /// `playlist reorder`'s --to
        #[arg(long, hide = true, requires = "at")]
        to: Option<usize>,
        #[command(flatten)]
        dry: DryRun,
    },
    /// Delete a playlist, or a folder and everything in it (the tracks stay in the library)
    #[command(alias = "rm")]
    Delete {
        #[arg(help = LIBRARY_HELP)]
        drive: String,
        /// The playlist or folder (path like Folder/Name, or a unique name)
        playlist: String,
        /// Also delete the tracks nothing else uses, with their files (e.g. a guest's merged crate)
        #[arg(long)]
        with_tracks: bool,
        #[command(flatten)]
        dry: DryRun,
    },
    /// Add tracks to a playlist
    #[command(
        after_long_help = "Examples:\n  locallibrary playlist add Home \"Sets/2026/Friday\" --query \"floorplan\" --position 1\n  \
locallibrary playlist add Home Peak --filter \"bpm:126-130 rating:>=4\" --sort -rating"
    )]
    Add {
        #[arg(help = LIBRARY_HELP)]
        drive: String,
        /// The playlist (path like Folder/Name, or a unique name)
        #[arg(id = "target", value_name = "PLAYLIST")]
        playlist: String,
        #[command(flatten)]
        select: SelectArgs,
        /// Insert at this position, from 1 [default: the end]
        #[arg(long)]
        position: Option<usize>,
        /// Order the added tracks, e.g. key,bpm or -rating
        #[arg(long, allow_hyphen_values = true)]
        sort: Option<String>,
        /// Add tracks the playlist already has again
        #[arg(long)]
        allow_repeats: bool,
        #[command(flatten)]
        dry: DryRun,
    },
    /// Remove tracks from a playlist, by track or by position
    Remove {
        #[arg(help = LIBRARY_HELP)]
        drive: String,
        /// The playlist (path like Folder/Name, or a unique name)
        #[arg(id = "target", value_name = "PLAYLIST")]
        playlist: String,
        #[command(flatten)]
        select: SelectArgs,
        /// Positions to remove, from 1 (comma-separated)
        #[arg(long = "at", value_delimiter = ',', value_name = "POSITIONS")]
        positions: Vec<usize>,
        #[command(flatten)]
        dry: DryRun,
    },
    /// Move tracks within a playlist, by position
    #[command(
        after_long_help = "Example:\n  locallibrary playlist reorder Home Friday --at 7,8 --to 1    # tracks 7 and 8 open the playlist"
    )]
    Reorder {
        #[arg(help = LIBRARY_HELP)]
        drive: String,
        /// The playlist (path like Folder/Name, or a unique name)
        playlist: String,
        /// Positions to move, from 1 (comma-separated; they keep their order)
        #[arg(long = "at", value_delimiter = ',', value_name = "POSITIONS", required = true)]
        positions: Vec<usize>,
        /// Where the first of them lands
        #[arg(long)]
        to: usize,
        #[command(flatten)]
        dry: DryRun,
    },
    /// Sort a playlist, e.g. --sort key,bpm or --sort -rating
    Sort {
        #[arg(help = LIBRARY_HELP)]
        drive: String,
        /// The playlist (path like Folder/Name, or a unique name)
        playlist: String,
        /// e.g. key,bpm or -rating (a leading - sorts highest first)
        #[arg(long = "sort", alias = "by", value_name = "KEYS", allow_hyphen_values = true)]
        by: String,
        #[command(flatten)]
        dry: DryRun,
    },
    /// Drop repeated entries from a playlist
    Dedupe {
        #[arg(help = LIBRARY_HELP)]
        drive: String,
        /// The playlist (path like Folder/Name, or a unique name)
        playlist: String,
        /// Also other copies of the same recording (same artist, title and duration)
        #[arg(long)]
        recordings: bool,
        #[command(flatten)]
        dry: DryRun,
    },
    /// Order a playlist for mixing: compatible keys, close tempos, energy that builds
    #[command(alias = "plan")]
    Order {
        #[arg(help = LIBRARY_HELP)]
        drive: String,
        /// The playlist (path like Folder/Name, or a unique name)
        playlist: String,
        /// Start with this track id [default: the calmest, slowest track]
        #[arg(long)]
        start: Option<u32>,
        /// Write the order to a new playlist instead of reordering this one
        #[arg(long)]
        into: Option<String>,
        #[command(flatten)]
        dry: DryRun,
    },
    /// Make a playlist hold the tracks matching a filter (created if needed)
    Fill {
        #[arg(help = LIBRARY_HELP)]
        drive: String,
        /// The playlist's path, like Folder/Name, letter case included (made there if missing)
        playlist: String,
        /// e.g. "bpm:120-128 key:8A~ rating:>=4"
        #[arg(long, allow_hyphen_values = true)]
        filter: String,
        /// Order, e.g. key,bpm or -rating [default: the drive's order]
        #[arg(long, allow_hyphen_values = true)]
        sort: Option<String>,
        /// At most this many tracks (the first, in --sort's order)
        #[arg(long)]
        limit: Option<usize>,
        #[command(flatten)]
        dry: DryRun,
    },
    /// Make a playlist from a tracklist: M3U, CSV (Spotify, TIDAL, Beatport... exports), rekordbox text or XML, or "Artist - Title" lines
    Import {
        #[arg(help = LIBRARY_HELP)]
        drive: String,
        /// The tracklist: .m3u/.m3u8, .csv, .txt or rekordbox .xml
        file: std::path::PathBuf,
        /// The playlist's path, letter case included (made there if missing)
        #[arg(long)]
        into: String,
        /// For rekordbox XML: the playlist to take, by path [default: the whole collection]
        #[arg(long = "from", value_name = "PLAYLIST")]
        from: Option<String>,
        /// Add to the playlist instead of replacing its contents
        #[arg(long)]
        append: bool,
        /// Settle an ambiguous entry: ENTRY=TRACK (numbers as listed; repeatable)
        #[arg(long = "choose", value_name = "ENTRY=TRACK")]
        choose: Vec<String>,
        #[command(flatten)]
        dry: DryRun,
    },
    /// Collect the tracks that are in no playlist into one, so players can find them
    Unfiled {
        #[arg(help = LIBRARY_HELP)]
        drive: String,
        /// The playlist's path, letter case included (made there if missing)
        #[arg(long, default_value = "Unfiled")]
        name: String,
        #[command(flatten)]
        dry: DryRun,
    },
}

/// `track`: with a library or drive and no subcommand, the chosen tracks' details.
#[derive(Args)]
#[command(args_conflicts_with_subcommands = true)]
pub struct TrackArgs {
    #[command(subcommand)]
    command: Option<TrackCommand>,
    /// A library (or drive) whose tracks to show, as `track show` does
    #[arg(value_name = "DRIVE")]
    drive: Option<String>,
    #[command(flatten)]
    select: SelectArgs,
}

// Parsed once per run; the size difference between variants doesn't matter.
#[allow(clippy::large_enum_variant)]
#[derive(Subcommand)]
pub enum TrackCommand {
    /// Change track details: rating, colour, genre, title, artist...
    #[command(
        after_long_help = "Examples:\n  locallibrary track set Home --filter \"bpm:>=128 genre:techno\" --rating 5 --colour red\n  \
locallibrary track set Home --track 12 --title \"One More Time\" --artist \"Daft Punk\""
    )]
    Set {
        #[arg(help = LIBRARY_HELP)]
        drive: String,
        #[command(flatten)]
        select: SelectArgs,
        #[command(flatten)]
        fields: FieldArgs,
        #[command(flatten)]
        dry: DryRun,
    },
    /// Delete tracks with their playlist entries, and their files unless another track uses them
    Rm {
        #[arg(help = LIBRARY_HELP)]
        drive: String,
        #[command(flatten)]
        select: SelectArgs,
        /// Leave the audio, analysis and artwork files on the drive
        #[arg(long)]
        keep_files: bool,
        #[command(flatten)]
        dry: DryRun,
    },
    /// Replace one copy of a recording with another everywhere, then delete it
    Replace {
        #[arg(help = LIBRARY_HELP)]
        drive: String,
        /// Track id of the copy to replace
        duplicate: u32,
        /// Track id of the copy to keep
        keep: u32,
        /// Keep the replaced copy in the library
        #[arg(long)]
        keep_duplicate: bool,
        #[command(flatten)]
        dry: DryRun,
    },
    /// Replace duplicate copies with the one `doctor` suggests keeping (best audio, then most cues
    /// and playlists), carrying cues onto it when it has none
    Dedupe {
        #[arg(help = LIBRARY_HELP)]
        drive: String,
        /// Also find copies by audio content (hashes audio files that share a size)
        #[arg(long)]
        hash_audio: bool,
        /// Also find the same recording in other files and encodings by its sound (decodes every
        /// track the first time; the store keeps the fingerprints)
        #[arg(long)]
        by_sound: bool,
        /// Also tracks that only share artist, title and length (they may be different edits)
        #[arg(long)]
        metadata: bool,
        #[command(flatten)]
        dry: DryRun,
    },
    /// Find the audio of tracks whose file is missing: on the drive (moved or renamed) and in the
    /// folders given, by file name. A track changes only when exactly one file fits it
    #[command(after_long_help = "Examples:\n  locallibrary track relocate Home\n  \
locallibrary track relocate Home --look-in ~/Music --dry-run")]
    Relocate {
        #[arg(help = LIBRARY_HELP)]
        drive: String,
        /// Also look in this folder and everything below it (repeatable); files found there are
        /// copied to where their track expects them
        #[arg(long = "look-in", value_name = "DIR")]
        folders: Vec<std::path::PathBuf>,
        #[command(flatten)]
        dry: DryRun,
    },
    /// Everything about tracks: details, playlists and history lists holding them, My Tags, other
    /// copies, a missing audio file, and what their analysis holds
    Show {
        #[arg(help = LIBRARY_HELP)]
        drive: String,
        #[command(flatten)]
        select: SelectArgs,
    },
    /// Tracks that mix into a track: compatible key, close tempo
    Matches {
        #[arg(help = LIBRARY_HELP)]
        drive: String,
        /// The track's id
        track: u32,
        /// Show at most this many
        #[arg(long, short = 'n', default_value_t = 20)]
        limit: usize,
    },
}

#[derive(Args)]
pub struct FieldArgs {
    /// 0-5 stars
    #[arg(long)]
    rating: Option<u8>,
    /// pink, red, orange, yellow, green, aqua, blue, purple or none
    #[arg(long, visible_alias = "colour")]
    color: Option<String>,
    /// New title
    #[arg(long)]
    title: Option<String>,
    /// New artist
    #[arg(long)]
    artist: Option<String>,
    /// New album name (--album selects tracks by album)
    #[arg(long = "album-name", id = "album_name", value_name = "ALBUM")]
    album: Option<String>,
    /// New genre
    #[arg(long)]
    genre: Option<String>,
    /// New record label
    #[arg(long)]
    label: Option<String>,
    /// Musical key in any notation (8A, Am, 1m)
    #[arg(long)]
    key: Option<String>,
    /// New remixer
    #[arg(long)]
    remixer: Option<String>,
    /// New comment
    #[arg(long)]
    comment: Option<String>,
    /// Mix name, e.g. "Extended Mix"
    #[arg(long)]
    mix: Option<String>,
    /// Release year
    #[arg(long)]
    year: Option<u16>,
    /// Play count
    #[arg(long)]
    play_count: Option<u16>,
}

impl FieldArgs {
    fn fields(&self) -> TrackFields {
        TrackFields {
            rating: self.rating,
            color: self.color.clone(),
            title: self.title.clone(),
            artist: self.artist.clone(),
            album: self.album.clone(),
            genre: self.genre.clone(),
            label: self.label.clone(),
            key: self.key.clone(),
            remixer: self.remixer.clone(),
            comment: self.comment.clone(),
            mix_name: self.mix.clone(),
            year: self.year,
            play_count: self.play_count,
            ..Default::default()
        }
    }
}

/// `smart`: with no subcommand, the rule-based playlists in the store.
#[derive(Args)]
pub struct SmartArgs {
    #[command(subcommand)]
    command: Option<SmartCommand>,
}

#[derive(Subcommand)]
pub enum SmartCommand {
    /// The rule-based playlists in the store
    List,
    /// Add (or replace) a rule-based playlist
    Add {
        /// Where it goes on drives, e.g. "Smart/Peak time"
        path: String,
        /// e.g. "bpm:>=126 rating:>=4 -genre:house"
        #[arg(long, allow_hyphen_values = true)]
        filter: String,
        /// Order, e.g. key,bpm or -rating [default: each drive's order]
        #[arg(long, allow_hyphen_values = true)]
        sort: Option<String>,
        /// At most this many tracks (the first, in --sort's order)
        #[arg(long)]
        limit: Option<usize>,
        /// Only for this library, or this drive's library (repeatable) [default: every drive]
        #[arg(long = "drive")]
        drives: Vec<String>,
    },
    /// Remove a rule-based playlist from the store (drives keep their copies)
    Rm {
        /// The rule's path, e.g. "Smart/Peak time"
        path: String,
    },
    /// Write the rule-based playlists to drives, filled from each drive's library
    Apply {
        /// Libraries or drives [default: each library plugged in, and each rekordbox drive of none,
        /// but archive and retired ones]
        drives: Vec<String>,
        #[command(flatten)]
        dry: DryRun,
    },
}

/// `edits`: with a drive and no subcommand, the edits recorded on it.
#[derive(Args)]
#[command(args_conflicts_with_subcommands = true)]
pub struct EditsArgs {
    #[command(subcommand)]
    command: Option<EditsCommand>,
    /// A library (or drive) whose recorded edits to list, as `edits list` does [default: each
    /// library plugged in, and each rekordbox drive of none]
    #[arg(value_name = "DRIVE")]
    drive: Option<String>,
}

#[derive(Subcommand)]
pub enum EditsCommand {
    /// The edits recorded on a library's drives
    List {
        #[arg(help = crate::DEFAULT_LIBRARY_HELP)]
        drive: Option<String>,
    },
    /// Make the recorded edits again, e.g. after rekordbox exported to the drive
    Replay {
        #[arg(help = LIBRARY_HELP)]
        drive: String,
        #[command(flatten)]
        dry: DryRun,
    },
    /// Forget the recorded edits
    Clear {
        #[arg(help = crate::DRIVE_HELP)]
        drive: String,
        #[command(flatten)]
        dry: DryRun,
    },
}

fn selector(select: &SelectArgs) -> Result<TrackSelector> {
    let s = select.selector();
    if s.is_empty() {
        return Err(crate::jobs_cmd::usage(
            "say which tracks: --track, --query, --filter, --playlist, --album, --path or --history",
        ));
    }
    Ok(s)
}

/// Plan, show, confirm and apply edits on a drive.
pub fn run_edits(drive: String, edits: Vec<Edit>, dry_run: bool, out: Output, ctx: &Ctx<'_>) -> Result<ExitCode> {
    let req = EditRequest { drive, edits, options: EditOptions::default() };
    confirm_and_apply(&req.drive, api::edit_plan(&req, ctx)?, dry_run, out, ctx)
}

/// Show a planned edit, ask, and apply exactly the plan shown.
pub(crate) fn confirm_and_apply(
    drive: &str,
    planned: Planned<EditPlan>,
    dry_run: bool,
    out: Output,
    ctx: &Ctx<'_>,
) -> Result<ExitCode> {
    if planned.noop {
        if !out.json {
            for w in &planned.warnings {
                warn(w);
            }
        }
        return Ok(out.nothing(&planned, "nothing to change"));
    }
    if !out.json {
        render_plan(&planned);
        output::clones(&planned);
        // What the plan says besides the edit's own and its clones' (a set's tracks not on the
        // drive, say).
        let about_clones = |w: &String| planned.clones.iter().any(|c| c.left_alone.as_ref() == Some(w));
        for w in planned.warnings.iter().filter(|w| !planned.plan.warnings.contains(w) && !about_clones(w)) {
            warn(w);
        }
    }
    if let Some(o) = out.decide(dry_run, "Make these changes?") {
        return Ok(out.not_applied(&planned, o));
    }
    let report = planned.apply(None, ctx)?;
    Ok(out.applied(&planned, &report, Outcome::Applied, || render_report(&report, drive)))
}
pub(crate) fn render_plan(p: &EditPlan) {
    say!("{}: {}", style(&p.drive).bold(), p.databases.join(" and "));
    for c in &p.changes {
        say!("  - {c}");
    }
    for path in &p.playlists_created {
        say!("    {} {path}", style("+").green());
    }
    for path in &p.playlists_deleted {
        say!("    {} {path}", style("-").red());
    }
    for c in &p.playlists_changed {
        let what = if c.reordered {
            "reordered".to_string()
        } else {
            let mut parts = Vec::new();
            if c.added > 0 {
                parts.push(format!("+{}", c.added));
            }
            if c.removed > 0 {
                parts.push(format!("-{}", c.removed));
            }
            if parts.is_empty() { "entries changed".into() } else { parts.join(" ") }
        };
        say!("    {} {} {}", style("~").yellow(), c.path, dim(format!("{what}, {} -> {} entries", c.before, c.after)));
    }
    if p.tracks_changed > 0 {
        say!("    {} {} track(s) changed", style("~").yellow(), p.tracks_changed);
    }
    if !p.tracks_removed.is_empty() {
        say!(
            "    {} {} track(s) removed{}",
            style("-").red(),
            p.tracks_removed.len(),
            if p.files.is_empty() { String::new() } else { format!(", {} file(s), {}", p.files.len(), bytes(p.bytes)) }
        );
        for t in p.tracks_removed.iter().take(10) {
            say!("        {}", dim(&t.name));
        }
        if p.tracks_removed.len() > 10 {
            say!("        {}", dim(format!("and {} more", p.tracks_removed.len() - 10)));
        }
    }
    if p.history_removed > 0 {
        say!("    {} {} history list(s) removed", style("-").red(), p.history_removed);
    }
    for w in &p.warnings {
        warn(w);
    }
}

pub(crate) fn render_report(r: &EditReport, drive: &str) {
    output::ok(format!(
        "{} change(s) written to {}{}",
        r.changes,
        r.databases.join(" and "),
        if r.files_deleted > 0 {
            format!("; {} file(s) deleted, {} freed", r.files_deleted, bytes(r.freed_bytes))
        } else {
            String::new()
        }
    ));
    crate::output::hint(format!(
        "Undo with `locallibrary undo {drive}`. rekordbox's next export replaces these changes; \
             `locallibrary edits replay {drive}` makes them again."
    ));
}

/// A drive's playlist tree (`playlist ls`, and `playlists`).
pub fn tree(drive: Option<String>, out: Output) -> Result<ExitCode> {
    let reads = crate::each_drive::read(drive, |drive| Ok(api::drive_library(&DriveRef { drive })?.library.playlists))?;
    out.page();
    let outcome = reads.emit_headed(out, |playlists| {
        render::playlist_tree(&Library { playlists: playlists.clone(), ..Default::default() })
    });
    Ok(outcome.exit())
}

pub fn playlist(args: PlaylistArgs, out: Output, ctx: &Ctx<'_>) -> Result<ExitCode> {
    let command = match (args.command, args.drive) {
        (Some(c), _) => c,
        (None, Some(drive)) => {
            crate::help::check_drive_word("playlist", &drive)?;
            PlaylistCommand::Ls { drive: Some(drive) }
        }
        (None, None) => PlaylistCommand::Ls { drive: None },
    };
    let (drive, edits, dry) = match command {
        PlaylistCommand::Ls { drive } => return tree(drive, out),
        PlaylistCommand::Import { drive, file, into, from, append, choose, dry } => {
            let mut choices = indexmap::IndexMap::new();
            for c in choose {
                let parsed = c.split_once('=').and_then(|(e, t)| {
                    Some((e.trim().parse::<usize>().ok()?, t.trim().trim_start_matches('#').parse::<u32>().ok()?))
                });
                let Some((entry, track)) = parsed.filter(|(e, _)| *e > 0) else {
                    bail!("--choose takes ENTRY=TRACK, e.g. 3=812")
                };
                choices.insert(entry - 1, track);
            }
            let req = api::ImportRequest { drive, file, into, playlist: from, append, choices };
            return import(req, dry.dry_run, out, ctx);
        }
        PlaylistCommand::New { drive, path, folder, select, dry } => {
            let mut edits = vec![Edit::CreatePlaylist { path: path.clone(), folder }];
            if !select.selector().is_empty() {
                if folder {
                    bail!("a folder holds playlists, not tracks");
                }
                edits.push(Edit::AddTracks {
                    playlist: path,
                    tracks: select.selector(),
                    position: None,
                    sort: None,
                    allow_repeats: false,
                });
            }
            (drive, edits, dry)
        }
        PlaylistCommand::Rename { drive, playlist, name, dry } => {
            (drive, vec![Edit::RenamePlaylist { playlist, name }], dry)
        }
        PlaylistCommand::Move { drive, playlist, at, to: Some(to), dry, .. } if !at.is_empty() => {
            (drive, vec![Edit::MoveTracks { playlist, positions: at, to }], dry)
        }
        PlaylistCommand::Move { drive, playlist, into, top, position, dry, .. } => {
            let into = if top {
                Some(String::new())
            } else {
                match into {
                    Some(f) => Some(f),
                    // Reordering in place: the current parent folder.
                    None => {
                        let lib = api::resolve_drive(&drive)?.library()?;
                        let p =
                            lib.find_playlist(&playlist).ok_or_else(|| anyhow::anyhow!("no playlist {playlist:?}"))?;
                        Some(p.parent_id.map(|id| lib.playlist_path(id)).unwrap_or_default())
                    }
                }
            };
            (drive, vec![Edit::MovePlaylist { playlist, into, position }], dry)
        }
        PlaylistCommand::Delete { drive, playlist, with_tracks, dry } => {
            (drive, vec![Edit::DeletePlaylist { playlist, with_tracks }], dry)
        }
        PlaylistCommand::Add { drive, playlist, select, position, sort, allow_repeats, dry } => {
            let tracks = selector(&select)?;
            (drive, vec![Edit::AddTracks { playlist, tracks, position, sort, allow_repeats }], dry)
        }
        PlaylistCommand::Remove { drive, playlist, select, positions, dry } => {
            let tracks = (!select.selector().is_empty()).then(|| select.selector());
            if tracks.is_none() && positions.is_empty() {
                return Err(crate::jobs_cmd::usage(
                    "say which tracks to remove (--track, --query, --filter...) or which positions (--at 3,4)",
                ));
            }
            (drive, vec![Edit::RemoveTracks { playlist, tracks, positions }], dry)
        }
        PlaylistCommand::Reorder { drive, playlist, positions, to, dry } => {
            (drive, vec![Edit::MoveTracks { playlist, positions, to }], dry)
        }
        PlaylistCommand::Sort { drive, playlist, by, dry } => (drive, vec![Edit::SortPlaylist { playlist, by }], dry),
        PlaylistCommand::Dedupe { drive, playlist, recordings, dry } => {
            (drive, vec![Edit::Dedupe { playlist, recordings }], dry)
        }
        PlaylistCommand::Order { drive, playlist, start, into, dry } => {
            let start = start.map(|id| TrackSelector { ids: vec![id], ..Default::default() });
            (drive, vec![Edit::PlanSet { playlist, start, into }], dry)
        }
        PlaylistCommand::Fill { drive, playlist, filter, sort, limit, dry } => {
            let tracks = TrackSelector { filter: Some(filter), ..Default::default() };
            (drive, vec![Edit::SetTracks { playlist, tracks, sort, limit, create: true }], dry)
        }
        PlaylistCommand::Unfiled { drive, name, dry } => {
            let tracks = TrackSelector { filter: Some("is:unfiled".into()), ..Default::default() };
            (
                drive,
                vec![Edit::SetTracks {
                    playlist: name,
                    tracks,
                    sort: Some("artist,title".into()),
                    limit: None,
                    create: true,
                }],
                dry,
            )
        }
    };
    run_edits(drive, edits, dry.dry_run, out, ctx)
}

pub fn track(args: TrackArgs, out: Output, ctx: &Ctx<'_>) -> Result<ExitCode> {
    let command = match (args.command, args.drive) {
        (Some(c), _) => c,
        (None, Some(drive)) => {
            crate::help::check_drive_word("track", &drive)?;
            TrackCommand::Show { drive, select: args.select }
        }
        (None, None) => {
            return Err(crate::jobs_cmd::usage(
                "give a library or drive and the tracks: `locallibrary track LIBRARY --query ...` shows them",
            ));
        }
    };
    let (drive, edits, dry) = match command {
        TrackCommand::Set { drive, select, fields, dry } => {
            let tracks = selector(&select)?;
            (drive, vec![Edit::EditTracks { tracks, fields: fields.fields() }], dry)
        }
        TrackCommand::Rm { drive, select, keep_files, dry } => {
            (drive, vec![Edit::DeleteTracks { tracks: selector(&select)?, keep_files }], dry)
        }
        TrackCommand::Replace { drive, duplicate, keep, keep_duplicate, dry } => {
            let one = |id| TrackSelector { ids: vec![id], ..Default::default() };
            (drive, vec![Edit::ReplaceTrack { duplicate: one(duplicate), keep: one(keep), keep_duplicate }], dry)
        }
        TrackCommand::Dedupe { drive, hash_audio, by_sound, metadata, dry } => {
            let req = api::DedupeRequest { drive: drive.clone(), hash_audio, by_sound, metadata };
            return confirm_and_apply(&drive, api::dedupe_plan(&req, ctx)?, dry.dry_run, out, ctx);
        }
        TrackCommand::Relocate { drive, folders, dry } => {
            return crate::relocate_cmd::run(drive, folders, dry.dry_run, out, ctx);
        }
        TrackCommand::Show { drive, select } => {
            let list = api::track_details(&api::TrackDetailsRequest { drive, tracks: selector(&select)? })?;
            out.emit_paged(&list, |l| {
                for (i, d) in l.iter().enumerate() {
                    if i > 0 {
                        say!();
                    }
                    crate::render::track_details(d);
                }
            });
            return Ok(ExitCode::SUCCESS);
        }
        TrackCommand::Matches { drive, track, limit } => {
            let list = api::harmonic_matches(&api::MatchesRequest { drive, track, limit: Some(limit) })?;
            out.emit(&list, |l| {
                if l.is_empty() {
                    say!("no tracks on the drive mix into that one (its key may be unknown)");
                }
                for m in l {
                    say!(
                        "{:>6}  {}  {:>6.2}  {:<4} {}",
                        format!("#{}", m.track.id),
                        output::fit(&m.track.display_name(), 48),
                        m.track.bpm,
                        m.key,
                        dim(format!("{}, {:.1}%", m.relation, m.tempo_gap * 100.0))
                    );
                }
            });
            return Ok(ExitCode::SUCCESS);
        }
    };
    run_edits(drive, edits, dry.dry_run, out, ctx)
}

pub fn smart(args: SmartArgs, store: &Store, out: Output, ctx: &Ctx<'_>) -> Result<ExitCode> {
    let show = |all: &locallibrary::smart::SmartPlaylists| {
        if all.playlists.is_empty() {
            say!("no smart playlists (add one with `locallibrary smart add PATH --filter ...`)");
        }
        for p in &all.playlists {
            let mut extra = Vec::new();
            if let Some(s) = &p.sort {
                extra.push(format!("sorted by {s}"));
            }
            if let Some(n) = p.limit {
                extra.push(format!("at most {n}"));
            }
            if !p.drives.is_empty() {
                extra.push("some libraries only".to_string());
            }
            say!("{}  {}  {}", style(&p.path).bold(), p.filter, dim(extra.join(", ")));
        }
    };
    match args.command.unwrap_or(SmartCommand::List) {
        SmartCommand::List => out.emit(&api::smart_list(store)?, show),
        SmartCommand::Add { path, filter, sort, limit, drives } => {
            let rule = locallibrary::smart::SmartPlaylist {
                path: path.clone(),
                filter: filter.clone(),
                sort: sort.clone(),
                limit,
                drives: Vec::new(),
            };
            rule.validate()?;
            let change = match api::smart_list(store)?.get(&path) {
                Some(old) => format!("replace the rule for {path:?} ({}) with {filter:?}", old.filter),
                None => format!("add a rule for {path:?}: {filter:?}"),
            };
            let req = api::SetSmartPlaylist { path, filter, sort, limit, drives };
            return out.one_step(
                &change,
                "Save it?",
                || Ok(api::smart_set(store, &req)?),
                |a| {
                    show(a);
                    crate::output::hint("Run `locallibrary smart apply` to write them to your drives.");
                },
            );
        }
        SmartCommand::Rm { path } => {
            let rule = api::smart_list(store)?.remove(&path)?;
            let change = format!("remove the rule for {:?} (drives keep their playlists)", rule.path);
            let req = api::RemoveSmartPlaylist { path };
            return out.one_step(&change, "Remove it?", || Ok(api::smart_remove(store, &req)?), show);
        }
        SmartCommand::Apply { drives, dry } => {
            let targets = match drives.is_empty() {
                true => crate::each_drive::targets(Some(store))?,
                false => api::libraries_once(drives)?,
            };
            return smart_apply(store, targets, dry.dry_run, out, ctx);
        }
    }
    Ok(ExitCode::SUCCESS)
}

/// `smart apply`: every drive's plan shown, one question, then each drive written.
fn smart_apply(store: &Store, drives: Vec<String>, dry_run: bool, out: Output, ctx: &Ctx<'_>) -> Result<ExitCode> {
    let mut plans = Vec::new();
    for drive in drives {
        plans.push((drive.clone(), api::smart_apply_plan(store, &DriveRef { drive }, ctx)?));
    }
    let show = |drive: &str, p: &Planned<EditPlan>| match p.noop {
        true => {
            for w in &p.warnings {
                warn(w);
            }
            output::ok(format!("{drive}: nothing to change"));
        }
        false => {
            render_plan(p);
            output::clones(p);
        }
    };
    crate::each_drive::apply_each(plans, dry_run, "Make these changes", out, ctx, show, |drive, r| {
        render_report(r, drive)
    })
}

pub fn edits(args: EditsArgs, out: Output, ctx: &Ctx<'_>) -> Result<ExitCode> {
    let command = match (args.command, args.drive) {
        (Some(c), _) => c,
        (None, Some(drive)) => {
            crate::help::check_drive_word("edits", &drive)?;
            EditsCommand::List { drive: Some(drive) }
        }
        (None, None) => EditsCommand::List { drive: None },
    };
    match command {
        EditsCommand::List { drive } => {
            let reads = crate::each_drive::read(drive, |drive| api::edits_list(&DriveRef { drive }))?;
            out.page();
            let outcome = reads.emit_headed(out, |l| {
                if l.is_empty() {
                    say!("no edits recorded on this drive");
                }
                for e in l {
                    say!("{}", style(e.at.format("%Y-%m-%d %H:%M")).bold());
                    for c in &e.changes {
                        say!("  - {c}");
                    }
                }
            });
            Ok(outcome.exit())
        }
        EditsCommand::Replay { drive, dry } => {
            let req = DriveRef { drive: drive.clone() };
            confirm_and_apply(&drive, api::edits_replay_plan(&req, ctx)?, dry.dry_run, out, ctx)
        }
        EditsCommand::Clear { drive, dry } => {
            let plan = api::edits_clear_plan(&api::ClearEdits { drive: drive.clone(), before: None })?;
            if plan.noop {
                return Ok(out.nothing(&plan, "no edits recorded on this drive"));
            }
            if !out.json {
                output::summary(&plan.summary, &plan.warnings);
            }
            if let Some(o) = out.decide(dry.dry_run, &format!("Forget the edits recorded on {drive}?")) {
                return Ok(out.not_applied(&plan, o));
            }
            let r = plan.apply(None, ctx)?;
            Ok(out
                .applied(&plan, &r, Outcome::Applied, || output::ok(format!("forgot {} recorded edit(s)", r.removed))))
        }
    }
}

/// `pack`: a drive for a set, holding only chosen playlists of another drive.
pub fn pack(
    req: api::PackRequest,
    dry_run: bool,
    store: Option<&Store>,
    out: Output,
    ctx: &Ctx<'_>,
) -> Result<ExitCode> {
    let plan = api::pack_plan(&req, ctx)?;
    if !out.json {
        say!(
            "{} -> {}: {} ({} track(s), {})",
            style(&plan.source).bold(),
            style(&plan.target).bold(),
            plan.playlists.join(", "),
            plan.tracks,
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
        for w in &plan.warnings {
            warn(w);
        }
    }
    let question = if plan.replaces_library {
        format!("Replace the library on {} with this?", plan.target)
    } else {
        format!("Update {}?", plan.target)
    };
    if let Some(o) = out.decide(dry_run, &question) {
        return Ok(out.not_applied(&plan, o));
    }
    let report = plan.apply(store, ctx)?;
    Ok(out.applied(&plan, &report, Outcome::Applied, || {
        if let Some(s) = &report.saved {
            output::ok(format!("saved snapshot {} ({})", s.short_id, s.message.as_deref().unwrap_or("")));
        }
        output::ok(format!(
            "packed {}: {} file(s) copied ({}), {} removed",
            plan.target,
            report.copied,
            bytes(report.copied_bytes),
            report.deleted
        ));
        crate::output::hint(format!("Run the same command without --playlist to update {} later.", plan.target));
        if let Some(b) = &report.backup {
            crate::output::hint(format!("undo with: locallibrary undo {:?} (backup {b})", req.target));
        }
    }))
}

fn import(req: api::ImportRequest, dry_run: bool, out: Output, ctx: &Ctx<'_>) -> Result<ExitCode> {
    use locallibrary::import::Status;
    let preview = api::import_plan(&req, ctx)?;
    if !out.json {
        let r = &preview.report;
        say!(
            "{}: {} entr(ies) as {:?}: {} found, {} ambiguous, {} missing",
            req.file.display(),
            r.summary.entries,
            r.format,
            r.summary.found,
            r.summary.ambiguous,
            r.summary.missing
        );
        let lib = api::resolve_drive(&req.drive)?.library()?;
        let name = |id: u32| lib.track(id).map(|t| t.display_name()).unwrap_or_else(|| format!("#{id}"));
        for (i, m) in r.entries.iter().enumerate() {
            match &m.status {
                Status::Found { .. } => {}
                Status::Ambiguous { candidates } => {
                    warn(format!("{:>3}. {} could be:", i + 1, m.entry.raw.trim()));
                    for c in candidates.iter().take(5) {
                        say!("         #{c}  {}", name(*c));
                    }
                }
                Status::Missing => say!("  {} {:>3}. {}", style("?").yellow(), i + 1, dim(m.entry.raw.trim())),
            }
        }
        if r.summary.ambiguous > 0 {
            crate::output::hint("Settle ambiguous entries with --choose ENTRY=TRACK; they're left out otherwise.");
        }
    }
    let Some(plan) = &preview.plan.plan else {
        if !out.json {
            warn("none of the list's tracks are on the drive");
        }
        return Ok(out.not_applied(&preview, Outcome::NothingToDo));
    };
    if !out.json {
        render_plan(plan);
        output::clones(&preview);
    }
    if let Some(o) = out.decide(dry_run, "Make this playlist?") {
        return Ok(out.not_applied(&preview, o));
    }
    let report = preview.apply(None, ctx)?;
    Ok(out.applied(&preview, &report, Outcome::Applied, || render_report(&report, &req.drive)))
}
