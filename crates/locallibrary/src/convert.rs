//! Converting a drive between rekordbox's two databases, without rekordbox: OneLibrary
//! (`exportLibrary.db`, the only one the CDJ-3000X, CDJ-1500X, OPUS-QUAD, OMNIS-DUO, XDJ-AZ and
//! XDJ-AN read) from `export.pdb`, as rekordbox's "Convert from Device Library" does, or
//! `export.pdb` (every other player) from OneLibrary.
//!
//! Tracks, artwork, playlists and history playlists are converted. Audio and analysis files are
//! shared by both databases, so nothing else is copied; artwork gets the other database's file
//! names (`a<id>.jpg` for `export.pdb`, `b<id>.jpg` for OneLibrary). OneLibrary's `cue` rows,
//! which players that read only OneLibrary use, are made from the tracks' analysis files.
//!
//! With `replace`, a database the drive already has is rebuilt from the other one, which is how
//! drift between the two is fixed (see [`crate::device::LibraryDrift`]). Rebuilding OneLibrary
//! keeps the rows it has for tracks `export.pdb` also lists (every column, cues, My Tags and
//! artwork), so only what `export.pdb` can't express is lost for tracks that were missing.
//!
//! My Tags come along: `exportExt.pdb`'s into a new OneLibrary (its categories and tags with
//! their ids, and every track's tags). A new `export.pdb` gives its tracks new ids, so the links
//! in `exportExt.pdb` move to them by file path (links to tracks it doesn't list go), and
//! OneLibrary's tags are added. A drive without `exportExt.pdb` doesn't get one.
//!
//! A new `export.pdb` starts from a real rekordbox export (rekordbox-pdb's one-track test file,
//! MIT) with its rows removed, which keeps rekordbox's colours and browse columns. A new OneLibrary
//! database starts from rekordbox's schema and default rows ([`OneLibrary::create`]).

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use rekordbox_pdb::{PdbEditor, TableType};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::artwork::{free_image_key, medium_variant};
use crate::commit::{self, Changes, NewDatabases, NewFile, Options};
use crate::cue_edit;
use crate::device::{Device, LineageOp};
use crate::error::{Action, Reason};
use crate::hash::Hash;
use crate::merge::PlaylistDb;
use crate::model::{Library, Playlist, Track};
use crate::onelibrary::{ContentRecord, OneLibrary};
use crate::paths;
use crate::pdb;
use crate::progress::{Ctx, Event, Phase};
use crate::tags::{self, DbTags, Wanted};
use crate::{Error, Result, fsutil};

/// A real rekordbox export holding one track, emptied to start new `export.pdb` files from. It's
/// rekordbox-pdb's `tests/data/one-song-export.pdb` (MIT, see `vendor/rekordbox-pdb/LICENSE`),
/// kept in this package so the build doesn't reach into another crate's tests.
const PDB_TEMPLATE: &[u8] = include_bytes!("convert/template.pdb");

/// Which database to build.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Target {
    /// `exportLibrary.db`, from `export.pdb`.
    OneLibrary,
    /// `export.pdb`, from `exportLibrary.db`.
    DeviceLibrary,
}

impl Target {
    pub fn file(self) -> &'static str {
        match self {
            Target::OneLibrary => "exportLibrary.db",
            Target::DeviceLibrary => "export.pdb",
        }
    }

    pub fn source_file(self) -> &'static str {
        match self {
            Target::OneLibrary => "export.pdb",
            Target::DeviceLibrary => "exportLibrary.db",
        }
    }
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct ConvertOptions {
    pub to: Target,
    /// Rebuild the target database if the drive already has one.
    #[serde(default)]
    pub replace: bool,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct ConvertPlan {
    pub drive: String,
    pub root: PathBuf,
    pub to: Target,
    /// The drive already has that database, and it's rebuilt.
    pub replaces: bool,
    pub tracks: usize,
    pub playlists: usize,
    pub folders: usize,
    pub history_playlists: usize,
    /// Artwork images copied under the other database's file names.
    pub artwork: usize,
    /// Tracks whose My Tags come along from the other database's tags.
    pub tagged_tracks: usize,
    /// Space needed: the new database, copied artwork, and the backup of the database files.
    pub needed_bytes: u64,
    pub free_space: Option<u64>,
    pub fits: bool,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, JsonSchema)]
