use super::*;

use hegel::TestCase;
use hegel::generators as gs;

/// A track's analysis files are the files beside its `.DAT` that share its stem, never ones in a
/// folder of that name; a key under `USBANLZ/` with no extension names none.
#[hegel::test(test_cases = 20)]
fn prop_analysis_files_are_the_dat_and_its_siblings(tc: TestCase) {
    let stem = tc.draw(gs::sampled_from(vec!["ANLZ0000", "anlz0001", "Track"]));
    let exts: Vec<&str> = tc.draw(gs::vecs(gs::sampled_from(vec!["DAT", "EXT", "2EX"])).max_size(3));
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("PIONEER/USBANLZ/P016/0000A1B2");
    std::fs::create_dir_all(folder.join(format!("{stem}.DAT.d"))).unwrap();
    std::fs::write(folder.join(format!("{stem}.DAT.d/nested.EXT")), b"no").unwrap();
    std::fs::write(folder.join(stem), b"no extension").unwrap();
    for ext in &exts {
        std::fs::write(folder.join(format!("{stem}.{ext}")), b"anlz").unwrap();
    }
    let tree = crate::tree::scan(dir.path(), &Default::default(), &Ctx::silent()).unwrap();
    let base = format!("PIONEER/USBANLZ/P016/0000A1B2/{stem}");
    let mut want: Vec<String> = exts.iter().map(|e| format!("{base}.{e}")).collect();
    want.sort();
    want.dedup();
    let mut got = analysis_files(&format!("/{base}.DAT"), &tree);
    got.sort();
    assert_eq!(got, want);
    assert!(analysis_files(&format!("/{base}"), &tree).is_empty(), "a key with no extension names no files");
}

/// A drive whose folder can't be read tells removing tracks nothing: the scan's error comes back.
#[cfg(unix)]
#[hegel::test(test_cases = 4)]
fn prop_a_drive_that_cant_be_scanned_has_no_removable_files(tc: TestCase) {
    use std::os::unix::fs::PermissionsExt;
    let top = tc.draw(gs::sampled_from(vec!["Contents", "PIONEER"]));
    let dir = tempfile::tempdir().unwrap();
    let locked = dir.path().join(top).join("Locked");
    std::fs::create_dir_all(locked.join("inner")).unwrap();
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();
    let device = Device::open(dir.path()).unwrap();
    let got = removable_files(&device, &[&Library::default()], None, &|_| true, &Ctx::silent());
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).unwrap();
    let e = got.unwrap_err();
    assert_eq!(e.code(), "io", "{e}");
    assert!(e.to_string().contains("Locked"), "{e}");
}
