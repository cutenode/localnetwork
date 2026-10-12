//! Drive management: the drives you own, their health and space, what players show, firmware,
//! formatting, backups, and setting drives up and ejecting them.

use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{Context, Result, bail};
use clap::{Args, Subcommand};
use console::style;
use locallibrary::Store;
use locallibrary::api::{self, DriveRef};
use locallibrary::compare::Relation;
use locallibrary::fleet::{DriveLog, KnownDrive, Role, Standing};
use locallibrary::format::{FormatOptions, FormatPlan};
use locallibrary::local_time;
use locallibrary::progress::Ctx;
use locallibrary::recordings::{CollectOptions, Tracklist};
use locallibrary::scrub::ScrubReport;
use locallibrary::space::{SpaceKind, SpaceReport};
use locallibrary::tidy::TidyOptions;

use crate::completion::DriveOnly;
use crate::output::{self, Outcome, Output, bytes, dim, duration, heading, ok, warn};
use crate::{DRIVE_HELP, FsArg, each_drive, open_store, parse_size, render};

/// `locallibrary drive`: with no subcommand, the mounted drives; with a drive, its details.
#[derive(Args)]
#[command(args_conflicts_with_subcommands = true)]
pub struct DriveArgs {
    #[command(subcommand)]
    command: Option<DriveCommand>,
    /// A drive to show, as `drive info` does [default: the mounted drives, as `drive ls` lists them]
    #[arg(value_name = "DRIVE", add = DriveOnly)]
    drive: Option<String>,
}

/// `locallibrary drive ...`
#[derive(Subcommand)]
pub enum DriveCommand {
    /// Mounted rekordbox drives (--known: every drive locallibrary has seen, mounted or not)
    Ls(LsArgs),
    /// A drive's library, space, settings files, identity and history
    #[command(alias = "show")]
    Info(DriveArg),
    /// Give a drive a stable identity and a name
    #[command(after_long_help = "Examples:\n  locallibrary drive adopt /Volumes/REKORDBOX\n  \
locallibrary drive adopt /Volumes/USB --as BACKUP    # after it was erased elsewhere")]
    Adopt(AdoptArgs),
    /// Rename a drive (with --label, its volume label too)
    Rename {
        #[arg(help = DRIVE_HELP)]
        drive: String,
        /// The new name
        name: String,
        /// Also change the volume label computers show (players show the Device Name instead)
        #[arg(long)]
        label: bool,
        /// Show what would change without changing anything
        #[arg(long)]
        dry_run: bool,
    },
    /// Remove a drive from the store's registry (it's added again the next time it's synced)
    Forget {
        #[arg(help = DRIVE_HELP)]
        drive: String,
    },
    /// Say what a drive is for: primary, backup, set or archive ("none" clears it)
    #[command(after_help = "A library has one primary. Archive drives are never overwritten by sync or push \
unless --overwrite-archive (or --force for push) is given. `gig`, the old name for `set`, works too.")]
    Role {
        #[arg(help = DRIVE_HELP)]
        drive: String,
        /// primary, backup, set, archive or none
        role: String,
    },
    /// Mark a drive as no longer in use, or --undo
    Retire {
        #[arg(help = DRIVE_HELP)]
        drive: String,
        /// Put it back in use
        #[arg(long)]
        undo: bool,
    },
    /// A drive's logbook: data written, ejects, check results, problems
    Log {
        #[arg(help = DRIVE_HELP)]
        drive: String,
    },
    /// Which known drive or stored snapshot a drive matches (for a drive that lost its identity)
    Recognise {
        #[arg(help = DRIVE_HELP)]
        drive: String,
    },
    /// Give a drive copied from another (in Finder or with a cloning tool) an identity of its own
    Reidentify {
        /// The copy's mount path
        path: PathBuf,
        /// A name for the copy [default: its name, with " (copy)" after it]
        #[arg(long)]
        name: Option<String>,
        /// Give it a new identity though nothing shows it's a copy
        #[arg(long)]
        force: bool,
        /// Show what would change without changing anything
        #[arg(long)]
        dry_run: bool,
    },
    /// Show the Device Name and colour players show for a drive, or set them
    #[command(after_help = "Sets them in OneLibrary (exportLibrary.db), which the CDJ-3000X, OPUS-QUAD, \
XDJ-AZ and other newer players read. Sync and push keep each drive's own values. rekordbox's next export \
writes the values rekordbox has for the drive.")]
    Player {
        #[arg(help = DRIVE_HELP)]
        drive: String,
        /// Device Name, up to 32 characters
        #[arg(long)]
        name: Option<String>,
        /// rekordbox's background colour code, 0 (none) to 8
        #[arg(long, alias = "color")]
        colour: Option<i64>,
        /// Show what would change without changing anything
        #[arg(long)]
        dry_run: bool,
    },
    /// Put your contact details on a drive for whoever finds it
    Contact {
        #[arg(help = DRIVE_HELP)]
        drive: String,
        /// Email address, phone number or name
        contact: String,
        /// Also add a top-level playlist "IF FOUND: <contact>", the first thing a player shows
        #[arg(long)]
        playlist: bool,
        /// Show what would change without changing anything
        #[arg(long)]
        dry_run: bool,
    },
    /// Mounted drives grouped by library: the drive groups `locallibrary sync` with no drives syncs
    #[command(alias = "sets")]
    Groups,
    /// External disks: their format, and how players get on with it
    Disks,
    /// Erase a drive and format it for players (FAT32 on MBR unless told otherwise)
    #[command(after_help = FORMAT_HELP, after_long_help = FORMAT_LONG_HELP)]
    Format(FormatArgs),
    /// Read every file on a drive and compare it with a known-good copy
    #[command(after_help = VERIFY_HELP)]
    Verify(VerifyArgs),
    /// Check a drive's file system with the operating system's own tool (with --repair, fix it)
    #[command(after_help = FSCK_HELP)]
    Fsck(FsckArgs),
    /// What is using a drive's space, and when it will be full
    Space(DriveArg),
    /// Remove the files macOS and Windows leave on a drive (._*, .DS_Store, Spotlight's index...)
    Tidy(TidyArgs),
    /// Set recordings made on the player (XDJ-AZ, OPUS-QUAD MASTER REC): list or copy them
    Recordings(RecordingsArgs),
    /// Put a player or mixer firmware update file on a drive, checked against the model's rules
    Firmware(FirmwareArgs),
    /// The backups locallibrary made on a drive before changing it
    Backups(DriveArg),
    /// Put back one of a drive's backups (`undo` puts back the latest), or bring back one playlist
    /// or folder from a snapshot or a backup (--playlist)
    #[command(after_long_help = "Examples:\n  locallibrary drive restore REKORDBOX --backup 20261004T165600.123Z\n  \
locallibrary drive restore Home --playlist \"Sets/2026/Friday\"\n  \
locallibrary drive restore Home --playlist \"Sets/2026\" --snapshot 3f9a2c --dry-run")]
    Restore(RestoreArgs),
    /// Every drive locallibrary has seen, mounted or not (`drive ls --known`)
    #[command(hide = true)]
    List {
        /// Include retired drives
        #[arg(long)]
        retired: bool,
    },
}

/// The drive commands at the top level: the `drive` group, `eject` and `provision`, and the old
/// top-level names of `drive` commands (hidden).
#[derive(Subcommand)]
pub enum TopCommand {
    /// The drives you own: list, info, adopt, rename, roles, logbook, health, space, formatting,
    /// backups
    Drive(DriveArgs),
    /// Unmount drives and power them off so they can be pulled out safely
    #[command(after_long_help = "Examples:\n  locallibrary eject    # the mounted drives of your library\n  \
locallibrary eject Home SPARE\n  \
locallibrary sync --eject    # sync every library's drives, then eject them")]
    Eject(EjectArgs),
    /// Turn a blank drive into a working copy of the library, in one step
    #[command(after_help = PROVISION_HELP, after_long_help = PROVISION_LONG_HELP)]
    Provision(ProvisionArgs),
    /// `drive ls`
    #[command(hide = true)]
    Drives(LsArgs),
    /// `drive info`
    #[command(hide = true)]
    Info(DriveArg),
    /// `drive adopt`
    #[command(hide = true)]
    Adopt(AdoptArgs),
    /// `drive disks`
    #[command(hide = true)]
    Disks,
    /// `drive format`
    #[command(hide = true, after_help = FORMAT_HELP)]
    Format(FormatArgs),
    /// `drive verify`
    #[command(hide = true, after_help = VERIFY_HELP)]
    Verify(VerifyArgs),
    /// `drive fsck`
    #[command(hide = true, after_help = FSCK_HELP)]
    Fsck(FsckArgs),
    /// `drive space`
    #[command(hide = true)]
    Space(DriveArg),
    /// `drive tidy`
    #[command(hide = true)]
    Tidy(TidyArgs),
    /// `drive recordings`
    #[command(hide = true)]
    Recordings(RecordingsArgs),
    /// `drive firmware`
    #[command(hide = true)]
    Firmware(FirmwareArgs),
    /// `drive backups`
    #[command(hide = true)]
    Backups(DriveArg),
    /// `drive restore`
    #[command(hide = true)]
    Restore(RestoreArgs),
}

