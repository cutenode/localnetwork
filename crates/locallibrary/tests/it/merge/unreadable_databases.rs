//! Merges between drives whose My Tag or OneLibrary databases can't be read: the tracks still
//! come across, and what's left out is said.

use crate::common::{self, Warnings};

use std::fs;

use locallibrary::device::Device;
use locallibrary::merge::{self, MergeOptions};
use locallibrary::paths;
use locallibrary::progress::Ctx;

#[test]
fn unreadable_tag_and_onelibrary_databases_are_left_out_with_warnings() {
    let dir = tempfile::tempdir().unwrap();
    let src_root = common::demo_drive(dir.path(), "Guest");
    common::add_onelibrary(&src_root);
    fs::write(src_root.join(paths::EXPORT_EXT_PDB), b"not a database").unwrap();
    fs::write(src_root.join(paths::LIBRARY_PLUS_DB), b"not a database").unwrap();
    let dst_root = common::bigger_drive(dir.path(), "Main");
    common::add_onelibrary(&dst_root);
    fs::write(dst_root.join(paths::EXPORT_EXT_PDB), b"not a database").unwrap();
    fs::write(dst_root.join(paths::LIBRARY_PLUS_DB), b"not a database either").unwrap();
    let (src, mut dst) = (Device::open_export(&src_root).unwrap(), Device::open_export(&dst_root).unwrap());

    let plan = merge::plan(&src, &dst, &MergeOptions::default(), &Ctx::silent()).unwrap();
    let left_out = plan.warnings.iter().filter(|w| w.ends_with("; My Tags there are left out of the merge")).count();
    assert!(left_out >= 2, "{:?}", plan.warnings);

    let warnings = Warnings::default();
    let report = merge::apply(&plan, &src, &mut dst, &MergeOptions::default(), &warnings.ctx()).unwrap();
    assert_eq!(report.tracks_added, 2);
    let warnings = warnings.said();
    assert!(warnings.iter().any(|w| w.ends_with("; the merged tracks' My Tags aren't added to it")), "{warnings:?}");
    assert!(warnings.iter().any(|w| w.contains("Only export.pdb will be changed")), "{warnings:?}");
    assert_eq!(dst.library().unwrap().tracks.len(), 14);
    assert_eq!(fs::read(dst_root.join(paths::EXPORT_EXT_PDB)).unwrap(), b"not a database");
    assert_eq!(fs::read(dst_root.join(paths::LIBRARY_PLUS_DB)).unwrap(), b"not a database either");
}
