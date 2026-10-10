//! Writing support for `export.pdb`.
//!
//! Follows rekordbox's own incremental save behaviour: fields are patched in
//! place and rows are appended to page heaps with full row-directory / page
//! bookkeeping, rather than rewriting the file.
//!
//! locallibrary patch: the editor parses the file once, into an index of where every row lives
//! (rows never move: appends go to the end of a heap and deletions only clear a presence bit),
//! and keeps that index up to date as it edits. Bulk edits no longer re-parse the whole database
//! for every row they add.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use crate::pdb::{Database, TableType, Track, TRACK_STRING_NAMES};
use crate::{
    align4, decode_isrc, decode_string, encode_isrc, encode_string, read_header, u16le, u32le, PdbError, Result,
    PAGE_HEADER_SIZE,
};

/// Fixed-width track field editable in place: name -> (row offset, size).
fn field_spec(field: &str) -> Option<(usize, usize)> {
    Some(match field {
        "sample_rate" => (0x08, 4),
        "composer_id" => (0x0c, 4),
        "file_size" => (0x10, 4),
        "artwork_id" => (0x1c, 4),
        "key_id" => (0x20, 4),
        "original_artist_id" => (0x24, 4),
        "label_id" => (0x28, 4),
        "remixer_id" => (0x2c, 4),
        "bitrate" => (0x30, 4),
        "track_number" => (0x34, 4),
        "tempo" => (0x38, 4),
        "genre_id" => (0x3c, 4),
        "album_id" => (0x40, 4),
        "artist_id" => (0x44, 4),
        "disc_number" => (0x4c, 2),
        "play_count" => (0x4e, 2),
        "year" => (0x50, 2),
        "sample_depth" => (0x52, 2),
        "duration" => (0x54, 2),
        "color_id" => (0x58, 1),
        "rating" => (0x59, 1),
        _ => return None,
    })
}

/// Size of a track row's fixed part; the 21 string offsets follow it.
const TRACK_FIXED: usize = 0x5e;
/// Where a track row's strings start (fixed part plus 21 u16 offsets).
const TRACK_STRINGS: usize = 0x88;
/// How far past the end of the file a table's next free page may lie: rekordbox reserves up to one
/// per table there (`export.pdb` has 20 tables), and a table whose free page was inside the file
/// gets one past the end once `allocate_page` uses it. (locallibrary patch.)
const MAX_PAGES_PAST_END: usize = 64;

fn file_type_code(path: &str) -> u16 {
    let ext = path.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    match ext.as_str() {
        "mp3" => 0x01,
        "m4a" => 0x04,
        "wav" => 0x0b,
        "aiff" | "aif" => 0x0c,
        _ => 0x01,
    }
}

fn check_range(field: &str, value: u64, size: usize) -> Result<()> {
    if value >= 1u64 << (8 * size) {
        return Err(PdbError::ValueOutOfRange {
            field: field.to_string(),
            value,
            size,
        });
    }
    Ok(())
}

/// Metadata for a track to add. Use `NewTrack::new(title, path)` then set
/// fields, or construct with `..Default::default()`.
#[derive(Clone, Debug)]
pub struct NewTrack {
    pub title: String,
    pub file_path: String,
    pub filename: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub genre: Option<String>,
    pub key: Option<String>,
    pub label: Option<String>,
    pub comment: String,
    pub mix_name: String,
    pub release_date: String,
    pub analyze_path: String,
    /// Defaults to empty; set to a `YYYY-MM-DD` string if desired.
    pub date_added: String,
    pub tempo: u32,
    pub duration: u16,
    pub year: u16,
    pub bitrate: u32,
    pub sample_rate: u32,
    pub sample_depth: u16,
    pub file_size: u32,
    pub track_number: u32,
    pub disc_number: u16,
    pub play_count: u16,
    pub rating: u8,
    pub color_id: u8,
    pub artwork_id: u32,
}

impl Default for NewTrack {
    fn default() -> Self {
        NewTrack {
            title: String::new(),
            file_path: String::new(),
            filename: None,
            artist: None,
            album: None,
            genre: None,
            key: None,
            label: None,
            comment: String::new(),
            mix_name: String::new(),
            release_date: String::new(),
            analyze_path: String::new(),
            date_added: String::new(),
            tempo: 0,
            duration: 0,
            year: 0,
            bitrate: 0,
            sample_rate: 44100,
            sample_depth: 16,
            file_size: 0,
            track_number: 0,
            disc_number: 0,
            play_count: 0,
            rating: 0,
            color_id: 0,
            artwork_id: 0,
        }
    }
}

impl NewTrack {
    pub fn new(title: impl Into<String>, file_path: impl Into<String>) -> NewTrack {
        NewTrack {
            title: title.into(),
            file_path: file_path.into(),
            ..Default::default()
        }
    }

    /// The file name stored with the track: `filename`, else the last part of its path.
    fn file_name(&self) -> String {
        self.filename
            .clone()
            .unwrap_or_else(|| self.file_path.rsplit('/').next().unwrap_or(&self.file_path).to_string())
    }

    /// The track row's 21 strings, with `filename` from [`NewTrack::file_name`].
    fn strings<'a>(&'a self, filename: &'a str) -> [&'a str; 21] {
        [
            "",
            "",
            "2",
            "2",
            "",
            "",
            "ON",
            "ON",
            "",
            "",
            self.date_added.as_str(),
            self.release_date.as_str(),
            self.mix_name.as_str(),
            "",
            self.analyze_path.as_str(),
            self.date_added.as_str(),
            self.comment.as_str(),
            self.title.as_str(),
            "",
            filename,
            self.file_path.as_str(),
        ]
    }
}

/// A row of an id + name lookup table (artists, genres, labels, keys).
#[derive(Default)]
struct Named {
    /// id -> (row location, name)
    rows: HashMap<u32, (usize, String)>,
    /// name -> id of the first row with that name
    by_name: HashMap<String, u32>,
}

impl Named {
    fn insert(&mut self, id: u32, loc: usize, name: &str) {
        self.rows.insert(id, (loc, name.to_string()));
        self.by_name.entry(name.to_string()).or_insert(id);
    }

    fn remove(&mut self, id: u32) -> Option<usize> {
        let (loc, name) = self.rows.remove(&id)?;
        if self.by_name.get(&name) == Some(&id) {
            self.by_name.remove(&name);
            // Another row with the same name takes over.
            if let Some((&other, _)) = self.rows.iter().filter(|(_, (_, n))| *n == name).min_by_key(|(id, _)| **id) {
                self.by_name.insert(name, other);
            }
        }
        Some(loc)
    }

    fn next_id(&self) -> u32 {
        self.rows.keys().max().copied().unwrap_or(0) + 1
    }
}

#[derive(Clone, Copy)]
struct Node {
    loc: usize,
    parent: u32,
    sort_order: u32,
    is_folder: bool,
}

/// A playlist (or history) entry row.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct EntryRow {
    index: u32,
    track: u32,
    loc: usize,
}

