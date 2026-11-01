//! My Tags: rekordbox's own track tags, grouped in four categories (Genre, Components, Situation
//! and a fourth the DJ names), which CDJ-3000-era players use for Track Filter and Related
//! Tracks.
//!
//! A drive holds them twice: in `exportExt.pdb` for players that read `export.pdb` (tag rows, and
//! rows linking tags to `export.pdb` track ids; see [`rekordbox_pdb::ext`]), and in OneLibrary's
//! `exportLibrary.db` (`myTag`, `myTag_content`). Row ids differ between the two, so categories
//! and tags are matched by name and tracks by file path. Reading reports where the two disagree.
//! An edit changes both, keeps the ids of a category or tag the same in both where it can, and
//! reads both back before either is written. The databases are backed up first (see
//! [`crate::backup`]), so `locallibrary undo` takes an edit back.
//!
//! A drive with `export.pdb` but no `exportExt.pdb` only has its OneLibrary tags edited, with a
//! warning: only `collection export` creates `exportExt.pdb` (see `new_ext`).
//!
//! Every applied edit is recorded in the drive's journal of edits (as a [`TagChange`], tracks
//! named by file), so `edits replay` makes it again after rekordbox exports to the drive.

use std::collections::BTreeSet;

use rustc_hash::{FxHashMap, FxHashSet};
use std::path::{Path, PathBuf};

use rekordbox_pdb::{ExtDatabase, ExtEditor, ExtTableType};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::commit::{self, Changes, NewDatabases, Options};
use crate::device::{Device, LineageOp};
use crate::edit::Replayed;
use crate::error::IoContext;
use crate::hash::Hash;
use crate::model::{Library, TrackRef};
use crate::onelibrary::OneLibrary;
use crate::onelibrary::open_plus;
use crate::paths;
use crate::pdb;
use crate::progress::{Ctx, Phase};
use crate::{Error, Result};

/// rekordbox's My Tag has four categories (columns), which can be renamed but not added to...
pub const MAX_CATEGORIES: usize = 4;
/// ...and up to 50 tags in each.
pub const MAX_TAGS: usize = 50;

/// One of the two databases that hold My Tags.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    /// `exportExt.pdb`, for players that read `export.pdb`.
    ExportExt,
    /// OneLibrary's `exportLibrary.db`.
    OneLibrary,
}

impl Source {
    pub fn file_name(self) -> &'static str {
        match self {
            Source::ExportExt => "exportExt.pdb",
            Source::OneLibrary => "exportLibrary.db",
        }
    }
}

/// A drive's My Tags, from both databases.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct MyTags {
    pub drive: String,
    /// The databases read.
    pub sources: Vec<Source>,
    /// In display order: the first source's order, then anything only the other one has.
    pub categories: Vec<Category>,
    /// Where `exportExt.pdb` and `exportLibrary.db` disagree.
    pub differences: Vec<String>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
#[schemars(rename = "MyTagCategory")]
pub struct Category {
    pub name: String,
    /// The databases that have it.
    pub sources: Vec<Source>,
    pub tags: Vec<TagInfo>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct TagInfo {
    pub name: String,
    /// `Category/Tag`.
    pub path: String,
    pub sources: Vec<Source>,
    /// Tracks carrying it in either database, with [`Device::library`]'s ids (or, for a track
    /// only OneLibrary lists, OneLibrary's).
    pub tracks: Vec<TrackRef>,
}

/// A change to a drive's My Tags. Tags are written `Category/Tag`; names match ignoring case.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Edit {
    /// Give tracks ([`Device::library`] ids) a tag, creating the tag (and its category) in
    /// whichever database lacks it.
    Add { tag: String, tracks: Vec<u32> },
    /// Take a tag off tracks.
    Remove { tag: String, tracks: Vec<u32> },
    /// Rename a tag (`Category/Tag`) or a category (`Category`).
    Rename { tag: String, to: String },
    /// Delete a tag, taking it off every track.
    Delete { tag: String },
}

impl Edit {
    fn action(&self) -> &'static str {
        match self {
            Edit::Add { .. } => "add",
            Edit::Remove { .. } => "remove",
            Edit::Rename { .. } => "rename",
            Edit::Delete { .. } => "delete",
        }
    }

    fn path(&self) -> &str {
        match self {
            Edit::Add { tag, .. } | Edit::Remove { tag, .. } | Edit::Rename { tag, .. } | Edit::Delete { tag } => tag,
        }
    }
}

/// A My Tag edit as the drive's journal records it (see [`crate::edit::journal`]): tracks by
/// their audio files (device paths), so a replay on a later export, where ids differ, finds them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum TagChange {
    Add { tag: String, tracks: Vec<String> },
    Remove { tag: String, tracks: Vec<String> },
    Rename { tag: String, to: String },
    Delete { tag: String },
}

impl TagChange {
    /// The edit for `library`'s tracks ([`Device::library`]), and the recorded files it has no
    /// track for.
    fn edit(&self, library: &Library) -> (Edit, Vec<String>) {
        let by_key: FxHashMap<String, u32> =
            library.tracks.iter().map(|t| (paths::file_key(&t.file_path), t.id)).collect();
        let mut gone = Vec::new();
        let mut ids = |files: &[String]| -> Vec<u32> {
            files
                .iter()
                .filter_map(|f| {
                    let id = by_key.get(&paths::file_key(f)).copied();
                    if id.is_none() {
                        gone.push(f.clone());
                    }
                    id
                })
                .collect()
        };
        let edit = match self {
            TagChange::Add { tag, tracks } => Edit::Add { tag: tag.clone(), tracks: ids(tracks) },
            TagChange::Remove { tag, tracks } => Edit::Remove { tag: tag.clone(), tracks: ids(tracks) },
            TagChange::Rename { tag, to } => Edit::Rename { tag: tag.clone(), to: to.clone() },
            TagChange::Delete { tag } => Edit::Delete { tag: tag.clone() },
        };
        (edit, gone)
    }

    /// `add My Tag Genre/Deep to 3 track(s)`.
    pub fn describe(&self) -> String {
        match self {
            TagChange::Add { tag, tracks } => format!("add My Tag {tag} to {} track(s)", tracks.len()),
            TagChange::Remove { tag, tracks } => format!("take My Tag {tag} off {} track(s)", tracks.len()),
            TagChange::Rename { tag, to } => format!("rename My Tag {tag} to {to:?}"),
            TagChange::Delete { tag } => format!("delete My Tag {tag}"),
        }
    }
}

/// A category or tag an edit creates.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Created {
    /// `Category` or `Category/Tag`.
    pub path: String,
    pub database: Source,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct TagPlan {
    pub drive: String,
    pub root: PathBuf,
    /// `add`, `remove`, `rename` or `delete`.
    pub action: String,
    /// The category or tag edited (as the drive spells it, if it has it).
    pub tag: String,
    /// The new name, for a rename.
    pub to: Option<String>,
    pub created: Vec<Created>,
    /// Tracks that gain (`add`) or lose (`remove`, `delete`) the tag.
    pub tracks: Vec<TrackRef>,
    /// Selected tracks that already have (`add`) or don't have (`remove`) the tag.
    pub unchanged: usize,
    /// The databases the edit changes.
    pub databases: Vec<Source>,
    pub warnings: Vec<String>,
    #[serde(skip)]
    edits: Vec<DbEdit>,
    /// The edit as the journal records it.
    #[serde(skip)]
    recorded: TagChange,
}

impl TagPlan {
    pub fn is_noop(&self) -> bool {
        self.edits.is_empty()
    }
}

#[derive(Debug, Clone, Default, Serialize, JsonSchema)]
pub struct TagReport {
    /// The databases written.
    pub databases: Vec<Source>,
    pub tracks_changed: usize,
    pub backup: Option<String>,
    pub fingerprint: Option<Hash>,
}

// ---------------------------------------------------------------------------------------------
// One database's My Tags
// ---------------------------------------------------------------------------------------------

