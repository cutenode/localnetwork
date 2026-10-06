//! Property-based tests of the vendored `export.pdb` editor: random sequences of edits checked
//! against an in-memory model, by our parser and by rekordcrate's independent one.

use crate::common;

use std::collections::BTreeMap;
use std::io::Cursor;
use std::path::PathBuf;

use binrw::BinRead;
use hegel::TestCase;
use hegel::generators::{self as gs, PrintableGenerator};
use rekordbox_pdb::pdb::TRACK_STRING_NAMES;
use rekordbox_pdb::{Database, NewTrack, PdbEditor, PdbError, TableType};

fn fixture(rel: &str) -> Vec<u8> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    std::fs::read(root.join(rel)).unwrap()
}

/// The starting databases: an empty export, one track, and twelve tracks with a playlist.
fn fixtures() -> Vec<(&'static str, Vec<u8>)> {
    vec![
        ("empty", fixture("tests/fixtures/rekordcrate/empty/PIONEER/rekordbox/export.pdb")),
        ("one song", fixture("../../vendor/rekordbox-pdb/tests/data/one-song-export.pdb")),
        ("bigger", fixture("../../vendor/rekordbox-pdb/tests/data/bigger-export.pdb")),
    ]
}

/// Bytes a DeviceSQL string takes: short ASCII up to 126 characters, long ASCII, else UTF-16.
fn encoded_len(s: &str) -> usize {
    if s.is_ascii() { if s.len() <= 126 { 1 + s.len() } else { 4 + s.len() } } else { 4 + 2 * s.encode_utf16().count() }
}

/// Names a person (or a corrupt import) might give a playlist, track or history list.
fn names() -> impl PrintableGenerator<String> {
    hegel::one_of!(
        gs::text().max_size(16),
        gs::text().alphabet("abcABC /._-'&0123").max_size(40),
        gs::sampled_from(vec![
            String::new(),
            "Foo".to_string(),
            "foo".to_string(),
            "Shows/2026".to_string(),
            "日本語のプレイリスト".to_string(),
            "🎛️ techno 🔊".to_string(),
            "Ünïcödé".to_string(),
            "a\u{0}b".to_string(),
        ]),
        // Either side of the short/long ASCII boundary.
        gs::text().codec("ascii").min_size(120).max_size(135),
        // Long enough that the row may not fit in a page.
        gs::text().min_size(500).max_size(2200),
    )
}

/// A string small enough that any row holding it fits in a page.
fn small(s: &str) -> bool {
    encoded_len(s) <= 1500
}

/// A string whose row can't fit in a 4096-byte page.
fn too_big(s: &str) -> bool {
    encoded_len(s) > 4100
}

#[derive(Clone, Debug, PartialEq)]
struct Node {
    parent: u32,
    sort_order: u32,
    name: String,
    folder: bool,
}

/// What the database should hold, as the tests understand the edits.
#[derive(Clone, Debug, PartialEq, Default)]
struct Model {
    tracks: BTreeMap<u32, Vec<String>>,
    artists: BTreeMap<u32, String>,
    tree: BTreeMap<u32, Node>,
    /// Playlist id → (entry index, track id), in entry order.
    entries: BTreeMap<u32, Vec<(u32, u32)>>,
    history: BTreeMap<u32, String>,
    history_entries: BTreeMap<u32, Vec<(u32, u32)>>,
}

impl Model {
    fn of(db: &Database) -> Model {
        let mut m = Model::default();
        for t in &db.tracks {
            m.tracks.insert(t.id, t.strings.to_vec());
        }
        for a in &db.artists {
            m.artists.insert(a.id, a.name.clone());
        }
        for n in &db.playlist_tree {
            let node =
                Node { parent: n.parent_id, sort_order: n.sort_order, name: n.name.clone(), folder: n.is_folder };
            m.tree.insert(n.id, node);
        }
        for e in &db.playlist_entries {
            m.entries.entry(e.playlist_id).or_default().push((e.entry_index, e.track_id));
        }
        for h in &db.history_playlists {
            m.history.insert(h.id, h.name.clone());
        }
        for e in &db.history_entries {
            m.history_entries.entry(e.playlist_id).or_default().push((e.entry_index, e.track_id));
        }
        for list in m.entries.values_mut().chain(m.history_entries.values_mut()) {
            list.sort_unstable();
        }
        m
    }

