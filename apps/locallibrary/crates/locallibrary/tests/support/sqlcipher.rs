//! The `sqlcipher` CLI, to check with SQLCipher's own implementation (the one rekordbox's files
//! follow) that it reads the databases locallibrary encrypts.
//!
//! Without the CLI the checks are skipped, unless `LOCALLIBRARY_REQUIRE_SQLCIPHER` is set (CI sets
//! it where it installs the CLI): then they fail, so CI can't pass without running them.
//!
//! Shared by the library's unit tests and its integration tests (`#[path]`).

use std::path::Path;
use std::process::Command;

/// What the `sqlcipher` CLI prints, a line per row, for `statements` run on the database at
/// `path` after `PRAGMA key = '<key>'` (which prints `ok`). `None` when the CLI isn't installed.
pub fn run(path: &Path, key: &str, statements: &[&str]) -> Option<Vec<String>> {
    if !Command::new("sqlcipher").arg("--version").output().is_ok_and(|o| o.status.success()) {
        assert!(
            std::env::var_os("LOCALLIBRARY_REQUIRE_SQLCIPHER").is_none(),
            "LOCALLIBRARY_REQUIRE_SQLCIPHER is set, but there's no sqlcipher CLI to run"
        );
        return None;
    }
    let out =
        Command::new("sqlcipher").arg(path).arg(format!("PRAGMA key = '{key}';")).args(statements).output().unwrap();
    assert!(out.status.success(), "sqlcipher failed: {}", String::from_utf8_lossy(&out.stderr));
    Some(String::from_utf8_lossy(&out.stdout).lines().map(String::from).collect())
}