/// One database's categories and tags, with tracks identified by lower-cased file key.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Tree {
    categories: Vec<CategoryNode>,
    /// Every id a category or tag row uses, including rows that belong to nothing.
    ids: BTreeSet<i64>,
    /// Rows left alone: tags without a category, links to a tag that doesn't exist.
    strays: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CategoryNode {
    id: i64,
    name: String,
    tags: Vec<TagNode>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TagNode {
    id: i64,
    name: String,
    tracks: BTreeSet<String>,
    /// Links to tracks the database doesn't have.
    strays: usize,
}

/// `name` matched exactly, else ignoring case if only one matches.
fn find_named<'a, T>(items: &'a [T], name: impl Fn(&T) -> &str, wanted: &str) -> Option<&'a T> {
    items.iter().find(|i| name(i) == wanted).or_else(|| {
        let lower = wanted.to_lowercase();
        let mut found = items.iter().filter(|i| name(i).to_lowercase() == lower);
        match (found.next(), found.next()) {
            (Some(i), None) => Some(i),
            _ => None,
        }
    })
}

impl Tree {
    fn category(&self, name: &str) -> Option<&CategoryNode> {
        find_named(&self.categories, |c| &c.name, name)
    }

    fn tag(&self, category: &str, tag: &str) -> Option<(&CategoryNode, &TagNode)> {
        let c = self.category(category)?;
        Some((c, find_named(&c.tags, |t| &t.name, tag)?))
    }

    fn tag_by_id(&mut self, id: i64) -> Option<&mut TagNode> {
        self.categories.iter_mut().flat_map(|c| c.tags.iter_mut()).find(|t| t.id == id)
    }

    /// What `op` does, applied to this model: the database should read back the same.
    fn apply(&mut self, op: &Op) {
        match op {
            Op::AddCategory { id, name } => {
                self.categories.push(CategoryNode { id: *id, name: name.clone(), tags: Vec::new() });
                self.ids.insert(*id);
            }
            Op::AddTag { category, id, name } => {
                if let Some(c) = self.categories.iter_mut().find(|c| c.id == *category) {
                    c.tags.push(TagNode { id: *id, name: name.clone(), tracks: BTreeSet::new(), strays: 0 });
                    self.ids.insert(*id);
                }
            }
            Op::Rename { id, name } => {
                if let Some(c) = self.categories.iter_mut().find(|c| c.id == *id) {
                    c.name = name.clone();
                } else if let Some(t) = self.tag_by_id(*id) {
                    t.name = name.clone();
                }
            }
            Op::Remove { id } => {
                if let Some(i) = self.categories.iter().position(|c| c.id == *id) {
                    let c = self.categories.remove(i);
                    self.ids.remove(id);
                    for t in c.tags {
                        self.ids.remove(&t.id);
                    }
                } else {
                    for c in &mut self.categories {
                        c.tags.retain(|t| t.id != *id);
                    }
                    self.ids.remove(id);
                }
            }
            Op::Link { tag, tracks } => {
                if let Some(t) = self.tag_by_id(*tag) {
                    t.tracks.extend(tracks.iter().map(|(_, k)| k.clone()));
                }
            }
            Op::Unlink { tag, tracks } => {
                if let Some(t) = self.tag_by_id(*tag) {
                    for (_, k) in tracks {
                        t.tracks.remove(k);
                    }
                }
            }
        }
    }
}

/// `track id -> key` for a library.
fn keys_by_id(library: &Library) -> FxHashMap<u32, String> {
    library.tracks.iter().map(|t| (t.id, paths::file_key(&t.file_path))).collect()
}

fn ext_tree(db: &ExtDatabase, library: &Library) -> Tree {
    let keys = keys_by_id(library);
    let mut tree = Tree { ids: db.tags.iter().map(|t| i64::from(t.id)).collect(), ..Default::default() };
    let mut placed = 0;
    for c in db.categories() {
        if tree.categories.iter().any(|x| x.id == i64::from(c.id)) {
            continue; // a second row with the same id belongs to nothing
        }
        let tags: Vec<TagNode> = db
            .tags_in(c.id)
            .iter()
            .map(|t| TagNode { id: t.id.into(), name: t.name.clone(), tracks: BTreeSet::new(), strays: 0 })
            .collect();
        placed += 1 + tags.len();
        tree.categories.push(CategoryNode { id: c.id.into(), name: c.name.clone(), tags });
    }
    tree.strays = db.tags.len() - placed;
    for link in &db.tag_tracks {
        match (tree.tag_by_id(link.tag_id.into()), keys.get(&link.track_id)) {
            (Some(t), Some(key)) => {
                t.tracks.insert(key.clone());
            }
            (Some(t), None) => t.strays += 1,
            (None, _) => tree.strays += 1,
        }
    }
    tree
}

fn plus_tree(one: &OneLibrary, library: &Library) -> Result<Tree> {
    let rows = one.my_tag_rows()?;
    let keys = keys_by_id(library);
    let mut tree = Tree { ids: rows.iter().map(|r| r.id).collect(), ..Default::default() };
    let mut placed = 0;
    for c in rows.iter().filter(|r| r.is_category && r.parent == 0) {
        let tags: Vec<TagNode> = rows
            .iter()
            .filter(|r| !r.is_category && r.parent == c.id)
            .map(|t| TagNode { id: t.id, name: t.name.clone(), tracks: BTreeSet::new(), strays: 0 })
            .collect();
        placed += 1 + tags.len();
        tree.categories.push(CategoryNode { id: c.id, name: c.name.clone(), tags });
    }
    tree.strays = rows.len() - placed;
    for (tag, track) in one.my_tag_links()? {
        let key = u32::try_from(track).ok().and_then(|id| keys.get(&id));
        match (tree.tag_by_id(tag), key) {
            (Some(t), Some(key)) => {
                t.tracks.insert(key.clone());
            }
            (Some(t), None) => t.strays += 1,
            (None, _) => tree.strays += 1,
        }
    }
    Ok(tree)
}

/// One of a drive's My Tag databases, read for planning or editing.
struct Db {
    source: Source,
    tree: Tree,
    /// The tracks this database lists (`export.pdb`'s for `exportExt.pdb`).
    library: Library,
    keys: FxHashMap<u32, String>,
    ids_by_key: FxHashMap<String, Vec<u32>>,
    handle: Handle,
}

enum Handle {
    /// `exportExt.pdb`'s bytes.
    Ext(Vec<u8>),
    Plus(Box<OneLibrary>),
}

impl Db {
    fn new(source: Source, tree: Tree, library: Library, handle: Handle) -> Db {
        let keys = keys_by_id(&library);
        let mut ids_by_key: FxHashMap<String, Vec<u32>> = FxHashMap::default();
        for (id, key) in &keys {
            ids_by_key.entry(key.clone()).or_default().push(*id);
        }
        for ids in ids_by_key.values_mut() {
            ids.sort_unstable();
        }
        Db { source, tree, library, keys, ids_by_key, handle }
    }

    fn file(&self) -> &'static str {
        self.source.file_name()
    }
}

/// Parse `exportExt.pdb` after the checks [`pdb::parse_database`] makes, converting panics on
/// malformed input into errors. Both My Tag tables must be there.
fn parse_ext(bytes: Vec<u8>, path: &Path) -> Result<ExtDatabase> {
    pdb::validate(&bytes, path)?;
    let db = match pdb::catch_quietly(|| ExtDatabase::from_bytes(bytes)) {
        Ok(Ok(db)) => db,
        Ok(Err(e)) => return Err(Error::parse(path, e)),
        Err(_) => return Err(Error::parse(path, "malformed database (truncated or corrupt pages)")),
    };
    if !db.has_table(ExtTableType::Tags) || !db.has_table(ExtTableType::TagTracks) {
        return Err(Error::parse(path, "it has no My Tag tables"));
    }
    Ok(db)
}

