//! Model-based tests of editing a drive: random batches of edits against a model of the playlist
//! tree, playlist entries, track details and history lists, checked against both databases,
//! rekordcrate's reading of export.pdb, undo and the journal of edits.

use crate::common;

use indexmap::IndexMap;
use std::collections::BTreeSet;

use rustc_hash::FxHashMap;
use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use crate::common::{Before, Files, apply, changed_files, drive_files, replay};
use binrw::{BinRead, BinWrite};
use hegel::generators as gs;
use hegel::{HealthCheck, TestCase};
use locallibrary::device::Device;
use locallibrary::edit::{self, Edit, EditOptions, EditPlan, TrackFields};
use locallibrary::model::TrackColor;
use locallibrary::progress::Ctx;
use locallibrary::query::TrackSelector;
use locallibrary::{Library, Playlist, Track, backup, harmony, paths, pdb, query};

// ---------------------------------------------------------------------------------------------
// The drive and what can be seen of it
// ---------------------------------------------------------------------------------------------

/// The twelve-track export with its playlist and two history lists.
fn fresh_pdb_drive(dir: &Path) -> PathBuf {
    let root = common::bigger_drive(dir, "Main");
    common::add_history(&root, "HISTORY 001", &[1, 2]);
    common::add_history(&root, "History 002", &[3, 1]);
    root
}

/// OneLibrary's database for that drive, made once: its key derivation is cached per salt, and
/// every drive made here shares this file's salt.
fn plus_db() -> &'static [u8] {
    static DB: OnceLock<Vec<u8>> = OnceLock::new();
    DB.get_or_init(|| {
        let dir = tempfile::tempdir().unwrap();
        let root = fresh_pdb_drive(dir.path());
        common::add_onelibrary(&root);
        fs::read(root.join(paths::LIBRARY_PLUS_DB)).unwrap()
    })
}

/// The track details an edit can change.
#[derive(Clone, Debug, PartialEq)]
struct Details {
    title: String,
    artist: String,
    album: String,
    album_artist: String,
    genre: String,
    label: String,
    key: String,
    remixer: String,
    original_artist: String,
    composer: String,
    comment: String,
    mix_name: String,
    rating: u8,
    color: Option<TrackColor>,
    year: u16,
    play_count: u16,
    track_number: u32,
    disc_number: u16,
}

impl From<&Track> for Details {
    fn from(t: &Track) -> Self {
        Details {
            title: t.title.clone(),
            artist: t.artist.clone(),
            album: t.album.clone(),
            album_artist: t.album_artist.clone(),
            genre: t.genre.clone(),
            label: t.label.clone(),
            key: t.key.clone(),
            remixer: t.remixer.clone(),
            original_artist: t.original_artist.clone(),
            composer: t.composer.clone(),
            comment: t.comment.clone(),
            mix_name: t.mix_name.clone(),
            rating: t.rating,
            color: t.color,
            year: t.year,
            play_count: t.play_count,
            track_number: t.track_number,
            disc_number: t.disc_number,
        }
    }
}

/// A playlist or folder with its entries (track keys) and, for folders, what's in it in order.
#[derive(Clone, Debug, PartialEq)]
struct Node {
    name: String,
    folder: bool,
    entries: Vec<String>,
    kids: Vec<Node>,
}

/// Everything an edit can change, comparable across databases (tracks by file, playlists by
/// their place in the tree).
#[derive(Clone, Debug, PartialEq)]
struct Shape {
    top: Vec<Node>,
    tracks: IndexMap<String, Details>,
    history: Vec<(String, Vec<String>)>,
}

fn shape(lib: &Library) -> Shape {
    let keys: FxHashMap<u32, String> = lib.tracks.iter().map(|t| (t.id, paths::file_key(&t.file_path))).collect();
    let entries = |ids: &[u32]| -> Vec<String> {
        ids.iter().map(|id| keys.get(id).cloned().unwrap_or_else(|| format!("<no track #{id}>"))).collect()
    };
    fn nodes(lib: &Library, parent: Option<u32>, entries: &dyn Fn(&[u32]) -> Vec<String>, depth: usize) -> Vec<Node> {
        assert!(depth < 64, "the playlist tree has a cycle");
        lib.children(parent)
            .into_iter()
            .map(|p| Node {
                name: p.name.clone(),
                folder: p.is_folder,
                entries: entries(&p.track_ids),
                kids: nodes(lib, Some(p.id), entries, depth + 1),
            })
            .collect()
    }
    let top = nodes(lib, None, &entries, 0);
    let mut history: Vec<&Playlist> = lib.history.iter().filter(|h| !h.is_folder).collect();
    history.sort_by_key(|h| (h.sort_order, h.id));
    let s = Shape {
        top,
        tracks: lib.tracks.iter().map(|t| (paths::file_key(&t.file_path), Details::from(t))).collect(),
        history: history.iter().map(|h| (h.name.clone(), entries(&h.track_ids))).collect(),
    };
    assert_eq!(s.walk().len(), lib.playlists.len(), "every playlist hangs off the tree");
    s
}

/// `F01` for `Contents/.../Framed And Defamed/01 A1 - ...mp3`: short enough to read a playlist.
fn label(key: &str) -> String {
    let mut parts = key.rsplit('/');
    let file = parts.next().unwrap_or_default();
    let album = parts.next().unwrap_or_default();
    format!("{}{}", album.chars().next().unwrap_or('?').to_ascii_uppercase(), file.chars().take(2).collect::<String>())
}

fn outline(nodes: &[Node]) -> String {
    let one = |n: &Node| {
        if n.folder {
            format!("{:?}/{{{}}}", n.name, outline(&n.kids))
        } else {
            format!("{:?}[{}]", n.name, n.entries.iter().map(|k| label(k)).collect::<Vec<_>>().join(" "))
        }
    };
    nodes.iter().map(one).collect::<Vec<_>>().join(", ")
}

/// Panic with the first difference between what a database holds and what it should.
fn assert_shape(actual: &Shape, expected: &Shape, what: &str) {
    if actual == expected {
        return;
    }
    for (k, t) in &expected.tracks {
        if actual.tracks.get(k) != Some(t) {
            panic!("{what}: track {k}: expected {t:?}, read back {:?}", actual.tracks.get(k));
        }
    }
    if let Some(k) = actual.tracks.keys().find(|k| !expected.tracks.contains_key(*k)) {
        panic!("{what}: track {k} should be gone");
    }
    if actual.top != expected.top {
        panic!("{what}: playlists differ\nexpected {}\nread     {}", outline(&expected.top), outline(&actual.top));
    }
    panic!("{what}: history differs: expected {:?}, read back {:?}", expected.history, actual.history);
}

/// export.pdb's playlist tree, entries and track count as rekordcrate (an independent parser)
/// reads them agree with locallibrary's reading, and entries are numbered 1..n.
fn assert_rekordcrate_agrees(root: &Path, lib: &Library) {
    use rekordcrate::pdb::{Header, PageType, Row};
    let bytes = fs::read(root.join(paths::EXPORT_PDB)).unwrap();
    let mut reader = Cursor::new(&bytes);
    let header = Header::read(&mut reader).unwrap();
    let mut rows = |t: PageType| -> Vec<Row> {
        let table = header.tables.iter().find(|x| x.page_type == t).unwrap();
        let pages =
            header.read_pages(&mut reader, binrw::Endian::NATIVE, (&table.first_page, &table.last_page)).unwrap();
        pages
            .into_iter()
            .flat_map(|p| p.row_groups.into_iter())
            .flat_map(|g| g.present_rows().collect::<Vec<_>>())
            .collect()
    };
    // Fields rekordcrate keeps private are read back from the row's own bytes.
    let raw = |row: &dyn Fn(&mut Cursor<Vec<u8>>)| -> Vec<u32> {
        let mut c = Cursor::new(Vec::new());
        row(&mut c);
        c.into_inner().chunks_exact(4).take(5).map(|b| u32::from_le_bytes(b.try_into().unwrap())).collect()
    };
    let mut tree = IndexMap::new();
    for r in rows(PageType::PlaylistTree) {
        let Row::PlaylistTreeNode(n) = r else { panic!("not a playlist tree row: {r:?}") };
        let fields = raw(&|c| n.write(c).unwrap());
        tree.insert(
            n.id.0,
            (n.parent_id.0, fields[2], n.is_folder(), n.name.clone().into_string().unwrap_or_default()),
        );
    }
    for p in lib.playlists.iter().filter(|p| common::rekordcrate_reads_as_isrc(&p.name)) {
        if let Some(node) = tree.get_mut(&p.id) {
            node.3.clone_from(&p.name);
        }
    }
    let ours: IndexMap<u32, (u32, u32, bool, String)> = lib
        .playlists
        .iter()
        .map(|p| (p.id, (p.parent_id.unwrap_or(0), p.sort_order, p.is_folder, p.name.clone())))
        .collect();
    assert_eq!(tree, ours, "the playlist tree as rekordcrate reads it");
    let mut entries: IndexMap<u32, Vec<(u32, u32)>> = IndexMap::new();
    for r in rows(PageType::PlaylistEntries) {
        let Row::PlaylistEntry(e) = r else { panic!("not a playlist entry row: {r:?}") };
        let f = raw(&|c| e.write(c).unwrap());
        entries.entry(f[2]).or_default().push((f[0], f[1]));
    }
    let mut theirs = IndexMap::new();
    for (playlist, mut list) in entries {
        list.sort_unstable();
        let numbers: Vec<u32> = list.iter().map(|(i, _)| *i).collect();
        assert_eq!(numbers, (1..=list.len() as u32).collect::<Vec<_>>(), "entry numbers of playlist #{playlist}");
        theirs.insert(playlist, list.into_iter().map(|(_, t)| t).collect::<Vec<u32>>());
    }
    let ours: IndexMap<u32, Vec<u32>> =
        lib.playlists.iter().filter(|p| !p.track_ids.is_empty()).map(|p| (p.id, p.track_ids.clone())).collect();
    assert_eq!(theirs, ours, "playlist entries as rekordcrate reads them");
    assert_eq!(rows(PageType::Tracks).len(), lib.tracks.len(), "track rows as rekordcrate reads them");
}

// ---------------------------------------------------------------------------------------------
// The model
// ---------------------------------------------------------------------------------------------

type Ix = Vec<usize>;

impl Shape {
    fn kids(&self, parent: &[usize]) -> &Vec<Node> {
        parent.iter().fold(&self.top, |v, &i| &v[i].kids)
    }

    fn kids_mut(&mut self, parent: &[usize]) -> &mut Vec<Node> {
        parent.iter().fold(&mut self.top, |v, &i| &mut v[i].kids)
    }

    fn node(&self, ix: &[usize]) -> &Node {
        let (last, parent) = ix.split_last().expect("a node");
        &self.kids(parent)[*last]
    }

    fn node_mut(&mut self, ix: &[usize]) -> &mut Node {
        let (last, parent) = ix.split_last().expect("a node");
        &mut self.kids_mut(parent)[*last]
    }

    /// Every node with its path, top down.
    fn walk(&self) -> Vec<(Ix, String)> {
        fn go(kids: &[Node], ix: &mut Ix, prefix: &str, out: &mut Vec<(Ix, String)>) {
            for (i, n) in kids.iter().enumerate() {
                ix.push(i);
                let path = if prefix.is_empty() { n.name.clone() } else { format!("{prefix}/{}", n.name) };
                out.push((ix.clone(), path.clone()));
                go(&n.kids, ix, &path, out);
                ix.pop();
            }
        }
        let mut out = Vec::new();
        go(&self.top, &mut Vec::new(), "", &mut out);
        out
    }

