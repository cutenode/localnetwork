//! A drive's library for other DJ software, written to a folder on this computer.

use std::path::PathBuf;

use schemars::JsonSchema;
use serde::Deserialize;

use super::resolve_drive;
use crate::Result;
use crate::interop::{self, InteropOptions, InteropPlan, Software};
use crate::op::{Operation, Planned, Probe, Touched};
use crate::progress::Ctx;
use crate::store::Store;

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct InteropRequest {
    pub drive: String,
    pub software: Software,
    /// The folder to write into (never on the drive).
    pub output: PathBuf,
    #[serde(flatten)]
    pub options: InteropOptions,
}

/// The files an export would write and what the format leaves out; writes nothing.
pub fn interop_plan(req: &InteropRequest, ctx: &Ctx<'_>) -> Result<Planned<InteropPlan, InteropRequest>> {
    let d = resolve_drive(&req.drive)?;
    let plan = interop::plan(&d, req.software, &req.output, &req.options, ctx)?;
    let line = format!(
        "write {} file(s) for {:?} into {} ({} tracks, {} playlists)",
        plan.files.len(),
        plan.software,
        req.output.display(),
        plan.tracks,
        plan.playlists
    );
    let destructive = plan.files.iter().any(|f| f.exists);
    let warnings = plan.warnings.clone();
    // From the absolute path: a relative one's last parent is empty, which never exists.
    let mut out = crate::fsutil::absolute(&req.output);
    while !out.exists() {
        match out.parent() {
            Some(p) => out = p.to_path_buf(),
            None => break,
        }
    }
    Ok(Planned::new("interop", plan, req.clone())
        .line(line)
        .warnings(warnings)
        .destructive(destructive)
        .touches(Touched::library(&d, false))
        .touches(Touched::path(&out, true, vec![Probe::Listing(String::new())])))
}

impl Operation for InteropPlan {
    type Request = InteropRequest;
    type Report = InteropPlan;

    fn apply(planned: &Planned<Self, InteropRequest>, _: Option<&Store>, ctx: &Ctx<'_>) -> Result<InteropPlan> {
        let req = planned.request();
        interop::apply(&planned.drive(0)?, req.software, &req.output, &req.options, ctx)
    }
}

/// Write the export (planned and applied in one go). Files already in the output folder are
/// replaced only with `replace`.
pub fn interop_apply(req: &InteropRequest, ctx: &Ctx<'_>) -> Result<InteropPlan> {
    interop_plan(req, ctx)?.apply(None, ctx)
}