pub struct ConvertReport {
    pub tracks: usize,
    pub playlists: usize,
    pub history_playlists: usize,
    pub artwork: usize,
    /// Tracks with My Tags in the converted database's tags.
    pub tagged_tracks: usize,
    /// OneLibrary cue rows made from analysis files.
    pub cues: usize,
    pub backup: Option<String>,
    pub fingerprint: Option<Hash>,
}

/// The library to convert from.
fn source_library(device: &Device, to: Target) -> Result<Library> {
    match to {
        Target::OneLibrary if device.has_device_library() => pdb::read_library(&device.path(paths::EXPORT_PDB)),
        Target::DeviceLibrary if device.has_library_plus() => device.library_plus()?.library(),
        _ => Err(Error::invalid(format!("{} has no {} to convert from", device.name(), to.source_file()))),
    }
}

fn has_target(device: &Device, to: Target) -> bool {
    match to {
        Target::OneLibrary => device.has_library_plus(),
        Target::DeviceLibrary => device.has_device_library(),
    }
}

/// The library the target database holds now, if it has one that can be read.
fn existing_library(device: &Device, to: Target) -> Option<Library> {
    match to {
        Target::OneLibrary if device.has_library_plus() => device.library_plus().and_then(|o| o.library()).ok(),
        Target::DeviceLibrary if device.has_device_library() => pdb::read_library(&device.path(paths::EXPORT_PDB)).ok(),
        _ => None,
    }
}

fn fold(s: &str) -> String {
    paths::key(s).to_lowercase()
}

pub fn plan(device: &Device, opts: &ConvertOptions, _ctx: &Ctx<'_>) -> Result<ConvertPlan> {
    let source = source_library(device, opts.to)?;
    let replaces = has_target(device, opts.to);
    if replaces && !opts.replace {
        let rebuild = format!("Rebuild {} from {}", opts.to.file(), opts.to.source_file());
        return Err(Error::conflict(
            Reason::Exists,
            format!(
                "{} already has {}; replace it to rebuild it from {}",
                device.name(),
                opts.to.file(),
                opts.to.source_file()
            ),
        )
        .with("path", opts.to.file())
        .action(Action::retry(serde_json::json!({ "replace": true }), rebuild)));
    }
    if opts.to == Target::DeviceLibrary {
        check_device_library(&source)?;
    }
    let mut warnings = Vec::new();
    let existing = if replaces { existing_library(device, opts.to) } else { None };
    if replaces {
        match &existing {
            Some(existing) => {
                let keep: HashSet<String> = source.tracks.iter().map(|t| fold(&t.file_path)).collect();
                let dropped = existing.tracks.iter().filter(|t| !keep.contains(&fold(&t.file_path))).count();
                if dropped > 0 {
                    warnings.push(format!(
                        "{dropped} track(s) only {} lists will no longer be in it (their files stay on the drive)",
                        opts.to.file()
                    ));
                }
            }
            None => warnings.push(format!("the current {} can't be read; it's rebuilt from scratch", opts.to.file())),
        }
    }
    let tagged_tracks = match opts.to {
        Target::OneLibrary => match DbTags::ext_of(device, &source) {
            Ok(ext) => ext.map_or(0, |t| t.tagged()),
            Err(e) => {
                warnings.push(format!("{e}; its My Tags aren't converted"));
                0
            }
        },
        Target::DeviceLibrary => {
            warnings.push("hot cue bank lists aren't converted".to_string());
            device_library_tags_plan(device, &source, existing.as_ref(), &mut warnings)
        }
    };
    warnings.extend(device.rekordbox_sync_warning(
        "it has no record of the converted database's playlists and may export them again when it next syncs",
    ));

    let mut artwork: HashSet<String> = HashSet::new();
    let mut missing: HashSet<String> = HashSet::new();
    let mut artwork_bytes = 0;
    let file_size = |k: &str| device.checked_path(k).ok().and_then(|p| std::fs::metadata(p).ok()).map(|m| m.len());
    for a in source.tracks.iter().filter_map(|t| t.artwork_path.as_ref()) {
        if artwork.contains(&fold(a)) || missing.contains(&fold(a)) {
            continue;
        }
        match file_size(&paths::key(a)) {
            Some(n) => {
                artwork.insert(fold(a));
                artwork_bytes += n + file_size(&medium_variant(&paths::key(a))).unwrap_or(0);
            }
            None => {
                missing.insert(fold(a));
            }
        }
    }
    if !missing.is_empty() {
        warnings.push(format!(
            "{} artwork image(s) the tracks point at are missing on the drive; those tracks get no artwork",
            missing.len()
        ));
    }
    let cluster = fsutil::cluster_size(device.root());
    let round = |n: u64| n.div_ceil(cluster) * cluster;
    let size = |key| std::fs::metadata(device.path(key)).map(|m| m.len()).unwrap_or(0);
    let entries: usize = source.playlists.iter().map(|p| p.track_ids.len()).sum();
    // Rows take well under 1 KiB per track or entry, including lookups and page slack.
    let database = 256 * 1024 + (source.tracks.len() + entries) as u64 * 1024;
    let backup: u64 = paths::DATABASE_FILES.iter().map(|k| round(size(k))).sum();
    let needed_bytes = round(database) + round(artwork_bytes) + artwork.len() as u64 * 2 * cluster + backup;
    let free_space = fsutil::available_space(device.root());
    let fits = free_space.is_none_or(|f| needed_bytes <= f);
    if !fits {
        warnings.push(format!("needs about {needed_bytes} bytes but only {} are free", free_space.unwrap_or(0)));
    }
    Ok(ConvertPlan {
        drive: device.name(),
        root: device.root().to_path_buf(),
        to: opts.to,
        replaces,
        tracks: source.tracks.len(),
        playlists: source.playlists.iter().filter(|p| !p.is_folder).count(),
        folders: source.playlists.iter().filter(|p| p.is_folder).count(),
        history_playlists: source.history.iter().filter(|h| !h.is_folder).count(),
        artwork: artwork.len(),
        tagged_tracks,
        needed_bytes,
        free_space,
        fits,
        warnings,
    })
}

