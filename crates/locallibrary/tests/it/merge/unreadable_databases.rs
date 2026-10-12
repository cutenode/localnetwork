//! Merges between drives whose My Tag or OneLibrary databases can't be read: the tracks still
//! come across, and what's left out is said.

use crate::common::{self, Warnings};

use std::fs;

use hegel::generators as gs;
use hegel::{HealthCheck, TestCase};
use locallibrary::device::Device;
use locallibrary::merge::{self, MergeOptions};
use locallibrary::onelibrary::OneLibrary;
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

/// The drive at `root`'s OneLibrary database without `table`.
fn drop_onelibrary_table(root: &std::path::Path, table: &str) {
    let one = OneLibrary::open_root(root).unwrap();
    one.connection().execute_batch(&format!("DROP TABLE \"{table}\"")).unwrap();
    fs::write(root.join(paths::LIBRARY_PLUS_DB), one.to_encrypted().unwrap()).unwrap();
}

/// The tables of a OneLibrary database as `common::add_onelibrary` makes it.
fn onelibrary_tables() -> Vec<String> {
    let one = OneLibrary::create().unwrap();
    let mut stmt = one
        .connection()
        .prepare("SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%'")
        .unwrap();
    stmt.query_map([], |r| r.get(0)).unwrap().collect::<rusqlite::Result<_>>().unwrap()
}

/// A merge planned from a source whose OneLibrary database lacks any one table (a database
/// someone else made) applies into a target with OneLibrary: whatever planning said the source's
/// OneLibrary can't give (its My Tags, or its rows, which are then built from export.pdb's) is
/// left out, and both of the target's databases take the tracks.
#[hegel::test(test_cases = 8, suppress_health_check = [HealthCheck::TooSlow])]
#[hegel::explicit_test_case(table = "myTag_content".to_string())]
#[hegel::explicit_test_case(table = "genre".to_string())]
#[hegel::explicit_test_case(table = "cue".to_string())]
fn a_source_onelibrary_lacking_a_table_still_merges(tc: TestCase) {
    let table = tc.draw(gs::sampled_from(onelibrary_tables()));
    let dir = tempfile::tempdir().unwrap();
    let src_root = common::demo_drive(dir.path(), "Guest");
    common::add_onelibrary(&src_root);
    drop_onelibrary_table(&src_root, &table);
    let dst_root = common::bigger_drive(dir.path(), "Main");
    common::add_onelibrary(&dst_root);
    let (src, mut dst) = (Device::open_export(&src_root).unwrap(), Device::open_export(&dst_root).unwrap());
    let opts = MergeOptions { history: true, ..Default::default() };
    let plan = merge::plan(&src, &dst, &opts, &Ctx::silent()).unwrap();
    let warnings = Warnings::default();
    let report = merge::apply(&plan, &src, &mut dst, &opts, &warnings.ctx()).unwrap();
    assert_eq!(report.tracks_added, 2);
    // Only a row the source can't give without its cues is built from export.pdb's.
    let said = warnings.said();
    let made = said.iter().filter(|w| w.ends_with("its OneLibrary row is made from export.pdb's")).count();
    assert_eq!(made, if table == "cue" { 2 } else { 0 }, "{said:?}");
    assert_eq!(dst.library().unwrap().tracks.len(), 14);
    assert_eq!(common::plus_library(&dst_root).tracks.len(), 14);
}

/// A target whose OneLibrary database lacks a My Tag table is planned with a warning that its My
/// Tags are left out, and applying does that: the tracks still go into it, without their tags.
#[hegel::test(test_cases = 2, suppress_health_check = [HealthCheck::TooSlow])]
fn a_target_onelibrary_without_my_tags_takes_the_tracks(tc: TestCase) {
    let table = tc.draw(gs::sampled_from(vec!["myTag", "myTag_content"]));
    let dir = tempfile::tempdir().unwrap();
    let src_root = common::demo_drive(dir.path(), "Guest");
    common::add_onelibrary(&src_root);
    let dst_root = common::bigger_drive(dir.path(), "Main");
    common::add_onelibrary(&dst_root);
    drop_onelibrary_table(&dst_root, table);
    let (src, mut dst) = (Device::open_export(&src_root).unwrap(), Device::open_export(&dst_root).unwrap());
    let plan = merge::plan(&src, &dst, &MergeOptions::default(), &Ctx::silent()).unwrap();
    let left_out = format!("no such table: {table}; My Tags there are left out of the merge");
    assert!(plan.warnings.iter().any(|w| w.ends_with(&left_out)), "{:?}", plan.warnings);
    let warnings = Warnings::default();
    let report = merge::apply(&plan, &src, &mut dst, &MergeOptions::default(), &warnings.ctx()).unwrap();
    assert_eq!(report.tracks_added, 2);
    let said = warnings.said();
    assert!(said.iter().any(|w| w.ends_with("; the merged tracks' My Tags aren't added to it")), "{said:?}");
    assert_eq!(common::plus_library(&dst_root).tracks.len(), 14);
}

/// A target OneLibrary without its `property` table (where it keeps a count of its tracks) plans
/// cleanly, so the merge goes through too: the tracks land, and only the count isn't kept.
#[test]
fn a_target_onelibrary_without_its_property_table_takes_the_tracks() {
    let dir = tempfile::tempdir().unwrap();
    let src_root = common::demo_drive(dir.path(), "Guest");
    let dst_root = common::bigger_drive(dir.path(), "Main");
    common::add_onelibrary(&dst_root);
    drop_onelibrary_table(&dst_root, "property");
    let (src, mut dst) = (Device::open_export(&src_root).unwrap(), Device::open_export(&dst_root).unwrap());
    let plan = merge::plan(&src, &dst, &MergeOptions::default(), &Ctx::silent()).unwrap();
    let report = merge::apply(&plan, &src, &mut dst, &MergeOptions::default(), &Ctx::silent()).unwrap();
    assert_eq!(report.tracks_added, 2);
    assert_eq!(common::plus_library(&dst_root).tracks.len(), 14);
}
