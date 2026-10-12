//! Format-independent library model: tracks, playlists and the differences between libraries.

use indexmap::IndexMap;

use rustc_hash::{FxHashMap, FxHashSet};
use serde::{Deserialize, Serialize};
use unicode_normalization::UnicodeNormalization;

use crate::tokens::{Rgb, track_colors};

/// A rekordbox library as seen on a device export.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct Library {
    pub tracks: Vec<Track>,
    pub playlists: Vec<Playlist>,
    /// Number of history playlists (players record what was played).
    pub history_playlists: usize,
    /// The history playlists themselves (and, in OneLibrary, their folders), shaped like
    /// playlists.
    #[serde(default)]
    pub history: Vec<Playlist>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct Track {
    /// Row id inside this export. Only meaningful within one export.
    pub id: u32,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub album_artist: String,
    pub genre: String,
    pub label: String,
    pub key: String,
    pub remixer: String,
    pub original_artist: String,
    pub composer: String,
    pub comment: String,
    pub mix_name: String,
    pub isrc: String,
    /// Beats per minute (as analysed).
    pub bpm: f64,
    pub duration_secs: u32,
    pub year: u16,
    pub track_number: u32,
    pub disc_number: u16,
    /// 0-5 stars.
    pub rating: u8,
    pub color: Option<TrackColor>,
    pub play_count: u16,
    pub bitrate: u32,
    pub sample_rate: u32,
    pub sample_depth: u16,
    pub file_size: u64,
    /// Absolute device path of the audio file, e.g. `/Contents/Artist/Album/Track.mp3`.
    pub file_path: String,
    /// Device path of the primary analysis file (`.DAT`); `.EXT`/`.2EX` sit next to it.
    pub analyze_path: String,
    pub date_added: String,
    pub release_date: String,
    pub artwork_id: Option<u32>,
    /// Device path of the artwork thumbnail, if any.
    pub artwork_path: Option<String>,
}

impl Track {
    /// "Artist - Title", for display.
    pub fn display_name(&self) -> String {
        if self.artist.is_empty() { self.title.clone() } else { format!("{} - {}", self.artist, self.title) }
    }

    /// Identity used to recognise the same recording across *different* libraries, where row
    /// ids and paths differ: normalised artist + title + duration.
    pub fn fingerprint_key(&self) -> String {
        format!("{}\u{1f}{}\u{1f}{}", normalize(&self.artist), normalize(&self.title), self.duration_secs)
    }
}

/// rekordbox's eight track colours.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum TrackColor {
    Pink,
    Red,
    Orange,
    Yellow,
    Green,
    Aqua,
    Blue,
    Purple,
}

impl TrackColor {
    pub fn from_id(id: u8) -> Option<Self> {
        Some(match id {
            1 => Self::Pink,
            2 => Self::Red,
            3 => Self::Orange,
            4 => Self::Yellow,
            5 => Self::Green,
            6 => Self::Aqua,
            7 => Self::Blue,
            8 => Self::Purple,
            _ => return None,
        })
    }

    pub fn id(self) -> u8 {
        self as u8 + 1
    }

    /// Roughly how rekordbox's players show it.
    pub const fn rgb(self) -> Rgb {
        match self {
            Self::Pink => track_colors::PINK,
            Self::Red => track_colors::RED,
            Self::Orange => track_colors::ORANGE,
            Self::Yellow => track_colors::YELLOW,
            Self::Green => track_colors::GREEN,
            Self::Aqua => track_colors::AQUA,
            Self::Blue => track_colors::BLUE,
            Self::Purple => track_colors::PURPLE,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct Playlist {
    pub id: u32,
    /// `None` for top-level nodes.
    pub parent_id: Option<u32>,
    pub name: String,
    pub is_folder: bool,
    pub sort_order: u32,
    /// Track ids in playlist order (empty for folders).
    pub track_ids: Vec<u32>,
}

/// Finds a playlist (or folder) again in another export of the same library, where ids differ:
/// the names down its folder path, each with whether it's a folder and how many same-named
/// siblings of its kind come before it. The path alone can't do that, since rekordbox allows two
/// playlists with one name in a folder, and a `/` in a name.
pub type PlaylistKey<'a> = Vec<(&'a str, bool, usize)>;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct LibrarySummary {
    pub tracks: usize,
    pub playlists: usize,
    pub folders: usize,
    pub artists: usize,
    pub audio_bytes: u64,
    pub total_duration_secs: u64,
    pub tracks_without_artwork: usize,
}

impl Library {
    pub fn summary(&self) -> LibrarySummary {
        let mut artists: Vec<&str> = self.tracks.iter().map(|t| t.artist.as_str()).collect();
        artists.sort_unstable();
        artists.dedup();
        LibrarySummary {
            tracks: self.tracks.len(),
            playlists: self.playlists.iter().filter(|p| !p.is_folder).count(),
            folders: self.playlists.iter().filter(|p| p.is_folder).count(),
            artists: artists.len(),
            audio_bytes: self.tracks.iter().map(|t| t.file_size).sum(),
            total_duration_secs: self.tracks.iter().map(|t| t.duration_secs as u64).sum(),
            tracks_without_artwork: self.tracks.iter().filter(|t| t.artwork_id.is_none()).count(),
        }
    }

    pub fn track(&self, id: u32) -> Option<&Track> {
        self.tracks.iter().find(|t| t.id == id)
    }

    /// The tracks of `ids` in order, repeats kept, leaving out ids no track has: a playlist's
    /// entries, say. [`Library::track`] for each would scan the library each time.
    pub fn tracks_by_id(&self, ids: &[u32]) -> Vec<&Track> {
        // Collected from the back, so the first of a repeated id wins, as in `track`.
        let by_id: FxHashMap<u32, &Track> = self.tracks.iter().rev().map(|t| (t.id, t)).collect();
        ids.iter().filter_map(|id| by_id.get(id).copied()).collect()
    }

    pub fn playlist(&self, id: u32) -> Option<&Playlist> {
        self.playlists.iter().find(|p| p.id == id)
    }

    /// [`Library::playlist`] through a map built once, for walking many lineages (a scan for each
    /// folder above each playlist adds up).
    fn playlist_lookup<'a>(&'a self) -> impl Fn(u32) -> Option<&'a Playlist> {
        // Collected from the back, so the first of a repeated id wins, as in `playlist`.
        let by_id: FxHashMap<u32, &Playlist> = self.playlists.iter().rev().map(|p| (p.id, p)).collect();
        move |id| by_id.get(&id).copied()
    }

    /// Slash-separated path of a playlist through its folders, e.g. `Sets/2026/Warmup`.
    pub fn playlist_path(&self, id: u32) -> String {
        path(&lineage(id, |id| self.playlist(id)))
    }

    /// The names of a playlist and the folders above it, top first.
    pub fn playlist_names(&self, id: u32) -> Vec<String> {
        lineage(id, |id| self.playlist(id)).iter().map(|p| p.name.clone()).collect()
    }

