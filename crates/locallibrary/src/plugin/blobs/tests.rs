#[cfg(unix)]
use hegel::TestCase;
#[cfg(unix)]
use hegel::generators as gs;

use super::*;

/// What an opened blob holds.
fn read(file: Result<std::fs::File, String>) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    std::io::Read::read_to_end(&mut file?, &mut bytes).unwrap();
    Ok(bytes)
}

/// A scratch folder in a temporary folder, with the registry it's in.
fn made() -> (tempfile::TempDir, Arc<Scratches>, Scratch) {
    let dir = tempfile::tempdir().unwrap();
    let scratches = Arc::new(Scratches::default());
    let scratch = scratches.make(dir.path(), &[]).unwrap();
    (dir, scratches, scratch)
}

#[test]
fn a_scratch_folder_is_there_until_its_read_and_holds_its_files() {
    let (_dir, scratches, scratch) = made();
    let folder = scratch.dir().unwrap().to_path_buf();
    assert!(folder.is_dir() && std::fs::read_dir(&folder).unwrap().next().is_none(), "made empty");
    std::fs::create_dir(folder.join("covers")).unwrap();
    std::fs::write(folder.join("covers/1.jpg"), [0; 300]).unwrap();
    std::fs::write(folder.join("a"), [0; 12]).unwrap();
    assert_eq!(scratch.size(), 312);
    assert_eq!(read(scratch.blob("covers/1.jpg")).unwrap(), [0; 300]);
    let (dir, name) = scratches.holding(&folder.join("covers/2.jpg")).unwrap();
    assert_eq!((dir, name), (folder.clone(), PathBuf::from("covers/2.jpg")));
    drop(scratch);
    assert!(!folder.exists(), "removed once dropped");
    let e = scratches.holding(&folder.join("a")).unwrap_err();
    assert!(e.ends_with("isn't in the scratch folder of a request under way"), "{e}");
}

#[test]
fn without_the_blobs_feature_there_is_no_folder() {
    let scratch = Arc::new(Scratches::default()).none(&[]);
    assert_eq!((scratch.dir(), scratch.size()), (None, 0));
    assert_eq!(scratch.blob("a.jpg").unwrap_err(), "it has no scratch folder for files (the blobs feature)");
}

/// What a blob's name could lead to.
#[cfg(unix)]
#[derive(Debug, Clone, Copy, PartialEq)]
enum Leads {
    /// A file in the folder.
    File,
    /// A folder in it.
    Folder,
    /// Nothing there.
    Nothing,
    /// A link to a file outside it.
    Out,
    /// A second name (a hard link) of a file outside it.
    Named,
    /// A pipe, which nothing ever writes to.
    Pipe,
}

/// For any name a plugin gives a blob, the host takes it only when it's a plain relative name
/// (no root or `..`) of a file in the folder, and reads what's in it: a folder, nothing, a link
/// out (or through a linked folder), a file with another name or a pipe are refused (never
/// waiting on the pipe); a fetch may write to any plain name but through a link.
#[cfg(unix)]
#[hegel::test(test_cases = 200)]
fn a_blob_is_a_file_in_the_scratch_folder_and_nothing_else(tc: TestCase) {
    let (dir, scratches, scratch) = made();
    let folder = scratch.dir().unwrap().to_path_buf();
    let outside = dir.path().join("outside.jpg");
    std::fs::write(&outside, "x").unwrap();
    let parts: Vec<String> = tc.draw(
        gs::vecs(hegel::one_of!(
            gs::sampled_from(vec!["covers".to_string(), "a.jpg".into(), "..".into()]),
            gs::from_regex("[a-z]{1,4}").fullmatch(true)
        ))
        .min_size(1)
        .max_size(3),
    );
    let absolute = tc.draw(gs::booleans());
    let name = format!("{}{}", if absolute { "/" } else { "" }, parts.join("/"));
    let plain = !absolute && parts.iter().all(|p| p != "..");
    let leads = tc.draw(gs::sampled_from(vec![
        Leads::File,
        Leads::Folder,
        Leads::Nothing,
        Leads::Out,
        Leads::Named,
        Leads::Pipe,
    ]));
    // Through a folder that links out, unless the name has no folder.
    let linked = plain && parts.len() > 1 && tc.draw(gs::booleans());
    if plain {
        let file = folder.join(&name);
        let parent = file.parent().unwrap();
        match linked {
            true => {
                std::fs::create_dir_all(parent.parent().unwrap()).unwrap();
                std::os::unix::fs::symlink(dir.path(), parent).unwrap();
            }
            false => std::fs::create_dir_all(parent).unwrap(),
        }
        match leads {
            Leads::File => std::fs::write(&file, "in").unwrap(),
            Leads::Folder => std::fs::create_dir_all(&file).unwrap(),
            Leads::Out => std::os::unix::fs::symlink(&outside, &file).unwrap(),
            Leads::Named => std::fs::hard_link(&outside, &file).unwrap(),
            Leads::Pipe => assert!(std::process::Command::new("mkfifo").arg(&file).status().unwrap().success()),
            Leads::Nothing => {}
        }
    }
    let taken = plain && !linked && leads == Leads::File;
    match read(scratch.blob(&name)) {
        Ok(bytes) => {
            assert!(taken, "{name:?} ({leads:?}, linked: {linked})");
            assert_eq!(bytes, b"in");
        }
        Err(why) => assert!(!taken, "{name:?}: {why}"),
    }
    // A fetch may write anywhere a blob could be named, through no link.
    let held = scratches.holding(&folder.join(&name));
    assert_eq!(held.is_ok(), plain && !linked, "{name:?}: {held:?}");
}

