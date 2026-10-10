//! The drives you own, mounted or not.
//!
//! The store's registry (`drives.json`, see [`crate::store::DriveRecord`]) remembers every drive
//! locallibrary has worked with: its name, library, where and when it was last seen and what it
//! held then. This module turns that into answers:
//!
//! * [`known`]: every drive, with how far behind its library it was when last seen.
//! * [`rename`], [`forget`], [`set_role`], [`retire`]: keeping the registry tidy. A role says
//!   what a drive is for; archive drives (and retired ones) are never overwritten by a sync
//!   unless asked to ([`check_target`]).
//! * [`recognise`] and [`adopt_as`]: a drive that lost `.locallibrary/` (erased elsewhere, or
//!   cleaned by another tool) is matched to the drive it was by its contents or its hardware.
//! * [`duplicates`] and [`reidentify`]: a drive copied in Finder or with a cloning tool carries
//!   the original's identity file along, so two sticks claim one id. The hardware recorded when
//!   the identity was given says which one is the original.
//! * [`DriveLog`]: a short logbook per drive (data written, ejects, check results, problems).

use indexmap::IndexMap;

use rustc_hash::FxHashMap;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::device::{Device, DeviceInfo, LineageEntry, LineageOp};
use crate::error::{Action, Reason};
use crate::hash::Hash;
use crate::progress::Ctx;
use crate::store::{DriveRecord, SnapshotInfo, Store};
use crate::{Error, Result};

/// Most problems kept per drive.
const MAX_PROBLEMS: usize = 50;

/// What a drive is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    /// Where rekordbox exports go: the source for the rest of its group.
    Primary,
    Backup,
    /// Taken out to play sets (`gig`, its old name, is read too).
    #[serde(alias = "gig")]
    Set,
    /// Kept as it is: never overwritten by a sync unless forced.
    Archive,
}

impl Role {
    pub const ALL: [Role; 4] = [Role::Primary, Role::Backup, Role::Set, Role::Archive];

    pub fn name(self) -> &'static str {
        match self {
            Role::Primary => "primary",
            Role::Backup => "backup",
            Role::Set => "set",
            Role::Archive => "archive",
        }
    }

    /// A role by name, ignoring case; `gig` is the old name of `set`.
    pub fn parse(s: &str) -> Result<Role> {
        if s.trim().eq_ignore_ascii_case("gig") {
            return Ok(Role::Set);
        }
        Role::ALL
            .into_iter()
            .find(|r| r.name().eq_ignore_ascii_case(s.trim()))
            .ok_or_else(|| Error::invalid(format!("unknown role {s:?} (primary, backup, set or archive)")))
    }
}

/// The disk a drive was seen on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Hardware {
    pub model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub serial: Option<String>,
    /// The file system's identifier. FAT and exFAT give every format a new one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub volume_uuid: Option<String>,
    /// Whole-disk size in bytes.
    pub size: u64,
}

impl Hardware {
    /// The disk holding the volume mounted at `root`, if the OS will say (it won't for a plain
    /// directory, which tests use as drives).
    pub fn of(root: &Path) -> Option<Hardware> {
        let disk = crate::format::disk_at(root).ok()?;
        let root = crate::device::dunce_canonicalize(root);
        let volume = disk.volumes.iter().find(|v| v.mount.as_deref() == Some(root.as_path()));
        Some(Hardware {
            model: disk.model.clone(),
            serial: disk.serial.clone().filter(|s| plausible_serial(s)),
            volume_uuid: volume.and_then(|v| v.uuid.clone()),
            size: disk.size,
        })
    }

    /// Whether both were seen on the same physical disk. Only a serial number can say so: cheap
    /// sticks often report none, or the same one for every unit of a model, so this is a hint.
    pub fn same_disk(&self, other: &Hardware) -> bool {
        matches!((&self.serial, &other.serial), (Some(a), Some(b)) if a == b)
            && self.model == other.model
            && self.size == other.size
    }

    /// Whether both were seen on the same volume, i.e. nothing reformatted it in between.
    pub fn same_volume(&self, other: &Hardware) -> bool {
        matches!((&self.volume_uuid, &other.volume_uuid), (Some(a), Some(b)) if a.eq_ignore_ascii_case(b))
    }
}

