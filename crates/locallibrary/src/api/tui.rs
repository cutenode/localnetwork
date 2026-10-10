//! What the terminal player needed that front ends had to do without: plans for saving a drive
//! in the store and for ejecting drives, so they can be shown and confirmed like every other
//! operation, and a drive's My Tags by track, to show them beside its tracks.

use rustc_hash::FxHashMap;
use std::path::{Path, PathBuf};

use schemars::JsonSchema;
use serde::Serialize;

use super::{DriveRef, EjectDrives, PullRequest, resolve_drive, resolve_one_drive};
use crate::compare::{self, Relation, Side};
use crate::device::Device;
use crate::eject::{self, EjectReport};
use crate::op::{Operation, Planned, Probe, Touched};
use crate::progress::Ctx;
use crate::store::{SnapshotInfo, Store};
use crate::sync::PullReport;
use crate::{Error, Result};

// ---------------------------------------------------------------------------------------------
// Pull
// ---------------------------------------------------------------------------------------------

/// What saving a drive in the store (`pull`) would do.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct PullPlan {
    pub drive: String,
    pub root: PathBuf,
    /// The library it's saved into.
    pub library: String,
    /// The store has no such library yet (or no store at all): pulling starts it.
    pub new_library: bool,
    /// The library's latest snapshot (left) against the drive (right): `right_newer` means the
    /// drive holds changes the store lacks, `identical` that it's saved already. `None` when the
    /// library has no snapshot yet.
    pub relation: Option<Relation>,
    /// The library's latest snapshot.
    pub head: Option<SnapshotInfo>,
}

/// What pulling needs besides the plan: the request, and the store's folder (made when there
/// isn't a store yet).
#[derive(Debug, Clone)]
pub struct PullApply {
    pub request: PullRequest,
    pub store_root: PathBuf,
}

/// What pulling a drive into the store at `store_root` would do: which library it goes into,
/// and whether the drive holds anything the store lacks. Reads the drive (its databases are
/// compared with the latest snapshot's), but writes nothing; applying pulls as [`super::pull`].
pub fn pull_plan(store_root: &Path, req: &PullRequest, ctx: &Ctx<'_>) -> Result<Planned<PullPlan, PullApply>> {
    let d = resolve_drive(&req.drive)?;
    if !d.has_export() && !d.has_library_plus() {
        return Err(Error::NotAnExport(d.root().to_path_buf()));
    }
    let store = Store::open(store_root).ok();
    let linked = d.state().and_then(|s| s.library_id);
    let existing = match (&store, &req.library, linked) {
        (None, _, _) => None,
        (Some(s), Some(name), _) => s.find_library(name)?,
        (Some(s), None, Some(id)) => s.library_by_id(id)?,
        (Some(_), None, None) => None,
    };
    if let (Some(name), None, false) = (&req.library, &existing, req.new_library) {
        return Err(Error::NotFound(format!("library {name:?} in the store; pull with new_library to start it")));
    }
    let (mut relation, mut head) = (None, None);
    if let (Some(store), Some(lib)) = (&store, &existing)
        && let Some(id) = lib.head
    {
        let snap = store.load_snapshot(&id)?;
        let left = Side::from_snapshot(store, &snap, "store")?;
        relation = Some(compare::compare(&left, &Side::from_device(&d, ctx)?).relation);
        head = Some(snap.info());
    }
    let name = d.name();
    let library = match (&existing, &req.library) {
        (Some(l), _) => l.name.clone(),
        (None, Some(n)) => n.clone(),
        (None, None) => name.clone(),
    };
    let summary = match (&existing, relation) {
        (None, _) => format!("save {name} in the store, starting the library {library}"),
        (Some(_), None) => format!("save {name} as the first snapshot of {library}"),
        (Some(_), Some(Relation::Identical)) => format!("{name} is saved in the store already ({library})"),
        (Some(_), Some(Relation::RightNewer)) => format!("save {name}'s new changes as a snapshot of {library}"),
        (Some(_), Some(_)) => format!("save {name} as a snapshot of {library}"),
    };
    let mut warnings = Vec::new();
    match relation {
        Some(Relation::LeftNewer) => warnings.push(format!(
            "the store holds a newer state of {library} than {name}: a pull is refused unless forced (push or sync the drive instead)"
        )),
        Some(Relation::Diverged) => warnings.push(format!(
            "{name} and the store's latest snapshot both changed since they were last the same: a pull may be refused (merge or sync first)"
        )),
        _ => {}
    }
    let plan = PullPlan {
        drive: name,
        root: d.root().to_path_buf(),
        library,
        new_library: existing.is_none(),
        relation,
        head,
    };
    let apply = PullApply { request: req.clone(), store_root: store_root.to_path_buf() };
    // Pulling records the drive's state in its .locallibrary folder, never its library: an
    // archive drive can be pulled.
    Planned::new("pull", plan, apply)
        .line(summary)
        .warnings(warnings)
        .noop(relation == Some(Relation::Identical))
        .touches(Touched::drive(&d, false, vec![Probe::Databases, Probe::Identity]))
        .guard(store.as_ref())
}

