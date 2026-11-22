//! Formatting a drive for rekordbox players (CDJ, XDJ, OPUS-QUAD, OMNIS-DUO...).
//!
//! A format repartitions the whole disk: one MBR partition from 1 MiB to the end, holding one
//! file system. [`players`] has what each file system and partition scheme works with, and
//! why FAT32 on MBR is the default.
//!
//! loclib-format finds the disks and does the work, with the operating system's own tools
//! (`diskutil` on macOS, `sfdisk` and `mkfs.*` on Linux, the Storage cmdlets on Windows), except
//! FAT32 on Windows: Windows refuses to create FAT32 above 32 GB, so its [`fat32`] writer makes
//! it. This module decides what may be erased and plans the erase.
//!
//! Safety:
//! * Only external or removable, physical, writable disks qualify. Disks holding the running
//!   system, the store or this program are refused, and so are archive and retired drives
//!   unless writing to them is allowed ([`crate::fleet::check_target`]).
//! * [`plan`] lists every volume that will be destroyed, the drive and role it was, and whether
//!   a rekordbox library on it is safe in the store. [`apply`] needs the plan's
//!   [`Disk::identity`], so if a different disk now answers to the same name (drives were
//!   swapped, partitions changed), nothing happens; it looks again just before erasing.

pub use loclib_capacity as capacity;
pub mod players;

use std::cell::Cell;
use std::path::{Path, PathBuf};

use loclib_format::{Cmd, Target, fat32};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::compare::{self, Relation, Side};
use crate::device::Device;
use crate::error::Reason;
use crate::progress::Ctx;
use crate::store::Store;
use crate::{Error, Result};

pub use loclib_equipment::FsKind;
// What plans, listings and reports hold.
pub use loclib_format::{Disk, Filesystem, Scheme, Volume};
pub use players::{Compatibility, Verdict};

thread_local! {
    /// The operating system [`untested_here`] answers for on this thread (`None`: this one).
    pub(crate) static THREAD_OS: Cell<Option<&'static str>> = const { Cell::new(None) };
}

/// The warning for `what` (formatting, ejecting...) on Linux and Windows: it drives the
/// operating system's disk tools, which have only ever run on macOS (docs/install.md, "Platforms"). The
/// plans that do it carry this. Delete it and its uses once each has had a run there.
pub fn untested_here(what: &str) -> Option<String> {
    let os = match THREAD_OS.with(Cell::get).unwrap_or(std::env::consts::OS) {
        "linux" => "Linux",
        "windows" => "Windows",
        _ => return None,
    };
    Some(format!("{what} hasn't been tested on {os} yet; check the result"))
}

/// FAT32 stores file sizes in 32 bits.
const FAT32_MAX_FILE: u64 = 1 << 32;

/// `disk`'s name to show: its model, identifier and size.
pub fn describe(disk: &Disk) -> String {
    let model = if disk.model.is_empty() { "Disk" } else { &disk.model };
    format!("{model} ({}, {})", disk.id, crate::op::bytes(disk.size))
}

/// A disk, how players would get on with it as it is, and whether it can be formatted.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct DiskView {
    pub disk: Disk,
    pub identity: String,
    pub compatibility: Compatibility,
    /// Why it can't be formatted (`None`: it can).
    pub problem: Option<String>,
}

/// External and removable disks.
pub fn list(protected: &[PathBuf]) -> Result<Vec<DiskView>> {
    Ok(views(loclib_format::list()?, protected))
}

/// The external and removable ones of `disks`, as [`list`] shows them.
fn views(disks: Vec<Disk>, protected: &[PathBuf]) -> Vec<DiskView> {
    disks
        .into_iter()
        .filter(|d| d.removable && !d.internal && !d.virtual_disk)
        .map(|disk| DiskView {
            identity: disk.identity(),
            compatibility: players::assess_disk(&disk),
            problem: disk.problem(protected),
            disk,
        })
        .collect()
}

/// The disk with this OS identifier (`disk4`, `/dev/sdb`, `PhysicalDrive2`). `None` if `spec`
/// isn't shaped like one.
pub fn disk_by_id(spec: &str) -> Result<Option<Disk>> {
    Ok(loclib_format::by_id(spec)?)
}

/// The disk holding the volume mounted at `root`. Refused while `$LOCALLIBRARY_VOLUMES` hides
/// this machine's disks (see [`crate::device::volume_folders`]): the disk holding a test folder is
/// this machine's own, and the system's disk tools don't run then.
pub fn disk_at(root: &Path) -> Result<Disk> {
    crate::device::disks_shown()?;
    Ok(loclib_format::by_mount(&root.canonicalize().unwrap_or_else(|_| root.to_path_buf()))?)
}

