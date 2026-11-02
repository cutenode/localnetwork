//! A plugin's access to the accounts the store keeps, in the player: "access for PLUGIN…" in the
//! palette opens a form with a choice for each account type the plugin asks for, saying what it
//! needs it for: the accounts of that type signed in, and signing in to another (through each
//! account plugin providing the type, when several do). What's chosen becomes its default, in
//! the background. A plugin wanting more of an account it has access to asks in a form too
//! (see [`App::ask_access`]), its hook waiting for the answer.

use std::sync::Arc;
use std::sync::mpsc::{Sender, channel};

use locallibrary::Store;
use locallibrary::plugin::{AccessQuestion, AccountId, AskAccess, Plugins};
use loclib_plugin::Manifest;
use serde_json::{Map, Value, json};

use super::accounts::{BROWSER_WAIT, open, unopened};
use crate::account_cmd::asking::wants;
use crate::tui::app::{App, Level, Msg};
use crate::tui::edits::Modal;
use crate::tui::form::{For, Form};
use crate::tui::keymap::Act;
use crate::tui::palette::Item;

/// What one of the form's choices for an account type gives the plugin.
pub enum Pick {
    /// That account.
    Account(AccountId),
    /// An account signed in to through this account plugin.
    SignIn(String),
}

/// The choices for an account type: each as the form shows it, and what it gives.
pub struct AccessChoice {
    pub kind: String,
    pub picks: Vec<(String, Pick)>,
}

/// Giving `m` access, when it asks for accounts.
pub(super) fn items(m: &'static Manifest) -> Option<(Act, Item)> {
    (!m.accounts.is_empty()).then(|| (Act::PluginAccess(&m.id), Item::new(format!("access for {}…", m.name), &m.name)))
}

/// The choices for each account type `m` asks for, with the account each has as its default now.
fn choices(plugins: &Plugins, m: &Manifest, store: Option<&Store>) -> (Vec<AccessChoice>, Map<String, Value>) {
    let kept = store.and_then(|s| plugins.kept_accounts(s).ok()).unwrap_or_default();
    let accesses = store.and_then(|s| plugins.accesses(s, Some(&m.id), None).ok()).unwrap_or_default();
    let mut filled = Map::new();
    let all: Vec<AccessChoice> = m
        .accounts
        .iter()
        .map(|need| {
            let providing = plugins.providing(&need.name);
            let several = providing.len() > 1;
            let mut picks: Vec<(String, Pick)> = kept
                .iter()
                .filter(|a| a.kind == need.name)
                .map(|a| {
                    let label = if several { format!("{} ({})", a.name, a.plugin) } else { a.name.clone() };
                    let id = AccountId { plugin: a.plugin.clone(), subject: a.subject.clone() };
                    if accesses.iter().any(|x| x.default && x.account == id) {
                        filled.insert(need.name.clone(), label.clone().into());
                    }
                    (label, Pick::Account(id))
                })
                .collect();
            let another = if picks.is_empty() { "sign in" } else { "sign in to another" };
            for with in providing {
                let label = if several { format!("{another} through {with}") } else { another.into() };
                picks.push((label, Pick::SignIn(with)));
            }
            AccessChoice { kind: need.name.clone(), picks }
        })
        .collect();
    (all, filled)
}

/// The titles of the account type `kind`'s `scopes` (by name), as an account plugin providing the
/// type has them.
fn titles(plugins: &Plugins, kind: &str, scopes: &[String]) -> Vec<String> {
    let t = plugins.providing(kind).iter().find_map(|with| {
        let types = &plugins.manifest(with).ok()?.contributes.account_types;
        types.iter().find(|t| t.name == kind).cloned()
    });
    scopes.iter().map(|s| t.as_ref().and_then(|t| t.scopes.get(s)).map_or(s.clone(), |s| s.title.clone())).collect()
}

/// What the form asks of the account type `need` asks for: what `m` needs it for (its scopes'
/// titles) and why.
fn about(plugins: &Plugins, m: &Manifest, need: &loclib_plugin::AccountNeed) -> String {
    format!("{} needs it to {}: {}", m.name, titles(plugins, &need.name, &need.scopes).join(", "), need.why)
}

