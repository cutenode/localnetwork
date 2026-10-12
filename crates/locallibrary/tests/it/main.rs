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
mod layout;
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
mod props_anlz_phrases;
mod props_api;
mod props_api_drives;
mod props_api_editing;
mod props_api_jobs;
mod props_api_reads;
mod props_api_reports;
mod props_api_watch;
mod props_artwork;
mod props_artwork_more;
#[cfg(feature = "account")]
mod props_atmosphere;
mod props_care;
mod props_check;
mod props_check_more;
mod props_clean;
mod props_clones;
mod props_convert;
mod props_core2_artwork;
mod props_core2_backup;
mod props_core2_doctor;
mod props_core2_interop;
mod props_core2_player;
mod props_core2_plays;
mod props_core2_probe;
mod props_core2_query;
mod props_core2_relocate;
mod props_core2_replay;
mod props_core2_settings;
mod props_core2_smart;
mod props_core2_space;
mod props_core2_store;
mod props_core2_tidy;
mod props_core2_tree;
mod props_cues;
mod props_cues_more;
mod props_device;
mod props_diff;
mod props_disk2_check;
mod props_disk2_clean;
mod props_disk2_device;
mod props_disk2_upgrade;
mod props_doctor;
mod props_doctor_more;
mod props_edit;
mod props_edit_drift;
mod props_edit_journal;
mod props_edit_order;
mod props_edits2_cues;
mod props_edits2_drive;
mod props_edits2_journal;
mod props_edits2_replay;
mod props_edits2_setpath;
mod props_fleet;
mod props_fleet_more;
mod props_format;
mod props_harmony;
mod props_import;
mod props_interop;
mod props_merge;
mod props_merge_more;
mod props_mytags;
mod props_mytags_carry;
mod props_no_space;
mod props_offsite;
mod props_onelibrary;
mod props_onelibrary_crafted;
mod props_onelibrary_fields;
mod props_op;
mod props_pack_more;
mod props_paths;
mod props_pdb;
#[cfg(feature = "peers")]
mod props_peers;
mod props_plays;
mod props_probe;
mod props_profile;
#[cfg(feature = "account")]
mod props_publish;
mod props_push;
mod props_query;
mod props_recordings;
mod props_relocate;
mod props_scrub;
mod props_scrub_more;
mod props_setpath;
mod props_sets;
mod props_settings;
mod props_smart;
mod props_sound;
mod props_space;
mod props_store;
mod props_store_more;
mod props_store_records;
mod props_sync;
mod props_sync_more;
mod props_syncconv2_convert;
mod props_syncconv2_fleet;
mod props_syncconv2_offsite;
mod props_syncconv2_pack;
mod props_syncconv2_plan;
mod props_syncconv2_sync;
mod props_tagmerge2_merge;
mod props_tagmerge2_tags;
mod props_tags_model;
mod props_transcode;
mod props_undo;
mod props_upgrade;
mod props_upgrade_lead;
mod props_watch;
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
