//! Reader and editor for `exportExt.pdb`, which holds rekordbox's My Tags: categories, the tags
//! in them, and which tracks carry each tag.
//!
//! The file has the same page format as `export.pdb`, with its own table types
//! ([`ExtTableType`]). Row layouts, from Deep Symmetry's crate-digger analysis
//! (`rekordbox_pdb.ksy` with `is_ext`) and rekordcrate:
//!
//! * tag row: `subtype` u16 (0x0680, or 0x0684 when the string offsets are two bytes wide),
//!   `index_shift` u16, 8 bytes of unknown purpose, `category` u32 (0 for a category),
//!   `position` u32 (zero-based, within the category, or among categories for a category), `id`
//!   u32, `raw_is_category` u32 (non-zero for a category), then an offset array at 0x1c: the byte
//!   0x03 and two one-byte offsets (the name, then a string that is always empty), or with
//!   subtype 0x0684 the u16 0x0003 and two u16 offsets.
//! * tag track row: u32 0, `track_id` u32 (an `export.pdb` track id), `tag_id` u32, u32 3.
//!
//! locallibrary patch: the reader used to parse only tag rows, read `is_category` from a single
//! byte and only the one-byte name offset, and took table type 8 for tag tracks.
//! [`ExtEditor`] is new.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use crate::edit::{check_alloc, name_at, PdbEditor};
use crate::pdb::{table_row_offsets, Table};
use crate::{
    align4, decode_string, encode_string, read_header, read_u16, read_u32, read_u8, u32le, PdbError,
    Result,
};

/// Table types in `exportExt.pdb` (a different set from `export.pdb`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u32)]
pub enum ExtTableType {
    /// My Tag categories and tags.
    Tags = 3,
    /// Which tags each track carries.
    TagTracks = 4,
}

/// Subtype of a tag row whose string offsets are single bytes.
const SUBTYPE_NEAR: u16 = 0x0680;
/// Subtype of a tag row whose string offsets are two bytes wide.
const SUBTYPE_FAR: u16 = 0x0684;
/// `raw_is_category` as rekordbox writes it for a category (bytes `00 00 00 01`).
const CATEGORY_FLAG: u32 = 0x0100_0000;
/// Where a tag row's offset array starts.
const TAG_OFFSETS: usize = 0x1c;
/// Size of a tag track row.
const TAG_TRACK_ROW: usize = 16;

/// A My Tag category or tag.
///
/// rekordbox's four categories have ids 1..4 and `is_category` set; tags carry their category's
/// id in `category_id` and a large 32-bit id.
#[derive(Clone, Debug)]
pub struct Tag {
    pub id: u32,
    pub name: String,
    /// The category a tag belongs to; 0 for a category.
    pub category_id: u32,
    /// Zero-based display position: within its category for a tag, among the categories for a
    /// category.
    pub position: u32,
    pub is_category: bool,
    pub index_shift: u16,
}

impl Tag {
    fn parse(buf: &[u8], row: usize) -> Result<Tag> {
        let subtype = read_u16(buf, row)?;
        let name_off = if subtype & 0x04 != 0 {
            read_u16(buf, row + 0x1e)? as usize
        } else {
            read_u8(buf, row + 0x1d)? as usize
        };
        Ok(Tag {
            index_shift: read_u16(buf, row + 2)?,
            category_id: read_u32(buf, row + 0x0c)?,
            position: read_u32(buf, row + 0x10)?,
            id: read_u32(buf, row + 0x14)?,
            is_category: read_u32(buf, row + 0x18)? != 0,
            name: decode_string(buf, row + name_off)?,
        })
    }
}

/// A tag assigned to a track.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TagTrack {
    /// The track's id in `export.pdb`.
    pub track_id: u32,
    pub tag_id: u32,
}

impl TagTrack {
    fn parse(buf: &[u8], row: usize) -> Result<TagTrack> {
        Ok(TagTrack {
            track_id: read_u32(buf, row + 4)?,
            tag_id: read_u32(buf, row + 8)?,
        })
    }
}

