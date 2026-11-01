//! Browsing a library: the sidebar tree (the playlists, plus CDJ-style categories under Browse),
//! the track list's sort order, and which of its two modes the search box is in.

use locallibrary::fields::TrackValues;
use locallibrary::harmony::{self, Camelot};
use locallibrary::model::{TrackColor, normalize};
use locallibrary::query::{FieldName, SortField, SortKey, compare_values};
use locallibrary::sets::Set;
use locallibrary::{Library, Playlist, Track, paths};
use rustc_hash::{FxHashMap, FxHashSet};
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;

/// The ways to browse a drive besides its playlists, as on a CDJ's top menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    Artist,
    Album,
    Genre,
    Key,
    /// In bands of 5 BPM.
    Bpm,
    Rating,
    Colour,
    /// By month.
    Added,
    /// rekordbox's My Tags, by `Category/Tag`.
    MyTags,
    /// The sets kept in the store, with their tracks that are on the drive.
    Sets,
    /// The lists players recorded, shown as sessions. Saved as `history`, the name they had
    /// before, so a list remembered then still opens.
    History,
    /// Tracks whose audio file is missing, and copies of the same recording.
    Problems,
}

impl Category {
    pub const ALL: [Category; 12] = [
        Category::Artist,
        Category::Album,
        Category::Genre,
        Category::Key,
        Category::Bpm,
        Category::Rating,
        Category::Colour,
        Category::Added,
        Category::MyTags,
        Category::Sets,
        Category::History,
        Category::Problems,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Category::Artist => "Artist",
            Category::Album => "Album",
            Category::Genre => "Genre",
            Category::Key => "Key",
            Category::Bpm => "BPM",
            Category::Rating => "Rating",
            Category::Colour => "Colour",
            Category::Added => "Date added",
            Category::MyTags => "My Tags",
            Category::Sets => "Sets",
            Category::History => "Sessions",
            Category::Problems => "Problems",
        }
    }
}

/// A row of the sidebar.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum NodeId {
    /// Every track on the drive.
    All,
    /// A playlist or folder.
    Playlist(u32),
    /// The folder holding the categories.
    Browse,
    Category(Category),
    /// One value of a category (an artist, a key, a BPM band, a set...), by its key.
    Group(Category, String),
}

pub struct Node {
    pub id: NodeId,
    pub name: String,
    pub depth: usize,
    pub folder: bool,
    pub expanded: bool,
    /// Tracks in a list, children of a folder.
    pub count: usize,
}

/// A node as remembered between reloads and runs: playlists by path, since ids change when a
/// drive is edited or exported again.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SavedNode {
    All,
    Browse,
    Playlist { path: String },
    Category { category: Category },
    Group { category: Category, key: String },
}

/// One value of a category and its tracks (indexes into the library), in library order (play
/// order for sets and history lists).
#[derive(Debug, Clone, PartialEq)]
pub struct Group {
    pub key: String,
    pub label: String,
    pub tracks: Vec<usize>,
    /// Entries whose track isn't on the drive (a set's tracks played from another), left out of
    /// `tracks`.
    pub left_out: usize,
}

/// What's wrong with tracks, by library index.
#[derive(Default)]
pub struct Problems {
    /// Audio files that aren't on the drive; `None` until the drive has been checked.
    pub missing: Option<Vec<bool>>,
    /// For each track, its set of copies (same artist, title and duration) in `copies`.
    pub copy_of: Vec<Option<u32>>,
    pub copies: Vec<Vec<usize>>,
}

impl Problems {
    pub fn is_missing(&self, i: usize) -> bool {
        self.missing.as_ref().is_some_and(|m| m.get(i).copied().unwrap_or(false))
    }

    /// The other copies of track `i` on the drive.
    pub fn others(&self, i: usize) -> impl Iterator<Item = usize> + '_ {
        let set = self.copy_of.get(i).copied().flatten().map_or(&[][..], |g| &self.copies[g as usize][..]);
        set.iter().copied().filter(move |&j| j != i)
    }

    pub fn has_copies(&self, i: usize) -> bool {
        self.copy_of.get(i).is_some_and(Option::is_some)
    }
}

