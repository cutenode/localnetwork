mod databases;
mod edits;

use crate::common;

use indexmap::IndexMap;
use std::fs;
use std::path::{Path, PathBuf};

use locallibrary::convert::{self, ConvertOptions, Target};
use locallibrary::device::{Device, LineageOp};
use locallibrary::merge::{self, MergeOptions};
use locallibrary::onelibrary::OneLibrary;
use locallibrary::progress::Ctx;
use locallibrary::tags::{self, Edit, Source, TagReport};
use locallibrary::{Error, backup, paths, pdb};
use rekordbox_pdb::{ExtDatabase, ExtEditor, ExtTableType};

/// Give a drive's OneLibrary the categories and tags its exportExt.pdb has (with the same ids),
/// as rekordbox exports them to both.
fn mirror_tags_into_onelibrary(root: &Path) {
    let ext = ExtDatabase::from_file(root.join(paths::EXPORT_EXT_PDB)).unwrap();
    let mut one = OneLibrary::open_root(root).unwrap();
    for c in ext.categories() {
        one.add_my_tag(c.id.into(), 0, &c.name, true).unwrap();
        for t in ext.tags_in(c.id) {
            one.add_my_tag(t.id.into(), c.id.into(), &t.name, false).unwrap();
        }
    }
    fs::write(root.join(paths::LIBRARY_PLUS_DB), one.to_encrypted().unwrap()).unwrap();
}

/// rekordcrate's demo export (rekordbox's default My Tags) with a third track, and OneLibrary as
/// well.
fn both_drive(dir: &Path) -> PathBuf {
    both_drive_named(dir, "Main")
}

fn both_drive_named(dir: &Path, name: &str) -> PathBuf {
    let root = common::demo_drive(dir, name);
    common::export_new_track(&root, "Noise Maker", "NOISE");
    common::add_onelibrary(&root);
    mirror_tags_into_onelibrary(&root);
    root
}

fn select(d: &Device, query: &str) -> Vec<u32> {
    let ids: Vec<u32> = d.library().unwrap().search(query).iter().map(|t| t.id).collect();
    assert!(!ids.is_empty(), "{query}");
    ids
}

fn apply(d: &mut Device, edit: Edit) -> TagReport {
    let plan = tags::plan(d, &edit).unwrap();
    tags::apply(&plan, d, &Ctx::silent()).unwrap()
}

fn add(tag: &str, tracks: Vec<u32>) -> Edit {
    Edit::Add { tag: tag.into(), tracks }
}

/// `Category/Tag` -> titles of the tracks carrying it, in exportExt.pdb.
fn ext_tags(root: &Path) -> IndexMap<String, Vec<String>> {
    let ext = ExtDatabase::from_file(root.join(paths::EXPORT_EXT_PDB)).unwrap();
    let lib = pdb::read_library(&root.join(paths::EXPORT_PDB)).unwrap();
    let mut out = IndexMap::new();
    for c in ext.categories() {
        for t in ext.tags_in(c.id) {
            let mut titles: Vec<String> = ext
                .tag_tracks
                .iter()
                .filter(|l| l.tag_id == t.id)
                .map(|l| lib.track(l.track_id).unwrap().title.clone())
                .collect();
            titles.sort();
            if !titles.is_empty() {
                out.insert(format!("{}/{}", c.name, t.name), titles);
            }
        }
    }
    out
}

/// The same, in exportLibrary.db.
fn plus_tags(root: &Path) -> IndexMap<String, Vec<String>> {
    let one = OneLibrary::open_root(root).unwrap();
    let lib = one.library().unwrap();
    let rows = one.my_tag_rows().unwrap();
    let name = |id: i64| rows.iter().find(|r| r.id == id).unwrap();
    let mut out: IndexMap<String, Vec<String>> = IndexMap::new();
    for (tag, track) in one.my_tag_links().unwrap() {
        let t = name(tag);
        let path = format!("{}/{}", name(t.parent).name, t.name);
        out.entry(path).or_default().push(lib.track(track as u32).unwrap().title.clone());
    }
    for titles in out.values_mut() {
        titles.sort();
    }
    out
}

/// A category's tag names, in display order, in each database.
fn order(root: &Path, category: &str) -> (Vec<String>, Vec<String>) {
    let ext = ExtDatabase::from_file(root.join(paths::EXPORT_EXT_PDB)).unwrap();
    let c = ext.categories().into_iter().find(|c| c.name == category).unwrap().id;
    let one = OneLibrary::open_root(root).unwrap();
    let rows = one.my_tag_rows().unwrap();
    let pc = rows.iter().find(|r| r.is_category && r.name == category).unwrap().id;
    (
        ext.tags_in(c).iter().map(|t| t.name.clone()).collect(),
        rows.iter().filter(|r| r.parent == pc).map(|r| r.name.clone()).collect(),
    )
}

fn map(entries: &[(&str, &[&str])]) -> IndexMap<String, Vec<String>> {
    entries.iter().map(|(k, v)| (k.to_string(), v.iter().map(|s| s.to_string()).collect())).collect()
}

#[test]
fn reads_rekordboxs_default_tags() {
    let dir = tempfile::tempdir().unwrap();
    let d = Device::open_export(common::demo_drive(dir.path(), "Demo")).unwrap();
    let t = tags::read(&d).unwrap();
    assert_eq!(t.sources, [Source::ExportExt]);
    let cats: Vec<&str> = t.categories.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(cats, ["Genre", "Components", "Situation", "Untitled Column"]);
    let genre: Vec<&str> = t.categories[0].tags.iter().map(|t| t.name.as_str()).collect();
    assert_eq!(genre, ["Acid House", "Deep House", "Techno", "Nu Disco", "Electro House", "Bass Music", "Trap"]);
    assert_eq!(t.categories[0].tags[2].path, "Genre/Techno");
    // rekordbox rewrote all 28 rows when it saved this file: only the present ones count.
    assert_eq!(t.categories.iter().map(|c| c.tags.len()).sum::<usize>(), 24);
    assert!(t.categories.iter().flat_map(|c| &c.tags).all(|t| t.tracks.is_empty()));
    assert!(t.differences.is_empty() && t.warnings.is_empty(), "{t:?}");
    assert!(tags::by_track(&d).unwrap().is_empty());
}

