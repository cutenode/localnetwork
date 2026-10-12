//! `tags` edits on a drive against a model of its My Tags: categories in order, each with its tags
//! in order and the tracks carrying them. Names match exactly, else ignoring case when only one
//! does; a new category or tag goes last; rekordbox's limits (4 categories, 50 tags in one) and
//! exportExt.pdb's row size are refused when planning, as are paths that aren't `Category/Tag`.
//! The drive has exportExt.pdb, OneLibrary, both, or neither (then every edit is refused).

use crate::common;

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use hegel::generators as gs;
use hegel::{HealthCheck, TestCase};
use locallibrary::device::Device;
use locallibrary::onelibrary::OneLibrary;
use locallibrary::paths;
use locallibrary::progress::Ctx;
use locallibrary::tags::{self, Edit};
use rekordbox_pdb::{ExtDatabase, ExtEditor};

/// Categories in order, each with its tags in order and the files of the tracks carrying them.
type Model = Vec<(String, Vec<(String, BTreeSet<String>)>)>;

fn read(d: &Device) -> Model {
    tags::read(d)
        .unwrap()
        .categories
        .iter()
        .map(|c| {
            let tags = c
                .tags
                .iter()
                .map(|t| (t.name.clone(), t.tracks.iter().map(|r| r.file_path.clone()).collect()))
                .collect();
            (c.name.clone(), tags)
        })
        .collect()
}

/// The one of `names` that `wanted` names: the exact one, or the only one ignoring case.
fn find(names: impl IntoIterator<Item = String>, wanted: &str) -> Option<usize> {
    let names: Vec<String> = names.into_iter().collect();
    if let Some(i) = names.iter().position(|n| n == wanted) {
        return Some(i);
    }
    let lower: Vec<usize> = (0..names.len()).filter(|i| names[*i].to_lowercase() == wanted.to_lowercase()).collect();
    (lower.len() == 1).then(|| lower[0])
}

/// The model after `edit` (tracks by file), or `Err` if it should be refused.
fn after(m: &Model, edit: &Edit, files: &[(u32, String)], ext: bool) -> Result<Model, ()> {
    let file = |id: &u32| files.iter().find(|(i, _)| i == id).unwrap().1.clone();
    let mut m = m.clone();
    let path = match edit {
        Edit::Add { tag, .. } | Edit::Remove { tag, .. } | Edit::Rename { tag, .. } | Edit::Delete { tag } => tag,
    };
    let (category, tag) = match path.split_once('/') {
        Some((c, t)) => (c.trim(), Some(t.trim())),
        None => (path.trim(), None),
    };
    if category.is_empty() || tag.is_some_and(str::is_empty) {
        return Err(());
    }
    // exportExt.pdb rows hold a name of a few thousand bytes; OneLibrary takes any.
    let too_long = |name: &str| ext && name.len() > 3000;
    let c = find(m.iter().map(|c| c.0.clone()), category);
    match edit {
        Edit::Add { tracks, .. } => {
            let tag = tag.ok_or(())?;
            let c = match c {
                Some(c) => c,
                None if m.len() >= tags::MAX_CATEGORIES || too_long(category) => return Err(()),
                None => {
                    m.push((category.to_string(), Vec::new()));
                    m.len() - 1
                }
            };
            let t = match find(m[c].1.iter().map(|t| t.0.clone()), tag) {
                Some(t) => t,
                None if m[c].1.len() >= tags::MAX_TAGS || too_long(tag) => return Err(()),
                None => {
                    m[c].1.push((tag.to_string(), BTreeSet::new()));
                    m[c].1.len() - 1
                }
            };
            m[c].1[t].1.extend(tracks.iter().map(file));
        }
        Edit::Remove { tracks, .. } => {
            let tag = tag.ok_or(())?;
            // Anything but adding needs the tag (or category) to be there.
            let c = c.ok_or(())?;
            let t = find(m[c].1.iter().map(|t| t.0.clone()), tag).ok_or(())?;
            for id in tracks {
                m[c].1[t].1.remove(&file(id));
            }
        }
        Edit::Rename { to, .. } => {
            let to = to.trim();
            if to.is_empty() || (tag.is_none() && to.contains('/')) {
                return Err(());
            }
            let c = c.ok_or(())?;
            let (names, at): (Vec<String>, usize) = match tag {
                None => (m.iter().map(|c| c.0.clone()).collect(), c),
                Some(tag) => (
                    m[c].1.iter().map(|t| t.0.clone()).collect(),
                    find(m[c].1.iter().map(|t| t.0.clone()), tag).ok_or(())?,
                ),
            };
            if names.iter().enumerate().any(|(i, n)| i != at && n.to_lowercase() == to.to_lowercase()) {
                return Err(());
            }
            if names[at] != to && too_long(to) {
                return Err(());
            }
            match tag {
                None => m[c].0 = to.to_string(),
                Some(_) => m[c].1[at].0 = to.to_string(),
            }
        }
        Edit::Delete { .. } => {
            let tag = tag.ok_or(())?;
            let c = c.ok_or(())?;
            let t = find(m[c].1.iter().map(|t| t.0.clone()), tag).ok_or(())?;
            m[c].1.remove(t);
        }
    }
    Ok(m)
}

