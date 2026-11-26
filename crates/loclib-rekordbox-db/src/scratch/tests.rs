use super::*;

/// The folders in `root`, by name.
fn names(root: &Path) -> Vec<String> {
    let mut names: Vec<String> =
        std::fs::read_dir(root).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    names.sort();
    names
}

/// A folder is its owner's alone, and goes with what's in it when dropped.
#[test]
fn a_folder_is_private_and_removed_when_dropped() {
    let root = tempfile::tempdir().unwrap();
    let folder = folder_in(root.path()).unwrap();
    let path = folder.path().to_path_buf();
    assert!(path.file_name().unwrap().to_string_lossy().starts_with(PREFIX));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o700);
    }
    std::fs::write(path.join("master.db"), b"decrypted").unwrap();
    drop(folder);
    assert!(!path.exists());
    assert_eq!(names(root.path()), Vec::<String>::new());
}

/// A folder shows where it is when debugged.
#[test]
fn a_folder_debugs_as_its_path() {
    let root = tempfile::tempdir().unwrap();
    let folder = folder_in(root.path()).unwrap();
    assert_eq!(format!("{folder:?}"), format!("Folder({:?})", folder.path()));
}

/// A sweep leaves folders in use (their lock held) and young ones without a lock, and removes
/// abandoned ones (a lock nobody holds, or none and old).
#[test]
fn a_sweep_removes_only_abandoned_folders() {
    let root = tempfile::tempdir().unwrap();
    let held = folder_in(root.path()).unwrap();
    // A process that ended: its lock file is there, and nobody holds it.
    let left = root.path().join(format!("{PREFIX}left"));
    std::fs::create_dir(&left).unwrap();
    std::fs::write(left.join(LOCK), b"").unwrap();
    std::fs::write(left.join("master.db"), b"decrypted").unwrap();
    // One being made: no lock file yet.
    let making = root.path().join(format!("{PREFIX}making"));
    std::fs::create_dir(&making).unwrap();
    let removed = sweep_in(root.path(), GRACE);
    assert_eq!(removed, [left]);
    assert!(held.path().is_dir() && making.is_dir());
    // Once old enough, one without a lock is taken as abandoned too; one held never is.
    assert_eq!(sweep_in(root.path(), Duration::ZERO), [making]);
    assert!(held.path().join(LOCK).is_file());
}

/// Folders earlier versions decrypted `master.db` into are removed once old enough; any other
/// `.tmp` folder, or one with more in it, stays.
#[test]
fn a_sweep_removes_copies_earlier_versions_left() {
    let root = tempfile::tempdir().unwrap();
    let folder = |name: &str, files: &[&str]| {
        let dir = root.path().join(name);
        std::fs::create_dir(&dir).unwrap();
        for f in files {
            std::fs::write(dir.join(f), b"x").unwrap();
        }
        dir
    };
    let read = folder(".tmpAAAAAA", &["master.db"]);
    let killed = folder(".tmpBBBBBB", &["master.db.enc", "master.db-wal"]);
    let exported = folder(".tmpCCCCCC", &["master.db", "export.db"]);
    folder(".tmpDDDDDD", &["master.db", "notes.txt"]);
    folder(".tmpEEEEEE", &["export.db"]);
    folder(".tmpFFFFFF", &[]);
    folder("rekordbox", &["master.db"]);
    std::fs::create_dir(root.path().join(".tmpGGGGGG")).unwrap();
    std::fs::create_dir(root.path().join(".tmpGGGGGG").join("master.db")).unwrap();
    assert_eq!(sweep_in(root.path(), GRACE), Vec::<PathBuf>::new(), "too new");
    let mut removed = sweep_in(root.path(), Duration::ZERO);
    removed.sort();
    assert_eq!(removed, [read, killed, exported]);
    assert_eq!(names(root.path()), [".tmpDDDDDD", ".tmpEEEEEE", ".tmpFFFFFF", ".tmpGGGGGG", "rekordbox"]);
}

/// A link named like a folder is never followed, nor removed.
#[cfg(unix)]
#[test]
fn a_sweep_follows_no_links() {
    let root = tempfile::tempdir().unwrap();
    let elsewhere = tempfile::tempdir().unwrap();
    std::fs::write(elsewhere.path().join(LOCK), b"").unwrap();
    std::fs::write(elsewhere.path().join("master.db"), b"x").unwrap();
    std::os::unix::fs::symlink(elsewhere.path(), root.path().join(format!("{PREFIX}link"))).unwrap();
    std::os::unix::fs::symlink(elsewhere.path(), root.path().join(".tmpLINKED")).unwrap();
    assert_eq!(sweep_in(root.path(), Duration::ZERO), Vec::<PathBuf>::new());
    assert_eq!(names(elsewhere.path()), [LOCK, "master.db"]);
}

/// A folder whose lock file can't be opened (someone else's, or not a file) is left as it is.
#[cfg(unix)]
#[test]
fn a_sweep_leaves_a_folder_whose_lock_cant_be_opened() {
    let root = tempfile::tempdir().unwrap();
    let other = root.path().join(format!("{PREFIX}other"));
    std::fs::create_dir(&other).unwrap();
    // A link to itself can't be opened, whoever runs the test (root can open any file).
    std::os::unix::fs::symlink(other.join(LOCK), other.join(LOCK)).unwrap();
    assert_eq!(sweep_in(root.path(), Duration::ZERO), Vec::<PathBuf>::new());
    assert!(other.is_dir());
}
