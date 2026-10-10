//! A mounted drive holding (or about to hold) a rekordbox device export.
//!
//! locallibrary keeps a small `.locallibrary/` directory at the drive root:
//!
//! * `state.json`: drive identity (a UUID that survives renames and remounts), the library
//!   it belongs to, and its *lineage*: the chain of library fingerprints this drive has held.
//!   Lineage is what lets [`crate::compare`] say "drive B is behind drive A" with confidence
//!   instead of guessing from timestamps.
//! * `index.json`: hash cache (see [`crate::tree::HashIndex`]).
//! * `backups/`: copies of the database files taken before locallibrary modifies them.
//!
//! Players ignore unknown directories, so this is invisible to CDJs/XDJs.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::{IoContext, Reason};
use crate::fsutil;
use crate::hash::Hash;
use crate::model::{Library, LibraryDiff, LibrarySummary, TrackChange, TrackRef};
use crate::onelibrary::OneLibrary;
use crate::paths::{self, FileClass};
use crate::pdb::{self, PdbHeader};
use crate::progress::Ctx;
use crate::tree::{self, FileEntry, FileTree, HashIndex, ScanOptions};
use crate::{Error, Result};

pub const STATE_FORMAT: u32 = 1;
const MAX_LINEAGE: usize = 256;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct DeviceState {
    pub format: u32,
    pub id: Uuid,
    pub name: String,
    pub created: DateTime<Utc>,
    /// Store library this drive is linked to, if any.
    #[serde(default)]
    pub library_id: Option<Uuid>,
    /// Oldest first. The last entry is the state locallibrary last saw or wrote.
    #[serde(default)]
    pub lineage: Vec<LineageEntry>,
    /// Set while an operation is modifying the drive; left behind if it was interrupted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending: Option<PendingOp>,
    /// The disk this identity was given on. A copy of the drive made in Finder or with a cloning
    /// tool carries this file along; the hardware says which stick is the original.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hardware: Option<crate::fleet::Hardware>,
    /// Set when the last sync or push left the drive's files differing from the state it copied
    /// (files kept with `--keep-extra`, or ones it had to skip): see [`StateAlias`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alias: Option<StateAlias>,
    /// The Device Name and colour `drive player` last gave the drive. rekordbox's next export
    /// writes its own values, and `refresh` puts these back (see [`crate::player`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub player: Option<crate::player::DbIdentity>,
}

/// A drive holding a library state plus files that state doesn't have: while the drive's
/// fingerprint is `fingerprint`, it also goes by `holds`, the state a sync or push copied onto it.
/// Without this, a drive synced with `--keep-extra` would look changed since, and newer than the
/// drive it was synced from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct StateAlias {
    pub fingerprint: Hash,
    pub holds: Hash,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct LineageEntry {
    pub fingerprint: Hash,
    pub at: DateTime<Utc>,
    pub op: LineageOp,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snapshot: Option<Hash>,
    /// Drive the content came from (sync/merge).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<Uuid>,
    #[serde(default)]
    pub tracks: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LineageOp {
    /// locallibrary saw this state (e.g. after a rekordbox export).
    Observed,
    Pull,
    Push,
    Sync,
    Merge,
    Artwork,
    Restore,
    Clean,
    /// One database built from the other (see [`crate::convert`]).
    Convert,
    /// Playlists, tracks or cues edited in place (see [`crate::edit`]).
    Edit,
    /// A subset of another drive's library written to this one (see [`crate::pack`]).
    Pack,
    /// Damaged files rewritten from a known-good copy (see [`crate::scrub`]).
    Repair,
    /// An operation from a newer version of locallibrary.
    #[serde(other)]
    Other,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct PendingOp {
    pub op: LineageOp,
    pub started: DateTime<Utc>,
    pub description: String,
    /// The backup it made before changing anything: putting it back takes it back.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backup: Option<String>,
    /// The player name and colour the drive had before a sync, push or pack replaced its
    /// `exportLibrary.db`, put back once it's done (see [`crate::player::stash`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub player: Option<crate::player::DbIdentity>,
}

impl PendingOp {
    /// What it was and when it started, in local time: ``a `locallibrary pack` (pack from Main)
    /// didn't finish (started 2026-10-05 21:30)``.
    pub fn describe(&self) -> String {
        let command = match self.op {
            LineageOp::Sync => Some("sync"),
            LineageOp::Push => Some("library push"),
            LineageOp::Pull => Some("pull"),
            LineageOp::Pack => Some("pack"),
            LineageOp::Merge => Some("merge"),
            LineageOp::Clean => Some("clean"),
            LineageOp::Convert => Some("convert"),
            LineageOp::Artwork => Some("art"),
            LineageOp::Restore => Some("undo"),
            LineageOp::Repair => Some("drive verify --repair"),
            LineageOp::Edit | LineageOp::Observed | LineageOp::Other => None,
        };
        let what = command.map_or("a locallibrary operation".into(), |c| format!("a `locallibrary {c}`"));
        let about = if self.description.is_empty() { String::new() } else { format!(" ({})", self.description) };
        let started = self.started.with_timezone(&chrono::Local).format("%Y-%m-%d %H:%M");
        format!("{what}{about} didn't finish (started {started})")
    }

    /// What finishes it, or takes it back.
    pub fn remedy(&self) -> &'static str {
        match (self.op, &self.backup) {
            (LineageOp::Sync | LineageOp::Push, _) => "sync onto it again to finish it",
            (LineageOp::Pack, _) => "pack it again to finish it",
            (_, Some(_)) => "`locallibrary undo` puts back the backup it made",
            _ => "run it again, or make any other change to the drive, to finish it",
        }
    }
}

impl DeviceState {
    pub fn new(name: impl Into<String>) -> Self {
        DeviceState {
            format: STATE_FORMAT,
            id: Uuid::new_v4(),
            name: name.into(),
            created: Utc::now(),
            library_id: None,
            lineage: Vec::new(),
            pending: None,
            hardware: None,
            alias: None,
            player: None,
        }
    }

    pub fn head(&self) -> Option<&LineageEntry> {
        self.lineage.last()
    }

    pub fn knows(&self, fingerprint: &Hash) -> bool {
        self.lineage.iter().any(|e| &e.fingerprint == fingerprint)
    }

    /// Append a state unless it is already the head.
    pub fn record(&mut self, entry: LineageEntry) {
        if self.head().is_some_and(|h| h.fingerprint == entry.fingerprint) {
            return;
        }
        self.lineage.push(entry);
        if self.lineage.len() > MAX_LINEAGE {
            let excess = self.lineage.len() - MAX_LINEAGE;
            self.lineage.drain(..excess);
        }
    }

    /// This drive now holds exactly what the source holds, so it takes over the source's history,
    /// ending with `entry` (which replaces the source's head if it describes the same state).
    pub fn inherit_with(&mut self, source: &[LineageEntry], entry: LineageEntry) {
        self.lineage = source.to_vec();
        match self.lineage.last_mut() {
            Some(head) if head.fingerprint == entry.fingerprint => *head = entry,
            _ => self.lineage.push(entry),
        }
        if self.lineage.len() > MAX_LINEAGE {
            let excess = self.lineage.len() - MAX_LINEAGE;
            self.lineage.drain(..excess);
        }
    }
}

/// Quick, hash-free description of a drive, for listings and UIs.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct DeviceInfo {
    pub root: PathBuf,
    pub volume_name: String,
    pub id: Option<Uuid>,
    pub name: Option<String>,
    pub library_id: Option<Uuid>,
    /// Either database below.
    pub has_export: bool,
    /// `export.pdb` present (the Device Library every player reads, except the newest).
    pub has_device_library: bool,
    /// OneLibrary `exportLibrary.db` present (the only database CDJ-3000X, OPUS-QUAD, XDJ-AZ...
    /// read).
    pub has_library_plus: bool,
    pub pdb: Option<PdbHeader>,
    pub exported_at: Option<DateTime<Utc>>,
    pub summary: Option<LibrarySummary>,
    pub settings_files: Vec<String>,
    pub total_space: Option<u64>,
    pub free_space: Option<u64>,
    pub pending: Option<PendingOp>,
    pub lineage_len: usize,
    pub last_known: Option<LineageEntry>,
    /// How `export.pdb` and OneLibrary differ, on a drive that has both (empty when they agree).
    pub drift: Option<LibraryDrift>,
    /// rekordbox's automatic playlist sync to this drive, per database.
    pub rekordbox_sync: Vec<RekordboxSync>,
}

