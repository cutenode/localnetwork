//! Property tests for My Tags (`exportExt.pdb`): `ExtEditor`'s edits as a model-based state
//! machine, every saved file checked with the reader (`ExtDatabase`) and a fresh editor, and
//! `tags` edits replayed from the drive's journal after a new export.

use crate::common;

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use hegel::generators as gs;
use hegel::{HealthCheck, TestCase};
use locallibrary::device::Device;
use locallibrary::edit::{self, EditOptions};
use locallibrary::onelibrary::OneLibrary;
use locallibrary::progress::Ctx;
use locallibrary::{paths, tags};
use rekordbox_pdb::{ExtDatabase, ExtEditor, PdbError};

/// rekordbox-pdb's one-track export and rekordcrate's demo export (rekordbox's four default
/// categories, no tags on tracks).
fn fixtures() -> [PathBuf; 2] {
    [
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../vendor/rekordbox-pdb/tests/data/one-song-exportExt.pdb"),
        common::fixtures().join("rekordcrate/demo_tracks/PIONEER/rekordbox/exportExt.pdb"),
    ]
}

/// A category: id, name and its tags' (id, name).
type Group = (u32, String, Vec<(u32, String)>);

/// Categories (with their tags) in display order, and (tag, track) links.
#[derive(Debug, Clone, PartialEq)]
struct Model {
    groups: Vec<Group>,
    links: BTreeSet<(u32, u32)>,
}

impl Model {
    fn read(db: &ExtDatabase) -> Model {
        let groups = db
            .categories()
            .iter()
            .map(|c| {
                let tags = db.tags_in(c.id).iter().map(|t| (t.id, t.name.clone())).collect();
                (c.id, c.name.clone(), tags)
            })
            .collect();
        Model { groups, links: db.tag_tracks.iter().map(|l| (l.tag_id, l.track_id)).collect() }
    }

    fn of_editor(ed: &ExtEditor, links: &BTreeSet<(u32, u32)>) -> Model {
        let groups = ed
            .categories()
            .iter()
            .map(|&(id, name)| (id, name.to_string(), ed.tags(id).iter().map(|&(t, n)| (t, n.to_string())).collect()))
            .collect();
        Model { groups, links: links.iter().copied().filter(|&(tag, track)| ed.is_linked(tag, track)).collect() }
    }

    fn ids(&self) -> Vec<u32> {
        self.groups.iter().flat_map(|(c, _, tags)| std::iter::once(*c).chain(tags.iter().map(|t| t.0))).collect()
    }

    fn tag_ids(&self) -> Vec<u32> {
        self.groups.iter().flat_map(|(_, _, tags)| tags.iter().map(|t| t.0)).collect()
    }
}

/// A name: short ASCII, any text, ASCII around the short/long string (126) and one-byte offset
/// (255) limits, or a long one (most of a page, or more than fits on one).
#[hegel::test_helper]
fn name(tc: &TestCase) -> String {
    match tc.draw(gs::integers::<u8>().max_value(4)) {
        0 => tc.draw(gs::text().alphabet("abcdefghijklmnopqrstuvwxyz ABCXYZ0123456789&-").max_size(16)),
        1 => tc.draw(gs::text().max_size(16)),
        2 => "n".repeat(tc.draw(gs::integers::<usize>().min_value(100).max_value(300))),
        3 => "é".repeat(tc.draw(gs::integers::<usize>().min_value(50).max_value(150))),
        _ => {
            let c = tc.draw(gs::sampled_from(vec!['x', 'ü', '音']));
            c.to_string().repeat(tc.draw(gs::integers::<usize>().min_value(1000).max_value(20_000)))
        }
    }
}

/// An id: mostly a new one, sometimes one in use or 0.
#[hegel::test_helper]
fn id(tc: &TestCase, model: &Model) -> u32 {
    let ids = model.ids();
    let reuse = !ids.is_empty() && tc.draw(gs::weighted_booleans(0.15));
    if reuse {
        tc.draw(gs::sampled_from(ids))
    } else {
        tc.draw(hegel::one_of!(gs::integers::<u32>().max_value(40), gs::integers::<u32>()))
    }
}

