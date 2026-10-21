//! Panels (`panel/render`, the `panels` feature): what plugins show about a track, under its
//! details. A panel answers a view of the track, never a question; the views a command or a panel
//! answers are read here ([`Slot::view`]): the kinds the `views` feature brings only from a plugin
//! with it, and each image's file read from the request's scratch folder and given on as a PNG
//! `data:` URL.

use std::io::Read;

use base64::prelude::*;
use loclib_plugin::{Context, Panel, RenderPanel, View, method};
use serde_json::Value;

use super::Plugins;
use super::blobs::Scratch;
use super::process::{Live, Slot};
use crate::Result;
use crate::progress::Ctx;
use crate::store::Store;

impl Plugins {
    /// Every panel, with its plugin's id, in the order the app gave the plugins.
    pub fn panels(&self) -> Vec<(&str, &Panel)> {
        self.manifests().flat_map(|m| m.contributes.panels.iter().map(move |p| (m.id.as_str(), p))).collect()
    }

    /// The plugin's panel's view of `track` on `drive` (by id, name or path as the CLI takes it),
    /// its images read (see the module's doc).
    pub fn render_panel(
        &self,
        plugin: &str,
        panel: &str,
        drive: &str,
        track: u32,
        store: Option<&Store>,
        ctx: &Ctx<'_>,
    ) -> Result<View> {
        let slot = self.slot(plugin)?;
        slot.declared("panel", panel, &slot.manifest.contributes.panels, |p| &p.name)?;
        let context = Context { drive: Some(drive.into()), tracks: vec![track], ..Context::default() };
        let (context, _) = slot.context(context, store)?;
        let render = RenderPanel { panel: panel.into(), context };
        let read = |answer: Value, scratch: &Scratch, live: &Live| {
            let view = slot.view(panel, answer, scratch, live)?;
            match view.asks() {
                true => Err(slot.misbehaved(format!("its panel {panel} asks a question, which a panel can't"))),
                false => Ok(view),
            }
        };
        slot.call_with(method::RENDER_PANEL, &render, store, ctx, &|_| {}, read)
    }
}

impl Slot {
    /// The view the plugin answered (of `of`, a command or a panel), as hosts take it: one of the
    /// kinds the `views` feature brings (a tree, a chart, an image, sections, a typed column) only
    /// from a plugin with it, no question in a section, and each image its file in `scratch`, once
    /// it's an image, given on as a PNG `data:` URL.
    pub(crate) fn view(&self, of: &str, answer: Value, scratch: &Scratch, live: &Live) -> Result<View> {
        let mut view: View =
            serde_json::from_value(answer).map_err(|e| self.misbehaved(format!("its view of {of} isn't one: {e}")))?;
        if view.needs_views() && !live.has("views") {
            return Err(self.misbehaved("its view needs the views feature, which it doesn't have"));
        }
        if matches!(&view, View::Sections { .. }) && view.asks() {
            return Err(self.misbehaved("a section of its view asks a question"));
        }
        for image in view.images_mut() {
            let View::Image { blob, data, .. } = image else { continue };
            let mut bytes = Vec::new();
            let read = scratch.blob(blob).and_then(|mut f| f.read_to_end(&mut bytes).map_err(|e| e.to_string()));
            read.map_err(|why| self.misbehaved(format!("its image: {why}")))?;
            let png = crate::artwork::to_png(&bytes).map_err(|e| self.misbehaved(format!("its image {blob}: {e}")))?;
            *data = Some(format!("data:image/png;base64,{}", BASE64_STANDARD.encode(png)));
        }
        Ok(view)
    }
}
