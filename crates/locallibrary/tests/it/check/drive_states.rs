//! The check's items for drives that differ from the first one, and for drives whose databases or
//! files aren't all there.

use crate::common;

use std::fs;
use std::path::Path;

use locallibrary::check::{self, CheckOptions, CheckReport, Level};
use locallibrary::device::Device;
use locallibrary::paths;
use locallibrary::progress::Ctx;
use locallibrary::sync::{self, SyncOptions};

fn check(roots: &[&Path]) -> CheckReport {
    let devices: Vec<Device> = roots.iter().map(|r| Device::open(r).unwrap()).collect();
    check::check(&devices, None, &CheckOptions::default(), &Ctx::silent()).unwrap()
}

/// The level and message of a drive's item for `check` (the worst, when there are several).
fn item(r: &CheckReport, drive: &str, check: &str) -> (Level, String) {
    let i = r.items.iter().filter(|i| i.drive == drive && i.check == check).max_by_key(|i| i.level);
    let i = i.unwrap_or_else(|| panic!("no {check} item for {drive}: {r:#?}"));
    (i.level, i.message.clone())
}

#[test]
fn every_drive_is_compared_with_the_first() {
    let dir = tempfile::tempdir().unwrap();
    let mut a = Device::open_export(common::demo_drive(dir.path(), "A")).unwrap();
    fs::create_dir(dir.path().join("B")).unwrap();
    let mut b = Device::open(dir.path().join("B")).unwrap();
    sync::sync_drive_to_drive(&mut a, &mut b, &SyncOptions::default(), &Ctx::silent()).unwrap();
    let other = common::bigger_drive(dir.path(), "Other");

    common::export_new_track(b.root(), "New", "On B");
    let r = check(&[a.root(), b.root(), &other]);
    let (level, message) = item(&r, "B", "same_library");
    assert_eq!(level, Level::Problem);
    assert_eq!(message, "newer than A (1 track(s) and 0 playlist(s) differ); sync the drives");
    assert_eq!(item(&r, "Other", "same_library"), (Level::Problem, "holds a different library from A".into()));

    common::export_new_track(a.root(), "New", "On A");
    let (level, message) = item(&check(&[a.root(), b.root()]), "B", "same_library");
    assert_eq!(level, Level::Problem);
    assert!(message.starts_with("B and A have both changed ("), "{message}");
}

#[test]
fn a_library_that_cant_be_read_is_a_problem() {
    let dir = tempfile::tempdir().unwrap();
    let root = common::demo_drive(dir.path(), "A");
    fs::write(root.join(paths::EXPORT_PDB), b"not a database").unwrap();
    let (level, message) = item(&check(&[&root]), "A", "export");
    assert_eq!(level, Level::Problem);
    assert!(message.starts_with("the library can't be read: "), "{message}");
}

#[test]
fn onelibrary_alone_or_unreadable_is_said() {
    let dir = tempfile::tempdir().unwrap();
    let only = common::bigger_drive(dir.path(), "Only");
    common::onelibrary_only(&only);
    let (level, message) = item(&check(&[&only]), "Only", "databases");
    assert_eq!((level, message.as_str()), (Level::Ok, "OneLibrary only (older players need export.pdb)"));

    let broken = common::bigger_drive(dir.path(), "Broken");
    common::add_onelibrary(&broken);
    fs::write(broken.join(paths::LIBRARY_PLUS_DB), b"not a database").unwrap();
    let (level, message) = item(&check(&[&broken]), "Broken", "databases");
    assert_eq!(level, Level::Problem);
    assert!(message.starts_with("OneLibrary can't be read: "), "{message}");
}

#[test]
fn artwork_missing_on_the_drive_is_a_warning() {
    let dir = tempfile::tempdir().unwrap();
    let root = common::demo_drive(dir.path(), "A");
    common::media::set_art(&root, &[1]);
    fs::remove_dir_all(root.join("PIONEER/Artwork")).unwrap();

    let (level, message) = item(&check(&[&root]), "A", "files");
    assert_eq!(level, Level::Warning);
    let track = Device::open(&root).unwrap().library().unwrap().track(1).unwrap().display_name();
    let want = format!(
        "1 track(s) have no artwork image: {track}; `locallibrary art auto \"A\"` fills in art their files embed"
    );
    assert_eq!(message, want);
}

/// A file FAT32 can't hold is a warning while no drive in the check is FAT32. The file is sparse,
/// so it takes no space.
#[test]
fn a_file_of_4_gb_is_a_warning() {
    let dir = tempfile::tempdir().unwrap();
    let root = common::demo_drive(dir.path(), "A");
    locallibrary::testing::sparse(&root.join("Contents/long mix.wav"), 1 << 32).unwrap();
    let (level, message) = item(&check(&[&root]), "A", "large_files");
    assert_eq!(level, Level::Warning);
    assert_eq!(message, "1 file(s) of 4 GB or more won't fit on a FAT32 drive: Contents/long mix.wav");
}
