//! `plugins.panel`: a plugin's panel's view of a track.

use loclib_plugin::View;
use schemars::JsonSchema;
use serde::Deserialize;

use crate::plugins::host;
use crate::rpc::methods::Registry;

#[derive(Deserialize, JsonSchema)]
struct RenderPluginPanel {
    plugin: String,
    /// One of the plugin's panels (`plugins.describe`).
    panel: String,
    /// A library's name, or a drive's mount path, name or id prefix.
    drive: String,
    /// The track, by id.
    track: u32,
}

pub(super) fn register(r: &mut Registry) {
    r.read("plugins.panel", |c, req: RenderPluginPanel| -> Result<View, _> {
        let store = c.existing()?;
        Ok(host()?.render_panel(&req.plugin, &req.panel, &req.drive, req.track, store.as_ref(), c.ctx)?)
    })
    .long();
}
