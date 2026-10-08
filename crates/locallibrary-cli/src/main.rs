//! `locallibrary`: keep rekordbox USB drives in sync, backed up, merged and configured.

// First, for its `say!`.
#[macro_use]
mod output;

#[cfg(feature = "account")]
mod account_cmd;
mod check_cmd;
mod collection_cmd;
mod completion;
mod cues_cmd;
mod diff_cmd;
mod doctor_cmd;
mod drive_cmd;
mod each_drive;
mod edit_cmd;
mod export_cmd;
mod help;
mod hints;
mod jobs_cmd;
mod library_cmd;
mod mix_cmd;
mod pager;
mod players_cmd;
mod plays_cmd;
mod profile_cmd;
#[cfg(feature = "account")]
mod publish_cmd;
mod render;
mod restore_cmd;
mod rpc;
mod sets_cmd;
mod store_cmd;
mod tags_cmd;
#[cfg(feature = "tui")]
mod tui;
mod watch_cmd;

use std::collections::BTreeMap;
#[cfg(feature = "tui")]
use std::io::IsTerminal;
use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{Context, Result, bail};
use clap::{Args, FromArgMatches, Parser, Subcommand, ValueEnum};
use locallibrary::Store;
use locallibrary::api::{self, Direction, TrackSelector};
use locallibrary::format::Filesystem;
use locallibrary::merge::{MatchMode, MergeOptions, PlaylistMode};
use locallibrary::progress::{CancelToken, Ctx};

use crate::output::{Outcome, Output};

/// The help of every `<DRIVE>` argument.
pub const DRIVE_HELP: &str = "The drive: its mount path, name or id prefix";

/// The help of a `[DRIVE]` argument that can be left out: the command then reads every mounted
/// rekordbox drive.
pub const DEFAULT_DRIVE_HELP: &str = "The drive: its mount path, name or id prefix [default: every mounted \
rekordbox drive]";

/// The help of a `[DRIVE]` argument that can be left out for one drive (see
/// [`jobs_cmd::drive_or_default`]).
pub const ONE_DRIVE_HELP: &str = "The drive: its mount path, name or id prefix [default: the only mounted \
rekordbox drive, or the library's primary]";

/// Where the global flags show in every command's help.
const GLOBAL: &str = "Global options";

/// What `locallibrary` on its own does, for the help: opens the player, when it's built in.
#[cfg(feature = "tui")]
macro_rules! on_its_own {
    () => {
        " `locallibrary` on its own opens the interactive player."
    };
}
#[cfg(not(feature = "tui"))]
macro_rules! on_its_own {
    () => {
        ""
    };
}