/// An editor for `exportExt.pdb` (`bytes`), refused like [`parse_ext`] refuses a damaged file:
/// the editor would otherwise read a cyclic page chain as repeated rows and edit those. Run it
/// in [`guard_ext`].
pub(crate) fn ext_editor(bytes: Vec<u8>) -> Result<ExtEditor> {
    pdb::validate(&bytes, Path::new(paths::EXPORT_EXT_PDB))?;
    Ok(ExtEditor::from_bytes(bytes)?)
}

/// [`pdb::guard_editor`] for `exportExt.pdb`, whose editor's errors would otherwise be labelled
/// export.pdb's.
pub(crate) fn guard_ext<T>(f: impl FnOnce() -> Result<T>) -> Result<T> {
    pdb::guard_editor(f).map_err(|e| match e {
        Error::Pdb(m) => Error::ExportExt(m),
        e => e,
    })
}

fn read_ext(device: &Device, library: &Library) -> Result<Db> {
    let path = device.path(paths::EXPORT_EXT_PDB);
    let bytes = crate::fsutil::read(&path).at(&path)?;
    let db = parse_ext(bytes.clone(), &path)?;
    Ok(Db::new(Source::ExportExt, ext_tree(&db, library), library.clone(), Handle::Ext(bytes)))
}

/// A drive's main library ([`Device::library`]) and its My Tag databases.
struct Opened {
    primary: Library,
    dbs: Vec<Db>,
    warnings: Vec<String>,
}

fn open(device: &Device) -> Result<Opened> {
    if !device.has_export() {
        return Err(Error::NotAnExport(device.root().to_path_buf()));
    }
    let mut warnings = Vec::new();
    let has_pdb = device.has_device_library();
    let has_ext = device.path(paths::EXPORT_EXT_PDB).is_file();
    let pdb_library = has_pdb.then(|| pdb::read_library(&device.path(paths::EXPORT_PDB))).transpose()?;
    let mut dbs = Vec::new();
    let mut ext_error = None;
    match &pdb_library {
        Some(library) if has_ext => match read_ext(device, library) {
            Ok(db) => dbs.push(db),
            Err(e) => ext_error = Some(e),
        },
        Some(_) => {}
        None if has_ext => warnings.push(
            "exportExt.pdb is left alone: its tags point at export.pdb's tracks, and the drive has no export.pdb"
                .to_string(),
        ),
        None => {}
    }
    if let Some((library, one)) = open_plus(device, has_pdb, &mut warnings)? {
        let tree = plus_tree(&one, &library)?;
        dbs.push(Db::new(Source::OneLibrary, tree, library, Handle::Plus(Box::new(one))));
    }
    match ext_error {
        Some(e) if dbs.is_empty() => return Err(e),
        Some(e) => warnings.push(format!("{e}. Only exportLibrary.db's My Tags are read and edited")),
        None if has_pdb && !has_ext && !dbs.is_empty() => warnings.push(
            "the drive has no exportExt.pdb, which holds My Tags for players that read export.pdb; only \
             exportLibrary.db's are read and edited (locallibrary can't create exportExt.pdb)"
                .to_string(),
        ),
        None => {}
    }
    for db in &dbs {
        let strays = db.tree.strays + db.tree.categories.iter().flat_map(|c| &c.tags).map(|t| t.strays).sum::<usize>();
        if strays > 0 {
            warnings.push(format!(
                "{}: {strays} My Tag row(s) belong to no category, or point at a tag or track that isn't there; \
                 they're left alone",
                db.file()
            ));
        }
    }
    let primary = match pdb_library {
        Some(l) => l,
        None => dbs.iter().find(|d| d.source == Source::OneLibrary).map(|d| d.library.clone()).unwrap_or_default(),
    };
    Ok(Opened { primary, dbs, warnings })
}

// ---------------------------------------------------------------------------------------------
// Reading
// ---------------------------------------------------------------------------------------------

struct MergedCategory {
    name: String,
    sources: Vec<Source>,
    tags: Vec<MergedTag>,
}

struct MergedTag {
    name: String,
    sources: Vec<Source>,
    tracks: BTreeSet<String>,
}

/// Both databases' categories and tags in one list: the first one's order, then whatever only
/// the other has. Names match exactly; tracks are the union.
fn merged(dbs: &[Db]) -> Vec<MergedCategory> {
    let mut out: Vec<MergedCategory> = Vec::new();
    for db in dbs {
        for c in &db.tree.categories {
            let i = match out.iter().position(|m| m.name == c.name) {
                Some(i) => i,
                None => {
                    out.push(MergedCategory { name: c.name.clone(), sources: Vec::new(), tags: Vec::new() });
                    out.len() - 1
                }
            };
            let m = &mut out[i];
            m.sources.push(db.source);
            for t in &c.tags {
                match m.tags.iter_mut().find(|x| x.name == t.name) {
                    Some(x) => {
                        x.sources.push(db.source);
                        x.tracks.extend(t.tracks.iter().cloned());
                    }
                    None => m.tags.push(MergedTag {
                        name: t.name.clone(),
                        sources: vec![db.source],
                        tracks: t.tracks.clone(),
                    }),
                }
            }
        }
    }
    out
}

/// Where two databases' My Tags disagree. Tracks only one database lists aren't counted (that's
/// a difference between the libraries, which `info` reports).
fn differences(dbs: &[Db]) -> Vec<String> {
    let [a, b] = dbs else { return Vec::new() };
    let (an, bn) = (a.file(), b.file());
    let mut out = Vec::new();
    let names = |x: &Tree, other: &Tree| -> Vec<String> {
        x.categories
            .iter()
            .filter(|c| other.categories.iter().any(|o| o.name == c.name))
            .map(|c| c.name.clone())
            .collect()
    };
    for (x, y, file) in [(a, b, an), (b, a, bn)] {
        for c in x.tree.categories.iter().filter(|c| !y.tree.categories.iter().any(|o| o.name == c.name)) {
            out.push(format!("category {:?} is only in {file}", c.name));
        }
    }
    if names(&a.tree, &b.tree) != names(&b.tree, &a.tree) {
        out.push("the categories are in a different order".to_string());
    }
    for ca in &a.tree.categories {
        let Some(cb) = b.tree.categories.iter().find(|c| c.name == ca.name) else { continue };
        for (x, y, file) in [(ca, cb, an), (cb, ca, bn)] {
            for t in x.tags.iter().filter(|t| !y.tags.iter().any(|o| o.name == t.name)) {
                out.push(format!("{}/{} is only in {file}", ca.name, t.name));
            }
        }
        let common = |x: &CategoryNode, y: &CategoryNode| -> Vec<String> {
            x.tags.iter().filter(|t| y.tags.iter().any(|o| o.name == t.name)).map(|t| t.name.clone()).collect()
        };
        if common(ca, cb) != common(cb, ca) {
            out.push(format!("{}'s tags are in a different order", ca.name));
        }
        for ta in &ca.tags {
            let Some(tb) = cb.tags.iter().find(|t| t.name == ta.name) else { continue };
            let only = |x: &TagNode, y: &TagNode, other: &Db| {
                x.tracks.iter().filter(|k| !y.tracks.contains(*k) && other.ids_by_key.contains_key(*k)).count()
            };
            let parts: Vec<String> = [(only(ta, tb, b), an), (only(tb, ta, a), bn)]
                .into_iter()
                .filter(|(n, _)| *n > 0)
                .map(|(n, file)| format!("{n} track(s) tagged only in {file}"))
                .collect();
            if !parts.is_empty() {
                out.push(format!("{}/{}: {}", ca.name, ta.name, parts.join(", ")));
            }
        }
    }
    out
}

/// Display references for track keys: the main library's tracks, then any only another
/// database lists.
fn track_refs(opened: &Opened) -> FxHashMap<String, TrackRef> {
    let mut refs = FxHashMap::default();
    for lib in std::iter::once(&opened.primary).chain(opened.dbs.iter().map(|d| &d.library)) {
        for t in &lib.tracks {
            refs.entry(paths::file_key(&t.file_path)).or_insert_with(|| TrackRef::from(t));
        }
    }
    refs
}

