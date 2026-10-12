//! `tags` edits where a drive's two My Tag databases differ: a category or tag one of them lacks
//! takes the other's id, one neither has gets the same new id in both, and what either can't hold
//! is refused before anything is written. Plans made stale before they're applied are refused, a
//! replay after a new export skips what no longer applies, and an exportExt.pdb without My Tag
//! tables is read around.

use crate::common;

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use hegel::generators as gs;
use hegel::{HealthCheck, TestCase};
use locallibrary::device::Device;
use locallibrary::edit::{self, EditOptions};
use locallibrary::onelibrary::OneLibrary;
use locallibrary::progress::Ctx;
use locallibrary::tags::{self, Edit, Source};
use locallibrary::{Error, paths};
use rekordbox_pdb::{ExtDatabase, ExtEditor};

/// Give OneLibrary the categories and tags exportExt.pdb has, with their ids, as rekordbox
/// exports both.
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

/// Each database's My Tags: `Category` or `Category/Tag` -> (id, files of the tracks carrying it).
type Held = Vec<(String, i64, BTreeSet<String>)>;

fn ext_held(root: &Path) -> Held {
    let ext = ExtDatabase::from_file(root.join(paths::EXPORT_EXT_PDB)).unwrap();
    let lib = locallibrary::pdb::read_library(&root.join(paths::EXPORT_PDB)).unwrap();
    let mut out = Vec::new();
    for c in ext.categories() {
        out.push((c.name.clone(), i64::from(c.id), BTreeSet::new()));
        for t in ext.tags_in(c.id) {
            let files = ext
                .tag_tracks
                .iter()
                .filter(|l| l.tag_id == t.id)
                .map(|l| lib.track(l.track_id).unwrap().file_path.clone())
                .collect();
            out.push((format!("{}/{}", c.name, t.name), i64::from(t.id), files));
        }
    }
    out
}

fn plus_held(root: &Path) -> Held {
    let one = OneLibrary::open_root(root).unwrap();
    let lib = one.library().unwrap();
    let rows = one.my_tag_rows().unwrap();
    let links = one.my_tag_links().unwrap();
    let mut out = Vec::new();
    for c in rows.iter().filter(|r| r.is_category) {
        out.push((c.name.clone(), c.id, BTreeSet::new()));
        for t in rows.iter().filter(|r| r.parent == c.id && !r.is_category) {
            let files = links
                .iter()
                .filter(|(tag, _)| *tag == t.id)
                .map(|(_, track)| lib.track(*track as u32).unwrap().file_path.clone())
                .collect();
            out.push((format!("{}/{}", c.name, t.name), t.id, files));
        }
    }
    out
}

fn find<'a>(held: &'a Held, path: &str) -> Option<&'a (String, i64, BTreeSet<String>)> {
    held.iter().find(|(p, ..)| p == path)
}

fn invalid(d: &Device, edit: &Edit) -> String {
    match tags::plan(d, edit) {
        Err(Error::Invalid(m)) => m,
        other => panic!("{edit:?} wasn't refused as invalid: {:?}", other.map(|p| p.created)),
    }
}