/// The help after the commands: common tasks and what a drive is. (A macro, for `concat!`.)
macro_rules! after_help {
    () => {
        concat!(
            "Common tasks:
  locallibrary setup                      first run: name your drives, start your library
  locallibrary status                     every mounted drive at a glance
  locallibrary refresh                    after a rekordbox export: put back what it replaced
  locallibrary prep                       the night before a set: save, sync, check, eject
  locallibrary sync                       bring the drive group up to date from its newest drive
  locallibrary sync --from Main Backup    copy one drive onto another
  locallibrary pull                       save the mounted drives in the store
  locallibrary undo Main                  take back the last change to a drive

Drives are a mount path (/Volumes/USB), a name (\"Club drive\") or an id prefix.",
            on_its_own!(),
            " `locallibrary help COMMAND` (or COMMAND --help) says more."
        )
    };
}

const AFTER_HELP: &str = after_help!();

const AFTER_LONG_HELP: &str = concat!(
    after_help!(),
    "

Exit status: 0 done (or nothing to do, or a dry run), 1 error, 2 usage, 3 not applied (declined, or no \
terminal to ask on and no --yes), 4 problems found (doctor, check, verify, fsck, store verify), 5 some of \
several failed (drives, files), 130 cancelled (Ctrl-C). With --json, results that plan or check say which \
as \"outcome\", and errors as {\"error\": {kind, message}, \"outcome\"}. Every command that writes prints \
{schema, plan, report, applied, outcome}."
);

#[derive(Parser)]
#[command(name = "locallibrary", version, about = "Store, sync, compare, merge and configure rekordbox USB libraries", long_about = None)]
#[command(after_help = AFTER_HELP, after_long_help = AFTER_LONG_HELP)]
pub struct Cli {
    /// Store location [default: $LOCALLIBRARY_STORE or the platform data directory]
    #[arg(long, global = true, env = "LOCALLIBRARY_STORE", value_name = "DIR", help_heading = GLOBAL)]
    store: Option<PathBuf>,
    /// Machine-readable JSON output (never prompts)
    #[arg(long, global = true, help_heading = GLOBAL)]
    json: bool,
    /// Don't ask for confirmation
    #[arg(short, long, global = true, help_heading = GLOBAL)]
    yes: bool,
    /// No progress bars and no hints (warnings and results still print)
    #[arg(short, long, global = true, help_heading = GLOBAL)]
    quiet: bool,
    /// Write to an archive or retired drive (any command that writes to a drive)
    #[arg(long, global = true, help_heading = GLOBAL)]
    overwrite_archive: bool,
    /// When to colour the output (NO_COLOR also turns it off). Before the command: `track set
    /// --color` is a track's colour
    #[arg(long, value_enum, value_name = "WHEN", default_value_t = output::ColorWhen::Auto, help_heading = GLOBAL)]
    color: output::ColorWhen,
    /// Plain ASCII symbols (v x * -> ...) instead of ✓ ✗ ★ →; the default when the locale isn't UTF-8
    #[arg(long, global = true, help_heading = GLOBAL)]
    ascii: bool,
    /// Print long lists straight to the terminal instead of through a pager ($PAGER, else less)
    #[arg(long, global = true, help_heading = GLOBAL)]
    no_pager: bool,
    /// Without one, opens the interactive player (`locallibrary tui`) in a terminal.
    #[command(subcommand)]
    command: Option<Command>,
}

// Parsed once per run; the size difference between variants doesn't matter.
#[allow(clippy::large_enum_variant)]
#[derive(Subcommand)]
enum Command {
    /// Every mounted drive against its library's latest snapshot, plus settings drift, roles and
    /// problems
    Status {
        /// Also every registered drive that isn't mounted, and how far behind it was (`offline` in
        /// --json's `{mounted, offline, duplicates}`)
        #[arg(long)]
        all: bool,
        /// With --all, retired drives too
        #[arg(long, requires = "all")]
        retired: bool,
    },
    /// Bring drives in sync: the newest one is copied onto the others (--from: that one, onto
    /// another)
    #[command(after_help = SYNC_HELP, after_long_help = SYNC_LONG_HELP)]
    Sync {
        /// Two or more drives [default: the mounted drive group]; with --from, the drive to copy onto
        #[arg(value_name = "DRIVE")]
        drives: Vec<String>,
        /// Copy this drive onto the drive given, overwriting its library, whichever is newer
        #[arg(long, value_name = "DRIVE", conflicts_with_all = ["library", "allow_guess"])]
        from: Option<String>,
        /// The library whose drive group to sync (with no drives given)
        #[arg(long, short)]
        library: Option<String>,
        #[command(flatten)]
        opts: SyncFlags,
        /// Accept a low-confidence verdict about which drive is newer
        #[arg(long)]
        allow_guess: bool,
    },
    /// Snapshot a drive into the store (with no drive: every mounted drive of one library)
    #[command(after_long_help = "Examples:\n  locallibrary pull Main\n  \
locallibrary pull    # every mounted drive of the library\n  \
locallibrary pull Main -m \"after Friday's export\" --label \"before NYE\"\n  \
locallibrary pull /Volumes/NEW --library Main    # into an existing library")]
    Pull {
        /// The drive: its mount path, name or id prefix [default: the mounted drives of one
        /// library, the newest export first, then drives back from a set beside it]
        drive: Option<String>,
        /// Library to pull into [default: the drive's, else a new one named after the drive]
        #[arg(long, short)]
        library: Option<String>,
        /// Start a new library if none has the --library name (otherwise you're asked, or it's refused)
        #[arg(long, requires = "library")]
        new_library: bool,
        /// Note for this snapshot
        #[arg(long, short)]
        message: Option<String>,
        /// Name the snapshot, e.g. "before NYE": shown in `library log`, usable as its id, and kept by
        /// `store prune`
        #[arg(long)]
        label: Option<String>,
        /// Record the drive's state even if it isn't a descendant of the library's latest snapshot
        #[arg(long)]
        force: bool,
    },
    /// Before a set: same library on every drive, nothing missing, history saved, readable by the players
    Check(check_cmd::CheckArgs),
    /// rekordbox just exported to a drive: put back what the export replaced, then save it
    #[command(after_help = jobs_cmd::REFRESH_HELP)]
    Refresh(jobs_cmd::RefreshArgs),
    /// The night before a set: save the drives, sync them, check them for the players, eject them
    #[command(after_help = jobs_cmd::PREP_HELP)]
    Prep(jobs_cmd::PrepArgs),
    /// First run: name your drives, start your library, import My Settings, remember your players
    #[command(after_help = jobs_cmd::SETUP_HELP)]
    Setup(jobs_cmd::SetupArgs),
    /// Take back the last change locallibrary made to a drive (again: the one before it)
    #[command(after_help = UNDO_HELP, after_long_help = UNDO_LONG_HELP)]
    Undo {
        /// The drive: its mount path, name or id prefix [default: the only mounted drive with
        /// something to undo]
        drive: Option<String>,
        /// What can be undone, on every mounted drive (or this one), newest first
        #[arg(long)]
        list: bool,
        /// Put it back even if the drive changed since, or the backup references audio that is no
        /// longer on the drive
        #[arg(long)]
        force: bool,
        /// Show what would be put back without changing anything
        #[arg(long)]
        dry_run: bool,
    },
    /// Health report: duplicates, missing and orphaned files, unfiled tracks, database drift, files too big for FAT32
    Doctor(doctor_cmd::DoctorArgs),
    /// List, search, filter and sort a drive's tracks
    #[command(after_help = FILTER_HELP, after_long_help = TRACKS_LONG_HELP)]
    Tracks(TracksArgs),
    /// Show a drive's playlist tree, or edit its playlists and folders: create, rename, move,
    /// delete, add and remove tracks, sort
    #[command(after_help = FILTER_HELP)]
    Playlist(edit_cmd::PlaylistArgs),
    /// Show tracks, change their details (rating, colour, genre...), delete them, replace duplicates
    #[command(after_help = FILTER_HELP)]
    Track(edit_cmd::TrackArgs),
    /// Interactive player: browse drives and playlists, play tracks with their rekordbox waveforms
    #[cfg(feature = "tui")]
    Tui {
        /// Drive to open [default: the first mounted rekordbox drive]
        drive: Option<String>,
    },
    #[command(flatten)]
    DriveManagement(drive_cmd::TopCommand),
    /// The libraries in the store: list them, their snapshot history, write one to a drive, prune it
    Library(library_cmd::LibraryArgs),
    /// What players recorded: sessions (history lists), play counts, writing a session out,
    /// importing one from Beat Link Trigger
    Plays(plays_cmd::PlaysArgs),
    /// The sets you played: declare them from sessions and recordings, edit them, write their
    /// tracklists out, make playlists of them, publish them
    #[command(alias = "set")]
    Sets(sets_cmd::SetsArgs),
    /// Playlist entries added, removed and moved between two drives or snapshots (--summary:
    /// which drive is newer, and why)
    Diff(diff_cmd::DiffArgs),
    /// My Tags (Track Filter, Related Tracks): list them, tag tracks, rename or delete tags
    Tags(tags_cmd::TagsArgs),
    /// A track's hot cues, memory cues and loops: list, set, rename, recolour, delete, copy
    Cues(cues_cmd::CuesArgs),
    /// Cover art on drives and in audio files
    Art(ArtArgs),
    /// The edits made to a drive, to make them again after rekordbox exports over them
    Edits(edit_cmd::EditsArgs),
    /// Rule-based playlists: kept in the store, written to drives as ordinary playlists
    #[command(after_help = FILTER_HELP)]
    Smart(edit_cmd::SmartArgs),
    /// Player/mixer "My Settings": one profile for every drive, with per-drive overrides
    Settings(SettingsArgs),
    /// Merge another library's tracks, analysis, artwork and playlists into a drive
    #[command(
        after_long_help = "Examples:\n  locallibrary merge \"Friend's USB\" Main --folder Sam --only \"Sets/Saturday\"\n  \
locallibrary merge Backup Main --playlists by-name --dry-run"
    )]
    Merge {
        /// The drive to take tracks and playlists from
        source: String,
        /// The drive to add them to
        target: String,
        /// How to bring playlists across
        #[arg(long, value_enum, default_value_t = PlaylistArg::Folder)]
        playlists: PlaylistArg,
        /// Folder name for --playlists folder [default: source drive's name]
        #[arg(long)]
        folder: Option<String>,
        /// Only merge these source playlists (and their tracks). Repeatable.
        #[arg(long = "only", value_name = "PLAYLIST")]
        only: Vec<String>,
        /// Treat tracks as duplicates by path+size only (default also matches artist/title/duration)
        #[arg(long)]
        match_path_only: bool,
        /// Also bring the source's history playlists (what players recorded) across
        #[arg(long)]
        history: bool,
        /// Show the plan without changing anything
        #[arg(long)]
        dry_run: bool,
    },
    /// Put only some playlists of a drive (and the tracks they use) on another drive, e.g. for a set
    #[command(
        after_long_help = "Examples:\n  locallibrary pack Main /Volumes/TONIGHT --playlist \"Sets/2026/Friday\" --playlist Classics\n  \
locallibrary pack Main /Volumes/TONIGHT    # later: the same playlists, copying only what changed"
    )]
    Pack {
        /// The drive to take the playlists from
        source: String,
        /// The drive to write them to
        target: String,
        /// Playlist or folder to include (repeatable) [default: what the target was packed with last]
        #[arg(long = "playlist", value_name = "PLAYLIST")]
        playlists: Vec<String>,
        /// Also bring the source's history lists
        #[arg(long)]
        history: bool,
        /// Keep files on the target that the packed library doesn't use
        #[arg(long)]
        keep_extra: bool,
        /// Show the plan without changing anything
        #[arg(long)]
        dry_run: bool,
    },
    /// Build OneLibrary (CDJ-3000X, OPUS-QUAD, XDJ-AZ...) from export.pdb, or the reverse
    Convert {
        #[arg(help = DRIVE_HELP)]
        drive: String,
        /// Database to build
        #[arg(long, value_enum)]
        to: ConvertArg,
        /// Rebuild it if the drive already has one (fixes drift between the two)
        #[arg(long)]
        replace: bool,
        /// Show the plan without changing anything
        #[arg(long)]
        dry_run: bool,
    },
    /// Remove useless tracks (rekordbox's bundled demo samples) and their files from a drive
    Clean {
        #[arg(help = DRIVE_HELP)]
        drive: String,
        /// Show what would be removed without removing anything
        #[arg(long)]
        dry_run: bool,
    },
    /// Which of a drive's tracks a recorded mix plays, and when; drawn as an interactive HTML report
    Mix(mix_cmd::MixArgs),
    /// What your sets, collection and settings say about how you play, and what a public profile would show
    #[command(after_help = PROFILE_HELP)]
    Profile(profile_cmd::ProfileArgs),
    /// Sign in with your Atmosphere account (the one you use for Bluesky and other AT Protocol apps), or sign out
    #[cfg(feature = "account")]
    Account(account_cmd::AccountArgs),
    /// rekordbox's own collection on this computer (master.db), read only: info, tracks, playlists, and what a drive lacks
    Collection(collection_cmd::CollectionArgs),
    /// The player models `check` uses when none are given, and what each model reads
    Players(players_cmd::PlayersArgs),
    /// Inspect and maintain the store
    Store {
        #[command(subcommand)]
        command: Option<StoreCommand>,
    },
    /// Save drives into the store as they're plugged in (history lists rekordbox would delete), and keep their group in sync
    Watch(watch_cmd::WatchArgs),
    /// Write a playlist, folder, history list or the whole library as M3U8, CSV or text, or for other DJ software
    Export(export_cmd::ExportArgs),
    /// Export a drive's library as rekordbox XML (import it into rekordbox to recover a library)
    Xml {
        #[arg(help = ONE_DRIVE_HELP)]
        drive: Option<String>,
        /// The file to write
        #[arg(long, short, default_value = "rekordbox.xml")]
        output: PathBuf,
        /// Where the audio will live when importing [default: the drive's mount point]
        #[arg(long)]
        location_root: Option<PathBuf>,
        /// Metadata and playlists only (no beat grids or cues)
        #[arg(long)]
        no_analysis: bool,
        /// Only this playlist or folder and its tracks (repeatable)
        #[arg(long = "only", value_name = "PLAYLIST")]
        only: Vec<String>,
        /// Replace the file if it's there already
        #[arg(long)]
        replace: bool,
    },
    /// Shell completion: the script to load in bash, zsh, fish, elvish or PowerShell
    Completions(completion::CompletionsArgs),
    /// Man pages for every command
    #[command(hide = true)]
    Man(completion::ManArgs),
    /// JSON-RPC 2.0 server on stdin/stdout, for GUI front ends
    #[command(hide = true)]
    Rpc {
        /// Requests of each kind (quick reads, and everything else) run at once
        #[arg(long, value_name = "N", env = "LOCALLIBRARY_RPC_WORKERS", default_value_t = rpc::WORKERS)]
        workers: usize,
        /// Write the sidecar's contract (schema.json, and TypeScript types in locallibrary.d.ts)
        /// to DIR instead of serving
        #[arg(long, value_name = "DIR")]
        emit_schema: Option<PathBuf>,
    },
    /// `library log`
    #[command(hide = true)]
    Log(library_cmd::LogArgs),
    /// `library push`
    #[command(hide = true)]
    Push(library_cmd::PushArgs),
    /// `plays lists`
    #[command(hide = true)]
    History(plays_cmd::HistoryArgs),
    /// `plays stats`
    #[command(hide = true)]
    Stats(plays_cmd::StatsArgs),
    /// `playlist ls`
    #[command(hide = true)]
    Playlists {
        #[arg(help = DEFAULT_DRIVE_HELP)]
        drive: Option<String>,
    },
    /// `sync --from SOURCE TARGET`
    #[command(hide = true)]
    Mirror {
        source: String,
        target: String,
        #[command(flatten)]
        opts: SyncFlags,
    },
    /// `diff --summary`
    #[command(hide = true)]
    Compare(diff_cmd::DiffArgs),
}

impl Command {
    /// The sidecar and the player: they handle their own output (and the player draws the whole
    /// terminal).
    fn owns_output(&self) -> bool {
        match self {
            Command::Rpc { .. } => true,
            #[cfg(feature = "tui")]
            Command::Tui { .. } => true,
            _ => false,
        }
    }
}

