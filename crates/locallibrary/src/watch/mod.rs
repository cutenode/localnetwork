//! Noticing drives come and go, and change, cheaply.
//!
//! [`VolumeWatcher`] polls the mounted volumes the way a GUI or the player would, but only
//! looks at file metadata: which volumes are mounted (and on which device), the size,
//! modification time and file id of each drive's database files, and its `.locallibrary`
//! state. No database is parsed, so polling every second costs a few `stat` calls per drive.
//! Each poll reports what happened since the last one as [`WatchEvent`]s: a drive attached,
//! detached or changed, and, when it watches a store, the store changed.
//!
//! [`LibraryStamp`] is the same metadata for one drive's databases: equal stamps mean the
//! library hasn't been rewritten, which is what keeps a parsed library valid for a cache.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::paths;

/// A mounted volume and the device it's on. The device matters because a mount point can show
/// up before its drive is mounted on it (macOS and udisks make the folder first): the path
/// alone wouldn't change again when the drive lands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mount {
    pub path: PathBuf,
    /// The device id (`st_dev` on Unix, the volume serial number on Windows).
    pub dev: u64,
}

/// The mounted volumes (see [`crate::device::mounted_volumes`]), each with its device.
pub fn mounts() -> Vec<Mount> {
    crate::device::mounted_volumes().into_iter().map(mount).collect()
}

fn mount(path: PathBuf) -> Mount {
    let dev = std::fs::metadata(&path).map_or(0, |m| ids(&path, &m).0);
    Mount { path, dev }
}

/// The file or folder at `path` (whose metadata is `m`): its device, and its id there. The
/// device and inode on Unix; on Windows the volume serial number and file index, through a
/// handle (0s when it can't be opened).
#[cfg(unix)]
fn ids(_: &Path, m: &std::fs::Metadata) -> (u64, u64) {
    use std::os::unix::fs::MetadataExt;
    (m.dev(), m.ino())
}

#[cfg(windows)]
fn ids(path: &Path, _: &std::fs::Metadata) -> (u64, u64) {
    let info = winapi_util::Handle::from_path_any(path).and_then(winapi_util::file::information);
    info.map_or((0, 0), |i| (i.volume_serial_number(), i.file_index()))
}

/// One file's metadata, enough to tell that it was rewritten.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FileStamp {
    pub len: u64,
    pub modified: Option<SystemTime>,
    /// The file's id on its file system (the inode on Unix, the file index on Windows). Files are
    /// replaced by renaming a new one into place, which changes it even within FAT's 2-second
    /// timestamps.
    pub ino: u64,
}

impl FileStamp {
    /// The file at `path`, if it is one.
    pub fn of(path: &Path) -> Option<FileStamp> {
        let m = std::fs::metadata(path).ok().filter(std::fs::Metadata::is_file)?;
        Some(FileStamp { len: m.len(), modified: m.modified().ok(), ino: ids(path, &m).1 })
    }
}

/// The metadata of a drive's database files ([`paths::DATABASE_FILES`]: `export.pdb`,
/// `exportExt.pdb`, OneLibrary and its WAL). Equal stamps mean none of them was rewritten.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct LibraryStamp([Option<FileStamp>; 4]);

impl LibraryStamp {
    pub fn of(root: &Path) -> LibraryStamp {
        LibraryStamp(paths::DATABASE_FILES.map(|key| FileStamp::of(&paths::to_fs(root, key))))
    }

    /// Whether the drive holds a rekordbox export (either database).
    pub fn has_export(&self) -> bool {
        let [pdb, _, plus, _] = &self.0;
        pdb.is_some() || plus.is_some()
    }
}

/// A mounted drive, as cheaply as it can be described: no database is read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MountedDrive {
    pub root: PathBuf,
    /// The identity locallibrary gave it (`.locallibrary/state.json`), if any.
    pub id: Option<Uuid>,
    /// Its adopted name, else the volume's name.
    pub name: String,
    /// Whether it holds a rekordbox export (either database).
    pub has_export: bool,
}

/// The part of `state.json` a listing needs.
#[derive(Deserialize, JsonSchema)]
struct Identity {
    id: Uuid,
    name: String,
}

fn state_path(root: &Path) -> PathBuf {
    root.join(paths::STATE_DIR).join("state.json")
}

fn describe(root: PathBuf, library: &LibraryStamp) -> MountedDrive {
    let identity: Option<Identity> = crate::fsutil::read_json_opt(&state_path(&root)).ok().flatten();
    let volume =
        root.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| root.display().to_string());
    MountedDrive {
        id: identity.as_ref().map(|i| i.id),
        name: identity.map_or(volume, |i| i.name),
        has_export: library.has_export(),
        root,
    }
}

