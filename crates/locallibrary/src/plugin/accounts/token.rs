//! Which token a `net/fetch` request naming an account by type gets (`{account:<type>}`, or
//! `{account:<type>/<access>}`): the token of the account the plugin's access names, its default
//! for the type or the access named, only when one of the access's scopes covers the request and
//! the account was granted what that scope asks of the service, and the access has every scope the
//! plugin needs. Anything else fails before the request is sent: `access` when the plugin has no
//! such access or the account is signed out (`reason` `signed_out`), `scope` when its scopes don't
//! reach, with the type, the scopes and the account plugins in its fields, and once there's an
//! access, its id, its account and the scopes signing in again `needs` to allow. A service
//! refusing the token is answered here too: it's refreshed for the request to be sent again (see
//! [`Plugin::refused_token`]), signed out (see [`Plugin::invalid_token`]), or no longer granted
//! the scopes the service says it lacks (see [`Plugin::scope_refused`]).

use std::sync::Arc;

use loclib_plugin::{AccountNeed, ErrorData, RpcError, Scope, TokenPlace};
use reqwest::Url;
use serde_json::{Map, json};

use super::access::Access;
use super::kept;
use crate::Error;
use crate::error::Reason;
use crate::plugin::process::{Live, Plugin};
use crate::store::Store;

/// An access a request may use, of the type `need` asks for: where its account's token goes, and
/// the scopes (by name) it may use it for (those the account was granted what they ask), which a
/// redirect has to stay within.
pub(crate) struct Usable {
    /// The account plugin, which gets the token.
    with: Arc<Plugin>,
    access: Access,
    need: AccountNeed,
    pub(crate) place: TokenPlace,
    pub(crate) scopes: Vec<(String, Scope)>,
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

/// The account of the access `a` is signed out, which signing in again gives back every scope of.
fn signed_out(why: String, need: &AccountNeed, a: &Access) -> RpcError {
    let mut e = about(failed("access", why, need, &a.scopes, vec![a.account.plugin.clone()]), a, &a.scopes);
    e.data.iter_mut().for_each(|d| d.reason = Some("signed_out".into()));
    e
}

/// `e`, naming the access `a` it's about, its account, and the scopes signing in again `needs` to
/// allow.
fn about(mut e: RpcError, a: &Access, needs: &[String]) -> RpcError {
    for d in e.data.iter_mut() {
        d.fields.insert("access".into(), json!(a.id));
        d.fields.insert("account".into(), json!(a.account));
        d.fields.insert("needs".into(), json!(needs));
    }
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
        let gone = || signed_out(format!("the {kind} account {at} is signed out: sign in again"), need, a);
        let k = book.account(on, &a.account.subject).ok_or_else(gone)?;
        let with = self.host.account_plugin(on);
        let t = with.as_ref().and_then(|w| w.account_type(kind).ok());
        let (Some(with), Some(t)) = (with.clone(), t) else {
            let why = format!("the {kind} account {at} was signed in through {on}, which isn't installed");
            return Err(about(no_access(why), a, &[]));
        };
        // Each naming the scopes signing in again needs to allow: none, when it can't help.
        let scope_failed = |why: String, scopes: &[String], needs: &[String]| {
            about(failed("scope", why, need, scopes, vec![on.clone()]), a, needs)
        };
        let missing: Vec<String> = need.scopes.iter().filter(|s| !a.scopes.contains(s)).cloned().collect();
        if !missing.is_empty() {
            let why = format!("its access to the {kind} account {at} doesn't have every scope it needs: give it again");
            return Err(scope_failed(why, &missing, &missing));
        }
        let scopes = || a.scopes.iter().filter_map(|n| Some((n, t.scopes.get(n)?)));
        let covering: Vec<(&String, &Scope)> = scopes().filter(|(_, s)| s.covers(method, url)).collect();
        if covering.is_empty() {
            let why = format!("none of its scopes on the {kind} account {at} covers {method} {url}");
            return Err(scope_failed(why, &a.scopes, &[]));
        }
        let is_granted = |s: &Scope| s.asks.iter().all(|asks| k.scopes.contains(asks));
        if !covering.iter().any(|(_, s)| is_granted(s)) {
            let names: Vec<String> = covering.iter().map(|(n, _)| n.to_string()).collect();
            let why = format!("the {kind} account {at} wasn't granted what {} asks: sign in again", names.join(", "));
            return Err(scope_failed(why, &names, &names));
        }
        let scopes = scopes().filter(|(_, s)| is_granted(s)).map(|(n, s)| (n.clone(), s.clone())).collect();
        Ok(Usable { with, access: a.clone(), need: need.clone(), place: t.token.clone(), scopes })
    }

