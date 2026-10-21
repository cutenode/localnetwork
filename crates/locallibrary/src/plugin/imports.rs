//! Plugins' importers (`import/read`, the `importers` feature): what another program's file says
//! about tracks (keys, BPMs, cues, My Tags) and the plays it logged. The plugin reads the file the
//! user names, answering what it read (or, too big for a message, a file in its scratch folder
//! holding it). The host finds its tracks on a drive with the matcher tracklists are found with
//! ([`crate::import::Matcher`]), says how many it didn't find, and plans the edits itself, adding
//! to what the tracks have: a key and a BPM where a track has none, as an analyzer's are written
//! ([`super::apply`]), cues in slots (or at places) a track has none in, and My Tags it lacks. The plays
//! are kept in the store as sessions, as Beat Link Trigger's logs are ([`crate::sets::import`]).
//! It's all one plan (`<plugin>.import.<importer>`), confirmed and applied as an edit is, and stale
//! once the file changes. A plugin's answer with what its manifest doesn't say it `provides` is
//! refused.

use std::io::BufReader;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use indexmap::IndexMap;
use loclib_plugin::{
    Context, ImportAnswer, ImportKind, Imported, ImportedCue, ImportedLog, Importer, ReadImport, TrackMatch, method,
};
use rustc_hash::FxHashSet;
use schemars::JsonSchema;
use serde::Serialize;
use serde_json::Value;

use super::Plugins;
use super::apply::key_and_bpm;
use super::process::Slot;
use crate::cue_edit::{CueChange, CuePosition, FIND_TOLERANCE_MS, HotCue, SetCue};
use crate::device::Device;
use crate::edit::{self, Edit, EditOptions, EditPlan, EditReport};
use crate::import::{Entry, Matched, Matcher, Summary};
use crate::model::{Library, Track};
use crate::op::{self, Operation, Planned, Probe, Touched};
use crate::progress::Ctx;
use crate::query::TrackSelector;
use crate::sets::import::log_id;
use crate::sets::{EntryOrigin, ImportedSession, SetBook, SetEntry, TimedPlay};
use crate::store::Store;
use crate::tags::TagChange;
use crate::{Error, Result};

/// An import a plugin read, as the host plans it.
#[derive(Serialize, JsonSchema)]
pub struct PluginImport {
    pub plugin: String,
    pub importer: String,
    /// The file it read.
    pub file: PathBuf,
    /// How many of the tracks it read were found on the drive: one track (`found`), several it
    /// could be (`ambiguous`) or none (`missing`); only those found are edited.
    pub tracks: Summary,
    /// The same, of its sessions' plays: those not found are kept as the log names them.
    pub plays: Summary,
    /// What's added to the tracks found, planned on the drive.
    pub edits: EditPlan,
    /// The sessions it keeps in the store, as `sessions.list` will list them.
    pub sessions: Vec<ImportedSession>,
}

/// What an import did.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct PluginImported {
    /// The edits' report, when there were any.
    pub edits: Option<EditReport>,
    /// The sessions kept, by id.
    pub sessions: Vec<String>,
}

/// What a file says about one track found on the drive, from every entry found to be it.
#[derive(Default)]
struct Gathered<'a> {
    key: Option<&'a str>,
    bpm: Option<f64>,
    cues: Vec<&'a ImportedCue>,
    tags: Vec<&'a str>,
}

