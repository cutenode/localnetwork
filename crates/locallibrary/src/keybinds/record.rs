//! `keybinds.json` as the account's keybinds record ([`lexicon::KEYBINDS`], one per account, key
//! `self`), and back. A record holds a list of bindings rather than a map, since a record's keys
//! can't be anything at all.

use chrono::{DateTime, Utc};
use indexmap::IndexMap;
use jacquard_common::types::did::validate_did;
use serde_json::{Value, json};

use super::{Key, Keybinds};
use crate::atmosphere::lexicon;
use crate::export::one_line;
use crate::profile::timestamp;
use crate::{Error, Result};

/// The most bindings a record holds, the most keys one has, and the longest action id, in bytes:
/// what its Lexicon allows.
const MOST_BINDINGS: usize = 512;
const MOST_KEYS: usize = 8;
const LONGEST_ACTION: usize = 128;

impl Keybinds {
    /// Each action's keys as a record has them, sorted by action: spelled as [`Key`] spells them,
    /// without the ones that can't be read (and without an action that had only those: no key at
    /// all isn't what it says), and no more than the Lexicon allows.
    pub(crate) fn readable(&self) -> IndexMap<String, Vec<String>> {
        let fits = self.keys.iter().filter(|(action, _)| action.len() <= LONGEST_ACTION);
        let mut readable: IndexMap<String, Vec<String>> = fits
            .filter_map(|(action, keys)| {
                let read = keys.iter().filter_map(|k| Key::parse(k).ok()).map(|k| k.to_string());
                let read: Vec<String> = read.take(MOST_KEYS).collect();
                (!read.is_empty() || keys.is_empty()).then(|| (action.clone(), read))
            })
            .collect();
        readable.sort_unstable_keys();
        readable.truncate(MOST_BINDINGS);
        readable
    }

    /// The keybinds record for these keys, written at `now`. `from` goes with them while it's a DID.
    pub fn to_record(&self, now: DateTime<Utc>) -> Value {
        let bindings: Vec<Value> =
            self.readable().into_iter().map(|(action, keys)| json!({ "action": action, "keys": keys })).collect();
        let mut record = json!({ "$type": lexicon::KEYBINDS, "bindings": bindings, "updatedAt": timestamp(now) });
        if let Some(from) = self.from.as_deref().filter(|d| validate_did(d).is_ok()) {
            record["from"] = from.into();
        }
        record
    }

    /// A keybinds record read back (someone else's, or written by another version of locallibrary),
    /// with what was skipped: a binding that isn't an action and its keys, an action listed again
    /// (the first stays), and a key this version can't read, as the Lexicon asks (an action with
    /// keys but none it can read is skipped whole). An error when it isn't a keybinds record.
    pub fn from_record(record: &Value) -> Result<(Keybinds, Vec<String>)> {
        if record["$type"] != lexicon::KEYBINDS {
            return Err(Error::invalid(format!(
                "what's published there isn't a keybinds record (its $type is {})",
                one_line(&record["$type"].to_string())
            )));
        }
        let text = |v: &Value| v.as_str().map(String::from);
        let mut out =
            Keybinds { from: text(&record["from"]), updated_at: text(&record["updatedAt"]), ..Default::default() };
        let mut skipped = Vec::new();
        for binding in record["bindings"].as_array().into_iter().flatten() {
            let (Some(action), Some(keys)) = (binding["action"].as_str(), binding["keys"].as_array()) else {
                skipped.push(one_line(&binding.to_string()));
                continue;
            };
            if out.keys.contains_key(action) {
                skipped.push(format!("{}, listed again", one_line(action)));
                continue;
            }
            let mut read = Vec::new();
            for k in keys {
                match k.as_str().map(Key::parse) {
                    Some(Ok(key)) => read.push(key.to_string()),
                    _ => skipped.push(format!("{}: {}", one_line(action), one_line(&k.to_string()))),
                }
            }
            if read.is_empty() && !keys.is_empty() {
                continue;
            }
            out.keys.insert(action.to_string(), read);
        }
        out.keys.sort_unstable_keys();
        Ok((out, skipped))
    }
}
