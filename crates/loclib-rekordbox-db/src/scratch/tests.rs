use super::*;

/// The folders in `root`, by name.
fn names(root: &Path) -> Vec<String> {
    let mut names: Vec<String> =
        std::fs::read_dir(root).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    names.sort();
    names
}

/// A folder named `name` in `root`, made as a [`Folder`] is (its owner's alone), holding `files`.
fn made(root: &Path, name: &str, files: &[&str]) -> PathBuf {
    let dir = root.join(name);
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut std::fs::DirBuilder::new(), 0o700).create(&dir).unwrap();
    #[cfg(not(unix))]
    std::fs::create_dir(&dir).unwrap();
    for f in files {
        std::fs::write(dir.join(f), b"x").unwrap();
    }
    dir
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
    let left = made(root.path(), &format!("{PREFIX}LEFT01"), &[LOCK, "master.db"]);
    // One being made: no lock file yet.
    let making = made(root.path(), &format!("{PREFIX}MAKE01"), &[]);
    let removed = sweep_in(root.path(), GRACE);
    assert_eq!(removed, [left]);
    assert!(held.path().is_dir() && making.is_dir());
    // Once old enough, one without a lock is taken as abandoned too; one held never is.
    assert_eq!(sweep_in(root.path(), Duration::ZERO), [making]);
    assert!(held.path().join(LOCK).is_file());
}

/// Folders made while other sweeps run are never taken as abandoned: their lock is held
/// before it can be found.
#[test]
fn a_sweep_never_removes_a_folder_being_made() {
    let root = tempfile::tempdir().unwrap();
    std::thread::scope(|s| {
        let sweeper = s.spawn(|| {
            let mut removed = Vec::new();
            for _ in 0..500 {
                removed.extend(sweep_in(root.path(), GRACE));
            }
            removed
        });
        // All kept: one being dropped lets go of its lock before it's removed.
        let folders: Vec<Folder> = (0..200)
            .map(|_| {
                let folder = folder_in(root.path()).unwrap();
                std::fs::write(folder.path().join("exportLibrary.db"), b"decrypted").unwrap();
                folder
            })
            .collect();
        assert_eq!(sweeper.join().unwrap(), Vec::<PathBuf>::new());
        assert!(folders.iter().all(|f| f.path().join(LOCK).is_file()));
    });
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
    std::os::unix::fs::symlink(elsewhere.path(), root.path().join(format!("{PREFIX}LINK01"))).unwrap();
    std::os::unix::fs::symlink(elsewhere.path(), root.path().join(".tmpLINKED")).unwrap();
    assert_eq!(sweep_in(root.path(), Duration::ZERO), Vec::<PathBuf>::new());
    assert_eq!(names(elsewhere.path()), [LOCK, "master.db"]);
}

/// A folder whose lock file can't be opened (someone else's, or not a file) is left as it is.
#[cfg(unix)]
#[test]
fn a_sweep_leaves_a_folder_whose_lock_cant_be_opened() {
    let root = tempfile::tempdir().unwrap();
    let other = made(root.path(), &format!("{PREFIX}OTHER1"), &[]);
    // A link to itself can't be opened, whoever runs the test (root can open any file).
    std::os::unix::fs::symlink(other.join(LOCK), other.join(LOCK)).unwrap();
    assert_eq!(sweep_in(root.path(), Duration::ZERO), Vec::<PathBuf>::new());
    assert!(other.is_dir());
}

