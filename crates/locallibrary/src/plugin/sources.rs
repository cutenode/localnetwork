//! Plugins' library sources (`library/read`, the `sources` feature): a library a plugin reads
//! from somewhere on this computer, written to a drive as rekordbox's collection is (see
//! [`crate::collection_export`]). The plugin is given the folders the user grants it for the read
//! (`fs:read`) and answers pages of tracks and playlists until one has no `next`. None of its
//! paths is taken on its word: a track's audio file must be, its links resolved, a file in one
//! of those folders (not on the drive written to) that players play, which the host opens to read
//! its format and length itself; it's hashed before it's copied, and the copy checked against
//! that. A track that isn't is left out with a warning, and so is a playlist's entry for one. Cues,
//! beat grids and waveforms a track brings go into analysis files made for it, each read back
//! first; what can't be is left out with a warning.

use std::io::Read;
use std::path::{Path, PathBuf};

use indexmap::IndexMap;
use loclib_model::audio::Format;
use loclib_plugin::{Context, LibraryPage, ReadLibrary, SourceCue, SourcePlaylist, SourceTrack, method};
use rustc_hash::{FxHashMap, FxHashSet};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;

use super::Plugins;
use super::blobs::Scratch;
use crate::analysis_edit::NewAnalysis;
use crate::anlz::{self, Bands, CuePoint, DEFAULT_HOT_CUE_RGB};
use crate::collection_export::{self, Checked, Collected, CollectionExportOptions, CollectionExportPlan};
use crate::device::without_verbatim_prefix;
use crate::harmony::Camelot;
use crate::masterdb::{TagCategory, TagEntry};
use crate::model::{Library, Playlist, Track};
use crate::op::Planned;
use crate::progress::{Ctx, Phase};
use crate::store::Store;
use crate::{Error, Result, api, cue_edit, probe};

/// Building a drive from a plugin's library source.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct SourceExport {
    pub plugin: String,
    /// One of its library sources, by name.
    pub source: String,
    /// As the source's `options` schema says.
    #[serde(default)]
    pub options: Value,
    /// The folders the plugin may read files in, absolute: granted for this read alone, and only
    /// audio files in them are copied.
    pub grants: Vec<PathBuf>,
    /// The drive or folder to write to.
    pub drive: String,
    #[serde(default, flatten)]
    pub export: CollectionExportOptions,
}

/// The most examples a warning about what was left out gives.
const EXAMPLES: usize = 3;