    /// How edits name playlists: a path (ignoring ASCII case), else a name that only one playlist
    /// has exactly or, failing that, ignoring ASCII case.
    fn find(&self, spec: &str) -> Option<Ix> {
        self.find_kind(spec, None)
    }

    /// [`Shape::find`] among folders only (`Some(true)`) or playlists only (`Some(false)`), as
    /// edits that need one of them look.
    fn find_kind(&self, spec: &str, folder: Option<bool>) -> Option<Ix> {
        let q = spec.trim_matches('/');
        let all: Vec<(Ix, String)> =
            self.walk().into_iter().filter(|(ix, _)| folder.is_none_or(|f| self.node(ix).folder == f)).collect();
        let by_path: Vec<&Ix> = all.iter().filter(|(_, p)| p.eq_ignore_ascii_case(q)).map(|(ix, _)| ix).collect();
        match by_path.as_slice() {
            [ix] => return Some((*ix).clone()),
            [] => {}
            _ => panic!("the model let two playlists share the path {q:?}"),
        }
        let named = |same: fn(&str, &str) -> bool| -> Vec<&Ix> {
            all.iter().filter(|(ix, _)| same(&self.node(ix).name, q)).map(|(ix, _)| ix).collect()
        };
        match (named(|a, b| a == b).as_slice(), named(str::eq_ignore_ascii_case).as_slice()) {
            ([ix], _) | ([], [ix]) => Some((*ix).clone()),
            _ => None,
        }
    }

    /// How an edit that makes a playlist if it's missing names one: by its exact path only, never
    /// a playlist of that name in another folder.
    fn find_exact(&self, spec: &str) -> Option<Ix> {
        let q = spec.trim_matches('/');
        self.walk().into_iter().find(|(ix, p)| !self.node(ix).folder && p == q).map(|(ix, _)| ix)
    }

    /// A node by its exact names from the top.
    fn exact(&self, names: &[String]) -> Option<Ix> {
        let mut ix = Vec::new();
        for name in names {
            ix.push(self.kids(&ix).iter().position(|n| n.name == *name)?);
        }
        Some(ix)
    }

    /// Paths as [`locallibrary::model::LibraryDiff`] lists them (folders end with `/`).
    fn listing(&self) -> BTreeSet<String> {
        self.walk().into_iter().map(|(ix, p)| if self.node(&ix).folder { format!("{p}/") } else { p }).collect()
    }

    fn playlists_mut(&mut self) -> Vec<&mut Vec<String>> {
        fn go<'a>(kids: &'a mut [Node], out: &mut Vec<&'a mut Vec<String>>) {
            for n in kids {
                out.push(&mut n.entries);
                go(&mut n.kids, out);
            }
        }
        let mut out = Vec::new();
        go(&mut self.top, &mut out);
        out.extend(self.history.iter_mut().map(|(_, e)| e));
        out
    }
}

fn subtree_entries(n: &Node, out: &mut Vec<String>) {
    out.extend(n.entries.iter().cloned());
    for k in &n.kids {
        subtree_entries(k, out);
    }
}

fn split_path(path: &str) -> Vec<String> {
    path.split('/').map(str::trim).filter(|n| !n.is_empty()).map(str::to_string).collect()
}

/// The most a row can take on export.pdb's 4096-byte pages: the page less its 40-byte header and
/// a one-row directory (6 bytes).
const ROW_MAX: usize = 4096 - 40 - 6;

/// Bytes a string takes in export.pdb: ASCII up to 126 characters after one byte, longer ASCII
/// after four, anything else as UTF-16 after four.
fn encoded_len(s: &str) -> usize {
    match s.is_ascii() {
        true if s.len() <= 126 => s.len() + 1,
        true => s.len() + 4,
        false => s.encode_utf16().count() * 2 + 4,
    }
}

/// Whether export.pdb holds a playlist or folder of this name: 20 bytes, then the name, padded to
/// four.
fn name_fits(name: &str) -> bool {
    (20 + encoded_len(name)).next_multiple_of(4) <= ROW_MAX
}

/// Whether a track's row holds its title, comment and mix name (each padded to four) next to the
/// rest of it (`others` bytes, see [`track_rows`]).
fn details_fit(others: usize, d: &Details) -> bool {
    let strings: usize = [&d.title, &d.comment, &d.mix_name].iter().map(|s| encoded_len(s).next_multiple_of(4)).sum();
    others + strings <= ROW_MAX
}

/// What each track's export.pdb row takes besides its title, comment and mix name: 136 bytes of
/// fields and string offsets, its other 18 strings (each padded to four) and 4 bytes more.
fn track_rows(root: &Path) -> IndexMap<String, usize> {
    let db = rekordbox_pdb::Database::from_file(root.join(paths::EXPORT_PDB)).unwrap();
    let edited = ["title", "comment", "mix_name"];
    db.tracks
        .iter()
        .map(|t| {
            let others: usize = t
                .strings
                .iter()
                .zip(rekordbox_pdb::pdb::TRACK_STRING_NAMES)
                .filter(|(_, name)| !edited.contains(name))
                .map(|(s, name)| if name == "isrc" { common::isrc_len(s) } else { encoded_len(s) }.next_multiple_of(4))
                .sum();
            (paths::file_key(t.file_path()), 136 + others + 4)
        })
        .collect()
}

/// What the drive should hold.
#[derive(Clone, Debug, PartialEq)]
struct Model {
    shape: Shape,
    /// Audio files (track keys) that edits deleted.
    audio_gone: BTreeSet<String>,
    /// [`track_rows`] as the drive started: edits never change the strings counted there.
    track_rows: IndexMap<String, usize>,
}

type Outcome<T = ()> = Result<T, String>;

impl Model {
    /// The model after `edits`, or why the library should refuse them (all or nothing).
    fn after(&self, edits: &[Edit]) -> Outcome<Model> {
        let mut m = self.clone();
        for e in edits {
            m.apply(e)?;
        }
        Ok(m)
    }

    fn playlist(&self, spec: &str) -> Outcome<Ix> {
        self.shape.find(spec).ok_or_else(|| format!("no playlist {spec:?}"))
    }

    fn list(&self, spec: &str) -> Outcome<Ix> {
        self.shape.find_kind(spec, Some(false)).ok_or_else(|| format!("no playlist {spec:?}"))
    }

    /// Track keys a selector names, each once, in order.
    fn select(&self, sel: &TrackSelector) -> Outcome<Vec<String>> {
        if sel.paths.is_empty() {
            return Err("no tracks selected".into());
        }
        let mut out = Vec::new();
        for p in &sel.paths {
            let k = paths::file_key(p);
            if !self.shape.tracks.contains_key(&k) {
                return Err(format!("no track at {p}"));
            }
            if !out.contains(&k) {
                out.push(k);
            }
        }
        Ok(out)
    }

    fn create(&mut self, path: &str, folder: bool) -> Outcome {
        let names = split_path(path);
        if names.is_empty() {
            return Err("no name".into());
        }
        let mut parent = Vec::new();
        for (i, name) in names.iter().enumerate() {
            let last = i + 1 == names.len();
            let kids = self.shape.kids(&parent);
            match kids.iter().position(|n| n.name == *name) {
                Some(j) if last => {
                    return if kids[j].folder == folder {
                        Ok(())
                    } else {
                        Err(format!("{path:?} exists as the other kind"))
                    };
                }
                Some(j) if !kids[j].folder => return Err(format!("{name:?} is a playlist, not a folder")),
                Some(j) => parent.push(j),
                None => {
                    if !name_fits(name) {
                        return Err("a name too long for export.pdb".into());
                    }
                    let kids = self.shape.kids_mut(&parent);
                    kids.push(Node {
                        name: name.clone(),
                        folder: !last || folder,
                        entries: Vec::new(),
                        kids: Vec::new(),
                    });
                    parent.push(kids.len() - 1);
                }
            }
        }
        Ok(())
    }

    fn delete_tracks(&mut self, keys: &[String], keep_files: bool) {
        for k in keys {
            self.shape.tracks.shift_remove(k);
            if !keep_files {
                self.audio_gone.insert(k.clone());
            }
        }
        for entries in self.shape.playlists_mut() {
            entries.retain(|k| !keys.contains(k));
        }
    }

    fn entries_mut(&mut self, ix: &[usize]) -> &mut Vec<String> {
        &mut self.shape.node_mut(ix).entries
    }

