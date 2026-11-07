use super::*;
use crate::progress::CancelToken;
use hegel::generators as gs;

/// Every file below `root`, and every folder outside locallibrary's own, with the files' bytes:
/// what a failed commit must leave as it found it.
fn snapshot(root: &Path) -> Vec<(String, Option<Vec<u8>>)> {
    let mut out: Vec<(String, Option<Vec<u8>>)> = walkdir::WalkDir::new(root)
        .min_depth(1)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let key = e.path().strip_prefix(root).unwrap().to_string_lossy().replace('\\', "/");
            match e.file_type().is_file() {
                true => Some((key, Some(std::fs::read(e.path()).unwrap()))),
                false if key.starts_with(paths::STATE_DIR) => None,
                false => Some((key, None)),
            }
        })
        .collect();
    out.sort();
    out
}

/// How one of a drawn commit's new files is made, and whether that fails.
#[derive(Debug, Clone, Copy)]
enum Kind {
    Bytes,
    Made,
    MadeFails,
    Copied,
    CopiedGone,
    CopiedCorrupt,
    /// Made, and stops the commit as it is.
    Stops,
    /// Copied, and stops the commit as it's copied.
    StopsCopying,
}

/// A commit of drawn new files (some failing, some optional), deletes (some of files that aren't
/// there) and a drawn backup either goes through, writing every file that could be made and
/// reporting the optional ones that couldn't, or, when a file it needs can't be made, a path is
/// unsafe, it's stopped, or it fails just before its swap, leaves the drive exactly as it was.
#[hegel::test(test_cases = 25)]
fn a_commit_writes_everything_or_leaves_the_drive_as_it_was(tc: hegel::TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("drive");
    let src = dir.path().join("src.mp3");
    std::fs::write(&src, b"copied audio").unwrap();
    for (key, bytes) in [("Contents/old.mp3", &b"old"[..]), ("Contents/gone.mp3", b"gone"), ("Contents/blocker", b"x")]
    {
        let p = paths::to_fs(&root, key);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, bytes).unwrap();
    }
    // At times the drive has been written before: it has a state and backups to put back.
    if tc.draw(gs::booleans()) {
        let mut device = Device::open(&root).unwrap();
        let mut opts = Options::new(LineageOp::Edit, "an earlier commit").backup("before an earlier commit");
        opts.lineage = false;
        let changes =
            Changes { files: vec![NewFile::bytes("Contents/prior.mp3", b"prior".to_vec())], ..Default::default() };
        commit(&mut device, changes, opts, &Ctx::silent()).unwrap();
    }
    let kinds = [
        Kind::Bytes,
        Kind::Made,
        Kind::MadeFails,
        Kind::Copied,
        Kind::CopiedGone,
        Kind::CopiedCorrupt,
        Kind::Stops,
        Kind::StopsCopying,
    ];
    let stopping: std::sync::Arc<std::sync::Mutex<Vec<String>>> = Default::default();
    let cancel = CancelToken::new();
    let n = tc.draw(gs::integers::<usize>().min_value(1).max_value(4));
    let mut files = Vec::new();
    // Each file's key, whether the commit fails without it, and the bytes it should get.
    let mut wanted: Vec<(String, bool, Option<Vec<u8>>)> = Vec::new();
    let mut unsafe_path = false;
    for i in 0..n {
        let key = match tc.draw(gs::integers::<u8>().max_value(9)) {
            0 => "Contents/old.mp3".to_string(),
            1 => format!("Contents/d{i}/e/f.mp3"),
            // Below a file: its folder can't be made.
            2 => format!("Contents/blocker/{i}.mp3"),
            3 => {
                unsafe_path = true;
                format!("../escape{i}.mp3")
            }
            _ => format!("Contents/new{i}.mp3"),
        };
        if wanted.iter().any(|(k, _, _)| *k == key) {
            continue;
        }
        let kind = tc.draw(gs::sampled_from(&kinds));
        let optional = tc.draw(gs::booleans());
        let bytes = format!("new {i}").into_bytes();
        let (content, made): (Content<'_>, Option<Vec<u8>>) = match kind {
            Kind::Bytes => (Content::Bytes(bytes.clone()), Some(bytes)),
            Kind::Made => {
                let b = bytes.clone();
                (Content::Make(Box::new(move || Ok(b))), Some(bytes))
            }
            Kind::MadeFails => (Content::Make(Box::new(|| Err(Error::invalid("can't make it")))), None),
            Kind::Copied => (Content::Copy { path: src.clone(), expected: None }, Some(b"copied audio".to_vec())),
            Kind::CopiedGone => (Content::Copy { path: dir.path().join("nowhere"), expected: None }, None),
            Kind::CopiedCorrupt => (Content::Copy { path: src.clone(), expected: Some(Hash::of(b"other")) }, None),
            Kind::StopsCopying => {
                stopping.lock().unwrap().push(key.clone());
                (Content::Copy { path: src.clone(), expected: None }, None)
            }
            Kind::Stops => {
                let c = cancel.clone();
                (
                    Content::Make(Box::new(move || {
                        c.cancel();
                        Ok(Vec::new())
                    })),
                    None,
                )
            }
        };
        let blocked = key.starts_with("Contents/blocker/");
        let made = made.filter(|_| !blocked);
        // One that stops the commit isn't a failure of its own: reached, it stops it.
        let stops = match kind {
            Kind::Stops => !blocked,
            Kind::StopsCopying => true,
            _ => false,
        };
        let needed = !optional && !stops;
        let mtime = tc.draw(gs::optional(gs::integers::<i64>().min_value(1_000_000_000).max_value(1_900_000_000)));
        files.push(NewFile { key: key.clone(), content, mtime, size: 4, optional });
        wanted.push((key, needed, made));
    }
    // A folder can't be deleted as a file: the commit fails once it has swapped its files in.
    // One that was never there is always among them.
    let deletes: Vec<String> = tc
        .draw(gs::subsequences(&["Contents/gone.mp3", "../outside.mp3", "Contents/adir"]))
        .into_iter()
        .chain(["Contents/never.mp3"])
        .map(String::from)
        .collect();
    unsafe_path |= deletes.iter().any(|d| d.starts_with(".."));
    let backup = tc.draw(gs::booleans());
    let fail = tc.draw(gs::booleans());
    let needed_fails = wanted.iter().any(|(_, needed, made)| *needed && made.is_none());

    let mut device = Device::open(&root).unwrap();
    let before = snapshot(device.root());
    let mut opts = Options::new(LineageOp::Edit, "a drawn commit");
    opts.lineage = false;
    if backup {
        opts = opts.backup("before a drawn commit");
    }
    // Stops the commit as it starts on a file that's stopped as it's copied.
    let progress = {
        let (cancel, stopping) = (cancel.clone(), stopping.clone());
        move |e: &Event| {
            if let Event::Item { path, .. } = e
                && stopping.lock().unwrap().contains(path)
            {
                cancel.cancel();
            }
        }
    };
    let ctx = Ctx::new(&progress, cancel.clone());
    let changes = Changes { files, deletes: deletes.clone(), ..Default::default() };
    fail_before_swap(fail);
    let result = commit(&mut device, changes, opts, &ctx);
    fail_before_swap(false);
    let stopped = cancel.is_cancelled();
    tc.note(&format!("{wanted:?} {deletes:?} backup {backup} fail {fail} stopped {stopped}: {result:?}"));
    if unsafe_path || needed_fails || fail || stopped {
        assert!(result.is_err());
        assert_eq!(snapshot(device.root()), before, "a failed commit changed the drive");
        assert!(Device::open(&root).unwrap().state().and_then(|s| s.pending.as_ref()).is_none());
        return;
    }
    let done = result.unwrap();
    let written: Vec<&String> = wanted.iter().filter(|(_, _, made)| made.is_some()).map(|(k, _, _)| k).collect();
    let failed: Vec<&String> = wanted.iter().filter(|(_, _, made)| made.is_none()).map(|(k, _, _)| k).collect();
    assert_eq!(done.written.iter().collect::<Vec<_>>(), written);
    assert_eq!(done.failed.iter().map(|(k, _)| k).collect::<Vec<_>>(), failed);
    for (key, _, made) in &wanted {
        if let Some(bytes) = made {
            assert_eq!(&std::fs::read(device.path(key)).unwrap(), bytes, "{key}");
        }
    }
    let gone = deletes.iter().any(|d| d == "Contents/gone.mp3");
    assert_eq!((done.deleted, done.freed), (usize::from(gone), if gone { 4 } else { 0 }));
    assert_eq!(device.path("Contents/gone.mp3").is_file(), !gone);
    assert_eq!(done.backup.is_some(), backup);
}