const FORMAT_HELP: &str = "Everything on the disk is erased, including other partitions. The plan is shown first, \
and you confirm by typing the drive's name (--yes doesn't skip that; --erase NAME gives it, for scripts). To set a \
new drive up in one step (format, identity, library, settings), see `locallibrary provision`.";

const FORMAT_LONG_HELP: &str = "Everything on the disk is erased, including other partitions. The plan is shown \
first, and you confirm by typing the drive's name (--yes doesn't skip that; --erase NAME gives it, for scripts). To \
set a new drive up in one step (format, identity, library, settings), see `locallibrary provision`.

Examples:
  locallibrary drive format BACKUP --dry-run      the plan: what gets erased, what gets created
  locallibrary drive format disk4 --fs exfat      a disk with no mounted volume
  locallibrary drive format disk4 --yes --erase KEYCHAIN    in a script";

const VERIFY_HELP: &str = "Compares with a stored snapshot of the drive's state, the drive's own hash records, or \
an older snapshot holding the same version of a file. With --repair, damaged and missing files are rewritten from \
the store or the --against drives, verified, and read back.";

const FSCK_HELP: &str = "macOS: diskutil verifyVolume/repairVolume (the drive is unmounted for the duration). \
Linux: fsck.vfat/fsck.exfat, as root. Windows: chkdsk, from an elevated prompt.";

const PROVISION_HELP: &str = "Steps: format (with --format), give it an identity, make it another copy of \
--like's library or write the latest snapshot, write the settings profile, set its role and player name, read \
everything back, eject (with --eject). One plan lists them all.";

const PROVISION_LONG_HELP: &str = "Steps: format (with --format), give it an identity, make it another copy of \
--like's library or write the latest snapshot, write the settings profile, set its role and player name, read \
everything back, eject (with --eject). One plan lists them all.

Examples:
  locallibrary provision /Volumes/NEW --like Home --role backup --eject
  locallibrary provision disk4 --format --check-capacity --name Spare";

/// One drive, or every mounted one.
#[derive(Args)]
pub struct DriveArg {
    #[arg(help = crate::DEFAULT_DRIVE_HELP)]
    pub drive: Option<String>,
}

/// `drive ls` (and `drives`).
#[derive(Args)]
pub struct LsArgs {
    /// Also mounted volumes without a rekordbox export (blank drives, possible targets)
    #[arg(long, conflicts_with = "known")]
    all: bool,
    /// Every drive locallibrary has seen, mounted or not, and how far behind each was
    #[arg(long)]
    known: bool,
    /// With --known: retired drives too
    #[arg(long, requires = "known")]
    retired: bool,
}

/// `drive adopt` (and `adopt`).
#[derive(Args)]
pub struct AdoptArgs {
    #[arg(help = DRIVE_HELP)]
    drive: String,
    /// The name to give it [default: its current name, the volume label for a drive never adopted]
    #[arg(long, conflicts_with = "known")]
    name: Option<String>,
    /// Give it back the identity of a known drive (after it was erased elsewhere; see `drive recognise`)
    #[arg(long = "as", value_name = "KNOWN DRIVE")]
    known: Option<String>,
    /// Show what would change without changing anything
    #[arg(long)]
    dry_run: bool,
}

/// `drive format` (and `format`).
#[derive(Args)]
pub struct FormatArgs {
    /// Mounted drive (path, or an adopted drive's name or id), or a disk: disk4 (macOS), /dev/sdb (Linux), PhysicalDrive2 (Windows)
    #[arg(add = DriveOnly)]
    drive: String,
    /// File system [default: fat32, which every player reads]
    #[arg(long = "fs", value_enum)]
    filesystem: Option<FsArg>,
    /// Volume label [default: the drive's current name]
    #[arg(long)]
    label: Option<String>,
    /// Cluster (allocation unit) size, like 32K [default: what Windows and macOS choose for the size]
    #[arg(long, value_parser = parse_size)]
    cluster_size: Option<u32>,
    /// Test for a fake-capacity drive first (writes test data across the disk; needs administrator rights)
    #[arg(long)]
    check_capacity: bool,
    /// The name the confirmation would ask for typed (the drive's, else the disk's identifier): lets a script erase it
    #[arg(long, value_name = "NAME")]
    erase: Option<String>,
    /// Show the plan without changing anything
    #[arg(long)]
    dry_run: bool,
}

/// `drive verify` (and `verify`).
#[derive(Args)]
pub struct VerifyArgs {
    #[arg(help = crate::ONE_DRIVE_HELP)]
    drive: Option<String>,
    /// Compare with this snapshot (id prefix)
    #[arg(long)]
    snapshot: Option<String>,
    /// Another mounted drive with the same library, for good copies (repeatable)
    #[arg(long = "against", value_name = "DRIVE", add = DriveOnly)]
    against: Vec<String>,
    /// Rewrite what's damaged or missing
    #[arg(long)]
    repair: bool,
    /// List every damaged, unreadable and missing file (not just the first 20 of each)
    #[arg(long)]
    all: bool,
}

/// `drive fsck` (and `fsck`).
#[derive(Args)]
pub struct FsckArgs {
    #[arg(help = DRIVE_HELP)]
    drive: String,
    /// Fix what the check finds (the drive is unmounted while it runs)
    #[arg(long)]
    repair: bool,
}

/// `drive tidy` (and `tidy`).
#[derive(Args)]
pub struct TidyArgs {
    #[arg(help = DRIVE_HELP)]
    drive: String,
    /// Also delete the files in the drive's trash (.Trashes, $RECYCLE.BIN), for good
    #[arg(long)]
    empty_trash: bool,
    /// Show what would be removed without removing anything
    #[arg(long)]
    dry_run: bool,
}

/// `drive recordings` (and `recordings`).
#[derive(Args)]
pub struct RecordingsArgs {
    #[arg(help = crate::ONE_DRIVE_HELP)]
    drive: Option<String>,
    /// Copy them into this folder (a subfolder per drive is made)
    #[arg(long, value_name = "DIR")]
    copy_to: Option<PathBuf>,
    /// Only this recording (repeatable; paths as listed)
    #[arg(long = "only", value_name = "PATH")]
    only: Vec<String>,
    /// Write a tracklist next to the copy: "latest" (the newest history list, for the newest
    /// recording) or a history list's name
    #[arg(long)]
    tracklist: Option<String>,
    /// Delete each recording from the drive once its copy is verified (asks first)
    #[arg(long, requires = "copy_to")]
    delete: bool,
    /// Show what would be copied (and deleted) without doing it
    #[arg(long, requires = "copy_to")]
    dry_run: bool,
}

/// `drive firmware` (and `firmware`).
#[derive(Args)]
pub struct FirmwareArgs {
    #[arg(help = DRIVE_HELP)]
    drive: String,
    /// The .UPD file, named as downloaded
    #[arg(required_unless_present = "remove")]
    file: Option<PathBuf>,
    /// Remove other update files from the drive
    #[arg(long)]
    replace: bool,
    /// Copy it even if a check fails
    #[arg(long)]
    force: bool,
    /// Remove the update files from the drive (after updating)
    #[arg(long, conflicts_with = "file")]
    remove: bool,
    /// Show what would change without changing anything
    #[arg(long)]
    dry_run: bool,
}

/// `drive restore` (and `restore`).
#[derive(Args)]
pub struct RestoreArgs {
    #[arg(help = DRIVE_HELP)]
    drive: String,
    /// Backup id [default: the most recent] (with --playlist: take the playlist from this backup)
    #[arg(long)]
    backup: Option<String>,
    /// Restore even if the drive changed since, or the backup references audio that is no longer
    /// on the drive
    #[arg(long, conflicts_with = "playlist")]
    force: bool,
    /// Only this playlist or folder (path like Folder/Name), as an ordinary playlist edit;
    /// tracks no longer on the drive are left out
    #[arg(long, value_name = "PLAYLIST")]
    playlist: Option<String>,
    /// With --playlist: the snapshot to take it from (id prefix or label) [default: the newest that has it]
    #[arg(long, requires = "playlist", conflicts_with = "backup")]
    snapshot: Option<String>,
    /// Show what restoring would change (files, playlists and tracks, recorded edits) without
    /// restoring
    #[arg(long)]
    dry_run: bool,
}

