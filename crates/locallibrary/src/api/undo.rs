//! Undo: put back the backup of the last change locallibrary made to a drive that the drive
//! still holds (see [`crate::backup::choose`]), after showing what that changes: the files and
//! databases, the playlists and tracks, the history lists dropped, and the recorded edits taken
//! off the journal. Writers that change a drive in place back it up first (not repairs, firmware
//! files, recordings or `drive tidy`'s deletions), so the last change can be taken back, and the
//! one before it next. A drive that changed since (a rekordbox
//! export, a sync, players at a set) is refused unless forced, as that would be lost too.

use std::path::PathBuf;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::{known_drives, resolve_drive};
use crate::artwork::{self, EmbedBackup};
use crate::backup::{self, BackupInfo};
use crate::device::Device;
use crate::diff::PlaylistDiff;
use crate::edit::journal::{self, JournalEntry};
use crate::error::{Action, Reason};
use crate::hash::{self, Hash};
use crate::local_time;
use crate::op::{Operation, Planned, Probe, Touched, bytes};
use crate::paths;
use crate::progress::{Ctx, Phase};
use crate::store::Store;
use crate::{Error, Result};

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct UndoRequest {
    /// The drive [default: the only mounted drive with backups].
    #[serde(default)]
    pub drive: Option<String>,
    /// A backup other than the one `undo` takes (see [`super::list_backups`]).
    #[serde(default)]
    pub backup: Option<String>,
    /// Put it back even if the drive changed since its change, or its database lists audio
    /// that's no longer on the drive (the library was replaced since).
    #[serde(default)]
    pub force: bool,
}

/// A file undoing changes.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct FileChange {
    /// The file, as a drive key.
    pub path: String,
    /// `put back` (it's gone), `replaced` (it differs) or `removed` (a database the backup didn't
    /// have).
    pub change: &'static str,
    pub bytes: u64,
}

/// What putting a backup back would change.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct UndoPlan {
    pub drive: String,
    pub root: PathBuf,
    pub backup: BackupInfo,
    /// The files that change.
    pub files: Vec<FileChange>,
    /// How the playlists and tracks change, from the drive as it is to the drive put back.
    pub diff: PlaylistDiff,
    /// History lists on the drive that the backup doesn't have: putting it back drops them.
    pub history_lists: Vec<String>,
    /// Recorded edits taken off the journal: `edits replay` won't make them again.
    pub journal: Vec<JournalEntry>,
    /// Tracks the backup lists whose audio is neither on the drive nor in the backup: putting it
    /// back is refused unless forced.
    pub missing_audio: Vec<String>,
}

/// The drive an undo is about: the one named, or the only mounted drive with backups.
fn undo_drive(spec: Option<&str>) -> Result<Device> {
    if let Some(spec) = spec {
        return resolve_drive(spec);
    }
    let mut with: Vec<Device> =
        known_drives(false).into_iter().filter(|d| backup::list(d).is_ok_and(|b| !b.is_empty())).collect();
    match with.len() {
        0 => Err(Error::NotFound("a mounted drive with something to undo".into())),
        1 => Ok(with.remove(0)),
        n => Err(Error::invalid(format!(
            "{n} drives have something to undo ({}): say which",
            with.iter().map(Device::name).collect::<Vec<_>>().join(", ")
        ))
        .because(Reason::ChooseDrive)
        .with("drives", with.iter().map(|d| d.root().display().to_string()).collect::<Vec<_>>())
        .action(Action::call("undo.list", serde_json::json!({}), "Show what can be undone"))),
    }
}

/// Whether the drive's file `now` holds what the backup's copy `from` does. The copy is known by
/// the hash the backup recorded for it, when it has one (putting it back checks the copy against
/// it), so only the drive's file is read.
fn same_file(from: &std::path::Path, recorded: Option<Hash>, now: &std::path::Path) -> bool {
    let size = |p: &std::path::Path| std::fs::metadata(p).map(|m| m.len()).ok();
    if size(from) != size(now) {
        return false;
    }
    let hash = |p: &std::path::Path| hash::hash_file(p, &Ctx::silent(), Phase::Verify).ok();
    let backed_up = recorded.or_else(|| hash(from));
    backed_up.is_some() && hash(now) == backed_up
}