/// Give OneLibrary the categories and tags exportExt.pdb has, as rekordbox exports both.
fn mirror(root: &Path) {
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

#[hegel::test_helper]
fn draw_edit(tc: &TestCase, m: &Model, tracks: &[u32]) -> Edit {
    // Paths the drive has (as written, in other case), new ones, and ones that aren't paths.
    let mut paths: Vec<String> = vec!["Mood/Dark".into(), "Genre/".into(), "/Deep".into(), " ".into()];
    for (c, ts) in m {
        paths.push(c.clone());
        paths.push(format!("{}/Deep", c.to_uppercase()));
        for (t, _) in ts.iter().take(3) {
            paths.push(format!("{c}/{t}"));
            paths.push(format!("{}/{}", c.to_lowercase(), t.to_lowercase()));
        }
    }
    let tag = tc.draw(gs::sampled_from(paths));
    let ids: Vec<u32> = tracks.iter().copied().filter(|_| tc.draw(gs::booleans())).collect();
    let ids = if ids.is_empty() { tracks[..1].to_vec() } else { ids };
    match tc.draw(gs::integers::<u8>().max_value(6)) {
        0..=2 => Edit::Add { tag, tracks: ids },
        3 => Edit::Remove { tag, tracks: ids },
        4 | 5 => {
            let long = "n".repeat(5000);
            let to =
                tc.draw(gs::sampled_from(vec!["Deep House", "deep", "Genre", "genre", "A/B", "", "  ", long.as_str()]));
            Edit::Rename { tag, to: to.to_string() }
        }
        _ => Edit::Delete { tag },
    }
}

#[hegel::test(test_cases = 16, suppress_health_check = [HealthCheck::TooSlow])]
fn tag_edits_on_a_drive_match_the_model(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let root = common::demo_drive(dir.path(), "Main");
    // At times a category already holding as many tags as rekordbox allows.
    if tc.draw(gs::booleans()) {
        let ext_path = root.join(paths::EXPORT_EXT_PDB);
        let mut ed = ExtEditor::from_file(&ext_path).unwrap();
        let (components, _) = ed.categories()[1];
        for n in 0..u32::try_from(tags::MAX_TAGS).unwrap() {
            ed.add_tag(components, 0x2_0000 + n, &format!("Part {n}")).unwrap();
        }
        ed.save(&ext_path).unwrap();
    }
    // At times a link to a track that isn't there, which reading counts and leaves alone.
    let stray = tc.draw(gs::booleans());
    if stray {
        let ext_path = root.join(paths::EXPORT_EXT_PDB);
        let mut ed = ExtEditor::from_file(&ext_path).unwrap();
        let (tag, _) = ed.tags(ed.categories()[0].0)[0];
        ed.link(tag, 999).unwrap();
        ed.save(&ext_path).unwrap();
    }
    // exportExt.pdb alone, OneLibrary as well, OneLibrary alone beside export.pdb, neither,
    // exportExt.pdb unreadable beside OneLibrary, or exportExt.pdb without the export.pdb its tags
    // point into.
    let dbs = tc.draw(gs::integers::<u8>().max_value(5));
    if dbs != 0 {
        common::add_onelibrary(&root);
        mirror(&root);
    }
    match dbs {
        2 | 3 => fs::remove_file(root.join(paths::EXPORT_EXT_PDB)).unwrap(),
        4 => fs::write(root.join(paths::EXPORT_EXT_PDB), b"not a database").unwrap(),
        5 => fs::remove_file(root.join(paths::EXPORT_PDB)).unwrap(),
        _ => {}
    }
    if dbs == 3 {
        fs::remove_file(root.join(paths::LIBRARY_PLUS_DB)).unwrap();
    }
    let ext = dbs <= 1;
    let ext_before = fs::read(root.join(paths::EXPORT_EXT_PDB)).ok();
    let mut d = Device::open_export(&root).unwrap();
    let files: Vec<(u32, String)> = d.library().unwrap().tracks.iter().map(|t| (t.id, t.file_path.clone())).collect();
    let ids: Vec<u32> = files.iter().map(|(id, _)| *id).collect();
    let mut model = if dbs == 3 { Vec::new() } else { read(&d) };
    if dbs != 3 {
        let warnings = tags::read(&d).unwrap().warnings;
        let says = |what: &str| warnings.iter().any(|w| w.contains(what));
        assert_eq!(
            says("belong to no category, or point at a tag or track that isn't there"),
            stray && ext,
            "{warnings:?}"
        );
        assert_eq!(says("Only exportLibrary.db's My Tags are read and edited"), dbs == 4, "{warnings:?}");
        assert_eq!(says("exportExt.pdb is left alone"), dbs == 5, "{warnings:?}");
    }

    for _ in 0..tc.draw(gs::integers::<usize>().min_value(1).max_value(6)) {
        let edit = draw_edit(&tc, &model, &ids);
        let want = if dbs == 3 { Err(()) } else { after(&model, &edit, &files, ext) };
        match tags::plan(&d, &edit) {
            Ok(plan) => {
                assert!(want.is_ok(), "{edit:?} was planned");
                if dbs == 2 {
                    assert!(plan.warnings.iter().any(|w| w.contains("has no exportExt.pdb")), "{:?}", plan.warnings);
                }
                tags::apply(&plan, &mut d, &Ctx::silent()).unwrap();
                model = want.unwrap();
                assert_eq!(read(&d), model, "after {edit:?}");
            }
            Err(e) => {
                assert!(want.is_err(), "{edit:?} was refused: {e}");
                if dbs == 3 {
                    assert!(e.to_string().contains("neither exportExt.pdb nor exportLibrary.db"), "{e}");
                }
            }
        }
    }
    // What's always refused: deleting a category, renaming one to a name with a slash or to
    // another's name, a fifth category, a fifty-first tag.
    let mut refused =
        vec![Edit::Rename { tag: "Genre".into(), to: "A/B".into() }, Edit::Delete { tag: "Genre".into() }];
    if let [first, second, ..] = &model[..] {
        refused.push(Edit::Rename { tag: first.0.clone(), to: second.0.to_lowercase() });
    }
    if model.len() >= tags::MAX_CATEGORIES {
        refused.push(Edit::Add { tag: "Fifth/Tag".into(), tracks: ids[..1].to_vec() });
    }
    if let Some((c, _)) = model.iter().find(|(_, ts)| ts.len() >= tags::MAX_TAGS) {
        refused.push(Edit::Add { tag: format!("{c}/One Too Many"), tracks: ids[..1].to_vec() });
    }
    for edit in refused {
        if after(&model, &edit, &files, ext).is_err() {
            assert!(tags::plan(&d, &edit).is_err(), "{edit:?} was planned");
        }
    }
    assert_eq!(read(&d), model);
    // A database the edits can't be made in is left as it was.
    if matches!(dbs, 4 | 5) {
        assert_eq!(fs::read(root.join(paths::EXPORT_EXT_PDB)).ok(), ext_before);
    }
}
