//! `PdbEditor`'s edits as a model-based state machine: each edit does (or refuses, with the
//! error the model expects) what the model says, and the saved file reads back through
//! `Database` exactly as the model has it.

use std::sync::atomic::{AtomicUsize, Ordering};

use hegel::generators as gs;
use hegel::TestCase;
use indexmap::IndexMap;
use rekordbox_pdb::pdb::TRACK_STRING_NAMES;
use rekordbox_pdb::{file_type_code, Database, NewTrack, PdbEditor, PdbError, TableType};

const FIXTURES: [&str; 2] = [
    concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/one-song-export.pdb"),
    concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/bigger-export.pdb"),
];

/// The fixed-width track fields `set_track_field` edits, with their widths in bytes (FORMAT.md).
const FIELDS: [(&str, usize); 22] = [
    ("sample_rate", 4),
    ("composer_id", 4),
    ("file_size", 4),
    ("artwork_id", 4),
    ("key_id", 4),
    ("original_artist_id", 4),
    ("label_id", 4),
    ("remixer_id", 4),
    ("bitrate", 4),
    ("track_number", 4),
    ("tempo", 4),
    ("genre_id", 4),
    ("album_id", 4),
    ("artist_id", 4),
    ("disc_number", 2),
    ("play_count", 2),
    ("year", 2),
    ("sample_depth", 2),
    ("duration", 2),
    ("color_id", 1),
    ("rating", 1),
    ("file_type", 2),
];

/// A track row as the reader sees it: its 21 strings and its fixed fields by name.
#[derive(Debug, Clone, PartialEq)]
struct TrackRow {
    strings: Vec<String>,
    fields: IndexMap<&'static str, u32>,
}

impl TrackRow {
    fn read(t: &rekordbox_pdb::Track, file_type: u16) -> TrackRow {
        let values = [
            t.sample_rate,
            t.composer_id,
            t.file_size,
            t.artwork_id,
            t.key_id,
            t.original_artist_id,
            t.label_id,
            t.remixer_id,
            t.bitrate,
            t.track_number,
            t.tempo,
            t.genre_id,
            t.album_id,
            t.artist_id,
            t.disc_number.into(),
            t.play_count.into(),
            t.year.into(),
            t.sample_depth.into(),
            t.duration.into(),
            t.color_id.into(),
            t.rating.into(),
            file_type.into(),
        ];
        TrackRow { strings: t.strings.to_vec(), fields: FIELDS.iter().map(|f| f.0).zip(values).collect() }
    }
}

#[derive(Debug, Clone, PartialEq)]
struct Node {
    parent: u32,
    sort_order: u32,
    is_folder: bool,
    name: String,
}

/// Everything the editor edits, by id. Entry lists hold (entry index, track) in index order.
#[derive(Debug, Clone, PartialEq, Default)]
struct Model {
    tracks: IndexMap<u32, TrackRow>,
    artists: IndexMap<u32, String>,
    albums: IndexMap<u32, (String, u32)>,
    genres: IndexMap<u32, String>,
    labels: IndexMap<u32, String>,
    keys: IndexMap<u32, String>,
    artwork: IndexMap<u32, String>,
    tree: IndexMap<u32, Node>,
    entries: IndexMap<u32, Vec<(u32, u32)>>,
    history: IndexMap<u32, String>,
    history_entries: IndexMap<u32, Vec<(u32, u32)>>,
}

fn group(rows: impl Iterator<Item = (u32, u32, u32)>) -> IndexMap<u32, Vec<(u32, u32)>> {
    let mut out: IndexMap<u32, Vec<(u32, u32)>> = IndexMap::new();
    for (list, index, track) in rows {
        out.entry(list).or_default().push((index, track));
    }
    out.values_mut().for_each(|l| l.sort_unstable());
    out
}

impl Model {
    fn read(db: &Database, bytes: &[u8]) -> Model {
        let named = |rows: Vec<(u32, &String)>| rows.into_iter().map(|(id, n)| (id, n.clone())).collect();
        let tracks = db
            .tracks
            .iter()
            .zip(db.row_locations(TableType::Tracks))
            .map(|(t, &loc)| (t.id, TrackRow::read(t, u16::from_le_bytes([bytes[loc + 0x5a], bytes[loc + 0x5b]]))))
            .collect();
        Model {
            tracks,
            artists: named(db.artists.iter().map(|a| (a.id, &a.name)).collect()),
            albums: db.albums.iter().map(|a| (a.id, (a.name.clone(), a.artist_id))).collect(),
            genres: named(db.genres.iter().map(|g| (g.id, &g.name)).collect()),
            labels: named(db.labels.iter().map(|l| (l.id, &l.name)).collect()),
            keys: named(db.keys.iter().map(|k| (k.id, &k.name)).collect()),
            artwork: db.artwork.iter().map(|a| (a.id, a.path.clone())).collect(),
            tree: db
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
                .collect(),
            entries: group(db.playlist_entries.iter().map(|e| (e.playlist_id, e.entry_index, e.track_id))),
            history: named(db.history_playlists.iter().map(|h| (h.id, &h.name)).collect()),
            history_entries: group(db.history_entries.iter().map(|e| (e.playlist_id, e.entry_index, e.track_id))),
        }
    }

    /// Without the entry lists left empty, which a file can't tell from missing ones.
    fn normalised(&self) -> Model {
        let mut m = self.clone();
        m.entries.retain(|_, l| !l.is_empty());
        m.history_entries.retain(|_, l| !l.is_empty());
        m
    }