    fn folders(&self) -> Vec<u32> {
        let mut out = vec![0];
        out.extend(self.tree.iter().filter(|(_, n)| n.folder).map(|(id, _)| *id));
        out
    }

    fn playlists(&self) -> Vec<u32> {
        self.tree.iter().filter(|(_, n)| !n.folder).map(|(id, _)| *id).collect()
    }

    fn track_ids(&self) -> Vec<u32> {
        self.tracks.keys().copied().collect()
    }

    /// Whether `node` is `ancestor` or sits somewhere under it.
    fn within(&self, mut node: u32, ancestor: u32) -> bool {
        for _ in 0..=self.tree.len() {
            if node == ancestor {
                return true;
            }
            match self.tree.get(&node) {
                Some(n) if node != 0 => node = n.parent,
                _ => return false,
            }
        }
        false
    }

    fn renumber(list: &mut [(u32, u32)]) {
        for (i, e) in list.iter_mut().enumerate() {
            e.0 = i as u32 + 1;
        }
    }
}

/// The playlist tree as rekordcrate reads it: id → (parent, name, folder).
fn rekordcrate_tree(pdb: &[u8]) -> BTreeMap<u32, (u32, String, bool)> {
    use rekordcrate::pdb::{Header, PageType, Row};
    let mut reader = Cursor::new(pdb);
    let header = Header::read(&mut reader).expect("rekordcrate reads the header");
    let table = header.tables.iter().find(|t| t.page_type == PageType::PlaylistTree).unwrap();
    let pages = header
        .read_pages(&mut reader, binrw::Endian::NATIVE, (&table.first_page, &table.last_page))
        .expect("rekordcrate reads the playlist tree");
    let mut out = BTreeMap::new();
    for group in pages.iter().flat_map(|p| p.row_groups.iter()) {
        for row in group.present_rows() {
            if let Row::PlaylistTreeNode(n) = row {
                let folder = n.is_folder();
                // rekordcrate reads a name whose first UTF-16 unit has low byte 3 as an ISRC
                // (`common::rekordcrate_reads_as_isrc`): some come back empty, others fail to
                // decode. Both count as empty; the comparison allows for them.
                out.insert(n.id.0, (n.parent_id.0, n.name.into_string().unwrap_or_default(), folder));
            }
        }
    }
    out
}

/// Present rows of a table, as rekordcrate counts them.
fn rekordcrate_rows(pdb: &[u8], page_type: rekordcrate::pdb::PageType) -> usize {
    use rekordcrate::pdb::Header;
    let mut reader = Cursor::new(pdb);
    let header = Header::read(&mut reader).unwrap();
    let Some(table) = header.tables.iter().find(|t| t.page_type == page_type) else { return 0 };
    let pages = header.read_pages(&mut reader, binrw::Endian::NATIVE, (&table.first_page, &table.last_page)).unwrap();
    pages.iter().flat_map(|p| p.row_groups.iter()).map(|g| g.present_rows().count()).sum()
}

struct Editing {
    editor: PdbEditor,
    model: Model,
}

impl Editing {
    /// Apply an edit the model expects to succeed (`Some(true)`), to fail (`Some(false)`), or
    /// either (`None`); on failure the database must be left as it was.
    fn expect<T: std::fmt::Debug>(&self, what: &str, ok: Option<bool>, result: &Result<T, PdbError>) -> bool {
        match (ok, result) {
            (Some(true), Err(e)) => panic!("{what} failed: {e:?}"),
            (Some(false), Ok(v)) => panic!("{what} should have failed, gave {v:?}"),
            _ => result.is_ok(),
        }
    }
}

