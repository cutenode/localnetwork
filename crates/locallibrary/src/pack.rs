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
//! recorded for that are forgotten (see [`crate::edit::journal`]).

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::PathBuf;

use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::backup;
use crate::commit::Swap;
use crate::device::{Device, LineageEntry, LineageOp, Scanned};
use crate::edit;
use crate::error::{Action, Reason};
use crate::fsutil;
use crate::hash::Hash;
use crate::model::Library;
use crate::paths::{self, FileClass};
use crate::progress::{Ctx, Event, Phase};
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

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct PackCopy {
    pub path: String,
    pub size: u64,
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
    pub copies: Vec<PackCopy>,
    pub copy_bytes: u64,
    /// Files on the target the packed library doesn't use.
    pub deletes: Vec<PackCopy>,
    pub delete_bytes: u64,
    pub unchanged: usize,
    /// The target holds a library that isn't an earlier pack of this source.
    pub replaces_library: bool,
    /// What the target holds that only it has: saved in the store before packing.
    pub unsaved: Option<Unsaved>,
    pub free_space: Option<u64>,
    pub fits: bool,
    pub warnings: Vec<String>,
    #[serde(skip)]
    pdb: Option<Vec<u8>>,
    #[serde(skip)]
    ext: Option<Vec<u8>>,
    #[serde(skip)]
    plus: Option<Vec<u8>>,
    /// Source file for each key to copy.
    #[serde(skip)]
    sources: BTreeMap<String, (PathBuf, i64)>,
    #[serde(skip)]
    history: bool,
    /// Files on the target the pack overwrites or removes that its backup keeps: analysis and
    /// artwork (audio isn't backed up).
    #[serde(skip)]
    backed_up: Vec<String>,
}

impl PackPlan {
    pub fn is_noop(&self) -> bool {
        false // the databases are always rewritten
    }
}