/// Where every row the editor works with lives, kept current as rows are added and removed.
#[derive(Default)]
struct Index {
    tracks: HashMap<u32, usize>,
    track_unique_max: u32,
    artists: Named,
    /// id -> (row location, name, artist id)
    albums: HashMap<u32, (usize, String, u32)>,
    album_by_name: HashMap<(String, u32), u32>,
    genres: Named,
    labels: Named,
    keys: Named,
    artwork: HashMap<u32, usize>,
    tree: HashMap<u32, Node>,
    entries: HashMap<u32, Vec<EntryRow>>,
    history: HashMap<u32, usize>,
    history_entries: HashMap<u32, Vec<EntryRow>>,
}

impl Index {
    fn build(db: &Database, buf: &[u8]) -> Index {
        let mut ix = Index::default();
        for (t, &loc) in db.tracks.iter().zip(db.row_locations(TableType::Tracks)) {
            ix.tracks.insert(t.id, loc);
            ix.track_unique_max = ix.track_unique_max.max(u32le(buf, loc + 0x14));
        }
        if ix.tracks.is_empty() {
            ix.track_unique_max = 0x0010_0000;
        }
        for (a, &loc) in db.artists.iter().zip(db.row_locations(TableType::Artists)) {
            ix.artists.insert(a.id, loc, &a.name);
        }
        for (a, &loc) in db.albums.iter().zip(db.row_locations(TableType::Albums)) {
            ix.albums.insert(a.id, (loc, a.name.clone(), a.artist_id));
            ix.album_by_name.entry((a.name.clone(), a.artist_id)).or_insert(a.id);
        }
        for (g, &loc) in db.genres.iter().zip(db.row_locations(TableType::Genres)) {
            ix.genres.insert(g.id, loc, &g.name);
        }
        for (l, &loc) in db.labels.iter().zip(db.row_locations(TableType::Labels)) {
            ix.labels.insert(l.id, loc, &l.name);
        }
        for (k, &loc) in db.keys.iter().zip(db.row_locations(TableType::Keys)) {
            ix.keys.insert(k.id, loc, &k.name);
        }
        for (a, &loc) in db.artwork.iter().zip(db.row_locations(TableType::Artwork)) {
            ix.artwork.insert(a.id, loc);
        }
        for (n, &loc) in db.playlist_tree.iter().zip(db.row_locations(TableType::PlaylistTree)) {
            ix.tree.insert(
                n.id,
                Node { loc, parent: n.parent_id, sort_order: n.sort_order, is_folder: n.is_folder },
            );
        }
        for (e, &loc) in db.playlist_entries.iter().zip(db.row_locations(TableType::PlaylistEntries)) {
            ix.entries.entry(e.playlist_id).or_default().push(EntryRow {
                index: e.entry_index,
                track: e.track_id,
                loc,
            });
        }
        for (h, &loc) in db.history_playlists.iter().zip(db.row_locations(TableType::HistoryPlaylists)) {
            ix.history.insert(h.id, loc);
        }
        for (e, &loc) in db.history_entries.iter().zip(db.row_locations(TableType::HistoryEntries)) {
            ix.history_entries.entry(e.playlist_id).or_default().push(EntryRow {
                index: e.entry_index,
                track: e.track_id,
                loc,
            });
        }
        for list in ix.entries.values_mut().chain(ix.history_entries.values_mut()) {
            list.sort_unstable();
        }
        ix
    }
}

/// The allocation of a lookup row (artist, album, genre, label or key) whose name encodes to
/// `len` bytes. (locallibrary patch.)
fn lookup_alloc(table: TableType, len: usize) -> usize {
    match table {
        TableType::Artists => align4(10 + align4(len)) + 4,
        TableType::Albums => align4(22 + align4(len)) + 4,
        TableType::Keys => align4(8 + len),
        _ => align4(4 + len),
    }
}

/// Where a row puts a string (`encoded`) that would start at `after`: rekordbox starts a long
/// string (an even kind byte) on a 4-byte boundary, in artist, album, track and My Tag rows.
/// [`lookup_alloc`] leaves room for the padding. (locallibrary patch.)
pub(crate) fn name_at(after: usize, encoded: &[u8]) -> usize {
    if encoded[0] & 1 == 0 {
        align4(after)
    } else {
        after
    }
}

/// The allocation of a playlist tree row whose name encodes to `len` bytes. (locallibrary
/// patch.)
fn tree_alloc(len: usize) -> usize {
    align4(20 + len)
}

/// `RowTooLarge` unless a row allocation of `alloc` bytes fits on a page of `page_size` bytes,
/// which holds the page less its header and a one-row directory. (locallibrary patch: the one
/// rule for `export.pdb`, `exportExt.pdb` and the checks [`Database`] makes before an edit.)
pub(crate) fn check_alloc(alloc: usize, page_size: usize) -> Result<()> {
    let max = page_size.saturating_sub(PAGE_HEADER_SIZE + PdbEditor::dir_bytes(1));
    if alloc > max {
        return Err(PdbError::RowTooLarge { alloc, max });
    }
    Ok(())
}

/// Everything [`PdbEditor::add_track`] can refuse `t` for, checked before it writes anything:
/// values too large for their fields, and a track row (or one of the artist, album, genre, key
/// and label rows it may add) that can't fit on a page. (locallibrary patch.)
fn check_new_track(t: &NewTrack, page_size: usize) -> Result<()> {
    for (name, value, size) in [
        ("sample_rate", t.sample_rate as u64, 4),
        ("file_size", t.file_size as u64, 4),
        ("bitrate", t.bitrate as u64, 4),
        ("track_number", t.track_number as u64, 4),
        ("tempo", t.tempo as u64, 4),
        ("disc_number", t.disc_number as u64, 2),
        ("play_count", t.play_count as u64, 2),
        ("year", t.year as u64, 2),
        ("sample_depth", t.sample_depth as u64, 2),
        ("duration", t.duration as u64, 2),
        ("artwork_id", t.artwork_id as u64, 4),
        ("color_id", t.color_id as u64, 1),
        ("rating", t.rating as u64, 1),
    ] {
        check_range(name, value, size)?;
    }
    // The row fits (the fixed part's values don't change its size).
    PdbEditor::track_row(vec![0; TRACK_FIXED], &t.strings(&t.file_name()), page_size)?;
    // And the lookup rows: one that doesn't fit mustn't leave the ones before it behind.
    for (table, name) in [
        (TableType::Artists, &t.artist),
        (TableType::Albums, &t.album),
        (TableType::Genres, &t.genre),
        (TableType::Keys, &t.key),
        (TableType::Labels, &t.label),
    ] {
        if let Some(name) = name {
            check_lookup(table, name, page_size)?;
        }
    }
    Ok(())
}

/// Whether a new lookup row named `name` fits on a page. (locallibrary patch.)
fn check_lookup(table: TableType, name: &str, page_size: usize) -> Result<()> {
    check_alloc(lookup_alloc(table, encode_string(name)?.len()), page_size)
}

/// A track's 21 strings with `values` (by name, see [`TRACK_STRING_NAMES`]) in place, or `None`
/// if that changes none of them. (locallibrary patch.)
fn with_strings<'a>(
    strings: &'a [String],
    values: &[(&str, &'a str)],
) -> Result<Option<Vec<&'a str>>> {
    let mut out: Vec<&str> = strings.iter().map(String::as_str).collect();
    let mut changed = false;
    for (name, value) in values {
        let slot = TRACK_STRING_NAMES
            .iter()
            .position(|n| n == name)
            .ok_or_else(|| PdbError::UnknownField(name.to_string()))?;
        changed |= out[slot] != *value;
        out[slot] = value;
    }
    Ok(changed.then_some(out))
}