#[hegel::state_machine]
impl Editing {
    #[rule]
    fn create_playlist(&mut self, tc: TestCase) {
        let name = tc.draw(names());
        let folder = tc.draw(gs::booleans());
        // Mostly a real folder; sometimes a playlist or a missing id, which must be refused.
        let mut parents = self.model.folders();
        parents.extend(self.model.playlists().into_iter().take(1));
        parents.push(9_999);
        let parent = tc.draw(gs::sampled_from(parents));
        let valid = parent == 0 || self.model.tree.get(&parent).is_some_and(|n| n.folder);
        let ok = if !valid || too_big(&name) {
            Some(false)
        } else if small(&name) {
            Some(true)
        } else {
            None
        };
        let result = self.editor.create_playlist(&name, parent, folder);
        if self.expect("create_playlist", ok, &result) {
            let id = result.unwrap();
            assert!(!self.model.tree.contains_key(&id), "id {id} reused");
            assert!(!self.model.entries.contains_key(&id), "new playlist {id} inherits stale entries");
            let sort_order =
                self.model.tree.values().filter(|n| n.parent == parent).map(|n| n.sort_order + 1).max().unwrap_or(0);
            self.model.tree.insert(id, Node { parent, sort_order, name, folder });
        }
    }

    #[rule]
    fn rename_playlist(&mut self, tc: TestCase) {
        tc.assume(!self.model.tree.is_empty());
        let id = tc.draw(gs::sampled_from(self.model.tree.keys().copied().collect::<Vec<_>>()));
        let name = tc.draw(names());
        let ok = if too_big(&name) {
            Some(false)
        } else if small(&name) {
            Some(true)
        } else {
            None
        };
        let result = self.editor.rename_playlist(id, &name);
        if self.expect("rename_playlist", ok, &result) {
            self.model.tree.get_mut(&id).unwrap().name = name;
        }
    }

    #[rule]
    fn move_playlist(&mut self, tc: TestCase) {
        tc.assume(!self.model.tree.is_empty());
        let id = tc.draw(gs::sampled_from(self.model.tree.keys().copied().collect::<Vec<_>>()));
        let parent = tc.draw(gs::sampled_from(self.model.folders()));
        let sort_order = tc.draw(gs::integers::<u32>().max_value(20));
        // A folder can't go inside itself.
        let ok = !self.model.within(parent, id);
        let result = self.editor.move_playlist(id, parent, sort_order);
        if self.expect("move_playlist", Some(ok), &result) {
            let n = self.model.tree.get_mut(&id).unwrap();
            (n.parent, n.sort_order) = (parent, sort_order);
        }
    }

    #[rule]
    fn remove_playlist(&mut self, tc: TestCase) {
        tc.assume(!self.model.tree.is_empty());
        let id = tc.draw(gs::sampled_from(self.model.tree.keys().copied().collect::<Vec<_>>()));
        let result = self.editor.remove_playlists(&[id]);
        self.expect("remove_playlists", Some(true), &result);
        // Children of a removed folder stay where they are (the caller removes them too).
        self.model.tree.remove(&id);
        self.model.entries.remove(&id);
    }

    #[rule]
    fn add_to_playlist(&mut self, tc: TestCase) {
        let playlists = self.model.playlists();
        tc.assume(!playlists.is_empty() && !self.model.tracks.is_empty());
        let playlist = tc.draw(gs::sampled_from(playlists));
        let track = tc.draw(gs::sampled_from(self.model.track_ids()));
        let result = self.editor.add_to_playlist(playlist, track, None);
        self.expect("add_to_playlist", Some(true), &result);
        let list = self.model.entries.entry(playlist).or_default();
        let index = list.iter().map(|e| e.0).max().unwrap_or(0) + 1;
        list.push((index, track));
    }