/// The lock is taken for a plan made from the databases the drive holds, and refused, as a stale
/// plan, for one made from any others: whatever the databases hold, and with the change in any
/// of them.
#[hegel::test(test_cases = 40)]
fn a_plan_is_applied_only_to_the_databases_it_was_made_from(tc: hegel::TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let device = Device::open(dir.path()).unwrap();
    for key in paths::DATABASE_FILES {
        if let Some(bytes) = tc.draw(gs::optional(gs::binary().max_size(12))) {
            let p = device.path(key);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, bytes).unwrap();
        }
    }
    let planned = database_state(&device);
    drop(lock(&device, Some(planned)).unwrap());
    drop(lock(&device, None).unwrap());
    // A database changes after the plan was made.
    let key = tc.draw(gs::sampled_from(paths::DATABASE_FILES.to_vec()));
    let p = device.path(key);
    let mut bytes = std::fs::read(&p).unwrap_or_default();
    bytes.push(tc.draw(gs::integers::<u8>()));
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(&p, bytes).unwrap();
    let Err(err) = lock(&device, Some(planned)) else { panic!("a stale plan took the lock") };
    assert!(matches!(err.inner(), Error::StalePlan(_)), "{err}");
    drop(lock(&device, Some(database_state(&device))).unwrap());
}

