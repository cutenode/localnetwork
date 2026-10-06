//! Formatting a drive for rekordbox players (CDJ, XDJ, OPUS-QUAD, OMNIS-DUO...).
//!
//! A format repartitions the whole disk: one MBR partition from 1 MiB to the end, holding one
//! file system. [`players`] has what each file system and partition scheme works with, and
//! why FAT32 on MBR is the default.
//!
//! The work is done by the operating system's own tools (`diskutil` on macOS, `sfdisk` and
//! `mkfs.*` on Linux, the Storage cmdlets on Windows), except FAT32 on Windows: Windows refuses
//! to create FAT32 above 32 GB, so [`fat32`] writes it.
//!
//! Safety:
//! * Only external or removable, physical, writable disks qualify. Disks holding the running
//!   system, the store or this program are refused, and so are archive and retired drives
//!   unless writing to them is allowed ([`crate::fleet::check_target`]).
//! * [`plan`] lists every volume that will be destroyed, the drive and role it was, and whether
//!   a rekordbox library on it is safe in the store. [`apply`] needs the plan's
//!   [`Disk::identity`], so if a different disk now answers to the same name (drives were
//!   swapped, partitions changed), nothing happens; it looks again just before erasing.

pub mod capacity;
pub mod fat32;
pub mod players;

#[cfg_attr(any(not(unix), target_os = "macos"), allow(dead_code))]
mod linux;
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
mod macos;
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) mod windows;

#[cfg(all(unix, not(target_os = "macos")))]
use linux as os;
#[cfg(target_os = "macos")]
use macos as os;
#[cfg(windows)]
use windows as os;

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::compare::{self, Relation, Side};
use crate::device::Device;
use crate::error::Reason;
use crate::progress::Ctx;
use crate::store::Store;
use crate::{Error, Result};

pub use players::{Compatibility, Verdict};

/// Where the partition starts: 1 MiB, the alignment every modern tool uses.
pub const PARTITION_START: u64 = 1 << 20;

/// FAT32 stores file sizes in 32 bits.
const FAT32_MAX_FILE: u64 = 1 << 32;

/// File systems a drive can be formatted with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Filesystem {
    Fat32,
    Exfat,
    Hfsplus,
}

impl Filesystem {
    pub fn name(self) -> &'static str {
        match self {
            Filesystem::Fat32 => "FAT32",
            Filesystem::Exfat => "exFAT",
            Filesystem::Hfsplus => "HFS+",
        }
    }

    /// The cluster size used unless one is asked for: what Windows and macOS pick by default
    /// (see [`players`] for why that is also the right choice for players).
    pub fn default_cluster_size(self, volume_bytes: u64) -> u32 {
        const MIB: u64 = 1 << 20;
        match self {
            Filesystem::Fat32 => fat32::default_cluster_size(volume_bytes),
            Filesystem::Exfat if volume_bytes <= 256 * MIB => 4096,
            Filesystem::Exfat if volume_bytes <= 32 * 1024 * MIB => 32 * 1024,
            Filesystem::Exfat => 128 * 1024,
            Filesystem::Hfsplus => 4096,
        }
    }

    fn check_cluster_size(self, size: u32, sector: u32) -> Result<()> {
        let ok = size.is_power_of_two()
            && size >= sector
            && match self {
                Filesystem::Fat32 => size <= fat32::MAX_CLUSTER_SIZE,
                Filesystem::Exfat => size <= 32 << 20,
                Filesystem::Hfsplus => size == 4096,
            };
        if ok {
            return Ok(());
        }
        Err(Error::invalid(match self {
            Filesystem::Fat32 => format!("FAT32 clusters must be a power of two from {sector} bytes to 32 KB"),
            Filesystem::Exfat => format!("exFAT clusters must be a power of two from {sector} bytes to 32 MB"),
            Filesystem::Hfsplus => "HFS+ is always formatted with 4 KB blocks".into(),
        }))
    }

    /// Make `name` a valid volume label for this file system.
    pub fn label(self, name: &str) -> String {
        const FALLBACK: &str = "REKORDBOX";
        match self {
            Filesystem::Fat32 => fat32::sanitize_label(name, FALLBACK),
            Filesystem::Exfat => {
                let s: String = name
                    .chars()
                    .map(|c| if c.is_control() || "\"*/:<>?\\|".contains(c) { '_' } else { c })
                    .take(11)
                    .collect();
                let s = s.trim();
                if s.is_empty() { FALLBACK.into() } else { s.into() }
            }
            Filesystem::Hfsplus => {
                let s: String =
                    name.chars().map(|c| if c == ':' || c.is_control() { '_' } else { c }).take(27).collect();
                let s = s.trim();
                if s.is_empty() { FALLBACK.into() } else { s.into() }
            }
        }
    }
}