    /// The token of `usable`'s account, told to [`crate::plugin::net::State::scrub`]: `access`
    /// (`signed_out`) when it's signed out, or the service signs it out.
    pub(crate) fn usable_token(&self, usable: &Usable, store: &Store) -> Result<String, RpcError> {
        let token = usable.with.account_token(store, &usable.access.account.subject);
        let token = token.map_err(|e| self.token_failed(usable, e))?;
        self.net.saw(&token);
        Ok(token)
    }

    /// `e`, which getting `usable`'s token failed with, as the plugin is told it.
    fn token_failed(&self, usable: &Usable, e: Error) -> RpcError {
        let why = self.net.scrub(&e.to_string());
        match e.reason() {
            Some(Reason::SessionEnded) => signed_out(why, &usable.need, &usable.access),
            _ => RpcError::failed("external", why),
        }
    }

    /// Its service refused `token`, `usable`'s account's: the account's token is refreshed, unless
    /// another host already has (see [`Plugin::account_token_refused`]), for the request to be
    /// sent again with it. `access` (`signed_out`) when the service won't refresh it.
    pub(crate) fn refused_token(&self, usable: &Usable, token: &str, store: &Store) -> Result<(), RpcError> {
        let subject = &usable.access.account.subject;
        usable.with.account_token_refused(store, subject, token).map(drop).map_err(|e| self.token_failed(usable, e))
    }

    /// Its service refused `token` again, saying it's no good: `usable`'s account is signed out,
    /// unless another host signed it in again since.
    pub(crate) fn invalid_token(&self, usable: &Usable, token: &str, store: &Store) -> Result<(), RpcError> {
        let subject = &usable.access.account.subject;
        usable.with.forget_refused(store, subject, token).map_err(|e| RpcError::failed("external", e.to_string()))
    }

    /// Its service said `usable`'s token hasn't the scopes a `method` request to `url` (as the
    /// plugin wrote it) needs, which it calls `asked` when it says: `scope`, naming the type's
    /// scopes whose asks are among them (else those of `usable`'s covering the request), which
    /// signing in again `needs` to allow. What the account was granted no longer has `asked` (else
    /// what those ask), so nothing through them is sent again until it's signed in again.
    pub(crate) fn scope_refused(
        &self,
        usable: &Usable,
        asked: &[String],
        method: &str,
        url: &Url,
        store: &Store,
    ) -> RpcError {
        let (kind, at, on) = (&usable.need.name, &usable.access.name, &usable.access.account.plugin);
        let t = usable.with.account_type(kind).ok();
        let meeting = t.iter().flat_map(|t| &t.scopes).filter(|(_, s)| s.asks.iter().any(|a| asked.contains(a)));
        let mut needs: Vec<(&String, &Scope)> = meeting.collect();
        if needs.is_empty() {
            needs = usable.scopes.iter().filter(|(_, s)| s.covers(method, url)).map(|(n, s)| (n, s)).collect();
        }
        let ungranted: Vec<String> = match asked {
            [] => needs.iter().flat_map(|(_, s)| s.asks.iter().cloned()).collect(),
            asked => asked.to_vec(),
        };
        let needs: Vec<String> = needs.into_iter().map(|(n, _)| n.clone()).collect();
        if let Err(e) = usable.with.not_granted(store, &usable.access.account.subject, &ungranted) {
            return RpcError::failed("external", e.to_string());
        }
        let why = format!(
            "the service says the {kind} account {at} wasn't granted what {} asks: sign in again",
            needs.join(", ")
        );
        about(failed("scope", why, &usable.need, &needs, vec![on.clone()]), &usable.access, &needs)
    }
}
