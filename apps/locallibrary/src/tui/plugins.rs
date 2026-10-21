//! Plugins in the player: their commands and operations in the palette, and in the menus their
//! manifests say (`where`: the actions menu, `e`, for the tracks selected or a playlist, and the
//! drive menu), a form for their params (see [`super::form`]), and each run in the background as
//! any task is (esc stops one). A command's answer shows in a panel;
//! an operation's plan is confirmed and applied as any edit is, and undone with `u`. A question
//! either asks first is a list to choose from, after which it runs again with the answer. Cover
//! art and tags providers find for the tracks selected are offered the same way, track by track,
//! one alone sure enough picked already, and the edits planned and confirmed. Analyzers run
//! from the palette too, as does writing an analyzer's key and BPM to the tracks (planned, then
//! confirmed); exports are planned and confirmed (overwriting asked about) before the plugin
//! writes them, and each plugin's settings are a form, kept in the store. Nothing here
//! tells one plugin from another: it's all drawn from their manifests, through the host the
//! command line uses (see [`crate::plugins`]).

use std::collections::VecDeque;
use std::path::{Path, PathBuf};

use indexmap::IndexMap;
use locallibrary::Store;
use locallibrary::fields::Role;
use locallibrary::op::AnyPlanned;
use locallibrary::plugin::{
    AUTO_PICK, ApplyFields, Cli, CommandOutput, Offer, OpPlanned, Pick, PlanProvided, Plugins, Provide, Provided,
    auto_pick, plan_provided, set_auto_pick,
};
use locallibrary::query::TrackSelector;
use loclib_plugin::{
    Capability, Choice, Context, ExportSource, Partial, ProviderKind, Setting, SettingType, View, Where,
};
use serde_json::{Map, Value, json};

use super::app::Fields;
use super::app::{App, Level, View as Screen};
use super::drives::{Report, Tone};
use super::edits::{Modal, Target};
use super::form::{For, Form};
use super::keymap::Act;
use super::palette::{Item, Picker, Then};
use crate::plugin_cmd::view::{pairs, table};
use crate::plugins::params::{cell, properties};
use locallibrary::query::{FieldName, SortField};

/// A plugin's command or operation, and where it runs: the drive, and what's selected there (its
/// context's drive is the drive's).
#[derive(Clone)]
pub struct Run {
    pub plugin: &'static str,
    pub hook: Cli<'static>,
    pub root: Option<PathBuf>,
    pub context: Context,
}

impl Run {
    /// Its title, as its manifest has it.
    pub fn title(&self) -> &'static str {
        match self.hook {
            Cli::Command(c) => &c.title,
            Cli::Operation(o) => &o.title,
        }
    }

    /// The JSON Schema of its params.
    pub fn params(&self) -> Option<&'static Value> {
        match self.hook {
            Cli::Command(c) => c.params.as_ref(),
            Cli::Operation(o) => o.params.as_ref(),
        }
    }
}

/// A plugin's question being asked, and what answering it goes on to.
pub enum Asking {
    /// A command's or an operation's: it runs again with the answer.
    Hook { run: Run, params: Value, choice: Choice },
    /// Which of the candidates providers found for a track: then the next track's, then the plan.
    Offer { offer: Offer, choice: Choice, fetching: Fetching },
}

impl Asking {
    pub fn choice(&self) -> &Choice {
        match self {
            Asking::Hook { choice, .. } | Asking::Offer { choice, .. } => choice,
        }
    }
}

/// Fetching from providers for the tracks of the drive at `root`: the picks so far, the tracks
/// left to ask about, and the images found, by id.
pub struct Fetching {
    kind: ProviderKind,
    root: PathBuf,
    replace: bool,
    picks: Vec<Pick>,
    left: VecDeque<Offer>,
    images: IndexMap<String, String>,
}

