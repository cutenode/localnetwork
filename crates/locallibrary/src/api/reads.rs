//! Reading a drive's library a piece at a time, for a GUI: pages of tracks, the playlist tree,
//! tracks by id, artwork as files or in batches, and analysis with compact waveforms. Each takes
//! a [`LibraryCache`], so a long-running caller parses a library once rather than per request.

use rustc_hash::FxHashMap;
use std::path::PathBuf;

use base64::prelude::*;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::{ArtworkImage, DriveRef, IdentifiedMix, IdentifyMixRequest, LibraryCache, ListCues};
use crate::anlz::{self, Beat, Cue, SongStructure, TrackAnalysis, Waveform, WaveformStyle};
use crate::artwork;
use crate::cue_edit::{self, TrackCues};
use crate::device::{Device, dunce_canonicalize, without_verbatim_prefix};
use crate::mix::export::{self, MixExport};
use crate::mix::index::Cache;
use crate::mix::report::{self, Drive};
use crate::mix::{self, Identifier};
use crate::model::{Library, LibrarySummary, Playlist, Track};
use crate::paths;
use crate::profile::ProfilePrefs;
use crate::progress::Ctx;
use crate::store::Store;
use crate::tags::MyTags;
use crate::{Error, Result, fsutil};

// ---------------------------------------------------------------------------------------------
// The library, in pieces
// ---------------------------------------------------------------------------------------------

/// `drive.library`, optionally paged: without `offset` or `limit` every track comes, as before.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct LibraryRequest {
    pub drive: String,
    #[serde(default)]
    pub offset: Option<usize>,
    #[serde(default)]
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct LibraryPage {
    pub summary: LibrarySummary,
    /// Its tracks are the page asked for (every track when unpaged).
    pub library: Library,
    /// Set when paged.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page: Option<Page>,
}

#[derive(Debug, Clone, Copy, Serialize, JsonSchema)]
pub struct Page {
    pub offset: usize,
    /// How many tracks the page holds.
    pub count: usize,
    /// How many tracks the library has.
    pub total: usize,
}

/// The drive's library ([`super::drive_library`]), its tracks paged when asked.
pub fn drive_library_cached(cache: &LibraryCache, req: &LibraryRequest) -> Result<LibraryPage> {
    let (_, cached) = cache.open(&req.drive)?;
    let lib = &cached.library;
    let summary = lib.summary();
    if req.offset.is_none() && req.limit.is_none() {
        return Ok(LibraryPage { summary, library: lib.clone(), page: None });
    }
    let offset = req.offset.unwrap_or(0);
    let tracks: Vec<Track> = lib.tracks.iter().skip(offset).take(req.limit.unwrap_or(usize::MAX)).cloned().collect();
    let page = Page { offset, count: tracks.len(), total: lib.tracks.len() };
    let library = Library {
        tracks,
        playlists: lib.playlists.clone(),
        history_playlists: lib.history_playlists,
        history: lib.history.clone(),
    };
    Ok(LibraryPage { summary, library, page: Some(page) })
}

/// A playlist or folder in [`PlaylistTree`].
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct PlaylistNode {
    pub id: u32,
    pub name: String,
    /// Through its folders, e.g. `Sets/2026/Warmup`.
    pub path: String,
    pub folder: bool,
    /// Entries in a playlist (a track can be in it twice); 0 for a folder.
    pub tracks: usize,
    /// The entries' total length.
    pub duration_secs: u64,
    /// A folder's contents, in display order.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<PlaylistNode>,
}

/// The playlist tree without the tracks: what a sidebar shows.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct PlaylistTree {
    pub playlists: Vec<PlaylistNode>,
    /// The history lists players recorded (in folders, on OneLibrary).
    pub history: Vec<PlaylistNode>,
}

fn tree(nodes: &[Playlist], durations: &FxHashMap<u32, u32>) -> Vec<PlaylistNode> {
    let mut children: FxHashMap<Option<u32>, Vec<&Playlist>> = FxHashMap::default();
    for p in nodes {
        children.entry(p.parent_id).or_default().push(p);
    }
    for v in children.values_mut() {
        v.sort_by_key(|p| (p.sort_order, p.id));
    }
    // A drive's tree, untangled as it was read (see `model::untangle`): every walk down ends.
    fn build(
        parent: Option<u32>,
        prefix: &str,
        children: &FxHashMap<Option<u32>, Vec<&Playlist>>,
        durations: &FxHashMap<u32, u32>,
    ) -> Vec<PlaylistNode> {
        let Some(nodes) = children.get(&parent) else { return Vec::new() };
        nodes
            .iter()
            .map(|p| {
                let path = if prefix.is_empty() { p.name.clone() } else { format!("{prefix}/{}", p.name) };
                PlaylistNode {
                    id: p.id,
                    name: p.name.clone(),
                    folder: p.is_folder,
                    tracks: p.track_ids.len(),
                    duration_secs: p.track_ids.iter().filter_map(|t| durations.get(t)).map(|d| u64::from(*d)).sum(),
                    children: if p.is_folder { build(Some(p.id), &path, children, durations) } else { Vec::new() },
                    path,
                }
            })
            .collect()
    }
    build(None, "", &children, durations)
}