    fn lookup(&mut self, table: TableType) -> &mut IndexMap<u32, String> {
        match table {
            TableType::Artists => &mut self.artists,
            TableType::Genres => &mut self.genres,
            TableType::Labels => &mut self.labels,
            TableType::Keys => &mut self.keys,
            other => unreachable!("{other:?} isn't a name lookup"),
        }
    }
}

/// Renumber a list's entries 1..n in their order.
fn renumber(list: &mut [(u32, u32)]) {
    list.sort_unstable();
    for (i, e) in list.iter_mut().enumerate() {
        e.0 = i as u32 + 1;
    }
}

/// A name: mostly short ones from a pool (some already in the fixtures, so lookups get reused),
/// any text, long ASCII and UTF-16 ones, ones too big for a page and, rarely, too long to encode.
#[hegel::test_helper]
fn name(tc: &TestCase) -> String {
    match tc.draw(gs::integers::<u8>().max_value(19)) {
        0..=9 => tc.draw(gs::sampled_from(vec!["Techno", "Em", "BABY GRAVY", "aac", "a", "b", "Mix", ""])).to_string(),
        10..=12 => tc.draw(gs::text().max_size(12)),
        13 => "n".repeat(tc.draw(gs::integers::<usize>().min_value(100).max_value(300))),
        14 => "é".repeat(tc.draw(gs::integers::<usize>().min_value(50).max_value(150))),
        15..=17 => tc.draw(gs::text().alphabet("abcdefgh .-").max_size(20)),
        18 => {
            let c = tc.draw(gs::sampled_from(vec!['x', 'ü']));
            c.to_string().repeat(tc.draw(gs::integers::<usize>().min_value(1500).max_value(5000)))
        }
        // Around the longest strings DeviceSQL can encode, in ASCII and UTF-16.
        _ => match tc.draw(gs::booleans()) {
            true => "x".repeat(tc.draw(gs::integers::<usize>().min_value(32_760).max_value(32_770))),
            false => "ü".repeat(tc.draw(gs::integers::<usize>().min_value(16_378).max_value(16_385))),
        },
    }
}

/// One of `items` (for types the generators can't print).
#[hegel::test_helper]
fn pick<T: Copy>(tc: &TestCase, items: &[T]) -> T {
    items[tc.draw(gs::integers::<usize>().max_value(items.len() - 1))]
}

/// An id among `known` (usually), else any small id or any id at all.
#[hegel::test_helper]
fn some_id(tc: &TestCase, known: Vec<u32>) -> u32 {
    if !known.is_empty() && tc.draw(gs::weighted_booleans(0.85)) {
        tc.draw(gs::sampled_from(known))
    } else {
        tc.draw(hegel::one_of!(gs::integers::<u32>().max_value(40), gs::integers::<u32>()))
    }
}

/// Up to three ids, each as [`some_id`] draws it.
#[hegel::test_helper]
fn some_ids(tc: &TestCase, known: Vec<u32>) -> Vec<u32> {
    (0..tc.draw(gs::integers::<usize>().max_value(3))).map(|_| some_id(tc, known.clone())).collect()
}

/// The bytes DeviceSQL encodes `s` in, or `None` if it can't (FORMAT.md's string framing).
fn encoded_len(s: &str) -> Option<usize> {
    let len = if s.is_ascii() {
        if s.len() <= 126 {
            s.len() + 1
        } else {
            s.len() + 4
        }
    } else {
        s.encode_utf16().count() * 2 + 4
    };
    (len <= 0x7fff).then_some(len)
}

/// The bytes an ISRC (a track's string 0) takes: rekordbox frames a non-empty one as a `0x90`
/// string holding `0x03`, the code and a NUL.
fn isrc_len(s: &str) -> Option<usize> {
    if s.is_empty() {
        Some(1)
    } else {
        (s.len() + 6 <= 0x7fff).then_some(s.len() + 6)
    }
}

/// How an edit that writes rows should end.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Outcome {
    Done,
    TooLarge,
    TooLong,
}

/// How `result` ended (a refusal other than these two is a bug in the edit or the model).
fn outcome<T>(result: &Result<T, PdbError>) -> Outcome {
    match result {
        Ok(_) => Outcome::Done,
        Err(PdbError::RowTooLarge { alloc, max }) if alloc > max => Outcome::TooLarge,
        Err(PdbError::StringTooLong) => Outcome::TooLong,
        Err(e) => panic!("unexpected refusal: {e}"),
    }
}

/// The outcomes allowed for an edit that checks rows in turn, each with a `fixed`-byte part and
/// strings of these encoded lengths (`None`: can't be encoded), against a 4096-byte page less
/// its 40-byte header and a one-row directory (4050 bytes). Near the limit, the padding
/// between strings decides, so either outcome is allowed there.
fn allowed(rows: &[(usize, Vec<Option<usize>>)]) -> Vec<Outcome> {
    let mut out = Vec::new();
    for (fixed, lens) in rows {
        let Some(lens) = lens.iter().copied().collect::<Option<Vec<usize>>>() else {
            out.push(Outcome::TooLong);
            return out;
        };
        let size = fixed + lens.iter().sum::<usize>();
        if size > 4050 {
            out.push(Outcome::TooLarge);
            return out;
        }
        if size + 4 * lens.len() + 8 > 4050 {
            out.push(Outcome::TooLarge);
        }
    }
    out.push(Outcome::Done);
    out
}

/// One row named `name` after a `fixed`-byte part.
fn named_row(fixed: usize, name: &str) -> Vec<(usize, Vec<Option<usize>>)> {
    vec![(fixed, vec![encoded_len(name)])]
}

