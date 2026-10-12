//! How a drive's two databases differ, read from the drive or from export.pdb's library handed
//! in: the same answer either way, none unless the drive has both and export.pdb reads, and an
//! error in place of the comparison when OneLibrary can't be read.

use crate::common;

use std::fs;

use hegel::generators as gs;
use hegel::{HealthCheck, TestCase};
use locallibrary::device::Device;
use locallibrary::paths;
use rekordbox_pdb::PdbEditor;

#[hegel::test(test_cases = 4, suppress_health_check = [HealthCheck::TooSlow])]
#[hegel::explicit_test_case(layout = 0u8, drifted = true, damaged = 0u8)]
#[hegel::explicit_test_case(layout = 0u8, drifted = false, damaged = 1u8)]
#[hegel::explicit_test_case(layout = 0u8, drifted = true, damaged = 2u8)]
#[hegel::explicit_test_case(layout = 1u8, drifted = false, damaged = 0u8)]
fn drift_is_the_same_read_or_handed_in(tc: TestCase) {
    // Both databases, export.pdb alone, OneLibrary alone.
    let layout = tc.draw(gs::integers::<u8>().max_value(2));
    // export.pdb gains a playlist OneLibrary doesn't have.
    let drifted = tc.draw(gs::booleans());
    // Nothing, export.pdb, or OneLibrary can't be read.
    let damaged = tc.draw(gs::integers::<u8>().max_value(2));
    let dir = tempfile::tempdir().unwrap();
    let root = common::bigger_drive(dir.path(), "Main");
    match layout {
        0 => common::add_onelibrary(&root),
        2 => common::onelibrary_only(&root),
        _ => {}
    }
    let pdb_path = root.join(paths::EXPORT_PDB);
    if drifted && pdb_path.is_file() {
        let mut ed = PdbEditor::from_file(&pdb_path).unwrap();
        ed.create_playlist("Only in export.pdb", 0, false).unwrap();
        ed.save(&pdb_path).unwrap();
    }
    let lib = pdb_path.is_file().then(|| common::library(&root));
    let plus_path = root.join(paths::LIBRARY_PLUS_DB);
    match damaged {
        1 if pdb_path.is_file() => fs::write(&pdb_path, b"not a database").unwrap(),
        2 if plus_path.is_file() => fs::write(&plus_path, b"not a database").unwrap(),
        _ => {}
    }
    let d = Device::open_export(&root).unwrap();
    let read = d.library_drift(None);
    let handed = lib.as_ref().and_then(|l| d.library_drift(Some(l)));
    let show = |x: &Option<locallibrary::device::LibraryDrift>| format!("{x:?}");
    match (layout, damaged) {
        // OneLibrary that can't be read: nothing compared, and saying why.
        (0, 2) => {
            assert_eq!(show(&read), show(&handed));
            let drift = read.unwrap();
            assert!(
                drift.error.is_some() && drift.only_in_device_library.is_empty() && drift.playlists.is_empty(),
                "{drift:?}"
            );
        }
        (0, 0) => {
            assert_eq!(show(&read), show(&handed));
            let drift = read.unwrap();
            assert!(drift.error.is_none());
            assert_eq!(drift.playlists.iter().any(|p| p.contains("Only in export.pdb")), drifted, "{drift:?}");
            assert_eq!(drift.is_empty(), !drifted);
        }
        // What's handed in stands for export.pdb, which needn't read.
        (0, _) => {
            assert!(read.is_none());
            assert_eq!(handed.map(|h| h.is_empty()), Some(!drifted));
        }
        _ => assert!(read.is_none() && handed.is_none()),
    }
}
