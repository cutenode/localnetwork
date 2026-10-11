//! Drives for a set: a drive holding only some playlists of another drive's library (with the folders
//! above them and the tracks they use), kept up to date incrementally. The databases are cut down
//! in memory from the source's (see [`crate::edit`]), so tracks keep their analysis, cues and
//! artwork; then only the files the subset needs are copied, and the rest removed from the target.
//!
//! The target remembers what it was packed from (`.locallibrary/pack.json`), so packing it again
//! without naming playlists updates it with the same selection. A packed drive holds a different
//! library state from its source: `sync` treats the two as different libraries.
//!
//! A packed drive comes back from a set with what the players wrote to it: history lists, memory
//! cues in its analysis files, TAG LIST playlists. Packing again would replace them with the
//! source's, so before it does, the target's state is saved in the store when it holds history
//! lists the packed library won't, or changed since it was last packed (see [`Unsaved`]). Like the
//! other writers, it backs up the databases it replaces (and the analysis and artwork files it
//! overwrites or removes) on the drive first. It replaces the target's library, so the edits
//! recorded for that are forgotten (see [`crate::edit::journal`]), but the target keeps the player
//! name and colour it had (see [`crate::player::stash`]).

use indexmap::IndexMap;

use rustc_hash::FxHashSet;
use std::path::PathBuf;

use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::backup;
use crate::device::{Device, LineageEntry, LineageOp, Scanned};
use crate::edit;
use crate::error::{Action, Reason};
use crate::fsutil;
use crate::hash::Hash;
use crate::library_write::{self, Databases, FileCopy, Source};
use crate::model::Library;
use crate::paths;
use crate::progress::Ctx;
use crate::store::{SnapshotInfo, Store};
use crate::sync::PullOptions;
use crate::tree::FileTree;
use crate::{Error, Result};

const PACK_FILE: &str = "pack.json";

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct PackOptions {
    /// Playlists or folders (paths) to put on the target. Empty: the selection the target was
    /// last packed with.
    #[serde(default)]
    pub playlists: Vec<String>,
    /// Keep the source's history lists too.
    #[serde(default)]
    pub history: bool,
    /// Leave files on the target that the packed library doesn't use.
    #[serde(default)]
    pub keep_extra: bool,
    /// Write to an archive or retired drive (see [`crate::fleet::check_target`]).
    #[serde(default)]
    pub overwrite_archive: bool,
}

/// What a target was packed with, kept on the target.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct PackRecord {
    pub source: Option<Uuid>,
    pub source_name: String,
    pub playlists: Vec<String>,
    pub history: bool,
    pub at: DateTime<Utc>,
    /// The source's library state when it was packed, so `check` can tell whether the pack is
    /// still up to date.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_fingerprint: Option<Hash>,
}

pub fn record(device: &Device) -> Result<Option<PackRecord>> {
    fsutil::read_json_opt(&device.state_dir().join(PACK_FILE))
}

/// The drive no longer holds a pack: a sync or push wrote a whole library over it.
pub fn forget(device: &Device) -> Result<()> {
    fsutil::remove_if_exists(&device.state_dir().join(PACK_FILE)).map(|_| ())
}

/// What the target holds that packing would replace and that nothing else keeps: it is saved in
/// the store first, as `sync` saves a drive back from a set before overwriting it.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Unsaved {
    /// History lists players recorded that the packed library won't have.
    pub history_lists: Vec<String>,
    /// The drive changed since it was last packed from this source (memory cues, TAG LIST
    /// playlists or history lists players saved, or edits).
    pub changed: bool,
}