/// What was left out of what a source read, by why: how many, and the first few.
#[derive(Default)]
struct LeftOut(IndexMap<&'static str, (usize, Vec<String>)>);

impl LeftOut {
    fn add(&mut self, what: &'static str, example: String) {
        let (n, examples) = self.0.entry(what).or_default();
        *n += 1;
        if examples.len() < EXAMPLES {
            examples.push(example);
        }
    }

    /// A warning for each kind, in the order they first came.
    fn warnings(self) -> Vec<String> {
        let line = |(what, (n, examples)): (&str, (usize, Vec<String>))| {
            format!("{n} {what} left out, e.g. {}", examples.join("; "))
        };
        self.0.into_iter().map(line).collect()
    }
}

/// A track as a source answered it, with its waveform's bands read from its request's scratch
/// folder (or why they couldn't be).
struct Answered {
    track: SourceTrack,
    bands: Option<std::result::Result<Bands, String>>,
}

impl Plugins {
    /// Plans writing a plugin's library source to the drive or folder `req.drive` names, as
    /// rekordbox's collection is written (`collection export`): the plugin reads it from
    /// `req.grants`, a page at a time, and the plan (operation `<plugin>.source.<source>`) says
    /// which folders it was granted, and what was left out.
    pub fn plan_source_export(
        &self,
        req: &SourceExport,
        store: Option<&Store>,
        ctx: &Ctx<'_>,
    ) -> Result<Planned<CollectionExportPlan, CollectionExportOptions>> {
        let slot = self.slot(&req.plugin)?;
        let declared = slot.declared("library source", &req.source, &slot.manifest.contributes.sources, |s| &s.name)?;
        let roots = granted(&req.grants)?;
        let (context, _) = slot.context(Context::default(), store)?;
        let options = super::fitting(declared.options.as_ref(), req.options.clone(), &context)?;
        let source = format!("{}:{}", req.plugin, req.source);
        let mut read = ReadLibrary { source: req.source.clone(), options, roots: roots.clone(), cursor: None, context };
        let operation = format!("{}.source.{}", req.plugin, req.source);
        let mut planned = api::export_planned(operation, &req.drive, &req.export, |d| {
            let drive = without_verbatim_prefix(d.root().canonicalize().unwrap_or_else(|_| d.root().to_path_buf()));
            if let Some(root) = roots.iter().find(|r| r.starts_with(&drive)) {
                return Err(Error::invalid(format!(
                    "{} is on {}, the drive written to: grant a folder elsewhere",
                    root.display(),
                    d.name()
                )));
            }
            let mut answered = Vec::new();
            let mut playlists = Vec::new();
            let mut cursors = FxHashSet::default();
            loop {
                ctx.check()?;
                let page = slot.call_with(method::READ_LIBRARY, &read, store, ctx, &|_| {}, |page, scratch, _| {
                    slot.page(page, scratch)
                })?;
                answered.extend(page.0);
                playlists.extend(page.1);
                read.cursor = match page.2 {
                    None => break,
                    Some(next) if !cursors.insert(next.clone()) => {
                        return Err(slot.misbehaved(format!("it answered the page after {next:?} twice")));
                    }
                    next => next,
                };
            }
            let (collected, left_out) = collect(&source, answered, playlists, &roots, &drive, ctx)?;
            let mut plan = collection_export::plan_from(&collected, d, &req.export, ctx)?;
            plan.warnings.extend(left_out.warnings());
            Ok(plan)
        })?;
        let folders: Vec<String> = roots.iter().map(|r| r.display().to_string()).collect();
        planned.summary.insert(1, format!("read the files {} finds in {}", declared.title, folders.join(", ")));
        planned.guard(None)
    }
}

impl super::process::Slot {
    /// A page as a source answered it: its tracks (with their waveforms) and playlists, those its
    /// blob holds after them, and its `next`.
    fn page(
        &self,
        mut page: LibraryPage,
        scratch: &Scratch,
    ) -> Result<(Vec<Answered>, Vec<SourcePlaylist>, Option<String>)> {
        if let Some(name) = page.blob.take() {
            let mut text = String::new();
            let read = scratch.blob(&name).and_then(|mut f| f.read_to_string(&mut text).map_err(|e| e.to_string()));
            read.map_err(|why| self.misbehaved(format!("its page's blob: {why}")))?;
            let more: LibraryPage = serde_json::from_str(&text)
                .map_err(|e| self.misbehaved(format!("its page's blob isn't a page: {e}")))?;
            page.tracks.extend(more.tracks);
            page.playlists.extend(more.playlists);
        }
        let answered = page
            .tracks
            .into_iter()
            .map(|track| {
                let bands = track.waveform.as_ref().map(|w| {
                    let mut bytes = Vec::new();
                    scratch.blob(&w.blob).and_then(|mut f| f.read_to_end(&mut bytes).map_err(|e| e.to_string()))?;
                    let bands = Bands::unpack(&bytes).filter(|b| !b.0.is_empty());
                    let bands = bands.ok_or("its waveform isn't three bytes a column, at least one column")?;
                    match w.columns_per_sec.is_finite() && w.columns_per_sec > 0.0 {
                        true => Ok(anlz::at_detail_rate(&bands, w.columns_per_sec)),
                        false => {
                            Err(format!("its waveform's columns a second, {}, aren't more than 0", w.columns_per_sec))
                        }
                    }
                });
                Answered { track, bands }
            })
            .collect();
        Ok((answered, page.playlists, page.next))
    }
}

/// The folders granted, each absolute and there, as they are with their links resolved (each
/// once).
fn granted(grants: &[PathBuf]) -> Result<Vec<PathBuf>> {
    if grants.is_empty() {
        return Err(Error::invalid("grant the plugin a folder to read its library's files in"));
    }
    let mut roots: Vec<PathBuf> = Vec::new();
    for g in grants {
        if !g.is_absolute() {
            return Err(Error::invalid(format!("a folder granted must be an absolute path, not {}", g.display())));
        }
        let real = g.canonicalize().map(without_verbatim_prefix).ok().filter(|r| r.is_dir());
        let real = real.ok_or_else(|| Error::NotFound(format!("the folder {}", g.display())))?;
        if !roots.contains(&real) {
            roots.push(real);
        }
    }
    Ok(roots)
}

/// `path` (as a plugin gave it) as it is with its links resolved, once it's an audio file players
/// play in one of `roots` and not below `drive`, with its size and modification time and its
/// format, or why it isn't.
fn checked(
    path: &Path,
    roots: &[PathBuf],
    drive: &Path,
) -> std::result::Result<(PathBuf, u64, i64, probe::Format), &'static str> {
    if !path.is_absolute() {
        return Err("isn't an absolute path");
    }
    let real = path.canonicalize().map(without_verbatim_prefix).map_err(|_| "isn't there")?;
    if !roots.iter().any(|r| real.starts_with(r)) {
        return Err("isn't in a folder granted (its links resolved)");
    }
    if real.starts_with(drive) {
        return Err("is on the drive written to");
    }
    let meta = std::fs::metadata(&real).map_err(|_| "can't be read")?;
    let name = real.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    if !meta.is_file() || Format::for_players(&name).is_none() {
        return Err("isn't an audio file players play");
    }
    if meta.len() == 0 {
        return Err("is empty");
    }
    let format = probe::format(&real).map_err(|_| "can't be read as audio")?;
    Ok((real, meta.len(), crate::fsutil::mtime_secs(&meta), format))
}

