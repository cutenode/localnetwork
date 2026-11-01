//! `accounts.*`: the accounts a store keeps, signed in to in the browser through account plugins
//! (the host keeps the tokens, never answered) and out of, and the access plugins are given to
//! them.

use std::time::Duration;

use locallibrary::api::{Cancelled, NoParams};
use locallibrary::plugin::AccountId;
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
    /// An account the store keeps, by subject, to sign in to again (asking for more scopes, say).
    #[serde(default)]
    account: Option<String>,
    /// The type's scopes to ask for besides, by name.
    #[serde(default)]
    scopes: Vec<String>,
}

#[derive(Deserialize, JsonSchema)]
struct FinishSignIn {
    /// What its `sign_in.begin` answered.
    id: String,
    /// How long to wait for the browser (default 300, an hour at most).
    #[serde(default)]
    timeout_secs: Option<u64>,
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

pub(super) fn register(r: &mut Registry) {
    r.read("accounts.list", |c, _: NoParams| c.or_empty(|store| host()?.accounts_and_accesses(store))).quick();
    r.immediate("accounts.sign_in.begin", |c, req: BeginSignIn| {
        let plugins = host()?;
        let with = plugins.provider(&req.kind, req.with.as_deref())?;
        let (for_plugin, again) = (req.for_plugin.as_deref(), req.account.as_deref());
        Ok(plugins.begin_account_sign_in(&req.kind, &with, for_plugin, again, &req.scopes, &c.store()?)?)
    })
    .keeps_cache();
    r.immediate("accounts.sign_in.finish", |c, req: FinishSignIn| {
        Ok(host()?.finish_account_sign_in(&req.id, req.timeout(), c.ctx)?)
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
    r.immediate("accounts.access.set_default", |c, req: AccessRef| {
        Ok(host()?.set_default_access(&req.plugin, &req.account, &c.store()?)?)
    })
    .keeps_cache();
    r.immediate("accounts.access.take_away", |c, req: AccessRef| {
        Ok(host()?.take_away_access(&req.plugin, &req.account, &c.store()?)?)
    })
    .keeps_cache();
}
