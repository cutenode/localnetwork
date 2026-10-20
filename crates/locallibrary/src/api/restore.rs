//! Bringing back one playlist or folder as a stored snapshot or a drive backup has it.
//!
//! `restore` with a backup puts a whole database back. This takes a single playlist (or a folder
//! and everything in it) from an earlier state and makes it again as an ordinary playlist edit,
//! so it is planned, backed up, journaled and undone like any other. Entries are matched to the
//! drive's tracks by audio file; tracks the drive no longer has are left out and listed.

use rustc_hash::{FxHashMap, FxHashSet};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::resolve_drive;
use crate::backup::{self, BackupInfo};
use crate::convert;
use crate::device::Device;
use crate::doctor::Databases;
use crate::edit::{self, Edit, EditOptions, EditPlan, EditReport};
use crate::error::{Action, Reason};
use crate::hash::Hash;
use crate::model::{Library, Playlist};
use crate::op::{Operation, Planned, Touched};
use crate::paths;
use crate::progress::Ctx;
use crate::store::{SnapshotInfo, Store};
use crate::{Error, Result};

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct PlaylistRestoreRequest {
    pub drive: String,
    /// The playlist or folder: its path (`Folder/Name`), or its name when that's unique, as the
    /// snapshot or backup has it.
    pub playlist: String,
    /// The snapshot to take it from: an id prefix or a label [default: the newest snapshot of the
    /// drive's library that has it].
    #[serde(default)]
    pub snapshot: Option<String>,
    /// Take it from one of the drive's backups instead (see [`super::list_backups`]).
    #[serde(default)]
    pub backup: Option<String>,
}

/// Where the restored playlist comes from.
#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RestoreSource {
    Snapshot { snapshot: Box<SnapshotInfo>, library: Option<String> },
    Backup { backup: Box<BackupInfo> },
}

/// An entry the source has whose track isn't on the drive any more.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct MissingEntry {
    /// The playlist's path.
    pub playlist: String,
    /// Its position in the source playlist, from 1.
    pub position: usize,
    /// `Artist - Title`.
    pub track: String,
    /// The audio file, as the database lists it.
    pub file: String,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct PlaylistRestorePreview {
    pub source: RestoreSource,
    /// The playlist or folder's path, as the source has it (and as it will be on the drive).
    pub path: String,
    pub folder: bool,
    /// Playlists restored: one, or every playlist in the folder.
    pub playlists: usize,
    /// Entries put back.
    pub entries: usize,
    /// Entries left out because their track isn't on the drive any more.
    pub missing: Vec<MissingEntry>,
    /// Playlists in the folder on the drive that the source doesn't have: left as they are.
    pub untouched: Vec<String>,
    pub plan: EditPlan,
}

/// What restoring a playlist or folder would do.
pub fn playlist_restore_plan(
    store: Option<&Store>,
    req: &PlaylistRestoreRequest,
    ctx: &Ctx<'_>,
) -> Result<Planned<PlaylistRestorePreview>> {
    let d = resolve_drive(&req.drive)?;
    let (source, restoration) = prepare(store, req, &d)?;
    let plan = edit::plan(&d, &restoration.edits, &EditOptions::default(), ctx)?;
    let line = format!(
        "bring back {} ({} playlist(s), {} entries) from {}",
        restoration.path,
        restoration.playlists,
        restoration.entries,
        describe(&source)
    );
    let mut warnings = plan.warnings.clone();
    if !restoration.missing.is_empty() {
        warnings.push(format!("{} entries are left out: their tracks aren't on the drive", restoration.missing.len()));
    }
    let noop = plan.is_noop();
    let preview = PlaylistRestorePreview {
        source,
        path: restoration.path,
        folder: restoration.folder,
        playlists: restoration.playlists,
        entries: restoration.entries,
        missing: restoration.missing,
        untouched: restoration.untouched,
        plan,
    };
    Planned::new("playlist.restore", preview, ())
        .line(line)
        .warnings(warnings)
        .noop(noop)
        .touches(Touched::library(&d, true))
        .reaching_clones(ctx)?
        .guard(store)
}

impl Operation for PlaylistRestorePreview {
    type Request = ();
    type Report = EditReport;

    fn apply(planned: &Planned<Self>, _: Option<&Store>, ctx: &Ctx<'_>) -> Result<EditReport> {
        edit::apply(&planned.plan.plan, &mut planned.drive(0)?, ctx)
    }
}

