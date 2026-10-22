//! Plugins' operations (`op/plan`): a plugin answers edits, which the host plans as one edit plan
//! on the drive the user is on, and applies as it applies any (locked, checked against the drive
//! and the files the plugin read as planned, backed up first and journaled). The plugin never
//! writes to the drive. An image or a waveform an edit gives is a file in the request's scratch
//! folder (`{"blob": "cover.jpg"}`), which the host reads as it plans. It may answer a question for the
//! user instead (`{"choose": ...}`, the `choose` feature), after which the host plans it again with
//! what was chosen.

use std::io::Read;

use base64::prelude::*;
use loclib_plugin::{Capability, Choice, Context, OpAnswer, OpPlan, PlanOp, method};
use schemars::JsonSchema;
use serde::Serialize;
use serde_json::Value;

use super::Plugins;
use super::blobs::Scratch;
use super::process::{Live, Slot};
use crate::analysis_edit::WaveSource;
use crate::api;
use crate::artwork::ImageSource;
use crate::edit::{self, Edit, EditOptions, EditPlan};
use crate::op::{self, Planned, Probe, Touched};
use crate::progress::Ctx;
use crate::store::Store;
use crate::{Error, Result};

/// What planning a plugin's operation comes to: its plan, or a question for the user first, after
/// which it's planned again with the param set ([`Choice::answered`]).
#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
// Made once a run, and the plan taken out at once: boxing it would only make each caller unbox it.
#[allow(clippy::large_enum_variant)]
pub enum OpPlanned {
    Choose { choose: Choice },
    Plan(Planned<EditPlan>),
}

impl OpPlanned {
    /// The plan, for a caller that can't ask: a question is refused, saying how to answer it.
    pub fn plan(self) -> Result<Planned<EditPlan>> {
        match self {
            OpPlanned::Plan(plan) => Ok(plan),
            OpPlanned::Choose { choose } => Err(Error::invalid(format!(
                "{}: it asks which first, as `{}` in its params",
                choose.title, choose.param
            ))),
        }
    }
}

impl Plugins {
    /// Plans one of a plugin's operations with `params` (checked against its schema; `null` is
    /// no params; one from the selection, `x-from`, that isn't given is filled in from `context`)
    /// on the drive the user is on (`context.drive`). The plan's operation is
    /// `<plugin>.<operation>`, never a built-in one; it reads as an edit plan, with the plugin's
    /// summary first, and it's destructive when its edits are (see [`EditPlan::destructive`]),
    /// which needs the `library:delete` capability. Planned strictly: every edit must apply, it's
    /// journaled, and archive and retired drives are refused, when it's applied too. Or the
    /// question it asks, one the user can answer with a param it takes.
    pub fn plan_op(
        &self,
        plugin: &str,
        operation: &str,
        params: Value,
        context: Context,
        store: Option<&Store>,
        ctx: &Ctx<'_>,
    ) -> Result<OpPlanned> {
        let slot = self.slot(plugin)?;
        let declared = slot.declared("operation", operation, &slot.manifest.contributes.operations, |o| &o.name)?;
        let (context, device) = slot.context(context, store)?;
        let params = super::fitting(declared.params.as_ref(), params, &context)?;
        let device = device.ok_or_else(|| Error::invalid(format!("{operation} edits a drive: say which")))?;
        let op = PlanOp { operation: operation.into(), params, context };
        let planned = |answer, scratch: &Scratch, live: &Live| match answer {
            OpAnswer::Plan(plan) => Ok(OpPlanned::Plan(slot.planned(operation, plan, scratch, &device, ctx)?)),
            OpAnswer::Choose { choose } => {
                slot.askable(&choose, declared.params.as_ref(), &op.params, live)?;
                Ok(OpPlanned::Choose { choose })
            }
        };
        slot.call_with(method::PLAN_OP, &op, store, ctx, &|_| {}, planned)
    }
}

impl Slot {
    /// Whether the plugin may ask `choice` of the user, for a hook whose params `schema`
    /// describes, run with `params`: it has the `choose` feature, the question is one the hook
    /// can be run again with ([`Choice::check`]), and not about a param it was given (which
    /// would have the user answer it forever).
    pub(crate) fn askable(&self, choice: &Choice, schema: Option<&Value>, params: &Value, live: &Live) -> Result<()> {
        if !live.has("choose") {
            return Err(self.misbehaved("it asks a question without the choose feature"));
        }
        if params.get(&choice.param).is_some() {
            return Err(self.misbehaved(format!("it asks again for {}, which it was given", choice.param)));
        }
        choice.check(schema).map_err(|why| self.misbehaved(format!("its question won't do: {why}")))
    }

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
    /// capabilities grant, and giving an image or a waveform only as a file in `scratch`, which
    /// the host reads as it plans.
    fn edit(&self, i: usize, edit: Value, scratch: &Scratch) -> Result<Edit> {
        let mut edit: Edit =
            serde_json::from_value(edit).map_err(|e| self.misbehaved(format!("its edit {i} isn't one: {e}")))?;
        let needs = match edit {
            Edit::MyTag(_) => Capability::TagsPlan,
            Edit::Cues { .. } => Capability::CuesPlan,
            Edit::SetArtwork { .. } => Capability::ArtworkPlan,
            Edit::SetAnalysis { .. } => Capability::AnalysisPlan,
            _ => Capability::LibraryPlan,
        };
        if !self.manifest.capabilities.contains(&needs) {
            return Err(self.misbehaved(format!("its edit {i} needs the {needs} capability, which it doesn't have")));
        }
        // A file named in its scratch folder, read now: the plugin can't change it once it's
        // checked.
        let blob = |what: &str, given: Option<&str>| -> Result<String> {
            let Some(name) = given else {
                return Err(self.misbehaved(format!(
                    "its edit {i} gives {what} otherwise than as a file in its scratch folder ({{\"blob\": name}})"
                )));
            };
            let mut bytes = Vec::new();
            let read = scratch.blob(name).and_then(|mut f| f.read_to_end(&mut bytes).map_err(|e| e.to_string()));
            read.map_err(|why| self.misbehaved(format!("its edit {i}: {why}")))?;
            Ok(BASE64_STANDARD.encode(bytes))
        };
        match &mut edit {
            Edit::SetArtwork { image, .. } => {
                let name = match image {
                    ImageSource::Blob(name) => Some(name.as_str()),
                    _ => None,
                };
                *image = ImageSource::Base64(blob("an image", name)?);
            }
            Edit::SetAnalysis { analysis, .. } if analysis.waveform.is_some() => {
                let name = match &analysis.waveform {
                    Some(WaveSource::Blob(name)) => Some(name.as_str()),
                    _ => None,
                };
                analysis.waveform = Some(WaveSource::Base64(blob("a waveform", name)?));
            }
            _ => {}
        }
        Ok(edit)
    }
}