/// A drive whose exportExt.pdb lost Situation and Untitled Column and whose OneLibrary lost
/// Components and Untitled Column, with a track only export.pdb lists: tags added to a category
/// only one database has take its id (and the tag's) in the other, a category neither has gets
/// one new id in both (past every category's and any tag's using the next one), and the tag
/// reaches every selected track either database lists, warning about the others. What either
/// database can't hold (a fifth category, a name too long for exportExt.pdb, a name taken, a
/// category deleted) is refused, the drive left alone.
#[hegel::test(test_cases = 2, suppress_health_check = [HealthCheck::TooSlow])]
fn tags_one_database_lacks_take_the_other_ones_ids(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let root = common::demo_drive(dir.path(), "Main");
    common::add_onelibrary(&root);
    mirror(&root);
    let only_ext = common::export_new_track(&root, "Only", "In export.pdb");
    let ext_path = root.join(paths::EXPORT_EXT_PDB);
    let mut ed = ExtEditor::from_file(&ext_path).unwrap();
    let categories: Vec<(u32, String)> = ed.categories().iter().map(|(id, n)| (*id, n.to_string())).collect();
    let id_of = |name: &str| categories.iter().find(|c| c.1 == name).unwrap().0;
    let tag_names = |c: &str| -> Vec<String> { ed.tags(id_of(c)).iter().map(|(_, n)| n.to_string()).collect() };
    let (genre, situation, components) = (tag_names("Genre"), tag_names("Situation"), tag_names("Components"));
    for c in ["Situation", "Untitled Column"] {
        ed.remove(id_of(c)).unwrap();
    }
    // The id after the last category's is a tag's, so a new category's id goes past it.
    let last = ["Genre", "Components", "Situation"].map(id_of).into_iter().max().unwrap();
    if !ed.id_in_use(last + 1) {
        ed.add_tag(id_of("Genre"), last + 1, "Extra").unwrap();
    }
    ed.save(&ext_path).unwrap();
    let mut one = OneLibrary::open_root(&root).unwrap();
    for c in ["Components", "Untitled Column"] {
        one.remove_my_tag(id_of(c).into()).unwrap();
    }
    // OneLibrary's Genre has its first tag last.
    let rows = one.my_tag_rows().unwrap();
    let first = rows.iter().find(|r| r.parent == i64::from(id_of("Genre")) && r.name == genre[0]).unwrap();
    let (first_id, first_name) = (first.id, first.name.clone());
    one.remove_my_tag(first_id).unwrap();
    one.add_my_tag(first_id, id_of("Genre").into(), &first_name, false).unwrap();
    fs::write(root.join(paths::LIBRARY_PLUS_DB), one.to_encrypted().unwrap()).unwrap();

    let mut d = Device::open_export(&root).unwrap();
    let files: Vec<(u32, String)> = d.library().unwrap().tracks.iter().map(|t| (t.id, t.file_path.clone())).collect();
    let file = |id: u32| files.iter().find(|(i, _)| *i == id).unwrap().1.clone();
    let before = common::Before::take(&root);
    // A category name too long for exportExt.pdb, while it has room for one.
    let long = "x".repeat(tc.draw(gs::integers::<usize>().min_value(4000).max_value(6000)));
    let err = invalid(&d, &Edit::Add { tag: format!("{long}/Tag"), tracks: vec![1] });
    assert!(err.starts_with("the category name is too long for exportExt.pdb"), "{err}");
    before.untouched(&root).unwrap();

    // Tracks both databases list, and the one only export.pdb does.
    let mut tracks: Vec<u32> = [1, 2].into_iter().filter(|_| tc.draw(gs::booleans())).collect();
    if tracks.is_empty() {
        tracks.push(1);
    }
    tracks.push(only_ext);
    let venue = tc.draw(gs::sampled_from(vec!["Club", "Rooftop", "Ünï Hall"])).to_string();
    let spelled = tc.draw(gs::sampled_from(vec!["Situation", "situation", "SITUATION"])).to_string();
    let moment = tc.draw(gs::sampled_from(situation.clone()));
    let part = tc.draw(gs::sampled_from(components.clone()));
    let edits = [
        Edit::Add { tag: format!("Venue/{venue}"), tracks: tracks.clone() },
        Edit::Add { tag: format!("{spelled}/{moment}"), tracks: tracks.clone() },
    ];
    for edit in &edits {
        let plan = tags::plan(&d, edit).unwrap();
        tc.note(&format!("{edit:?}: {:?}", plan.created));
        assert_eq!(plan.databases, [Source::ExportExt, Source::OneLibrary]);
        let warning =
            "1 selected track(s) aren't in exportLibrary.db, so players that read it won't show the tag on them";
        assert!(plan.warnings.iter().any(|w| w == warning), "{:?}", plan.warnings);
        tags::apply(&plan, &mut d, &Ctx::silent()).unwrap();
    }
    let (ext, plus) = (ext_held(&root), plus_held(&root));
    let tagged: BTreeSet<String> = tracks.iter().map(|id| file(*id)).collect();
    let in_plus: BTreeSet<String> = tagged.iter().filter(|f| !f.contains("In export.pdb")).cloned().collect();
    for path in ["Venue".to_string(), format!("Venue/{venue}"), "Situation".into(), format!("Situation/{moment}")] {
        let (e, p) = (find(&ext, &path).unwrap(), find(&plus, &path).unwrap());
        assert_eq!(e.1, p.1, "{path} has one id in both databases");
        if path.contains('/') {
            assert_eq!((&e.2, &p.2), (&tagged, &in_plus), "{path}'s tracks");
        }
    }
    let venue_id = find(&ext, "Venue").unwrap().1;
    assert!(venue_id > i64::from(last + 1), "{venue_id}: past every category's id, and the tag's after them");
    assert_eq!(find(&ext, "Situation").unwrap().1, i64::from(id_of("Situation")), "OneLibrary's id");
    // OneLibrary's category and tag would take exportExt.pdb's spelling (and ids).
    let plan =
        tags::plan(&d, &Edit::Add { tag: format!("components/{}", part.to_uppercase()), tracks: vec![1] }).unwrap();
    let created: Vec<(&str, Source)> = plan.created.iter().map(|c| (c.path.as_str(), c.database)).collect();
    let part_path = format!("Components/{part}");
    assert_eq!(created, [("Components", Source::OneLibrary), (part_path.as_str(), Source::OneLibrary)]);
    let read = tags::read(&d).unwrap();
    for difference in ["the categories are in a different order", "Genre's tags are in a different order"] {
        assert!(read.differences.iter().any(|x| x == difference), "{:?}", read.differences);
    }
    // Edits of what only exportExt.pdb has leave OneLibrary out, and taking off a tag no track
    // carries changes nothing.
    for edit in [
        Edit::Rename { tag: "components".into(), to: "Parts".into() },
        Edit::Rename { tag: format!("Components/{part}"), to: "Zither Ünï".into() },
        Edit::Delete { tag: format!("components/{part}") },
    ] {
        assert_eq!(tags::plan(&d, &edit).unwrap().databases, [Source::ExportExt], "{edit:?}");
    }
    for (tag, to) in [("Genre".to_string(), "Genre".to_string()), (format!("Genre/{}", genre[1]), genre[1].clone())] {
        assert!(tags::plan(&d, &Edit::Rename { tag, to }).unwrap().is_noop(), "renamed to its own name");
    }
    if let Some(other) = situation.iter().find(|t| **t != moment) {
        let plan = tags::plan(&d, &Edit::Remove { tag: format!("Situation/{other}"), tracks: vec![1] }).unwrap();
        assert!(plan.is_noop() && plan.unchanged == 1, "{:?}", plan.databases);
    }
    assert!(matches!(
        tags::plan(&d, &Edit::Add { tag: "Genre/Deep".into(), tracks: vec![9999] }),
        Err(Error::NotFound(_))
    ));

    // exportExt.pdb has four categories now. Taking a tag off where it is, in both.
    let off: Vec<u32> = tracks.iter().copied().filter(|_| tc.draw(gs::booleans())).collect();
    let off = if off.is_empty() { vec![tracks[0]] } else { off };
    let plan = tags::plan(&d, &Edit::Remove { tag: format!("Venue/{venue}"), tracks: off.clone() }).unwrap();
    assert_eq!(plan.tracks.len(), off.len());
    tags::apply(&plan, &mut d, &Ctx::silent()).unwrap();
    let left: BTreeSet<String> = tracks.iter().filter(|t| !off.contains(t)).map(|id| file(*id)).collect();
    assert_eq!(find(&ext_held(&root), &format!("Venue/{venue}")).unwrap().2, left);
    let left_plus: BTreeSet<String> = left.iter().filter(|f| !f.contains("In export.pdb")).cloned().collect();
    assert_eq!(find(&plus_held(&root), &format!("Venue/{venue}")).unwrap().2, left_plus);

    // What's refused, whatever the case it's written in.
    let before = common::Before::take(&root);
    let (a, b) = (tc.draw(gs::sampled_from(genre.clone())), tc.draw(gs::sampled_from(genre.clone())));
    let blank = tc.draw(gs::sampled_from(vec!["", " ", "\t "])).to_string();
    let refused: Vec<(Edit, String)> = vec![
        (Edit::Rename { tag: format!("Genre/{a}"), to: blank }, "give a new name".into()),
        (Edit::Rename { tag: "Genre".into(), to: "A/B".into() }, "can't contain '/'".into()),
        (Edit::Rename { tag: "Genre".into(), to: "VENUE".into() }, "already has a category named".into()),
        (Edit::Add { tag: "Mood/Dark".into(), tracks: vec![1] }, "4 My Tag categories".into()),
        (Edit::Add { tag: format!("Genre/{long}"), tracks: vec![1] }, "the tag name is too long".into()),
        (Edit::Rename { tag: format!("Genre/{a}"), to: long.clone() }, "the new name is too long".into()),
        (Edit::Delete { tag: "Genre".into() }, "can be renamed but not deleted".into()),
        (Edit::Add { tag: "Genre".into(), tracks: vec![1] }, "is a category".into()),
        (Edit::Add { tag: "Genre/Deep".into(), tracks: vec![] }, "no tracks selected".into()),
    ];
    let taken = (a != b).then(|| Edit::Rename { tag: format!("genre/{a}"), to: b.to_uppercase() });
    for (edit, why) in refused.into_iter().chain(taken.map(|e| (e, "already has a tag named".to_string()))) {
        let err = invalid(&d, &edit);
        assert!(err.contains(&why), "{edit:?}: {err}");
    }
    assert!(matches!(
        tags::plan(&d, &Edit::Remove { tag: "Genre/Nope".into(), tracks: vec![1] }),
        Err(Error::NotFound(_))
    ));
    before.untouched(&root).unwrap();
}