/// A parsed `exportExt.pdb` file.
pub struct ExtDatabase {
    pub page_size: u32,
    pub tables: Vec<Table>,
    /// Categories and tags, in file order.
    pub tags: Vec<Tag>,
    /// Tag assignments, in file order.
    pub tag_tracks: Vec<TagTrack>,
    locations: HashMap<u32, Vec<usize>>,
}

impl ExtDatabase {
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<ExtDatabase> {
        ExtDatabase::from_bytes(std::fs::read(path)?)
    }

    pub fn from_bytes(data: Vec<u8>) -> Result<ExtDatabase> {
        let (page_size, num_tables) = read_header(&data)?;
        let mut tables = Vec::with_capacity(num_tables);
        for i in 0..num_tables {
            let o = 0x1c + i * 16;
            tables.push(Table {
                table_type: u32le(&data, o),
                empty_candidate: u32le(&data, o + 4),
                first_page: u32le(&data, o + 8),
                last_page: u32le(&data, o + 12),
            });
        }

        let mut locations = HashMap::new();
        for ty in [ExtTableType::Tags, ExtTableType::TagTracks] {
            if let Some(t) = tables.iter().find(|t| t.table_type == ty as u32) {
                locations.insert(ty as u32, table_row_offsets(&data, page_size, t)?);
            }
        }
        let get = |ty: ExtTableType| locations.get(&(ty as u32)).map(Vec::as_slice).unwrap_or(&[]);
        let tags = get(ExtTableType::Tags)
            .iter()
            .map(|&off| Tag::parse(&data, off))
            .collect::<Result<_>>()?;
        let tag_tracks = get(ExtTableType::TagTracks)
            .iter()
            .map(|&off| TagTrack::parse(&data, off))
            .collect::<Result<_>>()?;

        Ok(ExtDatabase {
            page_size: page_size as u32,
            tables,
            tags,
            tag_tracks,
            locations,
        })
    }

    /// Whether the file's table directory has a table of this type.
    pub fn has_table(&self, table_type: ExtTableType) -> bool {
        self.tables.iter().any(|t| t.table_type == table_type as u32)
    }