/// Tracks that are the same recording (rekordbox's fingerprint: artist, title and duration).
/// Untitled tracks without an artist aren't compared.
pub fn duplicates(lib: &Library) -> (Vec<Option<u32>>, Vec<Vec<usize>>) {
    let mut by_key: FxHashMap<String, Vec<usize>> = FxHashMap::default();
    for (i, t) in lib.tracks.iter().enumerate() {
        if !(t.artist.trim().is_empty() && t.title.trim().is_empty()) {
            by_key.entry(t.fingerprint_key()).or_default().push(i);
        }
    }
    let mut copies: Vec<Vec<usize>> = by_key.into_values().filter(|v| v.len() > 1).collect();
    copies.sort();
    let mut copy_of = vec![None; lib.tracks.len()];
    for (g, set) in copies.iter().enumerate() {
        for &i in set {
            copy_of[i] = Some(g as u32);
        }
    }
    (copy_of, copies)
}

pub fn colour_name(c: TrackColor) -> &'static str {
    match c {
        TrackColor::Pink => "pink",
        TrackColor::Red => "red",
        TrackColor::Orange => "orange",
        TrackColor::Yellow => "yellow",
        TrackColor::Green => "green",
        TrackColor::Aqua => "aqua",
        TrackColor::Blue => "blue",
        TrackColor::Purple => "purple",
    }
}

pub const COLOURS: [TrackColor; 8] = [
    TrackColor::Pink,
    TrackColor::Red,
    TrackColor::Orange,
    TrackColor::Yellow,
    TrackColor::Green,
    TrackColor::Aqua,
    TrackColor::Blue,
    TrackColor::Purple,
];

fn capitalised(s: &str) -> String {
    let mut c = s.chars();
    c.next().map(|f| f.to_uppercase().chain(c).collect()).unwrap_or_default()
}

/// Tracks grouped by `f`'s (key, label); tracks it gives nothing for go in a last group named
/// `none`. Groups come out in first-seen order.
fn collect(lib: &Library, none: &str, f: impl Fn(&Track) -> Option<(String, String)>) -> (Vec<Group>, Option<Group>) {
    let mut index: FxHashMap<String, usize> = FxHashMap::default();
    let mut out: Vec<Group> = Vec::new();
    let mut rest = Group { key: String::new(), label: none.to_string(), tracks: Vec::new(), left_out: 0 };
    for (i, t) in lib.tracks.iter().enumerate() {
        match f(t) {
            Some((key, label)) => {
                let g = *index.entry(key.clone()).or_insert_with(|| {
                    out.push(Group { key, label, tracks: Vec::new(), left_out: 0 });
                    out.len() - 1
                });
                out[g].tracks.push(i);
            }
            None => rest.tracks.push(i),
        }
    }
    (out, (!rest.tracks.is_empty()).then_some(rest))
}

