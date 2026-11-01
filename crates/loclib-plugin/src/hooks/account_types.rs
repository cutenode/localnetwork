//! Account types: how to sign in to a service and what each of its scopes lets a plugin do there,
//! as data. An account plugin is a manifest that provides one and nothing else. No feature and no
//! method: the host runs every sign-in from it (OAuth 2's authorization code flow with PKCE, the
//! browser coming back to this computer, or the user entering keys the service gave them), keeps
//! the tokens or keys and never runs the plugin. Plugins ask for accounts of a type with
//! `accounts`.

use indexmap::IndexMap;
use rustc_hash::FxHashSet;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use url::Url;

pub use self::keys::{Key, KeyPlace};
use crate::hooks::accounts::{endpoint, endpoints, redirect_port, token_endpoint, token_place};
use crate::manifest::{NAME_RULE, invalid, is_name, shown};
use crate::{Contributions, Manifest, ManifestError, TokenPlace};

mod keys;

/// What an [`AccountType::one_account`] param's value names the account by: its subject, as
/// [`Whoami::subject`] found it.
pub const SUBJECT_PLACEHOLDER: &str = "{subject}";

/// The authorize params an account type may set. Only these: the others are the host's
/// (`redirect_uri`, `scope`, `state`...) or can replace them (`request_uri` brings a whole request).
pub(crate) const AUTHORIZE_PARAMS: [&str; 4] = ["login_hint", "prompt", "hd", "access_type"];

/// The methods a scope's requests may name.
pub(crate) const METHODS: [&str; 6] = ["GET", "HEAD", "POST", "PUT", "PATCH", "DELETE"];

/// What an account signed in to in the browser is named by in a [`KeyPlace`]'s value: its access
/// token.
pub const TOKEN_KEY: &str = "token";

/// The only header keys go in.
const KEYS_HEADER: &str = "Authorization";

/// `[[contributes.account_types]]`: an account type, signed in to in the browser, or by entering
/// keys (`kind = "keys"`). Its OAuth fields are an account type's signed in to in the browser,
/// and only its; `keys`, `keys_url` and `goes` are one's signed in to with keys, and only its.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct AccountType {
    /// The account type it provides (`google`), which other account plugins may provide too.
    #[serde(rename = "type")]
    pub name: String,
    pub title: String,
    /// How it's signed in to: in the browser unless it says.
    #[serde(default, skip_serializing_if = "SignIn::is_browser")]
    pub kind: SignIn,
    /// The service's authorization endpoint, where the browser signs in (https).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub authorize_url: Option<String>,
    /// Its token endpoint, where the host trades the browser's code for tokens and refreshes
    /// them (https). An account type signed in to in the browser has this or `token_exchange`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token_url: Option<String>,
    /// Local Network's token exchange for its service, in `token_url`'s place: only in an
    /// account plugin locallibrary ships, since it signs people in as Local Network.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token_exchange: Option<String>,
    /// Its client at the service: a public one, without a secret, or Local Network's, whose
    /// secret the token exchange holds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
    /// Where its tokens are revoked at signing out (RFC 7009), when the service has one (https).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revoke_url: Option<String>,
    /// Where its access token goes in a request: `Authorization: Bearer <token>` unless it says.
    #[serde(default, skip_serializing_if = "TokenPlace::is_bearer")]
    pub token: TokenPlace,
    /// The port on 127.0.0.1 the browser comes back to, for a service that compares the redirect
    /// URI with the one its client was registered with. Any free port otherwise.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub redirect_port: Option<u16>,
    /// The service's scopes every sign-in asks for, as the service names them (`openid`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub always: Vec<String>,
    /// Where the host asks the service who signed in. One signed in to with keys may leave it
    /// out: the user names the account then.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub whoami: Option<Whoami>,
    /// The authorize params that sign in again as one account the host knows (`login_hint =
    /// "{subject}"`): their values may name it only as [`SUBJECT_PLACEHOLDER`].
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    pub one_account: IndexMap<String, String>,
    /// The authorize params that sign in to another account than those the browser has
    /// (`prompt = "select_account"`).
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    pub another_account: IndexMap<String, String>,
    /// The authorize params every sign-in sends besides (`access_type = "offline"`), none of
    /// `one_account`'s or `another_account`'s.
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    pub params: IndexMap<String, String>,
    /// The keys the user enters, by name (`access`), which the service gave them.
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    pub keys: IndexMap<String, Key>,
    /// Where the user gets them (https).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub keys_url: Option<String>,
    /// Where they go in a request, each place naming the keys it's made of.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub goes: Vec<KeyPlace>,
    /// What a plugin may be given access to, by name (`youtube-upload`).
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    pub scopes: IndexMap<String, Scope>,
}