/// What providers fetch for the tracks selected, as a menu offers it on a drive (see
/// [`offered`]): each kind some plugin provides, with its providers' plugins' names, and why it
/// can't run here, if it can't.
pub fn fetches(drive: Option<bool>) -> Vec<(ProviderKind, String, String, Option<String>)> {
    let Ok(plugins) = crate::plugins::host() else { return Vec::new() };
    let why = match drive {
        None => Some("open a drive first"),
        Some(false) => Some("only on a plugged-in drive"),
        Some(true) => None,
    };
    let kinds = [ProviderKind::Artwork, ProviderKind::Metadata];
    let found = kinds.into_iter().filter_map(|kind| {
        let by = providing(plugins, Some(kind))?;
        Some((kind, format!("fetch {}…", kind.found()), by, why.map(String::from)))
    });
    found.collect()
}

/// The names of the plugins with a provider (of `kind`, when it says), each once: none when
/// there's none.
fn providing(plugins: &Plugins, kind: Option<ProviderKind>) -> Option<String> {
    let mut by: Vec<&str> = Vec::new();
    for (plugin, _) in plugins.providers(kind) {
        let name = plugins.manifest(plugin).map_or(plugin, |m| m.name.as_str());
        if !by.contains(&name) {
            by.push(name);
        }
    }
    (!by.is_empty()).then(|| by.join(", "))
}

/// Every place a command or an operation may be offered: the palette offers them all.
pub const EVERYWHERE: [Where; 3] = [Where::Track, Where::Playlist, Where::Drive];

/// A plugin's command or operation, as a menu offers it.
pub struct Offered {
    pub plugin: &'static str,
    pub name: &'static str,
    /// Where its manifest offers it.
    pub offered: &'static [Where],
    /// Its title as the player's own are named (see [`named`]), ending in … when it asks for
    /// params first.
    pub label: String,
    /// Its plugin's name.
    pub by: &'static str,
    /// Why it can't run here, if it can't.
    pub unavailable: Option<String>,
}

/// Plugins' commands and operations offered in a menu of one of `places`, as it offers them on a
/// drive: `drive` is `None` without one, `Some(false)` for a folder opened by path, which a plugin
/// can't name. An operation always needs a drive; a command, when its plugin reads one.
pub fn offered(drive: Option<bool>, places: &[Where]) -> Vec<Offered> {
    let Ok(plugins) = crate::plugins::host() else { return Vec::new() };
    let why = match drive {
        None => Some("open a drive first"),
        Some(false) => Some("only on a plugged-in drive"),
        Some(true) => None,
    };
    let mut all = Vec::new();
    for m in plugins.manifests() {
        let c = &m.contributes;
        let reads = m.capabilities.iter().any(reads_drive);
        let hooks = c.commands.iter().map(|x| (&x.name, &x.title, x.params.as_ref(), &x.offered, reads));
        let hooks = hooks.chain(c.operations.iter().map(|x| (&x.name, &x.title, x.params.as_ref(), &x.offered, true)));
        for (name, title, params, offered, needs_drive) in hooks {
            if !offered.iter().any(|w| places.contains(w)) {
                continue;
            }
            let asks = if properties(params).next().is_some() { "…" } else { "" };
            all.push(Offered {
                plugin: &m.id,
                name,
                offered,
                label: format!("{}{asks}", named(title)),
                by: &m.name,
                unavailable: why.filter(|_| needs_drive).map(String::from),
            });
        }
    }
    all
}

/// A title as the player names its own commands: its first letter lowercase, unless it starts a
/// name in capitals (`M3U`).
fn named(title: &str) -> String {
    let mut chars = title.chars();
    match (chars.next(), chars.next()) {
        (Some(first), Some(second)) if !second.is_uppercase() && !second.is_numeric() => {
            first.to_lowercase().chain(title.chars().skip(1)).collect()
        }
        _ => title.to_string(),
    }
}