    /// Absolute file offset of each row of a table, parallel to `tags` / `tag_tracks`.
    pub fn row_locations(&self, table_type: ExtTableType) -> &[usize] {
        self.locations
            .get(&(table_type as u32))
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    /// The categories, in display order.
    pub fn categories(&self) -> Vec<&Tag> {
        let mut v: Vec<&Tag> = self.tags.iter().filter(|t| t.is_category).collect();
        v.sort_by_key(|t| t.position);
        v
    }

    /// The tags of category `category_id`, in display order.
    pub fn tags_in(&self, category_id: u32) -> Vec<&Tag> {
        let mut v: Vec<&Tag> = self
            .tags
            .iter()
            .filter(|t| !t.is_category && t.category_id == category_id)
            .collect();
        v.sort_by_key(|t| t.position);
        v
    }
}

// ---------------------------------------------------------------------------
// Editing (locallibrary patch)
// ---------------------------------------------------------------------------

/// A category or tag as [`ExtEditor`] keeps it.
#[derive(Clone, Debug)]
struct Entry {
    id: u32,
    name: String,
    /// Row bytes 0x04..0x0c (unknown purpose), kept as they were.
    unknown: [u8; 8],
    /// `raw_is_category` as stored (0 for a tag).
    raw_category: u32,
}

#[derive(Clone, Debug)]
struct Group {
    category: Entry,
    tags: Vec<Entry>,
}

/// A tag row: its bytes and its allocation.
fn tag_row(e: &Entry, category: u32, position: u32) -> Result<(Vec<u8>, usize)> {
    let name = encode_string(&e.name)?;
    let near_start = name_at(TAG_OFFSETS + 3, &name);
    let near = near_start + name.len() <= 0xff;
    let start = if near { near_start } else { name_at(TAG_OFFSETS + 6, &name) };
    let empty = start + name.len();
    if empty > 0xffff {
        return Err(PdbError::StringTooLong);
    }
    let subtype = if near { SUBTYPE_NEAR } else { SUBTYPE_FAR };
    let mut row = Vec::with_capacity(empty + 1);
    row.extend_from_slice(&subtype.to_le_bytes());
    row.extend_from_slice(&[0, 0]); // index shift, set on append
    row.extend_from_slice(&e.unknown);
    row.extend_from_slice(&category.to_le_bytes());
    row.extend_from_slice(&position.to_le_bytes());
    row.extend_from_slice(&e.id.to_le_bytes());
    row.extend_from_slice(&e.raw_category.to_le_bytes());
    if near {
        row.extend_from_slice(&[0x03, start as u8, empty as u8]);
    } else {
        row.extend_from_slice(&0x0003u16.to_le_bytes());
        row.extend_from_slice(&(start as u16).to_le_bytes());
        row.extend_from_slice(&(empty as u16).to_le_bytes());
    }
    row.resize(start, 0);
    row.extend_from_slice(&name);
    row.push(0x03); // the empty string
    // rekordbox leaves 8 spare bytes after every tag row.
    let alloc = align4(row.len()) + 8;
    Ok((row, alloc))
}

/// Edits an `exportExt.pdb` in memory: My Tag categories, tags and the tracks that carry them.
///
/// Track links are appended and deleted one row at a time. Categories and tags are kept in
/// memory and, when any of them changed, written out again in full by [`ExtEditor::to_bytes`],
/// the way rekordbox saves them: every old row deleted, then each category followed by its tags,
/// with positions renumbered from 0. Tag rows whose category doesn't exist are left alone.
pub struct ExtEditor {
    editor: PdbEditor,
    page_size: usize,
    /// Categories with their tags, in display order.
    groups: Vec<Group>,
    /// Rows holding `groups`; deleted when they are written again.
    group_rows: Vec<usize>,
    /// Ids of tag rows outside `groups` (tags of a missing category).
    other_ids: HashSet<u32>,
    /// (tag id, track id) -> rows linking them.
    links: HashMap<(u32, u32), Vec<usize>>,
    dirty: bool,
}

impl ExtEditor {
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<ExtEditor> {
        ExtEditor::from_bytes(std::fs::read(path)?)
    }

    pub fn from_bytes(data: Vec<u8>) -> Result<ExtEditor> {
        let db = ExtDatabase::from_bytes(data.clone())?;
        let locs = db.row_locations(ExtTableType::Tags);
        let entry = |t: &Tag, loc: usize| Entry {
            id: t.id,
            name: t.name.clone(),
            unknown: data[loc + 4..loc + 12].try_into().expect("8 bytes"),
            raw_category: u32le(&data, loc + 0x18),
        };
        let mut groups = Vec::new();
        let mut group_rows = Vec::new();
        let mut cats: Vec<(&Tag, usize)> = db
            .tags
            .iter()
            .zip(locs.iter().copied())
            .filter(|(t, _)| t.is_category)
            .collect();
        cats.sort_by_key(|(t, _)| t.position);
        for (cat, loc) in cats {
            if groups.iter().any(|g: &Group| g.category.id == cat.id) {
                continue; // a second row with the same id stays as it is
            }
            let mut tags: Vec<(&Tag, usize)> = db
                .tags
                .iter()
                .zip(locs.iter().copied())
                .filter(|(t, _)| !t.is_category && t.category_id == cat.id)
                .collect();
            tags.sort_by_key(|(t, _)| t.position);
            group_rows.push(loc);
            group_rows.extend(tags.iter().map(|(_, l)| *l));
            groups.push(Group {
                category: entry(cat, loc),
                tags: tags.iter().map(|(t, l)| entry(t, *l)).collect(),
            });
        }
        let held: HashSet<usize> = group_rows.iter().copied().collect();
        let other_ids = db
            .tags
            .iter()
            .zip(locs.iter())
            .filter(|(_, l)| !held.contains(l))
            .map(|(t, _)| t.id)
            .collect();
        let mut links: HashMap<(u32, u32), Vec<usize>> = HashMap::new();
        for (l, &loc) in db
            .tag_tracks
            .iter()
            .zip(db.row_locations(ExtTableType::TagTracks))
        {
            links.entry((l.tag_id, l.track_id)).or_default().push(loc);
        }
        Ok(ExtEditor {
            editor: PdbEditor::from_bytes(data),
            page_size: db.page_size as usize,
            groups,
            group_rows,
            other_ids,
            links,
            dirty: false,
        })
    }

