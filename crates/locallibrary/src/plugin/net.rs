//! `net/fetch` (the `net` feature): HTTP requests the host makes for a plugin, only to the hosts
//! its `net:` capabilities grant, at public https addresses ([`crate::net::public`]) naming no user
//! or password, redirects followed by the host and each one checked the same way, as
//! `locallibrary/<version>` (with the plugin's `contact` setting, when it has one), spaced out as
//! its manifest's `[[net]]` limits say, together with every other plugin's to that host. Its
//! secret settings go only where a request names them (`{secret:<key>}`), and its accounts' access
//! tokens only where a request names one by type (`{account:<type>}`, see [`super::accounts`]) and
//! the account type says its token goes: the `Authorization` header, or a query parameter; and only
//! into a request a scope of the plugin's access covers (see [`super::accounts::token`]). They're
//! filled in once it's checked, never on to another server a redirect leads to, nor to an address
//! no scope of the access covers, and left out of its log and the errors it's told (see
//! [`State::scrub`]); what a server answers is passed on as it is, but for a token it was sent in
//! the query, given back as the placeholder it was named with, and a server refusing a token it was
//! sent, which the host answers (see [`Plugin::answered`]).
//! Answers are given up to [`super::Options::max_body`], or written to a file in the request's
//! scratch folder; nothing is kept. What it sends is text, or files it was given (see [`body`]).
//! A request cancelled stops at once, sending or reading its answer ([`Requests`]).

mod body;
mod challenge;

use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use indexmap::IndexMap;
use loclib_plugin::{
    ACCOUNT_PLACEHOLDER, AccountNeed, Capability, Fetch, Fetched, FormPart, RpcError, SettingType, TokenPlace,
    account_placeholder,
};
use reqwest::header::{
    AUTHORIZATION, CONTENT_ENCODING, CONTENT_LENGTH, CONTENT_TYPE, COOKIE, HeaderMap, HeaderName, HeaderValue,
    LOCATION, PROXY_AUTHORIZATION, TRANSFER_ENCODING, USER_AGENT, WWW_AUTHENTICATE,
};
use reqwest::{Client, Method, Response, Url};
use serde_json::Value;

use self::body::{Body, Field, FilePart};
use self::challenge::{Challenge, challenges, insufficient_scope};
use super::accounts::token::Usable;
use super::blobs;
use super::process::{Live, Plugin, printable};
use crate::export::one_line;
use crate::net::{CHECK_EVERY, Requests, public, this_computer, with_causes};
use crate::progress::CancelToken;
use crate::{lock, op};

/// The longest a request may take, besides sending files.
const TIMEOUT: Duration = Duration::from_secs(30);
/// The slowest a request sending files may send them, in bytes a second (a slow home connection's
/// upload, about a megabit): it may take as long as that would, besides [`TIMEOUT`].
const SLOWEST: u64 = 128 << 10;
/// The most redirects followed.
const MAX_REDIRECTS: usize = 5;
/// What a secret's placeholder starts with: `{secret:<key>}`.
const SECRET: &str = "{secret:";

/// How long sending `bytes` of files may take, at [`SLOWEST`].
pub(super) fn sending(bytes: u64) -> Duration {
    Duration::from_secs(bytes / SLOWEST)
}

/// A plugin's `net/fetch` state, whichever of its processes asks.
#[derive(Default)]
pub(crate) struct State {
    client: OnceLock<Result<Client, String>>,
    /// The secrets filled into its requests, and its accounts' tokens (and how URLs spell
    /// them), while this host runs.
    seen: Mutex<Vec<String>>,
}

impl State {
    /// `text` with every secret filled into the plugin's requests said as `<secret>`: what it
    /// logs, and the errors it and its requests fail with, never show one.
    pub(crate) fn scrub(&self, text: &str) -> String {
        lock(&self.seen).iter().fold(text.to_string(), |text, secret| text.replace(secret, "<secret>"))
    }