/// The drive's playlists and history lists as a tree, without their tracks.
pub fn playlists_tree(cache: &LibraryCache, req: &DriveRef) -> Result<PlaylistTree> {
    let (_, cached) = cache.open(&req.drive)?;
    let lib = &cached.library;
    let durations: FxHashMap<u32, u32> = lib.tracks.iter().map(|t| (t.id, t.duration_secs)).collect();
    Ok(PlaylistTree { playlists: tree(&lib.playlists, &durations), history: tree(&lib.history, &durations) })
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct TracksGet {
    pub drive: String,
    pub ids: Vec<u32>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct TracksById {
    /// In the order asked for.
    pub tracks: Vec<Track>,
    /// Ids the library doesn't have.
    pub missing: Vec<u32>,
}

/// Tracks by id (a playlist's entries, say, from [`playlists_tree`] and `tracks.query`).
pub fn tracks_get(cache: &LibraryCache, req: &TracksGet) -> Result<TracksById> {
    let (_, cached) = cache.open(&req.drive)?;
    let by_id: FxHashMap<u32, &Track> = cached.library.tracks.iter().rev().map(|t| (t.id, t)).collect();
    let mut out = TracksById { tracks: Vec::new(), missing: Vec::new() };
    for id in &req.ids {
        match by_id.get(id) {
            Some(t) => out.tracks.push((*t).clone()),
            None => out.missing.push(*id),
        }
    }
    Ok(out)
}

/// [`super::cues_list`], with the library from `cache`.
pub fn cues_list_cached(cache: &LibraryCache, req: &ListCues) -> Result<Vec<TrackCues>> {
    let (d, cached) = cache.open(&req.drive)?;
    let ids = req.tracks.select_on(&d, &cached.library)?;
    cue_edit::list_in(&d, &cached.library, &ids)
}

/// [`super::tags_list`], kept with the library in `cache`.
pub fn tags_list_cached(cache: &LibraryCache, req: &DriveRef) -> Result<std::sync::Arc<MyTags>> {
    let (d, cached) = cache.open(&req.drive)?;
    cached.tags(&d)
}

// ---------------------------------------------------------------------------------------------
// Artwork
// ---------------------------------------------------------------------------------------------

/// One track (`track`) or several (`tracks`).
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum ArtworkRequest {
    One { drive: String, track: u32 },
    Many { drive: String, tracks: Vec<u32> },
}

/// A track's artwork in a batch: `image` is `None` when it has none.
#[derive(Debug, Clone, Serialize, JsonSchema)]
#[schemars(rename = "TrackArtwork_for_{T}")]
pub struct TrackArtwork<T> {
    pub track: u32,
    pub image: Option<T>,
}

/// One answer for `One`, a list in the order asked for `Many`.
#[derive(Debug, Clone, Serialize, JsonSchema)]
#[schemars(rename = "OneOrMany_for_{T}")]
#[serde(untagged)]
pub enum OneOrMany<T> {
    One(Option<T>),
    Many(Vec<TrackArtwork<T>>),
}

impl ArtworkRequest {
    fn drive(&self) -> &str {
        match self {
            ArtworkRequest::One { drive, .. } | ArtworkRequest::Many { drive, .. } => drive,
        }
    }

    fn each<T>(
        &self,
        cache: &LibraryCache,
        f: impl Fn(&Device, &Library, u32) -> Result<Option<T>>,
    ) -> Result<OneOrMany<T>> {
        let (d, cached) = cache.open(self.drive())?;
        match self {
            ArtworkRequest::One { track, .. } => Ok(OneOrMany::One(f(&d, &cached.library, *track)?)),
            ArtworkRequest::Many { tracks, .. } => tracks
                .iter()
                .map(|&track| Ok(TrackArtwork { track, image: f(&d, &cached.library, track)? }))
                .collect::<Result<_>>()
                .map(OneOrMany::Many),
        }
    }
}

/// Tracks' artwork as base64 JPEG previews ([`super::artwork_get`] for one or several).
pub fn artwork_get_cached(cache: &LibraryCache, req: &ArtworkRequest) -> Result<OneOrMany<ArtworkImage>> {
    req.each(cache, |d, lib, track| {
        let bytes = artwork::read_from_drive(d, lib, track)?;
        Ok(bytes.map(|b| ArtworkImage { mime: "image/jpeg", base64: BASE64_STANDARD.encode(&b) }))
    })
}

/// A track's artwork file, for a webview to load directly.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct ArtworkFile {
    /// Absolute, with symlinks resolved, and inside the drive.
    pub path: PathBuf,
    pub mime: &'static str,
}

/// Tracks' artwork files (the medium size players show when there is one): absolute paths,
/// checked to be inside the drive with no symlink leading out of it.
pub fn artwork_path(cache: &LibraryCache, req: &ArtworkRequest) -> Result<OneOrMany<ArtworkFile>> {
    req.each(cache, |d, lib, track| {
        let t = lib.track(track).ok_or_else(|| Error::NotFound(format!("track #{track}")))?;
        let Some(p) = &t.artwork_path else { return Ok(None) };
        let key = paths::key(p);
        for k in [artwork::medium_variant(&key), key] {
            let Ok(f) = d.checked_path(&k) else { continue };
            if !f.is_file() {
                continue;
            }
            // Resolved again here: `checked_path` vouches for where it led, and this is that. As
            // drive roots are, so a client gets the path it gave on Windows, not a `\\?\` one.
            let resolved =
                f.canonicalize().map(without_verbatim_prefix).map_err(|e| Error::Io { path: f.clone(), source: e })?;
            let root = dunce_canonicalize(d.root());
            if !resolved.starts_with(&root) {
                return Err(Error::invalid(format!("refusing {k:?}: it resolves outside {}", root.display())));
            }
            let ext = resolved.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
            let mime = if ext == "png" { "image/png" } else { "image/jpeg" };
            return Ok(Some(ArtworkFile { path: resolved, mime }));
        }
        Ok(None)
    })
}

// ---------------------------------------------------------------------------------------------
// Analysis
// ---------------------------------------------------------------------------------------------

/// Which waveforms `track.analysis` sends.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum WaveformChoice {
    None,
    /// The whole-track overview.
    Preview,
    /// The scrolling one, which is large: about 150 columns a second.
    Detail,
    #[default]
    All,
}