/// A file system found on a volume.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum FsKind {
    Fat12,
    Fat16,
    Fat32,
    Exfat,
    Hfsplus,
    Apfs,
    Ntfs,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Scheme {
    /// Master Boot Record ("FDisk", "dos").
    Mbr,
    /// GUID partition table.
    Gpt,
    /// Apple partition map.
    Apm,
    Other,
}

/// One partition (or, for disks without a partition table, the disk itself).
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Volume {
    /// OS identifier: `disk4s1`, `/dev/sdb1`, `2:1` (disk:partition on Windows).
    pub id: String,
    pub size: u64,
    pub filesystem: Option<FsKind>,
    /// The file system as the OS names it ("MS-DOS FAT32", "vfat", "exFAT"...).
    pub filesystem_name: Option<String>,
    pub label: Option<String>,
    pub mount: Option<PathBuf>,
    pub cluster_size: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uuid: Option<String>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Disk {
    /// OS identifier: `disk4` (macOS), `/dev/sdb` (Linux), `PhysicalDrive2` (Windows).
    pub id: String,
    /// Whole-disk device path.
    pub device: String,
    pub model: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub serial: Option<String>,
    pub size: u64,
    pub sector_size: u32,
    /// "USB", "SD"...
    pub bus: Option<String>,
    /// External, removable or hot-pluggable.
    pub removable: bool,
    pub internal: bool,
    /// Holds the running operating system.
    pub system: bool,
    /// A disk image, loop device or virtual disk.
    pub virtual_disk: bool,
    pub read_only: bool,
    /// `None` when the disk has no partition table.
    pub scheme: Option<Scheme>,
    pub volumes: Vec<Volume>,
}

impl Disk {
    /// Fingerprint of what is on the disk, for confirming that the disk being formatted is the
    /// one whose plan was shown. Mount points aren't included: they come and go.
    pub fn identity(&self) -> String {
        self.fingerprint(true)
    }

    /// [`Self::identity`], or with `volumes` false, of the disk alone (what a capacity test,
    /// which overwrites the partition table, leaves as it was).
    fn fingerprint(&self, volumes: bool) -> String {
        let mut h = blake3::Hasher::new();
        for s in [&self.id, &self.model, self.serial.as_deref().unwrap_or("")] {
            h.update(s.as_bytes());
            h.update(&[0]);
        }
        h.update(&self.size.to_le_bytes());
        for v in self.volumes.iter().filter(|_| volumes) {
            for s in [&v.id, v.uuid.as_deref().unwrap_or(""), v.label.as_deref().unwrap_or("")] {
                h.update(s.as_bytes());
                h.update(&[0]);
            }
            h.update(&v.size.to_le_bytes());
            h.update(format!("{:?}", v.filesystem).as_bytes());
        }
        h.finalize().to_hex()[..16].to_string()
    }

    /// Why this disk must not be formatted, if it mustn't.
    pub fn problem(&self, protected: &[PathBuf]) -> Option<String> {
        if self.system {
            return Some("it holds the running operating system".into());
        }
        if self.internal || !self.removable {
            return Some("it's an internal disk (only external and removable drives can be formatted)".into());
        }
        if self.virtual_disk && !virtual_disks_allowed() {
            return Some("it's a disk image or virtual disk".into());
        }
        if self.read_only {
            return Some("it's write-protected".into());
        }
        if self.volumes.iter().any(|v| v.mount.as_deref() == Some(Path::new("/"))) {
            return Some("it holds the running operating system".into());
        }
        self.holding(protected).map(|(_, p)| format!("it holds {}", p.display()))
    }

    /// The first of `paths` on one of this disk's mounted volumes, and that volume's mount point.
    pub fn holding<'a>(&'a self, paths: &'a [PathBuf]) -> Option<(&'a Path, &'a PathBuf)> {
        let mounts = self.volumes.iter().filter_map(|v| v.mount.as_deref());
        mounts.flat_map(|m| paths.iter().filter(move |p| p.starts_with(m)).map(move |p| (m, p))).next()
    }

    /// Display name: model and size.
    pub fn describe(&self) -> String {
        let model = if self.model.is_empty() { "Disk" } else { &self.model };
        format!("{model} ({}, {})", self.id, crate::format::human_bytes(self.size))
    }

    /// Bytes available for the partition: the disk minus the 1 MiB lead-in.
    pub fn partition_size(&self) -> u64 {
        let usable = self.size.saturating_sub(PARTITION_START);
        usable - usable % u64::from(self.sector_size.max(512))
    }
}

