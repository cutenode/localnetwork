//! The keys someone set in the player: `keybinds.json`, beside what the player remembers
//! ([`crate::store::PLAYER_FILE`]). It holds only the actions whose keys differ from the player's
//! defaults, by the action's id (`playback.next`, `plugin.sound.analyze`), each with its keys in
//! order, spelled as [`Key`] spells them; an empty list means the action has no key. An id this
//! version doesn't know is kept and ignored, and spellings are kept as written: the player checks
//! them when it applies the file, so a newer version's file survives an older one.
//!
//! The file is the user's, not the player's state: it's written when a key changes, never over
//! one that can't be read. Signed in, the player publishes it as the account's keybinds record
//! (`api::keybinds_publish`), which loads on another computer and which anyone can import.

use std::path::Path;

use indexmap::IndexMap;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::error::IoContext;
use crate::store::{KEYBINDS_FILE, player_dir, player_file};
use crate::{Error, Result, fsutil};

mod key;
pub mod migrate;
#[cfg(feature = "account")]
mod record;

pub use key::{Key, Named};

/// `keybinds.json`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Keybinds {
    /// The actions whose keys differ from the player's defaults, by id, sorted: each one's keys in
    /// order (for `deck.hot-cue`, the nth key plays the nth hot cue), or none for no key. A key is
    /// one character as typed (a capital is shift and the letter), `space`, `enter`, `tab`,
    /// `backspace`, `delete`, `insert`, `home`, `end`, `pageup`, `pagedown`, `up`, `down`, `left`,
    /// `right`, `f1` to `f12`, `shift-` and one of those names but the f keys, or `ctrl-` and a
    /// lowercase letter.
    #[serde(default)]
    pub keys: IndexMap<String, Vec<String>>,
    /// The account some of these keys were imported from (a DID), for credit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<String>,
    /// When they last changed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
}

/// The keys set for the player whose store is at `store_root`: none when there's no file. An
/// error when there's one that can't be read or isn't the file: the player runs on its defaults
/// then, and mustn't save over it.
pub fn load(store_root: &Path) -> Result<Keybinds> {
    let Some(file) = player_file(store_root, KEYBINDS_FILE) else { return Ok(Keybinds::default()) };
    Ok(fsutil::read_json_opt(&file)?.unwrap_or_default())
}

/// Keep `keybinds` for the player whose store is at `store_root`, sorted by action, in the store
/// once there is one (see [`player_dir`]).
pub fn save(store_root: &Path, keybinds: &Keybinds) -> Result<()> {
    let dir = player_dir(store_root)
        .ok_or_else(|| Error::invalid(format!("there's nowhere beside {} to keep keys", store_root.display())))?;
    std::fs::create_dir_all(&dir).at(&dir)?;
    let mut sorted = keybinds.clone();
    sorted.keys.sort_unstable_keys();
    fsutil::write_json_atomic(&dir.join(KEYBINDS_FILE), &sorted)
}

#[cfg(test)]
mod tests;
