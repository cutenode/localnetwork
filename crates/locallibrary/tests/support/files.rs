//! Copying a folder, and keeping one in memory to write out again, with each file's modification
//! time, as macOS's copy keeps it and Linux's doesn't: a copy then looks the same on every system,
//! its files older than anything a test writes.
//!
//! Shared by the library's and the CLI's tests and the player's (`#[path]`), so it only uses what
//! they all have.

#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};

use filetime::FileTime;

/// A copy of `src` at `dst`.
pub fn copy_dir(src: &Path, dst: &Path) {
    for entry in walkdir::WalkDir::new(src) {
        let entry = entry.unwrap();
        let target = dst.join(entry.path().strip_prefix(src).unwrap());
        if entry.file_type().is_dir() {
            fs::create_dir_all(&target).unwrap();
        } else {
            fs::copy(entry.path(), &target).unwrap();
            filetime::set_file_mtime(&target, FileTime::from_last_modification_time(&entry.metadata().unwrap()))
                .unwrap();
        }
    }
}

/// A folder's folders and files, in memory: made once and written out again for each test case,
/// which is quicker than making it. Unlike a folder kept in a static, which is never dropped, it
/// leaves nothing behind in the temporary folder.
pub struct Template {
    dirs: Vec<PathBuf>,
    files: Vec<(PathBuf, Vec<u8>, FileTime)>,
}

impl Template {
    pub fn of(root: &Path) -> Template {
        let mut t = Template { dirs: Vec::new(), files: Vec::new() };
        for e in walkdir::WalkDir::new(root) {
            let e = e.unwrap();
            let rel = e.path().strip_prefix(root).unwrap().to_path_buf();
            match e.file_type().is_dir() {
                true => t.dirs.push(rel),
                false => {
                    let mtime = FileTime::from_last_modification_time(&e.metadata().unwrap());
                    t.files.push((rel, fs::read(e.path()).unwrap(), mtime));
                }
            }
        }
        t
    }

    /// The folder, at `root`.
    pub fn write(&self, root: &Path) -> PathBuf {
        for rel in &self.dirs {
            fs::create_dir_all(root.join(rel)).unwrap();
        }
        for (rel, bytes, mtime) in &self.files {
            let path = root.join(rel);
            fs::write(&path, bytes).unwrap();
            filetime::set_file_mtime(&path, *mtime).unwrap();
        }
        root.to_path_buf()
    }
}
