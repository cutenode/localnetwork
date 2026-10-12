//! rekordbox's own collection (`master.db`) on this computer: read only, what a drive lacks
//! against it, and exporting it to a drive or folder.

use std::path::{Path, PathBuf};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::resolve_drive;
use crate::collection_export::{self, CollectionExportOptions, CollectionExportPlan, CollectionExportReport};
use crate::masterdb::{self, CloudFolders, Collection, CollectionInfo, DriveDiff, TagCategory};
use crate::model::Library;
use crate::op::{Operation, Planned, Probe, Touched, bytes};
use crate::progress::Ctx;
use crate::store::Store;
use crate::{Error, Result};

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
    let path = match &req.db {
        Some(p) => p.clone(),
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
    masterdb::open(&path, &masterdb::resolve_key(req.key.as_deref()))
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
    collection.diff_drive(&drive, &d.name(), req.playlist.as_deref())
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
    let made = make_folder(&req.drive)?;
    let planned = resolve_drive(&req.drive).and_then(|d| {
        Ok((collection_export::plan(&collection, &CloudFolders::this_computer(), &d, &req.options, ctx)?, d))
    });
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
        [] => "rekordbox's collection".to_string(),
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
    let destructive = !plan.deletes.is_empty() || plan.replaces_library;
    let warnings = plan.warnings.clone();
    // rekordbox saving its collection in between makes the plan stale.
    let db = Touched::path(&collection.path, false, vec![Probe::Stamp(vec![String::new()])]);
    Planned::new("collection export", plan, req.options.clone())
        .summary(summary)
        .warnings(warnings)
        .destructive(destructive)
        .overwrite_archive(req.options.overwrite_archive)
        .touches(Touched::library(&d, true))
        .touches(db)
        .guard(None)
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
        }
        Ok(report)
    }
}

/// Plan and export in one go, for callers that don't confirm (see [`collection_export_plan`]).
pub fn collection_export_apply(
    store: Option<&Store>,
    req: &CollectionExportRequest,
    ctx: &Ctx<'_>,
) -> Result<CollectionExportReport> {
    collection_export_plan(req, ctx)?.apply(store, ctx)
}
