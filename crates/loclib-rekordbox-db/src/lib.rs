#![doc = include_str!("../README.md")]
#![warn(missing_docs)]
#![cfg_attr(docsrs, feature(doc_cfg))]

mod error;
pub mod masterdb;
pub mod onelibrary;
/// Databases and drawn tracks for tests, in this crate and those built on it (the `test-util`
/// feature). Not part of the stable API.
#[cfg(any(test, feature = "test-util"))]
#[doc(hidden)]
pub mod test_util;

pub use error::{Error, Refusal, Result};