#[test]
fn tag_rows_are_rewritten_the_way_rekordbox_does() {
    // rekordcrate's `empty` and `demo_tracks` exports have the same My Tags, and rekordbox saved
    // demo_tracks' a second time: it deleted the 28 tag rows and appended them again. Rewriting
    // `empty`'s gives the same data page, byte for byte, but for its sequence number and a flag.
    let read =
        |name: &str| fs::read(common::fixtures().join(format!("rekordcrate/{name}/PIONEER/rekordbox/exportExt.pdb")));
    let (empty, rekordbox) = (read("empty").unwrap(), read("demo_tracks").unwrap());
    let mut ed = ExtEditor::from_bytes(empty.clone()).unwrap();
    ed.rename(1, "Changed").unwrap();
    ed.rename(1, "Genre").unwrap();
    let ours = ed.to_bytes().unwrap();
    assert_eq!(ours.len(), rekordbox.len());
    let page = |b: &[u8], n: usize| b[n * 4096..(n + 1) * 4096].to_vec();
    let (a, b) = (page(&ours, 8), page(&rekordbox, 8));
    assert_ne!(a, page(&empty, 8));
    assert_eq!(a[..0x10], b[..0x10]);
    assert_eq!(a[0x14..0x1b], b[0x14..0x1b], "row counts");
    assert_eq!(a[0x1c..], b[0x1c..], "sizes, rows and row index");
    // rekordbox also filled in the table's index page (page 7), which locallibrary leaves as
    // rekordbox wrote it in `empty`. Everything else is the same.
    for n in (1..ours.len() / 4096).filter(|n| ![7, 8].contains(n)) {
        assert_eq!(page(&ours, n), page(&rekordbox, n), "page {n}");
    }
}

#[test]
fn edits_both_databases_and_can_be_undone() {
    let dir = tempfile::tempdir().unwrap();
    let root = both_drive(dir.path());
    let ext_before = fs::read(root.join(paths::EXPORT_EXT_PDB)).unwrap();
    let mut d = Device::open_export(&root).unwrap();
    let t = tags::read(&d).unwrap();
    assert_eq!(t.sources, [Source::ExportExt, Source::OneLibrary]);
    assert!(t.differences.is_empty(), "{:?}", t.differences);

    // An existing tag, named loosely.
    let demos = select(&d, "Demo Track");
    let plan = tags::plan(&d, &add("genre/techno", demos.clone())).unwrap();
    assert_eq!(plan.tag, "Genre/Techno");
    assert!(plan.created.is_empty());
    assert_eq!((plan.tracks.len(), plan.unchanged), (2, 0));
    assert_eq!(plan.databases, [Source::ExportExt, Source::OneLibrary]);
    assert!(plan.warnings.is_empty(), "{:?}", plan.warnings);
    let report = tags::apply(&plan, &mut d, &Ctx::silent()).unwrap();
    assert_eq!(report.databases, [Source::ExportExt, Source::OneLibrary]);
    let first_backup = report.backup.clone().unwrap();
    let want = map(&[("Genre/Techno", &["Demo Track 1", "Demo Track 2"])]);
    assert_eq!(ext_tags(&root), want);
    assert_eq!(plus_tags(&root), want);
    common::sqlcipher_reads(&root);
    let d2 = Device::open_export(&root).unwrap();
    assert_eq!(d2.state().unwrap().head().unwrap().op, LineageOp::Edit);
    let again = tags::plan(&d, &add("Genre/Techno", demos.clone())).unwrap();
    assert!(again.is_noop());
    assert_eq!(again.unchanged, 2);

    // A new tag gets the same id in both databases, at the end of its category.
    let noise = select(&d, "NOISE");
    let plan = tags::plan(&d, &add("Genre/Minimal", noise.clone())).unwrap();
    let created: Vec<(&str, Source)> = plan.created.iter().map(|c| (c.path.as_str(), c.database)).collect();
    assert_eq!(created, [("Genre/Minimal", Source::ExportExt), ("Genre/Minimal", Source::OneLibrary)]);
    tags::apply(&plan, &mut d, &Ctx::silent()).unwrap();
    let (ext_order, plus_order) = order(&root, "Genre");
    assert_eq!(ext_order.last().unwrap(), "Minimal");
    assert_eq!(ext_order, plus_order);
    let ext = ExtDatabase::from_file(root.join(paths::EXPORT_EXT_PDB)).unwrap();
    let ext_id = ext.tags.iter().find(|t| t.name == "Minimal").unwrap().id;
    let rows = OneLibrary::open_root(&root).unwrap().my_tag_rows().unwrap();
    assert_eq!(rows.iter().find(|r| r.name == "Minimal").unwrap().id, i64::from(ext_id));

    apply(&mut d, add("Situation/Lounge", vec![demos[0]]));
    let by_track = tags::by_track(&d).unwrap();
    let key = |q: &str| paths::key(&d.library().unwrap().search(q)[0].file_path).to_lowercase();
    assert_eq!(by_track[&key("Demo Track 1")], ["Genre/Techno", "Situation/Lounge"]);
    assert_eq!(by_track[&key("Demo Track 2")], ["Genre/Techno"]);
    assert_eq!(by_track[&key("NOISE")], ["Genre/Minimal"]);

    // Remove, rename, delete.
    apply(&mut d, Edit::Remove { tag: "Genre/Techno".into(), tracks: vec![demos[1]] });
    apply(&mut d, Edit::Rename { tag: "Genre/Minimal".into(), to: "Minimal Techno".into() });
    apply(&mut d, Edit::Rename { tag: "Untitled Column".into(), to: "Mood".into() });
    let want = map(&[
        ("Genre/Minimal Techno", &["NOISE"]),
        ("Genre/Techno", &["Demo Track 1"]),
        ("Situation/Lounge", &["Demo Track 1"]),
    ]);
    assert_eq!(ext_tags(&root), want);
    assert_eq!(plus_tags(&root), want);
    let plan = tags::plan(&d, &Edit::Delete { tag: "Genre/Techno".into() }).unwrap();
    assert_eq!(plan.tracks.len(), 1);
    tags::apply(&plan, &mut d, &Ctx::silent()).unwrap();
    let t = tags::read(&d).unwrap();
    assert!(t.differences.is_empty(), "{:?}", t.differences);
    assert_eq!(t.categories[3].name, "Mood");
    let genre: Vec<&str> = t.categories[0].tags.iter().map(|t| t.name.as_str()).collect();
    assert_eq!(
        genre,
        ["Acid House", "Deep House", "Nu Disco", "Electro House", "Bass Music", "Trap", "Minimal Techno"]
    );
    let (ext_order, plus_order) = order(&root, "Genre");
    assert_eq!(ext_order, plus_order);
    assert_eq!(ext_tags(&root).len(), 2);
    assert_eq!(plus_tags(&root).len(), 2);

    // The first backup holds the databases as they were before any of this.
    backup::restore(&mut d, Some(&first_backup), false).unwrap();
    assert_eq!(fs::read(root.join(paths::EXPORT_EXT_PDB)).unwrap(), ext_before);
    assert!(plus_tags(&root).is_empty());
    assert!(tags::read(&d).unwrap().differences.is_empty());
}

