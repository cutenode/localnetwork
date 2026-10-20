//! The host's reads of a drive, typed: [`Host::drive`] calls them with their params and reads
//! their results, the RPC sidecar's (see `apps/locallibrary/docs/rpc`), a page or a batch at a
//! time as the host bounds them.

use std::path::PathBuf;

use loclib_model::analysis::TrackAnalysis;
use loclib_model::model::LibrarySummary;
use loclib_model::{Library, Track};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{Host, RpcError};

/// The most tracks the host takes by id at once (`tracks.get`), which it holds a plugin to.
pub const MAX_IDS: usize = 1000;
/// The most tracks' analyses it gives at once (`tracks.analysis`): an analysis without its
/// scrolling waveform runs to tens of kilobytes.
pub const MAX_ANALYSES: usize = 100;
/// The most tracks' artwork it gives a plugin at once (`art.get`, base64 JPEGs).
pub const MAX_IMAGES: usize = 50;

impl Host {
    /// The reads of `drive` (its id or name), as the manifest's capabilities grant them.
    pub fn drive<'h>(&'h self, drive: &'h str) -> DriveReads<'h> {
        DriveReads { host: self, drive, combined: false }
    }
}

/// A drive's library, its tracks' analysis and artwork, and the drive itself, read through the
/// host ([`Host::drive`]). Each read is the library its players read, unless [`combined`].
///
/// [`combined`]: DriveReads::combined
#[derive(Clone, Copy)]
pub struct DriveReads<'h> {
    host: &'h Host,
    drive: &'h str,
    combined: bool,
}

impl<'h> DriveReads<'h> {
    /// The library as exports read it: on a drive with both databases, with the tracks, playlists
    /// and history only OneLibrary has (their ids 2^31 and up, which edits refuse).
    pub fn combined(self) -> DriveReads<'h> {
        DriveReads { combined: true, ..self }
    }

    /// `drive.info`.
    pub fn info(&self) -> Result<DriveInfo, RpcError> {
        self.host.call("drive.info", &json!({ "drive": self.drive }))
    }

    /// `drive.library`: every track, a page at a time.
    pub fn library(&self) -> Result<Library, RpcError> {
        let (tracks, last) = self.pages("drive.library", json!({}), |p: &mut LibraryPage| {
            (&mut p.library.tracks, p.page.as_ref().map(|p| p.total))
        })?;
        Ok(Library { tracks, ..last.library })
    }

    /// `playlists.tree`: the playlists and history lists, without their tracks.
    pub fn playlists(&self) -> Result<PlaylistTree, RpcError> {
        self.read("playlists.tree", json!({}))
    }

    /// `tracks.get`: the tracks with these ids, in their order, and the ids the library doesn't
    /// have.
    pub fn tracks(&self, ids: &[u32]) -> Result<TracksById, RpcError> {
        let mut all = TracksById::default();
        for ids in ids.chunks(MAX_IDS) {
            let mut got: TracksById = self.read("tracks.get", json!({ "ids": ids }))?;
            all.tracks.append(&mut got.tracks);
            all.missing.append(&mut got.missing);
        }
        Ok(all)
    }

    /// `tracks.query`: every track `query` matches, a page at a time.
    pub fn query(&self, query: &TracksQuery) -> Result<Vec<Track>, RpcError> {
        let params = serde_json::to_value(query).expect("a query is JSON");
        let (tracks, _) = self.pages("tracks.query", params, |p: &mut TracksPage| (&mut p.tracks, Some(p.total)))?;
        Ok(tracks)
    }

    /// `track.analysis`: the track's beat grid, cues and phrases, without its waveforms.
    pub fn analysis(&self, track: u32) -> Result<TrackAnalysis, RpcError> {
        self.read("track.analysis", json!({ "track": track, "waveform": "none" }))
    }

    /// `tracks.analysis`: [`DriveReads::analysis`] of each of these tracks, in their order, and
    /// the ids the library doesn't have.
    pub fn analyses(&self, ids: &[u32]) -> Result<TracksAnalysis, RpcError> {
        let mut all = TracksAnalysis::default();
        for ids in ids.chunks(MAX_ANALYSES) {
            let mut got: TracksAnalysis = self.read("tracks.analysis", json!({ "ids": ids, "waveform": "none" }))?;
            all.tracks.append(&mut got.tracks);
            all.missing.append(&mut got.missing);
        }
        Ok(all)
    }

    /// `art.get`: each of these tracks' artwork, in their order. An id the library doesn't have
    /// fails it.
    pub fn artwork(&self, tracks: &[u32]) -> Result<Vec<TrackArtwork>, RpcError> {
        let mut all = Vec::new();
        for tracks in tracks.chunks(MAX_IMAGES) {
            all.append(&mut self.read("art.get", json!({ "tracks": tracks }))?);
        }
        Ok(all)
    }

    /// The read `method` of this drive's library, with `params` besides the drive.
    fn read<R: DeserializeOwned>(&self, method: &str, mut params: Value) -> Result<R, RpcError> {
        params["drive"] = self.drive.into();
        if self.combined {
            params["combined"] = true.into();
        }
        self.host.call(method, &params)
    }

    /// Every track the paged read `method` has, a page at a time (of the size the host chooses)
    /// until one comes empty or its total is read, and the last page. `tracks` is a page's tracks
    /// and total.
    fn pages<P: DeserializeOwned>(
        &self,
        method: &str,
        mut params: Value,
        tracks: impl Fn(&mut P) -> (&mut Vec<Track>, Option<usize>),
    ) -> Result<(Vec<Track>, P), RpcError> {
        let mut all = Vec::new();
        loop {
            params["offset"] = all.len().into();
            let mut page: P = self.read(method, params.clone())?;
            let (got, total) = tracks(&mut page);
            let done = got.is_empty() || total.is_none_or(|total| all.len() + got.len() >= total);
            all.append(got);
            if done {
                return Ok((all, page));
            }
        }
    }
}