    fn apply(&mut self, edit: &Edit) -> Outcome {
        match edit {
            Edit::CreatePlaylist { path, folder } => self.create(path, *folder)?,
            Edit::RenamePlaylist { playlist, name } => {
                let ix = self.playlist(playlist)?;
                let name = name.trim();
                if name.is_empty() || name.contains('/') {
                    return Err("names can't be empty or contain /".into());
                }
                let (last, parent) = ix.split_last().unwrap();
                if self.shape.kids(parent).iter().enumerate().any(|(j, n)| j != *last && n.name == name) {
                    return Err(format!("{name:?} is taken"));
                }
                if !name_fits(name) {
                    return Err("a name too long for export.pdb".into());
                }
                self.shape.node_mut(&ix).name = name.to_string();
            }
            Edit::MovePlaylist { playlist, into, position } => {
                let ix = self.playlist(playlist)?;
                let mut parent = match into.as_deref().map(str::trim).filter(|s| !s.is_empty() && *s != "/") {
                    None => Vec::new(),
                    Some(f) => self.shape.find_kind(f, Some(true)).ok_or_else(|| format!("no folder {f:?}"))?,
                };
                if *position == Some(0) {
                    return Err("positions start at 1".into());
                }
                if parent.starts_with(&ix) {
                    return Err("can't move a folder into itself".into());
                }
                let (last, old_parent) = ix.split_last().unwrap();
                let name = &self.shape.node(&ix).name;
                // Playlists are named by path, so siblings never share a name.
                if self
                    .shape
                    .kids(&parent)
                    .iter()
                    .enumerate()
                    .any(|(j, n)| n.name == *name && !(parent == old_parent && j == *last))
                {
                    return Err(format!("{name:?} is taken there"));
                }
                let node = self.shape.kids_mut(old_parent).remove(*last);
                let depth = old_parent.len();
                if parent.len() > depth && parent.starts_with(old_parent) && parent[depth] > *last {
                    parent[depth] -= 1;
                }
                let kids = self.shape.kids_mut(&parent);
                let at = position.map_or(kids.len(), |p| (p - 1).min(kids.len()));
                kids.insert(at, node);
            }
            Edit::DeletePlaylist { playlist, with_tracks } => {
                let ix = self.playlist(playlist)?;
                let (last, parent) = ix.split_last().unwrap();
                let node = self.shape.kids_mut(parent).remove(*last);
                if *with_tracks {
                    let mut inside = Vec::new();
                    subtree_entries(&node, &mut inside);
                    let mut elsewhere = Vec::new();
                    for n in &self.shape.top {
                        subtree_entries(n, &mut elsewhere);
                    }
                    elsewhere.extend(self.shape.history.iter().flat_map(|(_, e)| e.iter().cloned()));
                    let mut doomed: Vec<String> = inside.into_iter().filter(|k| !elsewhere.contains(k)).collect();
                    doomed.sort();
                    doomed.dedup();
                    self.delete_tracks(&doomed, false);
                }
            }
            Edit::AddTracks { playlist, tracks, position, sort: _, allow_repeats } => {
                let ix = self.list(playlist)?;
                let mut add = self.select(tracks)?;
                let old = self.shape.node(&ix).entries.clone();
                if !allow_repeats {
                    add.retain(|k| !old.contains(k));
                }
                let at = match position {
                    Some(0) => return Err("positions start at 1".into()),
                    Some(n) => (n - 1).min(old.len()),
                    None => old.len(),
                };
                self.entries_mut(&ix).splice(at..at, add);
            }
            Edit::RemoveTracks { playlist, tracks, positions } => {
                let ix = self.list(playlist)?;
                let doomed = match tracks {
                    Some(sel) => self.select(sel)?,
                    None => Vec::new(),
                };
                let len = self.shape.node(&ix).entries.len();
                if positions.iter().any(|p| *p == 0 || *p > len) {
                    return Err("position outside the playlist".into());
                }
                if tracks.is_none() && positions.is_empty() {
                    return Err("nothing to remove".into());
                }
                let old = std::mem::take(self.entries_mut(&ix));
                *self.entries_mut(&ix) = old
                    .into_iter()
                    .enumerate()
                    .filter(|(i, k)| !positions.contains(&(i + 1)) && !doomed.contains(k))
                    .map(|(_, k)| k)
                    .collect();
            }
            Edit::MoveTracks { playlist, positions, to } => {
                let ix = self.list(playlist)?;
                let old = self.shape.node(&ix).entries.clone();
                if positions.iter().any(|p| *p == 0 || *p > old.len()) {
                    return Err("position outside the playlist".into());
                }
                let picked: BTreeSet<usize> = positions.iter().map(|p| p - 1).collect();
                if picked.is_empty() {
                    return Err("nothing to move".into());
                }
                let moving: Vec<String> = picked.iter().map(|i| old[*i].clone()).collect();
                let mut rest: Vec<String> =
                    old.into_iter().enumerate().filter(|(i, _)| !picked.contains(i)).map(|(_, k)| k).collect();
                let at = to.saturating_sub(1).min(rest.len());
                rest.splice(at..at, moving);
                *self.entries_mut(&ix) = rest;
            }
            Edit::SetTracks { playlist, tracks, sort: _, limit, create } => {
                let mut keys = self.select(tracks)?;
                if let Some(n) = limit {
                    keys.truncate(*n);
                }
                let found = match create {
                    true => self.shape.find_exact(playlist),
                    false => self.shape.find_kind(playlist, Some(false)),
                };
                let ix = match found {
                    Some(ix) => ix,
                    None if *create => {
                        self.create(playlist, false)?;
                        self.shape.exact(&split_path(playlist)).expect("just created")
                    }
                    None => return Err(format!("no playlist {playlist:?}")),
                };
                *self.entries_mut(&ix) = keys;
            }
            Edit::Dedupe { playlist, recordings: false } => {
                let ix = self.list(playlist)?;
                let mut seen = BTreeSet::new();
                self.entries_mut(&ix).retain(|k| seen.insert(k.clone()));
            }
            Edit::EditTracks { tracks, fields } => {
                validate(fields)?;
                let strings = [&fields.title, &fields.comment, &fields.mix_name].iter().any(|v| v.is_some());
                for k in self.select(tracks)? {
                    let d = self.shape.tracks.get_mut(&k).unwrap();
                    set_fields(d, fields);
                    if strings && !details_fit(self.track_rows[&k], d) {
                        return Err("track details too long for export.pdb".into());
                    }
                }
            }
            Edit::DeleteTracks { tracks, keep_files } => {
                let keys = self.select(tracks)?;
                self.delete_tracks(&keys, *keep_files);
            }
            Edit::ReplaceTrack { duplicate, keep, keep_duplicate } => {
                let (dup, keeper) = match (self.select(duplicate)?.as_slice(), self.select(keep)?.as_slice()) {
                    ([d], [k]) if d != k => (d.clone(), k.clone()),
                    _ => return Err("replace exactly one track with exactly one other".into()),
                };
                for entries in self.shape.playlists_mut() {
                    for k in entries.iter_mut().filter(|k| **k == dup) {
                        *k = keeper.clone();
                    }
                }
                if !keep_duplicate {
                    self.delete_tracks(&[dup], false);
                }
            }
            Edit::DeleteHistory { names, all } => {
                let matches = |n: &String, h: &str| n.trim().eq_ignore_ascii_case(h);
                if let Some(n) = names.iter().find(|n| !self.shape.history.iter().any(|(h, _)| matches(n, h))) {
                    return Err(format!("no history list {n:?}"));
                }
                let before = self.shape.history.len();
                self.shape.history.retain(|(h, _)| !*all && !names.iter().any(|n| matches(n, h)));
                if before == self.shape.history.len() && !all {
                    return Err("no history lists named".into());
                }
            }
            other => unimplemented!("the model doesn't do {other:?}"),
        }
        Ok(())
    }
}

/// Which fields an edit may set (as documented on [`TrackFields`]).
fn validate(f: &TrackFields) -> Outcome {
    if f.is_empty() {
        return Err("no fields".into());
    }
    if f.rating.is_some_and(|r| r > 5) {
        return Err("ratings go to 5".into());
    }
    if let Some(c) = &f.color {
        query::parse_color(c).map_err(|e| e.to_string())?;
    }
    if let Some(k) = &f.key
        && !k.trim().is_empty()
        && harmony::Camelot::parse(k).is_none()
    {
        return Err(format!("{k:?} isn't a key"));
    }
    Ok(())
}

/// Fields left out stay; an empty name clears artist, album (and its artist), genre, label, key
/// and the like; titles, comments and mix names are taken as given.
fn set_fields(d: &mut Details, f: &TrackFields) {
    let name = |v: &str| if v.trim().is_empty() { String::new() } else { v.to_string() };
    for (v, field) in [(&f.title, &mut d.title), (&f.comment, &mut d.comment), (&f.mix_name, &mut d.mix_name)] {
        if let Some(v) = v {
            *field = v.clone();
        }
    }
    for (v, field) in [
        (&f.artist, &mut d.artist),
        (&f.genre, &mut d.genre),
        (&f.label, &mut d.label),
        (&f.key, &mut d.key),
        (&f.remixer, &mut d.remixer),
        (&f.original_artist, &mut d.original_artist),
        (&f.composer, &mut d.composer),
    ] {
        if let Some(v) = v {
            *field = name(v);
        }
    }
    if let Some(a) = &f.album {
        d.album = name(a);
        if d.album.is_empty() {
            d.album_artist.clear();
        }
    }
    if let Some(r) = f.rating {
        d.rating = r;
    }
    if let Some(c) = &f.color {
        d.color = query::parse_color(c).unwrap();
    }
    if let Some(y) = f.year {
        d.year = y;
    }
    if let Some(n) = f.play_count {
        d.play_count = n;
    }
    if let Some(n) = f.track_number {
        d.track_number = n;
    }
    if let Some(n) = f.disc_number {
        d.disc_number = n;
    }
}

// ---------------------------------------------------------------------------------------------
// Running edits
// ---------------------------------------------------------------------------------------------

struct Drive {
    _dir: tempfile::TempDir,
    root: PathBuf,
    plus: bool,
    model: Model,
    /// Device paths of the tracks the drive started with (deleted ones stay, to be refused).
    pool: Vec<String>,
    /// What undo goes back to: the model and files before the last edit that made a backup.
    undo: Option<(Model, Files)>,
}

/// What came of a batch of edits.
#[derive(Debug)]
enum Attempt {
    /// `plan` refused them, as the model said it would.
    Refused,
    Applied,
    /// `plan` took them but `apply` failed; `untouched` says whether the drive was left alone.
    Failed {
        error: String,
        untouched: Outcome,
    },
}

impl Drive {
    fn new(plus: bool) -> Drive {
        let dir = tempfile::tempdir().unwrap();
        let root = fresh_pdb_drive(dir.path());
        if plus {
            fs::write(root.join(paths::LIBRARY_PLUS_DB), plus_db()).unwrap();
        }
        let lib = pdb::read_library(&root.join(paths::EXPORT_PDB)).unwrap();
        let s = shape(&lib);
        if plus {
            assert_shape(&shape(&common::plus_library(&root)), &s, "exportLibrary.db as made");
        }
        let pool = lib.tracks.iter().map(|t| t.file_path.clone()).collect();
        let model = Model { shape: s, audio_gone: BTreeSet::new(), track_rows: track_rows(&root) };
        Drive { _dir: dir, root, plus, model, pool, undo: None }
    }

    /// Plan and apply `edits`, holding the library to the model.
    fn attempt(&mut self, tc: &TestCase, edits: &[Edit]) -> Attempt {
        let expected = self.model.after(edits);
        tc.note(&format!(
            "edits: {edits:?}\n  model: {}",
            match &expected {
                Ok(m) => outline(&m.shape.top),
                Err(e) => format!("refused ({e})"),
            }
        ));
        let before = Before::take(&self.root);
        let mut d = Device::open_export(&self.root).unwrap();
        let plan = edit::plan(&d, edits, &EditOptions::default(), &Ctx::silent());
        let (plan, after) = match (plan, expected) {
            (Err(_), Err(_)) => {
                if let Err(e) = before.untouched(&self.root) {
                    panic!("a refused plan changed the drive: {e}");
                }
                tc.event("batch refused");
                return Attempt::Refused;
            }
            (Err(e), Ok(_)) => panic!("plan refused edits the model takes: {e}"),
            (Ok(p), Err(why)) => panic!("plan took edits the model refuses ({why}): {:?}", p.changes),
            (Ok(p), Ok(m)) => (p, m),
        };
        self.check_plan(&plan, &after);
        match edit::apply(&plan, &mut d, &Ctx::silent()) {
            Err(e) => Attempt::Failed { error: e.to_string(), untouched: before.untouched(&self.root) },
            Ok(report) => {
                tc.event("batch applied");
                // Undo takes back the last change that changed something: one that left every file
                // as it was is passed by.
                if report.backup.is_some() && !changed_files(&before.files, &drive_files(&self.root)).is_empty() {
                    self.undo = Some((self.model.clone(), before.files));
                }
                self.model = after;
                self.check();
                Attempt::Applied
            }
        }
    }

    /// The plan's summary agrees with the model.
    fn check_plan(&self, plan: &EditPlan, after: &Model) {
        let (old, new) = (self.model.shape.listing(), after.shape.listing());
        let set = |v: &[String]| v.iter().cloned().collect::<BTreeSet<_>>();
        assert_eq!(set(&plan.playlists_created), new.difference(&old).cloned().collect(), "playlists created");
        assert_eq!(set(&plan.playlists_deleted), old.difference(&new).cloned().collect(), "playlists deleted");
        let removed: BTreeSet<String> =
            self.model.shape.tracks.keys().filter(|k| !after.shape.tracks.contains_key(*k)).cloned().collect();
        let planned: BTreeSet<String> = plan.tracks_removed.iter().map(|t| paths::file_key(&t.file_path)).collect();
        assert_eq!(planned, removed, "tracks removed");
    }

    /// Both databases, rekordcrate and the audio files agree with the model.
    fn check(&self) {
        let d = Device::open_export(&self.root).unwrap();
        assert!(d.state().is_none_or(|s| s.pending.is_none()), "the drive is left marked as interrupted");
        let lib = d.library().unwrap();
        assert_shape(&shape(&lib), &self.model.shape, "export.pdb");
        if self.plus {
            assert_shape(&shape(&common::plus_library(&self.root)), &self.model.shape, "exportLibrary.db");
        }
        assert_rekordcrate_agrees(&self.root, &lib);
        for p in &self.pool {
            let present = paths::to_fs(&self.root, &paths::key(p)).is_file();
            assert_eq!(present, !self.model.audio_gone.contains(&paths::file_key(p)), "audio of {p} on the drive");
        }
    }

    /// Undo: the drive is as it was before the last edit, byte for byte. Undoing again would
    /// step back to the edit before, which isn't kept.
    fn undo(&mut self) -> bool {
        let Some((model, files)) = self.undo.take() else { return false };
        let mut d = Device::open_export(&self.root).unwrap();
        backup::restore(&mut d, None, false).unwrap();
        let changed = changed_files(&files, &drive_files(&self.root));
        assert!(changed.is_empty(), "restoring the backup left these files different: {changed:?}");
        self.model = model;
        self.check();
        true
    }

