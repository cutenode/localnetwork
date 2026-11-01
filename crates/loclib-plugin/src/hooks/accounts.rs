//! Feature `accounts`: a plugin's accounts on other services, which the host signs the user in to
//! (OAuth 2's authorization code flow with PKCE, the browser coming back to this computer) and
//! keeps the tokens of. A plugin never sees a token: a `net/fetch` request names an account as
//! `{account:<id>}` where its [`TokenPlace`] says the token goes (the `Authorization` header's
//! value unless it says otherwise), which the host fills in with the account's access token once
//! the request is checked, refreshing it first when it's about to expire. A service whose token
//! endpoint wants a client secret is reached through Local Network's token exchange, which holds
//! the secret: a secret can't ship inside an app people download.

use std::net::{Ipv4Addr, Ipv6Addr};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use url::{Host, Url};

use crate::manifest::{invalid, shown, unique_names};
use crate::{Capability, Feature, Manifest, ManifestError};

/// What a `net/fetch` request says to name an account, as the whole of a header's or a query
/// parameter's value: `{account:<id>}`.
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
    /// Where its access token goes in a request: `Authorization: Bearer <token>` unless it says.
    #[serde(default, skip_serializing_if = "TokenPlace::is_bearer")]
    pub token: TokenPlace,
    /// The port on 127.0.0.1 the browser comes back to, for a service that compares the redirect
    /// URI with the one its client was registered with, port and all (SoundCloud): that's then
    /// `http://127.0.0.1:<port>/callback`. Any free port otherwise.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub redirect_port: Option<u16>,
}

fn pkce() -> bool {
    true
}

/// Where an account's access token goes in a `net/fetch` request: `token = { scheme = "OAuth" }`
/// or `token = { query = "access_token" }`. The request names the account there, and only there.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TokenPlace {
    /// The `Authorization` header, as this scheme and the token (`OAuth <token>`).
    Scheme(String),
    /// This query parameter's value (`?access_token=<token>`), for a service that takes it only
    /// there (Mixcloud).
    Query(String),
}

impl Default for TokenPlace {
    fn default() -> TokenPlace {
        TokenPlace::Scheme("Bearer".into())
    }
}

impl TokenPlace {
    pub(crate) fn is_bearer(&self) -> bool {
        *self == TokenPlace::default()
    }
}

/// The lowest [`Account::redirect_port`]: one below it is a system service's, which a program run
/// by a user can't listen on everywhere.
const LOWEST_PORT: u16 = 1024;

