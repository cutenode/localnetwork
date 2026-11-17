//! `PdbEditor`'s edits on the vendored rekordbox-pdb's fixtures: a fuzzed sequence of playlist,
//! entry, track string and history edits, each read back through `Database` after it's made. An
//! edit that works changes what a model says it does and nothing else; one that's refused leaves
//! the file as it was. Either way the file still parses. The Hegel property `edits_match_the_model`
//! covers edits drawn from what's there; this one reaches the pages they leave behind (rows moved,
//! deleted and grown, pages filled), names of any length and ids that aren't there.

#![no_main]

use arbitrary::Arbitrary;
use indexmap::IndexMap;
use libfuzzer_sys::fuzz_target;
use rekordbox_pdb::pdb::TRACK_STRING_NAMES;
use rekordbox_pdb::{Database, PdbEditor};

const FIXTURES: [&[u8]; 2] = [
    include_bytes!("../../vendor/rekordbox-pdb/tests/data/one-song-export.pdb"),
    include_bytes!("../../vendor/rekordbox-pdb/tests/data/bigger-export.pdb"),
];

/// A playlist, folder, track or history list: one the model has, by its place among them, or
/// (from 240 up) an id that may not exist.
#[derive(Debug, Arbitrary, Clone, Copy)]
struct Pick(u8);

impl Pick {
    fn of(self, ids: &[u32]) -> u32 {
        match usize::from(self.0) {
            n if n < 240 && !ids.is_empty() => ids[n % ids.len()],
            n => (n % 16) as u32 * 7,
        }
    }
}

#[derive(Debug, Arbitrary)]
enum Edit {
    CreatePlaylist { name: String, parent: Pick, top: bool, folder: bool },
    Rename { node: Pick, name: String },
    Move { node: Pick, parent: Pick, top: bool, sort_order: u16 },
    Remove { nodes: Vec<Pick> },
    Add { list: Pick, track: Pick, index: Option<u16> },
    SetEntries { list: Pick, tracks: Vec<Pick> },
    SetString { track: Pick, field: u8, value: String },
    CreateHistory { name: String },
    AddToHistory { list: Pick, track: Pick },
    RemoveHistory { lists: Vec<Pick> },
}

#[derive(Debug, Arbitrary)]
struct Input {
    bigger: bool,
    edits: Vec<Edit>,
}

#[derive(Debug, Clone, PartialEq)]
struct Node {
    parent: u32,
    sort_order: u32,
    is_folder: bool,
    name: String,
}

/// What the edits change, by id. Entry lists hold (entry index, track) in index order, without
/// empty lists (a file can't tell one from a missing one).
#[derive(Debug, Clone, PartialEq)]
struct Model {
    tree: IndexMap<u32, Node>,
    entries: IndexMap<u32, Vec<(u32, u32)>>,
    strings: IndexMap<u32, Vec<String>>,
    history: IndexMap<u32, String>,
    history_entries: IndexMap<u32, Vec<(u32, u32)>>,
}

fn grouped(rows: impl Iterator<Item = (u32, u32, u32)>) -> IndexMap<u32, Vec<(u32, u32)>> {
    let mut out: IndexMap<u32, Vec<(u32, u32)>> = IndexMap::new();
    for (list, index, track) in rows {
        out.entry(list).or_default().push((index, track));
    }
    out.values_mut().for_each(|l| l.sort_unstable());
    out.sort_keys();
    out
}

impl Model {
    fn read(db: &Database) -> Model {
        let mut tree: IndexMap<u32, Node> = db
            .playlist_tree
            .iter()
            .map(|n| {
                let node = Node {
                    parent: n.parent_id,
                    sort_order: n.sort_order,
                    is_folder: n.is_folder,
                    name: n.name.clone(),
                };
                (n.id, node)
            })
            .collect();
        tree.sort_keys();
        let mut strings: IndexMap<u32, Vec<String>> = db.tracks.iter().map(|t| (t.id, t.strings.to_vec())).collect();
        strings.sort_keys();
        let mut history: IndexMap<u32, String> = db.history_playlists.iter().map(|h| (h.id, h.name.clone())).collect();
        history.sort_keys();
        Model {
            tree,
            entries: grouped(db.playlist_entries.iter().map(|e| (e.playlist_id, e.entry_index, e.track_id))),
            strings,
            history,
            history_entries: grouped(db.history_entries.iter().map(|e| (e.playlist_id, e.entry_index, e.track_id))),
        }
    }

    fn normalised(&self) -> Model {
        let mut m = self.clone();
        m.entries.retain(|_, l| !l.is_empty());
        m.history_entries.retain(|_, l| !l.is_empty());
        m.tree.sort_keys();
        m.entries.sort_keys();
        m.history.sort_keys();
        m.history_entries.sort_keys();
        m
    }

    fn nodes(&self) -> Vec<u32> {
        self.tree.keys().copied().collect()
    }

