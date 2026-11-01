//! Backups kept on the drive itself, taken before locallibrary changes it in place (edits, My
//! Tags, cues, merge, clean, convert, artwork, settings, the player name, the contact file,
//! orphan removal, packs, and putting a backup back). They contain the database files plus every
//! existing file the change overwrites or deletes (artwork images, analysis files, settings
//! files, removed tracks' files), so restoring one undoes the change (`locallibrary undo`): files
//! that were added stay on the drive but are no longer referenced. A backup also carries the
//! drive's journal of edits (see [`crate::edit::journal`]) and the player name and colour it
//! remembers (`Carried`), and restoring one puts those back as they were: `edits replay` then
//! makes again exactly the edits the restored library had, also after an undo is redone. Restoring
//! a backup an earlier version took, which carries neither, takes the edits it undoes off the
//! journal instead.
//!
//! Once its change is made, a backup notes what the change left in each file putting it back
//! would replace or remove ([`BackupInfo::after`]); a replay notes it after each of its steps.
//! Putting it back is refused, unless forced, while the drive holds something else there (a
//! rekordbox export, a sync or push, history lists players recorded): that would be lost with
//! the change ([`choose`]). Putting a backup back takes a backup of what it replaces first, so an
//! undo can itself be put back (a redo), and `undo` again steps back to the change before.
//!
//! Backups are numbered in the order they're made on the drive (computers' clocks differ), and
//! the newest [`KEEP`] of each kind are kept. Kinds split two ways: backups that hold removed
//! tracks' audio are counted apart from those that don't, so a run of small edits never prunes
//! the only copy of a deleted track, and the ones undos take apart from the others.

use indexmap::IndexMap;
use std::collections::BTreeSet;

use rustc_hash::FxHashMap;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::commit::{self, Changes, Content, NewFile, Options};
use crate::device::{Device, LineageOp};
use crate::edit::journal::{self, JournalEntry};
use crate::error::IoContext;
use crate::error::{Action, Reason};
use crate::fsutil;
use crate::hash::{self, Hash};
use crate::local_time;
use crate::paths::{self, FileClass};
use crate::player::DbIdentity;
use crate::progress::{Ctx, Phase};
use crate::{Error, Result};

/// Backups kept of each kind: holding audio or not, taken by an undo or not.
pub const KEEP: usize = 10;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct BackupInfo {
    pub id: String,
    pub created: DateTime<Utc>,
    pub reason: String,
    pub files: Vec<String>,
    /// Its place among the drive's backups, counting up (0: made by a version that didn't number
    /// them).
    #[serde(default)]
    pub seq: u64,
    /// How many of `files` are audio (removed tracks' files, which may be their only copy).
    #[serde(default)]
    pub audio: usize,
    /// When an undo took it: the backup that undo put back. Putting this one back redoes what the
    /// undo took back; `undo` passes it by.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub undoes: Option<String>,
    /// Each of `files` as it was backed up, by content.
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    pub hashes: IndexMap<String, Hash>,
    /// What its change left in each file putting it back replaces or removes, by content (`null`:
    /// absent). Missing when the change didn't finish, or an earlier version made the backup.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after: Option<IndexMap<String, Option<Hash>>>,
}

fn backups_dir(device: &Device) -> PathBuf {
    device.state_dir().join("backups")
}

/// Where a backup's files are, laid out like the drive (open it with [`Device::open`] to read it).
pub fn dir(device: &Device, id: &str) -> PathBuf {
    backups_dir(device).join(id)
}

fn audio_files(files: &[String]) -> usize {
    files.iter().filter(|k| paths::classify(k) == Some(FileClass::Audio)).count()
}

fn write_info(dir: &Path, info: &BackupInfo) -> Result<()> {
    fsutil::write_json_atomic(&dir.join("backup.json"), info)
}

/// What a backup carries besides the drive's files, in `carried.json` beside `backup.json`: the
/// player name and colour the drive remembered (see [`crate::player`]). The drive's journal, with
/// the files its edits staged, is copied below the backup's own state folder, as on a drive. A
/// backup without it was taken by an earlier version.
#[derive(Debug, Default, Serialize, Deserialize)]
struct Carried {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    player: Option<DbIdentity>,
}

const CARRIED: &str = "carried.json";

