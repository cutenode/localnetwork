#![doc = include_str!("../README.md")]

// The tests serve the example plugin, which names this crate as any plugin does.
#[cfg(test)]
extern crate self as loclib_plugin;

mod error;
pub mod framing;
mod manifest;
mod peer;
mod protocol;
mod sdk;
mod view;

pub use error::{ErrorData, RpcError, params};
pub use manifest::{
    Analyzer, Capability, CliHints, Command, Contributions, Exporter, FieldType, FieldValue, Manifest, ManifestError,
    Mode, Operation, Setting, SettingType, check_params, is_path,
};
pub use peer::{Incoming, Peer, Pending, Request};
pub use protocol::*;
pub use sdk::{Host, Plugin, serve};
pub use view::{ListItem, Pair, View};

/// The error codes.
pub mod code {
    pub use crate::error::{FAILED, INTERNAL, INVALID_PARAMS, INVALID_REQUEST, METHOD_NOT_FOUND, PARSE_ERROR};
}
