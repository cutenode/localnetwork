//! `plugins.export.*`: a drive's library, a set or a mix in a plugin's format, planned (the files
//! it would write) and written.

use std::path::PathBuf;

use locallibrary::op::Planned;
use locallibrary::plugin::PluginExport;
use loclib_plugin::{ExportSource, Exported};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;

use crate::plugins::host;
use crate::rpc::RpcError;
use crate::rpc::methods::{Call, PlanStore, Registry};

#[derive(Deserialize, JsonSchema)]
struct PlanPluginExport {
    plugin: String,
    /// The exporter's format.
    exporter: String,
    /// The library or drive to export, or (for a set's or a mix's exporter) whose tracks to give
    /// it.
    #[serde(default)]
    drive: String,
    /// Only these playlists (and folders), by path; every one when empty.
    #[serde(default)]
    playlists: Vec<String>,
    /// History lists to put in, by name.
    #[serde(default)]
    history: Vec<String>,
    /// A set's exporter's set: its id, title or day, or `last` (as `sets.get` takes it).
    #[serde(default)]
    set: Option<String>,
    /// A mix's exporter's mix: the id of its report in the store (`mix.reports.list`).
    #[serde(default)]
    mix: Option<String>,
    /// The file, or folder, to write (absolute), on this computer: never on a drive or in the
    /// store.
    output: PathBuf,
    /// As the exporter's `options` schema says (`plugins.describe`).
    #[serde(default)]
    options: Value,
}

pub(super) fn register(r: &mut Registry) {
    r.plan("plugins.export.plan", export_plan).long().destructive().writes_to("output");
    r.apply("plugins.export.apply", PlanStore::Existing, |c, req: PlanPluginExport| -> Result<Exported, _> {
        let plan = export_plan(c, req)?;
        Ok(plan.apply(c.existing()?.as_ref(), c.ctx)?)
    })
    .long()
    .destructive()
    .writes_to("output");
}

fn export_plan(c: &Call<'_>, req: PlanPluginExport) -> Result<Planned<PluginExport>, RpcError> {
    let source = ExportSource {
        drive: req.drive,
        playlists: req.playlists,
        history: req.history,
        set: req.set,
        mix: req.mix,
        ..ExportSource::default()
    };
    let store = c.existing()?;
    Ok(host()?.export_plan(&req.plugin, &req.exporter, source, &req.output, req.options, store.as_ref(), c.ctx)?)
}
