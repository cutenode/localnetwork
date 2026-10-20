//! The CLI's integration tests, in one test binary: each module was a file, and a binary, of its
//! own. They run `locallibrary` (the binary cargo built for the tests) with a store and drives in
//! a temporary folder. `cargo test -p locallibrary-cli --test it rpc::` runs one module's.

use std::path::Path;
use std::process::{Command, Stdio};

use assert_cmd::cargo::CommandCargoExt;

#[path = "../../../../crates/locallibrary/tests/support/files.rs"]
mod files;
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

/// When exports say they were written (`SOURCE_DATE_EPOCH`): 2026-09-21, 14:13:20 UTC.
const STAMPED: i64 = 1_790_000_000;

/// `locallibrary`, the binary cargo built for the tests, with its store at `store` and nothing to
/// read on stdin. It looks for drives only in the folders `volumes` (none: it sees no drive), never
/// among the ones plugged into the machine (see `LOCALLIBRARY_VOLUMES`), and has only the plugins
/// it ships. The account settings of whoever runs the tests don't reach it, nor do their
/// terminal's: it prints as it does piped (no colour) in a UTF-8 locale. Others' Lexicon documents
/// come from [`lexicons`], so nothing it checks is fetched. Traktor collections are stamped at one
/// time ([`STAMPED`]), so a dry run's sizes are what's written however the clock moves between.
fn locallibrary(store: &Path, volumes: &[&Path]) -> Command {
    let mut c = Command::cargo_bin("locallibrary").unwrap();
    #[cfg(feature = "account")]
    c.env("LOCALLIBRARY_LEXICONS", lexicons());
    c.env("LOCALLIBRARY_STORE", store)
        .env("LOCALLIBRARY_VOLUMES", std::env::join_paths(volumes).unwrap())
        .env("SOURCE_DATE_EPOCH", STAMPED.to_string())
        .env_remove("LOCALLIBRARY_OAUTH_CLIENT_ID")
        .env_remove("LOCALLIBRARY_SESSION_STORE")
        .env_remove("LOCALLIBRARY_TEST_PLUGINS")
        .env_remove("LOCALLIBRARY_USERINPUT_SPACE")
        .env_remove("CLICOLOR_FORCE")
        .env_remove("TERM")
        .env_remove("LC_ALL")
        .env_remove("LC_CTYPE")
        .env_remove("LANG")
        .stdin(Stdio::null());
    c
}

/// A folder of stand-ins for the Lexicon documents others maintain (see `LOCALLIBRARY_LEXICONS`),
/// written once for every test.
#[cfg(feature = "account")]
fn lexicons() -> &'static Path {
    use locallibrary::atmosphere::lexicon::ADDRESS;
    static DIR: std::sync::OnceLock<tempfile::TempDir> = std::sync::OnceLock::new();
    DIR.get_or_init(|| {
        let dir = tempfile::tempdir().unwrap();
        let doc = locallibrary::testing::address_lexicon().to_string();
        std::fs::write(dir.path().join(format!("{ADDRESS}.json")), doc).unwrap();
        dir
    })
    .path()
}
