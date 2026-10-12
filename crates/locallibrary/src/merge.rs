//! Merging one library into another: the answer for drives that hold *different* libraries
//! (two DJs going back to back, a second laptop, an old drive).
//!
//! For every source track the target doesn't already have, the audio file, its analysis files
//! (beat grid, cues, waveforms: no re-analysis needed) and its artwork are copied, and a row is
//! added to the target's `export.pdb` and OneLibrary `exportLibrary.db` (whichever it has).
//! Playlists are recreated, either inside one folder named after the source or merged into
//! same-named playlists.
//!
//! OneLibrary rows are copied whole from the source's OneLibrary database when it has one
//! (every column and the track's cue rows), and built from the `export.pdb` row otherwise, with
//! cue rows made from the track's analysis files, as `convert` makes them. Hot
//! cue bank lists that use the new tracks' cues come along too (OneLibrary only), and with
//! [`MergeOptions::history`] the source's history playlists.
//!
//! New tracks bring their My Tags into both of the target's databases (`exportExt.pdb` and
//! OneLibrary): each gets the source's from the same kind of database, else from the other one.
//! Categories and tags are matched by name and created where missing, within rekordbox's limits
//! (see [`crate::tags`]); a new tag gets the same id in both. Tracks matched as duplicates keep
//! the target's tags.
//!
//! A track counts as "already present" when the target has a file at the same path with the same
//! size, or (unless matching by path only) a track with the same artist, title and duration.
//! Paths that are taken by a *different* file get a ` (2)` suffix, and the analysis file's
//! embedded path is rewritten to match.
//!
//! The target's database is backed up first (see [`crate::backup`]), so `locallibrary undo` takes back a
//! merge.

use rustc_hash::{FxHashMap, FxHashSet};
use std::path::PathBuf;

use rekordbox_pdb::{NewTrack, PdbEditor};