/// Edits an `export.pdb` in memory; write the result with `save`.
pub struct PdbEditor {
    buf: Vec<u8>,
    page_size: usize,
    orig_slots: HashMap<u32, u32>,
    appends: HashMap<u32, u32>,
    new_gen: HashMap<u32, u32>,
    touched_pages: HashMap<u32, HashSet<u32>>,
    structural: bool,
    final_sequence: Option<u32>,
    /// Built on first use by the `export.pdb` edits (never by the raw-row helpers, which also
    /// serve `exportExt.pdb`).
    index: Option<Index>,
}

impl PdbEditor {
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<PdbEditor> {
        Ok(PdbEditor::from_bytes(std::fs::read(path)?))
    }

    pub fn from_bytes(data: Vec<u8>) -> PdbEditor {
        // locallibrary patch: a file too short for its header doesn't panic here; the first edit
        // parses it and fails.
        let page_size = read_header(&data).map_or(0, |(page_size, _)| page_size);
        PdbEditor {
            buf: data,
            page_size,
            orig_slots: HashMap::new(),
            appends: HashMap::new(),
            new_gen: HashMap::new(),
            touched_pages: HashMap::new(),
            structural: false,
            final_sequence: None,
            index: None,
        }
    }

    pub fn to_bytes(&mut self) -> Vec<u8> {
        self.finalize();
        self.buf.clone()
    }

    pub fn save<P: AsRef<Path>>(&mut self, path: P) -> Result<()> {
        let bytes = self.to_bytes();
        std::fs::write(path, bytes)?;
        Ok(())
    }

    /// Parse the current buffer state.
    pub fn database(&self) -> Result<Database> {
        Database::from_bytes(self.buf.clone())
    }

    fn ix(&mut self) -> Result<&mut Index> {
        if self.index.is_none() {
            let db = self.database()?;
            self.index = Some(Index::build(&db, &self.buf));
        }
        Ok(self.index.as_mut().expect("just built"))
    }

    // -- byte helpers --------------------------------------------------------

    fn u16(&self, off: usize) -> u16 {
        u16le(&self.buf, off)
    }
    fn u32(&self, off: usize) -> u32 {
        u32le(&self.buf, off)
    }
    fn put_u16(&mut self, off: usize, v: u16) {
        self.buf[off..off + 2].copy_from_slice(&v.to_le_bytes());
    }
    fn put_u32(&mut self, off: usize, v: u32) {
        self.buf[off..off + 4].copy_from_slice(&v.to_le_bytes());
    }

    fn table_entry_offset(&self, table_type: u32) -> Result<usize> {
        let num_tables = self.u32(8);
        for i in 0..num_tables as usize {
            let o = 0x1c + i * 16;
            if self.u32(o) == table_type {
                return Ok(o);
            }
        }
        Err(PdbError::NoTable(table_type))
    }

    /// Patch a u32 of an existing row in place, recording its page as changed (a structural
    /// edit, unlike [`PdbEditor::set_track_field`]).
    fn patch_row_u32(&mut self, table: TableType, loc: usize, offset: usize, value: u32) -> Result<()> {
        if self.u32(loc + offset) != value {
            self.put_u32(loc + offset, value);
            self.mark_table_touched(table as u32, (loc / self.page_size) as u32)?;
        }
        Ok(())
    }

    // -- in-place edits ------------------------------------------------------

    /// Patch a fixed-width numeric track field in place (changes only that
    /// field's bytes; does not touch page bookkeeping).
    pub fn set_track_field(&mut self, track_id: u32, field: &str, value: u32) -> Result<()> {
        let (offset, size) =
            field_spec(field).ok_or_else(|| PdbError::UnknownField(field.to_string()))?;
        check_range(field, value as u64, size)?;
        let loc = *self.ix()?.tracks.get(&track_id).ok_or(PdbError::TrackNotFound(track_id))?;
        let bytes = value.to_le_bytes();
        self.buf[loc + offset..loc + offset + size].copy_from_slice(&bytes[..size]);
        Ok(())
    }

    // -- appending rows ------------------------------------------------------

    fn slot_count(&self, page_off: usize) -> u32 {
        self.buf[page_off + 0x18] as u32 + 0x100 * (self.buf[page_off + 0x19] & 1) as u32
    }

    fn dir_bytes(n: u32) -> usize {
        if n == 0 {
            0
        } else {
            (2 * n + 4 * n.div_ceil(16)) as usize
        }
    }

    fn present_count(&self, page_off: usize, n: u32) -> u32 {
        let mut count = 0;
        let groups = n.div_ceil(16);
        for group in 0..groups {
            let base = page_off + self.page_size - group as usize * 0x24;
            let in_group = std::cmp::min(16, n - group * 16);
            let mask = if in_group >= 16 {
                u16::MAX
            } else {
                (1u16 << in_group) - 1
            };
            let flags = self.u16(base - 4) & mask;
            count += flags.count_ones();
        }
        count
    }

    // `contains_key` + `insert`, not the entry API: the body also borrows
    // `self` for other reads/writes, which the entry API would conflict with.
    #[allow(clippy::map_entry)]
    fn mark_table_touched(&mut self, table_type: u32, page_index: u32) -> Result<()> {
        if !self.new_gen.contains_key(&table_type) {
            let entry = self.table_entry_offset(table_type)?;
            let (first, last) = (self.u32(entry + 8), self.u32(entry + 12));
            let mut gen = 0;
            let mut page = first;
            // locallibrary patch: bounded walk (corrupt cyclic chains must not hang).
            for _ in 0..self.buf.len() / self.page_size.max(1) + 1 {
                gen = gen.max(self.u32(page as usize * self.page_size + 0x10));
                if page == last {
                    break;
                }
                page = self.u32(page as usize * self.page_size + 0x0c);
            }
            self.new_gen.insert(table_type, gen + 1);
        }
        self.touched_pages
            .entry(table_type)
            .or_default()
            .insert(page_index);
        self.structural = true;
        Ok(())
    }