/// The values of `cat` on the drive with their tracks, in display order. `by_id` maps track ids
/// to library indexes (for history lists); `tags` is each track's My Tags by its file key in lower
/// case (none until they're read); `sets` are the store's (none until they're read).
pub fn groups(
    lib: &Library,
    cat: Category,
    by_id: &FxHashMap<u32, usize>,
    problems: &Problems,
    tags: Option<&FxHashMap<String, Vec<String>>>,
    sets: &[Set],
) -> Vec<Group> {
    let text = |get: fn(&Track) -> &str, none: &str| {
        let (mut v, rest) = collect(lib, none, |t| {
            let raw = get(t).trim();
            (!raw.is_empty()).then(|| (normalize(raw), raw.to_string()))
        });
        v.sort_by(|a, b| a.key.cmp(&b.key));
        v.extend(rest);
        v
    };
    match cat {
        Category::Artist => text(|t| &t.artist, "Unknown artist"),
        Category::Album => text(|t| &t.album, "No album"),
        Category::Genre => text(|t| &t.genre, "No genre"),
        Category::Key => {
            let (mut v, rest) = collect(lib, "No key", |t| {
                Camelot::parse(&t.key).map(|k| (k.code(), format!("{:<3} {}", k.code(), k.note())))
            });
            v.sort_by_key(|g| harmony::key_order(&g.key));
            v.extend(rest);
            v
        }
        Category::Bpm => {
            let (mut v, rest) = collect(lib, "No BPM", |t| {
                (t.bpm > 0.0).then(|| {
                    let lo = (t.bpm / 5.0).floor() as u32 * 5;
                    (lo.to_string(), format!("{lo}–{}", lo + 4))
                })
            });
            v.sort_by_key(|g| g.key.parse::<u32>().unwrap_or(0));
            v.extend(rest);
            v
        }
        Category::Rating => {
            let (mut v, rest) =
                collect(lib, "Unrated", |t| (t.rating > 0).then(|| (t.rating.min(5).to_string(), stars(t.rating))));
            v.sort_by(|a, b| b.key.cmp(&a.key));
            v.extend(rest);
            v
        }
        Category::Colour => {
            let (mut v, rest) =
                collect(lib, "No colour", |t| t.color.map(|c| (c.id().to_string(), capitalised(colour_name(c)))));
            v.sort_by_key(|g| g.key.parse::<u8>().unwrap_or(0));
            v.extend(rest);
            v
        }
        Category::Added => {
            let (mut v, rest) = collect(lib, "Unknown", |t| {
                let month = t.date_added.get(..7)?;
                let ok = month.as_bytes()[4] == b'-' && month.chars().filter(char::is_ascii_digit).count() == 6;
                ok.then(|| (month.to_string(), month.to_string()))
            });
            v.sort_by(|a, b| b.key.cmp(&a.key));
            v.extend(rest);
            v
        }
        Category::MyTags => {
            let Some(tags) = tags else { return Vec::new() };
            let mut by_tag: indexmap::IndexMap<&str, Vec<usize>> = Default::default();
            for (i, t) in lib.tracks.iter().enumerate() {
                for tag in tags.get(&paths::file_key(&t.file_path)).into_iter().flatten() {
                    by_tag.entry(tag).or_default().push(i);
                }
            }
            by_tag.sort_unstable_keys();
            by_tag
                .into_iter()
                .map(|(tag, tracks)| Group {
                    key: tag.to_string(),
                    label: tag.replace('/', " › "),
                    tracks,
                    left_out: 0,
                })
                .collect()
        }
        Category::Sets => {
            if sets.is_empty() {
                return Vec::new(); // no store, or no sets in it: no need to key every track
            }
            // Tracks by their file as sessions key them; from the back, so the first of a file
            // that's in the library twice wins.
            let by_file: FxHashMap<String, usize> = lib
                .tracks
                .iter()
                .enumerate()
                .rev()
                .map(|(i, t)| (locallibrary::paths::file_key(&t.file_path), i))
                .collect();
            let mut newest: Vec<&Set> = sets.iter().collect();
            newest.sort_by(|a, b| Set::newest_first(a, b));
            newest
                .into_iter()
                .map(|s| {
                    let tracks: Vec<usize> =
                        s.tracks.iter().filter_map(|e| by_file.get(&e.file_key()?).copied()).collect();
                    let label = set_label(s);
                    Group { key: s.id.clone(), label, left_out: s.tracks.len() - tracks.len(), tracks }
                })
                .collect()
        }
        Category::History => lib
            .history
            .iter()
            .filter(|h| !h.is_folder)
            .map(|h| Group {
                key: h.id.to_string(),
                label: h.name.clone(),
                tracks: h.track_ids.iter().filter_map(|id| by_id.get(id).copied()).collect(),
                left_out: 0,
            })
            .collect(),
        Category::Problems => {
            let mut v = Vec::new();
            if let Some(missing) = &problems.missing {
                let tracks: Vec<usize> = (0..missing.len()).filter(|&i| missing[i]).collect();
                if !tracks.is_empty() {
                    v.push(Group { key: "missing".into(), label: "Missing files".into(), tracks, left_out: 0 });
                }
            }
            let tracks: Vec<usize> = (0..lib.tracks.len()).filter(|&i| problems.has_copies(i)).collect();
            if !tracks.is_empty() {
                v.push(Group { key: "duplicates".into(), label: "Duplicates".into(), tracks, left_out: 0 });
            }
            v
        }
    }
}

/// How Browse › Sets names a set: its day, and its name when it has one.
pub fn set_label(s: &Set) -> String {
    match s.name() {
        Some(name) => format!("{} {name}", s.date),
        None => s.date.to_string(),
    }
}

pub fn stars(r: u8) -> String {
    (0..5).map(|i| if i < r { '★' } else { '·' }).collect()
}

