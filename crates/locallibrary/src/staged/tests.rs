//! Unix only: read-only folders stand in for files that can't be removed.
#![cfg(unix)]

use super::*;

use hegel::TestCase;
use hegel::generators as gs;

/// Pruning leaves the files `keep` names and stops, naming the file, at one it can't remove
/// (its folder read-only); copying gives the target what it lacks and stops at a target whose
/// folder for them is a file.
#[hegel::test(test_cases = 4)]
fn prop_staged_files_that_cant_be_pruned_or_copied_stop_with_an_io_error(tc: TestCase) {
    use std::os::unix::fs::PermissionsExt;
    let set_mode =
        |p: &std::path::Path, mode| std::fs::set_permissions(p, std::fs::Permissions::from_mode(mode)).unwrap();
    let staged = tc.draw(gs::sampled_from(vec![IMAGES, WAVEFORMS]));
    let bytes = tc.draw(gs::binary().min_size(1).max_size(32));
    let hash = Hash::of(&bytes);
    let dir = tempfile::tempdir().unwrap();
    let (a, b) = (dir.path().join("A"), dir.path().join("B"));
    std::fs::create_dir_all(&a).unwrap();
    std::fs::create_dir_all(&b).unwrap();
    let (source, target) = (Device::open(&a).unwrap(), Device::open(&b).unwrap());
    let kept = staged.path(&source, &hash);
    std::fs::create_dir_all(kept.parent().unwrap()).unwrap();
    std::fs::write(&kept, &bytes).unwrap();

    // A file the keep list doesn't name, in a folder that can't be written: prune can't remove it.
    let stray = kept.with_file_name(format!("stray.{}", staged.ext));
    std::fs::write(&stray, b"stray").unwrap();
    let folder = kept.parent().unwrap();
    set_mode(folder, 0o555);
    let pruned = staged.prune(&source, &[hash].into_iter().collect());
    set_mode(folder, 0o755);
    let e = pruned.unwrap_err();
    assert_eq!(e.code(), "io", "{e}");
    assert!(e.to_string().contains("stray"), "{e}");
    staged.prune(&source, &[hash].into_iter().collect()).unwrap();
    assert!(kept.is_file() && !stray.exists());

    // The target's folder for these files is a file: the copy has nowhere to go.
    let theirs = staged.path(&target, &hash);
    std::fs::create_dir_all(theirs.parent().unwrap().parent().unwrap()).unwrap();
    std::fs::write(theirs.parent().unwrap(), b"not a folder").unwrap();
    let e = staged.copy(&source, &target, &[hash].into_iter().collect()).unwrap_err();
    assert_eq!(e.code(), "io", "{e}");
    std::fs::remove_file(theirs.parent().unwrap()).unwrap();
    staged.copy(&source, &target, &[hash].into_iter().collect()).unwrap();
    assert_eq!(std::fs::read(&theirs).unwrap(), bytes);
}
