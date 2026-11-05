//! The colours and icons to draw in, as the palette offers them, for a terminal that says the wrong
//! thing of itself. The one chosen is kept in tui.json; `LOCALLIBRARY_COLOR` and
//! `LOCALLIBRARY_ICONS` still win.

use super::app::{App, Level};
use super::icons::{self, IconSet};
use super::keymap::Act;
use super::palette::Item;
use super::theme::{self, Tier};

/// The choices, as the palette lists them.
pub(super) fn items() -> Vec<(Act, Item)> {
    let tiers = [
        (None, "colours as the terminal says"),
        (Some(Tier::Truecolor), "24-bit colour"),
        (Some(Tier::Colors256), "256 colours"),
        (Some(Tier::Colors16), "16 colours"),
    ];
    let sets = [
        (None, "icons for this terminal"),
        (Some(IconSet::Nerd), "Nerd Font icons"),
        (Some(IconSet::Unicode), "Unicode icons"),
        (Some(IconSet::Ascii), "ASCII icons"),
    ];
    let tiers = tiers.into_iter().map(|(tier, label)| (Act::Colours(tier), Item::new(label, "Player")));
    tiers.chain(sets.into_iter().map(|(set, label)| (Act::Icons(set), Item::new(label, "Player")))).collect()
}

impl App {
    /// Draw in the colour tier chosen, from the next frame, and keep it for next time.
    pub(super) fn use_colours(&mut self, tier: Option<Tier>) {
        theme::choose_tier(tier);
        self.toast(format!("drawn in {} colours", theme::tier().name()), Level::Info);
    }

    /// Draw in the icons chosen, from the next frame, and keep them for next time.
    pub(super) fn use_icons(&mut self, set: Option<IconSet>) {
        icons::choose(set);
        self.toast(format!("drawn in {} icons", icons::set().name()), Level::Info);
    }
}

#[cfg(test)]
mod tests;