/// The sidebar: All tracks, the Browse folder with its categories (and their values when
/// expanded), then the playlist tree. `groups` holds every category's groups once Browse is
/// open; categories with nothing in them are left out.
pub fn tree(lib: &Library, expanded: &FxHashSet<NodeId>, groups: &FxHashMap<Category, Vec<Group>>) -> Vec<Node> {
    let node = |id, name: &str, depth, folder, count| {
        let expanded = folder && expanded.contains(&id);
        Node { id, name: name.to_string(), depth, folder, expanded, count }
    };
    let mut out = vec![node(NodeId::All, "All tracks", 0, false, lib.tracks.len())];
    let browse = node(NodeId::Browse, "Browse", 0, true, Category::ALL.len());
    let open = browse.expanded;
    out.push(browse);
    if open {
        for cat in Category::ALL {
            let Some(gs) = groups.get(&cat).filter(|g| !g.is_empty()) else { continue };
            let c = node(NodeId::Category(cat), cat.name(), 1, true, gs.len());
            let open = c.expanded;
            out.push(c);
            if open {
                for g in gs {
                    out.push(node(NodeId::Group(cat, g.key.clone()), &g.label, 2, false, g.tracks.len()));
                }
            }
        }
    }
    // Each folder's children, in display order (as `Library::children` gives them), found in one
    // pass: asking the library for each folder's scans every playlist each time. One whose folder
    // isn't there is at the top level.
    let ids: FxHashSet<u32> = lib.playlists.iter().map(|p| p.id).collect();
    let mut children: FxHashMap<Option<u32>, Vec<&Playlist>> = FxHashMap::default();
    for p in &lib.playlists {
        children.entry(p.parent_id.filter(|id| ids.contains(id))).or_default().push(p);
    }
    for v in children.values_mut() {
        v.sort_by_key(|p| (p.sort_order, p.id));
    }
    type Children<'a> = FxHashMap<Option<u32>, Vec<&'a Playlist>>;
    fn walk(kids: &Children, parent: Option<u32>, depth: usize, expanded: &FxHashSet<NodeId>, out: &mut Vec<Node>) {
        if depth > 32 {
            return; // cyclic tree in a corrupt database
        }
        for p in kids.get(&parent).into_iter().flatten() {
            let open = p.is_folder && expanded.contains(&NodeId::Playlist(p.id));
            let count = if p.is_folder { kids.get(&Some(p.id)).map_or(0, Vec::len) } else { p.track_ids.len() };
            out.push(Node {
                id: NodeId::Playlist(p.id),
                name: p.name.clone(),
                depth,
                folder: p.is_folder,
                expanded: open,
                count,
            });
            if open {
                walk(kids, Some(p.id), depth + 1, expanded, out);
            }
        }
    }
    walk(&children, None, 0, expanded, &mut out);
    out
}

/// Every playlist and folder's path (`Sets/Friday`) by id, as `Library::playlist_path` gives
/// it, worked out with one map rather than a scan of every playlist for each folder above each.
pub fn playlist_paths(lib: &Library) -> FxHashMap<u32, String> {
    // Collected from the back, so the first of a repeated id wins, as in `Library::playlist`.
    let by_id: FxHashMap<u32, &Playlist> = lib.playlists.iter().rev().map(|p| (p.id, p)).collect();
    let mut out = FxHashMap::default();
    for p in &lib.playlists {
        let mut names = Vec::new();
        let mut cur = by_id.get(&p.id).copied();
        while let Some(q) = cur {
            names.push(q.name.as_str());
            cur = q.parent_id.and_then(|id| by_id.get(&id).copied());
            if names.len() > 64 {
                break; // cyclic tree in a corrupt database
            }
        }
        names.reverse();
        out.entry(p.id).or_insert_with(|| names.join("/"));
    }
    out
}

// ---------------------------------------------------------------------------------------------
// Sorting
// ---------------------------------------------------------------------------------------------

/// The columns the track list sorts by, in the order the sort key cycles through them.
pub const SORT_FIELDS: [SortField; 10] = [
    SortField::Title,
    SortField::Artist,
    SortField::Album,
    SortField::Genre,
    SortField::Bpm,
    SortField::Key,
    SortField::Rating,
    SortField::Time,
    SortField::Added,
    SortField::Plays,
];

pub fn sort_label(f: SortField) -> &'static str {
    match f {
        SortField::Title => "Title",
        SortField::Artist => "Artist",
        SortField::Album => "Album",
        SortField::Genre => "Genre",
        SortField::Label => "Label",
        SortField::Key => "Key",
        SortField::Bpm => "BPM",
        SortField::Rating => "Rating",
        SortField::Color => "Colour",
        SortField::Year => "Year",
        SortField::Plays => "Plays",
        SortField::Added => "Added",
        SortField::Time => "Time",
        SortField::Bitrate => "Bitrate",
        SortField::Path => "Path",
        SortField::Field(name) => name.as_str(),
    }
}

