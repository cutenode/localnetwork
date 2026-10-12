use super::*;

#[test]
fn repairing_files_fingerprinted_by_content_changes_the_fingerprint() {
    let problem = |path: &str| FileProblem {
        path: path.into(),
        class: paths::classify(path).unwrap(),
        size: 10,
        expected: None,
        actual: None,
        reference: None,
        mtime: None,
        error: None,
        repair: None,
    };
    assert!(changes_fingerprint(&problem("PIONEER/rekordbox/export.pdb")));
    assert!(changes_fingerprint(&problem("PIONEER/USBANLZ/P000/00000001/ANLZ0000.DAT")));
    assert!(changes_fingerprint(&problem("PIONEER/Artwork/00001/a1.jpg")));
    // Audio goes by its size and path.
    assert!(!changes_fingerprint(&problem("Contents/a.mp3")));
}
