//! Imports in the player: another program's file read with a plugin's importer, what it says
//! added to the drive's tracks and the plays it logged kept as sessions, planned and confirmed as
//! any change is.

use std::path::PathBuf;

use locallibrary::Store;
use loclib_plugin::Manifest;
use serde_json::{Map, Value};

use super::exporters::with_path;
use crate::tui::app::App;
use crate::tui::edits::Modal;
use crate::tui::form::{For, Form};
use crate::tui::keymap::Act;
use crate::tui::palette::Item;

/// The form's own field the file to import is typed in, beside the format's options.
pub(super) const READ_FROM: &str = "read_from";

/// `m`'s import formats as the palette and a drive's menu offer them, each unavailable for `why`.
pub(super) fn items(m: &'static Manifest, why: Option<&str>) -> Vec<(Act, Item)> {
    let formats = m.contributes.importers.iter().map(|i| {
        let item = Item {
            unavailable: why.map(String::from),
            ..Item::new(format!("import {}…", super::named(&i.title)), &m.name)
        };
        (Act::Import(&m.id, &i.format), item)
    });
    formats.collect()
}

/// The plugin whose import `operation` (as a plan is called, `<plugin>.import.<format>`) is, by
/// its name.
pub(in crate::tui) fn imported_by(operation: &str) -> Option<&'static str> {
    let plugins = crate::plugins::host().ok()?;
    let mut all = plugins.manifests();
    let ours = all.find(|m| {
        let rest = operation.strip_prefix(m.id.as_str()).and_then(|r| r.strip_prefix(".import."));
        rest.is_some_and(|f| m.contributes.importers.iter().any(|i| i.format == f))
    });
    ours.map(|m| m.name.as_str())
}

impl App {
    /// Import a file into the drive at `root` with a plugin's importer: a form asks for the file
    /// (on this computer) and the format's options first.
    pub(in crate::tui) fn plugin_import(&mut self, plugin: &'static str, format: &'static str, root: Option<PathBuf>) {
        let Ok(plugins) = crate::plugins::host() else { return };
        let Some(root) = self.drive_open(root) else { return };
        let importer =
            plugins.manifest(plugin).ok().and_then(|m| m.contributes.importers.iter().find(|i| i.format == format));
        let Some(i) = importer else { return };
        let what = match &i.extension {
            Some(ext) => format!("The .{ext} file to read, on this computer"),
            None => "The file to read, on this computer".into(),
        };
        let schema = with_path(READ_FROM, &what, i.options.as_ref());
        let form = Form::with(i.title.clone(), Some(schema), &Map::new(), For::Import { plugin, format, root });
        self.modal = Some(Modal::Form(form));
    }

    /// Plan the import the form has (`given`: the file, and the format's options) in the
    /// background: the plan comes back to confirm, as any change does.
    pub(in crate::tui) fn plan_import(
        &mut self,
        plugin: &'static str,
        format: &'static str,
        root: PathBuf,
        given: Value,
    ) {
        let Ok(plugins) = crate::plugins::host() else { return };
        let mut options = given;
        let file = options.as_object_mut().and_then(|o| o.remove(READ_FROM));
        let file = PathBuf::from(file.as_ref().and_then(Value::as_str).unwrap_or_default());
        let (store_root, drive) = (self.store_root.clone(), root.display().to_string());
        self.plan(root, move |ctx| {
            // Sessions are kept in the store, made for them if there's none yet.
            let store = Store::open_or_init(&store_root).ok();
            Ok(Box::new(plugins.import_plan(plugin, format, &file, &drive, options, store.as_ref(), ctx)?))
        });
    }
}
