//! Drive management: the drives you own (mounted or not), their health, their space, what the
//! players show, and setting drives up and handing them over.

use std::path::PathBuf;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::{
    ApplySettings, Direction, DriveRef, DriveStatus, FormatApply, FormatRequest, PushRequest, SyncManyPreview,
    SyncManyRequest, SyncRequest, TargetResult, known_drives, resolve_drive, resolve_one_drive,
};
use crate::compare::Relation;
use crate::device::{Device, DeviceInfo};
use crate::eject::{self, EjectReport};
use crate::error::{Action, Reason};
use crate::firmware::{self, FirmwarePlan, FirmwareReport};
use crate::fleet::{self, Check, DriveSet, Duplicate, KnownDrive, Recognition, RenameReport, Role};
use crate::format::{self, FormatOptions, FormatPlan, FormatReport};
use crate::fscheck::{self, FsCheckReport};
use crate::hash::Hash;
use crate::op::{Operation, Planned, Probe, Touched, bytes};
use crate::player::{self, ContactReport, PlayerIdentity};
use crate::progress::{Ctx, Steps};
use crate::recordings::{self, CollectOptions, CollectReport, Recording};
use crate::scrub::{self, FileProblem, RepairReport, ScrubOptions, ScrubReport};
use crate::settings::ApplyReport;
use crate::space::{self, SpaceReport};
use crate::store::{DriveRecord, SnapshotInfo, Store};
use crate::sync::SyncReport;
use crate::tidy::{self, TidyOptions, TidyPlan, TidyReport};
use crate::{Error, Result};

// ---------------------------------------------------------------------------------------------
// Every drive
// ---------------------------------------------------------------------------------------------

/// A mounted drive's status, with what the registry knows about it.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct FleetDrive {
    #[serde(flatten)]
    pub status: DriveStatus,
    pub role: Option<Role>,
    /// Write-protected: players can't save history or cues to it.
    pub read_only: bool,
    /// Less free space than players need for history and cues.
    pub low_space: bool,
    pub log: Option<fleet::DriveLog>,
    /// Other mounted drives claiming the same identity (copies of this one, or this a copy).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub same_identity_as: Vec<PathBuf>,
    /// Recorded edits a rekordbox export replaced (see [`super::exported_over_edits`]): 0, or how
    /// many `locallibrary refresh` (or `edits replay`) makes again.
    pub exported_over_edits: usize,
}

#[derive(Debug, Clone, Default, Serialize, JsonSchema)]
pub struct FleetStatus {
    pub mounted: Vec<FleetDrive>,
    /// Registered drives that aren't mounted (with `all`).
    pub offline: Vec<KnownDrive>,
    /// Mounted drives sharing one identity (copies of each other).
    pub duplicates: Vec<Duplicate>,
    /// The libraries of the mounted drives (with `all`, of every registered drive too), by name:
    /// each with its clones. Drives of no library in the store are in none.
    pub libraries: Vec<FleetLibrary>,
}

/// A library and its clones, the drives holding it: those plugged in, and those that aren't.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct FleetLibrary {
    pub id: Uuid,
    pub name: String,
    /// Its latest snapshot.
    pub head: Option<SnapshotInfo>,
    /// Where its clones that are plugged in are mounted (each is in `mounted`), by role: the
    /// primary first.
    pub mounted: Vec<PathBuf>,
    /// Its registered clones that aren't plugged in (retired ones with `retired`).
    pub offline: Vec<KnownDrive>,
    /// Whether its clones plugged in hold the same library: `None` with fewer than two, or when
    /// one couldn't be compared with the store.
    pub identical: Option<bool>,
    /// Whether a clone plugged in holds changes the store doesn't have (it's newer than the latest
    /// snapshot, or diverged from it).
    pub unsaved: bool,
}

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct FleetRequest {
    /// Include drives that aren't mounted.
    #[serde(default)]
    pub all: bool,
    /// Include retired drives.
    #[serde(default)]
    pub retired: bool,
}

/// [`super::status`] for mounted drives, plus roles, write protection, space, copies, each
/// library with its clones (those not plugged in too) and (with `all`) every registered drive
/// that isn't mounted.
pub fn fleet_status(store: &Store, req: &FleetRequest, ctx: &Ctx<'_>) -> Result<FleetStatus> {
    let registry = store.drives()?;
    let devices = known_drives(false);
    let duplicates = fleet::duplicates(&devices, &fleet::Hardware::of);
    let mut mounted = Vec::new();
    let mut states = Vec::new();
    for (status, state) in super::statuses(store, ctx)? {
        let same_identity_as = duplicates
            .iter()
            .filter(|d| d.roots.contains(&status.drive.root))
            .flat_map(|d| d.roots.iter().filter(|r| **r != status.drive.root).cloned())
            .collect();
        let rec = status.drive.id.and_then(|id| registry.get(&id));
        let read_only = crate::format::disk_at(&status.drive.root).is_ok_and(|d| d.read_only);
        let low_space = status.drive.free_space.is_some_and(|f| f < space::PLAYER_HEADROOM);
        let exported_over_edits =
            devices.iter().find(|d| d.root() == status.drive.root).map_or(0, super::exported_over_edits);
        mounted.push(FleetDrive {
            exported_over_edits,
            role: rec.and_then(|r| r.role),
            log: rec.map(|r| r.log.clone()),
            read_only,
            low_space,
            same_identity_as,
            status,
        });
        states.push(state);
    }
    let linked: Vec<Uuid> =
        (mounted.iter()).filter(|f| f.status.library.is_some()).filter_map(|f| f.status.drive.library_id).collect();
    // Reading every library's history for where the drives not plugged in stand takes a while:
    // only when there's one to show.
    let listed = |r: &DriveRecord| {
        (req.retired || r.log.retired.is_none())
            && !devices.iter().any(|d| d.id() == Some(r.id))
            && (req.all || r.library_id.is_some_and(|id| linked.contains(&id)))
    };
    let known = match registry.values().any(listed) {
        true => fleet::known(store, &devices, req.retired)?,
        false => Vec::new(),
    };
    let offline: Vec<KnownDrive> = known.into_iter().filter(|k| k.mounted.is_none()).collect();
    let mut libraries = Vec::new();
    for lib in store.libraries()? {
        let mut clones: Vec<usize> = (0..mounted.len())
            .filter(|&i| mounted[i].status.library.is_some() && mounted[i].status.drive.library_id == Some(lib.id))
            .collect();
        clones.sort_by_key(|&i| (mounted[i].role.is_none(), mounted[i].role));
        let away: Vec<KnownDrive> = offline.iter().filter(|k| k.library.as_ref() == Some(&lib.name)).cloned().collect();
        let shown = !clones.is_empty() || (req.all && !away.is_empty());
        if !shown {
            continue;
        }
        let held: Option<Vec<Hash>> = clones.iter().map(|&i| states[i]).collect();
        libraries.push(FleetLibrary {
            id: lib.id,
            head: lib.head.map(|h| store.load_info(&h)).transpose()?,
            mounted: clones.iter().map(|&i| mounted[i].status.drive.root.clone()).collect(),
            offline: away,
            identical: held.filter(|h| h.len() > 1).map(|h| h.iter().all(|x| *x == h[0])),
            unsaved: clones
                .iter()
                .any(|&i| matches!(mounted[i].status.relation, Some(Relation::RightNewer | Relation::Diverged))),
            name: lib.name,
        });
    }
    libraries.sort_by_key(|l| l.name.to_lowercase());
    let offline = if req.all { offline } else { Vec::new() };
    Ok(FleetStatus { mounted, offline, duplicates, libraries })
}