    fn tracks(&self) -> Vec<u32> {
        self.strings.keys().copied().collect()
    }

    fn lists(&self) -> Vec<u32> {
        self.history.keys().copied().collect()
    }

    /// The node `parent` (0, the top level, with `top`).
    fn parent(&self, parent: Pick, top: bool) -> u32 {
        if top { 0 } else { parent.of(&self.nodes()) }
    }

    /// Makes `edit` on `editor` and, if it works, on the model as the editor's docs say it does.
    fn apply(&mut self, editor: &mut PdbEditor, edit: &Edit) {
        match edit {
            Edit::CreatePlaylist { name, parent, top, folder } => {
                let parent = self.parent(*parent, *top);
                if let Ok(id) = editor.create_playlist(name, parent, *folder) {
                    // The last of its parent's children.
                    let siblings = self.tree.values().filter(|n| n.parent == parent);
                    let sort_order = siblings.map(|n| n.sort_order + 1).max().unwrap_or(0);
                    let node = Node { parent, sort_order, is_folder: *folder, name: name.clone() };
                    assert!(self.tree.insert(id, node).is_none(), "{id} was taken");
                }
            }
            Edit::Rename { node, name } => {
                let id = node.of(&self.nodes());
                if editor.rename_playlist(id, name).is_ok() {
                    self.tree[&id].name = name.clone();
                }
            }
            Edit::Move { node, parent, top, sort_order } => {
                let (id, parent) = (node.of(&self.nodes()), self.parent(*parent, *top));
                if editor.move_playlist(id, parent, u32::from(*sort_order)).is_ok() {
                    let n = &mut self.tree[&id];
                    (n.parent, n.sort_order) = (parent, u32::from(*sort_order));
                }
            }
            Edit::Remove { nodes } => {
                let ids: Vec<u32> = nodes.iter().map(|n| n.of(&self.nodes())).collect();
                // What's in a removed folder stays.
                if editor.remove_playlists(&ids).is_ok() {
                    for id in ids {
                        self.tree.shift_remove(&id);
                        self.entries.shift_remove(&id);
                    }
                }
            }
            Edit::Add { list, track, index } => {
                let (list, track) = (list.of(&self.nodes()), track.of(&self.tracks()));
                let index = index.map(u32::from);
                if editor.add_to_playlist(list, track, index).is_ok() {
                    let entries = self.entries.entry(list).or_default();
                    let next = entries.iter().map(|e| e.0).max().unwrap_or(0) + 1;
                    entries.push((index.unwrap_or(next), track));
                    entries.sort_unstable();
                }
            }
            Edit::SetEntries { list, tracks } => {
                let list = list.of(&self.nodes());
                let tracks: Vec<u32> = tracks.iter().map(|t| t.of(&self.tracks())).collect();
                if editor.set_playlist_entries(list, &tracks).is_ok() {
                    self.entries.insert(list, (1..).zip(tracks).collect());
                }
            }
            Edit::SetString { track, field, value } => {
                let id = track.of(&self.tracks());
                let i = usize::from(*field) % TRACK_STRING_NAMES.len();
                if editor.set_track_strings(id, &[(TRACK_STRING_NAMES[i], value)]).is_ok() {
                    self.strings[&id][i] = value.clone();
                }
            }
            Edit::CreateHistory { name } => {
                if let Ok(id) = editor.create_history_playlist(name) {
                    assert!(self.history.insert(id, name.clone()).is_none(), "{id} was taken");
                }
            }
            Edit::AddToHistory { list, track } => {
                let (list, track) = (list.of(&self.lists()), track.of(&self.tracks()));
                if editor.add_to_history(list, track).is_ok() {
                    let entries = self.history_entries.entry(list).or_default();
                    let next = entries.iter().map(|e| e.0).max().unwrap_or(0) + 1;
                    entries.push((next, track));
                }
            }
            Edit::RemoveHistory { lists } => {
                let ids: Vec<u32> = lists.iter().map(|l| l.of(&self.lists())).collect();
                if editor.remove_history_playlists(&ids).is_ok() {
                    for id in ids {
                        self.history.shift_remove(&id);
                        self.history_entries.shift_remove(&id);
                    }
                }
            }
        }
    }
}

fuzz_target!(|input: Input| {
    let fixture = FIXTURES[usize::from(input.bigger)];
    let mut model = Model::read(&Database::from_bytes(fixture.to_vec()).expect("the fixture reads"));
    let mut editor = PdbEditor::from_bytes(fixture.to_vec());
    for edit in input.edits.iter().take(64) {
        model.apply(&mut editor, edit);
        let db = Database::from_bytes(editor.to_bytes()).unwrap_or_else(|e| panic!("after {edit:?}: {e}"));
        assert_eq!(Model::read(&db), model.normalised(), "after {edit:?}");
    }
});