/// A plan applied after the drive's My Tags changed (another edit came first, or a database it
/// edits went) is refused as stale, the drive left as the other edit made it; one cancelled is
/// refused before anything is written.
#[hegel::test(test_cases = 2, suppress_health_check = [HealthCheck::TooSlow])]
fn a_plan_the_drive_has_moved_on_from_is_refused(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let root = common::demo_drive(dir.path(), "Main");
    common::add_onelibrary(&root);
    mirror(&root);
    let mut d = Device::open_export(&root).unwrap();
    let tag = tc.draw(gs::sampled_from(vec!["Genre/Techno", "Situation/Lounge", "Genre/Ünï"])).to_string();
    let first = tc.draw(gs::integers::<u32>().min_value(1).max_value(2));
    let stale = tags::plan(&d, &Edit::Add { tag: tag.clone(), tracks: vec![1, 2] }).unwrap();
    let plan = tags::plan(&d, &Edit::Add { tag: tag.clone(), tracks: vec![first] }).unwrap();
    tags::apply(&plan, &mut d, &Ctx::silent()).unwrap();
    let after = (ext_held(&root), plus_held(&root));
    let err = tags::apply(&stale, &mut d, &Ctx::silent()).unwrap_err();
    assert!(matches!(err, Error::StalePlan(_)), "{err}");
    assert_eq!((ext_held(&root), plus_held(&root)), after);

    // Cancelled before anything is written.
    let warnings = common::Warnings::default();
    let cancel = locallibrary::progress::CancelToken::new();
    cancel.cancel();
    let plan = tags::plan(&d, &Edit::Add { tag: "Genre/Cancelled".into(), tracks: vec![1] }).unwrap();
    let err = tags::apply(&plan, &mut d, &Ctx::new(&warnings, cancel)).unwrap_err();
    assert!(matches!(err, Error::Cancelled), "{err}");
    assert_eq!((ext_held(&root), plus_held(&root)), after);

    // A database the plan edits is gone.
    let stale = tags::plan(&d, &Edit::Rename { tag: tag.clone(), to: "Renamed".into() }).unwrap();
    fs::remove_file(root.join(paths::LIBRARY_PLUS_DB)).unwrap();
    let err = tags::apply(&stale, &mut d, &Ctx::silent()).unwrap_err();
    assert!(matches!(err, Error::StalePlan(_)), "{err}");
    assert_eq!(ext_held(&root), after.0);
}