static SAVES: AtomicUsize = AtomicUsize::new(0);

struct Editing {
    ed: PdbEditor,
    model: Model,
}

impl Editing {
    fn now(&self) -> Database {
        self.ed.database().unwrap()
    }

    fn tracks(&self) -> Vec<u32> {
        self.model.tracks.keys().copied().collect()
    }

    fn nodes(&self) -> Vec<u32> {
        self.model.tree.keys().copied().collect()
    }

    /// What `check_parent` accepts: the top level or an existing folder.
    fn parent_ok(&self, parent: u32) -> bool {
        parent == 0 || self.model.tree.get(&parent).is_some_and(|n| n.is_folder)
    }

    /// The top level, a folder (usually), or any node or id.
    fn some_parent(&self, tc: &TestCase) -> u32 {
        let folders: Vec<u32> = self.model.tree.iter().filter(|(_, n)| n.is_folder).map(|(&id, _)| id).collect();
        match tc.draw(gs::integers::<u8>().max_value(3)) {
            0 => 0,
            1 | 2 if !folders.is_empty() => tc.draw(gs::sampled_from(folders)),
            _ => some_id(tc, self.nodes()),
        }
    }

    fn is_playlist(&self, id: u32) -> bool {
        self.model.tree.get(&id).is_some_and(|n| !n.is_folder)
    }

    /// A lookup by name: an existing row with that name, else a new row with an unused id.
    fn expect_lookup(&mut self, table: TableType, name: &str, got: u32) {
        let rows = self.model.lookup(table);
        match rows.iter().any(|(_, n)| n == name) {
            true => assert_eq!(rows.get(&got).map(String::as_str), Some(name), "{table:?} {name:?} isn't reused"),
            false => assert!(rows.insert(got, name.to_string()).is_none(), "{table:?} id {got} was in use"),
        }
    }

    /// The model's moves, refused as `move_playlist` documents.
    fn expect_move(&mut self, id: u32, parent: u32, sort_order: u32, result: Result<(), PdbError>) {
        if !self.parent_ok(parent) {
            return assert!(matches!(result, Err(PdbError::PlaylistNotFound(p)) if p == parent), "{result:?}");
        }
        if !self.model.tree.contains_key(&id) {
            return assert!(matches!(result, Err(PdbError::PlaylistNotFound(p)) if p == id), "{result:?}");
        }
        // Into itself or one of its own folders.
        let ancestors = std::iter::successors(Some(parent), |p| self.model.tree.get(p).map(|n| n.parent));
        if ancestors.take(self.model.tree.len() + 1).take_while(|&p| p != 0).any(|p| p == id) {
            return assert!(matches!(result, Err(PdbError::PlaylistNotFound(p)) if p == parent), "{result:?}");
        }
        result.unwrap();
        let node = self.model.tree.get_mut(&id).unwrap();
        (node.parent, node.sort_order) = (parent, sort_order);
    }

    fn check(&mut self) {
        let bytes = self.ed.to_bytes();
        let db = Database::from_bytes(bytes.clone()).unwrap();
        assert_eq!(Model::read(&db, &bytes), self.model.normalised());
    }
}

#[hegel::state_machine]
impl Editing {
    #[rule(weight = 3)]
    fn add_track(&mut self, tc: TestCase) {
        let lookup = |tc: &TestCase| if tc.draw(gs::booleans()) { Some(name(tc)) } else { None };
        let ext = tc.draw(gs::sampled_from(vec!["mp3", "M4A", "flac", "wav", "aif", "ogg", ""]));
        let path = format!("/Contents/{}.{ext}", tc.draw(gs::text().alphabet("abc/").max_size(10)));
        let t = NewTrack {
            filename: if tc.draw(gs::booleans()) { Some(name(&tc)) } else { None },
            artist: lookup(&tc),
            album: lookup(&tc),
            genre: lookup(&tc),
            key: lookup(&tc),
            label: lookup(&tc),
            comment: name(&tc),
            tempo: tc.draw(gs::integers()),
            year: tc.draw(gs::integers()),
            rating: tc.draw(gs::integers()),
            color_id: tc.draw(gs::integers()),
            duration: tc.draw(gs::integers()),
            ..NewTrack::new(name(&tc), path)
        };
        let checked = self.now().check_new_track(&t);
        let result = self.ed.add_track(&t);
        assert_eq!(format!("{checked:?}"), format!("{:?}", result.as_ref().map(|_| ())), "check_new_track disagrees");
        let filename = t.filename.clone().unwrap_or_else(|| t.file_path.rsplit('/').next().unwrap().to_string());
        let strings = [t.title.as_str(), &t.comment, &filename, &t.file_path];
        // The track row: 0x88 bytes of fields and its 21 string offsets, the 17 other strings
        // each padded to 4 bytes (two "2"s, two "ON"s, the dates added and released, the mix
        // name, the analysis path, the rest empty), these four, and 4 bytes more. Then each lookup
        // row it may add: one that can't be added refuses the track before anything is.
        let others = [&t.date_added, &t.date_added, &t.release_date, &t.mix_name, &t.analyze_path];
        let padded = |s: &str| encoded_len(s).map(|n| n.div_ceil(4) * 4);
        let fixed = 0x88
            + 4
            + 4 * padded("2").unwrap()
            + 8 * padded("").unwrap()
            + others.iter().map(|s| padded(s)).sum::<Option<usize>>().unwrap_or(usize::MAX / 2);
        let mut rows = vec![(fixed, strings.iter().map(|s| encoded_len(s)).collect())];
        for (fixed, name) in [(14, &t.artist), (26, &t.album), (4, &t.genre), (8, &t.key), (4, &t.label)] {
            rows.extend(name.as_deref().map(|n| (fixed, vec![encoded_len(n)])));
        }
        let got = outcome(&result);
        assert!(allowed(&rows).contains(&got), "add_track: {got:?}, allowed {:?}", allowed(&rows));
        let Ok(id) = result else { return };
        assert!(!self.model.tracks.contains_key(&id), "track id {id} was in use");
        let bytes = self.ed.to_bytes();
        let db = Database::from_bytes(bytes.clone()).unwrap();
        let (row, &loc) = db.tracks.iter().zip(db.row_locations(TableType::Tracks)).find(|(r, _)| r.id == id).unwrap();
        let read = TrackRow::read(row, u16::from_le_bytes([bytes[loc + 0x5a], bytes[loc + 0x5b]]));
        assert_eq!(
            (row.title(), row.comment(), row.filename(), row.file_path()),
            (strings[0], strings[1], strings[2], strings[3])
        );
        assert_eq!(read.fields["file_type"], u32::from(file_type_code(&t.file_path)));
        assert_eq!(
            (row.tempo, row.year, row.rating, row.color_id, row.duration),
            (t.tempo, t.year, t.rating, t.color_id, t.duration)
        );
        for (table, name, field) in [
            (TableType::Artists, &t.artist, "artist_id"),
            (TableType::Genres, &t.genre, "genre_id"),
            (TableType::Keys, &t.key, "key_id"),
            (TableType::Labels, &t.label, "label_id"),
        ] {
            match name {
                Some(n) => self.expect_lookup(table, n, read.fields[field]),
                None => assert_eq!(read.fields[field], 0, "{field}"),
            }
        }
        let album = read.fields["album_id"];
        match &t.album {
            Some(n) => {
                let key = (n.clone(), read.fields["artist_id"]);
                if self.model.albums.values().any(|a| *a == key) {
                    assert_eq!(self.model.albums.get(&album), Some(&key), "album {key:?} isn't reused");
                } else {
                    assert!(self.model.albums.insert(album, key).is_none(), "album id {album} was in use");
                }
            }
            None => assert_eq!(album, 0),
        }
        self.model.tracks.insert(id, read);
    }

