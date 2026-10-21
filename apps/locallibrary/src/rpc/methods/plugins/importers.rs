//! `plugins.import.*`: another program's file read with a plugin's importer, what it says added to
//! a drive's tracks and the plays it logged kept as sessions, planned and applied.

use std::path::PathBuf;

use locallibrary::op::Planned;
use locallibrary::plugin::{PluginImport, PluginImported};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;

use crate::plugins::host;
use crate::rpc::RpcError;
use crate::rpc::methods::{Call, PlanStore, Registry};

#[derive(Deserialize, JsonSchema)]
struct PlanPluginImport {
    plugin: String,
    /// The importer's format.
    importer: String,
    /// The file to read (absolute), on this computer.
    file: PathBuf,
    /// The library or drive whose tracks it's about.
    drive: String,
    /// As the importer's `options` schema says (`plugins.describe`).
    #[serde(default)]
    options: Value,
}

pub(super) fn register(r: &mut Registry) {
    r.plan("plugins.import.plan", import_plan).long();
    r.apply("plugins.import.apply", PlanStore::Optional, |c, req: PlanPluginImport| -> Result<PluginImported, _> {
        Ok(import_plan(c, req)?.apply(c.store_opt().as_ref(), c.ctx)?)
    })
    .long();
}

/// The sessions it reads are kept in the store, made for them if there's none yet.
fn import_plan(c: &Call<'_>, req: PlanPluginImport) -> Result<Planned<PluginImport>, RpcError> {
    let store = c.store_opt();
    let plugins = host()?;
    Ok(plugins.import_plan(&req.plugin, &req.importer, &req.file, &req.drive, req.options, store.as_ref(), c.ctx)?)
}
