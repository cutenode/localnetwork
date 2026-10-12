//! Removing tracks nobody wants on a drive, starting with the demo samples rekordbox ships in its
//! collection. rekordbox exports them again and again, so a drive collects `House1.wav`,
//! `House1-1.wav`, `House1-2.wav`... in `Contents/rekordbox/`.
//!
//! A track matches when its artist and title equal an entry of [`USELESS_TRACKS`] (ignoring case
//! and spacing). Cleaning removes its row from `export.pdb` and from OneLibrary's
//! `exportLibrary.db` (whichever the drive has) along with its playlist and history entries, then
//! deletes its audio, analysis files and artwork unless a remaining track in either database still
//! uses them. Artists, albums, genres, labels, keys and artwork rows that only the removed tracks
//! used go too, so players' browse menus don't list them.
//!
//! The database and every deleted file are backed up first (see [`crate::backup`]), so
//! `locallibrary undo` takes a clean back.

use indexmap::IndexMap;

use rustc_hash::FxHashSet;
use std::path::PathBuf;

use rekordbox_pdb::{Database, PdbEditor, TableType};
use schemars::JsonSchema;
use serde::Serialize;

use crate::commit::{self, Changes, NewDatabases, Options};
use crate::device::{Device, LineageOp};
use crate::fsutil;
use crate::hash::Hash;
use crate::model::{Library, Track, TrackRef, normalize};
use crate::onelibrary::OneLibrary;
use crate::paths::{self, FileClass};
use crate::pdb;
use crate::progress::{Ctx, Phase};
use crate::tree::FileTree;
use crate::{Error, Result};

/// `(artist, title)` of tracks that are never wanted on a drive.
pub const USELESS_TRACKS: &[(&str, &str)] = &[
    // rekordbox's bundled "GROOVE CIRCUIT FACTORY SAMPLES".
    ("rekordbox", "House 1"),
    ("rekordbox", "House 2"),
    ("rekordbox", "House 3"),
    ("rekordbox", "House 4"),
    ("rekordbox", "Techno 1"),
    ("rekordbox", "Breaks 1"),
    ("rekordbox", "Breaks 2"),
    ("rekordbox", "Breaks 3"),
];