/// Tests format disk images; set `LOCALLIBRARY_FORMAT_VIRTUAL=1` to allow them.
fn virtual_disks_allowed() -> bool {
    std::env::var_os("LOCALLIBRARY_FORMAT_VIRTUAL").is_some_and(|v| v == "1")
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
    Ok(os::list()?
        .into_iter()
        .filter(|d| d.removable && !d.internal && (!d.virtual_disk || virtual_disks_allowed()))
        .map(|d| view(d, protected))
        .collect())
}

fn view(disk: Disk, protected: &[PathBuf]) -> DiskView {
    DiskView {
        identity: disk.identity(),
        compatibility: players::assess_disk(&disk),
        problem: disk.problem(protected),
        disk,
    }
}

/// The disk with this OS identifier (`disk4`, `/dev/sdb`, `PhysicalDrive2`). `None` if `spec`
/// isn't shaped like one.
pub fn disk_by_id(spec: &str) -> Result<Option<Disk>> {
    os::by_id(spec)
}

/// The disk holding the volume mounted at `root`.
pub fn disk_at(root: &Path) -> Result<Disk> {
    os::by_mount(&root.canonicalize().unwrap_or_else(|_| root.to_path_buf()))
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
        return Err(Error::invalid(format!("refusing to format {}: {p}", disk.describe())));
    }
    let filesystem = opts.filesystem.unwrap_or(players::DEFAULT_FILESYSTEM);
    os::supports(filesystem)?;
    if disk.size >= players::MBR_MAX {
        return Err(Error::invalid(format!(
            "{} is larger than the 2 TiB an MBR partition table can address, and players need MBR; use a drive of 2 TB or less",
            disk.describe(),
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
                if let Some(rec) = store.map(|s| s.drives()).transpose()?.and_then(|mut r| r.remove(&state.id)) {
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
                human_bytes(v.size)
            ));
        }
        erases.push(e);
    }
    if !os::MOUNTS_AFTER_FORMAT {
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
    let mut steps = os::steps(&disk, &target)?;
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

/// What to create (the resolved [`FormatOptions`]).
#[derive(Debug, Clone)]
pub(crate) struct Target {
    pub filesystem: Filesystem,
    pub label: String,
    pub cluster_size: u32,
    /// The OS tools would pick this cluster size anyway.
    pub default_cluster: bool,
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
                    plan.disk.describe(),
                    human_bytes(report.claimed),
                    human_bytes(works),
                    report.failed,
                    report.regions
                ),
            ));
        }
    }
    // Once more, just before erasing: planning again (which reads the libraries on it) and the
    // capacity test take a while. The test overwrote what was on the disk, so after it only the
    // disk itself is compared.
    let now = os::by_id(&plan.disk.id)?;
    still_the_disk(&plan.disk, now.as_ref(), !opts.check_capacity)?;
    let target = Target {
        filesystem: plan.filesystem,
        label: plan.label.clone(),
        cluster_size: plan.cluster_size,
        default_cluster: plan.cluster_size == plan.filesystem.default_cluster_size(plan.partition_size),
    };
    let mount = os::format(&plan.disk, &target, ctx)?;

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
        disk.describe()
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
    let mut dev = capacity::DeviceFile::open(&raw)?;
    capacity::probe(&mut dev, disk.size, disk.sector_size, ctx)
}