const SYNC_HELP: &str = "With no drives, syncs the mounted drives of one library's group (see `locallibrary \
drive groups`); archive and retired drives are left out. A drive back from a set whose only changes are the \
players' (history lists, cues) is saved in the store, then updated.";

const SYNC_LONG_HELP: &str = "With no drives, syncs the mounted drives of one library's group (see `locallibrary \
drive groups`); archive and retired drives are left out. A drive back from a set whose only changes are the \
players' (history lists, cues) is saved in the store, then updated.

Examples:
  locallibrary sync                       the drive group: the newest drive onto the others
  locallibrary sync Main Backup \"Club\"    these drives, whichever is newest
  locallibrary sync --from Main Backup    Main onto Backup, whichever is newer";

const UNDO_HELP: &str = "Every command that changes a drive in place backs it up first: playlist, track, \
My Tag and cue edits, merge, clean, convert, artwork, settings, the player name, the contact file, orphan \
removal and pack. The plan shows the files and databases that change, how the playlists and tracks change, and \
the recorded edits that come off the journal (so `edits replay` doesn't make them again). A drive that changed \
since (a rekordbox export, a sync, history players recorded) is refused unless --force. Older backups: \
`locallibrary drive restore --backup ID`. Art embedded in files on this computer: `locallibrary art unembed`.";

const UNDO_LONG_HELP: &str = "Every command that changes a drive in place backs it up first: playlist, track, \
My Tag and cue edits, merge, clean, convert, artwork, settings, the player name, the contact file, orphan \
removal and pack. The plan shows the files and databases that change, how the playlists and tracks change, and \
the recorded edits that come off the journal (so `edits replay` doesn't make them again). A drive that changed \
since (a rekordbox export, a sync, history players recorded) is refused unless --force. Older backups: \
`locallibrary drive restore --backup ID`. Art embedded in files on this computer: `locallibrary art unembed`.

Examples:
  locallibrary undo Main --dry-run    what taking back the last change would change
  locallibrary undo --list            what can be undone on every mounted drive";

/// The filter syntax, shown after the help of every command that takes `--filter`; `playlist`,
/// `track` and `smart` show it after the group's help. A macro, so `concat!` can add to it.
macro_rules! filter_help {
    () => {
        "Filters: words match artist, title, album, genre, label, key, comment, remixer, mix name or path; field:value matches one field (\"a:b\" in quotes is text); -term excludes; a|b allows either.
  artist: title: album: genre: label: comment: remixer: mix: path:   text
  key:8A  key:Am  key:8A~ (and keys that mix into it)
  bpm:128  bpm:120-128  bpm:>=124  bpm:~128 (within 6%, half/double time)
  rating:>=4  plays:0  year:1999  bitrate:<320  time:>6:00
  color:aqua|pink  color:none  added:2026-09  added:>=2026-01-01
  playlist:Sets/Friday (or a unique name)  tag:Genre/Deep  is:unfiled|played|unplayed  has:artwork|comment|key"
    };
}

const TRACKS_LONG_HELP: &str = concat!(
    filter_help!(),
    "

Examples:
  locallibrary tracks Main -s \"daft punk\"
  locallibrary tracks Main -f \"bpm:120-128 key:8A~ rating:>=4\" --sort key,bpm
  locallibrary tracks Main -p \"Sets/2026/Friday\" -n 20"
);

const PROFILE_HELP: &str = "Built from your sets (`locallibrary sets`; until you declare one, the sessions players
record stand in), each track's rekordbox analysis and tags, and My Settings. The report stays on this computer;
`profile document` is the part a public profile would show: rounded, without paths, list names or dates, and
naming tracks only when you include `signature`. Choose what it shows with include/exclude, pin, hide, alias,
headline and precision.

  locallibrary profile                      your mounted drives and the store
  locallibrary profile Main --since 2026    one drive, sets played this year
  locallibrary profile -s tempo,mixing      just those sections
  locallibrary profile include signature    name your most-played tracks
  locallibrary profile hide \"ID - ID\" --exact
  locallibrary profile document -o profile.json";

const FILTER_HELP: &str = filter_help!();

/// `tracks`.
#[derive(Args)]
struct TracksArgs {
    #[arg(help = DEFAULT_DRIVE_HELP)]
    drive: Option<String>,
    /// Words each track must have in its artist, title, album, genre, label, key, comment, remixer,
    /// mix name or path (free text, as in --filter)
    #[arg(long, short)]
    search: Option<String>,
    /// Only tracks in this playlist, in its order (path like Folder/Name, or a unique name)
    #[arg(long, short)]
    playlist: Option<String>,
    /// Only tracks matching a filter, e.g. "bpm:120-128 key:8A~ rating:>=4" (syntax below)
    #[arg(long, short, allow_hyphen_values = true)]
    filter: Option<String>,
    /// Order, e.g. key,bpm or -rating (title, artist, album, genre, label, key, bpm, rating, color, year, plays, added, time, bitrate, path)
    #[arg(long, allow_hyphen_values = true)]
    sort: Option<String>,
    /// Show at most this many
    #[arg(long, short = 'n')]
    limit: Option<usize>,
    /// The columns, in order: id, name (artist - title), title, artist, album, genre, label, key,
    /// bpm, rating, color, year, plays, added, time, bitrate, path, comment [default:
    /// id,name,bpm,key,time]
    #[arg(long, value_enum, value_delimiter = ',', value_name = "COLUMNS")]
    columns: Vec<render::TrackColumn>,
    /// table: aligned, for reading; tsv or csv: every value whole, with a header row, for other
    /// programs
    #[arg(long, value_enum, default_value_t = render::TableFormat::Table)]
    format: render::TableFormat,
}

#[derive(Args, Clone)]
pub struct SyncFlags {
    /// Compare audio by content hash, not size + modification time (slow, thorough)
    #[arg(long)]
    verify: bool,
    /// Keep files on the target that the source doesn't have (database files excepted)
    #[arg(long)]
    keep_extra: bool,
    /// Delete stale files before copying (when the target is nearly full)
    #[arg(long)]
    delete_first: bool,
    /// Eject the drives afterwards
    #[arg(long)]
    eject: bool,
    /// Show the plan without changing anything
    #[arg(long)]
    dry_run: bool,
}

#[derive(Subcommand)]
enum StoreCommand {
    /// Store location and size
    Info,
    /// Delete snapshots of no library the store has, and the files no snapshot uses
    Gc {
        /// Show what would be deleted without deleting anything
        #[arg(long)]
        dry_run: bool,
    },
    /// Re-hash every stored file
    Verify,
    /// Label a snapshot ("before NYE"): shown in `library log`, usable as its id, kept by `store prune`
    Label {
        /// Snapshot id (prefix) or another label
        snapshot: String,
        /// The label
        label: String,
    },
    /// Take a label off its snapshot
    Unlabel {
        /// The label
        label: String,
    },
    /// Remove old snapshots, keeping each library's latest, labelled ones, drives' last states
    /// and history lists; then free the files nothing uses
    Prune(library_cmd::PruneArgs),
    /// Copy the store to another folder (a NAS, another disk, a cloud-synced folder), or bring
    /// a copy up to date: only what's new is copied, and `--store DIR` reads the copy
    Copy {
        /// The folder to copy the store to
        dir: PathBuf,
        /// Also remove from the copy what the store no longer has (pruned or collected snapshots,
        /// and files nothing in the store uses)
        #[arg(long)]
        mirror: bool,
        /// Re-read the files the copy has already, copying again any that changed
        #[arg(long)]
        verify: bool,
        /// Copy over a copy that was written to since it was last copied to (with --store), or that
        /// has snapshots the store doesn't
        #[arg(long)]
        force: bool,
        /// Show what would be copied without copying
        #[arg(long)]
        dry_run: bool,
    },
    /// Where the store has been copied, when, and whether each copy is up to date
    Copies,
}

/// `settings`: with no subcommand, the profile; with a drive, its settings.
#[derive(Args)]
#[command(args_conflicts_with_subcommands = true)]
struct SettingsArgs {
    #[command(subcommand)]
    command: Option<SettingsCommand>,
    /// A drive whose settings to show, as `settings show` does [default: the profile, as
    /// `settings base` shows it]
    #[arg(value_name = "DRIVE")]
    drive: Option<String>,
}