/// Only folders named as `tempfile` names them, this user's own and private, are looked into:
/// another user on a shared `/tmp` can make one of any name and mode.
#[cfg(unix)]
#[test]
fn a_sweep_looks_only_into_this_users_private_folders() {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    let root = tempfile::tempdir().unwrap();
    let open = made(root.path(), &format!("{PREFIX}OPEN01"), &[LOCK, "master.db"]);
    std::fs::set_permissions(&open, std::fs::Permissions::from_mode(0o755)).unwrap();
    made(root.path(), &format!("{PREFIX}toolong"), &[LOCK]);
    made(root.path(), &format!("{PREFIX}sh0rt"), &[LOCK]);
    made(root.path(), &format!("{PREFIX}not-it"), &[LOCK]);
    made(root.path(), &format!("{PREFIX}KEEP"), &[]);
    assert_eq!(sweep_in(root.path(), Duration::ZERO), Vec::<PathBuf>::new());
    assert_eq!(names(&open), [LOCK, "master.db"]);
    // Whose it is, for a user this test can't be.
    let meta = std::fs::symlink_metadata(made(root.path(), "mine", &[])).unwrap();
    assert!(owned_by(&meta, meta.uid(), true));
    assert!(!owned_by(&meta, meta.uid() + 1, true));
    assert!(!owned_by(&meta, meta.uid() + 1, false));
    let meta = std::fs::symlink_metadata(&open).unwrap();
    assert!(!owned_by(&meta, meta.uid(), true), "not private");
    assert!(owned_by(&meta, meta.uid(), false), "an earlier version's folder needn't be");
}

/// A lock file that's a FIFO (opening one waits for a writer) or a link is left as it is, and
/// the sweep doesn't wait on it.
#[cfg(unix)]
#[test]
fn a_sweep_never_waits_on_or_follows_a_lock_file() {
    let root = tempfile::tempdir().unwrap();
    let fifo = made(root.path(), &format!("{PREFIX}FIFO01"), &["master.db"]);
    let made_fifo = std::process::Command::new("mkfifo").arg(fifo.join(LOCK)).status().unwrap();
    assert!(made_fifo.success());
    let elsewhere = tempfile::tempdir().unwrap();
    std::fs::write(elsewhere.path().join("lock"), b"").unwrap();
    let linked = made(root.path(), &format!("{PREFIX}LINK02"), &["master.db"]);
    std::os::unix::fs::symlink(elsewhere.path().join("lock"), linked.join(LOCK)).unwrap();
    let path = root.path().to_path_buf();
    let (done, removed) = std::sync::mpsc::channel();
    std::thread::spawn(move || done.send(sweep_in(&path, Duration::ZERO)));
    let removed = removed.recv_timeout(Duration::from_secs(10)).expect("the sweep waited on the FIFO");
    assert_eq!(removed, Vec::<PathBuf>::new());
    assert!(fifo.join("master.db").is_file() && linked.join("master.db").is_file());
}

/// A folder without a lock file, however old, is removed only when it holds nothing but what
/// one of ours can: anything else in it is someone's.
#[test]
fn a_sweep_removes_a_lockless_folder_only_when_it_holds_what_ours_can() {
    let root = tempfile::tempdir().unwrap();
    let killed = made(root.path(), &format!("{PREFIX}KILL01"), &[MAKING]);
    let dropped = made(root.path(), &format!("{PREFIX}DROP01"), &["master.db", "master.db-journal", "export.db"]);
    let vacuumed = made(root.path(), &format!("{PREFIX}VACU01"), &["export.db", "export.db-journal"]);
    let editing = made(root.path(), &format!("{PREFIX}EDIT01"), &["exportLibrary.db"]);
    made(root.path(), &format!("{PREFIX}NOTE01"), &["master.db", "notes.txt"]);
    let nested = made(root.path(), &format!("{PREFIX}NEST01"), &["master.db"]);
    std::fs::create_dir(nested.join("kept")).unwrap();
    #[cfg(unix)]
    {
        let linked = made(root.path(), &format!("{PREFIX}LINK03"), &[]);
        std::os::unix::fs::symlink(root.path().join("elsewhere"), linked.join("master.db")).unwrap();
    }
    let mut removed = sweep_in(root.path(), Duration::ZERO);
    removed.sort();
    assert_eq!(removed, [dropped, editing, killed, vacuumed]);
    let mut kept = vec![format!("{PREFIX}NEST01"), format!("{PREFIX}NOTE01")];
    if cfg!(unix) {
        kept.insert(0, format!("{PREFIX}LINK03"));
    }
    assert_eq!(names(root.path()), kept);
}

