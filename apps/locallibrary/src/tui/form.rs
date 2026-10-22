//! A form for a plugin's params, its settings (or providers'), an export's, or fetching from
//! providers: a field for each property of a JSON Schema, each typed as the command line takes it
//! (see [`crate::plugins::params`]), and checked against the schema before anything runs or is
//! saved. A switch and a choice of a few values are picked rather than typed.

use loclib_plugin::{Selection, check_params, is_path};
use ratatui::crossterm::event::{KeyCode, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Position;
use serde_json::{Map, Value};

use super::app::{App, edit_line};
use super::edits::Modal;
use super::plugins::Run;
use crate::plugins::params::{self, Takes, cell, properties, takes};

pub struct Form {
    pub title: String,
    pub fields: Vec<Field>,
    pub selected: usize,
    /// Why what's filled in won't do.
    pub error: Option<String>,
    /// The JSON Schema of what's filled in.
    schema: Option<Value>,
    /// What it's filled in for.
    pub then: For,
}

/// What a form is filled in for.
pub enum For {
    /// A plugin's command or operation's params.
    Run(Run),
    /// The settings of the plugin with this id.
    Settings(&'static str),
    /// The settings of every provider.
    ProviderSettings,
    /// Where to export the drive at `root` (its `playlists` only, when some are named), or the
    /// `set`, in a plugin's format, and the format's options.
    Export {
        plugin: &'static str,
        format: &'static str,
        root: std::path::PathBuf,
        playlists: Vec<String>,
        set: Option<String>,
    },
    /// What file to import into the drive at `root` with a plugin's importer, and the format's
    /// options.
    Import { plugin: &'static str, format: &'static str, root: std::path::PathBuf },
    /// What to write to the tracks of the drive at `root` from what a plugin's analyzer worked
    /// out.
    ApplyFields { plugin: &'static str, analyzer: &'static str, root: std::path::PathBuf },
    /// Which providers of a kind to ask about `tracks` (by id) on the drive at `root`, and whether
    /// over what they have.
    Fetch { kind: loclib_plugin::ProviderKind, root: std::path::PathBuf, tracks: Vec<u32> },
}

pub struct Field {
    /// The param's name.
    pub name: String,
    /// Its JSON Schema.
    pub schema: Value,
    pub required: bool,
    /// What's typed or picked: `true` or `false` for a switch, an enum's choice, or "" for none.
    pub text: String,
}

impl Field {
    /// What it's called: its name in words (`skip analysis`).
    pub fn label(&self) -> String {
        self.name.replace('_', " ")
    }

    /// A secret, a password say (`writeOnly`): what's typed is never shown.
    pub fn is_secret(&self) -> bool {
        self.schema["writeOnly"] == true
    }

    /// A boolean: on or off.
    pub fn is_switch(&self) -> bool {
        takes(&self.schema) == Some(Takes::Switch)
    }

    /// What it's picked from instead of typed: a switch's values, or an enum's choices (and none,
    /// when it can be left out).
    pub fn choices(&self) -> Vec<String> {
        if self.is_switch() {
            return vec!["false".into(), "true".into()];
        }
        let Some(choices) = self.schema["enum"].as_array() else { return Vec::new() };
        let none = (!self.required).then(String::new);
        none.into_iter().chain(choices.iter().map(cell)).collect()
    }

    /// A switch's value, or an enum's choice, `by` places on (back when negative).
    fn pick(&mut self, by: isize) {
        let choices = self.choices();
        let at = choices.iter().position(|c| *c == self.text).unwrap_or(0) as isize;
        if let Some(next) = choices.get((at + by).rem_euclid(choices.len().max(1) as isize) as usize) {
            self.text = next.clone();
        }
    }
}

impl Form {
    /// The form for `run`'s params, each field filled in with what's selected when it's from the
    /// selection (`x-from`) and something is, else with its default (none when it takes none).
    pub fn new(run: Run) -> Form {
        let schema = run.params().cloned();
        let selected = |s: &Value| Selection::of(s).ok().flatten()?.value(&run.context);
        let filled: Map<String, Value> =
            properties(schema.as_ref()).filter_map(|(name, s)| Some((name.clone(), selected(s)?))).collect();
        Form::with(run.title().to_string(), schema, &filled, For::Run(run))
    }

    /// The form for what `schema` describes, each field filled in as `filled` has it, else with
    /// its default (a choice that can be left out with none, a switch off).
    pub fn with(title: String, schema: Option<Value>, filled: &Map<String, Value>, then: For) -> Form {
        let required: Vec<&str> = (schema.as_ref().and_then(|s| s["required"].as_array()).into_iter().flatten())
            .filter_map(Value::as_str)
            .collect();
        let fields: Vec<Field> = properties(schema.as_ref())
            .map(|(name, schema)| {
                let required = required.contains(&name.as_str());
                let mut field = Field { name: name.clone(), schema: schema.clone(), required, text: String::new() };
                field.text = match filled.get(name).or(schema.get("default")) {
                    Some(value) => cell(value),
                    None => field.choices().first().cloned().unwrap_or_default(),
                };
                field
            })
            .collect();
        Form { title, fields, selected: 0, error: None, schema, then }
    }

    /// The params filled in, or which field won't do and why: one that's needed is empty, one
    /// isn't of its type, or doesn't fit the schema. Lists and pairs are typed with commas
    /// between them, and paths may start with `~`. One from the selection (`x-from`) left empty
    /// is null, none: the host would fill it in from the selection otherwise.
    pub fn params(&self) -> Result<Value, (usize, String)> {
        let mut given = Map::new();
        let mut cleared = Vec::new();
        for (i, f) in self.fields.iter().enumerate() {
            let text = f.text.trim();
            if text.is_empty() {
                if f.required {
                    return Err((i, format!("{} is needed", f.label())));
                }
                if matches!(Selection::of(&f.schema), Ok(Some(_))) {
                    cleared.push(f.name.clone());
                }
                continue;
            }
            let raws: Vec<String> = match takes(&f.schema) {
                Some(Takes::Many(_) | Takes::Pairs(_)) => {
                    text.split(',').map(str::trim).filter(|r| !r.is_empty()).map(String::from).collect()
                }
                _ => vec![text.to_string()],
            };
            let raws: Vec<String> = match is_path(&f.schema) {
                true => raws.iter().map(|r| super::mix::typed_path(r).display().to_string()).collect(),
                false => raws,
            };
            let raws: Vec<&str> = raws.iter().map(String::as_str).collect();
            given.insert(f.name.clone(), params::value(&f.schema, &raws, &f.label()).map_err(|e| (i, e))?);
        }
        check_params(self.schema.as_ref(), &Value::Object(given.clone())).map_err(|e| {
            // The field the schema's complaint is about (`/into`), named as it's shown.
            let path = e.data.and_then(|d| d.fields.get("path").and_then(Value::as_str).map(String::from));
            let at = path.as_deref().and_then(|p| p.trim_start_matches('/').split('/').next());
            let field = at.and_then(|name| self.fields.iter().position(|f| f.name == name));
            // Said as the form's own: without the protocol's "invalid params: " and the pointer.
            let said = e.message.strip_prefix("invalid params: ").unwrap_or(&e.message);
            let said = path.as_deref().and_then(|p| said.strip_prefix(&format!("{p}: "))).unwrap_or(said);
            match field {
                Some(i) => (i, format!("{}: {said}", self.fields[i].label())),
                None => (self.selected, said.to_string()),
            }
        })?;
        given.extend(cleared.into_iter().map(|name| (name, Value::Null)));
        Ok(Value::Object(given))
    }
}

impl App {
    /// Keys while a form is open: the arrows (or tab) move between fields, typing fills one in,
    /// space and ←→ pick a switch's or a choice's value, enter runs (or saves) it and esc closes
    /// it.
    pub(super) fn on_form_key(&mut self, mut f: Form, key: KeyEvent) {
        let n = f.fields.len();
        let field = &mut f.fields[f.selected];
        let picked = !field.choices().is_empty();
        match key.code {
            KeyCode::Esc => return,
            KeyCode::Enter => match f.params() {
                Ok(params) => {
                    return match f.then {
                        For::Run(run) => self.run_plugin(run, params),
                        For::Settings(plugin) => self.save_settings(plugin, params),
                        For::ProviderSettings => self.save_provider_settings(params),
                        For::Export { plugin, format, root, playlists, set } => {
                            self.plan_export(plugin, format, root, playlists, set, params)
                        }
                        For::Import { plugin, format, root } => self.plan_import(plugin, format, root, params),
                        For::ApplyFields { plugin, analyzer, root } => {
                            self.plan_apply_fields(plugin, analyzer, root, params)
                        }
                        For::Fetch { kind, root, tracks } => self.fetch(kind, root, tracks, params),
                    };
                }
                Err((i, why)) => (f.selected, f.error) = (i, Some(why)),
            },
            KeyCode::Down | KeyCode::Tab => f.selected = (f.selected + 1) % n,
            KeyCode::Up | KeyCode::BackTab => f.selected = (f.selected + n - 1) % n,
            KeyCode::Right | KeyCode::Char(' ') if picked => field.pick(1),
            KeyCode::Left if picked => field.pick(-1),
            _ if !picked && edit_line(&mut field.text, key) => f.error = None,
            _ => {}
        }
        self.modal = Some(Modal::Form(f));
    }

    /// Clicks while a form is open: a field is chosen; a click outside closes the form.
    pub(super) fn on_form_mouse(&mut self, mut f: Form, m: MouseEvent) {
        let r = self.areas.menu;
        if m.kind != MouseEventKind::Down(MouseButton::Left) {
            self.modal = Some(Modal::Form(f));
            return;
        }
        if !r.contains(Position::new(m.column, m.row)) {
            return;
        }
        // The fields are the first rows, under the border.
        let row = m.row.saturating_sub(r.y + 1) as usize;
        if m.row > r.y && row < f.fields.len() {
            f.selected = row;
        }
        self.modal = Some(Modal::Form(f));
    }
}

#[cfg(test)]
mod tests;