/// Who planned `operation` (as a plan is called), if plugins did: the name of the plugin whose
/// operation (`<plugin>.<name>`) or, with `export`, export (`<plugin>.export.<format>`) it is; or
/// `Plugins` for an edit the host plans from what plugins found or worked out (`plugins.provide`,
/// `plugins.apply_fields`).
pub fn planned_by(operation: &str, export: bool) -> Option<&'static str> {
    if !export && operation.starts_with("plugins.") {
        return Some("Plugins");
    }
    let mut all = crate::plugins::host().ok()?.manifests();
    let ours = all.find(|m| {
        let c = &m.contributes;
        match (operation.strip_prefix(m.id.as_str()).and_then(|rest| rest.strip_prefix('.')), export) {
            (Some(rest), true) => {
                rest.strip_prefix("export.").is_some_and(|f| c.exporters.iter().any(|e| e.format == f))
            }
            (Some(name), false) => c.operations.iter().any(|o| o.name == name),
            (None, _) => false,
        }
    });
    ours.map(|m| m.name.as_str())
}

/// The form's own field an export's output is typed in, beside the format's options.
const WRITE_TO: &str = "write_to";

/// The providers' settings form's field for the score a candidate is picked for the user at.
const AUTO_PICK_FIELD: &str = "auto_pick";

/// The roles an analyzer's values can be written to tracks as.
const WRITTEN: [Role; 2] = [Role::Key, Role::Bpm];

/// Settings as a form's JSON Schema: each as text, a number, a switch, one of its choices, a
/// path or a password, said with its title and its default.
fn settings_schema(settings: &[Setting]) -> Value {
    let properties: Map<String, Value> = settings
        .iter()
        .map(|s| {
            let mut p = match s.kind {
                SettingType::Number => json!({ "type": "number" }),
                SettingType::Boolean => json!({ "type": "boolean" }),
                SettingType::Enum => json!({ "type": "string", "enum": s.choices }),
                SettingType::File | SettingType::Directory => json!({ "type": "string", "x-path": true }),
                SettingType::String => json!({ "type": "string" }),
                // Typed as a password is (see `super::form::Field::is_secret`).
                SettingType::Secret => json!({ "type": "string", "writeOnly": true }),
            };
            let default = match (s.kind, &s.default) {
                // Never filled in: it's kept as it is unless something's typed.
                (SettingType::Secret, _) => " (empty keeps it)".to_string(),
                (_, Some(d)) => format!(" (empty for {})", cell(d)),
                (_, None) => String::new(),
            };
            p["description"] = Value::from(format!("{}{default}", s.title));
            (s.key.clone(), p)
        })
        .collect();
    json!({ "type": "object", "properties": properties })
}

/// Whether a plugin with `capability` reads a drive: it names one when it calls back.
fn reads_drive(capability: &Capability) -> bool {
    use Capability::*;
    matches!(
        capability,
        LibraryRead
            | AnalysisRead
            | ArtworkRead
            | DriveInfo
            | HealthRead
            | SettingsRead
            | PlaysRead
            | SetsRead
            | CollectionRead
    )
}

/// A view a command answered, as a panel's lines: a list's items (each detail under its label),
/// a table's rows under its headings, labelled values aligned, or text.
pub fn view_lines(v: &View) -> Vec<(Tone, String)> {
    let mut lines = Vec::new();
    match v {
        View::List { items, .. } => {
            for i in items {
                lines.push((Tone::Text, i.label.clone()));
                lines.extend(i.detail.as_ref().map(|d| (Tone::Faint, format!("  {d}"))));
            }
        }
        View::Table { columns, rows, .. } => {
            let mut table = table(columns, rows).into_iter();
            lines.extend(table.next().map(|headings| (Tone::Columns, headings)));
            lines.extend(table.map(|row| (Tone::Row, row)));
            if rows.is_empty() {
                lines.push((Tone::Faint, "nothing to show".into()));
            }
        }
        View::Kv { pairs: kv, .. } => lines.extend(pairs(kv).into_iter().map(|p| (Tone::Text, p))),
        View::Text { text, .. } => lines.extend(text.lines().map(|l| (Tone::Text, l.to_string()))),
        View::Choose(_) => unreachable!("a question is asked, never drawn"),
    }
    if lines.is_empty() {
        lines.push((Tone::Faint, "nothing to show".into()));
    }
    lines
}