fn fold(s: &str) -> String {
    s.to_lowercase()
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
    let mut wanted: BTreeMap<String, (PathBuf, u64, i64)> = BTreeMap::new();
    let add = |key: &str, tree: &FileTree, wanted: &mut BTreeMap<String, (PathBuf, u64, i64)>| {
        if let Some(e) = tree.resolve(key) {
            wanted.insert(e.path.clone(), (tree.fs_path(&e.path), e.size, e.mtime));
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
    let missing = libraries[0].tracks.iter().filter(|t| src_tree.resolve(&paths::key(&t.file_path)).is_none()).count();
    if missing > 0 {
        warnings.push(format!("{missing} track(s) have no audio file on {}; players won't load them", source.name()));
    }

    // Against the target.
    let target_lib = target.library().ok();
    let scanned = target.scan_fingerprinted(target_lib.as_ref(), ctx)?;
    let unsaved = unsaved(target, &scanned, same_source, &subset)?;
    let tgt_tree = scanned.tree;
    let target_by_fold: HashMap<String, &crate::tree::FileEntry> =
        tgt_tree.library_entries().map(|e| (fold(&e.path), e)).collect();
    let mut copies = Vec::new();
    let mut sources = BTreeMap::new();
    let mut unchanged = 0;
    let mut backed_up = Vec::new();
    let backs_up = |class: FileClass| matches!(class, FileClass::Analysis | FileClass::Artwork);
    // After a pack cut off on the target, a file modified since it started may be torn.
    let cut_off = crate::sync::cut_off_since(target);
    for (key, (src, size, mtime)) in &wanted {
        match target_by_fold.get(&fold(key)) {
            Some(e)
                if e.size == *size
                    && fsutil::mtime_eq(e.mtime, *mtime)
                    && cut_off.is_none_or(|since| e.mtime < since) =>
            {
                unchanged += 1
            }
            existing => {
                if let Some(e) = existing.filter(|e| backs_up(e.class)) {
                    backed_up.push(e.path.clone());
                }
                copies.push(PackCopy { path: key.clone(), size: *size });
                sources.insert(key.clone(), (src.clone(), *mtime));
            }
        }
    }
    let written: HashSet<String> = [
        subset.pdb.as_ref().map(|_| fold(paths::EXPORT_PDB)),
        subset.ext.as_ref().map(|_| fold(paths::EXPORT_EXT_PDB)),
        subset.plus.as_ref().map(|_| fold(paths::LIBRARY_PLUS_DB)),
        subset.plus.as_ref().map(|_| fold(paths::LIBRARY_PLUS_WAL)),
    ]
    .into_iter()
    .flatten()
    .collect();
    let wanted_folds: HashSet<String> = wanted.keys().map(|k| fold(k)).collect();
    let deletes: Vec<PackCopy> = if opts.keep_extra {
        Vec::new()
    } else {
        tgt_tree
            .library_entries()
            .filter(|e| !wanted_folds.contains(&fold(&e.path)) && !written.contains(&fold(&e.path)))
            // Library files only, and of those not what's under PIONEER/ besides the library's
            // databases, analysis and artwork. (Settings, and what players and rekordbox keep for
            // the drive, aren't library files.)
            .filter(|e| {
                matches!(
                    paths::classify(&e.path),
                    Some(FileClass::Audio | FileClass::Analysis | FileClass::Artwork | FileClass::Database)
                )
            })
            .map(|e| PackCopy { path: e.path.clone(), size: e.size })
            .collect()
    };
    backed_up.extend(deletes.iter().filter(|d| paths::classify(&d.path).is_some_and(backs_up)).map(|d| d.path.clone()));
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
    warnings.extend(edit::journal::forget_warning(target, "packing"));
    let cluster = fsutil::cluster_size(target.root());
    let round = |n: u64| n.div_ceil(cluster) * cluster;
    let db_bytes = round(subset.pdb.as_ref().map_or(0, |b| b.len() as u64))
        + round(subset.plus.as_ref().map_or(0, |b| b.len() as u64));
    let copy_bytes: u64 = copies.iter().map(|c| c.size).sum();
    let needed = copies.iter().map(|c| round(c.size)).sum::<u64>() + db_bytes;
    let free_space = fsutil::available_space(target.root());
    let fits = free_space.is_none_or(|f| needed <= f);
    if !fits {
        warnings.push(format!(
            "about {} bytes need copying but only {} are free on {}",
            needed,
            free_space.unwrap_or(0),
            target.name()
        ));
    }
    Ok(PackPlan {
        source: source.name(),
        target: target.name(),
        target_root: target.root().to_path_buf(),
        playlists,
        tracks: subset.library.tracks.len(),
        library_bytes: wanted.values().map(|(_, s, _)| s).sum::<u64>() + db_bytes,
        copy_bytes,
        delete_bytes: deletes.iter().map(|d| d.size).sum(),
        copies,
        deletes,
        unchanged,
        replaces_library,
        unsaved,
        free_space,
        fits,
        warnings,
        pdb: subset.pdb,
        ext: subset.ext,
        plus: subset.plus,
        sources,
        history,
        backed_up,
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
    let mut packed: HashSet<(String, Vec<String>)> = HashSet::new();
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
            needed: plan.copy_bytes,
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
        report.backup = Some(backup::create(target, &reason, &plan.backed_up, None)?.id);
    }
    target.begin(LineageOp::Pack, format!("pack from {}", plan.source), report.backup.clone())?;

    // Files are flushed before they get their source's modification time, which tells the next
    // pack they're complete (see `fsutil::copy_file`); a pack stopped part way still does both
    // for what it copied.
    ctx.phase_start(Phase::Copy, plan.copies.len() as u64, plan.copy_bytes);
    let mut written: Vec<(PathBuf, i64)> = Vec::new();
    let mut stopped = None;
    for c in &plan.copies {
        let copied = (|| -> Result<(PathBuf, i64)> {
            ctx.check()?;
            let (src, mtime) = plan.sources.get(&c.path).ok_or_else(|| Error::NotFound(c.path.clone()))?;
            let dst = target.checked_path(&c.path)?;
            ctx.emit(Event::Item { phase: Phase::Copy, path: c.path.clone(), bytes: c.size });
            fsutil::copy_file(src, &dst, None, None, false, ctx)?;
            Ok((dst, *mtime))
        })();
        match copied {
            Ok(w) => {
                written.push(w);
                report.copied += 1;
                report.copied_bytes += c.size;
            }
            Err(e) => {
                stopped = Some(e);
                break;
            }
        }
    }
    fsutil::flush(&written.iter().map(|(p, _)| p.clone()).collect::<Vec<_>>())?;
    for (path, mtime) in &written {
        fsutil::set_mtime(path, *mtime)?;
    }
    if let Some(e) = stopped {
        return Err(e);
    }
    ctx.phase_end(Phase::Copy);

    // The databases, staged, then swapped in back to back (`commit::swap`); OneLibrary's old WAL
    // goes before its new database (see `onelibrary::write`). A source without one of them leaves
    // none on the target. The pack is made from here: recording it isn't cancellable.
    let ctx = &ctx.uncancellable();
    let at = |key: &str| target.checked_path(key);
    let mut steps = Vec::new();
    if let Some(bytes) = &plan.pdb {
        steps.push(Swap::bytes(at(paths::EXPORT_PDB)?, bytes)?);
    }
    if let Some(bytes) = &plan.ext {
        steps.push(Swap::bytes(at(paths::EXPORT_EXT_PDB)?, bytes)?);
    }
    for key in [paths::LIBRARY_PLUS_WAL, paths::LIBRARY_PLUS_SHM] {
        steps.push(Swap::Remove(at(key)?));
    }
    steps.push(match &plan.plus {
        Some(bytes) => Swap::bytes(at(paths::LIBRARY_PLUS_DB)?, bytes)?,
        None => Swap::Remove(at(paths::LIBRARY_PLUS_DB)?),
    });
    if plan.pdb.is_none() {
        steps.push(Swap::Remove(at(paths::EXPORT_PDB)?));
    }
    crate::commit::swap(steps)?;

    if !plan.deletes.is_empty() {
        ctx.phase_start(Phase::Delete, plan.deletes.len() as u64, plan.delete_bytes);
        for d in &plan.deletes {
            let p = target.checked_path(&d.path)?;
            ctx.emit(Event::Item { phase: Phase::Delete, path: d.path.clone(), bytes: d.size });
            match std::fs::remove_file(&p) {
                Ok(()) => {
                    report.deleted += 1;
                    report.deleted_bytes += d.size;
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(Error::Io { path: p, source: e }),
            }
            if let Some(parent) = p.parent() {
                fsutil::prune_empty_dirs(parent, target.root());
            }
        }
        ctx.phase_end(Phase::Delete);
    }

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
