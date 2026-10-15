//! Plugins in the player: their commands and operations in the palette and the drive menu
//! (operations in the actions menu, `e`, too), a form for their params (see [`super::form`]), and
//! each run in the background as any task is (esc stops one). A command's answer shows in a panel;
//! an operation's plan is confirmed and applied as any edit is, and undone with `u`. Nothing here
//! tells one plugin from another: it's all drawn from their manifests, through the host the
//! command line uses (see [`crate::plugins`]).

use std::path::{Path, PathBuf};

use locallibrary::Store;
use locallibrary::plugin::{Cli, CommandOutput};
use loclib_plugin::{Capability, Context, View};
use serde_json::Value;

use super::app::{App, Level, View as Screen};
use super::drives::{Report, Tone};
use super::edits::{Modal, Target};
use super::form::Form;
use super::keymap::Act;
use super::palette::Item;
use crate::plugin_cmd::view::{pairs, table};
use crate::plugins::params::properties;

/// A plugin's command or operation, and where it runs: the drive, and what's selected there (its
/// context's drive is the drive's).
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

/// A plugin's command or operation, as a menu offers it.
pub struct Offered {
    pub plugin: &'static str,
    pub name: &'static str,
    /// Its title as the player's own are named (see [`named`]), ending in … when it asks for
    /// params first.
    pub label: String,
    /// Its plugin's name.
    pub by: &'static str,
    /// Why it can't run here, if it can't.
    pub unavailable: Option<String>,
}

/// Plugins' commands (with `commands`) and operations, as the menus offer them on a drive:
/// `drive` is `None` without one, `Some(false)` for a folder opened by path, which a plugin can't
/// name. An operation always needs a drive; a command, when its plugin reads one.
pub fn offered(drive: Option<bool>, commands: bool) -> Vec<Offered> {
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
        let hooks = c.commands.iter().filter(|_| commands).map(|x| (&x.name, &x.title, x.params.as_ref(), reads));
        let hooks = hooks.chain(c.operations.iter().map(|x| (&x.name, &x.title, x.params.as_ref(), true)));
        for (name, title, params, needs_drive) in hooks {
            let asks = if properties(params).next().is_some() { "…" } else { "" };
            all.push(Offered {
                plugin: &m.id,
                name,
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

/// The name of the plugin whose operation `operation` (`<plugin>.<name>`, as its plan is called)
/// is, if it's a plugin's.
pub fn planned_by(operation: &str) -> Option<&'static str> {
    let mut all = crate::plugins::host().ok()?.manifests();
    let ours = all.find(|m| {
        let name = operation.strip_prefix(m.id.as_str()).and_then(|rest| rest.strip_prefix('.'));
        name.is_some_and(|name| m.contributes.operations.iter().any(|o| o.name == name))
    });
    ours.map(|m| m.name.as_str())
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

    /// Every plugin's commands and operations, as the palette and the drive menu list them for
    /// the drive at `root`: by title, with the plugin's name beside each.
    pub(super) fn plugin_items(&self, root: Option<&Path>) -> Vec<(Act, Item)> {
        let all = offered(self.pluggable(root), true).into_iter();
        all.map(|o| (Act::Plugin(o.plugin, o.name), Item { unavailable: o.unavailable, ..Item::new(o.label, o.by) }))
            .collect()
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
        let form = Form::new(Run { plugin, hook, root, context });
        match form.fields.is_empty() {
            true => self.run_plugin(form.run, Value::Null),
            false => self.modal = Some(Modal::Form(form)),
        }
    }

    /// Run a plugin's command with `params` in the background, what it answers showing once it's
    /// done; or plan its operation, the plan to confirm showing once it's made.
    pub(super) fn run_plugin(&mut self, run: Run, params: Value) {
        let Ok(plugins) = crate::plugins::host() else { return };
        let Run { plugin, hook, root, context } = run;
        let context = Context { drive: root.as_ref().map(|r| r.display().to_string()), ..context };
        let store_root = self.store_root.clone();
        match hook {
            Cli::Command(command) => {
                let title = command.title.clone();
                self.task(title.clone(), move |ctx| {
                    let store = Store::open(&store_root).ok();
                    let answer = plugins.run_command(plugin, &command.name, params, context, store.as_ref(), ctx)?;
                    Ok(Box::new(move |app: &mut App| app.show_answer(title, answer)))
                });
            }
            Cli::Operation(op) => {
                let Some(root) = root else { return self.toast("open a drive first", Level::Info) };
                self.plan(root, move |ctx| {
                    let store = Store::open(&store_root).ok();
                    Ok(Box::new(plugins.plan_op(plugin, &op.name, params, context, store.as_ref(), ctx)?))
                });
            }
        }
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
                let (View::List { title: own, .. }
                | View::Table { title: own, .. }
                | View::Kv { title: own, .. }
                | View::Text { title: own, .. }) = &v;
                (own.clone().unwrap_or(title), view_lines(&v))
            }
        };
        self.modal = Some(Modal::Report(Report { title, lines, scroll: 0 }));
    }
}

#[cfg(test)]
mod tests;