/// Serial numbers some controllers report for every unit.
fn plausible_serial(s: &str) -> bool {
    let t = s.trim();
    t.len() >= 4 && !t.chars().all(|c| c == '0' || c == 'F' || c == 'f') && !t.eq_ignore_ascii_case("0123456789ABCDEF")
}

/// A drive's logbook, kept in the store's registry.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct DriveLog {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_seen: Option<DateTime<Utc>>,
    /// Bytes written to it by sync, push and repair.
    #[serde(default)]
    pub bytes_written: u64,
    /// Operations that wrote to it.
    #[serde(default)]
    pub writes: u32,
    #[serde(default)]
    pub clean_ejects: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_scrub: Option<CheckEntry>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_fs_check: Option<CheckEntry>,
    /// Newest last, at most 50 (the oldest are dropped).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub problems: Vec<Problem>,
    /// No longer in use: hidden from listings, refused as a sync target.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retired: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CheckEntry {
    pub at: DateTime<Utc>,
    pub ok: bool,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[schemars(rename = "FleetProblem")]
pub struct Problem {
    pub at: DateTime<Utc>,
    /// `damaged_files`, `unreadable_files`, `missing_files`, `repair_failed`, `fs_errors`.
    pub kind: String,
    pub detail: String,
}

/// Which check a [`CheckEntry`] records.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Check {
    Scrub,
    FileSystem,
}

// ---------------------------------------------------------------------------------------------
// Registry updates
// ---------------------------------------------------------------------------------------------

/// Read-modify-write one registry record (see [`Store::update_drives`]). `Ok(false)` if the
/// drive isn't registered.
pub fn update(store: &Store, id: Uuid, f: impl FnOnce(&mut DriveRecord)) -> Result<bool> {
    store.update_drives(|drives| Ok(drives.get_mut(&id).map(f).is_some()))
}

/// Note that an operation wrote `bytes` to a drive.
pub fn record_write(store: &Store, device: &Device, bytes: u64) -> Result<()> {
    let Some(id) = device.id() else { return Ok(()) };
    update(store, id, |r| {
        r.log.bytes_written += bytes;
        r.log.writes += 1;
    })?;
    Ok(())
}

pub fn record_eject(store: &Store, id: Uuid) -> Result<()> {
    update(store, id, |r| r.log.clean_ejects += 1)?;
    Ok(())
}

pub fn record_check(store: &Store, id: Uuid, check: Check, ok: bool, summary: impl Into<String>) -> Result<()> {
    let entry = CheckEntry { at: Utc::now(), ok, summary: summary.into() };
    update(store, id, |r| match check {
        Check::Scrub => r.log.last_scrub = Some(entry),
        Check::FileSystem => r.log.last_fs_check = Some(entry),
    })?;
    Ok(())
}

pub fn record_problem(store: &Store, id: Uuid, kind: &str, detail: impl Into<String>) -> Result<()> {
    let p = Problem { at: Utc::now(), kind: kind.to_string(), detail: detail.into() };
    update(store, id, |r| {
        r.log.problems.push(p);
        if r.log.problems.len() > MAX_PROBLEMS {
            let excess = r.log.problems.len() - MAX_PROBLEMS;
            r.log.problems.drain(..excess);
        }
    })?;
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// Every drive
// ---------------------------------------------------------------------------------------------

/// Where a drive stood against its library's latest snapshot when it was last seen.
#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(tag = "standing", rename_all = "snake_case")]
pub enum Standing {
    /// It held the latest snapshot.
    UpToDate,
    /// It held an older snapshot: `snapshots` newer ones have been saved since.
    Behind { snapshots: usize, held: String },
    /// It held a state the store never saved (a later export or edit that wasn't pulled).
    Unsaved,
    /// Not linked to a library, or the library has no snapshots.
    Unknown,
}

/// A drive from the registry.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct KnownDrive {
    pub id: Uuid,
    pub name: String,
    pub role: Option<Role>,
    pub library: Option<String>,
    /// Where it is mounted now, if it is.
    pub mounted: Option<PathBuf>,
    pub last_mount: PathBuf,
    pub last_seen: DateTime<Utc>,
    pub capacity: Option<u64>,
    pub standing: Standing,
    pub log: DriveLog,
}

