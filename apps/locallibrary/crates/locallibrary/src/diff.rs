//! Entry-level differences between the playlists of two libraries: two drives, a drive and a
//! store snapshot, or two snapshots (or a drive and one of its backups, to preview a restore).
//!
//! Playlists are matched by their place in the folder tree (`model::PlaylistKey`), tracks by
//! file (as everywhere: rekordbox keeps paths stable across exports of one library). Within a
//! playlist each entry is *added*, *removed* or *moved*: the k-th occurrence of a file on one side
//! pairs with its k-th occurrence on the other, and of the paired entries, the longest run that
//! kept its relative order stays put; the rest moved.

use indexmap::IndexMap;

use rustc_hash::FxHashMap;
use schemars::JsonSchema;
use serde::Serialize;

use crate::model::{Library, LibraryDiff, PlaylistKey, Track, TrackChange, TrackRef};
use crate::paths;
use crate::{Error, Result};

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct PlaylistDiff {
    pub left: String,
    pub right: String,
    /// Tracks (in the collection) only on the right.
    pub added_tracks: Vec<TrackRef>,
    /// Tracks only on the left.
    pub removed_tracks: Vec<TrackRef>,
    /// Tracks on both sides whose details differ.
    pub changed_tracks: Vec<TrackChange>,
    /// Playlists (and folders, ending in `/`) only on the right.
    pub added_playlists: Vec<String>,
    /// Playlists (and folders) only on the left.
    pub removed_playlists: Vec<String>,
    /// Playlists on both sides whose entries differ.
    pub changed_playlists: Vec<PlaylistChanges>,
    /// Playlists on both sides with the same entries in the same order.
    pub unchanged_playlists: usize,
}

