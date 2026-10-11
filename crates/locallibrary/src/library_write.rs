//! Writing a whole library onto a drive, for the writers that replace a drive's library with one
//! made elsewhere ([`crate::pack`] from another drive, [`crate::collection_export`] from
//! rekordbox's collection): the files it needs, copied when the drive's copy differs in size or
//! modification time, its databases swapped in back to back, then the library files it no longer
//! uses removed. Each writer backs up, records and reports around these steps itself.
//!
//! Files are flushed before they get their source's modification time, which tells the next run
//! they're complete (see [`fsutil::copy_file`]); a run stopped part way still does both for what it
//! copied, so running it again copies only the rest.

use indexmap::IndexMap;
use rustc_hash::{FxHashMap, FxHashSet};
use std::io::Write;
use std::path::{Path, PathBuf};

use schemars::JsonSchema;
use serde::Serialize;

use crate::anlz;
use crate::commit::Swap;
use crate::device::Device;
use crate::error::IoContext;
use crate::paths::{self, FileClass};
use crate::progress::{Ctx, Event, Phase};
use crate::tree::FileTree;
use crate::{Error, Result, backup, fsutil};

/// A file a write copies or removes.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct FileCopy {
    pub path: String,
    pub size: u64,
}

/// A file the library needs on the drive, and where it comes from.
#[derive(Debug, Clone)]
pub(crate) struct Source {
    pub(crate) path: PathBuf,
    /// Its size on the drive (an analysis file whose path is rewritten changes size).
    pub(crate) size: u64,
    pub(crate) mtime: i64,
    /// For an analysis file: the audio path to write into it (its `PPTH` tag).
    pub(crate) audio_path: Option<String>,
}

impl Source {
    /// A file copied as it is.
    pub(crate) fn file(path: PathBuf, size: u64, mtime: i64) -> Source {
        Source { path, size, mtime, audio_path: None }
    }
}

fn analysis_bytes(path: &Path, audio_path: &str) -> Result<Vec<u8>> {
    let bytes = std::fs::read(path).at(path)?;
    anlz::rewrite_path(&bytes, audio_path).map_err(|e| match e {
        Error::Parse { message, .. } => Error::parse(path, message),
        e => e,
    })
}

/// Write `bytes` to `dst` through a temp file beside it, without an fsync ([`fsutil::flush`]
/// follows, as for copies).
fn write_unsynced(dst: &Path, bytes: &[u8]) -> Result<()> {
    let dir = dst.parent().ok_or_else(|| Error::invalid(format!("{} has no parent", dst.display())))?;
    std::fs::create_dir_all(dir).at(dir)?;
    let mut tmp = fsutil::temp_for(dir, dst, "")?;
    tmp.write_all(bytes).at(tmp.path())?;
    tmp.persist(dst).map_err(|e| Error::Io { path: dst.to_path_buf(), source: e.error })?;
    Ok(())
}

/// The new databases. Without `pdb` or `plus`, the drive's is removed; without `ext`, the drive's
/// is left to the files the library needs (a writer copies it, or it's removed with the rest).
#[derive(Debug, Clone, Default)]
pub(crate) struct Databases {
    pub(crate) pdb: Option<Vec<u8>>,
    pub(crate) ext: Option<Vec<u8>>,
    pub(crate) plus: Option<Vec<u8>>,
}

impl Databases {
    /// The database files written (lower-cased keys), which the drive keeps.
    fn written(&self) -> FxHashSet<String> {
        [
            self.pdb.as_ref().map(|_| fold(paths::EXPORT_PDB)),
            self.ext.as_ref().map(|_| fold(paths::EXPORT_EXT_PDB)),
            self.plus.as_ref().map(|_| fold(paths::LIBRARY_PLUS_DB)),
            self.plus.as_ref().map(|_| fold(paths::LIBRARY_PLUS_WAL)),
        ]
        .into_iter()
        .flatten()
        .collect()
    }
}

/// What writing a library to a drive copies, keeps and removes.
#[derive(Debug, Clone, Default)]
pub(crate) struct Planned {
    pub(crate) copies: Vec<FileCopy>,
    pub(crate) copy_bytes: u64,
    pub(crate) deletes: Vec<FileCopy>,
    pub(crate) delete_bytes: u64,
    pub(crate) unchanged: usize,
    /// Size of the library's files, its databases included.
    pub(crate) library_bytes: u64,
    /// Space the write needs on the drive: the files it copies, its new databases and the backup
    /// of what it replaces.
    pub(crate) needed_bytes: u64,
    pub(crate) free_space: Option<u64>,
    pub(crate) fits: bool,
    /// Source of each file to copy.
    pub(crate) sources: IndexMap<String, Source>,
    /// Files on the drive the write overwrites or removes that its backup keeps: analysis and
    /// artwork (audio isn't backed up).
    pub(crate) backed_up: Vec<String>,
}

