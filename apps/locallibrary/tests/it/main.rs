//! The CLI's integration tests, in one test binary: each module was a file, and a binary, of its
//! own. They run `locallibrary` (the binary cargo built for the tests) with a store and drives in
//! a temporary folder. `cargo test -p locallibrary-cli --test it rpc::` runs one module's.

use std::path::Path;
use std::process::{Command, Stdio};

use assert_cmd::cargo::CommandCargoExt;

#[path = "../../../../crates/locallibrary/tests/support/files.rs"]
mod files;
#[path = "../../../../crates/locallibrary/tests/support/mock_oauth.rs"]
mod mock_oauth;
#[cfg(feature = "account")]
#[path = "../../../../crates/locallibrary/tests/support/mock_pds.rs"]
mod mock_pds;
#[path = "../../../../crates/locallibrary/tests/support/schema.rs"]
mod schema;
#[cfg(feature = "account")]
#[path = "../../../../crates/locallibrary/tests/support/session.rs"]
mod session;

mod account;
mod bug;
mod cli;
mod collection;
mod command_line;
mod commands;
mod completion_candidates;
mod drives;
mod interop;
mod jobs;
#[cfg(feature = "peers")]
mod peers;
// A release has no test plugin to serve, or to read through.
#[cfg(debug_assertions)]
mod plugin_reads;
#[cfg(debug_assertions)]
mod plugins;
mod profile;
mod props_cli;
mod props_cli_account;
mod props_cli_asked;
mod props_cli_changes;
mod props_cli_edits;
mod props_cli_exports;
mod props_cli_human;
mod props_cli_levels;
mod props_cli_listings;
mod props_cli_login_agent;
mod props_cli_mix;
mod props_cli_profile;
mod props_cli_restore;
mod props_cli_sets;
mod props_cli_smart;
mod props_cli_terminal;
mod props_cli_tracklist;
mod props_rpc;
mod props_rpc_every;
mod props_rpc_flows;
mod publish;
mod reports;
mod rpc;
#[cfg(debug_assertions)]
mod rpc_plugins;
mod sets;
mod support;
mod tags;
mod tracklist;
mod tracklist_xml;

/// When exports say they were written (`SOURCE_DATE_EPOCH`): 2026-09-21, 14:13:20 UTC.
const STAMPED: i64 = 1_790_000_000;

/// An address nothing answers at.
const NOWHERE: &str = "http://127.0.0.1:9";

/// `locallibrary`, the binary cargo built for the tests, with its store at `store` and nothing to
/// read on stdin. It looks for drives only in the folders `volumes` (none: it sees no drive), never
/// among the ones plugged into the machine (see `LOCALLIBRARY_VOLUMES`), and has only the plugins
/// it ships. The account settings of whoever runs the tests don't reach it, nor do their
/// terminal's: it prints as it does piped (no colour) in a UTF-8 locale. No plugin reaches a server
/// but this computer's (`LOCALLIBRARY_TEST_OFFLINE`), so none reaches MusicBrainz, nothing opens
/// a browser or an app (`LOCALLIBRARY_NO_OPEN`: a sign-in prints its link), and Local
/// Network's directory and website are at [`NOWHERE`] unless a test names its own. Traktor
/// collections are stamped at one time ([`STAMPED`]), so a dry run's sizes are what's written
/// however the clock moves between.
fn locallibrary(store: &Path, volumes: &[&Path]) -> Command {
    let mut c = Command::cargo_bin("locallibrary").unwrap();
    c.env("LOCALLIBRARY_STORE", store)
        .env("LOCALLIBRARY_VOLUMES", std::env::join_paths(volumes).unwrap())
        .env("SOURCE_DATE_EPOCH", STAMPED.to_string())
        .env("LOCALLIBRARY_TEST_OFFLINE", "1")
        .env("LOCALLIBRARY_NO_OPEN", "1")
        .env_remove("LOCALLIBRARY_OAUTH_CLIENT_ID")
        .env_remove("LOCALLIBRARY_SESSION_STORE")
        .env_remove("LOCALLIBRARY_TEST_PLUGINS")
        .env_remove("LOCALLIBRARY_TEST_LOOPBACK")
        .env_remove("LOCALLIBRARY_USERINPUT_SPACE")
        // Local Network's directory and website: nowhere, unless a test names its own.
        .env("LOCALLIBRARY_HAPPYVIEW_URL", NOWHERE)
        .env("LOCALLIBRARY_WEBSITE_URL", NOWHERE)
        .env_remove("CLICOLOR_FORCE")
        .env_remove("TERM")
        .env_remove("LC_ALL")
        .env_remove("LC_CTYPE")
        .env_remove("LANG")
        .stdin(Stdio::null());
    c
}
