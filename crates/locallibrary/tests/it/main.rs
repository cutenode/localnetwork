//! The library's integration tests, in one test binary. Each module was a file, and a binary,
//! of its own: every one linked the whole library, and they ran one after another. Together the
//! library is linked once and every test runs at once.
//!
//! `cargo test -p locallibrary --test it props_pdb::` runs one module's tests. Tests here that
//! need the mounted volumes, or the store drive lookups consult, name them for their own thread
//! (`common::isolate`, through `locallibrary::testing::volumes_on_this_thread` and
//! `store_root_on_this_thread`): `$LOCALLIBRARY_VOLUMES` and `api::set_store_root` would hold for
//! every test in the process.

#[path = "../common/mod.rs"]
mod common;
#[cfg(feature = "account")]
#[path = "../support/mock_pds.rs"]
mod mock_pds;
#[path = "../support/schema.rs"]
mod schema;
#[cfg(feature = "account")]
#[path = "../support/session.rs"]
mod session;

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
mod drawn;
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
#[cfg(feature = "peers")]
mod peers;
mod plans;
mod plays;
mod profile;
mod props_after_set;
mod props_anlz;
mod props_artwork;
mod props_care;
mod props_check;
mod props_clean;
mod props_clones;
mod props_convert;
mod props_cues;
mod props_device;
mod props_diff;
mod props_doctor;
mod props_edit;
mod props_fleet;
mod props_format;
mod props_harmony;
mod props_import;
mod props_interop;
mod props_merge;
mod props_mytags;
mod props_mytags_carry;
mod props_offsite;
mod props_onelibrary;
mod props_op;
mod props_paths;
mod props_pdb;
mod props_plays;
mod props_probe;
mod props_profile;
mod props_push;
mod props_query;
mod props_recordings;
mod props_relocate;
mod props_scrub;
mod props_setpath;
mod props_settings;
mod props_smart;
mod props_space;
mod props_store;
mod props_store_records;
mod props_sync;
mod props_upgrade;
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
mod upgrade;
mod watch;
