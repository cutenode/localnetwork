//! Accounts of a type, signed in to through the account plugins that provide it and kept once per
//! store (see [`kept`]), for plugins to be given access to. Signing in asks the service for what
//! the type always asks for, what the account was granted before and what every access to it
//! needs, and what the scopes asked for ask for; whoever signed in is then asked of the service
//! (see [`Plugin::whoami`]), and the same account signed in again through the same account plugin
//! is changed in place, keeping its refresh token when the service answers none.

use std::time::Duration;

use chrono::{DateTime, Utc};
use indexmap::IndexSet;
use loclib_plugin::{AccountType, SUBJECT_PLACEHOLDER};
use oauth2::TokenResponse;
use schemars::JsonSchema;
use serde::Serialize;

use super::access::Access;
use super::kept::{self, Kept};
use super::{Pending, Tokens, come_back, pasted};
use crate::callback::CallbackQuery;
use crate::error::Reason;
use crate::keychain::Storage;
use crate::plugin::Plugins;
use crate::plugin::process::{Host, Plugin};
use crate::plugin::secrets::secret;
use crate::progress::Ctx;
use crate::store::Store;
use crate::{Error, Result};

/// An account a store keeps, as hosts show it: never a token.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
#[schemars(rename = "StoreAccount")]
pub struct Account {
    #[serde(rename = "type")]
    pub kind: String,
    /// The account plugin it was signed in through.
    pub plugin: String,
    /// Its id at the service.
    pub subject: String,
    /// What it's shown as.
    pub name: String,
    /// What the service granted, as it names them.
    pub scopes: Vec<String>,
    /// When its access token expires, when the service said: it's refreshed before then when it
    /// can be.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires: Option<DateTime<Utc>>,
    /// It's refreshed without signing in again.
    pub refreshes: bool,
    /// When it signs itself out, when it does: as its access token expires when it isn't
    /// refreshed, else as its refresh token runs out, when the service said. Front ends warn
    /// within [`crate::plugin::WARN_DAYS`] of it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signs_out: Option<DateTime<Utc>>,
    /// Its tokens are kept (the keychain may have lost them).
    pub signed_in: bool,
}

impl Account {
    /// Whether it's signed in and signs itself out within [`crate::plugin::WARN_DAYS`].
    pub fn due_soon(&self) -> bool {
        self.signed_in && self.signs_out.is_some_and(super::due_soon)
    }
}

/// The accounts a store keeps and the accesses plugins were given to them, as hosts show them.
#[derive(Debug, Clone, Default, PartialEq, Serialize, JsonSchema)]
pub struct Accounts {
    /// Those it keeps, in the order they were first signed in, then those signed out that plugins
    /// still have access to.
    pub accounts: Vec<Account>,
    pub accesses: Vec<Access>,
}

/// A sign-in to an account of a type, waiting for the browser.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct AccountSignIn {
    /// Finishes it: [`Plugins::finish_account_sign_in`],
    /// [`Plugins::finish_account_sign_in_with_url`].
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String,
    /// The account plugin signing in.
    pub plugin: String,
    /// Open this in the browser.
    pub url: String,
    /// Where the browser comes back to.
    pub redirect_uri: String,
}

/// What signing in to an account of a type did.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct AccountSignedIn {
    #[serde(flatten)]
    pub account: Account,
    /// The account it was to sign in again, when the browser signed in to another: that one is
    /// kept too, and this one as it was. The plugin it was for isn't given access to it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instead_of: Option<String>,
    /// The access it gave the plugin it was signed in for, if it was.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub access: Option<Access>,
    /// The scopes (by name) asked for that plugin that the service didn't grant all they ask:
    /// those it can do without aren't in its access.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub declined: Vec<String>,
}

/// What signing out of an account of a type did.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct AccountSignedOut {
    /// The account, as it was.
    #[serde(flatten)]
    pub account: Account,
    /// The service was told to forget its tokens (it has a revocation endpoint, which answered).
    pub revoked: bool,
}

/// What a sign-in to an account of a type asks for (see [`Plugins::begin_account_sign_in`]).
pub(crate) struct Asked {
    kind: String,
    /// The account plugin signing in.
    with: String,
    /// The plugin it's for, the type's scopes (by name) it's given of the account as far as the
    /// service grants them, and those it needs, which it's given whatever the service grants.
    for_plugin: Option<(String, Vec<String>, Vec<String>)>,
    /// The account signed in again, by subject.
    account: Option<String>,
    /// The service's scopes.
    scopes: Vec<String>,
}