/// What undoing would change: putting back the backup [`backup::choose`] picks (or `backup`).
pub fn undo_plan(req: &UndoRequest) -> Result<Planned<UndoPlan, bool>> {
    let d = undo_drive(req.drive.as_deref())?;
    let info = backup::choose(&d, req.backup.as_deref(), req.force)?;
    let dir = backup::dir(&d, &info.id);
    let missing_audio = backup::missing_audio(&d, &info)?;
    if !missing_audio.is_empty() && !req.force {
        return Err(Error::conflict(
            Reason::MissingAudio,
            format!(
                "{} track(s) in backup {} reference audio that is no longer on the drive (e.g. {}); the library was \
                 probably replaced since. Force it to put the backup back anyway",
                missing_audio.len(),
                info.id,
                missing_audio[0]
            ),
        )
        .with("missing", &missing_audio)
        .action(Action::retry(serde_json::json!({ "force": true }), "Put it back anyway")));
    }
    let mut files = Vec::new();
    for key in &info.files {
        let from = paths::to_fs(&dir, key);
        let now = d.checked_path(key)?;
        let bytes = std::fs::metadata(&from).map(|m| m.len()).unwrap_or(0);
        if !now.is_file() {
            files.push(FileChange { path: key.clone(), change: "put back", bytes });
        } else if !same_file(&from, info.hashes.get(key).copied(), &now) {
            files.push(FileChange { path: key.clone(), change: "replaced", bytes });
        }
    }
    for key in paths::DATABASE_FILES {
        if !info.files.iter().any(|f| f == key) && d.path(key).is_file() {
            let bytes = std::fs::metadata(d.path(key)).map(|m| m.len()).unwrap_or(0);
            files.push(FileChange { path: key.to_string(), change: "removed", bytes });
        }
    }
    let put_back = Device::open(&dir)?;
    let label = format!("backup {}", info.id);
    let diff = super::diff::to_backup(&d, &put_back, &label);
    let journal = journal::taken_off(&d, &info)?;
    // Players record history in either database: every list of the drive's the backup lacks.
    let kept = crate::plays::drive_history(&put_back).map(|h| h.lists).unwrap_or_default();
    let history_lists: Vec<String> = crate::plays::drive_history(&d)
        .map(|h| h.lists)
        .unwrap_or_default()
        .into_iter()
        .filter(|l| !kept.iter().any(|k| k.same_as(l)))
        .map(|l| l.path)
        .collect();

    let mut summary = vec![format!(
        "put back {} ({}, {}) on {}",
        info.id,
        info.reason,
        local_time(info.created, "%Y-%m-%d %H:%M"),
        d.name()
    )];
    let put: u64 = files.iter().filter(|f| f.change != "removed").map(|f| f.bytes).sum();
    summary.push(format!("{} file(s) change ({})", files.len(), bytes(put)));
    if !journal.is_empty() {
        summary.push(format!("{} recorded edit(s) come off the journal", journal.len()));
    }
    let mut warnings = Vec::new();
    if !history_lists.is_empty() {
        warnings.push(format!(
            "{} history list(s) players recorded since are dropped: {}",
            history_lists.len(),
            history_lists.join(", ")
        ));
    }
    if !missing_audio.is_empty() {
        warnings.push(format!(
            "{} track(s) in the backup have no audio on the drive (e.g. {})",
            missing_audio.len(),
            missing_audio[0]
        ));
    }
    let keys = info.files.clone();
    let noop = files.is_empty() && journal.is_empty();
    let plan = UndoPlan {
        drive: d.name(),
        root: d.root().into(),
        backup: info,
        files,
        diff,
        history_lists,
        journal,
        missing_audio,
    };
    Planned::new("undo", plan, req.force)
        .summary(summary)
        .warnings(warnings)
        .destructive(true)
        .noop(noop)
        .touches(Touched::drive(&d, true, vec![Probe::Databases, Probe::Identity, Probe::Stamp(keys)]))
        .reaching_clones(&Ctx::silent())?
        .guard(None)
}

impl Operation for UndoPlan {
    /// Forced (see [`UndoRequest::force`]).
    type Request = bool;
    type Report = BackupInfo;

    fn apply(planned: &Planned<Self, bool>, _: Option<&Store>, ctx: &Ctx<'_>) -> Result<BackupInfo> {
        backup::restore_with(&mut planned.drive(0)?, Some(&planned.plan.backup.id), *planned.request(), ctx)
    }
}

/// Plan and undo in one go, for callers that don't confirm (see [`undo_plan`]).
pub fn undo_apply(req: &UndoRequest, ctx: &Ctx<'_>) -> Result<BackupInfo> {
    undo_plan(req)?.apply(None, ctx)
}

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct UndoListRequest {
    /// Only this drive [default: every mounted drive].
    #[serde(default)]
    pub drive: Option<String>,
}

/// What can be undone: every mounted drive's backups, and art embedded in files on this computer.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct UndoList {
    pub drives: Vec<DriveBackups>,
    /// `art unembed` puts these files back.
    pub embeds: Vec<EmbedBackup>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct DriveBackups {
    pub drive: String,
    pub root: PathBuf,
    /// Newest first.
    pub backups: Vec<UndoableBackup>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct UndoableBackup {
    #[serde(flatten)]
    pub backup: BackupInfo,
    /// Recorded edits putting it back takes off the journal.
    pub journal: usize,
    /// It's the one `undo` puts back.
    pub next: bool,
}

pub fn undo_list(store: Option<&Store>, req: &UndoListRequest) -> Result<UndoList> {
    let devices = match &req.drive {
        Some(spec) => vec![resolve_drive(spec)?],
        None => known_drives(false),
    };
    let mut drives = Vec::new();
    for d in devices {
        let list = backup::list(&d)?;
        if list.is_empty() && req.drive.is_none() {
            continue;
        }
        let next = backup::choose(&d, None, false).ok().map(|b| b.id);
        let mut backups = Vec::new();
        for b in list {
            let journal = journal::taken_off(&d, &b).map(|j| j.len()).unwrap_or(0);
            let next = next.as_deref() == Some(b.id.as_str());
            backups.push(UndoableBackup { backup: b, journal, next });
        }
        drives.push(DriveBackups { drive: d.name(), root: d.root().into(), backups });
    }
    let embeds = store.map(artwork::embed_backups).transpose()?.unwrap_or_default();
    Ok(UndoList { drives, embeds })
}