impl Operation for PullPlan {
    type Request = PullApply;
    type Report = PullReport;

    fn apply(planned: &Planned<Self, PullApply>, store: Option<&Store>, ctx: &Ctx<'_>) -> Result<PullReport> {
        let r = planned.request();
        let mut req = r.request.clone();
        req.drive = planned.plan.root.display().to_string();
        match store {
            Some(s) if s.root() == r.store_root => super::pull(s, &req, ctx),
            _ => super::pull(&Store::open_or_init(&r.store_root)?, &req, ctx),
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Eject
// ---------------------------------------------------------------------------------------------

/// A drive ejecting would unmount and power off.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct EjectTarget {
    pub drive: String,
    pub root: PathBuf,
    /// The other volumes mounted from its disk: ejecting unmounts them too.
    pub also_unmounts: Vec<PathBuf>,
    /// The commands that eject it.
    pub commands: Vec<String>,
}

impl EjectTarget {
    /// ` (and /Volumes/X, on the same disk)` for the other volumes ejecting it unmounts, if any.
    pub(super) fn also(&self) -> String {
        let also: Vec<String> = self.also_unmounts.iter().map(|m| m.display().to_string()).collect();
        if also.is_empty() { String::new() } else { format!(" (and {}, on the same disk)", also.join(", ")) }
    }
}

/// The drive `spec` names, and what ejecting it would do. Refused for a drive that isn't the root
/// of a volume mounted from a disk (a folder holding an export: there's nothing to eject, and the
/// commands would act on the disk the folder is on), and for one whose disk also holds the store
/// (at `store_root`) or this program, which ejecting would take away from under them.
pub(super) fn eject_target(spec: &str, store_root: Option<&Path>) -> Result<(Device, EjectTarget)> {
    let d = resolve_one_drive(spec)?;
    let Some((disk, volume)) = crate::doctor::volume_of(d.root()) else {
        return Err(Error::invalid(format!(
            "{} isn't the root of a volume mounted from a disk: there's nothing to eject",
            d.name()
        )));
    };
    if let Some((mount, p)) = disk.holding(&super::protected_paths(store_root)) {
        return Err(Error::invalid(format!(
            "ejecting {} would unmount {}, which holds {}: eject it from the system once nothing uses it",
            d.name(),
            mount.display(),
            p.display()
        )));
    }
    let also_unmounts = disk.volumes.iter().filter(|v| v.id != volume.id).filter_map(|v| v.mount.clone()).collect();
    let commands = eject::commands(d.root())?.iter().map(|c| c.display()).collect();
    let target = EjectTarget { drive: d.name(), root: d.root().to_path_buf(), also_unmounts, commands };
    Ok((d, target))
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct EjectPlan {
    pub drives: Vec<EjectTarget>,
}

/// What ejecting drives would do: the commands that unmount and power off each (see
/// [`crate::eject`]), and the other volumes of its disk that go with it. Applying ejects them as
/// [`super::drive_eject`] does, recording the eject in each drive's logbook. Refused for a drive
/// that isn't the root of a volume mounted from a disk, and for one whose disk also holds the
/// store (at `store_root`) or this program.
pub fn drive_eject_plan(store_root: Option<&Path>, req: &EjectDrives) -> Result<Planned<EjectPlan, EjectDrives>> {
    if req.drives.is_empty() {
        return Err(Error::invalid("name a drive to eject"));
    }
    let mut drives = Vec::new();
    let mut touched = Vec::new();
    for spec in &super::expand_libraries(&req.drives)?.0 {
        let (d, target) = eject_target(spec, store_root)?;
        drives.push(target);
        touched.push(Touched::drive(&d, false, vec![Probe::Identity]));
    }
    let summary: Vec<String> =
        drives.iter().map(|t| format!("eject {}: unmount it{} and power it off", t.drive, t.also())).collect();
    // Named by mount path from here on: a name could find another drive by then.
    let request = EjectDrives { drives: drives.iter().map(|t| t.root.display().to_string()).collect() };
    let mut planned = Planned::new("eject", EjectPlan { drives }, request)
        .summary(summary)
        .warnings(crate::format::untested_here("ejecting"));
    for t in touched {
        planned = planned.touches(t);
    }
    Ok(planned)
}

impl Operation for EjectPlan {
    type Request = EjectDrives;
    type Report = Vec<EjectReport>;

    fn apply(planned: &Planned<Self, EjectDrives>, store: Option<&Store>, _: &Ctx<'_>) -> Result<Vec<EjectReport>> {
        super::drive_eject(store, planned.request())
    }
}

// ---------------------------------------------------------------------------------------------
// My Tags
// ---------------------------------------------------------------------------------------------

/// Each tagged track's My Tags (`Category/Tag`, in display order) by the track's file key in
/// lower case, from both databases (see [`crate::tags::by_track`]).
pub fn tags_by_track(req: &DriveRef) -> Result<FxHashMap<String, Vec<String>>> {
    crate::tags::by_track(&resolve_drive(&req.drive)?)
}