    #[rule]
    fn set_playlist_entries(&mut self, tc: TestCase) {
        let playlists = self.model.playlists();
        tc.assume(!playlists.is_empty() && !self.model.tracks.is_empty());
        let playlist = tc.draw(gs::sampled_from(playlists));
        // Repeats allowed.
        let tracks = tc.draw(gs::vecs(gs::sampled_from(self.model.track_ids())).max_size(8));
        let result = self.editor.set_playlist_entries(playlist, &tracks);
        self.expect("set_playlist_entries", Some(true), &result);
        let list: Vec<(u32, u32)> = tracks.iter().enumerate().map(|(i, t)| (i as u32 + 1, *t)).collect();
        if list.is_empty() {
            self.model.entries.remove(&playlist);
        } else {
            self.model.entries.insert(playlist, list);
        }
    }

    #[rule]
    fn add_track(&mut self, tc: TestCase) {
        let title = tc.draw(names());
        let artist = tc.draw(gs::optional(names()));
        let album = tc.draw(gs::optional(names()));
        let comment = tc.draw(names());
        let file = tc.draw(gs::text().alphabet("abc/ ._-é").min_size(1).max_size(30));
        let file_path = format!("/Contents/{file}.mp3");
        let strings = [title.as_str(), comment.as_str(), file_path.as_str()];
        let lookups: Vec<&String> = artist.iter().chain(album.iter()).collect();
        let total: usize = strings.iter().map(|s| encoded_len(s)).sum();
        let ok = if total + 200 <= 3500 && lookups.iter().all(|s| small(s)) {
            Some(true)
        } else if total > 4100 || lookups.iter().any(|s| too_big(s)) {
            Some(false)
        } else {
            None
        };
        let track = NewTrack {
            title: title.clone(),
            file_path: file_path.clone(),
            artist: artist.clone(),
            album,
            comment: comment.clone(),
            ..Default::default()
        };
        let result = self.editor.add_track(&track);
        if self.expect("add_track", ok, &result) {
            let id = result.unwrap();
            assert!(!self.model.tracks.contains_key(&id), "track id {id} reused");
            let db = self.editor.database().unwrap();
            let row = db.tracks.iter().find(|t| t.id == id).expect("the new track reads back");
            assert_eq!(row.title(), title);
            assert_eq!(row.comment(), comment);
            assert_eq!(row.file_path(), file_path);
            match &artist {
                Some(a) => {
                    let found = db.artists.iter().find(|r| r.id == row.artist_id).map(|r| r.name.as_str());
                    assert_eq!(found, Some(a.as_str()), "the track's artist");
                }
                None => assert_eq!(row.artist_id, 0),
            }
            self.model.tracks.insert(id, row.strings.to_vec());
            self.model.artists = db.artists.iter().map(|a| (a.id, a.name.clone())).collect();
        }
    }

    #[rule]
    fn set_track_strings(&mut self, tc: TestCase) {
        tc.assume(!self.model.tracks.is_empty());
        let id = tc.draw(gs::sampled_from(self.model.track_ids()));
        let fields = tc.draw(gs::vecs(gs::sampled_from(TRACK_STRING_NAMES.to_vec())).min_size(1).max_size(3));
        let mut values = Vec::new();
        for _ in &fields {
            values.push(tc.draw(names()));
        }
        let mut strings = self.model.tracks[&id].clone();
        for (field, value) in fields.iter().zip(&values) {
            let slot = TRACK_STRING_NAMES.iter().position(|n| n == field).unwrap();
            strings[slot] = value.clone();
        }
        let total: usize =
            strings.iter().enumerate().map(|(i, s)| if i == 0 { common::isrc_len(s) } else { encoded_len(s) }).sum();
        let ok = if total + 200 <= 3500 {
            Some(true)
        } else if total > 4100 {
            Some(false)
        } else {
            None
        };
        let pairs: Vec<(&str, &str)> = fields.iter().copied().zip(values.iter().map(String::as_str)).collect();
        let result = self.editor.set_track_strings(id, &pairs);
        if self.expect("set_track_strings", ok, &result) {
            self.model.tracks.insert(id, strings);
        }
    }