/// Every registered drive, mounted ones first, then by name. Retired drives only with
/// `include_retired`.
pub fn known(store: &Store, mounted: &[Device], include_retired: bool) -> Result<Vec<KnownDrive>> {
    let libraries: FxHashMap<Uuid, crate::store::LibraryRecord> =
        store.libraries()?.into_iter().map(|l| (l.id, l)).collect();
    // A library's history, loaded once (snapshots are big).
    let mut histories: FxHashMap<Uuid, Vec<SnapshotInfo>> = FxHashMap::default();
    let mut out = Vec::new();
    for rec in store.drives()?.into_values() {
        if rec.log.retired.is_some() && !include_retired {
            continue;
        }
        let lib = rec.library_id.and_then(|id| libraries.get(&id));
        let standing = match (lib, rec.last_fingerprint) {
            (Some(lib), Some(fp)) if lib.head.is_some() => {
                let history = match histories.get(&lib.id) {
                    Some(h) => h,
                    None => histories.entry(lib.id).or_insert(store.history(lib, None)?),
                };
                standing(history, &fp)
            }
            _ => Standing::Unknown,
        };
        out.push(KnownDrive {
            id: rec.id,
            name: rec.name.clone(),
            role: rec.role,
            library: lib.map(|l| l.name.clone()),
            mounted: mounted.iter().find(|d| d.id() == Some(rec.id)).map(|d| d.root().to_path_buf()),
            last_mount: rec.last_mount.clone(),
            last_seen: rec.last_seen,
            capacity: rec.capacity,
            standing,
            log: rec.log.clone(),
        });
    }
    out.sort_by(|a, b| {
        (a.mounted.is_none(), a.name.to_lowercase(), a.id).cmp(&(b.mounted.is_none(), b.name.to_lowercase(), b.id))
    });
    Ok(out)
}

/// `history` is newest first.
fn standing(history: &[SnapshotInfo], fp: &Hash) -> Standing {
    match history.iter().position(|s| &s.fingerprint == fp) {
        Some(0) => Standing::UpToDate,
        Some(n) => Standing::Behind { snapshots: n, held: history[n].short_id.clone() },
        None => Standing::Unsaved,
    }
}

/// Registered drives by name (any case), or by id prefix (4 characters or more) when none has
/// that name.
pub(crate) fn records_named(store: &Store, spec: &str) -> Result<Vec<DriveRecord>> {
    let lower = spec.trim().to_lowercase();
    let drives = store.drives()?;
    let by_name: Vec<DriveRecord> = drives.values().filter(|r| r.name.to_lowercase() == lower).cloned().collect();
    Ok(match by_name.is_empty() && lower.len() >= 4 {
        true => drives.into_values().filter(|r| r.id.to_string().starts_with(&lower)).collect(),
        false => by_name,
    })
}

/// A registered drive by name (any case) or id prefix (4 characters or more). A name several
/// answer to is refused, naming them.
pub fn find_record(store: &Store, spec: &str) -> Result<DriveRecord> {
    let mut matches = records_named(store, spec)?;
    match matches.len() {
        0 => Err(Error::NotFound(format!("no drive in the store's registry is called {spec:?}"))),
        1 => Ok(matches.remove(0)),
        _ => Err(ambiguous(spec, &matches, None)),
    }
}

/// Refusing `spec`, which several drives answer to: these registered ones, and `mounted`, a drive
/// mounted under it that the registry doesn't have. The registered ones' ids (`fields.drives`)
/// tell them apart.
pub(crate) fn ambiguous(spec: &str, records: &[DriveRecord], mounted: Option<&Device>) -> Error {
    let unregistered =
        mounted.map(|d| format!("{} at {}, which the registry doesn't have", d.name(), d.root().display()));
    let named: Vec<String> = unregistered
        .into_iter()
        .chain(records.iter().map(|r| format!("the registered {} ({})", r.name, r.id)))
        .collect();
    Error::invalid(format!("{spec:?} could be {}: give the id", named.join(" or ")))
        .because(Reason::ChooseDrive)
        .with("drives", records.iter().map(|r| r.id.to_string()).collect::<Vec<_>>())
}

// ---------------------------------------------------------------------------------------------
// Rename, forget, roles, retire
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct RenameReport {
    pub id: Uuid,
    pub old_name: String,
    pub name: String,
    /// The volume label, when it was changed too.
    pub label: Option<String>,
}

