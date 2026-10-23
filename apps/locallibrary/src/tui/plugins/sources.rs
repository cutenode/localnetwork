//! Library sources in the player: a drive built from a plugin's library source, planned (what it
//! reads in the folders granted, what's copied and removed) and confirmed before it's written.

use std::path::PathBuf;

use locallibrary::Store;
use locallibrary::collection_export::CollectionExportOptions;
use locallibrary::plugin::SourceExport;
use loclib_plugin::Manifest;
use serde_json::{Map, Value, json};

use crate::plugins::params::properties;
use crate::tui::app::App;
use crate::tui::edits::Modal;
use crate::tui::form::{For, Form};
use crate::tui::keymap::Act;
use crate::tui::palette::Item;

/// The form's own fields, beside the source's options: the folders granted, and whether to
/// replace a library the drive holds.
pub(super) const READ_FROM: &str = "read_from";
const REPLACE: &str = "replace";

/// `m`'s library sources as the palette and a drive's menu offer them, each unavailable for `why`.
pub(super) fn items(m: &'static Manifest, why: Option<&str>) -> Vec<(Act, Item)> {
    let sources = m.contributes.sources.iter().map(|s| {
        let label = format!("build from {}…", super::named(&s.title));
        (Act::Source(&m.id, &s.name), Item { unavailable: why.map(String::from), ..Item::new(label, &m.name) })
    });
    sources.collect()
}

impl App {
    /// Build the drive at `root` from a plugin's library source: a form asks for the folders it
    /// may read files in (`~/Music` to begin with) and for the source's options first.
    pub(in crate::tui) fn plugin_source(&mut self, plugin: &'static str, source: &'static str, root: Option<PathBuf>) {
        let Ok(plugins) = crate::plugins::host() else { return };
        let Some(root) = self.plugged(root) else { return };
        let declared =
            plugins.manifest(plugin).ok().and_then(|m| m.contributes.sources.iter().find(|s| s.name == source));
        let Some(s) = declared else { return };
        let mut fields = Map::new();
        let folders = "Folders it may read files in, on this computer";
        let read_from =
            json!({ "type": "array", "items": { "type": "string" }, "x-path": true, "description": folders });
        fields.insert(READ_FROM.into(), read_from);
        let replace = "Replace a library the drive holds that wasn't built from it";
        fields.insert(REPLACE.into(), json!({ "type": "boolean", "description": replace }));
        fields.extend(properties(s.options.as_ref()).map(|(k, v)| (k.clone(), v.clone())));
        let mut schema = s.options.clone().unwrap_or_else(|| json!({ "type": "object" }));
        schema["properties"] = Value::Object(fields);
        let mut required = vec![Value::from(READ_FROM)];
        required.extend(schema["required"].as_array().cloned().unwrap_or_default());
        schema["required"] = Value::Array(required);
        let filled = Map::from_iter([(READ_FROM.to_string(), json!(["~/Music"]))]);
        let form = Form::with(s.title.clone(), Some(schema), &filled, For::Source { plugin, source, root });
        self.modal = Some(Modal::Form(form));
    }

    /// Plan the build the form has (`given`: the folders, whether to replace, and the source's
    /// options) in the background: the plugin reads its library, and the plan comes back to
    /// confirm, as any change does.
    pub(in crate::tui) fn plan_source(
        &mut self,
        plugin: &'static str,
        source: &'static str,
        root: PathBuf,
        given: Value,
    ) {
        let Ok(plugins) = crate::plugins::host() else { return };
        let mut options = given;
        let mut take = |key: &str| options.as_object_mut().and_then(|o| o.remove(key));
        let grants = take(READ_FROM).and_then(|v| serde_json::from_value::<Vec<PathBuf>>(v).ok()).unwrap_or_default();
        let replace = take(REPLACE) == Some(Value::Bool(true));
        let export = CollectionExportOptions { replace, ..CollectionExportOptions::default() };
        let drive = root.display().to_string();
        let req = SourceExport { plugin: plugin.into(), source: source.into(), options, grants, drive, export };
        let store_root = self.store_root.clone();
        self.plan(root, move |ctx| {
            let store = Store::open(&store_root).ok();
            Ok(Box::new(plugins.plan_source_export(&req, store.as_ref(), ctx)?))
        });
    }
}
