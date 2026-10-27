//! Plugins' accounts (the `accounts` feature): the host signs the user in to each in the browser,
//! with OAuth 2's authorization code flow and PKCE (the `oauth2` crate), the browser coming back
//! to this computer ([`crate::callback`]). An account's tokens are kept with the plugin's secrets
//! (see [`super::secrets`], as `account:<id>`) and go nowhere else: not to the plugin, a log, an
//! error, an answer or a copy of the store. A `net/fetch` request whose `Authorization` header is
//! `{account:<id>}` gets `Bearer` and the access token instead, once it's checked (see
//! [`super::net`]), refreshed first when it's within [`REFRESH_BEFORE`] of expiring. The service's
//! endpoints are reached as `net/fetch` reaches servers: at public https addresses (servers on
//! this computer in tests), following no redirect. An account whose service wants a client secret
//! for its tokens names Local Network's token exchange instead of the service's token endpoint:
//! the host sends it the same request as JSON, and it adds the secret and answers as the service.

use std::io::Read;
use std::net::TcpListener;
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use loclib_plugin::{Account, RpcError};
use oauth2::basic::{BasicClient, BasicErrorResponse, BasicTokenResponse};
use oauth2::{
    AuthUrl, AuthorizationCode, ClientId, CsrfToken, EndpointNotSet, EndpointSet, HttpRequest, HttpResponse,
    PkceCodeChallenge, PkceCodeVerifier, RedirectUrl, RefreshToken, RequestTokenError, Scope, TokenResponse, TokenUrl,
};
use reqwest::Url;
use rustc_hash::FxHashMap;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::Plugins;
use super::net::unreachable;
use super::process::{Live, Plugin};
use crate::callback::{self, CallbackQuery};
use crate::export::one_line;
use crate::net::with_causes;
use crate::progress::Ctx;
use crate::store::Store;
use crate::{Error, Result, lock};

/// How long before its access token expires an account's is refreshed.
pub const REFRESH_BEFORE: Duration = Duration::from_secs(60);

/// A sign-in nobody finishes is forgotten after this.
const PENDING_LIMIT: Duration = Duration::from_secs(15 * 60);

/// The most of a token endpoint's answer read.
const MAX_ANSWER: u64 = 1 << 20;

/// An account's client at its service, for signing in (with the browser's way back) and
/// refreshing. Revoking is the host's own request (see [`Plugin::revoke`]): the crate's refuses an
/// endpoint on this computer, which tests have.
type Client = BasicClient<EndpointSet, EndpointNotSet, EndpointNotSet, EndpointNotSet, EndpointSet>;

/// What's kept of an account's sign-in, as JSON with the plugin's secrets. Never printed: it has
/// no `Debug`.
#[derive(Serialize, Deserialize)]
struct Tokens {
    access_token: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    refresh_token: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    expires_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    scopes: Vec<String>,
}

impl Tokens {
    /// What a token endpoint answered, keeping the refresh token (and scopes) it had when a
    /// refresh's answer leaves them out.
    fn from(answer: &BasicTokenResponse, had: Option<Tokens>) -> Tokens {
        let scopes = answer.scopes().map(|s| s.iter().map(|s| s.to_string()).collect());
        let refresh_token = answer.refresh_token().map(|t| t.secret().clone());
        Tokens {
            access_token: answer.access_token().secret().clone(),
            expires_at: answer.expires_in().and_then(|d| chrono::Duration::from_std(d).ok()).map(|d| Utc::now() + d),
            refresh_token: refresh_token.or_else(|| had.as_ref().and_then(|h| h.refresh_token.clone())),
            scopes: scopes.or_else(|| had.map(|h| h.scopes)).unwrap_or_default(),
        }
    }

    /// Whether the access token is to be refreshed before it's used: it expires within
    /// [`REFRESH_BEFORE`], or has.
    fn expiring(&self, now: DateTime<Utc>) -> bool {
        let soon = chrono::Duration::from_std(REFRESH_BEFORE).unwrap_or_default();
        self.expires_at.is_some_and(|at| at - soon <= now)
    }
}

/// An account of a plugin's, as hosts show it: never a token.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
#[schemars(rename = "PluginAccountStatus")]
pub struct AccountStatus {
    pub plugin: String,
    pub account: String,
    pub title: String,
    pub signed_in: bool,
    /// When its access token expires, when the service said: it's refreshed before then when it
    /// can be.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires: Option<DateTime<Utc>>,
    /// It's refreshed without signing in again.
    pub refreshes: bool,
    /// What the service granted, as it names them.
    pub scopes: Vec<String>,
}

