//! Removing the files macOS and Windows leave on a drive.
//!
//! * `._*`: AppleDouble files, where macOS keeps Finder metadata on FAT and exFAT drives. Other
//!   devices list them as broken, zero-length tracks (`._track.mp3`). Only files that really are
//!   AppleDouble (they start with its magic number) are removed.
//! * `.DS_Store`, `Thumbs.db`, `.TemporaryItems`.
//! * `.Spotlight-V100` and `.fseventsd`: Spotlight's index and the file system event log.
//! * With `empty_trash`, `.Trashes` and `$RECYCLE.BIN`: files deleted in Finder or Explorer still
//!   use space until the trash is emptied with the drive plugged in. This deletes them for good.
//! * `.locallibrary-tmp-*`: what a locallibrary write that was cut off (a pulled plug, a crash)
//!   was writing, up to a whole audio file each. Every scan ignores them. A write that takes over
//!   the cut-off one's marker removes them too (see [`crate::device::Device::begin`]).
//!
//! `System Volume Information` is left alone (Windows recreates it) and so is locallibrary's own
//! folder. Unlike every other operation, this touches files outside `PIONEER/` and `Contents/`,
//! which is why it is a separate, explicit command.
//!
//! Preventing the clutter isn't reliable: `.metadata_never_index` is reported not to stop
//! Spotlight on macOS Sequoia, and turning indexing off for a volume (`mdutil -i off`) needs
//! administrator rights.

use std::io::Read;
use std::path::{Path, PathBuf};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use walkdir::WalkDir;

use crate::device::Device;
use crate::paths;
use crate::progress::{Ctx, Event, Phase};
use crate::schema::Failure;
use crate::space::dir_size;
use crate::{Error, Result};

/// AppleDouble's magic number.
const APPLE_DOUBLE_MAGIC: [u8; 4] = [0x00, 0x05, 0x16, 0x07];

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum TidyKind {
    AppleDouble,
    DsStore,
    Thumbnails,
    TemporaryItems,
    Spotlight,
    FsEvents,
    Trash,
    TempFiles,
}