/// Plan and restore a playlist or folder in one go, for callers that don't confirm.
pub fn playlist_restore_apply(
    store: Option<&Store>,
    req: &PlaylistRestoreRequest,
    ctx: &Ctx<'_>,
) -> Result<EditReport> {
    playlist_restore_plan(store, req, ctx)?.apply(store, ctx)
}
/// The source library holding the playlist, and the edits that bring it back.
fn prepare(store: Option<&Store>, req: &PlaylistRestoreRequest, d: &Device) -> Result<(RestoreSource, Restoration)> {
    let spec = req.playlist.trim();
    if spec.trim_matches('/').is_empty() {
        return Err(Error::invalid("give the playlist or folder to restore"));
    }
    let (source, library) = match (&req.snapshot, &req.backup) {
        (Some(_), Some(_)) => return Err(Error::invalid("restore from a snapshot or a backup, not both")),
        (None, Some(id)) => {
            let info = backup::list(d)?
                .into_iter()
                .find(|b| &b.id == id)
                .ok_or_else(|| Error::NotFound(format!("backup {id} on {}", d.name())))?;
            let library = convert::combined(&Databases::read(&Device::open(backup::dir(d, &info.id))?)?)?;
            (RestoreSource::Backup { backup: Box::new(info) }, library)
        }
        (Some(prefix), None) => {
            let store = store.ok_or_else(|| Error::invalid("restoring from a snapshot needs the store"))?;
            let prefix = prefix.strip_prefix("snapshot:").unwrap_or(prefix);
            let snap = store.load_snapshot(&store.resolve_snapshot(prefix)?)?;
            let library = convert::combined(&Databases::from_snapshot(store, &snap)?)?;
            let name = store.library_by_id(snap.library_id)?.map(|l| l.name);
            (RestoreSource::Snapshot { snapshot: Box::new(store.snapshot_info(&snap)?), library: name }, library)
        }
        (None, None) => {
            let store = store.ok_or_else(|| {
                Error::invalid("there's no store to restore from; give a backup instead")
                    .because(Reason::NoStore)
                    .action(backups(d))
            })?;
            newest_with(store, d, spec)?
        }
    };
    // What either database had: a CDJ-3000X makes playlists and adds entries in OneLibrary alone.
    let node = library.find_playlist(spec).ok_or_else(|| {
        Error::NotFound(format!("playlist {spec:?} in {} (give its path, like Folder/Name)", describe(&source)))
    })?;
    let restoration = restore_edits(&library, node, &d.library()?)?;
    Ok((source, restoration))
}

fn describe(source: &RestoreSource) -> String {
    match source {
        RestoreSource::Snapshot { snapshot, .. } => format!("snapshot {}", snapshot.short_id),
        RestoreSource::Backup { backup } => format!("backup {}", backup.id),
    }
}

/// The newest snapshot of the drive's library that has the playlist.
fn newest_with(store: &Store, d: &Device, spec: &str) -> Result<(RestoreSource, Library)> {
    let id = d.state().and_then(|s| s.library_id).ok_or_else(|| {
        Error::invalid(format!(
            "{} isn't linked to a library in the store (pull it first), so say which snapshot or backup to restore from",
            d.name()
        ))
    })?;
    let lib = store.library_by_id(id)?.ok_or_else(|| Error::NotFound(format!("library {id} in the store")))?;
    // Snapshots of one state share their database files: read each set once.
    let mut read: FxHashSet<Vec<Option<Hash>>> = FxHashSet::default();
    for info in store.history(&lib, None)? {
        let snap = store.load_snapshot(&info.id)?;
        let databases: Vec<Option<Hash>> = [paths::EXPORT_PDB, paths::LIBRARY_PLUS_DB, paths::LIBRARY_PLUS_WAL]
            .iter()
            .map(|k| snap.files.iter().find(|f| f.path == *k).and_then(|f| f.hash))
            .collect();
        if !read.insert(databases) {
            continue;
        }
        let library = convert::combined(&Databases::from_snapshot(store, &snap)?)?;
        if library.find_playlist(spec).is_some() {
            return Ok((
                RestoreSource::Snapshot { snapshot: Box::new(info), library: Some(lib.name.clone()) },
                library,
            ));
        }
    }
    Err(Error::NotFound(format!("playlist {spec:?} in any snapshot of {:?}; a backup may have it", lib.name))
        .action(backups(d)))
}

/// The edits that restore a playlist or folder, and what they leave out.
#[derive(Debug)]
struct Restoration {
    path: String,
    folder: bool,
    edits: Vec<Edit>,
    playlists: usize,
    entries: usize,
    missing: Vec<MissingEntry>,
    untouched: Vec<String>,
}

