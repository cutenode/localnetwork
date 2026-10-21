//! The protocol's optional features, each a family of hooks named in `hello`: the host lists
//! those it has, a plugin those it implements (the SDK works them out from its hooks). The host
//! calls a feature's methods only on a plugin that has it, answers what it sends only from one
//! that has it, and refuses one whose manifest contributes what a feature it doesn't have would
//! run. A new kind of hook is a new row here (its family's, see `crate::hooks`), not a new
//! protocol version.

use crate::{Manifest, hooks, method};

#[derive(Debug, Clone, Copy)]
pub struct Feature {
    /// As `hello` names it: its family of hooks (`commands`).
    pub name: &'static str,
    /// The host's methods a plugin with it answers. A feature with none is the SDK's own: what
    /// a plugin may send the host, which every plugin written with it has.
    pub methods: &'static [&'static str],
    /// The requests and notifications a plugin with it may send the host.
    pub sends: &'static [&'static str],
    /// Whether a manifest contributes something only it runs.
    pub contributed: fn(&Manifest) -> bool,
}

/// Every feature this side knows, in the order `hello` lists them (a family's placed as
/// `crate::hooks` says).
pub const FEATURES: &[Feature] = &[
    hooks::commands::FEATURE,
    hooks::operations::FEATURE,
    hooks::analyzers::FEATURE,
    hooks::exporters::FEATURE,
    hooks::importers::FEATURE,
    hooks::providers::FEATURE,
    // Every hook's `context.scratch`, `{"blob": name}` in answers, and a view's rows in parts.
    Feature { name: "blobs", methods: &[], sends: &[method::PARTIAL], contributed: |_| false },
    Feature { name: "net", methods: &[], sends: &[method::NET_FETCH], contributed: |m| !m.net.is_empty() },
    // A view command's or an operation's question for the user (`View::Choose`, `{"choose": ...}`).
    Feature { name: "choose", methods: &[], sends: &[], contributed: |_| false },
];