/// Copy into the backup at `dir` what it carries (see [`Carried`]). A journal this build can't
/// read isn't carried: restoring refuses that drive anyway, as it can't change its journal.
fn carry(device: &Device, dir: &Path) -> Result<()> {
    if journal::list(device).is_err() {
        return Ok(());
    }
    std::fs::create_dir_all(dir).at(dir)?;
    journal::copy(device, &Device::open(dir)?)?;
    let player = device.state().and_then(|s| s.player.clone());
    fsutil::write_json_atomic(&dir.join(CARRIED), &Carried { player })
}

fn carried(device: &Device, info: &BackupInfo) -> Result<Option<Carried>> {
    fsutil::read_json_opt(&dir(device, &info.id).join(CARRIED))
}

/// The journal `info` carries (`None`: an earlier version took it, without one).
pub(crate) fn carried_journal(device: &Device, info: &BackupInfo) -> Result<Option<Vec<JournalEntry>>> {
    match carried(device, info)? {
        Some(_) => Ok(Some(journal::list(&Device::open(dir(device, &info.id))?)?)),
        None => Ok(None),
    }
}

/// Put back what `info` carries, now that its files are back: the journal and the player name and
/// colour as the drive had them. For a backup carrying neither, take the edits it undoes off the
/// journal, and remember the name and colour the drive shows now (`shown`: before).
fn put_back_carried(device: &mut Device, info: &BackupInfo, shown: Option<DbIdentity>) -> Result<()> {
    let Some(carried) = carried(device, info)? else {
        journal::undone(device, info)?;
        return crate::player::after_undo(device, shown);
    };
    journal::copy(&Device::open(dir(device, &info.id))?, device)?;
    if device.state().and_then(|s| s.player.as_ref()) != carried.player.as_ref() {
        device.state_mut()?.player = carried.player;
        device.save_state()?;
    }
    Ok(())
}

/// Copy the database files and `extra` (keys of files the coming change overwrites or deletes)
/// into `.locallibrary/backups/<id>/`, with their modification times: a track put back must
/// not look changed to the next sync. `undoes`: the backup the change puts back, if it's an
/// undo.
///
/// Every older backup is kept: a write that fails before it changes anything takes its backup
/// away again and must leave the others as they were. [`seal`] it once the change is made, and
/// [`prune`] then.
pub(crate) fn create(device: &Device, reason: &str, extra: &[String], undoes: Option<&str>) -> Result<BackupInfo> {
    let seq = list(device)?.iter().map(|b| b.seq).max().unwrap_or(0) + 1;
    let created = Utc::now();
    let mut id = created.format("%Y%m%dT%H%M%S%.3fZ").to_string();
    let dir = loop {
        let d = backups_dir(device).join(&id);
        if !d.exists() {
            break d;
        }
        id.push('_');
    };
    let mut files = Vec::new();
    let mut hashes = IndexMap::new();
    let keys = paths::DATABASE_FILES.iter().map(|k| k.to_string()).chain(extra.iter().cloned());
    for key in keys {
        let src = device.checked_path(&key)?;
        if src.is_file() && !hashes.contains_key(&key) {
            let meta = std::fs::metadata(&src).at(&src)?;
            let dst = paths::to_fs(&dir, &key);
            let h = fsutil::copy_file(&src, &dst, Some(fsutil::mtime_secs(&meta)), None, true, &Ctx::silent())?;
            hashes.insert(key.clone(), h);
            files.push(key);
        }
    }
    carry(device, &dir)?;
    let info = BackupInfo {
        id,
        created,
        reason: reason.to_string(),
        audio: audio_files(&files),
        files,
        seq,
        undoes: undoes.map(str::to_string),
        hashes,
        after: None,
    };
    write_info(&dir, &info)?;
    Ok(info)
}

/// Take backup `id` away again, its change having failed before it changed anything: with the
/// backups folder when that leaves it empty.
pub(crate) fn discard(device: &Device, id: &str) -> Result<()> {
    let dir = dir(device, id);
    std::fs::remove_dir_all(&dir).at(&dir)?;
    let _ = std::fs::remove_dir(backups_dir(device));
    Ok(())
}

