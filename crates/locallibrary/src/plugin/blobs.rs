//! Requests' scratch folders (`context.scratch`, the `blobs` feature): the host makes one for each
//! hook call, `<plugin's folder>/scratch/<uuid>/`, where the plugin leaves the files its answer
//! names (`{"blob": "cover.jpg"}`) and `net/fetch` reads what it sends from and writes what it
//! fetches into, and removes it once it has read what it needs. A name leading out of the folder
//! is refused, and so is a folder holding more than [`super::Options::max_blobs`]. While a folder
//! is in use the host holds a lock beside it (`<uuid>.lock`): one nobody holds is a stopped
//! host's, which `store gc` removes ([`abandoned`]).

use std::fs::File;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex};

use crate::error::IoContext;
use crate::fsutil::FileLock;
use crate::store::Store;
use crate::{Result, lock};

/// A process's scratch folders, of the requests under way.
#[derive(Default)]
pub(crate) struct Scratches(Mutex<Vec<PathBuf>>);

impl Scratches {
    /// A new folder in `root`, until the [`Scratch`] is dropped. Its lock is held before the
    /// folder is there, so `store gc` never takes it for a stopped host's.
    pub(crate) fn make(self: &Arc<Self>, root: &Path) -> Result<Scratch> {
        let dir = root.join(uuid::Uuid::new_v4().to_string());
        std::fs::create_dir_all(root).at(root)?;
        let held = FileLock::wait(&dir.with_extension("lock"))?;
        std::fs::create_dir(&dir).at(&dir)?;
        lock(&self.0).push(dir.clone());
        Ok(Scratch { dir: Some(dir), held: Some(held), scratches: self.clone() })
    }

    /// No folder: for a plugin without the `blobs` feature, whose answers name no files.
    pub(crate) fn none(self: &Arc<Self>) -> Scratch {
        Scratch { dir: None, held: None, scratches: self.clone() }
    }

    /// The scratch folder under way that `path` (absolute, as `net/fetch` names a file) is in,
    /// and its name there, once it's one ([`file_name`]).
    pub(crate) fn holding(&self, path: &Path) -> Result<(PathBuf, PathBuf), String> {
        let shown = path.display();
        let found =
            lock(&self.0).iter().find_map(|dir| Some((dir.clone(), path.strip_prefix(dir).ok()?.to_path_buf())));
        let (dir, name) = found.ok_or_else(|| format!("{shown} isn't in the scratch folder of a request under way"))?;
        let name = name.to_str().ok_or_else(|| format!("{shown} isn't a name a file can have here"))?;
        let name = file_name(&dir, name)?;
        Ok((dir, name))
    }

    /// The file `path` (absolute) names in a scratch folder under way, opened to read as
    /// [`Scratch::blob`] opens one: what `net/fetch` sends (`body_blob`).
    pub(crate) fn open(&self, path: &Path) -> Result<File, String> {
        let (dir, name) = self.holding(path)?;
        open(&dir, &name.to_string_lossy())
    }
}

/// A request's scratch folder, removed when this is dropped.
pub(crate) struct Scratch {
    dir: Option<PathBuf>,
    /// Its lock, let go once it's removed.
    held: Option<FileLock>,
    scratches: Arc<Scratches>,
}

impl Scratch {
    /// The folder, absolute, for the request's context.
    pub(crate) fn dir(&self) -> Option<&Path> {
        self.dir.as_deref()
    }

    /// The file a blob `name` names in the folder, opened to read (see [`open`]).
    pub(crate) fn blob(&self, name: &str) -> Result<File, String> {
        let dir = self.dir.as_deref().ok_or("it has no scratch folder for files (the blobs feature)")?;
        open(dir, name)
    }

    /// How much its files hold, in bytes.
    pub(crate) fn size(&self) -> u64 {
        self.dir.as_deref().map_or(0, size)
    }
}