/// Differences between a drive's two databases. rekordbox exports the same collection to both,
/// but they can drift apart (rekordbox applies some edits to only one of them), and then players
/// that read OneLibrary (CDJ-3000X, OPUS-QUAD...) show a different library from the others.
#[derive(Debug, Clone, Default, Serialize, JsonSchema)]
pub struct LibraryDrift {
    /// Tracks only `export.pdb` lists: OneLibrary players don't see them.
    pub only_in_device_library: Vec<TrackRef>,
    /// Tracks only OneLibrary lists: older players don't see them.
    pub only_in_onelibrary: Vec<TrackRef>,
    /// Tracks in both (same file) whose details differ.
    pub changed_tracks: Vec<TrackChange>,
    /// Playlists (and folders, ending in `/`) in only one database, or with different contents.
    pub playlists: Vec<String>,
    /// OneLibrary couldn't be read, so nothing could be compared.
    pub error: Option<String>,
}

impl LibraryDrift {
    pub fn is_empty(&self) -> bool {
        self.only_in_device_library.is_empty()
            && self.only_in_onelibrary.is_empty()
            && self.changed_tracks.is_empty()
            && self.playlists.is_empty()
            && self.error.is_none()
    }

    /// Compare `export.pdb`'s library with OneLibrary's.
    pub fn between(device_library: &Library, onelibrary: &Library) -> LibraryDrift {
        let d = LibraryDiff::between(device_library, onelibrary);
        // rekordbox's own empty "CUE Analysis Playlist" only ever goes to export.pdb.
        let built_in = |p: &String| DEVICE_LIBRARY_ONLY_PLAYLISTS.contains(&p.as_str());
        let mut playlists: Vec<String> = d
            .added_playlists
            .into_iter()
            .chain(d.removed_playlists.into_iter().filter(|p| !built_in(p)))
            .chain(d.changed_playlists)
            .collect();
        playlists.sort();
        LibraryDrift {
            only_in_device_library: d.removed_tracks,
            only_in_onelibrary: d.added_tracks,
            changed_tracks: d.changed_tracks,
            playlists,
            error: None,
        }
    }
}

/// Playlists rekordbox writes to `export.pdb` but not OneLibrary.
const DEVICE_LIBRARY_ONLY_PLAYLISTS: &[&str] = &["CUE Analysis Playlist"];