    /// Append a row to a table's last page (allocating a new page if it doesn't fit) and return
    /// the row's absolute offset in the file.
    #[allow(clippy::map_entry)]
    fn append_row(
        &mut self,
        table_type: u32,
        row: &[u8],
        alloc: usize,
        index_shift_at: Option<usize>,
    ) -> Result<usize> {
        check_alloc(alloc, self.page_size)?;
        let entry = self.table_entry_offset(table_type)?;
        let mut page_index = self.u32(entry + 12); // last_page
        let mut page_off = page_index as usize * self.page_size;

        if self.buf[page_off + 0x1b] & 0x40 != 0 {
            // Empty table: its only page is the index page. Allocate.
            page_index = self.allocate_page(table_type)?;
            page_off = page_index as usize * self.page_size;
        }

        let mut n = self.slot_count(page_off);
        let mut used = self.u16(page_off + 0x1e) as usize;
        // locallibrary patch: a full slot directory also needs a new page (checked before
        // anything is written, so a failure leaves the page as it was).
        if n >= 511 || PAGE_HEADER_SIZE + used + alloc + Self::dir_bytes(n + 1) > self.page_size {
            page_index = self.allocate_page(table_type)?;
            page_off = page_index as usize * self.page_size;
            n = 0;
            used = 0;
        }

        if !self.orig_slots.contains_key(&page_index) {
            self.orig_slots.insert(page_index, n);
            self.appends.insert(page_index, 0);
            // Reset the written-this-save masks, as a fresh save does.
            for group in 0..n.div_ceil(16) {
                let off = page_off + self.page_size - group as usize * 0x24 - 2;
                self.put_u16(off, 0);
            }
        }
        self.mark_table_touched(table_type, page_index)?;

        let slot = n;
        let group = (slot / 16) as usize;
        let bit = slot % 16;
        let group_base = page_off + self.page_size - group * 0x24;
        if bit == 0 {
            // New group: init its presence + written flag words.
            self.put_u16(group_base - 4, 0);
            self.put_u16(group_base - 2, 0);
        }

        // Row bytes (zero-padded to the allocation) into the heap.
        let row_start = page_off + PAGE_HEADER_SIZE + used;
        self.buf[row_start..row_start + row.len()].copy_from_slice(row);
        for b in &mut self.buf[row_start + row.len()..row_start + alloc] {
            *b = 0;
        }
        if let Some(shift_at) = index_shift_at {
            self.put_u16(row_start + shift_at, (0x20 * slot) as u16);
        }

        // Row directory: offset, presence bit, written bit.
        self.put_u16(group_base - 6 - 2 * bit as usize, used as u16);
        let presence = self.u16(group_base - 4) | (1 << bit);
        self.put_u16(group_base - 4, presence);
        let written = self.u16(group_base - 2) | (1 << bit);
        self.put_u16(group_base - 2, written);
        *self.appends.get_mut(&page_index).unwrap() += 1;

        // Page header bookkeeping.
        let new_n = n + 1;
        let present = self.present_count(page_off, new_n);
        self.buf[page_off + 0x18] = (new_n & 0xff) as u8;
        self.put_u16(
            page_off + 0x19,
            (0x20 * present) as u16 | if new_n > 255 { 1 } else { 0 },
        );
        used += alloc;
        self.put_u16(page_off + 0x1e, used as u16);
        self.put_u16(
            page_off + 0x1c,
            (self.page_size - PAGE_HEADER_SIZE - used - Self::dir_bytes(new_n)) as u16,
        );
        let appends = self.appends[&page_index];
        self.put_u16(page_off + 0x20, appends as u16);
        self.put_u16(page_off + 0x22, self.orig_slots[&page_index] as u16);
        Ok(row_start)
    }

    fn allocate_page(&mut self, table_type: u32) -> Result<u32> {
        let entry = self.table_entry_offset(table_type)?;
        let new_page = self.u32(entry + 4); // empty_candidate
        let next_unused = self.u32(12);
        let old_last = self.u32(entry + 12);

        // locallibrary patch: the file grows to the empty candidate, so a corrupt one would
        // allocate as much as it says. A table's last page links to its candidate (nothing here
        // relinks it), and candidates lie within the file or a few pages past its end.
        let pages = self.buf.len() / self.page_size;
        let linked = self.u32(old_last as usize * self.page_size + 0x0c) == new_page;
        if !linked || new_page as usize > pages + MAX_PAGES_PAST_END {
            return Err(PdbError::Malformed(format!(
                "table {table_type}'s next free page ({new_page}) is out of place"
            )));
        }

        let needed = (new_page as usize + 1) * self.page_size;
        if self.buf.len() < needed {
            self.buf.resize(needed, 0);
        }

        let page_off = new_page as usize * self.page_size;
        for b in &mut self.buf[page_off..page_off + self.page_size] {
            *b = 0;
        }
        self.put_u32(page_off + 0x04, new_page);
        self.put_u32(page_off + 0x08, table_type);
        self.put_u32(page_off + 0x0c, next_unused); // next_page -> new empty candidate
        self.buf[page_off + 0x1b] = 0x24;
        self.put_u16(page_off + 0x1c, (self.page_size - PAGE_HEADER_SIZE) as u16);

        self.put_u32(entry + 12, new_page); // table.last_page
        self.put_u32(entry + 4, next_unused); // table.empty_candidate
        self.put_u32(12, next_unused + 1); // header next_unused_page

        if self.buf[old_last as usize * self.page_size + 0x1b] & 0x40 != 0 {
            // First data page of a previously-empty table: point the index
            // page's "first data page" heap field at it.
            let heap = old_last as usize * self.page_size + PAGE_HEADER_SIZE;
            if self.u32(heap + 4) == 0x03FFFFFF {
                self.put_u32(heap + 4, new_page);
            }
        }

        self.orig_slots.insert(new_page, 0);
        self.appends.insert(new_page, 0);
        self.mark_table_touched(table_type, new_page)?;
        Ok(new_page)
    }

    fn finalize(&mut self) {
        let touched: Vec<(u32, Vec<u32>)> = self
            .touched_pages
            .iter()
            .map(|(t, pages)| (*t, pages.iter().copied().collect()))
            .collect();
        for (table_type, pages) in touched {
            let gen = self.new_gen[&table_type];
            for page in pages {
                self.put_u32(page as usize * self.page_size + 0x10, gen);
            }
        }
        if self.structural {
            if self.final_sequence.is_none() {
                self.final_sequence = Some(self.u32(0x14) + 1);
            }
            let seq = self.final_sequence.unwrap();
            self.put_u32(0x14, seq);
        }
    }

    // -- raw rows (locallibrary patch) ---------------------------------------

    /// Append a raw row to any table and return its offset in the file. For files laid out like
    /// `export.pdb` whose rows this editor doesn't model (`exportExt.pdb`); `index_shift_at` is
    /// where the row keeps its index shift, if it has one.
    pub fn append_raw_row(
        &mut self,
        table_type: u32,
        row: &[u8],
        alloc: usize,
        index_shift_at: Option<usize>,
    ) -> Result<usize> {
        self.index = None;
        self.append_row(table_type, row, alloc, index_shift_at)
    }

    /// Delete the raw row at `loc` (from a parse of the current bytes) from any table.
    pub fn delete_raw_row(&mut self, table_type: u32, loc: usize) -> Result<()> {
        self.index = None;
        self.delete_row_at(table_type, loc)
    }

    // -- lookup-table helpers ------------------------------------------------

    /// The id of the artist named `name`, adding a row if there is none.
    pub fn artist_id_for(&mut self, name: &str) -> Result<u32> {
        if let Some(&id) = self.ix()?.artists.by_name.get(name) {
            return Ok(id);
        }
        let new_id = self.ix()?.artists.next_id();
        let s = encode_string(name)?;
        let mut row = vec![0x60u8, 0x00, 0x00, 0x00];
        row.extend_from_slice(&new_id.to_le_bytes());
        let at = name_at(10, &s);
        row.extend_from_slice(&[0x03, at as u8]);
        row.resize(at, 0);
        row.extend_from_slice(&s);
        let alloc = lookup_alloc(TableType::Artists, s.len());
        let loc = self.append_row(TableType::Artists as u32, &row, alloc, Some(2))?;
        self.ix()?.artists.insert(new_id, loc, name);
        Ok(new_id)
    }

