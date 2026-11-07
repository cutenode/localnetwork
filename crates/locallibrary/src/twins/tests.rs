use super::*;
use crate::paths::FileClass;

fn entry(path: &Path) -> FileEntry {
    let size = std::fs::metadata(path).unwrap().len();
    FileEntry { path: "Contents/a.mp3".into(), size, mtime: 0, class: FileClass::Audio, hash: None }
}

/// A drive's file finds the file on this computer with its content, whichever of those with its
/// size that is, and none when none has it. A size nothing here has doesn't read the drive's file.
#[hegel::test(test_cases = 40)]
fn a_twin_holds_what_the_drives_file_does(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let contents: Vec<u8> = tc.draw(gs::vecs(gs::integers::<u8>().max_value(3)).max_size(5));
    let on_drive = tc.draw(gs::integers::<u8>().max_value(3));
    let longer = tc.draw(gs::booleans());
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    let files: Vec<PathBuf> = contents
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let file = dir.path().join(format!("{i}.mp3"));
            std::fs::write(&file, [*c; 8]).unwrap();
            file
        })
        .collect();
    let twins = Twins::among(&store, files.iter().cloned().chain([dir.path().join("gone.mp3")]).collect());

    let drive = dir.path().join("drive.mp3");
    std::fs::write(&drive, if longer { vec![on_drive; 9] } else { vec![on_drive; 8] }).unwrap();
    let mut e = entry(&drive);
    let found = twins.of(&drive, &mut e, &Ctx::silent()).unwrap().map(Path::to_path_buf);
    let same_size = !longer && !files.is_empty();
    assert_eq!(e.hash.is_some(), same_size, "read only if something here has its size");
    let holds = !longer && contents.contains(&on_drive);
    assert_eq!(found.is_some(), holds);
    if let Some(found) = found {
        assert_eq!(std::fs::read(found).unwrap(), std::fs::read(&drive).unwrap());
    }

    // Asked again, the collection's files aren't read: their contents are kept by name, size and
    // time, and a hash the drive's scan had is taken as it is.
    let mut again = entry(&drive);
    again.hash = e.hash;
    assert_eq!(twins.of(&drive, &mut again, &Ctx::silent()).unwrap().is_some(), holds);
}

#[test]
fn a_kept_content_hash_reads_back() {
    let hash = Hash::of(b"the audio");
    assert_eq!(Contents::decode(&Contents::encode(&hash)), Some(hash));
    assert_eq!(Contents::decode(b"not a hash"), None);
}

/// A collection that isn't there, or can't be read, finds nothing: a pull goes on without it.
#[test]
fn no_collection_means_no_twins() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    let broken = dir.path().join("master.db");
    std::fs::write(&broken, b"not a database").unwrap();
    // SAFETY: nextest runs each test in a process of its own.
    unsafe { std::env::set_var(masterdb::PATH_ENV, &broken) };
    assert!(Twins::of_collection(&store).by_size.is_empty());
    unsafe { std::env::set_var(masterdb::PATH_ENV, dir.path().join("gone")) };
    assert!(Twins::of_collection(&store).by_size.is_empty());
    unsafe { std::env::remove_var(masterdb::PATH_ENV) };
}