/// How much the files in `dir` hold, in bytes (links aren't followed).
pub(crate) fn size(dir: &Path) -> u64 {
    let files = walkdir::WalkDir::new(dir).into_iter().flatten().filter(|e| e.file_type().is_file());
    files.filter_map(|e| e.metadata().ok()).map(|m| m.len()).sum()
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let Some(dir) = self.dir.take() else { return };
        lock(&self.scratches.0).retain(|d| *d != dir);
        let _ = std::fs::remove_dir_all(&dir);
        drop(self.held.take());
        let _ = std::fs::remove_file(dir.with_extension("lock"));
    }
}

/// The scratch folders in `store` whose lock nobody holds: left by a host that stopped without
/// removing them (it crashed, or was killed), for `store gc`. A folder in use is never one,
/// however long its request takes. Nor is one without its lock file: the host makes that first
/// and removes it last, so it's no folder of the host's.
pub(crate) fn abandoned(store: &Store) -> Vec<PathBuf> {
    let plugins = std::fs::read_dir(store.root().join("plugins")).into_iter().flatten().flatten();
    let folders = plugins.flat_map(|p| std::fs::read_dir(p.path().join("scratch")).into_iter().flatten().flatten());
    let folders = folders.filter(|e| e.file_type().is_ok_and(|t| t.is_dir())).map(|e| e.path());
    // Let go at once: nothing takes up a stopped host's folder again.
    folders.filter(|d| FileLock::try_take_existing(&d.with_extension("lock")).is_ok_and(|l| l.is_some())).collect()
}

/// `name` as a file's name below `dir` (`covers/1.jpg`): relative, without `.` or `..`, and in a
/// folder that, links resolved, is still below it.
fn file_name(dir: &Path, name: &str) -> Result<PathBuf, String> {
    let path = Path::new(name);
    let plain = path.components().all(|c| matches!(c, Component::Normal(_)));
    if name.is_empty() || !plain {
        return Err(format!("{name:?} isn't a file's name in the scratch folder"));
    }
    // As far as its folders are there (the scratch folder is).
    let file = dir.join(path);
    let there = file.ancestors().skip(1).find_map(|a| a.canonicalize().ok());
    let inside = there.zip(dir.canonicalize().ok()).is_some_and(|(there, dir)| there.starts_with(dir));
    match inside {
        true => Ok(path.to_path_buf()),
        false => Err(format!("{name:?} leads out of the scratch folder")),
    }
}

/// The file `name` names in the scratch folder `dir`, opened to read: a plain name ([`file_name`])
/// of a file there once links are resolved, and itself no link (it's opened without following
/// one, so neither is a link put in its place since) nor a file with another name (a hard link,
/// which could be to any file on that disk).
fn open(dir: &Path, name: &str) -> Result<File, String> {
    let file = dir.join(file_name(dir, name)?);
    let real = file.canonicalize().map_err(|e| format!("its file {name:?} isn't there: {e}"))?;
    if !dir.canonicalize().is_ok_and(|dir| real.starts_with(dir)) {
        return Err(format!("its file {name:?} leads out of the scratch folder"));
    }
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    // Nor waiting for a pipe's writer.
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::custom_flags(&mut options, libc::O_NOFOLLOW | libc::O_NONBLOCK);
    // FILE_FLAG_OPEN_REPARSE_POINT: a link itself, which then isn't a file.
    #[cfg(windows)]
    std::os::windows::fs::OpenOptionsExt::custom_flags(&mut options, 0x0020_0000);
    let unreadable = |e: std::io::Error| format!("its file {name:?} can't be read: {e}");
    let opened = options.open(&real).map_err(unreadable)?;
    let meta = opened.metadata().map_err(unreadable)?;
    if !meta.is_file() {
        return Err(format!("{name:?} isn't a file"));
    }
    if names(&opened, &meta) > 1 {
        return Err(format!("its file {name:?} has another name (a hard link), which could be anywhere"));
    }
    Ok(opened)
}

/// How many names (hard links) the file has.
#[cfg(unix)]
fn names(_: &File, meta: &std::fs::Metadata) -> u64 {
    std::os::unix::fs::MetadataExt::nlink(meta)
}

#[cfg(windows)]
fn names(file: &File, _: &std::fs::Metadata) -> u64 {
    winapi_util::file::information(file).map_or(u64::MAX, |i| i.number_of_links())
}

#[cfg(test)]
mod tests;