    /// The categories as (id, name), in display order.
    pub fn categories(&self) -> Vec<(u32, &str)> {
        self.groups
            .iter()
            .map(|g| (g.category.id, g.category.name.as_str()))
            .collect()
    }

    /// The tags of a category as (id, name), in display order.
    pub fn tags(&self, category_id: u32) -> Vec<(u32, &str)> {
        self.groups
            .iter()
            .find(|g| g.category.id == category_id)
            .map(|g| g.tags.iter().map(|t| (t.id, t.name.as_str())).collect())
            .unwrap_or_default()
    }

    /// Whether a category or tag already has this id.
    pub fn id_in_use(&self, id: u32) -> bool {
        self.other_ids.contains(&id)
            || self
                .groups
                .iter()
                .any(|g| g.category.id == id || g.tags.iter().any(|t| t.id == id))
    }

    fn check_new(&self, id: u32, name: &str) -> Result<()> {
        if id == 0 || self.id_in_use(id) {
            return Err(PdbError::IdInUse(id));
        }
        self.check_name(name)
    }

    /// Whether a category or tag can be named `name`: it must encode, and its row fit on a page
    /// (or saving would fail).
    pub fn check_name(&self, name: &str) -> Result<()> {
        let probe = Entry {
            id: 0,
            name: name.to_string(),
            unknown: [0; 8],
            raw_category: 0,
        };
        let (_, alloc) = tag_row(&probe, 0, 0)?;
        check_alloc(alloc, self.page_size)
    }

    /// Add a category after the existing ones.
    pub fn add_category(&mut self, id: u32, name: &str) -> Result<()> {
        self.check_new(id, name)?;
        self.groups.push(Group {
            category: Entry {
                id,
                name: name.to_string(),
                unknown: [0; 8],
                raw_category: CATEGORY_FLAG,
            },
            tags: Vec::new(),
        });
        self.dirty = true;
        Ok(())
    }

    /// Add a tag at the end of a category.
    pub fn add_tag(&mut self, category_id: u32, id: u32, name: &str) -> Result<()> {
        self.check_new(id, name)?;
        let group = self
            .groups
            .iter_mut()
            .find(|g| g.category.id == category_id)
            .ok_or(PdbError::TagNotFound(category_id))?;
        group.tags.push(Entry {
            id,
            name: name.to_string(),
            unknown: [0; 8],
            raw_category: 0,
        });
        self.dirty = true;
        Ok(())
    }

    fn entry_mut(&mut self, id: u32) -> Option<&mut Entry> {
        self.groups.iter_mut().find_map(|g| {
            if g.category.id == id {
                Some(&mut g.category)
            } else {
                g.tags.iter_mut().find(|t| t.id == id)
            }
        })
    }

    fn is_tag(&self, id: u32) -> bool {
        self.groups.iter().any(|g| g.tags.iter().any(|t| t.id == id))
    }

    /// Rename a category or tag.
    pub fn rename(&mut self, id: u32, name: &str) -> Result<()> {
        self.check_name(name)?;
        let e = self.entry_mut(id).ok_or(PdbError::TagNotFound(id))?;
        if e.name != name {
            e.name = name.to_string();
            self.dirty = true;
        }
        Ok(())
    }