/// How an account of a type is signed in to.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SignIn {
    /// In the browser, with OAuth 2: `oauth`.
    #[default]
    Oauth,
    /// By entering the keys the service gave the user: `keys`.
    Keys,
}

impl SignIn {
    fn is_browser(&self) -> bool {
        *self == SignIn::Oauth
    }
}

impl AccountType {
    /// Where an account of it goes in a request, and what goes there: its `goes`, or for one
    /// signed in to in the browser, the access token ([`TOKEN_KEY`]) where its `token` says.
    pub fn places(&self) -> Vec<KeyPlace> {
        if self.kind == SignIn::Keys {
            return self.goes.clone();
        }
        let token = format!("{{{TOKEN_KEY}}}");
        vec![match &self.token {
            TokenPlace::Scheme(scheme) => {
                KeyPlace { header: Some(KEYS_HEADER.into()), query: None, value: format!("{scheme} {token}") }
            }
            TokenPlace::Query(name) => KeyPlace { header: None, query: Some(name.clone()), value: token },
        }]
    }
}

/// `whoami = { url = "...", subject = "/sub", name = "/email" }`: the endpoint the host gets with a
/// new access token, or keys just entered, and JSON pointers into its answer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Whoami {
    /// Where (https).
    pub url: String,
    /// The account's id at the service, which tells two accounts apart (`/sub`).
    pub subject: String,
    /// What the account is shown as (`/email`).
    pub name: String,
}

/// `[contributes.account_types.scopes.<name>]`: one thing a plugin may do with an account.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Scope {
    pub title: String,
    /// The service's scopes it asks for at signing in, as the service names them. None for a
    /// scope only locallibrary knows, which the service's own scopes already allow.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub asks: Vec<String>,
    /// The requests it covers: the only ones the host fills the account's token into for it.
    pub requests: Vec<ScopeRequest>,
}

/// `{ method = "POST", url = "https://www.googleapis.com/upload/youtube/v3/videos*" }`: requests
/// with this method to this URL, or to any URL starting with it when it ends in `*`. The URL is
/// written as it's read (`https://a.com/*`, not `https://a.com*`), so the start it names is a whole
/// host.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ScopeRequest {
    /// `GET`, `HEAD`, `POST`, `PUT`, `PATCH` or `DELETE`.
    pub method: String,
    pub url: String,
}

impl ScopeRequest {
    /// The host its requests go to.
    pub fn host(&self) -> Option<String> {
        Url::parse(self.url.strip_suffix('*').unwrap_or(&self.url)).ok()?.host_str().map(String::from)
    }
}

impl Scope {
    /// Whether it covers a `method` request (in any case) to `url`.
    pub fn covers(&self, method: &str, url: &Url) -> bool {
        let url = url.as_str();
        self.requests.iter().any(|r| {
            let named = match r.url.strip_suffix('*') {
                Some(start) => url.starts_with(start),
                None => url == r.url,
            };
            named && r.method.eq_ignore_ascii_case(method)
        })
    }
}