/// Edits that make `current` hold `node` (a playlist or folder of `source`) as `source` has it:
/// what's missing is created, put back where it was among the playlists still there, and each
/// playlist gets the source's entries whose track `current` has. Playlists `current` has in the
/// folder that `source` lacks are left alone.
fn restore_edits(source: &Library, node: &Playlist, current: &Library) -> Result<Restoration> {
    let on_drive: FxHashSet<String> = current.tracks.iter().map(|t| paths::file_key(&t.file_path)).collect();
    let names = source.playlist_names(node.id);
    if let Some(n) = names.iter().find(|n| n.contains('/')) {
        return Err(Error::invalid(format!("can't restore {n:?}: names with a / can't be given as a path")));
    }
    let mut r = Restoration {
        path: names.join("/"),
        folder: node.is_folder,
        edits: Vec::new(),
        playlists: 0,
        entries: 0,
        missing: Vec::new(),
        untouched: Vec::new(),
    };
    let mut layout = Layout { current, children: FxHashMap::default() };
    let mut restored = FxHashSet::default();
    let mut stack = vec![(node, names)];
    while let Some((p, names)) = stack.pop() {
        let path = names.join("/");
        let (parent, name) = names.split_at(names.len() - 1);
        let created = layout.place(parent, &name[0], || {
            // The siblings before it in the source, nearest first.
            let siblings = source.children(p.parent_id);
            let at = siblings.iter().position(|s| s.id == p.id).unwrap_or(0);
            siblings[..at].iter().rev().map(|s| s.name.clone()).collect()
        });
        if p.is_folder {
            if created.is_some() {
                r.edits.push(Edit::CreatePlaylist { path: path.clone(), folder: true });
            }
            for child in source.children(Some(p.id)).into_iter().rev() {
                stack.push((child, [names.clone(), vec![child.name.clone()]].concat()));
            }
        } else {
            let mut files = Vec::new();
            for (i, t) in source.tracks_by_id(&p.track_ids).into_iter().enumerate() {
                if on_drive.contains(&paths::file_key(&t.file_path)) {
                    files.push(t.file_path.clone());
                } else {
                    r.missing.push(MissingEntry {
                        playlist: path.clone(),
                        position: i + 1,
                        track: t.display_name(),
                        file: t.file_path.clone(),
                    });
                }
            }
            r.playlists += 1;
            r.entries += files.len();
            // `create` also makes it again when the journal is replayed after an export.
            r.edits.push(Edit::SetEntries { playlist: path.clone(), paths: files, create: true });
        }
        if let Some(Some(position)) = created {
            let into = (!parent.is_empty()).then(|| parent.join("/"));
            r.edits.push(Edit::MovePlaylist { playlist: path.clone(), into, position: Some(position) });
        }
        restored.insert(path);
    }
    if r.entries == 0 && !r.missing.is_empty() {
        return Err(Error::NotFound(format!(
            "none of the {} entries of {} are on the drive any more",
            r.missing.len(),
            r.path
        )));
    }
    // What the drive has in the folder that the source doesn't.
    if let Some(now) = find(current, &r.path.split('/').map(str::to_string).collect::<Vec<_>>())
        && now.is_folder
    {
        let mut stack = vec![now.id];
        while let Some(id) = stack.pop() {
            for c in current.children(Some(id)) {
                let path = current.playlist_path(c.id);
                if c.is_folder {
                    stack.push(c.id);
                } else if !restored.contains(&path) {
                    r.untouched.push(path);
                }
            }
        }
        r.untouched.sort();
    }
    Ok(r)
}

/// The first node at `names` in display order, as edits find it.
fn find<'l>(lib: &'l Library, names: &[String]) -> Option<&'l Playlist> {
    let mut parent = None;
    let mut found = None;
    for name in names {
        let p = lib.children(parent).into_iter().find(|p| p.name == *name)?;
        parent = Some(p.id);
        found = Some(p);
    }
    found
}

/// The drive's playlist tree by name, as restoring changes it, so restored playlists go back
/// next to the siblings they had.
struct Layout<'l> {
    current: &'l Library,
    /// Names in each folder (by path), in display order.
    children: FxHashMap<Vec<String>, Vec<String>>,
}

impl Layout<'_> {
    fn names(&mut self, parent: &[String]) -> &mut Vec<String> {
        let current = self.current;
        self.children.entry(parent.to_vec()).or_insert_with(|| {
            let id = match parent.is_empty() {
                true => None,
                false => match find(current, parent) {
                    Some(p) => Some(p.id),
                    None => return Vec::new(),
                },
            };
            current.children(id).into_iter().map(|p| p.name.clone()).collect()
        })
    }

    /// Make sure `name` is in folder `parent`. `None` when it's there already; otherwise the
    /// position (from 1) to move it to after it is created at the end, `None` inside when the
    /// end is right: just after the nearest of `before` (names that came before it, nearest
    /// first) still there, else first.
    fn place(&mut self, parent: &[String], name: &str, before: impl FnOnce() -> Vec<String>) -> Option<Option<usize>> {
        let names = self.names(parent);
        if names.iter().any(|n| n == name) {
            return None;
        }
        let at = before().iter().find_map(|b| names.iter().position(|n| n == b)).map_or(0, |i| i + 1);
        names.insert(at, name.to_string());
        Some((at + 1 < names.len()).then_some(at + 1))
    }
}

/// Listing the drive's backups, as an action.
fn backups(d: &Device) -> Action {
    let drive = d.root().display().to_string();
    Action::call("drive.backups", serde_json::json!({ "drive": drive }), format!("Show {}'s backups", d.name()))
}

#[cfg(test)]
mod tests;
