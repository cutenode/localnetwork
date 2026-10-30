//! Writing what an analyzer worked out besides its columns to the tracks' analysis files
//! (`plugins.apply_analysis`, feature `analysis`): its beat grid, phrases and waveform become an
//! ordinary edit plan (each track's [`Edit::SetAnalysis`], with its BPM), shown and confirmed as
//! any is, and journaled. The plugin isn't run: they're read from the
//! [`crate::fields::overlay`] store, as it kept them.

use base64::prelude::*;
use loclib_plugin::FieldType;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::Plugins;
use crate::analysis_edit::{self, NewAnalysis, WaveSource};
use crate::anlz::{self, Beat};
use crate::api;
use crate::edit::{self, Edit, EditOptions, EditPlan};
use crate::fields::{Analyzer, OverlayStore};
use crate::model::Track;
use crate::op::{Planned, Touched};
use crate::progress::Ctx;
use crate::store::Store;
use crate::{Error, Result, paths};

/// A part of a track's analysis an analyzer can work out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AnalysisPart {
    /// The beat grid (a `beats` field).
    Grid,
    /// The phrases (a `phrases` field).
    Phrases,
    /// The waveform (a `waveform` field).
    Waveform,
}

impl AnalysisPart {
    pub const ALL: [AnalysisPart; 3] = [AnalysisPart::Grid, AnalysisPart::Phrases, AnalysisPart::Waveform];

    /// The type of the field it's in.
    pub fn kind(self) -> FieldType {
        match self {
            AnalysisPart::Grid => FieldType::Beats,
            AnalysisPart::Phrases => FieldType::Phrases,
            AnalysisPart::Waveform => FieldType::Waveform,
        }
    }

    /// As `--apply` names it.
    pub fn name(self) -> &'static str {
        match self {
            AnalysisPart::Grid => "grid",
            AnalysisPart::Phrases => "phrases",
            AnalysisPart::Waveform => "waveform",
        }
    }
}

/// What to write to a drive's tracks' analysis files, from what one analyzer kept.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct ApplyAnalysis {
    pub plugin: String,
    pub analyzer: String,
    /// The library or drive whose tracks to write to.
    pub drive: String,
    /// What to write, which the analyzer must work out: `grid`, `phrases`, `waveform`.
    pub parts: Vec<AnalysisPart>,
    /// Only these tracks, by id [default: every one].
    #[serde(default)]
    pub tracks: Option<Vec<u32>>,
    /// Also over what a track's analysis files have: without it, only where they have none.
    #[serde(default)]
    pub overwrite: bool,
}

/// Whether two grids are the same beats (to a millisecond): phrases count one's beats as the
/// other's.
pub fn same_grid(a: &[Beat], b: &[Beat]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| x.time_ms.abs_diff(y.time_ms) <= 1)
}

impl Plugins {
    /// Plans writing the beat grid, phrases and waveform (`req.parts`) an analyzer worked out to
    /// the analysis files of the tracks of the drive `req.drive` names, as an edit plan
    /// (`plugins.apply_analysis`): one `SetAnalysis` per track that would change, a grid with the
    /// track's BPM. A track keeps what its files have unless `req.overwrite`; phrases counting
    /// another grid's beats than the one the track ends up with are left out; tracks the analyzer
    /// hasn't worked them out for yet, and those whose files can't take them (none, or
    /// rekordbox's colour waveforms), are left out with a warning.
    pub fn plan_apply_analysis(&self, req: &ApplyAnalysis, store: &Store, ctx: &Ctx<'_>) -> Result<Planned<EditPlan>> {
        let slot = self.slot(&req.plugin)?;
        let declared = slot.declared("analyzer", &req.analyzer, &slot.manifest.contributes.analyzers, |a| &a.name)?;
        if req.parts.is_empty() {
            return Err(Error::invalid("say what to write: grid, phrases, waveform"));
        }
        if let Some(part) = req.parts.iter().find(|p| !declared.fields.values().any(|t| *t == p.kind())) {
            return Err(Error::invalid(format!("{} works out no {}", declared.title, part.name())));
        }
        let device = api::resolve_drive(&req.drive)?;
        let library = device.library()?;
        let selected: Vec<&Track> = match &req.tracks {
            Some(ids) => super::tracks_named(&library, ids)?,
            None => library.tracks.iter().collect(),
        };
        let analyzer =
            Analyzer { plugin: req.plugin.clone(), analyzer: req.analyzer.clone(), version: declared.version.clone() };
        let overlays = OverlayStore::in_store(store);
        let wants = |p: AnalysisPart| req.parts.contains(&p);
        let (mut edits, mut unanalysed, mut had, mut other_grid) = (Vec::new(), 0, 0, 0);
        let mut refused: Vec<String> = Vec::new();
        for t in selected {
            ctx.check()?;
            let kept = device.checked_path(&paths::key(&t.file_path)).ok().and_then(|a| overlays.get(&a, &analyzer));
            let Some(o) = kept else {
                unanalysed += 1;
                continue;
            };
            let drive = anlz::load(&device, t);
            let mut a = NewAnalysis::default();
            let mut take = |wanted: bool, given: bool, has: bool| {
                let take = wanted && given && (req.overwrite || !has);
                had += usize::from(wanted && given && !take);
                take
            };
            if take(wants(AnalysisPart::Grid), o.beats.is_some(), !drive.beats.is_empty()) {
                a.beats = o.beats.clone();
            }
            let grid = a.beats.as_deref().unwrap_or(&drive.beats);
            let fits = o.beats.as_deref().is_none_or(|own| same_grid(own, grid));
            if take(wants(AnalysisPart::Phrases), o.phrases.is_some(), drive.structure.is_some()) {
                match fits {
                    true => a.phrases = o.phrases.clone(),
                    false => other_grid += 1,
                }
            }
            let bands = o.waveform.as_ref().filter(|_| {
                take(wants(AnalysisPart::Waveform), true, drive.preview.is_some() || drive.detail.is_some())
            });
            if a.is_empty() && bands.is_none() {
                continue;
            }
            match analysis_edit::fits(&device, t, &a, bands) {
                Ok((true, _)) => {}
                // As it is already.
                Ok((false, _)) => continue,
                Err(e) => {
                    refused.push(e.to_string());
                    continue;
                }
            }
            a.waveform = bands.map(|b| WaveSource::Base64(BASE64_STANDARD.encode(b.pack())));
            edits.push(Edit::SetAnalysis { track: t.file_path.clone(), analysis: a });
        }
        let plan = edit::plan(&device, &edits, &EditOptions { lenient: false, no_journal: false }, ctx)?;
        let mut planned = api::planned_edit("plugins.apply_analysis", plan, Touched::library(&device, true), ctx)?;
        let what: Vec<&str> = AnalysisPart::ALL.into_iter().filter(|p| wants(*p)).map(AnalysisPart::name).collect();
        planned
            .summary
            .insert(0, format!("write the {} from {} to {} track(s)", what.join(" and "), declared.title, edits.len()));
        let title = &declared.title;
        if unanalysed > 0 {
            planned.warnings.push(format!("{unanalysed} track(s) aren't analysed by {title} yet"));
        }
        if had > 0 {
            planned
                .warnings
                .push(format!("{had} of what's asked the tracks have already, kept: overwrite to replace it"));
        }
        if other_grid > 0 {
            planned.warnings.push(format!(
                "{other_grid} track(s)' phrases left out: they count the beats of {title}'s grid, not the track's"
            ));
        }
        if let Some(first) = refused.first() {
            planned.warnings.push(format!("{} track(s) left out, e.g. {first}", refused.len()));
        }
        Ok(planned)
    }
}