/// The sort as `locallibrary::query::parse_sort` reads it (`-bpm`), for sorting a playlist on
/// the drive the same way.
pub fn sort_spec(k: SortKey) -> String {
    let name = match k.field {
        SortField::Title => "title",
        SortField::Artist => "artist",
        SortField::Album => "album",
        SortField::Genre => "genre",
        SortField::Label => "label",
        SortField::Key => "key",
        SortField::Bpm => "bpm",
        SortField::Rating => "rating",
        SortField::Color => "color",
        SortField::Year => "year",
        SortField::Plays => "plays",
        SortField::Added => "added",
        SortField::Time => "time",
        SortField::Bitrate => "bitrate",
        SortField::Path => "path",
        SortField::Field(name) => name.as_str(),
    };
    format!("{}{name}", if k.descending { "-" } else { "" })
}

/// Ratings, dates added and play counts read best highest first.
fn first_descending(f: SortField) -> bool {
    matches!(f, SortField::Rating | SortField::Added | SortField::Plays)
}

/// The next sort in the cycle: each column in turn, then the list's own order.
pub fn cycle(cur: Option<SortKey>) -> Option<SortKey> {
    let next = match cur {
        None => Some(SORT_FIELDS[0]),
        Some(k) => SORT_FIELDS.iter().position(|f| *f == k.field).and_then(|i| SORT_FIELDS.get(i + 1)).copied(),
    };
    next.map(|field| SortKey { field, descending: first_descending(field) })
}

/// Clicking a column header: sort by it, then the other way round, then back to the list's own
/// order.
pub fn click(cur: Option<SortKey>, field: SortField) -> Option<SortKey> {
    match cur {
        Some(k) if k.field == field && k.descending == first_descending(field) => {
            Some(SortKey { field, descending: !k.descending })
        }
        Some(k) if k.field == field => None,
        _ => Some(SortKey { field, descending: first_descending(field) }),
    }
}

/// Every track's rank under `field` (equal tracks share a rank), for sorting lists by integer
/// compares. Orders as `locallibrary::query::sort_tracks` does, so a playlist sorted on the drive
/// comes out as the list showed it.
pub fn ranks(lib: &Library, field: SortField) -> Vec<u32> {
    let t = &lib.tracks;
    let text = |get: fn(&Track) -> &str| t.iter().map(|x| normalize(get(x))).collect::<Vec<_>>();
    let by_text = |keys: Vec<String>| rank_by(t.len(), |a, b| keys[a].cmp(&keys[b]));
    match field {
        SortField::Title => by_text(text(|x| &x.title)),
        SortField::Artist => by_text(text(|x| &x.artist)),
        SortField::Album => by_text(text(|x| &x.album)),
        SortField::Genre => by_text(text(|x| &x.genre)),
        SortField::Label => by_text(text(|x| &x.label)),
        SortField::Path => {
            let keys: Vec<String> = t.iter().map(|x| paths::key(&x.file_path)).collect();
            by_text(keys)
        }
        SortField::Key => {
            let keys: Vec<u8> = t.iter().map(|x| harmony::key_order(&x.key)).collect();
            rank_by(t.len(), |a, b| keys[a].cmp(&keys[b]))
        }
        SortField::Bpm => rank_by(t.len(), |a, b| t[a].bpm.total_cmp(&t[b].bpm)),
        SortField::Rating => rank_by(t.len(), |a, b| t[a].rating.cmp(&t[b].rating)),
        SortField::Color => {
            let id = |i: usize| t[i].color.map_or(u8::MAX, TrackColor::id);
            rank_by(t.len(), |a, b| id(a).cmp(&id(b)))
        }
        SortField::Year => rank_by(t.len(), |a, b| t[a].year.cmp(&t[b].year)),
        SortField::Plays => rank_by(t.len(), |a, b| t[a].play_count.cmp(&t[b].play_count)),
        SortField::Added => rank_by(t.len(), |a, b| t[a].date_added.cmp(&t[b].date_added)),
        SortField::Time => rank_by(t.len(), |a, b| t[a].duration_secs.cmp(&t[b].duration_secs)),
        SortField::Bitrate => rank_by(t.len(), |a, b| t[a].bitrate.cmp(&t[b].bitrate)),
        // By its values, once they're read (see `field_ranks`).
        SortField::Field(_) => vec![0; t.len()],
    }
}

/// Every track's rank under a plugin's field `name`, by its `values` as
/// `locallibrary::query::sort_tracks_with` orders them (tracks without one to show share the
/// last).
pub fn field_ranks(lib: &Library, values: &TrackValues, name: FieldName) -> Vec<u32> {
    let value = |i: usize| values.shown(lib.tracks[i].id, name.as_str());
    rank_by(lib.tracks.len(), |a, b| match (value(a), value(b)) {
        (Some(x), Some(y)) => compare_values(x, y),
        (x, y) => x.is_none().cmp(&y.is_none()),
    })
}

