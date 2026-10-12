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
//! A database the drive already has keeps everything it holds and gains what the other one has
//! that it lacks: tracks (matched by file), playlists and their missing entries (merged by path,
//! as [`crate::merge`] merges by name), history lists and My Tags. Players write only the
//! database they read (a CDJ-3000X's history lists and playlist edits go to OneLibrary alone), so
//! that's how drift between the two (see [`crate::device::LibraryDrift`]) is fixed without losing
//! either side's.
//!
//! With `replace`, it's rebuilt from the other one instead, and the plan names what only it has,
//! which goes. Rebuilding OneLibrary keeps the rows it has for tracks `export.pdb` also lists
//! (every column, cues, My Tags and artwork) and the drive's player name and colour, so only what
//! `export.pdb` can't express is lost for tracks that were missing.
//!
//! My Tags come along: `exportExt.pdb`'s into a new OneLibrary (its categories and tags with
//! their ids, and every track's tags). A new `export.pdb` gives its tracks new ids, so the links
//! in `exportExt.pdb` move to them by file path (links to tracks it doesn't list go), and
//! OneLibrary's tags are added. A drive without `exportExt.pdb` doesn't get one.
//!
//! A new `export.pdb` starts from a real rekordbox export (rekordbox-pdb's one-track test file,
//! MIT) with its rows removed, which keeps rekordbox's colours and browse columns. A new OneLibrary
//! database starts from rekordbox's schema and default rows ([`OneLibrary::create`]).

use rustc_hash::{FxHashMap, FxHashSet};
use std::path::{Path, PathBuf};

use rekordbox_pdb::{PdbEditor, TableType};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::artwork::{free_image_key, medium_variant};
use crate::commit::{self, Changes, NewDatabases, NewFile, Options};
use crate::cue_edit;
use crate::device::{Device, LineageOp};
use crate::doctor::Databases;
use crate::error::Action;
use crate::hash::Hash;
use crate::merge::{self, MergeOptions, PlannedPlaylist, PlaylistDb, PlaylistMode};
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
    /// Rebuild the target database if the drive already has one, dropping what only it has.
    /// Otherwise it keeps what it has and gains what it lacks.
    #[serde(default)]
    pub replace: bool,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct ConvertPlan {
    pub drive: String,
    pub root: PathBuf,
    pub to: Target,
    /// The drive already has that database, and it's rebuilt, dropping what only it has (the
    /// warnings name it).
    pub replaces: bool,
    /// The drive already has that database, and it keeps what it has. The counts below are then
    /// what it gains: tracks it lacks, playlists it lacks or that gain entries, folders made for
    /// them, and history lists it lacks.
    pub adds: bool,
    pub tracks: usize,
    pub playlists: usize,
    pub folders: usize,
    pub history_playlists: usize,
    /// Artwork images copied under the other database's file names.
    pub artwork: usize,
    /// Tracks whose My Tags come along from the other database's tags (that gain some, when
    /// adding).
    pub tagged_tracks: usize,
    /// Space needed: the new database, copied artwork, and the backup of the database files.
    pub needed_bytes: u64,
    pub free_space: Option<u64>,
    pub fits: bool,
    pub warnings: Vec<String>,
}

impl ConvertPlan {
    /// Adding to the database the drive has would change nothing.
    pub fn is_noop(&self) -> bool {
        self.adds && self.tracks + self.playlists + self.history_playlists + self.tagged_tracks == 0
    }
}