/// rekordbox's automatic playlist sync to a drive (`playlists3.sync` / `playlists3Plus.sync`):
/// which playlists of its collection it keeps up to date there. Playlists locallibrary adds
/// aren't in it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct RekordboxSync {
    /// `device_library` (`export.pdb`) or `onelibrary`.
    pub library: &'static str,
    pub automatic: bool,
    /// rekordbox collection the playlists come from (OneLibrary's `masterDbId`).
    pub collection: Option<u32>,
    /// Playlists and folders rekordbox tracks.
    pub playlists: usize,
}

impl RekordboxSync {
    /// Parse a `playlists3*.sync` file (`<Sync DBID=".." AutomaticSync="1"><Playlists><NODE ../>`).
    pub fn parse(library: &'static str, xml: &str) -> Option<RekordboxSync> {
        let start = xml.find("<Sync ")?;
        let tag = &xml[start..start + xml[start..].find('>')?];
        let attr = |name: &str| {
            let at = tag.find(&format!(" {name}=\""))? + name.len() + 3;
            Some(&tag[at..at + tag[at..].find('"')?])
        };
        Some(RekordboxSync {
            library,
            automatic: attr("AutomaticSync") == Some("1"),
            // Written as a signed 32-bit number.
            collection: attr("DBID").and_then(|v| v.parse::<i64>().ok()).map(|v| v as u32),
            playlists: xml.matches("<NODE ").count(),
        })
    }
}

pub struct Device {
    root: PathBuf,
    state: Option<DeviceState>,
}

/// Exclusive hold on a drive for an operation that writes it, so two operations (say a sync and
/// a merge started from a GUI) can't interleave. Released on drop.
pub struct DriveLock {
    _lock: fsutil::FileLock,
}

impl std::fmt::Debug for Device {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Device").field("root", &self.root).field("id", &self.state.as_ref().map(|s| s.id)).finish()
    }
}

impl Device {
    /// Open a drive root (a mounted volume or any directory used as one).
    pub fn open(root: impl AsRef<Path>) -> Result<Device> {
        let root = root.as_ref();
        if !root.is_dir() {
            return Err(Error::NotFound(format!("{} is not a directory", root.display())));
        }
        let root = dunce_canonicalize(root);
        let state = fsutil::read_json_opt(&root.join(paths::STATE_DIR).join("state.json"))?;
        Ok(Device { root, state })
    }

    /// Open a drive and require an existing rekordbox export (either database) on it.
    pub fn open_export(root: impl AsRef<Path>) -> Result<Device> {
        let d = Device::open(root)?;
        if !d.has_export() {
            return Err(Error::NotAnExport(d.root.clone()));
        }
        Ok(d)
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn path(&self, key: &str) -> PathBuf {
        paths::to_fs(&self.root, key)
    }

    /// Like [`Device::path`], for keys that come from a database (untrusted): rejects keys that
    /// would escape the drive, and paths that resolve outside it through a symlink.
    pub fn checked_path(&self, key: &str) -> Result<PathBuf> {
        if !paths::is_safe_key(key) {
            return Err(Error::invalid(format!("refusing unsafe path {key:?} from the database on {}", self.name())));
        }
        let p = self.path(key);
        let mut existing = p.as_path();
        while !existing.exists() {
            match existing.parent() {
                Some(parent) => existing = parent,
                None => break,
            }
        }
        let resolved = dunce_canonicalize(existing);
        if !resolved.starts_with(&self.root) {
            return Err(Error::invalid(format!("refusing {key:?}: it resolves outside {}", self.root.display())));
        }
        Ok(p)
    }

    /// Whether the drive holds a rekordbox export: `export.pdb`, `exportLibrary.db` or both.
    pub fn has_export(&self) -> bool {
        self.has_device_library() || self.has_library_plus()
    }

    /// `export.pdb`, the Device Library.
    pub fn has_device_library(&self) -> bool {
        self.path(paths::EXPORT_PDB).is_file()
    }

    /// `exportLibrary.db`, OneLibrary.
    pub fn has_library_plus(&self) -> bool {
        self.path(paths::LIBRARY_PLUS_DB).is_file()
    }

    pub fn volume_name(&self) -> String {
        self.root
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| self.root.display().to_string())
    }

    pub fn state(&self) -> Option<&DeviceState> {
        self.state.as_ref()
    }

    pub fn id(&self) -> Option<Uuid> {
        self.state.as_ref().map(|s| s.id)
    }

    /// Display name: the adopted name, else the volume name.
    pub fn name(&self) -> String {
        self.state.as_ref().map(|s| s.name.clone()).unwrap_or_else(|| self.volume_name())
    }

    pub fn is_adopted(&self) -> bool {
        self.state.is_some()
    }

    /// Give the drive an identity if it has none. Writes `.locallibrary/state.json`.
    pub fn adopt(&mut self, name: Option<&str>) -> Result<&mut DeviceState> {
        match (&mut self.state, name) {
            (None, _) => {
                let name = name.map(str::to_string).unwrap_or_else(|| self.volume_name());
                self.state = Some(DeviceState::new(name));
                self.save_state()?;
            }
            (Some(state), Some(name)) => {
                state.name = name.to_string();
                self.save_state()?;
            }
            (Some(_), None) => {}
        }
        Ok(self.state.as_mut().expect("set above"))
    }

    /// Give a freshly formatted drive back the identity it had (id, name, library), without its
    /// lineage: it holds none of those states any more.
    pub fn reset_identity(&mut self, old: &DeviceState) -> Result<()> {
        self.state = Some(DeviceState { lineage: Vec::new(), pending: None, alias: None, ..old.clone() });
        self.save_state()
    }

    /// Replace the drive's identity outright (see [`crate::fleet::adopt_as`] and
    /// [`crate::fleet::reidentify`]).
    pub fn replace_state(&mut self, state: DeviceState) -> Result<()> {
        self.state = Some(state);
        self.save_state()
    }