/// A scratch folder a stopped host left (its lock held by nobody) goes with `store gc`, which
/// counts its files; one in use stays however old it is, and so does a folder without a lock.
#[test]
fn store_gc_removes_only_the_scratch_folders_a_stopped_host_left() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_or_init(dir.path().join("s")).unwrap();
    let root = store.root().join("plugins/echo/scratch");
    let scratch = Arc::new(Scratches::default()).make(&root, &[]).unwrap();
    let live = scratch.dir().unwrap().to_path_buf();
    std::fs::write(live.join("a"), "in use").unwrap();
    // As a host that crashed leaves one: its lock file, held by nobody.
    let left = root.join("left");
    std::fs::create_dir(&left).unwrap();
    std::fs::write(left.join("cover.jpg"), [0; 10]).unwrap();
    std::fs::write(root.join("left.lock"), "").unwrap();
    let unlocked = root.join("unlocked");
    std::fs::create_dir(&unlocked).unwrap();
    assert_eq!(abandoned(&store), std::slice::from_ref(&left));
    let plan = store.gc(true).unwrap();
    assert_eq!((plan.removed_objects, plan.freed_bytes), (1, 10));
    assert!(left.exists(), "a plan removes nothing");
    let _lock = store.lock().unwrap();
    assert_eq!(store.gc(false).unwrap().removed_objects, 1);
    assert!(!left.exists() && !root.join("left.lock").exists());
    assert!(live.join("a").exists() && unlocked.exists());
    drop(scratch);
    assert!(!live.exists() && !live.with_extension("lock").exists(), "removed, its lock too");
}

/// For any bytes a call's `net/fetch` sends of the files it was lent, they're its progress once
/// that's been sent (see [`Scratch::report_to`]), and nothing before. A file lent that isn't there
/// can't be opened, and says which.
#[hegel::test(test_cases = 50)]
fn prop_bytes_sent_of_a_lent_file_are_progress_once_reported(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let dir = tempfile::tempdir().unwrap();
    let (lent, gone) = (dir.path().join("set.wav"), dir.path().join("gone.wav"));
    std::fs::write(&lent, "rec").unwrap();
    let scratches = Arc::new(Scratches::default());
    let scratch = scratches.none(&[&lent, &gone]);
    let (_, under) = scratches.open(&lent).unwrap();
    let (before, after): (Vec<u64>, Vec<u64>) =
        tc.draw(gs::tuples!(gs::vecs(gs::integers::<u64>()).max_size(3), gs::vecs(gs::integers::<u64>()).max_size(3)));
    before.iter().for_each(|&b| under.sent(b));
    let (tx, rx) = std::sync::mpsc::channel();
    scratch.report_to(tx);
    after.iter().for_each(|&b| under.sent(b));
    let reported: Vec<u64> = rx
        .try_iter()
        .map(|r| match r {
            ProgressReport::Known(ProgressEvent {
                event: Progressed::Event(Event::Bytes { phase, bytes }), ..
            }) => {
                assert_eq!(phase, Phase::Upload);
                bytes
            }
            other => panic!("only bytes: {other:?}"),
        })
        .collect();
    assert_eq!(reported, after);
    let e = scratches.open(&gone).err().unwrap();
    assert!(e.starts_with(&format!("{}: ", gone.display())), "{e}");
    assert_eq!(scratch.blob("a.jpg").unwrap_err(), "it has no scratch folder for files (the blobs feature)");
}