/// A drive whose state can't be read right now isn't committed to: the rollback of a commit that
/// then failed would take the state for one that was never there, and delete it.
#[cfg(unix)]
#[hegel::test(test_cases = 5)]
fn a_commit_leaves_a_state_it_cant_read_alone(tc: hegel::TestCase) {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("drive");
    std::fs::create_dir_all(root.join("Contents")).unwrap();
    let mut device = Device::open(&root).unwrap();
    let mut opts = Options::new(LineageOp::Edit, "an earlier commit");
    opts.lineage = false;
    let changes = Changes { files: vec![NewFile::bytes("Contents/a.mp3", b"a".to_vec())], ..Default::default() };
    commit(&mut device, changes, opts, &Ctx::silent()).unwrap();
    let state = device.state_dir().join("state.json");
    let before = std::fs::read(&state).unwrap();
    let mode = |bits: u32| std::fs::set_permissions(&state, std::fs::Permissions::from_mode(bits)).unwrap();
    mode(0o000);
    // Root reads anything.
    if std::fs::read(&state).is_ok() {
        return mode(0o644);
    }
    let mut opts = Options::new(LineageOp::Edit, "a commit that fails");
    opts.lineage = false;
    let failing = NewFile {
        key: format!("Contents/{}.mp3", tc.draw(gs::from_regex("[a-z]{1,6}").fullmatch(true))),
        content: Content::Make(Box::new(|| Err(Error::invalid("can't make it")))),
        mtime: None,
        size: 1,
        optional: false,
    };
    let changes = Changes { files: vec![failing], ..Default::default() };
    let failed = commit(&mut device, changes, opts, &Ctx::silent());
    mode(0o644);
    let failed = failed.unwrap_err();
    assert!(matches!(&failed, Error::Io { path, .. } if *path == state), "{failed}");
    assert_eq!(std::fs::read(&state).unwrap(), before);
}

