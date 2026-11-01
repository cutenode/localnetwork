//! Checks in the player: what plugins' checks find shows with a check's results, and the fixes
//! they and locallibrary's own checks give are in the palette until the next check, each planned
//! and confirmed as any plugin operation, or the player's own flow, is. Which checks run, and how
//! long one has, is a form in the palette.

use std::path::PathBuf;

use locallibrary::Store;
use locallibrary::check::CheckReport;
use locallibrary::plugin::{
    CheckChange, Cli, Fix, MAX_TIMEOUT, MIN_TIMEOUT, PluginFix, Plugins, TIMEOUT, check_timeout, set_check_timeout,
};
use loclib_plugin::{Context, HostAction, ProviderKind, When};
use serde_json::{Map, Value, json};

use super::Run;
use crate::tui::app::{App, Level};
use crate::tui::edits::Modal;
use crate::tui::form::{For, Form};
use crate::tui::keymap::Act;
use crate::tui::palette::Item;

/// A fix a check gave, what it's about as the palette offers it, and who gave it (a plugin's
/// name; empty for locallibrary's own checks).
#[derive(Clone)]
pub struct Fixing {
    pub fix: Fixed,
    pub label: String,
    pub by: &'static str,
}

/// What a fix does on the drive it was found on.
#[derive(Clone)]
pub enum Fixed {
    /// A plugin's operation, with its params.
    Plugin(Run, Value),
    /// One of the player's own flows, on the drive at this root.
    Host(HostAction, PathBuf),
}

/// The fixes in `report`, of the drives at `roots` (in the order it checked them).
pub fn fixes(roots: &[PathBuf], report: &CheckReport) -> Vec<Fixing> {
    let fixing = report.items.iter().filter_map(|i| {
        let root = report.drives.iter().position(|d| *d == i.drive).and_then(|n| roots.get(n)).cloned();
        let label = format!("fix on {}: {}", i.drive, i.message);
        match i.fix.clone()? {
            Fix::Host(action) => Some(Fixing { fix: Fixed::Host(action, root?), label, by: "" }),
            Fix::Plugin(PluginFix { plugin, operation, params }) => {
                let m = crate::plugins::host().ok()?.manifest(&plugin).ok()?;
                let op = m.contributes.operations.iter().find(|o| o.name == operation)?;
                let run = Run { plugin: &m.id, hook: Cli::Operation(op), root, context: Context::default() };
                Some(Fixing { fix: Fixed::Plugin(run, params), label, by: &m.name })
            }
        }
    });
    fixing.collect()
}

/// The fixes the last check found, as the palette offers them.
pub(in crate::tui) fn items(app: &App) -> Vec<(Act, Item)> {
    app.fixes.iter().enumerate().map(|(i, f)| (Act::Fix(i), Item::new(f.label.clone(), f.by))).collect()
}

/// The names of the plugins with a check: none when there's none.
pub(super) fn checking(plugins: &Plugins) -> Option<String> {
    let by: Vec<&str> =
        plugins.manifests().filter(|m| !m.contributes.checks.is_empty()).map(|m| m.name.as_str()).collect();
    (!by.is_empty()).then(|| by.join(", "))
}

/// The check settings form's field for how long a check has to answer.
const TIMEOUT_FIELD: &str = "timeout";

/// The form's field for whether a check runs, and for a guard whether it stops changes.
fn on_field(plugin: &str, check: &str) -> String {
    format!("{plugin}.{check}")
}

fn blocking_field(plugin: &str, check: &str) -> String {
    format!("{plugin}.{check}.blocking")
}

