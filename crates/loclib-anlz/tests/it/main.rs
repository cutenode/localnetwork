//! The crate's integration tests, in one test binary: property tests of every reader and writer,
//! on rekordbox's own files from the demo export and on synthetic ones, checked against
//! rekordcrate's independent parser where it reads the same sections.

mod common;
mod cues;
mod edges;
mod phrases;
mod write;