/// `eject`.
#[derive(Args)]
pub struct EjectArgs {
    /// The drives: mount paths, names or id prefixes, or a library's name for its mounted drives
    /// [default: the mounted drives of one library]
    #[arg(value_name = "DRIVE")]
    drives: Vec<String>,
}

/// `provision`.
#[derive(Args)]
pub struct ProvisionArgs {
    /// The new drive (with --format, a disk such as disk4 or /dev/sdb works too)
    #[arg(add = DriveOnly)]
    target: String,
    /// Make it another copy of this drive's library, or of a library by name (copied from its
    /// newest drive) [default: write the library's latest snapshot]
    #[arg(long, value_name = "DRIVE")]
    like: Option<String>,
    /// The library to write [default: --like's, else the only library in the store]
    #[arg(long, short)]
    library: Option<String>,
    /// A name for the drive [default: its volume name]
    #[arg(long)]
    name: Option<String>,
    /// Erase and format it first
    #[arg(long)]
    format: bool,
    /// With --format: the file system [default: fat32, which every player reads]
    #[arg(long = "fs", value_enum, requires = "format")]
    filesystem: Option<FsArg>,
    /// With --format: the volume label [default: --name, else the drive's current name]
    #[arg(long, requires = "format")]
    label: Option<String>,
    /// With --format: the cluster size, like 32K [default: what Windows and macOS choose for the size]
    #[arg(long, value_parser = parse_size, requires = "format")]
    cluster_size: Option<u32>,
    /// Test for a fake-capacity drive before formatting (needs administrator rights)
    #[arg(long, requires = "format")]
    check_capacity: bool,
    /// primary, backup, set or archive
    #[arg(long)]
    role: Option<String>,
    /// Device Name players show
    #[arg(long)]
    player_name: Option<String>,
    /// Background colour code players show with the name, 0 (none) to 8
    #[arg(long, alias = "player-color")]
    player_colour: Option<i64>,
    /// Don't write the settings profile
    #[arg(long)]
    no_settings: bool,
    /// Don't read everything back afterwards
    #[arg(long)]
    no_verify: bool,
    /// Eject the drive once it's ready
    #[arg(long)]
    eject: bool,
    /// With --format, the name the confirmation would ask for typed (the drive's, else the disk's identifier): for scripts
    #[arg(long, value_name = "NAME", requires = "format")]
    erase: Option<String>,
    /// Show the plan without changing anything
    #[arg(long)]
    dry_run: bool,
}

/// The top-level drive commands.
pub fn run(command: TopCommand, store_path: &Option<PathBuf>, out: Output, ctx: &Ctx<'_>) -> Result<ExitCode> {
    match command {
        TopCommand::Drive(args) => drive(args, store_path, out, ctx),
        TopCommand::Eject(a) => eject(a, store_path, out, ctx),
        TopCommand::Provision(a) => provision(a, store_path, out, ctx),
        TopCommand::Drives(a) => ls(a, store_path, out),
        TopCommand::Info(a) => info(a.drive, out),
        TopCommand::Adopt(a) => adopt(a, store_path, out, ctx),
        TopCommand::Disks => disks(store_path, out),
        TopCommand::Format(a) => format(a, store_path, out, ctx),
        TopCommand::Verify(a) => verify(a, store_path, out, ctx),
        TopCommand::Fsck(a) => fsck(a, store_path, out),
        TopCommand::Space(a) => space(a.drive, store_path, out, ctx),
        TopCommand::Tidy(a) => tidy(a, out, ctx),
        TopCommand::Recordings(a) => recordings(a, store_path, out, ctx),
        TopCommand::Firmware(a) => firmware(a, out, ctx),
        TopCommand::Backups(a) => backups(a.drive, out),
        TopCommand::Restore(a) => restore(a, store_path, out, ctx),
    }
}