/// What a source answered as a library to write: each track whose audio file is checked, its
/// playlists (with folders made for their paths) of those, and My Tags; and what was left out.
fn collect(
    source: &str,
    answered: Vec<Answered>,
    playlists: Vec<SourcePlaylist>,
    roots: &[PathBuf],
    drive: &Path,
    ctx: &Ctx<'_>,
) -> Result<(Collected<'static>, LeftOut)> {
    let found = probe::parallel(
        &answered,
        Phase::Scan,
        |a| (a.track.path.display().to_string(), 0),
        |a| checked(&a.track.path, roots, drive),
        ctx,
    )?;
    let mut left_out = LeftOut::default();
    let (mut tracks, mut files) = (Vec::new(), FxHashMap::default());
    // Each track's id, by its path as the source gives it, and those taken by their files.
    let (mut ids, mut real_paths) = (FxHashMap::default(), FxHashSet::default());
    let mut tags: IndexMap<String, IndexMap<String, Vec<u32>>> = IndexMap::new();
    for (a, found) in answered.into_iter().zip(found) {
        let shown = a.track.path.display().to_string();
        let (real, size, mtime, format) = match found {
            Ok(found) => found,
            Err(why) => {
                left_out.add("track(s)", format!("{shown}: {why}"));
                continue;
            }
        };
        if !real_paths.insert(real.clone()) {
            left_out.add("track(s)", format!("{shown}: is another track's audio file too"));
            continue;
        }
        let id = tracks.len() as u32 + 1;
        ids.insert(a.track.path.clone(), id);
        for tag in &a.track.my_tags {
            match tag.split_once('/').filter(|(c, t)| !c.trim().is_empty() && !t.trim().is_empty()) {
                Some((category, tag)) => {
                    tags.entry(category.into()).or_default().entry(tag.into()).or_default().push(id)
                }
                None => left_out.add("My Tag(s)", format!("{shown}: {tag:?} isn't Category/Tag")),
            }
        }
        let (track, check) = made(id, a, &real, size, mtime, format, &mut left_out);
        tracks.push(track);
        files.insert(id, check);
    }
    let mut library = Library { tracks, ..Library::default() };
    for p in playlists {
        let id = folders(&mut library, &p.path);
        let (known, unknown): (Vec<_>, Vec<_>) = p.tracks.iter().partition(|t| ids.contains_key(*t));
        for t in unknown {
            left_out.add("playlist entr(ies)", format!("{} in {}: no track read", t.display(), p.path));
        }
        let playlist = library.playlists.iter_mut().find(|x| x.id == id).expect("just made");
        playlist.track_ids = known.into_iter().map(|t| ids[t]).collect();
    }
    let tags = tags
        .into_iter()
        .map(|(name, tags)| TagCategory {
            name,
            tags: tags.into_iter().map(|(name, track_ids)| TagEntry { name, track_ids }).collect(),
        })
        .collect();
    Ok((Collected::checked(source.into(), library, tags, files), left_out))
}