    /// [`State::scrub`] for every string in `v`.
    pub(crate) fn scrub_value(&self, v: &mut Value) {
        match v {
            Value::String(s) => *s = self.scrub(s),
            Value::Array(list) => list.iter_mut().for_each(|v| self.scrub_value(v)),
            Value::Object(map) => map.values_mut().for_each(|v| self.scrub_value(v)),
            _ => {}
        }
    }

    /// Keeps `secret` out of what [`State::scrub`] gives: as it is, as a URL spells it, and as it
    /// prints, which is how the host takes in what the plugin says ([`printable`]).
    pub(crate) fn saw(&self, secret: &str) {
        let encoded: String = form_urlencoded::byte_serialize(secret.as_bytes()).collect();
        let mut seen = lock(&self.seen);
        for spelt in [secret.to_string(), encoded, printable(secret)] {
            if !spelt.is_empty() && !seen.contains(&spelt) {
                seen.push(spelt);
            }
        }
        // Longest first: one secret inside another is left out with it, not leaving the rest.
        seen.sort_by_key(|s| std::cmp::Reverse(s.len()));
    }
}

/// A request as it's sent: its secrets filled in.
struct Sent {
    method: Method,
    /// Its headers without a secret in them.
    plain: HeaderMap,
    /// Those with one.
    filled: HeaderMap,
    body: Option<Body>,
    /// Whether the body has a secret in it.
    secret: bool,
    /// The accounts whose tokens are filled in, which go only where their scopes cover.
    accounts: Vec<Filled>,
    /// Its query, with its secrets and tokens filled in, when it has one to fill: the first hop
    /// is sent with it in place of the address's own, which a secret is never put in.
    query: Option<Vec<(String, String)>>,
}

/// An account's token filled into a request, and the access it's through.
struct Filled {
    usable: Usable,
    token: String,
}

/// The accounts' tokens filled into a request's query, and the placeholders that named them.
type InQuery = Vec<(String, String)>;

/// What a request was answered: its status and headers, and its body, to be read: what of it
/// was read already, then the rest of `response`'s.
struct Answer {
    status: u16,
    headers: HeaderMap,
    read: Vec<u8>,
    response: Response,
}

fn denied(why: String) -> RpcError {
    RpcError::failed("denied", why)
}

fn invalid(why: impl Into<String>) -> RpcError {
    RpcError::failed("invalid", why)
}

/// Where an account's token goes, which is at `place` (the `Authorization` header when it's not
/// known), saying how a request names it.
fn misplaced(place: Option<&TokenPlace>) -> RpcError {
    match place {
        Some(TokenPlace::Query(name)) => {
            invalid(format!("this account goes only in the query: {name}={{account:<type>}}"))
        }
        _ => invalid("an account goes only in the Authorization header: {account:<type>}"),
    }
}

/// Why the plugin may not reach `url`, if it may not: it names a user or a password (as an
/// account's endpoint may not), its host isn't one its capabilities grant, or [`unreachable`]
/// says so.
fn refused(capabilities: &[Capability], url: &Url, loopback: bool) -> Option<String> {
    if loclib_plugin::names_a_user(url) {
        return Some(loclib_plugin::not_plain_https(url.as_str()));
    }
    let host = url.host_str().unwrap_or_default();
    if !capabilities.iter().any(|c| c.reaches(host)) {
        return Some(format!("{host} isn't a server the plugin may talk to (no net:{host} capability)"));
    }
    unreachable(url, loopback)
}

/// Why the host doesn't send a plugin's request to `url`, if it doesn't: it isn't a public https
/// address. With `loopback` (tests only), this computer is, over http too.
pub(super) fn unreachable(url: &Url, loopback: bool) -> Option<String> {
    if loopback && this_computer(url) && matches!(url.scheme(), "http" | "https") {
        return None;
    }
    public(url.as_str()).err()
}

impl Plugin {
    /// Whether `net/fetch` may reach servers on this computer: in tests, never in a release.
    pub(super) fn loopback(&self) -> bool {
        cfg!(debug_assertions) && self.host.options.test_loopback
    }