    /// Every playlist and folder with its [`PlaylistKey`] and its path as people read it (folders
    /// end in `/`).
    pub fn keyed_playlists(&self) -> impl Iterator<Item = (PlaylistKey<'_>, String, &Playlist)> {
        // How many same-named siblings of its kind come before each node, in display order (as
        // `children` sorts them). A node whose folder isn't there is shown at the top level (its
        // lineage ends with it), so it counts among the top-level nodes: otherwise it would share
        // a key with a top-level node of its name, and a diff would miss one of them.
        let get = self.playlist_lookup();
        let mut order: Vec<&Playlist> = self.playlists.iter().collect();
        order.sort_by_key(|p| (p.sort_order, p.id));
        let mut seen: FxHashMap<(Option<u32>, &str, bool), usize> = FxHashMap::default();
        let nth: FxHashMap<u32, usize> = order
            .into_iter()
            .map(|p| {
                let n = seen.entry((p.parent_id.filter(|&id| get(id).is_some()), &p.name, p.is_folder)).or_default();
                *n += 1;
                (p.id, *n - 1)
            })
            .collect();
        self.playlists.iter().map(move |p| {
            let line = lineage(p.id, &get);
            let key = line.iter().map(|q| (q.name.as_str(), q.is_folder, nth[&q.id])).collect();
            (key, path(&line) + if p.is_folder { "/" } else { "" }, p)
        })
    }

    /// Find a playlist by its path (`Folder/Name`): the first shown that matches exactly, else
    /// ignoring ASCII case when every match shares one path. Failing that, by name, exactly and
    /// then ignoring ASCII case, each only when that leaves one playlist (`Foo` and `FoO` can sit
    /// side by side). A playlist and a folder can share a path too: the playlist is found unless
    /// the query ends in `/`, the way folders are listed. Blank names in the query are left out.
    pub fn find_playlist(&self, query: &str) -> Option<&Playlist> {
        self.find_node(query, None)
    }

    /// [`Library::find_playlist`] among folders only (`folder`) or playlists only, for something
    /// that needs one of them: a playlist shown after a folder of its name is still found.
    pub fn find_playlist_of_kind(&self, query: &str, folder: bool) -> Option<&Playlist> {
        self.find_node(query, Some(folder))
    }

    fn find_node(&self, query: &str, folder: Option<bool>) -> Option<&Playlist> {
        fn only<T>(mut found: impl Iterator<Item = T>) -> Option<T> {
            match (found.next(), found.next()) {
                (Some(x), None) => Some(x),
                _ => None,
            }
        }
        // A blank name is no name, as edits read paths (and won't give a node one): `A/ ` is the
        // folder `A/`, not a node named ` ` in it, and a query of blanks finds nothing.
        let names: Vec<&str> = query.split('/').filter(|n| !n.trim().is_empty()).collect();
        if names.is_empty() {
            return None;
        }
        let q: &str = &names.join("/");
        let kind = move |p: &Playlist| folder.is_none_or(|f| p.is_folder == f);
        let named = |same: fn(&str, &str) -> bool| self.playlists.iter().filter(move |p| kind(p) && same(&p.name, q));
        // Same-named siblings (rekordbox allows them) share a path: the one shown first wins, as
        // it does when edits are applied. Of a playlist and a folder sharing one, the kind asked
        // for comes first (a stable sort keeps the display order within each kind).
        let mut shown: Vec<(String, &Playlist)> = self.display_order().into_iter().filter(|(_, p)| kind(p)).collect();
        if folder.is_none() {
            let folders_first = query.trim_end().ends_with('/');
            shown.sort_by_key(|(_, p)| p.is_folder != folders_first);
        }
        pick(shown, q).or_else(|| only(named(|a, b| a == b))).or_else(|| only(named(str::eq_ignore_ascii_case)))
    }

    /// Every playlist and folder with its path, in display order, as the tree shows them: each
    /// folder followed by what's in it, siblings as [`Library::children`] sorts them.
    pub fn display_order(&self) -> Vec<(String, &Playlist)> {
        let get = self.playlist_lookup();
        let mut v: Vec<(Vec<&Playlist>, &Playlist)> = self.playlists.iter().map(|p| (lineage(p.id, &get), p)).collect();
        v.sort_by_cached_key(|(line, _)| line.iter().map(|l| (l.sort_order, l.id)).collect::<Vec<_>>());
        v.into_iter().map(|(line, p)| (path(&line), p)).collect()
    }

    /// Children of a folder (or top-level nodes for `None`) in display order. A node whose folder
    /// isn't there is at the top level, as it's shown, so a folder that isn't there has the
    /// top-level nodes for children.
    pub fn children(&self, parent: Option<u32>) -> Vec<&Playlist> {
        let get = self.playlist_lookup();
        let placed = |id: Option<u32>| id.filter(|&id| get(id).is_some());
        let parent = placed(parent);
        let mut v: Vec<&Playlist> = self.playlists.iter().filter(|p| placed(p.parent_id) == parent).collect();
        v.sort_by_key(|p| (p.sort_order, p.id));
        v
    }

    /// Search tracks by id (`#12` or `12`), or by case-insensitive substring of
    /// "artist - title" / file path.
    pub fn search(&self, query: &str) -> Vec<&Track> {
        let q = query.trim();
        if let Ok(id) = q.trim_start_matches('#').parse::<u32>()
            && let Some(t) = self.track(id)
        {
            return vec![t];
        }
        let needle = normalize(q);
        self.tracks
            .iter()
            .filter(|t| normalize(&t.display_name()).contains(&needle) || normalize(&t.file_path).contains(&needle))
            .collect()
    }
}

/// How deep folders nest at most in a tree read from a file: deeper rows are taken for a loop in
/// a damaged file (see [`untangle`]), so edits and merges never nest playlists deeper. A walk
/// over a library built elsewhere, which may still hold a loop, stops there too.
pub const MAX_DEPTH: usize = 64;

/// A playlist and the folders above it, top first, each found by id with `get`: [`MAX_DEPTH`] of
/// them at most, since [`Library`]'s fields are public and a library built elsewhere may hold a
/// loop.
fn lineage<'a>(id: u32, get: impl Fn(u32) -> Option<&'a Playlist>) -> Vec<&'a Playlist> {
    let mut parts = Vec::new();
    let mut cur = get(id);
    while let Some(p) = cur.filter(|_| parts.len() < MAX_DEPTH) {
        parts.push(p);
        cur = p.parent_id.and_then(&get);
    }
    parts.reverse();
    parts
}

/// A playlist (or history) tree as a database holds it, made safe to walk: the first row of each
/// id is kept, and a row whose folders lead back to it, or that would sit more than
/// [`MAX_DEPTH`] deep, moves to the top level. A damaged or crafted file can repeat an id, so a
/// folder holds itself or holds two copies of itself; walking children from the top would then
/// never end, or double at every level. Afterwards each row is reached once at most, and never
/// more than [`MAX_DEPTH`] down.
pub fn untangle(mut rows: Vec<Playlist>) -> Vec<Playlist> {
    let mut seen = FxHashSet::default();
    rows.retain(|p| seen.insert(p.id));
    let at: FxHashMap<u32, usize> = rows.iter().enumerate().map(|(i, p)| (p.id, i)).collect();
    let mut depth: Vec<Option<usize>> = vec![None; rows.len()];
    let mut on_path = vec![false; rows.len()];
    for start in 0..rows.len() {
        // Up from `start` until the top (or a folder that isn't there), a row already placed, or
        // a row met on the way up, which closes a loop.
        let mut path = Vec::new();
        let mut up = Some(start);
        while let Some(i) = up.filter(|&i| depth[i].is_none() && !on_path[i]) {
            on_path[i] = true;
            path.push(i);
            up = rows[i].parent_id.and_then(|p| at.get(&p).copied());
        }
        // Then back down, placing each row below the one above it. The folder above the top of
        // the path is still unplaced only when it's on the path: there the loop is cut.
        for (n, &i) in path.iter().enumerate().rev() {
            let above = path.get(n + 1).copied().or(up);
            depth[i] = Some(match above.map(|a| depth[a]) {
                None => 0,
                Some(Some(d)) if d + 1 < MAX_DEPTH => d + 1,
                Some(_) => {
                    rows[i].parent_id = None;
                    0
                }
            });
        }
    }
    rows
}