    #[rule]
    fn remove_track(&mut self, tc: TestCase) {
        tc.assume(!self.model.tracks.is_empty());
        let id = tc.draw(gs::sampled_from(self.model.track_ids()));
        let result = self.editor.remove_tracks(&[id]);
        self.expect("remove_tracks", Some(true), &result);
        self.model.tracks.remove(&id);
        for list in self.model.entries.values_mut().chain(self.model.history_entries.values_mut()) {
            let before = list.len();
            list.retain(|e| e.1 != id);
            if list.len() != before {
                Model::renumber(list);
            }
        }
        self.model.entries.retain(|_, l| !l.is_empty());
        self.model.history_entries.retain(|_, l| !l.is_empty());
    }

    #[rule]
    fn repoint_track(&mut self, tc: TestCase) {
        tc.assume(self.model.tracks.len() >= 2);
        let from = tc.draw(gs::sampled_from(self.model.track_ids()));
        let to = tc.draw(gs::sampled_from(self.model.track_ids()));
        let result = self.editor.repoint_track(from, to);
        self.expect("repoint_track", Some(true), &result);
        let mut changed = 0;
        for e in self.model.entries.values_mut().chain(self.model.history_entries.values_mut()).flatten() {
            if e.1 == from {
                e.1 = to;
                changed += 1;
            }
        }
        assert_eq!(result.unwrap(), changed);
    }

    #[rule]
    fn create_history(&mut self, tc: TestCase) {
        let name = tc.draw(names());
        let ok = if too_big(&name) {
            Some(false)
        } else if small(&name) {
            Some(true)
        } else {
            None
        };
        let result = self.editor.create_history_playlist(&name);
        if self.expect("create_history_playlist", ok, &result) {
            let id = result.unwrap();
            assert!(!self.model.history.contains_key(&id), "history id {id} reused");
            assert!(!self.model.history_entries.contains_key(&id), "new history list {id} inherits stale entries");
            self.model.history.insert(id, name);
        }
    }

    #[rule]
    fn add_to_history(&mut self, tc: TestCase) {
        tc.assume(!self.model.history.is_empty() && !self.model.tracks.is_empty());
        let list = tc.draw(gs::sampled_from(self.model.history.keys().copied().collect::<Vec<_>>()));
        let track = tc.draw(gs::sampled_from(self.model.track_ids()));
        let result = self.editor.add_to_history(list, track);
        self.expect("add_to_history", Some(true), &result);
        let entries = self.model.history_entries.entry(list).or_default();
        let index = entries.iter().map(|e| e.0).max().unwrap_or(0) + 1;
        entries.push((index, track));
    }

    #[rule]
    fn remove_history(&mut self, tc: TestCase) {
        tc.assume(!self.model.history.is_empty());
        let id = tc.draw(gs::sampled_from(self.model.history.keys().copied().collect::<Vec<_>>()));
        let result = self.editor.remove_history_playlists(&[id]);
        self.expect("remove_history_playlists", Some(true), &result);
        self.model.history.remove(&id);
        self.model.history_entries.remove(&id);
    }

    /// Saving and reopening mid-session changes nothing (the editor's index is rebuilt).
    #[rule]
    fn save_and_reopen(&mut self, _: TestCase) {
        let bytes = self.editor.to_bytes();
        self.editor = PdbEditor::from_bytes(bytes);
    }

    /// After every edit, the saved file holds exactly what the model says, and rekordcrate reads
    /// it the same way.
    #[invariant(always_run)]
    fn matches_the_model(&mut self, _: TestCase) {
        let bytes = self.editor.to_bytes();
        let db = Database::from_bytes(bytes.clone()).expect("the saved file parses");
        let got = Model::of(&db);
        assert_eq!(got.tree, self.model.tree, "playlist tree");
        assert_eq!(got.entries, self.model.entries, "playlist entries");
        assert_eq!(got.tracks, self.model.tracks, "track strings");
        assert_eq!(got.artists, self.model.artists, "artists");
        assert_eq!(got.history, self.model.history, "history lists");
        assert_eq!(got.history_entries, self.model.history_entries, "history entries");

        let mut theirs = rekordcrate_tree(&bytes);
        let ours: BTreeMap<u32, (u32, String, bool)> =
            got.tree.iter().map(|(id, n)| (*id, (n.parent, n.name.clone(), n.folder))).collect();
        for (id, (_, name, _)) in &ours {
            if common::rekordcrate_reads_as_isrc(name)
                && let Some(t) = theirs.get_mut(id)
            {
                t.1.clone_from(name);
            }
        }
        assert_eq!(theirs, ours, "rekordcrate's playlist tree");
        use rekordcrate::pdb::PageType;
        let entries: usize = got.entries.values().map(Vec::len).sum();
        assert_eq!(rekordcrate_rows(&bytes, PageType::PlaylistEntries), entries, "rekordcrate's entry rows");
        assert_eq!(rekordcrate_rows(&bytes, PageType::Tracks), got.tracks.len(), "rekordcrate's track rows");
    }
}

