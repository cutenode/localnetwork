//! Service layer: every user-facing operation as `request struct -> response struct`.
//!
//! Requests derive `Deserialize` and responses `Serialize`, so the same functions back the CLI,
//! the `locallibrary rpc` JSON-RPC sidecar (for an Electron or other GUI front end), and any future
//! native bindings. Operations that write come as `*_plan` / `*_apply` pairs. A `*_plan` returns a
//! [`Planned`] (see [`crate::op`]): show it, get confirmation, then [`Planned::apply`] it, which
//! makes exactly that change or refuses with `stale_plan` if what it touches changed since. The
//! request-based `*_apply(req)` functions plan and apply in one go, for callers that don't ask
//! anyone first.
//!
//! Drives can be referred to by mount path, adopted name, or id prefix ([`resolve_drive`]).

use rustc_hash::FxHashSet;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use base64::prelude::*;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::artwork::{self, Artwork};
use crate::backup::{self, BackupInfo};
use crate::clean;
use crate::compare::{self, Comparison, Relation, Side};
use crate::convert;
use crate::device::{self, Device, DeviceInfo};
use crate::doctor::Databases;
use crate::edit::{self, Edit, EditOptions, EditPlan, EditReport};
use crate::error::{Action, Reason};
use crate::format::{self, DiskView, FormatOptions, FormatPlan, FormatReport};
use crate::hash::Hash;
use crate::merge::{self, MergeOptions};
use crate::model::{Library, LibrarySummary, TrackRef};
use crate::op::{self, Operation, Planned, Touched, bytes};
use crate::progress::Ctx;
use crate::schema::{Failure, Skip};
pub use crate::settings::BaseChange;
use crate::settings::{self, DriveSettings, FlatSettings, SettingsProfile};
use crate::store::{LibraryRecord, SnapshotInfo, Store, StoreStats};
use crate::sync::{self, PullOptions, PullReport, PushOptions, SyncOptions, SyncPlan, SyncReport};
use crate::xml::{self, XmlOptions, XmlReport};
use crate::{Error, Result, fsutil};

// ---------------------------------------------------------------------------------------------
// Drives
// ---------------------------------------------------------------------------------------------

static STORE_ROOT: OnceLock<PathBuf> = OnceLock::new();

thread_local! {
    /// [`set_store_root`] for one thread: tests sharing a process each name their own store
    /// (`testing::store_root_on_this_thread`).
    pub(crate) static THREAD_STORE_ROOT: std::cell::RefCell<Option<PathBuf>> = const { std::cell::RefCell::new(None) };
}

/// The store drive lookups consult: this thread's, else the one [`set_store_root`] named.
fn default_store_root() -> Option<PathBuf> {
    THREAD_STORE_ROOT.with(|r| r.borrow().clone()).or_else(|| STORE_ROOT.get().cloned())
}

/// Tell drive resolution which store's registry to consult (drives mounted outside the usual
/// places, e.g. `/mnt/usb`, are found by their last known mount point). Set once, by the CLI: a
/// default for lookups that aren't given a store. Keeping the store safe never depends on it:
/// [`format_plan`], [`list_disks`] and [`drive_eject_plan`] are given the store's folder,
/// [`provision_plan`] and [`drive_eject`] the store.
pub fn set_store_root(root: PathBuf) {
    let _ = STORE_ROOT.set(root);
}

/// The store's folder [`set_store_root`] named (this thread's, in tests), if it was named.
pub fn store_root() -> Option<PathBuf> {
    default_store_root()
}

/// The store whose registry drive lookups consult, if it exists.
pub(crate) fn registry() -> Option<Store> {
    default_store_root().and_then(|r| Store::open(r).ok())
}

/// Mounted drives, plus registered drives still present at their last mount point (while
/// [`device::volume_folders`] names folders, only those last seen inside one of them).
pub(crate) fn known_drives(include_blank: bool) -> Vec<Device> {
    let mut out = device::detect(include_blank);
    let folders: Option<Vec<PathBuf>> =
        device::volume_folders().map(|f| f.iter().map(|d| device::dunce_canonicalize(d)).collect());
    let registry = registry().and_then(|s| s.drives().ok()).unwrap_or_default();
    for rec in registry.into_values() {
        if out.iter().any(|d| d.id() == Some(rec.id))
            || folders.as_ref().is_some_and(|f| !f.iter().any(|d| rec.last_mount.starts_with(d)))
        {
            continue;
        }
        if let Ok(d) = Device::open(&rec.last_mount)
            && d.id() == Some(rec.id)
        {
            out.push(d);
        }
    }
    out
}

/// [`known_drives`] with a rekordbox library.
fn mounted_exports() -> Vec<Device> {
    known_drives(false).into_iter().filter(Device::has_export).collect()
}

/// Find what a command acting on a library means by `spec`: a drive by mount path, adopted name
/// (case-insensitive), volume name or id prefix, or else a library's name, meaning its lead clone
/// (see [`jobs::lead_clone`]). A bare word (no `/`, not starting with `.` or `~`) is a drive's
/// name first: a folder of that name in the working directory is taken only when no drive or
/// library has the name, and is refused as ambiguous when a drive does (`sync Main Backup` run
/// where a `Backup/` folder is must not write there).
pub fn resolve_drive(spec: &str) -> Result<Device> {
    resolve(spec, true, true)
}

/// [`resolve_drive`] for a name from somewhere less trusted (a plugin): a drive known here (by
/// adopted name, volume name or id prefix) or a library's name, never a path or a folder.
pub fn resolve_known_drive(spec: &str) -> Result<Device> {
    found(find(spec, true, true, false)?)
}

/// [`resolve_drive`] for commands about one particular drive rather than its library (its role,
/// name, file system, backups, player history): a library's name isn't taken.
pub fn resolve_one_drive(spec: &str) -> Result<Device> {
    resolve(spec, true, false)
}

/// The mounted clones `spec` means when it names a library rather than a drive (as
/// [`resolve_drive`] takes it): the library's drive group (see [`jobs::drive_group`]). `None`
/// when it names a drive.
pub fn library_clones(spec: &str) -> Result<Option<DriveGroup>> {
    Ok(match find(spec, true, true, true)? {
        Found::Drive(_) => None,
        Found::Library(_, group) => Some(group),
    })
}

/// The drives a command that takes several (`sync`, `check`, `eject`, `prep`, `settings apply`) means by
/// `specs`: a library's name as its mounted clones (see [`library_clones`]), and a clone named
/// besides its library once. A drive named twice is left for the command to refuse. With them,
/// the libraries' mounted drives left out: archive and retired ones.
pub fn expand_libraries(specs: &[String]) -> Result<(Vec<String>, Vec<String>)> {
    let (mut drives, mut left_out) = (Vec::new(), Vec::new());
    let (mut named, mut cloned): (Vec<PathBuf>, Vec<PathBuf>) = (Vec::new(), Vec::new());
    for spec in specs {
        match find(spec, true, true, true)? {
            Found::Drive(d) if cloned.iter().any(|c| c == d.root()) => {}
            Found::Drive(d) => {
                named.push(d.root().to_path_buf());
                drives.push(spec.clone());
            }
            Found::Library(_, group) => {
                for root in group.drives {
                    if !named.contains(&root) && !cloned.contains(&root) {
                        drives.push(root.display().to_string());
                        cloned.push(root);
                    }
                }
                left_out.extend(group.excluded);
            }
        }
    }
    Ok((drives, left_out))
}

/// [`resolve_drive`]. Without `labels`, a name must be an adopted drive's, and a folder here is
/// taken only as a path (`./Backup`): for erasing, where the label of an unrelated volume could
/// match while the drive meant isn't plugged in. Without `libraries`, a library's name isn't taken.
fn resolve(spec: &str, labels: bool, libraries: bool) -> Result<Device> {
    found(find(spec, labels, libraries, true)?)
}

/// The drive what was found means: a library's lead clone.
fn found(found: Found) -> Result<Device> {
    match found {
        Found::Drive(d) => Ok(*d),
        Found::Library(store, group) => jobs::lead_clone(Some(&store), &group, &Ctx::silent()),
    }
}

/// What a spec names (see [`resolve`]).
enum Found {
    Drive(Box<Device>),
    /// A library, with the store that has it and its mounted clones.
    Library(Store, DriveGroup),
}