fn rank_by(n: usize, cmp: impl Fn(usize, usize) -> Ordering) -> Vec<u32> {
    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by(|&a, &b| cmp(a, b));
    let mut rank = vec![0u32; n];
    let mut r = 0;
    for k in 0..n {
        if k > 0 && cmp(order[k - 1], order[k]).is_ne() {
            r += 1;
        }
        rank[order[k]] = r;
    }
    rank
}

/// Sort a list of library indexes by rank. Stable both ways: equal tracks keep the list's order.
pub fn sort_list(list: &mut [usize], ranks: &[u32], descending: bool) {
    if descending {
        list.sort_by(|a, b| ranks[*b].cmp(&ranks[*a]));
    } else {
        list.sort_by_key(|i| ranks[*i]);
    }
}

/// A track's text as the filter language's plain words search it (`tracks --filter daft`):
/// artist, title, album, genre, label, key, comment, remixer, mix and file path, normalised.
/// Fields are kept apart so a word can't run from one into the next.
pub fn haystack(t: &Track) -> String {
    let fields =
        [&t.artist, &t.title, &t.album, &t.genre, &t.label, &t.key, &t.comment, &t.remixer, &t.mix_name, &t.file_path];
    normalize(&fields.map(String::as_str).join("\u{1f}"))
}

/// What's typed in the search box, read as the filter language reads it (`tracks --filter`),
/// in two parts: words the track's text must hold (or mustn't, `-word`), matched against the
/// [`haystack`]s made once per library, and the rest (`bpm:120-128 key:8A~`) for
/// `query::Filter`. A filter of words alone would have it put every track's text together again
/// on each search.
#[derive(Debug, Default, PartialEq)]
pub struct Search {
    /// Each must be in the text; `a|b` takes either.
    pub words: Vec<Vec<String>>,
    /// None of each may be.
    pub not: Vec<Vec<String>>,
    /// The terms with a field, as typed.
    pub rest: String,
}

impl Search {
    pub fn parse(query: &str) -> Search {
        let mut out = Search::default();
        let mut rest = Vec::new();
        // Split on whitespace outside quotes, as the filter language does; each token is kept
        // as typed (quotes and all) and without its quotes.
        let mut tokens: Vec<(&str, String)> = Vec::new();
        let (mut start, mut cur, mut quoted) = (None, String::new(), false);
        for (i, c) in query.char_indices() {
            if c.is_whitespace() && !quoted {
                if let Some(s) = start.take() {
                    tokens.push((&query[s..i], std::mem::take(&mut cur)));
                }
                continue;
            }
            start.get_or_insert(i);
            match c {
                '"' => quoted = !quoted,
                c => cur.push(c),
            }
        }
        if let Some(s) = start {
            tokens.push((&query[s..], cur));
        }
        // A token that's only quotes is nothing, as in the filter language.
        for (typed, token) in tokens.into_iter().filter(|(_, t)| !t.is_empty()) {
            let alts = |t: &str| t.split('|').map(normalize).filter(|w| !w.is_empty()).collect::<Vec<_>>();
            // `-word` leaves out, but `-5` is a word; anything with a colon is for the filter.
            let negated =
                token.strip_prefix('-').filter(|r| !r.is_empty() && !r.starts_with(|c: char| c.is_ascii_digit()));
            // So is a word with nothing to look for (`|`, as `a|b` is being typed): kept, it would
            // match no track, and negated, leave none out.
            match negated {
                _ if token.contains(':') => rest.push(typed),
                Some(word) => out.not.extend(Some(alts(word)).filter(|a| !a.is_empty())),
                None => out.words.extend(Some(alts(&token)).filter(|a| !a.is_empty())),
            }
        }
        out.rest = rest.join(" ");
        out
    }

    /// Whether a track's [`haystack`] has the words.
    pub fn matches(&self, hay: &str) -> bool {
        self.words.iter().all(|alts| alts.iter().any(|w| hay.contains(w.as_str())))
            && !self.not.iter().any(|alts| alts.iter().any(|w| hay.contains(w.as_str())))
    }

    pub fn has_words(&self) -> bool {
        !(self.words.is_empty() && self.not.is_empty())
    }
}

#[cfg(test)]
mod tests;
