//! Plugins' themes in the player: the palette offers each (and the default, once a plugin has
//! one) to draw in, and the one chosen is kept in tui.json. Data alone: no plugin runs for one.

use super::super::app::{App, Level};
use super::super::keymap::Act;
use super::super::palette::Item;
use super::super::theme;

/// The themes to draw in, as the palette lists them, by title with whose each is: none until a
/// plugin has one.
pub(super) fn items() -> Vec<(Act, Item)> {
    let all = theme::themes();
    if all.len() < 2 {
        return Vec::new();
    }
    let item = |t: &'static theme::Named| {
        let by = if t.by.is_empty() { "Player" } else { &t.by };
        (Act::Theme(&t.key), Item::new(format!("{} theme", t.title), by))
    };
    all.iter().map(item).collect()
}

impl App {
    /// Draw in the theme `key` names (the default when it's empty), from the next frame, and keep
    /// it for next time.
    pub(in super::super) fn use_theme(&mut self, key: &'static str) {
        let chosen = theme::select(Some(key).filter(|k| !k.is_empty()));
        let title =
            theme::themes().iter().find(|t| t.key == chosen.unwrap_or_default()).map_or("default", |t| &t.title);
        self.toast(format!("drawn in the {title} theme"), Level::Info);
    }
}
