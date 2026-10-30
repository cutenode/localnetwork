//! Accounts (the `accounts` feature): accounts of a type, signed in to through the account plugins
//! that provide it and kept once per store ([`shared`]), which plugins reach through the
//! [`access`]es they're given. The host signs the user in to each in the browser, with OAuth 2's
//! authorization code flow and PKCE (the `oauth2` crate), the browser coming back to this computer
//! ([`crate::callback`]), at the port the account type names if it names one. An account's tokens
//! go nowhere but where the store keeps them (see [`kept`]): not to a plugin, a log, an error, an
//! answer or a copy of the store. A `net/fetch` request naming an account by type,
//! `{account:<type>}`, in its `Authorization` header or the query parameter the type names, gets
//! the access token there instead (see [`token`]), once it's checked (see [`super::net`]),
//! refreshed first when it's within [`REFRESH_BEFORE`] of expiring. The service's endpoints are
//! reached as `net/fetch` reaches servers: at public https addresses (servers on this computer in
//! tests), following no redirect. An account type whose service wants a client secret for its
//! tokens names Local Network's token exchange instead of the service's token endpoint: the host
//! sends it the same request as JSON, and it adds the secret and answers as the service.

use std::io::Read;
use std::net::TcpListener;
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use indexmap::IndexMap;
use loclib_plugin::AccountType;
use oauth2::basic::{BasicClient, BasicErrorResponse, BasicTokenResponse};
use oauth2::{
    AuthUrl, AuthorizationCode, ClientId, CsrfToken, EndpointNotSet, EndpointSet, HttpRequest, HttpResponse,
    PkceCodeChallenge, PkceCodeVerifier, RedirectUrl, RefreshToken, RequestTokenError, Scope, TokenResponse, TokenUrl,
};
use reqwest::Url;
use rustc_hash::FxHashMap;
use serde::{Deserialize, Serialize};

use super::Plugins;
use super::net::unreachable;
use super::process::Plugin;
use crate::callback::{self, CallbackQuery};
use crate::export::one_line;
use crate::net::with_causes;
use crate::progress::Ctx;
use crate::store::Store;
use crate::{Error, Result, lock};

pub(super) mod access;
mod kept;
pub(super) mod ready;
pub(super) mod shared;
mod token;
mod whoami;

/// How long before its access token expires an account's is refreshed.
pub const REFRESH_BEFORE: Duration = Duration::from_secs(60);

/// A sign-in nobody finishes is forgotten after this.
const PENDING_LIMIT: Duration = Duration::from_secs(15 * 60);

/// The most of a token endpoint's answer read.
const MAX_ANSWER: u64 = 1 << 20;

/// An account type's client at its service, for signing in (with the browser's way back) and
/// refreshing. Revoking is the host's own request (see [`Plugin::revoke`]): the crate's refuses an
/// endpoint on this computer, which tests have.
type Client = BasicClient<EndpointSet, EndpointNotSet, EndpointNotSet, EndpointNotSet, EndpointSet>;

/// What's kept of an account's sign-in, as JSON. Never printed: it has no `Debug`.
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

