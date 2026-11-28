//! Keys made for a kind of keyboard, shown as an import is (what changes, each marked) before
//! they're taken.

use indexmap::IndexMap;
use locallibrary::keybinds::Keybinds;

use super::super::keymap::Preset;

/// The keys of `preset`.
pub fn keys(preset: Preset) -> Keybinds {
    let spelled = |keys: &[&str]| keys.iter().map(|k| k.to_string()).collect::<Vec<_>>();
    let keys: IndexMap<String, Vec<String>> = match preset {
        // A French keyboard types these on the digit row without shift. `&` goes to the
        // Library view, so the jobs have no key.
        Preset::Azerty => [
            ("view.library", spelled(&["&"])),
            ("view.prepare", spelled(&["é"])),
            ("view.drives", spelled(&["\""])),
            ("view.recordings", spelled(&["'"])),
            ("view.localnetwork", spelled(&["("])),
            ("deck.hot-cue", spelled(&["&", "é", "\"", "'", "(", "-", "è", "_"])),
            ("player.jobs", Vec::new()),
        ]
        .into_iter()
        .map(|(id, keys)| (id.to_string(), keys))
        .collect(),
    };
    Keybinds { keys, ..Keybinds::default() }
}

/// What `preset`'s list is titled.
pub fn title(preset: Preset) -> &'static str {
    match preset {
        Preset::Azerty => "Keys for an AZERTY keyboard",
    }
}