/// A drive's My Tags: categories and tags in order, the tracks carrying each, and where the two
/// databases disagree.
pub fn read(device: &Device) -> Result<MyTags> {
    let opened = open(device)?;
    let refs = track_refs(&opened);
    let categories = merged(&opened.dbs)
        .into_iter()
        .map(|c| Category {
            tags: c
                .tags
                .into_iter()
                .map(|t| TagInfo {
                    path: format!("{}/{}", c.name, t.name),
                    tracks: t.tracks.iter().filter_map(|k| refs.get(k).cloned()).collect(),
                    name: t.name,
                    sources: t.sources,
                })
                .collect(),
            name: c.name,
            sources: c.sources,
        })
        .collect();
    let mut warnings = opened.warnings.clone();
    if opened.dbs.is_empty() {
        warnings.push("the drive has no My Tags database (exportExt.pdb or exportLibrary.db)".to_string());
    }
    Ok(MyTags {
        drive: device.name(),
        sources: opened.dbs.iter().map(|d| d.source).collect(),
        categories,
        differences: differences(&opened.dbs),
        warnings,
    })
}

/// Every tagged track's tags, as `Category/Tag` in display order, keyed by the track's file
/// ([`paths::file_key`]). Tags from both databases.
pub fn by_track(device: &Device) -> Result<FxHashMap<String, Vec<String>>> {
    let opened = open(device)?;
    let mut out: FxHashMap<String, Vec<String>> = FxHashMap::default();
    for c in merged(&opened.dbs) {
        for t in &c.tags {
            for key in &t.tracks {
                out.entry(key.clone()).or_default().push(format!("{}/{}", c.name, t.name));
            }
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------------------------------------
// Planning
// ---------------------------------------------------------------------------------------------

/// One step of an edit, in one database's ids. Tracks are (id, key).
#[derive(Debug, Clone, PartialEq, Eq)]
enum Op {
    AddCategory {
        id: i64,
        name: String,
    },
    AddTag {
        category: i64,
        id: i64,
        name: String,
    },
    Rename {
        id: i64,
        name: String,
    },
    /// A tag with its links, or a category with its tags.
    Remove {
        id: i64,
    },
    Link {
        tag: i64,
        tracks: Vec<(u32, String)>,
    },
    Unlink {
        tag: i64,
        tracks: Vec<(u32, String)>,
    },
}

/// One database's part of a plan.
#[derive(Debug, Clone)]
struct DbEdit {
    source: Source,
    /// The database's My Tags when planned; applying refuses if they changed since.
    before: Tree,
    /// What it must read back as.
    after: Tree,
    ops: Vec<Op>,
}

impl DbEdit {
    fn tracks(&self) -> impl Iterator<Item = &(u32, String)> {
        self.ops.iter().flat_map(|op| match op {
            Op::Link { tracks, .. } | Op::Unlink { tracks, .. } => tracks.as_slice(),
            _ => &[],
        })
    }
}

/// `Category/Tag` (or `Category`): split at the first `/`.
fn split_path(path: &str) -> Result<(&str, Option<&str>)> {
    let (category, tag) = match path.split_once('/') {
        Some((c, t)) => (c.trim(), Some(t.trim())),
        None => (path.trim(), None),
    };
    if category.is_empty() || tag.is_some_and(str::is_empty) {
        return Err(Error::invalid(format!("{path:?} isn't a My Tag: write Category/Tag, e.g. \"Genre/Deep House\"")));
    }
    Ok((category, tag))
}

/// A tag's path is split at its first `/`, so a new name with one would read as another level
/// (`Genre/Genre/Dark`), and a category's would be split.
fn slash_in_name(category: bool) -> Error {
    Error::invalid(format!("a {} name can't contain '/'", if category { "category" } else { "tag" }))
}

fn need_tag<'a>(path: &str, tag: Option<&'a str>) -> Result<&'a str> {
    tag.ok_or_else(|| Error::invalid(format!("{path:?} is a category: write Category/Tag, e.g. \"Genre/Deep House\"")))
}

/// Ids every database uses, so a new category or tag can get the same id in each.
struct Ids {
    used: BTreeSet<i64>,
    max_category: i64,
}

impl Ids {
    fn of<'a>(trees: impl IntoIterator<Item = &'a Tree>) -> Ids {
        let mut ids = Ids { used: BTreeSet::new(), max_category: 0 };
        for tree in trees {
            ids.used.extend(tree.ids.iter().copied());
            ids.max_category = tree.categories.iter().map(|c| c.id).fold(ids.max_category, i64::max);
        }
        ids
    }

    /// Note the id of a category or tag just planned.
    fn take(&mut self, id: i64, is_category: bool) {
        self.used.insert(id);
        if is_category {
            self.max_category = self.max_category.max(id);
        }
    }

    fn fits(source: Source, id: i64) -> bool {
        match source {
            Source::ExportExt => (1..=i64::from(u32::MAX)).contains(&id),
            Source::OneLibrary => id >= 1,
        }
    }

    /// The other database's id for the same category, if this one doesn't use it; else the next
    /// small id (rekordbox's categories are 1..4).
    fn category(&self, source: Source, tree: &Tree, other: Option<&Tree>, name: &str) -> i64 {
        if let Some(id) = other.and_then(|o| o.category(name)).map(|c| c.id)
            && Ids::fits(source, id)
            && !tree.ids.contains(&id)
        {
            return id;
        }
        let mut id = self.max_category + 1;
        while self.used.contains(&id) {
            id += 1;
        }
        id
    }

    /// The other database's id for the same tag, if this one doesn't use it; else a large id
    /// (like rekordbox's) derived from the names, which neither database uses.
    fn tag(&self, source: Source, tree: &Tree, other: Option<&Tree>, category: &str, name: &str) -> i64 {
        if let Some((_, t)) = other.and_then(|o| o.tag(category, name))
            && Ids::fits(source, t.id)
            && !tree.ids.contains(&t.id)
        {
            return t.id;
        }
        let seed = format!("{}\u{1f}{}", category.to_lowercase(), name.to_lowercase());
        let hash = blake3::hash(seed.as_bytes());
        let mut id = i64::from(u32::from_le_bytes(hash.as_bytes()[..4].try_into().expect("4 bytes")));
        while id < 0x1_0000 || self.used.contains(&id) {
            id = if id >= i64::from(u32::MAX) { 0x1_0000 } else { id + 1 };
        }
        id
    }
}

