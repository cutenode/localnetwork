//! Sets: the performances the DJ declares, built from sessions and recordings (see
//! [`crate::sets`]). They live in the store's `sets.json`, written directly.

use std::path::{Path, PathBuf};

use chrono::{Local, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::editing::planned_edit;
use super::{mix_drive, parse_day, resolve_drive};
use crate::edit::{self, Edit, EditOptions, EditPlan, EditReport};
use crate::export::{Locator, PathStyle};
use crate::mix::index::Cache;
use crate::mix::{Identifier, MixReport, Options, report};
use crate::model::Library;
use crate::op::{Planned, Touched};
use crate::progress::Ctx;
use crate::query::TrackSelector;
pub use crate::sets::edit::SetFields;
use crate::sets::edit::{Changes, lookup};
pub use crate::sets::export::SetFormat;
use crate::sets::query::{Appearance, SetFilter};
use crate::sets::{Recording, Session, Set, SetBook, build, export, new_id, session};
use crate::store::Store;
use crate::{Error, Result, fsutil};

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct NewSet {
    #[serde(flatten)]
    pub fields: SetFields,
    /// Sessions to build it from, in play order (ids, or any unique prefix of 4 or more).
    #[serde(default)]
    pub sessions: Vec<String>,
    /// A recording of the set: identified against `drive`'s library, its report kept in the
    /// store. Its tracklist beats the sessions'.
    #[serde(default)]
    pub recording: Option<PathBuf>,
    /// Or a report kept in the store already (`mix.reports.list`).
    #[serde(default)]
    pub report: Option<String>,
    /// The drive whose library a recording is matched against (default: the only mounted
    /// rekordbox drive).
    #[serde(default)]
    pub drive: Option<String>,
}

/// A set as it was saved, and what's worth telling the DJ about it (tentative matches, tracks
/// logged but not heard in the recording, a day to check, how inserted tracks were found).
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct SetChange {
    pub set: Set,
    pub notes: Vec<String>,
}

/// Declare a set, from sessions and/or a recording (or neither, to type its tracks in). Its day
/// is the one given, else the recording's, else the one its sessions' windows agree on; failing
/// those, a note asks for it.
pub fn set_new(store: &Store, req: &NewSet, ctx: &Ctx<'_>) -> Result<SetChange> {
    let now = Utc::now();
    let today = now.with_timezone(&Local).date_naive();
    let mut set = Set { date: today, created_at: now, updated_at: now, ..Default::default() };
    req.fields.apply(&mut set)?;
    let book = SetBook::load(store)?;
    let all = match req.sessions.is_empty() {
        true => Vec::new(),
        false => sessions(store, &book, ctx)?,
    };
    let from = find_sessions(&all, &req.sessions)?;
    let recording = recording(store, req.recording.as_deref(), req.report.as_deref(), req.drive.as_deref(), ctx)?;
    let logged = build::from_sessions(&from);
    let built = match &recording {
        Some((r, _)) => build::with_recording(&logged, r),
        None => build::Tracklist { tracks: logged, notes: Vec::new() },
    };
    let (date, dated) = build::date(req.fields.date()?, recording.as_ref().map(|(r, _)| r), &from, today);
    set.date = date;
    set.id = new_id(date)?;
    set.sessions = from.iter().map(|s| s.id.clone()).collect();
    set.tracks = built.tracks;
    set.recording = recording.map(|(r, id)| Recording { path: r.mix.path, report: Some(id) });
    // Read again: gathering sessions and identifying a recording take a while.
    let mut book = SetBook::load(store)?;
    book.sets.push(set.clone());
    book.save(store)?;
    Ok(SetChange { set, notes: built.notes.into_iter().chain(dated).collect() })
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct Move {
    pub from: usize,
    pub to: usize,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct Insert {
    pub at: usize,
    /// A search for one of the drive's tracks (`Artist - Title`, part of it, or a path), else
    /// the track as typed: `Artist - Title`.
    pub track: String,
}

/// Changes to a set. Tracklist positions start at 1 and all name the tracklist as it was before
/// the edit: a track moved up lands just before the track that was at `to`, one moved down just
/// after it; an insert lands just before the track that was at `at` (one past the last appends).
/// Added sessions' tracks go at the end (or, when the set has a recording, are only checked
/// against it). Attaching a recording rebuilds the tracklist from it, so it can't come with
/// changes by position.
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct EditSet {
    /// The set: `last`, an id prefix of 4 or more, a title, or a day.
    pub set: String,
    #[serde(flatten)]
    pub fields: SetFields,
    /// Positions to drop.
    #[serde(default)]
    pub drop: Vec<usize>,
    #[serde(default)]
    pub moves: Vec<Move>,
    #[serde(default)]
    pub insert: Vec<Insert>,
    /// Sessions whose tracks to add.
    #[serde(default)]
    pub add_sessions: Vec<String>,
    /// A recording to attach (identified against `drive`'s library), or a kept report.
    #[serde(default)]
    pub recording: Option<PathBuf>,
    #[serde(default)]
    pub report: Option<String>,
    /// The drive inserted tracks are looked up on and a recording is matched against (default:
    /// the only mounted rekordbox drive).
    #[serde(default)]
    pub drive: Option<String>,
    /// Positions to publish without naming the track.
    #[serde(default)]
    pub unnamed: Vec<usize>,
    /// Positions to name again.
    #[serde(default)]
    pub named: Vec<usize>,
}

/// Change a set's details and tracklist. Nothing is saved unless every change is valid.
pub fn set_edit(store: &Store, req: &EditSet, ctx: &Ctx<'_>) -> Result<SetChange> {
    let book = SetBook::load(store)?;
    let mut set = book.find(&req.set)?.clone();
    req.fields.apply(&mut set)?;
    let changes = Changes {
        drop: req.drop.clone(),
        moves: req.moves.iter().map(|m| (m.from, m.to)).collect(),
        insert: req.insert.iter().map(|i| (i.at, i.track.clone())).collect(),
        unnamed: req.unnamed.clone(),
        named: req.named.clone(),
    };
    let attaching = req.recording.is_some() || req.report.is_some();
    if attaching && !changes.is_empty() {
        return Err(Error::invalid(
            "attach the recording first, then change its tracklist: positions name the tracklist it replaces",
        ));
    }
    changes.check(set.tracks.len())?;
    let library = match changes.insert.is_empty() {
        true => None,
        false => drive_library(req.drive.as_deref())?,
    };
    let mut notes = Vec::new();
    set.tracks = changes.apply(&set.tracks, |text| {
        let (entry, note) = lookup(text, library.as_ref())?;
        notes.push(note);
        Ok(entry)
    })?;
    if !req.add_sessions.is_empty() {
        let all = sessions(store, &book, ctx)?;
        let mut added = find_sessions(&all, &req.add_sessions)?;
        added.retain(|s| !set.sessions.contains(&s.id));
        let existing: Vec<&Session> = all.iter().filter(|s| set.sessions.contains(&s.id)).collect();
        match set.recording.is_some() && !attaching {
            true => notes.extend(build::not_heard(&build::from_sessions(&added), &set.tracks)),
            false => build::append_sessions(&mut set.tracks, &existing, &added),
        }
        set.sessions.extend(added.iter().map(|s| s.id.clone()));
    }
    if let Some((r, id)) = recording(store, req.recording.as_deref(), req.report.as_deref(), req.drive.as_deref(), ctx)?
    {
        let built = build::with_recording(&set.tracks, &r);
        set.tracks = built.tracks;
        notes.extend(built.notes);
        set.recording = Some(Recording { path: r.mix.path, report: Some(id) });
    }
    set.updated_at = Utc::now();
    // Read again: gathering sessions and identifying a recording take a while.
    let mut book = SetBook::load(store)?;
    *book.find_mut(&set.id)? = set.clone();
    book.save(store)?;
    Ok(SetChange { set, notes })
}

/// The library tracks typed in are looked up in: the drive named, else the only mounted rekordbox
/// drive; none when there's no drive to look in.
pub(super) fn drive_library(drive: Option<&str>) -> Result<Option<Library>> {
    match mix_drive(drive) {
        Ok(d) => Ok(Some(d.library()?)),
        Err(Error::NotFound(_)) if drive.is_none() => Ok(None),
        Err(e) => Err(e),
    }
}

/// Every session on the mounted rekordbox drives and in the store, each once.
fn sessions(store: &Store, book: &SetBook, ctx: &Ctx<'_>) -> Result<Vec<Session>> {
    let (gathered, _) = super::sessions::gather(Some(store), None, book, ctx)?;
    for w in gathered.warnings {
        ctx.warn(w);
    }
    Ok(gathered.sessions)
}

/// The sessions `queries` name, in the order given, each once.
fn find_sessions<'a>(all: &'a [Session], queries: &[String]) -> Result<Vec<&'a Session>> {
    let mut out: Vec<&Session> = Vec::new();
    for q in queries {
        let s = session::find(all, q)?;
        if !out.iter().any(|x| x.id == s.id) {
            out.push(s);
        }
    }
    Ok(out)
}

/// The recording a set is built from, with its report's id: identified against the drive's
/// library and kept in the store's reports, or a report kept there already.
fn recording(
    store: &Store,
    path: Option<&Path>,
    report: Option<&str>,
    drive: Option<&str>,
    ctx: &Ctx<'_>,
) -> Result<Option<(MixReport, String)>> {
    match (path, report) {
        (Some(_), Some(_)) => Err(Error::invalid("give a recording or a report, not both")),
        (Some(path), None) => {
            if !path.is_file() {
                return Err(Error::NotFound(path.display().to_string()));
            }
            if !crate::mix::is_mix_file(path) {
                return Err(Error::invalid(format!(
                    "{} isn't a recording mix reads (WAV, AIFF, MP3, M4A, FLAC, or an Audacity .aup3/.aup4 project)",
                    path.display()
                )));
            }
            let device = mix_drive(drive)?;
            let r = Identifier::new(&device, &Cache::in_store(store), ctx)?.identify(path, &Options::default(), ctx)?;
            let id = report::save(store, &r)?;
            Ok(Some((r, id)))
        }
        (None, Some(id)) => Ok(Some((report::load(store.root(), id)?, id.to_string()))),
        (None, None) => Ok(None),
    }
}

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct SetRef {
    /// The set: `last`, an id prefix of 4 or more, a title, or a day.
    #[serde(default)]
    pub set: Option<String>,
    /// Without `set`: the latest set at the venue whose name contains this (ignoring case).
    #[serde(default)]
    pub venue: Option<String>,
}

fn find<'a>(book: &'a SetBook, req: &SetRef) -> Result<&'a Set> {
    match (&req.set, &req.venue) {
        (Some(s), _) => book.find(s),
        (None, Some(v)) => SetFilter { venue: Some(v.clone()), ..Default::default() }
            .latest(book)
            .ok_or_else(|| Error::NotFound(format!("a set at {v:?}"))),
        (None, None) => Err(Error::invalid("name a set, or a venue for the latest set there")),
    }
}