/// A sign-in waiting for the browser.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct SignIn {
    /// Finishes it: [`Plugins::finish_sign_in`], [`Plugins::finish_sign_in_with_url`].
    pub id: String,
    pub plugin: String,
    pub account: String,
    /// Open this in the browser.
    pub url: String,
    /// Where the browser comes back to.
    pub redirect_uri: String,
}

/// What signing out did.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct SignedOut {
    #[serde(flatten)]
    pub status: AccountStatus,
    /// It was signed in.
    pub was_signed_in: bool,
    /// The service was told to forget its tokens (it has a revocation endpoint, which answered).
    pub revoked: bool,
}

/// A sign-in waiting for the browser (see [`Plugins::begin_sign_in`]).
pub(crate) struct Pending {
    plugin: String,
    account: String,
    store: Store,
    listener: TcpListener,
    client: Client,
    state: CsrfToken,
    verifier: PkceCodeVerifier,
    started: Instant,
}

/// The sign-ins waiting for the browser, by id.
pub(crate) type SigningIn = std::sync::Mutex<FxHashMap<String, Pending>>;

/// Why a request to a service's endpoint wasn't answered: its words, never a token.
#[derive(Debug, thiserror::Error)]
#[error("{0}")]
struct Unanswered(String);

/// The account `id`'s key among the plugin's secrets: no setting's key has a `:`.
fn key(id: &str) -> String {
    format!("account:{id}")
}

impl Plugin {
    /// The account `id` it declares: its service's endpoints, client and scopes. Neither this nor
    /// [`Plugin::sign_in_client`] is named for an account or OAuth: CodeQL guesses secrets from
    /// names, and would take the sign-in link made from them for one.
    fn declared_sign_in(&self, id: &str) -> Result<&Account> {
        self.declared("account", id, &self.manifest.contributes.accounts, |a| &a.id)
    }

    /// `account`'s client, coming back to `redirect` from the browser when it's given.
    fn sign_in_client(&self, account: &Account, redirect: Option<RedirectUrl>) -> Result<Client> {
        let bad = |what: &str, e: oauth2::url::ParseError| {
            self.misbehaved(format!("its {} account's {what} can't be used: {e}", account.title))
        };
        let token_url = account.token_url.as_ref().or(account.token_exchange.as_ref()).cloned().unwrap_or_default();
        let client = BasicClient::new(ClientId::new(account.client_id.clone()))
            .set_auth_uri(AuthUrl::new(account.authorize_url.clone()).map_err(|e| bad("authorize_url", e))?)
            .set_token_uri(TokenUrl::new(token_url).map_err(|e| bad("token_url", e))?);
        Ok(match redirect {
            Some(redirect) => client.set_redirect_uri(redirect),
            None => client,
        })
    }

    /// Sends a request to one of a service's endpoints: [`super::net`]'s client, which follows no
    /// redirect, to a public https address (in tests, this computer too).
    fn send(&self, request: HttpRequest) -> std::result::Result<HttpResponse, Unanswered> {
        let url = Url::parse(&request.uri().to_string()).map_err(|e| Unanswered(e.to_string()))?;
        if let Some(why) = unreachable(&url, self.loopback()).or_else(|| self.offline(&url)) {
            return Err(Unanswered(why));
        }
        let client = self.client().map_err(|e| Unanswered(e.message))?;
        let (parts, body) = request.into_parts();
        let response = client
            .request(parts.method, url)
            .headers(parts.headers)
            .body(body)
            .send()
            .map_err(|e| Unanswered(with_causes(&e)))?;
        let mut answer = oauth2::http::Response::builder().status(response.status());
        if let Some(headers) = answer.headers_mut() {
            headers.extend(response.headers().clone());
        }
        let mut body = Vec::new();
        response.take(MAX_ANSWER).read_to_end(&mut body).map_err(|e| Unanswered(with_causes(&e)))?;
        answer.body(body).map_err(|e| Unanswered(e.to_string()))
    }

