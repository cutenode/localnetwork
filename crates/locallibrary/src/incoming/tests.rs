use super::*;

use std::fs;

use hegel::generators as gs;

use crate::edit::EditOptions;

/// A drive in `dir` with rekordbox-pdb's twelve-track export database and nothing else.
fn drive(dir: &Path) -> Device {
    let root = dir.join("Main");
    fs::create_dir_all(root.join("PIONEER/rekordbox")).unwrap();
    let pdb = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../vendor/rekordbox-pdb/tests/data/bigger-export.pdb");
    fs::copy(pdb, root.join(paths::EXPORT_PDB)).unwrap();
    Device::open_export(&root).unwrap()
}

/// The audio files a folder offers: those with an audio extension (in any case), below it at any
/// depth, but not OS clutter, not inside locallibrary's own folders, and not the files of the
/// drive's own library when the folder is (or holds) the drive. A folder that isn't there is
/// refused.
#[hegel::test(test_cases = 12)]
#[hegel::explicit_test_case(files = vec![0usize, 2, 3, 5, 7], holds_drive = true)]
#[hegel::explicit_test_case(files = vec![1usize, 4, 6], holds_drive = false)]
fn a_folder_offers_its_audio_but_not_the_drives_library(tc: hegel::TestCase) {
    // (path below the folder, offered)
    const FILES: [(&str, bool); 8] = [
        ("Album/Song.flac", true),
        ("Album/cover.jpg", false),
        ("Loose.WAV", true),
        ("Album/._Song.flac", false),
        ("Deep/er/Track.aiff", true),
        (".locallibrary/backups/1/Old.mp3", false),
        ("notes.txt", false),
        ("Album/Song.mp3", true),
    ];
    let files: Vec<usize> =
        tc.draw(gs::vecs(gs::integers::<usize>().max_value(FILES.len() - 1)).max_size(6).unique(true));
    let holds_drive = tc.draw(gs::booleans());
    let dir = tempfile::tempdir().unwrap();
    // Canonical, as the drive's root is (macOS's temp folder is behind a symlink).
    let base = dir.path().canonicalize().unwrap();
    let folder = base.join("Music");
    let device = drive(&if holds_drive { folder.clone() } else { base.clone() });
    for &i in &files {
        let p = folder.join(FILES[i].0);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, b"audio").unwrap();
    }
    // The drive's library audio, and a file on it outside Contents/ that its library doesn't use.
    let library = [device.path("Contents/Artist/In library.mp3"), device.root().join("Elsewhere/Spare.mp3")];
    for p in &library {
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, b"audio").unwrap();
    }
    fs::create_dir_all(&folder).unwrap();

    let mut found: Vec<PathBuf> =
        audio_files(&folder, &device, &Ctx::silent()).unwrap().into_iter().map(|f| f.path).collect();
    found.sort();
    let mut want: Vec<PathBuf> = files.iter().filter(|&&i| FILES[i].1).map(|&i| folder.join(FILES[i].0)).collect();
    if holds_drive {
        want.push(library[1].clone());
    }
    want.sort();
    assert_eq!(found, want);
    for f in audio_files(&folder, &device, &Ctx::silent()).unwrap() {
        assert_eq!(f.name, f.path.file_name().unwrap().to_string_lossy());
        assert_eq!(f.size, 5);
    }
    let missing = folder.join("Nowhere");
    assert!(matches!(audio_files(&missing, &device, &Ctx::silent()), Err(Error::NotFound(m)) if m.contains("Nowhere")));
    let a_file = folder.join("notes2.txt");
    fs::write(&a_file, b"").unwrap();
    assert!(matches!(audio_files(&a_file, &device, &Ctx::silent()), Err(Error::NotFound(_))));
}

/// The space copies need: each file rounded up to whole clusters, plus the edit's and what else
/// is backed up; it fits exactly when the drive has that much free, a warning says so when it
/// doesn't, and applying then refuses with the same numbers before writing anything.
#[hegel::test(test_cases = 10)]
#[hegel::explicit_test_case(sizes = vec![1u64, 4096, 4097], extra = 0u64)]
#[hegel::explicit_test_case(sizes = vec![0u64], extra = u64::MAX / 4)]
fn copies_fit_or_are_refused_before_anything_is_written(tc: hegel::TestCase) {
    let sizes: Vec<u64> = tc.draw(gs::vecs(gs::integers::<u64>().max_value(1 << 20)).max_size(4));
    let extra = tc.draw(gs::sampled_from(vec![0u64, 1, 1 << 30, u64::MAX / 4]));
    let dir = tempfile::tempdir().unwrap();
    let mut device = drive(dir.path());
    let edit = edit::plan_writing(&device, &[], &EditOptions::default(), &[], &Ctx::silent()).unwrap();
    let source = dir.path().join("Lossless.flac");
    fs::write(&source, b"flac").unwrap();
    let incoming: Vec<Incoming> = sizes
        .iter()
        .enumerate()
        .map(|(n, &size)| Incoming { from: source.clone(), to: format!("Contents/New/{n}.flac"), size })
        .collect();
    let mut warnings = vec!["before".to_string()];
    let s = space(&device, &incoming, &edit, extra, &mut warnings);
    let cluster = fsutil::cluster_size(device.root());
    let copies: u64 = sizes.iter().map(|n| n.div_ceil(cluster) * cluster).sum();
    assert_eq!(s.needed_bytes, copies + edit.needed_bytes + extra);
    assert!(sizes.iter().all(|&n| n.div_ceil(cluster) * cluster >= n));
    let free = fsutil::available_space(device.root());
    assert_eq!(s.free_space.is_some(), free.is_some());
    assert_eq!(s.fits, s.free_space.is_none_or(|f| s.needed_bytes <= f));
    assert_eq!(warnings.len(), if s.fits { 1 } else { 2 }, "{warnings:?}");
    if !s.fits {
        assert!(warnings[1].contains(&format!("need about {} bytes", s.needed_bytes)), "{warnings:?}");
        let before = fs::read(device.path(paths::EXPORT_PDB)).unwrap();
        match apply(&edit, s, &incoming, &mut device, &Ctx::silent()) {
            Err(Error::NoSpace { needed, available, path }) => {
                assert_eq!((needed, Some(available), path.as_path()), (s.needed_bytes, s.free_space, device.root()))
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(fs::read(device.path(paths::EXPORT_PDB)).unwrap(), before);
        assert!(!device.path("Contents/New").exists());
    }
    // A file to copy that has gone since: refused, naming it, with nothing copied.
    let gone = Incoming { from: dir.path().join("Gone.flac"), to: "Contents/New/Gone.flac".into(), size: 1 };
    let fits = Space { fits: true, ..s };
    match apply(&edit, fits, std::slice::from_ref(&gone), &mut device, &Ctx::silent()) {
        Err(Error::Io { path, .. }) => assert_eq!(path, gone.from),
        other => panic!("{other:?}"),
    }
    assert!(!device.path("Contents/New").exists());
}