/// After rekordbox exports again without a track the journal's edits named, replaying them gives
/// the tracks still there their tags (and takes off what was taken off), says which files are
/// gone, skips an edit that no longer applies to anything, and leaves one the export made
/// already.
#[hegel::test(test_cases = 2, suppress_health_check = [HealthCheck::TooSlow])]
fn a_replay_after_tracks_went_skips_what_no_longer_applies(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let root = common::demo_drive(dir.path(), "Main");
    let before = fs::read(root.join(paths::EXPORT_PDB)).unwrap();
    let ext_before = fs::read(root.join(paths::EXPORT_EXT_PDB)).unwrap();
    let gone = common::export_new_track(&root, "Gone", "Soon");
    let gone_file = Device::open_export(&root).unwrap().library().unwrap().track(gone).unwrap().file_path.clone();
    let mut d = Device::open_export(&root).unwrap();
    let kept = tc.draw(gs::sampled_from(vec!["Genre/Techno", "Situation/Lounge"])).to_string();
    let only = tc.draw(gs::sampled_from(vec!["Components/Synth", "Genre/Trap"])).to_string();
    let edits = [
        Edit::Add { tag: kept.clone(), tracks: vec![1, 2, gone] },
        Edit::Remove { tag: kept.clone(), tracks: vec![2] },
        Edit::Add { tag: only.clone(), tracks: vec![gone] },
        Edit::Add { tag: "Genre/Acid House".into(), tracks: vec![2] },
    ];
    for edit in &edits {
        let plan = tags::plan(&d, edit).unwrap();
        tags::apply(&plan, &mut d, &Ctx::silent()).unwrap();
    }
    // rekordbox exports again: its databases as they were before the track was added.
    fs::write(root.join(paths::EXPORT_PDB), &before).unwrap();
    fs::write(root.join(paths::EXPORT_EXT_PDB), &ext_before).unwrap();
    // This time rekordbox exported one of the tags itself: its edit has nothing left to do.
    let mut ed = ExtEditor::from_file(root.join(paths::EXPORT_EXT_PDB)).unwrap();
    let acid = ed.tags(ed.categories()[0].0).iter().find(|(_, n)| *n == "Acid House").unwrap().0;
    ed.link(acid, 2).unwrap();
    ed.save(root.join(paths::EXPORT_EXT_PDB)).unwrap();
    let mut d = Device::open_export(&root).unwrap();
    let journal = edit::journal::edits(&d).unwrap();
    assert_eq!(journal.len(), edits.len(), "{journal:?}");
    let opts = EditOptions { lenient: true, no_journal: true };
    let plan = edit::plan(&d, &journal, &opts, &Ctx::silent()).unwrap();
    let report = edit::apply(&plan, &mut d, &Ctx::silent()).unwrap();
    let says = |what: &str| report.warnings.iter().filter(|w| w.contains(what)).count();
    assert_eq!(says(&format!("aren't on the drive any more (e.g. {gone_file}), so ")), 2, "{:?}", report.warnings);
    let partly = format!(
        "1 track(s) aren't on the drive any more (e.g. {gone_file}), so \"add My Tag {kept} to 3 track(s)\" is replayed \
         without them"
    );
    assert!(report.warnings.contains(&partly), "{:?}", report.warnings);
    assert_eq!(says(&format!("skipped an edit that no longer applies (add My Tag {only}")), 1, "{:?}", report.warnings);
    let lib = d.library().unwrap();
    let held = ext_held(&root);
    assert_eq!(find(&held, &kept).unwrap().2, BTreeSet::from([lib.track(1).unwrap().file_path.clone()]));
    assert!(find(&held, &only).unwrap().2.is_empty());
    assert_eq!(find(&held, "Genre/Acid House").unwrap().2, BTreeSet::from([lib.track(2).unwrap().file_path.clone()]));
}