    /// Why the plugin doesn't reach `url` in tests kept offline ([`super::Options::test_offline`]),
    /// if it doesn't: it isn't on this computer.
    pub(super) fn offline(&self, url: &Url) -> Option<String> {
        let offline = cfg!(debug_assertions) && self.host.options.test_offline && !this_computer(url);
        offline
            .then(|| format!("{} isn't on this computer, and tests stay offline", url.host_str().unwrap_or_default()))
    }

    /// The plugin's client, which follows no redirect: [`Plugin::fetch`] does, checking each.
    pub(super) fn client(&self) -> Result<&Client, RpcError> {
        let made = self.net.client.get_or_init(|| {
            let none = reqwest::redirect::Policy::none();
            // Each request's connections go with its runtime (see `Requests`).
            let builder = Client::builder().redirect(none).timeout(TIMEOUT).pool_max_idle_per_host(0);
            builder.build().map_err(|e| with_causes(&e))
        });
        made.as_ref().map_err(|e| RpcError::internal(format!("the network: {e}")))
    }

    /// Waits for the turn of a request to `url` under the `[[net]]` limit for its host, if it has
    /// one, unless it's cancelled first. Turns are the host's, shared by every plugin limiting
    /// the same host: two plugins asking one server keep to its limit between them.
    fn wait_turn(&self, url: &Url, cancel: &CancelToken) -> Result<(), RpcError> {
        let host = url.host_str().unwrap_or_default();
        let Some(limit) = self.manifest.net.iter().find(|l| Capability::Net(l.host.clone()).reaches(host)) else {
            return Ok(());
        };
        let every = Duration::from_secs_f64(1.0 / limit.per_second);
        loop {
            if cancel.is_cancelled() {
                return Err(RpcError::cancelled());
            }
            let mut turns = lock(&self.host.turns);
            let now = Instant::now();
            let next = turns.get(&limit.host).copied().unwrap_or(now);
            if next <= now {
                turns.insert(limit.host.clone(), now + every);
                return Ok(());
            }
            drop(turns);
            std::thread::sleep((next - now).min(CHECK_EVERY));
        }
    }

    /// `text` with each `{secret:<key>}` in it the plugin's secret setting `key`.
    fn fill(&self, live: &Live, text: &str) -> Result<String, RpcError> {
        let mut out = String::with_capacity(text.len());
        let mut rest = text;
        while let Some(at) = rest.find(SECRET) {
            let after = &rest[at + SECRET.len()..];
            let end = after.find('}').ok_or_else(|| invalid("a {secret:<key>} placeholder isn't closed"))?;
            let key = &after[..end];
            let declared =
                self.manifest.contributes.settings.iter().any(|s| s.key == key && s.kind == SettingType::Secret);
            if !declared {
                return Err(invalid(format!("{key:?} isn't one of its secret settings")));
            }
            let secret = match &live.store {
                Some(store) => self.secret(store, key).map_err(|e| RpcError::failed("external", e.to_string()))?,
                None => None,
            };
            let secret = secret.ok_or_else(|| {
                invalid(format!(
                    "its secret {key} isn't set (locallibrary plugin settings {} {key} --secret)",
                    self.manifest.id
                ))
            })?;
            self.net.saw(&secret);
            out.push_str(&rest[..at]);
            out.push_str(&secret);
            rest = &after[end + 1..];
        }
        out.push_str(rest);
        Ok(out)
    }

    /// Says it's locallibrary, and how to reach whoever runs it: the plugin's `contact`
    /// setting, when it has one.
    fn user_agent(&self, live: &Live) -> String {
        let agent = format!("locallibrary/{}", env!("CARGO_PKG_VERSION"));
        let contact =
            self.settings(live.store.as_ref()).get("contact").and_then(Value::as_str).map(str::trim).map(String::from);
        match contact.filter(|c| !c.is_empty()) {
            Some(contact) => format!("{agent} (+{contact})"),
            None => agent,
        }
    }

