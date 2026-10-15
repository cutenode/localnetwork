//! Plugins in the player: their commands in the palette and the drive menu, a form for their
//! params (see [`super::form`]), run in the background as any task is (esc stops one), and what
//! they answer in a panel. Nothing here tells one plugin from another: it's all drawn from their
//! manifests, through the host the command line uses (see [`crate::plugins`]).

use std::path::{Path, PathBuf};

use locallibrary::Store;
use locallibrary::plugin::CommandOutput;
use loclib_plugin::{Capability, Command, Context, View};
use serde_json::Value;

use super::app::{App, Level, View as Screen};
use super::drives::{Report, Tone};
use super::edits::Modal;
use super::form::Form;
use super::keymap::Act;
use super::palette::Item;
use crate::plugin_cmd::view::{pairs, table};

/// A plugin's command, and where it runs: the drive, and what's selected there.
pub struct Run {
    pub plugin: &'static str,
    pub command: &'static Command,
    pub context: Context,
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
    /// Every plugin's commands, as the palette and the drive menu list them for the drive at
    /// `root`: by title, with the plugin's name beside each. Those of plugins that read a drive
    /// can't run without one, nor on a drive opened as a folder, which a plugin can't name.
    pub(super) fn plugin_commands(&self, root: Option<&Path>) -> Vec<(Act, Item)> {
        let Ok(plugins) = crate::plugins::host() else { return Vec::new() };
        let why = match root {
            None => Some("open a drive first"),
            Some(root) if self.extra_roots.iter().any(|r| r == root) => Some("only on a plugged-in drive"),
            Some(_) => None,
        };
        let all = plugins.manifests().flat_map(|m| m.contributes.commands.iter().map(move |c| (m, c)));
        all.map(|(m, c)| {
            let reads = m.capabilities.iter().any(reads_drive);
            let unavailable = why.filter(|_| reads).map(String::from);
            (Act::Plugin(&m.id, &c.name), Item { unavailable, ..Item::new(&c.title, &m.name) })
        })
        .collect()
    }

    /// Where the user is, as a plugin's hook takes it: the drive actions work on, and in the
    /// library the playlist shown and the tracks marked or selected (by id).
    fn context(&self) -> Context {
        let drive = self.action_drive().map(|r| r.display().to_string());
        let target = (self.view == Screen::Library).then(|| self.target().zip(self.loaded.as_ref())).flatten();
        let Some((t, l)) = target else { return Context { drive, ..Context::default() } };
        let tracks = t.tracks.iter().filter_map(|p| l.index_of(p)).map(|i| l.library.tracks[i].id).collect();
        Context { drive, playlist: t.list, tracks, ..Context::default() }
    }

    /// A plugin's command, from the palette (where the user is) or the drive menu (on the drive
    /// at `root`): its form first when it takes params.
    pub(super) fn plugin_act(&mut self, plugin: &'static str, name: &'static str, root: Option<PathBuf>) {
        let manifest = crate::plugins::host().and_then(|p| p.manifest(plugin));
        let Some(command) = manifest.ok().and_then(|m| m.contributes.commands.iter().find(|c| c.name == name)) else {
            return;
        };
        let context = match root {
            Some(root) => Context { drive: Some(root.display().to_string()), ..Context::default() },
            None => self.context(),
        };
        let form = Form::new(Run { plugin, command, context });
        match form.fields.is_empty() {
            true => self.run_plugin(form.run, Value::Null),
            false => self.modal = Some(Modal::Form(form)),
        }
    }

    /// Run a plugin's command with `params` in the background; what it answers shows once it's
    /// done.
    pub(super) fn run_plugin(&mut self, run: Run, params: Value) {
        let Ok(plugins) = crate::plugins::host() else { return };
        let Run { plugin, command, context } = run;
        let store_root = self.store_root.clone();
        let title = command.title.clone();
        self.task(title.clone(), move |ctx| {
            let store = Store::open(&store_root).ok();
            let answer = plugins.run_command(plugin, &command.name, params, context, store.as_ref(), ctx)?;
            Ok(Box::new(move |app: &mut App| app.show_answer(title, answer)))
        });
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
