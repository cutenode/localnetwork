//! Lists the plugins locallibrary ships, and gives `locallibrary` on Windows the 8 MiB
//! main-thread stack it has on Linux and macOS.

use std::path::Path;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    first_party();
    stack();
}

/// Writes `first_party.rs` to `OUT_DIR`, the table `plugins.rs` includes: every folder in the
/// repository's `plugins/` is a plugin crate, `locallibrary-plugin-<id>`, exporting `MANIFEST` and
/// `serve`, in alphabetical order (the order `plugin list` shows them in). Cargo only links a crate
/// this one depends on, so a folder without a dependency here, or a dependency without a folder,
/// fails the build saying what to change.
fn first_party() {
    let here = Path::new(env!("CARGO_MANIFEST_DIR"));
    let dir = here.join("../../plugins");
    let manifest = here.join("Cargo.toml");
    // Cargo looks through a directory for changes: a plugin's folder added or removed reruns this.
    println!("cargo:rerun-if-changed={}", dir.display());
    println!("cargo:rerun-if-changed={}", manifest.display());

    let mut ids: Vec<String> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("reading {}: {e}", dir.display()))
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.is_dir())
        .map(|path| path.file_name().unwrap().to_str().expect("a plugin's folder is named in UTF-8").to_owned())
        .collect();
    ids.sort();

    let cargo: toml::Table = toml::from_str(&std::fs::read_to_string(&manifest).unwrap()).unwrap();
    let dependencies = cargo["dependencies"].as_table().unwrap();
    let mut wrong = Vec::new();
    for id in &ids {
        if !dependencies.contains_key(&format!("locallibrary-plugin-{id}")) {
            wrong.push(format!(
                "plugins/{id}/ isn't a dependency of locallibrary: from the repository's root, run `cargo add -p locallibrary-cli --path plugins/{id}`"
            ));
        }
    }
    for name in dependencies.keys() {
        if let Some(id) = name.strip_prefix("locallibrary-plugin-")
            && !ids.iter().any(|i| i == id)
        {
            wrong.push(format!("{name} has no folder in plugins/: run `cargo remove -p locallibrary-cli {name}`"));
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));

    let table: String = ids
        .iter()
        .map(|id| {
            let krate = format!("locallibrary_plugin_{}", id.replace('-', "_"));
            format!("    ({krate}::MANIFEST, {krate}::serve),\n")
        })
        .collect();
    let out = Path::new(&std::env::var_os("OUT_DIR").unwrap()).join("first_party.rs");
    std::fs::write(&out, format!("&[\n{table}]\n")).unwrap_or_else(|e| panic!("writing {}: {e}", out.display()));
}

/// A Windows executable's main thread gets the stack its linker reserves: 1 MiB with MSVC's, 2 MiB
/// with MinGW's. clap's derive builds the whole command tree in a few long functions, and without
/// optimisations every temporary in them gets its own stack slot. A debug build needs about 1.4 MiB
/// just to parse its command line (measured on macOS, arm64), so on Windows it overflowed before
/// doing anything.
fn stack() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    const STACK: u32 = 8 << 20;
    match std::env::var("CARGO_CFG_TARGET_ENV").as_deref() {
        Ok("msvc") => println!("cargo:rustc-link-arg-bins=/STACK:{STACK}"),
        // MinGW's linkers (GNU ld, and lld for the gnullvm targets) through the C compiler.
        _ => println!("cargo:rustc-link-arg-bins=-Wl,--stack,{STACK}"),
    }
}
