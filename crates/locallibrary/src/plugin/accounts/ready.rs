//! Whether a plugin's accounts will do for what it's about to start (a destination, publishing a
//! set), checked before it starts, so nothing begins (a render, an upload) that a request of its
//! would fail half way through: of each type it asks for, it has a default access, to an account
//! signed in, with every scope the plugin needs, each granted what it asks, and, when the account
//! isn't refreshed, signed in for an hour yet besides how long sending its files may take.

use std::time::Duration;

use chrono::Utc;
use serde_json::{Map, json};

use super::access::{Access, actions};
use super::kept;
use crate::error::Reason;
use crate::plugin::Plugins;
use crate::plugin::net::sending;
use crate::plugin::process::Plugin;
use crate::plugin::secrets::secret;
use crate::store::Store;
use crate::{Error, Result};

/// How long a plugin's accounts have to stay signed in once it starts, besides sending its files.
const LASTING: Duration = Duration::from_secs(60 * 60);

/// What keeps a plugin from using its default access of an account type it asks for.
#[derive(Debug, Clone, PartialEq)]
pub enum Unready {
    /// It has none of the type (by name), or none whose account plugin is installed.
    NoAccess(String),
    /// The access's account is signed out, or its tokens are lost.
    SignedOut(Access),
    /// The access's account isn't refreshed, and won't stay signed in long enough.
    Expiring(Access),
    /// The access hasn't these of the scopes (by name) the plugin needs.
    Ungiven(Access, Vec<String>),
    /// The account wasn't granted what these of the scopes (by name) the plugin needs ask.
    Ungranted(Access, Vec<String>),
}

impl Unready {
    /// The account type it's about.
    pub fn kind(&self) -> &str {
        match self {
            Unready::NoAccess(kind) => kind,
            Unready::SignedOut(a) | Unready::Expiring(a) | Unready::Ungiven(a, _) | Unready::Ungranted(a, _) => &a.kind,
        }
    }
}

impl Plugin {
    /// What keeps it from using its default access of each account type it asks for in `store`
    /// for as long as sending `bytes` of files may take, besides [`LASTING`]: the first of
    /// [`Unready`]'s, in its order, for each type.
    fn unready(&self, store: &Store, bytes: u64) -> Result<Vec<Unready>> {
        let book = kept::read(store)?;
        // None: later than anything expires.
        let until =
            chrono::Duration::from_std(LASTING + sending(bytes)).ok().and_then(|d| Utc::now().checked_add_signed(d));
        let mut unready = Vec::new();
        for need in &self.manifest.accounts {
            let kind = &need.name;
            let default = book.accesses.iter().find(|a| a.plugin == self.manifest.id && a.kind == *kind && a.default);
            let Some(a) = default else {
                unready.push(Unready::NoAccess(kind.clone()));
                continue;
            };
            let k = book.account(&a.account.plugin, &a.account.subject);
            let tokens = match k {
                Some(k) => secret(self.host.options.secrets, &kept::place(store, &k.plugin, &k.subject))?,
                None => None,
            };
            let (Some(k), Some(_)) = (k, tokens) else {
                unready.push(Unready::SignedOut(a.clone()));
                continue;
            };
            let with = self.host.account_plugin(&a.account.plugin);
            let Some(t) = with.as_ref().and_then(|w| w.account_type(kind).ok()) else {
                unready.push(Unready::NoAccess(kind.clone()));
                continue;
            };
            let ungiven: Vec<String> = need.scopes.iter().filter(|s| !a.scopes.contains(s)).cloned().collect();
            let is_granted =
                |name: &&String| t.scopes.get(*name).is_some_and(|s| s.asks.iter().all(|asks| k.scopes.contains(asks)));
            let ungranted: Vec<String> = need.scopes.iter().filter(|s| !is_granted(s)).cloned().collect();
            let expiring = !k.refreshes && k.expires_at.is_some_and(|at| until.is_none_or(|until| at <= until));
            unready.extend(match () {
                _ if !ungiven.is_empty() => Some(Unready::Ungiven(a.clone(), ungiven)),
                _ if !ungranted.is_empty() => Some(Unready::Ungranted(a.clone(), ungranted)),
                _ if expiring => Some(Unready::Expiring(a.clone())),
                _ => None,
            });
        }
        Ok(unready)
    }

    /// Fails `account` when its accounts won't do for sending `bytes` of files in `store` (see
    /// [`Plugin::unready`]): of the first that won't, with the reason (`no_access`,
    /// `not_signed_in`, `missing_scope`), its type, and what resolves it (see [`actions`]).
    pub(crate) fn check_accounts(&self, store: &Store, bytes: u64) -> Result<()> {
        let Some(u) = self.unready(store, bytes)?.into_iter().next() else { return Ok(()) };
        let name = &self.manifest.name;
        let (reason, why, about) = match &u {
            Unready::NoAccess(kind) => {
                (Reason::NoAccess, format!("the {name} plugin has no access to a {kind} account"), None)
            }
            Unready::SignedOut(a) => {
                let why = format!("the {} account {} is signed out: sign in again", a.kind, a.name);
                (Reason::NotSignedIn, why, Some((a, &a.scopes)))
            }
            Unready::Expiring(a) => {
                let why = format!("the {} account {} won't stay signed in long enough: sign in again", a.kind, a.name);
                (Reason::NotSignedIn, why, Some((a, &a.scopes)))
            }
            Unready::Ungiven(a, scopes) => {
                let why = format!(
                    "the {name} plugin's access to the {} account {} doesn't have every scope it needs: give it again",
                    a.kind, a.name
                );
                (Reason::MissingScope, why, Some((a, scopes)))
            }
            Unready::Ungranted(a, scopes) => {
                let why = format!(
                    "the {} account {} wasn't granted what {} asks: sign in again",
                    a.kind,
                    a.name,
                    scopes.join(", ")
                );
                (Reason::MissingScope, why, Some((a, scopes)))
            }
        };
        let said =
            about.map(|(a, needs)| Map::from_iter([("access".into(), json!(a.id)), ("needs".into(), json!(needs))]));
        let kind = u.kind();
        let e = Error::Account(why).because(reason).with("type", kind);
        let actions = actions(&self.host, &self.manifest, Some(kind), Some(store), &said.unwrap_or_default());
        Err(actions.into_iter().fold(e, Error::action))
    }
}

impl Plugins {
    /// What keeps `plugin` from using its default access of each account type it asks for in
    /// `store`, for an hour (see [`Unready`]).
    pub fn unready(&self, plugin: &str, store: &Store) -> Result<Vec<Unready>> {
        self.slot(plugin)?.unready(store, 0)
    }
}
