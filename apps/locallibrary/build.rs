//! Gives `locallibrary` on Windows the 8 MiB main-thread stack it has on Linux and macOS.
//!
//! A Windows executable's main thread gets the stack its linker reserves: 1 MiB with MSVC's, 2 MiB
//! with MinGW's. clap's derive builds the whole command tree in a few long functions, and without
//! optimisations every temporary in them gets its own stack slot. A debug build needs about 1.4 MiB
//! just to parse its command line (measured on macOS, arm64), so on Windows it overflowed before
//! doing anything.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
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
