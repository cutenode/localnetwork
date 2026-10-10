//! What was played: the history lists players record on a drive, every history list the store
//! has ever archived, and play statistics.
//!
//! Players record one history list per session (`HISTORY 001`, `HISTORY 002`...) in the database
//! they read: `export.pdb`, or OneLibrary on the CDJ-3000X, OPUS-QUAD, XDJ-AZ and the like. rekordbox
//! imports them on its next sync and deletes them from the drive, but each `locallibrary pull`
//! stores the drive's databases, so every list a pulled drive ever held can be read back from the
//! store's snapshots ([`archive`]).
//!
//! A list recorded on several drives or kept across several snapshots is the same list when its
//! name and its sequence of tracks (by file path) are the same.

use chrono::{DateTime, Utc};
use rustc_hash::{FxHashMap, FxHashSet};
use schemars::JsonSchema;
use serde::Serialize;
use uuid::Uuid;

use crate::Result;
use crate::device::Device;
use crate::doctor::Databases;
use crate::hash::Hash;
use crate::model::{Library, Playlist, Track, TrackRef};
use crate::paths;
use crate::progress::{Ctx, Event, Phase};
use crate::store::{Snapshot, SnapshotInfo, SnapshotSource, Store};

/// One history list: the tracks a player played during one session, in order.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct HistoryList {
    pub name: String,
    /// Folders and name (OneLibrary can keep history lists in folders).
    pub path: String,
    /// The database it was read from: `device_library` (`export.pdb`) or `onelibrary` (the first
    /// one, when both hold it).
    pub database: &'static str,
    pub tracks: Vec<Track>,
    /// Entries whose track isn't in the library any more.
    pub missing: usize,
    /// Where those were: the positions in `tracks` they came before (the tracks either side of
    /// one weren't played one after the other).
    #[serde(skip)]
    pub missing_at: Vec<usize>,
}

impl HistoryList {
    /// Identity across drives and snapshots: name and tracks by file.
    pub(crate) fn identity(&self) -> (String, Vec<String>) {
        (self.name.clone(), self.tracks.iter().map(|t| file_key(&t.file_path)).collect())
    }

    /// Whether `other` is the same list (recorded on another drive, or kept in another snapshot).
    pub fn same_as(&self, other: &HistoryList) -> bool {
        self.identity() == other.identity()
    }

    /// A hash of what makes two lists the same ([`HistoryList::same_as`]): equal for the same
    /// list wherever it was read, and saying nothing about it.
    pub fn fingerprint(&self) -> String {
        let (name, files) = self.identity();
        let mut h = blake3::Hasher::new();
        h.update(name.as_bytes());
        for f in files {
            h.update(b"\n");
            h.update(f.as_bytes());
        }
        h.finalize().to_hex()[..32].to_string()
    }
}

/// A track's file as sessions and sets key it, to match tracks across libraries by: its path in
/// the library, lower case, without the leading slash.
pub fn file_key(file_path: &str) -> String {
    paths::key(file_path).to_lowercase()
}

/// History lists of a library in display order, with their folder paths. The library is a
/// drive's or a snapshot's, its tree untangled as it was read (see [`crate::model::untangle`]).
fn history_tree(lib: &Library) -> Vec<(&Playlist, String)> {
    fn walk<'a>(lib: &'a Library, parent: Option<u32>, prefix: &str, out: &mut Vec<(&'a Playlist, String)>) {
        let mut kids: Vec<&Playlist> = lib.history.iter().filter(|h| h.parent_id == parent).collect();
        kids.sort_by_key(|h| (h.sort_order, h.id));
        for h in kids {
            let path = if prefix.is_empty() { h.name.clone() } else { format!("{prefix}/{}", h.name) };
            if h.is_folder {
                walk(lib, Some(h.id), &path, out);
            } else {
                out.push((h, path));
            }
        }
    }
    let mut out = Vec::new();
    walk(lib, None, "", &mut out);
    // Lists under a folder that doesn't exist still count.
    let reached: FxHashSet<u32> = out.iter().map(|(h, _)| h.id).collect();
    out.extend(lib.history.iter().filter(|h| !h.is_folder && !reached.contains(&h.id)).map(|h| (h, h.name.clone())));
    out
}

