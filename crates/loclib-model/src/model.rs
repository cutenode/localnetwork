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
mod tests;