    /// Read the identity file again: after something put an earlier one back, or once the drive
    /// is locked, as another process may have written it since this one opened the drive.
    pub(crate) fn reload_state(&mut self) -> Result<()> {
        self.state = fsutil::read_json_opt(&self.state_dir().join("state.json"))?;
        Ok(())
    }

    pub fn state_mut(&mut self) -> Result<&mut DeviceState> {
        self.adopt(None)
    }

    pub fn save_state(&self) -> Result<()> {
        if let Some(state) = &self.state {
            fsutil::write_json_atomic(&self.state_dir().join("state.json"), state)?;
        }
        Ok(())
    }

    pub fn state_dir(&self) -> PathBuf {
        self.root.join(paths::STATE_DIR)
    }

    pub fn load_index(&self) -> HashIndex {
        HashIndex::load(&self.state_dir().join("index.json"))
    }

    /// Persist the hash cache, but only on adopted drives (read-only commands never create
    /// files on a drive the user hasn't handed to us).
    pub fn save_index(&self, index: &HashIndex) -> Result<()> {
        if self.is_adopted() { index.save(&self.state_dir().join("index.json")) } else { Ok(()) }
    }

    pub fn pdb_bytes(&self) -> Result<Vec<u8>> {
        let p = self.path(paths::EXPORT_PDB);
        std::fs::read(&p).at(&p)
    }

    pub fn pdb_header(&self) -> Option<PdbHeader> {
        let mut f = std::fs::File::open(self.path(paths::EXPORT_PDB)).ok()?;
        let mut buf = [0u8; 0x1c];
        std::io::Read::read_exact(&mut f, &mut buf).ok()?;
        pdb::read_header(&buf)
    }

    /// When rekordbox last exported the library, as far as the drive tells: when `export.pdb` or
    /// OneLibrary (the newer of `exportLibrary.db` and its WAL) was written, and on a drive with
    /// both, the older of the two. A player writes only the database it reads (history lists, a
    /// CDJ-3000X's playlist edits) while rekordbox writes both, so there a set doesn't look like
    /// an export. With one database, a set can.
    pub fn exported_at(&self) -> Option<DateTime<Utc>> {
        let modified = |key| std::fs::metadata(self.path(key)).and_then(|m| m.modified()).ok();
        let pdb = modified(paths::EXPORT_PDB);
        let plus = modified(paths::LIBRARY_PLUS_DB).into_iter().chain(modified(paths::LIBRARY_PLUS_WAL)).max();
        let at: SystemTime = match (pdb, plus) {
            (Some(pdb), Some(plus)) => pdb.min(plus),
            (pdb, plus) => pdb.or(plus)?,
        };
        Some(at.into())
    }

    /// The library as players see it: `export.pdb` when the drive has one, otherwise OneLibrary.
    /// (rekordbox writes the same collection to both; [`Device::library_plus`] reads OneLibrary's.)
    pub fn library(&self) -> Result<Library> {
        if self.has_device_library() {
            pdb::read_library(&self.path(paths::EXPORT_PDB))
        } else if self.has_library_plus() {
            self.library_plus()?.library()
        } else {
            Err(Error::NotAnExport(self.root.clone()))
        }
    }

    /// The OneLibrary database, decrypted for reading or editing.
    pub fn library_plus(&self) -> Result<OneLibrary> {
        OneLibrary::open(self)
    }

    /// How the two databases differ, if the drive has both. `export.pdb`'s library can be passed
    /// in when the caller has it already.
    pub fn library_drift(&self, device_library: Option<&Library>) -> Option<LibraryDrift> {
        if !self.has_device_library() || !self.has_library_plus() {
            return None;
        }
        let read;
        let pdb = match device_library {
            Some(l) => l,
            None => {
                read = pdb::read_library(&self.path(paths::EXPORT_PDB)).ok()?;
                &read
            }
        };
        Some(match self.library_plus().and_then(|one| one.library()) {
            Ok(plus) => LibraryDrift::between(pdb, &plus),
            Err(e) => LibraryDrift { error: Some(e.to_string()), ..Default::default() },
        })
    }

    /// rekordbox's automatic playlist sync records on the drive.
    pub fn rekordbox_sync(&self) -> Vec<RekordboxSync> {
        [(paths::PLAYLIST_SYNC, "device_library"), (paths::PLAYLIST_SYNC_PLUS, "onelibrary")]
            .into_iter()
            .filter_map(|(key, library)| {
                let xml = std::fs::read(self.path(key)).ok()?;
                RekordboxSync::parse(library, &String::from_utf8_lossy(&xml))
            })
            .collect()
    }

    /// A warning for edits rekordbox's automatic playlist sync doesn't know about, if it's set up:
    /// "rekordbox syncs N playlist(s) to this drive automatically; `consequence`".
    pub fn rekordbox_sync_warning(&self, consequence: &str) -> Option<String> {
        let synced: Vec<RekordboxSync> = self.rekordbox_sync().into_iter().filter(|s| s.automatic).collect();
        let first = synced.first()?;
        Some(format!(
            "rekordbox syncs {} playlist(s) to this drive automatically{}; {consequence}",
            first.playlists,
            first.collection.map(|c| format!(" (from collection {c})")).unwrap_or_default()
        ))
    }