impl Unsaved {
    /// What it is, for messages: "2 history list(s) (HISTORY 001, HISTORY 002)".
    pub fn describe(&self) -> String {
        let mut parts = Vec::new();
        if !self.history_lists.is_empty() {
            let shown: Vec<&str> = self.history_lists.iter().take(3).map(String::as_str).collect();
            let more = if self.history_lists.len() > 3 { ", ..." } else { "" };
            parts.push(format!("{} history list(s) ({}{more})", self.history_lists.len(), shown.join(", ")));
        }
        if self.changed {
            parts.push("changes since it was last packed".to_string());
        }
        parts.join(" and ")
    }
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct PackPlan {
    pub source: String,
    pub target: String,
    pub target_root: PathBuf,
    pub playlists: Vec<String>,
    pub tracks: usize,
    /// Size of the packed library's files.
    pub library_bytes: u64,
    pub copies: Vec<FileCopy>,
    pub copy_bytes: u64,
    /// Files on the target the packed library doesn't use.
    pub deletes: Vec<FileCopy>,
    pub delete_bytes: u64,
    pub unchanged: usize,
    /// The target holds a library that isn't an earlier pack of this source.
    pub replaces_library: bool,
    /// What the target holds that only it has: saved in the store before packing.
    pub unsaved: Option<Unsaved>,
    /// Space the pack needs on the target: the files it copies, its new databases and the backup
    /// of what it replaces.
    pub needed_bytes: u64,
    pub free_space: Option<u64>,
    pub fits: bool,
    pub warnings: Vec<String>,
    #[serde(skip)]
    databases: Databases,
    #[serde(skip)]
    history: bool,
    #[serde(skip)]
    written: library_write::Planned,
}

impl PackPlan {
    pub fn is_noop(&self) -> bool {
        false // the databases are always rewritten
    }
}

pub fn plan(source: &Device, target: &Device, opts: &PackOptions, ctx: &Ctx<'_>) -> Result<PackPlan> {
    if source.root() == target.root() {
        return Err(Error::invalid("the source and the target are the same drive"));
    }
    source.finished()?;
    let previous = record(target)?;
    let same_source = previous.as_ref().is_some_and(|r| r.source.is_some() && r.source == source.id());
    let (playlists, history) = match (&opts.playlists[..], &previous) {
        ([], Some(r)) if same_source => (r.playlists.clone(), r.history || opts.history),
        ([], _) => {
            return Err(Error::invalid("choose the playlists or folders to pack")
                .because(Reason::MissingParam)
                .with("param", "playlists"));
        }
        (p, _) => (p.to_vec(), opts.history),
    };
    let subset = edit::subset(source, &playlists, history, ctx)?;
    let mut warnings = subset.warnings.clone();

    // The files the packed library uses, found on the source.
    let libraries: Vec<&Library> = std::iter::once(&subset.library).chain(subset.second.iter()).collect();
    let everything =
        Library { tracks: libraries.iter().flat_map(|l| l.tracks.iter().cloned()).collect(), ..Default::default() };
    let src_tree = source.scan(Some(&everything), ctx)?;
    let mut wanted: IndexMap<String, Source> = IndexMap::new();
    let add = |key: &str, tree: &FileTree, wanted: &mut IndexMap<String, Source>| {
        if let Some(e) = tree.resolve(key) {
            wanted.insert(e.path.clone(), Source::file(tree.fs_path(&e.path), e.size, e.mtime));
        }
    };
    for t in libraries.iter().flat_map(|l| l.tracks.iter()) {
        add(&paths::key(&t.file_path), &src_tree, &mut wanted);
        for k in crate::clean::analysis_files(&t.analyze_path, &src_tree) {
            add(&k, &src_tree, &mut wanted);
        }
        if let Some(a) = &t.artwork_path {
            add(&paths::key(a), &src_tree, &mut wanted);
            add(&crate::artwork::medium_variant(&paths::key(a)), &src_tree, &mut wanted);
        }
    }
    if source.has_library_plus()
        && let Ok(one) = source.library_plus()
    {
        for (_, a) in one.images_used_outside_tracks()? {
            add(&paths::key(&a), &src_tree, &mut wanted);
            add(&crate::artwork::medium_variant(&paths::key(&a)), &src_tree, &mut wanted);
        }
    }
    if subset.ext.is_none() {
        add(paths::EXPORT_EXT_PDB, &src_tree, &mut wanted);
    }
    wanted.sort_unstable_keys();
    let missing = libraries[0].tracks.iter().filter(|t| src_tree.resolve(&paths::key(&t.file_path)).is_none()).count();
    if missing > 0 {
        warnings.push(format!("{missing} track(s) have no audio file on {}; players won't load them", source.name()));
    }

    // Against the target.
    let target_lib = target.library().ok();
    let scanned = target.scan_fingerprinted(target_lib.as_ref(), ctx)?;
    let unsaved = unsaved(target, &scanned, same_source, &subset)?;
    let databases = Databases { pdb: subset.pdb.clone(), ext: subset.ext.clone(), plus: subset.plus.clone() };
    let written = library_write::plan(target, &scanned.tree, &wanted, &databases, opts.keep_extra);
    let replaces_library = target.has_export() && !same_source;
    if replaces_library {
        warnings.push(format!(
            "{} holds a library that wasn't packed from {}; packing replaces it (pull it into the store first to keep it)",
            target.name(),
            source.name()
        ));
    }
    if let Some(u) = &unsaved {
        warnings.push(format!(
            "{} holds {} that packing replaces; its state is saved in the store first",
            target.name(),
            u.describe()
        ));
    }
    warnings.extend(edit::journal::forget_warning(target, None, "packing"));
    if !written.fits {
        warnings.push(format!(
            "the pack needs about {} bytes (its files, databases and backup) but only {} are free on {}",
            written.needed_bytes,
            written.free_space.unwrap_or(0),
            target.name()
        ));
    }
    Ok(PackPlan {
        source: source.name(),
        target: target.name(),
        target_root: target.root().to_path_buf(),
        playlists,
        tracks: subset.library.tracks.len(),
        library_bytes: written.library_bytes,
        copy_bytes: written.copy_bytes,
        delete_bytes: written.delete_bytes,
        copies: written.copies.clone(),
        deletes: written.deletes.clone(),
        unchanged: written.unchanged,
        replaces_library,
        unsaved,
        needed_bytes: written.needed_bytes,
        free_space: written.free_space,
        fits: written.fits,
        warnings,
        databases,
        history,
        written,
    })
}

/// What the target holds that the pack would replace and only the target has (see [`Unsaved`]).
fn unsaved(target: &Device, scanned: &Scanned, same_source: bool, subset: &edit::Subset) -> Result<Option<Unsaved>> {
    if !target.has_export() {
        return Ok(None);
    }
    // History lists by name and tracks (as files), as `plays` tells them apart.
    let files = |tracks: &mut dyn Iterator<Item = &crate::model::Track>| -> Vec<String> {
        tracks.map(|t| paths::key(&t.file_path).to_lowercase()).collect()
    };
    let mut packed: FxHashSet<(String, Vec<String>)> = FxHashSet::default();
    for lib in std::iter::once(&subset.library).chain(subset.second.iter()) {
        for h in lib.history.iter().filter(|h| !h.is_folder) {
            packed.insert((h.name.clone(), files(&mut lib.tracks_by_id(&h.track_ids).into_iter())));
        }
    }
    let mut history_lists: Vec<String> = crate::plays::drive_history(target)?
        .lists
        .iter()
        .filter(|l| !packed.contains(&(l.name.clone(), files(&mut l.tracks.iter()))))
        .map(|l| l.name.clone())
        .collect();
    history_lists.dedup();
    // Changed since the last pack: players wrote to it. An interrupted pack's own writes don't
    // count (the state before it was saved first, if it needed to be).
    let state = target.state();
    let interrupted = state.and_then(|s| s.pending.as_ref()).is_some_and(|p| p.op == LineageOp::Pack);
    let changed = same_source
        && !interrupted
        && state.and_then(|s| s.head()).is_some_and(|h| !scanned.fingerprints().any(|fp| fp == h.fingerprint));
    Ok((changed || !history_lists.is_empty()).then_some(Unsaved { history_lists, changed }))
}

#[derive(Debug, Clone, Default, Serialize, JsonSchema)]
pub struct PackReport {
    pub copied: usize,
    pub copied_bytes: u64,
    pub deleted: usize,
    pub deleted_bytes: u64,
    pub fingerprint: Option<Hash>,
    /// The target's state, saved in the store before packing (see [`PackPlan::unsaved`]).
    pub saved: Option<SnapshotInfo>,
    /// The backup of what the pack replaced: `locallibrary undo` puts its databases, analysis and
    /// artwork back. Tracks the pack removed have no audio in it (their audio is on the source),
    /// so taking back a pack that removed tracks needs `--force` (and leaves them without audio)
    /// or packing again with the old selection.
    pub backup: Option<String>,
}

/// Write a pack: first the target's own state saved in the store when it needs to be (refused
/// without a store) and its databases backed up, then files, then the databases, then removal of
/// what the target no longer needs (the same crash-safe order as a sync).
pub fn apply(
    plan: &PackPlan,
    source: &Device,
    target: &mut Device,
    store: Option<&Store>,
    ctx: &Ctx<'_>,
) -> Result<PackReport> {
    if !plan.fits {
        return Err(Error::NoSpace {
            path: plan.target_root.clone(),
            needed: plan.needed_bytes,
            available: plan.free_space.unwrap_or(0),
        });
    }
    let mut report = PackReport::default();
    if let Some(u) = &plan.unsaved {
        let store = store.ok_or_else(|| {
            let drive = plan.target_root.display().to_string();
            Error::conflict(
                Reason::NotSaved,
                format!(
                    "{} holds {} that packing would replace; keeping it needs the store, so pull the drive first",
                    plan.target,
                    u.describe()
                ),
            )
            .with("drive", &drive)
            .action(Action::call(
                "pull",
                serde_json::json!({ "drive": drive }),
                format!("Pull {} into the store", plan.target),
            ))
        })?;
        let opts = PullOptions {
            library: None,
            message: Some(format!("{} before packing from {}: {}", plan.target, plan.source, u.describe())),
            force: true,
        };
        report.saved = crate::sync::pull(store, target, &opts, ctx)?.snapshot;
    }
    // The source's state, so `check` can tell later whether the pack is still up to date.
    let source_fingerprint = source.scan_fingerprinted(source.library().ok().as_ref(), ctx)?.fingerprint;
    let _lock = crate::commit::lock(target, None)?;
    if target.has_export() {
        let reason = format!("before packing from {}", plan.source);
        report.backup = Some(backup::create(target, &reason, &plan.written.backed_up, None)?.id);
    }
    target.begin(LineageOp::Pack, format!("pack from {}", plan.source), report.backup.clone())?;
    let identity = crate::player::stash(target, plan.databases.plus.is_some())?;

    let (copied, copied_bytes) = library_write::copy(target, &plan.written, ctx)?;
    (report.copied, report.copied_bytes) = (copied, copied_bytes);

    // The databases, then removal of what the pack no longer uses. The pack is made from here:
    // recording it isn't cancellable.
    let ctx = &ctx.uncancellable();
    library_write::swap_databases(target, &plan.databases)?;
    crate::player::preserve(target, identity.as_ref())?;
    (report.deleted, report.deleted_bytes) = library_write::delete(target, &plan.written, ctx)?;

    if let Some(id) = &report.backup {
        backup::seal(target, id)?;
    }
    let library = target.library()?;
    let scanned = target.scan_fingerprinted(Some(&library), ctx)?;
    report.fingerprint = scanned.fingerprint;
    target.finish(scanned.fingerprint.map(|fp| LineageEntry {
        fingerprint: fp,
        at: Utc::now(),
        op: LineageOp::Pack,
        snapshot: None,
        source: source.id(),
        tracks: library.tracks.len(),
    }))?;
    let rec = PackRecord {
        source: source.id(),
        source_name: source.name(),
        playlists: plan.playlists.clone(),
        history: plan.history,
        at: Utc::now(),
        source_fingerprint,
    };
    fsutil::write_json_atomic(&target.state_dir().join(PACK_FILE), &rec)?;
    edit::journal::forget(target)?;
    if report.backup.is_some() {
        backup::prune(target)?;
    }
    Ok(report)
}