/// A set, by its id, title or day, or the latest at a venue.
pub fn set_get(store: &Store, req: &SetRef) -> Result<Set> {
    Ok(find(&SetBook::load(store)?, req)?.clone())
}

/// Delete a set (its sessions and recording stay). Returns what was deleted.
pub fn set_delete(store: &Store, req: &SetRef) -> Result<Set> {
    let mut book = SetBook::load(store)?;
    let set = find(&book, req)?.clone();
    book.sets.retain(|s| s.id != set.id);
    book.save(store)?;
    Ok(set)
}

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct SetsQuery {
    /// Only sets at a venue whose name contains this (ignoring case).
    #[serde(default)]
    pub venue: Option<String>,
    /// Only sets of this kind (ignoring case).
    #[serde(default)]
    pub kind: Option<String>,
    /// Only sets played on or after this day (`2026`, `2026-09` or `2026-09-30`).
    #[serde(default)]
    pub since: Option<String>,
    /// Only sets played on or before this day (`2026` is the whole year).
    #[serde(default)]
    pub until: Option<String>,
}

/// The DJ's sets, newest first.
pub fn sets_list(store: &Store, req: &SetsQuery) -> Result<Vec<Set>> {
    let filter = SetFilter {
        venue: req.venue.clone(),
        kind: req.kind.clone(),
        since: req.since.as_deref().map(|d| parse_day(d, false)).transpose()?,
        until: req.until.as_deref().map(|d| parse_day(d, true)).transpose()?,
    };
    Ok(SetBook::load(store)?.sets.into_iter().filter(|s| filter.matches(s)).collect())
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct SetExport {
    pub set: String,
    pub format: SetFormat,
    /// For `m3u8`: the drive the tracks are on (default: the only mounted rekordbox drive).
    #[serde(default)]
    pub drive: Option<String>,
    /// Write it to this file (otherwise it's answered as `text`). An M3U8's paths are relative
    /// to it.
    #[serde(default)]
    pub output: Option<PathBuf>,
    /// Replace a file already at `output` (refused otherwise).
    #[serde(default)]
    pub replace: bool,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct SetExportReport {
    pub format: SetFormat,
    /// The document, when it wasn't written to a file.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<PathBuf>,
    /// Tracks written.
    pub tracks: usize,
    /// Rules of the format the set breaks, and tracks left out.
    pub warnings: Vec<String>,
}

/// Write a set out for publishing (text, Mixcloud, CUE, YouTube chapters, 1001Tracklists; a track
/// marked unnamed is `ID - ID` there) or for the DJ (CSV, and M3U8 of the tracks on a drive).
pub fn set_export(store: &Store, req: &SetExport) -> Result<SetExportReport> {
    let book = SetBook::load(store)?;
    let set = book.find(&req.set)?;
    let (device, library);
    let locator;
    let drive = match req.format {
        SetFormat::M3u8 => {
            device = mix_drive(req.drive.as_deref())?;
            library = device.library()?;
            locator = match &req.output {
                Some(path) => Locator::new(PathStyle::Relative, device.root(), path),
                None => Locator::new(PathStyle::Absolute, device.root(), Path::new(".")),
            };
            Some((&library, &locator))
        }
        _ => None,
    };
    let out = export::render(set, req.format, drive)?;
    if let Some(path) = &req.output {
        fsutil::write_output(path, out.text.as_bytes(), req.replace)?;
    }
    Ok(SetExportReport {
        format: req.format,
        text: req.output.is_none().then_some(out.text),
        path: req.output.clone(),
        tracks: out.tracks,
        warnings: out.warnings,
    })
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct TrackPlaysQuery {
    /// Part of `Artist - Title` (ignoring case), or a drive path (`/Contents/...`).
    pub track: String,
}

/// When and where a track was played: every place it has in the DJ's sets, newest first.
pub fn track_plays(store: &Store, req: &TrackPlaysQuery) -> Result<Vec<Appearance>> {
    if req.track.trim().is_empty() {
        return Err(Error::invalid("say which track: part of Artist - Title, or its path"));
    }
    Ok(crate::sets::query::played(&SetBook::load(store)?, &req.track))
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct SetPlaylist {
    pub set: String,
    pub drive: String,
    /// The playlist's path (default: `Sets/<date> <title or venue>`).
    #[serde(default)]
    pub name: Option<String>,
}

/// A playlist on a drive with a set's tracks that are on it, in set order (each once; one that
/// exists already keeps its tracks and gains the rest). Tracks not on the drive are warned about.
pub fn set_playlist_plan(store: &Store, req: &SetPlaylist, ctx: &Ctx<'_>) -> Result<Planned<EditPlan>> {
    let book = SetBook::load(store)?;
    let set = book.find(&req.set)?;
    let d = resolve_drive(&req.drive)?;
    let library = d.library()?;
    let (found, missing) = export::on_drive(set, &library);
    if found.is_empty() {
        return Err(Error::NotFound(format!("the set's tracks on {}", d.name())));
    }
    let playlist = req.name.clone().unwrap_or_else(|| match set.name() {
        Some(n) => format!("Sets/{} {}", set.date, n.replace('/', "-")),
        None => format!("Sets/{}", set.date),
    });
    let edits = [
        Edit::CreatePlaylist { path: playlist.clone(), folder: false },
        Edit::AddTracks {
            playlist,
            tracks: TrackSelector::paths(found.iter().map(|t| t.file_path.clone())),
            position: None,
            sort: None,
            allow_repeats: false,
        },
    ];
    Ok(planned_edit(
        "sets.playlist",
        edit::plan(&d, &edits, &EditOptions::default(), ctx)?,
        Touched::library(&d, true),
    )?
    .warnings(missing))
}

pub fn set_playlist_apply(store: &Store, req: &SetPlaylist, ctx: &Ctx<'_>) -> Result<EditReport> {
    set_playlist_plan(store, req, ctx)?.apply(Some(store), ctx)
}