#[test]
fn stale_plans_are_refused() {
    let dir = tempfile::tempdir().unwrap();
    let root = both_drive(dir.path());
    let mut d = Device::open_export(&root).unwrap();
    let demos = select(&d, "Demo Track");
    let stale = tags::plan(&d, &add("Genre/Techno", demos.clone())).unwrap();
    apply(&mut d, add("Genre/Techno", vec![demos[0]]));
    let err = tags::apply(&stale, &mut d, &Ctx::silent()).unwrap_err();
    assert!(matches!(err, Error::StalePlan(_)), "{err}");
    assert_eq!(ext_tags(&root), map(&[("Genre/Techno", &["Demo Track 1"])]));
}

#[test]
fn refuses_what_rekordbox_doesnt_allow() {
    let dir = tempfile::tempdir().unwrap();
    let root = both_drive(dir.path());
    let d = Device::open_export(&root).unwrap();
    let demos = select(&d, "Demo Track");
    let invalid = |e: Edit| match tags::plan(&d, &e) {
        Err(Error::Invalid(m)) => m,
        other => panic!("{e:?}: {other:?}"),
    };
    assert!(invalid(add("Energy/Peak", demos.clone())).contains("4 My Tag categories"));
    assert!(invalid(Edit::Delete { tag: "Genre".into() }).contains("not deleted"));
    assert!(invalid(add("Genre", demos.clone())).contains("Category/Tag"));
    assert!(invalid(Edit::Rename { tag: "Genre/Techno".into(), to: "deep house".into() }).contains("already has"));
    assert!(invalid(Edit::Rename { tag: "Genre".into(), to: "A/B".into() }).contains("'/'"));
    assert!(matches!(
        tags::plan(&d, &Edit::Remove { tag: "Genre/Nope".into(), tracks: demos.clone() }),
        Err(Error::NotFound(_))
    ));
    assert!(matches!(tags::plan(&d, &add("Genre/Techno", vec![9999])), Err(Error::NotFound(_))));

    // Fifty tags in a category is rekordbox's limit.
    let ext_path = root.join(paths::EXPORT_EXT_PDB);
    let mut ed = ExtEditor::from_file(&ext_path).unwrap();
    for i in ed.tags(1).len()..tags::MAX_TAGS {
        ed.add_tag(1, 0x7000_0000 + i as u32, &format!("Style {i}")).unwrap();
    }
    ed.save(&ext_path).unwrap();
    assert!(invalid(add("Genre/One Too Many", demos)).contains("50 tags"));
}

/// A new tag or category name can't hold a '/': a tag's path is split at its first one, so
/// `rename Genre/Deep House Genre/Dark` would make `Genre/Genre/Dark`. A tag rekordbox named with
/// one is still found and given to tracks.
#[hegel::test(test_cases = 10, suppress_health_check = [hegel::HealthCheck::TooSlow])]
fn new_names_with_a_slash_are_refused(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let dir = tempfile::tempdir().unwrap();
    let root = both_drive(dir.path());
    let mut ed = ExtEditor::from_file(root.join(paths::EXPORT_EXT_PDB)).unwrap();
    ed.add_tag(1, 0x7000_0000, "Drum/Bass").unwrap();
    ed.save(root.join(paths::EXPORT_EXT_PDB)).unwrap();
    let mut d = Device::open_export(&root).unwrap();
    let demos = select(&d, "Demo Track");
    let part = |tc: &hegel::TestCase| tc.draw(gs::sampled_from(vec!["", "Genre", "Dark", " "])).to_string();
    let named = format!("{}/{}", part(&tc), part(&tc));
    let edit = match tc.draw(gs::integers::<u8>().max_value(2)) {
        0 => Edit::Rename { tag: "Genre/Deep House".into(), to: named.clone() },
        1 => Edit::Rename { tag: "Genre".into(), to: named.clone() },
        _ => add(&format!("Genre/New/{named}"), demos.clone()),
    };
    let before = common::Before::take(&root);
    match tags::plan(&d, &edit) {
        Err(Error::Invalid(m)) => assert!(m.contains("can't contain '/'"), "{edit:?}: {m}"),
        other => panic!("{edit:?}: {other:?}"),
    }
    before.untouched(&root).unwrap();
    apply(&mut d, add("Genre/Drum/Bass", demos.clone()));
    assert_eq!(ext_tags(&root)["Genre/Drum/Bass"].len(), demos.len());
    assert_eq!(plus_tags(&root)["Genre/Drum/Bass"].len(), demos.len());
}