/// The steps that make `edit` in one database. `keys` are the selected tracks'.
fn plan_db(
    db: &Db,
    other: Option<&Db>,
    ids: &Ids,
    edit: &Edit,
    keys: &BTreeSet<String>,
    warnings: &mut Vec<String>,
) -> Result<Vec<Op>> {
    let file = db.file();
    let (category, tag) = split_path(edit.path())?;
    let tracks_for = |keys: &mut dyn Iterator<Item = &String>| -> Vec<(u32, String)> {
        keys.flat_map(|k| db.ids_by_key.get(k).into_iter().flatten().map(move |id| (*id, k.clone()))).collect()
    };
    let mut ops = Vec::new();
    match edit {
        Edit::Add { .. } => {
            let tag = need_tag(edit.path(), tag)?;
            // Spell a new category or tag the way the other database does, so the names match.
            let spelled = |o: Option<&str>, typed: &str| o.map(str::to_string).unwrap_or_else(|| typed.to_string());
            let existing = db.tree.category(category);
            let category_name = spelled(
                existing.or_else(|| other.and_then(|o| o.tree.category(category))).map(|c| c.name.as_str()),
                category,
            );
            let category_id = match existing {
                Some(c) => c.id,
                None => {
                    if db.tree.categories.len() >= MAX_CATEGORIES {
                        let names: Vec<&str> = db.tree.categories.iter().map(|c| c.name.as_str()).collect();
                        return Err(Error::invalid(format!(
                            "{file} already has rekordbox's {MAX_CATEGORIES} My Tag categories ({}); use one of them, or \
                             rename one to {category:?}",
                            names.join(", ")
                        )));
                    }
                    let id = ids.category(db.source, &db.tree, other.map(|o| &o.tree), &category_name);
                    ops.push(Op::AddCategory { id, name: category_name.clone() });
                    id
                }
            };
            let node = existing.and_then(|c| find_named(&c.tags, |t| &t.name, tag));
            let tag_id = match node {
                Some(t) => t.id,
                None => {
                    if existing.is_some_and(|c| c.tags.len() >= MAX_TAGS) {
                        return Err(Error::invalid(format!(
                            "{category_name} already has {MAX_TAGS} tags in {file}, as many as rekordbox allows"
                        )));
                    }
                    let tag_name =
                        spelled(other.and_then(|o| o.tree.tag(category, tag)).map(|(_, t)| t.name.as_str()), tag);
                    let id = ids.tag(db.source, &db.tree, other.map(|o| &o.tree), &category_name, &tag_name);
                    ops.push(Op::AddTag { category: category_id, id, name: tag_name });
                    id
                }
            };
            let missing = keys.iter().filter(|k| !db.ids_by_key.contains_key(*k)).count();
            if missing > 0 {
                warnings.push(format!(
                    "{missing} selected track(s) aren't in {file}, so players that read it won't show the tag on them"
                ));
            }
            let link = tracks_for(&mut keys.iter().filter(|k| !node.is_some_and(|t| t.tracks.contains(*k))));
            if !link.is_empty() {
                ops.push(Op::Link { tag: tag_id, tracks: link });
            }
        }
        Edit::Remove { .. } => {
            let tag = need_tag(edit.path(), tag)?;
            if let Some((_, t)) = db.tree.tag(category, tag) {
                let unlink = tracks_for(&mut keys.iter().filter(|k| t.tracks.contains(*k)));
                if !unlink.is_empty() {
                    ops.push(Op::Unlink { tag: t.id, tracks: unlink });
                }
            }
        }
        Edit::Rename { to, .. } => {
            let to = to.trim();
            if to.is_empty() {
                return Err(Error::invalid("give a new name"));
            }
            if to.contains('/') {
                return Err(slash_in_name(tag.is_none()));
            }
            match tag {
                None => {
                    if let Some(c) = db.tree.category(category) {
                        if db.tree.categories.iter().any(|o| o.id != c.id && o.name.to_lowercase() == to.to_lowercase())
                        {
                            return Err(Error::invalid(format!("{file} already has a category named {to:?}")));
                        }
                        if c.name != to {
                            ops.push(Op::Rename { id: c.id, name: to.to_string() });
                        }
                    }
                }
                Some(tag) => {
                    if let Some((c, t)) = db.tree.tag(category, tag) {
                        if c.tags.iter().any(|o| o.id != t.id && o.name.to_lowercase() == to.to_lowercase()) {
                            return Err(Error::invalid(format!("{} already has a tag named {to:?} in {file}", c.name)));
                        }
                        if t.name != to {
                            ops.push(Op::Rename { id: t.id, name: to.to_string() });
                        }
                    }
                }
            }
        }
        Edit::Delete { .. } => {
            let Some(tag) = tag else {
                return Err(Error::invalid(format!(
                    "rekordbox's {MAX_CATEGORIES} My Tag categories can be renamed but not deleted; delete the tags in \
                     {category:?} one by one, or rename it"
                )));
            };
            if let Some((_, t)) = db.tree.tag(category, tag) {
                ops.push(Op::Remove { id: t.id });
            }
        }
    }
    Ok(ops)
}

/// Refuse a name too long for one exportExt.pdb row, which its editor would only refuse when
/// applying (OneLibrary takes any length).
fn check_names(db: &Db, ops: &[Op]) -> Result<()> {
    let Handle::Ext(bytes) = &db.handle else { return Ok(()) };
    let editor = guard_ext(|| ext_editor(bytes.clone()))?;
    for op in ops {
        let (what, name) = match op {
            Op::AddCategory { name, .. } => ("the category name", name),
            Op::AddTag { name, .. } => ("the tag name", name),
            Op::Rename { name, .. } => ("the new name", name),
            _ => continue,
        };
        if editor.check_name(name).is_err() {
            return Err(Error::invalid(format!(
                "{what} is too long for exportExt.pdb ({} characters)",
                name.chars().count()
            )));
        }
    }
    Ok(())
}

/// What `edit` would change on the drive.
pub fn plan(device: &Device, edit: &Edit) -> Result<TagPlan> {
    let opened = open(device)?;
    if opened.dbs.is_empty() {
        return Err(Error::NotFound(format!(
            "a My Tags database on {}: it has neither exportExt.pdb nor exportLibrary.db",
            device.name()
        )));
    }
    let mut warnings = opened.warnings.clone();
    let (category, tag) = split_path(edit.path())?;

    // The selected tracks, by key.
    let selected: &[u32] = match edit {
        Edit::Add { tracks, .. } | Edit::Remove { tracks, .. } => tracks,
        _ => &[],
    };
    if matches!(edit, Edit::Add { .. } | Edit::Remove { .. }) && selected.is_empty() {
        return Err(Error::invalid("no tracks selected"));
    }
    let primary_keys = keys_by_id(&opened.primary);
    let keys: BTreeSet<String> = selected
        .iter()
        .map(|id| primary_keys.get(id).cloned().ok_or_else(|| Error::no_track(*id)))
        .collect::<Result<_>>()?;

    // Where the category or tag already is, for the plan's name and "not found".
    let found = opened.dbs.iter().find_map(|d| match tag {
        Some(t) => d.tree.tag(category, t).map(|(c, t)| format!("{}/{}", c.name, t.name)),
        None => d.tree.category(category).map(|c| c.name.clone()),
    });
    if found.is_none() && !matches!(edit, Edit::Add { .. }) {
        return Err(Error::NotFound(format!("My Tag {:?} on {}", edit.path(), device.name())));
    }
    // A tag neither database has is new; one rekordbox named with a '/' is still found.
    if found.is_none() && tag.is_some_and(|t| t.contains('/')) {
        return Err(slash_in_name(false));
    }

    let ids = Ids::of(opened.dbs.iter().map(|d| &d.tree));
    let mut edits = Vec::new();
    for (i, db) in opened.dbs.iter().enumerate() {
        let other = opened.dbs.iter().enumerate().find(|(j, _)| *j != i).map(|(_, d)| d);
        let ops = plan_db(db, other, &ids, edit, &keys, &mut warnings)?;
        if ops.is_empty() {
            continue;
        }
        check_names(db, &ops)?;
        let mut after = db.tree.clone();
        for op in &ops {
            after.apply(op);
        }
        edits.push(DbEdit { source: db.source, before: db.tree.clone(), after, ops });
    }

    let mut created = Vec::new();
    let mut changed: BTreeSet<String> = BTreeSet::new();
    for e in &edits {
        for op in &e.ops {
            match op {
                Op::AddCategory { name, .. } => created.push(Created { path: name.clone(), database: e.source }),
                Op::AddTag { category, name, .. } => {
                    let c = e.after.categories.iter().find(|c| c.id == *category).map_or("", |c| c.name.as_str());
                    created.push(Created { path: format!("{c}/{name}"), database: e.source });
                }
                Op::Remove { id } => {
                    for t in e.before.categories.iter().flat_map(|c| &c.tags).filter(|t| t.id == *id) {
                        changed.extend(t.tracks.iter().cloned());
                    }
                }
                Op::Link { tracks, .. } | Op::Unlink { tracks, .. } => {
                    changed.extend(tracks.iter().map(|(_, k)| k.clone()));
                }
                Op::Rename { .. } => {}
            }
        }
    }
    let refs = track_refs(&opened);
    let files =
        || -> Vec<String> { opened.primary.tracks_by_id(selected).into_iter().map(|t| t.file_path.clone()).collect() };
    let spelled = found.clone().unwrap_or_else(|| edit.path().trim().to_string());
    let recorded = match edit {
        Edit::Add { .. } => TagChange::Add { tag: spelled, tracks: files() },
        Edit::Remove { .. } => TagChange::Remove { tag: spelled, tracks: files() },
        Edit::Rename { to, .. } => TagChange::Rename { tag: spelled, to: to.trim().to_string() },
        Edit::Delete { .. } => TagChange::Delete { tag: spelled },
    };
    Ok(TagPlan {
        drive: device.name(),
        root: device.root().to_path_buf(),
        action: edit.action().to_string(),
        tag: found.unwrap_or_else(|| {
            let c = opened.dbs.iter().find_map(|d| d.tree.category(category)).map_or(category, |c| c.name.as_str());
            format!("{c}/{}", tag.unwrap_or_default())
        }),
        to: match edit {
            Edit::Rename { to, .. } => Some(to.trim().to_string()),
            _ => None,
        },
        created,
        unchanged: keys.iter().filter(|k| !changed.contains(*k)).count(),
        tracks: changed.iter().filter_map(|k| refs.get(k).cloned()).collect(),
        databases: edits.iter().map(|e| e.source).collect(),
        warnings,
        edits,
        recorded,
    })
}

