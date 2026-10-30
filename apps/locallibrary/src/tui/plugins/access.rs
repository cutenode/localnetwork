//! A plugin's access to the accounts the store keeps, in the player: "access for PLUGIN…" in the
//! palette opens a form with a choice for each account type the plugin asks for, saying what it
//! needs it for: the accounts of that type signed in, and signing in to another (through each
//! account plugin providing the type, when several do). What's chosen becomes its default, in
//! the background.

use locallibrary::Store;
use locallibrary::plugin::{AccountId, Plugins};
use loclib_plugin::Manifest;
use serde_json::{Map, Value, json};

use super::accounts::{BROWSER_WAIT, open};
use crate::tui::app::{App, Level};
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

/// What the form asks of the account type `need` asks for: what `m` needs it for (its scopes'
/// titles, as an account plugin providing the type has them) and why.
fn about(plugins: &Plugins, m: &Manifest, need: &loclib_plugin::AccountNeed) -> String {
    let t = plugins.providing(&need.name).iter().find_map(|with| {
        let types = &plugins.manifest(with).ok()?.contributes.account_types;
        types.iter().find(|t| t.name == need.name).cloned()
    });
    let scope = |s: &String| t.as_ref().and_then(|t| t.scopes.get(s)).map_or(s.clone(), |s| s.title.clone());
    let scopes: Vec<String> = need.scopes.iter().map(scope).collect();
    format!("{} needs it to {}: {}", m.name, scopes.join(", "), need.why)
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
    /// background, signing in to those chosen to sign in to first.
    pub(in crate::tui) fn save_access(&mut self, plugin: &'static str, all: Vec<AccessChoice>, given: Value) {
        let Ok(plugins) = crate::plugins::host() else { return };
        let Ok(name) = plugins.manifest(plugin).map(|m| m.name.clone()) else { return };
        let store_root = self.store_root.clone();
        self.task_showing("Giving access".into(), move |ctx, show| {
            let store = Store::open_or_init(&store_root)?;
            let mut used = Vec::new();
            for c in &all {
                let chosen = given.get(&c.kind).and_then(Value::as_str);
                let Some((_, pick)) = c.picks.iter().find(|(l, _)| Some(l.as_str()) == chosen) else { continue };
                let account = match pick {
                    Pick::Account(id) => id.clone(),
                    Pick::SignIn(with) => {
                        let start = plugins.begin_account_sign_in(&c.kind, with, Some(plugin), None, &[], &store)?;
                        open(&start.url, show);
                        let a = plugins.finish_account_sign_in(&start.id, BROWSER_WAIT, ctx)?.account;
                        AccountId { plugin: a.plugin, subject: a.subject }
                    }
                };
                let access = plugins.set_default_access(plugin, &account, &store)?;
                used.push(access.name);
            }
            let said = format!("{name} uses {}", used.join(", "));
            Ok(Box::new(move |app: &mut App| app.toast(said, Level::Info)))
        });
    }
}
