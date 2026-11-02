//! Writing what an analyzer worked out to the tracks themselves (`plugins.apply_fields`): the
//! values of its key and BPM roles become an ordinary edit plan (each track's `EditTracks`),
//! shown and confirmed as any is, and journaled. The plugin isn't run: its values are read from
//! the [`crate::fields`] store, as it kept them.

use loclib_plugin::{FieldValue, Role};
use schemars::JsonSchema;
use serde::Deserialize;

use super::Plugins;
use crate::api;
use crate::edit::{self, Edit, EditOptions, EditPlan, TrackFields};
use crate::fields::{Analyzer, FieldStore};
use crate::harmony::Camelot;
use crate::model::Track;
use crate::op::{Planned, Touched};
use crate::progress::Ctx;
use crate::query::TrackSelector;
use crate::store::Store;
use crate::{Error, Result, paths};

/// What to write to a drive's tracks, from what one analyzer kept.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct ApplyFields {
    pub plugin: String,
    pub analyzer: String,
    /// The library or drive whose tracks to write to.
    pub drive: String,
    /// The roles to write, which the analyzer must have: `key`, `bpm` (what a track has of them).
    pub fields: Vec<Role>,
    /// Only these tracks, by id [default: every one].
    #[serde(default)]
    pub tracks: Option<Vec<u32>>,
    /// Also over a key or BPM the track has: without it, only where it has none.
    #[serde(default)]
    pub overwrite: bool,
    /// Leave out a value the analyzer is less sure of than this, 0 to 1 (its `key_confidence` or
    /// `bpm_confidence`), and one it doesn't say how sure it is of.
    #[serde(default)]
    pub min_confidence: Option<f64>,
}

/// What of `key` (in any notation rekordbox shows, `8A`) and `bpm` to write to `t`, as a key
/// role's and a BPM role's values are written: a key as rekordbox names it (`Am`), a BPM to
/// hundredths (1 to 999.99); neither when `t` has it already (a key in any notation), nor over
/// one it has unless `overwrite`. A key that isn't one is left out.
pub(crate) fn key_and_bpm(t: &Track, key: Option<&str>, bpm: Option<f64>, overwrite: bool) -> TrackFields {
    let wanted = |has: bool, same: bool| !same && (overwrite || !has);
    let key =
        key.and_then(Camelot::parse).filter(|k| wanted(!t.key.trim().is_empty(), Camelot::parse(&t.key) == Some(*k)));
    let bpm = bpm
        .map(|b| (b * 100.0).round() / 100.0)
        .filter(|b| (1.0..=999.99).contains(b) && wanted(t.bpm > 0.0, *b == t.bpm));
    TrackFields { key: key.map(|k| k.note()), bpm, ..TrackFields::default() }
}

/// The role saying how sure an analyzer is of a value of `role`'s.
fn confidence_of(role: Role) -> Role {
    match role {
        Role::Key => Role::KeyConfidence,
        _ => Role::BpmConfidence,
    }
}

impl Plugins {
    /// Plans writing the key and BPM (`req.fields`) an analyzer worked out to the tracks of the
    /// drive `req.drive` names, as an edit plan (`plugins.apply_fields`): one `EditTracks` per
    /// track that would change. A track keeps a value it has already unless `req.overwrite`, and
    /// one it has in another notation either way; a key is written as rekordbox names it (`Am`),
    /// a BPM to hundredths. Values the analyzer is less sure of than `req.min_confidence`, and
    /// tracks it hasn't analysed yet, are left out with a warning.
    pub fn plan_apply_fields(&self, req: &ApplyFields, store: &Store, ctx: &Ctx<'_>) -> Result<Planned<EditPlan>> {
        let slot = self.slot(&req.plugin)?;
        let declared = slot.declared("analyzer", &req.analyzer, &slot.manifest.contributes.analyzers, |a| &a.name)?;
        if req.fields.is_empty() {
            return Err(Error::invalid("say what to write: key, bpm"));
        }
        if let Some(role) = req.fields.iter().find(|r| !matches!(r, Role::Key | Role::Bpm)) {
            return Err(Error::invalid(format!("a track has no {} to write: key or bpm", role.name())));
        }
        if let Some(role) = req.fields.iter().find(|r| !declared.roles.contains_key(*r)) {
            return Err(Error::invalid(format!("{} works out no {}", declared.title, role.name())));
        }
        let device = api::resolve_drive(&req.drive)?;
        let library = device.library()?;
        let selected: Vec<&Track> = match &req.tracks {
            Some(ids) => super::tracks_named(&library, ids)?,
            None => library.tracks.iter().collect(),
        };
        let analyzer =
            Analyzer { plugin: req.plugin.clone(), analyzer: req.analyzer.clone(), version: declared.version.clone() };
        let fields = FieldStore::in_store(store);
        let (mut edits, mut unanalysed, mut unsure) = (Vec::new(), 0, 0);
        for t in selected {
            let kept = device
                .checked_path(&paths::key(&t.file_path))
                .ok()
                .map(|audio| fields.get(&audio, std::slice::from_ref(&analyzer)));
            let Some(values) = kept.as_ref().and_then(|k| k.get(&req.plugin)?.get(&req.analyzer)) else {
                unanalysed += 1;
                continue;
            };
            let value = |role: Role| values.get(declared.roles.get(&role)?);
            let number = |role: Role| match value(role)? {
                FieldValue::Number(n) => Some(*n),
                FieldValue::Integer(n) => Some(*n as f64),
                _ => None,
            };
            let sure =
                |role: Role| req.min_confidence.is_none_or(|min| number(confidence_of(role)).is_some_and(|c| c >= min));
            let asked = |role: Role| req.fields.contains(&role);
            let key = match value(Role::Key) {
                Some(FieldValue::String(code)) if asked(Role::Key) => Some(code.as_str()),
                _ => None,
            };
            let mut set = key_and_bpm(t, key, number(Role::Bpm).filter(|_| asked(Role::Bpm)), req.overwrite);
            for (role, written) in [(Role::Key, set.key.is_some()), (Role::Bpm, set.bpm.is_some())] {
                if written && !sure(role) {
                    unsure += 1;
                    match role {
                        Role::Key => set.key = None,
                        _ => set.bpm = None,
                    }
                }
            }
            if !set.is_empty() {
                let tracks = TrackSelector { ids: vec![t.id], ..TrackSelector::default() };
                edits.push(Edit::EditTracks { tracks, fields: set });
            }
        }
        let plan = edit::plan(&device, &edits, &EditOptions { lenient: false, no_journal: false }, ctx)?;
        let mut planned = api::planned_edit("plugins.apply_fields", plan, Touched::library(&device, true), ctx)?;
        let what: Vec<&str> = req.fields.iter().map(|r| r.name()).collect();
        planned
            .summary
            .insert(0, format!("write {} from {} to {} track(s)", what.join(" and "), declared.title, edits.len()));
        if unanalysed > 0 {
            planned.warnings.push(format!("{unanalysed} track(s) aren't analysed by {} yet", declared.title));
        }
        if let Some(min) = req.min_confidence.filter(|_| unsure > 0) {
            planned
                .warnings
                .push(format!("{unsure} value(s) left out: {} is less sure of them than {min}", declared.title));
        }
        Ok(planned)
    }
}