// ---------------------------------------------------------------------------------------------
// Applying
// ---------------------------------------------------------------------------------------------

fn ext_id(id: i64) -> Result<u32> {
    u32::try_from(id).map_err(|_| Error::ExportExt(format!("My Tag id {id} doesn't fit")))
}

fn ext_ops(editor: &mut ExtEditor, ops: &[Op]) -> Result<()> {
    for op in ops {
        match op {
            Op::AddCategory { id, name } => editor.add_category(ext_id(*id)?, name)?,
            Op::AddTag { category, id, name } => editor.add_tag(ext_id(*category)?, ext_id(*id)?, name)?,
            Op::Rename { id, name } => editor.rename(ext_id(*id)?, name)?,
            Op::Remove { id } => editor.remove(ext_id(*id)?)?,
            Op::Link { tag, tracks } => {
                for (track, _) in tracks {
                    editor.link(ext_id(*tag)?, *track)?;
                }
            }
            Op::Unlink { tag, tracks } => {
                for (track, _) in tracks {
                    editor.unlink(ext_id(*tag)?, *track)?;
                }
            }
        }
    }
    Ok(())
}

fn edit_ext(bytes: Vec<u8>, ops: &[Op]) -> Result<Vec<u8>> {
    guard_ext(|| {
        let mut editor = ext_editor(bytes)?;
        ext_ops(&mut editor, ops)?;
        Ok(editor.to_bytes()?)
    })
}

fn edit_plus(one: &mut OneLibrary, ops: &[Op]) -> Result<()> {
    let ids = |tracks: &[(u32, String)]| tracks.iter().map(|(id, _)| *id).collect::<Vec<u32>>();
    for op in ops {
        match op {
            Op::AddCategory { id, name } => one.add_my_tag(*id, 0, name, true)?,
            Op::AddTag { category, id, name } => one.add_my_tag(*id, *category, name, false)?,
            Op::Rename { id, name } => one.rename_my_tag(*id, name)?,
            Op::Remove { id } => one.remove_my_tag(*id)?,
            Op::Link { tag, tracks } => {
                one.link_my_tag(*tag, &ids(tracks))?;
            }
            Op::Unlink { tag, tracks } => {
                one.unlink_my_tag(*tag, &ids(tracks))?;
            }
        }
    }
    Ok(())
}

/// Make a planned edit, and record it in the drive's journal.
pub fn apply(plan: &TagPlan, device: &mut Device, ctx: &Ctx<'_>) -> Result<TagReport> {
    if plan.is_noop() {
        return Ok(TagReport::default());
    }
    let _lock = commit::lock(device, None)?;
    crate::edit::journal::list(device)?; // one this build can add to
    let edit = [crate::edit::Edit::MyTag(plan.recorded.clone())];
    let changes = [plan.recorded.describe()];
    apply_locked(plan, device, Some((&edit, &changes)), ctx)
}

/// Make an edit of a plan's, under the backup the plan made (see [`crate::edit::apply`]).
/// Replaying (not `strict`), one that no longer applies is skipped with a warning; an error is a
/// failure while writing.
pub(crate) fn replay(device: &mut Device, change: &TagChange, strict: bool, ctx: &Ctx<'_>) -> Result<Replayed> {
    let mut out = Replayed::default();
    let (edit, gone) = change.edit(&device.library()?);
    if let Some(example) = gone.first() {
        out.tracks_gone(strict, gone.len(), example, &change.describe())?;
    }
    let plan = match plan(device, &edit) {
        Ok(plan) => plan,
        Err(e) => return out.skipped(strict, &change.describe(), e),
    };
    out.warnings.extend(plan.warnings.iter().cloned());
    if !plan.is_noop() {
        let report = apply_locked(&plan, device, None, ctx)?;
        out.databases = report.databases.iter().map(|s| s.file_name().to_string()).collect();
        out.fingerprint = report.fingerprint;
    }
    Ok(out)
}

/// [`apply`] with the drive locked already. With `journal` (the edit as recorded, and what it
/// did), the drive is backed up first and the edit recorded; a replay has backed it up already.
fn apply_locked(
    plan: &TagPlan,
    device: &mut Device,
    journal: Option<(&[crate::edit::Edit], &[String])>,
    ctx: &Ctx<'_>,
) -> Result<TagReport> {
    // Every database the plan edits must still hold what was planned (rekordbox may have
    // exported again, or another edit come first), down to the tracks' ids.
    let changed = || Error::stale(device.name(), "changed");
    let mut opened = open(device)?;
    let mut work = Vec::new();
    for edit in &plan.edits {
        let i = opened.dbs.iter().position(|d| d.source == edit.source).ok_or_else(changed)?;
        let db = opened.dbs.swap_remove(i);
        if db.tree != edit.before || edit.tracks().any(|(id, key)| db.keys.get(id) != Some(key)) {
            return Err(changed());
        }
        work.push((edit, db));
    }
    ctx.check()?;

    // 1. Both databases edited and read back before anything on the drive changes (a failure
    // here leaves no backup and no interrupted operation).
    ctx.phase_start(Phase::Write, work.len() as u64, 0);
    let failed = "edited database failed verification; drive left unchanged";
    let mut edited = NewDatabases::default();
    for (edit, db) in work {
        match db.handle {
            Handle::Ext(bytes) => {
                let path = device.path(paths::EXPORT_EXT_PDB);
                let bytes = edit_ext(bytes, &edit.ops)?;
                if ext_tree(&parse_ext(bytes.clone(), &path)?, &db.library) != edit.after {
                    return Err(Error::ExportExt(failed.into()));
                }
                edited.ext = Some(bytes);
            }
            Handle::Plus(mut one) => {
                edit_plus(&mut one, &edit.ops)?;
                let bytes = one.to_encrypted()?;
                if plus_tree(&OneLibrary::from_encrypted(&bytes, None)?, &db.library)? != edit.after {
                    return Err(Error::OneLibrary(failed.into()));
                }
                edited.plus = Some(bytes);
            }
        }
    }
    ctx.phase_end(Phase::Write);

    // 2. Back up, swap the databases in, record the new state (and the edit).
    let description = format!("My Tags: {} {}", plan.action, plan.tag);
    let mut opts = Options::new(LineageOp::Edit, description);
    if journal.is_some() {
        opts = opts.backup("before editing My Tags");
    }
    opts.journal = journal;
    let done = commit::commit(device, Changes { databases: edited, ..Default::default() }, opts, ctx)?;
    Ok(TagReport {
        databases: plan.databases.clone(),
        tracks_changed: plan.tracks.len(),
        backup: done.backup,
        fingerprint: done.fingerprint,
    })
}

