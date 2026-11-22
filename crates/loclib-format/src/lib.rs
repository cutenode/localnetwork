#![doc = include_str!("../README.md")]
#![warn(missing_docs)]
#![cfg_attr(docsrs, feature(doc_cfg))]

mod error;
pub mod fat32;

pub use error::{Error, Result};
