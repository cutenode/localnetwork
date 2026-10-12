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
    let on_drive: FxHashSet<String> = current.tracks.iter().map(|t| paths::key(&t.file_path).to_lowercase()).collect();
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
                if on_drive.contains(&paths::key(&t.file_path).to_lowercase()) {
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
mod tests {
    use super::*;
    use crate::model::Track;

    fn track(id: u32) -> Track {
        loclib_model::test_util::track(id, "", &format!("T{id}"), &format!("/Contents/t{id}.mp3"))
    }

    fn node(id: u32, parent: Option<u32>, name: &str, folder: bool, order: u32, tracks: &[u32]) -> Playlist {
        Playlist {
            id,
            parent_id: parent,
            name: name.into(),
            is_folder: folder,
            sort_order: order,
            track_ids: tracks.to_vec(),
        }
    }

    #[test]
    fn a_deleted_playlist_goes_back_between_its_old_neighbours() {
        let source = Library {
            tracks: (1..=4).map(track).collect(),
            playlists: vec![
                node(1, None, "Sets", true, 0, &[]),
                node(2, Some(1), "Warmup", false, 0, &[1]),
                node(3, Some(1), "Friday", false, 1, &[2, 3, 2, 4]),
                node(4, Some(1), "Closing", false, 2, &[4]),
            ],
            ..Default::default()
        };
        // Friday is gone, and so is track 3.
        let current = Library {
            tracks: vec![track(1), track(2), track(4)],
            playlists: vec![
                node(10, None, "Sets", true, 0, &[]),
                node(11, Some(10), "Warmup", false, 0, &[1]),
                node(12, Some(10), "Closing", false, 1, &[4]),
            ],
            ..Default::default()
        };
        let r = restore_edits(&source, source.find_playlist("Sets/Friday").unwrap(), &current).unwrap();
        assert_eq!(r.path, "Sets/Friday");
        assert_eq!(r.entries, 3);
        assert_eq!(r.missing.len(), 1);
        assert_eq!((r.missing[0].position, r.missing[0].file.as_str()), (2, "/Contents/t3.mp3"));
        assert_eq!(
            r.edits,
            [
                Edit::SetEntries {
                    playlist: "Sets/Friday".into(),
                    paths: vec!["/Contents/t2.mp3".into(), "/Contents/t2.mp3".into(), "/Contents/t4.mp3".into()],
                    create: true,
                },
                Edit::MovePlaylist { playlist: "Sets/Friday".into(), into: Some("Sets".into()), position: Some(2) },
            ]
        );

        // A whole folder: the playlists still there are refilled, the drive's own are left.
        let mut current = current;
        current.playlists.push(node(13, Some(10), "New", false, 2, &[1]));
        let r = restore_edits(&source, source.find_playlist("Sets").unwrap(), &current).unwrap();
        assert!(r.folder);
        assert_eq!(r.playlists, 3);
        assert_eq!(r.untouched, ["Sets/New"]);
        let filled: Vec<&str> = r
            .edits
            .iter()
            .filter_map(|e| match e {
                Edit::SetEntries { playlist, .. } => Some(playlist.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(filled, ["Sets/Warmup", "Sets/Friday", "Sets/Closing"]);
    }

    #[test]
    fn a_deleted_folder_is_made_again_in_order() {
        let source = Library {
            tracks: (1..=2).map(track).collect(),
            playlists: vec![
                node(1, None, "Old", true, 0, &[]),
                node(2, Some(1), "A", false, 0, &[1]),
                node(3, Some(1), "Inner", true, 1, &[]),
                node(4, Some(3), "B", false, 0, &[2]),
                node(5, None, "Last", false, 1, &[]),
            ],
            ..Default::default()
        };
        let current = Library {
            tracks: source.tracks.clone(),
            playlists: vec![node(9, None, "Last", false, 0, &[])],
            ..Default::default()
        };
        let r = restore_edits(&source, source.find_playlist("Old").unwrap(), &current).unwrap();
        let ops: Vec<String> = r
            .edits
            .iter()
            .map(|e| match e {
                Edit::CreatePlaylist { path, .. } => format!("create {path}"),
                Edit::SetEntries { playlist, paths, .. } => format!("fill {playlist} {}", paths.len()),
                Edit::MovePlaylist { playlist, position, .. } => format!("move {playlist} {position:?}"),
                e => format!("{e:?}"),
            })
            .collect();
        // Created at the end of the top level, then moved before Last; what's inside is made in
        // order, so it needs no moves.
        assert_eq!(ops, ["create Old", "move Old Some(1)", "fill Old/A 1", "create Old/Inner", "fill Old/Inner/B 1"]);
    }

    #[test]
    fn nothing_left_to_restore_is_refused() {
        let source = Library {
            tracks: vec![track(1)],
            playlists: vec![node(1, None, "Gone", false, 0, &[1])],
            ..Default::default()
        };
        let err = restore_edits(&source, &source.playlists[0], &Library::default()).unwrap_err();
        assert!(matches!(err, Error::NotFound(_)), "{err}");
        // An empty playlist is fine.
        let empty = Library { playlists: vec![node(1, None, "Empty", false, 0, &[])], ..Default::default() };
        assert_eq!(restore_edits(&empty, &empty.playlists[0], &Library::default()).unwrap().edits.len(), 1);
    }

    /// The drive's playlist tree by path, as a model of what the edits restoring do to it:
    /// children in display order, and each playlist's entries.
    #[derive(Debug, Default)]
    struct Tree {
        children: FxHashMap<Vec<String>, Vec<String>>,
        entries: FxHashMap<Vec<String>, Vec<String>>,
        folders: FxHashSet<Vec<String>>,
    }

    impl Tree {
        fn of(lib: &Library) -> Tree {
            let mut t = Tree::default();
            let mut stack = vec![(None, Vec::new())];
            while let Some((id, path)) = stack.pop() {
                let kids = lib.children(id);
                t.children.insert(path.clone(), kids.iter().map(|p| p.name.clone()).collect());
                for k in kids {
                    let at = [path.clone(), vec![k.name.clone()]].concat();
                    if k.is_folder {
                        t.folders.insert(at.clone());
                        stack.push((Some(k.id), at));
                    } else {
                        let files = lib.tracks_by_id(&k.track_ids).iter().map(|t| t.file_path.clone()).collect();
                        t.entries.insert(at, files);
                    }
                }
            }
            t
        }

        fn has(&self, path: &[String]) -> bool {
            self.children.get(&path[..path.len() - 1]).is_some_and(|c| c.iter().any(|n| *n == path[path.len() - 1]))
        }

        /// Made at the end of its folder, with missing folders above it, as `CreatePlaylist` and
        /// `SetEntries { create }` make them.
        fn create(&mut self, path: &[String], folder: bool) {
            for i in 1..=path.len() {
                if !self.has(&path[..i]) {
                    self.children.entry(path[..i - 1].to_vec()).or_default().push(path[i - 1].clone());
                    if i < path.len() || folder {
                        self.folders.insert(path[..i].to_vec());
                        self.children.entry(path[..i].to_vec()).or_default();
                    }
                }
            }
        }

        fn apply(&mut self, e: &Edit) {
            let split = |p: &str| p.split('/').map(str::to_string).collect::<Vec<_>>();
            match e {
                Edit::CreatePlaylist { path, folder } => self.create(&split(path), *folder),
                Edit::SetEntries { playlist, paths, create } => {
                    assert!(create, "restoring makes what's missing");
                    self.create(&split(playlist), false);
                    self.entries.insert(split(playlist), paths.clone());
                }
                Edit::MovePlaylist { playlist, into, position } => {
                    let path = split(playlist);
                    let parent = into.as_deref().map(split).unwrap_or_default();
                    assert_eq!(parent, path[..path.len() - 1], "restored nodes move only among their siblings");
                    let kids = self.children.get_mut(&parent).unwrap();
                    let at = kids.iter().position(|n| *n == path[path.len() - 1]).unwrap();
                    let name = kids.remove(at);
                    kids.insert(position.unwrap() - 1, name);
                }
                e => panic!("restoring made {e:?}"),
            }
        }
    }

    const NAMES: &[&str] = &["Sets", "Warmup", "Friday", "Closing", "Old", "Inner", "Peak"];

    /// A source library of up to three levels: unique names among siblings (as `Folder/Name`
    /// paths can name each), up to four entries a playlist from six tracks, repeats and all.
    fn draw_source(tc: &hegel::TestCase) -> Library {
        use hegel::generators as gs;
        let mut lib = Library { tracks: (1..=6).map(track).collect(), ..Default::default() };
        let mut next = 1;
        let mut folders = vec![(None, 0)];
        while let Some((parent, depth)) = folders.pop() {
            let mut names: Vec<&str> = tc.draw(gs::vecs(gs::sampled_from(NAMES)).max_size(3).unique(true));
            if parent.is_none() && names.is_empty() {
                names.push("Sets");
            }
            for (order, name) in names.into_iter().enumerate() {
                let folder = depth < 2 && tc.draw(gs::weighted_booleans(0.4));
                let tracks: Vec<u32> = match folder {
                    true => Vec::new(),
                    false => tc.draw(gs::vecs(gs::integers::<u32>().min_value(1).max_value(6)).max_size(4)),
                };
                lib.playlists.push(node(next, parent, name, folder, order as u32, &tracks));
                if folder {
                    folders.push((Some(next), depth + 1));
                }
                next += 1;
            }
        }
        lib
    }

    /// The drive now: the source with playlists, folders and tracks gone, and playlists of its
    /// own (never named like the source's) among them; what's still there is in the source's order.
    fn draw_current(tc: &hegel::TestCase, source: &Library) -> Library {
        use hegel::generators as gs;
        let tracks: Vec<Track> =
            source.tracks.iter().filter(|_| tc.draw(gs::weighted_booleans(0.7))).cloned().collect();
        let mut kept: FxHashSet<u32> = FxHashSet::default();
        let mut playlists = Vec::new();
        // Parents come before children in `source.playlists`, as drawn.
        for p in &source.playlists {
            let parent_kept = p.parent_id.is_none_or(|id| kept.contains(&id));
            if parent_kept && tc.draw(gs::weighted_booleans(0.6)) {
                kept.insert(p.id);
                let mut p = p.clone();
                p.sort_order *= 2;
                playlists.push(p);
            }
        }
        let folders: Vec<Option<u32>> =
            std::iter::once(None).chain(playlists.iter().filter(|p| p.is_folder).map(|p| Some(p.id))).collect();
        for (i, name) in ["Mine", "Ours"].iter().enumerate() {
            if tc.draw(gs::booleans()) {
                let parent = folders[tc.draw(gs::integers::<usize>().max_value(folders.len() - 1))];
                let order = tc.draw(gs::integers::<u32>().max_value(7));
                playlists.push(node(100 + i as u32, parent, name, false, order, &[1]));
            }
        }
        Library { tracks, playlists, ..Default::default() }
    }

    fn subtree<'l>(lib: &'l Library, top: &'l Playlist) -> Vec<&'l Playlist> {
        let mut out = vec![top];
        let mut i = 0;
        while i < out.len() {
            out.extend(lib.children(Some(out[i].id)));
            i += 1;
        }
        out
    }

    /// Restoring a playlist or folder makes the drive hold every playlist of it with the
    /// source's entries the drive still has the tracks of, in order, and the rest listed as
    /// missing; what it creates goes just after the nearest of its old siblings still there
    /// (first when none is), so the source's siblings keep their order; what the drive has of its
    /// own is left where it was. With nothing left to put back, it's refused.
    #[hegel::test(test_cases = 300)]
    fn restoring_a_node_brings_back_the_source_s_entries_in_place(tc: hegel::TestCase) {
        use hegel::generators as gs;
        let source = draw_source(&tc);
        let current = draw_current(&tc, &source);
        let top = &source.playlists[tc.draw(gs::integers::<usize>().max_value(source.playlists.len() - 1))];
        let on_drive: FxHashSet<&str> = current.tracks.iter().map(|t| t.file_path.as_str()).collect();
        let nodes = subtree(&source, top);
        let (mut kept, mut missing) = (0, Vec::new());
        for p in nodes.iter().filter(|p| !p.is_folder) {
            for (i, t) in source.tracks_by_id(&p.track_ids).into_iter().enumerate() {
                match on_drive.contains(t.file_path.as_str()) {
                    true => kept += 1,
                    false => missing.push((source.playlist_path(p.id), i + 1, t.file_path.clone())),
                }
            }
        }

        let r = restore_edits(&source, top, &current);
        if kept == 0 && !missing.is_empty() {
            assert!(matches!(r, Err(Error::NotFound(_))), "{r:?}");
            return;
        }
        let r = r.unwrap();
        let path = |p: &Playlist| source.playlist_names(p.id);
        assert_eq!(r.path, path(top).join("/"));
        assert_eq!(r.folder, top.is_folder);
        assert_eq!(r.playlists, nodes.iter().filter(|p| !p.is_folder).count());
        assert_eq!(r.entries, kept);
        // Listed playlist by playlist, as the edits go (the model walks the tree level by level).
        let mut said: Vec<(String, usize, String)> =
            r.missing.iter().map(|m| (m.playlist.clone(), m.position, m.file.clone())).collect();
        said.sort();
        missing.sort();
        assert_eq!(said, missing);

        let before = Tree::of(&current);
        let mut after = Tree::of(&current);
        for e in &r.edits {
            after.apply(e);
        }
        for p in &nodes {
            let at = path(p);
            assert!(after.has(&at), "{at:?} isn't there after restoring");
            assert_eq!(after.folders.contains(&at), p.is_folder, "{at:?}");
            if !p.is_folder {
                let want: Vec<String> = source
                    .tracks_by_id(&p.track_ids)
                    .iter()
                    .filter(|t| on_drive.contains(t.file_path.as_str()))
                    .map(|t| t.file_path.clone())
                    .collect();
                assert_eq!(after.entries[&at], want, "{at:?}");
            }
        }
        // Each folder restored into: the source's children there keep the source's order, each
        // created one just after the nearest one before it that's there, and the drive's own
        // nodes keep theirs.
        let parent = path(top)[..path(top).len() - 1].to_vec();
        let into: Vec<(Vec<String>, Option<u32>)> = std::iter::once((parent, top.parent_id))
            .chain(nodes.iter().filter(|p| p.is_folder).map(|p| (path(p), Some(p.id))))
            .collect();
        for (folder, id) in into {
            let Some(now) = after.children.get(&folder) else { continue };
            let theirs: Vec<String> = source.children(id).iter().map(|p| p.name.clone()).collect();
            let order: Vec<&String> = now.iter().filter(|n| theirs.contains(n)).collect();
            let want: Vec<&String> = theirs.iter().filter(|n| now.contains(n)).collect();
            assert_eq!(order, want, "in {folder:?}");
            let was = before.children.get(&folder).cloned().unwrap_or_default();
            for (i, name) in now.iter().enumerate().filter(|(_, n)| !was.contains(n)) {
                let k = theirs.iter().position(|n| n == name).unwrap();
                let nearest = theirs[..k].iter().rev().find(|n| now.contains(n));
                assert_eq!(i.checked_sub(1).map(|j| &now[j]), nearest, "{name:?} in {folder:?}: {now:?}");
            }
            let ours: Vec<&String> = now.iter().filter(|n| was.contains(n)).collect();
            assert_eq!(ours, was.iter().collect::<Vec<_>>(), "in {folder:?}");
        }
        // The drive's own playlists in a restored folder are listed, and kept as they were.
        let mut untouched: Vec<String> = Vec::new();
        for (at, files) in &before.entries {
            if at.len() > r.path.split('/').count() && at.join("/").starts_with(&format!("{}/", r.path)) {
                if !nodes.iter().any(|p| path(p) == *at) {
                    untouched.push(at.join("/"));
                }
                if r.folder && !nodes.iter().any(|p| path(p) == *at) {
                    assert_eq!(&after.entries[at], files);
                }
            }
        }
        untouched.sort();
        match r.folder && before.folders.contains(&path(top)) {
            true => assert_eq!(r.untouched, untouched),
            false => assert!(r.untouched.is_empty(), "{:?}", r.untouched),
        }
    }

    /// A name with a `/` in it (rekordbox allows one) can't be given as a path, so restoring it
    /// is refused, whatever it holds.
    #[hegel::test(test_cases = 50)]
    fn a_name_with_a_slash_is_refused(tc: hegel::TestCase) {
        use hegel::generators as gs;
        let mut source = draw_source(&tc);
        let i = tc.draw(gs::integers::<usize>().max_value(source.playlists.len() - 1));
        let name = format!("{}/{}", source.playlists[i].name, tc.draw(gs::sampled_from(NAMES)));
        source.playlists[i].name = name;
        // It, or anything inside it.
        let inside: Vec<u32> = subtree(&source, &source.playlists[i]).iter().map(|p| p.id).collect();
        let pick = inside[tc.draw(gs::integers::<usize>().max_value(inside.len() - 1))];
        let top = source.playlists.iter().find(|p| p.id == pick).unwrap();
        let current = draw_current(&tc, &source);
        let err = restore_edits(&source, top, &current).unwrap_err();
        assert!(matches!(&err, Error::Invalid(m) if m.contains("can't be given as a path")), "{err}");
    }
}