/// `locallibrary drive ...`
pub fn drive(args: DriveArgs, store_path: &Option<PathBuf>, out: Output, ctx: &Ctx<'_>) -> Result<ExitCode> {
    let store = || open_store(store_path);
    let command = match (args.command, args.drive) {
        (Some(c), _) => c,
        (None, Some(drive)) => {
            crate::help::check_drive_word("drive", &drive)?;
            return info(Some(drive), out);
        }
        (None, None) => DriveCommand::Ls(LsArgs { all: false, known: false, retired: false }),
    };
    match command {
        DriveCommand::Ls(a) => return ls(a, store_path, out),
        DriveCommand::List { retired } => return ls(LsArgs { all: false, known: true, retired }, store_path, out),
        DriveCommand::Info(a) => return info(a.drive, out),
        DriveCommand::Adopt(a) => return adopt(a, store_path, out, ctx),
        DriveCommand::Disks => return disks(store_path, out),
        DriveCommand::Format(a) => return format(a, store_path, out, ctx),
        DriveCommand::Verify(a) => return verify(a, store_path, out, ctx),
        DriveCommand::Fsck(a) => return fsck(a, store_path, out),
        DriveCommand::Space(a) => return space(a.drive, store_path, out, ctx),
        DriveCommand::Tidy(a) => return tidy(a, out, ctx),
        DriveCommand::Recordings(a) => return recordings(a, store_path, out, ctx),
        DriveCommand::Firmware(a) => return firmware(a, out, ctx),
        DriveCommand::Backups(a) => return backups(a.drive, out),
        DriveCommand::Restore(a) => return restore(a, store_path, out, ctx),
        DriveCommand::Rename { drive, name, label, dry_run } => {
            let store = store().ok();
            let plan = api::drive_rename_plan(&api::RenameDrive { drive, name, label })?;
            if plan.noop {
                return Ok(out.nothing(&plan, format!("{} has that name already", plan.from)));
            }
            if !out.json {
                output::summary(&plan.summary, &plan.warnings);
            }
            if let Some(o) = out.decide(dry_run, "Rename it?") {
                return Ok(out.not_applied(&plan, o));
            }
            let r = plan.apply(store.as_ref(), ctx)?;
            return Ok(out.applied(&plan, &r, Outcome::Applied, || {
                ok(format!("{} is now {}", r.old_name, r.name));
                if let Some(l) = &r.label {
                    ok(format!("volume label: {l}"));
                }
            }));
        }
        DriveCommand::Forget { drive } => {
            let store = store()?;
            let req = DriveRef { drive };
            let rec = api::drive_find(&store, &req)?;
            let change = format!("forget {} ({}): it's registered again the next time it's synced", rec.name, rec.id);
            return out.one_step(
                &change,
                "Forget it?",
                || Ok(api::drive_forget(&store, &req)?),
                |r| ok(format!("forgot {} ({})", r.name, r.id)),
            );
        }
        DriveCommand::Role { drive, role } => {
            let role = (!role.eq_ignore_ascii_case("none")).then_some(role);
            let parsed = role.as_deref().map(Role::parse).transpose()?;
            let store = store()?;
            let rec = api::drive_to_change(&store, &DriveRef { drive: drive.clone() })?;
            let change = match parsed {
                Some(r) => format!("make {} ({}) {}", rec.name, rec.id, role_drive(r)),
                None => format!("take the role off {} ({})", rec.name, rec.id),
            };
            return out.one_step(
                &change,
                "Set it?",
                || Ok(api::drive_role(&store, &api::SetRole { drive, role })?),
                |r| match r.role {
                    Some(role) => ok(format!("{} is {}", r.name, role_drive(role))),
                    None => ok(format!("{} has no role", r.name)),
                },
            );
        }
        DriveCommand::Retire { drive, undo } => {
            let store = store()?;
            let rec = api::drive_to_change(&store, &DriveRef { drive: drive.clone() })?;
            let change = match undo {
                true => format!("put {} ({}) back in use", rec.name, rec.id),
                false => format!("retire {} ({}): syncs leave it out, and writes to it are refused", rec.name, rec.id),
            };
            return out.one_step(
                &change,
                "Go ahead?",
                || Ok(api::drive_retire(&store, &api::RetireDrive { drive, undo })?),
                |r| ok(format!("{} is {}", r.name, if undo { "back in use" } else { "retired" })),
            );
        }
        DriveCommand::Log { drive } => {
            let r = api::drive_log(&store()?, &DriveRef { drive })?;
            out.emit(&r, |r| {
                heading(&r.name);
                say!("  id          {}", r.id);
                if let Some(role) = r.role {
                    say!("  role        {}", role.name());
                }
                say!("  last seen   {} at {}", local_time(r.last_seen, "%Y-%m-%d %H:%M"), r.last_mount.display());
                if let Some(c) = r.capacity {
                    say!("  capacity    {}", bytes(c));
                }
                if let Some(hw) = &r.hardware {
                    say!(
                        "  hardware    {}{}",
                        hw.model,
                        hw.serial.as_deref().map(|s| format!(", serial {s}")).unwrap_or_default()
                    );
                }
                drive_log(&r.log);
            });
        }
        DriveCommand::Recognise { drive } => {
            let r = api::drive_recognise(&store()?, &DriveRef { drive: drive.clone() }, ctx)?;
            out.emit(&r, |r| {
                if r.is_empty() {
                    say!("{drive} doesn't match any drive or snapshot the store knows.");
                    return;
                }
                if let Some(m) = &r.snapshot {
                    say!(
                        "holds {:?} as of snapshot {} ({})",
                        m.library,
                        m.snapshot.short_id,
                        local_time(m.snapshot.created, "%Y-%m-%d %H:%M")
                    );
                }
                if let Some(d) = &r.drive_by_content {
                    say!(
                        "holds what {} held when last seen ({})",
                        style(&d.name).bold(),
                        local_time(d.last_seen, "%Y-%m-%d")
                    );
                }
                if let Some(d) = &r.drive_by_hardware {
                    say!("is on the disk {} was last seen on", style(&d.name).bold());
                }
                if let Some(best) = r.best() {
                    crate::output::hint(format!(
                        "`locallibrary drive adopt {drive} --as \"{}\"` gives it that identity back",
                        best.name
                    ));
                }
            });
        }
        DriveCommand::Reidentify { path, name, force, dry_run } => {
            let store = store().ok();
            let req = api::Reidentify { drive: path.display().to_string(), name, force };
            let plan = api::drive_reidentify_plan(store.as_ref(), &req)?;
            if !out.json {
                output::summary(&plan.summary, &plan.warnings);
            }
            if let Some(o) = out.decide(dry_run, "Give it a new identity?") {
                return Ok(out.not_applied(&plan, o));
            }
            let r = plan.apply(store.as_ref(), ctx)?;
            return Ok(out.applied(&plan, &r, Outcome::Applied, || {
                ok(format!("{} is now {} ({})", r.root.display(), r.name.clone().unwrap_or_default(), r.id.unwrap()))
            }));
        }
        DriveCommand::Player { drive, name, colour, dry_run } => {
            let show = |r: &locallibrary::player::PlayerIdentity| {
                heading(&r.drive);
                match &r.onelibrary {
                    Some(id) => say!(
                        "  OneLibrary  name {}, colour {}",
                        id.name
                            .as_deref()
                            .map(|n| format!("{n:?}"))
                            .unwrap_or_else(|| "not set (players show NO DEVICE NAME)".into()),
                        id.colour.unwrap_or(0)
                    ),
                    None => say!("  OneLibrary  {}", dim("none")),
                }
                if let Some(rb) = &r.rekordbox {
                    say!(
                        "  rekordbox   name {:?}, colour {} (OneLibrary {}), {} set it up",
                        rb.device_name,
                        rb.background_color,
                        rb.background_color_libplus,
                        rb.backup_pc_name
                    );
                }
            };
            if name.is_none() && colour.is_none() {
                out.emit(&api::player_show(&DriveRef { drive })?, show);
                return Ok(ExitCode::SUCCESS);
            }
            let plan = api::player_set_plan(&api::SetPlayerIdentity { drive, name, colour })?;
            if !out.json {
                show(&plan.now);
                output::summary(&plan.summary, &plan.warnings);
            }
            if let Some(o) = out.decide(dry_run, "Set them?") {
                return Ok(out.not_applied(&plan, o));
            }
            let r = plan.apply(None, ctx)?;
            return Ok(out.applied(&plan, &r, Outcome::Applied, || show(&r)));
        }
        DriveCommand::Contact { drive, contact, playlist, dry_run } => {
            let plan = api::drive_contact_plan(&api::SetContact { drive, contact, playlist })?;
            if !out.json {
                output::summary(&plan.summary, &plan.warnings);
            }
            if let Some(o) = out.decide(dry_run, "Write it?") {
                return Ok(out.not_applied(&plan, o));
            }
            let r = plan.apply(None, ctx)?;
            return Ok(out.applied(&plan, &r, Outcome::Applied, || {
                ok(format!("wrote {}", r.file.display()));
                if let Some(p) = &r.playlist {
                    ok(format!(
                        "added the playlist {p:?} (`locallibrary edits replay` puts it back after a rekordbox export)"
                    ));
                }
            }));
        }
        DriveCommand::Groups => {
            let groups = api::drive_sets(&store()?)?;
            out.emit(&groups, |groups| {
                if groups.is_empty() {
                    say!("No mounted drives are linked to a library.");
                }
                for s in groups {
                    heading(s.library.as_deref().unwrap_or("?"));
                    for m in &s.members {
                        let primary = if Some(m) == s.primary.as_ref() { " (primary)" } else { "" };
                        say!("  {}{primary}", m.display());
                    }
                    for e in &s.excluded {
                        say!("  {}", dim(format!("{e} (archive or retired: not synced)")));
                    }
                }
            });
        }
    }
    Ok(ExitCode::SUCCESS)
}

/// `drive ls`: the mounted drives, or (--known) every drive the store has seen.
fn ls(a: LsArgs, store_path: &Option<PathBuf>, out: Output) -> Result<ExitCode> {
    if a.known {
        let list = api::drives_known(&open_store(store_path)?, &api::FleetRequest { all: true, retired: a.retired })?;
        out.emit(&list, |l| known_drives(l));
    } else {
        let drives = api::list_drives(&api::ListDrives { all: a.all });
        let libraries =
            crate::diff_cmd::existing_store(store_path).and_then(|s| s.libraries().ok()).unwrap_or_default();
        let names = libraries.into_iter().map(|l| (l.id, l.name)).collect();
        out.emit(&drives, |d| render::drives(d, &names));
    }
    Ok(ExitCode::SUCCESS)
}

fn info(drive: Option<String>, out: Output) -> Result<ExitCode> {
    let reads = each_drive::read_each_drive(drive, |drive| api::drive_info(&api::DriveRef { drive }))?;
    Ok(reads.emit(out, render::info).exit())
}

fn adopt(a: AdoptArgs, store_path: &Option<PathBuf>, out: Output, ctx: &Ctx<'_>) -> Result<ExitCode> {
    let AdoptArgs { drive, name, known, dry_run } = a;
    if let Some(known) = known {
        let store = open_store(store_path)?;
        let plan = api::drive_adopt_as_plan(&store, &api::AdoptAs { drive, known })?;
        if !out.json {
            output::summary(&plan.summary, &plan.warnings);
        }
        if let Some(o) = out.decide(dry_run, "Give it that identity?") {
            return Ok(out.not_applied(&plan, o));
        }
        let info = plan.apply(Some(&store), ctx)?;
        return Ok(out.applied(&plan, &info, Outcome::Applied, || {
            ok(format!(
                "{} is {} again ({})",
                info.root.display(),
                info.name.clone().unwrap_or_default(),
                info.id.unwrap()
            ))
        }));
    }
    let store = open_store(store_path).ok();
    let plan = api::adopt_plan(&api::AdoptDrive { drive, name })?;
    if plan.noop {
        return Ok(out.nothing(&plan, &plan.summary[0]));
    }
    if !out.json {
        output::summary(&plan.summary, &plan.warnings);
    }
    if let Some(o) = out.decide(dry_run, "Go ahead?") {
        return Ok(out.not_applied(&plan, o));
    }
    let info = plan.apply(store.as_ref(), ctx)?;
    Ok(out.applied(&plan, &info, Outcome::Applied, || {
        ok(format!("{} is {} ({})", info.root.display(), info.name.clone().unwrap_or_default(), info.id.unwrap()))
    }))
}

fn disks(store: &Option<PathBuf>, out: Output) -> Result<ExitCode> {
    let disks = api::list_disks(&store.clone().unwrap_or_else(Store::default_root))?;
    out.emit(&disks, |d| render::disks(d));
    Ok(ExitCode::SUCCESS)
}