    #[rule(weight = 2)]
    fn set_track_field(&mut self, tc: TestCase) {
        let id = some_id(&tc, self.tracks());
        let (field, size) =
            tc.draw(gs::sampled_from(FIELDS.iter().copied().chain([("title", 0), ("", 0)]).collect::<Vec<_>>()));
        // Values that fit, the first that doesn't, or any.
        let limit = 1u64 << (8 * size.clamp(1, 4));
        let value = match tc.draw(gs::integers::<u8>().max_value(3)) {
            0 | 1 => tc.draw(gs::integers::<u64>().max_value(limit - 1)) as u32,
            2 => limit.min(u64::from(u32::MAX)) as u32,
            _ => tc.draw(gs::integers::<u32>()),
        };
        let result = self.ed.set_track_field(id, field, value);
        match (size, self.model.tracks.get_mut(&id)) {
            (0, _) => assert!(matches!(result, Err(PdbError::UnknownField(f)) if f == field)),
            (s, _) if s < 4 && value >> (8 * s) != 0 => {
                let Err(PdbError::ValueOutOfRange { field: f, value: v, size: z }) = result else {
                    panic!("{result:?}")
                };
                assert_eq!((f.as_str(), v, z), (field, u64::from(value), s));
            }
            (_, None) => assert!(matches!(result, Err(PdbError::TrackNotFound(t)) if t == id)),
            (_, Some(row)) => {
                result.unwrap();
                row.fields.insert(field, value);
            }
        }
    }

    #[rule]
    fn set_track_strings(&mut self, tc: TestCase) {
        let id = some_id(&tc, self.tracks());
        let count = tc.draw(gs::integers::<usize>().max_value(3));
        let mut values = Vec::new();
        for _ in 0..count {
            // The ISRC, framed apart from the other strings, more often than its share.
            let field = match tc.draw(gs::integers::<u8>().max_value(9)) {
                0 => "tempo".to_string(),
                1 | 2 => "isrc".to_string(),
                _ => tc.draw(gs::sampled_from(TRACK_STRING_NAMES.to_vec())).to_string(),
            };
            values.push((field, name(&tc)));
        }
        let pairs: Vec<(&str, &str)> = values.iter().map(|(f, v)| (f.as_str(), v.as_str())).collect();
        let db = self.now();
        let result = self.ed.set_track_strings(id, &pairs);
        let Some(row) = self.model.tracks.get_mut(&id) else {
            return assert!(matches!(result, Err(PdbError::TrackNotFound(t)) if t == id), "{result:?}");
        };
        // The reader's own check of the same edit agrees.
        let track = db.tracks.iter().find(|t| t.id == id).unwrap();
        assert_eq!(format!("{:?}", db.check_track_strings(track, &pairs)), format!("{result:?}"));
        if let Some(unknown) = pairs.iter().find(|(f, _)| !TRACK_STRING_NAMES.contains(f)) {
            return assert!(matches!(&result, Err(PdbError::UnknownField(f)) if f == unknown.0), "{result:?}");
        }
        let mut strings = row.strings.clone();
        for (f, v) in &pairs {
            strings[TRACK_STRING_NAMES.iter().position(|n| n == f).unwrap()] = v.to_string();
        }
        if strings == row.strings {
            return result.unwrap();
        }
        let lens = strings.iter().enumerate().map(|(i, s)| if i == 0 { isrc_len(s) } else { encoded_len(s) });
        let rows = [(0x8c, lens.collect())];
        assert!(allowed(&rows).contains(&outcome(&result)), "set_track_strings: {result:?}");
        if result.is_ok() {
            row.strings = strings;
        }
    }