impl App {
    /// Plans the fix `i` the last check found, on its drive, confirmed as any plan is: its
    /// plugin's operation, or the player's own flow (saving a snapshot, syncing the drive's
    /// library, fetching artwork for the tracks without any).
    pub(in crate::tui) fn fix(&mut self, i: usize) {
        let Some(Fixing { fix, .. }) = self.fixes.get(i).cloned() else { return };
        match fix {
            Fixed::Plugin(mut run, params) => {
                run.context.drive = run.root.as_deref().and_then(|r| self.plugin_drive(r));
                self.run_plugin(run, params);
            }
            Fixed::Host(HostAction::Pull(_), root) => self.pull(root),
            Fixed::Host(HostAction::Sync(_), root) => self.sync_group(root),
            Fixed::Host(HostAction::ArtFetch(f), root) => {
                let (kind, tracks) = (ProviderKind::Artwork, crate::output::plural(f.tracks.len(), "track"));
                let title = format!("Fetch {}: {tracks} on {}", kind.found(), self.drive_label(&root));
                self.fetch_form_from(kind, title, root, f.tracks, None);
            }
            // No check of the host's gives another.
            Fixed::Host(..) => {}
        }
    }

    /// Which checks run, whether each guard stops changes it finds something about, and how long
    /// a check has to answer (`plugin checks`), in a form as they're set now.
    pub(in crate::tui) fn check_settings(&mut self) {
        let Ok(plugins) = crate::plugins::host() else { return };
        let store = Store::open(&self.store_root).ok();
        let (mut properties, mut filled) = (Map::new(), Map::new());
        for (plugin, c, settings) in plugins.checks(store.as_ref()) {
            let by = plugins.manifest(plugin).map_or(plugin, |m| m.name.as_str());
            let field = on_field(plugin, &c.name);
            properties
                .insert(field.clone(), json!({ "type": "boolean", "description": format!("{} ({by})", c.title) }));
            filled.insert(field, settings.on.into());
            if c.when.contains(&When::Apply) {
                let field = blocking_field(plugin, &c.name);
                let description = format!("{}: stop changes it finds something about", c.title);
                properties.insert(field.clone(), json!({ "type": "boolean", "description": description }));
                filled.insert(field, settings.blocking.into());
            }
        }
        let description = format!("Seconds a check has to answer (empty for {})", TIMEOUT.as_secs());
        properties.insert(
            TIMEOUT_FIELD.into(),
            json!({ "type": "number", "minimum": MIN_TIMEOUT.as_secs_f64(), "maximum": MAX_TIMEOUT.as_secs(), "description": description }),
        );
        filled.insert(TIMEOUT_FIELD.into(), check_timeout(store.as_ref()).as_secs_f64().into());
        let schema = json!({ "type": "object", "properties": properties });
        self.modal = Some(Modal::Form(Form::with("Check settings".into(), Some(schema), &filled, For::CheckSettings)));
    }

    /// Keep the check settings as the form has them (`given`), in the background, kept where
    /// `plugin checks` keeps them: those that changed, and a timeout left empty back to its
    /// default.
    pub(in crate::tui) fn save_check_settings(&mut self, given: Value) {
        let Ok(plugins) = crate::plugins::host() else { return };
        let store_root = self.store_root.clone();
        self.task("Saving settings".into(), move |_| {
            let store = Store::open_or_init(&store_root)?;
            let mut changed = 0;
            for (plugin, c, now) in plugins.checks(Some(&store)) {
                let differs = |field: String, was: bool| given[field].as_bool().filter(|v| *v != was);
                let on = differs(on_field(plugin, &c.name), now.on);
                let blocking = differs(blocking_field(plugin, &c.name), now.blocking);
                if on.is_some() || blocking.is_some() {
                    plugins.set_check(&store, plugin, &c.name, CheckChange { on, blocking, reset: false })?;
                    changed += 1;
                }
            }
            let timeout = given[TIMEOUT_FIELD].as_f64();
            if timeout.unwrap_or(TIMEOUT.as_secs_f64()) != check_timeout(Some(&store)).as_secs_f64() {
                set_check_timeout(&store, timeout)?;
                changed += 1;
            }
            let said = if changed == 0 { "nothing changed" } else { "check settings saved" };
            Ok(Box::new(move |app: &mut App| app.toast(said, Level::Info)))
        });
    }
}
