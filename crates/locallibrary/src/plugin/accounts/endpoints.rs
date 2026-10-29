//! What signing in to an account and refreshing and revoking its tokens take from a manifest,
//! whichever declares it: a plugin's [`Account`] or an account plugin's [`AccountType`].

use loclib_plugin::{Account, AccountType};

/// An account's endpoints and client at its service, and what it's shown as.
pub(super) struct Endpoints<'a> {
    pub title: &'a str,
    pub authorize_url: &'a str,
    pub token_url: Option<&'a str>,
    /// Local Network's token exchange, in `token_url`'s place.
    pub token_exchange: Option<&'a str>,
    pub client_id: &'a str,
    pub revoke_url: Option<&'a str>,
    pub redirect_port: Option<u16>,
}

impl<'a> From<&'a Account> for Endpoints<'a> {
    fn from(a: &'a Account) -> Endpoints<'a> {
        Endpoints {
            title: &a.title,
            authorize_url: &a.authorize_url,
            token_url: a.token_url.as_deref(),
            token_exchange: a.token_exchange.as_deref(),
            client_id: &a.client_id,
            revoke_url: a.revoke_url.as_deref(),
            redirect_port: a.redirect_port,
        }
    }
}

impl<'a> From<&'a AccountType> for Endpoints<'a> {
    fn from(t: &'a AccountType) -> Endpoints<'a> {
        Endpoints {
            title: &t.title,
            authorize_url: &t.authorize_url,
            token_url: t.token_url.as_deref(),
            token_exchange: t.token_exchange.as_deref(),
            client_id: &t.client_id,
            revoke_url: t.revoke_url.as_deref(),
            redirect_port: t.redirect_port,
        }
    }
}
