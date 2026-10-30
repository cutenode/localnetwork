//! Accesses: the scopes of an account a plugin was given, kept beside the accounts (see [`kept`])
//! and outlasting them, so an access to an account signed out still lists, and gets its token back
//! once the account is signed in again. A plugin has one access to an account at most, and of each
//! type it asks for one default at most, which `{account:<type>}` names: the first it's given,
//! or the one left when the default is taken away.

use indexmap::IndexSet;
use loclib_plugin::{AccountNeed, Manifest};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;

use super::kept::{self, Book, Kept};
use super::shared::not_kept;
use crate::error::Action;
use crate::plugin::Plugins;
use crate::plugin::process::Plugin;
use crate::store::Store;
use crate::{Error, Result};

/// The scopes of an account a plugin was given.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Access {
    /// 8 lowercase hex digits, which `{account:<type>/<id>}` names it by.
    pub id: String,
    /// The plugin given it.
    pub plugin: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub account: AccountId,
    /// What the account is shown as, kept so an access to one signed out still says.
    pub name: String,
    /// The type's scopes, by name.
    pub scopes: Vec<String>,
    /// It's the plugin's default for the type: `{account:<type>}` names it.
    #[serde(default)]
    pub default: bool,
}

/// An account kept in a store: the account plugin that signed it in, and its subject there.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct AccountId {
    pub plugin: String,
    pub subject: String,
}

impl AccountId {
    fn of(k: &Kept) -> AccountId {
        AccountId { plugin: k.plugin.clone(), subject: k.subject.clone() }
    }
}

/// A new access's id: 8 random lowercase hex digits, none of `taken`'s.
fn new_id(taken: &[Access]) -> String {
    loop {
        let id = uuid::Uuid::new_v4().simple().to_string()[..8].to_string();
        if !taken.iter().any(|a| a.id == id) {
            return id;
        }
    }
}

impl Book {
    /// Gives `plugin` access to `k`'s `scopes` besides those it has: the access it has, or a new
    /// one, its default for the type when it has none of the type.
    pub(super) fn give(&mut self, plugin: &str, k: &Kept, scopes: &[String]) -> Access {
        let account = AccountId::of(k);
        let i = match self.accesses.iter().position(|a| a.plugin == plugin && a.account == account) {
            Some(i) => i,
            None => {
                let first = !self.accesses.iter().any(|a| a.plugin == plugin && a.kind == k.kind);
                let id = new_id(&self.accesses);
                let (plugin, kind, name) = (plugin.into(), k.kind.clone(), k.name.clone());
                self.accesses.push(Access { id, plugin, kind, account, name, scopes: Vec::new(), default: first });
                self.accesses.len() - 1
            }
        };
        let a = &mut self.accesses[i];
        let mut all: IndexSet<String> = a.scopes.drain(..).collect();
        all.extend(scopes.iter().cloned());
        a.scopes = all.into_iter().collect();
        a.clone()
    }

    /// The accesses to `k` say what it's shown as now.
    pub(super) fn renamed(&mut self, k: &Kept) {
        let account = AccountId::of(k);
        self.accesses.iter_mut().filter(|a| a.account == account).for_each(|a| a.name.clone_from(&k.name));
    }

    /// Where `plugin`'s access to `account` is, if it has one.
    fn access(&self, plugin: &str, account: &AccountId) -> Option<usize> {
        self.accesses.iter().position(|a| a.plugin == plugin && a.account == *account)
    }
}

impl Plugin {
    /// What it asks for of accounts of type `kind`.
    pub(crate) fn needs(&self, kind: &str) -> Result<&AccountNeed> {
        let id = &self.manifest.id;
        let need = self.manifest.accounts.iter().find(|n| n.name == kind);
        need.ok_or_else(|| Error::invalid(format!("the {id} plugin doesn't ask for {kind} accounts")))
    }

    /// The scopes of type `kind` it's given when it's given `asked`: those it needs, and those of
    /// `asked` it can do without. One it doesn't ask for is refused.
    pub(super) fn given(&self, kind: &str, asked: &[String]) -> Result<Vec<String>> {
        let need = self.needs(kind)?;
        let optional: Vec<&String> = need.optional.iter().flat_map(|o| &o.scopes).collect();
        if let Some(other) = asked.iter().find(|s| !need.scopes.contains(s) && !optional.contains(s)) {
            let id = &self.manifest.id;
            return Err(Error::invalid(format!("the {id} plugin doesn't ask for the {kind} scope {other:?}")));
        }
        let mut given: IndexSet<String> = need.scopes.iter().cloned().collect();
        given.extend(asked.iter().cloned());
        Ok(given.into_iter().collect())
    }
}