/// What a conversion made (added, when the drive had the database: see [`ConvertPlan::adds`]).
#[derive(Debug, Clone, Default, Serialize, JsonSchema)]
pub struct ConvertReport {
    pub tracks: usize,
    pub playlists: usize,
    pub history_playlists: usize,
    pub artwork: usize,
    /// Tracks with My Tags in the converted database's tags (that gained some, when adding).
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

/// The library the target database holds now (the drive has one).
fn existing_library(device: &Device, to: Target) -> Result<Library> {
    match to {
        Target::OneLibrary => device.library_plus()?.library(),
        Target::DeviceLibrary => pdb::read_library(&device.path(paths::EXPORT_PDB)),
    }
}

/// The library the target database holds now, when the drive has one: refused when it can't be
/// read and the conversion adds to it, `None` when it's rebuilt anyway.
fn target_library(device: &Device, to: Target, adds: bool) -> Result<Option<Library>> {
    if !has_target(device, to) {
        return Ok(None);
    }
    match existing_library(device, to) {
        Ok(lib) => Ok(Some(lib)),
        Err(e) if adds => Err(Error::invalid(format!(
            "{}'s {} can't be read, so nothing can be added to it: {e}",
            device.name(),
            to.file()
        ))
        .action(Action::retry(
            serde_json::json!({ "replace": true }),
            format!("Rebuild it from {}", to.source_file()),
        ))),
        Err(_) => Ok(None),
    }
}

/// What adding the source library to the one a target database holds brings. Tracks are matched
/// by file: both databases list the drive's own files.
struct Additions {
    /// Source track id -> the target's, for tracks it has.
    matches: FxHashMap<u32, u32>,
    /// Source tracks it lacks.
    new_tracks: FxHashSet<u32>,
    /// Playlists it lacks or that gain entries, merged by path.
    playlists: Vec<PlannedPlaylist>,
    /// Folders made for them.
    folders: usize,
    /// Source history lists it lacks (by name and tracks).
    history: Vec<u32>,
}

impl Additions {
    fn of(source: &Library, target: &Library) -> Additions {
        let by_path: FxHashMap<String, u32> = target.tracks.iter().map(|t| (fold(&t.file_path), t.id)).collect();
        let (mut matches, mut new_tracks) = (FxHashMap::default(), FxHashSet::default());
        for t in &source.tracks {
            match by_path.get(&fold(&t.file_path)) {
                Some(id) => {
                    matches.insert(t.id, *id);
                }
                None => {
                    new_tracks.insert(t.id);
                }
            }
        }
        let all = source.playlists.iter().map(|p| p.id).collect();
        let mut playlists =
            merge::plan_playlists(source, target, &all, PlaylistMode::ByName, "", &matches, &new_tracks);
        playlists.retain(|p| p.create || p.tracks_added > 0);
        let mut folders = FxHashSet::default();
        for p in playlists.iter().filter(|p| p.create) {
            let parts: Vec<&str> = p.path.split('/').collect();
            let missing = (1..parts.len()).map(|n| parts[..n].join("/"));
            folders.extend(missing.filter(|f| merge::merge_target(target, f, true).is_none()));
        }
        let history = merge::plan_history(source, target, &matches, &new_tracks);
        Additions { matches, new_tracks, playlists, folders: folders.len(), history }
    }

