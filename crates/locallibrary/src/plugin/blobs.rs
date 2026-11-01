//! Requests' scratch folders (`context.scratch`, the `blobs` feature): the host makes one for each
//! hook call, `<plugin's folder>/scratch/<uuid>/`, where the plugin leaves the files its answer
//! names (`{"blob": "cover.jpg"}`) and `net/fetch` reads what it sends from and writes what it
//! fetches into, and removes it once it has read what it needs. A name leading out of the folder
//! is refused, and so is a folder holding more than [`super::Options::max_blobs`]. While a folder
//! is in use the host holds a lock beside it (`<uuid>.lock`): one nobody holds is a stopped
//! host's, which `store gc` removes ([`abandoned`]). A hook call may also be lent files to read
//! (a set's recording), which `net/fetch` sends too while it's under way.

use std::fs::File;
use std::path::{Component, Path, PathBuf};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex, OnceLock};

use loclib_plugin::{ProgressEvent, ProgressReport, Progressed};

use crate::error::IoContext;
use crate::fsutil::FileLock;
use crate::progress::{Event, Phase};
use crate::store::Store;
use crate::{Result, lock};

/// A process's hook calls under way, as `net/fetch` finds them by the files it names.
#[derive(Default)]
pub(crate) struct Scratches(Mutex<Vec<Arc<Under>>>);

/// A file in a scratch folder under way: the call it's for, the folder, and its name there.
type Found = (Arc<Under>, PathBuf, PathBuf);

/// A hook call under way: its scratch folder, the files it was lent, and where the bytes its
/// `net/fetch` requests send of them are reported.
pub(crate) struct Under {
    dir: Option<PathBuf>,
    lent: Vec<PathBuf>,
    /// Its progress, once it has been sent (see [`Scratch::report_to`]).
    progress: OnceLock<Sender<ProgressReport>>,
}

impl Under {
    /// Reports `bytes` more of its files sent, as its progress.
    pub(crate) fn sent(&self, bytes: u64) {
        let Some(to) = self.progress.get() else { return };
        let event = Progressed::Event(Event::Bytes { phase: Phase::Upload, bytes });
        let _ = to.send(ProgressReport::Known(ProgressEvent { event, target: None, step: None, items: None }));
    }
}

impl Scratches {
    /// A new folder in `root`, with the files `lent`, until the [`Scratch`] is dropped. Its lock
    /// is held before the folder is there, so `store gc` never takes it for a stopped host's.
    pub(crate) fn make(self: &Arc<Self>, root: &Path, lent: &[&Path]) -> Result<Scratch> {
        let dir = root.join(uuid::Uuid::new_v4().to_string());
        std::fs::create_dir_all(root).at(root)?;
        let held = FileLock::wait(&dir.with_extension("lock"))?;
        std::fs::create_dir(&dir).at(&dir)?;
        Ok(self.register(Some(dir), Some(held), lent))
    }

    /// No folder, but the files `lent`: for a plugin without the `blobs` feature, whose answers
    /// name no files.
    pub(crate) fn none(self: &Arc<Self>, lent: &[&Path]) -> Scratch {
        self.register(None, None, lent)
    }

    fn register(self: &Arc<Self>, dir: Option<PathBuf>, held: Option<FileLock>, lent: &[&Path]) -> Scratch {
        let lent = lent.iter().map(|p| p.to_path_buf()).collect();
        let under = Arc::new(Under { dir, lent, progress: OnceLock::new() });
        lock(&self.0).push(under.clone());
        Scratch { under, held, scratches: self.clone() }
    }

    /// The scratch folder under way that `path` (absolute, as `net/fetch` names a file) is in,
    /// with the call it's for, and its name there, once it's one ([`file_name`]). None when it's
    /// in none.
    fn find(&self, path: &Path) -> Option<Result<Found, String>> {
        let found = lock(&self.0).iter().find_map(|u| {
            let dir = u.dir.as_ref()?;
            Some((u.clone(), dir.clone(), path.strip_prefix(dir).ok()?.to_path_buf()))
        });
        let (under, dir, name) = found?;
        let Some(name) = name.to_str() else {
            return Some(Err(format!("{} isn't a name a file can have here", path.display())));
        };
        Some(file_name(&dir, name).map(|name| (under, dir, name)))
    }

    /// [`Scratches::find`]'s folder and name.
    pub(crate) fn holding(&self, path: &Path) -> Result<(PathBuf, PathBuf), String> {
        let not = || Err(format!("{} isn't in the scratch folder of a request under way", path.display()));
        let (_, dir, name) = self.find(path).unwrap_or_else(not)?;
        Ok((dir, name))
    }

    /// The file `path` (absolute) names, opened to read, with the call it's for: one lent to a
    /// call under way, or one in a scratch folder under way, opened as [`Scratch::blob`] opens
    /// one. What `net/fetch` sends (`body_blob`).
    pub(crate) fn open(&self, path: &Path) -> Result<(File, Arc<Under>), String> {
        let shown = path.display();
        let lent = lock(&self.0).iter().find(|u| u.lent.iter().any(|l| l == path)).cloned();
        if let Some(under) = lent {
            let opened = File::open(path).map_err(|e| format!("{shown}: {e}"))?;
            return Ok((opened, under));
        }
        let not =
            || Err(format!("{shown} isn't in the scratch folder of a request under way, nor a file one was given"));
        let (under, dir, name) = self.find(path).unwrap_or_else(not)?;
        Ok((open(&dir, &name.to_string_lossy())?, under))
    }
}

/// A hook call's scratch folder (and the files it was lent), removed when this is dropped.
pub(crate) struct Scratch {
    under: Arc<Under>,
    /// Its lock, let go once it's removed.
    held: Option<FileLock>,
    scratches: Arc<Scratches>,
}

impl Scratch {
    /// The folder, absolute, for the request's context.
    pub(crate) fn dir(&self) -> Option<&Path> {
        self.under.dir.as_deref()
    }

    /// Where what its files' sending makes of progress goes: the request's, once it's sent.
    pub(crate) fn report_to(&self, progress: Sender<ProgressReport>) {
        let _ = self.under.progress.set(progress);
    }

    /// The file a blob `name` names in the folder, opened to read (see [`open`]).
    pub(crate) fn blob(&self, name: &str) -> Result<File, String> {
        let dir = self.dir().ok_or("it has no scratch folder for files (the blobs feature)")?;
        open(dir, name)
    }

    /// How much its files hold, in bytes.
    pub(crate) fn size(&self) -> u64 {
        self.dir().map_or(0, size)
    }
}

/// How much the files in `dir` hold, in bytes (links aren't followed).
pub(crate) fn size(dir: &Path) -> u64 {
    let files = walkdir::WalkDir::new(dir).into_iter().flatten().filter(|e| e.file_type().is_file());
    files.filter_map(|e| e.metadata().ok()).map(|m| m.len()).sum()
}

impl Drop for Scratch {
    fn drop(&mut self) {
        lock(&self.scratches.0).retain(|u| !Arc::ptr_eq(u, &self.under));
        let Some(dir) = &self.under.dir else { return };
        let _ = std::fs::remove_dir_all(dir);
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
