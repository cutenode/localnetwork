//! An account signed in to with keys, in the player: a form with a field for each key, typed
//! unseen, and for the account's name when its service doesn't say whose they are, saying where
//! to get them (never opening the browser). Whatever signs in waits off the UI thread for what's
//! entered (see [`entered_keys`]); closing the form enters nothing.

use std::sync::mpsc::{Sender, channel};

use indexmap::IndexMap;
use locallibrary::plugin::{AccountSignIn, EnteredKeys};
use loclib_plugin::a_or_an;
use serde_json::{Map, Value, json};

use crate::account_cmd::{account_type, getting_keys};
use crate::tui::app::{App, Msg};
use crate::tui::edits::Modal;
use crate::tui::form::{For, Form};

/// The field the account's name is typed in.
const NAME: &str = "Account name";

/// The keys entered in the form [`App::enter_keys`] opens for `start`, asked for over `tx` and
/// waited for: `None` once the form is closed, or the player is.
pub(in crate::tui) fn entered_keys(tx: &Sender<Msg>, start: &AccountSignIn) -> Option<EnteredKeys> {
    let (reply, entered) = channel();
    tx.send(Msg::EnterKeys(Box::new(start.clone()), reply)).ok()?;
    entered.recv().ok()
}

impl App {
    /// The form taking the keys `start` asks for, each field named by its key's title, and the
    /// account's name when `start` is `named`; what's entered goes back on `reply`. Closed, it's
    /// never answered.
    pub(in crate::tui) fn enter_keys(&mut self, start: &AccountSignIn, reply: Sender<EnteredKeys>) {
        let Ok(plugins) = crate::plugins::host() else { return };
        let title = account_type(plugins, &start.plugin, &start.kind).map_or(start.kind.as_str(), |t| &t.title);
        let getting = format!("{} {}", getting_keys(plugins, start), start.url);
        let mut properties: Map<String, Value> = start
            .keys
            .values()
            .map(|key| (key.clone(), json!({ "type": "string", "writeOnly": true, "description": getting })))
            .collect();
        if start.named {
            let about = format!("What to call the account: {title} doesn't say who the keys are for");
            properties.insert(NAME.into(), json!({ "type": "string", "description": about }));
        }
        let required: Vec<&String> = properties.keys().collect();
        let schema = json!({ "type": "object", "properties": properties, "required": required });
        let form = Form::with(
            format!("Sign in to {} account", a_or_an(title)),
            Some(schema),
            &Map::new(),
            For::Keys(start.keys.clone(), reply),
        );
        self.modal = Some(Modal::Form(form));
    }
}

/// The keys `given` in the form for the keys titled `keys` (by name), and the account's name,
/// sent back on `reply`.
pub(in crate::tui) fn answer(keys: &IndexMap<String, String>, reply: &Sender<EnteredKeys>, given: &Value) {
    let text = |field: &str| given.get(field).and_then(Value::as_str).map(String::from);
    let entered = keys.iter().map(|(name, title)| (name.clone(), text(title).unwrap_or_default())).collect();
    let _ = reply.send((entered, text(NAME)));
}