    #[rule]
    fn remove_tracks(&mut self, tc: TestCase) {
        // Some ids, or every track (an export left with none).
        let ids = match tc.draw(gs::weighted_booleans(0.1)) {
            true => self.tracks(),
            false => some_ids(&tc, self.tracks()),
        };
        let result = self.ed.remove_tracks(&ids);
        if ids.iter().any(|i| !self.model.tracks.contains_key(i)) {
            // Any of the missing ones.
            let Err(PdbError::TrackNotFound(t)) = result else { panic!("{result:?}") };
            return assert!(!self.model.tracks.contains_key(&t) && ids.contains(&t), "{t}");
        }
        result.unwrap();
        for id in &ids {
            self.model.tracks.shift_remove(id);
        }
        for list in self.model.entries.values_mut().chain(self.model.history_entries.values_mut()) {
            let before = list.len();
            list.retain(|e| !ids.contains(&e.1));
            if list.len() != before {
                renumber(list);
            }
        }
    }

    #[rule(weight = 2)]
    fn remove_rows(&mut self, tc: TestCase) {
        let tables = vec![
            TableType::Artists,
            TableType::Albums,
            TableType::Genres,
            TableType::Labels,
            TableType::Keys,
            TableType::Artwork,
            TableType::PlaylistTree,
            TableType::HistoryPlaylists,
            TableType::Tracks,
            TableType::Colors,
        ];
        let table = pick(&tc, &tables);
        let m = &self.model;
        let known: Vec<u32> = match table {
            TableType::Artists => m.artists.keys().copied().collect(),
            TableType::Albums => m.albums.keys().copied().collect(),
            TableType::Genres => m.genres.keys().copied().collect(),
            TableType::Labels => m.labels.keys().copied().collect(),
            TableType::Keys => m.keys.keys().copied().collect(),
            TableType::Artwork => m.artwork.keys().copied().collect(),
            TableType::PlaylistTree => m.tree.keys().copied().collect(),
            TableType::HistoryPlaylists => m.history.keys().copied().collect(),
            _ => vec![1],
        };
        let ids = some_ids(&tc, known);
        let result = self.ed.remove_rows(table, &ids);
        if matches!(table, TableType::Tracks | TableType::Colors) && !ids.is_empty() {
            let Err(e) = result else { panic!("removed {table:?} rows") };
            assert!(matches!(e, PdbError::UnsupportedTable(t) if t == table as u32));
            return assert!(e.to_string().contains(&(table as u32).to_string()));
        }
        let mut removed = 0;
        for id in &ids {
            let gone = match table {
                TableType::Albums => self.model.albums.shift_remove(id).is_some(),
                TableType::Artwork => self.model.artwork.shift_remove(id).is_some(),
                TableType::PlaylistTree => self.model.tree.shift_remove(id).is_some(),
                TableType::HistoryPlaylists => self.model.history.shift_remove(id).is_some(),
                TableType::Tracks | TableType::Colors => false,
                other => self.model.lookup(other).shift_remove(id).is_some(),
            };
            removed += usize::from(gone);
        }
        assert_eq!(result.unwrap(), removed);
    }

    /// A row added behind the editor's back (a second genre or label with a name already in
    /// use), which the editor finds when it next parses the file.
    #[rule]
    fn append_raw_duplicate(&mut self, tc: TestCase) {
        let table = pick(&tc, &[TableType::Genres, TableType::Labels]);
        let rows = self.model.lookup(table);
        let names: Vec<String> = rows.values().filter(|n| n.len() <= 20 && n.is_ascii()).cloned().collect();
        tc.assume(!names.is_empty());
        let name = tc.draw(gs::sampled_from(names));
        let id = rows.keys().max().unwrap() + tc.draw(gs::integers::<u32>().min_value(1).max_value(3));
        // An id, then the name as a short ASCII string: its kind byte is (length + 1) * 2 + 1.
        let mut row = id.to_le_bytes().to_vec();
        row.push(((name.len() + 1) * 2 + 1) as u8);
        row.extend_from_slice(name.as_bytes());
        let alloc = row.len().next_multiple_of(4);
        self.ed.append_raw_row(table as u32, &row, alloc, None).unwrap();
        self.model.lookup(table).insert(id, name);
    }