/// Every mounted volume (blank ones too: a blank drive is a sync target), described without
/// reading a database.
pub fn mounted_drives() -> Vec<MountedDrive> {
    mounts()
        .into_iter()
        .map(|m| {
            let root = crate::device::dunce_canonicalize(&m.path);
            let stamp = LibraryStamp::of(&root);
            describe(root, &stamp)
        })
        .collect()
}

/// What changed on a drive (see [`WatchEvent::Changed`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum DriveChange {
    /// Its database files: an export, an edit, a sync, or a player recording history.
    Databases,
    /// Its `.locallibrary` state: its identity, name, library or lineage.
    State,
}

/// What a poll noticed.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
#[serde(tag = "event")]
#[non_exhaustive]
pub enum WatchEvent {
    /// A volume was mounted (or a different one mounted where another was).
    #[serde(rename = "drive.attached")]
    Attached(MountedDrive),
    /// A volume went away; the drive as it was last seen.
    #[serde(rename = "drive.detached")]
    Detached(MountedDrive),
    /// A mounted drive's databases or state changed.
    #[serde(rename = "drive.changed")]
    Changed {
        #[serde(flatten)]
        drive: MountedDrive,
        changed: Vec<DriveChange>,
    },
    /// The store's libraries, snapshots, labels, drive registry, settings, smart playlists or
    /// sets changed.
    #[serde(rename = "store.changed")]
    StoreChanged { root: PathBuf, changed: Vec<&'static str> },
}

impl WatchEvent {
    /// The `event` name, e.g. `drive.attached`.
    pub fn kind(&self) -> &'static str {
        match self {
            WatchEvent::Attached(_) => "drive.attached",
            WatchEvent::Detached(_) => "drive.detached",
            WatchEvent::Changed { .. } => "drive.changed",
            WatchEvent::StoreChanged { .. } => "store.changed",
        }
    }

    /// The drive it's about, if it's about one.
    pub fn drive(&self) -> Option<&MountedDrive> {
        match self {
            WatchEvent::Attached(d) | WatchEvent::Detached(d) | WatchEvent::Changed { drive: d, .. } => Some(d),
            WatchEvent::StoreChanged { .. } => None,
        }
    }
}

/// The store's files a [`WatchEvent::StoreChanged`] names, by what they hold.
const STORE_FILES: [(&str, &str); 7] = [
    ("libraries", "libraries.json"),
    ("labels", "labels.json"),
    ("pruned", "pruned.json"),
    ("drives", "drives.json"),
    ("settings", "settings.json"),
    ("smart", "smart.json"),
    ("sets", crate::sets::SETS_FILE),
];

/// The store's metadata: its small JSON files, and the snapshots folder (whose modification
/// time changes when a snapshot is added or removed).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct StoreStamp {
    files: [Option<FileStamp>; STORE_FILES.len()],
    snapshots: Option<(Option<SystemTime>, u64)>,
}

impl StoreStamp {
    fn of(root: &Path) -> StoreStamp {
        let dir = root.join("snapshots");
        let snapshots = std::fs::metadata(&dir).ok().map(|m| (m.modified().ok(), ids(&dir, &m).1));
        StoreStamp { files: STORE_FILES.map(|(_, f)| FileStamp::of(&root.join(f))), snapshots }
    }

    /// What differs from `other`.
    fn changes(&self, other: &StoreStamp) -> Vec<&'static str> {
        let mut out: Vec<&'static str> =
            (0..STORE_FILES.len()).filter(|&i| self.files[i] != other.files[i]).map(|i| STORE_FILES[i].0).collect();
        if self.snapshots != other.snapshots {
            out.push("snapshots");
        }
        out
    }
}

/// Where the volumes come from.
#[derive(Debug, Clone)]
enum Volumes {
    /// The OS's mounted volumes (or `$LOCALLIBRARY_VOLUMES`).
    Mounted,
    /// The subfolders of these folders.
    Folders(Vec<PathBuf>),
}

/// A mounted volume as last seen.
#[derive(Debug, Clone)]
struct Seen {
    dev: u64,
    library: LibraryStamp,
    state: Option<FileStamp>,
    drive: MountedDrive,
}

impl Seen {
    fn new(m: &Mount) -> Seen {
        let root = crate::device::dunce_canonicalize(&m.path);
        let library = LibraryStamp::of(&root);
        let state = FileStamp::of(&state_path(&root));
        Seen { dev: m.dev, drive: describe(root, &library), library, state }
    }
}