pub fn apply(plan: &ConvertPlan, device: &mut Device, opts: &ConvertOptions, ctx: &Ctx<'_>) -> Result<ConvertReport> {
    if !plan.fits {
        return Err(Error::NoSpace {
            path: plan.root.clone(),
            needed: plan.needed_bytes,
            available: plan.free_space.unwrap_or(0),
        });
    }
    let _lock = commit::lock(device, None)?;
    let source = source_library(device, opts.to)?;
    if source.tracks.len() != plan.tracks || has_target(device, opts.to) != plan.replaces {
        return Err(Error::stale(device.name(), "changed"));
    }
    // My Tags in exportExt.pdb name the export.pdb tracks there are now.
    let old_pdb = match opts.to {
        Target::DeviceLibrary if plan.replaces => existing_library(device, opts.to),
        _ => None,
    };
    let ext = match opts.to {
        Target::OneLibrary => DbTags::ext_of(device, &source).unwrap_or_else(|e| {
            ctx.warn(format!("{e}; its My Tags aren't converted"));
            None
        }),
        Target::DeviceLibrary => None,
    };
    ctx.check()?;

    // 1. The new database, built and checked in memory before anything on the drive changes (one
    // that fails leaves no backup and no interrupted operation). Artwork copies are only planned.
    ctx.phase_start(Phase::Write, source.tracks.len() as u64, 0);
    let mut report = ConvertReport::default();
    let mut copies = Vec::new();
    let mut builder = Builder { device, source: &source, copies: &mut copies, report: &mut report, ctx };
    let bytes = match opts.to {
        Target::OneLibrary => {
            let existing = if plan.replaces { builder.device.library_plus().ok() } else { None };
            builder.onelibrary(existing, ext.as_ref())?
        }
        Target::DeviceLibrary => builder.device_library()?,
    };
    let ext_bytes = match opts.to {
        Target::DeviceLibrary => device_library_tags(device, &source, old_pdb.as_ref(), &bytes, &mut report, ctx)?,
        Target::OneLibrary => None,
    };
    ctx.phase_end(Phase::Write);

    // 2. Back up, copy the artwork next to where it goes, then swap it and the database in.
    let mut files = Vec::new();
    for (from, to) in &copies {
        let src = device.checked_path(from)?;
        let size = std::fs::metadata(&src).map(|m| m.len()).unwrap_or(0);
        files.push(NewFile::copy(to.clone(), src, size, None));
    }
    let databases = match opts.to {
        Target::OneLibrary => NewDatabases { plus: Some(bytes), ..Default::default() },
        Target::DeviceLibrary => NewDatabases { pdb: Some(bytes), ext: ext_bytes, plus: None },
    };
    let file = opts.to.file();
    let opts =
        Options::new(LineageOp::Convert, format!("convert to {file}")).backup(format!("before converting to {file}"));
    let done =
        commit::commit(device, Changes { databases, files, deletes: Vec::new(), remove_first: Vec::new() }, opts, ctx)?;
    report.backup = done.backup;
    report.fingerprint = done.fingerprint;
    Ok(report)
}

