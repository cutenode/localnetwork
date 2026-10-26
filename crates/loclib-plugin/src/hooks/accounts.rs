//! Feature `accounts`: a plugin's accounts on other services, which the host signs the user in to
//! (OAuth 2's authorization code flow with PKCE, the browser coming back to this computer) and
//! keeps the tokens of. A plugin never sees a token: a `net/fetch` request names an account as its
//! `Authorization` header's value, `{account:<id>}`, which the host fills in with `Bearer` and the
//! account's access token once the request is checked, refreshing it first when it's about to
//! expire. A service whose token endpoint wants a client secret is reached through Local Network's
//! token exchange, which holds the secret: a secret can't ship inside an app people download.

use std::net::{Ipv4Addr, Ipv6Addr};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use url::{Host, Url};

use crate::manifest::{invalid, shown, unique_names};
use crate::{Capability, Feature, Manifest, ManifestError};

/// What a `net/fetch` request's `Authorization` header says to name an account: `{account:<id>}`.
pub const ACCOUNT_PLACEHOLDER: &str = "{account:";

/// Where Local Network's token exchange is, the only one an account may name: its path, then the
/// service's id. The host posts it what it would have sent the service's token endpoint, and the
/// website adds its client secret for the service and answers what the service did.
const EXCHANGE_ORIGIN: &str = "https://localnetwork.computer";
const EXCHANGE_PATH: &str = "/api/token-exchange/";

// Every plugin written with the SDK has it: what's an account in a request is the host's to fill.
pub(crate) const FEATURE: Feature =
    Feature { name: "accounts", methods: &[], sends: &[], contributed: |m| !m.contributes.accounts.is_empty() };

/// `[[contributes.accounts]]`: an account on a service, signed in to in the browser.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Account {
    /// How requests name it (`{account:<id>}`).
    pub id: String,
    pub title: String,
    /// The service's authorization endpoint, where the browser signs in (https).
    pub authorize_url: String,
    /// Its token endpoint, where the host trades the browser's code for tokens and refreshes
    /// them (https). An account has this or `token_exchange`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token_url: Option<String>,
    /// Local Network's token exchange for its service
    /// (`https://localnetwork.computer/api/token-exchange/<service>`), in `token_url`'s place, for
    /// a service that wants a client secret there.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token_exchange: Option<String>,
    /// What it asks the service for, as the service names them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub scopes: Vec<String>,
    /// The plugin's client at the service: a public one, without a secret, or Local Network's,
    /// whose secret its token exchange holds.
    pub client_id: String,
    /// Signing in uses PKCE: always, and a manifest saying otherwise is refused.
    #[serde(default = "pkce")]
    pub pkce: bool,
    /// Where its tokens are revoked at signing out (RFC 7009), when the service has one (https).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revoke_url: Option<String>,
}

fn pkce() -> bool {
    true
}

/// `url` as the host's HTTP client reads it, when an account's endpoint may be there: https, or
/// http on this computer for a test server (which the host reaches only in tests), with no user
/// name or password.
fn endpoint(url: &str) -> Option<Url> {
    let plain = !url.chars().any(|c| c.is_whitespace() || c.is_control());
    let parsed = Url::parse(url).ok().filter(|u| plain && u.username().is_empty() && u.password().is_none())?;
    let this_computer = match parsed.host()? {
        Host::Domain(name) => name == "localhost",
        Host::Ipv4(ip) => ip == Ipv4Addr::LOCALHOST,
        Host::Ipv6(ip) => ip == Ipv6Addr::LOCALHOST,
    };
    match parsed.scheme() {
        "https" => Some(parsed),
        "http" if this_computer => Some(parsed),
        _ => None,
    }
}

/// Whether `url` is Local Network's token exchange for a service, or the same path on this
/// computer for a test server. Nowhere else: the host sends it the user's codes and refresh
/// tokens, and any other server taking them would only be a token endpoint by another name.
fn is_exchange(url: &str) -> bool {
    let Some(parsed) = endpoint(url) else { return false };
    let ours = parsed.scheme() == "http" || parsed.origin().ascii_serialization() == EXCHANGE_ORIGIN;
    let service = parsed.path().strip_prefix(EXCHANGE_PATH).unwrap_or_default();
    let named = !service.is_empty() && service.bytes().all(|b| matches!(b, b'a'..=b'z' | b'0'..=b'9' | b'-'));
    ours && named && parsed.query().is_none() && parsed.fragment().is_none()
}

/// `[[contributes.accounts]]`: ids, titles, endpoints (a token endpoint or Local Network's token
/// exchange), scopes and PKCE, and a `net:` capability for the requests its token goes into.
pub(crate) fn validate(m: &Manifest) -> Result<(), ManifestError> {
    let (at, c) = ("contributes.accounts", &m.contributes);
    unique_names(at, "id", c.accounts.iter().map(|x| x.id.as_str()).enumerate())?;
    for (i, x) in c.accounts.iter().enumerate() {
        let at = format!("{at}[{i}]");
        shown(format!("{at}.title"), &x.title)?;
        shown(format!("{at}.client_id"), &x.client_id)?;
        let urls = [("authorize_url", Some(&x.authorize_url)), ("token_url", x.token_url.as_ref())];
        for (key, url) in urls.into_iter().chain([("revoke_url", x.revoke_url.as_ref())]) {
            if let Some(url) = url.filter(|u| endpoint(u).is_none()) {
                return Err(invalid(
                    format!("{at}.{key}"),
                    format!("{url:?} isn't an https:// address without a user name or password"),
                ));
            }
        }
        match (&x.token_url, &x.token_exchange) {
            (None, None) => return Err(invalid(format!("{at}.token_url"), "is missing, and so is token_exchange")),
            (Some(_), Some(_)) => {
                return Err(invalid(format!("{at}.token_exchange"), "goes in token_url's place, not beside it"));
            }
            (None, Some(url)) if !is_exchange(url) => {
                let why =
                    format!("{url:?} isn't Local Network's token exchange ({EXCHANGE_ORIGIN}{EXCHANGE_PATH}<service>)");
                return Err(invalid(format!("{at}.token_exchange"), why));
            }
            _ => {}
        }
        if let Some((j, scope)) = x.scopes.iter().enumerate().find(|(_, s)| s.is_empty() || s.contains(' ')) {
            return Err(invalid(format!("{at}.scopes[{j}]"), format!("{scope:?} isn't a scope: a word, no spaces")));
        }
        if !x.pkce {
            return Err(invalid(format!("{at}.pkce"), "is true: locallibrary signs in with PKCE only"));
        }
    }
    if !c.accounts.is_empty() && !m.capabilities.iter().any(|c| matches!(c, Capability::Net(_))) {
        let message = "goes into net/fetch requests, which needs a net:<host> capability";
        return Err(invalid(format!("{at}[0]"), message));
    }
    Ok(())
}
