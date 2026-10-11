#![doc = include_str!("../README.md")]

mod decide;
mod dither;
mod error;
mod resample;
mod transcode;
mod write;

pub use decide::{Container, Decision, Dither, Options, RECIPE_VERSION, Reason, Recipe, Source, decide, estimate_size};
pub use error::{Error, Result};
pub use transcode::{Transcoded, transcode};
