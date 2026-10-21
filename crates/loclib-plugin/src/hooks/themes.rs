//! Themes: colours a plugin gives hosts to draw in, as data. No feature and no method: a host
//! reads them from the manifest and never runs the plugin for one.

use loclib_model::theme;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::manifest::{invalid, shown, unique_names};
use crate::{Manifest, ManifestError};

/// `[[contributes.themes]]`: a theme's colours, by the names of [`theme::TOKENS`] as tables (`cues.hot =
/// "#28e214"`, `ui = { accent = "#ff8800" }`), each `#rrggbb` (a gradient, `waveform.*`, a list
/// of two or more). What it leaves out is the default's.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Theme {
    pub name: String,
    pub title: String,
    pub tokens: Map<String, Value>,
}

impl Theme {
    /// Its colours over [`theme::Theme::DEFAULT`]'s, or which token is wrong and why
    /// (`cues.hot`, "isn't #rrggbb...").
    pub fn theme(&self) -> Result<theme::Theme, Wrong> {
        let mut t = theme::Theme::DEFAULT;
        walk(&self.tokens, "", &mut |name, colours| {
            let colours: Option<Vec<_>> = match colours {
                Value::String(one) => vec![theme::hex(one)].into_iter().collect(),
                Value::Array(all) => all.iter().map(|c| c.as_str().and_then(theme::hex)).collect(),
                _ => None,
            };
            let set = match colours {
                Some(colours) => t.set(&name, &colours),
                None => Err("isn't #rrggbb (lowercase), or a list of them".into()),
            };
            set.map_err(|why| (name, why))
        })?;
        Ok(t)
    }
}

/// A token that's wrong (`cues.hot`), and why.
pub type Wrong = (String, String);

/// Each value in `tokens` that isn't a table, by its dotted name below `at`.
fn walk(
    tokens: &Map<String, Value>,
    at: &str,
    each: &mut impl FnMut(String, &Value) -> Result<(), Wrong>,
) -> Result<(), Wrong> {
    for (key, value) in tokens {
        let name = if at.is_empty() { key.clone() } else { format!("{at}.{key}") };
        match value {
            Value::Object(below) => walk(below, &name, each)?,
            other => each(name, other)?,
        }
    }
    Ok(())
}

/// `[[contributes.themes]]`: names and titles, and colours named as a theme names them, of their
/// kind: one, or a gradient's.
pub(crate) fn validate(m: &Manifest) -> Result<(), ManifestError> {
    let (at, c) = ("contributes.themes", &m.contributes);
    unique_names(at, "name", c.themes.iter().map(|x| x.name.as_str()).enumerate())?;
    for (i, x) in c.themes.iter().enumerate() {
        shown(format!("{at}[{i}].title"), &x.title)?;
        if x.tokens.is_empty() {
            return Err(invalid(format!("{at}[{i}].tokens"), "a theme sets a colour at least"));
        }
        x.theme().map_err(|(name, why)| invalid(format!("{at}[{i}].tokens.{name}"), why))?;
    }
    Ok(())
}