    /// The id of the album named `name` by `artist_id` (0 for none), adding a row if there is
    /// none. (locallibrary patch: albums of the same name by different artists are different.)
    pub fn album_id_for(&mut self, name: &str, artist_id: u32) -> Result<u32> {
        let ix = self.ix()?;
        if let Some(&id) = ix.album_by_name.get(&(name.to_string(), artist_id)) {
            return Ok(id);
        }
        let new_id = ix.albums.keys().max().copied().unwrap_or(0) + 1;
        let s = encode_string(name)?;
        let mut row = vec![0x80u8, 0x00, 0x00, 0x00]; // magic, index_shift
        row.extend_from_slice(&0u32.to_le_bytes()); // unknown
        row.extend_from_slice(&artist_id.to_le_bytes());
        row.extend_from_slice(&new_id.to_le_bytes());
        row.extend_from_slice(&0u32.to_le_bytes()); // unknown
        let at = name_at(22, &s);
        row.extend_from_slice(&[0x03, at as u8]);
        row.resize(at, 0);
        row.extend_from_slice(&s);
        let alloc = lookup_alloc(TableType::Albums, s.len());
        let loc = self.append_row(TableType::Albums as u32, &row, alloc, Some(2))?;
        let ix = self.ix()?;
        ix.albums.insert(new_id, (loc, name.to_string(), artist_id));
        ix.album_by_name.entry((name.to_string(), artist_id)).or_insert(new_id);
        Ok(new_id)
    }

    fn named(&mut self, table: TableType) -> Result<&mut Named> {
        let ix = self.ix()?;
        Ok(match table {
            TableType::Genres => &mut ix.genres,
            TableType::Labels => &mut ix.labels,
            TableType::Keys => &mut ix.keys,
            TableType::Artists => &mut ix.artists,
            other => return Err(PdbError::UnsupportedTable(other as u32)),
        })
    }

    fn named_id_for(&mut self, table: TableType, name: &str) -> Result<u32> {
        if let Some(&id) = self.named(table)?.by_name.get(name) {
            return Ok(id);
        }
        let new_id = self.named(table)?.next_id();
        let s = encode_string(name)?;
        let mut row = new_id.to_le_bytes().to_vec();
        if table == TableType::Keys {
            row.extend_from_slice(&new_id.to_le_bytes());
        }
        row.extend_from_slice(&s);
        let alloc = lookup_alloc(table, s.len());
        let loc = self.append_row(table as u32, &row, alloc, None)?;
        self.named(table)?.insert(new_id, loc, name);
        Ok(new_id)
    }

    /// The id of the genre named `name`, adding a row if there is none.
    pub fn genre_id_for(&mut self, name: &str) -> Result<u32> {
        self.named_id_for(TableType::Genres, name)
    }

    /// The id of the label named `name`, adding a row if there is none.
    pub fn label_id_for(&mut self, name: &str) -> Result<u32> {
        self.named_id_for(TableType::Labels, name)
    }

    /// The id of the key named `name`, adding a row if there is none.
    pub fn key_id_for(&mut self, name: &str) -> Result<u32> {
        self.named_id_for(TableType::Keys, name)
    }

    // -- tracks --------------------------------------------------------------

    /// A track row from its fixed part and its 21 strings: the row and its allocation.
    fn track_row(
        mut fixed: Vec<u8>,
        strings: &[&str],
        page_size: usize,
    ) -> Result<(Vec<u8>, usize)> {
        debug_assert_eq!(fixed.len(), TRACK_FIXED);
        let encoded: Vec<Vec<u8>> = strings
            .iter()
            .enumerate()
            .map(|(i, s)| if i == 0 { encode_isrc(s) } else { encode_string(s) })
            .collect::<Result<_>>()?;
        let alloc = TRACK_STRINGS + encoded.iter().map(|e| align4(e.len())).sum::<usize>() + 4;
        check_alloc(alloc, page_size)?;
        // 21 u16 string offsets (row-relative), then strings packed in order.
        let mut blob: Vec<u8> = Vec::new();
        let mut offsets = [0u16; 21];
        let mut pos = TRACK_STRINGS;
        for (i, enc) in encoded.iter().enumerate() {
            let at = name_at(pos, enc);
            blob.resize(blob.len() + at - pos, 0);
            pos = at;
            offsets[i] = pos as u16;
            blob.extend_from_slice(enc);
            pos += enc.len();
        }
        for off in offsets {
            fixed.extend_from_slice(&off.to_le_bytes());
        }
        fixed.extend_from_slice(&blob);
        Ok((fixed, alloc))
    }

    /// Add a track; returns its id. Creates or reuses artist/album/genre/key/
    /// label rows by name.
    pub fn add_track(&mut self, t: &NewTrack) -> Result<u32> {
        let filename = t.file_name();
        // Validate everything that can fail BEFORE creating lookup rows.
        check_new_track(t, self.page_size)?;
        let strings = t.strings(&filename);

        let artist_id = match &t.artist {
            Some(a) => self.artist_id_for(a)?,
            None => 0,
        };
        let album_id = match &t.album {
            Some(a) => self.album_id_for(a, artist_id)?,
            None => 0,
        };
        let genre_id = match &t.genre {
            Some(g) => self.genre_id_for(g)?,
            None => 0,
        };
        let key_id = match &t.key {
            Some(k) => self.key_id_for(k)?,
            None => 0,
        };
        let label_id = match &t.label {
            Some(l) => self.label_id_for(l)?,
            None => 0,
        };

        let ix = self.ix()?;
        let track_id = ix.tracks.keys().max().copied().unwrap_or(0) + 1;
        // Unique per-track value at 0x14 (unknown purpose; kept unique).
        let unique = ix.track_unique_max + 1;
        let file_type = file_type_code(&t.file_path);

        // Fixed 0x5e-byte part, field by field in file order.
        let mut fixed: Vec<u8> = Vec::with_capacity(TRACK_FIXED);
        let pu16 = |v: &mut Vec<u8>, x: u16| v.extend_from_slice(&x.to_le_bytes());
        let pu32 = |v: &mut Vec<u8>, x: u32| v.extend_from_slice(&x.to_le_bytes());
        pu16(&mut fixed, 0x0024); // magic
        pu16(&mut fixed, 0); // index_shift (patched on append)
        pu32(&mut fixed, 0x000C_0700); // bitmask
        pu32(&mut fixed, t.sample_rate);
        pu32(&mut fixed, 0); // composer_id
        pu32(&mut fixed, t.file_size);
        pu32(&mut fixed, unique);
        pu16(&mut fixed, 0xAE49);
        pu16(&mut fixed, 0x03DD);
        pu32(&mut fixed, t.artwork_id);
        pu32(&mut fixed, key_id);
        pu32(&mut fixed, 0); // original_artist_id
        pu32(&mut fixed, label_id);
        pu32(&mut fixed, 0); // remixer_id
        pu32(&mut fixed, t.bitrate);
        pu32(&mut fixed, t.track_number);
        pu32(&mut fixed, t.tempo);
        pu32(&mut fixed, genre_id);
        pu32(&mut fixed, album_id);
        pu32(&mut fixed, artist_id);
        pu32(&mut fixed, track_id);
        pu16(&mut fixed, t.disc_number);
        pu16(&mut fixed, t.play_count);
        pu16(&mut fixed, t.year);
        pu16(&mut fixed, t.sample_depth);
        pu16(&mut fixed, t.duration);
        pu16(&mut fixed, 0x0029);
        fixed.push(t.color_id);
        fixed.push(t.rating);
        pu16(&mut fixed, file_type);
        pu16(&mut fixed, 0x0003);
        debug_assert_eq!(fixed.len(), TRACK_FIXED);

        let (row, alloc) = Self::track_row(fixed, &strings, self.page_size)?;
        let loc = self.append_row(TableType::Tracks as u32, &row, alloc, Some(2))?;
        let ix = self.ix()?;
        ix.tracks.insert(track_id, loc);
        ix.track_unique_max = unique;
        Ok(track_id)
    }

