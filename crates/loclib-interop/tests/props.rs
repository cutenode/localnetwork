//! Property-based tests of the exports for other DJ software, each read back with an independent
//! reader (quick-xml for Traktor's NML and rekordbox XML, SQLite for Engine DJ, lofty for Serato's
//! tags) or by the format's own description.

// The modules sit in props/ beside this file; `hegel-coverage.sh` can't place tests in a
// `props/main.rs` crate root.
#[path = "props/common.rs"]
mod common;
#[path = "props/encode.rs"]
mod encode;
#[cfg(feature = "engine")]
#[path = "props/engine.rs"]
mod engine;
#[cfg(feature = "rekordbox")]
#[path = "props/rekordbox.rs"]
mod rekordbox;
#[cfg(feature = "serato")]
#[path = "props/serato.rs"]
mod serato;
#[path = "props/software.rs"]
mod software;
#[cfg(feature = "traktor")]
#[path = "props/traktor.rs"]
mod traktor;
