//! Editing playlists and tracks, track queries, harmonic matches, rule-based playlists and the
//! drive's journal of edits.

use rustc_hash::FxHashMap;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use uuid::Uuid;

use super::{DriveRef, LibraryCache, resolve_drive};
use crate::anlz::{self, Mood, TrackAnalysis};
use crate::device::Device;
use crate::edit::{self, Edit, EditOptions, EditPlan, EditReport, journal::JournalEntry};
use crate::error::{Action, Reason};
use crate::harmony;
use crate::model::{BpmRange, Track, TrackRef};
use crate::op::{Operation, Planned, Probe, Touched, bytes};
use crate::progress::Ctx;
use crate::query::{self, Filter, QueryContext, TrackSelector};
use crate::relocate::{self, RelocateOptions, RelocatePlan};
use crate::smart::{SmartPlaylist, SmartPlaylists};
use crate::store::Store;
use crate::{Error, Result};

// ---------------------------------------------------------------------------------------------
// Edits
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct EditRequest {
    pub drive: String,
    pub edits: Vec<Edit>,
    #[serde(default, flatten)]
    pub options: EditOptions,
}

/// What edits would do to a drive.
pub fn edit_plan(req: &EditRequest, ctx: &Ctx<'_>) -> Result<Planned<EditPlan>> {
    let d = resolve_drive(&req.drive)?;
    planned_edit("edit", edit::plan(&d, &req.edits, &req.options, ctx)?, Touched::library(&d, true), ctx)
}

/// Plan and apply edits in one go, for callers that don't confirm (see [`edit_plan`]).
pub fn edit_apply(req: &EditRequest, ctx: &Ctx<'_>) -> Result<EditReport> {
    edit_plan(req, ctx)?.apply(None, ctx)
}

/// An edit plan, as the operation `operation`, for the drive `touched` measured (its library, see
/// [`Touched::library`], and whatever else the plan was made from), reaching its library's other
/// clones (see `Planned::reaching_clones`).
pub(crate) fn planned_edit(
    operation: &'static str,
    plan: EditPlan,
    touched: Touched,
    ctx: &Ctx<'_>,
) -> Result<Planned<EditPlan>> {
    let mut summary = plan.changes.clone();
    if !plan.tracks_removed.is_empty() {
        summary.push(format!(
            "{} track(s) removed, {} file(s) ({}) deleted",
            plan.tracks_removed.len(),
            plan.files.len(),
            bytes(plan.bytes)
        ));
    }
    let destructive = !plan.tracks_removed.is_empty()
        || !plan.playlists_deleted.is_empty()
        || plan.history_removed > 0
        || !plan.files.is_empty();
    let (noop, warnings) = (plan.is_noop(), plan.warnings.clone());
    Planned::new(operation, plan, ())
        .summary(summary)
        .warnings(warnings)
        .destructive(destructive)
        .noop(noop)
        .touches(touched)
        .reaching_clones(ctx)?
        .guard(None)
}

impl Operation for EditPlan {
    type Request = ();
    type Report = EditReport;

    fn apply(planned: &Planned<Self>, _: Option<&Store>, ctx: &Ctx<'_>) -> Result<EditReport> {
        edit::apply(&planned.plan, &mut planned.drive(0)?, ctx)
    }
}

/// The edits recorded on a drive, oldest first.
pub fn edits_list(req: &DriveRef) -> Result<Vec<JournalEntry>> {
    edit::journal::list(&resolve_drive(&req.drive)?)
}

/// Make a drive's recorded edits again (after rekordbox exported over them). Edits that no
/// longer apply are skipped with a warning; the replay isn't recorded again.
pub fn edits_replay_plan(req: &DriveRef, ctx: &Ctx<'_>) -> Result<Planned<EditPlan>> {
    let d = resolve_drive(&req.drive)?;
    let plan = edit::plan(&d, &edit::journal::edits(&d)?, &replay_options(), ctx)?;
    // Made from the journal too: one cleared or added to since is another plan.
    let probes = vec![Probe::Databases, Probe::Identity, Probe::Content(vec![edit::journal::key()])];
    planned_edit("edits.replay", plan, Touched::drive(&d, true, probes), ctx)
}

pub fn edits_replay_apply(req: &DriveRef, ctx: &Ctx<'_>) -> Result<EditReport> {
    edits_replay_plan(req, ctx)?.apply(None, ctx)
}