/// Regression: plan took a tag name too long for one exportExt.pdb row, and apply then failed
/// after making a backup, leaving the drive marked as interrupted. A tag's row holds 3,999 ASCII
/// characters, or 1,999 as UTF-16.
#[test]
fn a_tag_name_one_character_too_long_for_exportext_pdb_is_refused() {
    for (c, longest) in [("x", 3999), ("é", 1999)] {
        let dir = tempfile::tempdir().unwrap();
        let root = both_drive(dir.path());
        let mut d = Device::open_export(&root).unwrap();
        let demos = select(&d, "Demo Track");
        let Err(err) = tags::plan(&d, &add(&format!("Genre/{}", c.repeat(longest + 1)), demos.clone())) else {
            panic!("plan took a tag name of {c:?} one character too long");
        };
        assert!(matches!(err, Error::Invalid(_)) && err.to_string().contains("too long"), "{err}");
        apply(&mut d, add(&format!("Genre/{}", c.repeat(longest)), demos));
    }
}

/// Regression: apply made its backup and marked the drive as interrupted before editing, so an
/// edit that then failed left both behind, with an error naming export.pdb. Here exportExt.pdb's
/// table directory gives the link table the file header as its next free page: the file reads
/// as it did, but the first link can't be added.
#[test]
fn an_apply_failing_after_planning_leaves_the_drive_alone() {
    let dir = tempfile::tempdir().unwrap();
    let root = both_drive(dir.path());
    // The table directory: the number of tables at 8, then 16 bytes each from 0x1c, the type and
    // then the next free page.
    let path = root.join(paths::EXPORT_EXT_PDB);
    let mut bytes = fs::read(&path).unwrap();
    let tables = u32::from_le_bytes(bytes[8..12].try_into().unwrap()) as usize;
    let links = (0..tables).map(|i| 0x1c + i * 16).find(|&o| bytes[o..o + 4] == [4, 0, 0, 0]).unwrap();
    bytes[links + 4..links + 8].fill(0);
    fs::write(&path, bytes).unwrap();
    let before = common::Before::take(&root);
    let mut d = Device::open_export(&root).unwrap();
    let plan = tags::plan(&d, &add("Genre/Techno", select(&d, "Demo Track"))).unwrap();
    let err = tags::apply(&plan, &mut d, &Ctx::silent()).unwrap_err();
    before.untouched(&root).unwrap();
    assert!(err.code() == "pdb" && err.to_string().starts_with("exportExt.pdb: "), "{err}");
}

#[test]
fn reports_where_the_databases_disagree() {
    let dir = tempfile::tempdir().unwrap();
    let root = both_drive(dir.path());
    let mut d = Device::open_export(&root).unwrap();
    let demos = select(&d, "Demo Track");
    apply(&mut d, add("Genre/Techno", demos.clone()));
    // Changes made behind locallibrary's back, to one database each.
    let ext_path = root.join(paths::EXPORT_EXT_PDB);
    let mut ed = ExtEditor::from_file(&ext_path).unwrap();
    ed.add_tag(2, 0x7000_0001, "Strings").unwrap();
    ed.save(&ext_path).unwrap();
    let mut one = OneLibrary::open_root(&root).unwrap();
    let techno = one.my_tag_rows().unwrap().into_iter().find(|r| r.name == "Techno").unwrap().id;
    let noise = one.library().unwrap().search("NOISE")[0].id;
    one.link_my_tag(techno, &[noise]).unwrap();
    fs::write(root.join(paths::LIBRARY_PLUS_DB), one.to_encrypted().unwrap()).unwrap();

    let t = tags::read(&d).unwrap();
    assert_eq!(
        t.differences,
        ["Genre/Techno: 1 track(s) tagged only in exportLibrary.db", "Components/Strings is only in exportExt.pdb"],
    );
    let techno = t.categories[0].tags.iter().find(|t| t.name == "Techno").unwrap();
    assert_eq!(techno.tracks.len(), 3, "tracks from either database");
    let strings = t.categories[1].tags.iter().find(|t| t.name == "Strings").unwrap();
    assert_eq!(strings.sources, [Source::ExportExt]);

    // Tagging NOISE in exportExt.pdb too brings them back together.
    let plan = tags::plan(&d, &add("Genre/Techno", select(&d, "NOISE"))).unwrap();
    assert_eq!(plan.databases, [Source::ExportExt]);
    tags::apply(&plan, &mut d, &Ctx::silent()).unwrap();
    assert_eq!(tags::read(&d).unwrap().differences, ["Components/Strings is only in exportExt.pdb"]);
}