impl TidyKind {
    pub fn describe(self) -> &'static str {
        match self {
            TidyKind::AppleDouble => "macOS metadata files (._*)",
            TidyKind::DsStore => "Finder folder settings (.DS_Store)",
            TidyKind::Thumbnails => "Windows thumbnail caches (Thumbs.db)",
            TidyKind::TemporaryItems => "temporary items",
            TidyKind::Spotlight => "Spotlight index",
            TidyKind::FsEvents => "file system event log",
            TidyKind::Trash => "files in the trash",
            TidyKind::TempFiles => "files a cut-off write left (.locallibrary-tmp-*)",
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct TidyOptions {
    /// Also delete what's in `.Trashes` / `$RECYCLE.BIN` (for good).
    #[serde(default)]
    pub empty_trash: bool,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct TidyItem {
    /// Relative to the drive root.
    pub path: String,
    pub kind: TidyKind,
    pub is_dir: bool,
    pub files: usize,
    pub bytes: u64,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct TidyPlan {
    pub drive: String,
    pub root: PathBuf,
    pub items: Vec<TidyItem>,
    pub bytes: u64,
    /// Space in the trash that `empty_trash` would free.
    pub trash_bytes: u64,
}

impl TidyPlan {
    pub fn is_noop(&self) -> bool {
        self.items.is_empty()
    }

    /// (kind, files, bytes), in [`TidyKind`] order.
    pub fn by_kind(&self) -> Vec<(TidyKind, usize, u64)> {
        let mut out: Vec<(TidyKind, usize, u64)> = Vec::new();
        for i in &self.items {
            match out.iter_mut().find(|(k, _, _)| *k == i.kind) {
                Some(slot) => {
                    slot.1 += i.files;
                    slot.2 += i.bytes;
                }
                None => out.push((i.kind, i.files, i.bytes)),
            }
        }
        out.sort();
        out
    }
}

fn is_apple_double(path: &Path) -> bool {
    let mut magic = [0u8; 4];
    std::fs::File::open(path).and_then(|mut f| f.read_exact(&mut magic)).is_ok() && magic == APPLE_DOUBLE_MAGIC
}

fn dir_kind(name: &str) -> Option<TidyKind> {
    match name {
        ".Spotlight-V100" => Some(TidyKind::Spotlight),
        ".fseventsd" => Some(TidyKind::FsEvents),
        ".TemporaryItems" => Some(TidyKind::TemporaryItems),
        ".Trashes" => Some(TidyKind::Trash),
        n if n.eq_ignore_ascii_case("$RECYCLE.BIN") => Some(TidyKind::Trash),
        _ => None,
    }
}

fn file_kind(path: &Path, name: &str) -> Option<TidyKind> {
    if name.starts_with("._") && is_apple_double(path) {
        Some(TidyKind::AppleDouble)
    } else if name == ".DS_Store" {
        Some(TidyKind::DsStore)
    } else if name.eq_ignore_ascii_case("Thumbs.db") {
        Some(TidyKind::Thumbnails)
    } else if name.starts_with(paths::TEMP_PREFIX) {
        Some(TidyKind::TempFiles)
    } else {
        None
    }
}

/// Find the clutter on a drive.
pub fn plan(device: &Device, opts: &TidyOptions, ctx: &Ctx<'_>) -> Result<TidyPlan> {
    let root = device.root();
    let mut plan =
        TidyPlan { drive: device.name(), root: root.to_path_buf(), items: Vec::new(), bytes: 0, trash_bytes: 0 };
    ctx.phase_start(Phase::Scan, 0, 0);
    let mut walker = WalkDir::new(root).follow_links(false).min_depth(1).into_iter();
    while let Some(entry) = walker.next() {
        ctx.check()?;
        let Ok(entry) = entry else { continue };
        let name = entry.file_name().to_string_lossy().to_string();
        let rel = entry.path().strip_prefix(root).expect("below root").to_string_lossy().replace('\\', "/");
        if entry.file_type().is_dir() {
            if entry.depth() == 1 && (name == paths::STATE_DIR || name == "System Volume Information") {
                walker.skip_current_dir();
                continue;
            }
            if let Some(kind) = dir_kind(&name) {
                let (files, bytes, _) = dir_size(entry.path());
                walker.skip_current_dir();
                if kind == TidyKind::Trash {
                    plan.trash_bytes += bytes;
                    if !opts.empty_trash {
                        continue;
                    }
                }
                plan.bytes += bytes;
                plan.items.push(TidyItem { path: rel, kind, is_dir: true, files, bytes });
            }
            continue;
        }
        if !entry.file_type().is_file() {
            continue;
        }
        if let Some(kind) = file_kind(entry.path(), &name) {
            let bytes = entry.metadata().map(|m| m.len()).unwrap_or(0);
            plan.bytes += bytes;
            plan.items.push(TidyItem { path: rel, kind, is_dir: false, files: 1, bytes });
        }
    }
    ctx.phase_end(Phase::Scan);
    plan.items.sort_by(|a, b| (a.kind, &a.path).cmp(&(b.kind, &b.path)));
    Ok(plan)
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct TidyReport {
    pub removed: usize,
    pub bytes: u64,
    pub failed: Vec<Failure>,
}

/// Remove the temp files cut-off writes left on the drive. The caller holds its lock, so none of
/// them belongs to a write still going on.
pub(crate) fn remove_leftovers(device: &Device) -> Result<()> {
    let found = plan(device, &TidyOptions::default(), &Ctx::silent())?;
    for item in found.items.iter().filter(|i| i.kind == TidyKind::TempFiles) {
        crate::fsutil::remove_if_exists(&device.root().join(&item.path))?;
    }
    Ok(())
}

/// Delete what [`plan`] found. Each item is checked again first; anything that changed is left.
pub fn apply(plan: &TidyPlan, device: &Device, ctx: &Ctx<'_>) -> Result<TidyReport> {
    let _lock = device.lock()?;
    let root = device.root();
    let mut report = TidyReport { removed: 0, bytes: 0, failed: Vec::new() };
    ctx.phase_start(Phase::Delete, plan.items.len() as u64, 0);
    for item in &plan.items {
        ctx.check()?;
        ctx.emit(Event::Item { phase: Phase::Delete, path: item.path.clone(), bytes: item.bytes });
        if item.path.split('/').any(|c| c.is_empty() || c == "." || c == "..") || item.path.contains('\\') {
            report.failed.push(Failure::new(item.path.clone(), "unexpected path"));
            continue;
        }
        let path = root.join(&item.path);
        let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        let still = if item.is_dir {
            path.is_dir() && dir_kind(&name) == Some(item.kind)
        } else {
            path.is_file() && file_kind(&path, &name) == Some(item.kind)
        };
        if !still {
            continue;
        }
        let result = if item.is_dir { std::fs::remove_dir_all(&path) } else { std::fs::remove_file(&path) };
        match result {
            Ok(()) => {
                report.removed += item.files.max(1);
                report.bytes += item.bytes;
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => report.failed.push(Failure::new(item.path.clone(), Error::Io { path, source: e })),
        }
    }
    ctx.phase_end(Phase::Delete);
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_and_removes_clutter_only() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let w = |p: &str, b: &[u8]| {
            let p = root.join(p);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, b).unwrap();
        };
        let mut ad = APPLE_DOUBLE_MAGIC.to_vec();
        ad.extend_from_slice(&[0; 60]);
        w("Contents/a.mp3", b"audio");
        w("Contents/._a.mp3", &ad);
        w("Contents/._not-apple-double", b"real data");
        w(".DS_Store", b"x");
        w(".Spotlight-V100/Store-V2/index", &[0; 100]);
        w(".Trashes/501/old.mp3", &[0; 1000]);
        w(".locallibrary/._state", &ad);
        w("System Volume Information/IndexerVolumeGuid", b"g");
        let d = Device::open(root).unwrap();
        let ctx = Ctx::silent();

        let p = plan(&d, &TidyOptions::default(), &ctx).unwrap();
        let paths: Vec<&str> = p.items.iter().map(|i| i.path.as_str()).collect();
        assert_eq!(paths, ["Contents/._a.mp3", ".DS_Store", ".Spotlight-V100"]);
        assert_eq!(p.trash_bytes, 1000);

        let with_trash = plan(&d, &TidyOptions { empty_trash: true }, &ctx).unwrap();
        assert_eq!(with_trash.items.len(), 4);

        let r = apply(&with_trash, &d, &ctx).unwrap();
        assert!(r.failed.is_empty(), "{:?}", r.failed);
        assert!(root.join("Contents/a.mp3").exists());
        assert!(root.join("Contents/._not-apple-double").exists());
        assert!(!root.join("Contents/._a.mp3").exists());
        assert!(!root.join(".Trashes").exists());
        assert!(root.join(".locallibrary/._state").exists());
        assert!(root.join("System Volume Information/IndexerVolumeGuid").exists());
        assert!(plan(&d, &TidyOptions { empty_trash: true }, &ctx).unwrap().is_noop());
    }

    #[test]
    fn finds_what_cut_off_writes_left() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        for p in ["Contents/A/.locallibrary-tmp-x1y2", "PIONEER/rekordbox/.locallibrary-tmp-ab", "Contents/A/a.mp3"] {
            std::fs::create_dir_all(root.join(p).parent().unwrap()).unwrap();
            std::fs::write(root.join(p), b"half a file").unwrap();
        }
        let d = Device::open(root).unwrap();
        let p = plan(&d, &TidyOptions::default(), &Ctx::silent()).unwrap();
        assert_eq!(p.by_kind(), [(TidyKind::TempFiles, 2, 22)]);
        remove_leftovers(&d).unwrap();
        assert!(plan(&d, &TidyOptions::default(), &Ctx::silent()).unwrap().is_noop());
        assert!(root.join("Contents/A/a.mp3").is_file());
    }

    /// A plan is checked again item by item: a path that leaves the drive is refused, and what
    /// is no longer the clutter it was is left.
    #[test]
    fn applying_leaves_what_changed_since_and_refuses_odd_paths() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("Contents")).unwrap();
        for p in ["Thumbs.db", "Contents/.DS_Store", "Contents/notes.txt"] {
            std::fs::write(root.join(p), b"x").unwrap();
        }
        let d = Device::open(root).unwrap();
        let mut p = plan(&d, &TidyOptions::default(), &Ctx::silent()).unwrap();
        let found: Vec<(&str, TidyKind)> = p.items.iter().map(|i| (i.path.as_str(), i.kind)).collect();
        assert_eq!(found, [("Contents/.DS_Store", TidyKind::DsStore), ("Thumbs.db", TidyKind::Thumbnails)]);
        // The .DS_Store is now a folder the DJ made.
        std::fs::remove_file(root.join("Contents/.DS_Store")).unwrap();
        std::fs::create_dir(root.join("Contents/.DS_Store")).unwrap();
        let odd =
            |path: &str| TidyItem { path: path.into(), kind: TidyKind::Thumbnails, is_dir: false, files: 1, bytes: 1 };
        p.items.extend([odd("../Thumbs.db"), odd("Contents//Thumbs.db"), odd("Contents\\Thumbs.db")]);

        let r = apply(&p, &d, &Ctx::silent()).unwrap();
        assert_eq!(r.removed, 1);
        let failed: Vec<(&str, &str)> = r.failed.iter().map(|f| (f.path.as_str(), f.error.as_str())).collect();
        assert_eq!(
            failed,
            [
                ("../Thumbs.db", "unexpected path"),
                ("Contents//Thumbs.db", "unexpected path"),
                ("Contents\\Thumbs.db", "unexpected path")
            ]
        );
        assert!(!root.join("Thumbs.db").exists() && root.join("Contents/.DS_Store").is_dir());
        assert!(root.join("Contents/notes.txt").is_file());
    }
}