/// What `spec` names, as [`resolve`] takes it. Without `folders`, neither a path nor a folder
/// here is taken.
fn find(spec: &str, labels: bool, libraries: bool, folders: bool) -> Result<Found> {
    let p = Path::new(spec);
    // A folder inside an export (e.g. `/Volumes/USB/Contents`) means the export's drive.
    let open = |p: &Path| match p.ancestors().skip(1).find(|a| crate::paths::is_export_root(a)) {
        Some(root) if !crate::paths::is_export_root(p) => Device::open(root),
        _ => Device::open(p),
    };
    let path_like = p.is_absolute() || spec.contains(['/', '\\']) || spec.starts_with(['.', '~']);
    if path_like && !folders {
        return Err(Error::invalid(format!(
            "{spec:?} is a path: name a drive by its name or identity, or a library by its name"
        )));
    }
    if path_like && p.is_dir() {
        return open(p).map(|d| Found::Drive(Box::new(d)));
    }
    let candidates = known_drives(true);
    let lower = spec.to_lowercase();
    let labelled = |d: &Device| d.name().to_lowercase() == lower || d.volume_name().to_lowercase() == lower;
    let named = |d: &Device| match labels {
        true => labelled(d),
        false => d.is_adopted() && d.name().to_lowercase() == lower,
    };
    let (mut matches, others): (Vec<Device>, Vec<Device>) = candidates
        .into_iter()
        .partition(|d| named(d) || (spec.len() >= 4 && d.id().is_some_and(|id| id.to_string().starts_with(&lower))));
    let same = |d: &Device| p.canonicalize().ok().zip(d.root().canonicalize().ok()).is_some_and(|(a, b)| a == b);
    if matches.is_empty()
        && libraries
        && let Some(store) = registry()
        && let Some(library) = store.find_library(spec)?
    {
        let group = jobs::drive_group(Some(&store), Some(&library.name))?;
        return Ok(Found::Library(store, group));
    }
    // A folder here is taken only when folders are.
    let folder = folders && p.is_dir();
    match matches.len() {
        0 if labels && folder => open(p).map(|d| Found::Drive(Box::new(d))),
        0 => match others.iter().find(|d| labelled(d)) {
            Some(d) => Err(Error::invalid(format!(
                "{spec:?} is the label of the volume at {}, not the name of a drive locallibrary adopted: give its path, or the disk's identifier",
                d.root().display()
            ))),
            None => Err(jobs::unmatched_drive(spec, &others)),
        },
        1 if folder && !same(&matches[0]) => Err(Error::invalid(format!(
            "{spec:?} is both a folder here and the drive at {}; give a path (./{spec}) or the drive's mount path",
            matches[0].root().display()
        ))),
        1 => Ok(Found::Drive(Box::new(matches.remove(0)))),
        n => {
            let paths: Vec<String> = matches.iter().map(|d| d.root().display().to_string()).collect();
            Err(Error::invalid(format!("{spec:?} matches {n} drives ({}); use a path instead", paths.join(", "))))
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct ListDrives {
    /// Include mounted volumes without a rekordbox export (possible sync targets).
    #[serde(default)]
    pub all: bool,
}

pub fn list_drives(req: &ListDrives) -> Vec<DeviceInfo> {
    known_drives(req.all).iter().map(Device::info).collect()
}

/// A drive [`list_drives`] lists, with the name of the library it's a clone of.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct ListedDrive {
    #[serde(flatten)]
    pub drive: DeviceInfo,
    /// The store's name for the library `library_id` is, when the store has it.
    pub library: Option<String>,
}

/// [`list_drives`], each with its library's name from `store` (none without a store, or when
/// its libraries can't be read: the drives are listed all the same).
pub fn list_drives_in_libraries(store: Option<&Store>, req: &ListDrives) -> Vec<ListedDrive> {
    let libraries = store.and_then(|s| s.libraries().ok()).unwrap_or_default();
    let name = |id: Option<Uuid>| libraries.iter().find(|l| Some(l.id) == id).map(|l| l.name.clone());
    list_drives(req).into_iter().map(|drive| ListedDrive { library: name(drive.library_id), drive }).collect()
}

/// The mounted rekordbox drives a write that names none goes to (`smart apply`, `settings
/// apply`), and the names of those left out: archive and retired drives are written only when
/// named, or when writes to them are allowed.
pub fn unnamed_targets(store: Option<&Store>) -> (Vec<Device>, Vec<String>) {
    let (writable, kept): (Vec<Device>, Vec<Device>) = known_drives(false)
        .into_iter()
        .filter(Device::has_export)
        .partition(|d| crate::fleet::check_target(store, d, false).is_ok());
    (writable, kept.iter().map(Device::name).collect())
}

/// The warning for drives [`unnamed_targets`] left out.
pub fn left_out(names: &[String]) -> Option<String> {
    (!names.is_empty())
        .then(|| format!("left out {}: archive and retired drives aren't taken unless named", names.join(", ")))
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct DriveRef {
    pub drive: String,
}

pub fn drive_info(req: &DriveRef) -> Result<DeviceInfo> {
    Ok(resolve_one_drive(&req.drive)?.info())
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct AdoptDrive {
    pub drive: String,
    #[serde(default)]
    pub name: Option<String>,
}

/// What adopting a drive would do: give it an identity (`.locallibrary/state.json`), or a name.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct AdoptPlan {
    pub drive: String,
    pub root: PathBuf,
    /// Its identity, if it has one already.
    pub id: Option<Uuid>,
    /// The name it will have.
    pub name: String,
}

pub fn adopt_plan(req: &AdoptDrive) -> Result<Planned<AdoptPlan, AdoptDrive>> {
    let d = resolve_one_drive(&req.drive)?;
    let name = req.name.clone().unwrap_or_else(|| d.name());
    let line = match d.id() {
        None => format!("give {} an identity, named {name:?}", d.root().display()),
        Some(_) if name != d.name() => format!("rename {} to {name:?}", d.name()),
        Some(id) => format!("{} has its identity already ({id})", d.name()),
    };
    let noop = d.id().is_some() && name == d.name();
    let plan = AdoptPlan { drive: d.name(), root: d.root().into(), id: d.id(), name };
    Ok(Planned::new("drive.adopt", plan, req.clone()).line(line).noop(noop).touches(Touched::drive(
        &d,
        true,
        vec![op::Probe::Identity],
    )))
}

impl Operation for AdoptPlan {
    type Request = AdoptDrive;
    type Report = DeviceInfo;

    fn apply(planned: &Planned<Self, AdoptDrive>, store: Option<&Store>, _: &Ctx<'_>) -> Result<DeviceInfo> {
        let mut d = planned.drive(0)?;
        d.adopt(Some(&planned.plan.name))?;
        crate::fleet::note_hardware(&mut d)?;
        if let Some(store) = store {
            sync::register_drive(store, &d, None)?;
        }
        Ok(d.info())
    }
}

/// Plan and adopt in one go, for callers that don't confirm (see [`adopt_plan`]).
pub fn adopt_drive(store: Option<&Store>, req: &AdoptDrive) -> Result<DeviceInfo> {
    adopt_plan(req)?.apply(store, &Ctx::silent())
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct LibraryView {
    pub summary: LibrarySummary,
    pub library: Library,
}

pub fn drive_library(req: &DriveRef) -> Result<LibraryView> {
    let library = resolve_drive(&req.drive)?.library()?;
    Ok(LibraryView { summary: library.summary(), library })
}

pub fn list_backups(req: &DriveRef) -> Result<Vec<BackupInfo>> {
    backup::list(&resolve_one_drive(&req.drive)?)
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct RestoreBackup {
    pub drive: String,
    #[serde(default)]
    pub backup: Option<String>,
    /// Restore even if the backup references files that are gone.
    #[serde(default)]
    pub force: bool,
}

/// Put a backup back (planned and applied in one go; see [`undo_plan`]).
pub fn restore_backup(req: &RestoreBackup) -> Result<BackupInfo> {
    let undo = UndoRequest { drive: Some(req.drive.clone()), backup: req.backup.clone(), force: req.force };
    undo_apply(&undo, &Ctx::silent())
}

// ---------------------------------------------------------------------------------------------
// Compare & sync
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct ComparePair {
    pub left: String,
    pub right: String,
}

pub fn compare(req: &ComparePair, ctx: &Ctx<'_>) -> Result<Comparison> {
    let l = resolve_drive(&req.left)?;
    let r = resolve_drive(&req.right)?;
    compare::compare_devices(&l, &r, ctx)
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    /// Copy from whichever side is newer (refuses when that can't be determined).
    #[default]
    Auto,
    /// Copy left -> right regardless.
    LeftToRight,
    /// Copy right -> left regardless.
    RightToLeft,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct SyncRequest {
    pub left: String,
    pub right: String,
    #[serde(default)]
    pub direction: Direction,
    /// Accept a low-confidence "newer" verdict in auto mode.
    #[serde(default)]
    pub allow_low_confidence: bool,
    #[serde(default)]
    pub verify: bool,
    #[serde(default)]
    pub keep_extra: bool,
    #[serde(default)]
    pub delete_first: bool,
    /// Write to an archive or retired drive (see [`crate::fleet::Role`]).
    #[serde(default)]
    pub overwrite_archive: bool,
    /// With an explicit direction: the target came back from a set, so save it in the store
    /// first (as an automatic sync does; see [`SyncPreview::after_set`]).
    #[serde(default)]
    pub keep_player_changes: bool,
}

impl SyncRequest {
    fn options(&self) -> SyncOptions {
        SyncOptions { delete: !self.keep_extra, verify: self.verify, delete_first: self.delete_first }
    }
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct SyncPreview {
    pub comparison: Comparison,
    /// `None` when nothing needs to happen.
    pub source: Option<PathBuf>,
    pub target: Option<PathBuf>,
    pub plan: Option<SyncPlan>,
    /// The drives diverged, but the target's changes are all player-made (it came back from a
    /// set): it is saved in the store before being overwritten (see [`crate::after_set`]).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub after_set: Option<crate::after_set::AfterSet>,
}

/// What resolves two drives that can't be synced on their own: copying one over the other (and,
/// with `merge`, merging either into the other).
fn directions(e: Error, l: &Device, r: &Device, merge: bool) -> Error {
    let (ln, rn) = (l.name(), r.name());
    let (lr, rr) = (l.root().display().to_string(), r.root().display().to_string());
    let sync = |direction: &str| serde_json::json!({ "left": lr, "right": rr, "direction": direction });
    let mut e = e
        .with("left", &lr)
        .with("right", &rr)
        .action(Action::call("sync.plan", sync("left_to_right"), format!("Copy {ln} over {rn}")))
        .action(Action::call("sync.plan", sync("right_to_left"), format!("Copy {rn} over {ln}")));
    if merge {
        e = e
            .action(Action::call(
                "merge.plan",
                serde_json::json!({ "source": lr, "target": rr }),
                format!("Merge {ln} into {rn}"),
            ))
            .action(Action::call(
                "merge.plan",
                serde_json::json!({ "source": rr, "target": lr }),
                format!("Merge {rn} into {ln}"),
            ));
    }
    e
}

/// Refusing to sync two drives `c` can't put in order: they both changed, or which is newer
/// can't be told (saying why).
fn unordered(c: &Comparison, l: &Device, r: &Device) -> Error {
    let (ln, rn) = (l.name(), r.name());
    let e = match c.undetermined() {
        Some(why) => Error::conflict(Reason::Uncertain, why),
        None => Error::conflict(
            Reason::Diverged,
            format!(
                "{ln} and {rn} both have changes the other lacks: merging combines them, or copying {ln} onto {rn} \
                 keeps {ln}'s (and loses {rn}'s)"
            ),
        ),
    };
    directions(e, l, r, true)
}

struct Endpoints {
    comparison: Comparison,
    /// Source, target.
    drives: Option<(Device, Device)>,
    after_set: Option<crate::after_set::AfterSet>,
}

fn sync_endpoints(req: &SyncRequest, ctx: &Ctx<'_>) -> Result<Endpoints> {
    let l = resolve_drive(&req.left)?;
    let r = resolve_drive(&req.right)?;
    if l.root() == r.root() {
        return Err(Error::invalid("both arguments refer to the same drive"));
    }
    // A blank drive can only be a target.
    let comparison = match (l.has_export(), r.has_export()) {
        (true, true) => compare::compare_devices(&l, &r, ctx)?,
        (true, false) | (false, true) => {
            let src = if l.has_export() { &l } else { &r };
            let side = Side::from_device(src, ctx)?;
            let empty = Side {
                label: if l.has_export() { r.name() } else { l.name() },
                fingerprint: None,
                aliases: vec![],
                lineage: vec![],
                library: Library::default(),
                ..side.clone()
            };
            let (left, right) = if l.has_export() { (side, empty) } else { (empty, side) };
            let mut c = compare::compare(&left, &right);
            c.relation = if l.has_export() { Relation::LeftNewer } else { Relation::RightNewer };
            c.suggestion =
                if l.has_export() { compare::Suggestion::UpdateRight } else { compare::Suggestion::UpdateLeft };
            c.confidence = compare::Confidence::High;
            c.diff = crate::model::LibraryDiff::between(&Library::default(), &src.library()?);
            c.evidence.clear();
            c
        }
        (false, false) => return Err(Error::invalid("neither drive holds a rekordbox export")),
    };
    let mut after_set = None;
    let left_to_right = match req.direction {
        Direction::LeftToRight | Direction::RightToLeft if req.keep_player_changes => {
            let left_to_right = req.direction == Direction::LeftToRight;
            let g = registry()
                .map(|s| crate::after_set::resolve(&s, &l, &r, ctx))
                .transpose()?
                .flatten()
                .filter(|g| g.left_is_back_from_set != left_to_right)
                .ok_or_else(|| {
                    Error::StalePlan("the target no longer looks like a drive back from a set; plan again".into())
                })?;
            after_set = Some(g);
            left_to_right
        }
        Direction::LeftToRight => true,
        Direction::RightToLeft => false,
        Direction::Auto => {
            let (ln, rn) = (l.name(), r.name());
            match comparison.relation {
                Relation::Identical => return Ok(Endpoints { comparison, drives: None, after_set: None }),
                Relation::LeftNewer | Relation::RightNewer => {
                    if comparison.confidence == compare::Confidence::Low && !req.allow_low_confidence {
                        return Err(directions(
                            Error::conflict(
                                Reason::Uncertain,
                                format!(
                                    "can't tell with confidence whether {ln} or {rn} is newer: say which to copy \
                                     from, or take the guess"
                                ),
                            ),
                            &l,
                            &r,
                            false,
                        )
                        .action(Action::retry(serde_json::json!({ "allow_low_confidence": true }), "Take the guess")));
                    }
                    comparison.relation == Relation::LeftNewer
                }
                Relation::Diverged => {
                    match registry().map(|s| crate::after_set::resolve(&s, &l, &r, ctx)).transpose()?.flatten() {
                        Some(g) => {
                            let left_to_right = !g.left_is_back_from_set;
                            after_set = Some(g);
                            left_to_right
                        }
                        None => return Err(unordered(&comparison, &l, &r)),
                    }
                }
                Relation::Undetermined => return Err(unordered(&comparison, &l, &r)),
                Relation::Unrelated => {
                    return Err(directions(
                        Error::conflict(
                            Reason::Unrelated,
                            format!(
                                "{ln} and {rn} hold different libraries: merging brings one's tracks into the \
                                 other, or copying {ln} onto {rn} overwrites {rn}"
                            ),
                        ),
                        &l,
                        &r,
                        true,
                    ));
                }
            }
        }
    };
    let (src, dst) = if left_to_right { (l, r) } else { (r, l) };
    crate::fleet::check_target(registry().as_ref(), &dst, req.overwrite_archive)?;
    Ok(Endpoints { comparison, drives: Some((src, dst)), after_set })
}

/// What syncing two drives would do. Applying it copies the way it was planned, and only if
/// both drives are as they were.
pub fn sync_plan(req: &SyncRequest, ctx: &Ctx<'_>) -> Result<Planned<SyncPreview, SyncRequest>> {
    let Endpoints { comparison, drives, after_set } = sync_endpoints(req, ctx)?;
    let Some((src, dst)) = drives else {
        let preview = SyncPreview { comparison, source: None, target: None, plan: None, after_set: None };
        return Ok(Planned::new("sync", preview, req.clone())
            .noop(true)
            .line("the drives already hold the same library"));
    };
    let (plan, _) = sync::plan_drive_to_drive(&src, &dst, &req.options(), ctx)?;
    // Applying copies this way round, as confirmed.
    let pinned = SyncRequest {
        left: src.root().display().to_string(),
        right: dst.root().display().to_string(),
        direction: Direction::LeftToRight,
        keep_player_changes: after_set.is_some(),
        ..req.clone()
    };
    let mut summary = sync_summary(&src.name(), &dst.name(), &plan);
    if let Some(g) = &after_set {
        summary.insert(
            0,
            format!(
                "{} came back from a set ({}): it's saved in the store first",
                g.back_from_set.drive,
                g.back_from_set.describe()
            ),
        );
    }
    let (noop, destructive, warnings) = (plan.is_noop(), !plan.deletes.is_empty(), plan.warnings.clone());
    let preview = SyncPreview {
        comparison,
        source: Some(src.root().into()),
        target: Some(dst.root().into()),
        plan: Some(plan),
        after_set,
    };
    Ok(Planned::new("sync", preview, pinned)
        .summary(summary)
        .warnings(warnings)
        .destructive(destructive)
        .noop(noop)
        .overwrite_archive(req.overwrite_archive)
        .touches(Touched::library(&dst, true))
        .touches(Touched::library(&src, false)))
}

/// What copying one drive onto another does, in a line or two.
fn sync_summary(source: &str, target: &str, plan: &SyncPlan) -> Vec<String> {
    let mut out =
        vec![format!("copy {} file(s) ({}) from {source} onto {target}", plan.copies.len(), bytes(plan.copy_bytes))];
    if !plan.deletes.is_empty() {
        out.push(format!("delete {} file(s) ({}) from {target}", plan.deletes.len(), bytes(plan.delete_bytes)));
    }
    out
}

impl Operation for SyncPreview {
    type Request = SyncRequest;
    type Report = SyncResult;

    fn apply(planned: &Planned<Self, SyncRequest>, store: Option<&Store>, ctx: &Ctx<'_>) -> Result<SyncResult> {
        match &planned.plan.plan {
            None => Ok(SyncResult { source: None, target: None, report: None, saved: Vec::new() }),
            Some(plan) => sync_pinned(store, planned.request(), plan, ctx),
        }
    }
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct SyncResult {
    pub source: Option<PathBuf>,
    pub target: Option<PathBuf>,
    pub report: Option<SyncReport>,
    /// Snapshots taken along the way (after a set: the state the drive came back with, then the
    /// result).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub saved: Vec<SnapshotInfo>,
}

/// Plan and apply a sync in one go, for callers that don't confirm (see [`sync_plan`]).
pub fn sync_apply(store: Option<&Store>, req: &SyncRequest, ctx: &Ctx<'_>) -> Result<SyncResult> {
    sync_plan(req, ctx)?.apply(store, ctx)
}

/// Sync with the direction `req` gives, copying exactly what `confirmed` does.
fn sync_pinned(store: Option<&Store>, req: &SyncRequest, confirmed: &SyncPlan, ctx: &Ctx<'_>) -> Result<SyncResult> {
    let Endpoints { drives, after_set, .. } = sync_endpoints(req, ctx)?;
    // Only `Direction::Auto` finds nothing to do, and a confirmed sync is pinned to a direction.
    let (mut src, mut dst) = drives.expect("a pinned sync names both drives");
    // Back from a set: keep the players' changes in the store before overwriting them.
    let mut saved = Vec::new();
    // Both snapshots go into the library of the state the drives last shared.
    let mut set_library = None;
    if let Some(g) = &after_set {
        let store = store.ok_or_else(|| {
            let drive = dst.root().display().to_string();
            Error::conflict(Reason::NotSaved, "keeping the changes players made needs the store; pull the drive first")
                .with("drive", &drive)
                .action(Action::call(
                    "pull",
                    serde_json::json!({ "drive": drive }),
                    format!("Pull {} into the store", dst.name()),
                ))
        })?;
        set_library = store.library_by_id(g.common.library_id)?.map(|l| l.name);
        let message = format!("{} after a set: {}", dst.name(), g.back_from_set.describe());
        let opts = PullOptions { library: set_library.clone(), message: Some(message), force: true };
        saved.extend(sync::pull(store, &mut dst, &opts, ctx)?.snapshot);
    }
    let report = sync::sync_drive_to_drive_as_planned(&mut src, &mut dst, &req.options(), confirmed, ctx)?;
    if let Some(store) = store {
        sync::register_drive(store, &src, None)?;
        sync::register_drive(store, &dst, report.fingerprint)?;
        crate::fleet::record_write(store, &dst, report.copied_bytes)?;
        if after_set.is_some() {
            // The newer library becomes the head, after the state the drive came back with.
            let opts = PullOptions {
                library: set_library.clone(),
                message: Some(format!("{} (synced after a set)", src.name())),
                force: true,
            };
            saved.extend(sync::pull(store, &mut src, &opts, ctx)?.snapshot);
            sync::register_drive(store, &dst, report.fingerprint)?;
        }
    }
    Ok(SyncResult { source: Some(src.root().into()), target: Some(dst.root().into()), report: Some(report), saved })
}

/// Sync any number of drives: the single newest one is copied onto all the others.
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct SyncManyRequest {
    /// Drives, or a library's name for its mounted clones (but archive and retired ones).
    pub drives: Vec<String>,
    #[serde(default)]
    pub allow_low_confidence: bool,
    #[serde(default)]
    pub verify: bool,
    #[serde(default)]
    pub keep_extra: bool,
    #[serde(default)]
    pub delete_first: bool,
    #[serde(default)]
    pub overwrite_archive: bool,
}

impl SyncManyRequest {
    fn pair(&self, source: &Path, target: &Path, direction: Direction) -> SyncRequest {
        SyncRequest {
            left: source.display().to_string(),
            right: target.display().to_string(),
            direction,
            allow_low_confidence: self.allow_low_confidence,
            verify: self.verify,
            keep_extra: self.keep_extra,
            delete_first: self.delete_first,
            overwrite_archive: self.overwrite_archive,
            keep_player_changes: false,
        }
    }

    fn options(&self) -> SyncOptions {
        SyncOptions { delete: !self.keep_extra, verify: self.verify, delete_first: self.delete_first }
    }
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct SyncManyPreview {
    pub source_name: String,
    pub source: PathBuf,
    /// One entry per other drive; `plan` is `None` for drives already identical to the source.
    pub targets: Vec<SyncPreview>,
}

/// Find the newest drive. Fails with `conflict` if two drives diverged, hold different
/// libraries, or can't be ordered with confidence.
fn newest_of(req: &SyncManyRequest, ctx: &Ctx<'_>) -> Result<(Device, Vec<Device>)> {
    let specs = expand_libraries(&req.drives)?.0;
    if specs.len() < 2 {
        return Err(Error::invalid("give at least two drives"));
    }
    let mut devices = Vec::new();
    for spec in &specs {
        let d = resolve_drive(spec)?;
        if devices.iter().any(|x: &Device| x.root() == d.root()) {
            return Err(Error::invalid(format!("{spec:?} is listed twice")));
        }
        devices.push(d);
    }
    let mut best: Option<usize> = None;
    for i in 0..devices.len() {
        if !devices[i].has_export() {
            continue; // blank drives are always targets
        }
        let Some(b) = best else {
            best = Some(i);
            continue;
        };
        let c = compare::compare_devices(&devices[b], &devices[i], ctx)?;
        let (l, r) = (devices[b].name(), devices[i].name());
        match c.relation {
            Relation::Identical | Relation::LeftNewer | Relation::RightNewer
                if c.confidence == compare::Confidence::Low
                    && !req.allow_low_confidence
                    && c.relation != Relation::Identical =>
            {
                return Err(directions(
                    Error::conflict(
                        Reason::Uncertain,
                        format!(
                            "can't tell with confidence whether {l} or {r} is newer: say which to copy from, or take \
                             the guess"
                        ),
                    ),
                    &devices[b],
                    &devices[i],
                    false,
                )
                .action(Action::retry(serde_json::json!({ "allow_low_confidence": true }), "Take the guess")));
            }
            Relation::Identical | Relation::LeftNewer => {}
            Relation::RightNewer => best = Some(i),
            Relation::Diverged | Relation::Undetermined => {
                let back = registry()
                    .map(|s| crate::after_set::resolve(&s, &devices[b], &devices[i], ctx))
                    .transpose()?
                    .flatten();
                let (lr, rr) = (devices[b].root().display().to_string(), devices[i].root().display().to_string());
                return Err(match back {
                    Some(g) => Error::conflict(
                        Reason::BackFromSet,
                        format!(
                            "{} came back from a set ({}); sync {l} and {r} as a pair first, which keeps those changes in the store",
                            if g.left_is_back_from_set { &l } else { &r },
                            g.back_from_set.describe()
                        ),
                    )
                    .with("left", &lr)
                    .with("right", &rr)
                    .action(Action::call(
                        "sync.plan",
                        serde_json::json!({ "left": lr, "right": rr }),
                        format!("Sync {l} and {r} first"),
                    )),
                    None => unordered(&c, &devices[b], &devices[i]),
                });
            }
            Relation::Unrelated => {
                return Err(Error::conflict(Reason::Unrelated, format!("{l} and {r} hold different libraries"))
                    .with("left", devices[b].root())
                    .with("right", devices[i].root()));
            }
        }
    }
    let best = best.ok_or_else(|| Error::invalid("none of the drives holds a rekordbox export"))?;
    let source = devices.remove(best);
    Ok((source, devices))
}

/// What syncing several drives would do: the newest one copied onto each of the others.
pub fn sync_many_plan(req: &SyncManyRequest, ctx: &Ctx<'_>) -> Result<Planned<SyncManyPreview, SyncManyRequest>> {
    many_planned("sync.many", req, ctx)
}

/// [`sync_many_plan`] as `operation`.
pub(crate) fn many_planned(
    operation: &'static str,
    req: &SyncManyRequest,
    ctx: &Ctx<'_>,
) -> Result<Planned<SyncManyPreview, SyncManyRequest>> {
    let (source, targets) = newest_of(req, ctx)?;
    let mut previews = Vec::new();
    for t in &targets {
        let p = ctx.for_target(sync::target_label(t), |ctx| {
            sync_plan(&req.pair(source.root(), t.root(), Direction::Auto), ctx)
        })?;
        if p.source.as_deref().is_some_and(|s| s != source.root()) {
            return Err(Error::conflict(
                Reason::Changed,
                format!("{} looks newer than {}; run the sync again", t.name(), source.name()),
            ));
        }
        previews.push(p.plan);
    }
    let (mut summary, mut warnings, mut destructive, mut noop) = (Vec::new(), Vec::new(), false, true);
    for (t, p) in targets.iter().zip(&previews) {
        match &p.plan {
            Some(plan) if !plan.is_noop() => {
                summary.extend(sync_summary(&source.name(), &t.name(), plan));
                warnings.extend(plan.warnings.iter().cloned());
                destructive |= !plan.deletes.is_empty();
                noop = false;
            }
            _ => summary.push(format!("{} already matches {}", t.name(), source.name())),
        }
    }
    let preview =
        SyncManyPreview { source_name: source.name(), source: source.root().to_path_buf(), targets: previews };
    let mut planned = Planned::new(operation, preview, req.clone())
        .summary(summary)
        .warnings(warnings)
        .destructive(destructive)
        .noop(noop)
        .overwrite_archive(req.overwrite_archive);
    for t in &targets {
        planned = planned.touches(Touched::library(t, true));
    }
    Ok(planned.touches(Touched::library(&source, false)))
}

impl Operation for SyncManyPreview {
    type Request = SyncManyRequest;
    type Report = Vec<TargetResult>;

    fn apply(
        planned: &Planned<Self, SyncManyRequest>,
        store: Option<&Store>,
        ctx: &Ctx<'_>,
    ) -> Result<Vec<TargetResult>> {
        sync_many_apply(store, planned.request(), &planned.plan, ctx)
    }
}

/// One target of a [`sync_many_apply`].
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct TargetResult {
    pub target: PathBuf,
    pub name: String,
    pub report: Option<SyncReport>,
    /// Why this target failed (the others carry on).
    pub error: Option<TargetError>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct TargetError {
    pub kind: &'static str,
    pub message: String,
}

impl From<Error> for TargetError {
    fn from(e: Error) -> Self {
        TargetError { kind: e.code(), message: e.to_string() }
    }
}

/// Apply a confirmed [`sync_many_plan`]: copy `source` onto every target that needs it, all at
/// once, each exactly as planned (see [`sync::sync_drive_to_many_as_planned`]). A target that
/// fails doesn't stop the others.
pub fn sync_many_apply(
    store: Option<&Store>,
    req: &SyncManyRequest,
    preview: &SyncManyPreview,
    ctx: &Ctx<'_>,
) -> Result<Vec<TargetResult>> {
    let mut source = resolve_drive(&preview.source.display().to_string())?;
    let mut targets = Vec::new();
    let mut confirmed = Vec::new();
    let mut out = Vec::new();
    for t in preview.targets.iter().filter(|t| t.plan.as_ref().is_some_and(|p| !p.is_noop())) {
        let path = t.target.as_ref().expect("planned targets have a path");
        // Re-check the direction that was confirmed, as a pair sync would.
        let checked = ctx
            .for_target(path.display().to_string(), |ctx| {
                sync_endpoints(&req.pair(&preview.source, path, Direction::LeftToRight), ctx)
            })
            .and_then(|e| e.drives.ok_or_else(|| Error::conflict(Reason::Changed, "nothing to copy any more")));
        match checked {
            Ok((_, dst)) => {
                targets.push(dst);
                confirmed.extend(t.plan.clone());
            }
            Err(e) => out.push(TargetResult {
                target: path.clone(),
                name: t.comparison.right.label.clone(),
                report: None,
                error: Some(e.into()),
            }),
        }
    }
    if targets.is_empty() {
        return Ok(out);
    }
    let results = sync::sync_drive_to_many_as_planned(&mut source, &mut targets, &req.options(), &confirmed, ctx)?;
    for (dst, result) in targets.iter().zip(results) {
        let (report, error) = match result {
            Ok(r) => (Some(r), None),
            Err(e) => (None, Some(TargetError::from(e))),
        };
        if let (Some(store), Some(r)) = (store, &report) {
            sync::register_drive(store, dst, r.fingerprint)?;
            crate::fleet::record_write(store, dst, r.copied_bytes)?;
        }
        out.push(TargetResult { target: dst.root().to_path_buf(), name: dst.name(), report, error });
    }
    if let Some(store) = store {
        sync::register_drive(store, &source, None)?;
    }
    Ok(out)
}

// ---------------------------------------------------------------------------------------------
// Store
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct StoreInfo {
    pub root: PathBuf,
    pub stats: StoreStats,
    /// Size of the copies `art embed` keeps of the files it rewrote, for `art unembed`.
    pub embed_backup_bytes: u64,
    pub libraries: Vec<LibraryRecord>,
    /// When this store is a copy of another (`store copy`): when it was made.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub copy_of: Option<crate::offsite::CopyRecord>,
}

pub fn store_info(store: &Store) -> Result<StoreInfo> {
    Ok(StoreInfo {
        root: store.root().to_path_buf(),
        stats: store.stats()?,
        embed_backup_bytes: artwork::embed_backup_bytes(store),
        libraries: store.libraries()?,
        copy_of: crate::offsite::marker(store.root())?,
    })
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct HistoryRequest {
    pub library: String,
    #[serde(default)]
    pub limit: Option<usize>,
}

/// A library's snapshots, newest first: its latest back through first parents, and the ones
/// saved beside that line (drives back from a set), marked `beside`.
pub fn history(store: &Store, req: &HistoryRequest) -> Result<Vec<SnapshotInfo>> {
    let lib = store.find_library(&req.library)?.ok_or_else(|| Error::NotFound(format!("library {:?}", req.library)))?;
    let mut out = store.history(&lib, None)?;
    let line: FxHashSet<_> = out.iter().map(|s| s.id).collect();
    let labels = store.labels_by_snapshot()?;
    for id in store.snapshot_ids()?.into_iter().filter(|id| !line.contains(id)) {
        let info = store.load_info(&id)?;
        if info.library_id == lib.id {
            out.push(SnapshotInfo { labels: labels.get(&id).cloned().unwrap_or_default(), beside: true, ..info });
        }
    }
    out.sort_by_key(|s| std::cmp::Reverse(s.created));
    out.truncate(req.limit.unwrap_or(usize::MAX));
    Ok(out)
}

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct PullRequest {
    pub drive: String,
    /// The library to pull into [default: the drive's, else a new one named after the drive].
    #[serde(default)]
    pub library: Option<String>,
    /// Start a new library when no library has the name `library` gives. Without it, an unknown
    /// name is refused, so a typo doesn't start a new library.
    #[serde(default)]
    pub new_library: bool,
    #[serde(default)]
    pub message: Option<String>,
    /// Label the snapshot (see [`Store::add_label`]); when the drive is already saved, the label
    /// goes on the snapshot it matches.
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub force: bool,
}

pub fn pull(store: &Store, req: &PullRequest, ctx: &Ctx<'_>) -> Result<PullReport> {
    if let Some(name) = &req.library
        && !req.new_library
        && store.find_library(name)?.is_none()
    {
        return Err(Error::NotFound(format!(
            "library {name:?} in the store ({}); pull into a new library to start one",
            snapshots::known_libraries(store)?
        ))
        .because(Reason::UnknownLibrary)
        .with("library", name)
        .action(Action::retry(serde_json::json!({ "new_library": true }), format!("Start the library {name:?}"))));
    }
    // Checked before the pull, which can take a while.
    let label = req.label.as_deref().map(str::trim);
    if let Some(l) = label {
        if l.is_empty() {
            return Err(Error::invalid("a label can't be empty"));
        }
        if let Some(taken) = store.find_label(l)? {
            return Err(crate::store::label_taken(&taken));
        }
    }
    let mut d = resolve_drive(&req.drive)?;
    let opts = PullOptions { library: req.library.clone(), message: req.message.clone(), force: req.force };
    let mut report = sync::pull(store, &mut d, &opts, ctx)?;
    if let Some(snap) = report.snapshot.as_mut() {
        if let Some(l) = label {
            let _lock = store.lock()?;
            store.add_label(&snap.id, l)?;
        }
        snap.labels = store.labels_by_snapshot()?.shift_remove(&snap.id).unwrap_or_default();
    }
    Ok(report)
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct PushRequest {
    pub drive: String,
    #[serde(default)]
    pub library: Option<String>,
    #[serde(default)]
    pub snapshot: Option<String>,
    #[serde(default)]
    pub force: bool,
    #[serde(default)]
    pub verify: bool,
    #[serde(default)]
    pub keep_extra: bool,
    #[serde(default)]
    pub delete_first: bool,
}

impl PushRequest {
    fn options(&self) -> PushOptions {
        PushOptions {
            library: self.library.clone(),
            snapshot: self.snapshot.clone(),
            force: self.force,
            sync: SyncOptions { delete: !self.keep_extra, verify: self.verify, delete_first: self.delete_first },
        }
    }
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct PushPreview {
    pub snapshot: SnapshotInfo,
    pub plan: SyncPlan,
}

/// What writing a snapshot onto a drive would do. Applying it writes that snapshot (even if the
/// library has a newer one by then), exactly as planned.
pub fn push_plan(store: &Store, req: &PushRequest, ctx: &Ctx<'_>) -> Result<Planned<PushPreview, PushRequest>> {
    let d = resolve_one_drive(&req.drive)?;
    let opts = req.options();
    let snap = sync::resolve_push_snapshot(store, &d, &opts)?;
    let plan = sync::plan_push(store, &snap, &d, &opts, ctx)?;
    let pinned = PushRequest { drive: d.root().display().to_string(), snapshot: Some(snap.id.to_hex()), ..req.clone() };
    let info = snap.info();
    let summary =
        sync_summary(&format!("snapshot {} ({} tracks)", info.short_id, info.summary.tracks), &d.name(), &plan);
    let (noop, destructive, warnings) = (plan.is_noop(), !plan.deletes.is_empty(), plan.warnings.clone());
    Planned::new("push", PushPreview { snapshot: info, plan }, pinned)
        .summary(summary)
        .warnings(warnings)
        .destructive(destructive)
        .noop(noop)
        .overwrite_archive(req.force)
        .touches(Touched::library(&d, true))
        .guard(Some(store))
}

impl Operation for PushPreview {
    type Request = PushRequest;
    type Report = SyncReport;

    fn apply(planned: &Planned<Self, PushRequest>, store: Option<&Store>, ctx: &Ctx<'_>) -> Result<SyncReport> {
        let store = store.ok_or_else(|| Error::invalid("writing a snapshot needs the store"))?;
        push_pinned(store, planned.request(), &planned.plan.plan, ctx)
    }
}

/// Plan and apply a push in one go, for callers that don't confirm (see [`push_plan`]).
pub fn push_apply(store: &Store, req: &PushRequest, ctx: &Ctx<'_>) -> Result<SyncReport> {
    push_plan(store, req, ctx)?.apply(Some(store), ctx)
}

fn push_pinned(store: &Store, req: &PushRequest, confirmed: &SyncPlan, ctx: &Ctx<'_>) -> Result<SyncReport> {
    let mut d = resolve_one_drive(&req.drive)?;
    crate::fleet::check_target(Some(store), &d, req.force)?;
    let opts = req.options();
    let snap = sync::resolve_push_snapshot(store, &d, &opts)?;
    let report = sync::push_as_planned(store, &snap, &mut d, &opts, confirmed, ctx)?;
    crate::fleet::record_write(store, &d, report.copied_bytes)?;
    Ok(report)
}
/// One drive's standing relative to its library's latest snapshot.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct DriveStatus {
    pub drive: DeviceInfo,
    pub library: Option<String>,
    /// The library head (left) vs. the drive (right): `identical`, `left_newer` (the drive is
    /// behind: push), `right_newer` (the drive has newer changes: pull them), `diverged`... `None`
    /// if not linked to a library.
    pub relation: Option<Relation>,
    pub head: Option<SnapshotInfo>,
    /// Back from a set: the drive is newer than the head only by what players add (history
    /// lists, TAG LIST playlists), with this many history lists new or longer. Pulling saves them.
    pub back_from_set: Option<usize>,
    /// Settings that differ from the profile.
    pub settings_drift: usize,
    pub error: Option<String>,
}

/// Every mounted drive against its library's head snapshot, plus settings drift. With no store,
/// no drive has a library or drifts.
pub fn status(store: Option<&Store>, ctx: &Ctx<'_>) -> Result<Vec<DriveStatus>> {
    Ok(statuses(store, ctx)?.into_iter().map(|(s, _)| s).collect())
}

/// [`status`], with the library state each drive holds, to tell which hold the same: the head's
/// fingerprint when it's identical to it (a drive synced with `--keep-extra` also goes by it),
/// else its own. `None` when it wasn't compared.
fn statuses(store: Option<&Store>, ctx: &Ctx<'_>) -> Result<Vec<(DriveStatus, Option<Hash>)>> {
    let profile = store.map(Store::settings_profile).transpose()?.unwrap_or_default();
    let mut out = Vec::new();
    for d in known_drives(false) {
        let info = d.info();
        let current = settings::read_all(&DriveSettings(d.root()));
        let settings_drift = if profile.base.is_empty() && profile.drives.is_empty() {
            0
        } else {
            settings::drift(&profile, d.id(), &current).len()
        };
        let mut st = DriveStatus {
            drive: info,
            library: None,
            relation: None,
            head: None,
            back_from_set: None,
            settings_drift,
            error: None,
        };
        let mut state = None;
        let lib = match (store, d.state().and_then(|s| s.library_id)) {
            (Some(store), Some(id)) => store.library_by_id(id)?,
            _ => None,
        };
        if let (Some(store), Some(lib), true) = (store, lib, d.has_export()) {
            st.library = Some(lib.name.clone());
            if let Some(head) = lib.head {
                let snap = store.load_snapshot(&head)?;
                st.head = Some(snap.info());
                match Side::from_snapshot(store, &snap, "store").and_then(|s| Ok((s, Side::from_device(&d, ctx)?))) {
                    Ok((left, right)) => {
                        let relation = compare::compare(&left, &right).relation;
                        st.relation = Some(relation);
                        state = if relation == Relation::Identical { left.fingerprint } else { right.fingerprint };
                        if relation == Relation::RightNewer {
                            let (then, now) = (Databases::from_snapshot(store, &snap), Databases::read(&d));
                            match then.and_then(|then| Ok(crate::after_set::database_changes(&then, &now?))) {
                                Ok(c) => st.back_from_set = c.player_only().then_some(c.history_lists.len()),
                                Err(e) => st.error = Some(e.to_string()),
                            }
                        }
                    }
                    Err(e) => st.error = Some(e.to_string()),
                }
            }
        }
        out.push((st, state));
    }
    Ok(out)
}

// ---------------------------------------------------------------------------------------------
// Merge
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct MergeRequest {
    pub source: String,
    pub target: String,
    #[serde(default, flatten)]
    pub options: MergeOptions,
}

/// What merging one drive's library into another's would add.
pub fn merge_plan(req: &MergeRequest, ctx: &Ctx<'_>) -> Result<Planned<merge::MergePlan, MergeOptions>> {
    let src = resolve_drive(&req.source)?;
    let dst = resolve_drive(&req.target)?;
    let plan = merge::plan(&src, &dst, &req.options, ctx)?;
    let mut summary = vec![format!(
        "add {} track(s) ({}) from {} to {}",
        plan.new_tracks.len(),
        bytes(plan.copy_bytes),
        src.name(),
        dst.name()
    )];
    let created = plan.playlists.iter().filter(|p| p.create).count();
    let filled = plan.playlists.iter().filter(|p| p.tracks_added > 0).count();
    if created + filled > 0 {
        summary.push(format!("create {created} playlist(s), add entries to {filled}"));
    }
    if !plan.history.is_empty() {
        summary.push(format!("bring {} history list(s) across", plan.history.len()));
    }
    let (noop, warnings) = (plan.is_noop(), plan.warnings.clone());
    Planned::new("merge", plan, req.options.clone())
        .summary(summary)
        .warnings(warnings)
        .noop(noop)
        .touches(Touched::library(&dst, true))
        .touches(Touched::library(&src, false))
        .reaching_clones(ctx)?
        .guard(None)
}

impl Operation for merge::MergePlan {
    type Request = MergeOptions;
    type Report = merge::MergeReport;

    fn apply(planned: &Planned<Self, MergeOptions>, _: Option<&Store>, ctx: &Ctx<'_>) -> Result<merge::MergeReport> {
        let (mut dst, src) = (planned.drive(0)?, planned.drive(1)?);
        merge::apply(&planned.plan, &src, &mut dst, planned.request(), ctx)
    }
}

/// Plan and apply a merge in one go, for callers that don't confirm (see [`merge_plan`]).
pub fn merge_apply(req: &MergeRequest, ctx: &Ctx<'_>) -> Result<merge::MergeReport> {
    merge_plan(req, ctx)?.apply(None, ctx)
}
// ---------------------------------------------------------------------------------------------
// Clean
// ---------------------------------------------------------------------------------------------

/// The useless tracks (rekordbox's demo samples) a clean would remove, with their files.
pub fn clean_plan(req: &DriveRef, ctx: &Ctx<'_>) -> Result<Planned<clean::CleanPlan>> {
    let d = resolve_drive(&req.drive)?;
    let plan = clean::plan(&d, ctx)?;
    let line = format!(
        "remove {} track(s) and {} file(s) ({}) from {}",
        plan.tracks.len(),
        plan.files.len(),
        bytes(plan.bytes),
        d.name()
    );
    let (noop, warnings) = (plan.is_noop(), plan.warnings.clone());
    Planned::new("clean", plan, ())
        .line(line)
        .warnings(warnings)
        .destructive(!noop)
        .noop(noop)
        .touches(Touched::library(&d, true))
        .reaching_clones(ctx)?
        .guard(None)
}

impl Operation for clean::CleanPlan {
    type Request = ();
    type Report = clean::CleanReport;

    fn apply(planned: &Planned<Self>, _: Option<&Store>, ctx: &Ctx<'_>) -> Result<clean::CleanReport> {
        clean::apply(&planned.plan, &mut planned.drive(0)?, ctx)
    }
}

/// Plan and apply a clean in one go, for callers that don't confirm (see [`clean_plan`]).
pub fn clean_apply(req: &DriveRef, ctx: &Ctx<'_>) -> Result<clean::CleanReport> {
    clean_plan(req, ctx)?.apply(None, ctx)
}
// ---------------------------------------------------------------------------------------------
// Convert
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct ConvertRequest {
    pub drive: String,
    #[serde(flatten)]
    pub options: convert::ConvertOptions,
}

/// What building one database from the other would make.
pub fn convert_plan(
    req: &ConvertRequest,
    ctx: &Ctx<'_>,
) -> Result<Planned<convert::ConvertPlan, convert::ConvertOptions>> {
    let d = resolve_drive(&req.drive)?;
    let plan = convert::plan(&d, &req.options, ctx)?;
    let (to, from) = (plan.to.file(), plan.to.source_file());
    let line = if plan.adds {
        format!(
            "add what {from} has to the {to} on {} ({} tracks, {} playlists, {} history lists)",
            d.name(),
            plan.tracks,
            plan.playlists,
            plan.history_playlists
        )
    } else {
        let replacing = if plan.replaces { format!(", replacing the {to} it has") } else { String::new() };
        format!(
            "build {to} from {from} on {} ({} tracks, {} playlists){replacing}",
            d.name(),
            plan.tracks,
            plan.playlists
        )
    };
    let (replaces, noop, warnings) = (plan.replaces, plan.is_noop(), plan.warnings.clone());
    Planned::new("convert", plan, req.options.clone())
        .line(line)
        .warnings(warnings)
        .noop(noop)
        .destructive(replaces)
        .touches(Touched::library(&d, true))
        .reaching_clones(ctx)?
        .guard(None)
}

impl Operation for convert::ConvertPlan {
    type Request = convert::ConvertOptions;
    type Report = convert::ConvertReport;

    fn apply(
        planned: &Planned<Self, convert::ConvertOptions>,
        _: Option<&Store>,
        ctx: &Ctx<'_>,
    ) -> Result<convert::ConvertReport> {
        convert::apply(&planned.plan, &mut planned.drive(0)?, planned.request(), ctx)
    }
}

/// Plan and apply a conversion in one go, for callers that don't confirm (see [`convert_plan`]).
pub fn convert_apply(req: &ConvertRequest, ctx: &Ctx<'_>) -> Result<convert::ConvertReport> {
    convert_plan(req, ctx)?.apply(None, ctx)
}
// ---------------------------------------------------------------------------------------------
// Settings
// ---------------------------------------------------------------------------------------------

/// Identify the drive a per-drive override applies to: a mounted drive (adopted on the spot), or
/// a drive in the store's registry by name or id.
fn override_target(store: &Store, spec: &str) -> Result<(Uuid, String)> {
    if let Ok(mut d) = resolve_one_drive(spec) {
        let state = d.adopt(None)?;
        let out = (state.id, state.name.clone());
        sync::register_drive(store, &d, None)?;
        return Ok(out);
    }
    let lower = spec.to_lowercase();
    store
        .drives()?
        .into_values()
        .find(|r| r.name.to_lowercase() == lower || r.id.to_string().starts_with(&lower))
        .map(|r| (r.id, r.name))
        .ok_or_else(|| Error::NotFound(format!("drive {spec:?} (not mounted and not in the store's registry)")))
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct SetSettings {
    /// Setting key -> value id (see [`settings::SCHEMA`]).
    pub values: indexmap::IndexMap<String, String>,
    /// Make these per-drive overrides instead of base values.
    #[serde(default)]
    pub drive: Option<String>,
}

pub fn settings_set(store: &Store, req: &SetSettings) -> Result<SettingsProfile> {
    let _lock = store.lock()?;
    let mut profile = store.settings_profile()?;
    let target = req.drive.as_deref().map(|d| override_target(store, d)).transpose()?;
    for (k, v) in &req.values {
        profile.set(target.as_ref().map(|(id, n)| (*id, n.as_str())), k, v)?;
    }
    store.save_settings_profile(&profile)?;
    Ok(profile)
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct UnsetSettings {
    pub keys: Vec<String>,
    #[serde(default)]
    pub drive: Option<String>,
}

pub fn settings_unset(store: &Store, req: &UnsetSettings) -> Result<SettingsProfile> {
    let _lock = store.lock()?;
    let mut profile = store.settings_profile()?;
    let target = req.drive.as_deref().map(|d| override_target(store, d)).transpose()?;
    for k in &req.keys {
        profile.unset(target.as_ref().map(|(id, _)| *id), k)?;
    }
    store.save_settings_profile(&profile)?;
    Ok(profile)
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct DriveSettingsView {
    pub drive: String,
    pub id: Option<Uuid>,
    pub current: settings::SettingsSnapshot,
    pub drift: Vec<settings::Drift>,
}

/// A drive's settings, and how they differ from the profile (none with no store).
pub fn settings_show(store: Option<&Store>, req: &DriveRef) -> Result<DriveSettingsView> {
    let d = resolve_one_drive(&req.drive)?;
    let current = settings::read_all(&DriveSettings(d.root()));
    let profile = store.map(Store::settings_profile).transpose()?.unwrap_or_default();
    let drift = settings::drift(&profile, d.id(), &current);
    Ok(DriveSettingsView { drive: d.name(), id: d.id(), current, drift })
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct ApplySettings {
    pub drive: String,
    #[serde(default)]
    pub dry_run: bool,
    /// Write all four files even when the profile doesn't mention their settings.
    #[serde(default)]
    pub create_missing: bool,
}

pub fn settings_apply(store: &Store, req: &ApplySettings) -> Result<settings::ApplyReport> {
    let mut d = resolve_drive(&req.drive)?;
    let profile = store.settings_profile()?;
    if req.dry_run {
        let (report, _) = settings::changes(&DriveSettings(d.root()), &profile.effective(d.id()), req.create_missing)?;
        return Ok(settings::ApplyReport { dry_run: true, ..report });
    }
    let effective = profile.effective(d.id());
    settings::apply_to_drive(&mut d, &effective, req.create_missing, &Ctx::silent())
}

/// Writing the settings profile to several drives.
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct SettingsApplyRequest {
    /// Drives, or libraries: each of a library's mounted clones (settings are each drive's own) but
    /// archive and retired ones [default: every mounted rekordbox drive but archive and retired
    /// ones].
    #[serde(default)]
    pub drives: Vec<String>,
    /// Write all four files even when the profile doesn't mention their settings.
    #[serde(default)]
    pub create_missing: bool,
}

/// What writing the profile would change on each drive.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct SettingsApplyPlan {
    pub targets: Vec<DriveSettingsPlan>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct DriveSettingsPlan {
    pub drive: String,
    pub root: PathBuf,
    pub changes: settings::ApplyReport,
}

/// One drive's part of a settings write: its report, or why it failed (the others carry on).
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct DriveSettingsResult {
    pub drive: String,
    pub root: PathBuf,
    pub report: Option<settings::ApplyReport>,
    pub error: Option<TargetError>,
}

/// What writing the settings profile to drives would change.
pub fn settings_apply_plan(store: &Store, req: &SettingsApplyRequest) -> Result<Planned<SettingsApplyPlan, bool>> {
    let profile = store.settings_profile()?;
    let (devices, left) = match req.drives.is_empty() {
        true => unnamed_targets(Some(store)),
        false => {
            let (specs, left) = expand_libraries(&req.drives)?;
            (specs.iter().map(|s| resolve_drive(s)).collect::<Result<_>>()?, left)
        }
    };
    if devices.is_empty() {
        return Err(Error::NotFound("a mounted rekordbox drive".into()));
    }
    let keys: Vec<String> = settings::SettingsFile::ALL.iter().map(|f| f.key().to_string()).collect();
    let mut plan = SettingsApplyPlan { targets: Vec::new() };
    let mut summary = Vec::new();
    let mut touched = Vec::new();
    for d in &devices {
        for f in settings::SettingsFile::ALL {
            d.checked_path(f.key())?; // PIONEER/ must not be a symlink out of the drive
        }
        let (changes, _) = settings::changes(&DriveSettings(d.root()), &profile.effective(d.id()), req.create_missing)?;
        summary.push(match changes.files_written.is_empty() {
            true => format!("{}: already as the profile has it", d.name()),
            false => {
                format!("{}: {} setting(s) in {} file(s)", d.name(), changes.changes.len(), changes.files_written.len())
            }
        });
        touched.push(Touched::drive(d, true, vec![op::Probe::Identity, op::Probe::Content(keys.clone())]));
        plan.targets.push(DriveSettingsPlan { drive: d.name(), root: d.root().into(), changes });
    }
    let noop = plan.targets.iter().all(|d| d.changes.files_written.is_empty());
    let mut planned =
        Planned::new("settings.write", plan, req.create_missing).summary(summary).warnings(left_out(&left)).noop(noop);
    for t in touched {
        planned = planned.touches(t);
    }
    planned
        .touches(Touched::path(store.root(), false, vec![op::Probe::Content(vec!["settings.json".into()])]))
        .guard(Some(store))
}

impl Operation for SettingsApplyPlan {
    /// `create_missing`.
    type Request = bool;
    type Report = Vec<DriveSettingsResult>;

    fn apply(planned: &Planned<Self, bool>, store: Option<&Store>, ctx: &Ctx<'_>) -> Result<Vec<DriveSettingsResult>> {
        let store = store.ok_or_else(|| Error::invalid("writing settings needs the store's profile"))?;
        let profile = store.settings_profile()?;
        let mut out = Vec::new();
        for (i, p) in planned.plan.targets.iter().enumerate() {
            if p.changes.files_written.is_empty() {
                continue;
            }
            let written = planned.drive(i).and_then(|mut d| {
                let effective = profile.effective(d.id());
                settings::apply_to_drive(&mut d, &effective, *planned.request(), ctx)
            });
            let (report, error) = match written {
                Ok(r) => (Some(r), None),
                Err(Error::Cancelled) => return Err(Error::Cancelled),
                Err(e) => (None, Some(TargetError::from(e))),
            };
            out.push(DriveSettingsResult { drive: p.drive.clone(), root: p.root.clone(), report, error });
        }
        Ok(out)
    }
}

/// What importing a drive's settings, or rekordbox's, would make of the profile's base. (For
/// the My Settings an account published, see [`SettingsImportPlan`].)
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct ImportSettingsPlan {
    /// The drive, or `rekordbox on this computer`.
    pub from: String,
    /// The values read.
    pub values: usize,
    pub changes: Vec<BaseChange>,
}

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct ImportSettings {
    /// Drive to import from; `None` imports rekordbox's own "My Settings" on this computer.
    #[serde(default)]
    pub drive: Option<String>,
}

/// What importing settings into the profile's base would change.
pub fn settings_import_plan(
    store: &Store,
    req: &ImportSettings,
) -> Result<Planned<ImportSettingsPlan, ImportSettings>> {
    let (from, snap, source) = import_source(req)?;
    let mut after = store.settings_profile()?;
    let before = after.base.clone();
    let values = after.import_base(&snap);
    let changes: Vec<BaseChange> = after
        .base
        .iter()
        .filter(|(k, v)| before.get(*k) != Some(*v))
        .map(|(k, v)| BaseChange { key: k.clone(), from: before.get(k).cloned(), to: Some(v.clone()) })
        .collect();
    let line = format!("set {} of the profile's {values} base value(s) from {from}", changes.len());
    let noop = changes.is_empty();
    let plan = ImportSettingsPlan { from, values, changes };
    Ok(Planned::new("settings.import", plan, req.clone()).line(line).noop(noop).touches(source).touches(Touched::path(
        store.root(),
        true,
        vec![op::Probe::Content(vec!["settings.json".into()])],
    )))
}

impl Operation for ImportSettingsPlan {
    type Request = ImportSettings;
    type Report = SettingsProfile;

    fn apply(planned: &Planned<Self, ImportSettings>, store: Option<&Store>, _: &Ctx<'_>) -> Result<SettingsProfile> {
        let store = store.ok_or_else(|| Error::invalid("importing settings needs the store"))?;
        settings_import(store, planned.request())
    }
}

/// Where imported settings come from: its name, its values, and what to watch of it.
fn import_source(req: &ImportSettings) -> Result<(String, settings::SettingsSnapshot, Touched)> {
    let keys = |f: fn(settings::SettingsFile) -> &'static str| {
        settings::SettingsFile::ALL.iter().map(|s| f(*s).to_string()).collect::<Vec<_>>()
    };
    let (from, snap, touched) = match &req.drive {
        Some(spec) => {
            let d = resolve_drive(spec)?;
            let touched = Touched::drive(&d, false, vec![op::Probe::Content(keys(settings::SettingsFile::key))]);
            (d.name(), settings::read_all(&DriveSettings(d.root())), touched)
        }
        None => {
            let dir = settings::rekordbox_settings_dir()
                .ok_or_else(|| Error::NotFound("rekordbox's settings folder on this computer".into()))?;
            let touched = Touched::path(&dir, false, vec![op::Probe::Content(keys(settings::SettingsFile::file_name))]);
            ("rekordbox on this computer".to_string(), settings::read_all(&FlatSettings(&dir)), touched)
        }
    };
    if snap.values.is_empty() {
        return Err(Error::NotFound("no settings files found to import".into()));
    }
    Ok((from, snap, touched))
}

/// Set the profile's base values from a drive's settings, or rekordbox's own, keeping the ones
/// they don't have (planned and applied in one go; see [`settings_import_plan`]).
pub fn settings_import(store: &Store, req: &ImportSettings) -> Result<SettingsProfile> {
    let (_, snap, _) = import_source(req)?;
    let _lock = store.lock()?;
    let mut profile = store.settings_profile()?;
    profile.import_base(&snap);
    store.save_settings_profile(&profile)?;
    Ok(profile)
}

// ---------------------------------------------------------------------------------------------
// Artwork
// ---------------------------------------------------------------------------------------------

pub use crate::query::TrackSelector;

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct SetArtwork {
    pub drive: String,
    /// Image file, or an audio file whose embedded art to use.
    pub image: PathBuf,
    #[serde(flatten)]
    pub tracks: TrackSelector,
}

/// What setting artwork on tracks would do: the edit [`Edit::SetArtwork`] (see
/// [`crate::artwork`]), planned as `edit.plan` plans it.
pub fn artwork_set_plan(req: &SetArtwork, ctx: &Ctx<'_>) -> Result<Planned<EditPlan>> {
    let d = resolve_drive(&req.drive)?;
    let set = Edit::SetArtwork { tracks: req.tracks.clone(), image: artwork::ImageSource::Path(req.image.clone()) };
    editing::planned_edit(
        "art.set",
        edit::plan(&d, &[set], &EditOptions::default(), ctx)?,
        Touched::library(&d, true),
        ctx,
    )
}

/// Plan and set artwork in one go, for callers that don't confirm (see [`artwork_set_plan`]).
pub fn artwork_set(req: &SetArtwork, ctx: &Ctx<'_>) -> Result<EditReport> {
    artwork_set_plan(req, ctx)?.apply(None, ctx)
}

pub fn artwork_missing(req: &DriveRef) -> Result<Vec<TrackRef>> {
    let d = resolve_drive(&req.drive)?;
    Ok(artwork::missing(&d, &d.library()?))
}

/// Artwork found for tracks that have none: embedded in their audio, or an image next to it.
pub fn artwork_auto_plan(req: &DriveRef) -> Result<Planned<artwork::AutoArtworkPlan>> {
    let d = resolve_drive(&req.drive)?;
    let plan = artwork::plan_auto(&d, &d.library()?);
    let line = format!("set artwork on {} track(s) of {}", plan.found.len(), d.name());
    let noop = plan.found.is_empty();
    Planned::new("art.auto", plan, ())
        .line(line)
        .noop(noop)
        .touches(Touched::library(&d, true))
        .reaching_clones(&Ctx::silent())?
        .guard(None)
}

impl Operation for artwork::AutoArtworkPlan {
    type Request = ();
    type Report = artwork::ArtworkReport;

    fn apply(planned: &Planned<Self>, _: Option<&Store>, ctx: &Ctx<'_>) -> Result<artwork::ArtworkReport> {
        artwork::apply_auto(&mut planned.drive(0)?, &planned.plan, ctx)
    }
}

/// Plan and apply missing artwork in one go, for callers that don't confirm.
pub fn artwork_auto_apply(req: &DriveRef, ctx: &Ctx<'_>) -> Result<artwork::ArtworkReport> {
    artwork_auto_plan(req)?.apply(None, ctx)
}
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct GetArtwork {
    pub drive: String,
    pub track: u32,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct ArtworkImage {
    pub mime: &'static str,
    pub base64: String,
}

pub fn artwork_get(req: &GetArtwork) -> Result<Option<ArtworkImage>> {
    let d = resolve_drive(&req.drive)?;
    let bytes = artwork::read_from_drive(&d, &d.library()?, req.track)?;
    Ok(bytes.map(|b| ArtworkImage { mime: "image/jpeg", base64: BASE64_STANDARD.encode(&b) }))
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct EmbedArtwork {
    pub files: Vec<PathBuf>,
    pub image: PathBuf,
}

/// A file art would be embedded in.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct EmbedTarget {
    pub path: PathBuf,
    pub size: u64,
    /// It has a front cover already, which is replaced.
    pub replaces_art: bool,
}

/// What embedding art in files would rewrite.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct EmbedPlan {
    pub image: PathBuf,
    pub files: Vec<EmbedTarget>,
    /// Files left alone, and why (on a rekordbox export, missing...).
    pub skipped: Vec<Skip>,
    /// Kept in the store before they're rewritten, so `art unembed` can put them back.
    pub backup_bytes: u64,
}

pub fn artwork_embed_plan(req: &EmbedArtwork) -> Result<Planned<EmbedPlan>> {
    Artwork::from_path(&req.image)?;
    let mut files = Vec::new();
    let mut skipped = Vec::new();
    for f in &req.files {
        // As the file really is: a relative path's ancestors stop at the working directory, and a
        // symlink can lead onto a drive.
        let real = f.canonicalize().unwrap_or_else(|_| f.clone());
        if real.ancestors().any(crate::paths::is_export_root) {
            skipped.push(Skip::new(
                f.display().to_string(),
                "file is on a rekordbox export; set the drive's artwork instead",
            ));
            continue;
        }
        match std::fs::metadata(f) {
            Ok(m) if m.is_file() => {
                let replaces_art = artwork::read_embedded(f).ok().flatten().is_some();
                files.push(EmbedTarget { path: f.clone(), size: m.len(), replaces_art });
            }
            Ok(_) => skipped.push(Skip::new(f.display().to_string(), "not a file")),
            Err(e) => skipped.push(Skip::new(f.display().to_string(), e.to_string())),
        }
    }
    let backup_bytes = files.iter().map(|f| f.size).sum();
    let mut planned =
        Planned::new("art.embed", EmbedPlan { image: req.image.clone(), files, skipped, backup_bytes }, ())
            .destructive(true)
            .touches(Touched::path(&req.image, false, vec![op::Probe::Content(vec![String::new()])]));
    planned.summary = vec![format!(
        "embed {} in {} file(s) on this computer (a copy of each, {}, is kept in the store first)",
        req.image.display(),
        planned.plan.files.len(),
        bytes(planned.plan.backup_bytes)
    )];
    planned.warnings = planned.plan.skipped.iter().map(|s| format!("{}: {}", s.path, s.reason)).collect();
    planned.noop = planned.plan.files.is_empty();
    let stamps: Vec<Touched> = planned
        .plan
        .files
        .iter()
        .map(|f| Touched::path(&f.path, true, vec![op::Probe::Stamp(vec![String::new()])]))
        .collect();
    for t in stamps {
        planned = planned.touches(t);
    }
    Ok(planned)
}

impl Operation for EmbedPlan {
    type Request = ();
    type Report = artwork::EmbedReport;

    fn apply(planned: &Planned<Self>, store: Option<&Store>, ctx: &Ctx<'_>) -> Result<artwork::EmbedReport> {
        let store = store.ok_or_else(|| Error::invalid("embedding art keeps the files' copies in the store"))?;
        let art = Artwork::from_path(&planned.plan.image)?;
        let files: Vec<PathBuf> = planned.plan.files.iter().map(|f| f.path.clone()).collect();
        let mut report = artwork::embed_with_backup(store, &files, &art, &planned.plan.image, ctx)?;
        report.failed.extend(planned.plan.skipped.iter().map(|s| Failure::new(s.path.clone(), &s.reason)));
        Ok(report)
    }
}

/// Embed art in audio files on this computer, keeping copies first (planned and applied in one
/// go; see [`artwork_embed_plan`]).
pub fn artwork_embed(store: &Store, req: &EmbedArtwork) -> Result<artwork::EmbedReport> {
    artwork_embed_plan(req)?.apply(Some(store), &Ctx::silent())
}

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct UnembedRequest {
    /// Which embed to take back [default: the latest].
    #[serde(default)]
    pub backup: Option<String>,
}

/// What taking embedded art back out would do.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct UnembedPlan {
    pub backup: artwork::EmbedBackup,
    /// Files put back as they were.
    pub restored: Vec<PathBuf>,
    /// Files changed since the art went in (or gone): left as they are.
    pub skipped: Vec<Skip>,
}

pub fn artwork_unembed_plan(store: &Store, req: &UnembedRequest) -> Result<Planned<UnembedPlan>> {
    let backups = artwork::embed_backups(store)?;
    let backup = match &req.backup {
        Some(id) => backups.into_iter().find(|b| &b.id == id).ok_or_else(|| Error::NotFound(format!("embed {id}")))?,
        None => backups.into_iter().next().ok_or_else(|| Error::NotFound("no embedded art to take back".into()))?,
    };
    let (mut restored, mut skipped) = (Vec::new(), Vec::new());
    for f in &backup.files {
        match std::fs::metadata(&f.path) {
            Ok(m) if m.len() == f.size && fsutil::mtime_eq(fsutil::mtime_secs(&m), f.mtime) => {
                restored.push(f.path.clone())
            }
            Ok(_) => skipped.push(Skip::new(f.path.display().to_string(), "changed since the art went in")),
            Err(_) => skipped.push(Skip::new(f.path.display().to_string(), "gone")),
        }
    }
    let line = format!("put back {} file(s) as they were before embed {}", restored.len(), backup.id);
    let warnings: Vec<String> = skipped.iter().map(|s| format!("{}: {}", s.path, s.reason)).collect();
    let touched: Vec<Touched> =
        restored.iter().map(|f| Touched::path(f, true, vec![op::Probe::Stamp(vec![String::new()])])).collect();
    let noop = restored.is_empty();
    let mut planned = Planned::new("art.unembed", UnembedPlan { backup, restored, skipped }, ())
        .line(line)
        .warnings(warnings)
        .destructive(true)
        .noop(noop);
    for t in touched {
        planned = planned.touches(t);
    }
    Ok(planned)
}

impl Operation for UnembedPlan {
    type Request = ();
    type Report = artwork::UnembedReport;

    fn apply(planned: &Planned<Self>, store: Option<&Store>, _: &Ctx<'_>) -> Result<artwork::UnembedReport> {
        let store = store.ok_or_else(|| Error::invalid("the files' copies are in the store"))?;
        artwork::restore_embedded(store, Some(&planned.plan.backup.id))
    }
}

// ---------------------------------------------------------------------------------------------
// Formatting
// ---------------------------------------------------------------------------------------------

/// What a format must never erase, nor an eject take away: the store at `store_root` (whether or
/// not it opens, or exists yet), the one [`set_store_root`] named if that's another, and this
/// program.
fn protected_paths(store_root: Option<&Path>) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = store_root.map(Path::to_path_buf).into_iter().collect();
    out.extend(default_store_root().filter(|r| Some(r.as_path()) != store_root));
    out.extend(std::env::current_exe().ok());
    // As the OS reports mount points (`E:\`, not `\\?\E:\`; `/private/var`, not `/var`), or none
    // would ever match: resolved as far as the path exists, as the store may not yet.
    out.iter().map(|p| fsutil::resolved(p)).collect()
}

/// External and removable disks, how players get on with each as it is, and whether it can be
/// formatted (one holding the store at `store_root` can't be).
pub fn list_disks(store_root: &Path) -> Result<Vec<DiskView>> {
    device::disks_shown()?;
    format::list(&protected_paths(Some(store_root)))
}

/// A disk identifier, a mounted drive by path, or an adopted drive by name or id (never by a
/// volume label, see [`resolve`]).
fn find_disk(spec: &str) -> Result<format::Disk> {
    device::disks_shown()?;
    if let Some(d) = format::disk_by_id(spec)? {
        return Ok(d);
    }
    format::disk_at(resolve(spec, false, false)?.root())
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct FormatRequest {
    /// A mounted drive (path, or an adopted drive's name or id) or a disk (`disk4`, `/dev/sdb`,
    /// `PhysicalDrive2`).
    pub drive: String,
    #[serde(default, flatten)]
    pub options: FormatOptions,
}

/// What erasing and formatting a disk would do. Applying it erases that disk and no other: the
/// plan's `identity` must still match. A disk holding the store at `store_root` is refused
/// (whether or not the store opens), as is the one holding this program. Only a store that's
/// there is read, to say whether a library on the disk is in it; none is made.
pub fn format_plan(
    store_root: &Path,
    req: &FormatRequest,
    ctx: &Ctx<'_>,
) -> Result<Planned<FormatPlan, FormatRequest>> {
    plan_format(store_root, find_disk(&req.drive)?, req, ctx)
}

/// [`format_plan`] for the disk found.
fn plan_format(
    store_root: &Path,
    disk: format::Disk,
    req: &FormatRequest,
    ctx: &Ctx<'_>,
) -> Result<Planned<FormatPlan, FormatRequest>> {
    let store = Store::open(store_root).ok();
    let plan = format::plan(disk, &req.options, store.as_ref(), &protected_paths(Some(store_root)), ctx)?;
    let mut summary = vec![format!("erase {} (everything on it)", plan.disk.describe())];
    summary.extend(plan.steps.iter().cloned());
    let warnings = plan.warnings.clone();
    Ok(Planned::new("format", plan, req.clone()).summary(summary).warnings(warnings).destructive(true))
}

impl Operation for FormatPlan {
    type Request = FormatRequest;
    type Report = FormatReport;

    /// What the plan was made to protect stays protected, whatever `store` is now.
    fn apply(planned: &Planned<Self, FormatRequest>, store: Option<&Store>, ctx: &Ctx<'_>) -> Result<FormatReport> {
        let req = planned.request();
        let mut protected = planned.plan.protected.clone();
        if let Some(s) = store {
            protected.extend(protected_paths(Some(s.root())));
        }
        let disk = find_disk(&req.drive)?;
        format::apply(disk, &planned.plan.identity, &req.options, store, &protected, ctx)
    }
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct FormatApply {
    pub drive: String,
    /// `identity` from the plan the user confirmed: the format is refused if the disk changed.
    pub identity: String,
    #[serde(default, flatten)]
    pub options: FormatOptions,
}

/// Erase and format a disk. Destroys everything on it. Refused for the disk holding the store at
/// `store_root`, as [`format_plan`] is.
pub fn format_apply(store_root: &Path, req: &FormatApply, ctx: &Ctx<'_>) -> Result<FormatReport> {
    let store = Store::open(store_root).ok();
    format::apply(
        find_disk(&req.drive)?,
        &req.identity,
        &req.options,
        store.as_ref(),
        &protected_paths(Some(store_root)),
        ctx,
    )
}

// ---------------------------------------------------------------------------------------------
// XML
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct ExportXml {
    pub drive: String,
    pub output: PathBuf,
    #[serde(default)]
    pub location_root: Option<PathBuf>,
    #[serde(default)]
    pub skip_analysis: bool,
    /// Only these playlists and folders, and their tracks.
    #[serde(default)]
    pub only: Vec<String>,
    /// Replace a file already at `output` (refused otherwise).
    #[serde(default)]
    pub replace: bool,
}

pub fn export_xml(req: &ExportXml, ctx: &Ctx<'_>) -> Result<XmlReport> {
    fsutil::refuse_to_replace(&[&req.output], req.replace)?;
    let d = resolve_drive(&req.drive)?;
    let opts = XmlOptions {
        location_root: req.location_root.clone(),
        skip_analysis: req.skip_analysis,
        only: req.only.clone(),
    };
    let (text, report) = xml::export(&d, &opts, ctx)?;
    fsutil::write_atomic(&req.output, text.as_bytes())?;
    Ok(report)
}

mod drives;
mod editing;
pub use drives::*;
pub use editing::*;
mod cues;
pub use cues::*;

#[cfg(test)]
mod tests;

mod tags;
pub use tags::*;
mod check;
mod diff;
mod doctor;
mod export;
mod mix;
mod plays;
mod sessions;
mod sets;
pub use check::*;
pub use diff::*;
pub use doctor::*;
pub use export::*;
pub use mix::*;
pub use plays::*;
pub use sessions::*;
pub use sets::*;
mod profile;
pub use profile::*;
#[cfg(feature = "account")]
mod account;
#[cfg(feature = "account")]
pub use account::*;
mod restore;
mod sound;
pub use sound::*;
mod undo;
pub use restore::*;
pub use undo::*;
mod snapshots;
pub use snapshots::*;
mod collection;
pub use collection::*;
#[cfg(feature = "account")]
mod publish;
#[cfg(feature = "account")]
pub use publish::*;
mod autopull;
pub use autopull::*;
mod cache;
pub use cache::*;
mod reads;
pub use reads::*;
mod jobs;
pub use jobs::*;
mod tui;
pub use tui::*;
mod sidecar;
pub use sidecar::*;
