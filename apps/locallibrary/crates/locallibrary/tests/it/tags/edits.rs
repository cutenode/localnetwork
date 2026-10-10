//! My Tag edits that make a new category, reach only one of a drive's databases, or are refused,
//! and a tag's life through the service layer.

use crate::common::{self, path};

use locallibrary::api::{self, DriveRef, TagAction, TagsEdit, TrackSelector};
use locallibrary::device::Device;
use locallibrary::progress::Ctx;
use locallibrary::tags::{self, Edit, Source};
use locallibrary::{Error, paths};
use rekordbox_pdb::ExtEditor;

fn invalid(d: &Device, edit: Edit) -> String {
    match tags::plan(d, &edit) {
        Err(Error::Invalid(m)) => m,
        other => panic!("not refused as invalid: {other:?}"),
    }
}

#[test]
fn edits_need_tracks_a_name_and_a_free_one() {
    let dir = tempfile::tempdir().unwrap();
    let d = Device::open_export(common::demo_drive(dir.path(), "Main")).unwrap();
    let categories: Vec<String> = tags::read(&d).unwrap().categories.into_iter().map(|c| c.name).collect();
    assert!(categories.len() >= 2, "{categories:?}");

    assert_eq!(invalid(&d, Edit::Add { tag: "Genre/Deep".into(), tracks: vec![] }), "no tracks selected");
    assert_eq!(invalid(&d, Edit::Rename { tag: categories[0].clone(), to: "  ".into() }), "give a new name");
    let taken = invalid(&d, Edit::Rename { tag: categories[0].clone(), to: categories[1].to_uppercase() });
    assert_eq!(taken, format!("exportExt.pdb already has a category named {:?}", categories[1].to_uppercase()));
}

#[test]
fn a_tag_in_a_new_category_makes_the_category() {
    let dir = tempfile::tempdir().unwrap();
    let root = common::demo_drive(dir.path(), "Main");
    // rekordbox's four categories fill exportExt.pdb: one goes to make room.
    let ext_path = root.join(paths::EXPORT_EXT_PDB);
    let mut ed = ExtEditor::from_file(&ext_path).unwrap();
    let (untitled, _) = *ed.categories().last().unwrap();
    ed.remove(untitled).unwrap();
    ed.save(&ext_path).unwrap();
    let mut d = Device::open_export(&root).unwrap();
    let plan = tags::plan(&d, &Edit::Add { tag: "Venue/Club".into(), tracks: vec![1] }).unwrap();
    let created: Vec<&str> = plan.created.iter().map(|c| c.path.as_str()).collect();
    assert_eq!(created, ["Venue", "Venue/Club"]);
    tags::apply(&plan, &mut d, &Ctx::silent()).unwrap();

    let read = tags::read(&d).unwrap();
    let venue = read.categories.iter().find(|c| c.name == "Venue").unwrap();
    let club: Vec<u32> = venue.tags.iter().find(|t| t.name == "Club").unwrap().tracks.iter().map(|t| t.id).collect();
    assert_eq!(club, [1]);
}

#[test]
fn a_track_only_export_pdb_lists_is_tagged_there_with_a_warning() {
    let dir = tempfile::tempdir().unwrap();
    let root = common::demo_drive(dir.path(), "Main");
    common::add_onelibrary(&root);
    let only = common::export_new_track(&root, "Only", "In export.pdb");
    let mut d = Device::open_export(&root).unwrap();

    let plan = tags::plan(&d, &Edit::Add { tag: "Genre/Deep".into(), tracks: vec![1, only] }).unwrap();
    assert_eq!(plan.databases, [Source::ExportExt, Source::OneLibrary]);
    let warning = "1 selected track(s) aren't in exportLibrary.db, so players that read it won't show the tag on them";
    assert!(plan.warnings.iter().any(|w| w == warning), "{:?}", plan.warnings);
    tags::apply(&plan, &mut d, &Ctx::silent()).unwrap();
    let by_track = tags::by_track(&d).unwrap();
    assert_eq!(by_track.values().filter(|t| t.contains(&"Genre/Deep".to_string())).count(), 2);
}

/// A My Tag is added to a track, renamed, taken off and deleted, each planned first.
#[test]
fn a_my_tag_is_added_renamed_and_removed() {
    let dir = tempfile::tempdir().unwrap();
    let root = common::demo_drive(dir.path(), "Main");
    let edit = |action| TagsEdit { drive: path(&root), action };
    let first = || TrackSelector { ids: vec![1], ..Default::default() };
    let tagged = |tag: &str| {
        let tags = api::tags_list(&DriveRef { drive: path(&root) }).unwrap();
        tags.categories.iter().flat_map(|c| &c.tags).find(|t| t.path == tag).map(|t| t.tracks.len())
    };

    let add = edit(TagAction::Add { tag: "Genre/Techno".into(), tracks: first() });
    assert_eq!(api::tags_plan(&add).unwrap().summary, ["add Genre/Techno (1 track(s))"]);
    api::tags_apply(&add, &Ctx::silent()).unwrap();
    assert_eq!(tagged("Genre/Techno"), Some(1));

    let rename = edit(TagAction::Rename { tag: "Genre/Techno".into(), to: "Tech".into() });
    assert_eq!(api::tags_plan(&rename).unwrap().summary, ["rename Genre/Techno to Tech"]);
    api::tags_apply(&rename, &Ctx::silent()).unwrap();
    assert_eq!(tagged("Genre/Tech"), Some(1));

    let remove = edit(TagAction::Remove { tag: "Genre/Tech".into(), tracks: first() });
    assert!(api::tags_plan(&remove).unwrap().destructive);
    assert_eq!(api::tags_apply(&remove, &Ctx::silent()).unwrap().tracks_changed, 1);
    assert_eq!(tagged("Genre/Tech"), Some(0));
    api::tags_apply(&edit(TagAction::Delete { tag: "Genre/Tech".into() }), &Ctx::silent()).unwrap();
    assert_eq!(tagged("Genre/Tech"), None);
}