    /// Draw and attempt a batch of one to three edits.
    fn edit(&mut self, tc: &TestCase, names: Names) -> Attempt {
        let n = tc.draw(gs::integers::<usize>().min_value(1).max_value(3));
        let mut edits = Vec::new();
        let mut m = self.model.clone();
        for _ in 0..n {
            let e = draw_edit(tc, &m, &self.pool, names);
            // Later edits are drawn against what the earlier ones make (while they work).
            let kind = format!("{e:?}");
            let kind = kind.split_once(' ').map_or(kind.as_str(), |(k, _)| k);
            match m.after(std::slice::from_ref(&e)) {
                Ok(next) => {
                    tc.event(format!("{kind} taken"));
                    m = next;
                }
                Err(_) => tc.event(format!("{kind} refused")),
            }
            edits.push(e);
        }
        self.attempt(tc, &edits)
    }
}

// ---------------------------------------------------------------------------------------------
// Drawing edits
// ---------------------------------------------------------------------------------------------

/// Names worth trying: case-only differences, Unicode (CJK, emoji, combining marks, characters
/// whose case mapping changes length), whitespace-only and empty, slashes, control characters.
const NAMES: &[&str] = &[
    "Friday",
    "friday",
    "FRIDAY",
    "Warmup",
    "aab",
    "AAB",
    "Shows",
    "日本語のプレイリスト",
    "🎧 Warmup 🔥",
    "Ünïcödé",
    "Straße",
    "STRASSE",
    "İstanbul",
    "e\u{301}",
    "é",
    "  padded  ",
    "a/b",
    "/",
    "x//y",
    " ",
    "",
    "\u{3000}",
    "\u{200b}",
    "a\tb",
    "nul\0x",
    "HISTORY 001",
    "Contents",
    ".",
    "..",
];

/// Strings for track details: names, plus ones that differ from the drive's only in case.
const DETAILS: &[&str] =
    &["[A]pendics.Shuffle", "[a]pendics.shuffle", "Techno", "TECHNO", "Framed And Defamed", "Gm", " ", "", "日本語"];

/// How long drawn names get.
#[derive(Clone, Copy, PartialEq)]
enum Names {
    /// Up to a few hundred bytes: always fits a database row.
    Fitting,
    /// Some too long for one export.pdb row.
    Oversized,
}

fn draw_name(tc: &TestCase, names: Names) -> String {
    // Around a 4 KiB page: as ASCII, and as UTF-16 (two bytes a character).
    if names == Names::Oversized && tc.draw(gs::booleans()) {
        let (c, min) = if tc.draw(gs::booleans()) { ("x", 3950) } else { ("é", 1975) };
        return c.repeat(tc.draw(gs::integers::<usize>().min_value(min).max_value(min + 150)));
    }
    match tc.draw(gs::integers::<u8>().max_value(9)) {
        0..=5 => tc.draw(gs::sampled_from(NAMES)).to_string(),
        6 | 7 => tc.draw(gs::text().max_size(6)),
        // Past rekordbox's short-string limit (126 ASCII characters), and long UTF-16.
        8 => "x".repeat(tc.draw(gs::integers::<usize>().min_value(120).max_value(140))),
        _ => "é".repeat(tc.draw(gs::integers::<usize>().min_value(60).max_value(200))),
    }
}

/// A name for a new or renamed playlist: often one already in the tree, to collide.
fn draw_node_name(tc: &TestCase, m: &Model, names: Names) -> String {
    let all = m.shape.walk();
    if all.is_empty() || !tc.draw(gs::weighted_booleans(0.3)) {
        return draw_name(tc, names);
    }
    m.shape.node(&tc.draw(gs::sampled_from(all)).0).name.clone()
}

fn draw_text(tc: &TestCase, names: Names) -> String {
    match tc.draw(gs::integers::<u8>().max_value(2)) {
        0 => tc.draw(gs::sampled_from(DETAILS)).to_string(),
        _ => draw_name(tc, names),
    }
}

fn swap_ascii_case(s: &str) -> String {
    s.chars().map(|c| if c.is_ascii_uppercase() { c.to_ascii_lowercase() } else { c.to_ascii_uppercase() }).collect()
}

/// A way to name a playlist: mostly an existing one's path, sometimes in other ASCII case, by
/// its bare name, with slashes around it, or something that may not exist.
fn draw_spec(tc: &TestCase, m: &Model, folders_only: bool, names: Names) -> String {
    let all: Vec<(Ix, String)> =
        m.shape.walk().into_iter().filter(|(ix, _)| !folders_only || m.shape.node(ix).folder).collect();
    if all.is_empty() || tc.draw(gs::integers::<u8>().max_value(9)) == 0 {
        return draw_name(tc, names);
    }
    let (ix, path) = &tc.draw(gs::sampled_from(all));
    match tc.draw(gs::integers::<u8>().max_value(9)) {
        0..=6 => path.clone(),
        7 => swap_ascii_case(path),
        8 => m.shape.node(ix).name.clone(),
        _ => format!("/{path}/"),
    }
}

fn draw_tracks(tc: &TestCase, pool: &[String], max: usize) -> TrackSelector {
    let n = tc.draw(gs::integers::<usize>().min_value(1).max_value(max));
    let paths = (0..n)
        .map(|_| {
            if tc.draw(gs::weighted_booleans(0.05)) {
                return "/Contents/Nobody/Nothing.mp3".to_string();
            }
            let p = tc.draw(gs::sampled_from(pool));
            if tc.draw(gs::weighted_booleans(0.1)) { p.to_uppercase() } else { p }
        })
        .collect::<Vec<_>>();
    TrackSelector::paths(paths)
}

fn draw_positions(tc: &TestCase) -> Vec<usize> {
    tc.draw(gs::vecs(gs::integers::<usize>().max_value(14)).max_size(3))
}

fn maybe(tc: &TestCase) -> bool {
    tc.draw(gs::weighted_booleans(0.25))
}

fn draw_fields(tc: &TestCase, names: Names) -> TrackFields {
    let mut f = TrackFields::default();
    for field in [&mut f.title, &mut f.comment, &mut f.mix_name] {
        if maybe(tc) {
            *field = Some(draw_text(tc, names));
        }
    }
    let fitting = Names::Fitting;
    for field in [
        &mut f.artist,
        &mut f.album,
        &mut f.genre,
        &mut f.label,
        &mut f.remixer,
        &mut f.original_artist,
        &mut f.composer,
    ] {
        if maybe(tc) {
            *field = Some(draw_text(tc, fitting));
        }
    }
    if maybe(tc) {
        let keys = ["8A", "12b", "Am", "F#m", "Bbm", "1m", "", " ", "13A", "H"];
        f.key = Some(tc.draw(gs::sampled_from(&keys[..])).to_string());
    }
    if maybe(tc) {
        f.rating = Some(tc.draw(gs::integers::<u8>().max_value(6)));
    }
    if maybe(tc) {
        let colors = ["aqua", "Red", "none", "0", "3", "8", "9", "mauve", " pink "];
        f.color = Some(tc.draw(gs::sampled_from(&colors[..])).to_string());
    }
    if maybe(tc) {
        f.year = Some(tc.draw(gs::integers::<u16>()));
    }
    if maybe(tc) {
        f.play_count = Some(tc.draw(gs::integers::<u16>()));
    }
    if maybe(tc) {
        f.track_number = Some(tc.draw(gs::integers::<u32>()));
    }
    if maybe(tc) {
        f.disc_number = Some(tc.draw(gs::integers::<u16>()));
    }
    f
}

fn draw_edit(tc: &TestCase, m: &Model, pool: &[String], names: Names) -> Edit {
    let spec = |tc: &TestCase| draw_spec(tc, m, false, names);
    let edit = match tc.draw(gs::integers::<u8>().max_value(19)) {
        0..=3 => {
            let parent = if tc.draw(gs::booleans()) { draw_spec(tc, m, true, names) } else { String::new() };
            let depth = tc.draw(gs::integers::<usize>().min_value(1).max_value(2));
            let mut parts = vec![parent];
            parts.extend((0..depth).map(|_| draw_node_name(tc, m, names)));
            Edit::CreatePlaylist { path: parts.join("/"), folder: tc.draw(gs::booleans()) }
        }
        4 => Edit::RenamePlaylist { playlist: spec(tc), name: draw_node_name(tc, m, names) },
        5 => Edit::MovePlaylist {
            playlist: spec(tc),
            into: tc.draw(gs::booleans()).then(|| draw_spec(tc, m, true, names)),
            position: tc.draw(gs::optional(gs::integers::<usize>().max_value(4))),
        },
        6 => Edit::DeletePlaylist { playlist: spec(tc), with_tracks: tc.draw(gs::booleans()) },
        7..=9 => Edit::AddTracks {
            playlist: spec(tc),
            tracks: draw_tracks(tc, pool, 4),
            position: tc.draw(gs::optional(gs::integers::<usize>().max_value(14))),
            sort: None,
            allow_repeats: tc.draw(gs::booleans()),
        },
        10 => Edit::RemoveTracks {
            playlist: spec(tc),
            tracks: tc.draw(gs::booleans()).then(|| draw_tracks(tc, pool, 2)),
            positions: draw_positions(tc),
        },
        11 => Edit::MoveTracks {
            playlist: spec(tc),
            positions: draw_positions(tc),
            to: tc.draw(gs::integers::<usize>().max_value(14)),
        },
        12 => Edit::SetTracks {
            playlist: spec(tc),
            tracks: draw_tracks(tc, pool, 4),
            sort: None,
            limit: tc.draw(gs::optional(gs::integers::<usize>().max_value(3))),
            create: tc.draw(gs::booleans()),
        },
        13 => Edit::Dedupe { playlist: spec(tc), recordings: false },
        14 | 15 => Edit::EditTracks { tracks: draw_tracks(tc, pool, 2), fields: draw_fields(tc, names) },
        16 | 17 => Edit::DeleteTracks { tracks: draw_tracks(tc, pool, 2), keep_files: tc.draw(gs::booleans()) },
        18 => Edit::ReplaceTrack {
            duplicate: draw_tracks(tc, pool, 1),
            keep: draw_tracks(tc, pool, 1),
            keep_duplicate: tc.draw(gs::booleans()),
        },
        _ => {
            let lists = ["HISTORY 001", "history 002", " History 001 ", "HISTORY 003", ""];
            Edit::DeleteHistory {
                names: tc
                    .draw(gs::vecs(gs::sampled_from(&lists[..])).max_size(2))
                    .into_iter()
                    .map(String::from)
                    .collect(),
                all: tc.draw(gs::weighted_booleans(0.2)),
            }
        }
    };
    without_case_twins(m, edit)
}