struct Builder<'a, 'c> {
    device: &'a Device,
    source: &'a Library,
    /// Image files to copy (from, to) once the database is built.
    copies: &'a mut Vec<(String, String)>,
    report: &'a mut ConvertReport,
    ctx: &'a Ctx<'c>,
}

impl Builder<'_, '_> {
    /// Plan copying an image (and its `_m` variant) to a free name with `prefix`, returning the
    /// new key, or `None` if the image is missing. (Each id gives another name, so two planned
    /// copies never share one.)
    fn copy_image(&mut self, from: &str, prefix: char, id: u32) -> Result<Option<String>> {
        let from = paths::key(from);
        let Ok(src) = self.device.checked_path(&from) else { return Ok(None) };
        if !src.is_file() || paths::classify(&from) != Some(paths::FileClass::Artwork) {
            return Ok(None);
        }
        let ext = from.rsplit_once('.').map(|(_, e)| e).unwrap_or("jpg");
        let to = free_image_key(self.device.root(), prefix, id, ext);
        for (a, b) in [(from.clone(), to.clone()), (medium_variant(&from), medium_variant(&to))] {
            if self.device.checked_path(&a)?.is_file() {
                self.device.checked_path(&b)?;
                self.copies.push((a, b));
            }
        }
        self.report.artwork += 1;
        Ok(Some(to))
    }

    fn progress(&self, t: &Track) -> Result<()> {
        self.ctx.check()?;
        self.ctx.emit(Event::Item { phase: Phase::Write, path: t.file_path.clone(), bytes: 0 });
        Ok(())
    }

