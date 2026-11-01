//! Which token a `net/fetch` request naming an account by type gets (`{account:<type>}`, or
//! `{account:<type>/<access>}`): the token of the account the plugin's access names, its default
//! for the type or the access named, only when one of the access's scopes covers the request and
//! the account was granted what that scope asks of the service, and the access has every scope the
//! plugin needs. Anything else fails before the request is sent: `access` when the plugin has no
//! such access or the account is signed out (`reason` `signed_out`), `scope` when its scopes don't
//! reach, with the type, the scopes and the account plugins in its fields.

use std::sync::Arc;

use loclib_plugin::{AccountNeed, ErrorData, RpcError, Scope, TokenPlace};
use reqwest::Url;
use serde_json::{Map, json};

use super::kept;
use crate::error::Reason;
use crate::plugin::process::{Live, Plugin};
use crate::store::Store;

/// An access a request may use: where its account's token goes, and the scopes it may use it
/// for (those the account was granted what they ask), which a redirect has to stay within.
pub(crate) struct Usable {
    /// The account plugin, which gets the token.
    with: Arc<Plugin>,
    subject: String,
    pub(crate) place: TokenPlace,
    pub(crate) scopes: Vec<Scope>,
}

/// A failure of kind `kind` naming the type `need` asks for, `scopes` and the account `plugins`.
fn failed(kind: &str, why: String, need: &AccountNeed, scopes: &[String], plugins: Vec<String>) -> RpcError {
    let fields = Map::from_iter([
        ("type".to_string(), json!(need.name)),
        ("scopes".to_string(), json!(scopes)),
        ("account_plugins".to_string(), json!(plugins)),
    ]);
    let data = ErrorData { kind: kind.into(), reason: None, fields };
    RpcError { data: Some(Box::new(data)), ..RpcError::failed(kind, why) }
}

/// The account its access is to is signed out, through the account plugin `with`.
fn signed_out(why: String, need: &AccountNeed, scopes: &[String], with: &str) -> RpcError {
    let mut e = failed("access", why, need, scopes, vec![with.into()]);
    e.data.iter_mut().for_each(|d| d.reason = Some("signed_out".into()));
    e
}

impl Plugin {
    /// The access a `method` request to `url` (as the plugin wrote it) may use of the type `need`
    /// asks for: the one named `access`, or its default for the type (see the module's docs).
    pub(crate) fn usable(
        &self,
        live: &Live,
        need: &AccountNeed,
        access: Option<&str>,
        method: &str,
        url: &Url,
    ) -> Result<Usable, RpcError> {
        let kind = &need.name;
        let no_access = |why: String| failed("access", why, need, &need.scopes, self.host.providing(kind));
        let Some(store) = live.store.as_ref() else {
            return Err(no_access(format!("it has no access to a {kind} account without a store")));
        };
        let book = kept::read(store).map_err(|e| RpcError::failed("external", e.to_string()))?;
        let mut its = book.accesses.iter().filter(|a| a.plugin == self.manifest.id && a.kind == *kind);
        let a = match access {
            Some(id) => its.find(|a| a.id == id).ok_or_else(|| no_access(format!("it has no {kind} access {id}")))?,
            None => {
                its.find(|a| a.default).ok_or_else(|| no_access(format!("it has no access to a {kind} account")))?
            }
        };
        let (on, at) = (&a.account.plugin, &a.name);
        let gone = || signed_out(format!("the {kind} account {at} is signed out: sign in again"), need, &a.scopes, on);
        let k = book.account(on, &a.account.subject).ok_or_else(gone)?;
        let with = self.host.account_plugin(on);
        let t = with.as_ref().and_then(|w| w.account_type(kind).ok());
        let (Some(with), Some(t)) = (with.clone(), t) else {
            return Err(no_access(format!(
                "the {kind} account {at} was signed in through {on}, which isn't installed"
            )));
        };
        let scope_failed = |why: String, scopes: &[String]| failed("scope", why, need, scopes, vec![on.clone()]);
        let missing: Vec<String> = need.scopes.iter().filter(|s| !a.scopes.contains(s)).cloned().collect();
        if !missing.is_empty() {
            let why = format!("its access to the {kind} account {at} doesn't have every scope it needs: give it again");
            return Err(scope_failed(why, &missing));
        }
        let scopes = || a.scopes.iter().filter_map(|n| Some((n, t.scopes.get(n)?)));
        let covering: Vec<(&String, &Scope)> = scopes().filter(|(_, s)| s.covers(method, url)).collect();
        if covering.is_empty() {
            let why = format!("none of its scopes on the {kind} account {at} covers {method} {url}");
            return Err(scope_failed(why, &a.scopes));
        }
        let is_granted = |s: &Scope| s.asks.iter().all(|asks| k.scopes.contains(asks));
        if !covering.iter().any(|(_, s)| is_granted(s)) {
            let names: Vec<String> = covering.iter().map(|(n, _)| n.to_string()).collect();
            let why = format!("the {kind} account {at} wasn't granted what {} asks: sign in again", names.join(", "));
            return Err(scope_failed(why, &names));
        }
        let scopes = scopes().filter(|(_, s)| is_granted(s)).map(|(_, s)| s.clone()).collect();
        Ok(Usable { with, subject: k.subject.clone(), place: t.token.clone(), scopes })
    }

    /// The token of `usable`'s account, told to [`crate::plugin::net::State::scrub`]: `access`
    /// (`signed_out`) when it's signed out, or the service signs it out.
    pub(crate) fn usable_token(&self, usable: &Usable, need: &AccountNeed, store: &Store) -> Result<String, RpcError> {
        let token = usable.with.account_token(store, &usable.subject).map_err(|e| {
            let why = self.net.scrub(&e.to_string());
            match e.reason() {
                Some(Reason::SessionEnded) => signed_out(why, need, &need.scopes, &usable.with.manifest.id),
                _ => RpcError::failed("external", why),
            }
        })?;
        self.net.saw(&token);
        Ok(token)
    }
}
