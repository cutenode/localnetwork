//! Exports in the player: a drive's library, or the set open (Browse › Sets), in a plugin's
//! format, planned and confirmed (overwriting asked about) before the plugin writes it. A mix's
//! formats are the command line's and the sidecar's.

use std::path::PathBuf;

use locallibrary::Store;
use loclib_plugin::{ExportFrom, ExportSource, Manifest};
use serde_json::{Map, Value, json};

use crate::plugins::params::properties;
use crate::tui::app::{App, View as Screen};
use crate::tui::browse::{Category, NodeId};
use crate::tui::edits::Modal;
use crate::tui::form::{For, Form};
use crate::tui::keymap::Act;
use crate::tui::palette::Item;

/// The form's own field an export's output is typed in, beside the format's options.
pub(super) const WRITE_TO: &str = "write_to";

/// `m`'s export formats as the palette and a drive's menu offer them, each unavailable for `why`:
/// a drive's, and a set's while one is open (`set`).
pub(super) fn items(m: &'static Manifest, why: Option<&str>, set: bool) -> Vec<(Act, Item)> {
    let formats = m.contributes.exporters.iter().filter_map(|e| {
        let (label, why) = match e.source {
            ExportFrom::Drive => (format!("export for {}…", e.format), why),
            ExportFrom::Set => {
                (format!("export the set for {}…", e.format), why.or((!set).then_some("open a set first")))
            }
            ExportFrom::Mix => return None,
        };
        let item = Item { unavailable: why.map(String::from), ..Item::new(label, &m.name) };
        Some((Act::Export(&m.id, &e.format), item))
    });
    formats.collect()
}

/// A form's schema for a file or folder on this computer (`field`, described as `what`) and then
/// the `options` a plugin's format takes.
pub(super) fn with_path(field: &str, what: &str, options: Option<&Value>) -> Value {
    let mut fields = Map::new();
    fields.insert(field.into(), json!({ "type": "string", "x-path": true, "description": what }));
    fields.extend(properties(options).map(|(k, v)| (k.clone(), v.clone())));
    let mut schema = options.cloned().unwrap_or_else(|| json!({ "type": "object" }));
    schema["properties"] = Value::Object(fields);
    let mut required = vec![Value::from(field)];
    required.extend(schema["required"].as_array().cloned().unwrap_or_default());
    schema["required"] = Value::Array(required);
    schema
}

impl App {
    /// The set open in the library, by id: what a set's export writes out, and what a plugin is
    /// told the user is on.
    pub(in crate::tui) fn open_set(&self) -> Option<String> {
        let l = self.loaded.as_ref().filter(|_| self.view == Screen::Library)?;
        match &l.node {
            NodeId::Group(Category::Sets, key) => Some(key.clone()),
            _ => None,
        }
    }

    /// Export the drive at `root` in a plugin's format (the playlists marked in the tree only, when
    /// it's the drive open and some are, as rekordbox XML is), or the set open for a set's format:
    /// a form asks where (a file or folder on this computer, named after the drive or the set) and
    /// for the format's options first.
    pub(in crate::tui) fn plugin_export(&mut self, plugin: &'static str, format: &'static str, root: Option<PathBuf>) {
        let Ok(plugins) = crate::plugins::host() else { return };
        let Some(root) = self.plugged(root) else { return };
        let exporter =
            plugins.manifest(plugin).ok().and_then(|m| m.contributes.exporters.iter().find(|e| e.format == format));
        let Some(e) = exporter else { return };
        let set = self.open_set().filter(|_| e.source == ExportFrom::Set);
        if e.source == ExportFrom::Set && set.is_none() {
            return self.toast("open a set first", crate::tui::app::Level::Info);
        }
        let label = |key: &String| {
            let group = self.loaded.as_ref().and_then(|l| l.grouped(Category::Sets, key).map(|g| g.label.clone()));
            group.unwrap_or_else(|| key.clone())
        };
        let name = set.as_ref().map_or_else(|| self.drive_label(&root), label);
        let (to, what) = match &e.extension {
            Some(ext) => (format!("~/Music/{name}.{ext}"), "The file to write, on this computer"),
            None => (format!("~/Music/{name} for {format}"), "The folder to write into, on this computer"),
        };
        let schema = with_path(WRITE_TO, what, e.options.as_ref());
        let filled = Map::from_iter([(WRITE_TO.to_string(), Value::from(to))]);
        // A set's format writes out the set, whatever lists are marked.
        let open = self.loaded.as_ref().is_some_and(|l| l.root() == root) && e.source == ExportFrom::Drive;
        let playlists = if open { self.list_marks.clone() } else { Vec::new() };
        let then = For::Export { plugin, format, root, playlists, set };
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
        set: Option<String>,
        given: Value,
    ) {
        let Ok(plugins) = crate::plugins::host() else { return };
        let mut options = given;
        let output = options.as_object_mut().and_then(|o| o.remove(WRITE_TO));
        let output = PathBuf::from(output.as_ref().and_then(Value::as_str).unwrap_or_default());
        let store_root = self.store_root.clone();
        let source = ExportSource { drive: root.display().to_string(), playlists, set, ..ExportSource::default() };
        self.plan(root, move |ctx| {
            let store = Store::open(&store_root).ok();
            Ok(Box::new(plugins.export_plan(plugin, format, source, &output, options, store.as_ref(), ctx)?))
        });
    }
}