    /// Build OneLibrary from `export.pdb`, keeping `existing`'s rows for tracks it already has,
    /// with `ext`'s My Tags (`exportExt.pdb`).
    fn onelibrary(&mut self, existing: Option<OneLibrary>, ext: Option<&DbTags>) -> Result<Vec<u8>> {
        let mut one = OneLibrary::create()?;
        // exportExt.pdb's categories and tags first, ids and all: kept rows link to them by name.
        if let Some(ext) = ext {
            tags::copy_categories(&mut one, ext)?;
        }
        let old = existing.as_ref().map(OneLibrary::library).transpose()?;
        let old_by_path: HashMap<String, &Track> =
            old.iter().flat_map(|l| l.tracks.iter()).map(|t| (fold(&t.file_path), t)).collect();
        let defaults = match &existing {
            Some(e) => e.content_defaults()?,
            None => one.content_defaults()?,
        };
        let mut images: HashMap<String, Option<u32>> = HashMap::new();
        let mut ids = HashMap::new();
        let mut unreadable = Vec::new();
        for t in &self.source.tracks {
            self.progress(t)?;
            let kept = old_by_path.get(&fold(&t.file_path)).copied();
            // The image OneLibrary already had for the track, else export.pdb's under a `b` name.
            let image = match (kept.and_then(|k| k.artwork_path.as_ref()), &t.artwork_path) {
                (Some(own), _) if self.device.checked_path(&paths::key(own)).is_ok_and(|p| p.is_file()) => {
                    match images.get(&fold(own)) {
                        Some(id) => *id,
                        None => {
                            let id = Some(one.add_image(own)?);
                            images.insert(fold(own), id);
                            id
                        }
                    }
                }
                (_, Some(art)) => match images.get(&fold(art)) {
                    Some(id) => *id,
                    None => {
                        let id = match self.copy_image(art, 'b', one.next_image_id()?)? {
                            Some(key) => Some(one.add_image(&paths::device_path(&key))?),
                            None => None,
                        };
                        images.insert(fold(art), id);
                        id
                    }
                },
                _ => None,
            };
            let rec = match (&existing, kept) {
                (Some(e), Some(k)) => e.export_track(k.id)?,
                _ => None,
            };
            let made = rec.is_none();
            let rec = rec.unwrap_or_else(|| ContentRecord::from_track(t, !t.analyze_path.is_empty(), &defaults));
            let id = one.add_track(&rec, &t.file_path, &t.analyze_path, image)?;
            ids.insert(t.id, id);
            // A row OneLibrary had keeps its cues; a new one gets the analysis files'.
            if made {
                match cue_edit::rows_from_analysis(self.device, t) {
                    Ok(Some(rows)) if !rows.is_empty() => {
                        self.report.cues += one.replace_cues(id, &rows)?.added;
                    }
                    Ok(_) => {}
                    Err(_) => unreadable.push(t.display_name()),
                }
            }
        }
        if let Some(example) = unreadable.first() {
            self.ctx.warn(format!(
                "{} track(s) have analysis files that can't be read (e.g. {example}); they get no cues in OneLibrary",
                unreadable.len()
            ));
        }
        self.trees(&mut one, &ids)?;
        if let Some(ext) = ext {
            let wanted: Wanted = self
                .source
                .tracks
                .iter()
                .filter_map(|t| {
                    let key = fold(&t.file_path);
                    Some((*ids.get(&t.id)?, key.clone(), ext.of(&key)?))
                })
                .collect();
            let library = one.library()?;
            tags::carry_into_plus(&mut one, &library, &[], &wanted, Some(ext))?;
        }
        self.report.tagged_tracks = DbTags::plus(&one, &one.library()?)?.tagged();
        let bytes = one.to_encrypted()?;
        let check = OneLibrary::from_encrypted(&bytes, None)?.library()?;
        if check.tracks.len() != self.source.tracks.len() {
            return Err(Error::OneLibrary("converted database failed verification; drive left unchanged".into()));
        }
        Ok(bytes)
    }

    /// Build `export.pdb` from OneLibrary.
    fn device_library(&mut self) -> Result<Vec<u8>> {
        let mut editor = empty_device_library()?;
        let mut artwork: HashMap<String, u32> = HashMap::new();
        let mut ids = HashMap::new();
        let source = self.source;
        let bytes = pdb::guard_editor(|| {
            for t in &source.tracks {
                self.progress(t)?;
                let artwork_id = match &t.artwork_path {
                    Some(art) => match artwork.get(&fold(art)) {
                        Some(id) => *id,
                        None => {
                            let next = editor.database()?.artwork.iter().map(|a| a.id).max().unwrap_or(0) + 1;
                            let id = match self.copy_image(art, 'a', next)? {
                                Some(key) => editor.add_artwork(&paths::device_path(&key))?,
                                None => 0,
                            };
                            artwork.insert(fold(art), id);
                            id
                        }
                    },
                    None => 0,
                };
                let row = crate::merge::new_track(t, &t.file_path, &t.analyze_path, artwork_id);
                ids.insert(t.id, editor.add_track(&row)?);
            }
            self.trees(&mut editor, &ids)?;
            Ok(editor.to_bytes())
        })?;
        let check = pdb::parse_database(bytes.clone(), &self.device.path(paths::EXPORT_PDB))?;
        if check.tracks.len() != self.source.tracks.len() {
            return Err(Error::Pdb("converted database failed verification; drive left unchanged".into()));
        }
        Ok(bytes)
    }

