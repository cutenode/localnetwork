//! Signing in again to an account plugins had access to before it was signed out, in the player:
//! what came back is said, and what's still remembered is asked about in a form, Restore picked
//! (enter restores it all), Choose taking the scopes typed for each plugin, and Reset forgetting
//! it. Closed, nothing changes: the access stays off until it's restored.

use indexmap::IndexSet;
use locallibrary::Store;
use locallibrary::plugin::{Access, AccountId, AccountSignedIn, Chosen};
use serde_json::{Map, Value, json};

use crate::tui::app::{App, Level};
use crate::tui::edits::{Done, Modal};
use crate::tui::form::{For, Form};

/// The form's field for what to do: one of [`ACTIONS`].
const ACTION: &str = "access";
const ACTIONS: [&str; 3] = ["Restore", "Choose", "Reset"];

/// After signing in to `done`'s account, shown with `show`: the access restored said, and a form
/// asking about what's still remembered.
pub(super) fn after_sign_in(done: &AccountSignedIn, show: &dyn Fn(Done)) {
    if !done.restored.is_empty() {
        let said = restored(&done.restored);
        show(Box::new(move |app: &mut App| app.toast(said, Level::Info)));
    }
    if done.remembered.is_empty() {
        return;
    }
    let (name, remembered) = (done.account.name.clone(), done.remembered.clone());
    let account = AccountId { plugin: done.account.plugin.clone(), subject: done.account.subject.clone() };
    show(Box::new(move |app: &mut App| app.ask_restore(name, account, remembered)));
}

/// Which plugins got their access back, and to what.
fn restored(back: &[Access]) -> String {
    let each: Vec<String> = back.iter().map(|a| format!("{} ({})", a.plugin, a.scopes.join(", "))).collect();
    format!("restored access: {}", each.join("; "))
}

impl App {
    /// The form asking what to do with the access `remembered` that plugins had to `account`
    /// (shown as `name`) before it was signed out: a field for the action, Restore first, and one
    /// for each plugin's scopes, filled in with those it had.
    pub(in crate::tui) fn ask_restore(&mut self, name: String, account: AccountId, remembered: Vec<Access>) {
        let Ok(plugins) = crate::plugins::host() else { return };
        let had: Vec<String> = remembered.iter().map(|a| format!("{} ({})", a.plugin, a.scopes.join(", "))).collect();
        let about = format!("Plugins had access to {name} before it was signed out: {}", had.join("; "));
        let mut properties = Map::new();
        properties.insert(ACTION.into(), json!({ "type": "string", "enum": ACTIONS, "description": about }));
        for a in &remembered {
            let need = plugins.manifest(&a.plugin).ok().and_then(|m| m.accounts.iter().find(|n| n.name == a.kind));
            let mut asked: IndexSet<&String> = need.iter().flat_map(|n| &n.scopes).collect();
            asked.extend(need.iter().flat_map(|n| n.optional.iter().flat_map(|o| &o.scopes)));
            asked.extend(&a.scopes);
            let about = format!("With Choose, what {} gets back: none leaves it off", a.plugin);
            let items = json!({ "type": "string", "enum": asked });
            properties.insert(
                a.plugin.clone(),
                json!({ "type": "array", "items": items, "default": a.scopes, "description": about }),
            );
        }
        let schema = json!({ "type": "object", "properties": properties, "required": [ACTION] });
        let form = Form::with(format!("Restore access to {name}"), Some(schema), &Map::new(), For::Restore(account));
        self.modal = Some(Modal::Form(form));
    }

    /// Does what the restore form says (`given`) with what's remembered of `account`, in the
    /// background: every plugin's access back, those chosen with their scopes, or none.
    pub(in crate::tui) fn restore_access(&mut self, account: AccountId, given: Value) {
        let Ok(plugins) = crate::plugins::host() else { return };
        let chosen = match given.get(ACTION).and_then(Value::as_str) {
            Some("Choose") => Some(
                (given.as_object().into_iter().flatten())
                    .filter(|(field, _)| *field != ACTION)
                    .filter_map(|(plugin, scopes)| {
                        let scopes: Vec<String> =
                            scopes.as_array()?.iter().filter_map(Value::as_str).map(String::from).collect();
                        (!scopes.is_empty()).then(|| Chosen { plugin: plugin.clone(), scopes })
                    })
                    .collect::<Vec<_>>(),
            ),
            Some("Reset") => Some(Vec::new()),
            _ => None,
        };
        let store_root = self.store_root.clone();
        self.task("Restoring access".into(), move |_| {
            let store = Store::open_or_init(&store_root)?;
            let back = plugins.restore_access(&account, chosen.as_deref(), &store)?;
            let said = if back.is_empty() { "reset: no plugin has access to it".into() } else { restored(&back) };
            Ok(Box::new(move |app: &mut App| app.toast(said, Level::Info)))
        });
    }
}
