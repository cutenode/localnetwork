//! Views of snapshots (`store browse`): a folder of links into the store, laid out as the
//! snapshot's drive tree, so a past version of a drive can be opened, played or dragged into
//! another app with nothing copied.
//!
//! Files are hard links where the view is on the store's volume and symlinks elsewhere. An app
//! treats a hard link by its own name, extension and all, where macOS follows a symlink to the
//! stored file, whose name is a hash, and opens it as text. A hard link also survives its snapshot
//! being pruned, holding the file's space until the view goes, where a symlink breaks.
//!
//! A write through either would land in a stored file, whose name is the hash of what it held. So
//! every object a view links is read-only before the link is made (new objects already are, and
//! older ones are changed here and nowhere else, so a store isn't walked to make one view). The
//! view's folders are read-only too: an app that saves through a temp file and a rename fails
//! there, instead of leaving an edit that goes when the view does.
//!
//! Open views are listed in `views.json`, so one a crash left is still found and can be removed.
//! That file stays on this computer: copies of the store don't take it.

use std::fs;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use rustc_hash::FxHashSet;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::error::{IoContext, Reason};
use crate::fsutil;
use crate::hash::Hash;
use crate::paths;
use crate::store::{Snapshot, Store};
use crate::{Error, Result};

/// An open view, as `views.json` lists it.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct View {
    pub path: PathBuf,
    pub snapshot: Hash,
    pub created: DateTime<Utc>,
}

/// A view [`open`] made.
#[derive(Debug, Clone, Serialize)]
pub struct Opened {
    #[serde(flatten)]
    pub view: View,
    /// Files linked, and their size.
    pub files: usize,
    pub bytes: u64,
    /// Files linked by symlink, which break if their snapshot is pruned.
    pub symlinks: usize,
    /// Files the store has lost: shown anyway, as broken symlinks.
    pub missing: Vec<String>,
    /// Files left out: a path that would leave the view, one differing from another only in case
    /// (they're one file on a drive), or one with no hash.
    pub skipped: Vec<String>,
}

/// A view as [`list`] finds it.
#[derive(Debug, Clone, Serialize)]
pub struct OpenView {
    #[serde(flatten)]
    pub view: View,
    pub symlinks: usize,
    /// Symlinks whose stored file is gone: nothing the store keeps uses it any more.
    pub broken: usize,
}

/// What [`close`] did.
#[derive(Debug, Clone, Serialize)]
pub struct Closed {
    pub path: PathBuf,
    /// Files in it that aren't the view's, which it left there (and the folders holding them).
    pub left: usize,
}

/// Make a view of `id` in `to` (a new or empty folder) or a temporary folder.
pub fn open(store: &Store, id: &Hash, to: Option<&Path>) -> Result<Opened> {
    if !cfg!(unix) {
        return Err(Error::invalid("`store browse` isn't available on Windows, where links take administrator rights"));
    }
    let snapshot = store.load_snapshot(id)?;
    let view = View { path: folder_for(id, to)?, snapshot: *id, created: Utc::now() };
    let path = view.path.clone();
    let opened = update(store, |views| views.push(view.clone())).and_then(|()| fill(store, &snapshot, view));
    if opened.is_err() {
        let _ = remove(store, path);
    }
    opened
}

/// `to` once it's there and empty, or a new temporary folder.
fn folder_for(id: &Hash, to: Option<&Path>) -> Result<PathBuf> {
    let Some(to) = to else {
        let temp = std::env::temp_dir();
        let name = format!("locallibrary-view-{}-", id.short());
        return Ok(tempfile::Builder::new().prefix(&name).tempdir_in(&temp).at(&temp)?.keep());
    };
    let to = fsutil::absolute(to);
    if fs::read_dir(&to).is_ok_and(|mut entries| entries.next().is_some()) {
        return Err(Error::conflict(Reason::Exists, format!("{} isn't empty; choose another folder", to.display()))
            .with("path", &to));
    }
    fs::create_dir_all(&to).at(&to)?;
    Ok(to)
}

/// Link `snapshot`'s files under the view's folder, then make its folders read-only.
fn fill(store: &Store, snapshot: &Snapshot, view: View) -> Result<Opened> {
    let mut opened = Opened { view, files: 0, bytes: 0, symlinks: 0, missing: Vec::new(), skipped: Vec::new() };
    let mut taken = FxHashSet::default();
    for f in &snapshot.files {
        let unique = paths::is_safe_key(&f.path) && taken.insert(f.path.to_lowercase());
        let (Some(h), true) = (f.hash, unique) else {
            opened.skipped.push(f.path.clone());
            continue;
        };
        let object = fsutil::absolute(&store.object_path(&h));
        match fsutil::seal(&object) {
            Ok(true) => {}
            Ok(false) => {
                return Err(Error::invalid(format!(
                    "the store at {} is on a disk that can't keep files read-only, so a write through a view would \
                     reach it",
                    store.root().display()
                )));
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => opened.missing.push(f.path.clone()),
            Err(e) => return Err(Error::Io { path: object, source: e }),
        }
        let at = opened.view.path.join(&f.path);
        let folder = at.parent().expect("a file in a view has a folder");
        fs::create_dir_all(folder).at(folder)?;
        link(&object, &at).at(&at)?;
        opened.files += 1;
        opened.bytes += f.size;
    }
    for e in walkdir::WalkDir::new(&opened.view.path).into_iter().flatten().filter(|e| e.file_type().is_dir()) {
        set_folder(e.path(), false).at(e.path())?;
    }
    opened.symlinks = symlinks(&opened.view.path).count();
    Ok(opened)
}

/// A hard link at `at` to `target`, or a symlink where there can't be one: another volume, a disk
/// without them, or a `target` the store has lost.
fn link(target: &Path, at: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        fs::hard_link(target, at).or_else(|_| std::os::unix::fs::symlink(target, at))
    }
    #[cfg(not(unix))]
    {
        let _ = (target, at);
        Err(std::io::ErrorKind::Unsupported.into())
    }
}

