//! Drives for a set: a drive holding only some playlists of another drive's library (with the folders
//! above them and the tracks they use), kept up to date incrementally. The databases are cut down
//! in memory from the source's (see [`crate::edit`]), so tracks keep their analysis, cues and
//! artwork; then only the files the subset needs are copied, and the rest removed from the target.
//!
//! Tracks the set's players can't play can be packed as lossless copies they can ([`Transcode`]),
//! in place of their masters.
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
use crate::equipment::MODELS;
use crate::error::{Action, Reason};
use crate::fsutil;
use crate::hash::Hash;
use crate::library_write::{self, Databases, FileCopy, Made, Source};
use crate::model::Library;
use crate::paths;
use crate::progress::{Ctx, Phase};
use crate::store::{SnapshotInfo, Store};
use crate::sync::PullOptions;
use crate::tree::FileTree;
use crate::{Error, Result};

mod transcode;

pub use loclib_transcode::Container;
pub use transcode::{PackTranscode, PackUnplayable};

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
    /// Pack tracks the players can't play as lossless copies they can.
    /// Without playlists, as the target was last packed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transcode: Option<Transcode>,
}

/// Which players a packed drive's copies are for, and how they're made.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Transcode {
    /// Player models (see `players models`); none: the ones remembered for the source's library.
    #[serde(default)]
    pub players: Vec<String>,
    /// The container copies are written in: AIFF unless given.
    #[serde(default)]
    pub container: loclib_transcode::Container,
    /// Also decode lossy files the players can't play (an MP3 at 22.05 kHz) into lossless copies:
    /// bigger, but no further loss. Lossy files are never encoded lossy again.
    #[serde(default)]
    pub decode_lossy: bool,
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
    /// The copies it was packed with.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transcode: Option<Transcode>,
}

pub fn record(device: &Device) -> Result<Option<PackRecord>> {
    fsutil::read_json_opt(&device.state_dir().join(PACK_FILE))
}

/// The drive no longer holds a pack: a sync or push wrote a whole library over it.
pub fn forget(device: &Device) -> Result<()> {
    transcode::forget(device)?;
    fsutil::remove_if_exists(&device.state_dir().join(PACK_FILE)).map(|_| ())
}

