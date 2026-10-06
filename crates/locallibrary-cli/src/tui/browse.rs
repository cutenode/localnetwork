//! Browsing a library: the sidebar tree (the playlists, plus CDJ-style categories under Browse),
//! the track list's sort order, and which of its two modes the search box is in.

use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};

use locallibrary::harmony::{self, Camelot};
use locallibrary::model::{TrackColor, normalize};
use locallibrary::query::{SortField, SortKey};
use locallibrary::{Library, Playlist, Track, paths};
use serde::{Deserialize, Serialize};

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
    /// The lists players recorded.
    History,
    /// Tracks whose audio file is missing, and copies of the same recording.
    Problems,
}

impl Category {
    pub const ALL: [Category; 11] = [
        Category::Artist,
        Category::Album,
        Category::Genre,
        Category::Key,
        Category::Bpm,
        Category::Rating,
        Category::Colour,
        Category::Added,
        Category::MyTags,
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
            Category::History => "History",
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
    /// One value of a category (an artist, a key, a BPM band, a history list...), by its key.
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

/// One value of a category and its tracks (indexes into the library), in library order (list
/// order for history lists).
#[derive(Debug, Clone, PartialEq)]
pub struct Group {
    pub key: String,
    pub label: String,
    pub tracks: Vec<usize>,
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
    let mut by_key: HashMap<String, Vec<usize>> = HashMap::new();
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
    let mut index: HashMap<String, usize> = HashMap::new();
    let mut out: Vec<Group> = Vec::new();
    let mut rest = Group { key: String::new(), label: none.to_string(), tracks: Vec::new() };
    for (i, t) in lib.tracks.iter().enumerate() {
        match f(t) {
            Some((key, label)) => {
                let g = *index.entry(key.clone()).or_insert_with(|| {
                    out.push(Group { key, label, tracks: Vec::new() });
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
/// case (none until they're read).
pub fn groups(
    lib: &Library,
    cat: Category,
    by_id: &HashMap<u32, usize>,
    problems: &Problems,
    tags: Option<&HashMap<String, Vec<String>>>,
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
            let mut by_tag: std::collections::BTreeMap<&str, Vec<usize>> = Default::default();
            for (i, t) in lib.tracks.iter().enumerate() {
                for tag in tags.get(&paths::key(&t.file_path).to_lowercase()).into_iter().flatten() {
                    by_tag.entry(tag).or_default().push(i);
                }
            }
            by_tag
                .into_iter()
                .map(|(tag, tracks)| Group { key: tag.to_string(), label: tag.replace('/', " › "), tracks })
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
            })
            .collect(),
        Category::Problems => {
            let mut v = Vec::new();
            if let Some(missing) = &problems.missing {
                let tracks: Vec<usize> = (0..missing.len()).filter(|&i| missing[i]).collect();
                if !tracks.is_empty() {
                    v.push(Group { key: "missing".into(), label: "Missing files".into(), tracks });
                }
            }
            let tracks: Vec<usize> = (0..lib.tracks.len()).filter(|&i| problems.has_copies(i)).collect();
            if !tracks.is_empty() {
                v.push(Group { key: "duplicates".into(), label: "Duplicates".into(), tracks });
            }
            v
        }
    }
}

pub fn stars(r: u8) -> String {
    (0..5).map(|i| if i < r { '★' } else { '·' }).collect()
}

/// The sidebar: All tracks, the Browse folder with its categories (and their values when
/// expanded), then the playlist tree. `groups` holds every category's groups once Browse is
/// open; categories with nothing in them are left out.
pub fn tree(lib: &Library, expanded: &HashSet<NodeId>, groups: &HashMap<Category, Vec<Group>>) -> Vec<Node> {
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
    // pass: asking the library for each folder's scans every playlist each time.
    let mut children: HashMap<Option<u32>, Vec<&Playlist>> = HashMap::new();
    for p in &lib.playlists {
        children.entry(p.parent_id).or_default().push(p);
    }
    for v in children.values_mut() {
        v.sort_by_key(|p| (p.sort_order, p.id));
    }
    type Children<'a> = HashMap<Option<u32>, Vec<&'a Playlist>>;
    fn walk(kids: &Children, parent: Option<u32>, depth: usize, expanded: &HashSet<NodeId>, out: &mut Vec<Node>) {
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
pub fn playlist_paths(lib: &Library) -> HashMap<u32, String> {
    // Collected from the back, so the first of a repeated id wins, as in `Library::playlist`.
    let by_id: HashMap<u32, &Playlist> = lib.playlists.iter().rev().map(|p| (p.id, p)).collect();
    let mut out = HashMap::new();
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
    }
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
            let alts = |t: &str| t.split('|').map(normalize).filter(|w| !w.is_empty()).collect();
            // `-word` leaves out, but `-5` is a word; anything with a colon is for the filter.
            let negated =
                token.strip_prefix('-').filter(|r| !r.is_empty() && !r.starts_with(|c: char| c.is_ascii_digit()));
            match negated {
                _ if token.contains(':') => rest.push(typed),
                Some(word) => out.not.push(alts(word)),
                None => out.words.push(alts(&token)),
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
pub(super) mod tests {
    use locallibrary::Playlist;

    use super::*;

    pub fn track(id: u32, artist: &str, title: &str) -> Track {
        Track {
            id,
            title: title.into(),
            artist: artist.into(),
            album: String::new(),
            album_artist: String::new(),
            genre: String::new(),
            label: String::new(),
            key: String::new(),
            remixer: String::new(),
            original_artist: String::new(),
            composer: String::new(),
            comment: String::new(),
            mix_name: String::new(),
            isrc: String::new(),
            bpm: 0.0,
            duration_secs: 300,
            year: 0,
            track_number: 0,
            disc_number: 0,
            rating: 0,
            color: None,
            play_count: 0,
            bitrate: 320,
            sample_rate: 44100,
            sample_depth: 16,
            file_size: 1000,
            file_path: format!("/Contents/{id}.mp3"),
            analyze_path: String::new(),
            date_added: String::new(),
            release_date: String::new(),
            artwork_id: None,
            artwork_path: None,
        }
    }

    /// Four tracks (two of them the same recording), a folder with a playlist, and a history list.
    pub fn library() -> Library {
        let mut a = track(1, "Daft Punk", "Around the World");
        a.key = "8A".into();
        a.bpm = 121.0;
        a.rating = 5;
        a.genre = "House".into();
        a.color = Some(TrackColor::Aqua);
        a.date_added = "2026-09-30".into();
        a.play_count = 2;
        let mut b = track(2, "floorplan", "Never Grow Old");
        b.key = "Am".into();
        b.bpm = 128.0;
        b.rating = 3;
        b.genre = "Techno".into();
        b.date_added = "2025-01-02".into();
        b.duration_secs = 420;
        let mut c = track(3, "Floorplan", "Tell You No Lie");
        c.key = "3B".into();
        c.bpm = 124.9;
        c.genre = "techno".into();
        let mut d = a.clone();
        d.id = 4;
        d.file_path = "/Contents/copy/1.mp3".into();
        let playlist = |id, parent, name: &str, folder, track_ids| Playlist {
            id,
            parent_id: parent,
            name: name.into(),
            is_folder: folder,
            sort_order: 0,
            track_ids,
        };
        Library {
            tracks: vec![a, b, c, d],
            playlists: vec![
                playlist(1, None, "Sets", true, vec![]),
                playlist(2, Some(1), "Friday", false, vec![2, 1, 3]),
            ],
            history_playlists: 1,
            history: vec![playlist(9, None, "HISTORY 001", false, vec![3, 2])],
        }
    }

    fn by_id(lib: &Library) -> HashMap<u32, usize> {
        lib.tracks.iter().enumerate().map(|(i, t)| (t.id, i)).collect()
    }

    fn summary(gs: &[Group]) -> Vec<(String, Vec<usize>)> {
        gs.iter().map(|g| (g.label.clone(), g.tracks.clone())).collect()
    }

    #[test]
    fn groups_tracks_by_category() {
        let lib = library();
        let ids = by_id(&lib);
        let (copy_of, copies) = duplicates(&lib);
        let problems = Problems { missing: None, copy_of, copies };
        let g = |cat| summary(&groups(&lib, cat, &ids, &problems, None));
        // Case and spacing don't split an artist; the first spelling names the group.
        assert_eq!(g(Category::Artist), [("Daft Punk".into(), vec![0, 3]), ("floorplan".into(), vec![1, 2])]);
        assert_eq!(g(Category::Album), [("No album".into(), vec![0, 1, 2, 3])]);
        // Am and 8A are the same key; round the wheel from 1A.
        assert_eq!(g(Category::Key), [("3B  Db".into(), vec![2]), ("8A  Am".into(), vec![0, 1, 3])]);
        assert_eq!(
            g(Category::Bpm),
            [("120–124".into(), vec![0, 2, 3]), ("125–129".into(), vec![1])],
            "124.9 is in the 120 band"
        );
        assert_eq!(
            g(Category::Rating),
            [("★★★★★".into(), vec![0, 3]), ("★★★··".into(), vec![1]), ("Unrated".into(), vec![2])]
        );
        assert_eq!(g(Category::Colour), [("Aqua".into(), vec![0, 3]), ("No colour".into(), vec![1, 2])]);
        assert_eq!(
            g(Category::Added),
            [("2026-09".into(), vec![0, 3]), ("2025-01".into(), vec![1]), ("Unknown".into(), vec![2])]
        );
        assert_eq!(g(Category::History), [("HISTORY 001".into(), vec![2, 1])]);
        assert_eq!(g(Category::Problems), [("Duplicates".into(), vec![0, 3])]);
        let problems = Problems { missing: Some(vec![false, true, false, false]), ..problems };
        assert_eq!(
            summary(&groups(&lib, Category::Problems, &ids, &problems, None))[0],
            ("Missing files".into(), vec![1])
        );
        assert_eq!(problems.others(0).collect::<Vec<_>>(), [3]);
        assert!(problems.is_missing(1) && !problems.is_missing(0));
    }

    #[test]
    fn tree_shows_categories_under_browse_and_the_playlists() {
        let lib = library();
        let ids = by_id(&lib);
        let all: HashMap<Category, Vec<Group>> =
            Category::ALL.iter().map(|&c| (c, groups(&lib, c, &ids, &Problems::default(), None))).collect();
        let names = |t: &[Node]| t.iter().map(|n| format!("{}{}", "-".repeat(n.depth), n.name)).collect::<Vec<_>>();
        let closed = tree(&lib, &HashSet::new(), &all);
        assert_eq!(names(&closed), ["All tracks", "Browse", "Sets"]);
        let open = HashSet::from([NodeId::Browse, NodeId::Category(Category::Key), NodeId::Playlist(1)]);
        let t = tree(&lib, &open, &all);
        assert_eq!(
            names(&t),
            [
                "All tracks",
                "Browse",
                "-Artist",
                "-Album",
                "-Genre",
                "-Key",
                "--3B  Db",
                "--8A  Am",
                "-BPM",
                "-Rating",
                "-Colour",
                "-Date added",
                "-History",
                "Sets",
                "-Friday",
            ],
            "Problems has nothing in it without a check"
        );
        assert_eq!(t[7].id, NodeId::Group(Category::Key, "8A".into()));
        assert_eq!((t[7].count, t[14].count), (3, 3));
    }

    #[test]
    fn sorting_by_rank_keeps_ties_in_list_order_both_ways() {
        let lib = library();
        let r = ranks(&lib, SortField::Key);
        let mut list = vec![3, 1, 2, 0];
        sort_list(&mut list, &r, false);
        assert_eq!(list, [2, 3, 1, 0], "3B before 8A; the three 8A tracks as they were");
        sort_list(&mut list, &r, true);
        assert_eq!(list, [3, 1, 0, 2]);
        let r = ranks(&lib, SortField::Artist);
        assert_eq!(r[1], r[2], "artists compare without case");
        let mut list = vec![1, 0, 2];
        sort_list(&mut list, &ranks(&lib, SortField::Bpm), true);
        assert_eq!(list, [1, 2, 0]);
    }

    #[test]
    fn ranks_agree_with_the_library_sort() {
        let lib = library();
        for field in SORT_FIELDS {
            let mut refs: Vec<&Track> = lib.tracks.iter().collect();
            locallibrary::query::sort_tracks(&mut refs, &[SortKey { field, descending: false }]);
            let expected: Vec<u32> = refs.iter().map(|t| t.id).collect();
            let mut list: Vec<usize> = (0..lib.tracks.len()).collect();
            sort_list(&mut list, &ranks(&lib, field), false);
            let got: Vec<u32> = list.iter().map(|&i| lib.tracks[i].id).collect();
            assert_eq!(got, expected, "{field:?}");
        }
    }

    #[test]
    fn sort_cycles_and_clicks() {
        let k = |field, descending| Some(SortKey { field, descending });
        assert_eq!(cycle(None), k(SortField::Title, false));
        assert_eq!(cycle(k(SortField::Key, true)), k(SortField::Rating, true), "ratings start highest first");
        assert_eq!(cycle(k(SortField::Plays, true)), None);
        assert_eq!(click(None, SortField::Bpm), k(SortField::Bpm, false));
        assert_eq!(click(k(SortField::Bpm, false), SortField::Bpm), k(SortField::Bpm, true));
        assert_eq!(click(k(SortField::Bpm, true), SortField::Bpm), None);
        assert_eq!(click(k(SortField::Title, false), SortField::Added), k(SortField::Added, true));
        assert_eq!(sort_spec(SortKey { field: SortField::Bpm, descending: true }), "-bpm");
        assert!(
            locallibrary::query::parse_sort(&sort_spec(SortKey { field: SortField::Added, descending: false })).is_ok()
        );
    }

    #[test]
    fn searches_split_into_words_and_the_rest() {
        let words = |q: &str| Search::parse(q).words;
        assert_eq!(words("  Daft  PUNK "), [vec!["daft"], vec!["punk"]]);
        assert_eq!(words("\"daft  punk\" house|techno"), [vec!["daft punk"], vec!["house", "techno"]]);
        assert_eq!(words("a| \"\""), [vec!["a"]]);
        let s = Search::parse("re - edit -techno -5 artist:\"daft punk\" bpm:120-128 -genre:house");
        assert_eq!(s.words, [vec!["re"], vec!["-"], vec!["edit"], vec!["-5"]]);
        assert_eq!(s.not, [vec!["techno"]]);
        assert_eq!(s.rest, "artist:\"daft punk\" bpm:120-128 -genre:house", "as typed");
        assert!(s.matches("re-edit, -5 - edit") && !s.matches("re - edit -5 techno"));
        let mut t = tests::track(1, "Daft Punk", "Around the World");
        t.comment = "Peak".into();
        let hay = haystack(&t);
        assert!(hay.contains("peak") && hay.contains("/contents/1.mp3"));
        assert!(!hay.contains("punk around"), "fields stay apart");
    }
}