/// Keep edits from giving a playlist a sibling whose name differs only in ASCII case: how those
/// are told apart is its own property (see `names_that_differ_only_in_case_name_different_playlists`).
fn without_case_twins(m: &Model, edit: Edit) -> Edit {
    let twin = |kids: &[Node], name: &str, skip: Option<usize>| {
        kids.iter().enumerate().any(|(j, n)| Some(j) != skip && n.name != name && n.name.eq_ignore_ascii_case(name))
    };
    let unique = |kids: &[Node], name: &str, skip: Option<usize>| {
        let mut name = name.to_string();
        while twin(kids, &name, skip) {
            name.push('~');
        }
        name
    };
    // A path whose missing parts get created: rename the parts that would be twins.
    let fix_path = |path: &str| -> String {
        let mut parent = Some(Vec::new());
        let mut changed = false;
        let mut out = Vec::new();
        for name in split_path(path) {
            let name = match &parent {
                Some(p) => {
                    let kids = m.shape.kids(p);
                    let fixed = unique(kids, &name, None);
                    changed |= fixed != name;
                    parent =
                        kids.iter().position(|n| n.name == fixed && n.folder).map(|j| [p.clone(), vec![j]].concat());
                    fixed
                }
                None => name,
            };
            out.push(name);
        }
        if changed { out.join("/") } else { path.to_string() }
    };
    match edit {
        Edit::CreatePlaylist { path, folder } => Edit::CreatePlaylist { path: fix_path(&path), folder },
        Edit::SetTracks { playlist, tracks, sort, limit, create: true } if m.shape.find_exact(&playlist).is_none() => {
            Edit::SetTracks { playlist: fix_path(&playlist), tracks, sort, limit, create: true }
        }
        Edit::RenamePlaylist { playlist, name } => match m.shape.find(&playlist) {
            Some(ix) => {
                let (last, parent) = ix.split_last().unwrap();
                let kids = m.shape.kids(parent);
                let name =
                    if twin(kids, name.trim(), Some(*last)) { unique(kids, name.trim(), Some(*last)) } else { name };
                Edit::RenamePlaylist { playlist, name }
            }
            None => Edit::RenamePlaylist { playlist, name },
        },
        Edit::MovePlaylist { playlist, into, position } => {
            let Some(ix) = m.shape.find(&playlist) else { return Edit::MovePlaylist { playlist, into, position } };
            let parent = match into.as_deref().map(str::trim).filter(|s| !s.is_empty() && *s != "/") {
                None => Some(Vec::new()),
                Some(t) => m.shape.find_kind(t, Some(true)),
            };
            if let Some(parent) = parent
                && twin(m.shape.kids(&parent), &m.shape.node(&ix).name, None)
            {
                // Reorder it where it is instead.
                let old_parent = &ix[..ix.len() - 1];
                let into = (!old_parent.is_empty()).then(|| m.shape.path(old_parent));
                return Edit::MovePlaylist { playlist, into, position };
            }
            Edit::MovePlaylist { playlist, into, position }
        }
        other => other,
    }
}

impl Shape {
    fn path(&self, ix: &[usize]) -> String {
        (1..=ix.len()).map(|i| self.node(&ix[..i]).name.clone()).collect::<Vec<_>>().join("/")
    }
}

// ---------------------------------------------------------------------------------------------
// Properties
// ---------------------------------------------------------------------------------------------

struct EditMachine {
    drive: Drive,
}

#[hegel::state_machine]
impl EditMachine {
    /// A batch the model takes is planned and applied, and the drive then holds what the model
    /// says (both databases, rekordcrate, audio files); a batch the model refuses is refused.
    #[rule(weight = 4)]
    fn edit(&mut self, tc: TestCase) {
        if let Attempt::Failed { error, untouched } = self.drive.edit(&tc, Names::Fitting) {
            panic!("plan took the edits but apply failed: {error} (drive left alone: {untouched:?})");
        }
    }

    /// Restoring the last backup gives back the drive's files byte for byte.
    #[rule]
    fn undo(&mut self, tc: TestCase) {
        if self.drive.undo() {
            tc.event("undone");
        }
    }
}

fn run_machine(tc: TestCase, plus: bool) {
    hegel::stateful::machine(EditMachine { drive: Drive::new(plus) }).steps(12).run(tc);
}

/// Edits on a drive with export.pdb only.
#[hegel::test(test_cases = 20, suppress_health_check = [HealthCheck::TooSlow])]
fn edits_on_an_export_pdb_drive_match_the_model(tc: TestCase) {
    run_machine(tc, false);
}

/// Edits on a drive with export.pdb and OneLibrary: both are edited alike.
#[hegel::test(test_cases = 12, suppress_health_check = [HealthCheck::TooSlow])]
fn edits_on_a_drive_with_both_databases_match_the_model(tc: TestCase) {
    run_machine(tc, true);
}

/// Whether any name or detail in `edits` is long enough that export.pdb may not hold it.
fn oversized(edits: &[Edit]) -> bool {
    let long = |s: &str| s.encode_utf16().count() * 2 > 2000;
    edits.iter().any(|e| match e {
        Edit::CreatePlaylist { path, .. } => long(path),
        Edit::RenamePlaylist { name, .. } => long(name),
        Edit::SetTracks { playlist, .. } => long(playlist),
        Edit::EditTracks { fields, .. } => {
            [&fields.title, &fields.comment, &fields.mix_name].iter().any(|v| v.as_deref().is_some_and(long))
        }
        _ => false,
    })
}

/// Edits with names and details around what one export.pdb row holds: an apply that fails anyway
/// (it used to, on the ones too long, before plan refused them) changes no file, leaves no backup
/// behind and doesn't mark the drive as interrupted; everything else is applied as the model says.
#[hegel::test(test_cases = 15, suppress_health_check = [HealthCheck::TooSlow])]
fn a_failed_apply_leaves_the_drive_untouched(tc: TestCase) {
    let mut drive = Drive::new(tc.draw(gs::booleans()));
    for _ in 0..tc.draw(gs::integers::<usize>().min_value(1).max_value(3)) {
        let n = tc.draw(gs::integers::<usize>().min_value(1).max_value(2));
        let edits: Vec<Edit> = (0..n).map(|_| draw_edit(&tc, &drive.model, &drive.pool, Names::Oversized)).collect();
        if let Attempt::Failed { error, untouched } = drive.attempt(&tc, &edits) {
            assert!(oversized(&edits), "apply failed on edits that fit: {error}");
            if let Err(e) = untouched {
                panic!("apply failed ({error}) and {e}");
            }
            tc.event("apply failed cleanly");
            drive.check();
        }
    }
}

/// Everything `plan` takes, `apply` can make: names and details too long for export.pdb are
/// refused when planning.
#[hegel::test(test_cases = 10, suppress_health_check = [HealthCheck::TooSlow])]
fn plan_refuses_what_export_pdb_cannot_hold(tc: TestCase) {
    let mut drive = Drive::new(false);
    for _ in 0..tc.draw(gs::integers::<usize>().min_value(1).max_value(3)) {
        if let Attempt::Failed { error, .. } = drive.edit(&tc, Names::Oversized) {
            panic!("plan took edits apply can't make: {error}");
        }
    }
}

/// A name or detail around what one export.pdb row holds is planned exactly when it fits, and then
/// applied: a playlist's or folder's name, a new artist, album, genre or label, and a track's title,
/// comment and mix name, alone or together. Refused, it says what's too long and the drive is as it
/// was; a lookup name one character longer than one refused is refused too.
#[hegel::test(test_cases = 8, suppress_health_check = [HealthCheck::TooSlow])]
fn names_and_details_are_planned_exactly_when_export_pdb_holds_them(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let root = fresh_pdb_drive(dir.path());
    let lib = pdb::read_library(&root.join(paths::EXPORT_PDB)).unwrap();
    let track = tc.draw(gs::sampled_from(&lib.tracks));
    // Around a row's 4,050 bytes: as ASCII, or as UTF-16 (two bytes a character).
    let (c, width) = tc.draw(gs::sampled_from(vec![("x", 1), ("é", 2)]));
    let around =
        |bytes: usize| c.repeat(tc.draw(gs::integers::<usize>().min_value(bytes - 12).max_value(bytes + 12)) / width);
    let plan = |edit: &Edit| {
        edit::plan(
            &Device::open_export(&root).unwrap(),
            std::slice::from_ref(edit),
            &EditOptions::default(),
            &Ctx::silent(),
        )
    };
    let fields =
        |f: TrackFields| Edit::EditTracks { tracks: TrackSelector::paths([track.file_path.clone()]), fields: f };
    for kind in 0..4 {
        let (edit, fits) = match kind {
            0 => {
                let name = around(4030);
                (
                    Edit::CreatePlaylist { path: format!("Shows/{name}"), folder: tc.draw(gs::booleans()) },
                    Some(name_fits(&name)),
                )
            }
            1 => {
                let name = around(4030);
                (Edit::RenamePlaylist { playlist: "aab".into(), name: name.clone() }, Some(name_fits(&name)))
            }
            2 => {
                let name = around(4030);
                let mut f = TrackFields::default();
                match tc.draw(gs::integers::<u8>().max_value(6)) {
                    0 => f.genre = Some(name),
                    1 => f.label = Some(name),
                    2 => f.artist = Some(name),
                    3 => f.album = Some(name),
                    4 => f.remixer = Some(name),
                    5 => f.composer = Some(name),
                    _ => f.original_artist = Some(name),
                }
                (fields(f), None)
            }
            _ => {
                let others = track_rows(&root)[&paths::file_key(&track.file_path)];
                // One, two or all three of the strings sharing the track's row, together near the limit.
                let mut f = TrackFields::default();
                let budget = 4050 - others;
                let set = [tc.draw(gs::booleans()), tc.draw(gs::booleans()), true];
                let short = || c.repeat(tc.draw(gs::integers::<usize>().max_value(40)));
                let mut d = Details::from(&track);
                for (i, on) in set.into_iter().enumerate() {
                    if !on {
                        continue;
                    }
                    let v = if i == 2 { around(budget) } else { short() };
                    match i {
                        0 => (f.comment, d.comment) = (Some(v.clone()), v),
                        1 => (f.mix_name, d.mix_name) = (Some(v.clone()), v),
                        _ => (f.title, d.title) = (Some(v.clone()), v),
                    }
                }
                (fields(f), Some(details_fit(others, &d)))
            }
        };
        let before = Before::take(&root);
        match plan(&edit) {
            Ok(p) => {
                assert_ne!(fits, Some(false), "plan took {edit:?}, which export.pdb can't hold");
                edit::apply(&p, &mut Device::open_export(&root).unwrap(), &Ctx::silent()).unwrap();
                if let Edit::EditTracks { fields: f, .. } = &edit {
                    let after = pdb::read_library(&root.join(paths::EXPORT_PDB)).unwrap();
                    let t = after.tracks.iter().find(|t| t.file_path == track.file_path).unwrap();
                    for (want, got) in
                        [(&f.genre, &t.genre), (&f.artist, &t.artist), (&f.title, &t.title), (&f.comment, &t.comment)]
                    {
                        if let Some(want) = want {
                            assert_eq!(got, want);
                        }
                    }
                }
            }
            Err(err) => {
                assert_ne!(fits, Some(true), "plan refused {edit:?}, which fits: {err}");
                assert!(
                    matches!(err, locallibrary::Error::Invalid(_)) && err.to_string().contains("too long"),
                    "{err}"
                );
                before.untouched(&root).unwrap();
                // Longer still is refused too.
                if let Edit::EditTracks { fields: f, .. } = &edit
                    && fits.is_none()
                {
                    let longer = |v: &Option<String>| v.as_ref().map(|v| format!("{v}{c}"));
                    let f = TrackFields {
                        genre: longer(&f.genre),
                        label: longer(&f.label),
                        artist: longer(&f.artist),
                        album: longer(&f.album),
                        remixer: longer(&f.remixer),
                        composer: longer(&f.composer),
                        original_artist: longer(&f.original_artist),
                        ..Default::default()
                    };
                    assert!(plan(&fields(f)).is_err());
                }
            }
        }
    }
}