    /// Answers the plugin's `net/fetch` (see the module's docs). A server refusing an account's
    /// token, on the last hop it went to, is answered for the plugin (see [`Plugin::answered`]).
    pub(crate) fn fetch(&self, live: &Live, params: Value, cancel: &CancelToken) -> Result<Value, RpcError> {
        let req: Fetch = loclib_plugin::params(params)?;
        let url = self.address(&req)?;
        let (sent, in_query) = self.prepare(live, &req, &url)?;
        // Before it's sent: an answer with nowhere to go is never asked for.
        let into = req.into_blob.as_deref().map(|blob| self.new_blob(live, blob)).transpose()?;
        let requests = Requests::new().map_err(|e| RpcError::internal(format!("the network: {e}")))?;
        let followed = self.follow(live, url, sent, &requests, cancel)?;
        let (answer, in_query) = self.answered(live, &req, followed, in_query, &requests, cancel)?;
        // A server says the address it was asked at back (a next page's link, a `Location`): a token
        // sent in the query is given back as the placeholder that named it, which asks for it again.
        let named = |text: String| {
            in_query.iter().fold(text, |text, (token, placeholder)| {
                let encoded: String = form_urlencoded::byte_serialize(token.as_bytes()).collect();
                text.replace(token, placeholder).replace(&encoded, placeholder)
            })
        };
        let mut answered: IndexMap<String, String> = IndexMap::new();
        for (name, value) in &answer.headers {
            let value = named(String::from_utf8_lossy(value.as_bytes()).into_owned());
            answered.entry(name.as_str().into()).and_modify(|v| *v = format!("{v}, {value}")).or_insert(value);
        }
        let fetched = Fetched { status: answer.status, headers: answered, body: None, blob: None };
        let fetched = match into {
            Some(into) => Fetched { blob: Some(self.write_blob(into, answer, &requests, cancel)?), ..fetched },
            None => Fetched { body: Some(named(self.text(answer, &requests, cancel)?)), ..fetched },
        };
        serde_json::to_value(fetched).map_err(|e| RpcError::internal(e.to_string()))
    }

    /// The response `followed`, which `req` was answered on a hop it went to with the tokens that
    /// went with it (see [`Plugin::follow`]), or what's answered instead when the server refused
    /// one (a 401 or 403; not after a hop that dropped them, which isn't about them): saying the
    /// token lacks scopes (see [`insufficient_scope`]), `scope`, its body not passed on (see
    /// [`Plugin::scope_refused`]); a 401 otherwise, the request sent again once with the account's
    /// token refreshed (see [`Plugin::refused_token`]), and its answer passed on. A 401 that isn't
    /// sent again, the second or one with no refresh token to refresh, signs the account out only
    /// when it says `error="invalid_token"` (see [`Plugin::invalid_token`]). With the placeholders
    /// of the tokens the request that was answered had in its query.
    fn answered(
        &self,
        live: &Live,
        req: &Fetch,
        followed: (Response, Vec<Filled>),
        mut in_query: InQuery,
        requests: &Requests,
        cancel: &CancelToken,
    ) -> Result<(Answer, InQuery), RpcError> {
        let (mut response, mut attached) = followed;
        let mut again = false;
        loop {
            let (status, headers) = (response.status().as_u16(), response.headers().clone());
            let Some(store) = live.store.as_ref().filter(|_| !attached.is_empty() && matches!(status, 401 | 403))
            else {
                return Ok((Answer { status, headers, read: Vec::new(), response }, in_query));
            };
            let said = headers.get_all(WWW_AUTHENTICATE).iter().filter_map(|v| v.to_str().ok());
            let said: Vec<Challenge> = said.flat_map(challenges).collect();
            let (most, answer) = (self.host.options.max_body, &mut response);
            let read = move || async move {
                let mut body = Vec::new();
                while (body.len() as u64) < most {
                    let Some(chunk) = answer.chunk().await.map_err(|e| with_causes(&e))? else { break };
                    body.extend_from_slice(&chunk);
                }
                Ok::<_, String>(body)
            };
            let read = requests.run(read, cancel).ok_or_else(RpcError::cancelled)?;
            let body = read.map_err(|e| RpcError::failed("external", self.net.scrub(&e)))?;
            if let Some(scopes) = insufficient_scope(status, &said, &body) {
                let written = Url::parse(&req.url).map_err(|_| invalid("it isn't a web address"))?;
                return Err(self.scope_refused(&attached[0].usable, &scopes, &req.method, &written, store));
            }
            let resend = status == 401 && !again && {
                let refreshed = attached.iter().map(|f| self.refused_token(&f.usable, &f.token, store));
                refreshed.collect::<Result<Vec<bool>, _>>()?.contains(&true)
            };
            if !resend {
                let invalid = status == 401 && said.iter().any(|c| c.param("error") == Some("invalid_token"));
                if invalid {
                    for f in &attached {
                        self.invalid_token(&f.usable, &f.token, store)?;
                    }
                }
                return Ok((Answer { status, headers, read: body, response }, in_query));
            }
            again = true;
            let url = self.address(req)?;
            let (sent, query) = self.prepare(live, req, &url)?;
            in_query = query;
            (response, attached) = self.follow(live, url, sent, requests, cancel)?;
        }
    }