/// A swap makes its steps in order, and one that can't be made (a file put where a folder with
/// something in it is) stops it there: what came before is made, what comes after isn't.
#[hegel::test(test_cases = 40)]
fn a_swap_makes_its_steps_in_order_up_to_one_that_fails(tc: hegel::TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let n = tc.draw(gs::integers::<usize>().min_value(1).max_value(5));
    let blocked = tc.draw(gs::optional(gs::integers::<usize>().max_value(n - 1)));
    let mut steps = Vec::new();
    let mut want: Vec<(PathBuf, Option<Vec<u8>>)> = Vec::new();
    for i in 0..n {
        let dst = dir.path().join(format!("f{i}"));
        if tc.draw(gs::booleans()) {
            std::fs::write(&dst, b"was").unwrap();
        }
        if Some(i) == blocked {
            std::fs::remove_file(&dst).ok();
            std::fs::create_dir_all(dst.join("inside")).unwrap();
        }
        match tc.draw(gs::booleans()) {
            true => {
                let bytes = tc.draw(gs::binary().max_size(8));
                steps.push(Swap::bytes(dst.clone(), &bytes).unwrap());
                want.push((dst, Some(bytes)));
            }
            false if Some(i) == blocked => {
                steps.push(Swap::bytes(dst.clone(), b"over a folder").unwrap());
                want.push((dst, None));
            }
            false => {
                steps.push(Swap::Remove(dst.clone()));
                want.push((dst, None));
            }
        }
    }
    // Staging below a file fails before anything is swapped.
    std::fs::write(dir.path().join("a file"), b"x").unwrap();
    assert!(Swap::bytes(dir.path().join("a file/f"), b"x").is_err());
    let result = swap(steps);
    assert_eq!(result.is_err(), blocked.is_some(), "{result:?}");
    for (i, (dst, bytes)) in want.iter().enumerate() {
        match blocked {
            Some(b) if i == b => assert!(dst.is_dir()),
            Some(b) if i > b => assert!(!dst.is_file() || std::fs::read(dst).unwrap() == b"was" || bytes.is_none()),
            _ => assert_eq!(std::fs::read(dst).ok(), bytes.clone(), "step {i}"),
        }
    }
}

/// A folder can't be deleted as a file: a commit that names one to delete fails once its files
/// are swapped in, and they're in place.
#[hegel::test(test_cases = 10)]
fn a_commit_deleting_a_folder_fails_with_its_files_in_place(tc: hegel::TestCase) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("Contents/adir/inside")).unwrap();
    let bytes = tc.draw(gs::binary().max_size(16));
    let mut device = Device::open(dir.path()).unwrap();
    let mut opts = Options::new(LineageOp::Edit, "a commit deleting a folder");
    opts.lineage = false;
    let changes = Changes {
        files: vec![NewFile::bytes("Contents/new.mp3", bytes.clone())],
        deletes: vec!["Contents/adir".into()],
        ..Default::default()
    };
    let err = commit(&mut device, changes, opts, &Ctx::silent()).unwrap_err();
    assert!(matches!(err, Error::Io { .. }), "{err}");
    assert_eq!(std::fs::read(device.path("Contents/new.mp3")).unwrap(), bytes);
    assert!(device.path("Contents/adir/inside").is_dir());
}

/// What stops a drawn commit before its swap.
#[derive(Debug, Clone, Copy)]
enum Obstacle {
    /// Cancelled before it starts.
    Cancelled,
    /// Cancelled as its last file is staged, before the databases.
    CancelledAfterFiles,
    /// `.locallibrary/backups` is a file, so no backup can be made.
    BackupsIsAFile,
    /// The databases' folder is a file, so the new database can't be staged.
    DatabaseFolderIsAFile,
    /// A needed file goes below a file, so its folder can't be made.
    BelowAFile,
    /// A needed file's content can't be made.
    CantMake,
    /// The folder a needed file goes in can't be written to.
    #[cfg(unix)]
    ReadOnlyFolder,
    /// The databases' folder is a link out of the drive.
    #[cfg(unix)]
    DatabaseFolderOutside,
}