/// The history lists of both databases, each once (rekordbox writes the same lists to both, but
/// players reading only one of them add theirs there).
pub(crate) fn histories(dbs: &Databases) -> Vec<HistoryList> {
    let mut out = Vec::new();
    let mut seen = FxHashSet::default();
    for (database, lib) in dbs.all() {
        let by_id: FxHashMap<u32, &Track> = lib.tracks.iter().map(|t| (t.id, t)).collect();
        for (h, path) in history_tree(lib) {
            let (mut tracks, mut missing_at) = (Vec::new(), Vec::new());
            for id in &h.track_ids {
                match by_id.get(id) {
                    Some(t) => tracks.push((*t).clone()),
                    None if missing_at.last() != Some(&tracks.len()) => missing_at.push(tracks.len()),
                    None => {}
                }
            }
            let missing = h.track_ids.len() - tracks.len();
            let list = HistoryList { name: h.name.clone(), path, database, missing, missing_at, tracks };
            if seen.insert(list.identity()) {
                out.push(list);
            }
        }
    }
    out
}

/// A history list by path: the first that matches exactly, else ignoring ASCII case when every
/// match shares one path. Failing that, by name ignoring ASCII case, or as a session by its id
/// (a prefix of 4 or more characters of its [`HistoryList::fingerprint`]), when only one list
/// has it.
pub fn find_list<'a>(lists: impl IntoIterator<Item = &'a HistoryList>, query: &str) -> Option<&'a HistoryList> {
    let lists: Vec<&HistoryList> = lists.into_iter().collect();
    let q = query.trim_matches('/');
    let only = |pred: &dyn Fn(&HistoryList) -> bool| match lists.iter().filter(|l| pred(l)).collect::<Vec<_>>()[..] {
        [l] => Some(*l),
        _ => None,
    };
    let id = q.to_ascii_lowercase();
    crate::model::pick(lists.iter().map(|l| (&l.path, *l)), q)
        .or_else(|| only(&|l| l.name.eq_ignore_ascii_case(q)))
        .or_else(|| only(&|l| id.len() >= 4 && l.fingerprint().starts_with(&id)))
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct DriveHistory {
    pub drive: String,
    pub lists: Vec<HistoryList>,
    pub warnings: Vec<String>,
}

/// The history lists on a drive, from both databases.
pub fn drive_history(device: &Device) -> Result<DriveHistory> {
    let dbs = Databases::read(device)?;
    let warnings = dbs.plus_error.iter().map(|e| format!("{e}; only export.pdb's history was read")).collect();
    Ok(DriveHistory { drive: device.name(), lists: histories(&dbs), warnings })
}

/// Where an archived list was first seen.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Sighting {
    pub snapshot: Hash,
    pub short_id: String,
    pub created: DateTime<Utc>,
    pub library: String,
    /// The drive the snapshot was pulled from.
    pub drive: Option<String>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct ArchivedHistory {
    #[serde(flatten)]
    pub list: HistoryList,
    /// The earliest snapshot holding it: it was recorded before then.
    pub first_seen: Sighting,
    /// It was recorded after this: when the snapshot before `first_seen` of the same drive was
    /// taken (which didn't hold it). `None` when that drive wasn't pulled before.
    pub recorded_after: Option<DateTime<Utc>>,
    /// How many snapshots hold it.
    pub snapshots: usize,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct HistoryArchive {
    /// Oldest first.
    pub lists: Vec<ArchivedHistory>,
    /// Snapshots read.
    pub snapshots: usize,
    /// Snapshots that couldn't be read.
    pub warnings: Vec<String>,
    /// When each drive's newest snapshot that was read was taken.
    #[serde(skip)]
    pulled: FxHashMap<DriveKey, DateTime<Utc>>,
}

impl HistoryArchive {
    /// When `device` was last pulled, of the snapshots read: a list on it that none of them
    /// holds was recorded after then.
    pub fn last_pulled(&self, device: &Device) -> Option<DateTime<Utc>> {
        let key = device.id().map_or_else(|| DriveKey::Name(device.name()), DriveKey::Id);
        self.pulled.get(&key).copied()
    }
}

/// The drive a snapshot was pulled from: its identity, or the name of a drive without one.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum DriveKey {
    Id(Uuid),
    Name(String),
}

impl DriveKey {
    fn of(source: &SnapshotSource) -> DriveKey {
        source.drive_id.map_or_else(|| DriveKey::Name(source.drive_name.clone()), DriveKey::Id)
    }
}

