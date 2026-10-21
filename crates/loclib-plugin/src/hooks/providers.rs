//! Feature `providers`: what a plugin finds for tracks (cover art, tags), which the host offers the
//! user and makes the edits of itself.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::manifest::{invalid, shown, unique_names};
use crate::sdk::declared;
use crate::{Blob, Capability, Context, Contributions, Feature, Hook, Manifest, ManifestError, RpcError};

/// Host to plugin (feature `providers`): [`ProvideCandidates`] -> [`Candidates`].
pub const PROVIDE: &str = "provide/candidates";

pub(crate) const FEATURE: Feature = Feature {
    name: "providers",
    methods: &[PROVIDE],
    sends: &[],
    contributed: |m| !m.contributes.providers.is_empty(),
};

/// `provide/candidates` (feature `providers`): what the provider has for these tracks, of its
/// kind. The host asks every provider of a kind, offers the user what they answer, and makes the
/// edits chosen itself: a provider never plans one.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ProvideCandidates {
    /// One of the manifest's providers.
    pub provider: String,
    /// Its kind, as the manifest declares it.
    pub kind: ProviderKind,
    pub tracks: Vec<TrackTags>,
    #[serde(default)]
    pub context: Context,
}

/// A track as a provider looks it up: by its tags, each left out when it has none.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TrackTags {
    pub id: u32,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub artist: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub title: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub album: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub album_artist: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub isrc: String,
    /// In seconds.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub duration: u32,
}

fn is_zero(n: &u32) -> bool {
    *n == 0
}

/// What `provide/candidates` answers: what it found for the tracks, any number for each (or
/// none), and why it found nothing for some, in a line each.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Candidates {
    pub candidates: Vec<Candidate>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

/// What a provider found for a track: an image (an artwork provider's), or tags (a metadata
/// provider's).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Candidate {
    /// The track's id, as it was asked for.
    pub track: u32,
    /// How sure the provider is that it's the track's, 0 to 1: the host picks one alone at the
    /// top for the user, when they let it.
    pub score: f64,
    /// Where it's from, in a few words (`Cover Art Archive`).
    pub source: String,
    /// What it is, in a line (`Discovery, album`).
    pub label: String,
    /// The image, a file in the request's scratch folder.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artwork: Option<Blob>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<TrackMetadata>,
}

/// The tags a metadata provider has for a track: each it doesn't have left out.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
pub struct TrackMetadata {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artist: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub album: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub year: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub genre: Option<String>,
    /// The record label.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub isrc: Option<String>,
}

/// For a provider of the kind asked for.
impl Hook for ProvideCandidates {
    const METHOD: &'static str = PROVIDE;
    type Answer = Candidates;

    fn declared(&self, c: &Contributions) -> Result<(), RpcError> {
        declared("provider", &self.provider, c.providers.iter().map(|x| &x.name))?;
        match c.providers.iter().any(|x| x.name == self.provider && x.kind == self.kind) {
            true => Ok(()),
            false => Err(RpcError::invalid_params(format!("{} provides no {}", self.provider, self.kind.name()))),
        }
    }
}

/// Asked with `provide/candidates` for what it has for tracks, of its kind: the host offers the
/// user every provider's, and makes the edits chosen itself. It's given the tracks' tags, which
/// takes `library:read`, and needs no capability to plan an edit.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Provider {
    pub name: String,
    pub title: String,
    pub kind: ProviderKind,
}

/// What a provider finds for a track.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ProviderKind {
    /// Cover art: an image.
    Artwork,
    /// Tags: a title, an artist, an album...
    Metadata,
}

impl ProviderKind {
    /// As a manifest names it (`artwork`).
    pub fn name(self) -> &'static str {
        match self {
            ProviderKind::Artwork => "artwork",
            ProviderKind::Metadata => "metadata",
        }
    }

    /// What it finds, as hosts say it to the user (`cover art`).
    pub fn found(self) -> &'static str {
        match self {
            ProviderKind::Artwork => "cover art",
            ProviderKind::Metadata => "track details",
        }
    }
}

/// `[[contributes.providers]]`: names and titles, and reading the tracks it's given.
pub(crate) fn validate(m: &Manifest) -> Result<(), ManifestError> {
    let (at, c) = ("contributes.providers", &m.contributes);
    unique_names(at, "name", c.providers.iter().map(|x| x.name.as_str()).enumerate())?;
    for (i, x) in c.providers.iter().enumerate() {
        shown(format!("{at}[{i}].title"), &x.title)?;
    }
    if !c.providers.is_empty() && !m.capabilities.contains(&Capability::LibraryRead) {
        let message = format!("is given tracks' tags, which needs the {} capability", Capability::LibraryRead);
        return Err(invalid(format!("{at}[0]"), message));
    }
    Ok(())
}