/// Rename a mounted drive: its identity file and the registry. With `label`, the volume label
/// too (what computers show; players show the Device Name set in rekordbox). The identity file is
/// read again once the drive is locked, as another process may have written it since this one
/// opened the drive.
pub fn rename(store: Option<&Store>, device: &mut Device, name: &str, label: bool) -> Result<RenameReport> {
    let name = name.trim();
    if name.is_empty() {
        return Err(Error::invalid("the name is empty"));
    }
    // Before anything changes: the label is refused under the test switch.
    if label {
        crate::device::disks_shown()?;
    }
    let _lock = device.lock()?;
    device.reload_state()?;
    let old_name = device.name();
    let id = device.adopt(Some(name))?.id;
    if let Some(store) = store {
        crate::sync::register_drive(store, device, None)?;
    }
    let label = if label { Some(set_volume_label(device.root(), name)?) } else { None };
    Ok(RenameReport { id, old_name, name: name.to_string(), label })
}

/// The volume label a name makes (FAT labels are 11 characters, upper case).
pub fn volume_label(name: &str) -> String {
    crate::format::fat32::sanitize_label(name, "REKORDBOX")
}

/// Change the volume label of the volume mounted at `root`. Returns the label as written (FAT
/// labels are 11 characters, upper case). Refused while `$LOCALLIBRARY_VOLUMES` hides the
/// machine's drives (`device::disks_shown`).
pub fn set_volume_label(root: &Path, name: &str) -> Result<String> {
    crate::device::disks_shown()?;
    let label = crate::format::fat32::sanitize_label(name, "REKORDBOX");
    #[cfg(target_os = "macos")]
    let cmd = crate::format::Cmd::new("diskutil", &["rename", &root.to_string_lossy(), &label]);
    #[cfg(windows)]
    let cmd = windows_label_command(root, &label)?;
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let _ = (root, &label);
        Err(Error::invalid(
            "changing the volume label isn't supported on Linux: unmount the drive and run fatlabel (or exfatlabel) on its partition",
        ))
    }
    #[cfg(any(target_os = "macos", windows))]
    {
        cmd.run()?;
        Ok(label)
    }
}

/// Windows: `Set-Volume`, for the drive whose root is `root` (`E:\`).
#[cfg_attr(not(windows), allow(dead_code))]
fn windows_label_command(root: &Path, label: &str) -> Result<crate::format::Cmd> {
    use crate::format::windows::{quote, root_letter};
    let script = format!("Set-Volume -DriveLetter {} -NewFileSystemLabel {}", root_letter(root)?, quote(label));
    Ok(crate::format::Cmd::new("powershell", &["-NoProfile", "-Command", &script]))
}

/// Remove a drive from the registry. Its own identity file is untouched: it is registered again
/// the next time locallibrary writes to it.
pub fn forget(store: &Store, spec: &str) -> Result<DriveRecord> {
    let rec = find_record(store, spec)?;
    store.forget_drive(rec.id)?;
    Ok(rec)
}

/// Set (or with `None` clear) a drive's role. A library has at most one primary: making a drive
/// primary makes its library's previous primary a backup.
pub fn set_role(store: &Store, id: Uuid, role: Option<Role>) -> Result<DriveRecord> {
    store.update_drives(|drives| {
        let library = drives.get(&id).ok_or_else(|| Error::NotFound(format!("drive {id} in the registry")))?.library_id;
        if role == Some(Role::Primary) {
            for other in drives.values_mut().filter(|r| r.id != id && r.library_id == library) {
                if other.role == Some(Role::Primary) {
                    other.role = Some(Role::Backup);
                }
            }
        }
        let rec = drives.get_mut(&id).expect("checked above");
        rec.role = role;
        Ok(rec.clone())
    })
}

/// Mark a drive retired (or, with `undo`, back in use).
pub fn retire(store: &Store, id: Uuid, undo: bool) -> Result<DriveRecord> {
    let mut out = None;
    update(store, id, |r| {
        r.log.retired = if undo { None } else { Some(Utc::now()) };
        out = Some(r.clone());
    })?;
    out.ok_or_else(|| Error::NotFound(format!("drive {id} in the registry")))
}