/// Lookup tables whose rows are removed once no remaining track uses them.
const LOOKUP_TABLES: [TableType; 6] =
    [TableType::Artists, TableType::Albums, TableType::Genres, TableType::Labels, TableType::Keys, TableType::Artwork];

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct AffectedPlaylist {
    pub path: String,
    pub entries_removed: usize,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct CleanPlan {
    pub drive: String,
    pub root: PathBuf,
    pub tracks: Vec<TrackRef>,
    /// Files to delete (audio, analysis, artwork), as drive keys.
    pub files: Vec<String>,
    /// Total size of `files`.
    pub bytes: u64,
    pub playlists: Vec<AffectedPlaylist>,
    /// Artist/album/genre/label/key/artwork rows left unused by the removal.
    pub lookup_rows: usize,
    /// Space needed for the backup of the database and the deleted files.
    pub needed_bytes: u64,
    pub free_space: Option<u64>,
    pub fits: bool,
    pub warnings: Vec<String>,
    /// `export.pdb` lookup rows to remove.
    #[serde(skip)]
    lookups: Vec<(TableType, Vec<u32>)>,
    /// Tracks to remove from each database: (id, file path).
    #[serde(skip)]
    pdb_tracks: Vec<(u32, String)>,
    #[serde(skip)]
    plus_tracks: Vec<(u32, String)>,
}

impl CleanPlan {
    pub fn is_noop(&self) -> bool {
        self.tracks.is_empty()
    }
}

fn is_useless(artist: &str, title: &str) -> bool {
    let (artist, title) = (normalize(artist), normalize(title));
    USELESS_TRACKS.iter().any(|(a, t)| normalize(a) == artist && normalize(t) == title)
}

fn fold(s: &str) -> String {
    s.to_lowercase()
}

pub fn plan(device: &Device, ctx: &Ctx<'_>) -> Result<CleanPlan> {
    if !device.has_export() {
        return Err(Error::NotAnExport(device.root().to_path_buf()));
    }
    let mut warnings = Vec::new();
    let pdb = match device.has_device_library() {
        true => {
            let db = pdb::parse_database(device.pdb_bytes()?, &device.path(paths::EXPORT_PDB))?;
            Some((pdb::library_from_database(&db), db))
        }
        false => None,
    };
    let plus = open_plus(device, pdb.is_some(), &mut warnings)?;
    let libraries: Vec<&Library> = pdb.iter().map(|(l, _)| l).chain(plus.iter().map(|(l, _)| l)).collect();
    let useless = |t: &Track| is_useless(&t.artist, &t.title);
    let doomed_in = |lib: &Library| -> Vec<(u32, String)> {
        lib.tracks.iter().filter(|t| useless(t)).map(|t| (t.id, t.file_path.clone())).collect()
    };
    let pdb_tracks = pdb.as_ref().map(|(l, _)| doomed_in(l)).unwrap_or_default();
    let plus_tracks = plus.as_ref().map(|(l, _)| doomed_in(l)).unwrap_or_default();
    let nothing_doomed = pdb_tracks.is_empty() && plus_tracks.is_empty();
    let (files, left_in_place) = removable_files(device, &libraries, plus.as_ref().map(|(_, o)| o), &useless, ctx)?;
    if !nothing_doomed {
        warnings.extend(
            device.rekordbox_sync_warning("its next sync may put removed tracks back into the playlists it syncs"),
        );
    }
    if let Some(example) = left_in_place.values().next() {
        warnings.push(format!(
            "{} audio file(s) outside Contents/ are left on the drive (e.g. {example}); locallibrary never deletes files there",
            left_in_place.len(),
        ));
    }

    // What to show: every row the drive's main database removes (two rows can share one file),
    // plus the tracks only OneLibrary has, matched by file as the two databases share audio.
    let primary = libraries[0];
    let primary_doomed: Vec<&Track> = primary.tracks.iter().filter(|t| useless(t)).collect();
    let primary_files: FxHashSet<String> = primary_doomed.iter().map(|t| fold(&paths::key(&t.file_path))).collect();
    let only_secondary = libraries[1..]
        .iter()
        .flat_map(|l| l.tracks.iter())
        .filter(|t| useless(t) && !primary_files.contains(&fold(&paths::key(&t.file_path))));
    let tracks: Vec<TrackRef> = primary_doomed.iter().copied().chain(only_secondary).map(TrackRef::from).collect();
    let primary_ids: FxHashSet<u32> = primary_doomed.iter().map(|t| t.id).collect();
    let playlists = primary
        .playlists
        .iter()
        .filter_map(|p| {
            let n = p.track_ids.iter().filter(|id| primary_ids.contains(id)).count();
            (n > 0).then(|| AffectedPlaylist { path: primary.playlist_path(p.id), entries_removed: n })
        })
        .collect();

    let lookups = pdb
        .as_ref()
        .map(|(_, db)| unused_lookups(db, &pdb_tracks.iter().map(|(id, _)| *id).collect()))
        .unwrap_or_default();
    let plus_ids: Vec<u32> = plus_tracks.iter().map(|(id, _)| *id).collect();
    let plus_lookups = match &plus {
        Some((_, one)) if !plus_ids.is_empty() => one.unused_lookup_rows(&plus_ids)?,
        _ => 0,
    };
    let bytes = files.values().sum();
    let cluster = fsutil::cluster_size(device.root());
    let round = |n: u64| n.div_ceil(cluster) * cluster;
    // The backup copies the database files and everything deleted; each new database is staged
    // next to the old one.
    let size = |key| std::fs::metadata(device.path(key)).map(|m| m.len()).unwrap_or(0);
    let staged = if pdb_tracks.is_empty() { 0 } else { round(size(paths::EXPORT_PDB)) }
        + if plus_tracks.is_empty() { 0 } else { round(size(paths::LIBRARY_PLUS_DB) + size(paths::LIBRARY_PLUS_WAL)) };
    let needed_bytes =
        if nothing_doomed { 0 } else { crate::backup::needed_bytes(device, files.keys().map(String::as_str)) + staged };
    let free_space = fsutil::available_space(device.root());
    let fits = free_space.is_none_or(|f| needed_bytes <= f);
    if !fits {
        warnings.push(format!(
            "the backup needs about {needed_bytes} bytes but only {} are free on the drive",
            free_space.unwrap_or(0)
        ));
    }
    Ok(CleanPlan {
        drive: device.name(),
        root: device.root().to_path_buf(),
        tracks,
        files: files.into_keys().collect(),
        bytes,
        playlists,
        lookup_rows: lookups.iter().map(|(_, ids)| ids.len()).sum::<usize>() + plus_lookups,
        needed_bytes,
        free_space,
        fits,
        warnings,
        lookups,
        pdb_tracks,
        plus_tracks,
    })
}

/// The files only tracks matching `doomed` use (audio, analysis files, artwork), as drive keys
/// with their sizes, plus audio outside `Contents/` that such tracks reference and that is left
/// where it is. Files a remaining track in any of `libraries` still uses are never included
/// (two rows can share one file, and the two databases share audio and analysis), nor are images
/// OneLibrary's albums, playlists and hot cue bank lists use.
pub(crate) fn removable_files(
    device: &Device,
    libraries: &[&Library],
    plus: Option<&OneLibrary>,
    doomed: &dyn Fn(&Track) -> bool,
    ctx: &Ctx<'_>,
) -> Result<(IndexMap<String, u64>, IndexMap<String, String>)> {
    // Scan with every track either database references (audio outside Contents/ included).
    let everything =
        Library { tracks: libraries.iter().flat_map(|l| l.tracks.iter().cloned()).collect(), ..Default::default() };
    let tree = device.scan(Some(&everything), ctx)?;
    let mut in_use: FxHashSet<String> = FxHashSet::default();
    for t in libraries.iter().flat_map(|l| l.tracks.iter()).filter(|t| !doomed(t)) {
        in_use.insert(fold(&paths::key(&t.file_path)));
        for e in analysis_files(&t.analyze_path, &tree) {
            in_use.insert(fold(&e));
        }
        if let Some(a) = &t.artwork_path {
            in_use.insert(fold(&paths::key(a)));
            in_use.insert(fold(&crate::artwork::medium_variant(&paths::key(a))));
        }
    }
    if let Some(one) = plus {
        for (_, a) in one.images_used_outside_tracks()? {
            in_use.insert(fold(&paths::key(&a)));
            in_use.insert(fold(&crate::artwork::medium_variant(&paths::key(&a))));
        }
    }
    let mut files: IndexMap<String, u64> = IndexMap::new();
    let add = |key: &str, class: FileClass, files: &mut IndexMap<String, u64>| {
        // Only files that really are what the database says, inside PIONEER/ or Contents/: a
        // crafted database must not get us to delete its own database files or anything else.
        if let Some(e) = tree.resolve(key)
            && e.class == class
            && paths::classify(&e.path) == Some(class)
            && !in_use.contains(&fold(&e.path))
        {
            files.insert(e.path.clone(), e.size);
        }
    };
    let mut left_in_place = IndexMap::new();
    for t in libraries.iter().flat_map(|l| l.tracks.iter()).filter(|t| doomed(t)) {
        let audio = paths::key(&t.file_path);
        if paths::classify(&audio).is_none() && tree.resolve(&audio).is_some() {
            left_in_place.insert(fold(&audio), t.file_path.clone());
        }
        add(&audio, FileClass::Audio, &mut files);
        for k in analysis_files(&t.analyze_path, &tree) {
            add(&k, FileClass::Analysis, &mut files);
        }
        if let Some(a) = &t.artwork_path {
            let key = paths::key(a);
            add(&crate::artwork::medium_variant(&key), FileClass::Artwork, &mut files);
            add(&key, FileClass::Artwork, &mut files);
        }
    }
    files.sort_unstable_keys();
    left_in_place.sort_unstable_keys();
    Ok((files, left_in_place))
}

/// The drive's OneLibrary database and its library, if it has one. On a drive that also has
/// `export.pdb`, one that can't be read is reported and left alone rather than failing the edit.
pub(crate) fn open_plus(
    device: &Device,
    has_pdb: bool,
    warnings: &mut Vec<String>,
) -> Result<Option<(Library, OneLibrary)>> {
    if !device.has_library_plus() {
        return Ok(None);
    }
    match device.library_plus().and_then(|one| Ok((one.library()?, one))) {
        Ok(x) => Ok(Some(x)),
        Err(e) if has_pdb => {
            warnings.push(format!(
                "{e}. Only export.pdb will be changed, so CDJ-3000X, OPUS-QUAD, XDJ-AZ and other players that \
                 read OneLibrary won't see this until the drive is exported from rekordbox again"
            ));
            Ok(None)
        }
        Err(e) => Err(e),
    }
}

/// A track's analysis files (`.DAT`, `.EXT`, `.2EX`) present on the drive.
pub(crate) fn analysis_files(analyze_path: &str, tree: &FileTree) -> Vec<String> {
    let dat = paths::key(analyze_path);
    if !paths::is_safe_key(&dat) || paths::classify(&dat) != Some(FileClass::Analysis) {
        return Vec::new();
    }
    let Some((stem, _)) = dat.rsplit_once('.') else { return Vec::new() };
    tree.with_prefix(&format!("{stem}."))
        .into_iter()
        .filter(|e| {
            e.class == FileClass::Analysis && e.path.get(stem.len() + 1..).is_some_and(|ext| !ext.contains('/'))
        })
        .map(|e| e.path.clone())
        .collect()
}

/// Lookup rows referenced by the removed tracks and by nothing that remains.
pub(crate) fn unused_lookups(db: &Database, removed: &FxHashSet<u32>) -> Vec<(TableType, Vec<u32>)> {
    let (gone, kept): (Vec<_>, Vec<_>) = db.tracks.iter().partition(|t| removed.contains(&t.id));
    let unused = |candidates: Vec<u32>, used: FxHashSet<u32>| {
        let mut ids: Vec<u32> = candidates.into_iter().filter(|id| *id != 0 && !used.contains(id)).collect();
        ids.sort_unstable();
        ids.dedup();
        ids
    };
    let by = |tracks: &[&rekordbox_pdb::Track], f: fn(&rekordbox_pdb::Track) -> Vec<u32>| -> Vec<u32> {
        tracks.iter().flat_map(|t| f(t)).collect()
    };
    let artist_refs: fn(&rekordbox_pdb::Track) -> Vec<u32> =
        |t| vec![t.artist_id, t.remixer_id, t.original_artist_id, t.composer_id];

    let albums = unused(by(&gone, |t| vec![t.album_id]), by(&kept, |t| vec![t.album_id]).into_iter().collect());
    // Albums reference their artist: one that stays keeps it, one that goes may take it along.
    let (gone_albums, kept_albums): (Vec<_>, Vec<_>) = db.albums.iter().partition(|a| albums.contains(&a.id));
    let artists = unused(
        by(&gone, artist_refs).into_iter().chain(gone_albums.iter().map(|a| a.artist_id)).collect(),
        by(&kept, artist_refs).into_iter().chain(kept_albums.iter().map(|a| a.artist_id)).collect(),
    );
    let simple = |f: fn(&rekordbox_pdb::Track) -> Vec<u32>| unused(by(&gone, f), by(&kept, f).into_iter().collect());
    let per_table = [
        artists,
        albums,
        simple(|t| vec![t.genre_id]),
        simple(|t| vec![t.label_id]),
        simple(|t| vec![t.key_id]),
        simple(|t| vec![t.artwork_id]),
    ];
    LOOKUP_TABLES.into_iter().zip(per_table).filter(|(_, ids)| !ids.is_empty()).collect()
}

#[derive(Debug, Clone, Default, Serialize, JsonSchema)]
pub struct CleanReport {
    pub tracks_removed: usize,
    pub files_deleted: usize,
    pub freed_bytes: u64,
    pub backup: Option<String>,
    pub fingerprint: Option<Hash>,
}

pub fn apply(plan: &CleanPlan, device: &mut Device, ctx: &Ctx<'_>) -> Result<CleanReport> {
    if plan.is_noop() {
        return Ok(CleanReport::default());
    }
    if !plan.fits {
        return Err(Error::NoSpace {
            path: plan.root.clone(),
            needed: plan.needed_bytes,
            available: plan.free_space.unwrap_or(0),
        });
    }
    let _lock = commit::lock(device, None)?;
    // The drive must still hold the tracks that were planned (rekordbox may have re-exported).
    let changed = || Error::stale(device.name(), "changed");
    let still_there = |lib: &Library, planned: &[(u32, String)]| {
        planned.iter().all(|(id, path)| lib.track(*id).is_some_and(|t| &t.file_path == path))
    };
    let pdb_ids: Vec<u32> = plan.pdb_tracks.iter().map(|(id, _)| *id).collect();
    let pdb_before = match pdb_ids.is_empty() {
        true => None,
        false => Some(pdb::read_library(&device.path(paths::EXPORT_PDB))?).filter(|l| still_there(l, &plan.pdb_tracks)),
    };
    if !pdb_ids.is_empty() && pdb_before.is_none() {
        return Err(changed());
    }
    let plus_ids: Vec<u32> = plan.plus_tracks.iter().map(|(id, _)| *id).collect();
    let mut plus = match plus_ids.is_empty() {
        true => None,
        false => Some(device.library_plus()?),
    };
    let plus_before = plus.as_ref().map(OneLibrary::library).transpose()?;
    if plus_before.as_ref().is_some_and(|l| !still_there(l, &plan.plus_tracks)) {
        return Err(changed());
    }
    ctx.check()?;

    // 1. Databases, both edited and verified in memory before anything on the drive changes (a
    //    failure here leaves no backup and no interrupted operation). Nothing references the files
    //    once they are replaced.
    ctx.phase_start(Phase::Write, u64::from(pdb_before.is_some()) + u64::from(plus.is_some()), 0);
    let removed_pdb: FxHashSet<u32> = pdb_ids.iter().copied().collect();
    let pdb_bytes = match &pdb_before {
        None => None,
        Some(before) => {
            let mut editor = PdbEditor::from_bytes(device.pdb_bytes()?);
            let bytes = pdb::guard_editor(|| {
                editor.remove_tracks(&pdb_ids)?;
                for (table, rows) in &plan.lookups {
                    editor.remove_rows(*table, rows)?;
                }
                Ok(editor.to_bytes())
            })?;
            // Never write a database we can't read back.
            let check = pdb::parse_database(bytes.clone(), &device.path(paths::EXPORT_PDB))?;
            if check.tracks.len() != before.tracks.len() - pdb_ids.len()
                || check.tracks.iter().any(|t| removed_pdb.contains(&t.id))
                || check.playlist_entries.iter().any(|e| removed_pdb.contains(&e.track_id))
            {
                return Err(Error::Pdb("cleaned database failed verification; drive left unchanged".into()));
            }
            Some(bytes)
        }
    };
    let plus_bytes = match (&mut plus, &plus_before) {
        (Some(one), Some(before)) => {
            one.remove_tracks(&plus_ids)?;
            let bytes = one.to_encrypted()?;
            let check = OneLibrary::from_encrypted(&bytes, None)?.library()?;
            let removed: FxHashSet<u32> = plus_ids.iter().copied().collect();
            if check.tracks.len() != before.tracks.len() - plus_ids.len()
                || check.tracks.iter().any(|t| removed.contains(&t.id))
                || check.playlists.iter().any(|p| p.track_ids.iter().any(|id| removed.contains(id)))
            {
                return Err(Error::OneLibrary("cleaned database failed verification; drive left unchanged".into()));
            }
            Some(bytes)
        }
        _ => None,
    };
    // My Tag links of removed tracks go too: a later export.pdb track would reuse their ids.
    let ext_bytes = match pdb_bytes {
        Some(_) => crate::edit::ext_after(device, &pdb_ids, &[]).unwrap_or_else(|e| {
            ctx.warn(format!("My Tags in exportExt.pdb weren't updated: {e}"));
            None
        }),
        None => None,
    };
    ctx.phase_end(Phase::Write);

    // 2. Back up (the databases and every file that goes), swap the databases in, then delete
    // the files: nothing references them any more.
    let changes = Changes {
        databases: NewDatabases { pdb: pdb_bytes, ext: ext_bytes, plus: plus_bytes },
        files: Vec::new(),
        deletes: plan.files.clone(),
        remove_first: Vec::new(),
    };
    let done = commit::commit(device, changes, Options::new(LineageOp::Clean, "clean").backup("before cleaning"), ctx)?;
    Ok(CleanReport {
        tracks_removed: plan.tracks.len(),
        files_deleted: done.deleted,
        freed_bytes: done.freed,
        backup: done.backup,
        fingerprint: done.fingerprint,
    })
}

#[cfg(test)]
mod tests;