/// An id that is (usually) a tag's.
#[hegel::test_helper]
fn some_tag(tc: &TestCase, model: &Model) -> u32 {
    let tags = model.tag_ids();
    let known = !tags.is_empty() && tc.draw(gs::weighted_booleans(0.9));
    if known { tc.draw(gs::sampled_from(tags)) } else { id(tc, model) }
}

/// Whether DeviceSQL can't encode `name` at all (the length field's limit).
fn unencodable(name: &str) -> bool {
    if name.is_ascii() { name.len() > 32_763 } else { name.encode_utf16().count() * 2 + 4 > 0x7fff }
}

/// Whether a name is refused as it should be: only if it can't be encoded, or its row (about 50
/// bytes plus the encoded name) can't fit on a 4096-byte page.
fn rightly_refused(name: &str, e: &PdbError) -> bool {
    let encoded = if name.is_ascii() { name.len() } else { name.encode_utf16().count() * 2 };
    match e {
        PdbError::StringTooLong => unencodable(name),
        PdbError::RowTooLarge { alloc, max } => alloc > max && encoded > 3900,
        _ => false,
    }
}

struct MyTags {
    editor: ExtEditor,
    model: Model,
    /// Every link ever made, to ask a reopened editor about.
    seen_links: BTreeSet<(u32, u32)>,
}

impl MyTags {
    /// Save and check the file: the reader and a fresh editor both see exactly the model, with
    /// positions numbered from 0 and no duplicate rows.
    fn check(&mut self) {
        let bytes = self.editor.to_bytes().unwrap_or_else(|e| panic!("saving: {e}"));
        let db = ExtDatabase::from_bytes(bytes.clone()).unwrap();
        assert_eq!(Model::read(&db), self.model);
        assert_eq!(db.tags.len(), self.model.ids().len(), "stray tag rows");
        assert_eq!(db.tag_tracks.len(), self.model.links.len(), "duplicate link rows");
        let categories = db.categories();
        assert!(categories.iter().enumerate().all(|(i, c)| c.position == i as u32 && c.category_id == 0));
        for c in categories {
            let tags = db.tags_in(c.id);
            assert!(tags.iter().enumerate().all(|(i, t)| t.position == i as u32 && !t.is_category));
        }
        let reopened = ExtEditor::from_bytes(bytes).unwrap();
        assert_eq!(Model::of_editor(&reopened, &self.seen_links), self.model);
    }
}

#[hegel::state_machine]
impl MyTags {
    #[rule]
    fn add_category(&mut self, tc: TestCase) {
        let (id, name) = (id(&tc, &self.model), name(&tc));
        match self.editor.add_category(id, &name) {
            Ok(()) => self.model.groups.push((id, name, Vec::new())),
            Err(PdbError::IdInUse(i)) => assert!(i == id && (id == 0 || self.model.ids().contains(&id))),
            Err(e) => assert!(rightly_refused(&name, &e), "add_category({id}): {e}"),
        }
    }

    #[rule]
    fn add_tag(&mut self, tc: TestCase) {
        let categories: Vec<u32> = self.model.groups.iter().map(|g| g.0).collect();
        let known = !categories.is_empty() && tc.draw(gs::weighted_booleans(0.9));
        let category = if known { tc.draw(gs::sampled_from(categories)) } else { id(&tc, &self.model) };
        let (id, name) = (id(&tc, &self.model), name(&tc));
        let result = self.editor.add_tag(category, id, &name);
        match (result, self.model.groups.iter_mut().find(|g| g.0 == category)) {
            (Ok(()), Some(g)) => g.2.push((id, name)),
            (Err(PdbError::IdInUse(i)), _) => assert!(i == id && (id == 0 || self.model.ids().contains(&id))),
            (Err(PdbError::TagNotFound(c)), None) => assert_eq!(c, category),
            (Err(e), _) if rightly_refused(&name, &e) => {}
            (r, g) => panic!("add_tag({category}, {id}): {r:?} with category {g:?}"),
        }
    }

    #[rule]
    fn rename(&mut self, tc: TestCase) {
        let (id, name) = (id(&tc, &self.model), name(&tc));
        let entry = self.model.groups.iter_mut().find_map(|g| {
            if g.0 == id { Some(&mut g.1) } else { g.2.iter_mut().find(|t| t.0 == id).map(|t| &mut t.1) }
        });
        match (self.editor.rename(id, &name), entry) {
            (Ok(()), Some(e)) => *e = name,
            (Err(PdbError::TagNotFound(i)), None) => assert_eq!(i, id),
            (Err(e), _) if rightly_refused(&name, &e) => {}
            (r, e) => panic!("rename({id}): {r:?} with entry {e:?}"),
        }
    }

