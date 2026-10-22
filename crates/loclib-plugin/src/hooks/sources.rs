//! Feature `sources`: a library a plugin reads from somewhere on this computer (another DJ
//! program's collection, a folder of tagged files), which the host builds a drive from: its
//! tracks, each an audio file in a folder the user granted the plugin (`fs:read`), with their tags,
//! cues and beat grids, and its playlists.

use std::path::PathBuf;

use loclib_model::analysis::Beat;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::manifest::{invalid, object_schema, shown, unique_names};
use crate::sdk::declared;
use crate::{Capability, Context, Contributions, Feature, Hook, Manifest, ManifestError, RpcError, WaveformBlob};

/// Host to plugin: [`ReadLibrary`] -> [`LibraryPage`].
pub const READ_LIBRARY: &str = "library/read";

pub(crate) const FEATURE: Feature = Feature {
    name: "sources",
    methods: &[READ_LIBRARY],
    sends: &[],
    contributed: |m| !m.contributes.sources.is_empty(),
};

/// `library/read`: a page of the source's library, the first or the one after `cursor`. The host
/// asks for pages until one has no `next`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ReadLibrary {
    /// One of the manifest's sources.
    pub source: String,
    /// What the source's `options` schema describes.
    #[serde(default)]
    pub options: Value,
    /// The folders the user granted the plugin for this read (`fs:read`), absolute: each track's
    /// audio file is in one of them, or the host leaves the track out.
    pub roots: Vec<PathBuf>,
    /// The `next` of the page before; none for the first.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
    #[serde(default)]
    pub context: Context,
}

/// What `library/read` answers: some of the library's tracks and playlists, in any order across
/// pages (a playlist names its tracks by path, on this page or another).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct LibraryPage {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tracks: Vec<SourceTrack>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub playlists: Vec<SourcePlaylist>,
    /// A file in the request's scratch folder holding more of the page, a page as JSON (its own
    /// `blob` and `next` are ignored): for a page too big for a message.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blob: Option<String>,
    /// The cursor of the page after this one; none on the last.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next: Option<String>,
}

/// A track of a source's library: its audio file and what's known of it, each left out when
/// there's nothing. The host reads the file itself (its length, format and size).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SourceTrack {
    /// Its audio file, absolute, in one of the request's `roots`.
    pub path: PathBuf,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub title: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub artist: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub album: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub album_artist: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub genre: String,
    /// The record label.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub label: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub remixer: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub composer: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub comment: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub isrc: String,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub year: u16,
    /// 0 to 5 stars.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub rating: u8,
    /// A Camelot code (`8A`), or a key as rekordbox names it (`Am`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bpm: Option<f64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cues: Vec<SourceCue>,
    /// Its beat grid: each beat's number in its bar (1-4), the tempo there and its time.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub beats: Vec<Beat>,
    /// Its waveform, three bands in a file in the request's scratch folder (as an analyzer's
    /// `waveform` field gives one).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub waveform: Option<WaveformBlob>,
    /// Its My Tags, each `Category/Tag`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub my_tags: Vec<String>,
}

fn is_zero<N: Default + PartialEq>(n: &N) -> bool {
    *n == N::default()
}

/// A hot cue, a memory cue or a loop.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SourceCue {
    /// Its hot cue's slot, 0 for A to 7 for H; a memory cue without one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hot_cue: Option<u8>,
    pub start_ms: u32,
    /// Where a loop ends.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_ms: Option<u32>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    /// As cue edits take a colour: a name (`red`, `aqua`), `#rrggbb` (the nearest players have),
    /// or a number (a hot cue's palette code, 1-62; a memory cue's colour, 1 pink to 8 purple).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
}

/// A playlist of a source's library.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SourcePlaylist {
    /// Its folders and name, `/` between them (`Sets/Friday`): the folders are made for it.
    pub path: String,
    /// Its tracks in order, by their `path`.
    pub tracks: Vec<PathBuf>,
}

impl Hook for ReadLibrary {
    const METHOD: &'static str = READ_LIBRARY;
    type Answer = LibraryPage;

    fn declared(&self, c: &Contributions) -> Result<(), RpcError> {
        declared("source", &self.source, c.sources.iter().map(|x| &x.name))
    }
}

/// Read with `library/read`, a page at a time, for the host to build a drive from. It's given the
/// folders the user grants it, which takes `fs:read`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct LibrarySource {
    pub name: String,
    pub title: String,
    /// A JSON Schema for its options (an object).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub options: Option<Value>,
}

/// `[[contributes.sources]]`: names, titles and options, and reading the folders granted.
pub(crate) fn validate(m: &Manifest) -> Result<(), ManifestError> {
    let (at, c) = ("contributes.sources", &m.contributes);
    unique_names(at, "name", c.sources.iter().map(|x| x.name.as_str()).enumerate())?;
    for (i, x) in c.sources.iter().enumerate() {
        shown(format!("{at}[{i}].title"), &x.title)?;
        object_schema(format!("{at}[{i}].options"), x.options.as_ref())?;
    }
    if !c.sources.is_empty() && !m.capabilities.contains(&Capability::FsRead) {
        let message =
            format!("reads files in the folders it's granted, which needs the {} capability", Capability::FsRead);
        return Err(invalid(format!("{at}[0]"), message));
    }
    Ok(())
}