/// An exportExt.pdb whose table directory lists no My Tag tables (or that isn't a database) is
/// refused when it's the drive's only My Tags database, and passed over with a warning beside
/// OneLibrary, whose tags are read and edited alone, as on a drive with OneLibrary only. A folder
/// with no export has no My Tags to read.
#[hegel::test(test_cases = 2, suppress_health_check = [HealthCheck::TooSlow])]
fn an_exportext_pdb_that_cant_be_used_is_read_around(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let blank = common::blank(dir.path(), "Blank");
    assert!(matches!(tags::read(&Device::open(&blank).unwrap()), Err(Error::NotAnExport(_))));
    for kind in 0u8..3 {
        let root = common::demo_drive(dir.path(), &format!("Main {kind}"));
        let path = root.join(paths::EXPORT_EXT_PDB);
        let mut bytes = fs::read(&path).unwrap();
        let why = match kind {
            // The directory's type of either My Tag table, as another table's.
            0 => {
                let tables = u32::from_le_bytes(bytes[8..12].try_into().unwrap()) as usize;
                let which = tc.draw(gs::sampled_from(vec![3u32, 4]));
                let at = (0..tables).map(|i| 0x1c + i * 16).find(|&o| bytes[o..o + 4] == which.to_le_bytes()).unwrap();
                bytes[at..at + 4]
                    .copy_from_slice(&tc.draw(gs::integers::<u32>().min_value(9).max_value(40)).to_le_bytes());
                Some("it has no My Tag tables")
            }
            1 => {
                bytes = tc.draw(gs::sampled_from(vec![b"not a database".to_vec(), vec![0; 4096]]));
                Some("exportExt.pdb")
            }
            _ => None,
        };
        if let Some(why) = why {
            fs::write(&path, &bytes).unwrap();
            let err = tags::read(&Device::open_export(&root).unwrap()).unwrap_err();
            assert!(err.to_string().contains(why), "{err}");
        }
        common::add_onelibrary(&root);
        if why.is_none() {
            fs::remove_file(root.join(paths::EXPORT_PDB)).unwrap();
            fs::remove_file(&path).unwrap();
        }
        let mut d = Device::open_export(&root).unwrap();
        let read = tags::read(&d).unwrap();
        assert_eq!(read.sources, [Source::OneLibrary]);
        let warned = read.warnings.iter().any(|w| w.ends_with(". Only exportLibrary.db's My Tags are read and edited"));
        assert_eq!(warned, why.is_some(), "{:?}", read.warnings);
        let plan = tags::plan(&d, &Edit::Add { tag: "Genre/Deep".into(), tracks: vec![1] }).unwrap();
        assert_eq!(plan.databases, [Source::OneLibrary]);
        // Applied once: the others edit the same way.
        if kind == 0 {
            tags::apply(&plan, &mut d, &Ctx::silent()).unwrap();
            assert_eq!(find(&plus_held(&root), "Genre/Deep").unwrap().2.len(), 1);
            assert_eq!(fs::read(&path).unwrap(), bytes, "left alone");
        }
    }
}

