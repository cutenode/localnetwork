//! Feature `destinations`: where a plugin keeps things for the user somewhere else. A store
//! destination keeps copies of locallibrary's store: its stored files by hash (BLAKE3, in hex),
//! and its records by name (`libraries.json`, `snapshots/<id>.json`...), each given as a file and
//! fetched into the request's scratch folder; the host decides what goes there, in what order,
//! and checks everything it fetches back. A set destination publishes a set's recording with its
//! tracklist, saying where it can be heard (and, when it keeps the recording as an Atmosphere
//! record, which one); the host keeps both.

use std::path::PathBuf;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::manifest::{shown, unique_names};
use crate::sdk::declared;
use crate::{Blob, Context, Contributions, Feature, Hook, Manifest, ManifestError, RpcError};

/// Host to plugin (feature `destinations`): [`HasBlobs`] -> [`Missing`].
pub const BLOBS_HAS: &str = "blobs/has";
/// Host to plugin (feature `destinations`): [`PutBlob`] -> [`Kept`].
pub const BLOBS_PUT: &str = "blobs/put";
/// Host to plugin (feature `destinations`): [`GetBlob`] -> [`Got`].
pub const BLOBS_GET: &str = "blobs/get";
/// Host to plugin (feature `destinations`): [`PutManifest`] -> [`Kept`].
pub const MANIFEST_PUT: &str = "manifest/put";
/// Host to plugin (feature `destinations`): [`GetManifest`] -> [`GotManifest`].
pub const MANIFEST_GET: &str = "manifest/get";
/// Host to plugin (feature `destinations`): [`PublishSet`] -> [`Published`].
pub const SET_PUBLISH: &str = "set/publish";

pub(crate) const FEATURE: Feature = Feature {
    name: "destinations",
    methods: &[BLOBS_HAS, BLOBS_PUT, BLOBS_GET, MANIFEST_PUT, MANIFEST_GET, SET_PUBLISH],
    sends: &[],
    contributed: |m| !m.contributes.destinations.is_empty(),
};

/// `[[contributes.destinations]]`: somewhere the user copies their store to (`store`), or
/// publishes sets at (`set`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Destination {
    pub name: String,
    pub title: String,
    pub kind: DestinationKind,
}

/// What a destination keeps.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DestinationKind {
    /// Copies of the store: `blobs/*` and `manifest/*`.
    Store,
    /// Sets, published: `set/publish`.
    Set,
}

impl DestinationKind {
    /// As a manifest names it (`store`).
    pub fn name(self) -> &'static str {
        match self {
            DestinationKind::Store => "store",
            DestinationKind::Set => "set",
        }
    }
}

/// `name` is one of the manifest's destinations, of `kind`.
fn of_kind(c: &Contributions, name: &str, kind: DestinationKind) -> Result<(), RpcError> {
    declared("destination", name, c.destinations.iter().map(|x| &x.name))?;
    match c.destinations.iter().any(|x| x.name == name && x.kind == kind) {
        true => Ok(()),
        false => Err(RpcError::invalid_params(format!("{name} isn't a {} destination", kind.name()))),
    }
}

/// `blobs/has`: which of these stored files the destination doesn't have.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct HasBlobs {
    /// One of the manifest's store destinations.
    pub destination: String,
    /// BLAKE3 hashes, in hex: a thousand at most.
    pub hashes: Vec<String>,
    #[serde(default)]
    pub context: Context,
}

/// What `blobs/has` answers: those of the hashes asked about it doesn't have.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Missing {
    pub missing: Vec<String>,
}

/// `blobs/put`: keep the stored file `path` (absolute, to read and never change), whose
/// content's BLAKE3 hash is `hash`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PutBlob {
    pub destination: String,
    pub hash: String,
    pub path: PathBuf,
    #[serde(default)]
    pub context: Context,
}

/// What `blobs/put` and `manifest/put` answer: nothing, once it's kept.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Kept {}

/// `blobs/get`: the stored file of this hash, written to the request's scratch folder. The host
/// hashes it and refuses one that isn't what was asked for.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct GetBlob {
    pub destination: String,
    pub hash: String,
    #[serde(default)]
    pub context: Context,
}

/// What `blobs/get` answers: the file, in the scratch folder. One it doesn't have fails with kind
/// `not_found`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Got {
    pub blob: Blob,
}