/// rekordbox exports again without some of the tracks, playlists and history lists the edits used:
/// replaying the journal never fails, makes what it still can, and brings none of the missing
/// tracks back; what it leaves out it says.
#[hegel::test(test_cases = 12, suppress_health_check = [HealthCheck::TooSlow])]
fn a_replay_on_an_export_missing_what_the_edits_used_skips_it(tc: TestCase) {
    let mut drive = Drive::new(false);
    let pdb_path = drive.root.join(paths::EXPORT_PDB);
    let exported = fs::read(&pdb_path).unwrap();
    for _ in 0..tc.draw(gs::integers::<usize>().min_value(3).max_value(8)) {
        if let Attempt::Failed { error, .. } = drive.edit(&tc, Names::Fitting) {
            panic!("plan took the edits but apply failed: {error}");
        }
    }
    let mut ed = rekordbox_pdb::PdbEditor::from_bytes(exported);
    let db = ed.database().unwrap();
    let ids: Vec<u32> = db.tracks.iter().map(|t| t.id).collect();
    let gone: Vec<u32> = crate::drawn::some(&tc, &ids).into_iter().take(3).collect();
    let gone_files: BTreeSet<String> =
        db.tracks.iter().filter(|t| gone.contains(&t.id)).map(|t| paths::file_key(t.file_path())).collect();
    let lists: Vec<u32> = db.playlist_tree.iter().map(|n| n.id).collect();
    let history: Vec<u32> = db.history_playlists.iter().map(|h| h.id).collect();
    ed.remove_tracks(&gone).unwrap();
    ed.remove_playlists(&crate::drawn::some(&tc, &lists)).unwrap();
    ed.remove_history_playlists(&crate::drawn::some(&tc, &history)).unwrap();
    fs::write(&pdb_path, ed.to_bytes()).unwrap();
    let exported = pdb::read_library(&pdb_path).unwrap();

    let mut d = Device::open_export(&drive.root).unwrap();
    let edits = edit::journal::edits(&d).unwrap();
    let plan = edit::plan(&d, &edits, &EditOptions { lenient: true, no_journal: true }, &Ctx::silent()).unwrap();
    tc.note(&format!("journal: {edits:?}\nwarnings: {:?}", plan.warnings));
    edit::apply(&plan, &mut d, &Ctx::silent()).unwrap();
    let after = d.library().unwrap();
    let files: BTreeSet<String> = after.tracks.iter().map(|t| paths::file_key(&t.file_path)).collect();
    assert!(files.is_disjoint(&gone_files), "a missing track came back");
    let before: BTreeSet<String> = exported.tracks.iter().map(|t| paths::file_key(&t.file_path)).collect();
    assert!(files.is_subset(&before), "a replay adds no tracks");
    // Each playlist that's left is one the export or the edits made.
    let made: BTreeSet<String> = drive.model.shape.walk().into_iter().map(|(_, path)| path).collect();
    for p in &after.playlists {
        let path = after.playlist_path(p.id);
        assert!(made.contains(&path) || exported.find_playlist(&path).is_some(), "{path} came from nowhere");
    }
}

/// After rekordbox exports over the edits (both databases as they were), replaying the journal
/// gives the library the edits made.
#[hegel::test(test_cases = 40, suppress_health_check = [HealthCheck::TooSlow])]
fn replaying_the_journal_after_a_new_export_redoes_the_edits(tc: TestCase) {
    let mut drive = Drive::new(tc.draw(gs::booleans()));
    let exported: Vec<(&str, Vec<u8>)> = [paths::EXPORT_PDB, paths::LIBRARY_PLUS_DB]
        .into_iter()
        .filter_map(|k| fs::read(drive.root.join(k)).ok().map(|b| (k, b)))
        .collect();
    for _ in 0..tc.draw(gs::integers::<usize>().min_value(3).max_value(8)) {
        if let Attempt::Failed { error, .. } = drive.edit(&tc, Names::Fitting) {
            panic!("plan took the edits but apply failed: {error}");
        }
    }
    for (key, bytes) in &exported {
        fs::write(drive.root.join(key), bytes).unwrap();
    }
    let mut d = Device::open_export(&drive.root).unwrap();
    let edits = edit::journal::edits(&d).unwrap();
    tc.note(&format!("journal: {edits:?}"));
    if edits.is_empty() {
        return;
    }
    let plan = edit::plan(&d, &edits, &EditOptions { lenient: true, no_journal: true }, &Ctx::silent()).unwrap();
    tc.note(&format!("replay warnings: {:?}", plan.warnings));
    edit::apply(&plan, &mut d, &Ctx::silent()).unwrap();
    assert_shape(&shape(&d.library().unwrap()), &drive.model.shape, "export.pdb after the replay");
    if drive.plus {
        assert_shape(
            &shape(&common::plus_library(&drive.root)),
            &drive.model.shape,
            "exportLibrary.db after the replay",
        );
    }
}

/// Two playlists whose names differ only in ASCII case can sit side by side (creating, renaming
/// and moving compare names exactly), so an edit naming one by its exact path changes that one.
#[hegel::test(test_cases = 10, suppress_health_check = [HealthCheck::TooSlow])]
fn names_that_differ_only_in_case_name_different_playlists(tc: TestCase) {
    let drive = Drive::new(tc.draw(gs::booleans()));
    let base = tc.draw(gs::sampled_from(&["Foo", "friday", "Set A", "aab"][..])).to_string();
    let flips = tc.draw(gs::vecs(gs::booleans()).min_size(base.len()).max_size(base.len()));
    let twin: String = base
        .chars()
        .zip(&flips)
        .map(|(c, flip)| if *flip { swap_ascii_case(&c.to_string()).chars().next().unwrap() } else { c })
        .collect();
    tc.assume(twin != base);
    let parent = if tc.draw(gs::booleans()) { "Shows/" } else { "" };
    let (first, second) = (format!("{parent}{base}"), format!("{parent}{twin}"));
    let run = |edits: Vec<Edit>| -> edit::EditReport {
        let mut d = Device::open_export(&drive.root).unwrap();
        let plan = edit::plan(&d, &edits, &EditOptions::default(), &Ctx::silent()).unwrap();
        edit::apply(&plan, &mut d, &Ctx::silent()).unwrap()
    };
    run(vec![Edit::CreatePlaylist { path: first.clone(), folder: false }]);
    let mut d = Device::open_export(&drive.root).unwrap();
    let create = [Edit::CreatePlaylist { path: second.clone(), folder: false }];
    if edit::plan(&d, &create, &EditOptions::default(), &Ctx::silent()).is_err() {
        return; // Refusing the twin is consistent too.
    }
    let plan = edit::plan(&d, &create, &EditOptions::default(), &Ctx::silent()).unwrap();
    edit::apply(&plan, &mut d, &Ctx::silent()).unwrap();
    let entries = |path: &str| {
        let lib = Device::open_export(&drive.root).unwrap().library().unwrap();
        let p = lib.playlists.iter().find(|p| lib.playlist_path(p.id) == path).unwrap_or_else(|| panic!("no {path}"));
        p.track_ids.len()
    };
    let (a, b) = (entries(&first), entries(&second));
    let track = tc.draw(gs::sampled_from(&drive.pool));
    run(vec![Edit::AddTracks {
        playlist: second.clone(),
        tracks: TrackSelector::paths([track]),
        position: None,
        sort: None,
        allow_repeats: true,
    }]);
    assert_eq!((entries(&first), entries(&second)), (a, b + 1), "adding to {second:?}");
}

/// Regression: an apply that fails after planning left the drive marked as interrupted and a
/// backup behind. Here export.pdb has no playlist table: it reads as having no playlists, and its
/// editor can't add one.
#[test]
fn an_apply_failing_after_planning_leaves_the_drive_alone() {
    let dir = tempfile::tempdir().unwrap();
    let root = fresh_pdb_drive(dir.path());
    // The table directory: the number of tables at 8, then 16 bytes each from 0x1c, type first.
    let path = root.join(paths::EXPORT_PDB);
    let mut bytes = fs::read(&path).unwrap();
    let tables = u32::from_le_bytes(bytes[8..12].try_into().unwrap()) as usize;
    let tree = (0..tables).map(|i| 0x1c + i * 16).find(|&o| bytes[o..o + 4] == [7, 0, 0, 0]).unwrap();
    bytes[tree] = 0xff;
    fs::write(&path, bytes).unwrap();
    let before = Before::take(&root);
    let mut d = Device::open_export(&root).unwrap();
    let edits = [Edit::CreatePlaylist { path: "Friday".into(), folder: false }];
    let plan = edit::plan(&d, &edits, &EditOptions::default(), &Ctx::silent()).unwrap();
    edit::apply(&plan, &mut d, &Ctx::silent()).unwrap_err();
    before.untouched(&root).unwrap();
}

/// Regression: plan took names and details too long for one export.pdb row, and apply then failed
/// on them. The longest that fit are planned and applied, and one character more is refused.
#[test]
fn plan_refuses_names_and_details_one_character_too_long_for_export_pdb() {
    // Rows get 4,050 bytes. A playlist's is 20 and the name, so 4,024 ASCII characters fit, or
    // 2,012 as UTF-16; a genre's is 4 and the name: 4,040 or 2,020.
    for (c, name_max, genre_max) in [("x", 4024, 4040), ("é", 2012, 2020)] {
        let dir = tempfile::tempdir().unwrap();
        let root = fresh_pdb_drive(dir.path());
        let track = pdb::read_library(&root.join(paths::EXPORT_PDB)).unwrap().tracks[0].clone();
        let others = track_rows(&root)[&paths::file_key(&track.file_path)];
        let title_fits = |n: usize| details_fit(others, &Details { title: c.repeat(n), ..Details::from(&track) });
        let title_max = (0..).take_while(|n| title_fits(*n)).last().unwrap();
        let fields = |title: Option<usize>, genre: Option<usize>| Edit::EditTracks {
            tracks: TrackSelector::paths([track.file_path.clone()]),
            fields: TrackFields {
                title: title.map(|n| c.repeat(n)),
                genre: genre.map(|n| c.repeat(n)),
                ..Default::default()
            },
        };
        let plan = |edits: &[Edit]| {
            edit::plan(&Device::open_export(&root).unwrap(), edits, &EditOptions::default(), &Ctx::silent())
        };
        let before = Before::take(&root);
        for (what, edit) in [
            ("new playlist", Edit::CreatePlaylist { path: c.repeat(name_max + 1), folder: false }),
            ("rename", Edit::RenamePlaylist { playlist: "aab".into(), name: c.repeat(name_max + 1) }),
            ("title", fields(Some(title_max + 1), None)),
            ("genre", fields(None, Some(genre_max + 1))),
        ] {
            let Err(err) = plan(&[edit]) else { panic!("plan took a {what} of {c:?} one character too long") };
            assert!(matches!(err, locallibrary::Error::Invalid(_)) && err.to_string().contains("too long"), "{err}");
        }
        before.untouched(&root).unwrap();
        let edits = [
            Edit::CreatePlaylist { path: format!("Shows/{}", c.repeat(name_max)), folder: false },
            Edit::RenamePlaylist { playlist: "aab".into(), name: c.repeat(name_max) },
            fields(Some(title_max), Some(genre_max)),
        ];
        edit::apply(&plan(&edits).unwrap(), &mut Device::open_export(&root).unwrap(), &Ctx::silent()).unwrap();
    }
}

/// Regression: a whitespace-only composer, original artist or album was kept as given in the
/// edit's model while both databases store no name, so apply failed verification.
#[test]
fn whitespace_only_names_clear_composer_original_artist_and_album() {
    let dir = tempfile::tempdir().unwrap();
    let root = fresh_pdb_drive(dir.path());
    fs::write(root.join(paths::LIBRARY_PLUS_DB), plus_db()).unwrap();
    let track = pdb::read_library(&root.join(paths::EXPORT_PDB)).unwrap().tracks[0].file_path.clone();
    let mut d = Device::open_export(&root).unwrap();
    let blank = Some(" ".to_string());
    let fields =
        TrackFields { composer: blank.clone(), original_artist: blank.clone(), album: blank, ..Default::default() };
    let edits = [Edit::EditTracks { tracks: TrackSelector::paths([track.clone()]), fields }];
    let plan = edit::plan(&d, &edits, &EditOptions::default(), &Ctx::silent()).unwrap();
    edit::apply(&plan, &mut d, &Ctx::silent()).unwrap();
    for lib in [d.library().unwrap(), common::plus_library(&root)] {
        let t = lib.tracks.iter().find(|t| t.file_path == track).unwrap();
        assert_eq!((t.composer.as_str(), t.original_artist.as_str(), t.album.as_str()), ("", "", ""));
    }
}