    #[rule]
    fn remove(&mut self, tc: TestCase) {
        let id = some_tag(&tc, &self.model);
        let category = tc.draw(gs::weighted_booleans(0.2));
        let id = if category && !self.model.groups.is_empty() {
            let i = tc.draw(gs::integers::<usize>().max_value(self.model.groups.len() - 1));
            self.model.groups[i].0
        } else {
            id
        };
        let doomed: Vec<u32> = match self.model.groups.iter().position(|g| g.0 == id) {
            Some(i) => self.model.groups.remove(i).2.iter().map(|t| t.0).collect(),
            None => {
                let g = self.model.groups.iter_mut().find(|g| g.2.iter().any(|t| t.0 == id));
                match g {
                    Some(g) => {
                        g.2.retain(|t| t.0 != id);
                        vec![id]
                    }
                    None => {
                        assert!(matches!(self.editor.remove(id), Err(PdbError::TagNotFound(i)) if i == id));
                        return;
                    }
                }
            }
        };
        self.editor.remove(id).unwrap();
        self.model.links.retain(|(tag, _)| !doomed.contains(tag));
    }

    #[rule]
    fn link(&mut self, tc: TestCase) {
        let tag = some_tag(&tc, &self.model);
        let track = tc.draw(gs::integers::<u32>().min_value(1).max_value(6));
        let result = self.editor.link(tag, track);
        if self.model.tag_ids().contains(&tag) {
            assert_eq!(result.unwrap(), self.model.links.insert((tag, track)));
            self.seen_links.insert((tag, track));
        } else {
            assert!(matches!(result, Err(PdbError::TagNotFound(t)) if t == tag));
        }
    }

    /// Tag a batch of tracks at once (enough to fill pages of links).
    #[rule]
    fn link_many(&mut self, tc: TestCase) {
        let tags = self.model.tag_ids();
        tc.assume(!tags.is_empty());
        let tag = tc.draw(gs::sampled_from(tags));
        let first = tc.draw(gs::integers::<u32>().min_value(1).max_value(1000));
        let count = tc.draw(gs::integers::<u32>().max_value(400));
        for track in first..first + count {
            assert_eq!(self.editor.link(tag, track).unwrap(), self.model.links.insert((tag, track)));
            self.seen_links.insert((tag, track));
        }
    }

    /// Untag a range of tracks (as when they leave `export.pdb`).
    #[rule]
    fn unlink_many(&mut self, tc: TestCase) {
        let first = tc.draw(gs::integers::<u32>().min_value(1).max_value(1000));
        let tracks: Vec<u32> = (first..first + tc.draw(gs::integers::<u32>().max_value(300))).collect();
        let before = self.model.links.len();
        self.model.links.retain(|(_, track)| !tracks.contains(track));
        assert_eq!(self.editor.unlink_tracks(&tracks).unwrap(), before - self.model.links.len());
    }

    #[rule]
    fn unlink(&mut self, tc: TestCase) {
        let tag = some_tag(&tc, &self.model);
        let track = tc.draw(gs::integers::<u32>().min_value(1).max_value(6));
        assert_eq!(self.editor.unlink(tag, track).unwrap(), self.model.links.remove(&(tag, track)));
    }

    #[rule]
    fn unlink_tracks(&mut self, tc: TestCase) {
        let tracks = tc.draw(gs::vecs(gs::integers::<u32>().min_value(1).max_value(6)).max_size(3));
        let before = self.model.links.len();
        self.model.links.retain(|(_, track)| !tracks.contains(track));
        assert_eq!(self.editor.unlink_tracks(&tracks).unwrap(), before - self.model.links.len());
    }

    #[rule]
    fn save(&mut self, _: TestCase) {
        self.check();
    }

    #[invariant]
    fn saved_file_matches(&mut self, _: TestCase) {
        self.check();
    }
}