#[test]
fn onelibrary_only_drives() {
    let dir = tempfile::tempdir().unwrap();
    let root = common::demo_drive(dir.path(), "Plus");
    common::onelibrary_only(&root);
    let mut d = Device::open_export(&root).unwrap();
    assert!(tags::read(&d).unwrap().categories.is_empty());
    let demos = select(&d, "Demo Track");
    let plan = tags::plan(&d, &add("Genre/Deep House", demos)).unwrap();
    let created: Vec<&str> = plan.created.iter().map(|c| c.path.as_str()).collect();
    assert_eq!(created, ["Genre", "Genre/Deep House"]);
    assert_eq!(plan.databases, [Source::OneLibrary]);
    tags::apply(&plan, &mut d, &Ctx::silent()).unwrap();
    assert_eq!(plus_tags(&root), map(&[("Genre/Deep House", &["Demo Track 1", "Demo Track 2"])]));
    common::sqlcipher_reads(&root);
    let t = tags::read(&d).unwrap();
    assert_eq!(t.sources, [Source::OneLibrary]);
    assert_eq!(t.categories[0].tags[0].tracks.len(), 2);
    assert!(!root.join(paths::EXPORT_EXT_PDB).exists());
}

#[test]
fn drives_without_exportext_get_onelibrary_only() {
    let dir = tempfile::tempdir().unwrap();
    let root = common::bigger_drive(dir.path(), "Old");
    let d = Device::open_export(&root).unwrap();
    // Nothing to hold tags at all.
    let t = tags::read(&d).unwrap();
    assert!(t.sources.is_empty() && !t.warnings.is_empty());
    let first = d.library().unwrap().tracks[0].id;
    assert!(matches!(tags::plan(&d, &add("Genre/House", vec![first])), Err(Error::NotFound(_))));

    common::add_onelibrary(&root);
    let mut d = Device::open_export(&root).unwrap();
    let plan = tags::plan(&d, &add("Genre/House", vec![first])).unwrap();
    assert_eq!(plan.databases, [Source::OneLibrary]);
    assert!(plan.warnings.iter().any(|w| w.contains("no exportExt.pdb")), "{:?}", plan.warnings);
    tags::apply(&plan, &mut d, &Ctx::silent()).unwrap();
    assert_eq!(plus_tags(&root).len(), 1);
    assert!(!root.join(paths::EXPORT_EXT_PDB).exists());
}

#[test]
fn an_unreadable_onelibrary_is_left_alone() {
    let dir = tempfile::tempdir().unwrap();
    let root = common::demo_drive(dir.path(), "Main");
    fs::write(root.join(paths::LIBRARY_PLUS_DB), b"not a database").unwrap();
    let mut d = Device::open_export(&root).unwrap();
    let plan = tags::plan(&d, &add("Genre/Techno", select(&d, "Demo Track 2"))).unwrap();
    assert_eq!(plan.databases, [Source::ExportExt]);
    assert!(plan.warnings.iter().any(|w| w.contains("exportLibrary.db")), "{:?}", plan.warnings);
    tags::apply(&plan, &mut d, &Ctx::silent()).unwrap();
    assert_eq!(ext_tags(&root), map(&[("Genre/Techno", &["Demo Track 2"])]));
    assert_eq!(fs::read(root.join(paths::LIBRARY_PLUS_DB)).unwrap(), b"not a database");
}

/// A damaged exportExt.pdb is refused as a damaged export.pdb is: the Tags table's first page
/// leading back to itself used to read as a tree with no My Tags, and tag edits went ahead.
#[test]
fn a_damaged_exportext_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let root = common::demo_drive(dir.path(), "Main");
    let path = root.join(paths::EXPORT_EXT_PDB);
    let good = fs::read(&path).unwrap();
    let db = ExtDatabase::from_bytes(good.clone()).unwrap();
    let i = db.tables.iter().position(|t| t.table_type == ExtTableType::Tags as u32).unwrap();
    let (entry, first) = (0x1c + i * 16, db.tables[i].first_page as usize * db.page_size as usize);
    let mut cyclic = good.clone();
    cyclic[first + 12..first + 16].copy_from_slice(&db.tables[i].first_page.to_le_bytes());
    cyclic[entry + 12..entry + 16].copy_from_slice(&u32::MAX.to_le_bytes());
    let mut huge = good;
    huge[8..12].copy_from_slice(&u32::MAX.to_le_bytes()); // the table count
    for bytes in [cyclic, huge] {
        fs::write(&path, &bytes).unwrap();
        let d = Device::open_export(&root).unwrap();
        let err = tags::read(&d).unwrap_err().to_string();
        assert!(err.contains(&paths::EXPORT_EXT_PDB.replace('/', std::path::MAIN_SEPARATOR_STR)), "{err}");
        assert!(tags::plan(&d, &add("Genre/Probe", select(&d, "Demo Track 2"))).is_err());
        assert_eq!(fs::read(&path).unwrap(), bytes);
    }
}

// ---------------------------------------------------------------------------------------------
// Merge and convert carry My Tags along
// ---------------------------------------------------------------------------------------------

/// rekordbox-pdb's twelve-track export with rekordbox's default My Tags in exportExt.pdb (none on
/// its tracks) and, unless `plus` is false, OneLibrary with the same.
fn bigger_with_tags(dir: &Path, plus: bool) -> PathBuf {
    let root = common::bigger_drive(dir, "Main");
    let ext = common::fixtures().join("rekordcrate/demo_tracks/PIONEER/rekordbox/exportExt.pdb");
    fs::copy(ext, root.join(paths::EXPORT_EXT_PDB)).unwrap();
    if plus {
        common::add_onelibrary(&root);
        mirror_tags_into_onelibrary(&root);
    }
    root
}

/// Give the tracks matching `query` a tag.
fn tag(d: &mut Device, path: &str, query: &str) {
    let tracks = select(d, query);
    apply(d, add(path, tracks));
}

/// A tag's id in each database.
fn ids_of(root: &Path, name: &str) -> (Option<u32>, Option<i64>) {
    let ext = ExtDatabase::from_file(root.join(paths::EXPORT_EXT_PDB)).unwrap();
    let plus = OneLibrary::open_root(root).ok().map(|one| one.my_tag_rows().unwrap());
    (
        ext.tags.iter().find(|t| t.name == name).map(|t| t.id),
        plus.and_then(|rows| rows.into_iter().find(|r| r.name == name)).map(|r| r.id),
    )
}