    /// Scan managed files. Audio referenced by the database outside `Contents/` is included.
    pub fn scan(&self, library: Option<&Library>, ctx: &Ctx<'_>) -> Result<FileTree> {
        // Audio the database references outside PIONEER/ and Contents/ (tracks rekordbox left
        // where they were on the drive) is part of the library too, but only if the path is
        // safe. Such files are copied when missing but never deleted.
        let extra_keys = library
            .map(|l| {
                l.tracks
                    .iter()
                    .map(|t| paths::key(&t.file_path))
                    .filter(|k| paths::classify(k).is_none() && self.checked_path(k).is_ok())
                    .collect()
            })
            .unwrap_or_default();
        tree::scan(&self.root, &ScanOptions { extra_keys }, ctx)
    }

    /// Take the drive's write lock. Fails with [`Error::Locked`] if another operation, in this
    /// process or another, holds it.
    pub fn lock(&self) -> Result<DriveLock> {
        let dir = self.state_dir();
        std::fs::create_dir_all(&dir).at(&dir)?;
        match fsutil::FileLock::try_take(&dir.join("lock"))? {
            Some(lock) => Ok(DriveLock { _lock: lock }),
            None => Err(Error::Locked(self.root.clone())),
        }
    }

    /// Mark the drive as being modified by `op`, with the backup it made first (the marker
    /// survives a crash, so the next run can tell). The caller holds the drive's lock; the
    /// identity file is read again, as another process may have written it since this one opened
    /// the drive.
    ///
    /// A drive marked already was cut off in the middle of something. Copying a whole library onto
    /// it (sync, push, pack) or putting a backup back finishes or replaces whatever that was: it
    /// takes the marker over, keeping when the cut-off run started (files written since then may
    /// be torn, and the next sync copies them again) and the player name it stashed, and removes
    /// the temp files that run left.
    /// So does any write after one in place that made no backup (an update file, a repair, a step
    /// of a replay, or an earlier version's edit): nothing could take that back, and refusing would
    /// leave the drive stuck. Anything else is refused, as an edit on top would hide that the
    /// library is half written.
    pub fn begin(&mut self, op: LineageOp, description: impl Into<String>, backup: Option<String>) -> Result<()> {
        self.reload_state()?;
        let copies = |op| matches!(op, LineageOp::Sync | LineageOp::Push | LineageOp::Pack);
        let (started, player) = match self.state.as_ref().and_then(|s| s.pending.as_ref()) {
            None => (Utc::now(), None),
            Some(p) if copies(op) || op == LineageOp::Restore || (p.backup.is_none() && !copies(p.op)) => {
                let kept = (p.started, p.player.clone());
                crate::tidy::remove_leftovers(self)?;
                kept
            }
            Some(p) => return Err(self.interrupted(p)),
        };
        let state = self.state_mut()?;
        state.pending = Some(PendingOp { op, started, description: description.into(), backup, player });
        self.save_state()
    }

    /// Refuse to read a library from this drive while an operation on it hasn't finished: it may
    /// hold half of one.
    pub fn finished(&self) -> Result<()> {
        match self.state().and_then(|s| s.pending.as_ref()) {
            Some(p) => Err(self.interrupted(p)),
            None => Ok(()),
        }
    }

    fn interrupted(&self, p: &PendingOp) -> Error {
        Error::conflict(
            Reason::Interrupted,
            format!("{}, so {}'s library may be half written: {}", p.describe(), self.name(), p.remedy()),
        )
        .with("drive", self.root.display().to_string())
        .with("operation", &p.description)
    }

    pub fn finish(&mut self, entry: Option<LineageEntry>) -> Result<()> {
        let state = self.state_mut()?;
        state.pending = None;
        if let Some(e) = entry {
            state.record(e);
        }
        self.save_state()
    }

    pub fn info(&self) -> DeviceInfo {
        let library = self.library().ok();
        let drift = self.library_drift(library.as_ref().filter(|_| self.has_device_library()));
        let settings_files = crate::settings::SettingsFile::ALL
            .iter()
            .filter(|f| self.path(f.key()).is_file())
            .map(|f| f.file_name().to_string())
            .collect();
        DeviceInfo {
            root: self.root.clone(),
            volume_name: self.volume_name(),
            id: self.id(),
            name: self.state.as_ref().map(|s| s.name.clone()),
            library_id: self.state.as_ref().and_then(|s| s.library_id),
            has_export: self.has_export(),
            has_device_library: self.has_device_library(),
            has_library_plus: self.has_library_plus(),
            pdb: self.pdb_header(),
            exported_at: self.exported_at(),
            summary: library.as_ref().map(Library::summary),
            settings_files,
            total_space: fsutil::total_space(&self.root),
            free_space: fsutil::available_space(&self.root),
            pending: self.state.as_ref().and_then(|s| s.pending.clone()),
            lineage_len: self.state.as_ref().map_or(0, |s| s.lineage.len()),
            last_known: self.state.as_ref().and_then(|s| s.head().cloned()),
            drift,
            rekordbox_sync: self.rekordbox_sync(),
        }
    }
}

/// Fingerprint of a library's *content*: every library file's path and size, plus the content
/// hash of everything except audio. Identical on any drive (or snapshot) holding the same export,
/// regardless of timestamps. Audio is covered by size only, so fingerprinting never requires
/// reading audio data. Paths are compared ignoring case, like the filesystems DJ drives use.
/// Callers must have hashed non-audio entries (see [`needs_content_hash`]).
///
/// `library_plus` is [`crate::onelibrary::content_hash`] of the OneLibrary database, which then
/// stands in for `exportLibrary.db` and its WAL: their bytes change whenever SQLite checkpoints
/// (a player opening the database may), and every page is re-encrypted with a fresh IV, while the
/// library stays the same. Without it (no OneLibrary, or one that can't be decrypted) the files'
/// bytes count like any other.
pub fn fingerprint<'a>(
    entries: impl IntoIterator<Item = &'a FileEntry>,
    library_plus: Option<Hash>,
) -> Result<Option<Hash>> {
    fingerprint_of(entries, library_plus, false)
}