/// export.pdb rebuilt from OneLibrary moves exportExt.pdb's links to the tracks' new ids, and
/// drops the ones it can't place: to a track export.pdb didn't list, or one OneLibrary doesn't. A
/// tag only one database gives a track both list is said to differ.
#[hegel::test(test_cases = 2, suppress_health_check = [HealthCheck::TooSlow])]
fn links_that_cant_follow_their_tracks_are_dropped(tc: TestCase) {
    use locallibrary::convert::{self, ConvertOptions, Target};
    let dir = tempfile::tempdir().unwrap();
    let root = common::demo_drive(dir.path(), "Main");
    common::export_new_track(&root, "Noise", "Maker");
    common::add_onelibrary(&root);
    mirror(&root);
    let mut d = Device::open_export(&root).unwrap();
    let tag = tc.draw(gs::sampled_from(vec!["Genre/Techno", "Situation/Lounge"])).to_string();
    let plan = tags::plan(&d, &Edit::Add { tag: tag.clone(), tracks: vec![1, 2, 3] }).unwrap();
    tags::apply(&plan, &mut d, &Ctx::silent()).unwrap();
    // A link to a track export.pdb doesn't list, and a track OneLibrary loses.
    let ext_path = root.join(paths::EXPORT_EXT_PDB);
    let mut ed = ExtEditor::from_file(&ext_path).unwrap();
    let stray = tc.draw(gs::integers::<u32>().min_value(50).max_value(5000));
    ed.link(ed.tags(ed.categories()[0].0)[0].0, stray).unwrap();
    ed.save(&ext_path).unwrap();
    let lost = tc.draw(gs::integers::<u32>().min_value(1).max_value(3));
    let lost_file = d.library().unwrap().track(lost).unwrap().file_path.clone();
    let mut one = OneLibrary::open_root(&root).unwrap();
    let plus = one.library().unwrap();
    let plus_id = plus.tracks.iter().find(|t| t.file_path == lost_file).unwrap().id;
    one.remove_tracks(&[plus_id]).unwrap();
    // And a track OneLibrary keeps loses the tag there alone.
    let other = plus.tracks.iter().find(|t| t.id != plus_id).unwrap().id;
    let tag_id = one.my_tag_rows().unwrap().iter().find(|r| tag.ends_with(&format!("/{}", r.name))).unwrap().id;
    one.unlink_my_tag(tag_id, &[other]).unwrap();
    fs::write(root.join(paths::LIBRARY_PLUS_DB), one.to_encrypted().unwrap()).unwrap();
    let differences = tags::read(&d).unwrap().differences;
    assert!(differences.contains(&format!("{tag}: 1 track(s) tagged only in exportExt.pdb")), "{differences:?}");

    let opts = ConvertOptions { to: Target::DeviceLibrary, replace: true };
    let plan = convert::plan(&d, &opts, &Ctx::silent()).unwrap();
    assert!(plan.warnings.iter().any(|w| w.contains("2 My Tag link(s)")), "{:?}", plan.warnings);
    convert::apply(&plan, &mut d, &opts, &Ctx::silent()).unwrap();
    let held = ext_held(&root);
    let want: BTreeSet<String> = locallibrary::pdb::read_library(&root.join(paths::EXPORT_PDB))
        .unwrap()
        .tracks
        .iter()
        .map(|t| t.file_path.clone())
        .collect();
    assert!(!want.contains(&lost_file));
    assert_eq!(find(&held, &tag).unwrap().2, want, "every track still there keeps its tag");
}