/// The speed `disk`'s USB link came up at, in bits a second, where the system says (macOS, Linux).
pub fn link_speed(disk: &Disk) -> Option<u64> {
    loclib_format::link_speed(disk)
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct FormatOptions {
    /// Default: FAT32, which every player reads.
    #[serde(default)]
    pub filesystem: Option<Filesystem>,
    /// Volume label. Default: the drive's locallibrary name, else its current label.
    #[serde(default)]
    pub label: Option<String>,
    /// Bytes per cluster. Default: [`Filesystem::default_cluster_size`].
    #[serde(default)]
    pub cluster_size: Option<u32>,
    /// Test the disk's real capacity first (see [`capacity`]), and stop if it is fake.
    #[serde(default)]
    pub check_capacity: bool,
}

/// A volume that formatting destroys.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct ErasedVolume {
    pub volume: Volume,
    /// locallibrary's name for the drive, if it was adopted.
    pub drive_name: Option<String>,
    /// Its role in the store. An archive or retired drive is only erased when writing to one is
    /// allowed (see [`crate::fleet::check_target`]).
    pub role: Option<crate::fleet::Role>,
    /// Retired in the store.
    pub retired: bool,
    /// Tracks in the rekordbox library on it, if any.
    pub tracks: Option<usize>,
    /// Whether that library's current state is in the store (`None`: no library, or no store).
    pub in_store: Option<bool>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct FormatPlan {
    pub disk: Disk,
    /// Pass this to [`apply`] to confirm the disk.
    pub identity: String,
    pub filesystem: Filesystem,
    pub scheme: Scheme,
    pub label: String,
    pub cluster_size: u32,
    /// Size of the new partition.
    pub partition_size: u64,
    pub erases: Vec<ErasedVolume>,
    /// The locallibrary name and id the drive keeps (only when the OS mounts it afterwards).
    pub keeps_name: Option<String>,
    /// How players will get on with the result.
    pub compatibility: Compatibility,
    /// What will run.
    pub steps: Vec<String>,
    pub warnings: Vec<String>,
    /// What the plan was made never to erase (`protected`): applying it keeps to that.
    #[serde(skip)]
    pub(crate) protected: Vec<PathBuf>,
}

impl FormatPlan {
    /// What to ask to have typed to confirm the erase: the adopted drive's name when the drive
    /// keeps it ([`FormatPlan::keeps_name`], never on Linux, which doesn't mount the new volume),
    /// else the disk's identifier. Never a volume label: the name just typed could be an
    /// unrelated volume's, and typing it again would confirm nothing.
    pub fn confirmation(&self) -> &str {
        self.keeps_name.as_deref().unwrap_or(&self.disk.id)
    }
}

/// Work out what formatting `disk` with `opts` would do. Nothing is changed. `protected` are
/// paths that must survive: a disk holding one of them (the store, this program) is refused.
pub fn plan(
    disk: Disk,
    opts: &FormatOptions,
    store: Option<&Store>,
    protected: &[PathBuf],
    ctx: &Ctx<'_>,
) -> Result<FormatPlan> {
    if let Some(p) = disk.problem(protected) {
        return Err(Error::invalid(format!("refusing to format {}: {p}", describe(&disk))));
    }
    let filesystem = opts.filesystem.unwrap_or(players::DEFAULT_FILESYSTEM);
    loclib_format::supports(filesystem)?;
    if disk.size >= loclib_equipment::MBR_MAX {
        return Err(Error::invalid(format!(
            "{} is larger than the 2 TiB an MBR partition table can address, and players need MBR; use a drive of 2 TB or less",
            describe(&disk),
        )));
    }
    let partition_size = disk.partition_size();
    let default_cluster = filesystem.default_cluster_size(partition_size);
    let cluster_size = opts.cluster_size.unwrap_or(default_cluster);
    filesystem.check_cluster_size(cluster_size, disk.sector_size)?;
    if filesystem == Filesystem::Fat32 {
        fat32::Layout::new(partition_size, disk.sector_size, cluster_size, 0)?;
    }

    let mut warnings = Vec::new();
    let mut erases = Vec::new();
    let mut keeps = None;
    for v in &disk.volumes {
        let mut e = ErasedVolume {
            volume: v.clone(),
            drive_name: None,
            role: None,
            retired: false,
            tracks: None,
            in_store: None,
        };
        if let Some(d) = v.mount.as_deref().and_then(|m| Device::open(m).ok()) {
            if let Some(state) = d.state() {
                // An archive or retired drive is kept as it is, by a format too.
                crate::fleet::check_target(store, &d, false)?;
                if let Some(rec) = store.map(|s| s.drives()).transpose()?.and_then(|mut r| r.shift_remove(&state.id)) {
                    (e.role, e.retired) = (rec.role, rec.log.retired.is_some());
                }
                e.drive_name = Some(state.name.clone());
                keeps = keeps.or_else(|| Some(state.name.clone()));
            }
            if d.has_export() {
                let library = d.library().ok();
                e.tracks = library.as_ref().map(|l| l.tracks.len());
                let too_big = library.iter().flat_map(|l| &l.tracks).filter(|t| t.file_size >= FAT32_MAX_FILE).count();
                if filesystem == Filesystem::Fat32 && too_big > 0 {
                    warnings.push(format!(
                        "{too_big} track(s) on {} are 4 GB or larger and won't fit on FAT32 (exFAT holds them)",
                        d.name()
                    ));
                }
                e.in_store = store.map(|s| library_in_store(s, &d, ctx)).transpose()?;
                let what = format!(
                    "{} holds a rekordbox library ({} tracks)",
                    d.name(),
                    e.tracks.map_or("unreadable".into(), |n| n.to_string())
                );
                match e.in_store {
                    Some(true) => {}
                    Some(false) => {
                        warnings.push(format!("{what} that isn't in the store. Pull it first to keep a copy."))
                    }
                    None => warnings.push(format!("{what}. Everything on it will be lost.")),
                }
            }
        } else if v.mount.is_none() && v.size > 0 && v.label.as_deref() != Some("EFI") {
            warnings.push(format!(
                "{} ({}) isn't mounted, so its contents can't be shown; it will be erased too.",
                v.label.as_deref().unwrap_or(&v.id),
                crate::op::bytes(v.size)
            ));
        }
        erases.push(e);
    }
    if !loclib_format::MOUNTS_AFTER_FORMAT {
        keeps = None;
    }
    let label = filesystem.label(opts.label.as_deref().or(keeps.as_deref()).unwrap_or_else(|| {
        disk.volumes.iter().find_map(|v| v.label.as_deref().filter(|l| !l.trim().is_empty())).unwrap_or("REKORDBOX")
    }));
    let target =
        Target { filesystem, label: label.clone(), cluster_size, default_cluster: cluster_size == default_cluster };
    let mut compatibility = players::assess(Some(Scheme::Mbr), Some(filesystem.into()), partition_size);
    if filesystem == Filesystem::Fat32 {
        compatibility.notes.push("FAT32 can't hold files of 4 GB or more".into());
    }
    if compatibility.verdict != Verdict::AllPlayers {
        warnings.push("this drive won't work on every player (see above)".into());
    }
    warnings.extend(untested_here(if opts.check_capacity { "formatting with a capacity check" } else { "formatting" }));
    let mut steps = loclib_format::steps(&disk, &target)?;
    if opts.check_capacity {
        capacity_supported()?;
        steps.insert(
            0,
            format!(
                "test the real capacity: write and read back {} test regions of 1 MiB across the whole disk (stops if it is fake)",
                capacity::region_count(disk.size, disk.sector_size)
            ),
        );
    }
    Ok(FormatPlan {
        identity: disk.identity(),
        steps,
        filesystem,
        scheme: Scheme::Mbr,
        label,
        cluster_size,
        partition_size,
        erases,
        keeps_name: keeps,
        compatibility,
        warnings,
        disk,
        protected: protected.to_vec(),
    })
}

/// Whether the drive's current library state is safely in the store.
fn library_in_store(store: &Store, d: &Device, ctx: &Ctx<'_>) -> Result<bool> {
    let Some(lib) = d.state().and_then(|s| s.library_id).map(|id| store.library_by_id(id)).transpose()?.flatten()
    else {
        return Ok(false);
    };
    let Some(head) = lib.head else { return Ok(false) };
    let snap = store.load_snapshot(&head)?;
    let left = Side::from_snapshot(store, &snap, "store")?;
    let right = Side::from_device(d, ctx)?;
    // Identical, or the store is ahead: nothing on the drive is missing from the store.
    Ok(matches!(compare::compare(&left, &right).relation, Relation::Identical | Relation::LeftNewer))
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct FormatReport {
    pub disk: String,
    pub filesystem: Filesystem,
    pub label: String,
    /// Where the new volume is mounted (`None` if the OS didn't mount it).
    pub mount: Option<PathBuf>,
    /// Cluster size of the mounted volume, as the OS reports it.
    pub cluster_size: Option<u64>,
    pub capacity: Option<u64>,
    /// The locallibrary name the drive kept.
    pub drive_name: Option<String>,
}

/// Format the disk `plan` was made for, re-checking everything first (an archive or retired drive
/// on it is refused again, unless writing to one is allowed). `identity` must be the plan's
/// [`FormatPlan::identity`].
pub fn apply(
    disk: Disk,
    identity: &str,
    opts: &FormatOptions,
    store: Option<&Store>,
    protected: &[PathBuf],
    ctx: &Ctx<'_>,
) -> Result<FormatReport> {
    if disk.identity() != identity {
        return Err(not_the_planned_disk(&disk));
    }
    let plan = plan(disk, opts, store, protected, ctx)?;
    // No locallibrary operation may be writing to the drive, and its identity is kept.
    let mut keep = None;
    for v in &plan.disk.volumes {
        let Some(d) = v.mount.as_deref().and_then(|m| Device::open(m).ok()) else { continue };
        if let Some(state) = d.state() {
            drop(d.lock()?);
            keep = keep.or_else(|| Some(state.clone()));
        }
    }
    ctx.check()?;
    if opts.check_capacity {
        let report = probe_capacity(&plan.disk, ctx)?;
        if report.is_fake() {
            let works = report.works_up_to.unwrap_or(0);
            return Err(Error::conflict(
                Reason::FakeCapacity,
                format!(
                    "{} is fake: it reports {} but only about the first {} keep their data ({} of {} test regions came back wrong). \
                 It wasn't formatted, and its previous contents are damaged where the test wrote. Return it.",
                    describe(&plan.disk),
                    crate::op::bytes(report.claimed),
                    crate::op::bytes(works),
                    report.failed,
                    report.regions
                ),
            ));
        }
    }
    // Once more, just before erasing: planning again (which reads the libraries on it) and the
    // capacity test take a while. The test overwrote what was on the disk, so after it only the
    // disk itself is compared.
    let now = loclib_format::by_id(&plan.disk.id)?;
    still_the_disk(&plan.disk, now.as_ref(), !opts.check_capacity)?;
    let target = Target {
        filesystem: plan.filesystem,
        label: plan.label.clone(),
        cluster_size: plan.cluster_size,
        default_cluster: plan.cluster_size == plan.filesystem.default_cluster_size(plan.partition_size),
    };
    let mount = loclib_format::format(&plan.disk, &target, ctx)?;

    let mut drive_name = None;
    if let (Some(m), Some(old)) = (&mount, keep) {
        let mut d = Device::open(m)?;
        d.reset_identity(&old)?;
        if let Some(store) = store {
            crate::sync::register_drive(store, &d, None)?;
        }
        drive_name = Some(old.name);
    }
    Ok(FormatReport {
        disk: plan.disk.id.clone(),
        filesystem: plan.filesystem,
        label: plan.label,
        cluster_size: mount.as_deref().map(crate::fsutil::cluster_size),
        capacity: mount.as_deref().and_then(crate::fsutil::total_space),
        mount,
        drive_name,
    })
}

/// Refuse unless `now`, the disk under the planned disk's identifier now, is the planned disk:
/// with `volumes`, holding what it held then too.
fn still_the_disk(planned: &Disk, now: Option<&Disk>, volumes: bool) -> Result<()> {
    if now.map(|d| d.fingerprint(volumes)) == Some(planned.fingerprint(volumes)) {
        return Ok(());
    }
    Err(not_the_planned_disk(planned))
}

/// The refusal when `disk` isn't the one a format was planned for any more: another disk answers
/// to its identifier, or what is on it changed.
pub(crate) fn not_the_planned_disk(disk: &Disk) -> Error {
    Error::StalePlan(format!(
        "{} isn't the disk that was planned any more (a drive was swapped, or what is on it changed); it wasn't formatted. Plan again",
        describe(disk)
    ))
}

fn capacity_supported() -> Result<()> {
    if cfg!(windows) {
        return Err(Error::invalid(
            "checking the capacity isn't available on Windows yet (GRC's ValiDrive tests drives on Windows)",
        ));
    }
    Ok(())
}

/// Unmount every volume of `disk` and run the capacity probe on the whole disk.
fn probe_capacity(disk: &Disk, ctx: &Ctx<'_>) -> Result<capacity::ProbeReport> {
    capacity_supported()?;
    #[cfg(target_os = "macos")]
    let raw = {
        Cmd::new("diskutil", &["unmountDisk", &disk.id]).run()?;
        format!("/dev/r{}", disk.id.trim_start_matches("/dev/"))
    };
    #[cfg(not(target_os = "macos"))]
    let raw = {
        for v in &disk.volumes {
            if let Some(m) = &v.mount {
                Cmd::new("umount", &[&m.to_string_lossy()]).run()?;
            }
        }
        disk.device.clone()
    };
    let mut dev = capacity::DeviceFile::open_to_overwrite(&raw)?;
    Ok(capacity::probe(&mut dev, disk.size, disk.sector_size, ctx)?)
}

#[cfg(test)]
pub(crate) mod tests;

#[cfg(test)]
#[path = "tests/disk_image.rs"]
mod disk_image;
