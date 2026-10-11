//! Drives coming back from a set: telling changes the players made apart from new exports.
//!
//! Players write to the stick: a new history list every time it's connected, memory cues (into
//! the analysis files) and My Settings, and on the CDJ-3000X playlist edits and TAG LIST
//! playlists; OneLibrary players write only to OneLibrary. Each of these but My Settings changes
//! the drive's fingerprint: My Settings belong to the settings profile, not the library (see
//! [`crate::paths::FileClass::Settings`]), and what players keep for themselves, logs and the
//! track filter, is left out too (see [`crate::paths::FileClass::DriveState`]). If the primary
//! got a new rekordbox export in the meantime, the two drives have both changed since they were
//! last in sync, and a plain sync has to refuse.
//!
//! [`resolve`] finds the last state both drives held (it must be a stored snapshot), and sorts
//! each drive's changes since then. When one drive's changes are only things players do (new
//! history lists, rewritten analysis files of existing tracks, TAG LIST playlists) and the
//! other's aren't, the sync can go ahead: from the exported drive onto the
//! one back from the set, after saving that drive's state as a snapshot so its histories and
//! cues are kept. Anything this can't classify still counts as a real change, so the rule never
//! overwrites an export.

use rustc_hash::{FxHashMap, FxHashSet};
use schemars::JsonSchema;
use serde::Serialize;

use crate::Result;
use crate::device::Device;
use crate::doctor::Databases;
use crate::fsutil;
use crate::model::{Library, LibraryDiff};
use crate::paths::FileClass;
use crate::progress::Ctx;
use crate::store::{Snapshot, SnapshotInfo, Store};

/// The library databases (their changes are judged through the library diff).
const LIBRARY_DATABASES: [&str; 4] = [
    "pioneer/rekordbox/export.pdb",
    "pioneer/rekordbox/exportext.pdb",
    "pioneer/rekordbox/exportlibrary.db",
    "pioneer/rekordbox/exportlibrary.db-wal",
];

/// What changed on one drive since the last state both drives held.
#[derive(Debug, Clone, Default, Serialize, JsonSchema)]
pub struct DriveChanges {
    pub drive: String,
    /// History lists added (players add one per session).
    pub history_lists: Vec<String>,
    /// Analysis files rewritten for tracks that were already there (memory cues saved on a
    /// player).
    pub cue_files: usize,
    /// Playlists a CDJ-3000X makes from its Tag List.
    pub tag_lists: Vec<String>,
    /// Changes players don't make: new or removed tracks, edited playlists, new audio...
    pub other: Vec<String>,
}

impl DriveChanges {
    pub fn is_empty(&self) -> bool {
        self.history_lists.is_empty() && self.cue_files == 0 && self.tag_lists.is_empty() && self.other.is_empty()
    }

    /// A change players don't make (the first 20 are kept).
    fn note(&mut self, what: String) {
        if self.other.len() < 20 && !self.other.contains(&what) {
            self.other.push(what);
        }
    }

    /// Add the changes sorted from the drive's other database (one export makes the same changes
    /// to both, counted once).
    fn add(&mut self, other: DriveChanges) {
        for name in other.history_lists {
            if !self.history_lists.contains(&name) {
                self.history_lists.push(name);
            }
        }
        for name in other.tag_lists {
            if !self.tag_lists.contains(&name) {
                self.tag_lists.push(name);
            }
        }
        for what in other.other {
            self.note(what);
        }
    }

    /// Only changes players make.
    pub fn player_only(&self) -> bool {
        self.other.is_empty() && !self.is_empty()
    }

    /// One line describing what the players changed.
    pub fn describe(&self) -> String {
        let mut parts = Vec::new();
        if !self.history_lists.is_empty() {
            parts.push(format!("{} history list(s)", self.history_lists.len()));
        }
        if self.cue_files > 0 {
            parts.push(format!("cues in {} analysis file(s)", self.cue_files));
        }
        if !self.tag_lists.is_empty() {
            parts.push(format!("{} TAG LIST playlist(s)", self.tag_lists.len()));
        }
        if parts.is_empty() { "nothing".into() } else { parts.join(", ") }
    }
}

