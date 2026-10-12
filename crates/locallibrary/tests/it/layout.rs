//! How the workspace's source is laid out, where a tool relies on it.

use std::path::Path;

/// Unit tests live in a module's own `tests.rs` (`#[cfg(test)] mod tests;`), never inline:
/// `cargo coverage` leaves test files out by name (.cargo/config.toml), so an inline test module
/// would count as code that ships, and `tools/hegel-coverage.sh` could never reach it.
#[test]
fn unit_tests_live_in_files_of_their_own() {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut inline = Vec::new();
    for top in ["apps", "crates", "vendor"] {
        let files = walkdir::WalkDir::new(workspace.join(top))
            .into_iter()
            .filter_entry(|e| !matches!(e.file_name().to_str(), Some("target" | "node_modules")))
            .flatten()
            .filter(|e| e.path().extension().is_some_and(|x| x == "rs"));
        for f in files {
            let text = std::fs::read_to_string(f.path()).unwrap();
            let lines: Vec<&str> = text.lines().map(str::trim).collect();
            for (i, pair) in lines.windows(2).enumerate() {
                let opens = pair[1].starts_with("mod ") || pair[1].contains(" mod ");
                if pair[0] == "#[cfg(test)]" && opens && pair[1].ends_with('{') {
                    inline.push(format!("{}:{}", f.path().strip_prefix(&workspace).unwrap().display(), i + 2));
                }
            }
        }
    }
    assert!(inline.is_empty(), "move these test modules into their own tests.rs: {inline:#?}");
}
