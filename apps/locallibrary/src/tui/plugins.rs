//! Plugins in the player: their commands and operations in the palette, and in the menus their
//! manifests say (`where`: the actions menu, `e`, for the tracks selected or a playlist, and the
//! drive menu), a form for their params (see [`super::form`]), and each run in the background as
//! any task is (esc stops one). A command's answer shows in a panel;
//! an operation's plan is confirmed and applied as any edit is, and undone with `u`. A question
//! either asks first is a list to choose from, after which it runs again with the answer. Each
//! plugin's settings are a form, kept in the store, with a switch for each of its services, and
//! its accounts are signed in to and out of beside them. What the other families of hooks add
//! (cover art and tags from providers, analyzers, checks, exports, imports, what plugins tell the
//! user and suggest) is in a module each. Nothing here tells one
//! plugin from another: it's all drawn from their manifests, through the host the command line
//! uses (see [`crate::plugins`]).

mod accounts;
mod analyzers;
pub(super) mod checks;
mod events;
mod exporters;
pub(super) mod importers;
pub(super) mod panels;
pub(super) mod providers;
mod sources;
mod themes;

use std::path::{Path, PathBuf};

use locallibrary::Store;
use locallibrary::op::AnyPlanned;
use locallibrary::plugin::{Cli, CommandOutput, Offer, OpPlanned};
use loclib_plugin::{Capability, Choice, Context, Partial, Setting, SettingType, View, Where};
use serde_json::{Map, Value, json};

use super::app::{App, Level, View as Screen};
use super::drives::{Report, Tone};
use super::edits::{Modal, Target};
use super::form::{For, Form};
use super::keymap::Act;
use super::palette::{Item, Picker, Then};
use crate::plugin_cmd::view::{Drawn, Names};
use crate::plugins::params::{cell, properties};

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
    Offer { offer: Offer, choice: Choice, fetching: providers::Fetching },
}

impl Asking {
    pub fn choice(&self) -> &Choice {
        match self {
            Asking::Hook { choice, .. } | Asking::Offer { choice, .. } => choice,
        }
    }
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
/// operation (`<plugin>.<name>`), library source (`<plugin>.source.<source>`) or, with `export`,
/// export (`<plugin>.export.<format>`) it is; or
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
            (Some(name), false) => {
                let source = name.strip_prefix("source.").is_some_and(|s| c.sources.iter().any(|x| x.name == s));
                source || c.operations.iter().any(|o| o.name == name)
            }
            (None, _) => false,
        }
    });
    ours.map(|m| m.name.as_str())
}

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

/// A view a command (or a panel) answered, as a panel's lines: each kind as plain text (see
/// [`crate::plugin_cmd::view::lines`]), an item's detail under its label, a section's title as a
/// heading. `names` names the tracks of a `track` column.
pub fn view_lines(v: &View, names: Names<'_>) -> Vec<(Tone, String)> {
    let mut lines = Vec::new();
    for line in crate::plugin_cmd::view::lines(v, names) {
        match line {
            Drawn::Heading(title) => lines.push((Tone::Heading, title)),
            Drawn::Item { label, detail, depth } => {
                let indent = "  ".repeat(depth);
                lines.push((Tone::Text, format!("{indent}{label}")));
                lines.extend(detail.map(|d| (Tone::Faint, format!("{indent}  {d}"))));
            }
            Drawn::Columns(headings) => lines.push((Tone::Columns, headings)),
            Drawn::Row(row) => lines.push((Tone::Row, row)),
            Drawn::Text(text) => lines.extend(text.lines().map(|l| (Tone::Text, l.to_string()))),
            Drawn::Missing => lines.push((Tone::Faint, "nothing to show".into())),
        }
    }
    lines
}

impl App {
    /// The open library's track `id` as it's named (`Artist - Title`), for a view's `track` cells.
    pub(super) fn track_name(&self, id: u32) -> Option<String> {
        self.loaded.as_ref()?.library.track(id).map(|t| t.display_name())
    }

    /// Whether plugins can run on the drive at `root`: one plugged in, rather than a folder
    /// opened by path. `None` without a drive.
    pub(super) fn pluggable(&self, root: Option<&Path>) -> Option<bool> {
        root.map(|root| !self.extra_roots.iter().any(|r| r == root))
    }

