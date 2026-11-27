//! Moving the keys earlier versions kept among plugins' settings (`keys.<name>` in
//! `plugins/<id>/settings.json`) into `keybinds.json`, as `plugin.<id>.<name>`. It can go once
//! released stores have all run it.

use indexmap::IndexMap;
use serde_json::Value;

use super::{Key, load, save};
use crate::store::Store;
use crate::{Result, plugin};

/// What a plugin's setting for the player's key to one of its commands was named with.
const KEYS: &str = "keys.";

/// Moves every plugin's `keys.<name>` settings into `keybinds.json`, spelled as [`Key`] spells
/// them, `keybinds.json` written first and then each plugin's settings without them. What's
/// dropped instead is said, one line each: a key that can't be read, one `keybinds.json` or an
/// earlier plugin's has already (the first stays), or an action `keybinds.json` has keys for
/// already. With no such settings it reads nothing else and writes nothing, so the player runs it
/// at every start.
pub fn plugin_keys(store: &Store) -> Result<Vec<String>> {
    let mut found = plugin::all_saved_settings(store)?;
    found.retain(|_, settings| settings.keys().any(|k| k.starts_with(KEYS)));
    if found.is_empty() {
        return Ok(Vec::new());
    }
    let mut binds = load(store.root())?;
    let mut taken: Vec<Key> = binds.keys.values().flatten().filter_map(|k| Key::parse(k).ok()).collect();
    let mut dropped = Vec::new();
    let mut left: IndexMap<String, IndexMap<String, Value>> = IndexMap::new();
    for (id, settings) in found {
        let (keys, rest): (IndexMap<String, Value>, IndexMap<String, Value>) =
            settings.into_iter().partition(|(k, _)| k.starts_with(KEYS));
        for (setting, value) in keys {
            let action = format!("plugin.{id}.{}", &setting[KEYS.len()..]);
            // A blank one set no key.
            let spelled = match &value {
                Value::String(s) if s.trim().is_empty() => continue,
                Value::String(s) => s,
                other => {
                    dropped.push(format!("{action}: {other} isn't a key"));
                    continue;
                }
            };
            let why = match Key::parse(spelled) {
                _ if binds.keys.contains_key(&action) => "keybinds.json has its keys already".to_string(),
                Ok(key) if taken.contains(&key) => format!("{key} is another action's"),
                Ok(key) => {
                    taken.push(key);
                    binds.keys.insert(action, vec![key.to_string()]);
                    continue;
                }
                Err(why) => why,
            };
            dropped.push(format!("{action}: {why}"));
        }
        left.insert(id, rest);
    }
    save(store.root(), &binds)?;
    for (id, rest) in &left {
        plugin::save_settings(store, id, rest)?;
    }
    Ok(dropped)
}