/// Plan writing `wanted` (the library's files by key) and `databases` to `target`, whose files are
/// `tree`. Library files the target has that `wanted` doesn't are removed unless `keep_extra`.
pub(crate) fn plan(
    target: &Device,
    tree: &FileTree,
    wanted: &IndexMap<String, Source>,
    databases: &Databases,
    keep_extra: bool,
) -> Planned {
    let by_fold: FxHashMap<String, &crate::tree::FileEntry> =
        tree.library_entries().map(|e| (fold(&e.path), e)).collect();
    let backs_up = |class: FileClass| matches!(class, FileClass::Analysis | FileClass::Artwork);
    // After a write cut off on the target, a file modified since it started may be torn.
    let cut_off = crate::sync::cut_off_since(target);
    let mut out = Planned::default();
    for (key, src) in wanted {
        match by_fold.get(&fold(key)) {
            Some(e)
                if e.size == src.size
                    && fsutil::mtime_eq(e.mtime, src.mtime)
                    && cut_off.is_none_or(|since| e.mtime < since) =>
            {
                out.unchanged += 1
            }
            existing => {
                if let Some(e) = existing.filter(|e| backs_up(e.class)) {
                    out.backed_up.push(e.path.clone());
                }
                out.copies.push(FileCopy { path: key.clone(), size: src.size });
                out.sources.insert(key.clone(), src.clone());
            }
        }
    }
    let written = databases.written();
    let wanted_folds: FxHashSet<String> = wanted.keys().map(|k| fold(k)).collect();
    if !keep_extra {
        out.deletes = tree
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
            .map(|e| FileCopy { path: e.path.clone(), size: e.size })
            .collect();
    }
    out.backed_up
        .extend(out.deletes.iter().filter(|d| paths::classify(&d.path).is_some_and(backs_up)).map(|d| d.path.clone()));
    let cluster = fsutil::cluster_size(target.root());
    let round = |n: u64| n.div_ceil(cluster) * cluster;
    let db_bytes = [&databases.pdb, &databases.ext, &databases.plus]
        .into_iter()
        .map(|b| round(b.as_ref().map_or(0, |b| b.len() as u64)))
        .sum::<u64>();
    out.copy_bytes = out.copies.iter().map(|c| c.size).sum();
    out.delete_bytes = out.deletes.iter().map(|d| d.size).sum();
    out.library_bytes = wanted.values().map(|s| s.size).sum::<u64>() + db_bytes;
    // The backup a write takes of a target that holds a library is on the target too.
    let backup_bytes =
        if target.has_export() { backup::needed_bytes(target, out.backed_up.iter().map(String::as_str)) } else { 0 };
    out.needed_bytes = out.copies.iter().map(|c| round(c.size)).sum::<u64>() + db_bytes + backup_bytes;
    out.free_space = fsutil::available_space(target.root());
    out.fits = out.free_space.is_none_or(|f| out.needed_bytes <= f);
    out
}

/// Copy what `planned` copies to `target`, flushed and then given their sources' modification
/// times; returns how many files and bytes were copied (what was copied before an error too).
pub(crate) fn copy(target: &Device, planned: &Planned, ctx: &Ctx<'_>) -> Result<(usize, u64)> {
    ctx.phase_start(Phase::Copy, planned.copies.len() as u64, planned.copy_bytes);
    let mut written: Vec<(PathBuf, i64)> = Vec::new();
    let mut bytes = 0;
    let mut stopped = None;
    for c in &planned.copies {
        let copied = (|| -> Result<(PathBuf, i64)> {
            ctx.check()?;
            let src = planned.sources.get(&c.path).ok_or_else(|| Error::NotFound(c.path.clone()))?;
            let dst = target.checked_path(&c.path)?;
            ctx.emit(Event::Item { phase: Phase::Copy, path: c.path.clone(), bytes: c.size });
            match &src.audio_path {
                Some(audio) => write_unsynced(&dst, &analysis_bytes(&src.path, audio)?)?,
                None => {
                    fsutil::copy_file(&src.path, &dst, None, None, false, ctx)?;
                }
            }
            Ok((dst, src.mtime))
        })();
        match copied {
            Ok(w) => {
                written.push(w);
                bytes += c.size;
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
    Ok((written.len(), bytes))
}

/// Swap `databases` in, back to back (see [`crate::commit::swap`]); OneLibrary's old WAL goes
/// before its new database (see `onelibrary::write`), and a database that's `None` is removed.
pub(crate) fn swap_databases(target: &Device, databases: &Databases) -> Result<()> {
    let at = |key: &str| target.checked_path(key);
    let mut steps = Vec::new();
    if let Some(bytes) = &databases.pdb {
        steps.push(Swap::bytes(at(paths::EXPORT_PDB)?, bytes)?);
    }
    if let Some(bytes) = &databases.ext {
        steps.push(Swap::bytes(at(paths::EXPORT_EXT_PDB)?, bytes)?);
    }
    for key in [paths::LIBRARY_PLUS_WAL, paths::LIBRARY_PLUS_SHM] {
        steps.push(Swap::Remove(at(key)?));
    }
    steps.push(match &databases.plus {
        Some(bytes) => Swap::bytes(at(paths::LIBRARY_PLUS_DB)?, bytes)?,
        None => Swap::Remove(at(paths::LIBRARY_PLUS_DB)?),
    });
    if databases.pdb.is_none() {
        steps.push(Swap::Remove(at(paths::EXPORT_PDB)?));
    }
    crate::commit::swap(steps)
}

/// Remove what `planned` removes from `target` (and folders left empty); returns how many files
/// and bytes went.
pub(crate) fn delete(target: &Device, planned: &Planned, ctx: &Ctx<'_>) -> Result<(usize, u64)> {
    if planned.deletes.is_empty() {
        return Ok((0, 0));
    }
    let (mut n, mut bytes) = (0, 0);
    ctx.phase_start(Phase::Delete, planned.deletes.len() as u64, planned.delete_bytes);
    for d in &planned.deletes {
        let p = target.checked_path(&d.path)?;
        ctx.emit(Event::Item { phase: Phase::Delete, path: d.path.clone(), bytes: d.size });
        match std::fs::remove_file(&p) {
            Ok(()) => {
                n += 1;
                bytes += d.size;
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(Error::Io { path: p, source: e }),
        }
        if let Some(parent) = p.parent() {
            fsutil::prune_empty_dirs(parent, target.root());
        }
    }
    ctx.phase_end(Phase::Delete);
    Ok((n, bytes))
}

fn fold(s: &str) -> String {
    s.to_lowercase()
}
