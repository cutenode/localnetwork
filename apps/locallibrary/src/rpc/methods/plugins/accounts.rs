//! `plugins.accounts` and `plugins.account.*`: plugins' accounts on other services, signed in to
//! in the browser (the host keeps the tokens, never answered) and out of.

use locallibrary::api::Cancelled;
use schemars::JsonSchema;
use serde::Deserialize;

use super::PluginRef;
use crate::plugins::host;
use crate::rpc::methods::Registry;
use crate::rpc::methods::accounts::{CancelSignIn, FinishSignIn, PasteSignIn};

#[derive(Deserialize, JsonSchema)]
struct AccountRef {
    plugin: String,
    /// The account's id, as the plugin's manifest has it.
    account: String,
}

pub(super) fn register(r: &mut Registry) {
    r.read("plugins.accounts", |c, req: PluginRef| Ok(host()?.accounts(&req.plugin, c.existing()?.as_ref())?)).quick();
    r.immediate("plugins.account.sign_in.begin", |c, req: AccountRef| {
        Ok(host()?.begin_sign_in(&req.plugin, &req.account, &c.store()?)?)
    })
    .keeps_cache();
    r.immediate("plugins.account.sign_in.finish", |c, req: FinishSignIn| {
        Ok(host()?.finish_sign_in(&req.id, req.timeout(), c.ctx)?)
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