#[derive(Subcommand)]
enum SettingsCommand {
    /// All settings, their values and defaults
    Schema,
    /// The settings profile: the base values for every drive, and per-drive overrides
    #[command(alias = "profile")]
    Base,
    /// Set values (KEY=VALUE ...), for all drives or as an override for one
    Set {
        /// Settings and their values, like quantize=off (`settings schema` lists them)
        #[arg(required = true, value_name = "KEY=VALUE")]
        values: Vec<String>,
        /// Only for this drive: an override of the profile's value
        #[arg(long)]
        drive: Option<String>,
    },
    /// Remove values from the profile (or a drive's overrides)
    Unset {
        /// The settings to remove, like quantize
        #[arg(required = true, value_name = "KEY")]
        keys: Vec<String>,
        /// Only this drive's overrides
        #[arg(long)]
        drive: Option<String>,
    },
    /// A drive's current settings and how they differ from the profile
    Show {
        #[arg(help = DEFAULT_DRIVE_HELP)]
        drive: Option<String>,
    },
    /// Write the profile to drives
    Apply {
        /// Drives [default: every mounted rekordbox drive but archive and retired ones]
        drives: Vec<String>,
        /// Also write settings files the profile doesn't touch (fills blank drives completely)
        #[arg(long)]
        create_missing: bool,
        /// Show what would change without writing anything
        #[arg(long)]
        dry_run: bool,
    },
    /// Set the profile's base from a drive's settings, rekordbox's own (no argument), or the My
    /// Settings an Atmosphere account published (settings the source doesn't have stay)
    Import {
        /// The drive to take them from [default: rekordbox's settings on this computer]
        drive: Option<String>,
        /// The My Settings this account published (a handle or DID)
        #[cfg(feature = "account")]
        #[arg(long, value_name = "HANDLE", conflicts_with_all = ["drive", "from_account"])]
        from_handle: Option<String>,
        /// The My Settings your signed-in Atmosphere account published
        #[cfg(feature = "account")]
        #[arg(long, conflicts_with = "drive")]
        from_account: bool,
        /// The account's data server, instead of finding it from the handle
        #[cfg(feature = "account")]
        #[arg(long, value_name = "URL")]
        pds: Option<String>,
        /// Replace the whole base with the account's, unsetting what it didn't publish
        #[cfg(feature = "account")]
        #[arg(long)]
        replace: bool,
        /// Show what would change without importing
        #[arg(long)]
        dry_run: bool,
    },
    /// Publish the profile's base to your Atmosphere account, for any computer to import
    /// (per-drive overrides stay here)
    #[cfg(feature = "account")]
    Publish {
        /// Show what would be published without publishing
        #[arg(long)]
        dry_run: bool,
        #[command(flatten)]
        browser: account_cmd::BrowserArgs,
    },
}

/// `art`: with a drive and no subcommand, its tracks without artwork.
#[derive(Args)]
#[command(args_conflicts_with_subcommands = true)]
struct ArtArgs {
    #[command(subcommand)]
    command: Option<ArtCommand>,
    /// A drive whose tracks without artwork to list, as `art missing` does [default: every
    /// mounted rekordbox drive]
    #[arg(value_name = "DRIVE")]
    drive: Option<String>,
}

#[derive(Subcommand)]
enum ArtCommand {
    /// Tracks with no artwork
    Missing {
        #[arg(help = DEFAULT_DRIVE_HELP)]
        drive: Option<String>,
    },
    /// Set artwork for tracks on a drive (image file, or an audio file with embedded art)
    #[command(after_help = FILTER_HELP)]
    Set {
        #[arg(help = DRIVE_HELP)]
        drive: String,
        /// An image, or an audio file whose embedded art to use
        image: PathBuf,
        #[command(flatten)]
        select: SelectArgs,
        /// Show the plan without changing anything
        #[arg(long)]
        dry_run: bool,
    },
    /// Fill in missing artwork from embedded tags or cover.jpg/folder.jpg next to the audio
    Auto {
        /// The drive: its mount path, name or id prefix [default: every mounted rekordbox drive but
        /// archive and retired ones]
        drive: Option<String>,
        /// Show what would be set without changing anything
        #[arg(long)]
        dry_run: bool,
    },
    /// Embed artwork into audio files in your computer's collection (then "Reload Tag" in rekordbox)
    #[command(after_help = "A copy of each file as it was is kept in the store first: `art unembed` puts \
them back (the last 5 embeds are kept).")]
    Embed {
        /// The image to embed
        image: PathBuf,
        /// The audio files to embed it in
        #[arg(required = true)]
        files: Vec<PathBuf>,
        /// Show the files that would be rewritten without changing them
        #[arg(long)]
        dry_run: bool,
    },
    /// Put back the files `art embed` changed, as they were (the latest embed, or --backup)
    Unembed {
        /// Which embed (see --list)
        #[arg(long)]
        backup: Option<String>,
        /// The embeds that can be taken back
        #[arg(long, conflicts_with = "backup")]
        list: bool,
        /// Show what would be put back without changing anything
        #[arg(long)]
        dry_run: bool,
    },
    /// Save a track's artwork from a drive to a file
    Get {
        #[arg(help = DRIVE_HELP)]
        drive: String,
        /// The track's id
        track: u32,
        /// The file to write
        #[arg(long, short)]
        output: PathBuf,
        /// Replace the file if it's there already
        #[arg(long)]
        replace: bool,
    },
}

#[derive(Args)]
pub struct SelectArgs {
    /// Track id(s)
    #[arg(long = "track", value_name = "ID")]
    tracks: Vec<u32>,
    /// Tracks matching "artist - title" or path
    #[arg(long)]
    query: Option<String>,
    /// Every track in a playlist
    #[arg(long)]
    playlist: Option<String>,
    /// Every track on an album
    #[arg(long)]
    album: Option<String>,
    /// Tracks matching a filter, e.g. "bpm:120-128 key:8A~ rating:>=4" (syntax in `locallibrary tracks --help`)
    #[arg(long, allow_hyphen_values = true)]
    filter: Option<String>,
    /// Tracks by audio file, as the drive's database lists it (/Contents/...)
    #[arg(long = "path", value_name = "PATH")]
    paths: Vec<String>,
    /// Every track of a history list (what a player recorded), by name
    #[arg(long)]
    history: Option<String>,
}

impl SelectArgs {
    fn selector(&self) -> TrackSelector {
        TrackSelector {
            ids: self.tracks.clone(),
            query: self.query.clone(),
            playlist: self.playlist.clone(),
            album: self.album.clone(),
            filter: self.filter.clone(),
            paths: self.paths.clone(),
            history: self.history.clone(),
        }
    }
}

#[derive(Clone, Copy, ValueEnum)]
pub enum FsArg {
    /// Every player
    Fat32,
    /// Newer players only (see `locallibrary drive disks`); no 4 GB file limit
    Exfat,
    /// Mac OS Extended, read by most players but not by Windows
    #[value(name = "hfs+", alias = "hfsplus")]
    Hfsplus,
}

impl From<FsArg> for Filesystem {
    fn from(f: FsArg) -> Self {
        match f {
            FsArg::Fat32 => Filesystem::Fat32,
            FsArg::Exfat => Filesystem::Exfat,
            FsArg::Hfsplus => Filesystem::Hfsplus,
        }
    }
}

/// `32K`, `32KB`, `32768`, `1M`.
fn parse_size(s: &str) -> std::result::Result<u32, String> {
    let t = s.trim().to_ascii_uppercase();
    let t = t.trim_end_matches("IB").trim_end_matches('B');
    let (num, mult) = match t.chars().last() {
        Some('K') => (&t[..t.len() - 1], 1024),
        Some('M') => (&t[..t.len() - 1], 1024 * 1024),
        _ => (t, 1),
    };
    num.trim().parse::<u32>().ok().and_then(|n| n.checked_mul(mult)).ok_or_else(|| format!("not a size: {s:?}"))
}

#[derive(Clone, Copy, ValueEnum)]
enum ConvertArg {
    /// exportLibrary.db, from export.pdb: for CDJ-3000X, CDJ-1500X, OPUS-QUAD, OMNIS-DUO, XDJ-AZ, XDJ-AN
    Onelibrary,
    /// export.pdb, from exportLibrary.db: for every other player
    DeviceLibrary,
}

#[derive(Clone, Copy, ValueEnum)]
enum PlaylistArg {
    /// Recreate the source's playlists inside one new folder
    Folder,
    /// Merge into playlists with the same name, creating missing ones
    ByName,
    /// Tracks only
    None,
}

fn main() -> ExitCode {
    // A shell asking what to complete (`COMPLETE=bash locallibrary -- ...`) is answered here.
    completion::complete();
    let cmd = help::command();
    let matches = match cmd.clone().try_get_matches() {
        Ok(m) => m,
        Err(e) => match help::prep_moved(&e) {
            Some(moved) => return usage_error(cmd.clone().error(clap::error::ErrorKind::InvalidSubcommand, moved)),
            None => return usage_error(help::tidy_suggestions(e, &cmd)),
        },
    };
    let cli = match Cli::from_arg_matches(&matches) {
        Ok(cli) => cli,
        Err(e) => return usage_error(e),
    };
    let out = Output { json: cli.json, quiet: cli.quiet, yes: cli.yes, pager: !cli.no_pager };
    output::configure(cli.json, cli.quiet, cli.ascii, cli.color);
    // Piped into `head`, stop quietly rather than fail writing (the player and the sidecar
    // handle their own output).
    if !cli.command.as_ref().is_none_or(Command::owns_output) {
        pager::default_sigpipe();
    }
    locallibrary::op::allow_archive_writes(cli.overwrite_archive);
    // A test switch that hides every drive and disk: never let it pass unnoticed.
    if let Some(dirs) = locallibrary::device::volume_folders() {
        let dirs: Vec<String> = dirs.iter().map(|d| d.display().to_string()).collect();
        output::diag(format!(
            "LOCALLIBRARY_VOLUMES is set (a test switch): drives are looked for only in {}",
            if dirs.is_empty() { "no folder".into() } else { dirs.join(", ") }
        ));
    }
    let code = run(cli, out);
    pager::finish();
    match code {
        Ok(code) => code,
        Err(e) => {
            let kind = error_kind(&e);
            let library = e.chain().find_map(|c| c.downcast_ref::<locallibrary::Error>());
            let message = describe(&e);
            if out.json {
                print_error(kind, library.map(locallibrary::Error::detail).unwrap_or_default(), &message);
            } else {
                eprintln!("{} {message}", console::style("error:").red().bold());
                for hint in library.map(hints::hints).unwrap_or_default() {
                    eprintln!("  {} {hint}", console::style("hint:").dim());
                }
            }
            match kind {
                "cancelled" => ExitCode::from(130),
                "usage" => ExitCode::from(2),
                _ => ExitCode::FAILURE,
            }
        }
    }
}

