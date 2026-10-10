//! Drive health reports and orphaned-file cleanup.

use schemars::JsonSchema;
use serde::Deserialize;

use super::{DriveRef, resolve_drive};
use crate::Result;
use crate::doctor::{self, DoctorOptions, DoctorReport, OrphanCleanReport, OrphanPlan};
use crate::op::{Operation, Planned, Probe, Touched, bytes};
use crate::progress::Ctx;
use crate::store::Store;

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct DoctorRequest {
    pub drive: String,
    #[serde(default, flatten)]
    pub options: DoctorOptions,
}

/// A drive's health report. Read-only.
pub fn doctor(req: &DoctorRequest, ctx: &Ctx<'_>) -> Result<DoctorReport> {
    doctor::run(&resolve_drive(&req.drive)?, &req.options, ctx)
}

/// Orphaned files a cleanup would delete, from the drive and (as a sync would) its clones.
pub fn doctor_orphans_plan(req: &DriveRef, ctx: &Ctx<'_>) -> Result<Planned<OrphanPlan>> {
    let d = resolve_drive(&req.drive)?;
    let plan = doctor::plan_orphans(&d, ctx)?;
    let line = format!("delete {} orphaned file(s) ({}) from {}", plan.files.len(), bytes(plan.bytes), d.name());
    let keys = plan.files.iter().map(|f| f.path.clone()).collect();
    let (noop, warnings) = (plan.is_noop(), plan.warnings.clone());
    Planned::new("doctor.orphans", plan, ())
        .line(line)
        .warnings(warnings)
        .destructive(!noop)
        .noop(noop)
        .touches(Touched::drive(&d, true, vec![Probe::Databases, Probe::Identity, Probe::Stamp(keys)]))
        .reaching_clones(ctx)?
        .guard(None)
}

impl Operation for OrphanPlan {
    type Request = ();
    type Report = OrphanCleanReport;

    fn apply(planned: &Planned<Self>, _: Option<&Store>, ctx: &Ctx<'_>) -> Result<OrphanCleanReport> {
        doctor::apply_orphans(&planned.plan, &mut planned.drive(0)?, ctx)
    }
}

/// Delete a drive's orphaned files (backed up first; `undo` takes it back), planned and applied
/// in one go.
pub fn doctor_orphans_apply(req: &DriveRef, ctx: &Ctx<'_>) -> Result<OrphanCleanReport> {
    doctor_orphans_plan(req, ctx)?.apply(None, ctx)
}