impl PlaylistDiff {
    pub fn is_empty(&self) -> bool {
        self.added_tracks.is_empty()
            && self.removed_tracks.is_empty()
            && self.changed_tracks.is_empty()
            && self.added_playlists.is_empty()
            && self.removed_playlists.is_empty()
            && self.changed_playlists.is_empty()
    }
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct PlaylistChanges {
    pub path: String,
    /// Entries only on the right, at their (1-based) position there.
    pub added: Vec<Entry>,
    /// Entries only on the left, at their position there.
    pub removed: Vec<Entry>,
    pub moved: Vec<Move>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
#[schemars(rename = "DiffEntry")]
pub struct Entry {
    pub track: TrackRef,
    pub position: usize,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Move {
    pub track: TrackRef,
    /// 1-based position on the left.
    pub from: usize,
    /// 1-based position on the right.
    pub to: usize,
}

/// Playlist key -> its path and tracks in order (`None` for folders, whose path ends in `/`).
fn playlists(lib: &Library) -> IndexMap<PlaylistKey<'_>, (String, Option<Vec<&Track>>)> {
    let by_id: FxHashMap<u32, &Track> = lib.tracks.iter().map(|t| (t.id, t)).collect();
    let mut out: IndexMap<_, _> = lib
        .keyed_playlists()
        .map(|(key, path, p)| match p.is_folder {
            true => (key, (path, None)),
            false => (key, (path, Some(p.track_ids.iter().filter_map(|id| by_id.get(id).copied()).collect()))),
        })
        .collect();
    out.sort_unstable_keys();
    out
}

fn file_key(t: &Track) -> String {
    paths::key(&t.file_path).to_lowercase()
}

/// Whether `path` is `filter` or below it (case-insensitive; folders end in `/`).
fn within(path: &str, filter: &str) -> bool {
    let (p, f) = (path.to_lowercase(), filter.trim_matches('/').to_lowercase());
    p == f || p.trim_end_matches('/') == f || p.starts_with(&format!("{f}/"))
}

/// Refuse a `playlist` filter for [`between`] that keeps nothing on either side: it would show as
/// no differences.
pub fn check_playlist(left: &Library, right: &Library, playlist: Option<&str>) -> Result<()> {
    let has = |lib: &Library, p: &str| playlists(lib).values().any(|(path, _)| within(path, p));
    match playlist {
        Some(p) if !has(left, p) && !has(right, p) => Err(Error::NotFound(format!("playlist {p:?} on either side"))),
        _ => Ok(()),
    }
}

/// Differences from `left` to `right`. With `playlist`, only that playlist (or the playlists in
/// that folder), and no collection-level changes.
pub fn between(left: &Library, right: &Library, labels: (&str, &str), playlist: Option<&str>) -> PlaylistDiff {
    let mut diff = PlaylistDiff {
        left: labels.0.to_string(),
        right: labels.1.to_string(),
        added_tracks: Vec::new(),
        removed_tracks: Vec::new(),
        changed_tracks: Vec::new(),
        added_playlists: Vec::new(),
        removed_playlists: Vec::new(),
        changed_playlists: Vec::new(),
        unchanged_playlists: 0,
    };
    if playlist.is_none() {
        let d = LibraryDiff::between(left, right);
        diff.added_tracks = d.added_tracks;
        diff.removed_tracks = d.removed_tracks;
        diff.changed_tracks = d.changed_tracks;
    }
    let keep = |path: &String| playlist.is_none_or(|f| within(path, f));
    let (old, new) = (playlists(left), playlists(right));
    for (key, (path, entries)) in new.iter().filter(|(_, (p, _))| keep(p)) {
        match (old.get(key), entries) {
            (None, _) => diff.added_playlists.push(path.clone()),
            (Some((_, Some(a))), Some(b)) => match entries_diff(path, a, b) {
                Some(changes) => diff.changed_playlists.push(changes),
                None => diff.unchanged_playlists += 1,
            },
            _ => {}
        }
    }
    diff.removed_playlists =
        old.iter().filter(|(k, (p, _))| keep(p) && !new.contains_key(*k)).map(|(_, (p, _))| p.clone()).collect();
    diff
}

/// How the entries of one playlist changed, or `None` if they didn't.
fn entries_diff(path: &str, left: &[&Track], right: &[&Track]) -> Option<PlaylistChanges> {
    let lk: Vec<String> = left.iter().map(|t| file_key(t)).collect();
    let rk: Vec<String> = right.iter().map(|t| file_key(t)).collect();
    if lk == rk {
        return None;
    }
    // Pair the k-th occurrence of each file on the left with its k-th occurrence on the right.
    let mut slots: FxHashMap<&str, Vec<usize>> = FxHashMap::default();
    for (j, k) in rk.iter().enumerate().rev() {
        slots.entry(k).or_default().push(j);
    }
    let mut pairs: Vec<(usize, usize)> = Vec::new(); // (left index, right index), by left index
    let mut paired_right = vec![false; right.len()];
    let mut removed = Vec::new();
    for (i, k) in lk.iter().enumerate() {
        match slots.get_mut(k.as_str()).and_then(Vec::pop) {
            Some(j) => {
                pairs.push((i, j));
                paired_right[j] = true;
            }
            None => removed.push(Entry { track: left[i].into(), position: i + 1 }),
        }
    }
    let added = (0..right.len())
        .filter(|j| !paired_right[*j])
        .map(|j| Entry { track: right[j].into(), position: j + 1 })
        .collect();
    // Paired entries in the longest increasing run of right positions kept their order.
    let stays = longest_increasing(&pairs.iter().map(|(_, j)| *j).collect::<Vec<_>>());
    let moved = pairs
        .iter()
        .enumerate()
        .filter(|(n, _)| !stays[*n])
        .map(|(_, (i, j))| Move { track: left[*i].into(), from: i + 1, to: j + 1 })
        .collect();
    Some(PlaylistChanges { path: path.to_string(), added, removed, moved })
}

/// Which elements of `v` (distinct values) belong to one longest strictly increasing
/// subsequence. O(n log n).
fn longest_increasing(v: &[usize]) -> Vec<bool> {
    // tails[k]: index in v of the smallest tail of an increasing run of length k + 1.
    let mut tails: Vec<usize> = Vec::new();
    let mut prev: Vec<Option<usize>> = vec![None; v.len()];
    for i in 0..v.len() {
        let k = tails.partition_point(|&t| v[t] < v[i]);
        prev[i] = k.checked_sub(1).map(|k| tails[k]);
        if k == tails.len() {
            tails.push(i);
        } else {
            tails[k] = i;
        }
    }
    let mut out = vec![false; v.len()];
    let mut cur = tails.last().copied();
    while let Some(i) = cur {
        out[i] = true;
        cur = prev[i];
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Playlist;
    use loclib_model::test_util::track;

    fn lib(lists: &[(&str, &[u32])]) -> Library {
        Library {
            tracks: (1..=6).map(|i| track(i, "A", &format!("T{i}"), &format!("/Contents/{i}.mp3"))).collect(),
            playlists: lists
                .iter()
                .enumerate()
                .map(|(n, (name, ids))| Playlist {
                    id: n as u32 + 1,
                    parent_id: None,
                    name: name.to_string(),
                    is_folder: false,
                    sort_order: n as u32,
                    track_ids: ids.to_vec(),
                })
                .collect(),
            ..Default::default()
        }
    }

    #[test]
    fn added_removed_and_moved_entries() {
        let a = lib(&[("Set", &[1, 2, 3, 4]), ("Same", &[1]), ("Gone", &[2])]);
        let b = lib(&[("Set", &[2, 3, 1, 5, 2]), ("Same", &[1]), ("New", &[])]);
        let d = between(&a, &b, ("a", "b"), None);
        assert_eq!(d.added_playlists, ["New"]);
        assert_eq!(d.removed_playlists, ["Gone"]);
        assert_eq!(d.unchanged_playlists, 1);
        let set = &d.changed_playlists[0];
        let names = |v: &[Entry]| v.iter().map(|e| (e.track.name.clone(), e.position)).collect::<Vec<_>>();
        assert_eq!(names(&set.added), [("A - T5".to_string(), 4), ("A - T2".to_string(), 5)]);
        assert_eq!(names(&set.removed), [("A - T4".to_string(), 4)]);
        // 2 and 3 kept their order; 1 moved from the top to after them.
        assert_eq!(set.moved.len(), 1);
        assert_eq!((set.moved[0].track.name.as_str(), set.moved[0].from, set.moved[0].to), ("A - T1", 1, 3));

        let only = between(&a, &b, ("a", "b"), Some("same"));
        assert!(only.is_empty() && only.unchanged_playlists == 1);
    }

    #[test]
    fn lis() {
        assert_eq!(longest_increasing(&[1, 2, 0]), [true, true, false]);
        assert_eq!(longest_increasing(&[3, 0, 1, 2]), [false, true, true, true]);
        assert!(longest_increasing(&[]).is_empty());
    }
}