impl Plugins {
    /// Plans importing `file` (absolute) with one of a plugin's importers, with `options`
    /// (checked against its schema; `null` is none), onto the drive `drive` names: the edits of
    /// what it read, adding to what the tracks found have (see the module's docs), and the
    /// sessions it read, kept in `store`. The plan (operation `<plugin>.import.<importer>`) is
    /// stale once the file or the drive changes, and never writes to an archive or retired drive.
    #[allow(clippy::too_many_arguments)]
    pub fn import_plan(
        &self,
        plugin: &str,
        importer: &str,
        file: &Path,
        drive: &str,
        options: Value,
        store: Option<&Store>,
        ctx: &Ctx<'_>,
    ) -> Result<Planned<PluginImport>> {
        let slot = self.slot(plugin)?;
        let declared = slot.declared("importer", importer, &slot.manifest.contributes.importers, |i| &i.format)?;
        if !file.is_absolute() {
            return Err(Error::invalid(format!("the file to import must be an absolute path, not {}", file.display())));
        }
        // Not a pipe or a device, which could hold the plugin (and the check before applying) up.
        if !std::fs::metadata(file).is_ok_and(|m| m.is_file()) {
            return Err(Error::invalid(format!("{} isn't a file", file.display())));
        }
        let (context, device) = slot.context(Context { drive: Some(drive.into()), ..Context::default() }, store)?;
        let options = super::fitting(declared.options.as_ref(), options, &context)?;
        let device = device.expect("a drive was named");
        let req = ReadImport { importer: importer.into(), file: file.into(), options, context };
        let read = |answer, scratch: &super::blobs::Scratch, _: &_| match answer {
            ImportAnswer::Read(read) => Ok(read),
            ImportAnswer::Blob(b) => {
                let opened = scratch.blob(&b.blob).map_err(|why| slot.misbehaved(format!("its answer: {why}")))?;
                serde_json::from_reader(BufReader::new(opened))
                    .map_err(|e| slot.misbehaved(format!("its answer in {} isn't one: {e}", b.blob)))
            }
        };
        let read: Imported = slot.call_with(method::READ_IMPORT, &req, store, ctx, &|_| {}, read)?;
        slot.provided(declared, &read)?;
        let library = device.library()?;
        let mut found = Found::new(&library);
        let mut gathered: IndexMap<u32, Gathered> = IndexMap::new();
        for (i, t) in read.tracks.iter().enumerate() {
            let Some(id) = found.track(i + 1, &t.find) else { continue };
            let g = gathered.entry(id).or_default();
            g.key = g.key.or(t.key.as_deref());
            g.bpm = g.bpm.or(t.bpm);
            g.cues.extend(&t.cues);
            g.tags.extend(t.my_tags.iter().map(|tag| tag.trim()).filter(|tag| !tag.is_empty()));
        }
        let tracks = Summary::of(&std::mem::take(&mut found.matched));
        let (edits, left_out) = slot.edits(&device, &library, &gathered)?;
        let tool = format!("{plugin}.{importer}");
        let (sessions, mut warnings) = slot.sessions(&tool, file, &read.sessions, &mut found)?;
        if !sessions.is_empty() && store.is_none() {
            return Err(Error::invalid("sessions are kept in the store, and there's none"));
        }
        let plays = Summary::of(&found.matched);
        let missing = tracks.ambiguous + tracks.missing;
        if missing > 0 {
            warnings.push(format!(
                "{missing} of the {} track(s) left out: {} aren't on the drive, {} could be several",
                tracks.entries, tracks.missing, tracks.ambiguous
            ));
        }
        if left_out > 0 {
            warnings.push(format!("{left_out} cue(s) left out: their tracks have one there already, or end before"));
        }
        warnings.extend(read.warnings.iter().map(|w| format!("{}: {w}", declared.title)));
        let planned = op::refusing_archive_writes(|| {
            let options = EditOptions { lenient: false, no_journal: false };
            let plan = edit::plan(&device, &edits, &options, ctx)?;
            let (writes, destructive) = (!plan.is_noop(), plan.destructive());
            let noop = !writes && sessions.is_empty();
            let mut summary = vec![format!(
                "import {} of {} track(s) found on {} from {}",
                tracks.found,
                tracks.entries,
                device.name(),
                file.display()
            )];
            summary.extend(plan.changes.iter().cloned());
            if !sessions.is_empty() {
                let n: usize = sessions.iter().map(|s| s.plays.len()).sum();
                summary.push(format!(
                    "keep {} session(s) of {n} play(s), {} of them found on the drive",
                    sessions.len(),
                    plays.found
                ));
            }
            let plan_warnings = plan.warnings.clone();
            let import = PluginImport {
                plugin: plugin.into(),
                importer: importer.into(),
                file: file.into(),
                tracks,
                plays,
                edits: plan,
                sessions,
            };
            Planned::new(format!("{plugin}.import.{importer}"), import, ())
                .summary(summary)
                .warnings(warnings)
                .warnings(plan_warnings)
                .destructive(destructive)
                .noop(noop)
                .touches(Touched::library(&device, writes))
                // Applied as planned only while the file stays as it was.
                .touches(Touched::path(file, false, vec![Probe::Content(vec![String::new()])]))
                .reaching_clones(ctx)?
                .guard(None)
        })
        // Writing to an archive drive anyway can't help: a plugin's plan never does.
        .map_err(Error::without_retries)?;
        Ok(planned.never_archive())
    }
}

/// A drive's tracks found as a file names them, and how each was.
struct Found<'a> {
    library: &'a Library,
    matcher: Matcher<'a>,
    matched: Vec<Matched>,
}

impl<'a> Found<'a> {
    fn new(library: &'a Library) -> Found<'a> {
        Found { library, matcher: Matcher::new(library), matched: Vec::new() }
    }

    /// The track `find` names (the file's `line`th), if it's one on the drive: none when it
    /// could be several.
    fn track(&mut self, line: usize, find: &TrackMatch) -> Option<u32> {
        let entry = Entry {
            line,
            title: find.title.clone().unwrap_or_default(),
            artist: find.artist.clone().unwrap_or_default(),
            duration_secs: find.duration,
            path: find.path.clone(),
            isrc: find.isrc.clone(),
            ..Entry::default()
        };
        let status = self.matcher.find(&entry);
        let id = status.track_id();
        self.matched.push(Matched { entry, status });
        id
    }

    fn get(&self, id: u32) -> &'a Track {
        self.library.track(id).expect("found in the library")
    }
}