/// An error under `--json`: `kind` as the sidecar's `error.data.kind` (`code`, its old name, is
/// the same), with what the error is about and what would resolve it, as the sidecar's
/// `error.data`.
fn print_error(kind: &str, detail: locallibrary::error::Detail, message: &str) {
    let error = serde_json::json!({
        "kind": kind,
        "code": kind,
        "reason": detail.reason,
        "fields": detail.fields,
        "actions": detail.actions,
        "message": message,
    });
    let outcome = if kind == "cancelled" { "cancelled" } else { "error" };
    output::print_json(serde_json::json!({ "error": error, "outcome": outcome }));
}

/// A command line that can't be read: clap's message and exit status 2, or under `--json` the
/// error as every other one prints (kind `usage`). Help and `--version` go out as they are.
fn usage_error(e: clap::Error) -> ExitCode {
    if !e.use_stderr() || !std::env::args().skip(1).take_while(|a| a != "--").any(|a| a == "--json") {
        e.exit();
    }
    print_error("usage", Default::default(), e.render().to_string().trim().trim_start_matches("error: "));
    ExitCode::from(2)
}

/// What kind of error, as the sidecar names them (`not_found`, `conflict`, `stale_plan`...): a
/// library error's own kind (`usage` when it's which drive or library is meant), `io` for a file
/// this command read or wrote, else `invalid` (the command line asked for something that can't
/// be done).
fn error_kind(e: &anyhow::Error) -> &'static str {
    if jobs_cmd::is_usage(e) {
        return "usage";
    }
    if let Some(e) = e.chain().find_map(|c| c.downcast_ref::<locallibrary::Error>()) {
        // Which drive or library was meant is for the command line to say.
        let choose = matches!(
            e.reason(),
            Some(locallibrary::error::Reason::ChooseDrive | locallibrary::error::Reason::ChooseLibrary)
        );
        return if choose { "usage" } else { e.code() };
    }
    if e.chain().any(|c| c.is::<std::io::Error>()) { "io" } else { "invalid" }
}

/// The error and its causes, skipping causes whose text is already included.
fn describe(e: &anyhow::Error) -> String {
    let mut out = String::new();
    for cause in e.chain() {
        let s = cause.to_string();
        if out.contains(&s) {
            continue;
        }
        if !out.is_empty() {
            out.push_str(": ");
        }
        out.push_str(&s);
    }
    out
}

fn open_store(cli_store: &Option<PathBuf>) -> Result<Store> {
    let root = cli_store.clone().unwrap_or_else(Store::default_root);
    Store::open_or_init(&root).with_context(|| format!("opening store at {}", root.display()))
}