/// Polls the mounted volumes (and optionally a store) and says what changed since the last
/// poll. It starts from what's there when it's made: drives already mounted aren't reported.
#[derive(Debug, Clone)]
pub struct VolumeWatcher {
    volumes: Volumes,
    /// By mount path.
    seen: BTreeMap<PathBuf, Seen>,
    store: Option<(PathBuf, StoreStamp)>,
}

impl Default for VolumeWatcher {
    fn default() -> Self {
        VolumeWatcher::new()
    }
}

impl VolumeWatcher {
    /// Watches the mounted volumes ([`mounts`]).
    pub fn new() -> VolumeWatcher {
        VolumeWatcher::start(Volumes::Mounted)
    }

    /// Watches the subfolders of `folders` as if they were mounted volumes (as
    /// `$LOCALLIBRARY_VOLUMES` does, without changing the environment).
    pub fn in_folders(folders: Vec<PathBuf>) -> VolumeWatcher {
        VolumeWatcher::start(Volumes::Folders(folders))
    }

    fn start(volumes: Volumes) -> VolumeWatcher {
        let mut w = VolumeWatcher { volumes, seen: BTreeMap::new(), store: None };
        w.seen = w.list().iter().map(|m| (m.path.clone(), Seen::new(m))).collect();
        w
    }

    /// Also watch the store at `root` ([`WatchEvent::StoreChanged`]).
    pub fn with_store(mut self, root: &Path) -> VolumeWatcher {
        self.store = Some((root.to_path_buf(), StoreStamp::of(root)));
        self
    }

    fn list(&self) -> Vec<Mount> {
        match &self.volumes {
            Volumes::Mounted => mounts(),
            Volumes::Folders(folders) => crate::device::volumes_in(folders).into_iter().map(mount).collect(),
        }
    }

    /// The drives mounted at the last poll.
    pub fn drives(&self) -> Vec<MountedDrive> {
        self.seen.values().map(|s| s.drive.clone()).collect()
    }

