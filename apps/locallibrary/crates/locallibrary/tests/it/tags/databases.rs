//! Which of a drive's databases My Tags are read from when one of them is missing, unreadable or
//! holds rows that point nowhere.

use crate::common;

use std::fs;

use locallibrary::device::Device;
use locallibrary::tags::{self, Source};
use locallibrary::{Error, paths};
use rekordbox_pdb::ExtEditor;

#[test]
fn exportext_pdb_without_export_pdb_is_left_alone() {
    let dir = tempfile::tempdir().unwrap();
    let root = common::demo_drive(dir.path(), "Main");
    common::add_onelibrary(&root);
    fs::remove_file(root.join(paths::EXPORT_PDB)).unwrap();

    let read = tags::read(&Device::open_export(&root).unwrap()).unwrap();
    assert_eq!(read.sources, [Source::OneLibrary]);
    let warning = "exportExt.pdb is left alone: its tags point at export.pdb's tracks, and the drive has no export.pdb";
    assert!(read.warnings.iter().any(|w| w == warning), "{:?}", read.warnings);
}

#[test]
fn an_unreadable_exportext_pdb_leaves_onelibrarys_tags_or_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let root = common::demo_drive(dir.path(), "Main");
    fs::write(root.join(paths::EXPORT_EXT_PDB), b"not a database").unwrap();
    let err = tags::read(&Device::open_export(&root).unwrap()).unwrap_err();
    assert!(matches!(err, Error::Parse { .. }), "{err}");

    common::add_onelibrary(&root);
    let read = tags::read(&Device::open_export(&root).unwrap()).unwrap();
    assert_eq!(read.sources, [Source::OneLibrary]);
    assert!(
        read.warnings.iter().any(|w| w.ends_with(". Only exportLibrary.db's My Tags are read and edited")),
        "{:?}",
        read.warnings
    );
}

#[test]
fn links_to_tracks_that_arent_there_are_counted_and_left_alone() {
    let dir = tempfile::tempdir().unwrap();
    let root = common::demo_drive(dir.path(), "Main");
    let ext_path = root.join(paths::EXPORT_EXT_PDB);
    let mut ed = ExtEditor::from_file(&ext_path).unwrap();
    let (tag, _) = ed.tags(1)[0];
    assert!(ed.link(tag, 999).unwrap());
    ed.save(&ext_path).unwrap();

    let read = tags::read(&Device::open_export(&root).unwrap()).unwrap();
    let warning = "exportExt.pdb: 1 My Tag row(s) belong to no category, or point at a tag or track that isn't there; \
                   they're left alone";
    assert!(read.warnings.iter().any(|w| w == warning), "{:?}", read.warnings);
    assert!(read.categories.iter().flat_map(|c| &c.tags).all(|t| t.tracks.is_empty()));
}