    /// Recreate playlists (folders first) and history playlists.
    fn trees(&mut self, db: &mut impl PlaylistDb, ids: &HashMap<u32, u32>) -> Result<()> {
        let lib = self.source;
        let mut new_ids: HashMap<u32, u32> = HashMap::new();
        let mut queue: Vec<Option<u32>> = vec![None];
        while let Some(parent) = queue.pop() {
            for p in lib.children(parent) {
                let dst_parent = p.parent_id.and_then(|id| new_ids.get(&id).copied()).unwrap_or(0);
                let id = db.add_playlist(&p.name, dst_parent, p.is_folder)?;
                new_ids.insert(p.id, id);
                if p.is_folder {
                    queue.push(Some(p.id));
                } else {
                    self.report.playlists += 1;
                    for t in p.track_ids.iter().filter_map(|t| ids.get(t)) {
                        db.add_entry(id, *t)?;
                    }
                }
            }
        }
        // Playlists whose parent folder is missing (a corrupt tree) still come across.
        let orphans: Vec<&Playlist> =
            lib.playlists.iter().filter(|p| !p.is_folder && !new_ids.contains_key(&p.id)).collect();
        for p in orphans {
            let id = db.add_playlist(&p.name, 0, false)?;
            self.report.playlists += 1;
            for t in p.track_ids.iter().filter_map(|t| ids.get(t)) {
                db.add_entry(id, *t)?;
            }
        }
        for h in lib.history.iter().filter(|h| !h.is_folder) {
            let id = db.add_history(&h.name)?;
            self.report.history_playlists += 1;
            for t in h.track_ids.iter().filter_map(|t| ids.get(t)) {
                db.add_history_entry(id, *t)?;
            }
        }
        self.report.tracks = lib.tracks.len();
        Ok(())
    }
}

/// Refuse, naming it, a track or name in `source` (OneLibrary, which takes any length) that a new
/// `export.pdb` can't hold, as building it would.
fn check_device_library(source: &Library) -> Result<()> {
    let db = pdb::parse_database(PDB_TEMPLATE.to_vec(), Path::new(paths::EXPORT_PDB))?;
    let refuse =
        |what: String, e: rekordbox_pdb::PdbError| Error::invalid(format!("{what} can't be stored in export.pdb: {e}"));
    for t in &source.tracks {
        let row = crate::merge::new_track(t, &t.file_path, &t.analyze_path, 0);
        db.check_new_track(&row).map_err(|e| refuse(format!("the track {}", t.display_name()), e))?;
    }
    for p in &source.playlists {
        let what = if p.is_folder { "folder" } else { "playlist" };
        db.check_playlist_name(&p.name).map_err(|e| refuse(format!("the {what} {}", source.playlist_path(p.id)), e))?;
    }
    for h in source.history.iter().filter(|h| !h.is_folder) {
        db.check_history_name(&h.name).map_err(|e| refuse(format!("the history list {}", h.name), e))?;
    }
    Ok(())
}

/// Planning a new `export.pdb`'s My Tags: how many tracks OneLibrary's tags give some, and
/// warnings for links in `exportExt.pdb` that go (and for a drive without one).
fn device_library_tags_plan(
    device: &Device,
    source: &Library,
    existing: Option<&Library>,
    warnings: &mut Vec<String>,
) -> usize {
    let plus = match device.library_plus().and_then(|one| DbTags::plus(&one, source)) {
        Ok(plus) => plus,
        Err(e) => {
            warnings.push(format!("{e}; My Tags aren't converted"));
            return 0;
        }
    };
    let path = device.path(paths::EXPORT_EXT_PDB);
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(_) if plus.tagged() > 0 => {
            warnings.push(format!(
                "{} track(s) have My Tags, but the drive has no exportExt.pdb for them (locallibrary can't create one), \
                 so players that read export.pdb won't show them",
                plus.tagged()
            ));
            return 0;
        }
        Err(_) => return 0,
    };
    match tags::remap_ext(bytes, existing, source) {
        Ok((_, dropped)) => {
            if dropped > 0 {
                warnings.push(format!(
                    "{dropped} My Tag link(s) in exportExt.pdb are for tracks the new export.pdb won't list; they go"
                ));
            }
            plus.tagged()
        }
        Err(e) => {
            warnings.push(format!("{e}; exportExt.pdb is left as it is, so its My Tags may name the wrong tracks"));
            0
        }
    }
}