/// Refuse to overwrite an archive or retired drive unless `force` (or
/// [`crate::op::allow_archive_writes`]).
pub fn check_target(store: Option<&Store>, target: &Device, force: bool) -> Result<()> {
    let force = force || crate::op::archive_writes_allowed();
    let (Some(store), Some(id), false) = (store, target.id(), force) else { return Ok(()) };
    let Some(rec) = store.drives()?.shift_remove(&id) else { return Ok(()) };
    if rec.log.retired.is_some() {
        let drive = target.root().display().to_string();
        return Err(Error::conflict(
            Reason::Retired,
            format!("{} is retired; bring it back, or force the write", rec.name),
        )
        .with("drive", &drive)
        .action(Action::call(
            "drive.retire",
            serde_json::json!({ "drive": drive, "undo": true }),
            format!("Bring {} back", rec.name),
        ))
        .action(Action::retry(serde_json::json!({ "overwrite_archive": true }), "Write to it anyway")));
    }
    if rec.role == Some(Role::Archive) {
        return Err(Error::conflict(
            Reason::Archive,
            format!("{} is an archive drive and is kept as it is; force the write to overwrite it", rec.name),
        )
        .with("drive", target.root())
        .action(Action::retry(serde_json::json!({ "overwrite_archive": true }), "Overwrite it anyway")));
    }
    Ok(())
}

/// The primary drive of a library (or of drives not linked to one).
pub fn primary_of(store: &Store, library: Option<Uuid>) -> Result<Option<DriveRecord>> {
    Ok(store
        .drives()?
        .into_values()
        .find(|r| r.library_id == library && r.role == Some(Role::Primary) && r.log.retired.is_none()))
}

/// A drive group: the mounted drives that share a library and can be synced together (not
/// archive or retired), plus the library's primary if it's mounted.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct DriveSet {
    pub library_id: Option<Uuid>,
    pub library: Option<String>,
    pub primary: Option<PathBuf>,
    pub members: Vec<PathBuf>,
    /// Mounted members left out: archive or retired.
    pub excluded: Vec<String>,
}

/// Mounted drives grouped by library.
pub fn sets(store: &Store, mounted: &[Device]) -> Result<Vec<DriveSet>> {
    let registry = store.drives()?;
    let libraries: FxHashMap<Uuid, String> = store.libraries()?.into_iter().map(|l| (l.id, l.name)).collect();
    let mut groups: IndexMap<Option<Uuid>, DriveSet> = IndexMap::new();
    for d in mounted.iter().filter(|d| d.has_export() || d.is_adopted()) {
        let Some(state) = d.state() else { continue };
        let lib = state.library_id;
        let set = groups.entry(lib).or_insert_with(|| DriveSet {
            library_id: lib,
            library: lib.and_then(|id| libraries.get(&id).cloned()),
            primary: None,
            members: Vec::new(),
            excluded: Vec::new(),
        });
        let rec = registry.get(&state.id);
        if rec.is_some_and(|r| r.log.retired.is_some() || r.role == Some(Role::Archive)) {
            set.excluded.push(d.name());
            continue;
        }
        if rec.is_some_and(|r| r.role == Some(Role::Primary)) {
            set.primary = Some(d.root().to_path_buf());
        }
        set.members.push(d.root().to_path_buf());
    }
    Ok(groups.into_values().filter(|s| s.library_id.is_some()).collect())
}

// ---------------------------------------------------------------------------------------------
// Recognising drives
// ---------------------------------------------------------------------------------------------

/// What an unadopted (or re-identified) drive matches.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Recognition {
    /// Its library state is a stored snapshot.
    pub snapshot: Option<SnapshotMatch>,
    /// A registered drive last seen holding exactly this state.
    pub drive_by_content: Option<RegisteredRef>,
    /// A registered drive last seen on this disk (same serial number).
    pub drive_by_hardware: Option<RegisteredRef>,
}

impl Recognition {
    pub fn is_empty(&self) -> bool {
        self.snapshot.is_none() && self.drive_by_content.is_none() && self.drive_by_hardware.is_none()
    }

