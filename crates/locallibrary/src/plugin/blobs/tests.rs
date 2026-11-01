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