impl Slot {
    /// Refuses `read` holding what `importer`'s manifest doesn't say it provides.
    fn provided(&self, importer: &Importer, read: &Imported) -> Result<()> {
        let t = &read.tracks;
        let has = [
            (ImportKind::Fields, t.iter().any(|t| t.key.is_some() || t.bpm.is_some())),
            (ImportKind::Cues, t.iter().any(|t| !t.cues.is_empty())),
            (ImportKind::Tags, t.iter().any(|t| !t.my_tags.is_empty())),
            (ImportKind::Sessions, !read.sessions.is_empty()),
        ];
        match has.into_iter().find(|(kind, has)| *has && !importer.provides.contains(kind)) {
            Some((kind, _)) => {
                Err(self.misbehaved(format!("it read {}, which its manifest doesn't say it provides", kind.name())))
            }
            None => Ok(()),
        }
    }

    /// The edits of what was `gathered` for the drive's tracks found, adding to what they have (a
    /// My Tag only to a track without it), and how many cues are left out (a track has one there,
    /// or it ends before).
    fn edits(
        &self,
        device: &Device,
        library: &Library,
        gathered: &IndexMap<u32, Gathered>,
    ) -> Result<(Vec<Edit>, usize)> {
        let with_cues: Vec<u32> = gathered.iter().filter(|(_, g)| !g.cues.is_empty()).map(|(id, _)| *id).collect();
        let cues = match with_cues.is_empty() {
            true => Vec::new(),
            false => crate::cue_edit::list_in(device, library, &with_cues)?,
        };
        // Each track's My Tags, `Category/Tag` in lowercase, as tags match.
        let tagged: FxHashSet<(String, u32)> = match gathered.values().any(|g| !g.tags.is_empty()) {
            false => FxHashSet::default(),
            true => {
                let read = crate::tags::read(device)?;
                let tags = read.categories.iter().flat_map(|c| &c.tags);
                tags.flat_map(|g| g.tracks.iter().map(|t| (g.path.to_lowercase(), t.id))).collect()
            }
        };
        let (mut edits, mut left_out) = (Vec::new(), 0);
        let mut tags: IndexMap<&str, Vec<String>> = IndexMap::new();
        for (&id, g) in gathered {
            let t = library.track(id).expect("found in the library");
            let fields = key_and_bpm(t, g.key, g.bpm, false);
            if !fields.is_empty() {
                edits.push(Edit::EditTracks {
                    tracks: TrackSelector { ids: vec![id], ..TrackSelector::default() },
                    fields,
                });
            }
            let has = cues.iter().find(|c| c.track.id == id).into_iter().flat_map(|c| &c.cues);
            let has: Vec<(Option<HotCue>, u32)> =
                has.map(|c| (c.hot_cue.as_deref().and_then(|h| h.parse().ok()), c.start_ms)).collect();
            let ends = (t.duration_secs > 0).then(|| t.duration_secs.saturating_add(1).saturating_mul(1000));
            let added = adding(&has, &g.cues, ends)
                .map_err(|why| self.misbehaved(format!("its cue for {}: {why}", t.display_name())))?;
            left_out += g.cues.len() - added.len();
            let on = || vec![t.file_path.clone()];
            edits.extend(added.into_iter().map(|c| Edit::Cues { tracks: on(), change: CueChange::Set(c) }));
            for tag in g.tags.iter().filter(|tag| !tagged.contains(&(tag.to_lowercase(), id))) {
                let files = tags.entry(*tag).or_default();
                if !files.contains(&t.file_path) {
                    files.push(t.file_path.clone());
                }
            }
        }
        let tags = tags.into_iter().map(|(tag, tracks)| Edit::MyTag(TagChange::Add { tag: tag.into(), tracks }));
        edits.extend(tags);
        Ok((edits, left_out))
    }