    /// Every plugin's commands and operations offered in a menu of one of `places`, analyzers,
    /// export and import formats, as the palette and the drive menu list them for the drive at
    /// `root`: by title, with the plugin's name beside each.
    pub(super) fn plugin_items(&self, root: Option<&Path>, places: &[Where]) -> Vec<(Act, Item)> {
        let drive = self.pluggable(root);
        let all = offered(drive, places).into_iter();
        let mut items: Vec<(Act, Item)> = all
            .map(|o| (Act::Plugin(o.plugin, o.name), Item { unavailable: o.unavailable, ..Item::new(o.label, o.by) }))
            .collect();
        if places.contains(&Where::Track) {
            let fetching = providers::fetches(drive).into_iter();
            items.extend(
                fetching.map(|(kind, label, by, why)| {
                    (Act::Fetch(kind), Item { unavailable: why, ..Item::new(label, by) })
                }),
            );
        }
        let Ok(plugins) = crate::plugins::host() else { return items };
        // An analyzer reads the tracks' audio files, an exporter the library, an importer's file is
        // about its tracks and a library source writes one: on a drive plugged in, as a command.
        let why = match drive {
            None => Some("open a drive first"),
            Some(false) => Some("only on a plugged-in drive"),
            Some(true) => None,
        };
        // Each family's, in the order the palette lists them (a family's placed as loclib-plugin's
        // `hooks` says).
        let set = self.open_set().is_some();
        for m in plugins.manifests() {
            items.extend(analyzers::items(m, why));
            items.extend(exporters::items(m, why, set));
            items.extend(importers::items(m, why));
            items.extend(sources::items(m, why));
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

    /// The settings of each plugin that has some (or a service) and signing in to and out of its
    /// accounts, then the providers' and the checks' settings when there are any, then the themes
    /// to draw in when a plugin has one, as the palette lists them, wherever it's open.
    pub(super) fn settings_items(&self) -> Vec<(Act, Item)> {
        let Ok(plugins) = crate::plugins::host() else { return Vec::new() };
        let mut items = Vec::new();
        for m in plugins.manifests() {
            let settings = (Act::PluginSettings(&m.id), Item::new(format!("{} plugin settings…", m.name), &m.name));
            let has_settings = !m.contributes.settings.is_empty() || !m.contributes.services.is_empty();
            items.extend(Some(settings).filter(|_| has_settings));
            items.extend(accounts::items(m));
        }
        let providers =
            providers::providing(plugins, None).map(|by| (Act::ProviderSettings, Item::new("provider settings…", by)));
        let checks = checks::checking(plugins).map(|by| (Act::CheckSettings, Item::new("check settings…", by)));
        items.extend(providers);
        items.extend(checks);
        items.extend(themes::items());
        items
    }

    /// A plugin's settings in a form, each filled in as it's set now (else its default), and a
    /// switch for each of its services.
    pub(super) fn plugin_settings(&mut self, plugin: &'static str) {
        let Ok(plugins) = crate::plugins::host() else { return };
        let (Ok(m), Ok(listed)) = (plugins.manifest(plugin), plugins.setting_list(plugin)) else { return };
        let store = Store::open(&self.store_root).ok();
        let now = plugins.settings(plugin, store.as_ref()).unwrap_or_default();
        let filled: Map<String, Value> = now.into_iter().collect();
        let schema = Some(settings_schema(&listed));
        let form = Form::with(format!("{} settings", m.name), schema, &filled, For::Settings(plugin));
        self.modal = Some(Modal::Form(form));
    }

    /// Keep a plugin's settings as the form has them (`given`), in the background: those that
    /// changed, each left empty put back to its default, and each secret typed in (one left
    /// empty is kept). A service turned on starts, one turned off stops.
    pub(super) fn save_settings(&mut self, plugin: &'static str, given: Value) {
        let Ok(plugins) = crate::plugins::host() else { return };
        let (Ok(m), Ok(listed)) = (plugins.manifest(plugin), plugins.setting_list(plugin)) else { return };
        let store_root = self.store_root.clone();
        self.task("Saving settings".into(), move |_| {
            let store = Store::open_or_init(&store_root)?;
            let now = plugins.settings(plugin, Some(&store))?;
            let mut changed = 0;
            for s in &listed {
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
            if changed > 0 && !m.contributes.services.is_empty() {
                plugins.run_services(Some(&store));
            }
            let said = match changed {
                0 => "nothing changed".to_string(),
                _ => format!("{} settings saved", m.name),
            };
            Ok(Box::new(move |app: &mut App| app.toast(said, Level::Info)))
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

    /// What's selected, as a plugin's hook takes it: the tracks (by id) and the playlist (not a
    /// folder) the actions menu works on, and the set open.
    fn context(&self, t: &Target) -> Context {
        let l = self.loaded.as_ref().filter(|l| l.root() == t.root);
        let ids = l.map(|l| t.tracks.iter().filter_map(|p| l.index_of(p)).map(|i| l.library.tracks[i].id).collect());
        let playlist = t.node.clone().filter(|(_, folder)| !folder).map(|(path, _)| path);
        Context { playlist, tracks: ids.unwrap_or_default(), set: self.open_set(), ..Context::default() }
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
            Asking::Offer { offer, fetching, .. } => self.answer_offer(offer, fetching, values),
        }
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
            CommandOutput::View(v) => {
                let names = |id| self.track_name(id);
                (v.title().map_or(title, String::from), view_lines(&v, &names))
            }
        };
        self.modal = Some(Modal::Report(Report { title, lines, scroll: 0 }));
    }
}

#[cfg(test)]
mod tests;