/// Space on the drive [`create`] needs for a backup with `extra`, each file rounded up to the
/// drive's allocation unit, so a plan can tell a drive too full for it before it starts.
pub(crate) fn needed_bytes<'a>(device: &Device, extra: impl IntoIterator<Item = &'a str>) -> u64 {
    let cluster = fsutil::cluster_size(device.root());
    let size = |key: &str| std::fs::metadata(device.path(key)).map(|m| m.len()).unwrap_or(0);
    paths::DATABASE_FILES.into_iter().chain(extra).map(|k| size(k).div_ceil(cluster) * cluster).sum()
}

/// The drive's backups, newest first.
pub fn list(device: &Device) -> Result<Vec<BackupInfo>> {
    let dir = backups_dir(device);
    if !dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for e in std::fs::read_dir(&dir).at(&dir)? {
        let e = e.at(&dir)?;
        if let Ok(Some(mut info)) = fsutil::read_json_opt::<BackupInfo>(&e.path().join("backup.json")) {
            // The directory name is authoritative (it's what prune and restore operate on).
            info.id = e.file_name().to_string_lossy().into_owned();
            info.audio = audio_files(&info.files);
            out.push(info);
        }
    }
    out.sort_by_key(|b| std::cmp::Reverse((b.seq, b.created)));
    Ok(out)
}

/// Remove all but the newest [`KEEP`] backups of each kind, by whether they hold audio and
/// whether an undo took them (a redo). Undoing many changes then doesn't prune the ones before
/// them.
pub(crate) fn prune(device: &Device) -> Result<()> {
    let backups = list(device)?;
    let kind = |b: &BackupInfo| (b.audio > 0, b.undoes.is_some());
    let mut kept: FxHashMap<(bool, bool), usize> = FxHashMap::default();
    for b in &backups {
        let n = kept.entry(kind(b)).or_default();
        *n += 1;
        if *n > KEEP {
            let d = backups_dir(device).join(&b.id);
            std::fs::remove_dir_all(&d).at(&d)?;
        }
    }
    Ok(())
}

/// The files putting `info` back replaces or removes: its own, and every database file.
fn keys(info: &BackupInfo) -> BTreeSet<String> {
    paths::DATABASE_FILES.iter().map(|k| k.to_string()).chain(info.files.iter().cloned()).collect()
}

/// Files on the drive by content, each hashed once (`None`: absent or unreadable).
#[derive(Default)]
struct Contents(FxHashMap<String, Option<Hash>>);

impl Contents {
    fn of(&mut self, device: &Device, key: &str) -> Option<Hash> {
        *self.0.entry(key.to_string()).or_insert_with(|| {
            let path = device.checked_path(key).ok().filter(|p| p.is_file())?;
            hash::hash_file(&path, &Ctx::silent(), Phase::Verify).ok()
        })
    }

    /// Whether `key` holds something other than `expected` (`None`: nothing). Where the change
    /// removed the file, being there is enough, and nothing is read: undoing a big `track rm`
    /// puts all that audio back.
    fn differs(&mut self, device: &Device, key: &str, expected: Option<Hash>) -> bool {
        match expected {
            None => device.checked_path(key).is_ok_and(|p| p.is_file()),
            Some(_) => self.of(device, key) != expected,
        }
    }
}

/// Note in backup `id` what its change left, now that the change is made (see
/// [`BackupInfo::after`]).
pub(crate) fn seal(device: &Device, id: &str) -> Result<()> {
    let dir = dir(device, id);
    let mut info: BackupInfo = fsutil::read_json(&dir.join("backup.json"))?;
    let mut now = Contents::default();
    info.after = Some(keys(&info).into_iter().map(|k| (k.clone(), now.of(device, &k))).collect());
    write_info(&dir, &info)
}

/// The files that differ from what `info`'s change left (empty: none), or `None` when the backup
/// doesn't say what that was.
fn changed(device: &Device, info: &BackupInfo, now: &mut Contents) -> Option<Vec<String>> {
    let after = info.after.as_ref()?;
    Some(after.iter().filter(|(k, h)| now.differs(device, k, **h)).map(|(k, _)| k.clone()).collect())
}

