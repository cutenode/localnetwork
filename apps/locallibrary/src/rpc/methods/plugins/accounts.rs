//! `plugins.accounts` and `plugins.account.*`: plugins' accounts on other services, signed in to
//! in the browser (the host keeps the tokens, never answered) and out of.

use std::time::Duration;

use locallibrary::api::Cancelled;
use schemars::JsonSchema;
use serde::Deserialize;

use super::PluginRef;
use crate::plugins::host;
use crate::rpc::methods::Registry;

#[derive(Deserialize, JsonSchema)]
struct AccountRef {
    plugin: String,
    /// The account's id, as the plugin's manifest has it.
    account: String,
}

#[derive(Deserialize, JsonSchema)]
struct FinishSignIn {
    /// What `plugins.account.sign_in.begin` answered.
    id: String,
    /// How long to wait for the browser (default 300, an hour at most).
    #[serde(default)]
    timeout_secs: Option<u64>,
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

pub(super) fn register(r: &mut Registry) {
    r.read("plugins.accounts", |c, req: PluginRef| Ok(host()?.accounts(&req.plugin, c.existing()?.as_ref())?)).quick();
    r.immediate("plugins.account.sign_in.begin", |c, req: AccountRef| {
        Ok(host()?.begin_sign_in(&req.plugin, &req.account, &c.store()?)?)
    })
    .keeps_cache();
    r.immediate("plugins.account.sign_in.finish", |c, req: FinishSignIn| {
        let timeout = Duration::from_secs(req.timeout_secs.unwrap_or(300));
        Ok(host()?.finish_sign_in(&req.id, timeout, c.ctx)?)
    })
    .long()
    .keeps_cache();
    r.immediate("plugins.account.sign_in.paste", |_, req: PasteSignIn| {
        Ok(host()?.finish_sign_in_with_url(&req.id, &req.url)?)
    })
    .keeps_cache();
    r.immediate("plugins.account.sign_in.cancel", |_, req: CancelSignIn| {
        Ok(Cancelled { cancelled: host()?.cancel_sign_in(&req.id) })
    })
    .quick()
    .keeps_cache();
    r.immediate("plugins.account.sign_out", |c, req: AccountRef| {
        Ok(host()?.sign_out(&req.plugin, &req.account, &c.store()?)?)
    })
    .keeps_cache();
}