fn run(cli: Cli, out: Output) -> Result<ExitCode> {
    let cancel = CancelToken::new();
    // The sidecar handles signals itself, for every request it runs.
    if !matches!(cli.command, Some(Command::Rpc { .. })) {
        let c = cancel.clone();
        // The first Ctrl-C (or SIGTERM) stops at the next safe point; a second exits once no
        // database swap is under way. The player quits on its own, putting the terminal back.
        let mut pressed = false;
        let _ = ctrlc::set_handler(move || {
            #[cfg(feature = "tui")]
            if tui::signal() {
                return;
            }
            if pressed {
                let _swaps = locallibrary::progress::wait_for_critical();
                std::process::exit(130);
            }
            pressed = true;
            eprintln!("\nstopping safely... (Ctrl-C again to force)");
            c.cancel();
        });
    }
    api::set_store_root(cli.store.clone().unwrap_or_else(Store::default_root));
    let progress = out.progress();
    let ctx = Ctx::new(&progress, cancel);
    let store = || open_store(&cli.store);

    let Some(command) = cli.command else {
        // Bare `locallibrary`: the player in a terminal, the usage everywhere else.
        #[cfg(feature = "tui")]
        if !out.json && std::io::stdout().is_terminal() && std::io::stdin().is_terminal() {
            tui::run(cli.store.clone().unwrap_or_else(Store::default_root), None)?;
            return Ok(ExitCode::SUCCESS);
        }
        let _ = help::command().write_help(&mut std::io::stderr());
        return Ok(ExitCode::from(2));
    };
    if !(matches!(command, Command::Setup(_)) || command.owns_output()) {
        jobs_cmd::suggest_setup(&cli.store, out);
    }
    match command {
        Command::Status { all, retired } => {
            let store = store()?;
            let st = api::fleet_status(&store, &api::FleetRequest { all, retired }, &ctx)?;
            out.emit(&st, drive_cmd::fleet_status);
        }
        Command::Tracks(a) => {
            // --search is free text in the filter.
            let filter: Vec<String> = a.search.as_deref().map(search_terms).into_iter().chain(a.filter).collect();
            let filter = (!filter.is_empty()).then(|| filter.join(" "));
            let reads = each_drive::read(a.drive, |drive| {
                let (filter, playlist, sort) = (filter.clone(), a.playlist.clone(), a.sort.clone());
                api::tracks_query(&api::TracksQuery { drive, filter, playlist, sort, offset: 0, limit: a.limit })
            })?;
            out.page();
            // Several drives' TSV or CSV is one listing, for the program reading it.
            if let (each_drive::Reads::Each(each), render::TableFormat::Tsv | render::TableFormat::Csv, false) =
                (&reads, a.format, out.json)
            {
                render::drive_tracks(each, &a.columns, a.format);
                return Ok(reads.outcome().exit());
            }
            return Ok(reads.emit_headed(out, |p| render::tracks(p, &a.columns, a.format)).exit());
        }
        Command::Playlist(a) => return edit_cmd::playlist(a, out, &ctx),
        Command::Playlists { drive } => return edit_cmd::tree(drive, out),
        Command::Track(a) => return edit_cmd::track(a, out, &ctx),
        Command::Smart(a) => return edit_cmd::smart(a, &store()?, out, &ctx),
        Command::Edits(a) => return edit_cmd::edits(a, out, &ctx),
        Command::Pack { source, target, playlists, history, keep_extra, dry_run } => {
            let options = locallibrary::pack::PackOptions { playlists, history, keep_extra, overwrite_archive: false };
            let store = open_store(&cli.store).ok();
            return edit_cmd::pack(api::PackRequest { source, target, options }, dry_run, store.as_ref(), out, &ctx);
        }
        Command::Compare(args) => return diff_cmd::compare_alias(args, &cli.store, out, &ctx),
        Command::Sync { drives, from: Some(source), library: _, opts, allow_guess: _ } => {
            let [target] = <[String; 1]>::try_from(drives).map_err(|d| {
                jobs_cmd::usage(format!("--from copies onto one drive: give the drive to copy onto (got {})", d.len()))
            })?;
            return mirror(&cli.store, source, target, &opts, out, &ctx);
        }
        Command::Sync { mut drives, from: None, library, opts, allow_guess } => {
            if drives.is_empty() {
                return drive_cmd::sync_set(&cli.store, library, &opts, allow_guess, out, &ctx);
            }
            if drives.len() == 1 {
                return Err(jobs_cmd::usage(format!(
                    "give two or more drives, or none to sync the mounted drive group (`sync --from {} DRIVE` copies it onto another)",
                    drives[0]
                )));
            }
            if drives.len() == 2 {
                let (right, left) = (drives.pop().unwrap(), drives.pop().unwrap());
                let req = sync_request(left, right, Direction::Auto, &opts, allow_guess);
                return run_sync(&cli.store, req, opts.dry_run, opts.eject, out, &ctx);
            }
            return run_sync_many(&cli.store, drives, &opts, allow_guess, out, &ctx);
        }
        Command::Mirror { source, target, opts } => return mirror(&cli.store, source, target, &opts, out, &ctx),
        Command::Merge { source, target, playlists, folder, only, match_path_only, history, dry_run } => {
            let options = MergeOptions {
                playlists: match playlists {
                    PlaylistArg::Folder => PlaylistMode::Folder,
                    PlaylistArg::ByName => PlaylistMode::ByName,
                    PlaylistArg::None => PlaylistMode::None,
                },
                folder_name: folder,
                only_playlists: only,
                match_mode: if match_path_only { MatchMode::Path } else { MatchMode::Metadata },
                history,
            };
            let req = api::MergeRequest { source, target, options };
            let plan = api::merge_plan(&req, &ctx)?;
            if plan.noop {
                return Ok(out.nothing(&plan, "nothing to merge: the target already has everything"));
            }
            if !out.json {
                render::merge_plan(&plan);
            }
            if let Some(o) = out.decide(dry_run, "Merge now?") {
                return Ok(out.not_applied(&plan, o));
            }
            let report = plan.apply(None, &ctx)?;
            return Ok(out.applied(&plan, &report, Outcome::Applied, || render::merge_report(&report, &req.target)));
        }
        Command::Convert { drive, to, replace, dry_run } => {
            let to = match to {
                ConvertArg::Onelibrary => locallibrary::convert::Target::OneLibrary,
                ConvertArg::DeviceLibrary => locallibrary::convert::Target::DeviceLibrary,
            };
            let req = api::ConvertRequest { drive, options: locallibrary::convert::ConvertOptions { to, replace } };
            let plan = api::convert_plan(&req, &ctx)?;
            if !out.json {
                render::convert_plan(&plan);
            }
            if let Some(o) = out.decide(dry_run, "Convert now?") {
                return Ok(out.not_applied(&plan, o));
            }
            let report = plan.apply(None, &ctx)?;
            return Ok(out.applied(&plan, &report, Outcome::Applied, || render::convert_report(&report, &req.drive)));
        }
        Command::Clean { drive, dry_run } => {
            let req = api::DriveRef { drive };
            let plan = api::clean_plan(&req, &ctx)?;
            if plan.noop {
                return Ok(out.nothing(&plan, "nothing to clean"));
            }
            if !out.json {
                render::clean_plan(&plan);
            }
            if let Some(o) = out.decide(dry_run, "Remove them?") {
                return Ok(out.not_applied(&plan, o));
            }
            let report = plan.apply(None, &ctx)?;
            return Ok(out.applied(&plan, &report, Outcome::Applied, || render::clean_report(&report, &req.drive)));
        }
        Command::Tags(args) => return tags_cmd::run(args, out, &ctx),
        Command::Pull { drive: None, library, new_library, message, label, force } => {
            if new_library || label.is_some() || force {
                return Err(jobs_cmd::usage("--new-library, --label and --force go with one drive: name it"));
            }
            return jobs_cmd::pull_group(&store()?, library, message, out, &ctx);
        }
        Command::Pull { drive: Some(drive), library, new_library, message, label, force } => {
            let req = api::PullRequest { drive, library, new_library, message, label, force };
            return store_cmd::pull(&store()?, req, out, &ctx);
        }
        Command::Library(a) => return library_cmd::run(a, &cli.store, out, &ctx),
        Command::Push(a) => return library_cmd::push(a, &cli.store, out, &ctx),
        Command::Log(a) => return library_cmd::log(a, &store()?, out),
        Command::Store { command } => {
            let store = store()?;
            match command.unwrap_or(StoreCommand::Info) {
                StoreCommand::Info => out.emit(&api::store_info(&store)?, render::store_info),
                StoreCommand::Gc { dry_run } => {
                    let plan = api::store_gc_plan(&store)?;
                    if plan.noop {
                        return Ok(out.nothing(&plan, "nothing to collect"));
                    }
                    if !out.json {
                        output::summary(&plan.summary, &plan.warnings);
                    }
                    if let Some(o) = out.decide(dry_run, "Delete them?") {
                        return Ok(out.not_applied(&plan, o));
                    }
                    let r = plan.apply(Some(&store), &ctx)?;
                    return Ok(out.applied(&plan, &r, Outcome::Applied, || {
                        output::ok(format!(
                            "freed {} ({} objects, {} snapshots)",
                            output::bytes(r.freed_bytes),
                            r.removed_objects,
                            r.removed_snapshots
                        ))
                    }));
                }
                StoreCommand::Label { snapshot, label } => {
                    let req = api::LabelRequest { label, snapshot: Some(snapshot), remove: false };
                    return store_cmd::label(&store, req, out);
                }
                StoreCommand::Unlabel { label } => {
                    return store_cmd::label(&store, api::LabelRequest { label, snapshot: None, remove: true }, out);
                }
                StoreCommand::Prune(a) => return store_cmd::prune(&store, a.options(), out, &ctx),
                StoreCommand::Copy { dir, mirror, verify, force, dry_run } => {
                    let options = api::CopyOptions { mirror, verify, force, dry_run };
                    return store_cmd::copy(&store, api::StoreCopyRequest { path: dir, options }, out, &ctx);
                }
                StoreCommand::Copies => return store_cmd::copies(&store, out),
                StoreCommand::Verify => {
                    let r = store.verify(&ctx)?;
                    let bad = !r.corrupt.is_empty() || !r.missing.is_empty();
                    let outcome = if bad { Outcome::Problems } else { Outcome::Applied };
                    return Ok(out.report(&r, outcome, |r| {
                        if r.corrupt.is_empty() && r.missing.is_empty() {
                            output::ok(format!("{} objects verified", r.checked));
                        } else {
                            for c in &r.corrupt {
                                output::warn(format!("corrupt: {c}"));
                            }
                            for m in &r.missing {
                                output::warn(format!("missing: {m}"));
                            }
                        }
                    }));
                }
            }
        }
        Command::Settings(a) => return settings(a, &cli.store, out, &ctx),
        Command::Art(a) => return art(a, &cli.store, out, &ctx),
        Command::Cues(args) => return cues_cmd::run(args, out, &ctx),
        Command::Undo { drive, list: true, .. } => {
            let store = diff_cmd::existing_store(&cli.store);
            let list = api::undo_list(store.as_ref(), &api::UndoListRequest { drive })?;
            out.emit(&list, restore_cmd::undo_list);
        }
        Command::Undo { drive, list: false, force, dry_run } => {
            return restore_cmd::undo(api::UndoRequest { drive, backup: None, force }, dry_run, out, &ctx);
        }
        Command::Xml { drive, output: path, location_root, no_analysis, only, replace } => {
            let drive = jobs_cmd::drive_or_default(drive, &cli.store, "export")?;
            let req = api::ExportXml {
                drive,
                output: path.clone(),
                location_root,
                skip_analysis: no_analysis,
                only,
                replace,
            };
            let r = api::export_xml(&req, &ctx)?;
            out.done(&r, |r| {
                output::ok(format!(
                    "wrote {} ({} tracks, {} playlists, {} beat grids, {} cues)",
                    path.display(),
                    r.tracks,
                    r.playlists,
                    r.tracks_with_beatgrid,
                    r.cues
                ));
                crate::output::hint("Import in rekordbox: Preferences > Advanced > rekordbox xml > Imported Library");
            });
        }
        Command::Doctor(args) => return doctor_cmd::run(args, &cli.store, out, &ctx),
        Command::Plays(a) => return plays_cmd::run(a, &cli.store, out, &ctx),
        Command::Sets(a) => return sets_cmd::run(a, &cli.store, out, &ctx),
        Command::History(args) => return plays_cmd::history(args, &cli.store, out, &ctx),
        Command::Stats(args) => return plays_cmd::stats(args, &cli.store, out),
        #[cfg(feature = "account")]
        Command::Account(args) => return account_cmd::run(args, &cli.store, out, &ctx),
        Command::Profile(args) => return profile_cmd::run(args, &cli.store, out, &ctx),
        Command::Export(args) => return export_cmd::run(args, &cli.store, out, &ctx),
        Command::Diff(args) => return diff_cmd::run(args, &cli.store, out, &ctx),
        Command::Collection(args) => return collection_cmd::run(args, out),
        Command::Check(args) => return check_cmd::run(args, &cli.store, out, &ctx),
        Command::Players(args) => return players_cmd::run(args, &cli.store, out, &ctx),
        Command::Mix(args) => return mix_cmd::run(args, &cli.store, out, &ctx),
        #[cfg(feature = "tui")]
        Command::Tui { drive } => {
            if !std::io::stdout().is_terminal() {
                bail!("the player needs a terminal");
            }
            tui::run(cli.store.clone().unwrap_or_else(Store::default_root), drive)?;
        }
        Command::Completions(a) => return completion::completions(a),
        Command::Man(a) => return completion::man(a),
        Command::Rpc { emit_schema: Some(dir), .. } => {
            for path in rpc::emit_schema(&dir).with_context(|| format!("writing to {}", dir.display()))? {
                output::ok(format!("wrote {}", path.display()));
            }
        }
        Command::Rpc { workers, emit_schema: None } => {
            rpc::serve(cli.store.clone().unwrap_or_else(Store::default_root), workers)?;
        }
        Command::Watch(args) => return watch_cmd::run(args, &cli.store, out, &ctx),
        Command::Refresh(args) => return jobs_cmd::refresh(args, &cli.store, out, &ctx),
        Command::Prep(args) => return jobs_cmd::prep(args, &cli.store, out, &ctx),
        Command::Setup(args) => return jobs_cmd::setup(args, &cli.store, out, &ctx),
        Command::DriveManagement(command) => return drive_cmd::run(command, &cli.store, out, &ctx),
    }
    Ok(ExitCode::SUCCESS)
}

/// `tracks --search`: its words as free text for the filter (each must match the track's
/// artist, title, album, genre, label, key, comment, remixer, mix name or path).
fn search_terms(search: &str) -> String {
    search
        .split_whitespace()
        .map(|w| w.trim_start_matches('-').replace('"', ""))
        .filter(|w| w.chars().any(char::is_alphanumeric))
        .map(|w| format!("\"{w}\""))
        .collect::<Vec<_>>()
        .join(" ")
}

/// `sync --from SOURCE TARGET` (and `mirror`): one drive onto another, whichever is newer.
fn mirror(
    store: &Option<PathBuf>,
    source: String,
    target: String,
    opts: &SyncFlags,
    out: Output,
    ctx: &Ctx<'_>,
) -> Result<ExitCode> {
    let req = sync_request(source, target, Direction::LeftToRight, opts, false);
    run_sync(store, req, opts.dry_run, opts.eject, out, ctx)
}

fn sync_request(
    left: String,
    right: String,
    direction: Direction,
    f: &SyncFlags,
    allow_low_confidence: bool,
) -> api::SyncRequest {
    api::SyncRequest {
        left,
        right,
        direction,
        allow_low_confidence,
        verify: f.verify,
        keep_extra: f.keep_extra,
        delete_first: f.delete_first,
        overwrite_archive: false,
        keep_player_changes: false,
    }
}

