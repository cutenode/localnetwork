//! The reads a plugin may call back with, by the sidecar's names: the capability that grants each
//! and the params a plugin may give it. The host refuses any other read, and any param not listed
//! for one. The README's table of reads is made from [`READS`].

use crate::{Capability, MAX_ANALYSES, MAX_IDS, MAX_IMAGES};

/// The most tracks (or snapshots, or matches, or most played) a page holds: a plugin asks for
/// the next.
const MAX_PAGE: u64 = 500;
/// The most tracks asked for at once by reads that answer each in full (details, cues).
const MAX_TRACKS: usize = 100;

/// What a read of a drive's library takes before its own params.
const LIBRARY: &[(&str, Param)] = &[("drive", Param::Drive), ("combined", Param::Plain)];

/// A param a plugin may give a read, and what the host makes of it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Param {
    /// Passed on as it is.
    Plain,
    /// A drive (or library) by id or name, never a path: the host finds it itself.
    Drive,
    /// A list of them.
    Drives,
    /// A list of at most this many.
    AtMost(usize),
    /// A page size of at most this many, this many when not given.
    Limit(u64),
    /// This when not given.
    Or(&'static str),
    /// A file in the request's scratch folder, absolute: the host refuses one anywhere else.
    Scratch,
}

/// A read a plugin may call.
#[derive(Debug, Clone, PartialEq)]
pub struct Read {
    /// As the sidecar names it (`tracks.query`).
    pub method: &'static str,
    /// What the manifest needs to call it.
    pub capability: Capability,
    /// Whether it reads a drive's library, and so takes `drive` and `combined` first.
    pub library: bool,
    /// Its other params.
    pub others: &'static [(&'static str, Param)],
}

impl Read {
    /// Every param a plugin may give it, a library's `drive` and `combined` first.
    pub fn params(&self) -> impl Iterator<Item = (&'static str, Param)> {
        let library = self.library;
        LIBRARY.iter().filter(move |_| library).chain(self.others).copied()
    }
}

/// A row of [`READS`]: a struct literal, which leaves no code to run.
macro_rules! read {
    ($method:literal, $capability:ident, $library:literal, $others:expr $(,)?) => {
        Read { method: $method, capability: Capability::$capability, library: $library, others: $others }
    };
}

/// Every read a plugin may call. Anything else no plugin may: writes, reads naming files on the
/// computer outside the request's scratch folder, and reads whose answers can't be bounded. A
/// param added to a read is refused until it's listed here.
pub const READS: &[Read] = {
    use Param::*;
    &[
        read!("drive.library", LibraryRead, true, &[("offset", Plain), ("limit", Limit(MAX_PAGE))]),
        read!("playlists.tree", LibraryRead, true, &[]),
        read!("tracks.get", LibraryRead, true, &[("ids", AtMost(MAX_IDS))]),
        read!(
            "tracks.query",
            LibraryRead,
            true,
            &[("filter", Plain), ("playlist", Plain), ("sort", Plain), ("offset", Plain), ("limit", Limit(MAX_PAGE))],
        ),
        read!("track.show", LibraryRead, true, &[("ids", AtMost(MAX_TRACKS)), ("paths", AtMost(MAX_TRACKS))]),
        read!("cues.list", LibraryRead, true, &[("ids", AtMost(MAX_TRACKS)), ("paths", AtMost(MAX_TRACKS))]),
        read!("track.matches", LibraryRead, true, &[("track", Plain), ("limit", Limit(MAX_PAGE))]),
        read!("tags.list", LibraryRead, false, &[("drive", Drive)]),
        read!("track.analysis", AnalysisRead, true, &[("track", Plain), ("waveform", Or("none")), ("encoding", Plain)]),
        read!(
            "tracks.analysis",
            AnalysisRead,
            true,
            &[("ids", AtMost(MAX_ANALYSES)), ("waveform", Plain), ("encoding", Plain)],
        ),
        read!("track.fields", AnalysisRead, true, &[("tracks", AtMost(MAX_IDS)), ("plugin", Plain)]),
        read!("art.get", ArtworkRead, true, &[("track", Plain), ("tracks", AtMost(MAX_IMAGES))]),
        read!("drive.info", DriveInfo, false, &[("drive", Drive)]),
        read!("drive.backups", DriveInfo, false, &[("drive", Drive)]),
        read!("drive.space", DriveInfo, false, &[("drive", Drive)]),
        read!("drives.list", DriveInfo, false, &[("all", Plain)]),
        read!("drives.mounted", DriveInfo, false, &[]),
        read!("drive.groups", DriveInfo, false, &[]),
        read!("fleet.status", DriveInfo, false, &[("all", Plain), ("retired", Plain)]),
        read!(
            "doctor",
            HealthRead,
            false,
            &[
                ("drive", Drive),
                ("hash_audio", Plain),
                ("by_sound", Plain),
                ("only", Plain),
                ("audio", Plain),
                ("analyze", Plain),
                ("sure_only", Plain),
            ],
        ),
        read!("check", HealthRead, false, &[("drives", Drives), ("players", Plain), ("audio", Plain)]),
        read!("drive.verify", HealthRead, false, &[("drive", Drive), ("snapshot", Plain)]),
        read!("store.info", StoreRead, false, &[]),
        read!("store.copies", StoreRead, false, &[]),
        read!("library.list", StoreRead, false, &[]),
        read!("smart.list", StoreRead, false, &[]),
        read!("store.history", StoreRead, false, &[("library", Plain), ("limit", Limit(MAX_PAGE))]),
        read!(
            "snapshot.library",
            StoreRead,
            false,
            &[("snapshot", Plain), ("offset", Plain), ("limit", Limit(MAX_PAGE))],
        ),
        read!("settings.schema", SettingsRead, false, &[]),
        read!("settings.profile", SettingsRead, false, &[]),
        read!("players.models", SettingsRead, false, &[]),
        read!("players.show", SettingsRead, false, &[]),
        read!("settings.show", SettingsRead, false, &[("drive", Drive)]),
        read!("player.show", SettingsRead, false, &[("drive", Drive)]),
        read!("plays.stats", PlaysRead, false, &[("source", Drive), ("since", Plain), ("limit", Limit(MAX_PAGE))]),
        read!("sets.played", PlaysRead, false, &[("track", Plain)]),
        read!("sets.list", SetsRead, false, &[("venue", Plain), ("kind", Plain), ("since", Plain), ("until", Plain)]),
        read!("sets.get", SetsRead, false, &[("set", Plain), ("venue", Plain)]),
        read!("sessions.list", SetsRead, false, &[("drive", Drive), ("unassigned", Plain)]),
        // A recorded set's tracklist, for a mix exporter.
        read!("mix.reports.get", SetsRead, false, &[("id", Plain)]),
        read!("account.status", AccountStatus, false, &[]),
        read!("collection.info", CollectionRead, false, &[]),
        read!("collection.diff", CollectionRead, false, &[("drive", Drive), ("playlist", Plain)]),
        // rekordbox's files, for a backup: the host answers these itself.
        read!("collection.locations", CollectionFiles, false, &[]),
        read!("collection.status", CollectionFiles, false, &[]),
        read!("collection.copy", CollectionFiles, false, &[("into", Scratch), ("with_credentials", Plain)]),
        read!(
            "collection.tracks",
            CollectionFiles,
            false,
            &[("db", Scratch), ("offset", Plain), ("limit", Limit(MAX_PAGE))],
        ),
    ]
};

#[cfg(test)]
mod tests;
