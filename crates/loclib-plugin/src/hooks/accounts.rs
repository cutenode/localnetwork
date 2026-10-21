//! Feature `accounts`: a plugin's accounts on other services, which the host signs the user in to
//! (OAuth 2's authorization code flow with PKCE, the browser coming back to this computer) and
//! keeps the tokens of. A plugin never sees a token: a `net/fetch` request names an account as its
//! `Authorization` header's value, `{account:<id>}`, which the host fills in with `Bearer` and the
//! account's access token once the request is checked, refreshing it first when it's about to
//! expire.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::manifest::{invalid, shown, unique_names};
use crate::{Capability, Feature, Manifest, ManifestError};

/// What a `net/fetch` request's `Authorization` header says to name an account: `{account:<id>}`.
pub const ACCOUNT_PLACEHOLDER: &str = "{account:";

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
    /// them (https).
    pub token_url: String,
    /// What it asks the service for, as the service names them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub scopes: Vec<String>,
    /// The plugin's client at the service: a public one, without a secret.
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

/// A web address an account's endpoint may be at: https, or http on this computer for a test
/// server (which the host reaches only in tests).
fn is_endpoint(url: &str) -> bool {
    let host = |rest: &str| rest.split(['/', '?', '#']).next().unwrap_or_default().to_string();
    let plain = !url.chars().any(|c| c.is_whitespace() || c.is_control());
    match (url.strip_prefix("https://"), url.strip_prefix("http://")) {
        (Some(rest), _) => plain && !host(rest).is_empty(),
        (_, Some(rest)) => {
            let host = host(rest);
            // Without its port.
            let name = match host.starts_with('[') {
                true => host.split_inclusive(']').next(),
                false => host.split(':').next(),
            };
            plain && matches!(name, Some("127.0.0.1" | "localhost" | "[::1]"))
        }
        _ => false,
    }
}

/// `[[contributes.accounts]]`: ids, titles, endpoints, scopes and PKCE, and a `net:` capability
/// for the requests its token goes into.
pub(crate) fn validate(m: &Manifest) -> Result<(), ManifestError> {
    let (at, c) = ("contributes.accounts", &m.contributes);
    unique_names(at, "id", c.accounts.iter().map(|x| x.id.as_str()).enumerate())?;
    for (i, x) in c.accounts.iter().enumerate() {
        let at = format!("{at}[{i}]");
        shown(format!("{at}.title"), &x.title)?;
        shown(format!("{at}.client_id"), &x.client_id)?;
        let urls = [("authorize_url", Some(&x.authorize_url)), ("token_url", Some(&x.token_url))];
        for (key, url) in urls.into_iter().chain([("revoke_url", x.revoke_url.as_ref())]) {
            if let Some(url) = url.filter(|u| !is_endpoint(u)) {
                return Err(invalid(format!("{at}.{key}"), format!("{url:?} isn't an https:// address")));
            }
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