    /// The address `req` asks for, as the plugin wrote it, when the plugin may go there.
    fn address(&self, req: &Fetch) -> Result<Url, RpcError> {
        let url = Url::parse(&req.url).map_err(|_| invalid(format!("{} isn't a web address", one_line(&req.url))))?;
        if let Some(why) = refused(&self.manifest.capabilities, &url, self.loopback()).or_else(|| self.offline(&url)) {
            return Err(denied(why));
        }
        Ok(url)
    }

    /// The request `req` asks for at `url` (its [`Plugin::address`]), as it's sent, its secrets
    /// and accounts' tokens filled in, the query's kept apart from the address: with the tokens
    /// filled into its query, and the placeholders that named them.
    fn prepare(&self, live: &Live, req: &Fetch, url: &Url) -> Result<(Sent, InQuery), RpcError> {
        let method = Method::from_bytes(req.method.as_bytes())
            .map_err(|_| invalid(format!("{} isn't an HTTP method", one_line(&req.method))))?;
        if req.body.is_some() && req.body_blob.is_some() {
            return Err(invalid("a request sends a body or a body_blob, not both"));
        }
        if !req.multipart.is_empty() && (req.body.is_some() || req.body_blob.is_some()) {
            return Err(invalid("a request sends a multipart form or a body, not both"));
        }
        if req.body_range.is_some() && req.body_blob.is_none() {
            return Err(invalid("a body_range is of a body_blob, which it doesn't send"));
        }
        let texts = req.body.iter().chain(req.multipart.iter().filter_map(|p| p.value.as_ref()));
        if let Some(text) = texts.clone().find(|t| t.contains(ACCOUNT_PLACEHOLDER)) {
            return Err(self.misplaced(text));
        }
        // What a scope covers is the request as the plugin wrote it.
        let asked = (method.clone(), url.clone());
        let (plain, filled) = (HeaderMap::new(), HeaderMap::new());
        let mut sent = Sent { method, plain, filled, body: None, secret: false, accounts: Vec::new(), query: None };
        // The accounts' tokens filled into the query, and the placeholders that named them.
        let mut in_query = Vec::new();
        if url.query_pairs().any(|(_, v)| v.contains(SECRET) || v.contains(ACCOUNT_PLACEHOLDER)) {
            let pairs: Vec<(String, String)> =
                url.query_pairs().map(|(k, v)| (k.into_owned(), v.into_owned())).collect();
            let mut filled = Vec::with_capacity(pairs.len());
            for (k, v) in pairs {
                if !v.contains(ACCOUNT_PLACEHOLDER) {
                    filled.push((k, self.fill(live, &v)?));
                    continue;
                }
                let token = self.account(live, &v, Some(&k), &asked, &mut sent.accounts)?;
                filled.push((k, token.clone()));
                in_query.push((token, v));
            }
            sent.query = Some(filled);
        }
        for (name, value) in &req.headers {
            let name = HeaderName::from_bytes(name.as_bytes())
                .map_err(|_| invalid(format!("{name:?} isn't a header's name")))?;
            // Never one the plugin sets: the host says who's asking, and how long the body is.
            if name == USER_AGENT || name == CONTENT_LENGTH {
                continue;
            }
            if value.contains(ACCOUNT_PLACEHOLDER) {
                if name != AUTHORIZATION {
                    return Err(self.misplaced(value));
                }
                let mut filled =
                    HeaderValue::from_str(&self.account(live, value, None, &asked, &mut sent.accounts)?)
                        .map_err(|_| RpcError::failed("external", "the account's token can't be sent in a header"))?;
                filled.set_sensitive(true);
                sent.filled.append(AUTHORIZATION, filled);
                continue;
            }
            let to = if value.contains(SECRET) { &mut sent.filled } else { &mut sent.plain };
            let value = HeaderValue::from_str(&self.fill(live, value)?)
                .map_err(|_| invalid(format!("the {name} header's value can't be sent")))?;
            to.append(name, value);
        }
        let agent = HeaderValue::from_str(&self.user_agent(live))
            .map_err(|_| invalid("its contact can't be sent in a header"))?;
        sent.plain.insert(USER_AGENT, agent);
        sent.secret = texts.clone().any(|t| t.contains(SECRET));
        if let Some(body) = &req.body {
            sent.body = Some(Body::Text(self.fill(live, body)?.into_bytes()));
        }
        if let Some(blob) = &req.body_blob {
            sent.body = Some(Body::File(FilePart::new(live, blob, req.body_range)?));
        }
        if !req.multipart.is_empty() {
            // The host says what it is: a form with the boundary it makes.
            sent.plain.remove(CONTENT_TYPE);
            sent.body = Some(Body::Form(self.form(live, &req.multipart)?));
        }
        Ok((sent, in_query))
    }