/// The slash-separated path of a [`lineage`].
fn path(lineage: &[&Playlist]) -> String {
    lineage.iter().map(|p| p.name.as_str()).collect::<Vec<_>>().join("/")
}

/// Of `items`, each with its path (or name), the one `query` names, as [`Library::find_playlist`]
/// names one by path: the first whose path is exactly `query` (same-named siblings share a path,
/// so give them in display order), else the first whose path matches ignoring ASCII case, when
/// only one path does.
pub fn pick<S: AsRef<str>, T>(items: impl IntoIterator<Item = (S, T)>, query: &str) -> Option<T> {
    let mut loose = Vec::new();
    for (path, x) in items {
        match path.as_ref() {
            p if p == query => return Some(x),
            p if p.eq_ignore_ascii_case(query) => loose.push((path, x)),
            _ => {}
        }
    }
    let one_path = loose.iter().all(|(p, _)| p.as_ref() == loose[0].0.as_ref());
    if one_path { loose.into_iter().next().map(|(_, x)| x) } else { None }
}

/// Lower-case, NFKC, whitespace-collapsed form for fuzzy comparisons.
pub fn normalize(s: &str) -> String {
    let lower: String = s.nfkc().collect::<String>().to_lowercase();
    lower.split_whitespace().collect::<Vec<_>>().join(" ")
}

// ---------------------------------------------------------------------------------------------
// Diffing
// ---------------------------------------------------------------------------------------------

/// Differences between two exports *of the same library*. Tracks are matched by file path,
/// which rekordbox keeps stable across exports; playlists by their place in the folder tree.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct LibraryDiff {
    pub added_tracks: Vec<TrackRef>,
    pub removed_tracks: Vec<TrackRef>,
    pub changed_tracks: Vec<TrackChange>,
    pub added_playlists: Vec<String>,
    pub removed_playlists: Vec<String>,
    /// Playlists whose contents or order differ.
    pub changed_playlists: Vec<String>,
}