    /// Set string fields of a track (`title`, `comment`, `mix_name`, `isrc`, `release_date`,
    /// `date_added`, `message`... see [`TRACK_STRING_NAMES`]). The row is rewritten with the
    /// same id and fixed fields, the way rekordbox replaces a row whose strings grew.
    /// (locallibrary patch.)
    pub fn set_track_strings(&mut self, track_id: u32, values: &[(&str, &str)]) -> Result<()> {
        let loc = *self.ix()?.tracks.get(&track_id).ok_or(PdbError::TrackNotFound(track_id))?;
        let fixed = self.buf[loc..loc + TRACK_FIXED].to_vec();
        let strings: Vec<String> = (0..21)
            .map(|i| {
                let at = loc + self.u16(loc + TRACK_FIXED + 2 * i) as usize;
                if i == 0 { decode_isrc(&self.buf, at) } else { decode_string(&self.buf, at) }
            })
            .collect::<Result<_>>()?;
        let Some(strings) = with_strings(&strings, values)? else {
            return Ok(());
        };
        let (row, alloc) = Self::track_row(fixed, &strings, self.page_size)?;
        self.delete_row_at(TableType::Tracks as u32, loc)?;
        let new_loc = self.append_row(TableType::Tracks as u32, &row, alloc, Some(2))?;
        self.ix()?.tracks.insert(track_id, new_loc);
        Ok(())
    }

    // -- artwork (locallibrary patch) ----------------------------------------

    /// Add an Artwork row pointing at `path` (e.g. `/PIONEER/Artwork/00001/a7.jpg`) and return
    /// its id. Players derive the larger variant by inserting `_m` before the extension.
    pub fn add_artwork(&mut self, path: &str) -> Result<u32> {
        let new_id = self.ix()?.artwork.keys().max().copied().unwrap_or(0) + 1;
        let s = encode_string(path)?;
        let mut row = new_id.to_le_bytes().to_vec();
        row.extend_from_slice(&s);
        let alloc = align4(4 + s.len());
        let loc = self.append_row(TableType::Artwork as u32, &row, alloc, None)?;
        self.ix()?.artwork.insert(new_id, loc);
        Ok(new_id)
    }

    // -- deleting rows (locallibrary patch) -----------------------------------

    /// Delete the row starting at absolute offset `loc` (from [`Database::row_locations`]) the
    /// way rekordbox does: clear its presence flag and update the page's present-row count. The
    /// row's heap bytes are left in place (rekordbox never compacts a page either).
    fn delete_row_at(&mut self, table_type: u32, loc: usize) -> Result<()> {
        let page_index = (loc / self.page_size) as u32;
        let page_off = page_index as usize * self.page_size;
        let n = self.slot_count(page_off);
        let heap_off = loc - page_off - PAGE_HEADER_SIZE;
        for slot in 0..n {
            let group_base = page_off + self.page_size - (slot / 16) as usize * 0x24;
            let bit = slot % 16;
            let presence = self.u16(group_base - 4);
            if presence & (1 << bit) == 0 || self.u16(group_base - 6 - 2 * bit as usize) as usize != heap_off {
                continue;
            }
            self.put_u16(group_base - 4, presence & !(1 << bit));
            let present = self.present_count(page_off, n);
            self.put_u16(
                page_off + 0x19,
                (0x20 * present) as u16 | if n > 255 { 1 } else { 0 },
            );
            return self.mark_table_touched(table_type, page_index);
        }
        Err(PdbError::RowNotFound(loc))
    }

    /// Renumber a playlist's (or history's) entries 1..n in their current order.
    fn renumber(&mut self, table: TableType, list: u32) -> Result<()> {
        let ix = self.ix()?;
        let entries = match table {
            TableType::HistoryEntries => ix.history_entries.get_mut(&list),
            _ => ix.entries.get_mut(&list),
        };
        let Some(entries) = entries else { return Ok(()) };
        entries.sort_unstable();
        let patches: Vec<(usize, u32)> = entries
            .iter_mut()
            .enumerate()
            .filter(|(i, e)| e.index != *i as u32 + 1)
            .map(|(i, e)| {
                e.index = i as u32 + 1;
                (e.loc, e.index)
            })
            .collect();
        let at = if table == TableType::HistoryEntries { 8 } else { 0 };
        for (loc, index) in patches {
            self.patch_row_u32(table, loc, at, index)?;
        }
        Ok(())
    }

    /// Remove tracks, together with their playlist and history entries (the remaining entries of
    /// each affected list are renumbered 1..n).
    pub fn remove_tracks(&mut self, ids: &[u32]) -> Result<()> {
        let ids: HashSet<u32> = ids.iter().copied().collect();
        let ix = self.ix()?;
        for id in &ids {
            if !ix.tracks.contains_key(id) {
                return Err(PdbError::TrackNotFound(*id));
            }
        }
        let mut doomed: Vec<(TableType, usize)> = Vec::new();
        for id in &ids {
            doomed.push((TableType::Tracks, ix.tracks.remove(id).expect("checked")));
        }
        let mut affected = Vec::new();
        for (table, lists) in [
            (TableType::PlaylistEntries, &mut ix.entries),
            (TableType::HistoryEntries, &mut ix.history_entries),
        ] {
            for (list, entries) in lists.iter_mut() {
                let before = entries.len();
                entries.retain(|e| {
                    let gone = ids.contains(&e.track);
                    if gone {
                        doomed.push((table, e.loc));
                    }
                    !gone
                });
                if entries.len() != before {
                    affected.push((table, *list));
                }
            }
        }
        for (table, loc) in doomed {
            self.delete_row_at(table as u32, loc)?;
        }
        for (table, list) in affected {
            self.renumber(table, list)?;
        }
        Ok(())
    }