/// `[[contributes.account_types]]`: one, in a manifest with nothing else; its type and scopes'
/// names and titles; how it's signed in to (see [`browser`] and [`keys::validate`]); pointers
/// into who signed in; the requests each scope covers, and the service's scopes each asks for,
/// none when it's signed in to with keys; and its whoami at a host its scopes' requests go to
/// then, since that's sent the keys.
pub(crate) fn validate(m: &Manifest, built_in: bool) -> Result<(), ManifestError> {
    let (at, c) = ("contributes.account_types", &m.contributes);
    let Some(x) = c.account_types.first() else { return Ok(()) };
    if c.account_types.len() > 1 {
        return Err(invalid(format!("{at}[1]"), "is a second account type: an account plugin provides one"));
    }
    let others = Contributions { account_types: Vec::new(), ..c.clone() };
    let alone = m.capabilities.is_empty() && m.accounts.is_empty() && m.net.is_empty();
    if !alone || others != Contributions::default() {
        let why = "makes an account plugin, which has nothing else: no capabilities, accounts, net limits or other \
                   contributions";
        return Err(invalid(format!("{at}[0]"), why));
    }
    let at = format!("{at}[0]");
    if !is_name(&x.name) {
        return Err(invalid(format!("{at}.type"), format!("{:?} isn't a name: {NAME_RULE}", x.name)));
    }
    shown(format!("{at}.title"), &x.title)?;
    match x.kind {
        SignIn::Oauth => browser(&at, x, built_in)?,
        SignIn::Keys => keys::validate(&at, x)?,
    }
    if let Some(w) = &x.whoami {
        endpoints(&at, [("whoami.url", Some(w.url.as_str()))])?;
        for (key, pointer) in [("subject", &w.subject), ("name", &w.name)] {
            if !is_pointer(pointer) {
                let why = format!("{pointer:?} isn't a JSON pointer, like /sub");
                return Err(invalid(format!("{at}.whoami.{key}"), why));
            }
        }
    }
    let keyed = x.kind == SignIn::Keys;
    for (name, scope) in &x.scopes {
        let at = format!("{at}.scopes.{name}");
        if !is_name(name) {
            return Err(invalid(at, format!("isn't a name: {NAME_RULE}")));
        }
        shown(format!("{at}.title"), &scope.title)?;
        if keyed && !scope.asks.is_empty() {
            let why = "asks the service for nothing: an account signed in to with keys was granted nothing there";
            return Err(invalid(format!("{at}.asks"), why));
        }
        service_scopes(&format!("{at}.asks"), &scope.asks)?;
        if scope.requests.is_empty() {
            return Err(invalid(format!("{at}.requests"), "a scope covers a request at least"));
        }
        for (j, r) in scope.requests.iter().enumerate() {
            let at = format!("{at}.requests[{j}]");
            if !METHODS.contains(&r.method.as_str()) {
                let why = format!("{:?} isn't one of {}", r.method, METHODS.join(", "));
                return Err(invalid(format!("{at}.method"), why));
            }
            pattern(&format!("{at}.url"), &r.url)?;
        }
    }
    let Some(w) = x.whoami.as_ref().filter(|_| keyed) else { return Ok(()) };
    let host = Url::parse(&w.url).ok().and_then(|u| u.host_str().map(String::from));
    match x.scopes.values().flat_map(|s| &s.requests).any(|r| r.host() == host) {
        true => Ok(()),
        false => {
            let why =
                format!("{} isn't a host its scopes' requests go to: it's sent the keys", host.unwrap_or_default());
            Err(invalid(format!("{at}.whoami.url"), why))
        }
    }
}

/// `x` (at `at`) an account type signed in to in the browser: none of the fields of one signed in
/// to with keys; endpoints (a token endpoint, or Local Network's token exchange in a built-in
/// plugin), a client, where its token goes and the port the browser comes back to, the service's
/// scopes, a whoami, and authorize params (each set in one place).
fn browser(at: &str, x: &AccountType, built_in: bool) -> Result<(), ManifestError> {
    let keyed = [("keys", !x.keys.is_empty()), ("keys_url", x.keys_url.is_some()), ("goes", !x.goes.is_empty())];
    if let Some((key, _)) = keyed.iter().find(|(_, set)| *set) {
        let why = "is for an account type signed in to with keys (kind = \"keys\")";
        return Err(invalid(format!("{at}.{key}"), why));
    }
    let missing = |key: &str| invalid(format!("{at}.{key}"), "is missing");
    let authorize_url = x.authorize_url.as_deref().ok_or_else(|| missing("authorize_url"))?;
    let client_id = x.client_id.as_deref().ok_or_else(|| missing("client_id"))?;
    if x.whoami.is_none() {
        return Err(missing("whoami"));
    }
    shown(format!("{at}.client_id"), client_id)?;
    let urls = [
        ("authorize_url", Some(authorize_url)),
        ("token_url", x.token_url.as_deref()),
        ("revoke_url", x.revoke_url.as_deref()),
    ];
    endpoints(at, urls)?;
    token_endpoint(at, x.token_url.as_deref(), x.token_exchange.as_deref())?;
    if x.token_exchange.is_some() && !built_in {
        let why = "is only for account plugins locallibrary ships: it signs people in as Local Network";
        return Err(invalid(format!("{at}.token_exchange"), why));
    }
    token_place(at, &x.token)?;
    redirect_port(at, x.redirect_port)?;
    service_scopes(&format!("{at}.always"), &x.always)?;
    params(&format!("{at}.one_account"), &x.one_account, true)?;
    params(&format!("{at}.another_account"), &x.another_account, false)?;
    params(&format!("{at}.params"), &x.params, false)?;
    let elsewhere = [("one_account", &x.one_account), ("another_account", &x.another_account)];
    for key in x.params.keys() {
        if let Some((which, _)) = elsewhere.iter().find(|(_, set)| set.contains_key(key)) {
            return Err(invalid(format!("{at}.params.{key}"), format!("is in {which} too: set it in one place")));
        }
    }
    Ok(())
}

