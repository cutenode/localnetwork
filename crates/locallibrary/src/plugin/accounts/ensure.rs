//! Which of an account's scopes a plugin may use (`accounts/ensure`), each of them one its manifest
//! asks for: those its access has and the account was granted what they ask; not those the user
//! (or the service) declined for the access before; and the rest as the user says, asked by the
//! front end (see [`AskAccess`]). Allowed, the access is given them, signing in again first when
//! the account is signed out or wasn't granted what they ask; declined, the access remembers it,
//! and the plugin isn't asked about them again until the user gives them. Where nobody can be
//! asked, it fails as `scope`, `needs` naming them, which passed on says how to give them. Without
//! an access of the type it fails as a request would (`access`). Never a token.

use std::sync::Arc;

use loclib_plugin::{AccountNeed, EnsureAccount, Ensured, RpcError, params};
use serde_json::Value;

use super::PENDING_LIMIT;
use super::access::Access;
use super::kept;
use super::token::{about, failed};
use crate::Error;
use crate::api::RpcFailure;
use crate::plugin::callbacks::{for_plugin, known_roots};
use crate::plugin::process::{Live, Plugin};
use crate::progress::Ctx;
use crate::store::Store;

/// What a plugin asks to use of an account it has access to, for the user to allow.
#[derive(Debug, Clone, PartialEq)]
pub struct AccessQuestion {
    /// The plugin, as it's shown.
    pub plugin: String,
    /// Its access to the account.
    pub access: Access,
    /// The account type's title.
    pub title: String,
    /// The scopes it asks for, by name, and their titles.
    pub scopes: Vec<String>,
    pub titles: Vec<String>,
    /// Why it asks for them, as its manifest says.
    pub why: Vec<String>,
    /// Allowing them signs in again first: the account is signed out, or wasn't granted what they
    /// ask.
    pub sign_in: bool,
}

/// How a front end asks the user whether a plugin may use more of an account it has access to
/// (see [`crate::plugin::Options::ask`]).
pub trait AskAccess: Send + Sync {
    /// Whether the user allows what `question` asks: `None` when they can't be asked here.
    fn allow(&self, question: &AccessQuestion) -> Option<bool>;

    /// Opens `url` in the browser to sign in again (or says to): the host waits for the browser
    /// to come back.
    fn sign_in(&self, url: &str);
}

/// Where the scopes a plugin asks about stand, as the store has them now.
struct Standing {
    /// The account plugin that signed in the access's account.
    with: Arc<Plugin>,
    granted: Vec<String>,
    declined: Vec<String>,
    /// The others, for the user to allow.
    question: AccessQuestion,
}

impl Plugin {
    /// Answers `accounts/ensure` (`asked`, an [`EnsureAccount`]) with [`Ensured`], asking the user
    /// as the module's docs say; cancelled through `ctx`. A scope it doesn't ask for is refused.
    pub(crate) fn ensure(&self, live: &Live, asked: Value, ctx: &Ctx<'_>) -> Result<Value, RpcError> {
        let req: EnsureAccount = params(asked)?;
        let refused = |e: Error| RpcError::invalid_params(e.to_string());
        self.given(&req.kind, &req.scopes).map_err(refused)?;
        let need = self.needs(&req.kind).map_err(refused)?;
        let mut now = self.standing(live, need, &req)?;
        if !now.question.scopes.is_empty() {
            self.ask(live, need, now, ctx)?;
            now = self.standing(live, need, &req)?;
        }
        let declined = now.declined.into_iter().chain(now.question.scopes).collect();
        Ok(serde_json::to_value(Ensured { granted: now.granted, declined }).expect("an answer is JSON"))
    }

    /// Where the scopes `req` asks about stand (see [`Standing`]).
    fn standing(&self, live: &Live, need: &AccountNeed, req: &EnsureAccount) -> Result<Standing, RpcError> {
        self.with_access(live, need, req.access.as_deref(), |a, k, with, t| {
            let serviced = |name: &String| {
                let asks = t.scopes.get(name).map(|s| &s.asks);
                k.is_some_and(|k| asks.is_some_and(|asks| asks.iter().all(|x| k.scopes.contains(x))))
            };
            let (granted, rest): (Vec<String>, Vec<String>) =
                req.scopes.iter().cloned().partition(|s| a.scopes.contains(s) && serviced(s));
            let (declined, open): (Vec<String>, Vec<String>) = rest.into_iter().partition(|s| a.declined.contains(s));
            let titles = open.iter().map(|s| t.scopes.get(s).map_or(s, |s| &s.title).clone()).collect();
            let groups =
                std::iter::once((&need.scopes, &need.why)).chain(need.optional.iter().map(|o| (&o.scopes, &o.why)));
            let why: Vec<String> = groups
                .filter(|(scopes, _)| scopes.iter().any(|s| open.contains(s)))
                .map(|(_, why)| why.clone())
                .collect();
            let question = AccessQuestion {
                plugin: self.manifest.name.clone(),
                access: a.clone(),
                title: t.title.clone(),
                sign_in: !open.iter().all(serviced),
                scopes: open,
                titles,
                why,
            };
            Ok(Standing { with: with.clone(), granted, declined, question })
        })
    }

    /// Asks the user about `s`'s question, and does as they say: gives the access its scopes
    /// (signing in again first when it has to) or remembers they declined them. Fails as the
    /// module's docs say when nobody can be asked.
    fn ask(&self, live: &Live, need: &AccountNeed, s: Standing, ctx: &Ctx<'_>) -> Result<(), RpcError> {
        let Standing { with, question: q, .. } = s;
        let store = live.store.as_ref().expect("an access is to an account a store keeps");
        let asked = self.host.options.ask.as_ref().and_then(|ask| Some((ask, ask.allow(&q)?)));
        let Some((ask, allowed)) = asked else { return Err(unasked(need, &q)) };
        let (id, scopes) = (&q.access.id, &q.scopes);
        let external = |e: Error| for_plugin(RpcFailure::from(e), &known_roots(Some(store)));
        if !allowed {
            return remember(store, id, |a| a.more(&[], scopes)).map_err(external);
        }
        if !q.sign_in {
            return remember(store, id, |a| a.more(scopes, &[])).map_err(external);
        }
        let account = &q.access.account.subject;
        let start = with.begin_sign_in(&q.access.kind, Some(self), Some(account), scopes, store).map_err(external)?;
        ask.sign_in(&start.url);
        self.host.finish_sign_in(&start.id, PENDING_LIMIT, ctx).map(drop).map_err(external)
    }
}

/// Changes the access `id` in `store` with `f`.
fn remember(store: &Store, id: &str, f: impl FnMut(&mut Access)) -> crate::Result<()> {
    kept::update(store, |book| {
        book.accesses.iter_mut().filter(|a| a.id == id).for_each(f);
        Ok(())
    })
}

/// What a plugin is answered when nobody can be asked `q`: `scope`, naming `q`'s scopes, which
/// giving the access or signing in again `needs` to allow.
fn unasked(need: &AccountNeed, q: &AccessQuestion) -> RpcError {
    let a = &q.access;
    let why =
        format!("it may not {} with the {} account {} until it's given them", q.titles.join(", "), need.name, a.name);
    about(failed("scope", why, need, &q.scopes, vec![a.account.plugin.clone()]), a, &q.scopes)
}