    /// Remove rows by id from an id-keyed table: artists, albums, genres, labels, keys, artwork,
    /// playlist tree nodes and history playlists (whose entries are left to the caller; see
    /// [`PdbEditor::remove_playlists`]). References to them are not checked; returns how many
    /// rows were removed.
    pub fn remove_rows(&mut self, table: TableType, ids: &[u32]) -> Result<usize> {
        let mut doomed = Vec::new();
        {
            let ix = self.ix()?;
            for id in ids {
                let loc = match table {
                    TableType::Artists => ix.artists.remove(*id),
                    TableType::Genres => ix.genres.remove(*id),
                    TableType::Labels => ix.labels.remove(*id),
                    TableType::Keys => ix.keys.remove(*id),
                    TableType::Albums => ix.albums.remove(id).map(|(loc, name, artist)| {
                        if ix.album_by_name.get(&(name.clone(), artist)) == Some(id) {
                            ix.album_by_name.remove(&(name, artist));
                        }
                        loc
                    }),
                    TableType::Artwork => ix.artwork.remove(id),
                    TableType::PlaylistTree => ix.tree.remove(id).map(|n| n.loc),
                    TableType::HistoryPlaylists => ix.history.remove(id),
                    other => return Err(PdbError::UnsupportedTable(other as u32)),
                };
                doomed.extend(loc);
            }
        }
        for &loc in &doomed {
            self.delete_row_at(table as u32, loc)?;
        }
        Ok(doomed.len())
    }

    // -- history playlists (locallibrary patch) ------------------------------

    /// Append a history playlist (a row of `id` + name, like genres) and return its id.
    pub fn create_history_playlist(&mut self, name: &str) -> Result<u32> {
        let new_id = self.ix()?.history.keys().max().copied().unwrap_or(0) + 1;
        let s = encode_string(name)?;
        let mut row = new_id.to_le_bytes().to_vec();
        row.extend_from_slice(&s);
        let alloc = align4(4 + s.len());
        let loc = self.append_row(TableType::HistoryPlaylists as u32, &row, alloc, None)?;
        self.ix()?.history.insert(new_id, loc);
        Ok(new_id)
    }

    /// Append a track to a history playlist (entries are numbered from 1).
    pub fn add_to_history(&mut self, history_id: u32, track_id: u32) -> Result<()> {
        let ix = self.ix()?;
        if !ix.history.contains_key(&history_id) {
            return Err(PdbError::PlaylistNotFound(history_id));
        }
        let entry_index = ix
            .history_entries
            .get(&history_id)
            .and_then(|e| e.iter().map(|e| e.index).max())
            .unwrap_or(0)
            + 1;
        let mut row = Vec::new();
        row.extend_from_slice(&track_id.to_le_bytes());
        row.extend_from_slice(&history_id.to_le_bytes());
        row.extend_from_slice(&entry_index.to_le_bytes());
        let loc = self.append_row(TableType::HistoryEntries as u32, &row, 12, None)?;
        self.ix()?.history_entries.entry(history_id).or_default().push(EntryRow {
            index: entry_index,
            track: track_id,
            loc,
        });
        Ok(())
    }

    /// Remove history playlists with their entries.
    pub fn remove_history_playlists(&mut self, ids: &[u32]) -> Result<()> {
        let ix = self.ix()?;
        for id in ids {
            if !ix.history.contains_key(id) {
                return Err(PdbError::PlaylistNotFound(*id));
            }
        }
        let mut doomed = Vec::new();
        for id in ids {
            if let Some(loc) = ix.history.remove(id) {
                doomed.push((TableType::HistoryPlaylists, loc));
            }
            for e in ix.history_entries.remove(id).unwrap_or_default() {
                doomed.push((TableType::HistoryEntries, e.loc));
            }
        }
        for (table, loc) in doomed {
            self.delete_row_at(table as u32, loc)?;
        }
        Ok(())
    }

    // -- playlists -----------------------------------------------------------

    fn check_parent(&mut self, parent_id: u32) -> Result<()> {
        if parent_id == 0 {
            return Ok(());
        }
        match self.ix()?.tree.get(&parent_id) {
            Some(n) if n.is_folder => Ok(()),
            _ => Err(PdbError::PlaylistNotFound(parent_id)),
        }
    }

    /// Create a playlist (or folder) as the last child of `parent_id` (0 for the top level).
    pub fn create_playlist(&mut self, name: &str, parent_id: u32, is_folder: bool) -> Result<u32> {
        self.check_parent(parent_id)?;
        let ix = self.ix()?;
        let node_id = ix.tree.keys().max().copied().unwrap_or(0) + 1;
        let sort_order = ix
            .tree
            .values()
            .filter(|n| n.parent == parent_id)
            .map(|n| n.sort_order as i64)
            .max()
            .unwrap_or(-1)
            + 1;
        let s = encode_string(name)?;
        let mut row = Vec::new();
        row.extend_from_slice(&parent_id.to_le_bytes());
        row.extend_from_slice(&0u32.to_le_bytes());
        row.extend_from_slice(&(sort_order as u32).to_le_bytes());
        row.extend_from_slice(&node_id.to_le_bytes());
        row.extend_from_slice(&(is_folder as u32).to_le_bytes());
        row.extend_from_slice(&s);
        let alloc = tree_alloc(s.len());
        let loc = self.append_row(TableType::PlaylistTree as u32, &row, alloc, None)?;
        self.ix()?.tree.insert(
            node_id,
            Node { loc, parent: parent_id, sort_order: sort_order as u32, is_folder },
        );
        Ok(node_id)
    }

    /// Rename a playlist or folder (its row is rewritten with the same id and position).
    /// (locallibrary patch.)
    pub fn rename_playlist(&mut self, id: u32, name: &str) -> Result<()> {
        let node = *self.ix()?.tree.get(&id).ok_or(PdbError::PlaylistNotFound(id))?;
        let s = encode_string(name)?;
        let mut row = self.buf[node.loc..node.loc + 20].to_vec();
        row.extend_from_slice(&s);
        let alloc = tree_alloc(s.len());
        check_alloc(alloc, self.page_size)?;
        self.delete_row_at(TableType::PlaylistTree as u32, node.loc)?;
        let loc = self.append_row(TableType::PlaylistTree as u32, &row, alloc, None)?;
        self.ix()?.tree.insert(id, Node { loc, ..node });
        Ok(())
    }

    /// Move a playlist or folder under `parent_id` (0 for the top level) with `sort_order`.
    /// Siblings keep their sort orders; see [`PdbEditor::set_sort_order`] to renumber them.
    /// (locallibrary patch.)
    pub fn move_playlist(&mut self, id: u32, parent_id: u32, sort_order: u32) -> Result<()> {
        self.check_parent(parent_id)?;
        let ix = self.ix()?;
        let node = *ix.tree.get(&id).ok_or(PdbError::PlaylistNotFound(id))?;
        // A folder can't go inside itself.
        let mut cur = parent_id;
        for _ in 0..=ix.tree.len() {
            if cur == 0 {
                break;
            }
            if cur == id {
                return Err(PdbError::PlaylistNotFound(parent_id));
            }
            cur = ix.tree.get(&cur).map_or(0, |n| n.parent);
        }
        self.patch_row_u32(TableType::PlaylistTree, node.loc, 0, parent_id)?;
        self.patch_row_u32(TableType::PlaylistTree, node.loc, 8, sort_order)?;
        self.ix()?.tree.insert(id, Node { parent: parent_id, sort_order, ..node });
        Ok(())
    }