    /// Sends a request for `account`'s tokens: to its service's token endpoint as it is, or to
    /// Local Network's token exchange as a JSON object of the same fields, since SvelteKit refuses
    /// a form posted without an `Origin` header (as it would a browser's from another site).
    fn send_token(&self, account: &Account, request: HttpRequest) -> std::result::Result<HttpResponse, Unanswered> {
        if account.token_exchange.is_none() {
            return self.send(request);
        }
        let (mut parts, form) = request.into_parts();
        let fields: serde_json::Map<String, serde_json::Value> =
            form_urlencoded::parse(&form).map(|(k, v)| (k.into_owned(), v.into_owned().into())).collect();
        let json = oauth2::http::HeaderValue::from_static("application/json");
        parts.headers.insert(oauth2::http::header::CONTENT_TYPE, json);
        self.send(HttpRequest::from_parts(parts, serde_json::Value::Object(fields).to_string().into_bytes()))
    }

    /// Asks `account`'s service to forget `tokens` (RFC 7009: the refresh token, which takes the
    /// access tokens with it, else the access token), when it has a revocation endpoint: whether
    /// it answered that it has.
    fn revoke(&self, account: &Account, tokens: &Tokens) -> bool {
        let Some(url) = &account.revoke_url else { return false };
        let (token, hint) = match &tokens.refresh_token {
            Some(refresh) => (refresh, "refresh_token"),
            None => (&tokens.access_token, "access_token"),
        };
        let form = form_urlencoded::Serializer::new(String::new())
            .append_pair("token", token)
            .append_pair("token_type_hint", hint)
            .append_pair("client_id", &account.client_id)
            .finish();
        let request = oauth2::http::Request::post(url.as_str())
            .header(oauth2::http::header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .body(form.into_bytes());
        request.ok().and_then(|r| self.send(r).ok()).is_some_and(|answer| answer.status().is_success())
    }

    /// What a token endpoint's failure says to the user, of `account`: what the service said (but
    /// any token the host holds), never what it answered otherwise.
    fn refused(&self, account: &Account, e: RequestTokenError<Unanswered, BasicErrorResponse>) -> Error {
        let why = match e {
            RequestTokenError::ServerResponse(r) => match r.error_description() {
                Some(d) => format!("{}: {}", r.error().as_ref(), one_line(d)),
                None => format!("it said {}", r.error().as_ref()),
            },
            RequestTokenError::Request(Unanswered(why)) => why,
            RequestTokenError::Parse(..) => "its answer couldn't be read".into(),
            RequestTokenError::Other(why) => why,
        };
        Error::Account(self.net.scrub(&format!("signing in to {}: {why}", account.title)))
    }

    /// The account's tokens, if it's signed in, each told to [`super::net::State::scrub`].
    fn tokens(&self, store: &Store, id: &str) -> Result<Option<Tokens>> {
        self.secret(store, &key(id))?.map(|kept| self.read_tokens(id, &kept)).transpose()
    }

    /// The tokens `kept` for the account `id`, told to [`super::net::State::scrub`].
    fn read_tokens(&self, id: &str, kept: &str) -> Result<Tokens> {
        let tokens: Tokens = serde_json::from_str(kept)
            .map_err(|_| Error::Account(format!("the {id} account's sign-in can't be read: sign in again")))?;
        self.net.saw(&tokens.access_token);
        if let Some(refresh) = &tokens.refresh_token {
            self.net.saw(refresh);
        }
        Ok(tokens)
    }

    fn keep(&self, store: &Store, id: &str, tokens: &Tokens) -> Result<()> {
        let json = serde_json::to_string(tokens).map_err(|e| Error::invalid(e.to_string()))?;
        self.set_secret(store, &key(id), Some(&json))
    }

    fn status(&self, account: &Account, tokens: Option<&Tokens>) -> AccountStatus {
        AccountStatus {
            plugin: self.manifest.id.clone(),
            account: account.id.clone(),
            title: account.title.clone(),
            signed_in: tokens.is_some(),
            expires: tokens.and_then(|t| t.expires_at),
            refreshes: tokens.is_some_and(|t| t.refresh_token.is_some()),
            scopes: tokens.map(|t| t.scopes.clone()).unwrap_or_default(),
        }
    }

    /// The access token of the account `id` for a `net/fetch` request: refreshed first when it's
    /// about to expire (one refresh at a time), told to [`super::net::State::scrub`]. The plugin
    /// is told how to sign in when it's signed out.
    pub(crate) fn bearer(&self, live: &Live, id: &str) -> std::result::Result<String, RpcError> {
        let invalid = |why: String| RpcError::failed("invalid", why);
        let account = self.declared_sign_in(id).map_err(|_| invalid(format!("{id:?} isn't one of its accounts")))?;
        let signed_out = || {
            let plugin = &self.manifest.id;
            invalid(format!(
                "its {} account isn't signed in (locallibrary plugin account {plugin} {id} sign-in)",
                account.title
            ))
        };
        let store = live.store.as_ref().ok_or_else(signed_out)?;
        let external = |e: Error| RpcError::failed("external", self.net.scrub(&e.to_string()));
        let _one = lock(&self.net.refreshing);
        let tokens = self.tokens(store, id).map_err(external)?.ok_or_else(signed_out)?;
        if !tokens.expiring(Utc::now()) {
            return Ok(tokens.access_token);
        }
        let Some(refresh) = tokens.refresh_token.clone() else { return Err(signed_out()) };
        let client = self.sign_in_client(account, None).map_err(external)?;
        let send = |r: HttpRequest| self.send_token(account, r);
        let answer = client.exchange_refresh_token(&RefreshToken::new(refresh)).request(&send);
        let answer = match answer {
            Ok(answer) => answer,
            // The service won't refresh it: it's signed out, and signing in again will do.
            Err(RequestTokenError::ServerResponse(r)) if r.error().as_ref() == "invalid_grant" => {
                self.set_secret(store, &key(id), None).map_err(external)?;
                return Err(signed_out());
            }
            Err(e) => return Err(external(self.refused(account, e))),
        };
        let fresh = Tokens::from(&answer, Some(tokens));
        self.net.saw(&fresh.access_token);
        fresh.refresh_token.iter().for_each(|t| self.net.saw(t));
        self.keep(store, id, &fresh).map_err(external)?;
        Ok(fresh.access_token)
    }
}

impl Plugins {
    /// Each of a plugin's accounts, and whether it's signed in (in `store`: none is without one).
    pub fn accounts(&self, plugin: &str, store: Option<&Store>) -> Result<Vec<AccountStatus>> {
        let slot = self.slot(plugin)?;
        let accounts = &slot.manifest.contributes.accounts;
        accounts
            .iter()
            .map(|a| {
                let tokens = store.map(|s| slot.tokens(s, &a.id)).transpose()?.flatten();
                Ok(slot.status(a, tokens.as_ref()))
            })
            .collect()
    }