/// Whether putting `info` back would leave every file as its change did: the change only added
/// files (the contact file, say), which stay.
fn takes_nothing_back(info: &BackupInfo) -> bool {
    !info.hashes.is_empty()
        && info.after.as_ref().is_some_and(|after| after.iter().all(|(k, h)| *h == info.hashes.get(k).copied()))
}

/// The backup to put back: `id`, or the one `undo` puts back when it's `None`. That's the one
/// the drive's interrupted write made, which recovers from it, or else the newest whose change the
/// drive still holds (an undo's own backup is a redo, and passed by), passing by those whose
/// change only added files while an older one takes something back.
///
/// Refused unless `force` when the drive changed since that change (see the module docs): a
/// rekordbox export, a sync, or history players recorded would be lost with it. A given `id` may
/// also be one before locallibrary's later changes the drive still holds: putting it back takes
/// those back too, as the plan shows.
pub fn choose(device: &Device, id: Option<&str>, force: bool) -> Result<BackupInfo> {
    let mut backups = list(device)?;
    let mut now = Contents::default();
    let cut_off = device.state().and_then(|s| s.pending.as_ref()).and_then(|p| p.backup.clone());
    let mut holds = |b: &BackupInfo| {
        cut_off.as_deref() == Some(b.id.as_str()) || changed(device, b, &mut now).is_some_and(|c| c.is_empty())
    };
    if let Some(id) = id {
        let i = backups.iter().position(|b| b.id == id).ok_or_else(|| Error::NotFound(format!("backup {id}")))?;
        if force || backups[..=i].iter().any(&mut holds) {
            return Ok(backups.swap_remove(i));
        }
        return Err(changed_since(device, &backups[i]));
    }
    if backups.is_empty() {
        return Err(Error::NotFound(format!("anything to undo on {}: it has no backups", device.name())));
    }
    // An interrupted write's backup recovers from it, even an undo's own.
    if let Some(b) = backups.iter().find(|b| cut_off.as_deref() == Some(b.id.as_str())) {
        return Ok(b.clone());
    }
    let undoable = |b: &&BackupInfo| b.undoes.is_none();
    let pick = match force {
        true => backups.iter().find(undoable),
        false => {
            let held: Vec<&BackupInfo> = backups.iter().filter(undoable).filter(|b| holds(b)).collect();
            held.iter().find(|b| !takes_nothing_back(b)).or(held.first()).copied()
        }
    };
    match pick {
        Some(b) => Ok(b.clone()),
        None if backups.iter().any(&mut holds) => Err(Error::NotFound(format!(
            "anything more to undo on {}: the changes its backups were taken for are undone already",
            device.name()
        ))),
        None => Err(changed_since(device, backups.iter().find(undoable).unwrap_or(&backups[0]))),
    }
}

/// Refusing to put `info` back over a drive that changed since its change, saying how.
fn changed_since(device: &Device, info: &BackupInfo) -> Error {
    let files = changed(device, info, &mut Contents::default());
    let message = match &files {
        Some(f) => {
            let more = if f.len() > 2 { format!(" and {} more file(s)", f.len() - 2) } else { String::new() };
            // Whole libraries copied onto it since take no backup of their own: name them.
            let copied = device.state().and_then(|s| {
                s.lineage.iter().rev().find(|e| {
                    e.at > info.created
                        && matches!(e.op, LineageOp::Sync | LineageOp::Push | LineageOp::Pack | LineageOp::Export)
                })
            });
            let by = match copied {
                Some(e) => {
                    format!(
                        "after a {} on {}",
                        format!("{:?}", e.op).to_lowercase(),
                        local_time(e.at, "%Y-%m-%d %H:%M")
                    )
                }
                None => "perhaps by a rekordbox export, a sync or players recording history".into(),
            };
            format!(
                "{} changed since backup {} ({}) was taken for its change: {}{more} changed, {by}. Putting it back \
                 would take that back too",
                device.name(),
                info.id,
                info.reason,
                f.iter().take(2).cloned().collect::<Vec<_>>().join(", ")
            )
        }
        None => format!(
            "backup {} ({}) doesn't say what its change left on {} (the change didn't finish, or an earlier version \
             of locallibrary took the backup), so whether anything changed the drive since can't be told",
            info.id,
            info.reason,
            device.name()
        ),
    };
    Error::conflict(Reason::ChangedSince, format!("{message}; force it to put it back anyway"))
        .with("drive", device.root().display().to_string())
        .with("backup", &info.id)
        .with("changed", files.unwrap_or_default())
        .action(Action::retry(serde_json::json!({ "force": true }), "Put it back anyway"))
}