/// A commit stopped before its swap (cancelled, or a backup, file or database it can't stage)
/// fails and leaves the drive exactly as it was, with no pending marker: whatever it would have
/// written, whether it was backing up, and whether the drive was written before.
#[hegel::test(test_cases = 30)]
fn prop_a_commit_stopped_before_its_swap_leaves_the_drive_as_it_was(tc: hegel::TestCase) {
    let obstacles = [
        Obstacle::Cancelled,
        Obstacle::CancelledAfterFiles,
        Obstacle::BackupsIsAFile,
        Obstacle::DatabaseFolderIsAFile,
        Obstacle::BelowAFile,
        Obstacle::CantMake,
        #[cfg(unix)]
        Obstacle::ReadOnlyFolder,
        #[cfg(unix)]
        Obstacle::DatabaseFolderOutside,
    ];
    let obstacle = tc.draw(gs::sampled_from(&obstacles));
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("drive");
    std::fs::create_dir_all(root.join("Contents")).unwrap();
    std::fs::write(root.join("Contents/blocker"), b"x").unwrap();
    // At times the drive was written before, backed up or not: its backups are kept on a rollback.
    if let Some(backed_up) = tc.draw(gs::optional(gs::booleans())) {
        let mut device = Device::open(&root).unwrap();
        let mut opts = Options::new(LineageOp::Edit, "an earlier commit");
        opts.lineage = false;
        if backed_up && !matches!(obstacle, Obstacle::BackupsIsAFile) {
            opts = opts.backup("before an earlier commit");
        }
        let changes =
            Changes { files: vec![NewFile::bytes("Contents/prior.mp3", b"p".to_vec())], ..Default::default() };
        commit(&mut device, changes, opts, &Ctx::silent()).unwrap();
    }
    let bytes = tc.draw(gs::binary().max_size(16));
    let mut files = vec![NewFile::bytes("Contents/new/a.mp3", bytes.clone())];
    let mut databases = NewDatabases::default();
    let backup = match obstacle {
        Obstacle::BackupsIsAFile => {
            std::fs::create_dir_all(root.join(paths::STATE_DIR)).unwrap();
            std::fs::write(root.join(paths::STATE_DIR).join("backups"), b"not a folder").unwrap();
            true
        }
        // Rolling back takes away a backups folder the commit made.
        Obstacle::BelowAFile => true,
        // A backup would refuse the databases first.
        #[cfg(unix)]
        Obstacle::DatabaseFolderOutside => false,
        _ => tc.draw(gs::booleans()),
    };
    match obstacle {
        Obstacle::DatabaseFolderIsAFile => {
            let pdb = root.join(paths::EXPORT_PDB);
            std::fs::create_dir_all(pdb.parent().unwrap().parent().unwrap()).unwrap();
            std::fs::write(pdb.parent().unwrap(), b"not a folder").unwrap();
            databases.pdb = Some(bytes.clone());
        }
        Obstacle::BelowAFile => files.push(NewFile::bytes("Contents/blocker/b.mp3", bytes.clone())),
        Obstacle::CantMake => files.push(NewFile::make("Contents/c.mp3", 1, || Err(Error::invalid("can't make it")))),
        #[cfg(unix)]
        Obstacle::ReadOnlyFolder => {
            use std::os::unix::fs::PermissionsExt;
            std::fs::create_dir_all(root.join("Contents/locked")).unwrap();
            std::fs::set_permissions(root.join("Contents/locked"), std::fs::Permissions::from_mode(0o555)).unwrap();
            files.push(NewFile::bytes("Contents/locked/d.mp3", bytes.clone()));
        }
        #[cfg(unix)]
        Obstacle::DatabaseFolderOutside => {
            std::fs::create_dir_all(dir.path().join("elsewhere/rekordbox")).unwrap();
            std::os::unix::fs::symlink(dir.path().join("elsewhere"), root.join("PIONEER")).unwrap();
            databases.pdb = Some(bytes.clone());
        }
        _ => {}
    }
    let mut device = Device::open(&root).unwrap();
    let before = snapshot(device.root());
    let mut opts = Options::new(LineageOp::Edit, "a stopped commit");
    opts.lineage = false;
    if backup {
        opts = opts.backup("before a stopped commit");
    }
    let cancel = CancelToken::new();
    if matches!(obstacle, Obstacle::Cancelled) {
        cancel.cancel();
    }
    let progress = {
        let (cancel, stop_after_files) = (cancel.clone(), matches!(obstacle, Obstacle::CancelledAfterFiles));
        move |e: &Event| {
            if stop_after_files && matches!(e, Event::PhaseEnd { phase: Phase::Copy }) {
                cancel.cancel();
            }
        }
    };
    let ctx = Ctx::new(&progress, cancel);
    let result = commit(&mut device, Changes { databases, files, ..Default::default() }, opts, &ctx);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let locked = root.join("Contents/locked");
        if locked.is_dir() {
            std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
    }
    tc.note(&format!("{obstacle:?} backup {backup}: {result:?}"));
    let err = result.unwrap_err();
    if matches!(obstacle, Obstacle::Cancelled | Obstacle::CancelledAfterFiles) {
        assert!(matches!(err.inner(), Error::Cancelled), "{err}");
    }
    assert_eq!(snapshot(device.root()), before, "a stopped commit changed the drive");
    assert!(Device::open(&root).unwrap().state().and_then(|s| s.pending.as_ref()).is_none());
}