    /// The sessions `logs` (read from `file` with `tool`) as the store keeps them, their plays
    /// found on the drive with `found`, and why any is left out: one without plays, or with a
    /// play that has no time (nor the session).
    fn sessions(
        &self,
        tool: &str,
        file: &Path,
        logs: &[ImportedLog],
        found: &mut Found<'_>,
    ) -> Result<(Vec<ImportedSession>, Vec<String>)> {
        let stem = file.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let time = |i: usize, at: &str| {
            DateTime::parse_from_rfc3339(at)
                .map(|t| t.with_timezone(&Utc))
                .map_err(|_| self.misbehaved(format!("its session {i}: {at:?} isn't a time (RFC 3339)")))
        };
        let (mut sessions, mut warnings) = (Vec::new(), Vec::new());
        for (i, log) in logs.iter().enumerate() {
            let name = match (&log.name, logs.len()) {
                (Some(name), _) if !name.trim().is_empty() => name.trim().to_string(),
                (_, 1) => stem.clone(),
                _ => format!("{stem} {}", i + 1),
            };
            let started = log.started.as_deref().map(|at| time(i, at)).transpose()?;
            let times = log.plays.iter().map(|p| p.started_at.as_deref().map(|at| time(i, at)).transpose());
            let Some(times) = times.map(|t| Ok(t?.or(started))).collect::<Result<Option<Vec<_>>>>()? else {
                warnings.push(format!("{name}: left out, a play has no time and nor has the session"));
                continue;
            };
            if log.plays.is_empty() {
                warnings.push(format!("{name}: left out, it has no plays"));
                continue;
            }
            let parts =
                log.plays.iter().zip(&times).flat_map(|(p, t)| [t.to_rfc3339(), p.artist.clone(), p.title.clone()]);
            let id = log_id(tool, parts);
            let mut plays: Vec<TimedPlay> = (log.plays.iter().zip(times).enumerate())
                .map(|(j, (p, started))| {
                    let find = TrackMatch {
                        path: p.path.clone(),
                        artist: Some(p.artist.clone()),
                        title: Some(p.title.clone()),
                        ..TrackMatch::default()
                    };
                    let entry = match found.track(j + 1, &find) {
                        Some(t) => SetEntry::from_track(found.get(t), EntryOrigin::Session, Some(&id)),
                        None => SetEntry {
                            title: p.title.trim().into(),
                            artist: p.artist.trim().into(),
                            origin: EntryOrigin::Session,
                            session: Some(id.clone()),
                            ..SetEntry::added("")
                        },
                    };
                    TimedPlay { entry, started, stopped: None, player: None }
                })
                .collect();
            plays.sort_by_key(|p| p.started);
            let imported_at = Utc::now();
            sessions.push(ImportedSession { id, name, tool: tool.into(), file: file.into(), imported_at, plays });
        }
        Ok((sessions, warnings))
    }
}

/// Of `cues` (a track's, as a file has them), those to add to a track with `has` (each cue's hot
/// cue slot, if it's one, and where it is), as cue edits: a hot cue in a slot neither the track
/// nor one before it has, a memory cue further than [`FIND_TOLERANCE_MS`] from any the track or
/// one before it has, and none at `ends` (ms, when the track's length is known) or after. A hot
/// cue slot that isn't one (`A` to `H`) is why they're refused.
fn adding(has: &[(Option<HotCue>, u32)], cues: &[&ImportedCue], ends: Option<u32>) -> Result<Vec<SetCue>, String> {
    let mut taken = has.to_vec();
    let mut added = Vec::new();
    for c in cues {
        let hot_cue = c.hot_cue.as_deref().map(str::parse::<HotCue>).transpose().map_err(|e| e.to_string())?;
        let there = taken.iter().any(|(slot, at)| match hot_cue {
            Some(_) => *slot == hot_cue,
            None => slot.is_none() && at.abs_diff(c.start_ms) <= FIND_TOLERANCE_MS,
        });
        if there || ends.is_some_and(|end| c.start_ms >= end) {
            continue;
        }
        taken.push((hot_cue, c.start_ms));
        added.push(SetCue {
            hot_cue,
            at: CuePosition::Ms(c.start_ms),
            snap: false,
            loop_beats: c.loop_beats,
            name: c.name.clone(),
            color: c.color.clone(),
        });
    }
    Ok(added)
}

impl Operation for PluginImport {
    type Request = ();
    type Report = PluginImported;

    /// Makes the edits, then keeps the sessions in `store`.
    fn apply(planned: &Planned<Self>, store: Option<&Store>, ctx: &Ctx<'_>) -> Result<PluginImported> {
        let p = &planned.plan;
        let edits = match p.edits.is_noop() {
            true => None,
            false => Some(edit::apply(&p.edits, &mut planned.drive(0)?, ctx)?),
        };
        if !p.sessions.is_empty() {
            let store = store.ok_or_else(|| Error::invalid("sessions are kept in the store, and there's none"))?;
            let mut book = SetBook::load(store)?;
            for s in &p.sessions {
                book.keep(s.clone());
            }
            book.save(store)?;
        }
        Ok(PluginImported { edits, sessions: p.sessions.iter().map(|s| s.id.clone()).collect() })
    }
}

#[cfg(test)]
mod tests;