    /// What goes where a request (`asked`, its method and URL as the plugin wrote it) names an
    /// account by type as the whole of `value`, `{account:<type>}` of a type the plugin asks for:
    /// in the `Authorization` header (`query` is `None`), its scheme and access token; as the query
    /// parameter `query`, the token. Only where the account type says its token goes, and where the
    /// plugin's access may go (see [`Plugin::usable`]), which `accounts` gets.
    fn account(
        &self,
        live: &Live,
        value: &str,
        query: Option<&str>,
        asked: &(Method, Url),
        accounts: &mut Vec<Filled>,
    ) -> Result<String, RpcError> {
        let Some((need, access)) = self.by_type(value) else {
            return Err(match account_placeholder(value) {
                Some((kind, _)) => invalid(format!("{kind:?} isn't an account type it asks for")),
                None => self.misplaced(value),
            });
        };
        let (method, url) = asked;
        let usable = self.usable(live, need, access, method.as_str(), url)?;
        let store = live.store.as_ref().ok_or_else(|| invalid("there's no store"))?;
        let filled = |token: &str| match (&usable.place, query) {
            (TokenPlace::Scheme(scheme), None) => Some(format!("{scheme} {token}")),
            (TokenPlace::Query(name), Some(query)) if name == query => Some(token.to_string()),
            _ => None,
        };
        if filled("").is_none() {
            return Err(misplaced(Some(&usable.place)));
        }
        let token = self.usable_token(&usable, store)?;
        let filled = filled(&token).unwrap_or_default();
        accounts.push(Filled { usable, token });
        Ok(filled)
    }