/// How waveforms are sent.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum WaveformEncoding {
    /// Each column as an object: the original shape, large (4-5 MB for a scrolling waveform).
    #[default]
    Json,
    /// Packed bytes in base64 ([`PackedWaveform`]), a few bytes a column.
    Base64,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct AnalysisRequest {
    pub drive: String,
    pub track: u32,
    #[serde(default)]
    pub waveform: WaveformChoice,
    #[serde(default)]
    pub encoding: WaveformEncoding,
}

/// One layer of a [`PackedWaveform`].
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct PackedLayer {
    /// One byte a column: its height, 0-255 of the waveform's full height.
    pub heights: String,
    /// Three bytes a column: its red, green and blue.
    pub colors: String,
}

/// A [`Waveform`] packed for sending: each layer's heights and colours as bytes in base64.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct PackedWaveform {
    /// Always `base64`; a [`Waveform`] has none.
    pub encoding: &'static str,
    pub style: WaveformStyle,
    pub columns_per_sec: Option<f64>,
    /// How many columns.
    pub columns: usize,
    pub back: PackedLayer,
    /// `None` when no column has a front layer (scrolling waveforms and blue ones).
    pub front: Option<PackedLayer>,
}

impl PackedWaveform {
    pub fn pack(w: &Waveform) -> PackedWaveform {
        let layer = |pick: fn(&crate::anlz::WaveColumn) -> &crate::anlz::WaveLayer| {
            let heights: Vec<u8> =
                w.columns.iter().map(|c| (pick(c).height.clamp(0.0, 1.0) * 255.0).round() as u8).collect();
            let colors: Vec<u8> = w.columns.iter().flat_map(|c| pick(c).color).collect();
            PackedLayer { heights: BASE64_STANDARD.encode(&heights), colors: BASE64_STANDARD.encode(&colors) }
        };
        let has_front = w.columns.iter().any(|c| c.front.height > 0.0);
        PackedWaveform {
            encoding: "base64",
            style: w.style,
            columns_per_sec: w.columns_per_sec,
            columns: w.columns.len(),
            back: layer(|c| &c.back),
            front: has_front.then(|| layer(|c| &c.front)),
        }
    }
}