/// Regression: in one batch, deleting a track with its files and another keeping them kept both
/// tracks' files (`keep_files` applied to the whole batch).
#[test]
fn each_delete_in_a_batch_keeps_or_removes_its_own_files() {
    let dir = tempfile::tempdir().unwrap();
    let root = fresh_pdb_drive(dir.path());
    let lib = pdb::read_library(&root.join(paths::EXPORT_PDB)).unwrap();
    let (gone, kept) = (lib.tracks[5].file_path.clone(), lib.tracks[6].file_path.clone());
    let mut d = Device::open_export(&root).unwrap();
    let edits = [
        Edit::DeleteTracks { tracks: TrackSelector::paths([gone.clone()]), keep_files: false },
        Edit::DeleteTracks { tracks: TrackSelector::paths([kept.clone()]), keep_files: true },
    ];
    let plan = edit::plan(&d, &edits, &EditOptions::default(), &Ctx::silent()).unwrap();
    edit::apply(&plan, &mut d, &Ctx::silent()).unwrap();
    assert!(!paths::to_fs(&root, &paths::key(&gone)).exists(), "{gone} is deleted");
    assert!(paths::to_fs(&root, &paths::key(&kept)).exists(), "{kept} stays");
}

/// Regression: moving a playlist next to one of the same name was allowed (creating and renaming
/// refuse that), leaving two playlists with one path that later edits can't tell apart.
#[test]
fn moving_a_playlist_next_to_a_namesake_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let root = fresh_pdb_drive(dir.path());
    let mut d = Device::open_export(&root).unwrap();
    let create = [Edit::CreatePlaylist { path: "Straße/aab".into(), folder: false }];
    let plan = edit::plan(&d, &create, &EditOptions::default(), &Ctx::silent()).unwrap();
    edit::apply(&plan, &mut d, &Ctx::silent()).unwrap();
    let before = Before::take(&root);
    let mv = [Edit::MovePlaylist { playlist: "Straße/aab".into(), into: None, position: None }];
    let err = edit::plan(&d, &mv, &EditOptions::default(), &Ctx::silent()).unwrap_err();
    assert!(matches!(err.inner(), locallibrary::Error::Conflict(_)), "{err}");
    before.untouched(&root).unwrap();
}

/// Plan and apply one edit replacing the track `duplicate` with `keep` (by file), keeping the
/// duplicate. Returns the files of the drive's tracks, as export.pdb lists them.
fn replace_keeping(root: &Path, duplicate: usize, keep: usize) -> Vec<String> {
    let files: Vec<String> =
        pdb::read_library(&root.join(paths::EXPORT_PDB)).unwrap().tracks.into_iter().map(|t| t.file_path).collect();
    apply(
        root,
        &[Edit::ReplaceTrack {
            duplicate: TrackSelector::paths([files[duplicate].clone()]),
            keep: TrackSelector::paths([files[keep].clone()]),
            keep_duplicate: true,
        }],
    );
    files
}

/// Regression: replacing a track only history lists held (its playlist was deleted) was planned
/// as changing nothing, so apply skipped it and the lists kept the duplicate.
#[test]
fn replacing_a_track_only_history_lists_hold_repoints_them() {
    let dir = tempfile::tempdir().unwrap();
    let root = fresh_pdb_drive(dir.path());
    fs::write(root.join(paths::LIBRARY_PLUS_DB), plus_db()).unwrap();
    apply(&root, &[Edit::DeletePlaylist { playlist: "aab".into(), with_tracks: false }]);
    let files = replace_keeping(&root, 0, 1);
    let d = Device::open_export(&root).unwrap();
    let (two, three) = (paths::file_key(&files[1]), paths::file_key(&files[2]));
    let expected = vec![
        ("HISTORY 001".to_string(), vec![two.clone(), two.clone()]),
        ("History 002".to_string(), vec![three, two]),
    ];
    for (what, lib) in [("export.pdb", d.library().unwrap()), ("exportLibrary.db", common::plus_library(&root))] {
        assert_eq!(shape(&lib).history, expected, "{what}'s history lists");
    }
}

/// Regression: the same for a track only a My Tag was on: the tag didn't reach the keeper.
#[test]
fn replacing_a_track_only_my_tags_hold_tags_the_keeper() {
    use locallibrary::tags;
    let dir = tempfile::tempdir().unwrap();
    let root = common::demo_drive(dir.path(), "Demo");
    let mut d = Device::open_export(&root).unwrap();
    let plan = tags::plan(&d, &tags::Edit::Add { tag: "Genre/Deep".into(), tracks: vec![2] }).unwrap();
    tags::apply(&plan, &mut d, &Ctx::silent()).unwrap();
    replace_keeping(&root, 1, 0);
    let ext = rekordbox_pdb::ExtDatabase::from_file(root.join(paths::EXPORT_EXT_PDB)).unwrap();
    let mut tagged: Vec<u32> = ext.tag_tracks.iter().map(|l| l.track_id).collect();
    tagged.sort_unstable();
    assert_eq!(tagged, [1, 2]);
}

/// Make `edits` on the drive at `root`, let `export` stand in for rekordbox exporting over them
/// and replay the journal: the library is as the edits left it.
fn assert_replay_after(root: &Path, edits: &[Edit], export: impl FnOnce()) {
    apply(root, edits);
    let edited = shape(&Device::open_export(root).unwrap().library().unwrap());
    export();
    let (edits, lib) = replay(root);
    assert_shape(&shape(&lib), &edited, &format!("the library after replaying {edits:?}"));
}

/// Make the edits `make` gives for the drive's tracks (by file), let rekordbox export over them
/// and replay the journal: the library is as the edits left it.
fn assert_replay_redoes(make: impl Fn(&[String]) -> Vec<Edit>) {
    let dir = tempfile::tempdir().unwrap();
    let root = fresh_pdb_drive(dir.path());
    let exported = fs::read(root.join(paths::EXPORT_PDB)).unwrap();
    let files: Vec<String> =
        pdb::read_library(&root.join(paths::EXPORT_PDB)).unwrap().tracks.into_iter().map(|t| t.file_path).collect();
    assert_replay_after(&root, &make(&files), || fs::write(root.join(paths::EXPORT_PDB), exported).unwrap());
}

/// [`assert_replay_redoes`] for the edits `make` gives for the library of a OneLibrary drive
/// whose next export numbers its tracks the other way round.
fn assert_replay_on_a_renumbered_export_redoes(make: impl Fn(&Library) -> Vec<Edit>) {
    let dir = tempfile::tempdir().unwrap();
    let root = fresh_pdb_drive(dir.path());
    let mut renumbered = pdb::read_library(&root.join(paths::EXPORT_PDB)).unwrap();
    renumbered.tracks.reverse();
    common::onelibrary_only(&root);
    let lib = common::plus_library(&root);
    assert_replay_after(&root, &make(&lib), || {
        fs::write(root.join(paths::LIBRARY_PLUS_DB), common::onelibrary_for(&renumbered).to_encrypted().unwrap())
            .unwrap();
        let first = &lib.tracks[0];
        let now = common::plus_library(&root);
        assert_ne!(now.track(first.id).unwrap().file_path, first.file_path, "the export keeps the ids");
    });
}

fn add(playlist: &str, files: &[&str], allow_repeats: bool) -> Edit {
    Edit::AddTracks {
        playlist: playlist.into(),
        tracks: TrackSelector::paths(files.iter().map(|f| f.to_string())),
        position: None,
        sort: None,
        allow_repeats,
    }
}

/// Regression: moving entries was recorded as filling the playlist with its tracks by file, and
/// a replay kept a track the playlist held twice only once.
#[test]
fn replaying_a_move_keeps_a_repeated_track() {
    assert_replay_redoes(|f| {
        vec![
            Edit::CreatePlaylist { path: "Set".into(), folder: false },
            add("Set", &[&f[0], &f[1]], false),
            add("Set", &[&f[0]], true),
            Edit::MoveTracks { playlist: "Set".into(), positions: vec![3], to: 1 },
        ]
    });
}

/// Regression: removing a playlist's last entries was recorded as filling it with no tracks,
/// which a replay skipped, leaving the entries in.
#[test]
fn replaying_a_remove_can_empty_a_playlist() {
    assert_replay_redoes(|f| {
        vec![
            Edit::CreatePlaylist { path: "Set".into(), folder: false },
            add("Set", &[&f[0], &f[1]], false),
            Edit::RemoveTracks {
                playlist: "Set".into(),
                tracks: Some(TrackSelector::paths([f[1].clone()])),
                positions: vec![1],
            },
        ]
    });
}

/// Regression: filling a playlist was recorded with the selection it was given, and a replay on
/// an export numbering the tracks differently filled it with other tracks.
#[test]
fn replaying_a_fill_on_a_renumbered_export_keeps_its_tracks() {
    assert_replay_on_a_renumbered_export_redoes(|lib| {
        let fill = |playlist: &str, tracks, limit| Edit::SetTracks {
            playlist: playlist.into(),
            tracks,
            sort: None,
            limit,
            create: true,
        };
        let ids =
            |n: &[usize]| TrackSelector { ids: n.iter().map(|i| lib.tracks[*i].id).collect(), ..Default::default() };
        let query = TrackSelector { query: Some(format!("#{}", lib.tracks[3].id)), ..Default::default() };
        // A filter picks in library order, which the export changes too.
        let filter = TrackSelector { filter: Some("bpm:>0".into()), ..Default::default() };
        vec![
            fill("aab", ids(&[4, 5]), None),
            fill("Shows/By id", ids(&[0, 1, 2]), None),
            fill("By search", query, None),
            fill("First three", filter, Some(3)),
        ]
    });
}

/// Regression: ordering a set was recorded with its start track by id, and a replay on an export
/// numbering the tracks differently started it with another.
#[test]
fn replaying_a_set_plan_on_a_renumbered_export_keeps_its_order() {
    assert_replay_on_a_renumbered_export_redoes(|lib| {
        let start = |i: usize| Some(TrackSelector { ids: vec![lib.tracks[i].id], ..Default::default() });
        let all: Vec<&str> = lib.tracks.iter().map(|t| t.file_path.as_str()).collect();
        vec![
            Edit::CreatePlaylist { path: "Set".into(), folder: false },
            add("Set", &all, false),
            Edit::PlanSet { playlist: "Set".into(), start: start(0), into: Some("Shows/Planned".into()) },
            Edit::PlanSet { playlist: "Set".into(), start: start(1), into: None },
        ]
    });
}

/// Fill an existing playlist with `files`, or make it, as `playlist fill` and `smart apply` do.
fn refill(playlist: &str, files: &[String]) -> Edit {
    Edit::SetTracks {
        playlist: playlist.into(),
        tracks: TrackSelector::paths(files.iter().cloned()),
        sort: None,
        limit: None,
        create: true,
    }
}

