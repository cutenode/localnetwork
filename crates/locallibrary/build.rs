//! Writes `flag_keys.rs` to `OUT_DIR`, which `flags.rs` includes: a constant for each feature flag
//! in the repository's `flags/catalog/`, so code still using a removed flag stops compiling. A
//! flag's file is named for its key, which the website's catalog test holds it to.

use std::path::Path;

fn main() {
    let catalog = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../flags/catalog");
    println!("cargo:rerun-if-changed=build.rs");
    // Cargo looks through a directory and everything in it for changes: a flag added, changed or
    // removed reruns this.
    println!("cargo:rerun-if-changed={}", catalog.display());

    let files = std::fs::read_dir(&catalog).unwrap_or_else(|e| panic!("reading {}: {e}", catalog.display()));
    let paths = files.map(|file| file.unwrap().path()).filter(|path| path.extension().is_some_and(|e| e == "json"));
    let stem = |path: std::path::PathBuf| path.file_stem().unwrap().to_str().expect("a key is ASCII").to_owned();
    let mut keys: Vec<String> = paths.map(stem).collect();
    keys.sort();

    let constants: String = keys.iter().map(|key| format!("pub const {}: &str = {key:?};\n", name(key))).collect();
    let all = keys.iter().map(|key| name(key)).collect::<Vec<_>>().join(", ");
    let out = Path::new(&std::env::var_os("OUT_DIR").unwrap()).join("flag_keys.rs");
    std::fs::write(&out, format!("{constants}pub const ALL: &[&str] = &[{all}];\n"))
        .unwrap_or_else(|e| panic!("writing {}: {e}", out.display()));
}

/// `parityCanary`'s constant: `PARITY_CANARY`.
fn name(key: &str) -> String {
    let words = key.chars().flat_map(|c| c.is_ascii_uppercase().then_some('_').into_iter().chain([c]));
    words.map(|c| c.to_ascii_uppercase()).collect()
}