/// The store `--store` names, else the default one: refused under sudo when the default belongs to
/// someone other than the user who ran sudo (root's: sudo resets `$HOME` and drops
/// `$LOCALLIBRARY_STORE` on most systems), as a format would then protect and read the wrong one.
fn store_root(store: &Option<PathBuf>) -> Result<PathBuf> {
    if let Some(root) = store {
        return Ok(root.clone());
    }
    let root = Store::default_root();
    #[cfg(unix)]
    if let Some(uid) = std::env::var("SUDO_UID").ok().and_then(|u| u.parse().ok())
        && someone_elses(&root, uid)
    {
        bail!(
            "under sudo the default store is {}, which isn't yours: pass --store with your store's folder",
            root.display()
        );
    }
    Ok(root)
}

/// Whether the store at `root` (or the nearest folder above it that exists) isn't the user
/// `uid`'s.
#[cfg(unix)]
fn someone_elses(root: &std::path::Path, uid: u32) -> bool {
    use std::os::unix::fs::MetadataExt;
    root.ancestors().find_map(|p| std::fs::metadata(p).ok()).is_some_and(|m| m.uid() != uid)
}

fn format(a: FormatArgs, store: &Option<PathBuf>, out: Output, ctx: &Ctx<'_>) -> Result<ExitCode> {
    let FormatArgs { drive, filesystem, label, cluster_size, check_capacity, erase, dry_run } = a;
    let options = FormatOptions { filesystem: filesystem.map(Into::into), label, cluster_size, check_capacity };
    // Only an existing store is consulted (to say whether a library on the drive is backed up);
    // formatting never creates one, which matters under sudo. Its disk is never formatted.
    let root = store_root(store)?;
    let req = api::FormatRequest { drive, options };
    let plan = api::format_plan(&root, &req, ctx)?;
    let store = Store::open(&root).ok();
    if !out.json {
        render::format_plan(&plan);
    }
    if dry_run {
        return Ok(out.not_applied(&plan, Outcome::DryRun));
    }
    if let Some(o) = confirm_erase(&out, &plan, erase.as_deref()) {
        return Ok(out.not_applied(&plan, o));
    }
    let report = plan.apply(store.as_ref(), ctx)?;
    Ok(out.applied(&plan, &report, Outcome::Applied, || render::format_report(&report)))
}

/// Ask for the erase to be confirmed by typing [`FormatPlan::confirmation`] (or `--erase`).
fn confirm_erase(out: &Output, plan: &FormatPlan, erase: Option<&str>) -> Option<Outcome> {
    let name = plan.confirmation();
    let prompt = format!("This erases everything on {}. Type {name:?} to continue:", plan.disk.describe());
    out.confirm_typed(&prompt, name, erase)
}

fn backups(drive: Option<String>, out: Output) -> Result<ExitCode> {
    let reads = each_drive::read_each_drive(drive, |drive| api::list_backups(&api::DriveRef { drive }))?;
    let outcome = reads.emit_headed(out, |l| {
        if l.is_empty() {
            say!("no backups");
        }
        for b in l {
            say!(
                "{}  {}  {}  {}",
                b.id,
                local_time(b.created, "%Y-%m-%d %H:%M"),
                b.reason,
                output::dim(backup_files(b))
            );
        }
    });
    Ok(outcome.exit())
}

/// What a backup holds, in a few words: `12 file(s), 4 audio` (audio may be a track's only copy).
pub fn backup_files(b: &locallibrary::backup::BackupInfo) -> String {
    match b.audio {
        0 => format!("{} file(s)", b.files.len()),
        n => format!("{} file(s), {n} audio", b.files.len()),
    }
}

fn restore(a: RestoreArgs, store_path: &Option<PathBuf>, out: Output, ctx: &Ctx<'_>) -> Result<ExitCode> {
    let RestoreArgs { drive, backup, force, playlist, snapshot, dry_run } = a;
    match playlist {
        Some(playlist) => {
            let req = api::PlaylistRestoreRequest { drive, playlist, snapshot, backup };
            crate::restore_cmd::playlist(req, store_path, dry_run, out, ctx)
        }
        None => crate::restore_cmd::undo(api::UndoRequest { drive: Some(drive), backup, force }, dry_run, out, ctx),
    }
}

fn eject(a: EjectArgs, store_path: &Option<PathBuf>, out: Output, ctx: &Ctx<'_>) -> Result<ExitCode> {
    let drives = crate::jobs_cmd::drives_or_group(a.drives, store_path, None)?;
    let store_root = store_path.clone().unwrap_or_else(Store::default_root);
    // Shown, not asked: ejecting changes nothing on a drive, but takes its disk's other volumes.
    let plan = api::drive_eject_plan(Some(&store_root), &api::EjectDrives { drives })?;
    if !out.json {
        output::summary(&plan.summary, &plan.warnings);
    }
    let r = plan.apply(open_store(store_path).ok().as_ref(), ctx)?;
    Ok(out.applied(&plan, &r, Outcome::Applied, || {
        for e in &r {
            ok(format!(
                "{} ejected{}",
                e.name,
                if e.powered_off { "; it can be pulled out" } else { " (unmounted; power stays on)" }
            ));
        }
    }))
}

fn verify(a: VerifyArgs, store_path: &Option<PathBuf>, out: Output, ctx: &Ctx<'_>) -> Result<ExitCode> {
    let VerifyArgs { drive, snapshot, against, repair, all } = a;
    let drive = crate::jobs_cmd::drive_or_default(drive, store_path, "verify")?;
    output::show_all(all);
    let store = open_store(store_path).ok();
    let req = api::VerifyRequest { drive: drive.clone(), snapshot, against: against.clone() };
    let report = api::drive_verify(store.as_ref(), &req, ctx)?;
    if !out.json {
        scrub_report(&report);
    }
    // Without --repair it's a check: the report and its outcome. With --repair, the check is the
    // plan and the repair its report.
    let found = if report.is_clean() { Outcome::Applied } else { Outcome::Problems };
    if report.is_clean() || !repair {
        return Ok(out.report(&report, found, |_| {}));
    }
    let fixable = report.problems().filter(|p| p.repair.is_some()).count();
    if fixable == 0 {
        warn(
            "no known-good copy of the damaged files is available (pull the drive regularly, or pass --against another drive)",
        );
        return Ok(out.not_applied(&report, found));
    }
    if let Some(o) = out.decide(false, &format!("Rewrite {fixable} file(s) on {drive}?")) {
        return Ok(out.not_applied(&report, o));
    }
    let problems: Vec<_> = report.problems().cloned().collect();
    let r = api::drive_repair(store.as_ref(), &api::RepairRequest { drive, problems, against }, ctx)?;
    let failed = !r.failed.is_empty();
    // Files still wrong after being rewritten: problems found, not fixed.
    let outcome = if failed { Outcome::Problems } else { Outcome::Applied };
    Ok(out.applied(&report, &r, outcome, || {
        if !r.repaired.is_empty() {
            ok(format!("rewrote {} file(s) ({})", r.repaired.len(), bytes(r.bytes)));
        }
        for f in &r.failed {
            warn(format!("{}: {}", f.path, f.error));
        }
        for s in r.skipped.iter().take(10) {
            say!("  {} {}: {}", dim("skipped"), s.path, s.reason);
        }
        if failed {
            warn("files that are still wrong after being rewritten mean the drive itself is failing: replace it");
        }
    }))
}

fn fsck(a: FsckArgs, store_path: &Option<PathBuf>, out: Output) -> Result<ExitCode> {
    let FsckArgs { drive, repair } = a;
    if repair
        && let Some(o) =
            out.decide(false, &format!("Repair the file system on {drive}? It is unmounted while this runs."))
    {
        return Ok(out.not_applied(&serde_json::Value::Null, o));
    }
    let store = open_store(store_path).ok();
    let r = api::drive_fs_check(store.as_ref(), &api::FsCheckRequest { drive, repair })?;
    let outcome = if r.ok { Outcome::Applied } else { Outcome::Problems };
    // Checking is a report; repairing changes the drive.
    Ok(match repair {
        true => out.done_with(&r, outcome, render_fsck),
        false => out.report(&r, outcome, render_fsck),
    })
}

