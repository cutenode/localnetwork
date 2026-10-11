//! The library's integration tests, in one test binary. Each module was a file, and a binary,
//! of its own: every one linked the whole library, and they ran one after another. Together the
//! library is linked once and every test runs at once.
//!
//! `cargo test -p locallibrary --test it props_pdb::` runs one module's tests. Tests here that
//! need the mounted volumes, or the store drive lookups consult, name them for their own thread
//! (`locallibrary::testing::volumes_on_this_thread`, `store_root_on_this_thread`):
//! `$LOCALLIBRARY_VOLUMES` and `api::set_store_root` would hold for every test in the process.

#[path = "../common/mod.rs"]
mod common;
#[cfg(feature = "account")]
#[path = "../support/mock_pds.rs"]
mod mock_pds;

mod analysis;
mod artwork;
mod check;
mod clean;
mod clones;
mod commit;
mod convert;
mod cues;
mod diff;
mod doctor;
mod drives;
mod edit;
mod export;
mod format;
mod interop;
mod jobs;
mod merge;
mod mixes;
mod onelibrary;
mod pack;
mod plans;
mod plays;
mod profile;
mod props_anlz;
mod props_clones;
mod props_cues;
mod props_diff;
mod props_edit;
mod props_format;
mod props_harmony;
mod props_import;
mod props_mytags;
mod props_onelibrary;
mod props_paths;
mod props_pdb;
mod props_profile;
mod props_query;
mod props_relocate;
mod props_settings;
mod props_store;
mod props_sync;
mod publish;
mod reads;
mod regressions;
mod relocate;
mod restore;
mod sessions;
mod sets;
mod settings;
mod sound;
mod store;
mod sync;
mod tags;
mod tui_api;
mod watch;