fn is_tag_list(path: &str) -> bool {
    path.rsplit('/').next().is_some_and(|n| n.to_ascii_uppercase().starts_with("TAG LIST"))
}

/// Sort a drive's changes since `ancestor`.
pub fn changes_since(store: &Store, device: &Device, ancestor: &Snapshot, ctx: &Ctx<'_>) -> Result<DriveChanges> {
    let (then, now) = (Databases::from_snapshot(store, ancestor)?, Databases::read(device)?);
    let mut out = DriveChanges { drive: device.name(), ..database_changes(&then, &now) };

    // The files.
    let scanned = device.scan_fingerprinted(Some(now.primary()), ctx)?;
    let old_files: FxHashMap<String, &crate::tree::FileEntry> =
        ancestor.files.iter().map(|f| (f.path.to_lowercase(), f)).collect();
    let mut seen = FxHashSet::default();
    for e in scanned.tree.library_entries() {
        let key = e.path.to_lowercase();
        seen.insert(key.clone());
        let old = old_files.get(&key);
        let changed = match old {
            None => true,
            Some(o) => match (o.hash, e.hash) {
                (Some(a), Some(b)) => a != b,
                _ => o.size != e.size || !fsutil::mtime_eq(o.mtime, e.mtime),
            },
        };
        if !changed {
            continue;
        }
        if LIBRARY_DATABASES.contains(&key.as_str()) {
            continue; // judged by the library diff (see `library_changes`)
        }
        match (e.class, old.is_some()) {
            (FileClass::Analysis, true) => out.cue_files += 1,
            (_, true) => out.note(format!("file changed: {}", e.path)),
            (_, false) => out.note(format!("file added: {}", e.path)),
        }
    }
    for (key, f) in &old_files {
        if f.is_library() && !seen.contains(key) && !LIBRARY_DATABASES.contains(&key.as_str()) {
            out.note(format!("file removed: {}", f.path));
        }
    }
    Ok(out)
}

/// [`library_changes`] from `then`, the databases of the last state both drives held, to `now`, a
/// drive's. The library players see is export.pdb's on a drive with both, and OneLibrary players
/// write only to OneLibrary, so its changes are sorted too; one that can't be read counts as a
/// change no player makes.
pub(crate) fn database_changes(then: &Databases, now: &Databases) -> DriveChanges {
    let mut out = library_changes(then.primary(), now.primary());
    match (then.beside(), now.beside()) {
        (Some(then), Some(now)) => out.add(library_changes(then, now)),
        _ if then.plus_error.is_some() || now.plus_error.is_some() => out.note("exportLibrary.db can't be read".into()),
        (None, Some(_)) => out.note("exportLibrary.db added".into()),
        (Some(_), None) => out.note("exportLibrary.db removed".into()),
        (None, None) => {}
    }
    out
}

/// Sort the library changes from `then` to `now` (a drive's files aside: see [`changes_since`]).
pub fn library_changes(then: &Library, now: &Library) -> DriveChanges {
    let mut out = DriveChanges::default();
    // Players add history lists and TAG LIST playlists, nothing else.
    let diff = LibraryDiff::between(then, now);
    for t in &diff.added_tracks {
        out.note(format!("track added: {}", t.name));
    }
    for t in &diff.removed_tracks {
        out.note(format!("track removed: {}", t.name));
    }
    for c in &diff.changed_tracks {
        out.note(format!(
            "track changed: {} ({})",
            c.track.name,
            c.fields.iter().map(|f| f.field.as_str()).collect::<Vec<_>>().join(", ")
        ));
    }
    for p in &diff.removed_playlists {
        out.note(format!("playlist removed: {p}"));
    }
    for p in &diff.changed_playlists {
        out.note(format!("playlist changed: {p}"));
    }
    for p in &diff.added_playlists {
        if is_tag_list(p) {
            out.tag_lists.push(p.clone());
        } else {
            out.note(format!("playlist added: {p}"));
        }
    }
    let old_history: FxHashSet<(&str, usize)> =
        then.history.iter().map(|h| (h.name.as_str(), h.track_ids.len())).collect();
    let old_names: FxHashSet<&str> = then.history.iter().map(|h| h.name.as_str()).collect();
    let new_names: FxHashSet<&str> = now.history.iter().map(|h| h.name.as_str()).collect();
    for h in &now.history {
        if !old_names.contains(h.name.as_str()) {
            out.history_lists.push(h.name.clone());
        } else if !old_history.contains(&(h.name.as_str(), h.track_ids.len())) {
            // A player adds to the list of the session it's in.
            out.history_lists.push(h.name.clone());
        }
    }
    for h in &then.history {
        if !new_names.contains(h.name.as_str()) {
            // rekordbox deletes history lists when it imports them.
            out.note(format!("history list removed: {}", h.name));
        }
    }
    out
}