/// Any sequence of edits leaves a database that holds what was asked for, and nothing else.
#[hegel::test(test_cases = 100)]
fn edits_match_a_model(tc: TestCase) {
    let (name, bytes) = fixtures().swap_remove(tc.draw(gs::integers::<usize>().max_value(2)));
    tc.note(&format!("starting from the {name} fixture"));
    let model = Model::of(&Database::from_bytes(bytes.clone()).unwrap());
    let editing = Editing { editor: PdbEditor::from_bytes(bytes), model };
    hegel::stateful::machine(editing).steps(50).run(tc);
}

/// Opening a file and saving it without edits gives back the same bytes.
#[test]
fn unedited_files_round_trip() {
    for (name, bytes) in fixtures() {
        assert_eq!(PdbEditor::from_bytes(bytes.clone()).to_bytes(), bytes, "{name}");
    }
}

/// A name survives being written and read back, whatever its length or script.
#[hegel::test(test_cases = 300)]
fn playlist_names_round_trip(tc: TestCase) {
    let name = tc.draw(names());
    let mut editor = PdbEditor::from_bytes(fixture("tests/fixtures/rekordcrate/empty/PIONEER/rekordbox/export.pdb"));
    match editor.create_playlist(&name, 0, false) {
        Ok(id) => {
            let db = Database::from_bytes(editor.to_bytes()).unwrap();
            let node = db.playlist_tree.iter().find(|n| n.id == id).unwrap();
            assert_eq!(node.name, name);
        }
        Err(e) => assert!(!small(&name), "{e:?} for a {}-byte name", encoded_len(&name)),
    }
}

/// Found by `edits_match_a_model`: an album name too long for a row used to fail only after
/// the artist row was written, leaving an artist no track uses.
#[test]
fn add_track_is_all_or_nothing() {
    let bytes = fixture("tests/fixtures/rekordcrate/empty/PIONEER/rekordbox/export.pdb");
    let mut editor = PdbEditor::from_bytes(bytes);
    let track =
        NewTrack {
            artist: Some(String::new()), album: Some("𐀀".repeat(1100)), ..NewTrack::new("", "/Contents/_.mp3")
        };
    assert!(matches!(editor.add_track(&track), Err(PdbError::RowTooLarge { .. })));
    let db = Database::from_bytes(editor.to_bytes()).unwrap();
    assert!(db.artists.is_empty() && db.albums.is_empty() && db.tracks.is_empty());
}