/// `kept` as hosts show it.
fn listed(kept: Kept, signed_in: bool) -> Account {
    Account {
        signs_out: kept.signs_out(),
        kind: kept.kind,
        plugin: kept.plugin,
        subject: kept.subject,
        name: kept.name,
        scopes: kept.scopes,
        expires: kept.expires_at,
        refreshes: kept.refreshes,
        signed_in,
    }
}

/// The account `subject` that `with` signed in isn't kept.
pub(super) fn not_kept(with: &str, subject: &str) -> Error {
    Error::NotFound(format!("the account {subject:?} signed in through the {with} plugin"))
}

/// The account `subject` that `with` signed in, of those `store` keeps.
fn find(store: &Store, with: &str, subject: &str) -> Result<Kept> {
    kept::read(store)?.account(with, subject).cloned().ok_or_else(|| not_kept(with, subject))
}

impl Plugin {
    /// The account type `kind`, which it provides.
    pub(super) fn account_type(&self, kind: &str) -> Result<&AccountType> {
        self.declared("account type", kind, &self.manifest.contributes.account_types, |t| &t.name)
    }

    /// The tokens kept for `kept`, with its expiry and scopes, told to
    /// [`crate::plugin::net::State::scrub`].
    fn kept_tokens(&self, store: &Store, kept: &Kept) -> Result<Option<Tokens>> {
        let secret = secret(self.host.options.secrets, &kept::place(store, &kept.plugin, &kept.subject))?;
        let read = |s: String| {
            let tokens = self.read_tokens(&kept.name, &s)?;
            let (expires_at, refresh_expires_at) = (kept.expires_at, kept.refresh_expires_at);
            Ok(Tokens { expires_at, refresh_expires_at, scopes: kept.scopes.clone(), ..tokens })
        };
        secret.map(read).transpose()
    }

    /// The access token of the account `subject` it signed in to in `store`, refreshed first when
    /// it's about to expire: one refresh at a time, whichever host on the store asks, each reading
    /// the tokens again once it's its turn. One the service won't refresh any more is signed out.
    pub(crate) fn account_token(&self, store: &Store, subject: &str) -> Result<String> {
        self.token(store, subject, None)
    }

    /// The access token of the account `subject` it signed in to in `store` to use instead of
    /// `refused`, which its service refused: the one another host refreshed it to since, else
    /// `refused` refreshed (as [`Plugin::account_token`] refreshes). Without a refresh token, or
    /// one the service won't refresh any more, it's signed out.
    pub(crate) fn account_token_refused(&self, store: &Store, subject: &str, refused: &str) -> Result<String> {
        self.token(store, subject, Some(refused))
    }

    /// [`Plugin::account_token`], or with `refused`, [`Plugin::account_token_refused`].
    fn token(&self, store: &Store, subject: &str, refused: Option<&str>) -> Result<String> {
        let with = &self.manifest.id;
        let _one = kept::hold(store, with, subject)?;
        let k = find(store, with, subject)?;
        let t = self.account_type(&k.kind)?;
        let signed_out = || {
            let why = format!("the {} account {} is signed out: sign in again", t.title, k.name);
            Error::Account(why).because(Reason::SessionEnded)
        };
        let tokens = self.kept_tokens(store, &k)?.ok_or_else(signed_out)?;
        let stale = match refused {
            Some(refused) => tokens.access_token == refused,
            None => tokens.expiring(Utc::now()),
        };
        if !stale {
            return Ok(tokens.access_token);
        }
        let storage = self.host.options.secrets;
        let Some(refresh) = tokens.refresh_token.clone() else {
            if refused.is_some() {
                forget(store, storage, &k)?;
            }
            return Err(signed_out());
        };
        let Some(fresh) = self.refreshed(t, refresh, tokens)? else {
            forget(store, storage, &k)?;
            return Err(signed_out());
        };
        kept::update(store, |book| {
            book.accounts.iter_mut().filter(|x| x.is(with, subject)).for_each(|x| x.granted(&fresh));
            kept::keep(storage, store, &k, Some(&fresh))
        })?;
        Ok(fresh.access_token)
    }

