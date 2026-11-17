//! `accounts.*`: the accounts a store keeps, signed in to through account plugins in the browser
//! or with keys (the host keeps the tokens and keys, never answered) and out of, and the access
//! plugins are given to them, which signing out removes and signing in again restores.

use std::time::Duration;

use indexmap::IndexMap;
use locallibrary::api::{Cancelled, NoParams};
use locallibrary::plugin::{AccountId, Chosen, Restoring};
use schemars::JsonSchema;
use serde::Deserialize;

use crate::plugins::host;
use crate::rpc::methods::Registry;

#[derive(Deserialize, JsonSchema)]
struct BeginSignIn {
    /// The account type (`google`).
    #[serde(rename = "type")]
    kind: String,
    /// The account plugin to sign in through [default: the only one providing the type].
    #[serde(default)]
    with: Option<String>,
    /// The plugin it's for, given access to it: to the scopes it needs, and those of `scopes` it
    /// can do without.
    #[serde(default, rename = "for")]
    for_plugin: Option<String>,
    /// An account the store keeps, or one signed out that plugins had access to, by subject, to
    /// sign in to again (asking for more scopes, say).
    #[serde(default)]
    account: Option<String>,
    /// The type's scopes to ask for besides, by name.
    #[serde(default)]
    scopes: Vec<String>,
    /// What signing in does with the access plugins had to the account before it was signed out:
    /// `restore` gives it all back, `reset` forgets it. Without, Automatically Restore Access
    /// decides, else it stays off and the answer's `remembered` lists it, for
    /// `accounts.access.restore`.
    #[serde(default)]
    restore: Option<Restoring>,
}

#[derive(Deserialize, JsonSchema)]
struct FinishSignIn {
    /// What its `sign_in.begin` answered.
    id: String,
    /// How long to wait for the browser (default 300, an hour at most).
    #[serde(default)]
    timeout_secs: Option<u64>,
    /// For a sign-in with keys, which finishes at once: each key its `sign_in.begin` listed, by
    /// name, as the user entered it. The host keeps them in the keychain and never answers them;
    /// pass them straight from the user's input, nowhere else.
    #[serde(default)]
    keys: Option<IndexMap<String, String>>,
    /// With `keys`, when its `sign_in.begin` answered `named`: what the user calls the account.
    #[serde(default)]
    name: Option<String>,
}

impl FinishSignIn {
    fn timeout(&self) -> Duration {
        Duration::from_secs(self.timeout_secs.unwrap_or(300))
    }
}

#[derive(Deserialize, JsonSchema)]
struct PasteSignIn {
    id: String,
    /// The address the browser ended up on after approving.
    url: String,
}

#[derive(Deserialize, JsonSchema)]
struct CancelSignIn {
    id: String,
}

#[derive(Deserialize, JsonSchema)]
struct SignOut {
    account: AccountId,
}

#[derive(Deserialize, JsonSchema)]
struct ListAccesses {
    /// Only this plugin's.
    #[serde(default)]
    plugin: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
struct AccessRef {
    /// The plugin's id.
    plugin: String,
    account: AccountId,
}

#[derive(Deserialize, JsonSchema)]
struct RestoreAccess {
    account: AccountId,
    /// The plugins to give their access back, with the scopes each gets: those left out are
    /// forgotten, and none resets it. Without, every plugin gets its access back as it was.
    #[serde(default)]
    plugins: Option<Vec<Chosen>>,
}

#[derive(Deserialize, JsonSchema)]
struct RestoreAutomatically {
    /// Only this account [default: every account without a setting of its own].
    #[serde(default)]
    account: Option<AccountId>,
    on: bool,
}

#[derive(Deserialize, JsonSchema)]
struct GiveAccess {
    /// The plugin's id.
    plugin: String,
    account: AccountId,
    /// Scopes of the account's type it can do without to give it besides those it needs, by name.
    #[serde(default)]
    scopes: Vec<String>,
}

pub(super) fn register(r: &mut Registry) {
    r.read("accounts.list", |c, _: NoParams| c.or_empty(|store| host()?.accounts_and_accesses(store))).quick();
    r.immediate("accounts.sign_in.begin", |c, req: BeginSignIn| {
        let plugins = host()?;
        let with = plugins.provider(&req.kind, req.with.as_deref())?;
        let (for_plugin, again) = (req.for_plugin.as_deref(), req.account.as_deref());
        Ok(plugins.begin_account_sign_in(&req.kind, &with, for_plugin, again, &req.scopes, req.restore, &c.store()?)?)
    })
    .keeps_cache();
    r.immediate("accounts.sign_in.finish", |c, req: FinishSignIn| {
        let plugins = host()?;
        match &req.keys {
            Some(keys) => Ok(plugins.finish_account_sign_in_with_keys(&req.id, keys, req.name.as_deref())?),
            None => Ok(plugins.finish_account_sign_in(&req.id, req.timeout(), c.ctx)?),
        }
    })
    .long()
    .keeps_cache();
    r.immediate("accounts.sign_in.paste", |_, req: PasteSignIn| {
        Ok(host()?.finish_account_sign_in_with_url(&req.id, &req.url)?)
    })
    .keeps_cache();
    r.immediate("accounts.sign_in.cancel", |_, req: CancelSignIn| {
        Ok(Cancelled { cancelled: host()?.cancel_sign_in(&req.id) })
    })
    .quick()
    .keeps_cache();
    r.immediate("accounts.sign_out", |c, req: SignOut| {
        Ok(host()?.sign_out_account(&req.account.plugin, &req.account.subject, &c.store()?)?)
    })
    .keeps_cache();
    r.read("accounts.access.list", |c, req: ListAccesses| {
        c.or_empty(|store| host()?.accesses(store, req.plugin.as_deref(), None))
    })
    .quick();
    r.immediate("accounts.access.give", |c, req: GiveAccess| {
        Ok(host()?.give_access(&req.plugin, &req.account, &req.scopes, &c.store()?)?)
    })
    .keeps_cache();
    r.immediate("accounts.access.set_default", |c, req: AccessRef| {
        Ok(host()?.set_default_access(&req.plugin, &req.account, &c.store()?)?)
    })
    .keeps_cache();
    r.immediate("accounts.access.take_away", |c, req: AccessRef| {
        Ok(host()?.take_away_access(&req.plugin, &req.account, &c.store()?)?)
    })
    .keeps_cache();
    r.immediate("accounts.access.restore", |c, req: RestoreAccess| {
        Ok(host()?.restore_access(&req.account, req.plugins.as_deref(), &c.store()?)?)
    })
    .keeps_cache();
    r.immediate("accounts.restore_automatically", |c, req: RestoreAutomatically| {
        Ok(host()?.restore_automatically(req.account.as_ref(), req.on, &c.store()?)?)
    })
    .keeps_cache();
}