/// `drive.info`'s answer, but for what only locallibrary uses (the database's header, work
/// left pending, rekordbox's sync, how the databases differ).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DriveInfo {
    /// Where it's mounted.
    pub root: PathBuf,
    pub volume_name: String,
    /// The id locallibrary gave it, once adopted.
    pub id: Option<String>,
    /// The name it was adopted as.
    pub name: Option<String>,
    /// The library in locallibrary's store it's a copy of.
    pub library_id: Option<String>,
    /// `export.pdb` is there (the Device Library every player but the newest reads).
    pub has_device_library: bool,
    /// OneLibrary's `exportLibrary.db` is there (the only database the newest players read).
    pub has_library_plus: bool,
    /// When rekordbox last exported to it (RFC 3339).
    pub exported_at: Option<String>,
    pub summary: Option<LibrarySummary>,
    pub total_space: Option<u64>,
    pub free_space: Option<u64>,
}

/// `playlists.tree`'s answer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlaylistTree {
    pub playlists: Vec<PlaylistNode>,
    /// The history lists players recorded (in folders, on OneLibrary).
    pub history: Vec<PlaylistNode>,
}

/// A playlist or a folder in a [`PlaylistTree`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlaylistNode {
    pub id: u32,
    pub name: String,
    /// Through its folders (`Sets/2026/Warmup`).
    pub path: String,
    pub folder: bool,
    /// A playlist's entries (a track can be in it twice); 0 for a folder.
    pub tracks: usize,
    /// The entries' total length.
    pub duration_secs: u64,
    /// A folder's contents, in display order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<PlaylistNode>,
}

/// `tracks.get`'s answer.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TracksById {
    /// In the order asked for.
    pub tracks: Vec<Track>,
    /// Ids the library doesn't have.
    pub missing: Vec<u32>,
}

/// Which tracks [`DriveReads::query`] reads, as `tracks.query` takes them: every one, in the
/// library's order, unless it says.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct TracksQuery {
    /// What they match, as `locallibrary tracks --filter` takes it (`bpm:120-128 genre:house`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filter: Option<String>,
    /// Only the tracks in this playlist (by path or unique name), in its order.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub playlist: Option<String>,
    /// How they're sorted (`key,bpm`, `-rating`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sort: Option<String>,
}

/// `tracks.analysis`'s answer.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TracksAnalysis {
    /// In the order asked for.
    pub tracks: Vec<AnalysedTrack>,
    /// Ids the library doesn't have.
    pub missing: Vec<u32>,
}

/// A track's analysis in [`TracksAnalysis`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AnalysedTrack {
    pub track: u32,
    pub analysis: TrackAnalysis,
}

/// A track's artwork in `art.get`'s answer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TrackArtwork {
    pub track: u32,
    /// `None` when it has none.
    pub image: Option<ArtworkImage>,
}

/// A preview of a track's artwork.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ArtworkImage {
    /// `image/jpeg`.
    pub mime: String,
    pub base64: String,
}

/// `drive.library`'s answer, as far as the pages are read.
#[derive(Deserialize)]
struct LibraryPage {
    library: Library,
    page: Option<Page>,
}

#[derive(Deserialize)]
struct Page {
    total: usize,
}

/// `tracks.query`'s answer, as far as the pages are read.
#[derive(Deserialize)]
struct TracksPage {
    total: usize,
    tracks: Vec<Track>,
}

#[cfg(test)]
mod tests;