/// An exportExt.pdb that reads but whose link table can't take a row (its table directory gives
/// the file header as its next free page): an edit linking tracks is planned, then refused when
/// applied, as exportExt.pdb's, with no backup made and the drive as it was.
#[hegel::test(test_cases = 3, suppress_health_check = [HealthCheck::TooSlow])]
fn an_edit_exportext_pdb_cant_take_leaves_the_drive_alone(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let root = common::demo_drive(dir.path(), "Main");
    let path = root.join(paths::EXPORT_EXT_PDB);
    let mut bytes = fs::read(&path).unwrap();
    let tables = u32::from_le_bytes(bytes[8..12].try_into().unwrap()) as usize;
    let links = (0..tables).map(|i| 0x1c + i * 16).find(|&o| bytes[o..o + 4] == [4, 0, 0, 0]).unwrap();
    bytes[links + 4..links + 8].fill(0);
    fs::write(&path, bytes).unwrap();
    let tag = tc.draw(gs::sampled_from(vec!["Genre/Techno", "Genre/New One", "Situation/Ünï"])).to_string();
    let tracks: Vec<u32> = [1, 2].into_iter().filter(|_| tc.draw(gs::booleans())).collect();
    let tracks = if tracks.is_empty() { vec![2] } else { tracks };
    let before = common::Before::take(&root);
    let mut d = Device::open_export(&root).unwrap();
    let plan = tags::plan(&d, &Edit::Add { tag, tracks }).unwrap();
    let err = tags::apply(&plan, &mut d, &Ctx::silent()).unwrap_err();
    assert!(err.code() == "pdb" && err.to_string().starts_with("exportExt.pdb: "), "{err}");
    before.untouched(&root).unwrap();
}
