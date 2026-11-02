//! Keys the user binds to plugins' commands and operations: each one's `keys.<name>` plugin
//! setting (`locallibrary plugin settings ID keys.NAME z`, or the plugin's settings form). The
//! player's own keys always win: a key it uses anywhere ([`KEYMAP`]), or one already bound to
//! another plugin's, is refused, saying why.

use locallibrary::Store;
use locallibrary::plugin::KEYS;
use loclib_plugin::Manifest;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use serde_json::Value;

use super::{Act, K, KEYMAP};
use crate::tui::app::{App, Level};

/// A key bound to a plugin's command or operation.
pub type Bound = (K, Act);

/// `spec` as a key: one character (`z`, `Z`, `%`), `ctrl-` and a letter, or `f1` to `f12`. Why
/// not, when it's none of them.
pub fn parse(spec: &str) -> Result<K, String> {
    let spec = spec.trim();
    let lower = spec.to_ascii_lowercase();
    let mut chars = spec.chars();
    let one = chars.next().filter(|c| !c.is_whitespace() && !c.is_control() && chars.next().is_none());
    let ctrl = lower.strip_prefix("ctrl-").or_else(|| lower.strip_prefix("ctrl+"));
    let f = lower.strip_prefix('f').and_then(|n| n.parse::<u8>().ok()).filter(|n| (1..=12).contains(n));
    match (one, ctrl, f) {
        (Some(c), ..) => Ok(K::Ch(c)),
        (_, Some(c), _) if c.len() == 1 && c.chars().all(|c| c.is_ascii_lowercase()) => {
            Ok(K::Ctrl(c.chars().next().unwrap_or_default()))
        }
        (_, _, Some(n)) => Ok(K::Code(KeyCode::F(n))),
        _ => Err(format!("{spec:?} isn't a key: a character, ctrl- and a letter, or f1 to f12")),
    }
}

/// How `key` is pressed.
fn pressed(key: K) -> KeyEvent {
    match key {
        K::Ch(c) if c.is_uppercase() => KeyEvent::new(KeyCode::Char(c), KeyModifiers::SHIFT),
        K::Ch(c) => KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE),
        K::Ctrl(c) => KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL),
        K::Code(c) | K::Shift(c) => KeyEvent::new(c, KeyModifiers::NONE),
        K::Digits(a, _) => KeyEvent::new(KeyCode::Char(a), KeyModifiers::NONE),
    }
}

/// `spec` bound to `act`, unless it isn't a key, the player uses it, or `bound` has it already:
/// then why not (naming the command as `named`).
pub fn bind(bound: &[Bound], spec: &str, act: Act, named: &str) -> Result<Bound, String> {
    let key = parse(spec).map_err(|why| format!("{named}: {why}"))?;
    let press = pressed(key);
    if let Some(b) = KEYMAP.iter().find(|b| b.keys.iter().any(|k| k.matches(&press))) {
        let what = [b.name, b.help].into_iter().find(|s| !s.is_empty()).unwrap_or(b.shown);
        return Err(format!("{named}: {} is the player's ({what})", spec.trim()));
    }
    if bound.iter().any(|(k, _)| k.matches(&press)) {
        return Err(format!("{named}: {} is bound to another command", spec.trim()));
    }
    Ok((key, act))
}

/// What `key` is bound to in `bound`.
pub fn lookup(bound: &[Bound], key: &KeyEvent) -> Option<Act> {
    bound.iter().find(|(k, _)| k.matches(key)).map(|(_, act)| *act)
}

/// `m`'s commands and operations, as each is acted on, its setting for a key, and how a refusal
/// names it.
fn hooks(m: &'static Manifest) -> impl Iterator<Item = (Act, String, String)> {
    let c = &m.contributes;
    let commands = c.commands.iter().map(|x| (&x.name, &x.title));
    let all = commands.chain(c.operations.iter().map(|x| (&x.name, &x.title)));
    all.map(move |(name, title)| (Act::Plugin(&m.id, name), format!("{KEYS}{name}"), format!("{title} ({})", m.name)))
}

/// The key `settings` sets with `key`, when it sets one.
fn spec<'v>(settings: impl Fn(&str) -> Option<&'v Value>, key: &str) -> Option<&'v str> {
    settings(key).and_then(Value::as_str).filter(|s| !s.trim().is_empty())
}

impl App {
    /// Binds the keys set for plugins' commands and operations (see [`bind`]), saying why of each
    /// it refuses: the first of two the same is bound.
    pub(in crate::tui) fn bind_plugin_keys(&mut self) {
        let Ok(plugins) = crate::plugins::host() else { return };
        let store = Store::open(&self.store_root).ok();
        let mut bound = Vec::new();
        for m in plugins.manifests() {
            let Ok(settings) = plugins.settings(&m.id, store.as_ref()) else { continue };
            for (act, key, named) in hooks(m) {
                let Some(spec) = spec(|k| settings.get(k), &key) else { continue };
                match bind(&bound, spec, act, &named) {
                    Ok(b) => bound.push(b),
                    Err(why) => self.toast(why, Level::Error),
                }
            }
        }
        self.plugin_keys = bound;
    }

    /// The settings of `plugin`'s keys `given` (its settings form) sets that are refused (see
    /// [`bind`], with the other plugins' keys bound now), each said: they're left as they were.
    pub(in crate::tui) fn refused_keys(&mut self, plugin: &str, given: &Value) -> Vec<String> {
        let Ok(m) = crate::plugins::host().and_then(|p| p.manifest(plugin)) else { return Vec::new() };
        let mut bound: Vec<Bound> = self
            .plugin_keys
            .iter()
            .filter(|(_, act)| !matches!(act, Act::Plugin(p, _) if *p == plugin))
            .copied()
            .collect();
        let mut refused = Vec::new();
        for (act, key, named) in hooks(m) {
            let Some(spec) = spec(|k| given.get(k), &key) else { continue };
            match bind(&bound, spec, act, &named) {
                Ok(b) => bound.push(b),
                Err(why) => {
                    self.toast(why, Level::Error);
                    refused.push(key);
                }
            }
        }
        refused
    }
}

#[cfg(test)]
mod tests;