fn run_sync(
    store: &Option<PathBuf>,
    req: api::SyncRequest,
    dry_run: bool,
    eject: bool,
    out: Output,
    ctx: &Ctx<'_>,
) -> Result<ExitCode> {
    let preview = api::sync_plan(&req, ctx)?;
    let Some(plan) = &preview.plan.plan else {
        return Ok(out.nothing(&preview, "already in sync"));
    };
    if !out.json {
        render::comparison_short(&preview.comparison);
        if let Some(g) = &preview.after_set {
            output::heading(&format!("{} came back from a set", g.back_from_set.drive));
            say!("  players added {}", g.back_from_set.describe());
            say!(
                "  {} has newer changes since snapshot {}; it will be copied over {}",
                g.newer.drive,
                g.common.short_id,
                g.back_from_set.drive
            );
            say!(
                "  {}",
                output::dim(format!(
                    "first, {}'s state is saved in the store (its history lists and cues stay there), then {} is saved as the newest snapshot",
                    g.back_from_set.drive, g.newer.drive
                ))
            );
        }
        render::sync_plan(plan);
    }
    if plan.is_noop() {
        return Ok(out.nothing(&preview, "nothing to copy"));
    }
    let target = preview.target.as_ref().map(|p| p.display().to_string()).unwrap_or_default();
    if let Some(o) = out.decide(dry_run, &format!("Update {target}?")) {
        return Ok(out.not_applied(&preview, o));
    }
    // The confirmed plan: the same direction and files, or nothing.
    let source = preview.source.as_ref().unwrap().display().to_string();
    let store_handle = open_store(store).ok();
    let result = preview.apply(store_handle.as_ref(), ctx)?;
    let code = out.applied(&preview, &result, Outcome::Applied, || {
        for s in &result.saved {
            output::ok(format!("saved snapshot {} ({})", s.short_id, s.message.as_deref().unwrap_or("")));
        }
        if let Some(r) = &result.report {
            render::sync_report(r);
        }
        if let Some(g) = preview.after_set.as_ref().filter(|_| !result.saved.is_empty()) {
            hints::new_sessions(g.back_from_set.history_lists.len());
        }
    });
    if eject {
        drive_cmd::eject_after(store, vec![source, target], out);
    }
    Ok(code)
}

fn run_sync_many(
    store_path: &Option<PathBuf>,
    drives: Vec<String>,
    f: &SyncFlags,
    allow_low_confidence: bool,
    out: Output,
    ctx: &Ctx<'_>,
) -> Result<ExitCode> {
    let req = api::SyncManyRequest {
        drives,
        allow_low_confidence,
        verify: f.verify,
        keep_extra: f.keep_extra,
        delete_first: f.delete_first,
        overwrite_archive: false,
    };
    let preview = api::sync_many_plan(&req, ctx)?;
    many_targets(preview, store_path, f, out, ctx)
}

/// Show, confirm and apply a plan that copies one drive onto several.
pub(crate) fn many_targets(
    preview: locallibrary::op::Planned<api::SyncManyPreview, api::SyncManyRequest>,
    store_path: &Option<PathBuf>,
    f: &SyncFlags,
    out: Output,
    ctx: &Ctx<'_>,
) -> Result<ExitCode> {
    let pending = preview.targets.iter().filter(|t| t.plan.as_ref().is_some_and(|p| !p.is_noop())).count();
    if !out.json {
        output::heading(&format!("{} is the newest", preview.source_name));
        for t in &preview.targets {
            match &t.plan {
                Some(p) if !p.is_noop() => render::sync_plan(p),
                _ => output::ok(format!("{} already matches", t.comparison.right.label)),
            }
        }
    }
    if pending == 0 {
        return Ok(out.not_applied(&preview, Outcome::NothingToDo));
    }
    if let Some(o) = out.decide(f.dry_run, &format!("Update {pending} drive(s)?")) {
        return Ok(out.not_applied(&preview, o));
    }
    let store = open_store(store_path).ok();
    let results = preview.apply(store.as_ref(), ctx)?;
    let failed = results.iter().any(|r| r.error.is_some());
    let outcome = if failed { Outcome::Partial } else { Outcome::Applied };
    let code = out.applied(&preview, &results, outcome, || {
        drive_cmd::target_results(&results);
        // Drives back from a set were saved before they were overwritten.
        let saved = preview
            .targets
            .iter()
            .filter(|t| results.iter().any(|r| r.report.is_some() && Some(&r.target) == t.target.as_ref()));
        hints::new_sessions(
            saved.filter_map(|t| t.after_set.as_ref()).map(|g| g.back_from_set.history_lists.len()).sum(),
        );
    });
    if f.eject {
        let mut all = vec![preview.source.display().to_string()];
        all.extend(results.iter().filter(|r| r.error.is_none()).map(|r| r.target.display().to_string()));
        drive_cmd::eject_after(store_path, all, out);
    }
    Ok(code)
}

/// Which drives a change to the store's settings profile is for, as its plan and result say.
fn settings_scope(drive: Option<&str>) -> String {
    drive.map_or_else(|| "for all drives".into(), |d| format!("for {d}"))
}

fn settings(args: SettingsArgs, store_path: &Option<PathBuf>, out: Output, ctx: &Ctx<'_>) -> Result<ExitCode> {
    let command = match (args.command, args.drive) {
        (Some(c), _) => c,
        (None, Some(drive)) => {
            help::check_drive_word("settings", &drive)?;
            SettingsCommand::Show { drive: Some(drive) }
        }
        (None, None) => SettingsCommand::Base,
    };
    match command {
        SettingsCommand::Schema => out.emit_paged(&locallibrary::settings::SCHEMA, |_| render::settings_schema()),
        SettingsCommand::Base => {
            let store = open_store(store_path)?;
            let profile = store.settings_profile()?;
            out.emit(&profile, render::profile);
        }
        SettingsCommand::Set { values, drive } => {
            let store = open_store(store_path)?;
            let mut map = BTreeMap::new();
            let mut set = Vec::new();
            for kv in values {
                let Some((k, v)) = kv.split_once('=') else { bail!("expected KEY=VALUE, got {kv:?}") };
                let (def, choice) = locallibrary::settings::validate(k.trim(), v.trim())?;
                set.push(format!("{} = {}", def.key, choice.id));
                map.insert(k.trim().to_string(), v.trim().to_string());
            }
            let scope = settings_scope(drive.as_deref());
            let change = format!("set {} {scope} in the store's settings profile", set.join(", "));
            let req = api::SetSettings { values: map, drive };
            return out.one_step(
                &change,
                "Set them?",
                || Ok(api::settings_set(&store, &req)?),
                |_| {
                    for s in &set {
                        output::ok(format!("{s} {}", output::dim(&scope)));
                    }
                    crate::output::hint("Run `locallibrary settings apply` to write the profile to your drives.");
                },
            );
        }
        SettingsCommand::Unset { keys, drive } => {
            let store = open_store(store_path)?;
            let scope = settings_scope(drive.as_deref());
            let change = format!("unset {} {scope} in the store's settings profile", keys.join(", "));
            let req = api::UnsetSettings { keys, drive };
            return out.one_step(&change, "Unset them?", || Ok(api::settings_unset(&store, &req)?), render::profile);
        }
        SettingsCommand::Show { drive } => {
            let store = open_store(store_path)?;
            let reads = each_drive::read(drive, |drive| api::settings_show(&store, &api::DriveRef { drive }))?;
            return Ok(reads.emit(out, render::drive_settings).exit());
        }
        SettingsCommand::Apply { drives, create_missing, dry_run } => {
            let store = open_store(store_path)?;
            let plan = api::settings_apply_plan(&store, &api::SettingsApplyRequest { drives, create_missing })?;
            let planned: BTreeMap<String, locallibrary::settings::ApplyReport> = plan
                .targets
                .iter()
                .map(|d| (d.drive.clone(), locallibrary::settings::ApplyReport { dry_run: true, ..d.changes.clone() }))
                .collect();
            if plan.noop {
                if !out.json {
                    render::settings_apply(&planned);
                }
                return Ok(out.not_applied(&plan, Outcome::NothingToDo));
            }
            if !out.json {
                render::settings_apply(&planned);
            }
            if let Some(o) = out.decide(dry_run, "Write them?") {
                return Ok(out.not_applied(&plan, o));
            }
            let results = plan.apply(Some(&store), ctx)?;
            let failed = results.iter().any(|r| r.error.is_some());
            let reports: BTreeMap<String, locallibrary::settings::ApplyReport> =
                results.iter().filter_map(|r| Some((r.drive.clone(), r.report.clone()?))).collect();
            return Ok(out.applied(&plan, &results, if failed { Outcome::Partial } else { Outcome::Applied }, || {
                render::settings_apply(&reports);
                for r in &results {
                    if let Some(e) = &r.error {
                        output::warn(format!("{}: {}", r.drive, e.message));
                    }
                }
            }));
        }
        #[cfg(feature = "account")]
        SettingsCommand::Publish { dry_run, browser } => {
            return publish_cmd::settings_publish(store_path, dry_run, browser, out, ctx);
        }
        #[cfg(feature = "account")]
        SettingsCommand::Import { from_handle, from_account, pds, replace, dry_run, .. }
            if from_account || from_handle.is_some() =>
        {
            let req = api::ImportPublishedSettings { from: from_handle, pds, replace };
            return publish_cmd::settings_import(store_path, &req, dry_run, out, ctx);
        }
        #[cfg(feature = "account")]
        SettingsCommand::Import { pds: Some(_), .. } => bail!("--pds goes with --from-handle or --from-account"),
        #[cfg(feature = "account")]
        SettingsCommand::Import { replace: true, .. } => bail!("--replace goes with --from-handle or --from-account"),
        SettingsCommand::Import { drive, dry_run, .. } => {
            let store = open_store(store_path)?;
            let plan = api::settings_import_plan(&store, &api::ImportSettings { drive })?;
            if plan.noop {
                return Ok(out.nothing(&plan, "the profile has those values already"));
            }
            if !out.json {
                output::summary(&plan.summary, &plan.warnings);
                for c in &plan.changes {
                    let shown = |v: &Option<String>| v.clone().unwrap_or_else(|| "(not set)".into());
                    say!("    {:<28} {} → {}", c.key, shown(&c.from), shown(&c.to));
                }
            }
            if let Some(o) = out.decide(dry_run, "Import them?") {
                return Ok(out.not_applied(&plan, o));
            }
            let profile = plan.apply(Some(&store), ctx)?;
            let from = plan.from.clone();
            return Ok(out.applied(&plan, &profile, Outcome::Applied, || {
                output::ok(format!("imported {} settings from {from}", profile.base.len()));
            }));
        }
    }
    Ok(ExitCode::SUCCESS)
}