/// Regression: refilling a playlist was recorded without `create`, so when the journal lacked the
/// edit that made it (cleared, or a merge made it), a replay after rekordbox exported over it
/// found nothing to fill.
#[test]
fn replaying_a_refill_makes_a_playlist_the_journal_did_not_make() {
    let dir = tempfile::tempdir().unwrap();
    let root = fresh_pdb_drive(dir.path());
    let pdb_path = root.join(paths::EXPORT_PDB);
    let exported = fs::read(&pdb_path).unwrap();
    let files: Vec<String> = pdb::read_library(&pdb_path).unwrap().tracks.into_iter().map(|t| t.file_path).collect();
    let mut d = Device::open_export(&root).unwrap();
    let make = [Edit::CreatePlaylist { path: "Rules/Five stars".into(), folder: false }];
    let opts = EditOptions { no_journal: true, ..Default::default() };
    edit::apply(&edit::plan(&d, &make, &opts, &Ctx::silent()).unwrap(), &mut d, &Ctx::silent()).unwrap();
    assert_replay_after(&root, &[refill("Rules/Five stars", &files[..2])], || fs::write(&pdb_path, exported).unwrap());
}

/// A playlist rekordbox named with a slash, or with spaces around the name, is refilled by a
/// replay too: its path would make another playlist, so the refill is recorded without `create`.
#[test]
fn replaying_a_refill_of_a_name_its_path_cannot_make_fills_that_playlist() {
    let dir = tempfile::tempdir().unwrap();
    let root = fresh_pdb_drive(dir.path());
    let pdb_path = root.join(paths::EXPORT_PDB);
    let mut ed = rekordbox_pdb::PdbEditor::from_file(&pdb_path).unwrap();
    for name in ["Deep/Tech", " Warm up "] {
        ed.create_playlist(name, 0, false).unwrap();
    }
    ed.save(&pdb_path).unwrap();
    let exported = fs::read(&pdb_path).unwrap();
    let files: Vec<String> = pdb::read_library(&pdb_path).unwrap().tracks.into_iter().map(|t| t.file_path).collect();
    let edits = [refill("Deep/Tech", &files[..2]), refill(" Warm up ", &files[2..3])];
    assert_replay_after(&root, &edits, || fs::write(&pdb_path, exported).unwrap());
}

/// Regression: deleting every history list was recorded as that, and a replay also deleted the
/// lists players recorded since.
#[test]
fn replaying_a_delete_of_all_history_keeps_lists_recorded_since() {
    let dir = tempfile::tempdir().unwrap();
    let root = fresh_pdb_drive(dir.path());
    let exported = fs::read(root.join(paths::EXPORT_PDB)).unwrap();
    apply(&root, &[Edit::DeleteHistory { names: Vec::new(), all: true }]);
    // The next export brings the deleted lists back, with one a player recorded since.
    fs::write(root.join(paths::EXPORT_PDB), exported).unwrap();
    common::add_history(&root, "HISTORY 003", &[4]);
    let (edits, lib) = replay(&root);
    assert_eq!(lib.history.iter().map(|h| h.name.as_str()).collect::<Vec<_>>(), ["HISTORY 003"], "{edits:?}");
}

/// Regression: deleting history lists was recorded by their names, and a replay matched them
/// ignoring case, so a list a player recorded since under a reused name went too. The journal
/// names each list with its entries, and a replay matches both exactly.
#[test]
fn replaying_a_delete_of_history_keeps_a_list_recorded_since_under_its_name() {
    let dir = tempfile::tempdir().unwrap();
    let root = fresh_pdb_drive(dir.path());
    let exported = fs::read(root.join(paths::EXPORT_PDB)).unwrap();
    apply(&root, &[Edit::DeleteHistory { names: vec!["history 001".into()], all: false }]);
    fs::write(root.join(paths::EXPORT_PDB), exported).unwrap();
    common::add_history(&root, "HISTORY 001", &[4]);
    common::add_history(&root, "history 001", &[1, 2]);
    let (edits, lib) = replay(&root);
    let left: Vec<(&str, usize)> = lib.history.iter().map(|h| (h.name.as_str(), h.track_ids.len())).collect();
    assert_eq!(left, [("History 002", 2), ("HISTORY 001", 1), ("history 001", 2)], "{edits:?}");
}

/// Plan and apply the drive's journal as `edits replay` does: the plan's warnings.
fn replay_warnings(root: &Path) -> Vec<String> {
    let mut d = Device::open_export(root).unwrap();
    let opts = EditOptions { lenient: true, no_journal: true };
    let plan = edit::plan(&d, &edit::journal::edits(&d).unwrap(), &opts, &Ctx::silent()).unwrap();
    edit::apply(&plan, &mut d, &Ctx::silent()).unwrap();
    plan.warnings
}

/// Regression: edits that changed nothing were recorded (an add of tracks the playlist has as
/// adding none, deleting every history list of a drive with none as deleting none), and a replay
/// skipped them with a warning. Playlists and folders are recorded by their exact paths, however
/// they were named.
#[test]
fn the_journal_records_changes_by_exact_paths() {
    let dir = tempfile::tempdir().unwrap();
    let root = fresh_pdb_drive(dir.path());
    let files: Vec<String> =
        pdb::read_library(&root.join(paths::EXPORT_PDB)).unwrap().tracks.into_iter().map(|t| t.file_path).collect();
    let exported = fs::read(root.join(paths::EXPORT_PDB)).unwrap();
    apply(&root, &[Edit::DeleteHistory { names: Vec::new(), all: true }]);
    apply(
        &root,
        &[
            Edit::CreatePlaylist { path: "Shows/Friday".into(), folder: false },
            add("Shows/Friday", &[&files[0]], false),
            add("shows/friday", &[&files[0]], false),
            add("shows/friday", &[&files[0]], true),
            Edit::DeleteHistory { names: Vec::new(), all: true },
            Edit::CreatePlaylist { path: "Shows".into(), folder: true },
            Edit::RenamePlaylist { playlist: "friday".into(), name: " Saturday ".into() },
            Edit::MovePlaylist { playlist: "shows".into(), into: None, position: Some(1) },
            // A playlist of one track is sorted already.
            Edit::SortPlaylist { playlist: "SATURDAY".into(), by: "title".into() },
            Edit::Dedupe { playlist: "SATURDAY".into(), recordings: false },
        ],
    );
    let d = Device::open_export(&root).unwrap();
    let journal = edit::journal::list(&d).unwrap();
    assert_eq!(journal.len(), 2);
    assert_eq!(
        journal[1].edits,
        [
            Edit::CreatePlaylist { path: "Shows/Friday".into(), folder: false },
            add("Shows/Friday", &[&files[0]], false),
            add("Shows/Friday", &[&files[0]], true),
            Edit::RenamePlaylist { playlist: "Shows/Friday".into(), name: "Saturday".into() },
            Edit::MovePlaylist { playlist: "Shows/".into(), into: None, position: Some(1) },
            Edit::Dedupe { playlist: "Shows/Saturday".into(), recordings: false },
        ]
    );
    assert_eq!(journal[1].changes.len(), 10, "every edit is still described");
    let edited = shape(&d.library().unwrap());
    fs::write(root.join(paths::EXPORT_PDB), exported).unwrap();
    let warnings = replay_warnings(&root);
    assert!(warnings.iter().all(|w| !w.contains("skipped")), "{warnings:?}");
    assert_shape(&shape(&pdb::read_library(&root.join(paths::EXPORT_PDB)).unwrap()), &edited, "after the replay");
}

/// Regression: a replay was all or nothing per edit, so a playlist recorded with one track that
/// a new export no longer has wasn't filled at all. It gets the tracks still there, and an edit
/// left with no track is skipped.
#[test]
fn replaying_a_fill_leaves_out_tracks_the_new_export_lacks() {
    let dir = tempfile::tempdir().unwrap();
    let root = fresh_pdb_drive(dir.path());
    let pdb_path = root.join(paths::EXPORT_PDB);
    let exported = fs::read(&pdb_path).unwrap();
    let lib = pdb::read_library(&pdb_path).unwrap();
    let files: Vec<String> = lib.tracks.iter().map(|t| t.file_path.clone()).collect();
    apply(&root, &[refill("Rules/Three", &files[..3]), add("aab", &[&files[1]], true)]);
    // rekordbox exports again, without the second track.
    let mut ed = rekordbox_pdb::PdbEditor::from_bytes(exported);
    ed.remove_tracks(&[lib.tracks[1].id]).unwrap();
    fs::write(&pdb_path, ed.to_bytes()).unwrap();
    let warnings = replay_warnings(&root);
    assert!(warnings.iter().any(|w| w.contains("aren't on the drive any more")), "{warnings:?}");
    assert!(warnings.iter().any(|w| w.starts_with("skipped an edit")), "{warnings:?}");
    let lib = pdb::read_library(&pdb_path).unwrap();
    let three = lib.find_playlist("Rules/Three").unwrap();
    let got: Vec<&str> = three.track_ids.iter().map(|id| lib.track(*id).unwrap().file_path.as_str()).collect();
    assert_eq!(got, [files[0].as_str(), files[2].as_str()]);
}

/// A tempo and an album artist set on a drive read back the same from every database it has:
/// the tempo to hundredths, the album artist with the album (given, or the track's own), and
/// refused, changing nothing, when the track would be left without an album to keep it with.
#[hegel::test(test_cases = 8, suppress_health_check = [HealthCheck::TooSlow])]
fn tempos_and_album_artists_read_back_from_every_database(tc: TestCase) {
    use hegel::Generator;
    let dir = tempfile::tempdir().unwrap();
    let root = fresh_pdb_drive(dir.path());
    let plus = tc.draw(gs::booleans());
    if plus {
        fs::write(root.join(paths::LIBRARY_PLUS_DB), plus_db()).unwrap();
    }
    let tracks = pdb::read_library(&root.join(paths::EXPORT_PDB)).unwrap().tracks;
    let was = tc.draw(gs::sampled_from(&tracks));
    let bpm = tc.draw(gs::optional(gs::floats::<f64>().min_value(1.0).max_value(999.99)));
    let album = tc.draw(gs::optional(gs::sampled_from(vec!["", "Isles"]).map(String::from)));
    let album_artist = tc.draw(gs::optional(gs::sampled_from(vec![" ", "Bicep", "Four Tet"]).map(String::from)));
    tc.assume(bpm.is_some() || album.is_some() || album_artist.is_some());
    let fields = TrackFields { bpm, album: album.clone(), album_artist: album_artist.clone(), ..Default::default() };
    let edits = [Edit::EditTracks { tracks: TrackSelector::paths([was.file_path.clone()]), fields }];
    let mut d = Device::open_export(&root).unwrap();
    let before = drive_files(&root);
    let albumless = album.as_deref().unwrap_or(&was.album).trim().is_empty();
    let planned = edit::plan(&d, &edits, &EditOptions::default(), &Ctx::silent());
    if album_artist.as_deref().is_some_and(|a| !a.trim().is_empty()) && albumless {
        let err = planned.unwrap_err().to_string();
        assert!(err.contains("has no album to keep an album artist with"), "{err}");
        assert_eq!(drive_files(&root), before);
        return;
    }
    edit::apply(&planned.unwrap(), &mut d, &Ctx::silent()).unwrap();
    let mut libraries = vec![d.library().unwrap()];
    if plus {
        libraries.push(common::plus_library(&root));
    }
    let want_album = album.clone().unwrap_or_else(|| was.album.clone());
    let want_artist = match &album_artist {
        _ if want_album.is_empty() => String::new(),
        Some(a) if a.trim().is_empty() => String::new(),
        Some(a) => a.clone(),
        None => was.album_artist.clone(),
    };
    for lib in libraries {
        let t = lib.tracks.iter().find(|t| t.file_path == was.file_path).unwrap();
        assert_eq!(t.bpm, bpm.map_or(was.bpm, |b| (b * 100.0).round() / 100.0));
        assert_eq!((t.album.as_str(), t.album_artist.as_str()), (want_album.as_str(), want_artist.as_str()));
    }
}