impl From<Filesystem> for FsKind {
    fn from(f: Filesystem) -> Self {
        match f {
            Filesystem::Fat32 => FsKind::Fat32,
            Filesystem::Exfat => FsKind::Exfat,
            Filesystem::Hfsplus => FsKind::Hfsplus,
        }
    }
}

pub(crate) fn human_bytes(n: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut v = n as f64;
    let mut u = 0;
    while v >= 1000.0 && u < UNITS.len() - 1 {
        v /= 1000.0;
        u += 1;
    }
    if u == 0 { format!("{n} B") } else { format!("{v:.1} {}", UNITS[u]) }
}

// ---------------------------------------------------------------------------------------------
// Running system tools
// ---------------------------------------------------------------------------------------------

/// A command line, run without a shell.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Cmd {
    pub program: String,
    pub args: Vec<String>,
    pub input: Option<String>,
}

impl Cmd {
    pub fn new(program: &str, args: &[&str]) -> Cmd {
        Cmd { program: program.into(), args: args.iter().map(|s| s.to_string()).collect(), input: None }
    }

    pub fn input(mut self, input: impl Into<String>) -> Cmd {
        self.input = Some(input.into());
        self
    }

    /// Shell-style rendering, for plans and error messages.
    pub fn display(&self) -> String {
        let mut s = self.program.clone();
        for a in &self.args {
            s.push(' ');
            if a.is_empty() || a.contains(|c: char| c.is_whitespace() || "\"'$\\".contains(c)) {
                s.push_str(&format!("{a:?}"));
            } else {
                s.push_str(a);
            }
        }
        if self.input.is_some() {
            s.push_str(" <<< (script)");
        }
        s
    }

    /// Run to completion and return standard output. A non-zero exit becomes
    /// [`Error::External`] carrying the tool's own message.
    pub fn run(&self) -> Result<String> {
        let mut child = Command::new(&self.program)
            .args(&self.args)
            .stdin(if self.input.is_some() { Stdio::piped() } else { Stdio::null() })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| Error::External {
                command: self.program.clone(),
                message: if e.kind() == std::io::ErrorKind::NotFound {
                    format!("not found{}", install_hint(&self.program))
                } else {
                    e.to_string()
                },
            })?;
        if let (Some(input), Some(mut stdin)) = (&self.input, child.stdin.take()) {
            let _ = stdin.write_all(input.as_bytes());
        }
        let out = child
            .wait_with_output()
            .map_err(|e| Error::External { command: self.program.clone(), message: e.to_string() })?;
        let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr);
            let message = [stderr.trim(), stdout.trim()].into_iter().find(|s| !s.is_empty()).unwrap_or("no output");
            return Err(Error::External { command: self.display(), message: message.to_string() });
        }
        Ok(stdout)
    }
}

fn install_hint(program: &str) -> &'static str {
    match program {
        "mkfs.vfat" => " (install dosfstools)",
        "mkfs.exfat" => " (install exfatprogs)",
        "mkfs.hfsplus" => " (install hfsprogs)",
        "sfdisk" | "wipefs" | "lsblk" => " (install util-linux)",
        _ => "",
    }
}

/// Fail with [`Error::Permission`] unless this process may write to the raw device.
pub(crate) fn require_write_access(device: &str, how: &str) -> Result<()> {
    match std::fs::OpenOptions::new().write(true).open(device) {
        Ok(_) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
            Err(Error::Permission(format!("formatting {device} needs administrator rights: {how}")))
        }
        // In use by a mounted volume: allowed, just not right now (the tools unmount first).
        Err(e) if e.kind() == std::io::ErrorKind::ResourceBusy => Ok(()),
        Err(e) => Err(Error::Io { path: device.into(), source: e }),
    }
}

/// Lenient accessors for the JSON that `lsblk`, `plutil` and PowerShell produce (numbers and
/// booleans sometimes arrive as strings, single-element arrays as bare objects).
pub(crate) mod json {
    use serde_json::Value;