/// `manifest/put`: keep a record of the store's by name (`libraries.json`, `snapshots/<id>.json`),
/// replacing the one of that name. Its content is a file in the request's scratch folder.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PutManifest {
    pub destination: String,
    pub name: String,
    pub blob: Blob,
    #[serde(default)]
    pub context: Context,
}

/// `manifest/get`: the record of that name, written to the request's scratch folder.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct GetManifest {
    pub destination: String,
    pub name: String,
    #[serde(default)]
    pub context: Context,
}

/// What `manifest/get` answers: the file, in the scratch folder, or none when it has no record
/// of that name.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct GotManifest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blob: Option<Blob>,
}

/// `set/publish`: publish a set's recording with its tracklist, or with `dry_run` say what would
/// happen and do nothing. The host always does the dry run first and asks the user.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PublishSet {
    /// One of the manifest's set destinations.
    pub destination: String,
    pub set: PublishedSet,
    /// The set's recording on this computer (absolute): read it, never change it.
    pub recording: PathBuf,
    /// In play order.
    pub tracklist: Vec<TracklistEntry>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub dry_run: bool,
    #[serde(default)]
    pub context: Context,
}

/// A set as it's published: its details, never the DJ's notes.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PublishedSet {
    pub id: String,
    /// The day it was played, `2026-10-03`.
    pub date: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// `club`, `festival`, `radio`, `livestream`, `mix` or the DJ's own word.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub venue: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub city: Option<String>,
    /// ISO 3166, `GB`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub country: Option<String>,
}

/// A track of a published set: its artist and title left out when the DJ publishes it without
/// naming it.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TracklistEntry {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artist: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Seconds from the start of the recording, when it's known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_seconds: Option<u32>,
}

/// What `set/publish` answers: where the set can be heard (none for a dry run, which may say
/// where it would be), and what the user should know, in a line each.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Published {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// What `url` is, as the set's published record names its link: `plyr.fm`, `Mixcloud`. 64
    /// characters at most.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// The Atmosphere record holding the recording, when the destination keeps it as one (a
    /// plyr.fm track, `fm.plyr.track`): the set's published record points to it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub record: Option<StrongRef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

/// An Atmosphere record and the version of it meant, as `com.atproto.repo.strongRef` has them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct StrongRef {
    /// Its at:// URI.
    pub uri: String,
    /// Its CID.
    pub cid: String,
}

/// For a store destination.
impl Hook for HasBlobs {
    const METHOD: &'static str = BLOBS_HAS;
    type Answer = Missing;

    fn declared(&self, c: &Contributions) -> Result<(), RpcError> {
        of_kind(c, &self.destination, DestinationKind::Store)
    }
}

/// For a store destination.
impl Hook for PutBlob {
    const METHOD: &'static str = BLOBS_PUT;
    type Answer = Kept;

    fn declared(&self, c: &Contributions) -> Result<(), RpcError> {
        of_kind(c, &self.destination, DestinationKind::Store)
    }
}

/// For a store destination.
impl Hook for GetBlob {
    const METHOD: &'static str = BLOBS_GET;
    type Answer = Got;

    fn declared(&self, c: &Contributions) -> Result<(), RpcError> {
        of_kind(c, &self.destination, DestinationKind::Store)
    }
}

/// For a store destination.
impl Hook for PutManifest {
    const METHOD: &'static str = MANIFEST_PUT;
    type Answer = Kept;

    fn declared(&self, c: &Contributions) -> Result<(), RpcError> {
        of_kind(c, &self.destination, DestinationKind::Store)
    }
}

/// For a store destination.
impl Hook for GetManifest {
    const METHOD: &'static str = MANIFEST_GET;
    type Answer = GotManifest;

    fn declared(&self, c: &Contributions) -> Result<(), RpcError> {
        of_kind(c, &self.destination, DestinationKind::Store)
    }
}

/// For a set destination.
impl Hook for PublishSet {
    const METHOD: &'static str = SET_PUBLISH;
    type Answer = Published;

    fn declared(&self, c: &Contributions) -> Result<(), RpcError> {
        of_kind(c, &self.destination, DestinationKind::Set)
    }
}

/// `[[contributes.destinations]]`: names and titles.
pub(crate) fn validate(m: &Manifest) -> Result<(), ManifestError> {
    let (at, c) = ("contributes.destinations", &m.contributes);
    unique_names(at, "name", c.destinations.iter().map(|x| x.name.as_str()).enumerate())?;
    for (i, x) in c.destinations.iter().enumerate() {
        shown(format!("{at}[{i}].title"), &x.title)?;
    }
    Ok(())
}