impl App {
    /// Whether plugins can run on the drive at `root`: one plugged in, rather than a folder
    /// opened by path. `None` without a drive.
    pub(super) fn pluggable(&self, root: Option<&Path>) -> Option<bool> {
        root.map(|root| !self.extra_roots.iter().any(|r| r == root))
    }

    /// Every plugin's commands and operations offered in a menu of one of `places`, analyzers and
    /// export formats, as the palette and the drive menu list them for the drive at `root`: by
    /// title, with the plugin's name beside each.
    pub(super) fn plugin_items(&self, root: Option<&Path>, places: &[Where]) -> Vec<(Act, Item)> {
        let drive = self.pluggable(root);
        let all = offered(drive, places).into_iter();
        let mut items: Vec<(Act, Item)> = all
            .map(|o| (Act::Plugin(o.plugin, o.name), Item { unavailable: o.unavailable, ..Item::new(o.label, o.by) }))
            .collect();
        if places.contains(&Where::Track) {
            let fetching = fetches(drive).into_iter();
            items.extend(
                fetching.map(|(kind, label, by, why)| {
                    (Act::Fetch(kind), Item { unavailable: why, ..Item::new(label, by) })
                }),
            );
        }
        let Ok(plugins) = crate::plugins::host() else { return items };
        // An analyzer reads the tracks' audio files and an exporter the library: on a drive plugged
        // in, as a command.
        let why = match drive {
            None => Some("open a drive first"),
            Some(false) => Some("only on a plugged-in drive"),
            Some(true) => None,
        };
        for m in plugins.manifests() {
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
            for e in &m.contributes.exporters {
                let item = Item {
                    unavailable: why.map(String::from),
                    ..Item::new(format!("export for {}…", e.format), &m.name)
                };
                items.push((Act::Export(&m.id, &e.format), item));
            }
        }
        items
    }

    /// The drive at `root`, when plugins can run on it; else `None`, saying why.
    fn plugged(&mut self, root: Option<PathBuf>) -> Option<PathBuf> {
        match (self.pluggable(root.as_deref()), root) {
            (Some(true), Some(root)) => Some(root),
            (None, _) => {
                self.toast("open a drive first", Level::Info);
                None
            }
            _ => {
                self.toast("only on a plugged-in drive", Level::Info);
                None
            }
        }
    }