/// Every registered drive, mounted or not.
pub fn drives_known(store: &Store, req: &FleetRequest) -> Result<Vec<KnownDrive>> {
    fleet::known(store, &known_drives(false), req.retired)
}

/// Every mounted volume (blank ones too), described without reading a database (see
/// [`crate::watch`]): cheap enough to call every second, unlike [`super::list_drives`].
pub fn drives_mounted() -> Vec<crate::watch::MountedDrive> {
    crate::watch::mounted_drives()
}

/// The drive `spec` names, identity first: the adopted drive mounted there (or by that name),
/// else, when no mounted drive answers to it, the registry's record of that name or id. A name
/// several mounted drives share, or a folder here with the drive's name, is refused.
enum Named {
    Mounted(Device),
    Registered(DriveRecord),
}

fn named(store: &Store, spec: &str) -> Result<Named> {
    match resolve_one_drive(spec) {
        Ok(d) if d.is_adopted() => Ok(Named::Mounted(d)),
        Ok(_) | Err(Error::NotFound(_)) => fleet::find_record(store, spec).map(Named::Registered),
        Err(e) => Err(e),
    }
}

/// The record of the drive `spec` names ([`named`]), a mounted one registered (or its record
/// brought up to date) first: for the registry writes that take it.
fn drive_record(store: &Store, spec: &str) -> Result<DriveRecord> {
    let mut d = match named(store, spec)? {
        Named::Mounted(d) => d,
        Named::Registered(rec) => return Ok(rec),
    };
    fleet::note_hardware(&mut d)?;
    crate::sync::register_drive(store, &d, None)?;
    let id = d.id().expect("adopted");
    store.drives()?.shift_remove(&id).ok_or_else(|| Error::NotFound(format!("drive {id}")))
}

/// The drive [`drive_role`] and [`drive_retire`] change, as the registry has it or, for a mounted
/// drive it doesn't have yet, as it would be registered. Nothing is written: a front end shows
/// it before asking.
pub fn drive_to_change(store: &Store, req: &DriveRef) -> Result<DriveRecord> {
    match named(store, &req.drive)? {
        Named::Registered(rec) => Ok(rec),
        Named::Mounted(d) => {
            let id = d.id().expect("adopted");
            Ok(store
                .drives()?
                .shift_remove(&id)
                .unwrap_or_else(|| DriveRecord::new(id, d.name(), d.root().to_path_buf())))
        }
    }
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct RenameDrive {
    pub drive: String,
    pub name: String,
    /// Also change the volume label.
    #[serde(default)]
    pub label: bool,
}

/// What renaming a drive would do.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct RenamePlan {
    pub drive: String,
    pub root: PathBuf,
    pub from: String,
    pub to: String,
    /// The volume label it gets, with `label`.
    pub label: Option<String>,
}

pub fn drive_rename_plan(req: &RenameDrive) -> Result<Planned<RenamePlan, RenameDrive>> {
    let d = resolve_one_drive(&req.drive)?;
    let to = req.name.trim().to_string();
    if to.is_empty() {
        return Err(Error::invalid("the name is empty"));
    }
    let label = req.label.then(|| fleet::volume_label(&to));
    let mut summary = vec![format!("rename {} to {to:?}", d.name())];
    if let Some(l) = &label {
        summary.push(format!("change the volume label of {} to {l:?}", d.root().display()));
    }
    let noop = to == d.name() && label.is_none();
    let plan = RenamePlan { drive: d.name(), root: d.root().into(), from: d.name(), to, label };
    Planned::new("drive.rename", plan, req.clone())
        .summary(summary)
        .warnings(req.label.then(|| format::untested_here("changing the volume label")).flatten())
        .noop(noop)
        .touches(Touched::drive(&d, true, vec![Probe::Identity]))
        .guard(None)
}

impl Operation for RenamePlan {
    type Request = RenameDrive;
    type Report = RenameReport;

    fn apply(planned: &Planned<Self, RenameDrive>, store: Option<&Store>, _: &Ctx<'_>) -> Result<RenameReport> {
        let req = planned.request();
        fleet::rename(store, &mut planned.drive(0)?, &req.name, req.label)
    }
}

/// Plan and rename in one go, for callers that don't confirm (see [`drive_rename_plan`]).
pub fn drive_rename(store: Option<&Store>, req: &RenameDrive) -> Result<RenameReport> {
    drive_rename_plan(req)?.apply(store, &Ctx::silent())
}

/// A registered drive by mount path, name or id prefix: the record of the adopted drive mounted
/// there (or by that name), by its identity, else, when no mounted drive answers to it, the
/// registry's of that name or id. A drive mounted there that the registry doesn't have is never
/// answered with another drive of its name, nor is a name several mounted drives share: those
/// are refused, naming them. Nothing is written.
pub fn drive_find(store: &Store, req: &DriveRef) -> Result<DriveRecord> {
    let mounted = match named(store, &req.drive)? {
        Named::Mounted(d) => d,
        Named::Registered(rec) => return Ok(rec),
    };
    if let Some(rec) = store.drives()?.shift_remove(&mounted.id().expect("adopted")) {
        return Ok(rec);
    }
    let others = fleet::records_named(store, &req.drive)?;
    match others.is_empty() {
        true => Err(Error::NotFound(format!(
            "{} (at {}) in the store's registry",
            mounted.name(),
            mounted.root().display()
        ))),
        false => Err(fleet::ambiguous(&req.drive, &others, Some(&mounted))),
    }
}