    /// Add the source's playlists, their missing entries and history lists to `db`, which holds
    /// `target`. `ids` maps every source track to `db`'s.
    fn write(
        &self,
        db: &mut impl PlaylistDb,
        source: &Library,
        target: &Library,
        ids: &FxHashMap<u32, u32>,
    ) -> Result<()> {
        let by_path = MergeOptions { playlists: PlaylistMode::ByName, ..Default::default() };
        merge::write_playlists(db, source, target, &by_path, "", ids)?;
        merge::write_history(db, source, &self.history, ids)?;
        Ok(())
    }
}

/// The drive's library with what only OneLibrary has added when it has `export.pdb` too (whose
/// library players see): its tracks, playlists, their missing entries and history lists, as
/// converting into `export.pdb` would add them. For readers that show the whole library, such as
/// exports for other software.
pub fn combined_library(device: &Device) -> Result<Library> {
    combined(&Databases::read(device)?)
}

/// [`combined_library`] of a drive's or a snapshot's databases.
pub(crate) fn combined(dbs: &Databases) -> Result<Library> {
    let mut library = dbs.primary().clone();
    let Some(plus) = dbs.beside() else { return Ok(library) };
    let target = library.clone();
    let additions = Additions::of(plus, &target);
    let mut ids = additions.matches.clone();
    let mut next = target.tracks.iter().map(|t| t.id).max().unwrap_or(0);
    for t in plus.tracks.iter().filter(|t| additions.new_tracks.contains(&t.id)) {
        next += 1;
        library.tracks.push(Track { id: next, ..t.clone() });
        ids.insert(t.id, next);
    }
    additions.write(&mut library, plus, &target, &ids)?;
    Ok(library)
}

/// Warnings naming what only `target` (the database rebuilt from `source`) has, which goes:
/// playlists and folders (ending in `/`), entries of playlists both have, and history lists.
fn dropped(target: &Library, source: &Library, file: &str) -> Option<String> {
    let label = |lib: &Library, p: &Playlist| {
        let path = lib.playlist_path(p.id);
        if p.is_folder { format!("{path}/") } else { path }
    };
    let files = |lib: &Library, ids: &[u32]| -> FxHashSet<String> {
        ids.iter().filter_map(|id| lib.track(*id)).map(|t| fold(&t.file_path)).collect()
    };
    let theirs: FxHashMap<String, &Playlist> = source.playlists.iter().map(|p| (fold(&label(source, p)), p)).collect();
    let (mut playlists, mut entries) = (Vec::new(), Vec::new());
    for p in &target.playlists {
        let name = label(target, p);
        match theirs.get(&fold(&name)) {
            None => playlists.push(name),
            Some(s) if !files(target, &p.track_ids).is_subset(&files(source, &s.track_ids)) => entries.push(name),
            Some(_) => {}
        }
    }
    let lists: FxHashSet<String> = source.history.iter().map(|h| fold(&h.name)).collect();
    let history: Vec<String> = target
        .history
        .iter()
        .filter(|h| !h.is_folder && !lists.contains(&fold(&h.name)))
        .map(|h| h.name.clone())
        .collect();
    let named = |what: &str, names: &[String]| {
        let shown: Vec<&str> = names.iter().take(5).map(String::as_str).collect();
        let more = if names.len() > 5 { format!(", +{} more", names.len() - 5) } else { String::new() };
        (!names.is_empty()).then(|| format!("{what} {}{more}", shown.join(", ")))
    };
    let parts: Vec<String> = [
        named("the playlist(s)", &playlists),
        named("the history list(s)", &history),
        named("entries only it has in", &entries),
    ]
    .into_iter()
    .flatten()
    .collect();
    (!parts.is_empty()).then(|| format!("rebuilding drops what only {file} has: {}", parts.join("; ")))
}

fn fold(s: &str) -> String {
    paths::key(s).to_lowercase()
}

pub fn plan(device: &Device, opts: &ConvertOptions, _ctx: &Ctx<'_>) -> Result<ConvertPlan> {
    let source = source_library(device, opts.to)?;
    let has = has_target(device, opts.to);
    let (replaces, adds) = (has && opts.replace, has && !opts.replace);
    if opts.to == Target::DeviceLibrary {
        check_device_library(&source)?;
    }
    let mut warnings = Vec::new();
    let existing = target_library(device, opts.to, adds)?;
    let additions = existing.as_ref().filter(|_| adds).map(|e| Additions::of(&source, e));
    if replaces {
        match &existing {
            Some(existing) => {
                let keep: FxHashSet<String> = source.tracks.iter().map(|t| fold(&t.file_path)).collect();
                let dropped_tracks = existing.tracks.iter().filter(|t| !keep.contains(&fold(&t.file_path))).count();
                if dropped_tracks > 0 {
                    warnings.push(format!(
                        "{dropped_tracks} track(s) only {} lists will no longer be in it (their files stay on the drive)",
                        opts.to.file()
                    ));
                }
                warnings.extend(dropped(existing, &source, opts.to.file()));
            }
            None => warnings.push(format!("the current {} can't be read; it's rebuilt from scratch", opts.to.file())),
        }
        let banks = match opts.to {
            Target::OneLibrary => device
                .library_plus()
                .and_then(|one| one.hot_cue_bank_lists())
                .map_or(0, |l| l.iter().filter(|l| !l.is_folder).count()),
            Target::DeviceLibrary => 0,
        };
        if banks > 0 {
            warnings.push(format!("its {banks} hot cue bank list(s) go too (export.pdb has none)"));
        }
    }
    let tagged_tracks = match opts.to {
        Target::OneLibrary => match DbTags::ext_of(device, &source) {
            Ok(ext) => match (&existing, &additions) {
                (Some(existing), Some(_)) => {
                    let plus = device.library_plus().and_then(|one| DbTags::plus(&one, existing))?;
                    tags_gained(&plus, ext.as_ref(), &source)
                }
                _ => ext.map_or(0, |t| t.tagged()),
            },
            Err(e) => {
                warnings.push(format!("{e}; its My Tags aren't converted"));
                0
            }
        },
        Target::DeviceLibrary => {
            warnings.push("hot cue bank lists aren't converted".to_string());
            device_library_tags_plan(device, &source, existing.as_ref(), adds, &mut warnings)
        }
    };
    warnings.extend(device.rekordbox_sync_warning(
        "it has no record of the converted database's playlists and may export them again when it next syncs",
    ));

    let mut artwork: FxHashSet<String> = FxHashSet::default();
    let mut missing: FxHashSet<String> = FxHashSet::default();
    let mut artwork_bytes = 0;
    let file_size = |k: &str| device.checked_path(k).ok().and_then(|p| std::fs::metadata(p).ok()).map(|m| m.len());
    let made = |t: &&Track| additions.as_ref().is_none_or(|a| a.new_tracks.contains(&t.id));
    for a in source.tracks.iter().filter(made).filter_map(|t| t.artwork_path.as_ref()) {
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
    // Rows take well under 1 KiB per track or entry, including lookups and page slack. Adding
    // writes a new copy of the database the drive has, grown by at most that.
    let base = match (adds, opts.to) {
        (true, Target::OneLibrary) => size(paths::LIBRARY_PLUS_DB),
        (true, Target::DeviceLibrary) => size(paths::EXPORT_PDB),
        (false, _) => 256 * 1024,
    };
    let database = base + (source.tracks.len() + entries) as u64 * 1024;
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
        adds,
        tracks: additions.as_ref().map_or(source.tracks.len(), |a| a.new_tracks.len()),
        playlists: match &additions {
            Some(a) => a.playlists.len(),
            None => source.playlists.iter().filter(|p| !p.is_folder).count(),
        },
        folders: match &additions {
            Some(a) => a.folders,
            None => source.playlists.iter().filter(|p| p.is_folder).count(),
        },
        history_playlists: match &additions {
            Some(a) => a.history.len(),
            None => source.history.iter().filter(|h| !h.is_folder).count(),
        },
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
    let adds = has_target(device, opts.to) && !opts.replace;
    // What the target database holds now: added to, or rebuilt keeping its rows for tracks.
    let existing = target_library(device, opts.to, adds)?;
    let additions = existing.as_ref().filter(|_| adds).map(|e| Additions::of(&source, e));
    let tracks = additions.as_ref().map_or(source.tracks.len(), |a| a.new_tracks.len());
    if tracks != plan.tracks || adds != plan.adds || has_target(device, opts.to) != (plan.adds || plan.replaces) {
        return Err(Error::stale(device.name(), "changed"));
    }
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
    let mut made = Vec::new();
    let mut builder = Builder {
        device,
        source: &source,
        target: existing.as_ref(),
        additions: additions.as_ref(),
        copies: &mut copies,
        made: &mut made,
        report: &mut report,
        ctx,
        keep_images: false,
    };
    let bytes = match opts.to {
        Target::OneLibrary => {
            // Added to, it was read above; rebuilt, one that can't be read is started afresh.
            let one = match (existing.is_some(), adds) {
                (true, true) => Some(builder.device.library_plus()?),
                (true, false) => builder.device.library_plus().ok(),
                (false, _) => None,
            };
            builder.onelibrary(one, ext.as_ref())?
        }
        Target::DeviceLibrary => builder.device_library()?,
    };
    let ext_bytes = match opts.to {
        Target::DeviceLibrary => {
            let added = adds.then_some(made.as_slice());
            device_library_tags(device, &source, existing.as_ref(), &bytes, added, &mut report, ctx)?
        }
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
    /// The library the target database holds, when the drive has one that can be read.
    target: Option<&'a Library>,
    /// What adding to it brings; `None` when the database is built afresh.
    additions: Option<&'a Additions>,
    /// Image files to copy (from, to) once the database is built.
    copies: &'a mut Vec<(String, String)>,
    /// Ids of the track rows built from the source's.
    made: &'a mut Vec<u32>,
    report: &'a mut ConvertReport,
    ctx: &'a Ctx<'c>,
    /// Images keep the keys the tracks give them, which both databases share, instead of being
    /// copied to the database's own names.
    keep_images: bool,
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
        if self.keep_images {
            self.report.artwork += 1;
            return Ok(Some(from));
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

    /// The id the target database already has for source track `t`, when adding to it.
    fn matched(&self, t: &Track) -> Option<u32> {
        self.additions?.matches.get(&t.id).copied()
    }

    /// Build OneLibrary from `export.pdb`, with `ext`'s My Tags (`exportExt.pdb`). `existing`, the
    /// drive's OneLibrary, is added to, or rebuilt keeping its rows for tracks `export.pdb` lists
    /// and its player name and colour.
    fn onelibrary(&mut self, existing: Option<OneLibrary>, ext: Option<&DbTags>) -> Result<Vec<u8>> {
        let (mut one, rebuilt) = match existing {
            Some(existing) if self.additions.is_some() => (existing, None),
            existing => {
                let mut one = OneLibrary::create()?;
                // exportExt.pdb's categories and tags first, ids and all: kept rows link to them by name.
                if let Some(ext) = ext {
                    tags::copy_categories(&mut one, ext)?;
                }
                if let Some(old) = &existing {
                    crate::player::keep_identity(old, &one)?;
                }
                (one, existing)
            }
        };
        let old_by_path: FxHashMap<String, &Track> =
            self.target.iter().flat_map(|l| l.tracks.iter()).map(|t| (fold(&t.file_path), t)).collect();
        let defaults = match &rebuilt {
            Some(e) => e.content_defaults()?,
            None => one.content_defaults()?,
        };
        let mut images: FxHashMap<String, Option<u32>> = FxHashMap::default();
        let mut ids = FxHashMap::default();
        let mut unreadable = Vec::new();
        for t in &self.source.tracks {
            self.progress(t)?;
            if let Some(id) = self.matched(t) {
                ids.insert(t.id, id);
                continue;
            }
            let kept = rebuilt.as_ref().and_then(|_| old_by_path.get(&fold(&t.file_path)).copied());
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
            let rec = match (&rebuilt, kept) {
                (Some(e), Some(k)) => e.export_track(k.id)?,
                _ => None,
            };
            let made = rec.is_none();
            let rec = rec.unwrap_or_else(|| ContentRecord::from_track(t, !t.analyze_path.is_empty(), &defaults));
            let id = one.add_track(&rec, &t.file_path, &t.analyze_path, image)?;
            ids.insert(t.id, id);
            // A row OneLibrary had keeps its cues; a new one gets the analysis files'.
            if made {
                self.made.push(id);
                match cue_edit::add_rows_from_analysis(&mut one, id, self.device, t)? {
                    Some(added) => self.report.cues += added,
                    None => unreadable.push(t.display_name()),
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
        let carried = match ext {
            Some(ext) => {
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
                Some(tags::carry_into_plus(&mut one, &library, self.made, &wanted, Some(ext))?)
            }
            None => None,
        };
        self.report.tagged_tracks = match self.additions {
            Some(_) => carried.map_or(0, |c| c.tracks.len()),
            None => DbTags::plus(&one, &one.library()?)?.tagged(),
        };
        let bytes = one.to_encrypted()?;
        let check = OneLibrary::from_encrypted(&bytes, None)?.library()?;
        if check.tracks.len() != self.expected_tracks() {
            return Err(Error::OneLibrary("converted database failed verification; drive left unchanged".into()));
        }
        Ok(bytes)
    }

    /// How many tracks the converted database holds.
    fn expected_tracks(&self) -> usize {
        match (self.additions, self.target) {
            (Some(a), Some(target)) => target.tracks.len() + a.new_tracks.len(),
            _ => self.source.tracks.len(),
        }
    }

    /// Build `export.pdb` from OneLibrary, or add to the drive's.
    fn device_library(&mut self) -> Result<Vec<u8>> {
        let mut editor = match self.additions {
            Some(_) => PdbEditor::from_bytes(self.device.pdb_bytes()?),
            None => empty_device_library()?,
        };
        let mut artwork: FxHashMap<String, u32> = FxHashMap::default();
        let mut ids = FxHashMap::default();
        let source = self.source;
        let bytes = pdb::guard_editor(|| {
            for t in &source.tracks {
                self.progress(t)?;
                if let Some(id) = self.matched(t) {
                    ids.insert(t.id, id);
                    continue;
                }
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
                let id = editor.add_track(&row)?;
                ids.insert(t.id, id);
                self.made.push(id);
            }
            self.trees(&mut editor, &ids)?;
            Ok(editor.to_bytes())
        })?;
        let check = pdb::parse_database(bytes.clone(), &self.device.path(paths::EXPORT_PDB))?;
        if check.tracks.len() != self.expected_tracks() {
            return Err(Error::Pdb("converted database failed verification; drive left unchanged".into()));
        }
        Ok(bytes)
    }

    /// Recreate playlists (folders first) and history playlists, or add the ones the target lacks
    /// and their missing entries. `ids` maps every source track to `db`'s.
    fn trees(&mut self, db: &mut impl PlaylistDb, ids: &FxHashMap<u32, u32>) -> Result<()> {
        let lib = self.source;
        if let (Some(a), Some(target)) = (self.additions, self.target) {
            a.write(db, lib, target, ids)?;
            self.report.tracks = a.new_tracks.len();
            self.report.playlists = a.playlists.len();
            self.report.history_playlists = a.history.len();
            return Ok(());
        }
        let mut new_ids: FxHashMap<u32, u32> = FxHashMap::default();
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
        // Playlists in a parent loop (a corrupt tree) are never reached from the top, but still
        // come across. Ones whose folder is missing are already at the top, in `children(None)`.
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

/// OneLibrary and `export.pdb` (in that order) for `source`, a library whose files are on `device`
/// already, as [`crate::collection_export`] writes it: images keep their keys, OneLibrary's cues
/// come from the analysis files, and it gets `tags`' My Tags.
pub(crate) fn build_databases(
    device: &Device,
    source: &Library,
    tags: Option<&DbTags>,
    ctx: &Ctx<'_>,
) -> Result<(Vec<u8>, Vec<u8>, ConvertReport)> {
    let (mut report, mut copies, mut made) = (ConvertReport::default(), Vec::new(), Vec::new());
    let mut builder = Builder {
        device,
        source,
        target: None,
        additions: None,
        copies: &mut copies,
        made: &mut made,
        report: &mut report,
        ctx,
        keep_images: true,
    };
    let plus = builder.onelibrary(None, tags)?;
    let pdb = builder.device_library()?;
    Ok((plus, pdb, report))
}

/// Refuse, naming it, a track or name in `source` (OneLibrary, which takes any length) that a new
/// `export.pdb` can't hold, as building it would.
pub(crate) fn check_device_library(source: &Library) -> Result<()> {
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

/// How many of the tracks in `source` gain My Tags from `from` in a database whose tags are
/// `into`.
fn tags_gained(into: &DbTags, from: Option<&DbTags>, source: &Library) -> usize {
    let Some(from) = from else { return 0 };
    let wanted: Wanted = source
        .tracks
        .iter()
        .filter_map(|t| {
            let key = fold(&t.file_path);
            Some((t.id, key.clone(), from.of(&key)?))
        })
        .collect();
    tags::carry_preview(into, Some(from), &wanted).tracks.len()
}

/// Planning `export.pdb`'s My Tags: how many tracks OneLibrary's tags give some (that gain some,
/// when `adds` to the drive's `existing` one), and warnings for links in `exportExt.pdb` a new
/// one drops (and for a drive without one).
fn device_library_tags_plan(
    device: &Device,
    source: &Library,
    existing: Option<&Library>,
    adds: bool,
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
    if let (true, Some(existing)) = (adds, existing) {
        return match DbTags::ext(&bytes, existing) {
            Ok(ext) => tags_gained(&ext, Some(&plus), source),
            Err(e) => {
                warnings.push(format!("{e}; its My Tags aren't converted"));
                0
            }
        };
    }
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

/// `exportExt.pdb` for the converted `export.pdb` (`pdb_bytes`), with OneLibrary's My Tags added.
/// Added to (`added`: the ids of the tracks it gained), its links stay; rebuilt, they follow their
/// tracks (in `old`, the export.pdb being replaced) to the new ids by file path. `None` when the
/// drive has no `exportExt.pdb`, or it can't be used.
fn device_library_tags(
    device: &Device,
    source: &Library,
    old: Option<&Library>,
    pdb_bytes: &[u8],
    added: Option<&[u32]>,
    report: &mut ConvertReport,
    ctx: &Ctx<'_>,
) -> Result<Option<Vec<u8>>> {
    let Ok(ext) = std::fs::read(device.path(paths::EXPORT_EXT_PDB)) else { return Ok(None) };
    let plus = device.library_plus().and_then(|one| DbTags::plus(&one, source)).ok();
    let new = pdb::library_from_database(&pdb::parse_database(pdb_bytes.to_vec(), &device.path(paths::EXPORT_PDB))?);
    let moved = match added {
        Some(_) => None,
        None => match tags::remap_ext(ext.clone(), old, &new) {
            Ok((moved, _)) => moved,
            Err(e) => {
                ctx.warn(format!("{e}; exportExt.pdb is left as it is, so its My Tags may name the wrong tracks"));
                return Ok(None);
            }
        },
    };
    let wanted: Wanted = new
        .tracks
        .iter()
        .filter_map(|t| {
            let key = fold(&t.file_path);
            Some((t.id, key.clone(), plus.as_ref()?.of(&key)?))
        })
        .collect();
    let base = moved.clone().unwrap_or(ext);
    let (out, carried) = match tags::carry_into_ext(base, &new, added.unwrap_or(&[]), &wanted, plus.as_ref()) {
        Ok(carried) => carried,
        // Added to, it wasn't read before (rebuilt, moving its links did).
        Err(e) if added.is_some() => {
            ctx.warn(format!("{e}; its My Tags aren't converted"));
            return Ok(None);
        }
        Err(e) => return Err(e),
    };
    let out = out.or(moved);
    report.tagged_tracks = match (added, &out) {
        (Some(_), _) => carried.tracks.len(),
        (None, Some(bytes)) => DbTags::ext(bytes, &new)?.tagged(),
        (None, None) => 0,
    };
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
