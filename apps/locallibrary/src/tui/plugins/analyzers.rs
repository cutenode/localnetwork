//! Analyzers in the player: run from the palette (or a drive's menu) on a drive's tracks, their
//! fields as columns to show, and writing an analyzer's key and BPM to the tracks (planned, then
//! confirmed).

use std::path::PathBuf;

use locallibrary::Store;
use locallibrary::fields::Role;
use locallibrary::plugin::ApplyFields;
use locallibrary::query::{FieldName, SortField};
use loclib_plugin::Manifest;
use serde_json::{Map, Value, json};

use super::named;
use crate::tui::app::{App, Fields, Level};
use crate::tui::edits::Modal;
use crate::tui::form::{For, Form};
use crate::tui::keymap::Act;
use crate::tui::palette::Item;

/// The roles an analyzer's values can be written to tracks as.
const WRITTEN: [Role; 2] = [Role::Key, Role::Bpm];

/// `m`'s analyzers as the palette and a drive's menu offer them, each unavailable for `why`: run,
/// and write its keys and BPMs when it has those roles.
pub(super) fn items(m: &'static Manifest, why: Option<&str>) -> Vec<(Act, Item)> {
    let mut items = Vec::new();
    for a in &m.contributes.analyzers {
        let label = format!("analyze the tracks: {}", named(&a.title));
        let item = Item { unavailable: why.map(String::from), ..Item::new(label, &m.name) };
        items.push((Act::Analyze(&m.id, &a.name), item));
        let written: Vec<&str> = (WRITTEN.iter().zip(["keys", "BPMs"]))
            .filter_map(|(r, what)| a.roles.contains_key(r).then_some(what))
            .collect();
        if !written.is_empty() {
            let label = format!("write {} to the tracks: {}…", written.join(" and "), named(&a.title));
            let item = Item { unavailable: why.map(String::from), ..Item::new(label, &m.name) };
            items.push((Act::ApplyFields(&m.id, &a.name), item));
        }
    }
    items
}

impl App {
    /// A column for each plugin's field, as the palette lists them (in the library): shown or
    /// hidden, with the plugin's name beside each.
    pub(in crate::tui) fn field_items(&self) -> Vec<(Act, Item)> {
        let Ok(plugins) = crate::plugins::host() else { return Vec::new() };
        let all = plugins.manifests().flat_map(|m| m.contributes.analyzers.iter().map(move |a| (m, a)));
        all.flat_map(|(m, a)| a.fields.keys().map(move |f| (m, FieldName::new(&format!("{}.{f}", m.id)))))
            .map(|(m, name)| {
                let doing = if self.field_columns.contains(&name) { "hide" } else { "show" };
                (Act::FieldColumn(name), Item::new(format!("{doing} the {} column", name.as_str()), &m.name))
            })
            .collect()
    }

    /// Show the column of a plugin's field, after the others, or hide it.
    pub(in crate::tui) fn toggle_field_column(&mut self, name: FieldName) {
        match self.field_columns.iter().position(|f| *f == name) {
            Some(i) => {
                self.field_columns.remove(i);
            }
            None => {
                self.field_columns.push(name);
                self.read_fields();
            }
        }
    }

    /// Run a plugin's analyzer on the tracks of the drive at `root` in the background (esc stops
    /// it), keeping what it works out in the store; then its columns, filters and sorts read it.
    pub(in crate::tui) fn analyze(&mut self, plugin: &'static str, analyzer: &'static str, root: Option<PathBuf>) {
        let Ok(plugins) = crate::plugins::host() else { return };
        let Some(root) = self.plugged(root) else { return };
        let Some(declared) =
            plugins.manifest(plugin).ok().and_then(|m| m.contributes.analyzers.iter().find(|a| a.name == analyzer))
        else {
            return;
        };
        let title = named(&declared.title);
        let store_root = self.store_root.clone();
        self.task("Analyzing".into(), move |ctx| {
            let store = Store::open_or_init(&store_root)?;
            let done = plugins.analyze(plugin, analyzer, &root.display().to_string(), None, &store, ctx)?;
            Ok(Box::new(move |app: &mut App| {
                let tracks = crate::output::plural(done.analyzed, "track");
                let mut said = format!("{title}: {tracks} analyzed, {} already", done.kept);
                if done.failed > 0 {
                    said.push_str(&format!(", {} failed (! says why)", done.failed));
                }
                let level = if done.failed > 0 && done.analyzed + done.kept == 0 { Level::Error } else { Level::Info };
                app.toast(said, level);
                // Read again, with what it worked out.
                if let Some(l) = app.loaded.as_mut().filter(|l| l.root() == root) {
                    l.fields = Fields::Unread;
                    l.ranks.retain(|(f, _)| !matches!(f, SortField::Field(_)));
                }
                app.refresh_visible();
                if !app.field_columns.is_empty() {
                    app.read_fields();
                }
            }))
        });
    }

    /// Write what a plugin's analyzer worked out to the tracks of the drive at `root`: a form asks
    /// which of its key and BPM (both, to start with), whether over what tracks have, and how sure
    /// it must be; then the plan comes back to confirm, as any edit's does.
    pub(in crate::tui) fn apply_fields(&mut self, plugin: &'static str, analyzer: &'static str, root: Option<PathBuf>) {
        let Ok(plugins) = crate::plugins::host() else { return };
        let Some(root) = self.plugged(root) else { return };
        let declared =
            plugins.manifest(plugin).ok().and_then(|m| m.contributes.analyzers.iter().find(|a| a.name == analyzer));
        let Some(a) = declared else { return };
        let has: Vec<&str> = WRITTEN.iter().filter(|r| a.roles.contains_key(*r)).map(|r| r.name()).collect();
        let schema = json!({
            "type": "object",
            "properties": {
                "fields": { "type": "array", "items": { "type": "string", "enum": has }, "description": "What to write" },
                "overwrite": { "type": "boolean", "description": "Also over what a track has" },
                "min_confidence": {
                    "type": "number",
                    "minimum": 0,
                    "maximum": 1,
                    "description": "Leave out what it's less sure of (0 to 1)"
                }
            },
            "required": ["fields"]
        });
        let filled = Map::from_iter([("fields".to_string(), json!(has))]);
        let title = format!("Write from {}", named(&a.title));
        let form = Form::with(title, Some(schema), &filled, For::ApplyFields { plugin, analyzer, root });
        self.modal = Some(Modal::Form(form));
    }

    /// Plan writing what the form has (`given`) from a plugin's analyzer to the tracks of the
    /// drive at `root`, in the background: the plan comes back to confirm.
    pub(in crate::tui) fn plan_apply_fields(
        &mut self,
        plugin: &'static str,
        analyzer: &'static str,
        root: PathBuf,
        given: Value,
    ) {
        let Ok(plugins) = crate::plugins::host() else { return };
        let store_root = self.store_root.clone();
        let drive = root.display().to_string();
        self.plan(root, move |ctx| {
            let fields: Vec<Role> = serde_json::from_value(given["fields"].clone()).unwrap_or_default();
            let req = ApplyFields {
                plugin: plugin.into(),
                analyzer: analyzer.into(),
                drive,
                fields,
                tracks: None,
                overwrite: given["overwrite"] == true,
                min_confidence: given["min_confidence"].as_f64(),
            };
            Ok(Box::new(plugins.plan_apply_fields(&req, &Store::open_or_init(&store_root)?, ctx)?))
        });
    }
}
