//! # locallibrary
//!
//! Tooling for rekordbox USB library exports ("device libraries"):
//!
//! * [`store`]: a content-addressed, deduplicating vault of library snapshots with history;
//!   [`retention`] prunes it and [`offsite`] copies it elsewhere.
//! * [`sync`]: incremental, crash-safe mirroring between drives and snapshots.
//! * [`compare`]: decide which of two drives holds the newer state of a library.
//! * [`merge`]: merge one library's tracks, analysis, artwork and playlists into another.
//! * [`import`]: read outside tracklists (M3U, CSV, rekordbox text and XML, plain text) and find
//!   their tracks in a library (the `loclib-tracklist` crate, re-exported).
//! * [`smart`]: rule-based playlists kept in the store and written to drives as plain playlists.
//! * [`pack`]: drives for a set, holding only chosen playlists of another drive.
//! * [`settings`]: manage the player/mixer "My Settings" on every drive from one profile (the
//!   files themselves are [`loclib_mysettings`]'s).
//! * [`artwork`]: set and audit cover art on drives and in audio files.
//! * `atmosphere`: sign in with an Atmosphere account (AT Protocol OAuth) and keep the session.
//! * `publish`: the records locallibrary publishes to that account (My Settings, the profile,
//!   sets).
//! * [`clean`]: remove tracks nobody wants (rekordbox's bundled demo samples) from a drive.
//! * [`cue_edit`]: set, rename, recolour, delete and copy hot cues, memory cues and loops.
//! * [`edit`]: edit playlists, folders and track details on a drive, in both databases.
//! * [`onelibrary`]: read and edit `exportLibrary.db`, the only database newer players
//!   (CDJ-3000X, OPUS-QUAD, XDJ-AZ...) read.
//! * [`convert`]: build either database from the other.
//! * [`tags`]: read and edit My Tags in `exportExt.pdb` and OneLibrary.
//! * [`harmony`]: Camelot keys, harmonic matches and mix costs (from loclib-model, like the
//!   library [`model`] itself).
//! * [`profile`]: what a DJ's sets, collection and settings say about how they play, and the part
//!   of it they choose to show on a public profile.
//! * [`mix`]: find which library tracks a recorded DJ mix plays, when, and how (the fingerprints
//!   and the matcher are loclib-mixid's).
//! * [`query`]: the track filter and sort language, and track selectors.
//! * [`xml`]: export a drive's library as rekordbox XML (to import it back into rekordbox).
//! * [`masterdb`]: read rekordbox's own collection (`master.db`) on this computer, and compare a
//!   drive with it.
//! * [`interop`]: export a drive's library for other DJ software (the formats are written by
//!   loclib-interop).
//! * [`op`]: planned operations: what a change will do, confirmed and then applied exactly as
//!   planned.
//! * [`opener`]: hand an https address to the browser, or a file to the app that opens it.
//! * [`mod@format`]: format a drive the way players read it best.
//! * [`equipment`]: what each player, all-in-one system and mixer reads: databases, file systems,
//!   audio formats (the `loclib-equipment` crate's table), and the models a DJ plays on.
//! * [`fleet`]: every drive you own, mounted or not: roles, logbook, copies and recognition.
//! * [`watch`]: notice drives being plugged in, unplugged and changed, by polling cheaply.
//!
//! Every operation is exposed as a function taking plain data and returning serde-serialisable
//! reports, with progress and cancellation through [`progress::Ctx`], so the same API can back
//! the `locallibrary` CLI, a JSON-RPC sidecar, or native bindings for an Electron app.
//!
//! ## Features
//!
//! * `account` (default): the Atmosphere account and publishing to it (`atmosphere`, `publish`,
//!   and the `account_*` and `*_publish_*` functions in [`api`]). It brings an HTTP client, an
//!   async runtime, TLS and the system keychain, most of the library's dependencies. Without it
//!   everything else works, the profile document included; it just can't be published.

pub mod after_set;
pub mod anlz;
pub mod api;
pub mod artwork;
#[cfg(feature = "account")]
pub mod atmosphere;
pub mod audio_cache;
pub mod backup;
pub mod check;
pub mod clean;
mod commit;
pub mod compare;
pub mod convert;
pub mod cue_edit;
pub mod device;
pub mod diff;
pub mod doctor;
pub mod edit;
pub mod eject;
pub mod equipment;
pub mod error;
pub mod export;
pub mod firmware;
pub mod fleet;
pub mod format;
pub mod fscheck;
pub mod fsutil;
pub mod hash;
pub mod import;
mod incoming;
pub mod interop;
pub mod masterdb;
pub mod merge;
pub mod mix;
pub mod offsite;
pub mod onelibrary;
pub mod op;
pub mod opener;
pub mod pack;
pub mod paths;
pub mod pdb;
#[cfg(feature = "peers")]
pub mod peers;
pub mod player;
pub mod plays;
pub mod probe;
pub mod profile;
pub mod progress;
#[cfg(feature = "account")]
pub mod publish;
pub mod query;
pub mod recordings;
pub mod relocate;
pub mod retention;
pub mod schema;
pub mod scrub;
pub mod sets;
pub mod settings;
pub mod smart;
pub mod sound;
pub mod space;
pub mod store;
pub mod sync;
pub mod tags;
pub mod tidy;
mod timeline;
pub mod tree;
pub mod upgrade;
pub mod watch;
pub mod xml;

/// Hooks for tests: what a failed write leaves behind, and tests that share a process.
#[doc(hidden)]
pub mod testing {
    pub use crate::commit::fail_before_swap;

    /// Drive lookups on this thread consult the store at `root` (`None`: the one
    /// [`crate::api::set_store_root`] named), as `set_store_root` would make them do for the whole
    /// process, which it can only do once: tests sharing a process each name their own store.
    pub fn store_root_on_this_thread(root: Option<std::path::PathBuf>) {
        crate::api::THREAD_STORE_ROOT.with(|r| *r.borrow_mut() = root);
    }

    /// Drives on this thread are looked for only in the subfolders of `folders` (`None`: as
    /// `$LOCALLIBRARY_VOLUMES` says), as that variable would make them for the whole process.
    pub fn volumes_on_this_thread(folders: Option<Vec<std::path::PathBuf>>) {
        crate::device::THREAD_VOLUMES.with(|v| *v.borrow_mut() = folders);
    }

    /// Plans on this thread warn as they would on `os` (`linux`, `windows`; `None`: this one)
    /// about what hasn't been tested there (see [`crate::format::untested_here`]).
    pub fn os_on_this_thread(os: Option<&'static str>) {
        crate::format::THREAD_OS.with(|o| o.set(os));
    }
}

pub use loclib_model::{harmony, model, tokens};

pub use device::{Device, DeviceInfo};
pub use error::{Error, Result};
pub use hash::Hash;
pub use model::{Library, Playlist, Track};
pub use progress::{CancelToken, Ctx, Event, Progress};
pub use store::Store;