/// What earlier versions left of each copy they made: a read, a plain one SQLite replayed,
/// an export, a drive's database being edited and its hash read. A name none of them made, or a
/// folder without a copy, stays.
#[test]
fn a_sweep_removes_every_shape_earlier_versions_left() {
    let root = tempfile::tempdir().unwrap();
    let mut left = [
        made(root.path(), ".tmpREAD01", &["master.db.enc", "master.db"]),
        made(root.path(), ".tmpPLAIN1", &["master.db", "master.db-wal", "master.db-shm"]),
        made(root.path(), ".tmpEXPO01", &["master.db", "master.db-journal", "export.db"]),
        made(root.path(), ".tmpVACU01", &["master.db", "export.db", "export.db-journal"]),
        made(root.path(), ".tmpEDIT01", &["exportLibrary.db"]),
        made(root.path(), ".tmpHASH01", &["hash.db"]),
    ];
    left.sort();
    made(root.path(), ".tmpJOUR01", &["exportLibrary.db", "exportLibrary.db-journal"]);
    made(root.path(), ".tmpSHM001", &["master.db-shm", "master.db-wal"]);
    made(root.path(), ".tmpLONGER1", &["master.db"]);
    // A link inside one, where its copy would be, isn't a copy (Windows needs a privilege to
    // make links).
    #[cfg(unix)]
    {
        let linked = made(root.path(), ".tmpLINKIN", &["master.db.enc"]);
        std::os::unix::fs::symlink(root.path().join("elsewhere"), linked.join("master.db")).unwrap();
    }
    let kept: &[&str] = if cfg!(unix) {
        &[".tmpJOUR01", ".tmpLINKIN", ".tmpLONGER1", ".tmpSHM001"]
    } else {
        &[".tmpJOUR01", ".tmpLONGER1", ".tmpSHM001"]
    };
    let mut removed = sweep_in(root.path(), Duration::ZERO);
    removed.sort();
    assert_eq!(removed, left);
    assert_eq!(names(root.path()), kept);
}

/// Without a root of their own, tests' folders go in one folder of the temporary folder for
/// them, never in the temporary folder itself, which their sweep would look into.
#[test]
fn tests_make_folders_outside_the_temporary_folder() {
    let folder = folder().unwrap();
    let parent = folder.path().parent().unwrap();
    assert_eq!(parent, std::env::temp_dir().join("locallibrary-scratch-tests"));
    let root = tempfile::tempdir().unwrap();
    test_root(root.path());
    assert_eq!(folder_in(&super::root()).unwrap().path().parent().unwrap(), root.path());
}

/// What's in a folder goes before its lock does, when it's dropped and when a sweep removes an
/// abandoned one: a process killed in between leaves a lock nobody holds, which the next sweep
/// takes at once, never a folder without one that still holds a copy (kept for an hour). Here
/// something the removal can't take stands in for the moment of the kill.
#[cfg(unix)]
#[test]
fn a_folder_loses_its_lock_only_once_nothing_else_is_in_it() {
    use std::os::unix::fs::PermissionsExt;
    // SAFETY: geteuid has no preconditions; root removes what this test makes unremovable.
    if unsafe { libc::geteuid() } == 0 {
        return;
    }
    let stuck = |dir: &Path, mode: u32| {
        let inner = dir.join("stuck");
        if mode == 0o500 {
            std::fs::create_dir(&inner).unwrap();
            std::fs::write(inner.join("export.db"), b"decrypted").unwrap();
        }
        std::fs::set_permissions(&inner, std::fs::Permissions::from_mode(mode)).unwrap();
    };
    let root = tempfile::tempdir().unwrap();
    let folder = folder_in(root.path()).unwrap();
    let path = folder.path().to_path_buf();
    stuck(&path, 0o500);
    drop(folder);
    assert!(path.join(LOCK).is_file(), "the lock went before what it guards");
    // Abandoned, the same: the sweep can't take it yet, and leaves its lock.
    assert_eq!(sweep_in(root.path(), GRACE), Vec::<PathBuf>::new());
    assert!(path.join(LOCK).is_file(), "the sweep took the lock before what it guards");
    stuck(&path, 0o700);
    assert_eq!(sweep_in(root.path(), GRACE), [path]);
}