    #[rule]
    fn lookup(&mut self, tc: TestCase) {
        let table =
            pick(&tc, &[TableType::Artists, TableType::Albums, TableType::Genres, TableType::Labels, TableType::Keys]);
        let name = name(&tc);
        let albums: Vec<(String, u32)> = self.model.albums.values().cloned().collect();
        let (name, artist) = if table == TableType::Albums && !albums.is_empty() && tc.draw(gs::booleans()) {
            albums[pick(&tc, &(0..albums.len()).collect::<Vec<_>>())].clone()
        } else {
            (name, some_id(&tc, self.model.artists.keys().copied().collect()))
        };
        let exists = match table {
            TableType::Albums => self.model.albums.values().any(|a| a.0 == name && a.1 == artist),
            other => self.model.lookup(other).values().any(|n| *n == name),
        };
        let checked = self.now().check_lookup_name(table, &name);
        let result = match table {
            TableType::Artists => self.ed.artist_id_for(&name),
            TableType::Albums => self.ed.album_id_for(&name, artist),
            TableType::Genres => self.ed.genre_id_for(&name),
            TableType::Labels => self.ed.label_id_for(&name),
            _ => self.ed.key_id_for(&name),
        };
        if exists {
            return match table {
                TableType::Albums => assert_eq!(self.model.albums.get(&result.unwrap()), Some(&(name, artist))),
                other => self.expect_lookup(other, &name, result.unwrap()),
            };
        }
        assert_eq!(checked.is_ok(), result.is_ok(), "check_lookup_name disagrees: {checked:?} {result:?}");
        let fixed = match table {
            TableType::Artists => 14,
            TableType::Albums => 26,
            TableType::Keys => 8,
            _ => 4,
        };
        assert!(allowed(&named_row(fixed, &name)).contains(&outcome(&result)), "{table:?}: {result:?}");
        let Ok(id) = result else { return };
        match table {
            TableType::Albums => assert!(self.model.albums.insert(id, (name, artist)).is_none()),
            other => self.expect_lookup(other, &name, id),
        }
    }

    #[rule]
    fn add_artwork(&mut self, tc: TestCase) {
        let path = format!("/PIONEER/Artwork/00001/{}.jpg", tc.draw(gs::text().alphabet("ab").max_size(40)));
        let id = self.ed.add_artwork(&path).unwrap();
        assert!(self.model.artwork.insert(id, path).is_none(), "artwork id {id} was in use");
    }

    #[rule(weight = 2)]
    fn create_playlist(&mut self, tc: TestCase) {
        let parent = self.some_parent(&tc);
        let (name, is_folder) = (name(&tc), tc.draw(gs::booleans()));
        let checked = self.now().check_playlist_name(&name);
        let result = self.ed.create_playlist(&name, parent, is_folder);
        if !self.parent_ok(parent) {
            return assert!(matches!(result, Err(PdbError::PlaylistNotFound(p)) if p == parent), "{result:?}");
        }
        assert_eq!(checked.is_ok(), result.is_ok(), "check_playlist_name disagrees: {checked:?} {result:?}");
        assert!(allowed(&named_row(20, &name)).contains(&outcome(&result)), "create_playlist: {result:?}");
        let Ok(id) = result else { return };
        // The last child of its parent.
        let siblings = self.model.tree.values().filter(|n| n.parent == parent);
        let sort_order = siblings.map(|n| n.sort_order + 1).max().unwrap_or(0);
        let node = Node { parent, sort_order, is_folder, name };
        assert!(self.model.tree.insert(id, node).is_none(), "playlist id {id} was in use");
    }

    #[rule]
    fn rename_playlist(&mut self, tc: TestCase) {
        let id = some_id(&tc, self.nodes());
        let name = name(&tc);
        let checked = self.now().check_playlist_name(&name);
        let result = self.ed.rename_playlist(id, &name);
        let Some(node) = self.model.tree.get_mut(&id) else {
            return assert!(matches!(result, Err(PdbError::PlaylistNotFound(p)) if p == id), "{result:?}");
        };
        assert_eq!(checked.is_ok(), result.is_ok(), "check_playlist_name disagrees: {checked:?} {result:?}");
        assert!(allowed(&named_row(20, &name)).contains(&outcome(&result)), "rename_playlist: {result:?}");
        if result.is_ok() {
            node.name = name;
        }
    }

    #[rule]
    fn move_playlist(&mut self, tc: TestCase) {
        let id = some_id(&tc, self.nodes());
        let parent = self.some_parent(&tc);
        let sort_order = tc.draw(gs::integers::<u32>().max_value(10_000));
        let result = self.ed.move_playlist(id, parent, sort_order);
        self.expect_move(id, parent, sort_order, result);
    }

    #[rule]
    fn set_sort_order(&mut self, tc: TestCase) {
        let id = some_id(&tc, self.nodes());
        let sort_order = tc.draw(gs::integers::<u32>().max_value(10_000));
        let result = self.ed.set_sort_order(id, sort_order);
        match self.model.tree.get(&id) {
            Some(n) => self.expect_move(id, n.parent, sort_order, result),
            None => assert!(matches!(result, Err(PdbError::PlaylistNotFound(p)) if p == id), "{result:?}"),
        }
    }

    #[rule]
    fn remove_playlists(&mut self, tc: TestCase) {
        let ids = some_ids(&tc, self.nodes());
        let result = self.ed.remove_playlists(&ids);
        if let Some(&missing) = ids.iter().find(|i| !self.model.tree.contains_key(*i)) {
            return assert!(matches!(result, Err(PdbError::PlaylistNotFound(p)) if p == missing), "{result:?}");
        }
        result.unwrap();
        for id in &ids {
            self.model.tree.shift_remove(id);
            self.model.entries.shift_remove(id);
        }
    }

    #[rule(weight = 2)]
    fn add_to_playlist(&mut self, tc: TestCase) {
        let playlist = some_id(&tc, self.nodes());
        let track = some_id(&tc, self.tracks());
        let index =
            if tc.draw(gs::weighted_booleans(0.2)) { Some(tc.draw(gs::integers::<u32>().max_value(20))) } else { None };
        let result = self.ed.add_to_playlist(playlist, track, index);
        if !self.is_playlist(playlist) {
            return assert!(matches!(result, Err(PdbError::PlaylistNotFound(p)) if p == playlist), "{result:?}");
        }
        if !self.model.tracks.contains_key(&track) {
            return assert!(matches!(result, Err(PdbError::TrackNotFound(t)) if t == track), "{result:?}");
        }
        result.unwrap();
        let list = self.model.entries.entry(playlist).or_default();
        let index = index.unwrap_or_else(|| list.iter().map(|e| e.0).max().unwrap_or(0) + 1);
        list.push((index, track));
        list.sort_unstable();
    }