    /// Signs out of the account `subject` it signed in to in `store` when its access token is
    /// still `refused`, which its service said is no good.
    pub(crate) fn forget_refused(&self, store: &Store, subject: &str, refused: &str) -> Result<()> {
        let _one = kept::hold(store, &self.manifest.id, subject)?;
        let Some(k) = kept::read(store)?.account(&self.manifest.id, subject).cloned() else { return Ok(()) };
        match self.kept_tokens(store, &k)? {
            Some(tokens) if tokens.access_token == refused => forget(store, self.host.options.secrets, &k),
            _ => Ok(()),
        }
    }

    /// The account `subject` it signed in to in `store` wasn't granted `asks` after all, as its
    /// service said.
    pub(crate) fn not_granted(&self, store: &Store, subject: &str, asks: &[String]) -> Result<()> {
        kept::update(store, |book| {
            let k = book.accounts.iter_mut().filter(|k| k.is(&self.manifest.id, subject));
            k.for_each(|k| k.scopes.retain(|s| !asks.contains(s)));
            Ok(())
        })
    }
}

impl Plugins {
    /// The accounts `store` keeps, in the order they were first signed in.
    pub fn store_accounts(&self, store: &Store) -> Result<Vec<Account>> {
        let listing = |k: Kept| {
            let signed_in = secret(self.host.options.secrets, &kept::place(store, &k.plugin, &k.subject))?.is_some();
            Ok(listed(k, signed_in))
        };
        kept::read(store)?.accounts.into_iter().map(listing).collect()
    }

    /// The accounts `store` keeps, as [`Plugins::store_accounts`] lists them but without asking
    /// where their tokens are kept (the keychain, which may stop to ask): each `signed_in`, as
    /// it is unless the keychain lost its tokens.
    pub fn kept_accounts(&self, store: &Store) -> Result<Vec<Account>> {
        Ok(kept::read(store)?.accounts.into_iter().map(|k| listed(k, true)).collect())
    }

    /// The accounts `store` keeps (see [`Plugins::store_accounts`]), then those signed out that
    /// plugins still have access to, signed out, and every access.
    pub fn accounts_and_accesses(&self, store: &Store) -> Result<Accounts> {
        let mut accounts = self.store_accounts(store)?;
        let accesses = kept::read(store)?.accesses;
        for a in &accesses {
            if !accounts.iter().any(|k| k.plugin == a.account.plugin && k.subject == a.account.subject) {
                accounts.push(Account {
                    kind: a.kind.clone(),
                    plugin: a.account.plugin.clone(),
                    subject: a.account.subject.clone(),
                    name: a.name.clone(),
                    scopes: Vec::new(),
                    expires: None,
                    refreshes: false,
                    signs_out: None,
                    signed_in: false,
                });
            }
        }
        Ok(Accounts { accounts, accesses })
    }

    /// The account plugins providing the account type `kind`, by id.
    pub fn providing(&self, kind: &str) -> Vec<String> {
        self.host.providing(kind)
    }

    /// The account plugin to sign in to an account of type `kind` through: `with`, or the only one
    /// providing the type.
    pub fn provider(&self, kind: &str, with: Option<&str>) -> Result<String> {
        if let Some(with) = with {
            return Ok(with.into());
        }
        match self.providing(kind).as_slice() {
            [] => Err(Error::NotFound(format!("an account plugin providing {kind} accounts"))),
            [only] => Ok(only.clone()),
            several => {
                let why = format!("several account plugins provide {kind} accounts ({}): name one", several.join(", "));
                Err(Error::invalid(why)
                    .because(Reason::MissingParam)
                    .with("param", "with")
                    .with("account_plugins", several))
            }
        }
    }

    /// Starts signing in to an account of type `kind` in `store` through the account plugin
    /// `with`, for the plugin `for_plugin` if it's for one: the address to open in the browser,
    /// which comes back to this computer. With `account`, it signs in again as that account (the
    /// subject of one `with` signed in, or of one signed out that plugins still have access to);
    /// without, the service is asked to offer another than those it signed in, when there are
    /// any; the type's own params go with either. It asks the service for what the type always
    /// asks for, what `account` was granted and what every access to it needs, and what `scopes`
    /// (the type's, by name) ask for, and for a plugin, what the scopes it needs ask for: it's
    /// given access to those, and to those of `scopes` it can do without that the service grants
    /// (see [`Plugins::give_access`]). Finished with [`Plugins::finish_account_sign_in`] (or
    /// [`Plugins::finish_account_sign_in_with_url`], when the browser runs on another computer),
    /// within a quarter of an hour.
    pub fn begin_account_sign_in(
        &self,
        kind: &str,
        with: &str,
        for_plugin: Option<&str>,
        account: Option<&str>,
        scopes: &[String],
        store: &Store,
    ) -> Result<AccountSignIn> {
        let for_plugin = for_plugin.map(|p| self.slot(p)).transpose()?;
        self.slot(with)?.begin_sign_in(kind, for_plugin.map(|s| &*s.plugin), account, scopes, store)
    }