/// What fails a drawn commit once its files are staged.
#[derive(Debug, Clone, Copy)]
enum Late {
    /// A file to remove first has an unsafe path: nothing is swapped in.
    UnsafeRemoveFirst,
    /// Its file goes where a folder with something in it is: the swap stops there.
    OverAFolder,
    /// The backup it seals, made by its caller, isn't there.
    MissingCallerBackup,
    /// It records its lineage, and the drive has no library to record.
    NoLibrary,
    /// It records its lineage, and the drive's hash index can't be saved.
    IndexUnsaved,
}

/// A commit whose remove-first path is unsafe fails without swapping anything in. Once it has
/// swapped (a library to record that isn't there, a hash index it can't save, a caller's backup
/// that's gone), it fails with its file in place; a swap that can't place its file fails with the folder there left alone.
#[hegel::test(test_cases = 20)]
fn prop_a_commit_failing_at_or_after_its_swap_reports_it(tc: hegel::TestCase) {
    let late = tc.draw(gs::sampled_from(&[
        Late::UnsafeRemoveFirst,
        Late::OverAFolder,
        Late::MissingCallerBackup,
        Late::NoLibrary,
        Late::IndexUnsaved,
    ]));
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("Contents/adir/inside")).unwrap();
    if matches!(late, Late::IndexUnsaved) {
        let pdb = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../vendor/rekordbox-pdb/tests/data/bigger-export.pdb");
        let at = dir.path().join(paths::EXPORT_PDB);
        std::fs::create_dir_all(at.parent().unwrap()).unwrap();
        std::fs::copy(pdb, at).unwrap();
        Device::open(dir.path()).unwrap().adopt(None).unwrap();
        let index = dir.path().join(paths::STATE_DIR).join("index.json");
        std::fs::remove_file(&index).ok();
        std::fs::create_dir_all(index.join("in the way")).unwrap();
    }
    let bytes = tc.draw(gs::binary().max_size(16));
    let key = match late {
        Late::OverAFolder => "Contents/adir",
        _ => "Contents/new.mp3",
    };
    let mut device = Device::open(dir.path()).unwrap();
    let mut opts = Options::new(LineageOp::Edit, "a commit failing late");
    opts.lineage = matches!(late, Late::NoLibrary | Late::IndexUnsaved);
    let changes = Changes {
        files: vec![NewFile::bytes(key, bytes.clone())],
        remove_first: match late {
            Late::UnsafeRemoveFirst => vec!["../outside.mp3".into()],
            _ => Vec::new(),
        },
        ..Default::default()
    };
    let result = match late {
        Late::MissingCallerBackup => {
            under_backup("19700101T000000.000Z", || commit(&mut device, changes, opts, &Ctx::silent()))
        }
        _ => commit(&mut device, changes, opts, &Ctx::silent()),
    };
    tc.note(&format!("{late:?}: {result:?}"));
    let err = result.unwrap_err();
    match late {
        Late::UnsafeRemoveFirst => assert!(!device.path(key).exists()),
        Late::OverAFolder => assert!(device.path("Contents/adir/inside").is_dir()),
        Late::NoLibrary => {
            assert!(matches!(err.inner(), Error::NotAnExport(_)), "{err}");
            assert_eq!(std::fs::read(device.path(key)).unwrap(), bytes);
        }
        Late::MissingCallerBackup | Late::IndexUnsaved => {
            assert_eq!(std::fs::read(device.path(key)).unwrap(), bytes)
        }
    }
}