/// A real export damaged the way a failing drive or a crafted file can be: playlists given each
/// other's ids as their own or their folder's, header, table directory and page header fields set
/// to edge values, bytes changed anywhere, the file cut short.
#[hegel::test_helper]
fn damaged(tc: &TestCase) -> Vec<u8> {
    let (name, bytes) = fixtures().swap_remove(tc.draw(gs::integers::<usize>().max_value(2)));
    tc.note(&format!("damaging the {name} fixture, with folders F/G holding playlist P"));
    let mut editor = PdbEditor::from_bytes(bytes);
    let f = editor.create_playlist("F", 0, true).unwrap();
    let g = editor.create_playlist("G", f, true).unwrap();
    editor.create_playlist("P", g, false).unwrap();
    let mut bytes = editor.to_bytes();
    let page_size = u32::from_le_bytes(bytes[4..8].try_into().unwrap()) as usize;
    let pages = bytes.len() / page_size;
    let db = Database::from_bytes(bytes.clone()).unwrap();
    let (rows, tree) = (db.row_locations(TableType::PlaylistTree), &db.playlist_tree);
    if !rows.is_empty() {
        let index = || gs::integers::<usize>().max_value(rows.len() - 1);
        for (row, other, field) in
            tc.draw(gs::vecs(gs::tuples!(index(), index(), gs::sampled_from(vec![0, 12]))).max_size(3))
        {
            let at = rows[row] + field; // the row's folder, or its own id
            bytes[at..at + 4].copy_from_slice(&tree[other].id.to_le_bytes());
        }
    }
    let value = || {
        hegel::one_of!(
            gs::sampled_from(vec![0u32, 1, 2, 0x40000, 0x7fff_ffff, u32::MAX]),
            gs::integers::<u32>().max_value(pages as u32 + 80),
            gs::integers::<u32>(),
        )
    };
    // Often a table's next free page (an edit grows the file to it).
    if tc.draw(gs::booleans()) {
        let table = tc.draw(gs::integers::<usize>().max_value(19));
        let v = tc.draw(value());
        bytes[0x1c + table * 16 + 4..0x1c + table * 16 + 8].copy_from_slice(&v.to_le_bytes());
    }
    for (at, v) in tc.draw(gs::vecs(gs::tuples!(gs::integers::<usize>(), value())).max_size(6)) {
        // A u32 of the header, of the table directory (20 tables) or of some page's header.
        let (kind, n) = (at % 3, at / 3);
        let at = match kind {
            0 => n % 7 * 4,
            1 => 0x1c + n % 80 * 4,
            _ => n % pages * page_size + n / pages % 10 * 4,
        };
        bytes[at..at + 4].copy_from_slice(&v.to_le_bytes());
    }
    for (at, b) in tc.draw(gs::vecs(gs::tuples!(gs::integers::<usize>(), gs::integers::<u8>())).max_size(8)) {
        let i = at % bytes.len();
        bytes[i] = b;
    }
    if tc.draw(gs::weighted_booleans(0.1)) {
        let len = tc.draw(gs::integers::<usize>().max_value(bytes.len()));
        bytes.truncate(len);
    }
    bytes
}

/// Whatever a damaged export holds, reading it ends: the vendored reader returns (or panics,
/// which `pdb::parse_database` turns into an error), and a library read from it has each
/// playlist once and a tree every walk from the top ends in. Editing what `parse_database`
/// accepts grows the file by a page per table past its end at most, whatever its free page
/// fields say (no edit took many gigabytes before).
#[hegel::test(test_cases = 300)]
fn damaged_exports_are_read_or_refused(tc: TestCase) {
    let bytes = damaged(&tc);
    let _ = std::panic::catch_unwind(|| Database::from_bytes(bytes.clone()));
    let Ok(db) = locallibrary::pdb::parse_database(bytes.clone(), std::path::Path::new("export.pdb")) else { return };
    let lib = locallibrary::pdb::library_from_database(&db);
    let mut ids: Vec<u32> = lib.playlists.iter().map(|p| p.id).collect();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), lib.playlists.len(), "a playlist id repeats");
    let mut reached = 0;
    let mut stack = vec![None];
    while let Some(parent) = stack.pop() {
        for p in lib.children(parent) {
            reached += 1;
            assert!(reached <= lib.playlists.len(), "a walk from the top went round");
            stack.push(Some(p.id));
        }
    }

    let page_size = db.page_size as usize;
    let mut editor = PdbEditor::from_bytes(bytes.clone());
    let edited = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let folder = editor.create_playlist("x", 0, true)?;
        editor.create_history_playlist("HISTORY 001")?;
        editor.create_playlist("y", folder, false)?;
        Ok::<_, PdbError>(editor.to_bytes())
    }));
    if let Ok(Ok(out)) = edited {
        assert!(out.len() <= bytes.len() + 3 * 65 * page_size, "{} bytes from {}", out.len(), bytes.len());
    }
}