    /// Change a playlist's or folder's sort order among its siblings. (locallibrary patch.)
    pub fn set_sort_order(&mut self, id: u32, sort_order: u32) -> Result<()> {
        let parent = self.ix()?.tree.get(&id).ok_or(PdbError::PlaylistNotFound(id))?.parent;
        self.move_playlist(id, parent, sort_order)
    }

    /// Remove playlists and folders with their entries. Children of a removed folder are not
    /// removed; pass them too. (locallibrary patch.)
    pub fn remove_playlists(&mut self, ids: &[u32]) -> Result<()> {
        let ix = self.ix()?;
        for id in ids {
            if !ix.tree.contains_key(id) {
                return Err(PdbError::PlaylistNotFound(*id));
            }
        }
        let mut doomed = Vec::new();
        for id in ids {
            if let Some(n) = ix.tree.remove(id) {
                doomed.push((TableType::PlaylistTree, n.loc));
            }
            for e in ix.entries.remove(id).unwrap_or_default() {
                doomed.push((TableType::PlaylistEntries, e.loc));
            }
        }
        for (table, loc) in doomed {
            self.delete_row_at(table as u32, loc)?;
        }
        Ok(())
    }

    /// Append a track to a playlist (at the end, or with an explicit entry index, which doesn't
    /// move the other entries).
    pub fn add_to_playlist(
        &mut self,
        playlist_id: u32,
        track_id: u32,
        entry_index: Option<u32>,
    ) -> Result<()> {
        let ix = self.ix()?;
        match ix.tree.get(&playlist_id) {
            Some(n) if !n.is_folder => {}
            _ => return Err(PdbError::PlaylistNotFound(playlist_id)),
        }
        if !ix.tracks.contains_key(&track_id) {
            return Err(PdbError::TrackNotFound(track_id));
        }
        let entry_index = entry_index.unwrap_or_else(|| {
            ix.entries
                .get(&playlist_id)
                .and_then(|e| e.iter().map(|e| e.index).max())
                .unwrap_or(0)
                + 1
        });
        let mut row = Vec::new();
        row.extend_from_slice(&entry_index.to_le_bytes());
        row.extend_from_slice(&track_id.to_le_bytes());
        row.extend_from_slice(&playlist_id.to_le_bytes());
        let loc = self.append_row(TableType::PlaylistEntries as u32, &row, 12, None)?;
        let entries = self.ix()?.entries.entry(playlist_id).or_default();
        entries.push(EntryRow { index: entry_index, track: track_id, loc });
        entries.sort_unstable();
        Ok(())
    }

    /// Replace a playlist's contents with `tracks`, in order (repeats allowed). Existing entry
    /// rows are reused in place; extra ones are removed and missing ones appended, numbered
    /// 1..n. (locallibrary patch.)
    pub fn set_playlist_entries(&mut self, playlist_id: u32, tracks: &[u32]) -> Result<()> {
        let ix = self.ix()?;
        match ix.tree.get(&playlist_id) {
            Some(n) if !n.is_folder => {}
            _ => return Err(PdbError::PlaylistNotFound(playlist_id)),
        }
        if let Some(t) = tracks.iter().find(|t| !ix.tracks.contains_key(t)) {
            return Err(PdbError::TrackNotFound(*t));
        }
        let mut old = ix.entries.remove(&playlist_id).unwrap_or_default();
        old.sort_unstable();
        let mut new = Vec::with_capacity(tracks.len());
        for (i, (&track, slot)) in tracks.iter().zip(old.iter()).enumerate() {
            let index = i as u32 + 1;
            self.patch_row_u32(TableType::PlaylistEntries, slot.loc, 0, index)?;
            self.patch_row_u32(TableType::PlaylistEntries, slot.loc, 4, track)?;
            new.push(EntryRow { index, track, loc: slot.loc });
        }
        for slot in old.iter().skip(tracks.len()) {
            self.delete_row_at(TableType::PlaylistEntries as u32, slot.loc)?;
        }
        for (i, &track) in tracks.iter().enumerate().skip(old.len()) {
            let index = i as u32 + 1;
            let mut row = Vec::new();
            row.extend_from_slice(&index.to_le_bytes());
            row.extend_from_slice(&track.to_le_bytes());
            row.extend_from_slice(&playlist_id.to_le_bytes());
            let loc = self.append_row(TableType::PlaylistEntries as u32, &row, 12, None)?;
            new.push(EntryRow { index, track, loc });
        }
        if !new.is_empty() {
            self.ix()?.entries.insert(playlist_id, new);
        }
        Ok(())
    }

    /// Point every playlist and history entry for track `from` at track `to` instead (for
    /// replacing a duplicate with the copy to keep). Returns how many entries changed.
    /// (locallibrary patch.)
    pub fn repoint_track(&mut self, from: u32, to: u32) -> Result<usize> {
        let ix = self.ix()?;
        for id in [from, to] {
            if !ix.tracks.contains_key(&id) {
                return Err(PdbError::TrackNotFound(id));
            }
        }
        let mut patches = Vec::new();
        for (table, lists, at) in [
            (TableType::PlaylistEntries, &mut ix.entries, 4),
            (TableType::HistoryEntries, &mut ix.history_entries, 0),
        ] {
            for e in lists.values_mut().flatten().filter(|e| e.track == from) {
                e.track = to;
                patches.push((table, e.loc, at));
            }
        }
        for &(table, loc, at) in &patches {
            self.patch_row_u32(table, loc, at, to)?;
        }
        Ok(patches.len())
    }
}

// -- checking an edit before making it (locallibrary patch) ------------------

/// Each check fails as the [`PdbEditor`] edit it names would (`RowTooLarge`, `StringTooLong`)
/// when that edit's row can't fit on one of this file's pages, so a caller can refuse an edit
/// before making any of it.
impl Database {
    /// A playlist or folder named `name` ([`PdbEditor::create_playlist`],
    /// [`PdbEditor::rename_playlist`]).
    pub fn check_playlist_name(&self, name: &str) -> Result<()> {
        check_alloc(tree_alloc(encode_string(name)?.len()), self.page_size as usize)
    }

    /// A new artist, album, genre, label or key row named `name`, which
    /// [`PdbEditor::artist_id_for`] and the like add for a name they don't find.
    pub fn check_lookup_name(&self, table: TableType, name: &str) -> Result<()> {
        check_lookup(table, name, self.page_size as usize)
    }

    /// A new track ([`PdbEditor::add_track`]), with the lookup rows it may add.
    pub fn check_new_track(&self, track: &NewTrack) -> Result<()> {
        check_new_track(track, self.page_size as usize)
    }

    /// A history playlist named `name` ([`PdbEditor::create_history_playlist`]).
    pub fn check_history_name(&self, name: &str) -> Result<()> {
        check_alloc(align4(4 + encode_string(name)?.len()), self.page_size as usize)
    }

    /// `track` with some of its strings changed ([`PdbEditor::set_track_strings`]).
    pub fn check_track_strings(&self, track: &Track, values: &[(&str, &str)]) -> Result<()> {
        match with_strings(&track.strings, values)? {
            Some(strings) => {
                PdbEditor::track_row(vec![0; TRACK_FIXED], &strings, self.page_size as usize).map(drop)
            }
            None => Ok(()),
        }
    }
}