    /// The single registered drive this most likely is.
    pub fn best(&self) -> Option<&RegisteredRef> {
        self.drive_by_content.as_ref().or(self.drive_by_hardware.as_ref())
    }
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct SnapshotMatch {
    pub library: String,
    pub snapshot: SnapshotInfo,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct RegisteredRef {
    pub id: Uuid,
    pub name: String,
    pub last_seen: DateTime<Utc>,
}

impl From<&DriveRecord> for RegisteredRef {
    fn from(r: &DriveRecord) -> Self {
        RegisteredRef { id: r.id, name: r.name.clone(), last_seen: r.last_seen }
    }
}

/// Match a drive to what the store knows, by content (its fingerprint) and by hardware.
pub fn recognise(store: &Store, device: &Device, ctx: &Ctx<'_>) -> Result<Recognition> {
    let hardware = Hardware::of(device.root());
    if !device.has_export() {
        return recognise_with(store, device, None, hardware.as_ref());
    }
    let library = device.library().ok();
    let scanned = device.scan_fingerprinted(library.as_ref(), ctx)?;
    let current = recognise_with(store, device, scanned.fingerprint, hardware.as_ref())?;
    // A snapshot taken by an earlier version may carry one of the state's legacy fingerprints.
    let mut found = current;
    for legacy in scanned.legacy_fingerprints {
        if found.snapshot.is_some() {
            break;
        }
        let older = recognise_with(store, device, Some(legacy), hardware.as_ref())?;
        found = Recognition {
            snapshot: older.snapshot,
            drive_by_content: found.drive_by_content.or(older.drive_by_content),
            drive_by_hardware: found.drive_by_hardware,
        };
    }
    Ok(found)
}

/// [`recognise`] with the fingerprint and hardware already known.
pub fn recognise_with(
    store: &Store,
    device: &Device,
    fp: Option<Hash>,
    hardware: Option<&Hardware>,
) -> Result<Recognition> {
    let registry = store.drives()?;
    let own = device.id();
    let mut snapshot = None;
    let mut drive_by_content = None;
    if let Some(fp) = fp {
        for lib in store.libraries()? {
            if let Some(s) = store.find_by_fingerprint(&lib, &fp)? {
                snapshot = Some(SnapshotMatch { library: lib.name.clone(), snapshot: s });
                break;
            }
        }
        // Most recently seen first.
        let mut candidates: Vec<&DriveRecord> =
            registry.values().filter(|r| Some(r.id) != own && r.last_fingerprint == Some(fp)).collect();
        candidates.sort_by_key(|r| std::cmp::Reverse(r.last_seen));
        if candidates.len() == 1 {
            drive_by_content = Some(RegisteredRef::from(candidates[0]));
        } else if let Some(source) = snapshot.as_ref().and_then(|m| m.snapshot.source.as_ref()).and_then(|s| s.drive_id)
        {
            // Several drives held it; the one the snapshot was taken from is the best guess.
            drive_by_content = candidates.iter().find(|r| r.id == source).map(|r| RegisteredRef::from(*r));
        }
    }
    let drive_by_hardware = hardware.and_then(|hw| {
        registry
            .values()
            .filter(|r| Some(r.id) != own)
            .find(|r| r.hardware.as_ref().is_some_and(|h| h.same_disk(hw)))
            .map(RegisteredRef::from)
    });
    Ok(Recognition { snapshot, drive_by_content, drive_by_hardware })
}

/// Give a drive the identity of a registered drive: its id, name, library, and the lineage up to
/// the state it holds if that is a stored snapshot. Refused while another mounted drive holds
/// that identity, or the drive has one of its own by the time it's locked.
pub fn adopt_as(
    store: &Store,
    device: &mut Device,
    spec: &str,
    mounted: &[Device],
    ctx: &Ctx<'_>,
) -> Result<DeviceInfo> {
    let rec = find_record(store, spec)?;
    if let Some(other) = mounted.iter().find(|d| d.id() == Some(rec.id) && d.root() != device.root()) {
        return Err(Error::conflict(
            Reason::IdentityInUse,
            format!("{} is mounted at {} and already holds that identity", rec.name, other.root().display()),
        )
        .with("path", other.root()));
    }
    let _lock = device.lock()?;
    device.reload_state()?;
    if device.id().is_some_and(|id| id != rec.id) {
        let drive = device.root().display().to_string();
        return Err(Error::conflict(
            Reason::HasIdentity,
            format!(
                "{} already has its own identity ({}); re-identifying it replaces an identity",
                device.name(),
                device.id().expect("checked")
            ),
        )
        .with("drive", &drive)
        .action(Action::call(
            "drive.reidentify",
            serde_json::json!({ "drive": drive }),
            "Give it a new identity",
        )));
    }
    let fp = if device.has_export() {
        let library = device.library().ok();
        device.scan_fingerprinted(library.as_ref(), ctx)?.fingerprint
    } else {
        None
    };
    let mut lineage: Vec<LineageEntry> = Vec::new();
    if let (Some(fp), Some(lib_id)) = (fp, rec.library_id)
        && let Some(lib) = store.library_by_id(lib_id)?
        && let Some(info) = store.find_by_fingerprint(&lib, &fp)?
    {
        lineage = store.load_snapshot(&info.id)?.lineage;
    }
    if let Some(fp) = fp
        && lineage.last().is_none_or(|e| e.fingerprint != fp)
    {
        let tracks = device.library().map(|l| l.tracks.len()).unwrap_or(0);
        lineage.push(LineageEntry {
            fingerprint: fp,
            at: Utc::now(),
            op: LineageOp::Observed,
            snapshot: None,
            source: None,
            tracks,
        });
    }
    let mut state = crate::device::DeviceState::new(&rec.name);
    state.id = rec.id;
    state.library_id = rec.library_id;
    state.lineage = lineage;
    state.created = rec.log.first_seen.unwrap_or(state.created);
    state.hardware = Hardware::of(device.root()).or(rec.hardware.clone());
    device.replace_state(state)?;
    crate::sync::register_drive(store, device, fp)?;
    Ok(device.info())
}

// ---------------------------------------------------------------------------------------------
// Copies
// ---------------------------------------------------------------------------------------------

/// Mounted drives sharing one identity.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Duplicate {
    pub id: Uuid,
    pub name: String,
    pub roots: Vec<PathBuf>,
    /// The one on the disk the identity was given on, when the hardware says.
    pub original: Option<PathBuf>,
}

/// Groups of mounted drives that claim the same identity. With `hardware`, the disk behind each
/// root (looked up with [`Hardware::of`] when `None`).
pub fn duplicates(mounted: &[Device], hardware: &dyn Fn(&Path) -> Option<Hardware>) -> Vec<Duplicate> {
    let mut by_id: IndexMap<Uuid, Vec<&Device>> = IndexMap::new();
    for d in mounted {
        if let Some(id) = d.id() {
            by_id.entry(id).or_default().push(d);
        }
    }
    by_id
        .into_iter()
        .filter(|(_, ds)| ds.len() > 1)
        .map(|(id, ds)| {
            let recorded = ds.iter().find_map(|d| d.state().and_then(|s| s.hardware.clone()));
            let original = recorded.and_then(|rec| {
                let hits: Vec<&&Device> = ds
                    .iter()
                    .filter(|d| hardware(d.root()).is_some_and(|h| h.same_disk(&rec) || h.same_volume(&rec)))
                    .collect();
                (hits.len() == 1).then(|| hits[0].root().to_path_buf())
            });
            Duplicate { id, name: ds[0].name(), roots: ds.iter().map(|d| d.root().to_path_buf()).collect(), original }
        })
        .collect()
}

/// Whether `device` is a copy of another drive that carried its identity file along: another of
/// `mounted` has its identity, or it's on another disk or volume than the one recorded with the
/// identity (as far as the OS says).
pub fn is_copy(device: &Device, mounted: &[Device]) -> bool {
    let twin = mounted.iter().any(|d| d.id().is_some() && d.id() == device.id() && d.root() != device.root());
    let recorded = device.state().and_then(|s| s.hardware.clone());
    let moved = recorded.zip(Hardware::of(device.root())).is_some_and(|(then, now)| {
        let other_disk = then.serial.is_some() && now.serial.is_some() && !now.same_disk(&then);
        let other_volume = then.volume_uuid.is_some() && now.volume_uuid.is_some() && !now.same_volume(&then);
        other_disk || other_volume
    });
    twin || moved
}

/// The name a copy gets from [`reidentify`] when it's given none.
pub fn copy_name(device: &Device) -> String {
    format!("{} (copy)", device.name())
}

/// Give a drive a new identity, keeping its library link and lineage (it does hold those
/// states), as its identity file holds them once the drive is locked. For the copy of a drive
/// that carried the original's identity file along.
pub fn reidentify(store: Option<&Store>, device: &mut Device, name: Option<&str>) -> Result<DeviceInfo> {
    let _lock = device.lock()?;
    device.reload_state()?;
    let old =
        device.state().cloned().ok_or_else(|| Error::invalid(format!("{} has no identity yet", device.name())))?;
    let mut state = crate::device::DeviceState::new(name.map(str::to_string).unwrap_or_else(|| copy_name(device)));
    state.library_id = old.library_id;
    state.lineage = old.lineage;
    state.hardware = Hardware::of(device.root());
    device.replace_state(state)?;
    if let Some(store) = store {
        crate::sync::register_drive(store, device, None)?;
    }
    Ok(device.info())
}

/// Record the disk a drive is on in its identity, if it isn't recorded yet.
pub fn note_hardware(device: &mut Device) -> Result<()> {
    if device.state().is_none_or(|s| s.hardware.is_some()) {
        return Ok(());
    }
    if let Some(hw) = Hardware::of(device.root()) {
        device.state_mut()?.hardware = Some(hw);
        device.save_state()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snap(fp: u8, id: &str) -> SnapshotInfo {
        SnapshotInfo {
            id: Hash::of(id.as_bytes()),
            short_id: id.into(),
            library_id: Uuid::nil(),
            parents: vec![],
            created: Utc::now(),
            message: None,
            source: None,
            fingerprint: Hash::of(&[fp]),
            summary: Default::default(),
            files: 0,
            bytes: 0,
            labels: Vec::new(),
            beside: false,
        }
    }

    #[test]
    fn standing_counts_newer_snapshots() {
        let history = vec![snap(3, "c"), snap(2, "b"), snap(1, "a")];
        assert!(matches!(standing(&history, &Hash::of(&[3])), Standing::UpToDate));
        assert!(matches!(standing(&history, &Hash::of(&[1])), Standing::Behind { snapshots: 2, .. }));
        assert!(matches!(standing(&history, &Hash::of(&[9])), Standing::Unsaved));
    }

    #[test]
    fn roles_parse_by_name() {
        assert_eq!(Role::parse("Archive").unwrap(), Role::Archive);
        assert!(Role::parse("spare").is_err());
        // `gig` is the old name of `set`: read in requests and the registry, written as `set`.
        assert_eq!(Role::parse("GIG").unwrap(), Role::Set);
        assert_eq!(serde_json::from_str::<Role>("\"gig\"").unwrap(), Role::Set);
        assert_eq!(serde_json::to_string(&Role::Set).unwrap(), "\"set\"");
    }

    #[test]
    fn windows_labels_the_drive_letter_with_set_volume() {
        let cmd = windows_label_command(Path::new(r"E:\"), &volume_label("DJ's USB")).unwrap();
        assert_eq!(cmd.args.last().unwrap(), "Set-Volume -DriveLetter E -NewFileSystemLabel 'DJ''S USB'");
        assert!(windows_label_command(Path::new(r"E:\Music"), "X").is_err(), "only a drive's root");
    }

    /// Under the test switch the volume holding a drive folder is this machine's own: its label
    /// stays, and the drive keeps its name too.
    #[test]
    fn the_test_switch_leaves_volume_labels_alone() {
        let dir = tempfile::tempdir().unwrap();
        let mut d = Device::open(dir.path()).unwrap();
        d.adopt(Some("Main")).unwrap();
        crate::testing::volumes_on_this_thread(Some(Vec::new()));
        let renamed = rename(None, &mut d, "Club", true);
        // A path that isn't there: were the switch ignored, no tool could find a volume for it.
        let labelled = set_volume_label(&dir.path().join("gone"), "Club");
        crate::testing::volumes_on_this_thread(None);
        for refused in [renamed.map(|r| r.name), labelled] {
            assert!(
                matches!(&refused, Err(Error::Invalid(m)) if m.starts_with("LOCALLIBRARY_VOLUMES is set")),
                "{refused:?}"
            );
        }
        assert_eq!(Device::open(dir.path()).unwrap().name(), "Main");
    }

    #[test]
    fn same_disk_needs_a_serial() {
        let a = Hardware { model: "Cruzer".into(), serial: None, volume_uuid: Some("A".into()), size: 64 };
        assert!(!a.same_disk(&a.clone()));
        let b = Hardware { serial: Some("4C530001".into()), ..a.clone() };
        assert!(b.same_disk(&b.clone()));
        assert!(!b.same_disk(&Hardware { size: 32, ..b.clone() }));
        assert!(a.same_volume(&Hardware { volume_uuid: Some("a".into()), ..a.clone() }));
        assert!(!plausible_serial("0000000000"));
        assert!(plausible_serial("4C530001123456789012"));
    }
}