/// Each of `scopes` (at `at[j]`) a scope as OAuth 2 writes one (RFC 6749), used once.
fn service_scopes(at: &str, scopes: &[String]) -> Result<(), ManifestError> {
    let mut seen = FxHashSet::default();
    for (j, scope) in scopes.iter().enumerate() {
        let word = !scope.is_empty() && scope.bytes().all(|b| matches!(b, b'!' | b'#'..=b'[' | b']'..=b'~'));
        if !word {
            let why = format!("{scope:?} isn't a scope: printable ASCII without spaces, \" or \\");
            return Err(invalid(format!("{at}[{j}]"), why));
        }
        if !seen.insert(scope) {
            return Err(invalid(format!("{at}[{j}]"), format!("{scope:?} is there twice")));
        }
    }
    Ok(())
}

/// Whether `pointer` is a JSON pointer (RFC 6901): empty, or `/` and its parts, with `~` only as
/// `~0` or `~1`.
fn is_pointer(pointer: &str) -> bool {
    (pointer.is_empty() || pointer.starts_with('/'))
        && pointer.split('~').skip(1).all(|after| after.starts_with(['0', '1']))
}

/// Each of `params` (at `at.<key>`) one of [`AUTHORIZE_PARAMS`], its value naming the account only
/// as [`SUBJECT_PLACEHOLDER`], and only where `subject`.
fn params(at: &str, params: &IndexMap<String, String>, subject: bool) -> Result<(), ManifestError> {
    for (key, value) in params {
        let at = format!("{at}.{key}");
        if !AUTHORIZE_PARAMS.contains(&key.as_str()) {
            let allowed = AUTHORIZE_PARAMS.join(", ");
            return Err(invalid(at, format!("isn't one of the params an account type may set: {allowed}")));
        }
        let unnamed = match subject {
            true => value.replace(SUBJECT_PLACEHOLDER, ""),
            false => value.clone(),
        };
        if unnamed.contains(['{', '}']) {
            let why = match subject {
                true => format!("{value:?} has a {{ or }} besides {SUBJECT_PLACEHOLDER}"),
                false => format!("{value:?} has a {{ or }}: only one_account names an account"),
            };
            return Err(invalid(at, why));
        }
    }
    Ok(())
}

/// `url` (at `at`) a request pattern: an [`endpoint`] without a fragment, maybe ending in one `*`,
/// and written as it's read, which puts a `/` after the host and leaves out `..` and a default
/// port.
fn pattern(at: &str, url: &str) -> Result<(), ManifestError> {
    let start = url.strip_suffix('*').unwrap_or(url);
    let read = endpoint(start).filter(|u| !start.contains('*') && u.fragment().is_none());
    let Some(read) = read else {
        let why =
            format!("{url:?} isn't an https:// address without a user name, password or fragment, maybe ending in *");
        return Err(invalid(at, why));
    };
    if read.as_str() != start {
        let as_read = format!("{read}{}", &url[start.len()..]);
        return Err(invalid(at, format!("{url:?} is read as {as_read:?}: write it that way")));
    }
    Ok(())
}