    /// Replace a playlist's contents, sometimes with enough entries to fill pages of them.
    #[rule]
    fn set_playlist_entries(&mut self, tc: TestCase) {
        let playlist = some_id(&tc, self.nodes());
        let known = self.tracks();
        let counts =
            hegel::one_of!(gs::just(0), gs::integers::<usize>().max_value(6), gs::integers::<usize>().max_value(700));
        let count = tc.draw(counts);
        let skip = tc.draw(gs::integers::<usize>().max_value(known.len().max(1) - 1));
        let mut tracks: Vec<u32> = known.iter().copied().cycle().skip(skip).take(count).collect();
        if known.is_empty() || tc.draw(gs::weighted_booleans(0.1)) {
            let at = tc.draw(gs::integers::<usize>().max_value(tracks.len()));
            tracks.insert(at, tc.draw(gs::integers::<u32>()));
        }
        let result = self.ed.set_playlist_entries(playlist, &tracks);
        if !self.is_playlist(playlist) {
            return assert!(matches!(result, Err(PdbError::PlaylistNotFound(p)) if p == playlist), "{result:?}");
        }
        if let Some(&missing) = tracks.iter().find(|t| !self.model.tracks.contains_key(*t)) {
            return assert!(matches!(result, Err(PdbError::TrackNotFound(t)) if t == missing), "{result:?}");
        }
        result.unwrap();
        self.model.entries.insert(playlist, tracks.into_iter().zip(1..).map(|(t, i)| (i, t)).collect());
    }

    #[rule]
    fn repoint_track(&mut self, tc: TestCase) {
        let (from, to) = (some_id(&tc, self.tracks()), some_id(&tc, self.tracks()));
        let result = self.ed.repoint_track(from, to);
        if let Some(&missing) = [from, to].iter().find(|t| !self.model.tracks.contains_key(*t)) {
            return assert!(matches!(result, Err(PdbError::TrackNotFound(t)) if t == missing), "{result:?}");
        }
        let mut changed = 0;
        for list in self.model.entries.values_mut().chain(self.model.history_entries.values_mut()) {
            for e in list.iter_mut().filter(|e| e.1 == from) {
                e.1 = to;
                changed += 1;
            }
            list.sort_unstable();
        }
        assert_eq!(result.unwrap(), changed);
    }

    #[rule]
    fn create_history_playlist(&mut self, tc: TestCase) {
        let name = name(&tc);
        let checked = self.now().check_history_name(&name);
        let result = self.ed.create_history_playlist(&name);
        assert_eq!(checked.is_ok(), result.is_ok(), "check_history_name disagrees: {checked:?} {result:?}");
        assert!(allowed(&named_row(4, &name)).contains(&outcome(&result)), "create_history_playlist: {result:?}");
        if let Ok(id) = result {
            assert!(self.model.history.insert(id, name).is_none(), "history id {id} was in use");
        }
    }

    /// History entries name any track id: the editor doesn't check them.
    #[rule(weight = 2)]
    fn add_to_history(&mut self, tc: TestCase) {
        let history = some_id(&tc, self.model.history.keys().copied().collect());
        let track = some_id(&tc, self.tracks());
        let result = self.ed.add_to_history(history, track);
        if !self.model.history.contains_key(&history) {
            return assert!(matches!(result, Err(PdbError::PlaylistNotFound(p)) if p == history), "{result:?}");
        }
        result.unwrap();
        let list = self.model.history_entries.entry(history).or_default();
        list.push((list.iter().map(|e| e.0).max().unwrap_or(0) + 1, track));
    }

    #[rule]
    fn remove_history_playlists(&mut self, tc: TestCase) {
        let ids = some_ids(&tc, self.model.history.keys().copied().collect());
        let result = self.ed.remove_history_playlists(&ids);
        if let Some(&missing) = ids.iter().find(|i| !self.model.history.contains_key(*i)) {
            return assert!(matches!(result, Err(PdbError::PlaylistNotFound(p)) if p == missing), "{result:?}");
        }
        result.unwrap();
        for id in &ids {
            self.model.history.shift_remove(id);
            self.model.history_entries.shift_remove(id);
        }
    }

    /// Deleting where no row starts is refused, and changes nothing.
    #[rule]
    fn delete_where_no_row_starts(&mut self, tc: TestCase) {
        let db = self.now();
        let table = pick(&tc, &[TableType::Tracks, TableType::Keys, TableType::PlaylistEntries]);
        let locs = db.row_locations(table);
        tc.assume(!locs.is_empty());
        let loc = tc.draw(gs::sampled_from(locs.to_vec())) + tc.draw(gs::integers::<usize>().min_value(1).max_value(3));
        tc.assume(!locs.contains(&loc));
        let result = self.ed.delete_raw_row(table as u32, loc);
        assert!(matches!(result, Err(PdbError::RowNotFound(l)) if l == loc), "{result:?}");
    }

    /// Save to a file and go on editing the file read back.
    #[rule]
    fn save_and_reopen(&mut self, _: TestCase) {
        let path = std::env::temp_dir().join(format!(
            "rekordbox-pdb-props-{}-{}.pdb",
            std::process::id(),
            SAVES.fetch_add(1, Ordering::Relaxed)
        ));
        self.ed.save(&path).unwrap();
        let reopened = PdbEditor::from_file(&path);
        let read = Database::from_file(&path);
        std::fs::remove_file(&path).unwrap();
        let bytes = self.ed.to_bytes();
        assert_eq!(Model::read(&read.unwrap(), &bytes), self.model.normalised());
        self.ed = reopened.unwrap();
        assert_eq!(self.ed.to_bytes(), bytes);
    }