/// [`fingerprint`], counting what the drive keeps for itself as library data with
/// `with_drive_state`, as versions before [`FileClass::DriveState`] did.
fn fingerprint_of<'a>(
    entries: impl IntoIterator<Item = &'a FileEntry>,
    library_plus: Option<Hash>,
    with_drive_state: bool,
) -> Result<Option<Hash>> {
    let mut hasher = blake3::Hasher::new();
    let mut any_db = false;
    let is_plus = |p: &str| p == paths::LIBRARY_PLUS_DB.to_lowercase() || p == paths::LIBRARY_PLUS_WAL.to_lowercase();
    let mut items: Vec<(String, &FileEntry)> = entries
        .into_iter()
        .filter(|e| e.class.is_library() || (with_drive_state && e.class == FileClass::DriveState))
        .map(|e| (e.path.to_lowercase(), e))
        .filter(|(p, _)| library_plus.is_none() || !is_plus(p))
        .collect();
    items.sort_by(|a, b| a.0.cmp(&b.0));
    if let Some(h) = library_plus {
        any_db = true;
        hasher.update(b"\0onelibrary\0");
        hasher.update(h.as_bytes());
    }
    for (folded, e) in items {
        any_db |= e.path == paths::EXPORT_PDB || e.path == paths::LIBRARY_PLUS_DB;
        hasher.update(folded.as_bytes());
        hasher.update(&[0]);
        hasher.update(&e.size.to_le_bytes());
        if needs_content_hash(e) {
            let h = e.hash.ok_or_else(|| Error::invalid(format!("{} has not been hashed", e.path)))?;
            hasher.update(h.as_bytes());
        }
    }
    Ok(any_db.then(|| hasher.finalize().into()))
}

/// Non-audio files are small and must be compared by content. What the drive keeps for itself
/// is hashed too, for the fingerprint earlier versions gave the same state (see
/// [`Scanned::legacy_fingerprints`]).
pub fn needs_content_hash(e: &FileEntry) -> bool {
    (e.class.is_library() && e.class != FileClass::Audio) || e.class == FileClass::DriveState
}

/// Scan + hash what's needed + fingerprint, persisting the hash cache when allowed.
pub struct Scanned {
    pub tree: FileTree,
    pub fingerprint: Option<Hash>,
    /// The same state's fingerprints as earlier versions computed them, where they differ:
    /// before what players and rekordbox keep for one drive was left out
    /// ([`FileClass::DriveState`]), and before OneLibrary's per-drive name was too (see
    /// [`crate::onelibrary::content_hashes`]). Lineage and snapshots recorded then hold these.
    pub legacy_fingerprints: Vec<Hash>,
    /// The state a sync or push copied onto the drive, when the files it kept besides make its
    /// fingerprint differ (see [`StateAlias`]).
    pub alias: Option<Hash>,
}

impl Scanned {
    /// Every fingerprint this state may have been recorded under.
    pub fn fingerprints(&self) -> impl Iterator<Item = Hash> + '_ {
        self.fingerprint.into_iter().chain(self.legacy_fingerprints.iter().copied()).chain(self.alias)
    }
}

impl Device {
    pub fn scan_fingerprinted(&self, library: Option<&Library>, ctx: &Ctx<'_>) -> Result<Scanned> {
        let mut tree = self.scan(library, ctx)?;
        let mut index = self.load_index();
        // The database files decide everything else; never trust the cache for them. (They are
        // page-aligned, so an edit often keeps their size, and FAT's 2 s mtimes can hide it.)
        for key in paths::DATABASE_FILES {
            index.forget(key);
        }
        tree.ensure_hashes(&mut index, needs_content_hash, ctx)?;
        index.retain_keys(&tree);
        self.save_index(&index)?;
        let (fingerprint, legacy_fingerprints) = self.fingerprints(&tree)?;
        let alias = self.state().and_then(|s| s.alias).filter(|a| fingerprint == Some(a.fingerprint)).map(|a| a.holds);
        Ok(Scanned { tree, fingerprint, legacy_fingerprints, alias })
    }

    /// [`fingerprint`] of a scan of this drive (non-audio files hashed), reading OneLibrary's
    /// content from the drive.
    pub fn fingerprint(&self, tree: &FileTree) -> Result<Option<Hash>> {
        Ok(self.fingerprints(tree)?.0)
    }

    /// [`Device::fingerprint`], and the legacy fingerprints of the same state where they differ
    /// (see [`Scanned::legacy_fingerprints`]).
    pub fn fingerprints(&self, tree: &FileTree) -> Result<(Option<Hash>, Vec<Hash>)> {
        let plus = match tree.resolve(paths::LIBRARY_PLUS_DB) {
            Some(_) => crate::onelibrary::read_files(&self.root)
                .and_then(|(db, wal)| crate::onelibrary::content_hashes(&db, wal.as_deref()))
                .ok(),
            None => None,
        };
        let current = fingerprint(tree.entries.values(), plus.map(|p| p.0))?;
        // Both legacy states counted the drive's own state; the older one OneLibrary's name too.
        // (Not where a scan read only the library's files, leaving that state unhashed.)
        let mut legacy = Vec::new();
        for library_plus in [plus.map(|p| p.0), plus.map(|p| p.1)] {
            if let Ok(Some(fp)) = fingerprint_of(tree.entries.values(), library_plus, true)
                && Some(fp) != current
                && !legacy.contains(&fp)
            {
                legacy.push(fp);
            }
        }
        Ok((current, legacy))
    }
}

