//! Writing rekordbox's collection (`master.db` on this computer, see [`crate::masterdb`]) to a
//! drive or a folder, as rekordbox's Export does, so a library can be had from this computer
//! without a drive rekordbox exported to. A folder written this way is a drive like any other:
//! `pull` saves it in the store, and `library push` writes it to any drive.
//!
//! The export holds every track (or those of the chosen playlists), the playlists and folders
//! (intelligent playlists, whose rules aren't read, are left out), cues and My Tags. Audio goes in
//! `Contents/<artist>/<album>/`, and each track's analysis files (`.DAT`, `.EXT`, `.2EX`) are
//! rekordbox's own from its `share` folder, in a `PIONEER/USBANLZ` folder named from the audio
//! path, with that path written into them. Artwork keeps rekordbox's export layout
//! (`PIONEER/Artwork/00001/a1.jpg`, with its `_m` copy), shared by both databases.
//!
//! Tracks Cloud Library Sync keeps in Dropbox or Google Drive are read from the service's folder
//! (see [`crate::masterdb::CloudFolders`]). An online-only file is downloaded as it's read; the
//! plan says how many there are, and how big. Analysis or artwork that hasn't finished syncing is
//! copied as it is, with a warning, and a track whose audio isn't on this computer is left out.
//!
//! Writing goes through [`crate::library_write`], as [`crate::pack`] does: only files that
//! differ are copied, so exporting again is quick, and a run stopped part way finishes when it's
//! run again. The databases are built on the target from what was copied (see
//! `convert::build_databases`); `exportExt.pdb` isn't made, so players that read
//! `export.pdb` don't show My Tags (OneLibrary has them). The target remembers what it was
//! exported from (`.locallibrary/collection.json`) and which image got which key, so images keep
//! theirs. A library it held that wasn't exported from the collection (or loose files the export
//! would remove) is only replaced when asked, and what only it has (history lists players
//! recorded, changes since the last export) is saved in the store first, as [`crate::pack`] does.

use indexmap::IndexMap;
use rustc_hash::FxHashSet;
use std::path::PathBuf;

use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::artwork::{image_key, medium_variant};
use crate::device::{Device, LineageEntry, LineageOp};
use crate::error::{Action, Reason};
use crate::hash::Hash;
use crate::library_write::{self, FileCopy, Planned, Source, Written};
use crate::masterdb::{CloudFolders, Collection, TagCategory};
use crate::model::{Library, Track};
use crate::pack::Unsaved;
use crate::paths;
use crate::progress::{Ctx, Event, Phase};
use crate::store::{SnapshotInfo, Store};
use crate::sync::PullOptions;
use crate::tags::DbTags;
use crate::{Error, Result, backup, convert, edit, fsutil, merge};

const RECORD_FILE: &str = "collection.json";

/// The analysis files a track's `.DAT` comes with, in rekordbox's export (its `.3EX`, which only
/// rekordbox reads, stays behind).
const ANALYSIS_EXTENSIONS: [&str; 3] = ["DAT", "EXT", "2EX"];

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct CollectionExportOptions {
    /// Playlists or folders (paths) to export, with the tracks they use. Empty: every track and
    /// playlist, or the selection the target was last exported with.
    #[serde(default)]
    pub playlists: Vec<String>,
    /// Export every track and playlist, whatever the target was exported with last.
    #[serde(default)]
    pub all: bool,
    /// Replace a library the target holds that wasn't exported from the collection, or the
    /// files on a target with no library.
    #[serde(default)]
    pub replace: bool,
    /// Write to an archive or retired drive (see [`crate::fleet::check_target`]).
    #[serde(default)]
    pub overwrite_archive: bool,
}

/// What a target was exported from, kept on the target.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CollectionExportRecord {
    /// The `master.db` it was exported from.
    pub collection: PathBuf,
    /// The playlists chosen; empty for the whole collection.
    pub playlists: Vec<String>,
    pub at: DateTime<Utc>,
    /// The key each image got, by its path in rekordbox's `share` folder (lower-cased), so an
    /// image keeps its key from one export to the next.
    #[serde(default)]
    pub artwork: IndexMap<String, String>,
}

