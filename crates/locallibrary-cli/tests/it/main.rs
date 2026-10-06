//! The CLI's integration tests, in one test binary: each module was a file, and a binary, of its
//! own. They run `locallibrary` (the binary cargo built for the tests) with a store and drives in
//! a temporary folder. `cargo test -p locallibrary-cli --test it rpc::` runs one module's.

use std::path::Path;
use std::process::{Command, Stdio};

use assert_cmd::cargo::CommandCargoExt;

#[cfg(feature = "account")]
#[path = "../../../locallibrary/tests/support/mock_pds.rs"]
mod mock_pds;

mod account;
mod cli;
mod collection;
mod interop;
mod jobs;
mod profile;
mod publish;
mod reports;
mod rpc;
mod tags;

/// `locallibrary`, the binary cargo built for the tests, with its store at `store` and nothing to
/// read on stdin. It looks for drives only in the folders `volumes` (none: it sees no drive), never
/// among the ones plugged into the machine (see `LOCALLIBRARY_VOLUMES`), and the account settings
/// of whoever runs the tests don't reach it.
fn locallibrary(store: &Path, volumes: &[&Path]) -> Command {
    let mut c = Command::cargo_bin("locallibrary").unwrap();
    c.env("LOCALLIBRARY_STORE", store)
        .env("LOCALLIBRARY_VOLUMES", std::env::join_paths(volumes).unwrap())
        .env_remove("LOCALLIBRARY_OAUTH_CLIENT_ID")
        .env_remove("LOCALLIBRARY_SESSION_STORE")
        .stdin(Stdio::null());
    c
}