    /// The account type `value` names as `{account:<type>}` or `{account:<type>/<access>}`, when
    /// the plugin asks for it, and the access it names.
    fn by_type<'a>(&self, value: &'a str) -> Option<(&AccountNeed, Option<&'a str>)> {
        let (kind, access) = account_placeholder(value)?;
        Some((self.manifest.accounts.iter().find(|n| n.name == kind)?, access))
    }

    /// Why `text`, which names an account, can't be where it is: where that account's token goes,
    /// as the first account plugin providing its type says.
    fn misplaced(&self, text: &str) -> RpcError {
        let named = text.split(ACCOUNT_PLACEHOLDER).nth(1).and_then(|rest| rest.split('}').next()).unwrap_or_default();
        let need = self.by_type(&format!("{ACCOUNT_PLACEHOLDER}{named}}}")).map(|(need, _)| need);
        let with = need.and_then(|n| self.host.providing(&n.name).first().and_then(|id| self.host.account_plugin(id)));
        let place = with.and_then(|w| w.manifest.contributes.account_types.first().map(|t| t.token.clone()));
        misplaced(place.as_ref())
    }

    /// A form's parts, its text's secrets filled in and its files checked.
    fn form(&self, live: &Live, parts: &[FormPart]) -> Result<Vec<(String, Field)>, RpcError> {
        let field = |p: &FormPart| match (&p.value, &p.file) {
            (Some(value), None) if p.range.is_none() => Ok(Field::Text(self.fill(live, value)?)),
            (None, Some(file)) => Ok(Field::File {
                file: FilePart::new(live, file, p.range)?,
                filename: p.filename.clone(),
                content_type: p.content_type.clone(),
            }),
            _ => Err(invalid(format!("the form's part {:?} has a value or a file (and its range), not both", p.name))),
        };
        parts.iter().map(|p| Ok((p.name.clone(), field(p)?))).collect()
    }

    /// Sends `sent` to `url`, and follows the redirects it's answered with as browsers do, each
    /// to where the plugin may go ([`refused`]) and in its turn: its last answer, and the accounts
    /// whose tokens went with it. Once one leads to another server, or where no scope of an
    /// access whose token it carries covers, the headers its secrets were filled into are left
    /// out from then on; once it's another server, those reqwest never sends on to another (its
    /// credentials) too, and a body with a secret in it is never sent there.
    fn follow(
        &self,
        live: &Live,
        mut url: Url,
        mut sent: Sent,
        requests: &Requests,
        cancel: &CancelToken,
    ) -> Result<(Response, Vec<Filled>), RpcError> {
        let external = |why: String| RpcError::failed("external", self.net.scrub(&why));
        let mut hops = 0;
        loop {
            self.wait_turn(&url, cancel)?;
            let mut request = match sent.query.take() {
                Some(query) => {
                    let mut bare = url.clone();
                    bare.set_query(None);
                    self.client()?.request(sent.method.clone(), bare).query(&query)
                }
                None => self.client()?.request(sent.method.clone(), url.clone()),
            };
            request = request.headers(sent.plain.clone()).headers(sent.filled.clone());
            if let Some(body) = &sent.body {
                let files = body.file_bytes();
                if files > 0 {
                    let to = url.host_str().unwrap_or_default();
                    let said = format!("net/fetch: sending {} of files to {to} ({})", op::bytes(files), sent.method);
                    self.host.log(&self.manifest.id, &said);
                    request = request.timeout(TIMEOUT + sending(files));
                }
                request = body.send(request, live, cancel)?;
            }
            let sending = requests.run(|| request.send(), cancel);
            let response = sending.ok_or_else(RpcError::cancelled)?.map_err(|e| external(with_causes(&e)))?;
            let location = response.headers().get(LOCATION).and_then(|l| l.to_str().ok());
            let (Some(location), 301 | 302 | 303 | 307 | 308) = (location, response.status().as_u16()) else {
                return Ok((response, sent.accounts));
            };
            if hops == MAX_REDIRECTS {
                return Err(external(format!("too many redirects (it follows {MAX_REDIRECTS})")));
            }
            hops += 1;
            let next = url
                .join(location)
                .map_err(|_| external(format!("redirected to {}, which isn't a web address", one_line(location))))?;
            if let Some(why) =
                refused(&self.manifest.capabilities, &next, self.loopback()).or_else(|| self.offline(&next))
            {
                return Err(external(format!("redirected to where it may not go: {why}")));
            }
            // 301, 302 and 303 ask again without the body (with GET, but for HEAD).
            if matches!(response.status().as_u16(), 301..=303) {
                sent.body = None;
                sent.secret = false;
                for name in [CONTENT_TYPE, CONTENT_LENGTH, CONTENT_ENCODING, TRANSFER_ENCODING] {
                    sent.plain.remove(name);
                }
                if sent.method != Method::HEAD {
                    sent.method = Method::GET;
                }
            }
            let leaves = next.origin() != url.origin();
            let method = sent.method.as_str();
            let uncovered = sent.accounts.iter().any(|f| !f.usable.scopes.iter().any(|(_, s)| s.covers(method, &next)));
            if leaves || uncovered {
                // The server put a secret in the address it sent the request on to: it stays here.
                if self.net.scrub(next.as_str()) != next.as_str() {
                    return Err(external(format!("redirected to {next}, which a secret in its address isn't for")));
                }
                sent.filled.clear();
                sent.accounts.clear();
            }
            if leaves {
                if sent.secret {
                    return Err(external(format!("redirected to {next}, which its body's secret isn't for")));
                }
                for name in [AUTHORIZATION, COOKIE, PROXY_AUTHORIZATION, WWW_AUTHENTICATE] {
                    sent.plain.remove(name);
                }
            }
            url = next;
        }
    }

    /// Writes an answer's body to `out` as it comes (what of it was read already first), up to a
    /// byte past `most`, unless it's cancelled first: how much it wrote.
    fn read_answer(
        &self,
        answer: Answer,
        most: u64,
        out: &mut impl Write,
        requests: &Requests,
        cancel: &CancelToken,
    ) -> Result<u64, RpcError> {
        let Answer { read: first, mut response, .. } = answer;
        let read = || async move {
            let first = &first[..first.len().min((most + 1) as usize)];
            out.write_all(first).map_err(|e| with_causes(&e))?;
            let mut written = first.len() as u64;
            while written <= most {
                let Some(chunk) = response.chunk().await.map_err(|e| with_causes(&e))? else { break };
                let chunk = &chunk[..chunk.len().min((most + 1 - written) as usize)];
                out.write_all(chunk).map_err(|e| with_causes(&e))?;
                written += chunk.len() as u64;
            }
            out.flush().map_err(|e| with_causes(&e))?;
            Ok::<_, String>(written)
        };
        let read = requests.run(read, cancel).ok_or_else(RpcError::cancelled)?;
        read.map_err(|e| RpcError::failed("external", self.net.scrub(&e)))
    }

    /// An answer's body as text, up to [`super::Options::max_body`].
    fn text(&self, answer: Answer, requests: &Requests, cancel: &CancelToken) -> Result<String, RpcError> {
        let most = self.host.options.max_body;
        let mut body = Vec::new();
        self.read_answer(answer, most, &mut body, requests, cancel)?;
        if body.len() as u64 > most {
            let why = format!("the answer is over {}: fetch it into_blob", op::bytes(most));
            return Err(RpcError::failed("too_big", why));
        }
        String::from_utf8(body).map_err(|_| invalid("the answer isn't text: fetch it into_blob"))
    }

    /// A new file at `blob` (absolute) in a scratch folder under way, for an answer.
    fn new_blob(&self, live: &Live, blob: &Path) -> Result<NewBlob, RpcError> {
        let (dir, name) = live.scratches.holding(blob).map_err(invalid)?;
        let room = self.host.options.max_blobs.saturating_sub(blobs::size(&dir));
        let path = dir.join(name);
        let out = File::create_new(&path).map_err(|e| invalid(format!("{}: {e}", path.display())))?;
        Ok(NewBlob { path, out, room, written: false })
    }

    /// Writes an answer's body to `into`, as far as its folder has room (see
    /// [`super::Options::max_blobs`]).
    fn write_blob(
        &self,
        mut into: NewBlob,
        answer: Answer,
        requests: &Requests,
        cancel: &CancelToken,
    ) -> Result<PathBuf, RpcError> {
        let room = into.room;
        if self.read_answer(answer, room, &mut into.out, requests, cancel)? > room {
            let why = format!("the answer is more than the scratch folder has room for ({})", op::bytes(room));
            return Err(RpcError::failed("too_big", why));
        }
        into.written = true;
        Ok(into.path.clone())
    }
}

/// A file a request's answer is to be written to (`into_blob`), new in a scratch folder under
/// way, with the room left there: removed again unless the answer was written to it.
struct NewBlob {
    path: PathBuf,
    out: File,
    room: u64,
    written: bool,
}

impl Drop for NewBlob {
    fn drop(&mut self) {
        if !self.written {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

#[cfg(test)]
mod tests;