/// Any sequence of edits, saved at any points, gives files the reader and a fresh editor see as
/// the model has it. (A name `add_category`, `add_tag` or `rename` accepts must also save.)
#[hegel::test(test_cases = 300)]
fn edits_match_the_model(tc: TestCase) {
    let bytes = std::fs::read(tc.draw(gs::sampled_from(&fixtures()))).unwrap();
    let model = Model::read(&ExtDatabase::from_bytes(bytes.clone()).unwrap());
    let editor = ExtEditor::from_bytes(bytes).unwrap();
    let seen_links = model.links.clone();
    hegel::stateful::machine(MyTags { editor, model, seen_links }).steps(40).run(tc);
}

/// Shrunk counterexample of `edits_match_the_model`: a 4000-character category name was
/// accepted, then saving failed after the old tag rows had been deleted. It is now refused up
/// front, and the file still saves.
#[test]
fn a_name_too_long_for_a_page_is_refused_up_front() {
    let original = std::fs::read(&fixtures()[0]).unwrap();
    let mut ed = ExtEditor::from_bytes(original.clone()).unwrap();
    let long = "x".repeat(4000);
    assert!(matches!(ed.add_category(5, &long), Err(PdbError::RowTooLarge { .. })));
    assert!(matches!(ed.add_tag(1, 5, &long), Err(PdbError::RowTooLarge { .. })));
    assert!(matches!(ed.rename(1, &long), Err(PdbError::RowTooLarge { .. })));
    ed.add_category(5, &"x".repeat(3900)).unwrap();
    let saved = ed.to_bytes().unwrap();
    let db = ExtDatabase::from_bytes(saved).unwrap();
    assert_eq!(db.categories().len(), 5);
}

/// Opening and saving without an edit, or after edits that change nothing (renaming to the same
/// name, refused additions and removals, unlinking what isn't linked), gives back the file as it
/// was, byte for byte.
#[hegel::test]
fn edits_that_change_nothing_leave_the_file_as_it_was(tc: TestCase) {
    let original = std::fs::read(tc.draw(gs::sampled_from(&fixtures()))).unwrap();
    let model = Model::read(&ExtDatabase::from_bytes(original.clone()).unwrap());
    let mut ed = ExtEditor::from_bytes(original.clone()).unwrap();
    let ids = model.ids();
    let steps = tc.draw(gs::integers::<usize>().max_value(6));
    for _ in 0..steps {
        let id = tc.draw(gs::sampled_from(ids.clone()));
        let track = tc.draw(gs::integers::<u32>().min_value(1).max_value(6));
        match tc.draw(gs::integers::<u8>().max_value(5)) {
            0 => {
                let name = model.groups.iter().find(|g| g.0 == id).map(|g| g.1.clone());
                let name =
                    name.or_else(|| model.groups.iter().flat_map(|g| &g.2).find(|t| t.0 == id).map(|t| t.1.clone()));
                ed.rename(id, &name.unwrap()).unwrap();
            }
            1 => assert!(ed.add_category(id, "dup").is_err()),
            2 => assert!(ed.add_tag(id, 0, "zero").is_err()),
            3 => assert!(!ed.unlink(id, track).unwrap()),
            4 => assert_eq!(ed.unlink_tracks(&[track]).unwrap(), 0),
            _ => assert!(ed.remove(u32::MAX - id).is_err()),
        }
    }
    assert_eq!(ed.to_bytes().unwrap(), original);
}

// ---------------------------------------------------------------------------------------------
// Replaying `tags` edits
// ---------------------------------------------------------------------------------------------

/// rekordcrate's demo export with a third track, and OneLibrary holding the same library and My
/// Tags (rekordbox's four default categories), as rekordbox exports both.
fn tagged_drive(dir: &Path) -> PathBuf {
    let root = common::demo_drive(dir, "Main");
    common::export_new_track(&root, "Noise Maker", "NOISE");
    common::add_onelibrary(&root);
    let ext = ExtDatabase::from_file(root.join(paths::EXPORT_EXT_PDB)).unwrap();
    let mut one = OneLibrary::open_root(&root).unwrap();
    for c in ext.categories() {
        one.add_my_tag(c.id.into(), 0, &c.name, true).unwrap();
        for t in ext.tags_in(c.id) {
            one.add_my_tag(t.id.into(), c.id.into(), &t.name, false).unwrap();
        }
    }
    fs::write(root.join(paths::LIBRARY_PLUS_DB), one.to_encrypted().unwrap()).unwrap();
    root
}