/// Every history list in the store's snapshots (of one library, or all of them), each once, in
/// the order they were first seen.
pub fn archive(store: &Store, library: Option<Uuid>, ctx: &Ctx<'_>) -> Result<HistoryArchive> {
    let names: FxHashMap<Uuid, String> = store.libraries()?.into_iter().map(|l| (l.id, l.name)).collect();
    // Which snapshots, without their file lists; each one's files are read in turn below.
    let mut snapshots: Vec<SnapshotInfo> = Vec::new();
    for id in store.snapshot_ids()? {
        let s = store.load_info(&id)?;
        if library.is_none_or(|l| l == s.library_id) {
            snapshots.push(s);
        }
    }
    snapshots.sort_by_key(|s| (s.created, s.id));

    let mut out = HistoryArchive {
        lists: Vec::new(),
        snapshots: snapshots.len(),
        warnings: Vec::new(),
        pulled: FxHashMap::default(),
    };
    let mut index: FxHashMap<(String, Vec<String>), usize> = FxHashMap::default();
    // Snapshots of one state share their database files; read each set once.
    let mut cache: FxHashMap<Vec<Option<Hash>>, Vec<HistoryList>> = FxHashMap::default();
    ctx.phase_start(Phase::Scan, snapshots.len() as u64, 0);
    for info in &snapshots {
        ctx.check()?;
        ctx.emit(Event::Item { phase: Phase::Scan, path: info.short_id.clone(), bytes: 0 });
        let s = store.load_snapshot(&info.id)?;
        let databases: Vec<Option<Hash>> = [paths::EXPORT_PDB, paths::LIBRARY_PLUS_DB, paths::LIBRARY_PLUS_WAL]
            .iter()
            .map(|k| s.files.iter().find(|f| f.path == *k).and_then(|f| f.hash))
            .collect();
        if !cache.contains_key(&databases) {
            match Databases::from_snapshot(store, &s) {
                Ok(dbs) => cache.insert(databases.clone(), histories(&dbs)),
                Err(e) => {
                    out.warnings.push(format!("snapshot {}: {e}", s.id.short()));
                    continue;
                }
            };
        }
        let lists = &cache[&databases];
        // The drive's snapshot before this one held none of the lists this one is the first to
        // hold. Only snapshots that were read count: one that wasn't might have held them.
        let drive = s.source.as_ref().map(DriveKey::of);
        let recorded_after = drive.as_ref().and_then(|d| out.pulled.get(d).copied());
        for list in lists {
            match index.get(&list.identity()) {
                Some(&i) => out.lists[i].snapshots += 1,
                None => {
                    index.insert(list.identity(), out.lists.len());
                    out.lists.push(ArchivedHistory {
                        list: list.clone(),
                        first_seen: Sighting {
                            snapshot: s.id,
                            short_id: s.id.short(),
                            created: s.created,
                            library: names.get(&s.library_id).cloned().unwrap_or_else(|| s.library_id.to_string()),
                            drive: s.source.as_ref().map(|src| src.drive_name.clone()),
                        },
                        recorded_after,
                        snapshots: 1,
                    });
                }
            }
        }
        if let Some(d) = drive {
            out.pulled.insert(d, s.created);
        }
    }
    ctx.phase_end(Phase::Scan);
    Ok(out)
}

/// Whether `list` is among `archived` (same name and tracks).
pub fn is_archived(list: &HistoryList, archived: &HistoryArchive) -> bool {
    archived.lists.iter().any(|a| a.list.same_as(list))
}