crate::labelled_tuple! {
    /// A tempo range: `[low, high]` in BPM.
    #[derive(Debug, Clone, Copy, PartialEq)]
    pub struct BpmRange {
        pub low: f64,
        pub high: f64,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct TrackRef {
    pub id: u32,
    pub name: String,
    pub file_path: String,
}

impl From<&Track> for TrackRef {
    fn from(t: &Track) -> Self {
        TrackRef { id: t.id, name: t.display_name(), file_path: t.file_path.clone() }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct TrackChange {
    pub track: TrackRef,
    pub fields: Vec<FieldChange>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct FieldChange {
    pub field: String,
    pub old: String,
    pub new: String,
}

impl LibraryDiff {
    pub fn is_empty(&self) -> bool {
        self.added_tracks.is_empty()
            && self.removed_tracks.is_empty()
            && self.changed_tracks.is_empty()
            && self.added_playlists.is_empty()
            && self.removed_playlists.is_empty()
            && self.changed_playlists.is_empty()
    }

    /// Diff `old` -> `new`.
    pub fn between(old: &Library, new: &Library) -> LibraryDiff {
        let mut diff = LibraryDiff::default();
        let old_by_path: FxHashMap<String, &Track> =
            old.tracks.iter().map(|t| (crate::paths::key(&t.file_path), t)).collect();
        let new_by_path: FxHashMap<String, &Track> =
            new.tracks.iter().map(|t| (crate::paths::key(&t.file_path), t)).collect();

        for t in &new.tracks {
            match old_by_path.get(&crate::paths::key(&t.file_path)) {
                None => diff.added_tracks.push(t.into()),
                Some(o) => {
                    let fields = track_field_changes(o, t);
                    if !fields.is_empty() {
                        diff.changed_tracks.push(TrackChange { track: t.into(), fields });
                    }
                }
            }
        }
        for t in &old.tracks {
            if !new_by_path.contains_key(&crate::paths::key(&t.file_path)) {
                diff.removed_tracks.push(t.into());
            }
        }

        let old_lists = playlist_contents(old);
        let new_lists = playlist_contents(new);
        for (key, (path, entries)) in &new_lists {
            match old_lists.get(key) {
                None => diff.added_playlists.push(path.clone()),
                Some((_, o)) if o != entries => diff.changed_playlists.push(path.clone()),
                _ => {}
            }
        }
        for (key, (path, _)) in &old_lists {
            if !new_lists.contains_key(key) {
                diff.removed_playlists.push(path.clone());
            }
        }
        diff
    }
}

/// Playlist key -> its path and ordered list of track file paths (folders map to an empty list).
fn playlist_contents(lib: &Library) -> IndexMap<PlaylistKey<'_>, (String, Vec<String>)> {
    let paths: FxHashMap<u32, &str> = lib.tracks.iter().map(|t| (t.id, t.file_path.as_str())).collect();
    let mut out: IndexMap<_, _> = lib
        .keyed_playlists()
        .map(|(key, path, p)| {
            let entries =
                p.track_ids.iter().map(|id| paths.get(id).map(|s| crate::paths::key(s)).unwrap_or_default()).collect();
            (key, (path, entries))
        })
        .collect();
    out.sort_unstable_keys();
    out
}

fn track_field_changes(a: &Track, b: &Track) -> Vec<FieldChange> {
    let mut out = Vec::new();
    let mut cmp = |field: &str, x: String, y: String| {
        if x != y {
            out.push(FieldChange { field: field.into(), old: x, new: y });
        }
    };
    cmp("title", a.title.clone(), b.title.clone());
    cmp("artist", a.artist.clone(), b.artist.clone());
    cmp("album", a.album.clone(), b.album.clone());
    cmp("genre", a.genre.clone(), b.genre.clone());
    cmp("key", a.key.clone(), b.key.clone());
    cmp("label", a.label.clone(), b.label.clone());
    cmp("comment", a.comment.clone(), b.comment.clone());
    cmp("bpm", format!("{:.2}", a.bpm), format!("{:.2}", b.bpm));
    cmp("rating", a.rating.to_string(), b.rating.to_string());
    cmp("color", format!("{:?}", a.color), format!("{:?}", b.color));
    cmp("file_size", a.file_size.to_string(), b.file_size.to_string());
    cmp("artwork", a.artwork_id.is_some().to_string(), b.artwork_id.is_some().to_string());
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::track;

    #[test]
    fn diff_detects_adds_removes_changes() {
        let old = Library {
            tracks: vec![track(1, "A", "One", "/Contents/A/1.mp3"), track(2, "B", "Two", "/Contents/B/2.mp3")],
            playlists: vec![Playlist {
                id: 1,
                parent_id: None,
                name: "Set".into(),
                is_folder: false,
                sort_order: 0,
                track_ids: vec![1, 2],
            }],
            history_playlists: 0,
            history: Vec::new(),
        };
        let mut new = old.clone();
        new.tracks.remove(1);
        new.tracks[0].rating = 5;
        // ids differ between exports; matching must use paths
        new.tracks[0].id = 10;
        new.tracks.push(track(11, "C", "Three", "/Contents/C/3.mp3"));
        new.playlists[0].track_ids = vec![10, 11];

        let d = LibraryDiff::between(&old, &new);
        assert_eq!(d.added_tracks.len(), 1);
        assert_eq!(d.removed_tracks.len(), 1);
        assert_eq!(d.changed_tracks.len(), 1);
        assert_eq!(d.changed_tracks[0].fields[0].field, "rating");
        assert_eq!(d.changed_playlists, vec!["Set".to_string()]);
        assert!(LibraryDiff::between(&old, &old).is_empty());
    }

    /// A playlist whose folder isn't there is shown at the top level, so it's the second of its
    /// name there: its key differs from the first's, and a diff sees it go.
    #[test]
    fn a_playlist_whose_folder_is_missing_is_keyed_at_the_top_level() {
        let node = |id, parent_id| Playlist {
            id,
            parent_id,
            name: "A".into(),
            is_folder: false,
            sort_order: 0,
            track_ids: vec![],
        };
        let old = Library { playlists: vec![node(1, None), node(2, Some(3))], ..Default::default() };
        let keys: Vec<PlaylistKey> = old.keyed_playlists().map(|(k, ..)| k).collect();
        assert_eq!(keys, [vec![("A", false, 0)], vec![("A", false, 1)]]);
        let new = Library { playlists: vec![node(1, None)], ..Default::default() };
        assert_eq!(LibraryDiff::between(&old, &new).removed_playlists, ["A"]);
    }

    #[test]
    fn search_and_playlist_paths() {
        let lib = Library {
            tracks: vec![track(1, "Daft Punk", "Around the World", "/Contents/x.mp3")],
            playlists: vec![
                Playlist {
                    id: 1,
                    parent_id: None,
                    name: "Shows".into(),
                    is_folder: true,
                    sort_order: 0,
                    track_ids: vec![],
                },
                Playlist {
                    id: 2,
                    parent_id: Some(1),
                    name: "Warmup".into(),
                    is_folder: false,
                    sort_order: 0,
                    track_ids: vec![1],
                },
            ],
            history_playlists: 0,
            history: Vec::new(),
        };
        assert_eq!(lib.search("daft  punk").len(), 1);
        assert_eq!(lib.search("#1").len(), 1);
        assert_eq!(lib.playlist_path(2), "Shows/Warmup");
        assert_eq!(lib.find_playlist("shows/warmup").unwrap().id, 2);
        assert_eq!(lib.find_playlist("Warmup").unwrap().id, 2);
    }

    #[test]
    fn playlists_match_exactly_then_ignoring_case_when_only_one_does() {
        let node = |id, parent_id, name: &str| Playlist {
            id,
            parent_id,
            name: name.into(),
            is_folder: parent_id.is_none(),
            sort_order: 0,
            track_ids: vec![],
        };
        let lib = Library {
            playlists: vec![
                node(1, None, "Shows"),
                node(2, Some(1), "Foo"),
                node(3, Some(1), "FoO"),
                node(4, None, "Sets"),
                node(5, Some(4), "foo"),
            ],
            ..Default::default()
        };
        let found = |q| lib.find_playlist(q).map(|p| p.id);
        assert_eq!(found("Shows/FoO"), Some(3));
        assert_eq!(found("Shows/Foo"), Some(2));
        assert_eq!(found("shows/foo"), None, "ambiguous path");
        assert_eq!(found("sets/FOO"), Some(5));
        assert_eq!(found("FoO"), Some(3));
        assert_eq!(found("foo"), Some(5));
        assert_eq!(found("FOO"), None, "ambiguous name");
    }

    /// Of playlists sharing a path, the one shown first is found, top down, whatever order they're
    /// stored in.
    #[test]
    fn same_named_siblings_are_found_in_display_order() {
        let node = |id, parent_id, name: &str, sort_order| Playlist {
            id,
            parent_id,
            name: name.into(),
            is_folder: name == "Shows",
            sort_order,
            track_ids: vec![],
        };
        let lib = Library {
            playlists: vec![
                node(1, None, "Set", 1),
                node(2, None, "Set", 0),
                node(3, None, "Shows", 2),
                node(4, None, "Shows", 3),
                // Shown after 6 (its folder is), though first among the two by its own order.
                node(5, Some(4), "Set", 0),
                node(6, Some(3), "Set", 1),
            ],
            ..Default::default()
        };
        assert_eq!(lib.find_playlist("Set").map(|p| p.id), Some(2));
        assert_eq!(lib.find_playlist("Shows/Set").map(|p| p.id), Some(6));
        // Ignoring case too: the twins share one path, so that's not ambiguous.
        assert_eq!(lib.find_playlist("shows/set").map(|p| p.id), Some(6));
    }

    /// A playlist shown after a folder of its name is found by its path, and the folder by the
    /// path with a `/` after it (as folders are listed), or when a folder is what's wanted.
    #[test]
    fn a_playlist_and_a_folder_sharing_a_path_are_told_apart_by_kind() {
        let node = |id, name: &str, is_folder, sort_order| Playlist {
            id,
            parent_id: None,
            name: name.into(),
            is_folder,
            sort_order,
            track_ids: vec![],
        };
        let lib = Library {
            playlists: vec![node(1, "Set", true, 0), node(2, "Set", false, 1), node(3, "Foo", false, 2)],
            ..Default::default()
        };
        let found = |q| lib.find_playlist(q).map(|p| p.id);
        assert_eq!((found("Set"), found("set"), found("/Set/"), found("set/")), (Some(2), Some(2), Some(1), Some(1)));
        assert_eq!(lib.find_playlist_of_kind("Set/", false).map(|p| p.id), Some(2));
        assert_eq!(lib.find_playlist_of_kind("Set", true).map(|p| p.id), Some(1));
        assert_eq!(lib.find_playlist_of_kind("Foo", true), None);
    }

    /// A blank name in a query is no name, as edits read paths: `Set/ ` is the folder `Set/`,
    /// never a node named ` ` in it, and a query of blanks finds nothing.
    #[test]
    fn blank_names_in_a_query_are_no_names() {
        let node = |id, parent_id, name: &str, is_folder| Playlist {
            id,
            parent_id,
            name: name.into(),
            is_folder,
            sort_order: id,
            track_ids: vec![],
        };
        let lib = Library {
            playlists: vec![
                node(1, None, "Set", true),
                node(2, None, "Set", false),
                node(3, Some(1), "  ", true),
                node(4, Some(1), "  ", false),
                node(5, None, " ", false),
            ],
            ..Default::default()
        };
        let found = |q| lib.find_playlist(q).map(|p| p.id);
        assert_eq!((found("Set/  "), found("Set/  /"), found(" /Set")), (Some(1), Some(1), Some(2)));
        assert_eq!(lib.find_playlist_of_kind("Set/  ", false).map(|p| p.id), Some(2));
        assert_eq!((found(" "), found("  "), found(" / ")), (None, None, None));
    }

    #[test]
    fn untangled_trees_have_each_id_once_no_loops_and_bounded_depth() {
        let node = |id, parent_id, name: &str| Playlist {
            id,
            parent_id,
            name: name.into(),
            is_folder: true,
            sort_order: 0,
            track_ids: vec![],
        };
        let parents = |rows: &[Playlist]| rows.iter().map(|p| (p.id, p.parent_id)).collect::<Vec<_>>();
        // A folder, then two rows repeating its id that name it as their folder (a damaged
        // export.pdb): only the first stays.
        let rows = untangle(vec![node(1, None, "F"), node(1, Some(1), "P"), node(1, Some(1), "Q")]);
        assert_eq!((parents(&rows), rows[0].name.as_str()), (vec![(1, None)], "F"));
        // A row in its own folder, and two folders in each other: the row closing each loop moves
        // to the top. Rows below a folder that isn't there keep their parent.
        let rows = untangle(vec![
            node(2, Some(2), "self"),
            node(3, Some(4), "a"),
            node(4, Some(3), "b"),
            node(5, Some(3), "in a"),
            node(6, Some(99), "lost"),
        ]);
        assert_eq!(parents(&rows), [(2, None), (3, Some(4)), (4, None), (5, Some(3)), (6, Some(99))]);
        // A chain of 200 nested folders is cut every 64.
        let chain: Vec<Playlist> = (1..=200).map(|id| node(id, (id > 1).then(|| id - 1), "x")).collect();
        let lib = Library { playlists: untangle(chain), ..Default::default() };
        let top: Vec<u32> = lib.children(None).iter().map(|p| p.id).collect();
        assert_eq!(top, [1, 65, 129, 193]);
        assert_eq!(lineage(64, |id| lib.playlist(id)).len(), 64);
        // Built by hand, a library can still hold a loop: a path through it names MAX_DEPTH nodes.
        let looped = Library { playlists: vec![node(1, Some(2), "a"), node(2, Some(1), "b")], ..Default::default() };
        assert_eq!(looped.playlist_names(1).len(), MAX_DEPTH);
    }

    use hegel::TestCase;
    use hegel::generators as gs;

    /// A track with every field drawn: short text, any numbers, a tempo in hundredths of a beat
    /// per minute (as rekordbox stores it).
    #[hegel::test_helper]
    fn any_track(tc: &TestCase, id: u32) -> Track {
        let text = || tc.draw(gs::text().max_size(6));
        Track {
            id,
            title: text(),
            artist: text(),
            album: text(),
            album_artist: text(),
            genre: text(),
            label: text(),
            key: text(),
            remixer: text(),
            original_artist: text(),
            composer: text(),
            comment: text(),
            mix_name: text(),
            isrc: text(),
            bpm: f64::from(tc.draw(gs::integers::<u32>().max_value(30_000))) / 100.0,
            duration_secs: tc.draw(gs::integers()),
            year: tc.draw(gs::integers()),
            track_number: tc.draw(gs::integers()),
            disc_number: tc.draw(gs::integers()),
            rating: tc.draw(gs::integers::<u8>().max_value(5)),
            color: TrackColor::from_id(tc.draw(gs::integers::<u8>().max_value(9))),
            play_count: tc.draw(gs::integers()),
            bitrate: tc.draw(gs::integers()),
            sample_rate: tc.draw(gs::integers()),
            sample_depth: tc.draw(gs::integers()),
            // Up to 4 GiB, so a library's total stays far from u64's limit.
            file_size: u64::from(tc.draw(gs::integers::<u32>())),
            file_path: text(),
            analyze_path: text(),
            date_added: text(),
            release_date: text(),
            artwork_id: tc.draw(gs::optional(gs::integers())),
            artwork_path: tc.draw(gs::optional(gs::text().max_size(6))),
        }
    }

    /// Up to `n` playlist rows with ids and folders from a small range, so ids repeat, folders
    /// are missing or hold themselves, and names collide; or a long chain of folders, maybe
    /// closed into a loop.
    #[hegel::test_helper]
    fn rows(tc: &TestCase, n: usize) -> Vec<Playlist> {
        let node = |id, parent_id| Playlist {
            id,
            parent_id,
            name: tc.draw(gs::text().alphabet("aAb ").min_size(1).max_size(2)),
            is_folder: tc.draw(gs::booleans()),
            sort_order: tc.draw(gs::integers::<u32>().max_value(3)),
            track_ids: tc.draw(gs::vecs(gs::integers::<u32>().max_value(4)).max_size(3)),
        };
        if tc.draw(gs::weighted_booleans(0.15)) {
            let len = tc.draw(gs::integers::<u32>().min_value(1).max_value(150));
            let closed = tc.draw(gs::booleans());
            return (1..=len).map(|id| node(id, if id == 1 { closed.then_some(len) } else { Some(id - 1) })).collect();
        }
        let row_count = tc.draw(gs::integers::<usize>().max_value(n));
        (0..row_count)
            .map(|_| {
                let id = tc.draw(gs::integers::<u32>().min_value(1).max_value(8));
                node(id, tc.draw(gs::optional(gs::integers::<u32>().min_value(1).max_value(9))))
            })
            .collect()
    }

    /// How many rows a walk up from `p` meets (itself included) before the top, a folder that
    /// isn't there, or a row it met already; and whether it ended on such a repeat.
    fn ancestors(rows: &[Playlist], p: &Playlist) -> (usize, bool) {
        let mut seen = vec![p.id];
        let mut cur = p.parent_id;
        while let Some(parent) = cur.and_then(|id| rows.iter().find(|r| r.id == id)) {
            if seen.contains(&parent.id) {
                return (seen.len(), true);
            }
            seen.push(parent.id);
            cur = parent.parent_id;
        }
        (seen.len(), false)
    }

    /// Libraries, playlist trees, diffs and tempo ranges read back from JSON as they were.
    #[hegel::test(test_cases = 100)]
    fn libraries_round_trip_through_json(tc: TestCase) {
        let count = tc.draw(gs::integers::<u32>().max_value(3));
        let tracks: Vec<Track> = (0..count).map(|id| any_track(&tc, id)).collect();
        let lib = Library {
            tracks,
            playlists: rows(&tc, 4),
            history_playlists: tc.draw(gs::integers::<usize>().max_value(10)),
            history: rows(&tc, 2),
        };
        let json = serde_json::to_string(&lib).unwrap();
        assert_eq!(serde_json::from_str::<Library>(&json).unwrap(), lib);
        // JSON from before history was kept reads as having none.
        let mut old: serde_json::Value = serde_json::from_str(&json).unwrap();
        old.as_object_mut().unwrap().remove("history");
        assert!(serde_json::from_value::<Library>(old).unwrap().history.is_empty());

        let diff = LibraryDiff::between(&lib, &Library::default());
        let value = serde_json::to_value(&diff).unwrap();
        let back: LibraryDiff = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(serde_json::to_value(&back).unwrap(), value);
        assert_eq!(back.is_empty(), lib.tracks.is_empty() && lib.playlists.is_empty());

        let low = lib.tracks.first().map_or(0.0, |t| t.bpm);
        let high = f64::from(tc.draw(gs::integers::<u16>()));
        let range = BpmRange { low, high };
        assert_eq!(serde_json::to_value(range).unwrap(), serde_json::json!([low, high]));
        assert_eq!(serde_json::from_value::<BpmRange>(serde_json::json!([low, high])).unwrap(), range);
    }

    /// Track colours are ids 1 to 8 and back, drawn in their tokens' colours in that order, and
    /// named in lower case in JSON.
    #[hegel::test(test_cases = 100)]
    fn track_colours_round_trip_through_their_ids(tc: TestCase) {
        let id = tc.draw(hegel::one_of!(gs::integers::<u8>().max_value(9), gs::integers::<u8>()));
        let tokens = [
            track_colors::PINK,
            track_colors::RED,
            track_colors::ORANGE,
            track_colors::YELLOW,
            track_colors::GREEN,
            track_colors::AQUA,
            track_colors::BLUE,
            track_colors::PURPLE,
        ];
        let every: Vec<Rgb> = (0..=u8::MAX).filter_map(TrackColor::from_id).map(TrackColor::rgb).collect();
        assert_eq!(every, tokens);
        match TrackColor::from_id(id) {
            None => assert!(!(1..=8).contains(&id)),
            Some(c) => {
                assert_eq!(c.id(), id);
                assert_eq!(c.rgb(), tokens[usize::from(id) - 1]);
                let name = serde_json::to_value(c).unwrap();
                assert_eq!(name, format!("{c:?}").to_lowercase());
                assert_eq!(serde_json::from_value::<TrackColor>(name).unwrap(), c);
            }
        }
    }

    /// The fields a diff compares, in the order it lists them.
    const DIFFED_FIELDS: [&str; 12] = [
        "title",
        "artist",
        "album",
        "genre",
        "key",
        "label",
        "comment",
        "bpm",
        "rating",
        "color",
        "file_size",
        "artwork",
    ];

    /// `t` with `field` changed to something else.
    fn change(t: &mut Track, field: &str) {
        match field {
            "title" => t.title.push('x'),
            "artist" => t.artist.push('x'),
            "album" => t.album.push('x'),
            "genre" => t.genre.push('x'),
            "key" => t.key.push('x'),
            "label" => t.label.push('x'),
            "comment" => t.comment.push('x'),
            "bpm" => t.bpm += 1.0,
            "rating" => t.rating = (t.rating + 1) % 6,
            "color" => t.color = TrackColor::from_id(t.color.map_or(1, |c| c.id() % 8 + 1)),
            "file_size" => t.file_size += 1,
            "artwork" => t.artwork_id = if t.artwork_id.is_some() { None } else { Some(1) },
            _ => unreachable!("{field}"),
        }
    }

    /// Between two exports of one library, tracks are matched by their path however it's written
    /// (ids differ), and the diff lists exactly the tracks added, removed and changed (with each
    /// field that changed, and only those), and the playlists added, removed and reordered or
    /// refilled. Diffing the other way round swaps what's added and removed, and an export
    /// against itself differs in nothing.
    #[hegel::test(test_cases = 200)]
    fn diffs_list_exactly_what_changed(tc: TestCase) {
        let n = tc.draw(gs::integers::<u32>().max_value(6));
        let old_tracks: Vec<Track> =
            (1..=n).map(|i| track(i, "A", &format!("T{i}"), &format!("/Contents/{i}.mp3"))).collect();
        let playlist = |id, name: String, track_ids| Playlist {
            id,
            parent_id: None,
            name,
            is_folder: false,
            sort_order: id,
            track_ids,
        };
        let entries = || tc.draw(gs::vecs(gs::integers::<u32>().min_value(1).max_value(n.max(1))).max_size(4));
        let lists = tc.draw(gs::integers::<u32>().max_value(3));
        let old_lists: Vec<Playlist> = (1..=lists).map(|j| playlist(j, format!("P{j}"), entries())).collect();
        let old = Library { tracks: old_tracks.clone(), playlists: old_lists.clone(), ..Default::default() };

        let mut new_tracks = Vec::new();
        let (mut removed, mut changed) = (Vec::new(), Vec::new());
        for t in &old_tracks {
            if tc.draw(gs::weighted_booleans(0.25)) {
                removed.push(t.file_path.clone());
                continue;
            }
            let mut t = t.clone();
            t.id += 100;
            // Fields the diff doesn't compare.
            t.year += 1;
            t.play_count += 1;
            t.file_path = tc.draw(gs::sampled_from(vec![
                t.file_path.clone(),
                t.file_path.replace('/', "\\"),
                t.file_path.trim_start_matches('/').to_string(),
            ]));
            let fields: Vec<&str> = DIFFED_FIELDS.into_iter().filter(|_| tc.draw(gs::weighted_booleans(0.2))).collect();
            fields.iter().for_each(|f| change(&mut t, f));
            if !fields.is_empty() {
                changed.push((t.id, fields));
            }
            new_tracks.push(t);
        }
        let new_count = tc.draw(gs::integers::<u32>().max_value(2));
        let added: Vec<String> = (0..new_count).map(|k| format!("/Contents/new{k}.mp3")).collect();
        new_tracks.extend(added.iter().enumerate().map(|(k, p)| track(200 + k as u32, "B", "New", p)));

        // Old ids to new: entries of removed tracks drop out.
        let renumbered = |ids: &[u32]| -> Vec<u32> {
            ids.iter().filter(|id| !removed.contains(&format!("/Contents/{id}.mp3"))).map(|id| id + 100).collect()
        };
        let (mut new_lists, mut removed_lists, mut changed_lists) = (Vec::new(), Vec::new(), Vec::new());
        for p in &old_lists {
            if tc.draw(gs::weighted_booleans(0.2)) {
                removed_lists.push(p.name.clone());
                continue;
            }
            let mut ids = renumbered(&p.track_ids);
            if tc.draw(gs::booleans()) {
                ids.reverse();
            }
            if ids.iter().map(|id| id - 100).collect::<Vec<_>>() != p.track_ids {
                changed_lists.push(p.name.clone());
            }
            new_lists.push(Playlist { id: p.id + 10, track_ids: ids, ..p.clone() });
        }
        let new_list_count = tc.draw(gs::integers::<u32>().max_value(2));
        let added_lists: Vec<String> = (0..new_list_count).map(|k| format!("N{k}")).collect();
        new_lists.extend(added_lists.iter().enumerate().map(|(k, name)| playlist(20 + k as u32, name.clone(), vec![])));
        let new = Library { tracks: new_tracks, playlists: new_lists, ..Default::default() };

        let d = LibraryDiff::between(&old, &new);
        let paths = |refs: &[TrackRef]| refs.iter().map(|r| crate::paths::key(&r.file_path)).collect::<Vec<_>>();
        let keys = |v: &[String]| v.iter().map(|p| crate::paths::key(p)).collect::<Vec<_>>();
        assert_eq!(paths(&d.added_tracks), keys(&added));
        assert_eq!(paths(&d.removed_tracks), keys(&removed));
        let got: Vec<(u32, Vec<&str>)> = d
            .changed_tracks
            .iter()
            .map(|c| (c.track.id, c.fields.iter().map(|f| f.field.as_str()).collect()))
            .collect();
        assert_eq!(got, changed);
        assert!(d.changed_tracks.iter().flat_map(|c| &c.fields).all(|f| f.old != f.new));
        let sorted = |mut v: Vec<String>| {
            v.sort();
            v
        };
        assert_eq!(sorted(d.added_playlists.clone()), sorted(added_lists));
        assert_eq!(sorted(d.removed_playlists.clone()), sorted(removed_lists));
        assert_eq!(sorted(d.changed_playlists.clone()), sorted(changed_lists));
        let nothing = d.added_tracks.is_empty() && d.removed_tracks.is_empty() && got.is_empty();
        let no_lists = d.added_playlists.is_empty() && d.removed_playlists.is_empty() && d.changed_playlists.is_empty();
        assert_eq!(d.is_empty(), nothing && no_lists);

        let back = LibraryDiff::between(&new, &old);
        assert_eq!(paths(&back.added_tracks), paths(&d.removed_tracks));
        assert_eq!(paths(&back.removed_tracks), paths(&d.added_tracks));
        assert_eq!(back.changed_tracks.len(), d.changed_tracks.len());
        assert_eq!(sorted(back.added_playlists), sorted(d.removed_playlists));
        assert!(LibraryDiff::between(&old, &old).is_empty() && LibraryDiff::between(&new, &new).is_empty());
    }

    /// Untangling keeps the first row of each id, as it was but for a folder it may drop; walking
    /// up from any row then ends within MAX_DEPTH rows without meeting one twice, walking down
    /// from the top meets each row once at most, and a tree that was already sound (or already
    /// untangled) is left alone.
    #[hegel::test(test_cases = 300)]
    fn untangled_trees_are_walkable(tc: TestCase) {
        let rows = rows(&tc, 12);
        let mut first: Vec<Playlist> = Vec::new();
        for r in &rows {
            if !first.iter().any(|f| f.id == r.id) {
                first.push(r.clone());
            }
        }
        let out = untangle(rows);
        assert_eq!(out.len(), first.len());
        for (o, f) in out.iter().zip(&first) {
            assert_eq!(*o, Playlist { parent_id: o.parent_id, ..f.clone() });
            assert!(o.parent_id == f.parent_id || o.parent_id.is_none(), "{o:?} from {f:?}");
        }
        for p in &out {
            let (len, looped) = ancestors(&out, p);
            assert!(!looped && len <= MAX_DEPTH, "{} sits {len} deep (loop: {looped})", p.id);
        }
        let lib = Library { playlists: out.clone(), ..Default::default() };
        let mut met = Vec::new();
        let mut level = vec![None];
        for _ in 0..=MAX_DEPTH {
            level = level.into_iter().flat_map(|parent| lib.children(parent)).map(|p| Some(p.id)).collect();
            met.extend(level.iter().flatten().copied());
        }
        assert!(level.is_empty(), "rows below MAX_DEPTH");
        let mut unique = met.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(unique.len(), met.len(), "a row met twice");

        let sound = first.iter().all(|p| matches!(ancestors(&first, p), (len, false) if len <= MAX_DEPTH));
        if sound {
            assert_eq!(out, first);
        }
        assert_eq!(untangle(out.clone()), out);
    }

    /// In an untangled tree, display order lists each playlist once, after its folder, with the
    /// path `playlist_path` gives and the names `playlist_names` gives; keys tell every node
    /// apart. Each node is found by its path among its kind, the first shown with that path
    /// winning; without a kind, playlists win, unless the path ends in `/`. A query's blank names
    /// are left out of it, so a path with one finds what the path without it does. Whatever any
    /// query finds matches it by path or name, at most ignoring ASCII case.
    #[hegel::test(test_cases = 300)]
    fn playlists_are_found_by_the_paths_they_are_shown_with(tc: TestCase) {
        let lib = Library { playlists: untangle(rows(&tc, 10)), ..Default::default() };
        let shown = lib.display_order();
        assert_eq!(shown.len(), lib.playlists.len());
        let at = |id: u32| shown.iter().position(|(_, p)| p.id == id).unwrap();
        for (i, (path, p)) in shown.iter().enumerate() {
            assert_eq!(*path, lib.playlist_path(p.id));
            assert_eq!(lib.playlist_names(p.id).join("/"), *path);
            if let Some(parent) = p.parent_id.filter(|&id| lib.playlist(id).is_some()) {
                assert!(at(parent) < i, "{} shown before its folder", p.id);
            }
        }
        // Walking the folders down from the top, as a tree view does, meets every node in display
        // order: one whose folder isn't there among the top-level nodes.
        let mut walked = Vec::new();
        let mut stack: Vec<&Playlist> = lib.children(None).into_iter().rev().collect();
        while let Some(p) = stack.pop() {
            walked.push(p.id);
            stack.extend(lib.children(Some(p.id)).into_iter().rev());
        }
        assert_eq!(walked, shown.iter().map(|(_, p)| p.id).collect::<Vec<_>>());
        let keyed: Vec<_> = lib.keyed_playlists().collect();
        let mut keys: Vec<_> = keyed.iter().map(|(k, ..)| k.clone()).collect();
        keys.sort_unstable();
        keys.dedup();
        assert_eq!(keys.len(), lib.playlists.len(), "two nodes share a key");
        for (_, path, p) in &keyed {
            let slash = if p.is_folder { "/" } else { "" };
            assert_eq!(*path, format!("{}{slash}", lib.playlist_path(p.id)));
        }

        let first = |path: &str, folder: bool| {
            shown.iter().find(|(s, p)| s == path && p.is_folder == folder).map(|(_, p)| p.id)
        };
        let without_blanks = |q: &str| q.split('/').filter(|n| !n.trim().is_empty()).collect::<Vec<_>>().join("/");
        let found = |q: &str| lib.find_playlist(q).map(|f| f.id);
        // A few nodes, not all: each search walks the whole tree, which can be a long chain.
        let picks = tc.draw(gs::vecs(gs::integers::<usize>().max_value(shown.len().max(1) - 1)).max_size(4));
        for (path, p) in picks.iter().filter_map(|&i| shown.get(i)) {
            // Read as it's written without them: `A/ ` as `A/`, the folder.
            let slash = if path.trim_end().ends_with('/') { "/" } else { "" };
            let read = without_blanks(path) + slash;
            if read != *path {
                assert_eq!(found(path), found(&read), "{path:?}");
                assert_eq!(found(&format!("{path}/")), found(&format!("{read}/")), "{path:?}");
                continue;
            }
            assert_eq!(lib.find_playlist_of_kind(path, p.is_folder).map(|f| f.id), first(path, p.is_folder));
            assert_eq!(lib.find_playlist(path).map(|f| f.id), first(path, false).or(first(path, true)));
            let as_folder = lib.find_playlist(&format!("{path}/")).map(|f| f.id);
            assert_eq!(as_folder, first(path, true).or(first(path, false)));
            // A name no path matches finds the one node that has it.
            let path_like = shown.iter().any(|(s, _)| s.eq_ignore_ascii_case(&p.name));
            let named = lib.playlists.iter().filter(|o| o.name == p.name).count();
            if !path_like && named == 1 {
                assert_eq!(lib.find_playlist(&p.name).map(|f| f.id), Some(p.id));
            }
        }
        let query = tc.draw(gs::text().alphabet("aAb /").max_size(6));
        let q = without_blanks(&query);
        if let Some(p) = lib.find_playlist(&query) {
            let path = lib.playlist_path(p.id);
            assert!(!q.is_empty(), "{query:?} has no names but found {p:?}");
            assert!(path.eq_ignore_ascii_case(&q) || p.name.eq_ignore_ascii_case(&q), "{query:?}: {p:?}");
        }
    }

    /// `pick` takes the first item whose path is the query exactly; failing that, the first that
    /// matches ignoring ASCII case, but only when every such match has the same path.
    #[hegel::test(test_cases = 300)]
    fn pick_prefers_exact_paths_then_one_case_insensitive_path(tc: TestCase) {
        let paths = tc.draw(gs::vecs(gs::text().alphabet("aAb/").max_size(3)).max_size(6));
        let query = tc.draw(gs::text().alphabet("aAb/").max_size(3));
        let picked = pick(paths.iter().enumerate().map(|(i, p)| (p.as_str(), i)), &query);
        let exact = paths.iter().position(|p| *p == query);
        let loose: Vec<usize> = (0..paths.len()).filter(|&i| paths[i].eq_ignore_ascii_case(&query)).collect();
        let one_path = loose.iter().all(|&i| paths[i] == paths[loose[0]]);
        let expected = exact.or_else(|| loose.first().copied().filter(|_| one_path));
        assert_eq!(picked, expected, "{query:?} in {paths:?}");
    }

    /// Searching by `#id` or `id` finds the first track with that id; otherwise every track found
    /// has the query (normalised) in its "artist - title" or path, and any piece of a track's
    /// normalised name finds it.
    #[hegel::test(test_cases = 300)]
    fn search_finds_by_id_or_by_any_piece_of_the_name(tc: TestCase) {
        let count = tc.draw(gs::integers::<usize>().max_value(5));
        let tracks: Vec<Track> = (0..count)
            .map(|i| {
                let id = tc.draw(gs::integers::<u32>().max_value(6));
                let word = || tc.draw(gs::text().alphabet("aB 1#\t").max_size(5));
                track(id, &word(), &word(), &format!("/Contents/{i}.mp3"))
            })
            .collect();
        let lib = Library { tracks, ..Default::default() };
        let by_id = |q: &str| q.trim().trim_start_matches('#').parse::<u32>().ok().and_then(|id| lib.track(id));
        let query = tc.draw(gs::text().alphabet("aBb 1#\t").max_size(4));
        let found = lib.search(&query);
        match by_id(&query) {
            Some(t) => assert_eq!(found, [t]),
            None => {
                let needle = normalize(query.trim());
                for t in &found {
                    let in_name = normalize(&t.display_name()).contains(&needle);
                    assert!(in_name || normalize(&t.file_path).contains(&needle), "{query:?} found {t:?}");
                }
            }
        }
        if let Some(t) = lib.tracks.first() {
            let name: Vec<char> = normalize(&t.display_name()).chars().collect();
            let from = tc.draw(gs::integers::<usize>().max_value(name.len()));
            let to = tc.draw(gs::integers::<usize>().min_value(from).max_value(name.len()));
            let piece: String = name[from..to].iter().collect();
            let found = lib.search(&piece);
            match by_id(&piece) {
                Some(other) => assert_eq!(found, [other]),
                None => assert!(found.iter().any(|f| std::ptr::eq(*f, t)), "{piece:?} misses {t:?}"),
            }
        }
    }

    /// Normalising is idempotent, ignores ASCII case and how words are spaced, and leaves no
    /// blanks at the ends or doubled; so tracks whose artist and title differ only so are taken
    /// for one recording, while a different length tells them apart.
    #[hegel::test(test_cases = 300)]
    fn normalised_names_ignore_case_and_spacing(tc: TestCase) {
        let s = tc.draw(hegel::one_of!(
            gs::text().max_size(10),
            gs::text().alphabet("aBc \t\u{a0}\u{3000}\u{1c4}").max_size(10)
        ));
        let n = normalize(&s);
        assert_eq!(normalize(&n), n, "{s:?}");
        assert_eq!(normalize(&s.to_ascii_uppercase()), n, "{s:?}");
        assert!(!n.starts_with(' ') && !n.ends_with(' ') && !n.contains("  "), "{n:?}");
        let spaced: String = s.split_whitespace().collect::<Vec<_>>().join(" \t ");
        assert_eq!(normalize(&format!("  {spaced} ")), n, "{s:?}");

        let a = track(1, &s, &s, "/Contents/a.mp3");
        let mut b = track(2, &format!(" {}", s.to_ascii_uppercase()), &spaced, "/Contents/b.mp3");
        assert_eq!(a.fingerprint_key(), b.fingerprint_key());
        b.duration_secs += 1;
        assert_ne!(a.fingerprint_key(), b.fingerprint_key());
    }

    /// Looking tracks up by a list of ids is `track` for each, repeats kept and unknown ids left
    /// out; the summary counts the tracks, playlists, folders, artists, bytes, seconds and missing
    /// artwork the library holds; names read "artist - title".
    #[hegel::test(test_cases = 200)]
    fn lookups_and_summaries_agree_with_the_library(tc: TestCase) {
        let count = tc.draw(gs::integers::<usize>().max_value(6));
        let tracks: Vec<Track> = (0..count)
            .map(|_| {
                let id = tc.draw(gs::integers::<u32>().max_value(5));
                let mut t = any_track(&tc, id);
                t.artist = tc.draw(gs::sampled_from(vec!["A", "B", "a", ""])).into();
                t
            })
            .collect();
        let lib = Library { tracks, playlists: rows(&tc, 5), ..Default::default() };
        let ids = tc.draw(gs::vecs(gs::integers::<u32>().max_value(7)).max_size(8));
        let looked_up: Vec<*const Track> = lib.tracks_by_id(&ids).into_iter().map(|t| t as *const Track).collect();
        let one_by_one: Vec<*const Track> =
            ids.iter().filter_map(|&id| lib.track(id)).map(|t| t as *const Track).collect();
        assert_eq!(looked_up, one_by_one);

        let s = lib.summary();
        assert_eq!(s.tracks, lib.tracks.len());
        assert_eq!(s.playlists + s.folders, lib.playlists.len());
        assert_eq!(s.folders, lib.playlists.iter().filter(|p| p.is_folder).count());
        let mut artists: Vec<&str> = lib.tracks.iter().map(|t| t.artist.as_str()).collect();
        artists.sort_unstable();
        artists.dedup();
        assert_eq!(s.artists, artists.len());
        assert_eq!(s.audio_bytes, lib.tracks.iter().map(|t| t.file_size).sum::<u64>());
        assert_eq!(s.total_duration_secs, lib.tracks.iter().map(|t| u64::from(t.duration_secs)).sum::<u64>());
        assert_eq!(s.tracks_without_artwork, lib.tracks.iter().filter(|t| t.artwork_id.is_none()).count());
        for t in &lib.tracks {
            let name = t.display_name();
            let expected = if t.artist.is_empty() { t.title.clone() } else { format!("{} - {}", t.artist, t.title) };
            assert_eq!(name, expected);
            let r = TrackRef::from(t);
            assert_eq!((r.id, r.name, r.file_path), (t.id, name, t.file_path.clone()));
        }
    }
}