/// The databases a new export replaces.
fn exported(root: &Path) -> Vec<(&'static str, Vec<u8>)> {
    [paths::EXPORT_PDB, paths::EXPORT_EXT_PDB, paths::LIBRARY_PLUS_DB]
        .into_iter()
        .map(|k| (k, fs::read(root.join(k)).unwrap()))
        .collect()
}

/// A drive's My Tags, read from both databases: each category with the databases holding it,
/// and its tags with theirs and the files of the tracks carrying them; and where the two differ.
fn tag_state(root: &Path) -> String {
    let d = Device::open_export(root).unwrap();
    let mine = tags::read(&d).unwrap();
    let mut out = String::new();
    for c in &mine.categories {
        out += &format!("{} {:?}\n", c.name, c.sources);
        for t in &c.tags {
            let mut files: Vec<&str> = t.tracks.iter().map(|r| r.file_path.as_str()).collect();
            files.sort_unstable();
            out += &format!("  {} {:?} {files:?}\n", t.name, t.sources);
        }
    }
    out + &format!("{:?}", mine.differences)
}

/// Tag names to try: case-only differences of each other and of rekordbox's defaults.
const TAG_PATHS: &[&str] = &[
    "Genre/Deep",
    "genre/deep",
    "Genre/Techno",
    "Situation/Peak Time",
    "Components/Vocal",
    "Untitled Column/Tokyo",
    "Mood/Dark",
];

/// One `tags` edit on the drive's tracks.
#[hegel::test_helper]
fn tag_edit(tc: &TestCase, ids: &[u32]) -> tags::Edit {
    let tag = tc.draw(gs::sampled_from(TAG_PATHS)).to_string();
    let tracks: Vec<u32> = tc.draw(gs::subsequences(ids).min_size(1));
    match tc.draw(gs::integers::<u8>().max_value(5)) {
        0..=2 => tags::Edit::Add { tag, tracks },
        3 => tags::Edit::Remove { tag, tracks },
        4 => {
            let category = tc.draw(gs::booleans());
            let tag = if category { tag.split('/').next().unwrap().to_string() } else { tag };
            let to = tc.draw(gs::sampled_from(&["Deep House", "deep", "Mood", "Genre", "Peak"][..])).to_string();
            tags::Edit::Rename { tag, to }
        }
        _ => tags::Edit::Delete { tag },
    }
}

/// After rekordbox exports over `tags` edits (all three databases as they were), replaying the
/// drive's journal gives back the My Tags the edits made, in both databases.
#[hegel::test(test_cases = 25, suppress_health_check = [HealthCheck::TooSlow])]
fn replaying_tag_edits_after_a_new_export_redoes_them(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let root = tagged_drive(dir.path());
    let before = exported(&root);
    let ids: Vec<u32> = Device::open_export(&root).unwrap().library().unwrap().tracks.iter().map(|t| t.id).collect();
    for _ in 0..tc.draw(gs::integers::<usize>().min_value(1).max_value(6)) {
        let edit = tag_edit(&tc, &ids);
        let mut d = Device::open_export(&root).unwrap();
        match tags::plan(&d, &edit) {
            Ok(plan) => {
                tc.note(&format!("{edit:?}"));
                tags::apply(&plan, &mut d, &Ctx::silent()).unwrap();
            }
            Err(e) => tc.note(&format!("{edit:?} refused: {e}")),
        }
    }
    let edited = tag_state(&root);
    for (key, bytes) in &before {
        fs::write(root.join(key), bytes).unwrap();
    }
    let mut d = Device::open_export(&root).unwrap();
    let edits = edit::journal::edits(&d).unwrap();
    tc.note(&format!("journal: {edits:?}"));
    if edits.is_empty() {
        return;
    }
    let opts = EditOptions { lenient: true, no_journal: true };
    let plan = edit::plan(&d, &edits, &opts, &Ctx::silent()).unwrap();
    let report = edit::apply(&plan, &mut d, &Ctx::silent()).unwrap();
    assert!(report.warnings.iter().all(|w| !w.starts_with("skipped")), "{:?}", report.warnings);
    assert_eq!(tag_state(&root), edited, "My Tags after the replay");
}