fn art(args: ArtArgs, store_path: &Option<PathBuf>, out: Output, ctx: &Ctx<'_>) -> Result<ExitCode> {
    let command = match (args.command, args.drive) {
        (Some(c), _) => c,
        (None, Some(drive)) => {
            help::check_drive_word("art", &drive)?;
            ArtCommand::Missing { drive: Some(drive) }
        }
        (None, None) => ArtCommand::Missing { drive: None },
    };
    match command {
        ArtCommand::Missing { drive } => {
            let reads = each_drive::read(drive, |drive| api::artwork_missing(&api::DriveRef { drive }))?;
            out.page();
            let outcome = reads.emit_headed(out, |l| {
                for t in l {
                    say!("{:>6}  {}", format!("#{}", t.id), t.name);
                }
                say!("{}", output::dim(format!("{} track(s) without artwork", l.len())));
            });
            return Ok(outcome.exit());
        }
        ArtCommand::Set { drive, image, select, dry_run } => {
            let req = api::SetArtwork { drive, image, tracks: select.selector() };
            let plan = api::artwork_set_plan(&req)?;
            if !out.json {
                output::summary(&plan.summary, &plan.warnings);
                for t in plan.tracks.iter().take(10) {
                    say!("    {}", output::dim(&t.name));
                }
                if plan.tracks.len() > 10 {
                    say!("    {}", output::dim(format!("and {} more", plan.tracks.len() - 10)));
                }
            }
            if let Some(o) = out.decide(dry_run, "Set it?") {
                return Ok(out.not_applied(&plan, o));
            }
            let r = plan.apply(None, ctx)?;
            return Ok(out.applied(&plan, &r, Outcome::Applied, || {
                output::ok(format!("artwork set on {} track(s)", r.tracks_updated))
            }));
        }
        ArtCommand::Auto { drive, dry_run } => {
            let drives = match drive {
                Some(drive) => vec![drive],
                None => each_drive::targets(diff_cmd::existing_store(store_path).as_ref())?,
            };
            let mut plans = Vec::new();
            for drive in drives {
                plans.push((drive.clone(), api::artwork_auto_plan(&api::DriveRef { drive })?));
            }
            let show = |p: &locallibrary::artwork::AutoArtworkPlan| {
                for f in &p.found {
                    say!(
                        "{:>6}  {}  {}",
                        format!("#{}", f.track.id),
                        f.track.name,
                        output::dim(format!("<- {}", f.source))
                    );
                }
                say!("{} found, {} still missing", p.found.len(), p.not_found.len());
            };
            let done = |r: &locallibrary::artwork::ArtworkReport| {
                output::ok(format!("artwork set on {} track(s)", r.tracks_updated))
            };
            if plans.len() > 1 {
                let show = |drive: &str, p: &locallibrary::op::Planned<_>| {
                    output::heading(drive);
                    show(p);
                };
                return each_drive::apply_each(plans, dry_run, "Set the artwork", out, ctx, show, |_, r| done(r));
            }
            let (_, plan) = plans.remove(0);
            if !out.json {
                show(&plan);
            }
            if plan.noop {
                return Ok(out.not_applied(&plan, Outcome::NothingToDo));
            }
            if let Some(o) = out.decide(dry_run, "Apply?") {
                return Ok(out.not_applied(&plan, o));
            }
            let r = plan.apply(None, ctx)?;
            return Ok(out.applied(&plan, &r, Outcome::Applied, || done(&r)));
        }
        ArtCommand::Embed { image, files, dry_run } => {
            let store = open_store(store_path)?;
            let plan = api::artwork_embed_plan(&api::EmbedArtwork { files, image })?;
            if !out.json {
                output::summary(&plan.summary, &plan.warnings);
                for f in &plan.files {
                    let replaces = if f.replaces_art { "replacing its art" } else { "" };
                    say!("    {} {}", f.path.display(), output::dim(replaces));
                }
            }
            if plan.noop {
                return Ok(out.not_applied(&plan, Outcome::NothingToDo));
            }
            if let Some(o) = out.decide(dry_run, "Rewrite these files?") {
                return Ok(out.not_applied(&plan, o));
            }
            let r = plan.apply(Some(&store), ctx)?;
            let outcome = if r.failed.is_empty() { Outcome::Applied } else { Outcome::Partial };
            return Ok(out.applied(&plan, &r, outcome, || {
                for f in &r.failed {
                    output::warn(format!("{}: {}", f.path, f.error));
                }
                output::ok(format!("embedded into {} file(s)", r.embedded.len()));
                if let Some(b) = &r.backup {
                    crate::output::hint(format!("`locallibrary art unembed --backup {b}` puts them back"));
                }
            }));
        }
        ArtCommand::Unembed { backup, list: true, .. } => {
            let _ = backup;
            let store = open_store(store_path)?;
            let list = locallibrary::artwork::embed_backups(&store)?;
            out.emit(&list, |l| {
                if l.is_empty() {
                    say!("nothing to take back");
                }
                for b in l {
                    say!("{}  {} file(s)  {}", b.id, b.files.len(), output::dim(b.image.display()));
                }
            });
        }
        ArtCommand::Unembed { backup, list: false, dry_run } => {
            let store = open_store(store_path)?;
            let plan = api::artwork_unembed_plan(&store, &api::UnembedRequest { backup })?;
            if !out.json {
                output::summary(&plan.summary, &plan.warnings);
            }
            if plan.noop {
                return Ok(out.not_applied(&plan, Outcome::NothingToDo));
            }
            if let Some(o) = out.decide(dry_run, "Put them back?") {
                return Ok(out.not_applied(&plan, o));
            }
            let r = plan.apply(Some(&store), ctx)?;
            let outcome = if r.skipped.is_empty() { Outcome::Applied } else { Outcome::Partial };
            return Ok(out.applied(&plan, &r, outcome, || {
                for s in &r.skipped {
                    output::warn(format!("{}: {}", s.path, s.reason));
                }
                output::ok(format!("put back {} file(s)", r.restored.len()));
            }));
        }
        ArtCommand::Get { drive, track, output: path, replace } => {
            let d = api::resolve_drive(&drive)?;
            let lib = d.library()?;
            let bytes =
                locallibrary::artwork::read_from_drive(&d, &lib, track)?.context("that track has no artwork")?;
            locallibrary::fsutil::write_output(&path, &bytes, replace)?;
            out.done(&serde_json::json!({ "written": path }), |_| output::ok(format!("wrote {}", path.display())));
        }
    }
    Ok(ExitCode::SUCCESS)
}

#[cfg(test)]
mod tests {
    #[test]
    fn command_line_is_well_formed() {
        use clap::CommandFactory;
        super::Cli::command().debug_assert();
        super::help::command().debug_assert();
    }

    /// `--cluster-size` takes sizes as people write them.
    #[test]
    fn sizes_read_as_written() {
        for (written, bytes) in [("32768", 32768), ("32K", 32768), ("32kb", 32768), ("64 KiB", 65536), ("1M", 1 << 20)]
        {
            assert_eq!(super::parse_size(written), Ok(bytes), "{written}");
        }
        for wrong in ["", "K", "big", "-1K", "4096M"] {
            assert!(super::parse_size(wrong).is_err(), "{wrong}");
        }
    }

    /// The filter syntax follows the help of every command that takes `--filter`, or its group's.
    #[test]
    fn every_filter_flag_has_the_syntax_after_it() {
        fn walk(cmd: &clap::Command, path: &str, missing: &mut Vec<String>) {
            let shows = |c: &clap::Command| c.get_after_help().is_some_and(|h| h.to_string() == super::FILTER_HELP);
            for sc in cmd.get_subcommands() {
                let path = format!("{path} {}", sc.get_name());
                let filters = sc.get_arguments().any(|a| a.get_long() == Some("filter"));
                if filters && !shows(sc) && !shows(cmd) {
                    missing.push(path.clone());
                }
                walk(sc, &path, missing);
            }
        }
        let mut missing = Vec::new();
        walk(&super::help::command(), "locallibrary", &mut missing);
        assert!(missing.is_empty(), "no filter syntax after {missing:?}");
    }
}
