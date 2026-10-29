//! Lists the plugins locallibrary ships, and gives `locallibrary` on Windows the 8 MiB
//! main-thread stack it has on Linux and macOS.

use std::path::{Path, PathBuf};

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    built_in();
    stack();
}

/// Writes `built_in.rs` to `OUT_DIR`, the table `plugins.rs` includes, in alphabetical order of id
/// (the order `plugin list` shows them in): each plugin's manifest, and how it serves. Every folder
/// in the repository's `plugins/` but `accounts/` is a plugin crate, `locallibrary-plugin-<id>`,
/// exporting `MANIFEST` and `serve`. Cargo only links a crate this one depends on, so a folder
/// without a dependency here, or a dependency without a folder, fails the build saying what to
/// change. Every folder in `plugins/accounts/` is an account plugin, its `locallibrary-plugin.toml`
/// alone, which nothing serves: a folder without one fails the build too.
fn built_in() {
    let here = Path::new(env!("CARGO_MANIFEST_DIR"));
    let dir = here.join("../../plugins");
    let manifest_only = dir.join("accounts");
    let manifest = here.join("Cargo.toml");
    // Cargo looks through a directory and everything in it for changes: a plugin's folder added
    // or removed reruns this, an account plugin's too.
    println!("cargo:rerun-if-changed={}", dir.display());
    println!("cargo:rerun-if-changed={}", manifest.display());

    let ids: Vec<String> =
        folders(&dir).into_iter().filter(|(_, path)| *path != manifest_only).map(|(id, _)| id).collect();
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
    let mut table: Vec<(String, String)> = ids
        .into_iter()
        .map(|id| {
            let krate = format!("locallibrary_plugin_{}", id.replace('-', "_"));
            let row = format!("    ({krate}::MANIFEST, Some({krate}::serve)),\n");
            (id, row)
        })
        .collect();
    for (id, path) in folders(&manifest_only) {
        let file = path.join("locallibrary-plugin.toml");
        match file.is_file() {
            // Written as a string literal, which escapes Windows' backslashes.
            true => table.push((id, format!("    (include_str!({file:?}), None),\n"))),
            false => wrong.push(format!(
                "plugins/accounts/{id}/ has no locallibrary-plugin.toml: an account plugin is that manifest alone"
            )),
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
    table.sort();

    let table: String = table.into_iter().map(|(_, row)| row).collect();
    let out = Path::new(&std::env::var_os("OUT_DIR").unwrap()).join("built_in.rs");
    std::fs::write(&out, format!("&[\n{table}]\n")).unwrap_or_else(|e| panic!("writing {}: {e}", out.display()));
}

/// The folders in `dir`, each with its name.
fn folders(dir: &Path) -> Vec<(String, PathBuf)> {
    let entries = std::fs::read_dir(dir).unwrap_or_else(|e| panic!("reading {}: {e}", dir.display()));
    let paths = entries.map(|entry| entry.unwrap().path()).filter(|path| path.is_dir());
    let named = |path: PathBuf| {
        let name = path.file_name().unwrap().to_str().expect("a plugin's folder is named in UTF-8").to_owned();
        (name, path)
    };
    paths.map(named).collect()
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