fn render_fsck(r: &locallibrary::fscheck::FsCheckReport) {
    {
        say!("{}", dim(&r.command));
        for line in &r.output {
            say!("  {}", dim(line));
        }
        for w in &r.warnings {
            warn(w);
        }
        if r.ok {
            ok(format!("{}: {}", r.drive, r.summary()));
        } else {
            warn(format!("{}: {}", r.drive, r.summary()));
            if !r.repaired {
                crate::output::hint(
                    "Pull the drive into the store first, then run `locallibrary drive fsck --repair`.",
                );
            }
        }
    }
}

fn space(drive: Option<String>, store_path: &Option<PathBuf>, out: Output, ctx: &Ctx<'_>) -> Result<ExitCode> {
    let store = open_store(store_path).ok();
    let reads = each_drive::read_each_drive(drive, |drive| api::drive_space(store.as_ref(), &DriveRef { drive }, ctx))?;
    Ok(reads.emit(out, space_report).exit())
}

fn tidy(a: TidyArgs, out: Output, ctx: &Ctx<'_>) -> Result<ExitCode> {
    let TidyArgs { drive, empty_trash, dry_run } = a;
    let req = api::TidyRequest { drive: drive.clone(), options: TidyOptions { empty_trash } };
    let plan = api::tidy_plan(&req, ctx)?;
    if plan.noop {
        let trash = plan.trash_bytes;
        if !out.json && trash > 0 {
            crate::output::hint(format!("{} in the trash (--empty-trash deletes it)", bytes(trash)));
        }
        return Ok(out.nothing(&plan, "nothing to tidy"));
    }
    if !out.json {
        heading(&format!("{}: {}", plan.drive, bytes(plan.bytes)));
        for (kind, files, b) in plan.by_kind() {
            say!("  {:<38} {:>6} files  {}", kind.describe(), files, bytes(b));
        }
        if !empty_trash && plan.trash_bytes > 0 {
            crate::output::hint(format!("{} in the trash is kept (--empty-trash deletes it)", bytes(plan.trash_bytes)));
        }
    }
    if let Some(o) = out.decide(dry_run, "Remove them?") {
        return Ok(out.not_applied(&plan, o));
    }
    let r = plan.apply(None, ctx)?;
    Ok(out.applied(&plan, &r, Outcome::Applied, || {
        ok(format!("removed {} file(s), {}", r.removed, bytes(r.bytes)));
        for f in &r.failed {
            warn(format!("{}: {}", f.path, f.error));
        }
    }))
}

fn recordings(a: RecordingsArgs, store_path: &Option<PathBuf>, out: Output, ctx: &Ctx<'_>) -> Result<ExitCode> {
    let RecordingsArgs { drive, copy_to, only, tracklist, delete, dry_run } = a;
    let drive = crate::jobs_cmd::drive_or_default(drive, store_path, "take recordings from")?;
    let Some(to) = copy_to else {
        let list = api::recordings_list(&DriveRef { drive })?;
        out.emit(&list, |l| {
            if l.is_empty() {
                say!("No recordings on this drive.");
            }
            for r in l {
                say!(
                    "{}  {}  {}  {}",
                    r.local_time("%Y-%m-%d %H:%M"),
                    r.duration_secs.map(|d| duration(d as u64)).unwrap_or_else(|| "?".into()),
                    bytes(r.size),
                    r.path
                );
            }
        });
        return Ok(ExitCode::SUCCESS);
    };
    let tracklist = match tracklist.as_deref() {
        None => Tracklist::None,
        Some(t) if t.eq_ignore_ascii_case("latest") => Tracklist::Latest,
        Some(t) => Tracklist::Named(t.to_string()),
    };
    let req = api::CollectRecordings { drive, options: CollectOptions { to, only, tracklist, delete } };
    let plan = api::recordings_collect_plan(&req)?;
    if plan.noop {
        return Ok(out.nothing(&plan, "no recordings on this drive"));
    }
    if !out.json {
        output::summary(&plan.summary, &plan.warnings);
    }
    // Copying off the drive changes nothing on it; deleting is asked about first.
    let stop = match delete {
        true => out.decide(dry_run, "Delete each recording from the drive once its copy is verified?"),
        false => dry_run.then_some(Outcome::DryRun),
    };
    if let Some(o) = stop {
        return Ok(out.not_applied(&plan, o));
    }
    let r = plan.apply(None, ctx)?;
    let failed = !r.failed.is_empty();
    let outcome = if failed { Outcome::Partial } else { Outcome::Applied };
    Ok(out.applied(&plan, &r, outcome, || {
        for c in &r.collected {
            ok(format!(
                "{} → {}{}",
                c.recording,
                c.copied_to.display(),
                if c.deleted { " (deleted from the drive)" } else { "" }
            ));
            if let Some(t) = &c.tracklist {
                say!("  {}", dim(format!("tracklist: {}", t.display())));
            }
        }
        for f in &r.failed {
            warn(format!("{}: {}", f.path, f.error));
        }
    }))
}

fn firmware(a: FirmwareArgs, out: Output, ctx: &Ctx<'_>) -> Result<ExitCode> {
    let FirmwareArgs { drive, file, replace, force, remove, dry_run } = a;
    if remove {
        let plan = api::firmware_remove_plan(&DriveRef { drive })?;
        if plan.noop {
            return Ok(out.nothing(&plan, "no update files on the drive"));
        }
        if !out.json {
            output::summary(&plan.summary, &plan.warnings);
        }
        if let Some(o) = out.decide(dry_run, "Delete them?") {
            return Ok(out.not_applied(&plan, o));
        }
        let removed = plan.apply(None, ctx)?;
        return Ok(out.applied(&plan, &removed, Outcome::Applied, || {
            for f in &removed {
                ok(format!("removed {f}"));
            }
        }));
    }
    let req = api::FirmwareRequest { drive, file: file.expect("required by clap"), replace, force };
    let plan = api::firmware_plan(&req)?;
    if !out.json {
        match &plan.model {
            Some(m) => say!("{} update for the {}: {}", plan.file_name, style(m.model).bold(), bytes(plan.size)),
            None => say!("{}: {}", plan.file_name, bytes(plan.size)),
        }
        for e in &plan.existing {
            say!("  remove {e}");
        }
        for p in &plan.problems {
            warn(p);
        }
        for n in &plan.notes {
            say!("{}", dim(n));
        }
    }
    if !plan.problems.is_empty() && !force {
        out.not_applied(&plan, Outcome::Problems);
        return Ok(Outcome::Problems.exit());
    }
    if let Some(o) = out.decide(dry_run, &format!("Copy it to {}?", plan.drive)) {
        return Ok(out.not_applied(&plan, o));
    }
    let r = plan.apply(None, ctx)?;
    Ok(out.applied(&plan, &r, Outcome::Applied, || {
        ok(format!("wrote {}", r.written.display()));
        crate::output::hint("After updating, `locallibrary drive firmware <drive> --remove` removes it.");
    }))
}

fn provision(a: ProvisionArgs, store_path: &Option<PathBuf>, out: Output, ctx: &Ctx<'_>) -> Result<ExitCode> {
    let ProvisionArgs {
        target,
        like,
        library,
        name,
        format,
        filesystem,
        label,
        cluster_size,
        check_capacity,
        role,
        player_name,
        player_colour,
        no_settings,
        no_verify,
        eject,
        erase,
        dry_run,
    } = a;
    // Under sudo, never one made now: it would belong to root.
    let store = match std::env::var_os("SUDO_UID") {
        Some(_) => {
            let root = store_root(store_path)?;
            Store::open(&root).with_context(|| format!("opening store at {}", root.display()))?
        }
        None => open_store(store_path)?,
    };
    let req = api::ProvisionRequest {
        target,
        like,
        library,
        name,
        format,
        format_options: FormatOptions { filesystem: filesystem.map(Into::into), label, cluster_size, check_capacity },
        role,
        player_name,
        player_colour,
        settings: !no_settings,
        verify: !no_verify,
        eject,
    };
    let plan = api::provision_plan(&store, &req, ctx)?;
    if !out.json {
        if let Some(f) = &plan.format {
            render::format_plan(f);
        }
        heading("Steps");
        for (i, s) in plan.steps.iter().enumerate() {
            say!("  {}. {s}", i + 1);
        }
        for w in &plan.warnings {
            warn(w);
        }
    }
    if dry_run {
        return Ok(out.not_applied(&plan, Outcome::DryRun));
    }
    let decline = match &plan.format {
        Some(f) => confirm_erase(&out, f, erase.as_deref()),
        None => out.decide(false, "Go ahead?"),
    };
    if let Some(o) = decline {
        return Ok(out.not_applied(&plan, o));
    }
    let r = plan.apply(Some(&store), ctx)?;
    let stopped = r.stopped.is_some();
    // Stopped part way: the steps before it were made.
    let outcome = if stopped { Outcome::Partial } else { Outcome::Applied };
    Ok(out.applied(&plan, &r, outcome, || {
        if let Some(f) = &r.format {
            render::format_report(f);
        }
        if let Some(c) = &r.copy {
            render::sync_report(c);
        }
        if r.settings.is_some() {
            ok("settings written");
        }
        if let Some(v) = &r.verify {
            scrub_report(v);
        }
        if let Some(s) = &r.stopped {
            warn(format!("stopped at \"{}\": {}", s.step, s.message));
        } else if let Some(d) = &r.drive {
            ok(format!(
                "{} is ready{}",
                d.name.clone().unwrap_or_default(),
                if r.eject.is_some() { " and ejected" } else { "" }
            ));
        }
    }))
}

