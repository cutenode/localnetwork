//! rekordbox's own collection (`master.db`) on this computer: read only, what a drive lacks
//! against it, and exporting it to a drive or folder.

use std::path::{Path, PathBuf};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::{resolve_drive, write_estimate};
use crate::collection_export::{self, CollectionExportOptions, CollectionExportPlan, CollectionExportReport};
use crate::collection_write;
use crate::device::Device;
use crate::import::{self, MatchKind, Matcher, Status};
use crate::model::{Library, TrackRef};
use crate::op::{Operation, Planned, Probe, Touched, bytes};
use crate::progress::Ctx;
use crate::store::Store;
use crate::{Error, Result};
use loclib_drive_speed::Work;
use loclib_model::diff::{self, PlaylistDiff};
use loclib_rekordbox_db::masterdb::{self, CloudFolders, Collection, CollectionInfo, TagCategory};

/// Which `master.db` to read, and how.
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct CollectionRef {
    /// Default: `$LOCALLIBRARY_MASTERDB`, else rekordbox's own (see [`masterdb::default_path`]).
    #[serde(default)]
    pub db: Option<PathBuf>,
    /// SQLCipher passphrase. Default: `$LOCALLIBRARY_MASTERDB_KEY`, else the one every rekordbox
    /// 6/7 installation uses.
    #[serde(default)]
    pub key: Option<String>,
}

/// Read the collection `req` names.
pub fn open_collection(req: &CollectionRef) -> Result<Collection> {
    Ok(masterdb::open(&collection_path(req.db.as_deref())?, &masterdb::resolve_key(req.key.as_deref()))?)
}

/// The `master.db` to read: `db`, else rekordbox's own ([`masterdb::default_path`]), once it's
/// there.
pub(crate) fn collection_path(db: Option<&Path>) -> Result<PathBuf> {
    let path = match db {
        Some(p) => p.to_path_buf(),
        None => masterdb::default_path().ok_or_else(|| {
            Error::NotFound(
                "rekordbox's collection: give the master.db to read (rekordbox runs on macOS and Windows)".into(),
            )
        })?,
    };
    if !path.is_file() {
        return Err(Error::NotFound(format!(
            "rekordbox collection {} (is rekordbox 6 or 7 installed?)",
            path.display()
        )));
    }
    Ok(path)
}

pub fn collection_info(req: &CollectionRef) -> Result<CollectionInfo> {
    Ok(open_collection(req)?.info())
}

/// Everything `collection.library` returns: counts, the library (tracks, playlists, history), cues
/// by track and My Tags.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct CollectionView {
    pub info: CollectionInfo,
    pub library: Library,
    pub cues: indexmap::IndexMap<u32, Vec<crate::anlz::Cue>>,
    pub tags: Vec<TagCategory>,
}