pub fn drive_forget(store: &Store, req: &DriveRef) -> Result<DriveRecord> {
    fleet::forget(store, &drive_find(store, req)?.id.to_string())
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct SetRole {
    pub drive: String,
    /// `primary`, `backup`, `set` (or `gig`, its old name) or `archive`; `None` clears it.
    #[serde(default)]
    pub role: Option<String>,
}

pub fn drive_role(store: &Store, req: &SetRole) -> Result<DriveRecord> {
    let role = req.role.as_deref().map(Role::parse).transpose()?;
    let rec = drive_record(store, &req.drive)?;
    fleet::set_role(store, rec.id, role)
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct RetireDrive {
    pub drive: String,
    #[serde(default)]
    pub undo: bool,
}

pub fn drive_retire(store: &Store, req: &RetireDrive) -> Result<DriveRecord> {
    let rec = drive_record(store, &req.drive)?;
    fleet::retire(store, rec.id, req.undo)
}

/// A drive's registry record, logbook included, as the registry has it. Nothing is written, so
/// reading a store copy's with `--store` doesn't count as writing to it (see [`crate::offsite`]).
pub fn drive_log(store: &Store, req: &DriveRef) -> Result<DriveRecord> {
    drive_find(store, req)
}

pub fn drive_recognise(store: &Store, req: &DriveRef, ctx: &Ctx<'_>) -> Result<Recognition> {
    fleet::recognise(store, &resolve_one_drive(&req.drive)?, ctx)
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct AdoptAs {
    pub drive: String,
    /// The registered drive whose identity it takes.
    pub known: String,
}

/// What giving a drive back a known drive's identity would do.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct AdoptAsPlan {
    pub drive: String,
    pub root: PathBuf,
    /// The registered drive whose identity it takes.
    pub known: DriveRecord,
}

pub fn drive_adopt_as_plan(store: &Store, req: &AdoptAs) -> Result<Planned<AdoptAsPlan, AdoptAs>> {
    let d = resolve_one_drive(&req.drive)?;
    let known = fleet::find_record(store, &req.known)?;
    let line = format!("give {} the identity of {} ({})", d.root().display(), known.name, known.id);
    let plan = AdoptAsPlan { drive: d.name(), root: d.root().into(), known };
    Ok(Planned::new("drive.adopt_as", plan, req.clone()).line(line).touches(Touched::drive(
        &d,
        true,
        vec![Probe::Identity, Probe::Databases],
    )))
}

impl Operation for AdoptAsPlan {
    type Request = AdoptAs;
    type Report = DeviceInfo;

    fn apply(planned: &Planned<Self, AdoptAs>, store: Option<&Store>, ctx: &Ctx<'_>) -> Result<DeviceInfo> {
        let store = store.ok_or_else(|| Error::invalid("a known drive's identity is in the store"))?;
        let known = planned.plan.known.id.to_string();
        fleet::adopt_as(store, &mut planned.drive(0)?, &known, &known_drives(false), ctx)
    }
}

/// Plan and give back an identity in one go, for callers that don't confirm.
pub fn drive_adopt_as(store: &Store, req: &AdoptAs, ctx: &Ctx<'_>) -> Result<DeviceInfo> {
    drive_adopt_as_plan(store, req)?.apply(Some(store), ctx)
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct Reidentify {
    pub drive: String,
    #[serde(default)]
    pub name: Option<String>,
    /// Give it a new identity though nothing shows it's a copy (no other drive mounted has its
    /// identity, and it's on the disk recorded with it).
    #[serde(default)]
    pub force: bool,
}

/// What giving a copied drive an identity of its own would do.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct ReidentifyPlan {
    pub drive: String,
    pub root: PathBuf,
    /// The identity it shares with the drive it was copied from, which keeps it.
    pub id: uuid::Uuid,
    /// The name it gets.
    pub name: String,
}

pub fn drive_reidentify_plan(store: Option<&Store>, req: &Reidentify) -> Result<Planned<ReidentifyPlan, Reidentify>> {
    // A name or id is ambiguous between two copies: a path is required.
    if !std::path::Path::new(&req.drive).is_dir() {
        return Err(Error::invalid("give the copy's mount path (both copies answer to the same name)"));
    }
    let d = resolve_one_drive(&req.drive)?;
    let id = d.id().ok_or_else(|| Error::invalid(format!("{} has no identity yet", d.name())))?;
    if !req.force && !fleet::is_copy(&d, &known_drives(false)) {
        let drive = d.root().display().to_string();
        return Err(Error::conflict(
            Reason::HasIdentity,
            format!(
                "nothing shows {} is a copy (no other drive mounted has its identity, and it's on the disk it \
                 was given on); force it to give it a new identity anyway",
                d.name()
            ),
        )
        .with("drive", &drive)
        .action(Action::retry(serde_json::json!({ "force": true }), "Give it a new identity anyway")));
    }
    let name = req.name.clone().unwrap_or_else(|| fleet::copy_name(&d));
    // Roles live on the registry's record of the identity, which stays with the original.
    let kept = store.and_then(|s| s.drives().ok()).and_then(|mut r| r.shift_remove(&id));
    let warnings = kept.and_then(|rec| rec.role.map(|role| (rec.name, role))).map(|(was, role)| {
        format!("its role ({}) stays with {was}'s identity: give the copy one with `drive role`", role.name())
    });
    let line =
        format!("give {} an identity of its own, named {name:?} (it has {}'s, {id})", d.root().display(), d.name());
    let plan = ReidentifyPlan { drive: d.name(), root: d.root().into(), id, name };
    Planned::new("drive.reidentify", plan, req.clone())
        .line(line)
        .warnings(warnings)
        .touches(Touched::drive(&d, true, vec![Probe::Identity]))
        .guard(store)
}

impl Operation for ReidentifyPlan {
    type Request = Reidentify;
    type Report = DeviceInfo;

    fn apply(planned: &Planned<Self, Reidentify>, store: Option<&Store>, _: &Ctx<'_>) -> Result<DeviceInfo> {
        fleet::reidentify(store, &mut planned.drive(0)?, Some(&planned.plan.name))
    }
}

/// Plan and give a new identity in one go, for callers that don't confirm (see
/// [`drive_reidentify_plan`]).
pub fn drive_reidentify(store: Option<&Store>, req: &Reidentify) -> Result<DeviceInfo> {
    drive_reidentify_plan(store, req)?.apply(store, &Ctx::silent())
}

pub fn drive_sets(store: &Store) -> Result<Vec<DriveSet>> {
    fleet::sets(store, &known_drives(false))
}

/// Sync a drive group: every mounted member of a library's group (not archive or retired drives)
/// from the newest of them, which should be the primary.
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct SyncSetRequest {
    /// The library whose group to sync [default: the only group with two or more drives mounted;
    /// with several, refused as `choose_library`, listing them].
    #[serde(default)]
    pub library: Option<String>,
    #[serde(default)]
    pub allow_low_confidence: bool,
    #[serde(default)]
    pub verify: bool,
    #[serde(default)]
    pub keep_extra: bool,
    #[serde(default)]
    pub delete_first: bool,
}

impl SyncSetRequest {
    /// The [`SyncManyRequest`] for the chosen group.
    pub fn resolve(&self, store: &Store) -> Result<SyncManyRequest> {
        let sets: Vec<DriveSet> = drive_sets(store)?.into_iter().filter(|s| s.members.len() >= 2).collect();
        let set = match &self.library {
            Some(name) => {
                let lib = store.find_library(name)?.ok_or_else(|| Error::NotFound(format!("library {name:?}")))?;
                sets.into_iter()
                    .find(|s| s.library_id == Some(lib.id))
                    .ok_or_else(|| Error::NotFound(format!("two or more mounted drives of library {name:?}")))?
            }
            None => match sets.len() {
                0 => {
                    return Err(Error::NotFound(
                        "two or more mounted drives of one library (pull or sync drives to link them to a library)"
                            .into(),
                    ));
                }
                1 => sets.into_iter().next().expect("one"),
                _ => {
                    let libraries: Vec<String> = sets.iter().filter_map(|s| s.library.clone()).collect();
                    return Err(Error::invalid(format!(
                        "drives of {} libraries are mounted; choose one ({})",
                        sets.len(),
                        libraries.join(", ")
                    ))
                    .because(Reason::ChooseLibrary)
                    .with("libraries", &libraries)
                    .action(Action::retry(serde_json::json!({ "library": null }), "Choose the library")));
                }
            },
        };
        // The primary first, so it is the source when drives are identical.
        let mut drives: Vec<String> = set.primary.iter().map(|p| p.display().to_string()).collect();
        drives.extend(set.members.iter().filter(|m| Some(*m) != set.primary.as_ref()).map(|m| m.display().to_string()));
        Ok(SyncManyRequest {
            drives,
            allow_low_confidence: self.allow_low_confidence,
            verify: self.verify,
            keep_extra: self.keep_extra,
            delete_first: self.delete_first,
            overwrite_archive: false,
        })
    }
}

pub fn sync_set_plan(
    store: &Store,
    req: &SyncSetRequest,
    ctx: &Ctx<'_>,
) -> Result<Planned<SyncManyPreview, SyncManyRequest>> {
    super::many_planned("sync.group", &req.resolve(store)?, ctx)
}

/// Plan and apply a drive group's sync in one go, for callers that don't confirm.
pub fn sync_set_apply(store: &Store, req: &SyncSetRequest, ctx: &Ctx<'_>) -> Result<Vec<TargetResult>> {
    sync_set_plan(store, req, ctx)?.apply(Some(store), ctx)
}

// ---------------------------------------------------------------------------------------------
// Eject
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct EjectDrives {
    /// Drives, or a library's name for its mounted clones (but archive and retired ones).
    pub drives: Vec<String>,
}

/// Eject drives, recording each eject in the drive's logbook. Refused as
/// [`super::drive_eject_plan`] says.
pub fn drive_eject(store: Option<&Store>, req: &EjectDrives) -> Result<Vec<EjectReport>> {
    let mut out = Vec::new();
    for spec in &super::expand_libraries(&req.drives)?.0 {
        let (d, _) = super::tui::eject_target(spec, store.map(Store::root))?;
        let id = d.id();
        let report = eject::eject(&d)?;
        if let (Some(store), Some(id)) = (store, id) {
            fleet::record_eject(store, id)?;
        }
        out.push(report);
    }
    Ok(out)
}

// ---------------------------------------------------------------------------------------------
// Health
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct VerifyRequest {
    pub drive: String,
    /// Compare with this snapshot (id prefix) [default: the snapshot of the drive's state].
    #[serde(default)]
    pub snapshot: Option<String>,
    /// Other mounted drives that may hold good copies.
    #[serde(default)]
    pub against: Vec<String>,
}

fn siblings(specs: &[String], exclude: &Device) -> Result<Vec<Device>> {
    let mut out = Vec::new();
    for s in specs {
        let d = resolve_one_drive(s)?;
        if d.root() != exclude.root() {
            out.push(d);
        }
    }
    Ok(out)
}

/// Re-read every library file on a drive and compare it with a known-good copy.
pub fn drive_verify(store: Option<&Store>, req: &VerifyRequest, ctx: &Ctx<'_>) -> Result<ScrubReport> {
    let d = resolve_one_drive(&req.drive)?;
    let against = siblings(&req.against, &d)?;
    let snapshot = match (&req.snapshot, store) {
        (Some(prefix), Some(store)) => Some(store.resolve_snapshot(prefix)?),
        (Some(_), None) => return Err(Error::invalid("comparing with a snapshot needs the store")),
        _ => None,
    };
    let report = scrub::scrub(&d, store, &against, &ScrubOptions { snapshot }, ctx)?;
    if let (Some(store), Some(id)) = (store, d.id()) {
        fleet::record_check(store, id, Check::Scrub, report.is_clean(), report.summary())?;
        for (kind, n) in [
            ("damaged_files", report.damaged.len()),
            ("unreadable_files", report.unreadable.len()),
            ("missing_files", report.missing.len()),
        ] {
            if n > 0 {
                fleet::record_problem(store, id, kind, format!("{n} file(s)"))?;
            }
        }
    }
    Ok(report)
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct RepairRequest {
    pub drive: String,
    /// The problems a [`drive_verify`] found (each is checked again before anything is written).
    pub problems: Vec<FileProblem>,
    #[serde(default)]
    pub against: Vec<String>,
}

pub fn drive_repair(store: Option<&Store>, req: &RepairRequest, ctx: &Ctx<'_>) -> Result<RepairReport> {
    let mut d = resolve_one_drive(&req.drive)?;
    let against = siblings(&req.against, &d)?;
    let report = scrub::repair(&mut d, store, &against, &req.problems, ctx)?;
    if let (Some(store), Some(id)) = (store, d.id()) {
        fleet::record_write(store, &d, report.bytes)?;
        if !report.failed.is_empty() {
            fleet::record_problem(
                store,
                id,
                "repair_failed",
                format!("{} file(s) still wrong after being rewritten", report.failed.len()),
            )?;
        }
    }
    Ok(report)
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct FsCheckRequest {
    pub drive: String,
    #[serde(default)]
    pub repair: bool,
}

pub fn drive_fs_check(store: Option<&Store>, req: &FsCheckRequest) -> Result<FsCheckReport> {
    let d = resolve_one_drive(&req.drive)?;
    let id = d.id();
    let report = fscheck::check(&d, req.repair)?;
    if let (Some(store), Some(id)) = (store, id) {
        fleet::record_check(store, id, Check::FileSystem, report.ok, report.summary())?;
        if !report.ok {
            fleet::record_problem(store, id, "fs_errors", report.summary())?;
        }
    }
    Ok(report)
}

// ---------------------------------------------------------------------------------------------
// Space
// ---------------------------------------------------------------------------------------------

/// What's using a drive, its growth and whether its library fits on the rest of its group.
pub fn drive_space(store: Option<&Store>, req: &DriveRef, ctx: &Ctx<'_>) -> Result<SpaceReport> {
    let d = resolve_one_drive(&req.drive)?;
    let mut report = space::breakdown(&d, ctx)?;
    if let Some(store) = store {
        report.forecast = space::forecast(store, &d, report.free)?;
        let library_bytes = report.bytes(space::SpaceKind::Audio)
            + report.bytes(space::SpaceKind::Analysis)
            + report.bytes(space::SpaceKind::Artwork)
            + report.bytes(space::SpaceKind::Databases)
            + report.bytes(space::SpaceKind::RekordboxOther);
        report.fits = space::fits_on_set(store, &d, library_bytes, &known_drives(false))?;
    }
    Ok(report)
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct TidyRequest {
    pub drive: String,
    #[serde(default, flatten)]
    pub options: TidyOptions,
}

/// The OS clutter (and with `empty_trash`, the trash) a tidy would remove.
pub fn tidy_plan(req: &TidyRequest, ctx: &Ctx<'_>) -> Result<Planned<TidyPlan>> {
    let d = resolve_one_drive(&req.drive)?;
    let plan = tidy::plan(&d, &req.options, ctx)?;
    let line = format!("remove {} item(s) of clutter ({}) from {}", plan.items.len(), bytes(plan.bytes), d.name());
    let keys = plan.items.iter().map(|i| i.path.clone()).collect();
    let noop = plan.is_noop();
    Planned::new("tidy", plan, ())
        .line(line)
        .destructive(!noop)
        .noop(noop)
        .touches(Touched::drive(&d, true, vec![Probe::Identity, Probe::Stamp(keys)]))
        .guard(None)
}

impl Operation for TidyPlan {
    type Request = ();
    type Report = TidyReport;

    fn apply(planned: &Planned<Self>, _: Option<&Store>, ctx: &Ctx<'_>) -> Result<TidyReport> {
        tidy::apply(&planned.plan, &planned.drive(0)?, ctx)
    }
}

/// Plan and tidy in one go, for callers that don't confirm.
pub fn tidy_apply(req: &TidyRequest, ctx: &Ctx<'_>) -> Result<TidyReport> {
    tidy_plan(req, ctx)?.apply(None, ctx)
}

pub fn recordings_list(req: &DriveRef) -> Result<Vec<Recording>> {
    recordings::list(&resolve_one_drive(&req.drive)?)
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct CollectRecordings {
    pub drive: String,
    #[serde(flatten)]
    pub options: CollectOptions,
}

/// What collecting recordings would copy (and, with `delete`, delete from the drive).
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct CollectPlan {
    pub drive: String,
    pub root: PathBuf,
    pub to: PathBuf,
    pub recordings: Vec<Recording>,
    pub bytes: u64,
    /// Each is deleted from the drive once its copy is verified.
    pub delete: bool,
}

pub fn recordings_collect_plan(req: &CollectRecordings) -> Result<Planned<CollectPlan, CollectOptions>> {
    let d = resolve_one_drive(&req.drive)?;
    let all = recordings::list(&d)?;
    let chosen: Vec<Recording> = match req.options.only.is_empty() {
        true => all,
        false => {
            let mut v = Vec::new();
            for want in &req.options.only {
                v.push(
                    all.iter()
                        .find(|r| r.path.eq_ignore_ascii_case(want.trim_start_matches('/')))
                        .cloned()
                        .ok_or_else(|| Error::NotFound(format!("recording {want:?}")))?,
                );
            }
            v
        }
    };
    let bytes_total = chosen.iter().map(|r| r.size).sum();
    let mut summary = vec![format!(
        "copy {} recording(s) ({}) from {} into {}",
        chosen.len(),
        bytes(bytes_total),
        d.name(),
        req.options.to.display()
    )];
    if req.options.delete {
        summary.push("then delete each from the drive once its copy is verified".into());
    }
    let keys = chosen.iter().map(|r| crate::paths::key(&r.path)).collect();
    let noop = chosen.is_empty();
    let plan = CollectPlan {
        drive: d.name(),
        root: d.root().into(),
        to: req.options.to.clone(),
        recordings: chosen,
        bytes: bytes_total,
        delete: req.options.delete,
    };
    Planned::new("recordings.collect", plan, req.options.clone())
        .summary(summary)
        .destructive(req.options.delete)
        .noop(noop)
        .touches(Touched::drive(&d, req.options.delete, vec![Probe::Stamp(keys)]))
        .guard(None)
}

impl Operation for CollectPlan {
    type Request = CollectOptions;
    type Report = CollectReport;

    fn apply(planned: &Planned<Self, CollectOptions>, _: Option<&Store>, ctx: &Ctx<'_>) -> Result<CollectReport> {
        let only = planned.plan.recordings.iter().map(|r| r.path.clone()).collect();
        let opts = CollectOptions { only, ..planned.request().clone() };
        recordings::collect(&mut planned.drive(0)?, &opts, ctx)
    }
}

/// Plan and collect in one go, for callers that don't confirm (see [`recordings_collect_plan`]).
pub fn recordings_collect(req: &CollectRecordings, ctx: &Ctx<'_>) -> Result<CollectReport> {
    recordings_collect_plan(req)?.apply(None, ctx)
}

// ---------------------------------------------------------------------------------------------
// Firmware
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct FirmwareRequest {
    pub drive: String,
    pub file: PathBuf,
    /// Remove other update files from the drive's root.
    #[serde(default)]
    pub replace: bool,
    /// Copy it even if a check fails.
    #[serde(default)]
    pub force: bool,
}

/// Whether an update file can go on a drive, and what copying it would replace.
pub fn firmware_plan(req: &FirmwareRequest) -> Result<Planned<FirmwarePlan, bool>> {
    let d = resolve_one_drive(&req.drive)?;
    let plan = firmware::plan(&d, &req.file, req.replace)?;
    let mut summary = vec![format!("copy {} ({}) to the root of {}", plan.file_name, bytes(plan.size), d.name())];
    if !plan.existing.is_empty() {
        summary.push(format!("remove {}", plan.existing.join(", ")));
    }
    let warnings = plan.problems.clone();
    let destructive = !plan.existing.is_empty();
    Planned::new("firmware", plan, req.force)
        .summary(summary)
        .warnings(warnings)
        .destructive(destructive)
        .touches(Touched::drive(&d, true, vec![Probe::Identity, Probe::Listing(String::new())]))
        .touches(Touched::path(&req.file, false, vec![Probe::Stamp(vec![String::new()])]))
        .guard(None)
}

impl Operation for FirmwarePlan {
    /// Copy it even if a check fails.
    type Request = bool;
    type Report = FirmwareReport;

    fn apply(planned: &Planned<Self, bool>, _: Option<&Store>, ctx: &Ctx<'_>) -> Result<FirmwareReport> {
        firmware::apply(&planned.plan, &mut planned.drive(0)?, *planned.request(), ctx)
    }
}

/// Plan and copy an update file in one go, for callers that don't confirm.
pub fn firmware_apply(req: &FirmwareRequest, ctx: &Ctx<'_>) -> Result<FirmwareReport> {
    firmware_plan(req)?.apply(None, ctx)
}

/// The update files removing them would delete from a drive's root.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct FirmwareRemovePlan {
    pub drive: String,
    pub root: PathBuf,
    pub files: Vec<String>,
    pub bytes: u64,
}

pub fn firmware_remove_plan(req: &DriveRef) -> Result<Planned<FirmwareRemovePlan>> {
    let d = resolve_one_drive(&req.drive)?;
    let files = firmware::existing_updates(d.root());
    let total = files.iter().map(|f| std::fs::metadata(d.root().join(f)).map(|m| m.len()).unwrap_or(0)).sum();
    let line =
        format!("delete {} from {}", if files.is_empty() { "nothing".into() } else { files.join(", ") }, d.name());
    let noop = files.is_empty();
    let plan = FirmwareRemovePlan { drive: d.name(), root: d.root().into(), files, bytes: total };
    Planned::new("firmware.remove", plan, ())
        .line(line)
        .destructive(!noop)
        .noop(noop)
        .touches(Touched::drive(&d, true, vec![Probe::Listing(String::new())]))
        .guard(None)
}

impl Operation for FirmwareRemovePlan {
    type Request = ();
    type Report = Vec<String>;

    fn apply(planned: &Planned<Self>, _: Option<&Store>, ctx: &Ctx<'_>) -> Result<Vec<String>> {
        firmware::remove(&mut planned.drive(0)?, ctx)
    }
}

/// Plan and remove the update files in one go, for callers that don't confirm.
pub fn firmware_remove(req: &DriveRef, ctx: &Ctx<'_>) -> Result<Vec<String>> {
    firmware_remove_plan(req)?.apply(None, ctx)
}

// ---------------------------------------------------------------------------------------------
// What the players show
// ---------------------------------------------------------------------------------------------

pub fn player_show(req: &DriveRef) -> Result<PlayerIdentity> {
    Ok(player::read(&resolve_one_drive(&req.drive)?))
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct SetPlayerIdentity {
    pub drive: String,
    #[serde(default)]
    pub name: Option<String>,
    /// rekordbox's background colour code, 0 (none) to 8.
    #[serde(default)]
    pub colour: Option<i64>,
}

/// What setting the name and colour players show would change.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct PlayerPlan {
    pub drive: String,
    pub root: PathBuf,
    /// What players show now.
    pub now: PlayerIdentity,
    pub name: Option<String>,
    pub colour: Option<i64>,
}

pub fn player_set_plan(req: &SetPlayerIdentity) -> Result<Planned<PlayerPlan>> {
    let d = resolve_one_drive(&req.drive)?;
    player::check(&d, req.name.as_deref(), req.colour)?;
    let now = player::read(&d);
    let mut summary = Vec::new();
    if let Some(n) = &req.name {
        summary.push(format!("players show {} as {:?}", d.name(), n.trim()));
    }
    if let Some(c) = req.colour {
        summary.push(format!("in colour {c}"));
    }
    let plan = PlayerPlan { drive: d.name(), root: d.root().into(), now, name: req.name.clone(), colour: req.colour };
    Planned::new("player.set", plan, ()).summary(summary).touches(Touched::library(&d, true)).guard(None)
}

impl Operation for PlayerPlan {
    type Request = ();
    type Report = PlayerIdentity;

    fn apply(planned: &Planned<Self>, _: Option<&Store>, _: &Ctx<'_>) -> Result<PlayerIdentity> {
        player::set(&mut planned.drive(0)?, planned.plan.name.as_deref(), planned.plan.colour)
    }
}

/// Plan and set the player name in one go, for callers that don't confirm.
pub fn player_set(req: &SetPlayerIdentity) -> Result<PlayerIdentity> {
    player_set_plan(req)?.apply(None, &Ctx::silent())
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct SetContact {
    pub drive: String,
    pub contact: String,
    /// Also add a top-level playlist named after it (shown first on the player).
    #[serde(default)]
    pub playlist: bool,
}

/// What putting a contact on a drive would write.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct ContactPlan {
    pub drive: String,
    pub root: PathBuf,
    pub file: PathBuf,
    /// The drive has one already, which is replaced.
    pub replaces: bool,
    /// The playlist made, with `playlist`.
    pub playlist: Option<String>,
}

pub fn drive_contact_plan(req: &SetContact) -> Result<Planned<ContactPlan, SetContact>> {
    let d = resolve_one_drive(&req.drive)?;
    if req.contact.trim().is_empty() || req.contact.chars().any(char::is_control) {
        return Err(Error::invalid("give an email address, phone number or name to contact"));
    }
    let file = d.root().join(player::CONTACT_FILE);
    let replaces = file.is_file();
    let playlist = (req.playlist && d.has_export()).then(|| player::contact_playlist(&req.contact));
    let mut summary = vec![format!("{} {}", if replaces { "replace" } else { "write" }, file.display())];
    if let Some(p) = &playlist {
        summary.push(format!("add the playlist {p:?}"));
    }
    let plan = ContactPlan { drive: d.name(), root: d.root().into(), file, replaces, playlist };
    Planned::new("drive.contact", plan, req.clone())
        .summary(summary)
        .touches(Touched::drive(&d, true, vec![Probe::Databases, Probe::Content(vec![player::CONTACT_FILE.into()])]))
        .guard(None)
}

impl Operation for ContactPlan {
    type Request = SetContact;
    type Report = ContactReport;

    fn apply(planned: &Planned<Self, SetContact>, _: Option<&Store>, ctx: &Ctx<'_>) -> Result<ContactReport> {
        let req = planned.request();
        player::set_contact(&mut planned.drive(0)?, &req.contact, req.playlist, ctx)
    }
}

/// Plan and write the contact in one go, for callers that don't confirm.
pub fn drive_contact(req: &SetContact, ctx: &Ctx<'_>) -> Result<ContactReport> {
    drive_contact_plan(req)?.apply(None, ctx)
}

// ---------------------------------------------------------------------------------------------
// Setting up a new drive
// ---------------------------------------------------------------------------------------------

fn yes() -> bool {
    true
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct ProvisionRequest {
    /// The new drive: a mounted volume, or (with `format`) a disk such as `disk4`.
    pub target: String,
    /// A drive to copy the library from [default: push the library's latest snapshot].
    #[serde(default)]
    pub like: Option<String>,
    /// The library to push (without `like`) [default: the only library].
    #[serde(default)]
    pub library: Option<String>,
    /// locallibrary name for the new drive.
    #[serde(default)]
    pub name: Option<String>,
    /// Erase and format it first.
    #[serde(default)]
    pub format: bool,
    #[serde(default, flatten)]
    pub format_options: FormatOptions,
    #[serde(default)]
    pub role: Option<String>,
    /// Device Name and colour players show.
    #[serde(default)]
    pub player_name: Option<String>,
    #[serde(default)]
    pub player_colour: Option<i64>,
    /// Write the settings profile [default: yes].
    #[serde(default = "yes")]
    pub settings: bool,
    /// Read everything back afterwards [default: yes].
    #[serde(default = "yes")]
    pub verify: bool,
    #[serde(default)]
    pub eject: bool,
}

/// [`provision_apply`] with a request (rather than a plan id).
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct ProvisionApply {
    #[serde(flatten)]
    pub request: ProvisionRequest,
    /// The plan's `format.identity`, when formatting: the disk the user confirmed.
    #[serde(default)]
    pub format_identity: Option<String>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct ProvisionPlan {
    pub steps: Vec<String>,
    /// The format's plan, when formatting (its `identity` confirms the disk on apply).
    pub format: Option<FormatPlan>,
    pub warnings: Vec<String>,
}

pub fn provision_plan(
    store: &Store,
    req: &ProvisionRequest,
    ctx: &Ctx<'_>,
) -> Result<Planned<ProvisionPlan, ProvisionRequest>> {
    let role = req.role.as_deref().map(Role::parse).transpose()?;
    let mut steps = Vec::new();
    let mut warnings = Vec::new();
    let mut touched = None;
    let format = if req.format {
        let p = super::format_plan(
            store.root(),
            &FormatRequest { drive: req.target.clone(), options: req.format_options.clone() },
            ctx,
        )?;
        steps.extend(p.steps.iter().cloned());
        warnings.extend(p.warnings.iter().cloned());
        Some(p.plan)
    } else {
        let d = resolve_one_drive(&req.target)?;
        if d.has_export() {
            warnings.push(format!("{} already holds a rekordbox library; it will be replaced", d.name()));
        }
        touched = Some(Touched::library(&d, true));
        None
    };
    // Without a format (whose plan shows the disk), the steps say which drive they're for.
    let it = touched.as_ref().map_or("it".into(), |t| format!("the drive at {}", t.root.display()));
    // Without a name, it keeps its volume's, as every drive does.
    steps.push(match &req.name {
        Some(name) => format!("give {it} the name {name:?}"),
        None => format!("give {it} an identity"),
    });
    match &req.like {
        Some(spec) => {
            let src = resolve_drive(spec)?;
            let library = src.state().and_then(|s| s.library_id).map(|id| store.library_by_id(id)).transpose()?;
            steps.push(match library.flatten() {
                Some(l) => format!("make it another copy of {:?}, from {}", l.name, src.name()),
                None => format!("copy {}'s library onto it", src.name()),
            });
        }
        None => {
            let lib = match &req.library {
                Some(n) => store.find_library(n)?.ok_or_else(|| Error::NotFound(format!("library {n:?}")))?,
                None => match store.libraries()?.as_slice() {
                    [one] => one.clone(),
                    [] => {
                        return Err(Error::NotFound(
                            "the store has no libraries yet; pull a drive first, or copy from one with `like`".into(),
                        ));
                    }
                    _ => {
                        return Err(Error::invalid(
                            "the store holds several libraries; choose one, or copy from a drive with `like`",
                        ));
                    }
                },
            };
            let head = lib.head.ok_or_else(|| Error::NotFound(format!("library {:?} has no snapshots", lib.name)))?;
            let snap = store.load_snapshot(&head)?.info();
            steps.push(format!(
                "write {:?}'s latest snapshot ({}, {} tracks) onto it",
                lib.name, snap.short_id, snap.summary.tracks
            ));
        }
    }
    let profile = store.settings_profile()?;
    if req.settings && !(profile.base.is_empty() && profile.drives.is_empty()) {
        steps.push("write the My Settings profile".into());
    }
    if let Some(r) = role {
        let article = if r == Role::Archive { "an" } else { "a" };
        steps.push(format!("mark it as {article} {} drive", r.name()));
    }
    if req.player_name.is_some() || req.player_colour.is_some() {
        steps.push("set the name and colour players show".into());
    }
    if req.verify {
        steps.push("read every file back and check it".into());
    }
    // A format or a library it replaces (the only warning so far) is destructive; the platform's
    // warning below isn't.
    let destructive = req.format || !warnings.is_empty();
    if req.eject {
        steps.push("eject it".into());
        warnings.extend(format::untested_here("ejecting"));
    }
    let plan = ProvisionPlan { steps: steps.clone(), format, warnings: warnings.clone() };
    let mut planned =
        Planned::new("provision", plan, req.clone()).summary(steps).warnings(warnings).destructive(destructive);
    if let Some(t) = touched {
        planned = planned.touches(t);
    }
    planned.guard(Some(store))
}

impl Operation for ProvisionPlan {
    type Request = ProvisionRequest;
    type Report = ProvisionReport;

    fn apply(
        planned: &Planned<Self, ProvisionRequest>,
        store: Option<&Store>,
        ctx: &Ctx<'_>,
    ) -> Result<ProvisionReport> {
        let store = store.ok_or_else(|| Error::invalid("setting up a drive needs the store"))?;
        let identity = planned.plan.format.as_ref().map(|f| f.identity.as_str());
        provision_run(store, planned.request(), identity, ctx)
    }
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct StepError {
    pub step: String,
    pub kind: &'static str,
    pub message: String,
}

#[derive(Debug, Clone, Default, Serialize, JsonSchema)]
pub struct ProvisionReport {
    pub format: Option<FormatReport>,
    pub drive: Option<DeviceInfo>,
    pub copy: Option<SyncReport>,
    pub settings: Option<ApplyReport>,
    pub player: Option<PlayerIdentity>,
    pub verify: Option<ScrubReport>,
    pub eject: Option<EjectReport>,
    /// Where it stopped, when a step failed (the steps before it stay done).
    pub stopped: Option<StepError>,
}

/// Plan a provision and apply it in one go, refused as [`provision_plan`] is (an archive or
/// retired target, before anything is written). When formatting, `format_identity` is the
/// confirmed plan's `format.identity`, and the disk must still be that one.
pub fn provision_apply(
    store: &Store,
    req: &ProvisionRequest,
    format_identity: Option<&str>,
    ctx: &Ctx<'_>,
) -> Result<ProvisionReport> {
    let planned = provision_plan(store, req, ctx)?;
    if let Some(f) = &planned.plan.format
        && format_identity != Some(f.identity.as_str())
    {
        return Err(match format_identity {
            None => Error::invalid("formatting needs the plan's format.identity: the disk that was confirmed"),
            Some(_) => crate::format::not_the_planned_disk(&f.disk),
        });
    }
    planned.apply(Some(store), ctx)
}

/// Run the steps of a checked [`provision_plan`]. `format_identity` is the plan's
/// `format.identity`.
fn provision_run(
    store: &Store,
    req: &ProvisionRequest,
    format_identity: Option<&str>,
    ctx: &Ctx<'_>,
) -> Result<ProvisionReport> {
    let mut report = ProvisionReport::default();
    let result = provision_steps(store, req, format_identity, &mut report, ctx);
    if let Err((step, e)) = result {
        if matches!(e, Error::Cancelled) {
            return Err(e);
        }
        report.stopped = Some(StepError { step: step.to_string(), kind: e.code(), message: e.to_string() });
    }
    Ok(report)
}

fn provision_steps(
    store: &Store,
    req: &ProvisionRequest,
    format_identity: Option<&str>,
    report: &mut ProvisionReport,
    ctx: &Ctx<'_>,
) -> std::result::Result<(), (&'static str, Error)> {
    let role = req.role.as_deref().map(Role::parse).transpose().map_err(|e| ("check the role", e))?;
    let profile = store.settings_profile();
    // The steps this request runs, numbered for progress events.
    let ids = [
        ("format", req.format),
        ("name", true),
        ("library", true),
        ("settings", req.settings && !profile.as_ref().is_ok_and(|p| p.base.is_empty() && p.drives.is_empty())),
        ("role", role.is_some()),
        ("player", req.player_name.is_some() || req.player_colour.is_some()),
        ("verify", req.verify),
        ("eject", req.eject),
    ];
    let steps = Steps::new(ctx, ids.into_iter().filter_map(|(id, runs)| runs.then_some(id)).collect());
    let ctx = &steps.ctx();

    // 1. Format.
    let root: PathBuf = if req.format {
        steps.begin("format");
        let identity = format_identity.ok_or(("format", Error::invalid("formatting needs the plan's identity")))?;
        let apply = FormatApply {
            drive: req.target.clone(),
            identity: identity.to_string(),
            options: req.format_options.clone(),
        };
        let r = super::format_apply(store.root(), &apply, ctx).map_err(|e| ("format", e))?;
        let mount = r.mount.clone();
        report.format = Some(r);
        mount.ok_or((
            "format",
            Error::invalid(
                "the new volume wasn't mounted: reconnect the drive and provision it again without formatting",
            ),
        ))?
    } else {
        resolve_one_drive(&req.target).map_err(|e| ("find the drive", e))?.root().to_path_buf()
    };
    let target = root.display().to_string();

    // 2. Identity.
    steps.begin("name");
    let mut d = Device::open(&root).map_err(|e| ("name it", e))?;
    d.adopt(req.name.as_deref()).map_err(|e| ("name it", e))?;
    fleet::note_hardware(&mut d).map_err(|e| ("name it", e))?;
    crate::sync::register_drive(store, &d, None).map_err(|e| ("name it", e))?;

    // 3. The library.
    steps.begin("library");
    let copy = match &req.like {
        Some(source) => {
            let r = super::sync_apply(
                Some(store),
                &SyncRequest {
                    left: source.clone(),
                    right: target.clone(),
                    direction: Direction::LeftToRight,
                    allow_low_confidence: false,
                    verify: false,
                    keep_extra: false,
                    delete_first: false,
                    overwrite_archive: false,
                    keep_player_changes: false,
                },
                ctx,
            )
            .map_err(|e| ("copy the library", e))?;
            r.report
        }
        None => Some(
            super::push_apply(
                store,
                &PushRequest {
                    drive: target.clone(),
                    library: req.library.clone(),
                    snapshot: None,
                    force: false,
                    verify: false,
                    keep_extra: false,
                    delete_first: false,
                },
                ctx,
            )
            .map_err(|e| ("write the library", e))?,
        ),
    };
    report.copy = copy;

    // 4. Settings, role, player name.
    let profile = profile.map_err(|e| ("write settings", e))?;
    if req.settings && !(profile.base.is_empty() && profile.drives.is_empty()) {
        steps.begin("settings");
        let r = super::settings_apply(
            store,
            &ApplySettings { drive: target.clone(), dry_run: false, create_missing: true },
        )
        .map_err(|e| ("write settings", e))?;
        report.settings = Some(r);
    }
    if role.is_some() {
        steps.begin("role");
    }
    let d = Device::open(&root).map_err(|e| ("set the role", e))?;
    if let (Some(role), Some(id)) = (role, d.id()) {
        fleet::set_role(store, id, Some(role)).map_err(|e| ("set the role", e))?;
    }
    if req.player_name.is_some() || req.player_colour.is_some() {
        steps.begin("player");
        let r = player_set(&SetPlayerIdentity {
            drive: target.clone(),
            name: req.player_name.clone(),
            colour: req.player_colour,
        })
        .map_err(|e| ("set the player name", e))?;
        report.player = Some(r);
    }

    // 5. Check, hand over.
    if req.verify {
        steps.begin("verify");
        let r = drive_verify(
            Some(store),
            &VerifyRequest { drive: target.clone(), snapshot: None, against: Vec::new() },
            ctx,
        )
        .map_err(|e| ("check it", e))?;
        let clean = r.is_clean();
        report.verify = Some(r);
        if !clean {
            return Err((
                "check it",
                Error::conflict(Reason::VerifyFailed, "some files didn't read back correctly; see the check"),
            ));
        }
    }
    report.drive = Some(Device::open(&root).map_err(|e| ("check it", e))?.info());
    if req.eject {
        steps.begin("eject");
        let mut r = drive_eject(Some(store), &EjectDrives { drives: vec![target] }).map_err(|e| ("eject it", e))?;
        report.eject = r.pop();
    }
    Ok(())
}