/// Let its owner change what's in `dir`, or not. Nobody else gets in: a view lists a library.
fn set_folder(dir: &Path, writable: bool) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        let mode = if writable { 0o700 } else { 0o500 };
        fs::set_permissions(dir, std::os::unix::fs::PermissionsExt::from_mode(mode))
    }
    #[cfg(not(unix))]
    {
        let _ = (dir, writable);
        Ok(())
    }
}

/// The symlinks under `dir`.
fn symlinks(dir: &Path) -> impl Iterator<Item = walkdir::DirEntry> {
    walkdir::WalkDir::new(dir).into_iter().flatten().filter(walkdir::DirEntry::path_is_symlink)
}

/// The open views, without those whose folder has gone (a restart clears temporary folders).
pub fn list(store: &Store) -> Result<Vec<OpenView>> {
    let open = recorded(store)?.into_iter().filter(|v| v.path.exists()).map(|view| {
        let links: Vec<walkdir::DirEntry> = symlinks(&view.path).collect();
        let broken = links.iter().filter(|e| !e.path().exists()).count();
        OpenView { view, symlinks: links.len(), broken }
    });
    Ok(open.collect())
}

/// Whether there's still a view at `path`: another `store unbrowse` may have closed it, whatever it
/// left there.
pub fn is_open(store: &Store, path: &Path) -> Result<bool> {
    Ok(path.exists() && recorded(store)?.iter().any(|v| v.path == path))
}

/// Remove the view at `path`. Anything else is refused: this deletes.
pub fn close(store: &Store, path: &Path) -> Result<Closed> {
    let path = fsutil::absolute(path);
    if !is_open(store, &path)? {
        return Err(Error::NotFound(format!("a view at {}", path.display())));
    }
    remove(store, path)
}

/// Delete the view at `path` and its entry.
fn remove(store: &Store, path: PathBuf) -> Result<Closed> {
    let left = remove_links(&path)?;
    update(store, |views| views.retain(|v| v.path != path))?;
    Ok(Closed { path, left })
}

/// Delete the links and folders under `dir`, and `dir` if that empties it. A hard link looks like
/// any file, and may be the last of its stored file by now, but a view's are read-only, and it
/// holds nothing else unless someone made its folders writable to put something there. That
/// stays. Returns how many such files there are.
fn remove_links(dir: &Path) -> Result<usize> {
    set_folder(dir, true).at(dir)?;
    let mut left = 0;
    for e in fs::read_dir(dir).at(dir)? {
        let e = e.at(dir)?;
        let path = e.path();
        let kind = e.file_type().at(&path)?;
        if kind.is_dir() {
            left += remove_links(&path)?;
        } else if kind.is_symlink() || e.metadata().at(&path)?.permissions().readonly() {
            fs::remove_file(&path).at(&path)?;
        } else {
            left += 1;
        }
    }
    if left == 0 {
        fs::remove_dir(dir).at(dir)?;
    }
    Ok(left)
}

/// What to warn of before the snapshots in `removed` go: of a view showing one, symlinks to files
/// nothing else uses break, and hard links keep those files' space from being freed.
pub(crate) fn breaking(store: &Store, removed: &FxHashSet<Hash>) -> Result<Vec<String>> {
    let open = list(store)?.into_iter().filter(|v| removed.contains(&v.view.snapshot));
    let warning = |v: OpenView| {
        let outcome = match v.symlinks {
            0 => "the space its files take stays in use until the view is removed",
            _ => "links to files nothing else uses will break",
        };
        format!("the view at {} shows a snapshot this removes: {outcome}", v.view.path.display())
    };
    Ok(open.map(warning).collect())
}

fn views_file(store: &Store) -> PathBuf {
    store.root().join("views.json")
}

fn recorded(store: &Store) -> Result<Vec<View>> {
    Ok(fsutil::read_json_opt(&views_file(store))?.unwrap_or_default())
}

/// Change the list of views under a lock of its own (views live longer than the store's lock
/// should be held), dropping those whose folder has gone.
fn update(store: &Store, change: impl FnOnce(&mut Vec<View>)) -> Result<()> {
    let _lock = fsutil::FileLock::wait(&store.root().join("views.lock"))?;
    let mut views = recorded(store)?;
    views.retain(|v| v.path.exists());
    change(&mut views);
    fsutil::write_json_atomic(&views_file(store), &views)
}