    #[invariant]
    fn saved_file_matches(&mut self, _: TestCase) {
        self.check();
    }
}

/// Any sequence of edits, refused or not, gives a file the reader sees as the model has it.
#[hegel::test(test_cases = 150)]
fn edits_match_the_model(tc: TestCase) {
    let mut bytes = std::fs::read(tc.draw(gs::sampled_from(FIXTURES.to_vec()))).unwrap();
    let start = tc.draw(gs::integers::<u8>().max_value(2));
    if start == 1 {
        // An export left with no tracks.
        let mut ed = PdbEditor::from_bytes(bytes);
        let ids: Vec<u32> = ed.database().unwrap().tracks.iter().map(|t| t.id).collect();
        ed.remove_tracks(&ids).unwrap();
        bytes = ed.to_bytes();
    } else if start == 2 {
        // Start from a file that already has what's slow to build one rule at a time: folders
        // two deep with a playlist in them, and a history list, each holding entries of two
        // tracks.
        let mut ed = PdbEditor::from_bytes(bytes);
        let outer = ed.create_playlist("Outer", 0, true).unwrap();
        let inner = ed.create_playlist("Inner", outer, true).unwrap();
        let list = ed.create_playlist("List", inner, false).unwrap();
        let history = ed.create_history_playlist("HISTORY 001").unwrap();
        // Two tracks in turn, so removing either leaves entries to renumber.
        let other = ed.add_track(&NewTrack::new("Other", "/Contents/other.mp3")).unwrap();
        for i in 0..tc.draw(gs::integers::<u32>().min_value(2).max_value(4)) {
            let track = if i % 2 == 0 { 1 } else { other };
            ed.add_to_playlist(list, track, None).unwrap();
            ed.add_to_history(history, track).unwrap();
        }
        bytes = ed.to_bytes();
    }
    let model = Model::read(&Database::from_bytes(bytes.clone()).unwrap(), &bytes);
    hegel::stateful::machine(Editing { ed: PdbEditor::from_bytes(bytes), model }).steps(30).run(tc);
}

/// A track string is refused as too long exactly when DeviceSQL can't frame it (an ISRC has its
/// own framing, 6 bytes on top of the code), and otherwise, being far bigger than a page, as too
/// large; the reader's check of the same edit says the same, and the track is left as it was.
#[hegel::test(test_cases = 100)]
fn strings_too_long_to_encode_are_refused(tc: TestCase) {
    let bytes = std::fs::read(FIXTURES[0]).unwrap();
    let mut ed = PdbEditor::from_bytes(bytes.clone());
    let field =
        if tc.draw(gs::booleans()) { "isrc" } else { tc.draw(gs::sampled_from(vec!["title", "comment", "message"])) };
    // A few characters either side of the longest that can be framed: 0x7fff bytes with the
    // header, which UTF-16 spends 2 bytes a character on (and an ISRC's UTF-8, 2 on a "ü").
    let ascii = tc.draw(gs::booleans());
    let longest = match (field, ascii) {
        ("isrc", true) => 0x7fff - 6,
        ("isrc", false) => (0x7fff - 6) / 2,
        (_, true) => 0x7fff - 4,
        (_, false) => (0x7fff - 4) / 2,
    };
    let len = longest + 3 - tc.draw(gs::integers::<usize>().max_value(6));
    let value = if ascii { "x".repeat(len) } else { "ü".repeat(len) };
    let db = Database::from_bytes(bytes.clone()).unwrap();
    let checked = db.check_track_strings(&db.tracks[0], &[(field, &value)]);
    let result = ed.set_track_strings(1, &[(field, &value)]);
    let encodable = if field == "isrc" { isrc_len(&value) } else { encoded_len(&value) }.is_some();
    assert_eq!(
        outcome(&result),
        if encodable { Outcome::TooLarge } else { Outcome::TooLong },
        "{field} of {} bytes",
        value.len()
    );
    assert_eq!(format!("{checked:?}"), format!("{result:?}"));
    assert_eq!(ed.to_bytes(), bytes);
    // So does every other edit that writes a name or path, and its check (no such row fits on a
    // page), and they leave the file as it was too.
    if field != "isrc" {
        let want = if encodable { Outcome::TooLarge } else { Outcome::TooLong };
        for table in [TableType::Artists, TableType::Albums, TableType::Genres, TableType::Labels, TableType::Keys] {
            assert_eq!(outcome(&db.check_lookup_name(table, &value)), want, "{table:?}");
        }
        assert_eq!(outcome(&db.check_playlist_name(&value)), want);
        assert_eq!(outcome(&db.check_history_name(&value)), want);
        let edits = [
            outcome(&ed.artist_id_for(&value)),
            outcome(&ed.album_id_for(&value, 1)),
            outcome(&ed.genre_id_for(&value)),
            outcome(&ed.key_id_for(&value)),
            outcome(&ed.add_artwork(&value)),
            outcome(&ed.create_playlist(&value, 0, false)),
            outcome(&ed.rename_playlist(1, &value)),
            outcome(&ed.create_history_playlist(&value)),
        ];
        assert_eq!(edits, [want; 8]);
        assert_eq!(ed.to_bytes(), bytes);
    }
}
