#![doc = include_str!("../README.md")]

// The tests serve the example plugin, which names this crate as any plugin does.
#[cfg(test)]
extern crate self as loclib_plugin;

mod choice;
mod error;
mod features;
pub mod framing;
mod hooks;
mod manifest;
mod peer;
mod protocol;
mod reads;
mod sdk;
mod view;

pub use choice::{Choice, ChoiceItem};
pub use error::{ErrorData, RpcError, params};
pub use features::{FEATURES, Feature};
pub use hooks::account_types::{AccountType, SUBJECT_PLACEHOLDER, Scope, ScopeRequest, Whoami};
pub use hooks::accounts::{ACCOUNT_PLACEHOLDER, Account, AccountNeed, OptionalScopes, TokenPlace, account_placeholder};
pub use hooks::analysis::{Output, WaveformBlob};
pub use hooks::analyzers::{
    Analysis, AnalyzeTrack, Analyzer, FieldDisplay, FieldFormat, FieldType, FieldValue, Role, Track,
};
pub use hooks::checks::{Check, CheckFinding, Findings, Fix, PlanCheck, RunCheck, When};
pub use hooks::commands::{Command, Mode, RunCommand};
pub use hooks::destinations::{
    Destination, DestinationKind, GetBlob, GetManifest, Got, GotManifest, HasBlobs, Kept, Missing, PublishSet,
    Published, PublishedSet, PutBlob, PutManifest, StrongRef, TracklistEntry,
};
pub use hooks::events::{
    AnalyzeAction, EventName, EventNotify, ExportAction, FetchAction, HostAction, HostNotify, Listener,
    MAX_SUGGESTED_PARAMS, Suggested, Suggestion,
};
pub use hooks::exporters::{ExportFrom, ExportSource, ExportSources, Exported, ExportedFile, Exporter, WriteExport};
pub use hooks::importers::{
    ImportAnswer, ImportKind, Imported, ImportedCue, ImportedLog, ImportedPlay, ImportedTrack, Importer, ReadImport,
    TrackMatch,
};
pub use hooks::operations::{OpAnswer, OpPlan, Operation, PlanOp};
pub use hooks::panels::{Panel, RenderPanel};
pub use hooks::providers::{
    Candidate, Candidates, ProvideCandidates, Provider, ProviderKind, TrackMetadata, TrackTags,
};
pub use hooks::services::{Service, StartService};
pub use hooks::sources::{LibraryPage, LibrarySource, ReadLibrary, SourceCue, SourcePlaylist, SourceTrack};
pub use hooks::themes::Theme;
/// The types [`DriveReads`] answers with (`Track`, `Library`, `TrackAnalysis`), for a plugin to
/// name without depending on loclib-model itself.
pub use loclib_model;
/// What a check finds, as [`CheckFinding`]s carry it.
pub use loclib_model::finding::{Finding, Level};
pub use manifest::{
    Capability, CliHints, Contributions, Manifest, ManifestError, NetLimit, Selection, Setting, SettingType, Where,
    check_params, is_path,
};
pub use peer::{Incoming, Peer, Pending, Request};
pub use protocol::*;
pub use reads::{
    AnalysedTrack, ArtworkImage, DriveInfo, DriveReads, MAX_ANALYSES, MAX_IDS, MAX_IMAGES, PlaylistNode, PlaylistTree,
    TrackArtwork, TracksAnalysis, TracksById, TracksQuery,
};
pub use sdk::{Hook, Hooks, Host, serve};
pub use view::{ChartStyle, Column, ColumnKind, ListItem, Pair, Section, Series, TreeItem, TypedColumn, View};

/// The error codes.
pub mod code {
    pub use crate::error::{FAILED, INTERNAL, INVALID_PARAMS, INVALID_REQUEST, METHOD_NOT_FOUND, PARSE_ERROR};
}