// ---------------------------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------------------------

/// "a backup drive", "an archive drive".
fn role_drive(r: Role) -> String {
    let article = if r == Role::Archive { "an" } else { "a" };
    format!("{article} {} drive", r.name())
}

fn standing(s: &Standing) -> String {
    match s {
        Standing::UpToDate => style("up to date").green().to_string(),
        Standing::Behind { snapshots, held } => {
            style(format!("{snapshots} snapshot(s) behind (held {held})")).yellow().to_string()
        }
        Standing::Unsaved => style("had changes not in the store").yellow().to_string(),
        Standing::Unknown => dim("not linked to a library").to_string(),
    }
}

pub fn known_drives(list: &[KnownDrive]) {
    if list.is_empty() {
        say!("No drives registered yet (drives are registered when you pull, sync or adopt them).");
        return;
    }
    for k in list {
        let role = k.role.map(|r| format!(" [{}]", r.name())).unwrap_or_default();
        let place = match &k.mounted {
            Some(p) => style(format!("mounted at {}", p.display())).green().to_string(),
            None => format!("last seen {}", local_time(k.last_seen, "%Y-%m-%d")),
        };
        let retired = if k.log.retired.is_some() { dim(" retired").to_string() } else { String::new() };
        say!(
            "{}{role}{retired}  {}  {place}  {}",
            style(&k.name).bold(),
            k.library.as_deref().unwrap_or("-"),
            standing(&k.standing)
        );
        if let Some(p) = k.log.problems.last() {
            warn(format!("  {}: {} ({})", k.name, p.detail, local_time(p.at, "%Y-%m-%d")));
        }
    }
}

fn drive_log(l: &DriveLog) {
    if let Some(f) = l.first_seen {
        say!("  first seen  {}", local_time(f, "%Y-%m-%d"));
    }
    say!("  written     {} in {} operation(s), {} clean eject(s)", bytes(l.bytes_written), l.writes, l.clean_ejects);
    for (what, c) in [("last check", &l.last_scrub), ("file system", &l.last_fs_check)] {
        if let Some(c) = c {
            let mark = if c.ok { style("ok").green() } else { style("problems").red() };
            say!("  {what:<11} {mark}: {} ({})", c.summary, local_time(c.at, "%Y-%m-%d"));
        }
    }
    if let Some(r) = l.retired {
        say!("  retired     {}", local_time(r, "%Y-%m-%d"));
    }
    for p in l.problems.iter().rev().take(10) {
        warn(format!("{}  {}: {}", local_time(p.at, "%Y-%m-%d"), p.kind, p.detail));
    }
}

/// Each library, a line saying how its clones stand together, then a line for each clone (those
/// not plugged in too); the drives of no library last.
pub fn fleet_status(s: &api::FleetStatus) {
    let in_library = |root: &PathBuf| s.libraries.iter().any(|l| l.mounted.contains(root));
    let loose: Vec<&api::FleetDrive> = s.mounted.iter().filter(|d| !in_library(&d.status.drive.root)).collect();
    let loose_away: Vec<&KnownDrive> = s.offline.iter().filter(|k| k.library.is_none()).collect();
    if s.libraries.is_empty() && loose.is_empty() && loose_away.is_empty() {
        say!("No rekordbox drives mounted.");
        return;
    }
    let away = s.libraries.iter().flat_map(|l| &l.offline).chain(loose_away.iter().copied());
    let name_w = (s.mounted.iter().map(|d| drive_name(&d.status.drive).chars().count()))
        .chain(away.clone().map(|k| k.name.chars().count()))
        .max()
        .unwrap_or(0);
    let role_w = (s.mounted.iter().map(|d| d.role)).chain(away.map(|k| k.role)).flatten().map(|r| r.name().len()).max();
    let clone = |name: &str, role: Option<Role>, state: &str| {
        let role = role_w.map(|w| format!("{:<w$}  ", role.map_or("", Role::name))).unwrap_or_default();
        say!("  {name:<name_w$}  {role}{state}");
    };
    let plugged_in = |d: &api::FleetDrive| {
        let name = drive_name(&d.status.drive);
        let mut state = render::drive_state(&d.status);
        if d.status.settings_drift > 0 {
            state.push_str(&style(format!("  {} setting(s) differ", d.status.settings_drift)).yellow().to_string());
        }
        if d.status.drive.drift.as_ref().is_some_and(|d| !d.is_empty()) {
            state.push_str(&style("  export.pdb and OneLibrary differ (locallibrary drive info)").yellow().to_string());
        }
        clone(&name, d.role, &state);
        let notes = drive_notes(d, &name);
        if !notes.is_empty() {
            say!("    {}", notes.join(", "));
        }
    };
    let unplugged = |k: &KnownDrive| {
        let mut state = dim("not plugged in").to_string();
        if !matches!(k.standing, Standing::Unknown) {
            state.push_str(&format!(" · {}", standing(&k.standing)));
        }
        if k.log.retired.is_some() {
            state.push_str(&dim(" · retired").to_string());
        }
        clone(&k.name, k.role, &state);
    };
    let mounted = |root: &PathBuf| s.mounted.iter().find(|d| d.status.drive.root == *root);
    for lib in &s.libraries {
        let clones = lib.mounted.len() + lib.offline.len();
        let mut about = vec![format!("{} of {} plugged in", lib.mounted.len(), output::plural(clones, "clone"))];
        match lib.identical {
            Some(true) => about.push(style("identical").green().to_string()),
            Some(false) => about.push(style("not identical").yellow().to_string()),
            None => {}
        }
        about.push(match (&lib.head, lib.unsaved) {
            (None, _) => dim("no snapshots yet").to_string(),
            (Some(_), true) => style("changes not saved").yellow().to_string(),
            (Some(h), false) => format!("saved ({})", h.short_id),
        });
        say!("{}  {}", style(&lib.name).bold(), about.join(" · "));
        lib.mounted.iter().filter_map(mounted).for_each(plugged_in);
        lib.offline.iter().for_each(unplugged);
    }
    if !loose.is_empty() || !loose_away.is_empty() {
        heading("No library");
        loose.into_iter().for_each(plugged_in);
        loose_away.into_iter().for_each(unplugged);
    }
    for dup in &s.duplicates {
        warn(format!(
            "{} drives claim to be {} ({}): one is a copy. `locallibrary drive reidentify <path>` gives the copy its own identity{}",
            dup.roots.len(),
            dup.name,
            dup.roots.iter().map(|p| p.display().to_string()).collect::<Vec<_>>().join(", "),
            dup.original.as_ref().map(|o| format!(" ({} is the original)", o.display())).unwrap_or_default()
        ));
    }
    let unsaved = s.mounted.iter().any(|d| d.status.relation == Some(Relation::RightNewer));
    if unsaved && !crate::autostart::installed() && crate::autostart::path().is_some() {
        output::hint("`locallibrary watch --install` saves drives into the store as they're plugged in");
    }
}

fn drive_name(d: &locallibrary::DeviceInfo) -> String {
    d.name.clone().unwrap_or_else(|| d.volume_name.clone())
}

/// What else to know of a mounted drive: write protection, space, an export over recorded edits,
/// a failed check.
fn drive_notes(d: &api::FleetDrive, name: &str) -> Vec<String> {
    let mut notes = Vec::new();
    if d.read_only {
        notes.push(style("write-protected: players can't save history or cues").yellow().to_string());
    }
    if d.low_space {
        notes.push(style("nearly full: players need room for history and cues").yellow().to_string());
    }
    if d.exported_over_edits > 0 {
        let n = d.exported_over_edits;
        notes.push(
            style(format!("rekordbox exported over {n} recorded edit(s): run `locallibrary refresh {name:?}`"))
                .yellow()
                .to_string(),
        );
    }
    if let Some(c) = d.log.as_ref().and_then(|l| l.last_scrub.as_ref()).filter(|c| !c.ok) {
        notes.push(style(format!("last check: {}", c.summary)).red().to_string());
    }
    notes
}