/// `url` as the host's HTTP client reads it, when an account's endpoint may be there: https, or
/// http on this computer for a test server (which the host reaches only in tests), with no user
/// name or password.
pub(crate) fn endpoint(url: &str) -> Option<Url> {
    let plain = !url.chars().any(|c| c.is_whitespace() || c.is_control());
    let parsed = Url::parse(url).ok().filter(|u| plain && !names_a_user(u))?;
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

/// Whether `url` names a user or a password, which the host sends to no server: not an account's
/// endpoint, nor in a plugin's `net/fetch`.
pub fn names_a_user(url: &Url) -> bool {
    !url.username().is_empty() || url.password().is_some()
}

/// Why the host sends nothing to `url`: it isn't https (but on this computer, in tests), or it
/// names a user or a password.
pub fn not_plain_https(url: &str) -> String {
    format!("{url:?} isn't an https:// address without a user name or password")
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
/// exchange), scopes, PKCE, where its token goes and the port the browser comes back to, and a
/// `net:` capability for the requests its token goes into.
pub(crate) fn validate(m: &Manifest) -> Result<(), ManifestError> {
    let (at, c) = ("contributes.accounts", &m.contributes);
    unique_names(at, "id", c.accounts.iter().map(|x| x.id.as_str()).enumerate())?;
    for (i, x) in c.accounts.iter().enumerate() {
        let at = format!("{at}[{i}]");
        shown(format!("{at}.title"), &x.title)?;
        shown(format!("{at}.client_id"), &x.client_id)?;
        let urls = [
            ("authorize_url", Some(x.authorize_url.as_str())),
            ("token_url", x.token_url.as_deref()),
            ("revoke_url", x.revoke_url.as_deref()),
        ];
        endpoints(&at, urls)?;
        token_endpoint(&at, x.token_url.as_deref(), x.token_exchange.as_deref())?;
        if let Some((j, scope)) = x.scopes.iter().enumerate().find(|(_, s)| s.is_empty() || s.contains(' ')) {
            return Err(invalid(format!("{at}.scopes[{j}]"), format!("{scope:?} isn't a scope: a word, no spaces")));
        }
        if !x.pkce {
            return Err(invalid(format!("{at}.pkce"), "is true: locallibrary signs in with PKCE only"));
        }
        token_place(&at, &x.token)?;
        redirect_port(&at, x.redirect_port)?;
    }
    if !c.accounts.is_empty() && !m.capabilities.iter().any(|c| matches!(c, Capability::Net(_))) {
        let message = "goes into net/fetch requests, which needs a net:<host> capability";
        return Err(invalid(format!("{at}[0]"), message));
    }
    Ok(())
}

/// Each of `urls` (`at.key`), where it's given, an [`endpoint`].
pub(crate) fn endpoints<'a>(
    at: &str,
    urls: impl IntoIterator<Item = (&'a str, Option<&'a str>)>,
) -> Result<(), ManifestError> {
    for (key, url) in urls {
        if let Some(url) = url.filter(|u| endpoint(u).is_none()) {
            return Err(invalid(format!("{at}.{key}"), not_plain_https(url)));
        }
    }
    Ok(())
}

/// Tokens (at `at`) traded at one place: `token_url`, or `token_exchange` when that's Local
/// Network's token exchange.
pub(crate) fn token_endpoint(
    at: &str,
    token_url: Option<&str>,
    token_exchange: Option<&str>,
) -> Result<(), ManifestError> {
    match (token_url, token_exchange) {
        (None, None) => Err(invalid(format!("{at}.token_url"), "is missing, and so is token_exchange")),
        (Some(_), Some(_)) => Err(invalid(format!("{at}.token_exchange"), "goes in token_url's place, not beside it")),
        (None, Some(url)) if !is_exchange(url) => {
            let why =
                format!("{url:?} isn't Local Network's token exchange ({EXCHANGE_ORIGIN}{EXCHANGE_PATH}<service>)");
            Err(invalid(format!("{at}.token_exchange"), why))
        }
        _ => Ok(()),
    }
}

/// What [`is_query_name`] takes.
const QUERY_NAME_RULE: &str = "a query parameter's name: letters, digits, -, ., _ and ~";

/// Whether `name` is letters, digits and `allowed`, one at least.
fn spelled(name: &str, allowed: &str) -> bool {
    !name.is_empty() && name.bytes().all(|b| b.is_ascii_alphanumeric() || allowed.as_bytes().contains(&b))
}

/// A query parameter's name, kept to what no query has to escape, so a request names it as the
/// service does.
fn is_query_name(name: &str) -> bool {
    spelled(name, "-._~")
}

/// `token` (at `at.token`) a place a request can name it: a scheme is an HTTP token (RFC 9110),
/// a parameter's name an [`is_query_name`].
pub(crate) fn token_place(at: &str, token: &TokenPlace) -> Result<(), ManifestError> {
    let (key, name, fits, rule) = match token {
        TokenPlace::Scheme(s) => {
            ("scheme", s, spelled(s, "!#$%&'*+-.^_`|~"), "a scheme: one word, like Bearer or OAuth")
        }
        TokenPlace::Query(q) => ("query", q, is_query_name(q), QUERY_NAME_RULE),
    };
    match fits {
        true => Ok(()),
        false => Err(invalid(format!("{at}.token.{key}"), format!("{name:?} isn't {rule}"))),
    }
}

/// `port` (at `at.redirect_port`), where it's given, one a program may listen on.
pub(crate) fn redirect_port(at: &str, port: Option<u16>) -> Result<(), ManifestError> {
    match port.filter(|p| *p < LOWEST_PORT) {
        Some(port) => {
            let why = format!("is {LOWEST_PORT} or more: {port} is a system service's");
            Err(invalid(format!("{at}.redirect_port"), why))
        }
        None => Ok(()),
    }
}