/// The last library state both drives held, as a stored snapshot.
pub fn common_snapshot(store: &Store, a: &Device, b: &Device) -> Result<Option<Snapshot>> {
    let (Some(sa), Some(sb)) = (a.state(), b.state()) else { return Ok(None) };
    let b_fps: FxHashSet<_> = sb.lineage.iter().map(|e| e.fingerprint).collect();
    let Some(common) = sa.lineage.iter().rev().find(|e| b_fps.contains(&e.fingerprint)) else { return Ok(None) };
    // A snapshot id recorded with that state, on either drive.
    let recorded =
        sa.lineage.iter().chain(&sb.lineage).filter(|e| e.fingerprint == common.fingerprint).find_map(|e| e.snapshot);
    if let Some(id) = recorded
        && let Ok(s) = store.load_snapshot(&id)
    {
        return Ok(Some(s));
    }
    // Otherwise look it up in the drives' library.
    for lib_id in [sa.library_id, sb.library_id].into_iter().flatten() {
        if let Some(lib) = store.library_by_id(lib_id)?
            && let Some(info) = store.find_by_fingerprint(&lib, &common.fingerprint)?
        {
            return Ok(Some(store.load_snapshot(&info.id)?));
        }
    }
    Ok(None)
}

/// Which way a sync between two diverged drives can go.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct AfterSet {
    /// The last state both held.
    pub common: SnapshotInfo,
    /// The drive whose changes are all player-made: it is overwritten, after being saved.
    pub back_from_set: DriveChanges,
    /// The other drive (a new export, an edit...).
    pub newer: DriveChanges,
    /// Whether `left` (as given) is the drive back from the set.
    pub left_is_back_from_set: bool,
}

/// When exactly one of two diverged drives changed only the way players do, say so.
pub fn resolve(store: &Store, left: &Device, right: &Device, ctx: &Ctx<'_>) -> Result<Option<AfterSet>> {
    let Some(common) = common_snapshot(store, left, right)? else { return Ok(None) };
    let l = changes_since(store, left, &common, ctx)?;
    let r = changes_since(store, right, &common, ctx)?;
    let (left_is_back_from_set, back, newer) = match (l.player_only(), r.player_only()) {
        (true, false) if !r.is_empty() => (true, l, r),
        (false, true) if !l.is_empty() => (false, r, l),
        _ => return Ok(None),
    };
    Ok(Some(AfterSet { common: common.info(), back_from_set: back, newer, left_is_back_from_set }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tag_lists_are_recognised_by_name() {
        assert!(is_tag_list("TAG LIST 001"));
        assert!(is_tag_list("Folder/Tag List 2"));
        assert!(!is_tag_list("Friday"));
    }

    #[test]
    fn player_only_needs_changes_and_nothing_else() {
        let mut c = DriveChanges::default();
        assert!(!c.player_only());
        c.history_lists.push("HISTORY 001".into());
        assert!(c.player_only());
        assert_eq!(c.describe(), "1 history list(s)");
        c.other.push("track added: x".into());
        assert!(!c.player_only());
    }
}
