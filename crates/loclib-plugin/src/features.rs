//! The protocol's optional features, each a family of hooks named in `hello`: the host lists
//! those it has, a plugin those it implements (the SDK works them out from its hooks). The host
//! calls a feature's methods only on a plugin that has it, and refuses one whose manifest
//! contributes what a feature it doesn't have would run. A new kind of hook is a new row here, not
//! a new protocol version.

use crate::{Contributions, method};

#[derive(Debug, Clone, Copy)]
pub struct Feature {
    /// As `hello` names it: its family of hooks (`commands`).
    pub name: &'static str,
    /// The host's methods a plugin with it answers.
    pub methods: &'static [&'static str],
    /// Whether a manifest contributes something only its methods run.
    pub contributed: fn(&Contributions) -> bool,
}

/// Every feature this side knows.
pub const FEATURES: &[Feature] = &[
    Feature { name: "commands", methods: &[method::RUN_COMMAND], contributed: |c| !c.commands.is_empty() },
    Feature { name: "operations", methods: &[method::PLAN_OP], contributed: |c| !c.operations.is_empty() },
    Feature { name: "analyzers", methods: &[method::ANALYZE_TRACK], contributed: |c| !c.analyzers.is_empty() },
    Feature { name: "exporters", methods: &[method::WRITE_EXPORT], contributed: |c| !c.exporters.is_empty() },
];