/// A waveform as asked for.
#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum WaveformData {
    Json(Waveform),
    Packed(PackedWaveform),
}

/// [`TrackAnalysis`] with only the waveforms asked for, in the encoding asked for. With the
/// defaults it's the same as [`TrackAnalysis`].
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct AnalysisView {
    pub beats: Vec<Beat>,
    pub cues: Vec<Cue>,
    pub preview: Option<WaveformData>,
    pub detail: Option<WaveformData>,
    pub structure: Option<SongStructure>,
}

/// A track's beat grid, cues, waveforms and phrases ([`super::track_analysis`]), with the
/// waveforms chosen and encoded as asked.
pub fn track_analysis_cached(cache: &LibraryCache, req: &AnalysisRequest) -> Result<AnalysisView> {
    let (d, cached) = cache.open(&req.drive)?;
    let t = cached.library.track(req.track).ok_or_else(|| Error::NotFound(format!("track #{}", req.track)))?;
    let a: TrackAnalysis = anlz::load(&d, t);
    let encode = |w: Waveform| match req.encoding {
        WaveformEncoding::Json => WaveformData::Json(w),
        WaveformEncoding::Base64 => WaveformData::Packed(PackedWaveform::pack(&w)),
    };
    let (preview, detail) = match req.waveform {
        WaveformChoice::None => (false, false),
        WaveformChoice::Preview => (true, false),
        WaveformChoice::Detail => (false, true),
        WaveformChoice::All => (true, true),
    };
    Ok(AnalysisView {
        beats: a.beats,
        cues: a.cues,
        preview: a.preview.filter(|_| preview).map(encode),
        detail: a.detail.filter(|_| detail).map(encode),
        structure: a.structure,
    })
}

// ---------------------------------------------------------------------------------------------
// Mixes
// ---------------------------------------------------------------------------------------------

/// The drive's mix identifier, kept with its library in `cache`.
fn identifier(
    cache: &LibraryCache,
    store: &Store,
    drive: Option<&str>,
    ctx: &Ctx<'_>,
) -> Result<(Device, std::sync::Arc<Identifier>)> {
    let device = super::mix_drive(drive)?;
    let cached = cache.get(&device)?;
    let id = cached.identifier(store.root(), ctx, |library| {
        Identifier::with_library(&device, library.into(), &Cache::in_store(store), ctx)
    })?;
    Ok((device, id))
}

/// Which of a drive's tracks a recorded mix plays, when, at what speed and in what key, with the
/// drive's identifier kept in `cache`. A report that can't be kept (`save`) is still answered,
/// with a warning.
pub fn mix_identify_cached(
    cache: &LibraryCache,
    store: &Store,
    req: &IdentifyMixRequest,
    ctx: &Ctx<'_>,
) -> Result<IdentifiedMix> {
    let (device, id) = identifier(cache, store, req.identify.drive.as_deref(), ctx)?;
    let options = mix::Options { tempo_range: req.identify.tempo_range.unwrap_or(mix::DEFAULT_TEMPO_RANGE) };
    let report = id.identify(&req.identify.mix, &options, ctx)?;
    let mut saved = None;
    if req.save {
        match report::save(store, &report) {
            Ok(id) => saved = Some(id),
            Err(e) => ctx.warn(format!("the report wasn't kept: {e}")),
        }
    }
    Ok(IdentifiedMix { saved, ..IdentifiedMix::of(report, Some(Drive::new(&device, &id.library)), req.extras) })
}

/// Identify a mix and return the report as a publishable data document (see
/// [`crate::mix::export`]), with the drive's identifier kept in `cache`.
pub fn mix_export_cached(
    cache: &LibraryCache,
    store: &Store,
    req: &super::ExportMix,
    ctx: &Ctx<'_>,
) -> Result<MixExport> {
    // Refused before the long run, and checked again as it's written.
    fsutil::refuse_to_replace(req.output.as_deref().as_slice(), req.replace)?;
    let (device, id) = identifier(cache, store, req.identify.drive.as_deref(), ctx)?;
    let options = mix::Options { tempo_range: req.identify.tempo_range.unwrap_or(mix::DEFAULT_TEMPO_RANGE) };
    let report = id.identify(&req.identify.mix, &options, ctx)?;
    let doc = export::build(&report, Some(Drive::new(&device, &id.library)), &ProfilePrefs::load(store)?);
    if let Some(path) = &req.output {
        let json = serde_json::to_vec(&doc).map_err(|e| Error::invalid(e.to_string()))?;
        fsutil::write_output(path, &json, req.replace)?;
    }
    Ok(doc)
}