    /// Remove a tag with its track links, or a category with its tags and their links. Later
    /// tags (or categories) move up.
    pub fn remove(&mut self, id: u32) -> Result<()> {
        let doomed: Vec<u32> = if let Some(i) = self.groups.iter().position(|g| g.category.id == id) {
            let g = self.groups.remove(i);
            g.tags.iter().map(|t| t.id).collect()
        } else {
            let g = self
                .groups
                .iter_mut()
                .find(|g| g.tags.iter().any(|t| t.id == id))
                .ok_or(PdbError::TagNotFound(id))?;
            g.tags.retain(|t| t.id != id);
            vec![id]
        };
        self.dirty = true;
        let keys: Vec<(u32, u32)> = self
            .links
            .keys()
            .filter(|(tag, _)| doomed.contains(tag))
            .copied()
            .collect();
        for key in keys {
            for loc in self.links.remove(&key).unwrap_or_default() {
                self.editor
                    .delete_raw_row(ExtTableType::TagTracks as u32, loc)?;
            }
        }
        Ok(())
    }

    /// Whether a track carries a tag.
    pub fn is_linked(&self, tag_id: u32, track_id: u32) -> bool {
        self.links.contains_key(&(tag_id, track_id))
    }

    /// Give a track (an `export.pdb` track id) a tag. Returns false if it already had it.
    pub fn link(&mut self, tag_id: u32, track_id: u32) -> Result<bool> {
        if !self.is_tag(tag_id) {
            return Err(PdbError::TagNotFound(tag_id));
        }
        if self.is_linked(tag_id, track_id) {
            return Ok(false);
        }
        let mut row = Vec::with_capacity(TAG_TRACK_ROW);
        row.extend_from_slice(&0u32.to_le_bytes());
        row.extend_from_slice(&track_id.to_le_bytes());
        row.extend_from_slice(&tag_id.to_le_bytes());
        row.extend_from_slice(&3u32.to_le_bytes());
        let loc = self.editor.append_raw_row(
            ExtTableType::TagTracks as u32,
            &row,
            TAG_TRACK_ROW,
            None,
        )?;
        self.links.insert((tag_id, track_id), vec![loc]);
        Ok(true)
    }

    /// Take a tag off a track. Returns false if it didn't have it.
    pub fn unlink(&mut self, tag_id: u32, track_id: u32) -> Result<bool> {
        let Some(locs) = self.links.remove(&(tag_id, track_id)) else {
            return Ok(false);
        };
        for loc in locs {
            self.editor
                .delete_raw_row(ExtTableType::TagTracks as u32, loc)?;
        }
        Ok(true)
    }

    /// Take every tag off these tracks (e.g. when they are removed from `export.pdb`, whose ids
    /// a later track may reuse). Returns how many links were removed.
    pub fn unlink_tracks(&mut self, track_ids: &[u32]) -> Result<usize> {
        let ids: HashSet<u32> = track_ids.iter().copied().collect();
        let keys: Vec<(u32, u32)> = self
            .links
            .keys()
            .filter(|(_, track)| ids.contains(track))
            .copied()
            .collect();
        for &(tag, track) in &keys {
            self.unlink(tag, track)?;
        }
        Ok(keys.len())
    }

    /// The edited file. Categories and tags are written out again if any of them changed.
    pub fn to_bytes(&mut self) -> Result<Vec<u8>> {
        if self.dirty {
            // Every row is built (and its strings and size checked) before any old one goes.
            let mut rows = Vec::new();
            for (ci, g) in self.groups.iter().enumerate() {
                rows.push(tag_row(&g.category, 0, ci as u32)?);
                for (ti, t) in g.tags.iter().enumerate() {
                    rows.push(tag_row(t, g.category.id, ti as u32)?);
                }
            }
            for (_, alloc) in &rows {
                check_alloc(*alloc, self.page_size)?;
            }
            for loc in std::mem::take(&mut self.group_rows) {
                self.editor.delete_raw_row(ExtTableType::Tags as u32, loc)?;
            }
            for (row, alloc) in rows {
                let loc = self.editor.append_raw_row(
                    ExtTableType::Tags as u32,
                    &row,
                    alloc,
                    Some(2),
                )?;
                self.group_rows.push(loc);
            }
            self.dirty = false;
        }
        Ok(self.editor.to_bytes())
    }

    pub fn save<P: AsRef<Path>>(&mut self, path: P) -> Result<()> {
        let bytes = self.to_bytes()?;
        std::fs::write(path, bytes)?;
        Ok(())
    }
}