impl Plugins {
    /// The accesses `store` keeps: `plugin`'s when it's given, those to `account` when it's given.
    pub fn accesses(&self, store: &Store, plugin: Option<&str>, account: Option<&AccountId>) -> Result<Vec<Access>> {
        let of = |a: &Access| plugin.is_none_or(|p| a.plugin == p) && account.is_none_or(|x| a.account == *x);
        Ok(kept::read(store)?.accesses.into_iter().filter(of).collect())
    }

    /// Gives `plugin` access to `account` (one `store` keeps): to the scopes it needs of the
    /// account's type, and those of `scopes` it can do without, besides what it has. Its first
    /// access of the type is its default.
    pub fn give_access(&self, plugin: &str, account: &AccountId, scopes: &[String], store: &Store) -> Result<Access> {
        let slot = self.slot(plugin)?;
        kept::update(store, |book| {
            let k = book.account(&account.plugin, &account.subject).cloned();
            let k = k.ok_or_else(|| not_kept(&account.plugin, &account.subject))?;
            let given = slot.given(&k.kind, scopes)?;
            Ok(book.give(plugin, &k, &given))
        })
    }

    /// Makes `plugin`'s access to `account` its default for the account's type, giving it access
    /// first (to the scopes it needs) when it has none.
    pub fn set_default_access(&self, plugin: &str, account: &AccountId, store: &Store) -> Result<Access> {
        let slot = self.slot(plugin)?;
        kept::update(store, |book| {
            let i = match book.access(plugin, account) {
                Some(i) => i,
                None => {
                    let k = book.account(&account.plugin, &account.subject).cloned();
                    let k = k.ok_or_else(|| not_kept(&account.plugin, &account.subject))?;
                    let given = slot.given(&k.kind, &[])?;
                    book.give(plugin, &k, &given);
                    book.accesses.len() - 1
                }
            };
            let kind = book.accesses[i].kind.clone();
            for a in book.accesses.iter_mut().filter(|a| a.plugin == plugin && a.kind == kind) {
                a.default = a.account == *account;
            }
            Ok(book.accesses[i].clone())
        })
    }

    /// Takes `plugin`'s access to `account` away: what it was. When it was the default for its
    /// type and the plugin has one access of the type left, that one is.
    pub fn take_away_access(&self, plugin: &str, account: &AccountId, store: &Store) -> Result<Access> {
        kept::update(store, |book| {
            let i = book.access(plugin, account).ok_or_else(|| {
                Error::NotFound(format!("the {plugin} plugin's access to {:?} ({})", account.subject, account.plugin))
            })?;
            let taken = book.accesses.remove(i);
            let mut left: Vec<&mut Access> =
                book.accesses.iter_mut().filter(|a| a.plugin == plugin && a.kind == taken.kind).collect();
            if let [only] = left.as_mut_slice() {
                only.default |= taken.default;
            }
            Ok(taken)
        })
    }
}

/// What resolves a plugin (its `manifest`) having no access, or not enough, to an account of
/// type `kind` (one it asks for; any it asks for when it doesn't): signing in to one for it, and
/// making each `store` keeps of the type its default.
pub(crate) fn actions(manifest: &Manifest, kind: Option<&str>, store: Option<&Store>) -> Vec<Action> {
    let asked = manifest.accounts.iter().map(|n| n.name.as_str());
    let kinds: Vec<&str> = match kind.filter(|k| manifest.accounts.iter().any(|n| n.name == *k)) {
        Some(kind) => vec![kind],
        None => asked.collect(),
    };
    let book = store.and_then(|s| kept::read(s).ok()).unwrap_or_default();
    let plugin = &manifest.id;
    let mut actions = Vec::new();
    for kind in kinds {
        let begin = json!({ "type": kind, "for": plugin });
        actions.push(Action::call("accounts.sign_in.begin", begin, format!("Sign in to a {kind} account")));
        for k in book.accounts.iter().filter(|k| k.kind == kind) {
            let params = json!({ "plugin": plugin, "account": AccountId::of(k) });
            actions.push(Action::call("accounts.access.set_default", params, format!("Use {}", k.name)));
        }
    }
    actions
}
