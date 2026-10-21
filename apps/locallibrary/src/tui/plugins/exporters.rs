//! Exports in the player: a drive's library in a plugin's format, planned and confirmed
//! (overwriting asked about) before the plugin writes it.

use std::path::PathBuf;

use locallibrary::Store;
use loclib_plugin::{ExportSource, Manifest};
use serde_json::{Map, Value, json};

use crate::plugins::params::properties;
use crate::tui::app::App;
use crate::tui::edits::Modal;
use crate::tui::form::{For, Form};
use crate::tui::keymap::Act;
use crate::tui::palette::Item;

/// The form's own field an export's output is typed in, beside the format's options.
pub(super) const WRITE_TO: &str = "write_to";

/// `m`'s export formats as the palette and a drive's menu offer them, each unavailable for `why`.
pub(super) fn items(m: &'static Manifest, why: Option<&str>) -> Vec<(Act, Item)> {
    let formats = m.contributes.exporters.iter().map(|e| {
        let item =
            Item { unavailable: why.map(String::from), ..Item::new(format!("export for {}…", e.format), &m.name) };
        (Act::Export(&m.id, &e.format), item)
    });
    formats.collect()
}

impl App {
    /// Export the drive at `root` in a plugin's format (the playlists marked in the tree only, when
    /// it's the drive open and some are, as rekordbox XML is): a form asks where (a file or folder
    /// on this computer, named after the drive) and for the format's options first.
    pub(in crate::tui) fn plugin_export(&mut self, plugin: &'static str, format: &'static str, root: Option<PathBuf>) {
        let Ok(plugins) = crate::plugins::host() else { return };
        let Some(root) = self.plugged(root) else { return };
        let exporter =
            plugins.manifest(plugin).ok().and_then(|m| m.contributes.exporters.iter().find(|e| e.format == format));
        let Some(e) = exporter else { return };
        let name = self.drive_label(&root);
        let (to, what) = match &e.extension {
            Some(ext) => (format!("~/Music/{name}.{ext}"), "The file to write, on this computer"),
            None => (format!("~/Music/{name} for {format}"), "The folder to write into, on this computer"),
        };
        let mut fields = Map::new();
        fields.insert(WRITE_TO.into(), json!({ "type": "string", "x-path": true, "description": what }));
        fields.extend(properties(e.options.as_ref()).map(|(k, v)| (k.clone(), v.clone())));
        let mut schema = e.options.clone().unwrap_or_else(|| json!({ "type": "object" }));
        schema["properties"] = Value::Object(fields);
        let mut required = vec![Value::from(WRITE_TO)];
        required.extend(schema["required"].as_array().cloned().unwrap_or_default());
        schema["required"] = Value::Array(required);
        let filled = Map::from_iter([(WRITE_TO.to_string(), Value::from(to))]);
        let open = self.loaded.as_ref().is_some_and(|l| l.root() == root);
        let playlists = if open { self.list_marks.clone() } else { Vec::new() };
        let then = For::Export { plugin, format, root, playlists };
        let form = Form::with(e.title.clone(), Some(schema), &filled, then);
        self.modal = Some(Modal::Form(form));
    }

    /// Plan the export the form has (`given`: where, and the format's options) in the background:
    /// the plugin says which files it would write, writing none, and the plan comes back to
    /// confirm, as any change does (overwriting files there already asked about).
    pub(in crate::tui) fn plan_export(
        &mut self,
        plugin: &'static str,
        format: &'static str,
        root: PathBuf,
        playlists: Vec<String>,
        given: Value,
    ) {
        let Ok(plugins) = crate::plugins::host() else { return };
        let mut options = given;
        let output = options.as_object_mut().and_then(|o| o.remove(WRITE_TO));
        let output = PathBuf::from(output.as_ref().and_then(Value::as_str).unwrap_or_default());
        let store_root = self.store_root.clone();
        let source = ExportSource { drive: root.display().to_string(), playlists, ..ExportSource::default() };
        self.plan(root, move |ctx| {
            let store = Store::open(&store_root).ok();
            Ok(Box::new(plugins.export_plan(plugin, format, source, &output, options, store.as_ref(), ctx)?))
        });
    }
}