/// For any scratch folders a stopped host left in a store (their locks held by nobody), `store gc`
/// finds exactly those: never one in use, nor a folder without a lock, nor a file.
#[hegel::test(test_cases = 20)]
fn prop_only_the_scratch_folders_a_stopped_host_left_are_abandoned(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_or_init(dir.path().join("s")).unwrap();
    let root = store.root().join("plugins/echo/scratch");
    let scratch = Arc::new(Scratches::default()).make(&root, &[]).unwrap();
    let names: Vec<String> = tc.draw(gs::vecs(gs::from_regex("[a-z]{1,6}").fullmatch(true)).max_size(4));
    let mut left: Vec<PathBuf> = Vec::new();
    for (i, name) in names.iter().enumerate() {
        let folder = root.join(format!("{name}{i}"));
        match i % 3 {
            0 => {
                std::fs::create_dir(&folder).unwrap();
                std::fs::write(folder.with_extension("lock"), "").unwrap();
                left.push(folder);
            }
            1 => std::fs::create_dir(&folder).unwrap(),
            _ => std::fs::write(&folder, "").unwrap(),
        }
    }
    let mut found = abandoned(&store);
    found.sort();
    left.sort();
    assert_eq!(found, left);
    assert!(!found.iter().any(|f| Some(f.as_path()) == scratch.dir()));
}

/// A scratch folder can't be made where a file is, nor (on Unix) in a folder the host can't
/// write: the call fails, naming where.
#[cfg(unix)]
#[hegel::test(test_cases = 5)]
fn prop_a_scratch_folder_that_cant_be_made_fails_the_call(tc: TestCase) {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let scratches = Arc::new(Scratches::default());
    let file = dir.path().join("file");
    std::fs::write(&file, "").unwrap();
    let under_file = file.join(tc.draw(gs::from_regex("[a-z]{1,4}").fullmatch(true)));
    let e = scratches.make(&under_file, &[]).err().unwrap();
    assert!(e.to_string().contains(&under_file.display().to_string()), "{e}");
    let locked = dir.path().join("locked");
    std::fs::create_dir(&locked).unwrap();
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o500)).unwrap();
    let e = scratches.make(&locked, &[]).err().unwrap();
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o700)).unwrap();
    assert!(e.to_string().contains(&locked.display().to_string()), "{e}");
    assert_eq!(std::fs::read_dir(&locked).unwrap().count(), 0);
}

/// A file a fetch names is found in the one call under way whose folder holds it, past calls
/// without a folder; a name that isn't text (on Unix) is refused, and so is a file there the
/// host can't read.
#[cfg(unix)]
#[hegel::test(test_cases = 10)]
fn prop_a_file_named_is_found_in_its_calls_folder_and_read_only_as_text(tc: TestCase) {
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let scratches = Arc::new(Scratches::default());
    // A call without a folder is under way first.
    let _without = scratches.none(&[]);
    let scratch = scratches.make(dir.path(), &[]).unwrap();
    let _another = made();
    let folder = scratch.dir().unwrap().to_path_buf();
    let name: String = tc.draw(gs::from_regex("[a-z]{1,6}").fullmatch(true));
    assert_eq!(scratches.holding(&folder.join(&name)).unwrap(), (folder.clone(), PathBuf::from(&name)));
    let mut bytes = name.clone().into_bytes();
    bytes.push(0xff);
    let odd = folder.join(std::ffi::OsStr::from_bytes(&bytes));
    let e = scratches.holding(&odd).unwrap_err();
    assert_eq!(e, format!("{} isn't a name a file can have here", odd.display()));
    std::fs::write(folder.join(&name), "x").unwrap();
    std::fs::set_permissions(folder.join(&name), std::fs::Permissions::from_mode(0o000)).unwrap();
    let e = scratch.blob(&name).unwrap_err();
    assert!(e.starts_with(&format!("its file {name:?} can't be read: ")), "{e}");
    let e = scratches.open(&folder.join(&name)).err().unwrap();
    assert!(e.starts_with(&format!("its file {name:?} can't be read: ")), "{e}");
}