    /// Waits for the browser to come back to sign-in `id` (at most `timeout`, an hour at most),
    /// trades its code for the account's tokens, asks the service whose they are, and keeps the
    /// account. The browser is told how it went.
    pub fn finish_account_sign_in(&self, id: &str, timeout: Duration, ctx: &Ctx<'_>) -> Result<AccountSignedIn> {
        self.host.finish_sign_in(id, timeout, ctx)
    }

    /// Finishes sign-in `id` with the address the browser ended up on, pasted from another
    /// computer.
    pub fn finish_account_sign_in_with_url(&self, id: &str, url: &str) -> Result<AccountSignedIn> {
        let p = self.host.pending(id)?;
        let query = pasted(&p, url)?;
        self.slot(&p.asked.with)?.signed_in(p, &query)
    }
}

impl Host {
    /// [`Plugins::finish_account_sign_in`].
    pub(super) fn finish_sign_in(&self, id: &str, timeout: Duration, ctx: &Ctx<'_>) -> Result<AccountSignedIn> {
        let p = self.pending(id)?;
        let with =
            self.account_plugin(&p.asked.with).ok_or_else(|| Error::NotFound(format!("plugin {:?}", p.asked.with)))?;
        come_back(p, timeout, ctx, |p, query| with.signed_in(p, query), |s| s.account.name.clone())
    }
}

impl Plugin {
    /// [`Plugins::begin_account_sign_in`] through it, for `for_plugin` if it's for one.
    pub(super) fn begin_sign_in(
        &self,
        kind: &str,
        for_plugin: Option<&Plugin>,
        account: Option<&str>,
        scopes: &[String],
        store: &Store,
    ) -> Result<AccountSignIn> {
        let with = self.manifest.id.as_str();
        let t = self.account_type(kind)?;
        let book = kept::read(store)?;
        let mut asking: IndexSet<String> = t.always.iter().cloned().collect();
        let mut params = t.params.clone();
        match account {
            Some(subject) => {
                let accesses: Vec<&Access> =
                    book.accesses.iter().filter(|a| a.account.plugin == with && a.account.subject == subject).collect();
                let k = book.account(with, subject);
                if k.is_none() && accesses.is_empty() {
                    return Err(not_kept(with, subject));
                }
                asking.extend(k.into_iter().flat_map(|k| k.scopes.iter().cloned()));
                let needed = accesses.iter().flat_map(|a| &a.scopes).filter_map(|name| t.scopes.get(name));
                asking.extend(needed.flat_map(|scope| scope.asks.iter().cloned()));
                params.extend(t.one_account.iter().map(|(k, v)| (k.clone(), v.replace(SUBJECT_PLACEHOLDER, subject))));
                // The keychain can be used, before the browser is sent anywhere. Only whether it
                // can, what it keeps dropped first: CodeQL follows a `?` on it into the sign-in
                // link.
                secret(self.host.options.secrets, &kept::place(store, with, subject)).map(drop)?;
            }
            None if book.accounts.iter().any(|k| k.plugin == with) => params.extend(t.another_account.clone()),
            None => {}
        }
        let given = for_plugin
            .map(|p| Ok::<_, Error>((p.manifest.id.clone(), p.given(kind, scopes)?, p.needs(kind)?.scopes.clone())));
        let given = given.transpose()?;
        for name in scopes.iter().chain(given.iter().flat_map(|(_, given, _)| given)) {
            let scope = t.scopes.get(name);
            let scope = scope.ok_or_else(|| Error::NotFound(format!("the {kind} account type's scope {name:?}")))?;
            asking.extend(scope.asks.iter().cloned());
        }
        let scopes: Vec<String> = asking.into_iter().collect();
        let asked = Asked {
            kind: kind.into(),
            with: with.into(),
            for_plugin: given,
            account: account.map(String::from),
            scopes: scopes.clone(),
        };
        let (id, url, redirect_uri) = self.begin(t, &scopes, &params, asked, store)?;
        Ok(AccountSignIn { id, kind: kind.into(), plugin: with.into(), url, redirect_uri })
    }