/// `p` canonicalised as drive roots are (`p` itself if it can't be): on Windows without the
/// `\\?\` prefix [`Path::canonicalize`] adds, so it compares equal to [`Device::root`] and to the
/// mount points the OS reports (`E:\`).
pub fn dunce_canonicalize(p: &Path) -> PathBuf {
    let c = p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
    // Strip Windows' verbatim prefix so paths stay readable and joinable.
    #[cfg(windows)]
    {
        let s = c.to_string_lossy();
        if let Some(rest) = s.strip_prefix(r"\\?\")
            && !rest.starts_with("UNC\\")
        {
            return PathBuf::from(rest);
        }
    }
    c
}

// ---------------------------------------------------------------------------------------------
// Detection
// ---------------------------------------------------------------------------------------------

thread_local! {
    /// [`volume_folders`] for one thread (`testing::volumes_on_this_thread`).
    pub(crate) static THREAD_VOLUMES: std::cell::RefCell<Option<Vec<PathBuf>>> = const { std::cell::RefCell::new(None) };
}

/// The folders `$LOCALLIBRARY_VOLUMES` names (a path list, like `PATH`), when it's set. It's a
/// test switch, to be set with `$LOCALLIBRARY_STORE`: drives are then looked for only in those
/// folders (set it empty to see none), never among the OS's mount points or where the store last
/// saw them outside these folders, and the system's disk tools don't run (`disks_shown`).
/// Harnesses use it so a run never sees, or touches, the drives plugged into the machine.
pub fn volume_folders() -> Option<Vec<PathBuf>> {
    if let Some(dirs) = THREAD_VOLUMES.with(|v| v.borrow().clone()) {
        return Some(dirs);
    }
    let dirs = std::env::var_os("LOCALLIBRARY_VOLUMES")?;
    Some(std::env::split_paths(&dirs).filter(|d| !d.as_os_str().is_empty()).collect())
}

/// Refused while [`volume_folders`] hides this machine's drives: its disks are hidden too, so they
/// can't be listed or formatted, nor the volume holding a drive folder checked or relabelled.
pub(crate) fn disks_shown() -> Result<()> {
    match volume_folders() {
        Some(_) => Err(Error::invalid(
            "LOCALLIBRARY_VOLUMES is set, which hides this machine's drives: its disks can't be listed, formatted, checked or relabelled",
        )),
        None => Ok(()),
    }
}

/// Candidate mount points for removable drives on this OS (or in [`volume_folders`]), each once.
pub fn mounted_volumes() -> Vec<PathBuf> {
    if let Some(dirs) = volume_folders() {
        return volumes_in(&dirs);
    }
    let mut out = Vec::new();
    #[cfg(target_os = "macos")]
    out.extend(list_dirs(Path::new("/Volumes")));
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let user = std::env::var("USER").unwrap_or_default();
        let folders = [format!("/media/{user}"), format!("/run/media/{user}"), "/media".into(), "/mnt".into()];
        out.extend(in_mount_folders(&folders.map(PathBuf::from)));
    }
    #[cfg(windows)]
    {
        for letter in b'D'..=b'Z' {
            let p = PathBuf::from(format!("{}:\\", letter as char));
            if p.is_dir() {
                out.push(p);
            }
        }
    }
    tidy_volumes(out)
}

/// The subfolders of `folders` as volumes, each once (see [`tidy_volumes`]).
pub(crate) fn volumes_in(folders: &[PathBuf]) -> Vec<PathBuf> {
    tidy_volumes(folders.iter().flat_map(|f| list_dirs(f)).collect())
}

/// Each volume once, by its canonical path (a folder listed twice, or by two names, gives the
/// same volumes), without the boot volume: macOS lists it in `/Volumes` as a symlink to `/`.
fn tidy_volumes(found: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = found.iter().map(|p| dunce_canonicalize(p)).filter(|p| p != Path::new("/")).collect();
    out.sort();
    out.dedup();
    out
}

/// Linux: the volumes in its mount folders, which are their subfolders but not the folders
/// themselves (udisks mounts in `/media/<user>`, which is in `/media`, where other tools mount).
#[cfg_attr(not(all(unix, not(target_os = "macos"))), allow(dead_code))]
fn in_mount_folders(folders: &[PathBuf]) -> Vec<PathBuf> {
    folders.iter().flat_map(|f| list_dirs(f)).filter(|p| !folders.contains(p)).collect()
}

fn list_dirs(dir: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(dir)
        .map(|rd| rd.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.is_dir()).collect())
        .unwrap_or_default()
}