fn merge_all(src: &Path, dst: &Path) -> (merge::MergePlan, merge::MergeReport) {
    let (src, mut dst) = (Device::open_export(src).unwrap(), Device::open_export(dst).unwrap());
    let plan = merge::plan(&src, &dst, &MergeOptions::default(), &Ctx::silent()).unwrap();
    let report = merge::apply(&plan, &src, &mut dst, &MergeOptions::default(), &Ctx::silent()).unwrap();
    (plan, report)
}

#[test]
fn merge_brings_my_tags_into_both_databases() {
    let dir = tempfile::tempdir().unwrap();
    let src = both_drive_named(dir.path(), "Guest");
    let mut s = Device::open_export(&src).unwrap();
    tag(&mut s, "Genre/Techno", "Demo Track 1");
    tag(&mut s, "Genre/Minimal", "Demo Track 1");
    tag(&mut s, "Situation/Lounge", "Demo Track 2");
    let dst = bigger_with_tags(dir.path(), true);
    let ext_before = fs::read(dst.join(paths::EXPORT_EXT_PDB)).unwrap();

    let (plan, report) = merge_all(&src, &dst);
    assert_eq!(plan.new_tracks.len(), 3);
    assert_eq!(plan.tagged_tracks, 2);
    let created: Vec<(&str, Source)> = plan.new_tags.iter().map(|c| (c.path.as_str(), c.database)).collect();
    assert_eq!(created, [("Genre/Minimal", Source::ExportExt), ("Genre/Minimal", Source::OneLibrary)]);
    assert_eq!(report.tagged_tracks, 2);

    let want = map(&[
        ("Genre/Minimal", &["Demo Track 1"]),
        ("Genre/Techno", &["Demo Track 1"]),
        ("Situation/Lounge", &["Demo Track 2"]),
    ]);
    assert_eq!(ext_tags(&dst), want);
    assert_eq!(plus_tags(&dst), want);
    // The new tag has the same id in both databases, and they agree on everything.
    let (ext_id, plus_id) = ids_of(&dst, "Minimal");
    assert_eq!(ext_id.map(i64::from), plus_id);
    let d = Device::open_export(&dst).unwrap();
    assert!(tags::read(&d).unwrap().differences.is_empty(), "{:?}", tags::read(&d).unwrap().differences);

    // Undone with the merge.
    let mut d = Device::open_export(&dst).unwrap();
    backup::restore(&mut d, report.backup.as_deref(), false).unwrap();
    assert_eq!(fs::read(dst.join(paths::EXPORT_EXT_PDB)).unwrap(), ext_before);
    assert!(plus_tags(&dst).is_empty());
}

#[test]
fn merged_tags_come_from_whichever_source_database_has_them() {
    // A source with exportExt.pdb only, into a target with both databases: OneLibrary gets the
    // tags too, with exportExt.pdb's ids.
    let dir = tempfile::tempdir().unwrap();
    let src = common::demo_drive(dir.path(), "Guest");
    let mut s = Device::open_export(&src).unwrap();
    tag(&mut s, "Genre/Techno", "Demo Track 1");
    let dst = common::bigger_drive(dir.path(), "Main");
    fs::copy(src.join(paths::EXPORT_EXT_PDB), dst.join(paths::EXPORT_EXT_PDB)).unwrap();
    {
        // The target's own copy carries no links (they'd name the source's track ids).
        let mut ed = ExtEditor::from_file(dst.join(paths::EXPORT_EXT_PDB)).unwrap();
        ed.unlink_tracks(&[1, 2]).unwrap();
        ed.save(dst.join(paths::EXPORT_EXT_PDB)).unwrap();
    }
    common::add_onelibrary(&dst);
    let (plan, _) = merge_all(&src, &dst);
    assert_eq!(plan.tagged_tracks, 1);
    let want = map(&[("Genre/Techno", &["Demo Track 1"])]);
    assert_eq!(ext_tags(&dst), want);
    assert_eq!(plus_tags(&dst), want);
    let (ext_id, plus_id) = ids_of(&dst, "Techno");
    assert_eq!(ext_id.map(i64::from), plus_id, "OneLibrary's new tag takes exportExt.pdb's id");

    // A OneLibrary-only source, into a target with exportExt.pdb only.
    let dir = tempfile::tempdir().unwrap();
    let src = both_drive_named(dir.path(), "Guest");
    let mut s = Device::open_export(&src).unwrap();
    tag(&mut s, "Components/Synth", "NOISE");
    for key in [paths::EXPORT_PDB, paths::EXPORT_EXT_PDB] {
        fs::remove_file(src.join(key)).unwrap();
    }
    let dst = bigger_with_tags(dir.path(), false);
    let (plan, _) = merge_all(&src, &dst);
    assert_eq!(plan.tagged_tracks, 1);
    assert_eq!(ext_tags(&dst), map(&[("Components/Synth", &["NOISE"])]));
}