// ---------------------------------------------------------------------------------------------
// Carrying tags along (merge, convert)
// ---------------------------------------------------------------------------------------------

/// One database's My Tags, for merge and convert, which match tracks across databases (and
/// drives) by file key.
#[derive(Debug, Clone)]
pub(crate) struct DbTags {
    tree: Tree,
    /// Keys of the tracks the database lists.
    tracks: FxHashSet<String>,
}

impl DbTags {
    fn new(tree: Tree, library: &Library) -> DbTags {
        DbTags { tree, tracks: library.tracks.iter().map(|t| paths::file_key(&t.file_path)).collect() }
    }

    /// `exportExt.pdb`'s, whose track ids are `library`'s (the drive's export.pdb).
    pub(crate) fn ext(bytes: &[u8], library: &Library) -> Result<DbTags> {
        let db = parse_ext(bytes.to_vec(), Path::new(paths::EXPORT_EXT_PDB))?;
        Ok(DbTags::new(ext_tree(&db, library), library))
    }

    /// A drive's `exportExt.pdb`'s, if it has the file (`library`: its export.pdb's).
    pub(crate) fn ext_of(device: &Device, library: &Library) -> Result<Option<DbTags>> {
        let path = device.path(paths::EXPORT_EXT_PDB);
        match crate::fsutil::read(&path) {
            Ok(bytes) => Ok(Some(DbTags::new(ext_tree(&parse_ext(bytes, &path)?, library), library))),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(Error::Io { path, source: e }),
        }
    }

    /// OneLibrary's (`library`: its tracks).
    pub(crate) fn plus(one: &OneLibrary, library: &Library) -> Result<DbTags> {
        Ok(DbTags::new(plus_tree(one, library)?, library))
    }

    /// rekordbox's collection's (see [`crate::masterdb`]), whose track ids are `library`'s. Ids
    /// are numbered in order.
    pub(crate) fn collection(categories: &[crate::masterdb::TagCategory], library: &Library) -> DbTags {
        let keys = keys_by_id(library);
        let mut tree = Tree::default();
        let mut next: i64 = 0;
        let mut id = || {
            next += 1;
            next
        };
        for c in categories {
            let category = id();
            let tags = c
                .tags
                .iter()
                .map(|t| TagNode {
                    id: id(),
                    name: t.name.clone(),
                    tracks: t.track_ids.iter().filter_map(|i| keys.get(i).cloned()).collect(),
                    strays: 0,
                })
                .collect();
            tree.categories.push(CategoryNode { id: category, name: c.name.clone(), tags });
        }
        tree.ids =
            tree.categories.iter().flat_map(|c| std::iter::once(c.id).chain(c.tags.iter().map(|t| t.id))).collect();
        DbTags::new(tree, library)
    }

    /// A track's tags as (category, tag), in display order; `None` if the database doesn't list
    /// the track.
    pub(crate) fn of(&self, key: &str) -> Option<TagNames> {
        if !self.tracks.contains(key) {
            return None;
        }
        let tags =
            self.tree.categories.iter().flat_map(|c| {
                c.tags.iter().filter(|t| t.tracks.contains(key)).map(|t| (c.name.clone(), t.name.clone()))
            });
        Some(tags.collect())
    }

    /// How many tracks have a tag.
    pub(crate) fn tagged(&self) -> usize {
        self.tree.categories.iter().flat_map(|c| &c.tags).flat_map(|t| &t.tracks).collect::<FxHashSet<_>>().len()
    }
}

/// A track's tags (by file key) from `first`, or from `second` if `first` doesn't list it.
pub(crate) fn tags_of(first: Option<&DbTags>, second: Option<&DbTags>, key: &str) -> TagNames {
    first.and_then(|d| d.of(key)).or_else(|| second.and_then(|d| d.of(key))).unwrap_or_default()
}

/// A track's My Tags, as (category, tag) names.
pub(crate) type TagNames = Vec<(String, String)>;

/// Tracks to give My Tags in one database: the track's id there, its file key, and its tags.
pub(crate) type Wanted = Vec<(u32, String, TagNames)>;

/// What giving tracks their tags in one database does.
#[derive(Debug, Clone, Default)]
pub(crate) struct Carried {
    /// Keys of the tracks that gain tags.
    pub(crate) tracks: BTreeSet<String>,
    /// Categories and tags created (`Category`, `Category/Tag`).
    pub(crate) created: Vec<String>,
    /// Tags left out, beyond rekordbox's limits (`Category/Tag`).
    pub(crate) skipped: BTreeSet<String>,
}

impl Carried {
    pub(crate) fn warning(&self, file: &str) -> Option<String> {
        let first = self.skipped.iter().next()?;
        Some(format!(
            "{} My Tag(s) weren't added to {file} (e.g. {first}): rekordbox allows {MAX_CATEGORIES} categories of up \
             to {MAX_TAGS} tags",
            self.skipped.len()
        ))
    }
}

/// The id of `category`/`tag` in `tree`, adding the steps that create them where missing
/// (spelled as `other`, the drive's other database, spells them, and with its ids where this
/// one doesn't use them). `None` beyond rekordbox's limits.
#[allow(clippy::too_many_arguments)]
fn ensure_tag(
    source: Source,
    tree: &mut Tree,
    other: Option<&Tree>,
    ids: &mut Ids,
    category: &str,
    tag: &str,
    ops: &mut Vec<Op>,
    created: &mut Vec<String>,
) -> Option<i64> {
    let mut step = |tree: &mut Tree, op: Op| {
        tree.apply(&op);
        ops.push(op);
    };
    let category_id = match tree.category(category) {
        Some(c) => c.id,
        None if tree.categories.len() >= MAX_CATEGORIES => return None,
        None => {
            let name = other.and_then(|o| o.category(category)).map_or(category, |c| c.name.as_str()).to_string();
            let id = ids.category(source, tree, other, &name);
            ids.take(id, true);
            created.push(name.clone());
            step(tree, Op::AddCategory { id, name });
            id
        }
    };
    let c = tree.categories.iter().find(|c| c.id == category_id)?;
    if let Some(t) = find_named(&c.tags, |t| &t.name, tag) {
        return Some(t.id);
    }
    if c.tags.len() >= MAX_TAGS {
        return None;
    }
    let category_name = c.name.clone();
    let name = other.and_then(|o| o.tag(category, tag)).map_or(tag, |(_, t)| t.name.as_str()).to_string();
    let id = ids.tag(source, tree, other, &category_name, &name);
    ids.take(id, false);
    created.push(format!("{category_name}/{name}"));
    step(tree, Op::AddTag { category: category_id, id, name });
    Some(id)
}

/// The steps that give tracks their tags in a database whose My Tags are `tree`, and the tree
/// they lead to.
fn carry_ops(source: Source, tree: &Tree, other: Option<&Tree>, wanted: &Wanted) -> (Vec<Op>, Tree, Carried) {
    let mut ids = Ids::of(std::iter::once(tree).chain(other));
    let mut after = tree.clone();
    let mut ops = Vec::new();
    let mut carried = Carried::default();
    let mut links: Vec<(i64, Vec<(u32, String)>)> = Vec::new();
    for (id, key, tags) in wanted {
        for (category, tag) in tags {
            let Some(tag_id) =
                ensure_tag(source, &mut after, other, &mut ids, category, tag, &mut ops, &mut carried.created)
            else {
                carried.skipped.insert(format!("{category}/{tag}"));
                continue;
            };
            if after.tag_by_id(tag_id).is_some_and(|t| t.tracks.contains(key)) {
                continue;
            }
            let i = match links.iter().position(|(t, _)| *t == tag_id) {
                Some(i) => i,
                None => {
                    links.push((tag_id, Vec::new()));
                    links.len() - 1
                }
            };
            if !links[i].1.iter().any(|(x, _)| x == id) {
                links[i].1.push((*id, key.clone()));
                carried.tracks.insert(key.clone());
            }
        }
    }
    for (tag, tracks) in links {
        let op = Op::Link { tag, tracks };
        after.apply(&op);
        ops.push(op);
    }
    (ops, after, carried)
}