impl App {
    /// The form giving `plugin` access: a choice for each account type it asks for.
    pub(in crate::tui) fn plugin_access(&mut self, plugin: &'static str) {
        let Ok(plugins) = crate::plugins::host() else { return };
        let Ok(m) = plugins.manifest(plugin) else { return };
        let store = Store::open_existing(&self.store_root).ok().flatten();
        let (all, filled) = choices(plugins, m, store.as_ref());
        let properties: Map<String, Value> = all
            .iter()
            .zip(&m.accounts)
            .map(|(c, need)| {
                let labels: Vec<&str> = c.picks.iter().map(|(l, _)| l.as_str()).collect();
                (c.kind.clone(), json!({ "type": "string", "enum": labels, "description": about(plugins, m, need) }))
            })
            .collect();
        let required: Vec<&String> = properties.keys().collect();
        let schema = json!({ "type": "object", "properties": properties, "required": required });
        let form = Form::with(format!("Access for {}", m.name), Some(schema), &filled, For::Access(plugin, all));
        self.modal = Some(Modal::Form(form));
    }

    /// Give `plugin` the accounts the form has chosen (`given`) as its defaults, in the
    /// background, signing in to those chosen to sign in to first, and say what the service
    /// declined of it.
    pub(in crate::tui) fn save_access(&mut self, plugin: &'static str, all: Vec<AccessChoice>, given: Value) {
        let Ok(plugins) = crate::plugins::host() else { return };
        let Ok(name) = plugins.manifest(plugin).map(|m| m.name.clone()) else { return };
        let store_root = self.store_root.clone();
        self.task_showing("Giving access".into(), move |ctx, show| {
            let store = Store::open_or_init(&store_root)?;
            let (mut used, mut declined) = (Vec::new(), Vec::new());
            for c in &all {
                let chosen = given.get(&c.kind).and_then(Value::as_str);
                let Some((_, pick)) = c.picks.iter().find(|(l, _)| Some(l.as_str()) == chosen) else { continue };
                let account = match pick {
                    Pick::Account(id) => id.clone(),
                    Pick::SignIn(with) => {
                        let start = plugins.begin_account_sign_in(&c.kind, with, Some(plugin), None, &[], &store)?;
                        open(&start.url, show);
                        let done = plugins.finish_account_sign_in(&start.id, BROWSER_WAIT, ctx)?;
                        declined.extend(titles(plugins, &c.kind, &done.declined));
                        AccountId { plugin: done.account.plugin, subject: done.account.subject }
                    }
                };
                let access = plugins.set_default_access(plugin, &account, &store)?;
                used.push(access.name);
            }
            let mut said = format!("{name} uses {}", used.join(", "));
            if !declined.is_empty() {
                said.push_str(&format!("; the service didn't allow it to {}", declined.join(", ")));
            }
            Ok(Box::new(move |app: &mut App| app.toast(said, Level::Info)))
        });
    }

    /// The form asking whether a plugin may use more of an account it has access to (`q`), its
    /// answer going back on `reply`. Closed, it's never answered: the plugin is told how to give
    /// it instead.
    pub(in crate::tui) fn ask_access(&mut self, q: &AccessQuestion, reply: Sender<bool>) {
        let allow = if q.sign_in { SIGN_IN } else { ALLOW };
        let answer = json!({ "type": "string", "enum": [allow, DECLINE], "description": wants(q) });
        let schema = json!({ "type": "object", "properties": { ANSWER: answer }, "required": [ANSWER] });
        let form = Form::with(format!("Access for {}", q.plugin), Some(schema), &Map::new(), For::AskAccess(reply));
        self.modal = Some(Modal::Form(form));
    }

    /// Where plugins ask whether they may use more of an account while the player runs: the
    /// form ([`App::ask_access`]), each waiting for its answer; a sign-in it takes opens the
    /// browser, saying where to go when it can't.
    pub(in crate::tui) fn plugin_asker(&self) -> Arc<dyn AskAccess> {
        Arc::new(Asking(self.sender()))
    }
}

/// The form's field, and its choices: allowing what a plugin asks (signing in again to, when the
/// account wasn't granted it), or not.
const ANSWER: &str = "answer";
const ALLOW: &str = "allow";
const SIGN_IN: &str = "sign in again to allow";
const DECLINE: &str = "don't allow";

/// The answer `given` in the form asking whether a plugin may use more of an account, sent back
/// on `reply`.
pub(in crate::tui) fn answer(reply: &Sender<bool>, given: &Value) {
    let _ = reply.send(given[ANSWER] != DECLINE);
}

/// Asks in the player (see [`App::plugin_asker`]).
struct Asking(Sender<Msg>);

impl AskAccess for Asking {
    fn allow(&self, q: &AccessQuestion) -> Option<bool> {
        let (reply, answered) = channel();
        self.0.send(Msg::AskAccess(Box::new(q.clone()), reply)).ok()?;
        answered.recv().ok()
    }

    fn sign_in(&self, url: &str) {
        if let Some(link) = unopened(url) {
            let _ = self.0.send(Msg::Note(link, Level::Info));
        }
    }
}