pub fn scrub_report(r: &ScrubReport) {
    heading(&format!("{}: {} files, {}", r.drive, r.files, bytes(r.bytes)));
    if let Some(s) = r.snapshot {
        say!("  {}", dim(format!("compared with snapshot {}", s.short())));
    }
    say!("  {} intact, {} without a reference to compare with", r.verified, r.unverified);
    let source = |p: &locallibrary::scrub::FileProblem| match &p.repair {
        Some(locallibrary::scrub::RepairSource::Store) => dim(" (good copy in the store)").to_string(),
        Some(locallibrary::scrub::RepairSource::Drive { name, .. }) => {
            dim(format!(" (good copy on {name})")).to_string()
        }
        None => String::new(),
    };
    let n = output::cap(20);
    for p in r.damaged.iter().take(n) {
        say!("  {} {}{}", style("damaged").red(), p.path, source(p));
    }
    for p in r.unreadable.iter().take(n) {
        say!("  {} {}: {}{}", style("unreadable").red(), p.path, p.error.as_deref().unwrap_or(""), source(p));
    }
    for p in r.missing.iter().take(n) {
        say!("  {} {}{}", style("missing").yellow(), p.path, source(p));
    }
    let shown = r.damaged.len().min(n) + r.unreadable.len().min(n) + r.missing.len().min(n);
    let total = r.damaged.len() + r.unreadable.len() + r.missing.len();
    if total > shown {
        say!("  {}", dim(format!("... and {} more", total - shown)));
    }
    if !r.tracks.is_empty() {
        say!("  affects {} track(s) in {} playlist(s):", r.tracks.len(), r.playlists.len());
        for t in r.tracks.iter().take(output::cap(10)) {
            say!("    {}", t.name);
        }
    }
    if r.is_clean() {
        ok(format!("{} is intact", r.drive));
    } else if r.problems().any(|p| p.repair.is_some()) {
        crate::output::hint("`locallibrary drive verify --repair` rewrites them from the good copies");
    }
}

fn kind_name(k: SpaceKind) -> &'static str {
    match k {
        SpaceKind::Audio => "audio",
        SpaceKind::Analysis => "analysis (beat grids, cues, waveforms)",
        SpaceKind::Artwork => "artwork",
        SpaceKind::Databases => "databases",
        SpaceKind::Settings => "My Settings",
        SpaceKind::RekordboxOther => "other rekordbox files",
        SpaceKind::LocallibraryBackups => "locallibrary undo backups",
        SpaceKind::Locallibrary => "locallibrary records",
        SpaceKind::Recordings => "recordings made on the player",
        SpaceKind::OsClutter => "macOS/Windows clutter (locallibrary drive tidy)",
        SpaceKind::Trash => "trash (locallibrary drive tidy --empty-trash)",
        SpaceKind::LooseAudio => "audio outside the library",
        SpaceKind::Other => "other files",
        _ => "other",
    }
}

pub fn space_report(r: &SpaceReport) {
    let used = r.categories.iter().map(|c| c.bytes).sum::<u64>();
    heading(&format!("{}: {} used", r.drive, bytes(used)));
    if let (Some(free), Some(total)) = (r.free, r.total) {
        say!("  {} free of {}", bytes(free), bytes(total));
    }
    for c in &r.categories {
        let pct = if used > 0 { c.bytes as f64 * 100.0 / used as f64 } else { 0.0 };
        say!("  {:<44} {:>10}  {:>5.1}%  {} files", kind_name(c.kind), bytes(c.bytes), pct, c.files);
    }
    for f in r.loose_audio.iter().take(5) {
        say!("    {}", dim(format!("{} ({})", f.path, bytes(f.size))));
    }
    if r.loose_audio.len() > 5 {
        say!(
            "    {}",
            dim(format!(
                "... and {} more not in the library (players list them with no analysis)",
                r.loose_audio.len() - 5
            ))
        );
    }
    if let Some(f) = &r.forecast {
        let rate = if f.per_month >= 0 {
            format!("grows about {} a month", bytes(f.per_month as u64))
        } else {
            format!("shrinks about {} a month", bytes(f.per_month.unsigned_abs()))
        };
        say!(
            "  {:?} {rate} ({} snapshots over {} days){}",
            f.library,
            f.snapshots,
            f.span_days,
            f.full_around.map(|d| format!("; full around {}", d.format("%B %Y"))).unwrap_or_default()
        );
    }
    for d in &r.fits {
        if d.fits {
            say!("  {}", dim(format!("fits on {} ({})", d.name, bytes(d.capacity))));
        } else {
            warn(format!("no longer fits on {} ({} usable, {} needed)", d.name, bytes(d.capacity), bytes(d.needed)));
        }
    }
    if r.unreadable > 0 {
        say!("  {}", dim(format!("{} file(s) couldn't be measured", r.unreadable)));
    }
}

/// `sync` with a library, or no drives: the library's mounted drive group, or each mounted
/// library's, the newest drive of each onto the others, with one question for them all.
pub fn sync_set(
    store_path: &Option<PathBuf>,
    library: Option<String>,
    f: &crate::SyncFlags,
    allow_guess: bool,
    out: Output,
    ctx: &Ctx<'_>,
) -> Result<ExitCode> {
    let store: Store = open_store(store_path)?;
    let req = |library| api::SyncSetRequest {
        library,
        allow_low_confidence: allow_guess,
        verify: f.verify,
        keep_extra: f.keep_extra,
        delete_first: f.delete_first,
    };
    let mut libraries: Vec<String> = match library {
        Some(l) => vec![l],
        None => {
            let sets = api::drive_sets(&store)?.into_iter().filter(|s| s.members.len() >= 2);
            sets.filter_map(|s| s.library).collect()
        }
    };
    if libraries.len() < 2 {
        // A group has two or more mounted drives: `resolve` refuses fewer.
        return crate::run_sync_many(
            store_path,
            req(libraries.pop()).resolve(&store)?.drives,
            f,
            allow_guess,
            out,
            ctx,
        );
    }
    // Every mounted library: one that can't be planned (its drives diverged, say) is left out.
    let (mut previews, mut left_out) = (Vec::new(), None);
    for library in libraries {
        let many = req(Some(library.clone())).resolve(&store);
        match many.and_then(|many| api::sync_many_plan(&many, ctx)) {
            Ok(p) => previews.push(p),
            Err(locallibrary::Error::Cancelled) => return Err(locallibrary::Error::Cancelled.into()),
            Err(e) => {
                warn(format!("{library} left out: {e}"));
                left_out = Some(e);
            }
        }
    }
    match (previews.is_empty(), left_out) {
        (true, Some(e)) => Err(e.into()),
        (_, left_out) => crate::many_targets(previews, left_out.is_some(), store_path, f, out, ctx),
    }
}

pub fn target_results(results: &[api::TargetResult]) {
    for r in results {
        match (&r.report, &r.error) {
            (Some(rep), _) => {
                print!("{}: ", style(&r.name).bold());
                render::sync_report(rep);
            }
            (None, Some(e)) => warn(format!("{}: {}", r.name, e.message)),
            _ => {}
        }
    }
}

/// Eject drives after a sync, reporting failures without failing the sync.
pub fn eject_after(store_path: &Option<PathBuf>, drives: Vec<String>, out: Output) {
    let store_root = store_path.clone().unwrap_or_else(Store::default_root);
    let store = open_store(store_path).ok();
    let mut said = Vec::new();
    for d in drives {
        let ejected =
            api::drive_eject_plan(Some(&store_root), &api::EjectDrives { drives: vec![d.clone()] }).and_then(|plan| {
                for w in &plan.warnings {
                    if !said.contains(w) {
                        output::warn(w);
                        said.push(w.clone());
                    }
                }
                plan.apply(store.as_ref(), &Ctx::silent())
            });
        match ejected {
            Ok(r) => {
                if !out.json {
                    for e in r {
                        ok(format!("{} ejected", e.name));
                    }
                }
            }
            Err(e) => output::warn(format!("couldn't eject {d}: {e}")),
        }
    }
}

#[cfg(test)]
mod tests;