/// The track `id` a source answered (`a`), its audio file checked at `real`, and how its files are
/// written; what of it is left out goes in `left_out`.
fn made(
    id: u32,
    a: Answered,
    real: &Path,
    size: u64,
    mtime: i64,
    format: probe::Format,
    left_out: &mut LeftOut,
) -> (Track, Checked) {
    let t = a.track;
    let shown = t.path.display().to_string();
    let beats = match t.beats.as_slice() {
        [] => None,
        beats => anlz::checked_grid(beats).map_err(|e| left_out.add("beat grid(s)", format!("{shown}: {e}"))).ok(),
    };
    let waveform = a.bands.and_then(|b| b.map_err(|why| left_out.add("waveform(s)", format!("{shown}: {why}"))).ok());
    let cues = cues(&t.cues, &shown, left_out);
    let key = t.key.as_deref().filter(|k| !k.trim().is_empty());
    let key = key.and_then(|k| {
        let note = Camelot::parse(k).map(Camelot::note);
        note.ok_or_else(|| left_out.add("key(s)", format!("{shown}: {k:?} isn't a key"))).ok()
    });
    let bpm = t.bpm.filter(|b| b.is_finite() && *b > 0.0);
    let bpm = bpm.or_else(|| NewAnalysis { beats: beats.clone(), ..NewAnalysis::default() }.tempo());
    let secs = format.duration_secs.unwrap_or(0.0);
    let title = match t.title.trim() {
        "" => real.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default(),
        title => title.to_string(),
    };
    let track = Track {
        id,
        title,
        artist: t.artist,
        album: t.album,
        album_artist: t.album_artist,
        genre: t.genre,
        label: t.label,
        key: key.unwrap_or_default(),
        remixer: t.remixer,
        composer: t.composer,
        comment: t.comment,
        isrc: t.isrc,
        bpm: bpm.unwrap_or(0.0),
        duration_secs: secs.round() as u32,
        year: t.year,
        rating: t.rating.min(5),
        bitrate: if secs > 0.0 { (size as f64 * 8.0 / secs / 1000.0).round() as u32 } else { 0 },
        sample_rate: format.sample_rate,
        sample_depth: format.bit_depth,
        file_size: size,
        file_path: real.to_string_lossy().into_owned(),
        ..Track::default()
    };
    (track, Checked { size, mtime, cues, beats, waveform })
}

/// The cues a source gave a track (`shown`) as its analysis files hold them: a hot cue's slot
/// A-H, each slot once, a loop ending after it starts, colours as cue edits take them.
fn cues(given: &[SourceCue], shown: &str, left_out: &mut LeftOut) -> Vec<CuePoint> {
    let mut slots = FxHashSet::default();
    let mut out = Vec::new();
    for c in given {
        let at = format!("{shown} at {} ms", c.start_ms);
        let why = match c.hot_cue {
            Some(slot) if slot > 7 => Some("its hot cue isn't A-H"),
            Some(slot) if !slots.insert(slot) => Some("another hot cue has its slot"),
            _ if c.end_ms.is_some_and(|end| end <= c.start_ms) => Some("its loop doesn't end after it starts"),
            _ => None,
        };
        if let Some(why) = why {
            left_out.add("cue(s)", format!("{at}: {why}"));
            continue;
        }
        let rgb = if c.hot_cue.is_some() { DEFAULT_HOT_CUE_RGB } else { [0; 3] };
        let mut p = CuePoint {
            hot_cue: c.hot_cue,
            start_ms: c.start_ms,
            end_ms: c.end_ms,
            comment: c.name.clone(),
            color_rgb: rgb,
            ..CuePoint::default()
        };
        if let Some(Err(e)) = c.color.as_deref().map(|spec| cue_edit::set_color(&mut p, spec)) {
            left_out.add("cue colour(s)", format!("{at}: {e}"));
        }
        out.push(p);
    }
    out
}

/// The playlist `path` names (`Sets/Friday`) in `library`, made with the folders above it that
/// aren't there: its id. A playlist is always a new one; folders are shared.
fn folders(library: &mut Library, path: &str) -> u32 {
    let mut parent = None;
    let parts: Vec<&str> = path.split('/').filter(|p| !p.trim().is_empty()).collect();
    let (name, folders) = parts.split_last().map_or((path, &[][..]), |(n, f)| (*n, f));
    for folder in folders {
        let there = library.playlists.iter().find(|p| p.is_folder && p.parent_id == parent && p.name == *folder);
        parent = Some(match there {
            Some(p) => p.id,
            None => add(library, parent, folder, true),
        });
    }
    add(library, parent, name, false)
}

/// A new playlist (or folder) `name` in `parent`, last there: its id.
fn add(library: &mut Library, parent: Option<u32>, name: &str, is_folder: bool) -> u32 {
    let id = library.playlists.len() as u32 + 1;
    let sort_order = library.playlists.iter().filter(|p| p.parent_id == parent).count() as u32;
    let p = Playlist { id, parent_id: parent, name: name.into(), is_folder, sort_order, track_ids: Vec::new() };
    library.playlists.push(p);
    id
}