#[test]
fn merged_tracks_dont_inherit_stale_links_and_duplicates_keep_their_tags() {
    let dir = tempfile::tempdir().unwrap();
    let src = both_drive_named(dir.path(), "Guest");
    let mut s = Device::open_export(&src).unwrap();
    tag(&mut s, "Genre/Techno", "Demo Track 1");
    let dst = bigger_with_tags(dir.path(), false);
    // A link left by a track removed from export.pdb: its id is the next one a track gets.
    let next = Device::open_export(&dst).unwrap().library().unwrap().tracks.iter().map(|t| t.id).max().unwrap() + 1;
    let mut ed = ExtEditor::from_file(dst.join(paths::EXPORT_EXT_PDB)).unwrap();
    let trap = ed.tags(1).iter().find(|(_, n)| *n == "Trap").unwrap().0;
    ed.link(trap, next).unwrap();
    ed.save(dst.join(paths::EXPORT_EXT_PDB)).unwrap();

    merge_all(&src, &dst);
    assert_eq!(ext_tags(&dst), map(&[("Genre/Techno", &["Demo Track 1"])]));

    // Merging again: every track is a duplicate now and keeps the target's tags.
    let mut d = Device::open_export(&dst).unwrap();
    let tracks = select(&d, "Demo Track 1");
    apply(&mut d, Edit::Remove { tag: "Genre/Techno".into(), tracks });
    let (src_dev, dst_dev) = (Device::open_export(&src).unwrap(), Device::open_export(&dst).unwrap());
    let plan = merge::plan(&src_dev, &dst_dev, &MergeOptions::default(), &Ctx::silent()).unwrap();
    assert!(plan.new_tracks.is_empty());
    assert_eq!(plan.tagged_tracks, 0);
    assert!(ext_tags(&dst).is_empty());
}

#[test]
fn merging_tags_beyond_rekordboxs_limits_skips_them() {
    let dir = tempfile::tempdir().unwrap();
    let src = common::demo_drive(dir.path(), "Guest");
    let mut ed = ExtEditor::from_file(src.join(paths::EXPORT_EXT_PDB)).unwrap();
    ed.add_category(5, "Energy").unwrap();
    ed.add_tag(5, 0x7000_0005, "Peak").unwrap();
    let techno = ed.tags(1).iter().find(|(_, n)| *n == "Techno").unwrap().0;
    for track in [1, 2] {
        ed.link(0x7000_0005, track).unwrap();
        ed.link(techno, track).unwrap();
    }
    ed.save(src.join(paths::EXPORT_EXT_PDB)).unwrap();
    let dst = bigger_with_tags(dir.path(), false);

    let (plan, report) = merge_all(&src, &dst);
    assert!(plan.warnings.iter().any(|w| w.contains("Energy/Peak")), "{:?}", plan.warnings);
    assert!(plan.new_tags.is_empty());
    assert_eq!(report.tagged_tracks, 2);
    assert_eq!(ext_tags(&dst), map(&[("Genre/Techno", &["Demo Track 1", "Demo Track 2"])]));
}

#[test]
fn convert_to_onelibrary_brings_exportext_tags() {
    let dir = tempfile::tempdir().unwrap();
    let root = common::demo_drive(dir.path(), "Main");
    let mut d = Device::open_export(&root).unwrap();
    tag(&mut d, "Genre/Techno", "Demo Track");
    tag(&mut d, "Situation/Lounge", "Demo Track 2");
    let opts = ConvertOptions { to: Target::OneLibrary, replace: false };
    let plan = convert::plan(&d, &opts, &Ctx::silent()).unwrap();
    assert_eq!(plan.tagged_tracks, 2);
    assert!(plan.warnings.iter().all(|w| !w.contains("My Tags")), "{:?}", plan.warnings);
    let report = convert::apply(&plan, &mut d, &opts, &Ctx::silent()).unwrap();
    assert_eq!(report.tagged_tracks, 2);
    assert_eq!(plus_tags(&root), ext_tags(&root));
    // Every category and tag, in order and with the same ids, tracks or not.
    let t = tags::read(&d).unwrap();
    assert!(t.differences.is_empty(), "{:?}", t.differences);
    assert_eq!(t.categories.iter().map(|c| c.tags.len()).sum::<usize>(), 24);
    let (ext_id, plus_id) = ids_of(&root, "Acid House");
    assert_eq!(ext_id.map(i64::from), plus_id);
}

#[test]
fn convert_to_device_library_moves_exportext_links_to_the_new_ids() {
    let dir = tempfile::tempdir().unwrap();
    let root = both_drive(dir.path());
    let mut d = Device::open_export(&root).unwrap();
    tag(&mut d, "Situation/Lounge", "Demo Track 1");
    tag(&mut d, "Genre/Techno", "Demo Track 2");
    tag(&mut d, "Genre/Minimal", "NOISE");
    // OneLibrary drifts: it loses Demo Track 1 (so the rebuilt export.pdb numbers the others
    // differently) and NOISE gains a tag only it has.
    let mut one = OneLibrary::open_root(&root).unwrap();
    let lib = one.library().unwrap();
    let id = |title: &str| lib.tracks.iter().find(|t| t.title == title).unwrap().id;
    one.remove_tracks(&[id("Demo Track 1")]).unwrap();
    let synth = one.my_tag_rows().unwrap().into_iter().find(|r| r.name == "Synth").unwrap().id;
    one.link_my_tag(synth, &[id("NOISE")]).unwrap();
    fs::write(root.join(paths::LIBRARY_PLUS_DB), one.to_encrypted().unwrap()).unwrap();
    let before: Vec<(u32, String)> =
        d.library().unwrap().tracks.iter().map(|t| (t.id, t.title.clone())).collect::<Vec<_>>();

    let opts = ConvertOptions { to: Target::DeviceLibrary, replace: true };
    let plan = convert::plan(&d, &opts, &Ctx::silent()).unwrap();
    assert!(plan.warnings.iter().any(|w| w.contains("1 My Tag link(s)")), "{:?}", plan.warnings);
    convert::apply(&plan, &mut d, &opts, &Ctx::silent()).unwrap();
    let after: Vec<(u32, String)> = d.library().unwrap().tracks.iter().map(|t| (t.id, t.title.clone())).collect();
    assert_ne!(before, after, "the tracks have new ids");
    let want =
        map(&[("Components/Synth", &["NOISE"]), ("Genre/Minimal", &["NOISE"]), ("Genre/Techno", &["Demo Track 2"])]);
    assert_eq!(ext_tags(&root), want);
    assert_eq!(plus_tags(&root), want);
    assert!(tags::read(&d).unwrap().differences.is_empty());
}