/// What the target holds that packing would replace and that nothing else keeps: it is saved in
/// the store first, as `sync` saves a drive back from a set before overwriting it.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Unsaved {
    /// History lists players recorded that the packed library won't have.
    pub history_lists: Vec<String>,
    /// The drive changed since it was last written this way (packed from this source, or exported
    /// from the collection): memory cues, TAG LIST playlists or history lists players saved, or
    /// edits.
    pub changed: bool,
    /// Its OneLibrary database can't be read, so history lists players that read only OneLibrary
    /// recorded there can't be told apart: it's kept whole.
    pub unreadable_onelibrary: bool,
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
            parts.push("changes since locallibrary last wrote it".to_string());
        }
        if self.unreadable_onelibrary {
            parts.push("an exportLibrary.db that can't be read (it may hold history lists)".to_string());
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
    /// Tracks the players can't play, packed as copies they can.
    pub transcodes: Vec<PackTranscode>,
    /// About how much the copies to make take (not those current already).
    pub transcode_bytes: u64,
    /// The copies' players and how they're made, as the target will remember them.
    pub transcode: Option<Transcode>,
    /// Tracks none of the players play that the pack leaves as they are.
    pub unplayable: Vec<PackUnplayable>,
    /// Space the pack needs on the target: the files it copies, the copies it makes, its new
    /// databases and the backup of what it replaces.
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
    let choice = match (&opts.playlists[..], &previous) {
        ([], Some(r)) if same_source => opts.transcode.clone().or_else(|| r.transcode.clone()),
        _ => opts.transcode.clone(),
    };
    let subset = edit::subset(source, &playlists, history, ctx)?;
    // Packing export.pdb alone would take the target's OneLibrary away, and the drive with it from
    // the players that read nothing else.
    if source.has_library_plus() && subset.plus.is_none() {
        let only: Vec<&str> = MODELS.iter().filter(|m| !m.is_mixer() && m.onelibrary_only()).map(|m| m.name).collect();
        let drive = source.root().display().to_string();
        return Err(Error::invalid(format!(
            "{}'s exportLibrary.db can't be read, so the packed drive would have no OneLibrary database and {} \
             couldn't browse it",
            source.name(),
            only.join(", ")
        ))
        .with("drive", &drive)
        .action(Action::call(
            "convert.plan",
            serde_json::json!({ "drive": drive, "to": "one_library", "replace": true }),
            format!("Rebuild {}'s OneLibrary from export.pdb", source.name()),
        )));
    }
    let mut warnings = subset.warnings.clone();

    // The files the packed library uses, found on the source.
    let libraries: Vec<&Library> = std::iter::once(&subset.library).chain(subset.second.iter()).collect();
    let everything =
        Library { tracks: libraries.iter().flat_map(|l| l.tracks.iter().cloned()).collect(), ..Default::default() };
    let mut src_tree = source.scan(Some(&everything), ctx)?;
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
    let unsaved = unsaved(target, &scanned, same_source, LineageOp::Pack, &libraries)?;
    let made = match &choice {
        None => transcode::Copies::default(),
        Some(c) => {
            let tracks: Vec<&crate::model::Track> = libraries.iter().flat_map(|l| l.tracks.iter()).collect();
            transcode::plan(source, target, &mut src_tree, &scanned.tree, &tracks, c, ctx)?
        }
    };
    // The copies stand in for their sources, and analysis files record the copies' paths.
    wanted.retain(|key, _| !made.replaced.contains(key));
    wanted.extend(made.rewrites);
    warnings.extend(made.warnings);
    let databases = Databases { pdb: subset.pdb.clone(), ext: subset.ext.clone(), plus: subset.plus.clone() };
    let player_copies: Vec<Made<'_>> =
        made.transcodes.iter().map(|t| Made { path: &t.to, size: t.size, current: t.current }).collect();
    let written =
        library_write::plan(target, &scanned.tree, &wanted, &player_copies, databases.written(), opts.keep_extra);
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
    let transcode_bytes: u64 = made.transcodes.iter().filter(|t| !t.current).map(|t| t.size).sum();
    if !written.fits {
        warnings.push(format!(
            "the pack needs about {} bytes (its files, copies, databases and backup) but only {} are free on {}",
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
        transcode: choice,
        transcode_bytes,
        transcodes: made.transcodes,
        unplayable: made.unplayable,
    })
}

/// What `target` holds that writing a whole library over it would replace and only it has (see
/// [`Unsaved`]): history lists players recorded that none of `kept` (the libraries written) has, a
/// OneLibrary that can't be read, and, when it was written by the same kind of write (`op`) last
/// (`rewrites`), changes since. Shared with [`crate::collection_export`].
pub(crate) fn unsaved(
    target: &Device,
    scanned: &Scanned,
    rewrites: bool,
    op: LineageOp,
    kept: &[&Library],
) -> Result<Option<Unsaved>> {
    if !target.has_export() {
        return Ok(None);
    }
    // History lists by name and tracks (as files), as `plays` tells them apart.
    let files = |tracks: &mut dyn Iterator<Item = &crate::model::Track>| -> Vec<String> {
        tracks.map(|t| paths::file_key(&t.file_path)).collect()
    };
    let mut written: FxHashSet<(String, Vec<String>)> = FxHashSet::default();
    for lib in kept {
        for h in lib.history.iter().filter(|h| !h.is_folder) {
            written.insert((h.name.clone(), files(&mut lib.tracks_by_id(&h.track_ids).into_iter())));
        }
    }
    let on_drive = crate::plays::drive_history(target)?;
    let mut history_lists: Vec<String> = on_drive
        .lists
        .iter()
        .filter(|l| !written.contains(&(l.name.clone(), files(&mut l.tracks.iter()))))
        .map(|l| l.name.clone())
        .collect();
    history_lists.dedup();
    // Changed since it was last written: players wrote to it, or an edit or undo. An interrupted
    // write's own changes don't count (the state before it was saved first, if it needed to be).
    let state = target.state();
    let interrupted = state.and_then(|s| s.pending.as_ref()).is_some_and(|p| p.op == op);
    let changed = rewrites
        && !interrupted
        && state.and_then(|s| s.head()).is_some_and(|h| !scanned.fingerprints().any(|fp| fp == h.fingerprint));
    // Its warnings say OneLibrary couldn't be read.
    let unreadable_onelibrary = !on_drive.warnings.is_empty();
    let unsaved = changed || unreadable_onelibrary || !history_lists.is_empty();
    Ok(unsaved.then_some(Unsaved { history_lists, changed, unreadable_onelibrary }))
}

#[derive(Debug, Clone, Default, Serialize, JsonSchema)]
pub struct PackReport {
    pub copied: usize,
    pub copied_bytes: u64,
    /// Copies made for the players (see [`PackPlan::transcodes`]).
    pub transcoded: usize,
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

    // The files, then the copies made for the players, in one copy phase.
    let new_copies: Vec<&PackTranscode> = plan.transcodes.iter().filter(|t| !t.current).collect();
    ctx.phase_start(
        Phase::Copy,
        (plan.copies.len() + new_copies.len()) as u64,
        plan.copy_bytes + new_copies.iter().map(|t| t.job.source_size).sum::<u64>(),
    );
    (report.copied, report.copied_bytes) = library_write::copy(target, &plan.written, ctx)?;
    let moves = transcode::make(&plan.transcodes, target, ctx)?;
    report.transcoded = new_copies.len();
    ctx.phase_end(Phase::Copy);
    let repointed;
    let databases = match moves.is_empty() {
        true => &plan.databases,
        false => {
            let (pdb, plus) =
                edit::repoint_in_memory(source, plan.databases.pdb.as_deref(), plan.databases.plus.as_deref(), &moves)?;
            repointed = Databases { pdb, ext: plan.databases.ext.clone(), plus };
            &repointed
        }
    };

    // The databases, then removal of what the pack no longer uses. The pack is made from here:
    // recording it isn't cancellable.
    let ctx = &ctx.uncancellable();
    library_write::swap_databases(target, databases)?;
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
        transcode: plan.transcode.clone(),
    };
    fsutil::write_json_atomic(&target.state_dir().join(PACK_FILE), &rec)?;
    crate::collection_export::forget(target)?;
    edit::journal::forget(target)?;
    if report.backup.is_some() {
        backup::prune(target)?;
    }
    Ok(report)
}
