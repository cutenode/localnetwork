//! A drive's library as the player holds it, and what's read for a track on it.

use rustc_hash::{FxHashMap, FxHashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use locallibrary::anlz::TrackAnalysis;
use locallibrary::api;
use locallibrary::fields::{OverlayStore, TrackValues};
use locallibrary::query::SortField;
use locallibrary::sets::Set;
use locallibrary::{Device, Library, Store, Track, artwork, paths};
use loclib_waveform::Art;

use crate::tui::browse::{self, Category, Group, Node, NodeId, Problems, SavedNode};
use crate::tui::keys::{Camelot, Relation};

/// Numbers each library load, so results worked out in the background for one (a file check,
/// a track's matches) aren't applied to another.
static GENERATION: AtomicU64 = AtomicU64::new(1);

/// The drive's My Tags (by lowercase file key, as `Category/Tag`), for `tag:` filters: read in
/// the background the first time a filter asks for them.
#[derive(Default)]
pub enum MyTags {
    #[default]
    Unread,
    Reading,
    Read(Arc<FxHashMap<String, Vec<String>>>),
    Failed(String),
}

/// What the analyzers installed worked out for the drive's tracks (see
/// [`locallibrary::fields::TrackValues`]), for their columns, filters and sorts: read in the
/// background the first time one asks.
#[derive(Default)]
pub enum Fields {
    #[default]
    Unread,
    Reading,
    Read(Arc<TrackValues>),
    Failed(String),
}

/// A drive's library, loaded for browsing.
pub struct Loaded {
    pub device: Device,
    pub library: Arc<Library>,
    pub generation: u64,
    pub(super) by_id: FxHashMap<u32, usize>,
    by_path: FxHashMap<String, usize>,
    /// Every playlist's path, by id.
    paths: FxHashMap<u32, String>,
    pub(super) haystack: Vec<String>,
    pub(super) expanded: FxHashSet<NodeId>,
    pub tree: Vec<Node>,
    /// The list in the track pane: all tracks, a playlist, or one value of a category.
    pub node: NodeId,
    /// Each category's values, worked out when Browse is first opened.
    pub(super) groups: FxHashMap<Category, Vec<Group>>,
    /// Each track's place under a sort field, worked out when first sorted by it.
    pub(in crate::tui) ranks: Vec<(SortField, Vec<u32>)>,
    pub problems: Problems,
    pub my_tags: MyTags,
    pub fields: Fields,
    /// The store's sets, for Browse › Sets: `None` until first read (see `App::read_sets`).
    pub(super) sets: Option<Vec<Set>>,
    /// The drive went away: what's shown may be out of date, and can't be played or edited.
    pub stale: bool,
}

impl Loaded {
    /// Index a library for browsing. Runs off the UI thread: it normalises every track's text.
    pub fn prepare(device: Device, library: Library) -> Loaded {
        let by_id = library.tracks.iter().enumerate().map(|(i, t)| (t.id, i)).collect();
        let by_path = library.tracks.iter().enumerate().map(|(i, t)| (t.file_path.clone(), i)).collect();
        let haystack = library.tracks.iter().map(browse::haystack).collect();
        let paths = browse::playlist_paths(&library);
        let (copy_of, copies) = browse::duplicates(&library);
        let mut l = Loaded {
            device,
            library: Arc::new(library),
            generation: GENERATION.fetch_add(1, Ordering::Relaxed),
            by_id,
            by_path,
            paths,
            haystack,
            expanded: FxHashSet::default(),
            tree: Vec::new(),
            node: NodeId::All,
            groups: FxHashMap::default(),
            ranks: Vec::new(),
            problems: Problems { missing: None, copy_of, copies },
            my_tags: MyTags::Unread,
            fields: Fields::Unread,
            sets: None,
            stale: false,
        };
        l.rebuild_tree();
        l
    }

    pub fn rebuild_tree(&mut self) {
        if self.expanded.contains(&NodeId::Browse) {
            for c in Category::ALL {
                self.ensure_groups(c);
            }
        }
        self.tree = browse::tree(&self.library, &self.expanded, &self.groups);
    }

    pub(super) fn ensure_groups(&mut self, c: Category) {
        if !self.groups.contains_key(&c) {
            let tags = match &self.my_tags {
                MyTags::Read(t) => Some(t.clone()),
                _ => None,
            };
            let sets = self.sets.as_deref().unwrap_or_default();
            let g = browse::groups(&self.library, c, &self.by_id, &self.problems, tags.as_deref(), sets);
            self.groups.insert(c, g);
        }
    }

    pub fn group(&mut self, c: Category, key: &str) -> Option<&Group> {
        self.ensure_groups(c);
        self.grouped(c, key)
    }

    /// A value of a category as last grouped (none before it is).
    pub fn grouped(&self, c: Category, key: &str) -> Option<&Group> {
        self.groups.get(&c)?.iter().find(|g| g.key == key)
    }

    /// How many entries of the list shown aren't on the drive (a set's, played from another).
    pub fn left_out(&self) -> usize {
        match &self.node {
            NodeId::Group(c, key) => self.grouped(*c, key).map_or(0, |g| g.left_out),
            _ => 0,
        }
    }

    /// The list shown is a set, and the store's sets haven't been read yet.
    pub fn reading_set(&self) -> bool {
        self.sets.is_none() && matches!(self.node, NodeId::Group(Category::Sets, _))
    }

    /// Open or close a folder (or Browse, or a category).
    pub fn toggle(&mut self, id: &NodeId) {
        if !self.expanded.remove(id) {
            self.expanded.insert(id.clone());
        }
        self.rebuild_tree();
    }

    pub fn rank(&mut self, field: SortField) -> &[u32] {
        let i = match self.ranks.iter().position(|(f, _)| *f == field) {
            Some(i) => i,
            None => {
                let ranks = match (field, &self.fields) {
                    (SortField::Field(name), Fields::Read(v)) => browse::field_ranks(&self.library, v, name),
                    _ => browse::ranks(&self.library, field),
                };
                self.ranks.push((field, ranks));
                self.ranks.len() - 1
            }
        };
        &self.ranks[i].1
    }

    /// What the analyzers worked out, once read.
    pub fn values(&self) -> Option<&Arc<TrackValues>> {
        match &self.fields {
            Fields::Read(v) => Some(v),
            _ => None,
        }
    }

    /// The tracks of the list shown (library indexes, in the list's order).
    pub fn base(&mut self) -> Vec<usize> {
        match self.node.clone() {
            NodeId::Playlist(id) => match self.library.playlist(id) {
                Some(p) => p.track_ids.iter().filter_map(|id| self.by_id.get(id).copied()).collect(),
                None => Vec::new(),
            },
            NodeId::Group(c, key) => self.group(c, &key).map(|g| g.tracks.clone()).unwrap_or_default(),
            _ => (0..self.library.tracks.len()).collect(),
        }
    }

    pub fn root(&self) -> &Path {
        self.device.root()
    }

    pub fn index_of(&self, file_path: &str) -> Option<usize> {
        self.by_path.get(file_path).copied()
    }

    /// How a node is named in titles: `Sets › Friday`, `Key › 8A Am`.
    pub fn node_title(&self, id: &NodeId) -> String {
        match id {
            NodeId::All => "All tracks".into(),
            NodeId::Playlist(id) => self.playlist_path(*id).replace('/', " › "),
            NodeId::Browse => "Browse".into(),
            NodeId::Category(c) => c.name().into(),
            NodeId::Group(c, key) => {
                let label = self.grouped(*c, key).map(|g| g.label.trim());
                format!("{} › {}", c.name(), label.unwrap_or(key))
            }
        }
    }

    pub fn playlist_name(&self) -> String {
        self.node_title(&self.node)
    }

    /// A playlist's path (`Sets/Friday`).
    pub fn playlist_path(&self, id: u32) -> String {
        self.paths.get(&id).cloned().unwrap_or_default()
    }

    pub fn save_node(&self, id: &NodeId) -> SavedNode {
        match id {
            NodeId::All => SavedNode::All,
            NodeId::Browse => SavedNode::Browse,
            NodeId::Playlist(id) => SavedNode::Playlist { path: self.playlist_path(*id) },
            NodeId::Category(c) => SavedNode::Category { category: *c },
            NodeId::Group(c, key) => SavedNode::Group { category: *c, key: key.clone() },
        }
    }

    /// A remembered node in this library, if it's still there.
    pub fn find_node(&self, s: &SavedNode) -> Option<NodeId> {
        Some(match s {
            SavedNode::All => NodeId::All,
            SavedNode::Browse => NodeId::Browse,
            SavedNode::Category { category } => NodeId::Category(*category),
            SavedNode::Group { category, key } => NodeId::Group(*category, key.clone()),
            SavedNode::Playlist { path } => {
                NodeId::Playlist(self.library.playlists.iter().find(|p| self.paths.get(&p.id) == Some(path))?.id)
            }
        })
    }

    /// Open the folders above a node so it shows in the tree.
    pub fn reveal(&mut self, id: &NodeId) {
        match id {
            NodeId::Playlist(pid) => {
                let mut cur = self.library.playlist(*pid).and_then(|p| p.parent_id);
                for _ in 0..64 {
                    let Some(f) = cur else { break };
                    self.expanded.insert(NodeId::Playlist(f));
                    cur = self.library.playlist(f).and_then(|p| p.parent_id);
                }
            }
            NodeId::Category(_) => {
                self.expanded.insert(NodeId::Browse);
            }
            NodeId::Group(c, _) => {
                self.expanded.insert(NodeId::Browse);
                self.expanded.insert(NodeId::Category(*c));
            }
            NodeId::All | NodeId::Browse => {}
        }
        self.rebuild_tree();
    }

    pub fn tree_position(&self, id: &NodeId) -> Option<usize> {
        self.tree.iter().position(|n| &n.id == id)
    }

    pub(super) fn set_missing(&mut self, missing: Vec<bool>) {
        self.problems.missing = Some(missing);
        self.groups.remove(&Category::Problems);
        self.rebuild_tree();
    }
}

/// Tracks that mix into one track (from `harmony::suggestions`), as indexes into the library they
/// were found in, which they keep.
pub struct Matches {
    pub library: Arc<Library>,
    pub root: PathBuf,
    pub items: Vec<Mix>,
}

pub struct Mix {
    pub index: usize,
    pub key: Camelot,
    pub relation: Relation,
    pub tempo_gap: f64,
}

impl Matches {
    pub(in crate::tui) fn find(library: Arc<Library>, root: PathBuf, track: &Track, limit: usize) -> Matches {
        let items = api::matches_in(&library, track, limit)
            .into_iter()
            .filter_map(|s| {
                let index = library.tracks.iter().position(|t| t.id == s.track.id)?;
                Some(Mix { index, key: s.key, relation: s.relation, tempo_gap: s.tempo_gap })
            })
            .collect();
        Matches { library, root, items }
    }

    pub fn track(&self, m: &Mix) -> &Track {
        &self.library.tracks[m.index]
    }
}

/// Playlists holding a track (any entry for its file), by path, sorted.
pub(super) fn playlists_of(lib: &Library, track: &Track) -> Vec<String> {
    let ids: Vec<u32> = lib.tracks.iter().filter(|t| t.file_path == track.file_path).map(|t| t.id).collect();
    let mut v: Vec<String> = lib
        .playlists
        .iter()
        .filter(|p| !p.is_folder && p.track_ids.iter().any(|id| ids.contains(id)))
        .map(|p| lib.playlist_path(p.id).replace('/', " › "))
        .collect();
    v.sort();
    v
}

/// The selected track's matches and playlists, found in the background.
pub struct Details {
    pub generation: u64,
    pub index: usize,
    pub matches: Matches,
    pub playlists: Vec<String>,
}

/// What starting a track reads in the background: its analysis files and artwork from the
/// drive (what they lack from what plugins' analyzers kept in the store, if they have it), and its
/// matches and playlists from the library.
#[derive(Default)]
pub struct TrackInfo {
    pub(in crate::tui) analysis: TrackAnalysis,
    /// What of the analysis is a plugin's, not the drive's: a line to show.
    pub(in crate::tui) from_plugins: Option<String>,
    pub(in crate::tui) art: Option<Art>,
    pub(in crate::tui) matches: Option<Matches>,
    pub(in crate::tui) playlists: Vec<String>,
}

pub(super) fn track_info(root: &Path, track: &Track, library: Option<Arc<Library>>, store_root: &Path) -> TrackInfo {
    let device = Device::open(root).ok();
    let mut analysis = device.as_ref().map(|d| api::analysis_of(d, track)).unwrap_or_default();
    let analyzers = crate::plugins::host().map(|p| p.analyzers()).unwrap_or_default();
    let audio = device.as_ref().and_then(|d| d.checked_path(&paths::key(&track.file_path)).ok());
    let from_plugins = audio.filter(|_| !analyzers.is_empty()).and_then(|audio| {
        let overlays = OverlayStore::in_store(&Store::open(store_root).ok()?);
        overlays.first(&audio, &analyzers).fill(&mut analysis)
    });
    let art = (device.as_ref().and_then(|d| artwork::file_on_drive(d, track)))
        .and_then(|f| std::fs::read(f).ok())
        .and_then(|b| Art::decode(&b));
    let playlists = library.as_deref().map(|lib| playlists_of(lib, track)).unwrap_or_default();
    let matches = library.map(|lib| Matches::find(lib, root.to_path_buf(), track, 40));
    TrackInfo { analysis, from_plugins, art, matches, playlists }
}

/// Sort ranks and Browse's groups, worked out in the background once a library is shown, so
/// the first sort by a column and opening Browse don't hold up the screen.
pub struct Index {
    pub(super) ranks: Vec<(SortField, Vec<u32>)>,
    pub(super) groups: FxHashMap<Category, Vec<Group>>,
}