use crate::error::{Action, Reason};
use crate::onelibrary::{ContentRecord, OneLibrary};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::commit::{self, Changes, NewDatabases, NewFile, Options};
use crate::device::{Device, LineageOp};
use crate::error::IoContext;
use crate::fsutil;
use crate::hash::Hash;
use crate::model::{self, Library, Playlist, Track};
use crate::paths;
use crate::pdb;
use crate::progress::{Ctx, Phase, WarningLimiter};
use crate::tags::{self, Carried, Created, DbTags, Source, TagNames, Wanted};
use crate::tree::FileTree;
use crate::{Error, Result, anlz};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PlaylistMode {
    /// Recreate the source's playlist tree inside a new folder (default).
    #[default]
    Folder,
    /// Merge into playlists with the same folder path, creating missing ones.
    ByName,
    /// Only add tracks.
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MatchMode {
    /// Same path + size, or same artist/title/duration (default).
    #[default]
    Metadata,
    /// Same path + size only.
    Path,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct MergeOptions {
    #[serde(default)]
    pub playlists: PlaylistMode,
    /// Folder name for [`PlaylistMode::Folder`] (default: the source drive's name).
    #[serde(default)]
    pub folder_name: Option<String>,
    /// Only merge these source playlists (paths like `Sets/Warmup` or unique names) and the
    /// tracks in them. Empty = everything.
    #[serde(default)]
    pub only_playlists: Vec<String>,
    #[serde(default)]
    pub match_mode: MatchMode,
    /// Also bring the source's history playlists (what players recorded during a set) across,
    /// except ones the target already has.
    #[serde(default)]
    pub history: bool,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct PlannedTrack {
    pub source_id: u32,
    pub name: String,
    /// Target key of the audio file.
    pub audio: String,
    pub renamed: bool,
    pub size: u64,
    #[serde(skip)]
    source_audio: String,
    /// (source key, target key) for each analysis file.
    #[serde(skip)]
    analysis: Vec<(String, String)>,
    #[serde(skip)]
    analyze_path: String,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct PlannedPlaylist {
    /// Target path of the playlist.
    pub path: String,
    pub create: bool,
    pub tracks_added: usize,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct MergePlan {
    pub source: String,
    pub target: PathBuf,
    pub new_tracks: Vec<PlannedTrack>,
    /// Source tracks already in the target.
    pub matched_tracks: usize,
    pub playlists: Vec<PlannedPlaylist>,
    pub artwork_files: usize,
    pub copy_bytes: u64,
    /// Estimated space needed on the target (cluster-rounded, including the new database).
    pub needed_bytes: u64,
    pub free_space: Option<u64>,
    pub fits: bool,
    pub warnings: Vec<String>,
    /// History playlists to bring across (with [`MergeOptions::history`]).
    pub history: Vec<String>,
    /// New tracks that bring My Tags along.
    pub tagged_tracks: usize,
    /// My Tag categories and tags the target gets for them.
    pub new_tags: Vec<Created>,
    /// Their ids in the source.
    #[serde(skip)]
    history_ids: Vec<u32>,
    /// The target's databases when planned (see [`crate::commit::lock`]).
    #[serde(skip)]
    state: Option<Hash>,
    /// source track id -> existing target track id
    #[serde(skip)]
    matches: FxHashMap<u32, u32>,
    #[serde(skip)]
    folder_name: String,
}

impl MergePlan {
    pub fn is_noop(&self) -> bool {
        self.new_tracks.is_empty()
            && self.playlists.iter().all(|p| !p.create && p.tracks_added == 0)
            && self.history.is_empty()
    }
}

fn fold(s: &str) -> String {
    s.to_lowercase()
}

/// Source playlists selected by `opts.only_playlists` (all when empty).
fn selected_playlists(source: &Library, opts: &MergeOptions) -> Result<FxHashSet<u32>> {
    if opts.only_playlists.is_empty() {
        return Ok(source.playlists.iter().map(|p| p.id).collect());
    }
    let mut out = FxHashSet::default();
    for q in &opts.only_playlists {
        let p =
            source.find_playlist(q).ok_or_else(|| Error::NotFound(format!("playlist {q:?} in the source library")))?;
        // A folder selects everything below it.
        let mut stack = vec![p.id];
        while let Some(id) = stack.pop() {
            out.insert(id);
            stack.extend(source.playlists.iter().filter(|c| c.parent_id == Some(id)).map(|c| c.id));
        }
    }
    Ok(out)
}

pub fn plan(source: &Device, target: &Device, opts: &MergeOptions, ctx: &Ctx<'_>) -> Result<MergePlan> {
    if source.root() == target.root() {
        return Err(Error::invalid("source and target are the same drive"));
    }
    source.finished()?;
    let src_lib = source.library()?;
    if !target.has_export() {
        let (from, to) = (source.root().display().to_string(), target.root().display().to_string());
        return Err(Error::invalid(format!(
            "{} has no rekordbox export to merge into; sync copies the whole library instead",
            target.name()
        ))
        .because(Reason::NoExport)
        .with("drive", &to)
        .action(Action::call(
            "sync.plan",
            serde_json::json!({ "left": from, "right": to, "direction": "left_to_right" }),
            format!("Copy {}'s whole library", source.name()),
        )));
    }
    let dst_lib = target.library()?;
    let src_tree = source.scan(Some(&src_lib), ctx)?;
    let dst_tree = target.scan(Some(&dst_lib), ctx)?;

    let selected = selected_playlists(&src_lib, opts)?;
    let wanted_tracks: Option<FxHashSet<u32>> = (!opts.only_playlists.is_empty()).then(|| {
        src_lib
            .playlists
            .iter()
            .filter(|p| selected.contains(&p.id))
            .flat_map(|p| p.track_ids.iter().copied())
            .collect()
    });

    let mut warnings = Vec::new();
    // `dst_lib` is export.pdb's when the target has one; OneLibrary's tracks count too.
    let dst_plus =
        if target.has_device_library() { crate::clean::open_plus(target, true, &mut warnings)? } else { None };

    // Index the target.
    let dst_by_path: FxHashMap<String, &Track> =
        dst_lib.tracks.iter().map(|t| (fold(&paths::key(&t.file_path)), t)).collect();
    let dst_by_meta: FxHashMap<String, &Track> = dst_lib.tracks.iter().map(|t| (t.fingerprint_key(), t)).collect();
    // Paths that are off limits: everything on the target, plus everything its database
    // references even if the file is missing (so two rows never end up sharing one file).
    let mut taken: FxHashSet<String> = dst_tree.entries.keys().map(|k| fold(k)).collect();
    for t in dst_lib.tracks.iter().chain(dst_plus.iter().flat_map(|(l, _)| l.tracks.iter())) {
        taken.insert(fold(&paths::key(&t.file_path)));
        if let Some((stem, _)) = paths::key(&t.analyze_path).rsplit_once('.') {
            for ext in ["DAT", "EXT", "2EX"] {
                taken.insert(fold(&format!("{stem}.{ext}")));
            }
        }
        if let Some(a) = &t.artwork_path {
            taken.insert(fold(&paths::key(a)));
            taken.insert(fold(&crate::artwork::medium_variant(&paths::key(a))));
        }
    }

    let cluster = fsutil::cluster_size(target.root());
    let round = |n: u64| n.div_ceil(cluster) * cluster;
    let mut new_tracks = Vec::new();
    let mut unanalysed = Vec::new();
    let mut matches = FxHashMap::default();
    let mut copy_bytes = 0u64;
    let mut needed = 0u64;
    let mut artwork_ids = FxHashSet::default();

    for t in &src_lib.tracks {
        if wanted_tracks.as_ref().is_some_and(|w| !w.contains(&t.id)) {
            continue;
        }
        let src_key = paths::key(&t.file_path);
        // A crafted or corrupt database could point anywhere, including at the target's own
        // database files.
        if !paths::is_safe_key(&src_key) || fold(&src_key).starts_with("pioneer/") {
            warnings.push(format!("{}: unsafe file path {:?}; skipped", t.display_name(), t.file_path));
            continue;
        }
        if let Some(d) = dst_by_path.get(&fold(&src_key))
            && d.file_size == t.file_size
        {
            matches.insert(t.id, d.id);
            continue;
        }
        if opts.match_mode == MatchMode::Metadata
            && let Some(d) = dst_by_meta.get(&t.fingerprint_key())
        {
            matches.insert(t.id, d.id);
            continue;
        }
        let Some(src_entry) = src_tree.resolve(&src_key) else {
            warnings.push(format!(
                "{}: audio file {} is missing on the source; skipped",
                t.display_name(),
                t.file_path
            ));
            continue;
        };

        let src_key = src_entry.path.clone();
        // The target's scan only lists PIONEER/, Contents/ and audio its own library keeps
        // elsewhere: anything else already on the drive at a path is the DJ's, so it's taken too.
        let on_disk = |k: &str| std::fs::symlink_metadata(target.path(k)).is_ok();
        let audio = unique_key(&paths::key(&t.file_path), &mut taken, &on_disk);
        let renamed = audio != paths::key(&t.file_path);
        let (analysis, analyze_path) = plan_analysis(t, &src_tree, &audio, &mut taken);
        if analysis.is_empty() {
            unanalysed.push(t.display_name());
        }
        if let (Some(id), Some(art)) = (t.artwork_id, &t.artwork_path)
            && artwork_ids.insert(id)
        {
            let key = paths::key(art);
            for k in [crate::artwork::medium_variant(&key), key] {
                if let Some(e) = src_tree.resolve(&k) {
                    copy_bytes += e.size;
                    needed += round(e.size);
                }
            }
        }
        for size in std::iter::once(src_entry.size)
            .chain(analysis.iter().filter_map(|(s, _)| src_tree.resolve(s)).map(|e| e.size))
        {
            copy_bytes += size;
            needed += round(size);
        }
        new_tracks.push(PlannedTrack {
            source_id: t.id,
            name: t.display_name(),
            audio,
            renamed,
            size: src_entry.size,
            source_audio: src_key,
            analysis,
            analyze_path,
        });
    }

    if !unanalysed.is_empty() {
        let sample: Vec<&str> = unanalysed.iter().take(3).map(String::as_str).collect();
        let more = if unanalysed.len() > 3 { format!(" and {} more", unanalysed.len() - 3) } else { String::new() };
        warnings.push(format!(
            "{} track(s) have no analysis files on the source ({}{more}); players will show them without beat grids or cues",
            unanalysed.len(),
            sample.join(", ")
        ));
    }

    let folder_name =
        unique_top_level_name(&dst_lib, opts.folder_name.clone().unwrap_or_else(|| source.name()).as_str());
    // What the target's export.pdb can't hold is found now, not when applying.
    let dst_pdb = match target.has_device_library() {
        true => Some(pdb::parse_database(target.pdb_bytes()?, &target.path(paths::EXPORT_PDB))?),
        false => None,
    };
    if let Some(db) = &dst_pdb {
        new_tracks.retain(|t| {
            let fits = src_lib.track(t.source_id).is_none_or(|st| db.check_new_track(&new_track_row(st, t, 0)).is_ok());
            if !fits {
                warnings.push(format!("{}: its details are too long for the target's export.pdb; skipped", t.name));
            }
            fits
        });
    }
    let new_ids: FxHashSet<u32> = new_tracks.iter().map(|t| t.source_id).collect();
    let history_ids = if opts.history { plan_history(&src_lib, &dst_lib, &matches, &new_ids) } else { Vec::new() };
    let history: Vec<String> = history_ids
        .iter()
        .filter_map(|id| src_lib.history.iter().find(|h| h.id == *id))
        .map(|h| h.name.clone())
        .collect();
    let playlists = plan_playlists(&src_lib, &dst_lib, &selected, opts.playlists, &folder_name, &matches, &new_ids);
    if let Some(db) = &dst_pdb {
        check_names(db, &src_lib, &dst_lib, &selected, opts.playlists, &folder_name, &history)?;
    }
    let (tagged_tracks, new_tags) =
        plan_my_tags(source, &src_lib, target, &dst_lib, &dst_plus, &new_tracks, &mut warnings);
    if !new_tracks.is_empty() || !playlists.is_empty() {
        warnings.extend(target.rekordbox_sync_warning(
            "it doesn't know about merged tracks and playlists, so check them after rekordbox next syncs this drive",
        ));
    }

    // Each new database is written next to the old one before replacing it; rows take roughly
    // 1 KiB per track including lookup rows and page slack.
    let size = |key| std::fs::metadata(target.path(key)).map(|m| m.len()).unwrap_or(0);
    for db in [paths::EXPORT_PDB, paths::LIBRARY_PLUS_DB].into_iter().filter(|k| target.path(k).is_file()) {
        needed += round(size(db) + new_tracks.len() as u64 * 1024);
    }
    let free_space = fsutil::available_space(target.root());
    let fits = free_space.is_none_or(|f| needed <= f);
    if !fits {
        warnings
            .push(format!("needs about {needed} bytes but only {} are free on the target", free_space.unwrap_or(0)));
    }
    Ok(MergePlan {
        source: source.name(),
        target: target.root().to_path_buf(),
        matched_tracks: matches.len(),
        new_tracks,
        playlists,
        artwork_files: artwork_ids.len(),
        copy_bytes,
        needed_bytes: needed,
        free_space,
        fits,
        warnings,
        history,
        tagged_tracks,
        new_tags,
        history_ids,
        matches,
        folder_name,
        state: Some(crate::commit::database_state(target)),
    })
}

/// A drive's My Tags, in each of its databases (missing or unreadable ones are `None`).
struct DriveTags {
    ext: Option<DbTags>,
    plus: Option<DbTags>,
}

impl DriveTags {
    /// `library` is [`Device::library`]'s; `plus` the drive's OneLibrary, if it's open already.
    fn read(device: &Device, library: &Library, plus: Option<&OneLibrary>, warnings: &mut Vec<String>) -> DriveTags {
        let mut warn = |e: Error| warnings.push(format!("{e}; My Tags there are left out of the merge"));
        let ext = match device.has_device_library() {
            true => DbTags::ext_of(device, library).unwrap_or_else(|e| {
                warn(e);
                None
            }),
            false => None,
        };
        let opened;
        let one = match plus {
            Some(one) => Some(one),
            None if device.has_library_plus() => {
                opened = device.library_plus().ok();
                opened.as_ref()
            }
            None => None,
        };
        let plus = one.and_then(|one| match one.library().and_then(|l| DbTags::plus(one, &l)) {
            Ok(t) => Some(t),
            Err(e) => {
                warn(e);
                None
            }
        });
        DriveTags { ext, plus }
    }

    /// A source track's tags for each of the target's databases: from the same kind of database,
    /// else the other one.
    fn of(&self, key: &str) -> (TagNames, TagNames) {
        let (ext, plus) = (self.ext.as_ref(), self.plus.as_ref());
        (tags::tags_of(ext, plus, key), tags::tags_of(plus, ext, key))
    }
}

/// Refuse a name the merge gives a new playlist, folder or history list that is too long for a
/// row of the target's export.pdb (`db`), whose editor would only refuse it when applying
/// (OneLibrary takes any length). The names are the ones [`write_playlists`] and
/// [`write_history`] write.
fn check_names(
    db: &rekordbox_pdb::Database,
    src: &Library,
    dst: &Library,
    selected: &FxHashSet<u32>,
    mode: PlaylistMode,
    folder_name: &str,
    history: &[String],
) -> Result<()> {
    let too_long = |what: String, name: &str| {
        Error::invalid(format!("{what} is too long for the target's export.pdb ({} characters)", name.chars().count()))
    };
    match mode {
        PlaylistMode::None => {}
        PlaylistMode::Folder => {
            if db.check_playlist_name(folder_name).is_err() {
                let e = too_long("the merge folder's name".into(), folder_name);
                return Err(Error::invalid(format!("{e}; choose another folder name"))
                    .action(Action::retry(serde_json::json!({ "folder_name": null }), "Choose another folder name")));
            }
            for p in src.playlists.iter().filter(|p| selected.contains(&p.id)) {
                if db.check_playlist_name(&p.name).is_err() {
                    return Err(too_long(format!("the name of {}", src.playlist_path(p.id)), &p.name));
                }
                // One level down, in the merge folder: past `MAX_DEPTH`, a reader takes it for a loop.
                if src.playlist_names(p.id).len() >= crate::model::MAX_DEPTH {
                    return Err(Error::invalid(format!(
                        "{} would be {} levels deep in the merge folder; playlists nest {} levels at most, so \
                         merge by name instead",
                        src.playlist_path(p.id),
                        crate::model::MAX_DEPTH + 1,
                        crate::model::MAX_DEPTH
                    )));
                }
            }
        }
        PlaylistMode::ByName => {
            for p in src.playlists.iter().filter(|p| selected.contains(&p.id) && !p.is_folder) {
                let path = src.playlist_path(p.id);
                if merge_target(dst, &path, false).is_some() {
                    continue;
                }
                let parts: Vec<&str> = path.split('/').collect();
                let folders =
                    (0..parts.len() - 1).filter(|i| merge_target(dst, &parts[..=*i].join("/"), true).is_none());
                for name in folders.map(|i| parts[i]).chain([p.name.as_str()]) {
                    if db.check_playlist_name(name).is_err() {
                        return Err(too_long(format!("a name in {path}"), name));
                    }
                }
            }
        }
    }
    for name in history {
        if db.check_history_name(name).is_err() {
            return Err(too_long("the name of a history list".into(), name));
        }
    }
    Ok(())
}

/// How many new tracks bring My Tags, and the categories and tags that creates in the target.
fn plan_my_tags(
    source: &Device,
    src_lib: &Library,
    target: &Device,
    dst_lib: &Library,
    dst_plus: &Option<(Library, OneLibrary)>,
    new_tracks: &[PlannedTrack],
    warnings: &mut Vec<String>,
) -> (usize, Vec<Created>) {
    if new_tracks.is_empty() {
        return (0, Vec::new());
    }
    let src = DriveTags::read(source, src_lib, None, warnings);
    let dst = DriveTags::read(target, dst_lib, dst_plus.as_ref().map(|(_, one)| one), warnings);
    let (mut for_ext, mut for_plus): (Wanted, Wanted) = (Vec::new(), Vec::new());
    let mut tagged = 0;
    for (i, t) in new_tracks.iter().enumerate() {
        let Some(st) = src_lib.track(t.source_id) else { continue };
        let (ext, plus) = src.of(&fold(&paths::key(&st.file_path)));
        tagged += usize::from(!ext.is_empty() || !plus.is_empty());
        for_ext.push((i as u32, fold(&t.audio), ext));
        for_plus.push((i as u32, fold(&t.audio), plus));
    }
    let mut created = Vec::new();
    for (db, other, wanted, source) in
        [(&dst.ext, &dst.plus, &for_ext, Source::ExportExt), (&dst.plus, &dst.ext, &for_plus, Source::OneLibrary)]
    {
        let Some(db) = db else { continue };
        let carried = tags::carry_preview(db, other.as_ref(), wanted);
        warnings.extend(carried.warning(source.file_name()));
        created.extend(carried.created.into_iter().map(|path| Created { path, database: source }));
    }
    if tagged > 0 && target.has_device_library() && !target.path(paths::EXPORT_EXT_PDB).is_file() {
        warnings.push(format!(
            "{tagged} new track(s) have My Tags, but the target has no exportExt.pdb, so players that read export.pdb \
             won't show them"
        ));
    }
    (tagged, created)
}

/// Source history playlists worth bringing across: ones holding a track the merge carries, that
/// the target doesn't already have (same name, same tracks).
pub(crate) fn plan_history(
    src: &Library,
    dst: &Library,
    matches: &FxHashMap<u32, u32>,
    new_ids: &FxHashSet<u32>,
) -> Vec<u32> {
    let existing: FxHashSet<(String, Vec<u32>)> =
        dst.history.iter().filter(|h| !h.is_folder).map(|h| (fold(&h.name), h.track_ids.clone())).collect();
    src.history
        .iter()
        .filter(|h| !h.is_folder && h.track_ids.iter().any(|id| matches.contains_key(id) || new_ids.contains(id)))
        .filter(|h| {
            // Every track already in the target, under the same name: it's been merged before.
            let mapped: Option<Vec<u32>> = h.track_ids.iter().map(|id| matches.get(id).copied()).collect();
            !mapped.is_some_and(|ids| existing.contains(&(fold(&h.name), ids)))
        })
        .map(|h| h.id)
        .collect()
}

/// `key`, or `key` with ` (2)`, ` (3)`... before the extension, whichever is free: not `taken`
/// and not `on_disk`.
pub(crate) fn unique_key(key: &str, taken: &mut FxHashSet<String>, on_disk: &dyn Fn(&str) -> bool) -> String {
    let mut candidate = key.to_string();
    let (stem, ext) = match key.rsplit_once('.') {
        Some((s, e)) if !e.contains('/') => (s.to_string(), format!(".{e}")),
        _ => (key.to_string(), String::new()),
    };
    let mut n = 2;
    while taken.contains(&fold(&candidate)) || on_disk(&candidate) {
        candidate = format!("{stem} ({n}){ext}");
        n += 1;
    }
    taken.insert(fold(&candidate));
    candidate
}

/// Analysis files for a track and where they go. Keeps the source directory name when free,
/// otherwise derives a new `PIONEER/USBANLZ/Pxxx/xxxxxxxx` from the target audio path.
fn plan_analysis(
    t: &Track,
    src_tree: &FileTree,
    target_audio: &str,
    taken: &mut FxHashSet<String>,
) -> (Vec<(String, String)>, String) {
    let dat = paths::key(&t.analyze_path);
    if !paths::is_safe_key(&dat) || paths::classify(&dat) != Some(paths::FileClass::Analysis) {
        return (Vec::new(), String::new());
    }
    let Some((dir, file)) = dat.rsplit_once('/') else { return (Vec::new(), String::new()) };
    let stem = file.rsplit_once('.').map(|(s, _)| s).unwrap_or(file);
    let found: Vec<&crate::tree::FileEntry> = src_tree
        .with_prefix(&format!("{dir}/{stem}."))
        .into_iter()
        .filter(|e| e.class == paths::FileClass::Analysis)
        .collect();
    let Some(first) = found.first() else { return (Vec::new(), String::new()) };
    // Use the on-disk spelling of the directory from here on.
    let dir = first.path.rsplit_once('/').map_or(dir, |(d, _)| d).to_string();
    let dir = dir.as_str();
    let siblings: Vec<&str> = found.iter().map(|e| e.path.as_str()).collect();
    let dir_taken =
        |d: &str, taken: &FxHashSet<String>| siblings.iter().any(|s| taken.contains(&fold(&s.replacen(dir, d, 1))));
    let mut target_dir = dir.to_string();
    let mut salt = 0u32;
    while dir_taken(&target_dir, taken) {
        target_dir = analysis_dir(target_audio, salt);
        salt += 1;
    }
    let pairs: Vec<(String, String)> =
        siblings.iter().map(|s| (s.to_string(), s.replacen(dir, &target_dir, 1))).collect();
    for (_, d) in &pairs {
        taken.insert(fold(d));
    }
    let analyze_path = format!("{target_dir}/{file}");
    (pairs, analyze_path)
}

/// An analysis folder for the audio file `audio` (a key), as rekordbox lays them out
/// (`PIONEER/USBANLZ/Pxxx/xxxxxxxx`): from a hash of its path, another for each `salt`.
pub(crate) fn analysis_dir(audio: &str, salt: u32) -> String {
    let h = Hash::of(format!("{audio}\0{salt}").as_bytes());
    let b = h.as_bytes();
    let p = u16::from_be_bytes([b[0], b[1]]) & 0x0fff;
    let q = u32::from_be_bytes([b[2], b[3], b[4], b[5]]);
    format!("PIONEER/USBANLZ/P{p:03X}/{q:08X}")
}

fn unique_top_level_name(lib: &Library, base: &str) -> String {
    let names: FxHashSet<String> = lib.children(None).iter().map(|p| fold(&p.name)).collect();
    let mut name = base.to_string();
    let mut n = 2;
    while names.contains(&fold(&name)) {
        name = format!("{base} ({n})");
        n += 1;
    }
    name
}

/// The target playlist (or folder) a source one at `path` merges into by name: of the target's
/// nodes of its kind, the one [`model::pick`] picks in display order, so plan and apply agree when
/// the target has names that differ only in case, or same-named siblings. Only its kind counts,
/// unlike in edits: a folder shown before a playlist of the same name doesn't hide it.
pub(crate) fn merge_target<'a>(dst: &'a Library, path: &str, folder: bool) -> Option<&'a Playlist> {
    model::pick(dst.display_order().into_iter().filter(|(_, d)| d.is_folder == folder), path)
}

pub(crate) fn plan_playlists(
    src: &Library,
    dst: &Library,
    selected: &FxHashSet<u32>,
    mode: PlaylistMode,
    folder_name: &str,
    matches: &FxHashMap<u32, u32>,
    new_ids: &FxHashSet<u32>,
) -> Vec<PlannedPlaylist> {
    let carried = |id: &u32| matches.contains_key(id) || new_ids.contains(id);
    let mut out = Vec::new();
    match mode {
        PlaylistMode::None => {}
        PlaylistMode::Folder => {
            out.push(PlannedPlaylist { path: folder_name.to_string(), create: true, tracks_added: 0 });
            for p in src.playlists.iter().filter(|p| selected.contains(&p.id)) {
                out.push(PlannedPlaylist {
                    path: format!("{folder_name}/{}", src.playlist_path(p.id)),
                    create: true,
                    tracks_added: p.track_ids.iter().filter(|id| carried(id)).count(),
                });
            }
        }
        PlaylistMode::ByName => {
            for p in src.playlists.iter().filter(|p| selected.contains(&p.id) && !p.is_folder) {
                let path = src.playlist_path(p.id);
                let existing = merge_target(dst, &path, false);
                let already: FxHashSet<u32> =
                    existing.map(|d| d.track_ids.iter().copied().collect()).unwrap_or_default();
                let added = p
                    .track_ids
                    .iter()
                    .filter(|id| match matches.get(id) {
                        Some(dst_id) => !already.contains(dst_id),
                        None => new_ids.contains(id),
                    })
                    .count();
                out.push(PlannedPlaylist { path, create: existing.is_none(), tracks_added: added });
            }
        }
    }
    out
}

#[derive(Debug, Clone, Default, Serialize, JsonSchema)]
pub struct MergeReport {
    pub tracks_added: usize,
    pub tracks_matched: usize,
    pub playlists_created: usize,
    pub playlist_entries_added: usize,
    pub artwork_added: usize,
    pub history_playlists_added: usize,
    pub hot_cue_bank_lists_added: usize,
    /// New tracks given My Tags (in either database).
    pub tagged_tracks: usize,
    pub copied_bytes: u64,
    pub backup: Option<String>,
    pub fingerprint: Option<Hash>,
}

pub fn apply(
    plan: &MergePlan,
    source: &Device,
    target: &mut Device,
    opts: &MergeOptions,
    ctx: &Ctx<'_>,
) -> Result<MergeReport> {
    if !plan.fits {
        return Err(Error::NoSpace {
            path: plan.target.clone(),
            needed: plan.needed_bytes,
            available: plan.free_space.unwrap_or(0),
        });
    }
    let _lock = commit::lock(target, plan.state)?;
    let src_lib = source.library()?;
    let src_tree = source.scan(Some(&src_lib), ctx)?;
    let mut report = MergeReport { tracks_matched: plan.matched_tracks, ..Default::default() };
    // The source's own OneLibrary rows carry every column; read them if it has a database.
    let src_plus = if source.has_library_plus() { source.library_plus().ok() } else { None };
    // My Tags each new track brings, for the target's exportExt.pdb and OneLibrary.
    let mut tag_warnings = Vec::new();
    let src_tags = DriveTags::read(source, &src_lib, src_plus.as_ref(), &mut tag_warnings);
    let (mut ext_tags, mut plus_tags) = (FxHashMap::default(), FxHashMap::default());
    for t in &plan.new_tracks {
        let Some(st) = src_lib.track(t.source_id) else { continue };
        let (ext, plus) = src_tags.of(&fold(&paths::key(&st.file_path)));
        ext_tags.insert(t.source_id, ext);
        plus_tags.insert(t.source_id, plus);
    }

    let mut warnings = WarningLimiter::new(ctx, 3);
    // The source's files must all still be there, and the target's paths fit to write.
    let mut audio = Vec::new();
    for t in &plan.new_tracks {
        let src_entry = src_tree.resolve(&t.source_audio).ok_or_else(|| Error::NotFound(t.source_audio.clone()))?;
        audio.push((src_entry.mtime, target.checked_path(&t.audio)?));
        for (_, to) in &t.analysis {
            target.checked_path(to)?;
        }
    }

    // 1. Databases: each one the target has is edited and verified in memory before anything on
    // the drive changes (a merge that fails here leaves no backup, no interrupted operation and
    // no copied files). Artwork copies are only planned.
    let mut copies: Vec<(PathBuf, String)> = Vec::new();
    let target_lib = target.library()?;
    let has_pdb = target.has_device_library();
    let mut plus_warnings = Vec::new();
    let plus = crate::clean::open_plus(target, has_pdb, &mut plus_warnings)?;
    for w in plus_warnings {
        warnings.warn("unreadable OneLibrary", w);
    }
    let ext_path = target.path(paths::EXPORT_EXT_PDB);
    let ext = match has_pdb && ext_path.is_file() {
        true => {
            let bytes = std::fs::read(&ext_path).at(&ext_path)?;
            match DbTags::ext(&bytes, &target_lib) {
                Ok(tags) => Some((bytes, tags)),
                Err(e) => {
                    tag_warnings.push(format!("{e}; the merged tracks' My Tags aren't added to it"));
                    None
                }
            }
        }
        false => None,
    };
    for w in tag_warnings {
        warnings.warn("My Tags", w);
    }
    ctx.phase_start(Phase::Write, u64::from(has_pdb) + u64::from(plus.is_some()), 0);
    let tgt: &Device = target;
    let mut pdb_bytes = None;
    // Source track id -> export.pdb id, and export.pdb's tracks once merged.
    let mut id_map: FxHashMap<u32, u32> = plan.matches.clone();
    let mut pdb_after = None;
    if has_pdb {
        let mut editor = PdbEditor::from_bytes(target.pdb_bytes()?);
        let mut artwork_map: FxHashMap<u32, u32> = FxHashMap::default();
        // The editor indexes raw bytes; contain any panic on a malformed database.
        let bytes = pdb::guard_editor(|| {
            for t in &plan.new_tracks {
                ctx.check()?;
                let st = src_lib
                    .track(t.source_id)
                    .ok_or_else(|| Error::NotFound(format!("source track {}", t.source_id)))?;
                let artwork_id = match (st.artwork_id, st.artwork_path.as_deref()) {
                    (Some(src_art), Some(src_path)) => match artwork_map.get(&src_art) {
                        Some(id) => *id,
                        None => {
                            let id = match copy_artwork(&mut editor, &src_tree, src_path, tgt, &mut copies)? {
                                Some(id) => {
                                    report.artwork_added += 1;
                                    id
                                }
                                None => {
                                    warnings.warn(
                                        "missing artwork files",
                                        format!("artwork {src_path} missing on the source"),
                                    );
                                    0
                                }
                            };
                            artwork_map.insert(src_art, id);
                            id
                        }
                    },
                    _ => 0,
                };
                let new_id = editor.add_track(&new_track_row(st, t, artwork_id))?;
                id_map.insert(t.source_id, new_id);
                report.tracks_added += 1;
            }
            let (created, entries) =
                write_playlists(&mut editor, &src_lib, &target_lib, opts, &plan.folder_name, &id_map)?;
            report.playlists_created = created;
            report.playlist_entries_added = entries;
            report.history_playlists_added = write_history(&mut editor, &src_lib, &plan.history_ids, &id_map)?;
            Ok(editor.to_bytes())
        })?;
        // Never write a database we can't read back.
        let check = pdb::parse_database(bytes.clone(), &target.path(paths::EXPORT_PDB))?;
        if check.tracks.len() != target_lib.tracks.len() + report.tracks_added {
            return Err(Error::Pdb("merged database failed verification; drive left unchanged".into()));
        }
        pdb_after = Some(pdb::library_from_database(&check));
        pdb_bytes = Some(bytes);
    }
    let mut plus_bytes = None;
    let mut plus_after = None;
    let mut tagged: Vec<Carried> = Vec::new();
    if let Some((plus_lib, mut one)) = plus {
        // Planning said a OneLibrary whose My Tags can't be read is left out of them.
        let tags_read = match DbTags::plus(&one, &plus_lib) {
            Ok(_) => true,
            Err(e) => {
                warnings.warn("My Tags", format!("{e}; the merged tracks' My Tags aren't added to it"));
                false
            }
        };
        let into = PlusMerge {
            source,
            src_lib: &src_lib,
            src_plus: src_plus.as_ref(),
            src_tree: &src_tree,
            target: tgt,
            target_lib: &target_lib,
            plus_lib: &plus_lib,
            primary: !has_pdb,
            my_tags: tags_read.then_some(&plus_tags),
            ext_tags: ext.as_ref().map(|(_, tags)| tags),
        };
        let (bytes, carried) = into.apply(&mut one, plan, opts, &mut copies, &mut report, &mut warnings, ctx)?;
        if let Some(w) = carried.warning(Source::OneLibrary.file_name()) {
            warnings.warn("My Tags", w);
        }
        tagged.push(carried);
        plus_after = tags_read.then(|| one.library().and_then(|l| DbTags::plus(&one, &l))).transpose()?;
        plus_bytes = Some(bytes);
    }
    // exportExt.pdb after OneLibrary, so a tag new to both gets OneLibrary's id there too.
    let mut ext_bytes = None;
    if let (Some((bytes, _)), Some(after)) = (ext, &pdb_after) {
        let wanted: Wanted = plan
            .new_tracks
            .iter()
            .filter_map(|t| Some((*id_map.get(&t.source_id)?, fold(&t.audio), ext_tags.remove(&t.source_id)?)))
            .collect();
        let fresh: Vec<u32> = wanted.iter().map(|(id, _, _)| *id).collect();
        let (out, carried) = tags::carry_into_ext(bytes, after, &fresh, &wanted, plus_after.as_ref())?;
        if let Some(w) = carried.warning(Source::ExportExt.file_name()) {
            warnings.warn("My Tags", w);
        }
        tagged.push(carried);
        ext_bytes = out;
    }
    report.tagged_tracks = tagged.iter().flat_map(|c| &c.tracks).collect::<FxHashSet<_>>().len();
    ctx.phase_end(Phase::Write);

    // 2. Back up, copy the files next to where they go, then swap them and the databases in.
    // Analysis files are pointed at the (possibly renamed) audio file as they're copied.
    let warnings = std::cell::RefCell::new(warnings);
    let mut files = Vec::new();
    for (t, (mtime, _)) in plan.new_tracks.iter().zip(audio) {
        files.push(NewFile::copy(t.audio.clone(), src_tree.fs_path(&t.source_audio), t.size, Some(mtime)));
        for (from, to) in &t.analysis {
            let src_path = src_tree.fs_path(from);
            let size = std::fs::metadata(&src_path).map(|m| m.len()).unwrap_or(0);
            let warnings = &warnings;
            files.push(NewFile::make(to.clone(), size, move || {
                let bytes = std::fs::read(&src_path).at(&src_path)?;
                Ok(match anlz::rewrite_path(&bytes, &paths::device_path(&t.audio)) {
                    Ok(b) => b,
                    Err(_) => {
                        warnings.borrow_mut().warn(
                            "unrecognised analysis files",
                            format!("{from}: unrecognised analysis format, copied unchanged"),
                        );
                        bytes
                    }
                })
            }));
        }
    }
    for (src, to) in &copies {
        let size = std::fs::metadata(src).map(|m| m.len()).unwrap_or(0);
        files.push(NewFile::copy(to.clone(), src.clone(), size, None));
    }
    let changes = Changes {
        databases: NewDatabases { pdb: pdb_bytes, ext: ext_bytes, plus: plus_bytes },
        files,
        deletes: Vec::new(),
        remove_first: Vec::new(),
    };
    let mut opts = Options::new(LineageOp::Merge, format!("merge from {}", plan.source))
        .backup(format!("before merging {}", plan.source));
    opts.source = source.id();
    let done = commit::commit(target, changes, opts, ctx);
    warnings.into_inner().finish();
    let done = done?;
    report.backup = done.backup;
    report.fingerprint = done.fingerprint;
    report.copied_bytes = done.copied_bytes;
    Ok(report)
}

/// Merging into the target's OneLibrary database.
struct PlusMerge<'a> {
    source: &'a Device,
    src_lib: &'a Library,
    src_plus: Option<&'a OneLibrary>,
    src_tree: &'a FileTree,
    target: &'a Device,
    /// The target's library as [`Device::library`] reads it (`plan.matches` uses its ids).
    target_lib: &'a Library,
    /// The target's OneLibrary library.
    plus_lib: &'a Library,
    /// OneLibrary is the target's only database, so the report counts what happens here.
    primary: bool,
    /// Source track id -> the My Tags it brings; `None` leaves the target's My Tags alone.
    my_tags: Option<&'a FxHashMap<u32, TagNames>>,
    /// The target's `exportExt.pdb` tags, for new tags' names and ids.
    ext_tags: Option<&'a DbTags>,
}

impl PlusMerge<'_> {
    #[allow(clippy::too_many_arguments)]
    fn apply(
        &self,
        one: &mut OneLibrary,
        plan: &MergePlan,
        opts: &MergeOptions,
        copies: &mut Vec<(PathBuf, String)>,
        report: &mut MergeReport,
        warnings: &mut WarningLimiter<'_, '_>,
        ctx: &Ctx<'_>,
    ) -> Result<(Vec<u8>, Carried)> {
        let by_path = |lib: &Library| -> FxHashMap<String, u32> {
            lib.tracks.iter().map(|t| (fold(&paths::key(&t.file_path)), t.id)).collect()
        };
        // Source tracks the target already has, in OneLibrary's ids.
        let mut id_map: FxHashMap<u32, u32> = if self.primary {
            plan.matches.clone()
        } else {
            let plus_ids = by_path(self.plus_lib);
            let mut missing = 0;
            let map = plan
                .matches
                .iter()
                .filter_map(|(src, dst)| {
                    let path = &self.target_lib.track(*dst)?.file_path;
                    let found = plus_ids.get(&fold(&paths::key(path))).map(|id| (*src, *id));
                    missing += usize::from(found.is_none());
                    found
                })
                .collect();
            if missing > 0 {
                warnings.warn(
                    "tracks missing from OneLibrary",
                    format!(
                        "{missing} matched track(s) are in the target's export.pdb but not its exportLibrary.db; their playlist entries are left out of OneLibrary"
                    ),
                );
            }
            map
        };
        // Planning only warned of a source OneLibrary whose tracks can't be read (its My Tags are
        // left out): its rows are built from export.pdb's instead.
        let src_plus_ids = self.src_plus.and_then(|one| one.library().ok()).map(|l| by_path(&l));
        let defaults = one.content_defaults()?;
        let mut cue_ids: FxHashMap<i64, i64> = FxHashMap::default();
        let mut images: FxHashMap<u32, Option<u32>> = FxHashMap::default();
        let mut wanted: Wanted = Vec::new();
        for t in &plan.new_tracks {
            ctx.check()?;
            let st = self
                .src_lib
                .track(t.source_id)
                .ok_or_else(|| Error::NotFound(format!("source track {}", t.source_id)))?;
            let image = match (st.artwork_id, st.artwork_path.as_deref()) {
                (Some(art), Some(src_path)) => match images.get(&art) {
                    Some(id) => *id,
                    None => {
                        let id = copy_image(one, self.src_tree, src_path, self.target, copies)?;
                        match (id, self.primary) {
                            (Some(_), true) => report.artwork_added += 1,
                            (None, true) => warnings
                                .warn("missing artwork files", format!("artwork {src_path} missing on the source")),
                            _ => {}
                        }
                        images.insert(art, id);
                        id
                    }
                },
                _ => None,
            };
            let own_row = match (self.src_plus, &src_plus_ids) {
                (Some(src), Some(ids)) => match ids.get(&fold(&paths::key(&st.file_path))) {
                    // A row a source database made by something other than rekordbox can't give
                    // (it lacks a table) is built from export.pdb's, as planning expected.
                    Some(id) => src.export_track(*id).unwrap_or_else(|e| {
                        warnings.warn(
                            "unreadable source OneLibrary rows",
                            format!("{}: {e}; its OneLibrary row is made from export.pdb's", st.display_name()),
                        );
                        None
                    }),
                    None => None,
                },
                _ => None,
            };
            let made = own_row.is_none();
            let mut rec =
                own_row.unwrap_or_else(|| ContentRecord::from_track(st, !t.analyze_path.is_empty(), &defaults));
            // My Tags are added below, from whichever source database has them.
            rec.my_tags.clear();
            let analyze = if t.analyze_path.is_empty() { String::new() } else { paths::device_path(&t.analyze_path) };
            let (id, cues) = one.add_track_with_cues(&rec, &paths::device_path(&t.audio), &analyze, image)?;
            cue_ids.extend(cues);
            // A row built from export.pdb's has no cues: players reading OneLibrary would show none.
            if made && crate::cue_edit::add_rows_from_analysis(one, id, self.source, st)?.is_none() {
                warnings.warn(
                    "unreadable analysis files",
                    format!("{}: its analysis files can't be read, so it has no cues in OneLibrary", st.display_name()),
                );
            }
            id_map.insert(t.source_id, id);
            let tags = self.my_tags.and_then(|m| m.get(&t.source_id)).cloned().unwrap_or_default();
            wanted.push((id, fold(&t.audio), tags));
            if self.primary {
                report.tracks_added += 1;
            }
        }
        let (created, entries) = write_playlists(one, self.src_lib, self.plus_lib, opts, &plan.folder_name, &id_map)?;
        let history = write_history(one, self.src_lib, &plan.history_ids, &id_map)?;
        if self.primary {
            report.playlists_created = created;
            report.playlist_entries_added = entries;
            report.history_playlists_added = history;
        }
        if let Some(src) = self.src_plus.filter(|_| !cue_ids.is_empty() && opts.playlists != PlaylistMode::None) {
            report.hot_cue_bank_lists_added = copy_hot_cue_bank_lists(src, one, &plan.folder_name, &cue_ids)?;
        }
        let carried = match self.my_tags {
            Some(_) => {
                let fresh: Vec<u32> = wanted.iter().map(|(id, _, _)| *id).collect();
                tags::carry_into_plus(one, &one.library()?, &fresh, &wanted, self.ext_tags)?
            }
            None => Carried::default(),
        };
        let bytes = one.to_encrypted()?;
        // Never write a database we can't read back.
        let check = OneLibrary::from_encrypted(&bytes, None)?.library()?;
        if check.tracks.len() != self.plus_lib.tracks.len() + plan.new_tracks.len() {
            return Err(Error::OneLibrary("merged database failed verification; drive left unchanged".into()));
        }
        Ok((bytes, carried))
    }
}

/// Plan copying a source artwork (thumbnail + `_m` variant) as OneLibrary's `b<id>` images
/// (source path, target key) and add its row. `None` if the source files are missing.
fn copy_image(
    one: &mut OneLibrary,
    src_tree: &FileTree,
    src_path: &str,
    target: &Device,
    copies: &mut Vec<(PathBuf, String)>,
) -> Result<Option<u32>> {
    let Some(src_key) = src_tree.resolve(&paths::key(src_path)).map(|e| e.path.clone()) else {
        return Ok(None);
    };
    let ext = src_key.rsplit_once('.').map(|(_, e)| e).unwrap_or("jpg");
    let key = crate::artwork::free_image_key(target.root(), 'b', one.next_image_id()?, ext);
    for (from, to) in [
        (src_key.clone(), key.clone()),
        (crate::artwork::medium_variant(&src_key), crate::artwork::medium_variant(&key)),
    ] {
        if let Some(e) = src_tree.resolve(&from) {
            target.checked_path(&to)?;
            copies.push((src_tree.fs_path(&e.path), to));
        }
    }
    Ok(Some(one.add_image(&paths::device_path(&key))?))
}

fn new_track_row(st: &Track, t: &PlannedTrack, artwork_id: u32) -> NewTrack {
    let analyze = if t.analyze_path.is_empty() { String::new() } else { paths::device_path(&t.analyze_path) };
    new_track(st, &paths::device_path(&t.audio), &analyze, artwork_id)
}

/// An `export.pdb` row for `st` at device paths `file_path` / `analyze_path`.
pub(crate) fn new_track(st: &Track, file_path: &str, analyze_path: &str, artwork_id: u32) -> NewTrack {
    let opt = |s: &str| (!s.is_empty()).then(|| s.to_string());
    NewTrack {
        title: st.title.clone(),
        file_path: file_path.to_string(),
        filename: None,
        artist: opt(&st.artist),
        album: opt(&st.album),
        genre: opt(&st.genre),
        key: opt(&st.key),
        label: opt(&st.label),
        comment: st.comment.clone(),
        mix_name: st.mix_name.clone(),
        release_date: st.release_date.clone(),
        analyze_path: analyze_path.to_string(),
        date_added: st.date_added.clone(),
        tempo: (st.bpm * 100.0).round() as u32,
        duration: st.duration_secs.min(u16::MAX as u32) as u16,
        year: st.year,
        bitrate: st.bitrate,
        sample_rate: st.sample_rate,
        sample_depth: st.sample_depth,
        file_size: st.file_size.min(u32::MAX as u64) as u32,
        track_number: st.track_number,
        disc_number: st.disc_number,
        play_count: st.play_count,
        rating: st.rating,
        color_id: st.color.map_or(0, |c| c.id()),
        artwork_id,
    }
}

/// Plan copying a source artwork (thumbnail + `_m` variant; source path, target key) and add its
/// row. Returns the new id, or `None` if the source files are missing.
fn copy_artwork(
    editor: &mut PdbEditor,
    src_tree: &FileTree,
    src_path: &str,
    target: &Device,
    copies: &mut Vec<(PathBuf, String)>,
) -> Result<Option<u32>> {
    let Some(src_key) = src_tree.resolve(&paths::key(src_path)).map(|e| e.path.clone()) else {
        return Ok(None);
    };
    let next_id = editor.database()?.artwork.iter().map(|a| a.id).max().unwrap_or(0) + 1;
    let ext = src_key.rsplit_once('.').map(|(_, e)| e).unwrap_or("jpg");
    let key = crate::artwork::free_artwork_key(target.root(), next_id, ext);
    for (from, to) in [
        (src_key.clone(), key.clone()),
        (crate::artwork::medium_variant(&src_key), crate::artwork::medium_variant(&key)),
    ] {
        if let Some(e) = src_tree.resolve(&from) {
            target.checked_path(&to)?;
            copies.push((src_tree.fs_path(&e.path), to));
        }
    }
    let id = editor.add_artwork(&paths::device_path(&key))?;
    debug_assert_eq!(id, next_id);
    Ok(Some(id))
}

/// What recreating playlists and history needs from a database.
pub(crate) trait PlaylistDb {
    fn add_playlist(&mut self, name: &str, parent: u32, is_folder: bool) -> Result<u32>;
    fn add_entry(&mut self, playlist: u32, track: u32) -> Result<()>;
    fn add_history(&mut self, name: &str) -> Result<u32>;
    fn add_history_entry(&mut self, history: u32, track: u32) -> Result<()>;
}

impl PlaylistDb for PdbEditor {
    fn add_playlist(&mut self, name: &str, parent: u32, is_folder: bool) -> Result<u32> {
        Ok(self.create_playlist(name, parent, is_folder)?)
    }

    fn add_entry(&mut self, playlist: u32, track: u32) -> Result<()> {
        Ok(self.add_to_playlist(playlist, track, None)?)
    }

    fn add_history(&mut self, name: &str) -> Result<u32> {
        Ok(self.create_history_playlist(name)?)
    }

    fn add_history_entry(&mut self, history: u32, track: u32) -> Result<()> {
        Ok(self.add_to_history(history, track)?)
    }
}

impl PlaylistDb for OneLibrary {
    fn add_playlist(&mut self, name: &str, parent: u32, is_folder: bool) -> Result<u32> {
        self.create_playlist(name, parent, is_folder)
    }

    fn add_entry(&mut self, playlist: u32, track: u32) -> Result<()> {
        self.add_to_playlist(playlist, track)
    }

    fn add_history(&mut self, name: &str) -> Result<u32> {
        self.create_history(name, 0, false)
    }

    fn add_history_entry(&mut self, history: u32, track: u32) -> Result<()> {
        self.add_to_history(history, track)
    }
}

/// A library in memory, as [`crate::convert::combined_library`] adds to it.
impl PlaylistDb for Library {
    fn add_playlist(&mut self, name: &str, parent: u32, is_folder: bool) -> Result<u32> {
        let parent_id = (parent != 0).then_some(parent);
        let (id, sort_order) = next_node(&self.playlists, parent_id);
        self.playlists.push(Playlist {
            id,
            parent_id,
            name: name.into(),
            is_folder,
            sort_order,
            track_ids: Vec::new(),
        });
        Ok(id)
    }

    fn add_entry(&mut self, playlist: u32, track: u32) -> Result<()> {
        append(&mut self.playlists, playlist, track)
    }

    fn add_history(&mut self, name: &str) -> Result<u32> {
        let (id, sort_order) = next_node(&self.history, None);
        let list =
            Playlist { id, parent_id: None, name: name.into(), is_folder: false, sort_order, track_ids: Vec::new() };
        self.history.push(list);
        self.history_playlists += 1;
        Ok(id)
    }

    fn add_history_entry(&mut self, history: u32, track: u32) -> Result<()> {
        append(&mut self.history, history, track)
    }
}

/// A free id among `nodes`, and the place after the last child of `parent` (where a node whose
/// folder is missing counts as at the top, as `Library::children` shows it).
fn next_node(nodes: &[Playlist], parent: Option<u32>) -> (u32, u32) {
    let id = nodes.iter().map(|p| p.id).max().unwrap_or(0) + 1;
    let placed = |p: Option<u32>| p.filter(|&p| nodes.iter().any(|x| x.id == p));
    let place = nodes.iter().filter(|p| placed(p.parent_id) == parent).map(|p| p.sort_order + 1).max().unwrap_or(0);
    (id, place)
}

fn append(nodes: &mut [Playlist], list: u32, track: u32) -> Result<()> {
    let list = nodes.iter_mut().find(|p| p.id == list).ok_or_else(|| Error::NotFound(format!("list {list}")))?;
    list.track_ids.push(track);
    Ok(())
}

/// Add `src`'s history playlists `ids` (at the top level). Returns how many.
pub(crate) fn write_history(
    db: &mut impl PlaylistDb,
    src: &Library,
    ids: &[u32],
    id_map: &FxHashMap<u32, u32>,
) -> Result<usize> {
    for h in ids.iter().filter_map(|id| src.history.iter().find(|h| h.id == *id)) {
        let id = db.add_history(&h.name)?;
        for t in h.track_ids.iter().filter_map(|t| id_map.get(t)) {
            db.add_history_entry(id, *t)?;
        }
    }
    Ok(ids.len())
}

/// Recreate the source's hot cue bank lists that use the copied cues (`cue_ids`: source ->
/// target), inside a top-level folder named `folder`. Returns how many lists were added.
fn copy_hot_cue_bank_lists(
    src: &OneLibrary,
    dst: &mut OneLibrary,
    folder: &str,
    cue_ids: &FxHashMap<i64, i64>,
) -> Result<usize> {
    let lists = src.hot_cue_bank_lists()?;
    let uses_copied = |l: &crate::Playlist| l.track_ids.iter().any(|c| cue_ids.contains_key(&i64::from(*c)));
    // Lists with copied cues, and the folders above them.
    let mut keep: FxHashSet<u32> = FxHashSet::default();
    for l in lists.iter().filter(|l| !l.is_folder && uses_copied(l)) {
        let mut cur = Some(l);
        while let Some(n) = cur.filter(|n| keep.insert(n.id)) {
            cur = n.parent_id.and_then(|p| lists.iter().find(|x| x.id == p));
        }
    }
    if keep.is_empty() {
        return Ok(0);
    }
    let root = dst.create_hot_cue_bank_list(folder, 0, true)?;
    let mut new_ids: FxHashMap<u32, u32> = FxHashMap::default();
    let mut added = 0;
    // A list whose folder is missing is shown at the top, as `Library::children` places it.
    let placed = |p: Option<u32>| p.filter(|&p| lists.iter().any(|x| x.id == p));
    let mut queue = vec![None];
    while let Some(parent) = queue.pop() {
        let mut children: Vec<&crate::Playlist> =
            lists.iter().filter(|l| placed(l.parent_id) == parent && keep.contains(&l.id)).collect();
        children.sort_by_key(|l| (l.sort_order, l.id));
        for l in children {
            let dst_parent = l.parent_id.and_then(|p| new_ids.get(&p).copied()).unwrap_or(root);
            let id = dst.create_hot_cue_bank_list(&l.name, dst_parent, l.is_folder)?;
            new_ids.insert(l.id, id);
            if l.is_folder {
                queue.push(Some(l.id));
            } else {
                added += 1;
                for cue in l.track_ids.iter().filter_map(|c| cue_ids.get(&i64::from(*c))) {
                    dst.add_to_hot_cue_bank_list(id, *cue)?;
                }
            }
        }
    }
    Ok(added)
}

/// Recreate the source's playlists in `db`, whose library is `dst`, as `opts` says (in a folder named
/// `folder_name`, or by name). `id_map` maps source track ids to `db`'s.
pub(crate) fn write_playlists(
    editor: &mut impl PlaylistDb,
    src: &Library,
    dst: &Library,
    opts: &MergeOptions,
    folder_name: &str,
    id_map: &FxHashMap<u32, u32>,
) -> Result<(usize, usize)> {
    let selected = selected_playlists(src, opts)?;
    let mut created = 0;
    let mut entries = 0;
    let mut add_entries =
        |editor: &mut dyn PlaylistDb, playlist: u32, tracks: &[u32], skip: &FxHashSet<u32>| -> Result<()> {
            let mut seen = skip.clone();
            for sid in tracks {
                if let Some(&tid) = id_map.get(sid)
                    && seen.insert(tid)
                {
                    editor.add_entry(playlist, tid)?;
                    entries += 1;
                }
            }
            Ok(())
        };
    match opts.playlists {
        PlaylistMode::None => {}
        PlaylistMode::Folder => {
            let root_folder = editor.add_playlist(folder_name, 0, true)?;
            created += 1;
            // Recreate the selected part of the tree, parents before children.
            let mut new_ids: FxHashMap<u32, u32> = FxHashMap::default();
            let mut queue: Vec<Option<u32>> = vec![None];
            while let Some(parent) = queue.pop() {
                for p in src.children(parent) {
                    if !selected.contains(&p.id) {
                        // Selected descendants of an unselected folder go to the merge folder.
                        queue.push(Some(p.id));
                        continue;
                    }
                    let dst_parent = p.parent_id.and_then(|pid| new_ids.get(&pid).copied()).unwrap_or(root_folder);
                    let id = editor.add_playlist(&p.name, dst_parent, p.is_folder)?;
                    created += 1;
                    new_ids.insert(p.id, id);
                    if p.is_folder {
                        queue.push(Some(p.id));
                    } else {
                        add_entries(editor, id, &p.track_ids, &FxHashSet::default())?;
                    }
                }
            }
        }
        PlaylistMode::ByName => {
            // Folder path -> id of the folders we create.
            let mut made: FxHashMap<String, u32> = FxHashMap::default();
            for p in src.playlists.iter().filter(|p| selected.contains(&p.id) && !p.is_folder) {
                let path = src.playlist_path(p.id);
                let existing = merge_target(dst, &path, false);
                let (id, already) = match existing {
                    Some(d) => (d.id, d.track_ids.iter().copied().collect()),
                    None => {
                        let mut parent = 0;
                        let parts: Vec<&str> = path.split('/').collect();
                        for i in 0..parts.len() - 1 {
                            let folder_path = parts[..=i].join("/");
                            let found = merge_target(dst, &folder_path, true).map(|d| d.id);
                            parent = match found.or_else(|| made.get(&folder_path).copied()) {
                                Some(id) => id,
                                None => {
                                    let id = editor.add_playlist(parts[i], parent, true)?;
                                    created += 1;
                                    made.insert(folder_path, id);
                                    id
                                }
                            };
                        }
                        created += 1;
                        (editor.add_playlist(&p.name, parent, false)?, FxHashSet::default())
                    }
                };
                add_entries(editor, id, &p.track_ids, &already)?;
            }
        }
    }
    Ok((created, entries))
}

#[cfg(test)]
mod tests;