pub fn record(device: &Device) -> Result<Option<CollectionExportRecord>> {
    fsutil::read_json_opt(&device.state_dir().join(RECORD_FILE))
}

/// The drive no longer holds an export of the collection: a sync, push or pack wrote another
/// library over it.
pub fn forget(device: &Device) -> Result<()> {
    fsutil::remove_if_exists(&device.state_dir().join(RECORD_FILE)).map(|_| ())
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct CollectionExportPlan {
    /// The `master.db` exported.
    pub collection: PathBuf,
    pub target: String,
    pub target_root: PathBuf,
    /// The playlists chosen; empty for the whole collection.
    pub playlists: Vec<String>,
    pub tracks: usize,
    /// Playlists exported (not counting folders).
    pub playlist_count: usize,
    /// Size of the exported library's files.
    pub library_bytes: u64,
    pub copies: Vec<FileCopy>,
    pub copy_bytes: u64,
    /// Files on the target the exported library doesn't use.
    pub deletes: Vec<FileCopy>,
    pub delete_bytes: u64,
    pub unchanged: usize,
    /// Audio files to copy that are online only in Dropbox or Google Drive: copying downloads
    /// them.
    pub downloads: usize,
    pub download_bytes: u64,
    /// Tracks left out because their audio isn't on this computer ("Artist - Title").
    pub missing: Vec<String>,
    /// Tracks whose analysis or artwork Cloud Library Sync hasn't finished bringing to this
    /// computer.
    pub syncing: Vec<String>,
    /// Tracks with an analysis file that can't be read: exported without it.
    pub unreadable: Vec<String>,
    /// The target holds a library that wasn't exported from the collection, or files the export
    /// removes though it holds no library.
    pub replaces_library: bool,
    /// The folder exported to was made for this export (see
    /// [`crate::api::collection_export_plan`]): a caller that doesn't apply the plan takes it away.
    pub made_folder: bool,
    /// What the target holds that the export replaces and only it has (history lists players
    /// recorded, changes since the last export): saved in the store first.
    pub unsaved: Option<Unsaved>,
    /// Space the export needs on the target: the files it copies, its new databases and the
    /// backup of what it replaces.
    pub needed_bytes: u64,
    pub free_space: Option<u64>,
    pub fits: bool,
    pub warnings: Vec<String>,
    /// The exported library: its paths are the target's.
    #[serde(skip)]
    library: Library,
    #[serde(skip)]
    tags: Vec<TagCategory>,
    #[serde(skip)]
    artwork: IndexMap<String, String>,
    #[serde(skip)]
    written: Planned,
}

impl CollectionExportPlan {
    pub fn is_noop(&self) -> bool {
        false // the databases are always rebuilt
    }
}

#[derive(Debug, Clone, Default, Serialize, JsonSchema)]
pub struct CollectionExportReport {
    pub tracks: usize,
    pub playlists: usize,
    pub copied: usize,
    pub copied_bytes: u64,
    pub deleted: usize,
    pub deleted_bytes: u64,
    /// OneLibrary cue rows made from the analysis files.
    pub cues: usize,
    /// Tracks with My Tags.
    pub tagged_tracks: usize,
    pub fingerprint: Option<Hash>,
    /// The target's state, saved in the store before the export (see
    /// [`CollectionExportPlan::unsaved`]).
    pub saved: Option<SnapshotInfo>,
    /// The backup of what the export replaced (its databases, analysis and artwork).
    pub backup: Option<String>,
}

/// Plan writing `collection` (its tracks' files found through `cloud`) to `target`.
pub fn plan(
    collection: &Collection,
    cloud: &CloudFolders,
    target: &Device,
    opts: &CollectionExportOptions,
    ctx: &Ctx<'_>,
) -> Result<CollectionExportPlan> {
    let previous = record(target)?;
    // An export cut off before it recorded itself counts as one.
    let interrupted = target.state().and_then(|s| s.pending.as_ref()).is_some_and(|p| p.op == LineageOp::Export);
    let exported = previous.is_some() || interrupted;
    let playlists = match (&opts.playlists[..], &previous) {
        _ if opts.all => Vec::new(),
        ([], Some(r)) => r.playlists.clone(),
        (p, _) => p.to_vec(),
    };
    let mut warnings = Vec::new();
    let chosen = choose(collection, &playlists, &mut warnings)?;
    // Each track's analysis files are read to write its path into them, which takes a while.
    let mut layout = Layout { artwork: previous.map(|r| r.artwork).unwrap_or_default(), ..Default::default() };
    layout.used_keys = layout.artwork.values().map(|k| k.to_lowercase()).collect();
    ctx.phase_start(Phase::Scan, chosen.tracks.len() as u64, 0);
    for t in &chosen.tracks {
        ctx.check()?;
        ctx.emit(Event::Item { phase: Phase::Scan, path: t.file_path.clone(), bytes: 0 });
        layout.add(collection, cloud, t)?;
    }
    ctx.phase_end(Phase::Scan);
    let mut library = chosen;
    let kept: FxHashSet<u32> = layout.tracks.iter().map(|t| t.id).collect();
    library.tracks = std::mem::take(&mut layout.tracks);
    for p in &mut library.playlists {
        p.track_ids.retain(|id| kept.contains(id));
    }
    convert::check_device_library(&library)?;
    if let Some(first) = layout.missing.first() {
        warnings.push(format!(
            "{} track(s) aren't exported: their audio isn't on this computer (e.g. {first})",
            layout.missing.len()
        ));
    }
    if let Some(first) = layout.syncing.first() {
        warnings.push(format!(
            "Cloud Library Sync hasn't finished bringing the analysis or artwork of {} track(s) to this computer \
             (e.g. {first}); they're exported as they are",
            layout.syncing.len()
        ));
    }
    if let Some(first) = layout.unreadable.first() {
        warnings.push(format!(
            "{} track(s) have analysis files that can't be read (e.g. {first}); they're exported without them",
            layout.unreadable.len()
        ));
    }
    let tags = collection.tags.clone();

    let target_lib = target.library().ok();
    let scanned = target.scan_fingerprinted(target_lib.as_ref(), ctx)?;
    let unsaved = crate::pack::unsaved(target, &scanned, exported, LineageOp::Export, &[&library])?;
    // Rows take well under 1 KiB per track or entry, including lookups and page slack.
    let entries: usize = library.playlists.iter().map(|p| p.track_ids.len()).sum();
    let database = 256 * 1024 + (library.tracks.len() + entries) as u64 * 1024;
    let databases = Written { pdb: Some(database), ext: None, plus: Some(database) };
    let written = library_write::plan(target, &scanned.tree, &layout.files, &[], databases, false);
    let downloads: Vec<&FileCopy> = written.copies.iter().filter(|c| layout.online_only.contains(&c.path)).collect();
    // Without a record of an earlier export, whatever the target holds (a library, or loose audio
    // the export would remove) is only replaced when asked.
    let replaces_library = !exported && (target.has_export() || !written.deletes.is_empty());
    if replaces_library && !opts.replace {
        let what = match target.has_export() {
            true => "a library that wasn't exported from rekordbox's collection",
            false => "files the export would remove",
        };
        return Err(Error::conflict(
            Reason::Exists,
            format!(
                "{} holds {what}; replace it to export over it (pull it into the store first to keep a library)",
                target.name()
            ),
        )
        .with("drive", target.root().display().to_string())
        .action(Action::retry(serde_json::json!({ "replace": true }), "Replace it")));
    }
    if replaces_library {
        warnings.push(format!("{} holds a library or files the export replaces", target.name()));
    }
    if let Some(u) = &unsaved {
        warnings.push(format!(
            "{} holds {} that the export replaces; it's saved in the store first",
            target.name(),
            u.describe()
        ));
    }
    warnings.extend(edit::journal::forget_warning(target, None, "exporting the collection"));
    if !written.fits {
        warnings.push(format!(
            "the export needs about {} bytes (its files, databases and backup) but only {} are free on {}",
            written.needed_bytes,
            written.free_space.unwrap_or(0),
            target.name()
        ));
    }
    Ok(CollectionExportPlan {
        collection: collection.path.clone(),
        target: target.name(),
        target_root: target.root().to_path_buf(),
        playlists,
        tracks: library.tracks.len(),
        playlist_count: library.playlists.iter().filter(|p| !p.is_folder).count(),
        library_bytes: written.library_bytes,
        copies: written.copies.clone(),
        copy_bytes: written.copy_bytes,
        deletes: written.deletes.clone(),
        delete_bytes: written.delete_bytes,
        unchanged: written.unchanged,
        downloads: downloads.len(),
        download_bytes: downloads.iter().map(|c| c.size).sum(),
        missing: layout.missing,
        syncing: layout.syncing,
        unreadable: layout.unreadable,
        replaces_library,
        made_folder: false,
        unsaved,
        needed_bytes: written.needed_bytes,
        free_space: written.free_space,
        fits: written.fits,
        warnings,
        library,
        tags,
        artwork: layout.artwork,
        written,
    })
}

/// The collection's tracks and playlists to export: those of `playlists` (with the folders above
/// them), or everything. Intelligent playlists and history lists are left out.
fn choose(collection: &Collection, playlists: &[String], warnings: &mut Vec<String>) -> Result<Library> {
    let lib = &collection.library;
    let smart: FxHashSet<u32> = collection.smart_playlists.iter().copied().collect();
    let keep = match playlists {
        [] => lib.playlists.iter().map(|p| p.id).collect(),
        specs => edit::chosen_nodes(lib, specs)?,
    };
    let left_out: Vec<String> =
        collection.smart_playlists.iter().filter(|id| keep.contains(id)).map(|id| lib.playlist_path(*id)).collect();
    if let Some(first) = left_out.first() {
        warnings.push(format!(
            "{} intelligent playlist(s) aren't exported (e.g. {first}): their rules aren't read",
            left_out.len()
        ));
    }
    let chosen: Vec<_> =
        lib.playlists.iter().filter(|p| keep.contains(&p.id) && !smart.contains(&p.id)).cloned().collect();
    let tracks = match playlists {
        [] => lib.tracks.clone(),
        _ => {
            let used: FxHashSet<u32> = chosen.iter().flat_map(|p| p.track_ids.iter().copied()).collect();
            lib.tracks.iter().filter(|t| used.contains(&t.id)).cloned().collect()
        }
    };
    Ok(Library { tracks, playlists: chosen, ..Default::default() })
}

/// Where the exported library's files go on the target, and where they come from.
#[derive(Default)]
struct Layout {
    /// The exported tracks, their paths the target's.
    tracks: Vec<Track>,
    files: IndexMap<String, Source>,
    /// Keys (lower-cased) taken on the target.
    taken: FxHashSet<String>,
    /// Artwork key on the target by image path in the `share` folder (lower-cased): the last
    /// export's, then new ones.
    artwork: IndexMap<String, String>,
    /// Artwork keys (lower-cased) given out, the last export's included.
    used_keys: FxHashSet<String>,
    /// Audio keys whose source is online only.
    online_only: FxHashSet<String>,
    missing: Vec<String>,
    syncing: Vec<String>,
    unreadable: Vec<String>,
}

impl Layout {
    fn add(&mut self, collection: &Collection, cloud: &CloudFolders, t: &Track) -> Result<()> {
        let audio = collection.audio_file(t, cloud).and_then(|p| std::fs::metadata(&p).ok().map(|m| (p, m)));
        let Some((audio, meta)) = audio.filter(|(_, m)| m.is_file()) else {
            self.missing.push(t.display_name());
            return Ok(());
        };
        let folder = |s: &str, unknown: &str| match s.trim() {
            "" => unknown.to_string(),
            s => paths::sanitize_component(s),
        };
        let name = audio.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let wanted = format!(
            "Contents/{}/{}/{}",
            folder(&t.artist, "UnknownArtist"),
            folder(&t.album, "UnknownAlbum"),
            paths::sanitize_component(&name)
        );
        let key = merge::unique_key(&wanted, &mut self.taken, &|_| false);
        let device_path = format!("/{key}");
        if fsutil::is_online_only(&meta) {
            self.online_only.insert(key.clone());
        }
        self.files.insert(key, Source::file(audio, meta.len(), mtime(&meta)));

        let mut track = t.clone();
        track.file_path = device_path.clone();
        track.analyze_path = String::new();
        track.artwork_path = None;
        let (mut syncing, mut unreadable) = (false, false);
        if let Some((stem, _)) = t.analyze_path.rsplit_once('.').filter(|_| !t.analyze_path.is_empty()) {
            let mut salt = 0;
            let mut dir = merge::analysis_dir(&device_path, salt);
            while self.taken.contains(&format!("{dir}/anlz0000.dat").to_lowercase()) {
                salt += 1;
                dir = merge::analysis_dir(&device_path, salt);
            }
            for ext in ANALYSIS_EXTENSIONS {
                let share = format!("{stem}.{ext}");
                syncing |= collection.still_syncing(&share);
                let Ok(meta) = std::fs::metadata(collection.share_file(&share)) else { continue };
                let to = format!("{dir}/ANLZ0000.{ext}");
                let Ok(source) = Source::analysis(collection.share_file(&share), mtime(&meta), &device_path, 0) else {
                    unreadable = true;
                    continue;
                };
                self.taken.insert(to.to_lowercase());
                self.files.insert(to.clone(), source);
                if ext == "DAT" {
                    track.analyze_path = format!("/{to}");
                }
            }
        }
        if let Some(image) = &t.artwork_path {
            syncing |= collection.still_syncing(image);
            track.artwork_path = self.artwork(collection, image).map(|k| format!("/{k}"));
        }
        if syncing {
            self.syncing.push(t.display_name());
        }
        if unreadable {
            self.unreadable.push(t.display_name());
        }
        self.tracks.push(track);
        Ok(())
    }

    /// The key of the image at `image` in the `share` folder on the target (with its `_m` copy,
    /// when rekordbox made one), or `None` if it isn't there.
    fn artwork(&mut self, collection: &Collection, image: &str) -> Option<String> {
        let src = collection.share_file(image);
        let meta = std::fs::metadata(&src).ok().filter(|m| m.is_file())?;
        let key = match self.artwork.get(&image.to_lowercase()) {
            Some(key) => key.clone(),
            None => {
                let ext = image.rsplit_once('.').map(|(_, e)| e.to_lowercase()).unwrap_or_else(|| "jpg".into());
                let key = (1..)
                    .map(|n| image_key('a', n, &ext))
                    .find(|k| !self.used_keys.contains(&k.to_lowercase()))
                    .expect("unbounded");
                self.used_keys.insert(key.to_lowercase());
                self.artwork.insert(image.to_lowercase(), key.clone());
                key
            }
        };
        if self.files.contains_key(&key) {
            return Some(key);
        }
        self.files.insert(key.clone(), Source::file(src, meta.len(), mtime(&meta)));
        let medium = collection.share_file(&medium_variant(image));
        if let Ok(m) = std::fs::metadata(&medium) {
            self.files.insert(medium_variant(&key), Source::file(medium, m.len(), mtime(&m)));
        }
        Some(key)
    }
}

fn mtime(meta: &std::fs::Metadata) -> i64 {
    filetime::FileTime::from_last_modification_time(meta).unix_seconds()
}

/// Write the export: the target's history lists saved in the store first when it has some
/// (refused without a store) and its databases backed up, then the files, then the databases built
/// from them, then removal of what the exported library doesn't use.
pub fn apply(
    plan: &CollectionExportPlan,
    target: &mut Device,
    store: Option<&Store>,
    ctx: &Ctx<'_>,
) -> Result<CollectionExportReport> {
    if !plan.fits {
        return Err(Error::NoSpace {
            path: plan.target_root.clone(),
            needed: plan.needed_bytes,
            available: plan.free_space.unwrap_or(0),
        });
    }
    let mut report = CollectionExportReport::default();
    if let Some(u) = &plan.unsaved {
        let drive = plan.target_root.display().to_string();
        let store =
            store.ok_or_else(|| {
                Error::conflict(
                Reason::NotSaved,
                format!(
                    "{} holds {} that the export would replace; keeping it needs the store, so pull the drive first",
                    plan.target,
                    u.describe()
                ),
            )
            .with("drive", &drive)
            .action(Action::call("pull", serde_json::json!({ "drive": drive }), format!("Pull {}", plan.target)))
            })?;
        let opts = PullOptions {
            library: None,
            message: Some(format!("{} before exporting rekordbox's collection to it: {}", plan.target, u.describe())),
            force: true,
        };
        report.saved = crate::sync::pull(store, target, &opts, ctx)?.snapshot;
    }
    let _lock = crate::commit::lock(target, None)?;
    if target.has_export() {
        let reason = "before exporting rekordbox's collection".to_string();
        report.backup = Some(backup::create(target, &reason, &plan.written.backed_up, None)?.id);
    }
    target.begin(LineageOp::Export, "export rekordbox's collection", report.backup.clone())?;
    let identity = crate::player::stash(target, true)?;
    ctx.phase_start(Phase::Copy, plan.written.copies.len() as u64, plan.written.copy_bytes);
    (report.copied, report.copied_bytes) = library_write::copy(target, &plan.written, ctx)?;
    ctx.phase_end(Phase::Copy);

    // The databases, built from the files now on the target and checked before they're swapped
    // in. From the swap on, the export is made: recording it isn't cancellable.
    let tags = DbTags::collection(&plan.tags, &plan.library);
    let (plus, pdb, built) = convert::build_databases(target, &plan.library, Some(&tags), ctx)?;
    let ctx = &ctx.uncancellable();
    let databases = library_write::Databases { pdb: Some(pdb), ext: None, plus: Some(plus) };
    library_write::swap_databases(target, &databases)?;
    crate::player::preserve(target, identity.as_ref())?;
    (report.deleted, report.deleted_bytes) = library_write::delete(target, &plan.written, ctx)?;

    if let Some(id) = &report.backup {
        backup::seal(target, id)?;
    }
    let library = target.library()?;
    let scanned = target.scan_fingerprinted(Some(&library), ctx)?;
    report.fingerprint = scanned.fingerprint;
    target.finish(scanned.fingerprint.map(|fp| LineageEntry {
        fingerprint: fp,
        at: Utc::now(),
        op: LineageOp::Export,
        snapshot: None,
        source: None,
        tracks: library.tracks.len(),
    }))?;
    let rec = CollectionExportRecord {
        collection: plan.collection.clone(),
        playlists: plan.playlists.clone(),
        at: Utc::now(),
        artwork: plan.artwork.clone(),
    };
    fsutil::write_json_atomic(&target.state_dir().join(RECORD_FILE), &rec)?;
    crate::pack::forget(target)?;
    edit::journal::forget(target)?;
    if report.backup.is_some() {
        backup::prune(target)?;
    }
    report.tracks = library.tracks.len();
    report.playlists = plan.playlist_count;
    report.cues = built.cues;
    report.tagged_tracks = built.tagged_tracks;
    Ok(report)
}

#[cfg(test)]
mod tests;