/// `exportExt.pdb` for a new `export.pdb` (`pdb_bytes`): its links follow their tracks (in `old`,
/// the export.pdb being replaced) to the new ids by file path, and OneLibrary's My Tags are added.
/// `None` when the drive has no `exportExt.pdb`, or it can't be used.
fn device_library_tags(
    device: &Device,
    source: &Library,
    old: Option<&Library>,
    pdb_bytes: &[u8],
    report: &mut ConvertReport,
    ctx: &Ctx<'_>,
) -> Result<Option<Vec<u8>>> {
    let Ok(ext) = std::fs::read(device.path(paths::EXPORT_EXT_PDB)) else { return Ok(None) };
    let plus = device.library_plus().and_then(|one| DbTags::plus(&one, source)).ok();
    let new = pdb::library_from_database(&pdb::parse_database(pdb_bytes.to_vec(), &device.path(paths::EXPORT_PDB))?);
    let moved = match tags::remap_ext(ext.clone(), old, &new) {
        Ok((moved, _)) => moved,
        Err(e) => {
            ctx.warn(format!("{e}; exportExt.pdb is left as it is, so its My Tags may name the wrong tracks"));
            return Ok(None);
        }
    };
    let wanted: Wanted = new
        .tracks
        .iter()
        .filter_map(|t| {
            let key = fold(&t.file_path);
            Some((t.id, key.clone(), plus.as_ref()?.of(&key)?))
        })
        .collect();
    let (added, _) = tags::carry_into_ext(moved.clone().unwrap_or(ext), &new, &[], &wanted, plus.as_ref())?;
    let out = added.or(moved);
    if let Some(bytes) = &out {
        report.tagged_tracks = DbTags::ext(bytes, &new)?.tagged();
    }
    Ok(out)
}

/// An `export.pdb` with no tracks, playlists or lookup rows, but rekordbox's colours and browse
/// columns: [`PDB_TEMPLATE`] with its rows removed (deleted the way rekordbox deletes rows).
fn empty_device_library() -> Result<PdbEditor> {
    let mut editor = PdbEditor::from_bytes(PDB_TEMPLATE.to_vec());
    pdb::guard_editor(|| {
        let db = editor.database()?;
        let tracks: Vec<u32> = db.tracks.iter().map(|t| t.id).collect();
        let lookups = [
            (TableType::Artists, db.artists.iter().map(|r| r.id).collect::<Vec<_>>()),
            (TableType::Albums, db.albums.iter().map(|r| r.id).collect()),
            (TableType::Genres, db.genres.iter().map(|r| r.id).collect()),
            (TableType::Labels, db.labels.iter().map(|r| r.id).collect()),
            (TableType::Keys, db.keys.iter().map(|r| r.id).collect()),
            (TableType::Artwork, db.artwork.iter().map(|r| r.id).collect()),
            (TableType::PlaylistTree, db.playlist_tree.iter().map(|r| r.id).collect()),
            (TableType::HistoryPlaylists, db.history_playlists.iter().map(|r| r.id).collect()),
        ];
        editor.remove_tracks(&tracks)?;
        for (table, ids) in lookups {
            editor.remove_rows(table, &ids)?;
        }
        let db = editor.database()?;
        if !(db.tracks.is_empty() && db.artists.is_empty() && db.playlist_tree.is_empty() && db.artwork.is_empty()) {
            return Err(Error::Pdb("couldn't empty the export.pdb template".into()));
        }
        Ok(())
    })?;
    Ok(editor)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_template_keeps_colours() {
        let mut editor = empty_device_library().unwrap();
        let db = editor.database().unwrap();
        assert!(db.tracks.is_empty() && db.playlist_tree.is_empty() && db.playlist_entries.is_empty());
        assert_eq!(db.colors.len(), 8);
        let bytes = editor.to_bytes();
        let lib = pdb::library_from_database(&pdb::parse_database(bytes, std::path::Path::new("x")).unwrap());
        assert!(lib.tracks.is_empty() && lib.playlists.is_empty());
    }
}