    /// The account `p` asked for, signed in through it from what the browser came back to `p`
    /// with.
    fn signed_in(&self, p: Pending, query: &CallbackQuery) -> Result<AccountSignedIn> {
        let asked = p.asked;
        let t = self.account_type(&asked.kind)?;
        let answer = self.traded(t, &p.client, p.verifier, query)?;
        let access_token = answer.access_token().secret();
        self.net.saw(access_token);
        let (subject, name) = self.whoami(t, access_token)?;
        let instead_of = asked.account.filter(|a| *a != subject);
        // The plugin it's for is given the scopes it needs whatever the service grants (a request
        // through one not granted says to sign in again), those it can do without only when
        // granted, and nothing when the browser signed in to another account than asked.
        let for_plugin = asked.for_plugin.filter(|_| instead_of.is_none());
        let (store, storage) = (&p.store, self.host.options.secrets);
        let _one = kept::hold(store, &asked.with, &subject)?;
        // Signed in again, it keeps the refresh token it had when the service answers none. One
        // that can't be read is signed in to anew.
        let had = kept::read(store)?.account(&asked.with, &subject).cloned();
        let had = had.and_then(|k| self.kept_tokens(store, &k).ok().flatten());
        let mut tokens = Tokens::from(&answer, had);
        // A service that doesn't say what it granted granted what it was asked (RFC 6749 5.1).
        if answer.scopes().is_none() {
            tokens.scopes = asked.scopes;
        }
        self.seen(&tokens);
        let (account, access, declined) = kept::update(store, |book| {
            let all = &mut book.accounts;
            let i = match all.iter().position(|k| k.is(&asked.with, &subject)) {
                Some(i) => i,
                None => {
                    all.push(Kept { plugin: asked.with.clone(), subject: subject.clone(), ..Kept::default() });
                    all.len() - 1
                }
            };
            let k = &mut all[i];
            (k.kind, k.name) = (asked.kind, name);
            k.granted(&tokens);
            kept::keep(storage, store, k, Some(&tokens))?;
            let k = k.clone();
            book.renamed(&k);
            let Some((plugin, scopes, needs)) = &for_plugin else { return Ok((listed(k, true), None, Vec::new())) };
            let is_granted =
                |name: &String| t.scopes.get(name).is_some_and(|s| s.asks.iter().all(|a| k.scopes.contains(a)));
            let declined: Vec<String> = scopes.iter().filter(|s| !is_granted(s)).cloned().collect();
            let given: Vec<String> =
                scopes.iter().filter(|s| needs.contains(s) || !declined.contains(s)).cloned().collect();
            let access = book.give(plugin, &k, &given, &declined);
            Ok((listed(k, true), Some(access), declined))
        })?;
        Ok(AccountSignedIn { account, instead_of, access, declined })
    }
}

impl Plugins {
    /// The access token of the account `subject` that `with` signed in to in `store`, as a
    /// plugin's request gets it (see [`Plugin::account_token`]).
    pub(crate) fn account_token(&self, store: &Store, with: &str, subject: &str) -> Result<String> {
        self.slot(with)?.account_token(store, subject)
    }

    /// Signs out of the account `subject` that `with` signed in to in `store`: it's forgotten here,
    /// and its tokens are revoked at the service when its type has a revocation endpoint (as far
    /// as it answers: they're forgotten here all the same). Plugins keep their accesses to it,
    /// which have no token until it's signed in again.
    pub fn sign_out_account(&self, with: &str, subject: &str, store: &Store) -> Result<AccountSignedOut> {
        let slot = self.slot(with)?;
        let _one = kept::hold(store, with, subject)?;
        let k = find(store, with, subject)?;
        let t = slot.account_type(&k.kind)?;
        let storage = self.host.options.secrets;
        let kept_secret = secret(storage, &kept::place(store, with, subject))?;
        // One that can't be read is forgotten all the same, with nothing to revoke it with.
        let tokens = kept_secret.as_deref().and_then(|s| slot.read_tokens(&k.name, s).ok());
        let revoked = tokens.is_some_and(|tokens| slot.revoke(t, &tokens));
        forget(store, storage, &k)?;
        Ok(AccountSignedOut { account: listed(k, kept_secret.is_some()), revoked })
    }
}

/// Forgets `kept` and its tokens, but not the accesses to it.
fn forget(store: &Store, storage: Storage, kept: &Kept) -> Result<()> {
    kept::update(store, |book| {
        book.accounts.retain(|k| !k.is(&kept.plugin, &kept.subject));
        kept::keep(storage, store, kept, None)
    })
}
