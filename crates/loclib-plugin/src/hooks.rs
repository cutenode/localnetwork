//! The protocol's families of hooks, a module each, named after its feature: its methods' names,
//! their requests and answers, what a manifest contributes for it and how that's checked, its
//! [`crate::Hook`]s and its row in [`crate::FEATURES`]. What every family shares (the handshake, a
//! hook's context, progress, blobs, fetching; capabilities, settings, `where` and params) is in
//! `protocol` and `manifest`.
//!
//! A new family is a module here and a line in each list naming the families: the modules below,
//! the crate's re-exports and [`crate::method`]'s, alphabetical, and [`crate::FEATURES`],
//! [`crate::Contributions`] and [`crate::Manifest::validate`], whose order is on the wire or in the
//! sidecar's contract: there a family's line goes right after the line of the family before it
//! alphabetically (first, when there's none). Families added side by side then add separate lines.

pub(crate) mod analysis;
pub(crate) mod analyzers;
pub(crate) mod commands;
pub(crate) mod exporters;
pub(crate) mod operations;
pub(crate) mod providers;