fn replay_options() -> EditOptions {
    EditOptions { lenient: true, no_journal: true }
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct ClearEdits {
    pub drive: String,
    /// Only entries recorded before this time.
    #[serde(default)]
    pub before: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct ClearedEdits {
    pub removed: usize,
}

/// The recorded edits clearing the journal would forget.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct ClearEditsPlan {
    pub drive: String,
    pub root: std::path::PathBuf,
    pub entries: Vec<JournalEntry>,
}

pub fn edits_clear_plan(req: &ClearEdits) -> Result<Planned<ClearEditsPlan, ClearEdits>> {
    let d = resolve_drive(&req.drive)?;
    let entries: Vec<JournalEntry> =
        edit::journal::list(&d)?.into_iter().filter(|e| req.before.is_none_or(|b| e.at < b)).collect();
    let line =
        format!("forget {} recorded edit(s) on {} (`edits replay` won't make them again)", entries.len(), d.name());
    let noop = entries.is_empty();
    let plan = ClearEditsPlan { drive: d.name(), root: d.root().into(), entries };
    Planned::new("edits.clear", plan, req.clone())
        .line(line)
        .destructive(!noop)
        .noop(noop)
        .touches(Touched::drive(&d, true, vec![Probe::Content(vec![edit::journal::key()])]))
        .guard(None)
}

impl Operation for ClearEditsPlan {
    type Request = ClearEdits;
    type Report = ClearedEdits;

    fn apply(planned: &Planned<Self, ClearEdits>, _: Option<&Store>, _: &Ctx<'_>) -> Result<ClearedEdits> {
        let d = planned.drive(0)?;
        let _lock = d.lock()?;
        Ok(ClearedEdits { removed: edit::journal::clear(&d, planned.request().before)? })
    }
}

/// Plan and clear the journal in one go, for callers that don't confirm.
pub fn edits_clear(req: &ClearEdits) -> Result<ClearedEdits> {
    edits_clear_plan(req)?.apply(None, &Ctx::silent())
}

// ---------------------------------------------------------------------------------------------
// Importing tracklists
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct ImportRequest {
    pub drive: String,
    /// A tracklist: M3U, CSV (Spotify, TIDAL, Beatport... exports), rekordbox text or XML, or
    /// plain "Artist - Title" lines.
    pub file: std::path::PathBuf,
    /// The playlist to fill (created if needed).
    pub into: String,
    /// For rekordbox XML: which of its playlists, by path (the whole collection otherwise).
    #[serde(default)]
    pub playlist: Option<String>,
    /// Add to the playlist instead of replacing its contents.
    #[serde(default)]
    pub append: bool,
    /// Decisions for ambiguous entries: entry index (in the report) -> track id.
    #[serde(default)]
    pub choices: indexmap::IndexMap<usize, u32>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct ImportPreview {
    pub report: crate::import::Report,
    /// `None` when no entry was found.
    pub plan: Option<EditPlan>,
}

fn import_edits(req: &ImportRequest, lib: &crate::Library) -> Result<(crate::import::Report, Vec<Edit>)> {
    use crate::import;
    let bytes = std::fs::read(&req.file).map_err(|e| Error::Io { path: req.file.clone(), source: e })?;
    let hint = req.file.file_name().map(|n| n.to_string_lossy().into_owned());
    let (format, entries) = match &req.playlist {
        Some(p) => {
            let lists = import::xml_playlists(&bytes)?;
            let entries = crate::model::pick(lists, p.trim().trim_matches('/'))
                .ok_or_else(|| Error::NotFound(format!("playlist {p:?} in {}", req.file.display())))?;
            (import::Format::RekordboxXml, entries)
        }
        None => (import::detect(&bytes, hint.as_deref()), import::parse(&bytes, hint.as_deref())?),
    };
    let mut report = import::Report::new(format, import::match_entries(lib, &entries));
    for (&i, &track) in &req.choices {
        let m = report.entries.get_mut(i).ok_or_else(|| Error::invalid(format!("no entry {i} in the list")))?;
        if lib.track(track).is_none() {
            return Err(Error::NotFound(format!("track #{track}")));
        }
        m.status = import::Status::Found { track_id: track, how: import::MatchKind::Chosen };
    }
    report.summary = import::Summary::of(&report.entries);
    // As entries, by file: a set list that played a track twice keeps both.
    let mut paths: Vec<String> =
        lib.tracks_by_id(&report.track_ids()).into_iter().map(|t| t.file_path.clone()).collect();
    if paths.is_empty() {
        return Ok((report, Vec::new()));
    }
    if req.append
        && let Some(p) = edit::find_exact(lib, &req.into)?
    {
        let mut entries: Vec<String> =
            lib.tracks_by_id(&p.track_ids).into_iter().map(|t| t.file_path.clone()).collect();
        entries.append(&mut paths);
        paths = entries;
    }
    Ok((report, vec![Edit::SetEntries { playlist: req.into.clone(), paths, create: true }]))
}

/// Read a tracklist, find its tracks on the drive, and plan the playlist.
pub fn import_plan(req: &ImportRequest, ctx: &Ctx<'_>) -> Result<Planned<ImportPreview>> {
    let d = resolve_drive(&req.drive)?;
    let (report, edits) = import_edits(req, &d.library()?)?;
    let plan = match edits.is_empty() {
        true => None,
        false => Some(edit::plan(&d, &edits, &EditOptions::default(), ctx)?),
    };
    let summary = match &plan {
        Some(p) => p.changes.clone(),
        None => vec!["none of the list's tracks are on the drive".to_string()],
    };
    let warnings = plan.iter().flat_map(|p| p.warnings.iter().cloned()).collect::<Vec<_>>();
    let noop = plan.as_ref().is_none_or(EditPlan::is_noop);
    Planned::new("import", ImportPreview { report, plan }, ())
        .summary(summary)
        .warnings(warnings)
        .noop(noop)
        .touches(Touched::library(&d, true))
        .touches(Touched::path(&req.file, false, vec![crate::op::Probe::Content(vec![String::new()])]))
        .reaching_clones(ctx)?
        .guard(None)
}

impl Operation for ImportPreview {
    type Request = ();
    type Report = EditReport;

    fn apply(planned: &Planned<Self>, _: Option<&Store>, ctx: &Ctx<'_>) -> Result<EditReport> {
        let plan = planned
            .plan
            .plan
            .as_ref()
            .ok_or_else(|| Error::NotFound("none of the list's tracks are on the drive".into()))?;
        edit::apply(plan, &mut planned.drive(0)?, ctx)
    }
}

/// Plan and import in one go, for callers that don't confirm (see [`import_plan`]).
pub fn import_apply(req: &ImportRequest, ctx: &Ctx<'_>) -> Result<EditReport> {
    import_plan(req, ctx)?.apply(None, ctx)
}
// ---------------------------------------------------------------------------------------------
// Duplicates and history
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct DedupeRequest {
    pub drive: String,
    /// Also find copies by audio content (see [`crate::doctor`]).
    #[serde(default)]
    pub hash_audio: bool,
    /// Also find copies by their sound: the same recording in another file or encoding (see
    /// [`crate::doctor`]).
    #[serde(default)]
    pub by_sound: bool,
    /// Also replace tracks that only share artist, title and duration (they may be different
    /// edits of a recording).
    #[serde(default)]
    pub metadata: bool,
}

/// Replace every copy of a recording with the copy doctor suggests keeping: playlist and history
/// entries and My Tags move to it, and the copies (and files nothing else uses) go. Without
/// `metadata`, copies are only those joined to the others by file, audio, sound or Sync Manager's
/// naming: a track that only shares artist, title and length stays, in a group with those too.
///
/// A copy's hot cues, memory cues and loops never go with it. A keeper without cues gets those of
/// the best copy that has some, moved by the offset between their audio (an MP3 made from a WAV
/// starts ~25 ms later), when the two sound like one recording and the offset was measured. A copy
/// whose cues the keeper then has (at that offset) goes; one with cues that can't be carried or
/// that the keeper lacks stays, and a warning says why. Returns the edits and those warnings.
fn dedupe_edits(d: &Device, req: &DedupeRequest, ctx: &Ctx<'_>) -> Result<(Vec<Edit>, Vec<String>)> {
    use crate::cue_edit::{self, CueChange, CueEdit};
    use crate::doctor::{self, DoctorOptions, DuplicateReason, Section};
    let opts = DoctorOptions {
        hash_audio: req.hash_audio,
        by_sound: req.by_sound,
        only: vec![Section::Duplicates],
        sure_only: !req.metadata,
        ..Default::default()
    };
    let groups = doctor::run(d, &opts, ctx)?.duplicates.unwrap_or_default();
    let library = d.library()?;
    let track = |id: u32| library.track(id).ok_or_else(|| Error::NotFound(format!("track #{id}")));
    let (mut edits, mut warnings) = (Vec::new(), Vec::new());
    for g in groups {
        // Several rows for one file can't be told apart by file: rekordbox's next export fixes those.
        if g.reasons == [DuplicateReason::SameFile] {
            continue;
        }
        let keep = track(g.keep)?;
        // The keeper's cues as the plan leaves them.
        let mut kept = cue_spots(d, keep);
        let one = |id: u32| crate::query::TrackSelector { ids: vec![id], ..Default::default() };
        for t in g.tracks.iter().filter(|t| t.track.id != g.keep) {
            let from = track(t.track.id)?;
            let theirs = cue_spots(d, from);
            // Byte-identical copies' cues line up as they are; others need the offset measured.
            let kept_already = theirs.is_empty() || covered(&theirs, &kept, 0);
            let carried = match kept_already {
                true => Ok(None),
                false => match doctor::offset_between(d, from, keep, ctx)? {
                    Err(why) => Err(why.to_string()),
                    Ok(secs) => {
                        let shift_ms = (secs * 1000.0).round() as i32;
                        match kept.is_empty() {
                            true => {
                                // Planned now too, so a carry that can't be made keeps the copy.
                                let carry = CueEdit::Carry { from: from.id, shift_ms };
                                match cue_edit::plan(d, &[keep.id], &carry, ctx) {
                                    Ok(plan) => Ok(Some((shift_ms, plan.warnings))),
                                    Err(e) if matches!(e.inner(), Error::Cancelled) => return Err(e),
                                    Err(e) => Err(e.to_string()),
                                }
                            }
                            false if covered(&theirs, &kept, i64::from(shift_ms)) => Ok(None),
                            false => Err("it has cues of its own, and not all of these".to_string()),
                        }
                    }
                },
            };
            match carried {
                Ok(carry) => {
                    if let Some((shift_ms, plan_warnings)) = carry {
                        warnings.extend(plan_warnings);
                        kept = theirs
                            .iter()
                            .map(|&(h, s, e)| (h, s + i64::from(shift_ms), e.map(|e| e + i64::from(shift_ms))))
                            .collect();
                        let change = CueChange::Carry { from: from.file_path.clone(), shift_ms };
                        edits.push(Edit::Cues { tracks: vec![keep.file_path.clone()], change });
                    }
                    edits.push(Edit::ReplaceTrack {
                        duplicate: one(from.id),
                        keep: one(keep.id),
                        keep_duplicate: false,
                    });
                }
                Err(why) => warnings.push(format!(
                    "{} stays: its cues couldn't be carried onto {}: {why}",
                    from.file_path, keep.file_path
                )),
            }
        }
    }
    Ok((edits, warnings))
}

/// A cue as copies are compared by: hot cue slot (`None`: a memory cue), start and loop end (ms).
type CueSpot = (Option<u8>, i64, Option<i64>);

fn cue_spots(d: &Device, t: &crate::model::Track) -> Vec<CueSpot> {
    anlz::load_grid(d, t).cues.iter().map(|c| (c.hot_cue, i64::from(c.start_ms), c.end_ms.map(i64::from))).collect()
}

/// Whether every cue of `theirs`, `shift_ms` later, is one of `kept`: in its slot, and within
/// 2 ms (positions are whole milliseconds, and so is the shift).
fn covered(theirs: &[CueSpot], kept: &[CueSpot], shift_ms: i64) -> bool {
    let near = |a: i64, b: i64| (a - b).abs() <= 2;
    theirs.iter().all(|&(slot, start, end)| {
        kept.iter().any(|&(k_slot, k_start, k_end)| {
            k_slot == slot
                && near(k_start, start + shift_ms)
                && match (end, k_end) {
                    (None, None) => true,
                    (Some(e), Some(k)) => near(k, e + shift_ms),
                    _ => false,
                }
        })
    })
}

/// Replacing duplicates: a plan with nothing to do when there are none.
pub fn dedupe_plan(req: &DedupeRequest, ctx: &Ctx<'_>) -> Result<Planned<EditPlan>> {
    let d = resolve_drive(&req.drive)?;
    let (edits, kept) = dedupe_edits(&d, req, ctx)?;
    let by = if req.by_sound { "file, audio, sound" } else { "file, audio" };
    let none = (edits.is_empty() && !req.metadata).then(|| {
        format!(
            "no copies by {by} or Sync Manager's naming; `metadata` also replaces tracks that only share artist, \
             title and length"
        )
    });
    // Carried cues are made first, after the plan's one backup; the replacements stay strict.
    let plan = edit::plan_with_steps(&d, &edits, &EditOptions::default(), ctx)?;
    // The analysis files carrying writes and reads: one changed since is another plan.
    let carrying: Vec<String> = edits
        .iter()
        .filter_map(|e| match e {
            Edit::Cues { tracks, change: crate::cue_edit::CueChange::Carry { from, .. } } => {
                Some(tracks.iter().chain([from]).cloned().collect::<Vec<_>>())
            }
            _ => None,
        })
        .flatten()
        .collect();
    let analysis = crate::cue_edit::analysis_files(&d, &d.library()?, &carrying);
    let touched = Touched::drive(&d, true, vec![Probe::Databases, Probe::Identity, Probe::Content(analysis)]);
    Ok(planned_edit("dedupe", plan, touched, ctx)?.warnings(none).warnings(kept))
}

pub fn dedupe_apply(req: &DedupeRequest, ctx: &Ctx<'_>) -> Result<EditReport> {
    dedupe_plan(req, ctx)?.apply(None, ctx)
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct RelocateRequest {
    pub drive: String,
    #[serde(default, flatten)]
    pub options: RelocateOptions,
}

/// Where the drive's tracks whose audio file is missing have it, each only when exactly one file
/// fits (see [`crate::relocate`]).
pub fn relocate_plan(req: &RelocateRequest, ctx: &Ctx<'_>) -> Result<Planned<RelocatePlan>> {
    let d = resolve_drive(&req.drive)?;
    let plan = relocate::plan(&d, &req.options, ctx)?;
    let mut summary = vec![format!(
        "relocate {} track(s) on {}: {} to their file where it is on the drive, {} with their file copied ({})",
        plan.repointed.len() + plan.copied.len(),
        d.name(),
        plan.repointed.len(),
        plan.copied.len(),
        bytes(plan.copy_bytes)
    )];
    if !plan.ambiguous.is_empty() {
        summary.push(format!("{} track(s) with several files that fit are left as they are", plan.ambiguous.len()));
    }
    if !plan.unfit.is_empty() {
        summary
            .push(format!("{} track(s) whose file of that name isn't as long are left as they are", plan.unfit.len()));
    }
    if !plan.not_found.is_empty() {
        summary.push(format!("{} track(s) have no file of their name to be found", plan.not_found.len()));
    }
    // The files it uses and the paths it copies to, as planned.
    let stamps = plan.repointed.iter().map(|r| r.file.path.clone());
    let stamps = stamps.chain(plan.copied.iter().map(|r| crate::paths::key(&r.track.file_path))).collect();
    let sources: Vec<PathBuf> = plan.copied.iter().map(|r| PathBuf::from(&r.file.path)).collect();
    let (noop, warnings) = (plan.is_noop(), plan.warnings.clone());
    let planned = Planned::new("relocate", plan, ()).summary(summary).warnings(warnings).noop(noop);
    copying_onto(planned, &d, stamps, sources, ctx)
}

/// `planned`, which copies `sources` (files on this computer) onto drive `d` at `stamps` (keys)
/// along with an edit: what it touches, checked again before it's made, and the drive's clones.
fn copying_onto<P: Operation>(
    planned: Planned<P>,
    d: &Device,
    stamps: Vec<String>,
    sources: Vec<PathBuf>,
    ctx: &Ctx<'_>,
) -> Result<Planned<P>> {
    let mut planned =
        planned.touches(Touched::drive(d, true, vec![Probe::Databases, Probe::Identity, Probe::Stamp(stamps)]));
    for source in sources {
        planned = planned.touches(Touched::path(&source, false, vec![Probe::Stamp(vec![String::new()])]));
    }
    planned.reaching_clones(ctx)?.guard(None)
}

impl Operation for RelocatePlan {
    type Request = ();
    type Report = EditReport;

    fn apply(planned: &Planned<Self>, _: Option<&Store>, ctx: &Ctx<'_>) -> Result<EditReport> {
        relocate::apply(&planned.plan, &mut planned.drive(0)?, ctx)
    }
}

/// Plan and relocate in one go, for callers that don't confirm (see [`relocate_plan`]).
pub fn relocate_apply(req: &RelocateRequest, ctx: &Ctx<'_>) -> Result<EditReport> {
    relocate_plan(req, ctx)?.apply(None, ctx)
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct UpgradeRequest {
    pub drive: String,
    #[serde(default, flatten)]
    pub options: crate::upgrade::UpgradeOptions,
}

/// Which lossy tracks have a lossless file of their recording in the folders, and pointing them
/// at it (see [`crate::upgrade`]).
pub fn upgrade_plan(req: &UpgradeRequest, ctx: &Ctx<'_>) -> Result<Planned<crate::upgrade::UpgradePlan>> {
    let d = resolve_drive(&req.drive)?;
    let mut options = req.options.clone();
    // No players named: those remembered for the drive's library.
    if options.players.is_empty()
        && let Some(store) = super::registry()
    {
        options.players = store.player_models()?.for_library(d.state().and_then(|s| s.library_id)).0.to_vec();
    }
    let plan = crate::upgrade::plan(&d, &options, ctx)?;
    let mut summary = vec![format!(
        "upgrade {} track(s) on {} to lossless files ({})",
        plan.upgrades.len(),
        d.name(),
        bytes(plan.copy_bytes)
    )];
    if !plan.not_upgraded.is_empty() {
        summary.push(format!("{} track(s) with a lossless file are left as they are", plan.not_upgraded.len()));
    }
    if !plan.removed.is_empty() {
        summary.push(format!("delete {} lossy file(s), after backing them up", plan.removed.len()));
    }
    let stamps = plan.upgrades.iter().map(|u| u.to.clone()).collect();
    let sources: Vec<PathBuf> = plan.upgrades.iter().map(|u| u.file.clone()).collect();
    let (noop, warnings, destructive) = (plan.is_noop(), plan.warnings.clone(), !plan.removed.is_empty());
    let planned =
        Planned::new("upgrade", plan, ()).summary(summary).warnings(warnings).noop(noop).destructive(destructive);
    copying_onto(planned, &d, stamps, sources, ctx)
}

impl Operation for crate::upgrade::UpgradePlan {
    type Request = ();
    type Report = EditReport;

    fn apply(planned: &Planned<Self>, _: Option<&Store>, ctx: &Ctx<'_>) -> Result<EditReport> {
        crate::upgrade::apply(&planned.plan, &mut planned.drive(0)?, ctx)
    }
}

/// Plan and upgrade in one go, for callers that don't confirm (see [`upgrade_plan`]).
pub fn upgrade_apply(req: &UpgradeRequest, ctx: &Ctx<'_>) -> Result<EditReport> {
    upgrade_plan(req, ctx)?.apply(None, ctx)
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct DeleteHistoryRequest {
    pub drive: String,
    /// History lists by name; all of them when empty.
    #[serde(default)]
    pub names: Vec<String>,
    /// Delete lists the store hasn't kept (`locallibrary pull` keeps them).
    #[serde(default)]
    pub force: bool,
}

/// Delete history lists from a drive, the way a player's ALL DELETE does, but only those the store
/// has archived unless `force`.
fn delete_history_edits(store: Option<&Store>, req: &DeleteHistoryRequest, ctx: &Ctx<'_>) -> Result<Vec<Edit>> {
    let d = resolve_drive(&req.drive)?;
    let on_drive = crate::plays::drive_history(&d)?;
    let wanted: Vec<&crate::plays::HistoryList> = on_drive
        .lists
        .iter()
        .filter(|l| req.names.is_empty() || req.names.iter().any(|n| n.trim().eq_ignore_ascii_case(&l.name)))
        .collect();
    if wanted.is_empty() {
        return Err(Error::NotFound("no such history list on the drive".into()));
    }
    if !req.force {
        let archive = match store {
            Some(s) => Some(super::history_archive(
                s,
                &super::HistoryArchiveRequest { library: None, drive: Some(req.drive.clone()) },
                ctx,
            )?),
            None => None,
        };
        let unsaved: Vec<&str> = wanted
            .iter()
            .filter(|l| !archive.as_ref().is_some_and(|a| crate::plays::is_archived(l, a)))
            .map(|l| l.name.as_str())
            .collect();
        if !unsaved.is_empty() {
            return Err(Error::conflict(
                Reason::NotSaved,
                format!("{} isn't in the store yet: pull the drive first to keep it, or force it", unsaved.join(", ")),
            )
            .with("drive", &req.drive)
            .with("lists", &unsaved)
            .action(Action::call("pull", serde_json::json!({ "drive": req.drive }), "Pull the drive into the store"))
            .action(Action::retry(serde_json::json!({ "force": true }), "Delete them anyway")));
        }
    }
    let names: Vec<String> = wanted.iter().map(|l| l.name.clone()).collect();
    Ok(vec![Edit::DeleteHistory {
        all: req.names.is_empty(),
        names: if req.names.is_empty() { Vec::new() } else { names },
    }])
}

pub fn delete_history_plan(
    store: Option<&Store>,
    req: &DeleteHistoryRequest,
    ctx: &Ctx<'_>,
) -> Result<Planned<EditPlan>> {
    let edits = delete_history_edits(store, req, ctx)?;
    let d = resolve_drive(&req.drive)?;
    let plan = edit::plan(&d, &edits, &EditOptions::default(), ctx)?;
    planned_edit("history.delete", plan, Touched::library(&d, true), ctx)
}

pub fn delete_history_apply(store: Option<&Store>, req: &DeleteHistoryRequest, ctx: &Ctx<'_>) -> Result<EditReport> {
    delete_history_plan(store, req, ctx)?.apply(store, ctx)
}

// ---------------------------------------------------------------------------------------------
// Queries
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct TracksQuery {
    pub drive: String,
    /// See [`crate::query`].
    #[serde(default)]
    pub filter: Option<String>,
    /// Only tracks in this playlist, in its order (unless `sort` is given).
    #[serde(default)]
    pub playlist: Option<String>,
    /// E.g. `key,bpm` or `-rating`.
    #[serde(default)]
    pub sort: Option<String>,
    #[serde(default)]
    pub offset: usize,
    #[serde(default)]
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct TracksPage {
    /// Tracks matching, before `offset` and `limit`.
    pub total: usize,
    pub total_duration_secs: u64,
    pub tracks: Vec<Track>,
}

/// Find and order tracks on a drive.
pub fn tracks_query(req: &TracksQuery) -> Result<TracksPage> {
    tracks_query_cached(LibraryCache::none(), req)
}

/// [`tracks_query`], with the library from `cache`.
pub fn tracks_query_cached(cache: &LibraryCache, req: &TracksQuery) -> Result<TracksPage> {
    let (d, cached) = cache.open(&req.drive)?;
    let lib = &cached.library;
    let filter = req.filter.as_deref().map(Filter::parse).transpose()?.unwrap_or_default();
    let tags = if filter.uses_tags() { Some(cached.tags_by_track(&d)?) } else { None };
    let mut ctx = QueryContext::new(lib);
    if let Some(t) = &tags {
        ctx = ctx.with_tags(t);
    }
    filter.check(&ctx)?;
    let mut tracks: Vec<&Track> = match &req.playlist {
        Some(p) => {
            let pl = lib.find_playlist(p).ok_or_else(|| Error::NotFound(format!("playlist {p:?}")))?;
            lib.tracks_by_id(&pl.track_ids).into_iter().filter(|t| filter.matches(t, &ctx)).collect()
        }
        None => filter.select(&ctx),
    };
    if let Some(s) = &req.sort {
        query::sort_tracks(&mut tracks, &query::parse_sort(s)?);
    }
    let total = tracks.len();
    let total_duration_secs = tracks.iter().map(|t| u64::from(t.duration_secs)).sum();
    let tracks = tracks.into_iter().skip(req.offset).take(req.limit.unwrap_or(usize::MAX)).cloned().collect();
    Ok(TracksPage { total, total_duration_secs, tracks })
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct TrackRequest {
    pub drive: String,
    pub track: u32,
}

/// A track's beat grid, cues, waveforms and phrases.
pub fn track_analysis(req: &TrackRequest) -> Result<TrackAnalysis> {
    let d = resolve_drive(&req.drive)?;
    let lib = d.library()?;
    let t = lib.track(req.track).ok_or_else(|| Error::NotFound(format!("track #{}", req.track)))?;
    Ok(anlz::load(&d, t))
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct TrackDetailsRequest {
    pub drive: String,
    #[serde(flatten)]
    pub tracks: TrackSelector,
}

/// Everything known about one track on a drive: its details, where it's filed, problems with it
/// and a summary of its analysis.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct TrackDetails {
    pub track: Track,
    /// Whether the audio file is on the drive.
    pub audio_present: bool,
    /// Playlists holding the track, by path, sorted.
    pub playlists: Vec<String>,
    /// History lists (what players recorded) holding the track, by name.
    pub history: Vec<String>,
    /// My Tags, as `Category/Tag`. Empty when the drive's tag database can't be read.
    pub tags: Vec<String>,
    /// Other tracks on the drive that are the same recording (artist, title and duration).
    pub copies: Vec<TrackRef>,
    pub analysis: AnalysisSummary,
}

/// What a track's analysis files hold, counted rather than listed (`track.analysis` and
/// `cues.list` have the full data).
#[derive(Debug, Clone, Default, Serialize, JsonSchema)]
pub struct AnalysisSummary {
    /// Whether the drive has the track's analysis files.
    pub analysed: bool,
    pub beats: usize,
    pub bars: usize,
    /// The beat grid's slowest and fastest tempo.
    pub tempo_range: Option<BpmRange>,
    pub hot_cues: usize,
    pub memory_cues: usize,
    /// Hot cues and memory cues that are loops.
    pub loops: usize,
    /// Whether there's a scrolling (detail) waveform.
    pub waveform: bool,
    pub mood: Option<Mood>,
    pub phrases: Vec<PhraseSummary>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct PhraseSummary {
    /// `Intro`, `Up`, `Verse 2`...
    pub kind: String,
    pub bars: u16,
}

impl AnalysisSummary {
    fn of(a: &TrackAnalysis, analysed: bool) -> AnalysisSummary {
        let tempos = a.beats.iter().map(|b| b.bpm).filter(|bpm| *bpm > 0.0);
        let tempo_range =
            tempos.clone().reduce(f64::min).zip(tempos.reduce(f64::max)).map(|(low, high)| BpmRange { low, high });
        let hot_cues = a.cues.iter().filter(|c| c.hot_cue.is_some()).count();
        AnalysisSummary {
            analysed,
            beats: a.beats.len(),
            bars: a.beats.len().div_ceil(4),
            tempo_range,
            hot_cues,
            memory_cues: a.cues.len() - hot_cues,
            loops: a.cues.iter().filter(|c| c.end_ms.is_some()).count(),
            waveform: a.detail.is_some(),
            mood: a.structure.as_ref().map(|s| s.mood),
            phrases: a.structure.as_ref().map_or_else(Vec::new, |s| {
                s.phrases
                    .iter()
                    .map(|p| PhraseSummary { kind: p.kind.name(), bars: (p.end_beat - p.start_beat).div_ceil(4) })
                    .collect()
            }),
        }
    }
}

/// The selected tracks' details, in the order selected.
pub fn track_details(req: &TrackDetailsRequest) -> Result<Vec<TrackDetails>> {
    track_details_cached(LibraryCache::none(), req)
}

/// [`track_details`], with the library from `cache`.
pub fn track_details_cached(cache: &LibraryCache, req: &TrackDetailsRequest) -> Result<Vec<TrackDetails>> {
    let (d, cached) = cache.open(&req.drive)?;
    let lib = &cached.library;
    let tags = cached.tags_by_track(&d).unwrap_or_default();
    let history = match &req.tracks.history {
        Some(_) => crate::plays::drive_history(&d)?.lists,
        None => Vec::new(),
    };
    let ctx = QueryContext::new(lib).with_tags(&tags).with_history(&history);
    let ids = req.tracks.select_ordered(&ctx, false)?;
    // Rekordbox's fingerprint; untitled tracks without an artist aren't compared.
    let fingerprint =
        |t: &Track| (!(t.artist.trim().is_empty() && t.title.trim().is_empty())).then(|| t.fingerprint_key());
    let mut recordings: FxHashMap<String, Vec<&Track>> = FxHashMap::default();
    for t in &lib.tracks {
        if let Some(k) = fingerprint(t) {
            recordings.entry(k).or_default().push(t);
        }
    }
    Ok(lib
        .tracks_by_id(&ids)
        .into_iter()
        .map(|t| {
            let mut playlists = ctx.playlists_of(t.id).to_vec();
            playlists.sort();
            let analysed = !t.analyze_path.is_empty()
                && d.checked_path(&crate::paths::key(&t.analyze_path)).is_ok_and(|p| p.is_file());
            TrackDetails {
                audio_present: d.checked_path(&crate::paths::key(&t.file_path)).is_ok_and(|p| p.is_file()),
                playlists,
                history: lib
                    .history
                    .iter()
                    .filter(|l| !l.is_folder && l.track_ids.contains(&t.id))
                    .map(|l| l.name.clone())
                    .collect(),
                tags: tags.get(&crate::paths::key(&t.file_path).to_lowercase()).cloned().unwrap_or_default(),
                copies: fingerprint(t)
                    .and_then(|k| recordings.get(&k))
                    .map_or_else(Vec::new, |v| v.iter().filter(|o| o.id != t.id).map(|o| TrackRef::from(*o)).collect()),
                analysis: AnalysisSummary::of(&anlz::load(&d, t), analysed),
                track: t.clone(),
            }
        })
        .collect())
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct MatchesRequest {
    pub drive: String,
    pub track: u32,
    #[serde(default)]
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct HarmonicMatch {
    pub track: Track,
    /// Camelot code, e.g. `8A`.
    pub key: String,
    /// `same key`, `±1`, `relative` or `energy boost`.
    pub relation: &'static str,
    /// 0.02 is 2% apart (half and double time allowed).
    pub tempo_gap: f64,
}

/// Tracks that mix into a track: a compatible key and a close tempo, best first.
pub fn harmonic_matches(req: &MatchesRequest) -> Result<Vec<HarmonicMatch>> {
    harmonic_matches_cached(LibraryCache::none(), req)
}

/// [`harmonic_matches`], with the library from `cache`.
pub fn harmonic_matches_cached(cache: &LibraryCache, req: &MatchesRequest) -> Result<Vec<HarmonicMatch>> {
    let (_, cached) = cache.open(&req.drive)?;
    let lib = &cached.library;
    let t = lib.track(req.track).ok_or_else(|| Error::NotFound(format!("track #{}", req.track)))?;
    Ok(harmony::suggestions(lib, t, req.limit.unwrap_or(40))
        .into_iter()
        .map(|s| HarmonicMatch {
            track: s.track,
            key: s.key.code(),
            relation: s.relation.describe(),
            tempo_gap: s.tempo_gap,
        })
        .collect())
}

// ---------------------------------------------------------------------------------------------
// Packing a drive for a set
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct PackRequest {
    pub source: String,
    pub target: String,
    #[serde(default, flatten)]
    pub options: crate::pack::PackOptions,
}

/// What packing a drive for a set would copy and remove.
pub fn pack_plan(req: &PackRequest, ctx: &Ctx<'_>) -> Result<Planned<crate::pack::PackPlan, crate::pack::PackOptions>> {
    let src = resolve_drive(&req.source)?;
    let dst = resolve_drive(&req.target)?;
    let mut options = req.options.clone();
    // Copies for no players named are for those remembered for the source's library.
    if let Some(t) = options.transcode.as_mut().filter(|t| t.players.is_empty())
        && let Some(store) = super::registry()
    {
        let library = src.state().and_then(|s| s.library_id);
        t.players = store.player_models()?.for_library(library).0.to_vec();
    }
    let plan = crate::pack::plan(&src, &dst, &options, ctx)?;
    let mut summary = vec![format!(
        "pack {} from {} onto {} ({} track(s))",
        plan.playlists.join(", "),
        plan.source,
        plan.target,
        plan.tracks
    )];
    summary.push(format!(
        "copy {} file(s) ({}), remove {} file(s) ({})",
        plan.copies.len(),
        bytes(plan.copy_bytes),
        plan.deletes.len(),
        bytes(plan.delete_bytes)
    ));
    if !plan.transcodes.is_empty() {
        let current = plan.transcodes.iter().filter(|t| t.current).count();
        summary.push(format!(
            "pack {} track(s) the players can't play as copies they can: make {} (about {}), keep {current}",
            plan.transcodes.len(),
            plan.transcodes.len() - current,
            bytes(plan.transcode_bytes)
        ));
    }
    if !plan.unplayable.is_empty() {
        summary.push(format!("{} track(s) stay unplayable on these players", plan.unplayable.len()));
    }
    let destructive = !plan.deletes.is_empty() || plan.replaces_library;
    let warnings = plan.warnings.clone();
    let overwrite_archive = options.overwrite_archive;
    Planned::new("pack", plan, options)
        .summary(summary)
        .warnings(warnings)
        .destructive(destructive)
        .overwrite_archive(overwrite_archive)
        .touches(Touched::library(&dst, true))
        .touches(Touched::library(&src, false))
        .guard(None)
}

impl Operation for crate::pack::PackPlan {
    type Request = crate::pack::PackOptions;
    type Report = crate::pack::PackReport;

    fn apply(
        planned: &Planned<Self, crate::pack::PackOptions>,
        store: Option<&Store>,
        ctx: &Ctx<'_>,
    ) -> Result<crate::pack::PackReport> {
        let (mut dst, mut src) = (planned.drive(0)?, planned.drive(1)?);
        crate::fleet::check_target(store, &dst, planned.request().overwrite_archive)?;
        // The target remembers its source by id.
        src.adopt(None)?;
        let report = crate::pack::apply(&planned.plan, &src, &mut dst, store, ctx)?;
        if let Some(store) = store {
            crate::sync::register_drive(store, &dst, report.fingerprint)?;
            crate::fleet::record_write(store, &dst, report.copied_bytes)?;
        }
        Ok(report)
    }
}

/// Plan and pack in one go, for callers that don't confirm (see [`pack_plan`]).
pub fn pack_apply(store: Option<&Store>, req: &PackRequest, ctx: &Ctx<'_>) -> Result<crate::pack::PackReport> {
    pack_plan(req, ctx)?.apply(store, ctx)
}
// ---------------------------------------------------------------------------------------------
// Rule-based playlists
// ---------------------------------------------------------------------------------------------

pub fn smart_list(store: &Store) -> Result<SmartPlaylists> {
    store.smart_playlists()
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct SetSmartPlaylist {
    pub path: String,
    pub filter: String,
    #[serde(default)]
    pub sort: Option<String>,
    #[serde(default)]
    pub limit: Option<usize>,
    /// Only for these libraries, or the libraries of these drives (names, ids or mount paths);
    /// every drive when empty.
    #[serde(default)]
    pub drives: Vec<String>,
}

pub fn smart_set(store: &Store, req: &SetSmartPlaylist) -> Result<SmartPlaylists> {
    let _lock = store.lock()?;
    let mut drives = Vec::new();
    for d in &req.drives {
        drives.push(match super::override_target(store, d) {
            Ok((id, _)) => id,
            Err(e @ Error::NotFound(_)) => store.find_library(d)?.ok_or(e)?.id,
            Err(e) => return Err(e),
        });
    }
    let mut all = store.smart_playlists()?;
    all.set(SmartPlaylist {
        path: req.path.trim().trim_matches('/').to_string(),
        filter: req.filter.clone(),
        sort: req.sort.clone(),
        limit: req.limit,
        drives,
    })?;
    store.save_smart_playlists(&all)?;
    Ok(all)
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct RemoveSmartPlaylist {
    pub path: String,
}

pub fn smart_remove(store: &Store, req: &RemoveSmartPlaylist) -> Result<SmartPlaylists> {
    let _lock = store.lock()?;
    let mut all = store.smart_playlists()?;
    all.remove(&req.path)?;
    store.save_smart_playlists(&all)?;
    Ok(all)
}

fn smart_request(store: &Store, drive: &str) -> Result<EditRequest> {
    let d = resolve_drive(drive)?;
    // A rule limited to a drive is limited to its library: its clones are kept identical.
    let mut ids: Vec<Uuid> = d.id().into_iter().collect();
    if let Some(library) = d.state().and_then(|s| s.library_id) {
        let drives = store.drives()?.into_values();
        ids.extend(drives.filter(|r| r.library_id == Some(library)).map(|r| r.id).chain([library]));
    }
    let edits = store.smart_playlists()?.edits_for(&ids);
    if edits.is_empty() {
        return Err(Error::NotFound("no smart playlists apply to this drive (add one with `smart add`)".into()));
    }
    Ok(EditRequest { drive: drive.to_string(), edits, options: EditOptions::default() })
}

/// Bring a drive's copies of the smart playlists up to date.
pub fn smart_apply_plan(store: &Store, req: &DriveRef, ctx: &Ctx<'_>) -> Result<Planned<EditPlan>> {
    smart_fill_plan(store, req, true, ctx)
}

/// [`smart_apply_plan`], recording the fill in the drive's journal (so `edits replay` makes it
/// again) or not. A refresh fills them from their rules each time, so it can do without the
/// journal when that can't be read.
pub(super) fn smart_fill_plan(store: &Store, req: &DriveRef, record: bool, ctx: &Ctx<'_>) -> Result<Planned<EditPlan>> {
    let mut req = smart_request(store, &req.drive)?;
    req.options.no_journal = !record;
    let d = resolve_drive(&req.drive)?;
    planned_edit("smart.write", edit::plan(&d, &req.edits, &req.options, ctx)?, Touched::library(&d, true), ctx)
}

pub fn smart_apply(store: &Store, req: &DriveRef, ctx: &Ctx<'_>) -> Result<EditReport> {
    smart_apply_plan(store, req, ctx)?.apply(Some(store), ctx)
}
