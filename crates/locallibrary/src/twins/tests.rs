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

/// A kept content hash reads back as it was written, in either case, and anything that isn't
/// exactly a hash's 64 hex digits reads as nothing: such an entry is read again, never taken as a
/// hash.
#[hegel::test(test_cases = 60)]
#[hegel::explicit_test_case(data = b"the audio".to_vec(), change = 0u8)]
#[hegel::explicit_test_case(data = b"the audio".to_vec(), change = 5u8)]
fn a_kept_content_hash_reads_back(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let data: Vec<u8> = tc.draw(gs::vecs(gs::integers::<u8>()).max_size(16));
    // As written, in capitals, one digit short, one too many, one not a digit, padded.
    let change = tc.draw(gs::integers::<u8>().max_value(5));
    let hash = Hash::of(&data);
    let mut kept = Contents::encode(&hash);
    match change {
        0 => {}
        1 => kept.make_ascii_uppercase(),
        2 => drop(kept.pop()),
        3 => kept.push(b'0'),
        4 => kept[17] = b'g',
        _ => kept.insert(0, b' '),
    }
    assert_eq!(Contents::decode(&kept), (change < 2).then_some(hash), "{:?}", String::from_utf8_lossy(&kept));
}

/// A collection that isn't there, or can't be read, finds nothing: a pull goes on without it.
#[hegel::test(test_cases = 8)]
#[hegel::explicit_test_case(how = 0u8)]
#[hegel::explicit_test_case(how = 1u8)]
#[hegel::explicit_test_case(how = 2u8)]
#[hegel::explicit_test_case(how = 3u8)]
fn no_collection_means_no_twins(tc: hegel::TestCase) {
    // Unset, gone, not a database, a folder.
    let how = tc.draw(hegel::generators::integers::<u8>().max_value(3));
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_or_init(dir.path().join("store")).unwrap();
    let at = dir.path().join("master.db");
    match how {
        0 | 1 => {}
        2 => std::fs::write(&at, b"not a database").unwrap(),
        _ => std::fs::create_dir(&at).unwrap(),
    }
    // SAFETY: nextest runs each test in a process of its own, and Hegel runs its cases in turn.
    unsafe {
        match how {
            0 => std::env::remove_var(masterdb::PATH_ENV),
            _ => std::env::set_var(masterdb::PATH_ENV, &at),
        }
    }
    assert!(Twins::of_collection(&store).by_size.is_empty());
    unsafe { std::env::remove_var(masterdb::PATH_ENV) };
}