    /// Export the drive at `root` in a plugin's format (the playlists marked in the tree only, when
    /// it's the drive open and some are, as rekordbox XML is): a form asks where (a file or folder
    /// on this computer, named after the drive) and for the format's options first.
    pub(super) fn plugin_export(&mut self, plugin: &'static str, format: &'static str, root: Option<PathBuf>) {
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
    pub(super) fn plan_export(
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

    /// The settings of each plugin that has some, then the providers' when there are any, as the
    /// palette lists them, wherever it's open.
    pub(super) fn settings_items(&self) -> Vec<(Act, Item)> {
        let Ok(plugins) = crate::plugins::host() else { return Vec::new() };
        let with = plugins.manifests().filter(|m| !m.contributes.settings.is_empty());
        let items =
            with.map(|m| (Act::PluginSettings(&m.id), Item::new(format!("{} plugin settings…", m.name), &m.name)));
        let providers = providing(plugins, None).map(|by| (Act::ProviderSettings, Item::new("provider settings…", by)));
        items.chain(providers).collect()
    }

    /// The providers' settings in a form, as they're set now: the score a candidate alone at or
    /// above is picked for the user with (`plugin providers --auto-pick`).
    pub(super) fn provider_settings(&mut self) {
        let store = Store::open(&self.store_root).ok();
        let description = format!("Pick a candidate alone scoring this or more, 0 to 1 (empty for {AUTO_PICK})");
        let field = json!({ "type": "number", "minimum": 0, "description": description });
        let schema = json!({ "type": "object", "properties": { AUTO_PICK_FIELD: field } });
        let filled = Map::from_iter([(AUTO_PICK_FIELD.to_string(), Value::from(auto_pick(store.as_ref())))]);
        let form = Form::with("Provider settings".into(), Some(schema), &filled, For::ProviderSettings);
        self.modal = Some(Modal::Form(form));
    }

    /// Keep the providers' settings as the form has them (`given`), in the background, kept where
    /// `plugin providers` keeps them; left empty, back to the default.
    pub(super) fn save_provider_settings(&mut self, given: Value) {
        let store_root = self.store_root.clone();
        let score = given[AUTO_PICK_FIELD].as_f64();
        self.task("Saving settings".into(), move |_| {
            let store = Store::open_or_init(&store_root)?;
            let said = match score.unwrap_or(AUTO_PICK) == auto_pick(Some(&store)) {
                true => "nothing changed",
                false => {
                    set_auto_pick(&store, score)?;
                    "provider settings saved"
                }
            };
            Ok(Box::new(move |app: &mut App| app.toast(said, Level::Info)))
        });
    }

    /// A plugin's settings in a form, each filled in as it's set now (else its default).
    pub(super) fn plugin_settings(&mut self, plugin: &'static str) {
        let Ok(plugins) = crate::plugins::host() else { return };
        let Ok(m) = plugins.manifest(plugin) else { return };
        let store = Store::open(&self.store_root).ok();
        let now = plugins.settings(plugin, store.as_ref()).unwrap_or_default();
        let filled: Map<String, Value> = now.into_iter().collect();
        let schema = Some(settings_schema(&m.contributes.settings));
        let form = Form::with(format!("{} settings", m.name), schema, &filled, For::Settings(plugin));
        self.modal = Some(Modal::Form(form));
    }

    /// Keep a plugin's settings as the form has them (`given`), in the background: those that
    /// changed, each left empty put back to its default, and each secret typed in (one left
    /// empty is kept).
    pub(super) fn save_settings(&mut self, plugin: &'static str, given: Value) {
        let Ok(plugins) = crate::plugins::host() else { return };
        let Ok(m) = plugins.manifest(plugin) else { return };
        let store_root = self.store_root.clone();
        self.task("Saving settings".into(), move |_| {
            let store = Store::open_or_init(&store_root)?;
            let now = plugins.settings(plugin, Some(&store))?;
            let mut changed = 0;
            for s in &m.contributes.settings {
                let value = given.get(&s.key);
                let typed = value.and_then(Value::as_str).is_some_and(|v| !v.is_empty());
                if s.kind == SettingType::Secret && !typed {
                    continue;
                }
                if s.kind == SettingType::Secret || value.or(s.default.as_ref()) != now.get(&s.key) {
                    plugins.set_setting(plugin, &store, &s.key, value.cloned())?;
                    changed += 1;
                }
            }
            let said = match changed {
                0 => "nothing changed".to_string(),
                _ => format!("{} settings saved", m.name),
            };
            Ok(Box::new(move |app: &mut App| app.toast(said, Level::Info)))
        });
    }

    /// A column for each plugin's field, as the palette lists them (in the library): shown or
    /// hidden, with the plugin's name beside each.
    pub(super) fn field_items(&self) -> Vec<(Act, Item)> {
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
    pub(super) fn toggle_field_column(&mut self, name: FieldName) {
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
    pub(super) fn analyze(&mut self, plugin: &'static str, analyzer: &'static str, root: Option<PathBuf>) {
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

    /// The drive at `root` as the plugin host names it to a plugin (by id, else by name): what a
    /// param from the selection is filled in with, here as on the command line and in the
    /// sidecar.
    pub(super) fn plugin_drive(&self, root: &Path) -> Option<String> {
        let listed = self.drives.iter().find(|d| d.root == root);
        let listed = listed.map(|d| (d.id, d.name.clone().unwrap_or_else(|| d.volume_name.clone())));
        let open = || self.loaded.as_ref().filter(|l| l.root() == root).map(|l| (l.device.id(), l.device.name()));
        let (id, name) = listed.or_else(open)?;
        Some(id.map_or(name, |id| id.to_string()))
    }

    /// Write what a plugin's analyzer worked out to the tracks of the drive at `root`: a form asks
    /// which of its key and BPM (both, to start with), whether over what tracks have, and how sure
    /// it must be; then the plan comes back to confirm, as any edit's does.
    pub(super) fn apply_fields(&mut self, plugin: &'static str, analyzer: &'static str, root: Option<PathBuf>) {
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
    pub(super) fn plan_apply_fields(
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

    /// What's selected, as a plugin's hook takes it: the tracks (by id) and the playlist (not a
    /// folder) the actions menu works on.
    fn context(&self, t: &Target) -> Context {
        let l = self.loaded.as_ref().filter(|l| l.root() == t.root);
        let ids = l.map(|l| t.tracks.iter().filter_map(|p| l.index_of(p)).map(|i| l.library.tracks[i].id).collect());
        let playlist = t.node.clone().filter(|(_, folder)| !folder).map(|(path, _)| path);
        Context { playlist, tracks: ids.unwrap_or_default(), ..Context::default() }
    }

    /// A plugin's command or operation from the palette: on the drive actions work on, with what's
    /// selected in the library.
    pub(super) fn plugin_here(&mut self, plugin: &'static str, name: &'static str) {
        let target = (self.view == Screen::Library).then(|| self.target()).flatten();
        self.plugin_act(plugin, name, self.action_drive(), target.as_ref());
    }

    /// A plugin's command or operation, on the drive at `root` with what `target` selects: its
    /// form first when it takes params.
    pub(super) fn plugin_act(
        &mut self,
        plugin: &'static str,
        name: &'static str,
        root: Option<PathBuf>,
        target: Option<&Target>,
    ) {
        let Ok(m) = crate::plugins::host().and_then(|p| p.manifest(plugin)) else { return };
        let c = &m.contributes;
        let command = c.commands.iter().find(|x| x.name == name).map(Cli::Command);
        let Some(hook) = command.or_else(|| c.operations.iter().find(|x| x.name == name).map(Cli::Operation)) else {
            return;
        };
        let context = target.map(|t| self.context(t)).unwrap_or_default();
        let context = Context { drive: root.as_deref().and_then(|r| self.plugin_drive(r)), ..context };
        let run = Run { plugin, hook, root, context };
        match properties(run.params()).next() {
            None => self.run_plugin(run, Value::Null),
            Some(_) => self.modal = Some(Modal::Form(Form::new(run))),
        }
    }

    /// Run a plugin's command with `params` in the background, what it answers showing once it's
    /// done; or plan its operation, the plan to confirm showing once it's made. A question either
    /// asks first is a list to choose from: answered, it runs again with the answer.
    pub(super) fn run_plugin(&mut self, run: Run, params: Value) {
        let Ok(plugins) = crate::plugins::host() else { return };
        let again = run.clone();
        let Run { plugin, hook, root, context } = run;
        let context = Context { drive: root.as_ref().map(|r| r.display().to_string()), ..context };
        let store_root = self.store_root.clone();
        match hook {
            Cli::Command(command) => {
                let title = command.title.clone();
                self.task_showing(title.clone(), move |ctx, show| {
                    let store = Store::open(&store_root).ok();
                    let partial = |p: Partial| {
                        let title = title.clone();
                        show(Box::new(move |app: &mut App| app.show_partial(title, p)));
                    };
                    let name = &command.name;
                    let answer =
                        plugins.run_command(plugin, name, params.clone(), context, store.as_ref(), ctx, &partial)?;
                    Ok(Box::new(move |app: &mut App| match answer {
                        CommandOutput::View(View::Choose(choice)) => {
                            app.ask(Asking::Hook { run: again, params, choice })
                        }
                        answer => app.show_answer(title, answer),
                    }))
                });
            }
            Cli::Operation(op) => {
                let Some(root) = root else { return self.toast("open a drive first", Level::Info) };
                if self.unplugged(&root) {
                    return;
                }
                self.task("Planning".into(), move |ctx| {
                    let store = Store::open(&store_root).ok();
                    let planned = plugins.plan_op(plugin, &op.name, params.clone(), context, store.as_ref(), ctx);
                    Ok(Box::new(move |app: &mut App| match planned {
                        Ok(OpPlanned::Choose { choose }) => {
                            app.ask(Asking::Hook { run: again, params, choice: choose })
                        }
                        other => {
                            let plan = other.and_then(OpPlanned::plan).map(|p| Box::new(p) as Box<dyn AnyPlanned>);
                            app.planned(root, None, plan)
                        }
                    }))
                });
            }
        }
    }

    /// Ask a plugin's question: a list of its items to choose from (any of them, marked with
    /// space, when it takes several).
    pub(super) fn ask(&mut self, asking: Asking) {
        let c = asking.choice();
        let items = c.items.iter().map(|i| Item::new(i.label.clone(), i.detail.clone().unwrap_or_default()));
        let (title, items, multi) = (c.title.clone(), items.collect::<Vec<_>>(), c.multi);
        let n = items.len();
        let mut p = Picker::new(title, items, Then::Choose(Box::new(asking)));
        if multi {
            p.marks = Some(vec![false; n]);
        }
        self.modal = Some(Modal::Pick(p));
    }

    /// Go on with what was chosen for `asking`: the values of the items chosen.
    pub(super) fn answer(&mut self, asking: Asking, values: Vec<Value>) {
        match asking {
            Asking::Hook { run, params, choice } => self.run_plugin(run, choice.answered(params, values)),
            Asking::Offer { offer, mut fetching, .. } => {
                fetching.picks.extend(values.first().and_then(|v| offer.chosen(v)));
                self.next_offer(fetching)
            }
        }
    }

    /// What providers of `kind` find for the tracks `target` selects on the drive at `root`: a
    /// form asks which provider (any, to start with) and whether over what tracks have first.
    pub(super) fn fetch_form(&mut self, kind: ProviderKind, root: Option<PathBuf>, target: Option<&Target>) {
        let Ok(plugins) = crate::plugins::host() else { return };
        let Some(root) = self.plugged(root) else { return };
        let tracks = target.map(|t| self.context(t).tracks).unwrap_or_default();
        if tracks.is_empty() {
            return self.toast("no track selected", Level::Info);
        }
        let names: Vec<String> =
            plugins.providers(Some(kind)).into_iter().map(|(plugin, p)| format!("{plugin}.{}", p.name)).collect();
        let schema = json!({
            "type": "object",
            "properties": {
                "from": { "type": "string", "enum": names, "description": "Only from this provider" },
                "replace": { "type": "boolean", "description": "Also over what tracks have" }
            }
        });
        let title = format!("Fetch {}", kind.found());
        let form = Form::with(title, Some(schema), &Map::new(), For::Fetch { kind, root, tracks });
        self.modal = Some(Modal::Form(form));
    }

    /// Ask the providers of `kind` (the one `given` names, if it does) about `tracks` on the drive
    /// at `root`, in the background: what they find is offered track by track.
    pub(super) fn fetch(&mut self, kind: ProviderKind, root: PathBuf, tracks: Vec<u32>, given: Value) {
        let Ok(plugins) = crate::plugins::host() else { return };
        let store_root = self.store_root.clone();
        let replace = given["replace"] == true;
        let req = Provide {
            kind,
            drive: root.display().to_string(),
            tracks: TrackSelector { ids: tracks, ..TrackSelector::default() },
            from: given["from"].as_str().map(String::from),
            replace,
        };
        self.task(format!("Fetching {}", kind.found()), move |ctx| {
            let provided = plugins.provide(&req, Store::open(&store_root).ok().as_ref(), ctx)?;
            Ok(Box::new(move |app: &mut App| app.fetched(root, replace, provided)))
        });
    }

    /// What the providers found: their warnings said, then each track's candidates asked about in
    /// turn, but those picked already.
    pub(super) fn fetched(&mut self, root: PathBuf, replace: bool, provided: Provided) {
        let Provided { kind, asked, offers, images, warnings } = provided;
        for w in &warnings {
            self.toast(w.clone(), Level::Info);
        }
        let what = kind.found();
        let why = if warnings.is_empty() { "" } else { " (! says why)" };
        if asked == 0 {
            return self.toast(format!("the tracks selected have {what} already"), Level::Info);
        }
        if offers.is_empty() {
            return self
                .toast(format!("no {what} found for {}{why}", crate::output::plural(asked, "track")), Level::Info);
        }
        let (picked, left): (Vec<Offer>, Vec<Offer>) = offers.into_iter().partition(|o| o.picked.is_some());
        let picks = picked.iter().filter_map(Offer::pick).collect();
        self.next_offer(Fetching { kind, root, replace, picks, left: left.into(), images });
    }

    /// The next track to ask about, or once there's none the plan of the picks, to confirm.
    fn next_offer(&mut self, mut f: Fetching) {
        if let Some(offer) = f.left.pop_front() {
            let choice = offer.choice();
            return self.ask(Asking::Offer { offer, choice, fetching: f });
        }
        if f.picks.is_empty() {
            return self.toast(format!("no {} chosen", f.kind.found()), Level::Info);
        }
        let (drive, images) = (f.root.display().to_string(), f.images);
        let req = PlanProvided { kind: f.kind, drive, picks: f.picks, images, replace: f.replace };
        self.plan(f.root, move |ctx| Ok(Box::new(plan_provided(&req, ctx)?)));
    }

    /// Rows (or items) a view command sent in parts, added to the panel it's in as they come
    /// (opened for the first): until it answers, then its view replaces them.
    pub(super) fn show_partial(&mut self, title: String, p: Partial) {
        let mut report = match self.modal.take() {
            Some(Modal::Report(r)) if r.title == title => r,
            None => Report { title, lines: Vec::new(), scroll: 0 },
            // Something else is open: the view shows once it answers.
            other => return self.modal = other,
        };
        let rows = p.rows.iter().map(|r| (Tone::Row, r.iter().map(cell).collect::<Vec<_>>().join("  ")));
        report.lines.extend(rows);
        for i in p.items {
            report.lines.push((Tone::Text, i.label));
            report.lines.extend(i.detail.map(|d| (Tone::Faint, format!("  {d}"))));
        }
        self.modal = Some(Modal::Report(report));
    }

    /// What a command answered: nothing or a line of text as a message, a view in a panel
    /// (titled as it says, else as the command is), and anything else in a panel as it is.
    pub(super) fn show_answer(&mut self, title: String, answer: CommandOutput) {
        let (title, lines) = match answer {
            CommandOutput::Headless(Value::Null) => return self.toast(format!("{title}: done"), Level::Info),
            CommandOutput::Headless(Value::String(s)) if !s.contains('\n') => return self.toast(s, Level::Info),
            CommandOutput::Headless(Value::String(s)) => (title, s.lines().map(|l| (Tone::Text, l.into())).collect()),
            CommandOutput::Headless(v) => {
                let text = serde_json::to_string_pretty(&v).unwrap_or_default();
                (title, text.lines().map(|l| (Tone::Text, l.into())).collect())
            }
            CommandOutput::View(v) => (v.title().map_or(title, String::from), view_lines(&v)),
        };
        self.modal = Some(Modal::Report(Report { title, lines, scroll: 0 }));
    }
}

#[cfg(test)]
mod tests;