    /// Looks again, and says what changed since the last look: detachments first, then
    /// attachments, then changes, each in mount path order, then the store.
    pub fn poll(&mut self) -> Vec<WatchEvent> {
        let now = self.list();
        let mut detached = Vec::new();
        let mut attached = Vec::new();
        let mut changed = Vec::new();
        let gone: Vec<PathBuf> = self
            .seen
            .iter()
            .filter(|(path, seen)| !now.iter().any(|m| m.path == **path && m.dev == seen.dev))
            .map(|(path, _)| path.clone())
            .collect();
        for path in gone {
            if let Some(seen) = self.seen.remove(&path) {
                detached.push(WatchEvent::Detached(seen.drive));
            }
        }
        for m in &now {
            let Some(seen) = self.seen.get_mut(&m.path) else {
                let seen = Seen::new(m);
                attached.push(WatchEvent::Attached(seen.drive.clone()));
                self.seen.insert(m.path.clone(), seen);
                continue;
            };
            let root = seen.drive.root.clone();
            let (library, state) = (LibraryStamp::of(&root), FileStamp::of(&state_path(&root)));
            let mut what = Vec::new();
            if library != seen.library {
                what.push(DriveChange::Databases);
            }
            if state != seen.state {
                what.push(DriveChange::State);
            }
            if !what.is_empty() {
                seen.drive = describe(root, &library);
                (seen.library, seen.state) = (library, state);
                changed.push(WatchEvent::Changed { drive: seen.drive.clone(), changed: what });
            }
        }
        let mut out = detached;
        out.append(&mut attached);
        out.append(&mut changed);
        if let Some((root, stamp)) = &mut self.store {
            let now = StoreStamp::of(root);
            let what = now.changes(stamp);
            if !what.is_empty() {
                *stamp = now;
                out.push(WatchEvent::StoreChanged { root: root.clone(), changed: what });
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(path: &Path, bytes: &[u8]) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, bytes).unwrap();
    }

    fn roots(events: &[WatchEvent]) -> Vec<(&'static str, String)> {
        events
            .iter()
            .map(|e| match e {
                WatchEvent::StoreChanged { changed, .. } => (e.kind(), changed.join(",")),
                _ => (e.kind(), e.drive().unwrap().name.clone()),
            })
            .collect()
    }

    #[test]
    fn reports_drives_attached_changed_and_detached() {
        let dir = tempfile::tempdir().unwrap();
        let volumes = dir.path().join("volumes");
        std::fs::create_dir_all(volumes.join("Old")).unwrap();
        let mut w = VolumeWatcher::in_folders(vec![volumes.clone()]);
        assert_eq!(w.drives().len(), 1, "what's mounted at the start is known");
        assert!(w.poll().is_empty(), "and not reported");

        // Plugged in: a drive with an export.
        let a = volumes.join("A");
        write(&a.join(paths::EXPORT_PDB), b"one");
        let events = w.poll();
        assert_eq!(roots(&events), [("drive.attached", "A".to_string())]);
        let WatchEvent::Attached(d) = &events[0] else { unreachable!() };
        assert!(d.has_export && d.id.is_none());
        assert_eq!(d.root, crate::device::dunce_canonicalize(&a));
        assert!(w.poll().is_empty(), "nothing changed since");

        // Its database rewritten, then its identity given.
        write(&a.join(paths::EXPORT_PDB), b"two, longer");
        assert_eq!(
            w.poll(),
            [WatchEvent::Changed { drive: d.clone(), changed: vec![DriveChange::Databases] }],
            "a rewritten database"
        );
        let id = Uuid::new_v4();
        let state = serde_json::json!({ "format": 1, "id": id, "name": "Main", "created": "2026-10-05T00:00:00Z" });
        write(&a.join(".locallibrary/state.json"), state.to_string().as_bytes());
        let events = w.poll();
        let WatchEvent::Changed { drive, changed } = &events[0] else { panic!("{events:?}") };
        assert_eq!((drive.id, drive.name.as_str(), changed.as_slice()), (Some(id), "Main", &[DriveChange::State][..]));

        // Unplugged: the drive as it was last seen.
        std::fs::remove_dir_all(&a).unwrap();
        std::fs::remove_dir_all(volumes.join("Old")).unwrap();
        assert_eq!(roots(&w.poll()), [("drive.detached", "Main".to_string()), ("drive.detached", "Old".to_string())]);
        assert!(w.drives().is_empty());
    }

    /// Folders are looked in the way `$LOCALLIBRARY_VOLUMES` is: a folder given by two names
    /// holds each drive once.
    #[test]
    fn a_folder_given_twice_holds_each_drive_once() {
        let dir = tempfile::tempdir().unwrap();
        let volumes = dir.path().join("volumes");
        write(&volumes.join("A").join(paths::EXPORT_PDB), b"one");
        let w = VolumeWatcher::in_folders(vec![volumes.clone(), volumes.join("../volumes")]);
        assert_eq!(w.drives().len(), 1);
    }

    #[test]
    fn reports_the_store_changing() {
        let dir = tempfile::tempdir().unwrap();
        let store = dir.path().join("store");
        std::fs::create_dir_all(store.join("snapshots")).unwrap();
        let mut w = VolumeWatcher::in_folders(vec![]).with_store(&store);
        assert!(w.poll().is_empty());
        write(&store.join("libraries.json"), b"[]");
        write(&store.join("drives.json"), b"{}");
        assert_eq!(roots(&w.poll()), [("store.changed", "libraries,drives".to_string())]);
        // A new snapshot: the folder's listing changes.
        std::thread::sleep(std::time::Duration::from_millis(20));
        write(&store.join("snapshots/abc.json"), b"{}");
        assert_eq!(roots(&w.poll()), [("store.changed", "snapshots".to_string())]);
        write(&store.join(crate::sets::SETS_FILE), b"{}");
        assert_eq!(roots(&w.poll()), [("store.changed", "sets".to_string())]);
        assert!(w.poll().is_empty());
    }

    #[test]
    fn stamps_tell_rewritten_databases_apart() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        assert!(!LibraryStamp::of(root).has_export());
        write(&root.join(paths::LIBRARY_PLUS_DB), b"x");
        let before = LibraryStamp::of(root);
        assert!(before.has_export());
        assert_eq!(LibraryStamp::of(root), before);
        // Replaced by a file of the same size, as a sync renames a new one into place.
        write(&root.join("new"), b"y");
        std::fs::rename(root.join("new"), root.join(paths::LIBRARY_PLUS_DB)).unwrap();
        assert_ne!(LibraryStamp::of(root), before);
    }

    #[test]
    fn events_serialize_flat() {
        let d = MountedDrive { root: "/Volumes/A".into(), id: None, name: "A".into(), has_export: true };
        let e = WatchEvent::Changed { drive: d.clone(), changed: vec![DriveChange::Databases] };
        assert_eq!(
            serde_json::to_value(&e).unwrap(),
            serde_json::json!({ "event": "drive.changed", "root": "/Volumes/A", "id": null, "name": "A", "has_export": true, "changed": ["databases"] })
        );
        assert_eq!(serde_json::to_value(WatchEvent::Attached(d)).unwrap()["event"], "drive.attached");
    }
}
