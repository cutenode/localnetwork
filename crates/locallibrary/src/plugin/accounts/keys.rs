//! Accounts signed in to with keys (an account type's `kind = "keys"`): the keys the user enters,
//! checked against the type, kept where an account's tokens would be (see [`kept::keep_keys`]) and
//! told to [`crate::plugin::net::State::scrub`] whenever they're entered or read; and what a
//! request naming an account is filled in with, a token or keys ([`Credential`]). Nothing
//! refreshes keys: a service refusing them is answered to the plugin as it is, and they stay kept.

use indexmap::IndexMap;
use loclib_plugin::{AccountType, KeyPlace, SignIn};
use serde::{Deserialize, Serialize};

use super::Waiting;
use super::kept::{self, Kept};
use super::shared::{AccountSignedIn, Asked, find};
use crate::error::Reason;
use crate::plugin::Plugins;
use crate::plugin::process::Plugin;
use crate::plugin::secrets::secret;
use crate::store::Store;
use crate::{Error, Result};

/// The fewest characters a key entered has: whatever the host scrubs out of what it shows is
/// replaced wherever it's seen, which a short one would garble.
const SHORTEST_KEY: usize = 8;

/// An account's keys, by name, as they're kept. Never printed: it has no `Debug`.
#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct Keys {
    keys: IndexMap<String, String>,
}

/// What an account is signed in with, which a request naming it is filled in with. Never printed:
/// it has no `Debug`.
pub(crate) enum Credential {
    Token(String),
    Keys(Keys),
}

impl Credential {
    /// What goes at `place`: its value, with the token or the keys it names.
    pub(crate) fn filled(&self, place: &KeyPlace) -> String {
        match self {
            Credential::Token(token) => place.filled(|_| token),
            Credential::Keys(k) => place.filled(|name| k.keys.get(name).map_or("", String::as_str)),
        }
    }

    /// Its token, or each of its keys.
    pub(crate) fn secrets(&self) -> Vec<&str> {
        match self {
            Credential::Token(token) => vec![token],
            Credential::Keys(k) => k.keys.values().map(String::as_str).collect(),
        }
    }

    /// What it is, as the user knows it.
    pub(super) fn said(&self) -> &'static str {
        match self {
            Credential::Token(_) => "token",
            Credential::Keys(_) => "keys",
        }
    }
}

impl Plugin {
    /// The keys `entered` (by name) for an account of type `t`, trimmed and told to
    /// [`crate::plugin::net::State::scrub`]: each of its keys, none besides, each at least
    /// [`SHORTEST_KEY`] printable ASCII characters without spaces. Never saying what one is.
    fn entered(&self, t: &AccountType, entered: &IndexMap<String, String>) -> Result<Keys> {
        if let Some(name) = entered.keys().find(|n| !t.keys.contains_key(*n)) {
            return Err(Error::invalid(format!("{name:?} isn't one of the keys of a {} account", t.title)));
        }
        let mut keys = IndexMap::new();
        for (name, key) in &t.keys {
            let value = entered.get(name).map_or("", |v| v.trim());
            let wrong = match value {
                "" => Some("is missing".to_string()),
                v if !v.bytes().all(|b| b.is_ascii_graphic()) => Some("isn't printable ASCII without spaces".into()),
                v if v.len() < SHORTEST_KEY => {
                    Some(format!("is {} characters: a key is {SHORTEST_KEY} at least", v.len()))
                }
                _ => None,
            };
            if let Some(why) = wrong {
                return Err(Error::invalid(format!("the {} account's {} ({name}) {why}", t.title, key.title)));
            }
            keys.insert(name.clone(), value.to_string());
        }
        keys.values().for_each(|k| self.net.saw(k));
        Ok(Keys { keys })
    }

    /// The keys kept for `k`, an account signed in to with keys, told to
    /// [`crate::plugin::net::State::scrub`]: `account` (`SessionEnded`) when it's signed out.
    fn kept_keys(&self, store: &Store, k: &Kept, t: &AccountType) -> Result<Keys> {
        let signed_out = || {
            let why = format!("the {} account {} is signed out: sign in again", t.title, k.name);
            Error::Account(why).because(Reason::SessionEnded)
        };
        let kept = secret(self.host.options.secrets, &kept::place(store, &k.plugin, &k.subject))?;
        let unread = || Error::Account(format!("the {} account's sign-in can't be read: sign in again", k.name));
        let keys: Keys = serde_json::from_str(&kept.ok_or_else(signed_out)?).map_err(|_| unread())?;
        keys.keys.values().for_each(|v| self.net.saw(v));
        Ok(keys)
    }

    /// What the account `subject` it signed in to in `store` is signed in with: its keys, or its
    /// access token (see [`Plugin::account_token`]).
    pub(crate) fn account_credential(&self, store: &Store, subject: &str) -> Result<Credential> {
        let k = find(store, &self.manifest.id, subject)?;
        let t = self.account_type(&k.kind)?;
        match t.kind {
            SignIn::Keys => self.kept_keys(store, &k, t).map(Credential::Keys),
            SignIn::Oauth => self.account_token(store, subject).map(Credential::Token),
        }
    }

    /// The account `asked` asked for in `store`, signed in to with the keys `entered`: whoever its
    /// type's whoami says they're for, else the account the user named `name` (its subject too),
    /// which has neither a line break nor one of the keys in it.
    fn signed_in_with_keys(
        &self,
        asked: Asked,
        store: &Store,
        entered: &IndexMap<String, String>,
        name: Option<&str>,
    ) -> Result<AccountSignedIn> {
        let t = self.account_type(&asked.kind)?;
        let keys = self.entered(t, entered)?;
        let (subject, name) = match (&t.whoami, name.map(str::trim).filter(|n| !n.is_empty())) {
            (Some(_), None) => self.whoami(t, &Credential::Keys(keys.clone()))?,
            (Some(_), Some(_)) => {
                let why = format!("{} says who the keys are for: sign in without naming the account", t.title);
                return Err(Error::invalid(why));
            }
            (None, None) => {
                let why = format!("name the {} account: {} doesn't say who the keys are for", t.title, t.title);
                return Err(Error::invalid(why));
            }
            (None, Some(name)) => {
                if name.contains(char::is_control) || keys.keys.values().any(|k| name.contains(k.as_str())) {
                    return Err(Error::invalid("an account's name has no line break or key in it"));
                }
                (name.to_string(), name.to_string())
            }
        };
        let storage = self.host.options.secrets;
        let _one = kept::hold(store, &asked.with, &subject)?;
        self.keep(asked, store, subject, name, |k| {
            k.keyed();
            kept::keep_keys(storage, store, k, &keys)
        })
    }
}

impl Plugins {
    /// Finishes sign-in `id`, to an account of a type signed in to with keys, with the keys the
    /// user `entered` (by name), which are checked first: the account they're for, as the type's
    /// whoami says, else named `name`, which is its subject too. The same account signed in again
    /// keeps the keys last entered. The sign-in is over either way.
    pub fn finish_account_sign_in_with_keys(
        &self,
        id: &str,
        entered: &IndexMap<String, String>,
        name: Option<&str>,
    ) -> Result<AccountSignedIn> {
        let Waiting::Keys { asked, store, .. } = self.host.pending(id)? else {
            return Err(Error::invalid(format!("sign-in {id:?} finishes in the browser")));
        };
        self.slot(&asked.with)?.signed_in_with_keys(asked, &store, entered, name)
    }
}