/// A sign-in waiting for the browser (see [`Plugins::begin_account_sign_in`]).
pub(crate) struct Pending {
    asked: shared::Asked,
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

impl Plugin {
    /// An account type's client, coming back to `redirect` from the browser when it's given. Not
    /// named for an account or OAuth: CodeQL guesses secrets from names, and would take the
    /// sign-in link made from it for one.
    fn sign_in_client(&self, t: &AccountType, redirect: Option<RedirectUrl>) -> Result<Client> {
        let bad = |what: &str, e: oauth2::url::ParseError| {
            self.misbehaved(format!("its {} account's {what} can't be used: {e}", t.title))
        };
        let token_url = t.token_url.as_ref().or(t.token_exchange.as_ref()).cloned().unwrap_or_default();
        let client = BasicClient::new(ClientId::new(t.client_id.clone()))
            .set_auth_uri(AuthUrl::new(t.authorize_url.clone()).map_err(|e| bad("authorize_url", e))?)
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

    /// Sends a request for an account's tokens: to its service's token endpoint as it is, or to
    /// Local Network's token exchange as a JSON object of the same fields, since SvelteKit refuses
    /// a form posted without an `Origin` header (as it would a browser's from another site).
    fn send_token(&self, t: &AccountType, request: HttpRequest) -> std::result::Result<HttpResponse, Unanswered> {
        if t.token_exchange.is_none() {
            return self.send(request);
        }
        let (mut parts, form) = request.into_parts();
        let fields: serde_json::Map<String, serde_json::Value> =
            form_urlencoded::parse(&form).map(|(k, v)| (k.into_owned(), v.into_owned().into())).collect();
        let json = oauth2::http::HeaderValue::from_static("application/json");
        parts.headers.insert(oauth2::http::header::CONTENT_TYPE, json);
        self.send(HttpRequest::from_parts(parts, serde_json::Value::Object(fields).to_string().into_bytes()))
    }

    /// Asks an account's service to forget `tokens` (RFC 7009: the refresh token, which takes the
    /// access tokens with it, else the access token), when its type has a revocation endpoint:
    /// whether it answered that it has.
    fn revoke(&self, t: &AccountType, tokens: &Tokens) -> bool {
        let Some(url) = &t.revoke_url else { return false };
        let (token, hint) = match &tokens.refresh_token {
            Some(refresh) => (refresh, "refresh_token"),
            None => (&tokens.access_token, "access_token"),
        };
        let form = form_urlencoded::Serializer::new(String::new())
            .append_pair("token", token)
            .append_pair("token_type_hint", hint)
            .append_pair("client_id", &t.client_id)
            .finish();
        let request = oauth2::http::Request::post(url)
            .header(oauth2::http::header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .body(form.into_bytes());
        request.ok().and_then(|r| self.send(r).ok()).is_some_and(|answer| answer.status().is_success())
    }

    /// What a token endpoint's failure says to the user, of an account of type `t`: what the
    /// service said (but any token the host holds), never what it answered otherwise.
    fn refused(&self, t: &AccountType, e: RequestTokenError<Unanswered, BasicErrorResponse>) -> Error {
        let why = match e {
            RequestTokenError::ServerResponse(r) => match r.error_description() {
                Some(d) => format!("{}: {}", r.error().as_ref(), one_line(d)),
                None => format!("it said {}", r.error().as_ref()),
            },
            RequestTokenError::Request(Unanswered(why)) => why,
            RequestTokenError::Parse(..) => "its answer couldn't be read".into(),
            RequestTokenError::Other(why) => why,
        };
        Error::Account(self.net.scrub(&format!("signing in to {}: {why}", t.title)))
    }

    /// The tokens `kept` for the account `name`, told to [`super::net::State::scrub`].
    fn read_tokens(&self, name: &str, kept: &str) -> Result<Tokens> {
        let tokens: Tokens = serde_json::from_str(kept)
            .map_err(|_| Error::Account(format!("the {name} account's sign-in can't be read: sign in again")))?;
        self.seen(&tokens);
        Ok(tokens)
    }

    /// Keeps `tokens` out of what [`super::net::State::scrub`] gives.
    fn seen(&self, tokens: &Tokens) {
        self.net.saw(&tokens.access_token);
        tokens.refresh_token.iter().for_each(|t| self.net.saw(t));
    }

    /// What `t`'s service answered for the code the browser came back with in `query`, traded
    /// with `client` and the PKCE `verifier`; what it said, when it didn't sign in.
    fn traded(
        &self,
        t: &AccountType,
        client: &Client,
        verifier: PkceCodeVerifier,
        query: &CallbackQuery,
    ) -> Result<BasicTokenResponse> {
        if let Some(why) = query.refusal() {
            return Err(Error::Account(why));
        }
        let code = query.code.clone().ok_or_else(|| Error::Account("the browser came back without a code".into()))?;
        let send = |r: HttpRequest| self.send_token(t, r);
        let exchange = client.exchange_code(AuthorizationCode::new(code)).set_pkce_verifier(verifier);
        exchange.request(&send).map_err(|e| self.refused(t, e))
    }

    /// `had` refreshed with its `refresh` token at `t`'s service, told to
    /// [`super::net::State::scrub`]: `None` when the service won't refresh it any more
    /// (`invalid_grant`), and signing in again will do.
    fn refreshed(&self, t: &AccountType, refresh: String, had: Tokens) -> Result<Option<Tokens>> {
        let client = self.sign_in_client(t, None)?;
        let send = |r: HttpRequest| self.send_token(t, r);
        match client.exchange_refresh_token(&RefreshToken::new(refresh)).request(&send) {
            Ok(answer) => {
                let fresh = Tokens::from(&answer, Some(had));
                self.seen(&fresh);
                Ok(Some(fresh))
            }
            Err(RequestTokenError::ServerResponse(r)) if r.error().as_ref() == "invalid_grant" => Ok(None),
            Err(e) => Err(self.refused(t, e)),
        }
    }
}

impl Plugins {
    /// Starts a sign-in to what `asked` asks for in `store` at `t`'s service (of `slot`), asking
    /// for `scopes` and sending the authorize `params` besides: its id, the address to open in the
    /// browser, and where the browser comes back to.
    fn begin(
        &self,
        slot: &Plugin,
        t: &AccountType,
        scopes: &[String],
        params: &IndexMap<String, String>,
        asked: shared::Asked,
        store: &Store,
    ) -> Result<(String, String, String)> {
        if let Some(port) = t.redirect_port {
            // A sign-in still waiting on its port gives way: the browser comes back to the newest.
            lock(&self.signing_in).retain(|_, p| p.listener.local_addr().map_or(true, |a| a.port() != port));
        }
        let (listener, redirect_uri) = callback::bind(t.redirect_port.unwrap_or(0))?;
        let redirect = RedirectUrl::new(redirect_uri.clone()).map_err(|e| Error::Account(e.to_string()))?;
        let client = slot.sign_in_client(t, Some(redirect))?;
        let (challenge, verifier) = PkceCodeChallenge::new_random_sha256();
        let scopes = scopes.iter().cloned().map(Scope::new);
        let mut request = client.authorize_url(CsrfToken::new_random).add_scopes(scopes).set_pkce_challenge(challenge);
        for (name, value) in params {
            request = request.add_extra_param(name, value);
        }
        let (url, state) = request.url();
        if let Some(why) = unreachable(&url, slot.loopback()) {
            return Err(slot.misbehaved(format!("its {} account's sign-in page can't be opened: {why}", t.title)));
        }
        let id = uuid::Uuid::new_v4().simple().to_string();
        let mut pending = lock(&self.signing_in);
        pending.retain(|_, p| p.started.elapsed() < PENDING_LIMIT);
        let waiting =
            Pending { asked, store: store.clone(), listener, client, state, verifier, started: Instant::now() };
        pending.insert(id.clone(), waiting);
        Ok((id, url.to_string(), redirect_uri))
    }

    /// Takes sign-in `id` from those waiting for the browser.
    fn pending(&self, id: &str) -> Result<Pending> {
        lock(&self.signing_in).remove(id).ok_or_else(|| {
            Error::NotFound(format!(
                "sign-in {id:?} (it finished, was cancelled, or is over {} minutes old)",
                PENDING_LIMIT.as_secs() / 60
            ))
        })
    }

    /// Forgets sign-in `id`, if it's waiting.
    pub fn cancel_sign_in(&self, id: &str) -> bool {
        lock(&self.signing_in).remove(id).is_some()
    }
}

/// Waits for the browser to come back to `p` (at most `timeout`, an hour at most), finishes it
/// with `done`, and tells the browser how it went: that `who` is signed in, or why not.
fn come_back<T>(
    p: Pending,
    timeout: Duration,
    ctx: &Ctx<'_>,
    done: impl FnOnce(Pending, &CallbackQuery) -> Result<T>,
    who: impl FnOnce(&T) -> String,
) -> Result<T> {
    let request = callback::wait(&p.listener, timeout, ctx, |state| state == p.state.secret())?;
    let finished = done(p, &request.query);
    match &finished {
        Ok(signed_in) => request.respond(true, &format!("{} is signed in.", who(signed_in))),
        Err(e) => request.respond(false, &e.to_string()),
    }
    finished
}

/// What the browser came back to `p` with, from the address it ended up on, pasted from another
/// computer.
fn pasted(p: &Pending, url: &str) -> Result<CallbackQuery> {
    let query = CallbackQuery::from_url(url)?;
    if query.state.as_deref() != Some(p.state.secret()) {
        return Err(Error::Account("that address is from another sign-in: sign in again".into()));
    }
    Ok(query)
}

#[cfg(test)]
mod tests;