    pub fn str(v: &Value, key: &str) -> Option<String> {
        match v.get(key)? {
            Value::String(s) => Some(s.trim().to_string()).filter(|s| !s.is_empty() && s != "\0"),
            Value::Number(n) => Some(n.to_string()),
            _ => None,
        }
    }

    pub fn u64(v: &Value, key: &str) -> Option<u64> {
        match v.get(key)? {
            Value::Number(n) => n.as_u64(),
            Value::String(s) => s.trim().parse().ok(),
            _ => None,
        }
    }

    pub fn bool(v: &Value, key: &str) -> bool {
        match v.get(key) {
            Some(Value::Bool(b)) => *b,
            Some(Value::String(s)) => matches!(s.trim(), "1" | "true" | "True"),
            Some(Value::Number(n)) => n.as_u64() == Some(1),
            _ => false,
        }
    }

    pub fn array(v: Option<&Value>) -> Vec<&Value> {
        match v {
            Some(Value::Array(a)) => a.iter().collect(),
            // Windows PowerShell 5.1 sometimes wraps arrays as {"value": [...], "Count": n}.
            Some(Value::Object(o)) if o.get("value").is_some_and(Value::is_array) => array(o.get("value")),
            Some(o @ Value::Object(_)) => vec![o],
            _ => Vec::new(),
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn sample_disk() -> Disk {
        Disk {
            id: "disk4".into(),
            device: "/dev/disk4".into(),
            model: "DJ TechTools".into(),
            serial: None,
            size: 128_000_000_000,
            sector_size: 512,
            bus: Some("USB".into()),
            removable: true,
            internal: false,
            system: false,
            virtual_disk: false,
            read_only: false,
            scheme: Some(Scheme::Mbr),
            volumes: vec![Volume {
                id: "disk4s1".into(),
                size: 127_999_000_000,
                filesystem: Some(FsKind::Fat32),
                filesystem_name: Some("MS-DOS FAT32".into()),
                label: Some("DNA".into()),
                mount: Some("/Volumes/DNA".into()),
                cluster_size: Some(32768),
                uuid: Some("12345678".into()),
            }],
        }
    }

    #[test]
    fn refuses_system_internal_virtual_and_protected_disks() {
        let ok = sample_disk();
        assert_eq!(ok.problem(&[]), None);
        assert!(Disk { system: true, ..ok.clone() }.problem(&[]).unwrap().contains("operating system"));
        assert!(Disk { internal: true, ..ok.clone() }.problem(&[]).unwrap().contains("internal"));
        assert!(Disk { removable: false, ..ok.clone() }.problem(&[]).unwrap().contains("internal"));
        assert!(Disk { read_only: true, ..ok.clone() }.problem(&[]).unwrap().contains("write-protected"));
        let store = PathBuf::from("/Volumes/DNA/store");
        assert!(ok.problem(&[store]).unwrap().contains("/Volumes/DNA/store"));
        assert_eq!(ok.problem(&[PathBuf::from("/Volumes/DNA2/x")]), None);
    }

    #[test]
    fn holding_finds_a_path_on_any_volume() {
        let mut disk = sample_disk();
        let mut store = disk.volumes[0].clone();
        store.mount = Some("/Volumes/STORE".into());
        disk.volumes.push(store);
        let paths = [PathBuf::from("/Users/x/store"), PathBuf::from("/Volumes/STORE/locallibrary")];
        assert_eq!(disk.holding(&paths), Some((Path::new("/Volumes/STORE"), &paths[1])));
        assert_eq!(disk.holding(&paths[..1]), None);
    }

    /// Regression: the disk was compared with the plan once, before planning again and the
    /// capacity test, and erased by its identifier without looking again.
    #[test]
    fn the_disk_is_checked_again_just_before_it_is_erased() {
        let planned = sample_disk();
        assert!(still_the_disk(&planned, Some(&planned.clone()), true).is_ok());
        let mut relabelled = planned.clone();
        relabelled.volumes[0].label = Some("OTHER".into());
        assert!(matches!(still_the_disk(&planned, Some(&relabelled), true), Err(Error::StalePlan(_))));
        // After the capacity test, which rewrote what was on it, only the disk itself counts.
        relabelled.volumes.clear();
        assert!(still_the_disk(&planned, Some(&relabelled), false).is_ok());
        let swapped = Disk { serial: Some("4C530002".into()), ..planned.clone() };
        assert!(still_the_disk(&planned, Some(&swapped), false).is_err());
        assert!(still_the_disk(&planned, None, false).is_err(), "gone");
    }

    /// Regression: the CLI asked for a volume label when the disk held no adopted drive, the very
    /// word that may have found an unrelated disk.
    #[test]
    fn erasing_is_confirmed_by_the_drives_name_or_the_disks_identifier() {
        let disk = sample_disk();
        let plan = FormatPlan {
            identity: disk.identity(),
            filesystem: Filesystem::Fat32,
            scheme: Scheme::Mbr,
            label: "DNA".into(),
            cluster_size: 32768,
            partition_size: disk.partition_size(),
            erases: Vec::new(),
            keeps_name: None,
            compatibility: players::assess_disk(&disk),
            steps: Vec::new(),
            warnings: Vec::new(),
            protected: Vec::new(),
            disk,
        };
        assert_eq!(plan.disk.volumes[0].label.as_deref(), Some("DNA"));
        assert_eq!(plan.confirmation(), "disk4", "not the label");
        let named = FormatPlan { keeps_name: Some("Backup".into()), ..plan };
        assert_eq!(named.confirmation(), "Backup");
    }

    #[test]
    fn identity_tracks_contents_not_mounts() {
        let a = sample_disk();
        let mut b = a.clone();
        b.volumes[0].mount = None;
        assert_eq!(a.identity(), b.identity());
        b.volumes[0].label = Some("OTHER".into());
        assert_ne!(a.identity(), b.identity());
        let c = Disk { size: a.size + 1, ..a.clone() };
        assert_ne!(a.identity(), c.identity());
    }

    #[test]
    fn labels_per_filesystem() {
        assert_eq!(Filesystem::Fat32.label("Club drive"), "CLUB DRIVE");
        assert_eq!(Filesystem::Exfat.label("Club drive: Tōkyō"), "Club drive_");
        assert_eq!(Filesystem::Hfsplus.label("Back:up"), "Back_up");
        assert_eq!(Filesystem::Exfat.label("  "), "REKORDBOX");
    }

    #[test]
    fn cluster_size_rules() {
        assert!(Filesystem::Fat32.check_cluster_size(32768, 512).is_ok());
        assert!(Filesystem::Fat32.check_cluster_size(65536, 512).is_err());
        assert!(Filesystem::Exfat.check_cluster_size(131072, 512).is_ok());
        assert!(Filesystem::Exfat.check_cluster_size(1000, 512).is_err());
        assert!(Filesystem::Hfsplus.check_cluster_size(8192, 512).is_err());
        assert_eq!(Filesystem::Exfat.default_cluster_size(64_000_000_000), 131072);
        assert_eq!(Filesystem::Exfat.default_cluster_size(16_000_000_000), 32768);
    }

    #[test]
    fn commands_render_like_a_shell() {
        let c = Cmd::new("diskutil", &["eraseDisk", "MS-DOS FAT32", "DNA", "MBR", "disk4"]);
        assert_eq!(c.display(), r#"diskutil eraseDisk "MS-DOS FAT32" DNA MBR disk4"#);
    }

    #[test]
    fn lenient_json() {
        let v: serde_json::Value = serde_json::json!({"a": "1", "b": true, "c": "12", "d": 7, "e": {"value": [1, 2], "Count": 2}, "f": {"x": 1}, "g": "\u{0}"});
        assert!(json::bool(&v, "a") && json::bool(&v, "b") && !json::bool(&v, "zz"));
        assert_eq!(json::u64(&v, "c"), Some(12));
        assert_eq!(json::u64(&v, "d"), Some(7));
        assert_eq!(json::array(v.get("e")).len(), 2);
        assert_eq!(json::array(v.get("f")).len(), 1);
        assert_eq!(json::str(&v, "g"), None);
    }
}