    /// One of a plugin's accounts, and whether it's signed in.
    pub fn account(&self, plugin: &str, account: &str, store: Option<&Store>) -> Result<AccountStatus> {
        let slot = self.slot(plugin)?;
        let declared = slot.declared_sign_in(account)?;
        let tokens = store.map(|s| slot.tokens(s, account)).transpose()?.flatten();
        Ok(slot.status(declared, tokens.as_ref()))
    }

    /// Starts signing in to one of a plugin's accounts in `store`: the address to open in the
    /// browser, which comes back to this computer. Finished with [`Plugins::finish_sign_in`] (or
    /// [`Plugins::finish_sign_in_with_url`], when the browser runs on another computer), within
    /// a quarter of an hour.
    pub fn begin_sign_in(&self, plugin: &str, account: &str, store: &Store) -> Result<SignIn> {
        let slot = self.slot(plugin)?;
        let declared = slot.declared_sign_in(account)?;
        // The keychain can be used, before the browser is sent anywhere. Only whether it can, what
        // it keeps dropped first: CodeQL follows a `?` on it into the sign-in link. A kept sign-in
        // that can't be read is replaced.
        slot.secret(store, &key(account)).map(drop)?;
        let (listener, redirect_uri) = callback::bind()?;
        let redirect = RedirectUrl::new(redirect_uri.clone()).map_err(|e| Error::Account(e.to_string()))?;
        let client = slot.sign_in_client(declared, Some(redirect))?;
        let (challenge, verifier) = PkceCodeChallenge::new_random_sha256();
        let scopes = declared.scopes.iter().cloned().map(Scope::new);
        let (url, state) =
            client.authorize_url(CsrfToken::new_random).add_scopes(scopes).set_pkce_challenge(challenge).url();
        if let Some(why) = unreachable(&url, slot.loopback()) {
            return Err(
                slot.misbehaved(format!("its {} account's sign-in page can't be opened: {why}", declared.title))
            );
        }
        let id = uuid::Uuid::new_v4().simple().to_string();
        let mut pending = lock(&self.signing_in);
        pending.retain(|_, p| p.started.elapsed() < PENDING_LIMIT);
        let (plugin, account) = (plugin.to_string(), account.to_string());
        let waiting = Pending {
            plugin: plugin.clone(),
            account: account.clone(),
            store: store.clone(),
            listener,
            client,
            state,
            verifier,
            started: Instant::now(),
        };
        pending.insert(id.clone(), waiting);
        Ok(SignIn { id, plugin, account, url: url.to_string(), redirect_uri })
    }

