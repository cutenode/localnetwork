//! Plugins' operations (`op/plan`): a plugin answers edits, which the host plans as one edit plan
//! on the drive the user is on, and applies as it applies any (locked, checked against the drive
//! and the files the plugin read as planned, backed up first and journaled). The plugin never
//! writes to the drive. An image an edit gives is a file in the request's scratch folder
//! (`{"blob": "cover.jpg"}`), which the host reads as it plans.

use std::io::Read;

use base64::prelude::*;
use loclib_plugin::{Capability, Context, OpPlan, PlanOp, method};
use serde_json::Value;

use super::Plugins;
use super::blobs::Scratch;
use super::process::Slot;
use crate::api;
use crate::artwork::ImageSource;
use crate::edit::{self, Edit, EditOptions, EditPlan};
use crate::op::{self, Planned, Probe, Touched};
use crate::progress::Ctx;
use crate::store::Store;
use crate::{Error, Result};

impl Plugins {
    /// Plans one of a plugin's operations with `params` (checked against its schema; `null` is
    /// no params; one from the selection, `x-from`, that isn't given is filled in from `context`)
    /// on the drive the user is on (`context.drive`). The plan's operation is
    /// `<plugin>.<operation>`, never a built-in one; it reads as an edit plan, with the plugin's
    /// summary first, and it's destructive when its edits are (see [`EditPlan::destructive`]),
    /// which needs the `library:delete` capability. Planned strictly: every edit must apply, it's
    /// journaled, and archive and retired drives are refused, when it's applied too.
    pub fn plan_op(
        &self,
        plugin: &str,
        operation: &str,
        params: Value,
        context: Context,
        store: Option<&Store>,
        ctx: &Ctx<'_>,
    ) -> Result<Planned<EditPlan>> {
        let slot = self.slot(plugin)?;
        let declared = slot.declared("operation", operation, &slot.manifest.contributes.operations, |o| &o.name)?;
        let (context, device) = slot.context(context, store)?;
        let params = super::fitting(declared.params.as_ref(), params, &context)?;
        let device = device.ok_or_else(|| Error::invalid(format!("{operation} edits a drive: say which")))?;
        let op = PlanOp { operation: operation.into(), params, context };
        let planned = |plan, scratch: &Scratch| slot.planned(operation, plan, scratch, &device, ctx);
        slot.call_with(method::PLAN_OP, &op, store, ctx, &|_| {}, planned)
    }
}

impl Slot {
    /// The plan of the plugin's operation, as it answered it (with the files it left in
    /// `scratch`), on `device`.
    fn planned(
        &self,
        operation: &str,
        plan: OpPlan,
        scratch: &Scratch,
        device: &crate::device::Device,
        ctx: &Ctx<'_>,
    ) -> Result<Planned<EditPlan>> {
        let OpPlan { summary, warnings, edits, inputs } = plan;
        let edits = edits.into_iter().enumerate().map(|(i, e)| self.edit(i, e, scratch)).collect::<Result<Vec<_>>>()?;
        if let Some(input) = inputs.iter().find(|f| !f.is_absolute()) {
            return Err(self.misbehaved(format!("it read {}, which isn't an absolute path", input.display())));
        }
        // Read again before it's applied: a pipe or a device could hold that up forever.
        if let Some(input) = inputs.iter().find(|f| !std::fs::metadata(f).is_ok_and(|m| m.is_file())) {
            return Err(self.misbehaved(format!("it read {}, which isn't a file", input.display())));
        }
        let planned = op::refusing_archive_writes(|| {
            let options = EditOptions { lenient: false, no_journal: false };
            let plan = edit::plan(device, &edits, &options, ctx)?;
            api::planned_edit(format!("{}.{operation}", self.manifest.id), plan, Touched::library(device, true), ctx)
        })
        // Writing to an archive drive anyway can't help: a plugin's plan never does.
        .map_err(Error::without_retries)?
        .never_archive();
        if planned.destructive && !self.manifest.capabilities.contains(&Capability::LibraryDelete) {
            return Err(self.misbehaved(format!(
                "{operation} removes from the library, which needs the {} capability",
                Capability::LibraryDelete
            )));
        }
        // Applied as planned only while the files it was worked out from stay as they were.
        let content = || vec![Probe::Content(vec![String::new()])];
        let planned = inputs.iter().fold(planned, |p, input| p.touches(Touched::path(input, false, content())));
        let mut planned = planned.warnings(warnings);
        planned.summary.insert(0, summary);
        Ok(planned)
    }

    /// The plugin's edit `i`, once it may make it: one the host can read, of a kind its
    /// capabilities grant, and giving an image only as a file in `scratch`, which the host reads
    /// as it plans.
    fn edit(&self, i: usize, edit: Value, scratch: &Scratch) -> Result<Edit> {
        let mut edit: Edit =
            serde_json::from_value(edit).map_err(|e| self.misbehaved(format!("its edit {i} isn't one: {e}")))?;
        let needs = match edit {
            Edit::MyTag(_) => Capability::TagsPlan,
            Edit::Cues { .. } => Capability::CuesPlan,
            Edit::SetArtwork { .. } => Capability::ArtworkPlan,
            _ => Capability::LibraryPlan,
        };
        if !self.manifest.capabilities.contains(&needs) {
            return Err(self.misbehaved(format!("its edit {i} needs the {needs} capability, which it doesn't have")));
        }
        if let Edit::SetArtwork { image, .. } = &mut edit {
            let ImageSource::Blob(name) = image else {
                return Err(self.misbehaved(format!(
                    "its edit {i} gives an image otherwise than as a file in its scratch folder ({{\"blob\": name}})"
                )));
            };
            let mut bytes = Vec::new();
            let read = scratch.blob(name).and_then(|mut f| f.read_to_end(&mut bytes).map_err(|e| e.to_string()));
            read.map_err(|why| self.misbehaved(format!("its edit {i}: {why}")))?;
            // Its bytes, read now: the plugin can't change the file it named once it's checked.
            *image = ImageSource::Base64(BASE64_STANDARD.encode(bytes));
        }
        Ok(edit)
    }
}