pub fn collection_library(req: &CollectionRef) -> Result<CollectionView> {
    let c = open_collection(req)?;
    Ok(CollectionView { info: c.info(), library: c.library, cues: c.cues, tags: c.tags })
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct CollectionDiffRequest {
    #[serde(flatten)]
    pub collection: CollectionRef,
    pub drive: String,
    /// Only this playlist, or the playlists in this folder (and no track-level changes).
    #[serde(default)]
    pub playlist: Option<String>,
}

/// What the drive lacks against the collection (`diff.added_*`), and what only it has.
pub fn collection_diff(req: &CollectionDiffRequest) -> Result<DriveDiff> {
    let d = resolve_drive(&req.drive)?;
    let drive = d.library()?;
    let collection = open_collection(&req.collection)?;
    diff_drive(&collection, &drive, &d.name(), req.playlist.as_deref())
}

/// [`collection_diff`]: `diff.added_*` is what the drive lacks, `diff.removed_*` what only the
/// drive has, and changed tracks are details the collection has since changed (or the drive was
/// edited).
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct DriveDiff {
    /// Collection tracks found on the drive by their file.
    pub matched_by_file: usize,
    /// Found by ISRC, or artist, title and length.
    pub matched_by_details: usize,
    /// Collection tracks only a loose match (title alone, or similar words) finds on the drive;
    /// counted as missing from it.
    pub uncertain: Vec<TrackRef>,
    pub diff: PlaylistDiff,
}

/// What `drive` (a drive's library) lacks against `collection`, and what it has that the
/// collection doesn't: tracks, playlists and their entries, through [`diff::between`] with the
/// drive on the left. Collection tracks are found on the drive by file name and the folders above
/// it, then ISRC, then artist, title and length ([`import::Matcher`]); a track only a looser match
/// finds counts as missing, and is listed in `uncertain`. A `playlist` neither has is refused.
fn diff_drive(
    collection: &Collection,
    drive: &Library,
    drive_label: &str,
    playlist: Option<&str>,
) -> Result<DriveDiff> {
    diff::check_playlist(drive, &collection.library, playlist)?;
    let matcher = Matcher::new(drive);
    let drive_track = drive.track_lookup();
    let mut translated = collection.library.clone();
    let (mut by_file, mut by_details, mut uncertain) = (0, 0, Vec::new());
    for t in &mut translated.tracks {
        let entry = import::Entry {
            title: t.title.clone(),
            artist: t.artist.clone(),
            album: (!t.album.is_empty()).then(|| t.album.clone()),
            mix: (!t.mix_name.is_empty()).then(|| t.mix_name.clone()),
            duration_secs: (t.duration_secs > 0).then_some(t.duration_secs),
            path: (!t.file_path.is_empty()).then(|| t.file_path.clone()),
            isrc: (!t.isrc.is_empty()).then(|| t.isrc.clone()),
            ..Default::default()
        };
        match matcher.find(&entry) {
            Status::Found { track_id, how } if how != MatchKind::Title && how != MatchKind::Fuzzy => {
                // The matcher finds only the drive's own tracks.
                let Some(d) = drive_track(track_id) else { continue };
                t.file_path = d.file_path.clone();
                if how == MatchKind::Path { by_file += 1 } else { by_details += 1 }
            }
            Status::Missing => {}
            _ => uncertain.push(TrackRef::from(&*t)),
        }
    }
    Ok(DriveDiff {
        matched_by_file: by_file,
        matched_by_details: by_details,
        uncertain,
        diff: diff::between(drive, &translated, (drive_label, "rekordbox collection"), playlist),
    })
}

#[cfg(test)]
mod tests;

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct CollectionExportRequest {
    #[serde(flatten)]
    pub collection: CollectionRef,
    /// The drive or folder to export to.
    pub drive: String,
    #[serde(default, flatten)]
    pub options: CollectionExportOptions,
}

/// What exporting the collection to a drive or folder would copy and remove.
pub fn collection_export_plan(
    req: &CollectionExportRequest,
    ctx: &Ctx<'_>,
) -> Result<Planned<CollectionExportPlan, CollectionExportOptions>> {
    let collection = open_collection(&req.collection)?;
    let planned = export_planned("collection export", &req.drive, &req.options, ctx, |d| {
        collection_export::plan(&collection, &CloudFolders::this_computer(), d, &req.options, ctx)
    })?;
    // rekordbox saving its collection in between makes the plan stale.
    planned.touches(Touched::path(&collection.path, false, vec![Probe::Stamp(vec![String::new()])])).guard(None)
}

/// An export (`operation`) to the drive or folder `drive` names, as `plan` plans it with `options`:
/// a folder that isn't there yet is made for it, and taken away again when planning fails. For the
/// caller to add what else it touches to, and guard.
pub(crate) fn export_planned(
    operation: impl Into<std::borrow::Cow<'static, str>>,
    drive: &str,
    options: &CollectionExportOptions,
    ctx: &Ctx<'_>,
    plan: impl FnOnce(&Device) -> Result<CollectionExportPlan>,
) -> Result<Planned<CollectionExportPlan, CollectionExportOptions>> {
    let made = make_folder(drive)?;
    let planned = resolve_drive(drive).and_then(|d| Ok((plan(&d)?, d)));
    let (mut plan, d) = match planned {
        Ok(p) => p,
        Err(e) => {
            if let Some(dir) = &made {
                let _ = std::fs::remove_dir(dir);
            }
            return Err(e);
        }
    };
    plan.made_folder = made.is_some();
    let what = match plan.playlists.as_slice() {
        [] => plan.what(),
        p => p.join(", "),
    };
    let mut summary = vec![
        format!("export {what} to {} ({} track(s), {} playlist(s))", plan.target, plan.tracks, plan.playlist_count),
        format!(
            "copy {} file(s) ({}), remove {} file(s) ({})",
            plan.copies.len(),
            bytes(plan.copy_bytes),
            plan.deletes.len(),
            bytes(plan.delete_bytes)
        ),
    ];
    if plan.downloads > 0 {
        summary.push(format!(
            "download {} online-only file(s) ({}) from cloud storage",
            plan.downloads,
            bytes(plan.download_bytes)
        ));
    }
    if !plan.smart.is_empty() {
        summary.push(format!(
            "keep a smart rule for {} intelligent playlist(s), so `smart apply` keeps them up to date",
            plan.smart.len()
        ));
    }
    let destructive = !plan.deletes.is_empty() || plan.replaces_library;
    let warnings = plan.warnings.clone();
    let work = Work::copying(d.root(), plan.copies.iter().map(|c| (c.path.as_str(), c.size)));
    let estimate = write_estimate(&d, if plan.downloads > 0 { work.plus("downloads") } else { work }, ctx)?;
    Ok(Planned::new(operation, plan, options.clone())
        .summary(summary)
        .estimated(estimate)
        .warnings(warnings)
        .destructive(destructive)
        .overwrite_archive(options.overwrite_archive)
        .touches(Touched::library(&d, true)))
}

/// A folder `spec` names that isn't there yet, made to export into; not the folders above it, as a
/// missing `/Volumes/USB` means a drive that isn't mounted.
fn make_folder(spec: &str) -> Result<Option<PathBuf>> {
    let path = Path::new(spec);
    let path_like = path.is_absolute() || spec.contains(['/', '\\']) || spec.starts_with('.');
    if !path_like || path.exists() {
        return Ok(None);
    }
    match std::fs::create_dir(path) {
        Ok(()) => Ok(Some(path.to_path_buf())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Err(Error::NotFound(format!(
            "{}, the folder to export into (is its drive mounted?)",
            path.parent().unwrap_or(path).display()
        ))),
        Err(e) => Err(Error::Io { path: path.to_path_buf(), source: e }),
    }
}

impl Operation for CollectionExportPlan {
    type Request = CollectionExportOptions;
    type Report = CollectionExportReport;

    fn apply(
        planned: &Planned<Self, CollectionExportOptions>,
        store: Option<&Store>,
        ctx: &Ctx<'_>,
    ) -> Result<CollectionExportReport> {
        let mut dst = planned.drive(0)?;
        crate::fleet::check_target(store, &dst, planned.request().overwrite_archive)?;
        let report = collection_export::apply(&planned.plan, &mut dst, store, ctx)?;
        if let Some(store) = store {
            crate::sync::register_drive(store, &dst, report.fingerprint)?;
            crate::fleet::record_write(store, &dst, report.copied_bytes)?;
            keep_smart_rules(store, &dst, &planned.plan, ctx)?;
        }
        Ok(report)
    }
}

/// Keep the smart rules of the intelligent playlists `plan` exported to `drive` in the store,
/// and take away those of the ones its last export had and this one doesn't.
fn keep_smart_rules(store: &Store, drive: &Device, plan: &CollectionExportPlan, ctx: &Ctx<'_>) -> Result<()> {
    let Some(id) = drive.id().filter(|_| !plan.smart.is_empty() || !plan.smart_gone.is_empty()) else { return Ok(()) };
    let _lock = store.lock()?;
    let mut all = store.smart_playlists()?;
    let left = all.keep_for_drive(id, &plan.smart, &plan.smart_gone)?;
    store.save_smart_playlists(&all)?;
    if let Some(first) = left.first() {
        ctx.warn(format!(
            "{} intelligent playlist(s) got no smart rule, as the store has another rule for the same path (e.g. {first})",
            left.len()
        ));
    }
    Ok(())
}

/// Plan and export in one go, for callers that don't confirm (see [`collection_export_plan`]).
pub fn collection_export_apply(
    store: Option<&Store>,
    req: &CollectionExportRequest,
    ctx: &Ctx<'_>,
) -> Result<CollectionExportReport> {
    collection_export_plan(req, ctx)?.apply(store, ctx)
}

/// The collection `req` names, and its key, to write.
fn collection_target(req: &CollectionRef) -> Result<(PathBuf, String)> {
    Ok((collection_path(req.db.as_deref())?, masterdb::resolve_key(req.key.as_deref())))
}

/// Plan making the collection's cue lists (`contentCue`) agree with its cue rows (see
/// [`collection_write::repair_plan`]).
pub fn collection_repair_plan(
    req: &CollectionRef,
    create_missing: bool,
) -> Result<Planned<collection_write::CollectionRepairPlan>> {
    let (db, key) = collection_target(req)?;
    collection_write::repair_plan(&db, &key, create_missing)
}

/// Plan writing the collection's waiting edits now.
pub fn collection_apply_plan(
    store: &Store,
    req: &CollectionRef,
) -> Result<Planned<collection_write::CollectionFlushPlan>> {
    let (db, key) = collection_target(req)?;
    collection_write::apply_staged_plan(store, &db, &key)
}

/// Plan taking back the latest edit of the collection made here (see [`collection_write::undo_plan`]).
pub fn collection_undo_plan(
    store: &Store,
    req: &CollectionRef,
) -> Result<Planned<collection_write::CollectionUndoPlan>> {
    let (db, key) = collection_target(req)?;
    collection_write::undo_plan(store, &db, &key)
}