    fn pending(&self, id: &str) -> Result<Pending> {
        lock(&self.signing_in).remove(id).ok_or_else(|| {
            Error::NotFound(format!(
                "sign-in {id:?} (it finished, was cancelled, or is over {} minutes old)",
                PENDING_LIMIT.as_secs() / 60
            ))
        })
    }

    /// Waits for the browser to come back to sign-in `id` (at most `timeout`, an hour at most),
    /// then trades its code for the account's tokens and keeps them. The browser is told how it
    /// went.
    pub fn finish_sign_in(&self, id: &str, timeout: Duration, ctx: &Ctx<'_>) -> Result<AccountStatus> {
        let p = self.pending(id)?;
        let request = callback::wait(&p.listener, timeout, ctx, |state| state == p.state.secret())?;
        let finished = self.signed_in(p, &request.query);
        match &finished {
            Ok(status) => request.respond(true, &format!("{} is signed in.", status.title)),
            Err(e) => request.respond(false, &e.to_string()),
        }
        finished
    }

    /// Finishes sign-in `id` with the address the browser ended up on, pasted from another
    /// computer.
    pub fn finish_sign_in_with_url(&self, id: &str, url: &str) -> Result<AccountStatus> {
        let p = self.pending(id)?;
        let query = CallbackQuery::from_url(url)?;
        if query.state.as_deref() != Some(p.state.secret()) {
            return Err(Error::Account("that address is from another sign-in: sign in again".into()));
        }
        self.signed_in(p, &query)
    }

    /// Forgets sign-in `id`, if it's waiting.
    pub fn cancel_sign_in(&self, id: &str) -> bool {
        lock(&self.signing_in).remove(id).is_some()
    }

    /// The account signed in, from what the browser came back with.
    fn signed_in(&self, p: Pending, query: &CallbackQuery) -> Result<AccountStatus> {
        let slot = self.slot(&p.plugin)?;
        let account = slot.declared_sign_in(&p.account)?;
        if let Some(why) = query.refusal() {
            return Err(Error::Account(why));
        }
        let code = query.code.clone().ok_or_else(|| Error::Account("the browser came back without a code".into()))?;
        let send = |r: HttpRequest| slot.send_token(account, r);
        let answer = p
            .client
            .exchange_code(AuthorizationCode::new(code))
            .set_pkce_verifier(p.verifier)
            .request(&send)
            .map_err(|e| slot.refused(account, e))?;
        let tokens = Tokens::from(&answer, None);
        slot.net.saw(&tokens.access_token);
        tokens.refresh_token.iter().for_each(|t| slot.net.saw(t));
        slot.keep(&p.store, &p.account, &tokens)?;
        Ok(slot.status(account, Some(&tokens)))
    }

    /// Signs out of one of a plugin's accounts: its tokens are forgotten here, and revoked at the
    /// service when it has a revocation endpoint (as far as it answers: they're forgotten here
    /// all the same).
    pub fn sign_out(&self, plugin: &str, account: &str, store: &Store) -> Result<SignedOut> {
        let slot = self.slot(plugin)?;
        let declared = slot.declared_sign_in(account)?;
        let kept = slot.secret(store, &key(account))?;
        // One that can't be read is forgotten all the same, with nothing to revoke it with.
        let tokens = kept.as_deref().and_then(|kept| slot.read_tokens(account, kept).ok());
        let revoked = tokens.as_ref().is_some_and(|t| slot.revoke(declared, t));
        slot.set_secret(store, &key(account), None)?;
        Ok(SignedOut { status: slot.status(declared, None), was_signed_in: kept.is_some(), revoked })
    }
}

#[cfg(test)]
mod tests;