/// What [`carry_into_ext`] / [`carry_into_plus`] would do to a database whose My Tags are
/// `tags` (track ids in `wanted` don't matter).
pub(crate) fn carry_preview(tags: &DbTags, other: Option<&DbTags>, wanted: &Wanted) -> Carried {
    let source = Source::ExportExt; // only decides which ids fit, which a preview doesn't show
    carry_ops(source, &tags.tree, other.map(|o| &o.tree), wanted).2
}

/// Give tracks their tags in `exportExt.pdb` (`bytes`), whose track ids are now `library`'s.
/// Links naming `fresh` ids (tracks just added) go first: tracks removed earlier left them, and
/// their ids were reused. `other`: the drive's OneLibrary tags, for names and ids. The result is
/// read back before it's returned (`None`: nothing changed).
pub(crate) fn carry_into_ext(
    bytes: Vec<u8>,
    library: &Library,
    fresh: &[u32],
    wanted: &Wanted,
    other: Option<&DbTags>,
) -> Result<(Option<Vec<u8>>, Carried)> {
    let path = Path::new(paths::EXPORT_EXT_PDB);
    guard_ext(|| {
        let mut editor = ext_editor(bytes.clone())?;
        let stale = editor.unlink_tracks(fresh)?;
        let base = if stale > 0 { editor.to_bytes()? } else { bytes };
        let tree = ext_tree(&parse_ext(base, path)?, library);
        let (ops, after, carried) = carry_ops(Source::ExportExt, &tree, other.map(|o| &o.tree), wanted);
        if ops.is_empty() && stale == 0 {
            return Ok((None, carried));
        }
        ext_ops(&mut editor, &ops)?;
        let out = editor.to_bytes()?;
        if ext_tree(&parse_ext(out.clone(), path)?, library) != after {
            return Err(Error::ExportExt("failed verification after adding My Tags; drive left unchanged".into()));
        }
        Ok((Some(out), carried))
    })
}

/// A real rekordbox `exportExt.pdb` to start new ones from (see [`new_ext`]). It's rekordbox-pdb's
/// `tests/data/one-song-exportExt.pdb` (MIT, see `vendor/rekordbox-pdb/LICENSE`), kept in this
/// package as `convert`'s `export.pdb` template is.
const EXT_TEMPLATE: &[u8] = include_bytes!("tags/template_ext.pdb");

/// A new `exportExt.pdb` for `library` (a new `export.pdb`'s tracks) with `tags`' categories and
/// tags, in their order and with their ids (as [`copy_categories`] gives OneLibrary), and their
/// tracks' links: [`EXT_TEMPLATE`] emptied, then filled through [`carry_into_ext`]. What goes past
/// rekordbox's limits is left out, which the [`Carried`] says.
pub(crate) fn new_ext(tags: &DbTags, library: &Library) -> Result<(Vec<u8>, Carried)> {
    let empty = guard_ext(|| {
        let mut editor = ext_editor(EXT_TEMPLATE.to_vec())?;
        let template: Vec<u32> = editor.categories().iter().map(|(id, _)| *id).collect();
        for id in template {
            editor.remove(id)?;
        }
        for c in tags.tree.categories.iter().take(MAX_CATEGORIES) {
            editor.add_category(ext_id(c.id)?, &c.name)?;
            for t in c.tags.iter().take(MAX_TAGS) {
                editor.add_tag(ext_id(c.id)?, ext_id(t.id)?, &t.name)?;
            }
        }
        Ok(editor.to_bytes()?)
    })?;
    let wanted: Wanted = library
        .tracks
        .iter()
        .filter_map(|t| {
            let key = paths::file_key(&t.file_path);
            Some((t.id, key.clone(), tags.of(&key)?))
        })
        .collect();
    let (out, carried) = carry_into_ext(empty.clone(), library, &[], &wanted, Some(tags))?;
    Ok((out.unwrap_or(empty), carried))
}

/// [`carry_into_ext`] for OneLibrary's working copy (`library`: its tracks now).
pub(crate) fn carry_into_plus(
    one: &mut OneLibrary,
    library: &Library,
    fresh: &[u32],
    wanted: &Wanted,
    other: Option<&DbTags>,
) -> Result<Carried> {
    one.unlink_my_tags_of(fresh)?;
    let tree = plus_tree(one, library)?;
    let (ops, after, carried) = carry_ops(Source::OneLibrary, &tree, other.map(|o| &o.tree), wanted);
    edit_plus(one, &ops)?;
    if plus_tree(one, library)? != after {
        return Err(Error::OneLibrary("My Tags failed verification after adding them; drive left unchanged".into()));
    }
    Ok(carried)
}

/// Give a OneLibrary database that has no My Tags yet `from`'s categories and tags: the same
/// names, order and ids.
pub(crate) fn copy_categories(one: &mut OneLibrary, from: &DbTags) -> Result<()> {
    for c in &from.tree.categories {
        one.add_my_tag(c.id, 0, &c.name, true)?;
        for t in &c.tags {
            one.add_my_tag(t.id, c.id, &t.name, false)?;
        }
    }
    Ok(())
}

/// `exportExt.pdb` for an export.pdb rebuilt with new track ids: each link moves to the track
/// with the same file key in `new`. Links to tracks `old` doesn't have (or `new` doesn't list), or
/// to a tag that isn't there, go. Returns the new file (`None`: it has no links) and how many
/// links went.
pub(crate) fn remap_ext(bytes: Vec<u8>, old: Option<&Library>, new: &Library) -> Result<(Option<Vec<u8>>, usize)> {
    let path = Path::new(paths::EXPORT_EXT_PDB);
    guard_ext(|| {
        let db = parse_ext(bytes.clone(), path)?;
        if db.tag_tracks.is_empty() {
            return Ok((None, 0));
        }
        let old_keys = old.map(keys_by_id).unwrap_or_default();
        let mut new_ids: FxHashMap<String, Vec<u32>> = FxHashMap::default();
        for t in &new.tracks {
            new_ids.entry(paths::file_key(&t.file_path)).or_default().push(t.id);
        }
        let mut editor = ext_editor(bytes)?;
        let mut tracks: Vec<u32> = db.tag_tracks.iter().map(|l| l.track_id).collect();
        tracks.sort_unstable();
        tracks.dedup();
        editor.unlink_tracks(&tracks)?;
        let mut dropped = 0;
        let mut expected: BTreeSet<(u32, String)> = BTreeSet::new();
        for l in &db.tag_tracks {
            let Some(key) = old_keys.get(&l.track_id) else {
                dropped += 1;
                continue;
            };
            let Some(ids) = new_ids.get(key) else {
                dropped += 1;
                continue;
            };
            for id in ids {
                match editor.link(l.tag_id, *id) {
                    Ok(_) => {
                        expected.insert((l.tag_id, key.clone()));
                    }
                    Err(rekordbox_pdb::PdbError::TagNotFound(_)) => {
                        dropped += 1;
                        break;
                    }
                    Err(e) => return Err(e.into()),
                }
            }
        }
        let out = editor.to_bytes()?;
        let check = parse_ext(out.clone(), path)?;
        let new_keys = keys_by_id(new);
        let got: BTreeSet<(u32, String)> =
            check.tag_tracks.iter().filter_map(|l| Some((l.tag_id, new_keys.get(&l.track_id)?.clone()))).collect();
        if got != expected || check.tag_tracks.iter().any(|l| !new_keys.contains_key(&l.track_id)) {
            return Err(Error::ExportExt("failed verification after moving My Tags; drive left unchanged".into()));
        }
        Ok((Some(out), dropped))
    })
}

#[cfg(test)]
mod tests;