/// Restore a backup (the one `undo` puts back when `id` is `None`; see [`choose`]).
///
/// Refuses (unless `force`) when the drive changed since the backup's change, or the backed-up
/// database references audio that is neither on the drive nor in the backup, e.g. because a
/// later sync or push replaced the library.
pub fn restore(device: &mut Device, id: Option<&str>, force: bool) -> Result<BackupInfo> {
    restore_with(device, id, force, &Ctx::silent())
}

/// [`restore`], reporting progress to `ctx`.
pub fn restore_with(device: &mut Device, id: Option<&str>, force: bool, ctx: &Ctx<'_>) -> Result<BackupInfo> {
    let _lock = commit::lock(device, None)?;
    // The journal must be one this build can change, as the edits undone come off it.
    journal::list(device)?;
    let info = choose(device, id, force)?;
    let dir = backups_dir(device).join(&info.id);
    if !force {
        let missing = missing_audio(device, &info)?;
        if !missing.is_empty() {
            return Err(Error::conflict(
                Reason::MissingAudio,
                format!(
                    "{} track(s) in backup {} reference audio that is no longer on the drive (e.g. {}); the library was \
                     probably replaced since. Force it to restore it anyway",
                    missing.len(),
                    info.id,
                    missing[0]
                ),
            )
            .with("missing", &missing)
            .action(Action::retry(serde_json::json!({ "force": true }), "Restore it anyway")));
        }
    }
    // The backup's files go back through the drive's write routine as they were backed up (their
    // content checked, their modification times kept): staged, then swapped in, with OneLibrary's
    // current WAL removed first (it must never sit next to the restored database; the backup's
    // own WAL, if any, goes in after its database). Database files the backup didn't have didn't
    // exist when it was taken, and go. What they replace is backed up first.
    let mut files = Vec::new();
    for key in &info.files {
        let src = paths::to_fs(&dir, key);
        let meta = std::fs::metadata(&src).at(&src)?;
        files.push(NewFile {
            key: key.clone(),
            mtime: Some(fsutil::mtime_secs(&meta)),
            size: meta.len(),
            content: Content::Copy { path: src, expected: info.hashes.get(key).copied() },
            optional: false,
        });
    }
    let deletes = paths::DATABASE_FILES
        .iter()
        .filter(|k| !info.files.iter().any(|f| f == *k) && device.path(k).is_file())
        .map(|k| k.to_string())
        .collect();
    let remove_first = vec![paths::LIBRARY_PLUS_WAL.to_string(), paths::LIBRARY_PLUS_SHM.to_string()];
    let changes = Changes { files, deletes, remove_first, ..Default::default() };
    let mut opts = Options::new(LineageOp::Restore, format!("restore backup {}", info.id))
        .backup(format!("before putting back {}", info.id));
    opts.undoes = Some(info.id.clone());
    // Read only when the drive remembers a player name and the backup doesn't carry it, as
    // reading it decrypts OneLibrary.
    let remembers = device.state().is_some_and(|s| s.player.is_some()) && carried(device, &info)?.is_none();
    let shown = remembers.then(|| crate::player::db_identity(device.root())).flatten();
    commit::commit(device, changes, opts, ctx)?;
    put_back_carried(device, &info, shown)?;
    Ok(info)
}

/// Tracks a backup's database lists whose audio is neither on the drive nor in the backup
/// (restoring refuses them unless forced), by name.
pub fn missing_audio(device: &Device, info: &BackupInfo) -> Result<Vec<String>> {
    let old = Device::open(dir(device, &info.id))?.library()?;
    let saved: rustc_hash::FxHashSet<String> = info.files.iter().map(|f| f.to_lowercase()).collect();
    Ok(old
        .tracks
        .iter()
        .map(|t| (t, paths::key(&t.file_path)))
        .filter(|(_, k)| !saved.contains(&k.to_lowercase()))
        .filter(|(_, k)| !device.checked_path(k).is_ok_and(|p| p.exists()))
        .map(|(t, _)| t.display_name())
        .collect())
}