// ---------------------------------------------------------------------------------------------
// Statistics
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct TrackPlays {
    pub track: TrackRef,
    /// The play count rekordbox and the players keep (the higher of the two databases').
    pub play_count: u16,
    /// History lists the track appears in.
    pub history_lists: usize,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct PlayStats {
    /// What the statistics are about: a drive, or a snapshot.
    pub source: String,
    pub tracks: usize,
    pub history_lists: usize,
    /// Tracks with a play count or in a history list.
    pub played: usize,
    /// Play count 0 and in no history list.
    pub never_played: Vec<TrackRef>,
    /// Most played first.
    pub most_played: Vec<TrackPlays>,
    /// With a `since` snapshot: what was played between it and this state.
    pub since: Option<PlayedSince>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct PlayedSince {
    pub snapshot: Hash,
    pub short_id: String,
    pub created: DateTime<Utc>,
    /// History lists recorded since.
    pub new_histories: Vec<String>,
    /// Tracks played since, most first.
    pub tracks: Vec<PlayDelta>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct PlayDelta {
    pub track: TrackRef,
    pub before: u16,
    pub after: u16,
    /// History lists recorded since that the track appears in.
    pub history_lists: usize,
}

/// Play count per file: the higher of the two databases'.
fn play_counts(dbs: &Databases) -> FxHashMap<String, u16> {
    let mut out: FxHashMap<String, u16> = FxHashMap::default();
    for (_, lib) in dbs.all() {
        for t in &lib.tracks {
            let n = out.entry(file_key(&t.file_path)).or_default();
            *n = (*n).max(t.play_count);
        }
    }
    out
}

/// Number of lists each file appears in.
fn list_counts<'a>(lists: impl IntoIterator<Item = &'a HistoryList>) -> FxHashMap<String, usize> {
    let mut out: FxHashMap<String, usize> = FxHashMap::default();
    for list in lists {
        for key in list.tracks.iter().map(|t| file_key(&t.file_path)).collect::<FxHashSet<_>>() {
            *out.entry(key).or_default() += 1;
        }
    }
    out
}

/// Play statistics of a library: tracks never played, the `limit` most played.
pub(crate) fn stats(source: String, dbs: &Databases, limit: usize) -> PlayStats {
    let library = dbs.primary();
    let counts = play_counts(dbs);
    let lists = histories(dbs);
    let in_lists = list_counts(&lists);
    let mut played: Vec<TrackPlays> = Vec::new();
    let mut never_played = Vec::new();
    for t in &library.tracks {
        let key = file_key(&t.file_path);
        let play_count = counts.get(&key).copied().unwrap_or(0);
        let history_lists = in_lists.get(&key).copied().unwrap_or(0);
        if play_count == 0 && history_lists == 0 {
            never_played.push(TrackRef::from(t));
        } else {
            played.push(TrackPlays { track: t.into(), play_count, history_lists });
        }
    }
    played.sort_by(|a, b| {
        b.play_count.cmp(&a.play_count).then(b.history_lists.cmp(&a.history_lists)).then(a.track.id.cmp(&b.track.id))
    });
    let n = played.len();
    played.truncate(limit);
    PlayStats {
        source,
        tracks: library.tracks.len(),
        history_lists: lists.len(),
        played: n,
        never_played,
        most_played: played,
        since: None,
    }
}

/// What was played between `snapshot` (holding `before`) and `after`: play counts that went up
/// (tracks matched by file) and history lists recorded since.
pub(crate) fn played_since(before: &Databases, snapshot: &Snapshot, after: &Databases) -> PlayedSince {
    let old_counts = play_counts(before);
    let new_counts = play_counts(after);
    let old_lists: FxHashSet<(String, Vec<String>)> = histories(before).iter().map(HistoryList::identity).collect();
    let new_lists: Vec<HistoryList> =
        histories(after).into_iter().filter(|l| !old_lists.contains(&l.identity())).collect();
    let in_lists = list_counts(&new_lists);
    let mut tracks: Vec<PlayDelta> = after
        .primary()
        .tracks
        .iter()
        .filter_map(|t| {
            let key = file_key(&t.file_path);
            let after = new_counts.get(&key).copied().unwrap_or(0);
            // A track that wasn't there before came with whatever count rekordbox had for it.
            let before = old_counts.get(&key).copied().unwrap_or(after);
            let history_lists = in_lists.get(&key).copied().unwrap_or(0);
            (after > before || history_lists > 0).then(|| PlayDelta { track: t.into(), before, after, history_lists })
        })
        .collect();
    tracks.sort_by(|a, b| {
        (b.after.saturating_sub(b.before))
            .cmp(&a.after.saturating_sub(a.before))
            .then(b.history_lists.cmp(&a.history_lists))
            .then(a.track.id.cmp(&b.track.id))
    });
    PlayedSince {
        snapshot: snapshot.id,
        short_id: snapshot.id.short(),
        created: snapshot.created,
        new_histories: new_lists.into_iter().map(|l| l.path).collect(),
        tracks,
    }
}