/// Converting into a drive that has the database adds the My Tags only the other one has, and
/// keeps its own.
#[test]
fn converting_into_a_database_the_drive_has_adds_the_other_ones_tags() {
    let dir = tempfile::tempdir().unwrap();
    let root = both_drive(dir.path());
    let mut d = Device::open_export(&root).unwrap();
    tag(&mut d, "Genre/Techno", "Demo Track 2");
    tag(&mut d, "Situation/Lounge", "Demo Track 1");
    // OneLibrary loses Demo Track 1's Lounge and gains Synth on NOISE.
    let mut one = OneLibrary::open_root(&root).unwrap();
    let lib = one.library().unwrap();
    let id = |title: &str| lib.tracks.iter().find(|t| t.title == title).unwrap().id;
    one.unlink_my_tags_of(&[id("Demo Track 1")]).unwrap();
    let synth = one.my_tag_rows().unwrap().into_iter().find(|r| r.name == "Synth").unwrap().id;
    one.link_my_tag(synth, &[id("NOISE")]).unwrap();
    fs::write(root.join(paths::LIBRARY_PLUS_DB), one.to_encrypted().unwrap()).unwrap();

    for to in [Target::DeviceLibrary, Target::OneLibrary] {
        let opts = ConvertOptions { to, replace: false };
        let plan = convert::plan(&d, &opts, &Ctx::silent()).unwrap();
        assert!(plan.adds && plan.tagged_tracks == 1, "{plan:?}");
        let report = convert::apply(&plan, &mut d, &opts, &Ctx::silent()).unwrap();
        assert_eq!(report.tagged_tracks, 1);
    }
    let want = map(&[
        ("Genre/Techno", &["Demo Track 2"]),
        ("Components/Synth", &["NOISE"]),
        ("Situation/Lounge", &["Demo Track 1"]),
    ]);
    let sorted = |m: IndexMap<String, Vec<String>>| -> Vec<(String, Vec<String>)> {
        let mut v: Vec<_> = m.into_iter().collect();
        v.sort();
        v
    };
    assert_eq!(sorted(ext_tags(&root)), sorted(want.clone()));
    assert_eq!(sorted(plus_tags(&root)), sorted(want));
    common::sqlcipher_reads(&root);
}

#[test]
fn convert_to_device_library_drops_links_it_cant_place() {
    // exportExt.pdb left behind on a OneLibrary-only drive: its links name tracks of an
    // export.pdb that's gone, so none of them can be trusted.
    let dir = tempfile::tempdir().unwrap();
    let root = both_drive(dir.path());
    let mut d = Device::open_export(&root).unwrap();
    tag(&mut d, "Genre/Techno", "Demo Track 1");
    let ext = fs::read(root.join(paths::EXPORT_EXT_PDB)).unwrap();
    let mut one = OneLibrary::open_root(&root).unwrap();
    let techno = one.my_tag_rows().unwrap().into_iter().find(|r| r.name == "Techno").unwrap().id;
    let noise = one.library().unwrap().tracks.iter().find(|t| t.title == "NOISE").unwrap().id;
    one.unlink_my_tags_of(&[1, 2, 3]).unwrap();
    one.link_my_tag(techno, &[noise]).unwrap();
    fs::write(root.join(paths::LIBRARY_PLUS_DB), one.to_encrypted().unwrap()).unwrap();
    fs::remove_file(root.join(paths::EXPORT_PDB)).unwrap();
    fs::write(root.join(paths::EXPORT_EXT_PDB), ext).unwrap();

    let mut d = Device::open_export(&root).unwrap();
    let opts = ConvertOptions { to: Target::DeviceLibrary, replace: false };
    let plan = convert::plan(&d, &opts, &Ctx::silent()).unwrap();
    assert!(plan.warnings.iter().any(|w| w.contains("1 My Tag link(s)")), "{:?}", plan.warnings);
    convert::apply(&plan, &mut d, &opts, &Ctx::silent()).unwrap();
    assert_eq!(ext_tags(&root), map(&[("Genre/Techno", &["NOISE"])]));
}

/// Regression: `tags add`, `art set` and `cues` selected without the drive's My Tags, so a
/// `tag:` filter matched no track, and negated, every track.
#[test]
fn tag_filters_select_with_the_drives_my_tags() {
    use locallibrary::api::{self, ListCues, TagAction, TagsEdit};
    use locallibrary::query::TrackSelector;
    let dir = tempfile::tempdir().unwrap();
    let root = both_drive(dir.path());
    let mut d = Device::open_export(&root).unwrap();
    let peak = select(&d, "Demo Track 1");
    apply(&mut d, add("Situation/Peak Time", peak.clone()));
    let drive = root.display().to_string();
    let filter = |f: &str| TrackSelector { filter: Some(f.into()), ..Default::default() };

    let untagged = TagAction::Add { tag: "Genre/Techno".into(), tracks: filter("-tag:\"Peak Time\"") };
    let plan = api::tags_plan(&TagsEdit { drive: drive.clone(), action: untagged }).unwrap();
    let titles: Vec<&str> = plan.tracks.iter().map(|t| t.name.as_str()).collect();
    assert_eq!(titles.len(), 2, "{titles:?}");
    assert!(!titles.iter().any(|t| t.contains("Demo Track 1")), "{titles:?}");
    let cues = api::cues_list(&ListCues { drive, tracks: filter("tag:\"Peak Time\"") }).unwrap();
    assert_eq!(cues.iter().map(|c| c.track.id).collect::<Vec<_>>(), peak);
}