/// Mounted drives that hold a rekordbox export or were adopted by locallibrary.
/// With `include_blank`, every mounted volume is returned (for picking a sync target).
pub fn detect(include_blank: bool) -> Vec<Device> {
    mounted_volumes()
        .into_iter()
        .filter_map(|p| Device::open(p).ok())
        .filter(|d| include_blank || d.has_export() || d.is_adopted() || d.path(paths::PIONEER_DIR).is_dir())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Regression: on Linux, `/media/<user>`, where udisks mounts drives, was a blank volume in
    /// `/media`.
    #[test]
    fn mount_folders_are_not_volumes() {
        let dir = tempfile::tempdir().unwrap();
        let (media, user) = (dir.path().join("media"), dir.path().join("media/sam"));
        for v in [user.join("STICK"), media.join("usb0")] {
            std::fs::create_dir_all(v).unwrap();
        }
        let mut found = in_mount_folders(&[user.clone(), media.clone()]);
        found.sort();
        assert_eq!(found, [user.join("STICK"), media.join("usb0")]);
    }

    fn entry(fp: u8) -> LineageEntry {
        LineageEntry {
            fingerprint: Hash::of(&[fp]),
            at: Utc::now(),
            op: LineageOp::Observed,
            snapshot: None,
            source: None,
            tracks: 0,
        }
    }

    #[test]
    fn lineage_dedupes_head_and_caps() {
        let mut s = DeviceState::new("x");
        s.record(entry(1));
        s.record(entry(1));
        assert_eq!(s.lineage.len(), 1);
        for i in 0..=255u8 {
            s.record(entry(i));
        }
        // 1 + 256 entries recorded; the oldest one is dropped.
        assert_eq!(s.lineage.len(), MAX_LINEAGE);
        assert_eq!(s.lineage[0].fingerprint, Hash::of(&[0]));
        assert_eq!(s.head().unwrap().fingerprint, Hash::of(&[255]));
    }

    #[test]
    fn parses_rekordbox_playlist_sync() {
        let xml = "<?xml version=\"1.0\"?>\r\n<Sync DBID=\"-1940252023\" AutomaticSync=\"1\" AllPlaylists=\"0\">\
                   <Playlists><NODE Id=\"0\" ParentId=\"0\"/><NODE Id=\"B181727E\" ParentId=\"0\"/></Playlists></Sync>";
        let s = RekordboxSync::parse("onelibrary", xml).unwrap();
        assert_eq!(
            s,
            RekordboxSync { library: "onelibrary", automatic: true, collection: Some(2_354_715_273), playlists: 2 }
        );
        assert!(RekordboxSync::parse("onelibrary", "<Other/>").is_none());
    }

    /// What a run that didn't finish says, and what finishes it.
    #[test]
    fn an_unfinished_run_says_what_it_was_and_what_to_do() {
        let pending = |op, description: &str, backup: Option<&str>| PendingOp {
            op,
            started: Utc::now(),
            description: description.into(),
            backup: backup.map(String::from),
            player: None,
        };
        let pack = pending(LineageOp::Pack, "pack from Main", None);
        assert!(pack.describe().starts_with("a `locallibrary pack` (pack from Main) didn't finish (started "));
        assert_eq!(pack.remedy(), "pack it again to finish it");
        let repair = pending(LineageOp::Repair, "", Some("20261005-213000"));
        assert!(repair.describe().starts_with("a `locallibrary drive verify --repair` didn't finish"));
        assert_eq!(repair.remedy(), "`locallibrary undo` puts back the backup it made");
        let edit = pending(LineageOp::Edit, "", None);
        assert!(edit.describe().starts_with("a locallibrary operation didn't finish"));
        assert_eq!(edit.remedy(), "run it again, or make any other change to the drive, to finish it");
        assert_eq!(pending(LineageOp::Push, "", None).remedy(), "sync onto it again to finish it");
        for op in [
            LineageOp::Sync,
            LineageOp::Pull,
            LineageOp::Merge,
            LineageOp::Clean,
            LineageOp::Convert,
            LineageOp::Artwork,
            LineageOp::Restore,
        ] {
            assert!(pending(op, "", None).describe().starts_with("a `locallibrary "), "{op:?}");
        }
    }

    /// A drive that copied another's whole library takes over its history, capped like any other.
    #[test]
    fn inheriting_a_lineage_ends_with_the_new_state() {
        let source: Vec<LineageEntry> = (0..=255u8).map(entry).collect();
        let mut s = DeviceState::new("Backup");
        s.inherit_with(&source, LineageEntry { op: LineageOp::Sync, ..entry(255) });
        assert_eq!(s.lineage.len(), MAX_LINEAGE);
        assert_eq!(s.head().unwrap().op, LineageOp::Sync, "the source's head is replaced");
        s.inherit_with(&source, entry(7));
        assert_eq!((s.lineage.len(), s.lineage[0].fingerprint), (MAX_LINEAGE, Hash::of(&[1])));
        assert_eq!(s.head().unwrap().fingerprint, Hash::of(&[7]));
    }

    /// A formatted drive gets its identity back, but none of the states it held before.
    #[test]
    fn a_formatted_drive_gets_its_identity_back() {
        let dir = tempfile::tempdir().unwrap();
        let mut d = Device::open(dir.path()).unwrap();
        assert!(matches!(Device::open_export(dir.path()), Err(Error::NotAnExport(_))));
        d.save_state().unwrap();
        assert!(!dir.path().join(paths::STATE_DIR).join("state.json").exists(), "nothing to save");
        let mut old = DeviceState::new("Main");
        old.record(entry(1));
        d.reset_identity(&old).unwrap();
        let d = Device::open(dir.path()).unwrap();
        assert_eq!((d.id(), d.name()), (Some(old.id), "Main".to_string()));
        assert!(d.state().unwrap().lineage.is_empty());
        assert!(format!("{d:?}").contains(&old.id.to_string()));
    }

    #[test]
    fn adopt_persists_identity() {
        let dir = tempfile::tempdir().unwrap();
        let mut d = Device::open(dir.path()).unwrap();
        assert!(!d.is_adopted());
        let id = d.adopt(Some("Backup A")).unwrap().id;
        let d2 = Device::open(dir.path()).unwrap();
        assert_eq!(d2.id(), Some(id));
        assert_eq!(d2.name(), "Backup A");
    }
}
