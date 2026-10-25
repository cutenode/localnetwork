//! `net/fetch` (the `net` feature): HTTP requests the host makes for a plugin, only to the hosts
//! its `net:` capabilities grant, at public https addresses ([`crate::net::public`]), redirects
//! followed by the host and each one checked the same way, as `locallibrary/<version>` (with the
//! plugin's `contact` setting, when it has one), spaced out as its manifest's `[[net]]` limits
//! say, together with every other plugin's to that host. Its secret settings go only where a request names them (`{secret:<key>}`), and its
//! accounts' access tokens only into the `Authorization` header that names one (`{account:<id>}`,
//! see [`super::accounts`]), filled in once it's checked, never on to another server a redirect
//! leads to, and left out of its log and the errors it's told (see [`State::scrub`]); what a
//! server answers is passed on as it is.
//! Answers are given up to [`super::Options::max_body`], or written to a file in the request's
//! scratch folder; nothing is kept.

use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use indexmap::IndexMap;
use loclib_plugin::{ACCOUNT_PLACEHOLDER, Capability, Fetch, Fetched, RpcError, SettingType};
use reqwest::blocking::{Client, Response};
use reqwest::header::{
    AUTHORIZATION, CONTENT_ENCODING, CONTENT_LENGTH, CONTENT_TYPE, COOKIE, HeaderMap, HeaderName, HeaderValue,
    LOCATION, PROXY_AUTHORIZATION, TRANSFER_ENCODING, USER_AGENT, WWW_AUTHENTICATE,
};
use reqwest::{Method, Url};
use serde_json::Value;

use super::blobs;
use super::process::{Live, Plugin};
use crate::export::one_line;
use crate::net::{public, this_computer, with_causes};
use crate::progress::CancelToken;
use crate::{lock, op};

/// The longest a request may take.
const TIMEOUT: Duration = Duration::from_secs(30);
/// The most redirects followed.
const MAX_REDIRECTS: usize = 5;
/// How often a request waiting for its turn notices being cancelled.
const CHECK_EVERY: Duration = Duration::from_millis(50);
/// What a secret's placeholder starts with: `{secret:<key>}`.
const SECRET: &str = "{secret:";

/// A plugin's `net/fetch` state, whichever of its processes asks.
#[derive(Default)]
pub(crate) struct State {
    client: OnceLock<Result<Client, String>>,
    /// The secrets filled into its requests, and its accounts' tokens (and how URLs spell
    /// them), while this host runs.
    seen: Mutex<Vec<String>>,
    /// Held while one of its accounts' tokens is read and refreshed, so one refresh is made at a
    /// time.
    pub(crate) refreshing: Mutex<()>,
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

    /// Keeps `secret` out of what [`State::scrub`] gives.
    pub(crate) fn saw(&self, secret: &str) {
        let encoded: String = form_urlencoded::byte_serialize(secret.as_bytes()).collect();
        let mut seen = lock(&self.seen);
        for spelt in [secret.to_string(), encoded] {
            if !seen.contains(&spelt) {
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
    body: Option<Vec<u8>>,
    /// Whether the body has a secret in it.
    secret: bool,
}

fn denied(why: String) -> RpcError {
    RpcError::failed("denied", why)
}

fn invalid(why: impl Into<String>) -> RpcError {
    RpcError::failed("invalid", why)
}

/// Why the plugin may not reach `url`, if it may not: its host isn't one its capabilities grant,
/// or [`unreachable`] says so.
fn refused(capabilities: &[Capability], url: &Url, loopback: bool) -> Option<String> {
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
            Client::builder().redirect(none).timeout(TIMEOUT).build().map_err(|e| with_causes(&e))
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

    /// Answers the plugin's `net/fetch` (see the module's docs).
    pub(crate) fn fetch(&self, live: &Live, params: Value, cancel: &CancelToken) -> Result<Value, RpcError> {
        let req: Fetch = loclib_plugin::params(params)?;
        let mut url =
            Url::parse(&req.url).map_err(|_| invalid(format!("{} isn't a web address", one_line(&req.url))))?;
        if let Some(why) = refused(&self.manifest.capabilities, &url, self.loopback()).or_else(|| self.offline(&url)) {
            return Err(denied(why));
        }
        let method = Method::from_bytes(req.method.as_bytes())
            .map_err(|_| invalid(format!("{} isn't an HTTP method", one_line(&req.method))))?;
        if req.body.is_some() && req.body_blob.is_some() {
            return Err(invalid("a request sends a body or a body_blob, not both"));
        }
        let named_account = url.query_pairs().any(|(_, v)| v.contains(ACCOUNT_PLACEHOLDER));
        if named_account || req.body.as_deref().is_some_and(|b| b.contains(ACCOUNT_PLACEHOLDER)) {
            return Err(invalid("an account goes only in the Authorization header: {account:<id>}"));
        }
        if url.query_pairs().any(|(_, v)| v.contains(SECRET)) {
            let pairs: Vec<(String, String)> =
                url.query_pairs().map(|(k, v)| (k.into_owned(), v.into_owned())).collect();
            let filled =
                pairs.into_iter().map(|(k, v)| Ok((k, self.fill(live, &v)?))).collect::<Result<Vec<_>, RpcError>>()?;
            url.query_pairs_mut().clear().extend_pairs(filled);
        }
        let mut sent = Sent { method, plain: HeaderMap::new(), filled: HeaderMap::new(), body: None, secret: false };
        for (name, value) in &req.headers {
            let name = HeaderName::from_bytes(name.as_bytes())
                .map_err(|_| invalid(format!("{name:?} isn't a header's name")))?;
            // Never one the plugin sets: the host says who's asking.
            if name == USER_AGENT {
                continue;
            }
            if value.contains(ACCOUNT_PLACEHOLDER) {
                let id = value.strip_prefix(ACCOUNT_PLACEHOLDER).and_then(|v| v.strip_suffix('}'));
                let Some(id) = id.filter(|id| name == AUTHORIZATION && !id.contains('}')) else {
                    return Err(invalid("an account goes only in the Authorization header: {account:<id>}"));
                };
                let mut bearer = HeaderValue::from_str(&format!("Bearer {}", self.bearer(live, id)?))
                    .map_err(|_| RpcError::failed("external", "the account's token can't be sent in a header"))?;
                bearer.set_sensitive(true);
                sent.filled.append(AUTHORIZATION, bearer);
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
        if let Some(body) = &req.body {
            sent.secret = body.contains(SECRET);
            sent.body = Some(self.fill(live, body)?.into_bytes());
        }
        if let Some(blob) = &req.body_blob {
            let mut bytes = Vec::new();
            let read = live.scratches.open(blob).and_then(|mut f| f.read_to_end(&mut bytes).map_err(|e| e.to_string()));
            read.map_err(|e| invalid(format!("{}: {e}", blob.display())))?;
            sent.body = Some(bytes);
        }
        // Before it's sent: an answer with nowhere to go is never asked for.
        let into = req.into_blob.as_deref().map(|blob| self.new_blob(live, blob)).transpose()?;
        let response = self.follow(url, sent, cancel)?;
        let status = response.status().as_u16();
        let mut answered: IndexMap<String, String> = IndexMap::new();
        for (name, value) in response.headers() {
            let value = String::from_utf8_lossy(value.as_bytes());
            answered.entry(name.as_str().into()).and_modify(|v| *v = format!("{v}, {value}")).or_insert(value.into());
        }
        let fetched = Fetched { status, headers: answered, body: None, blob: None };
        let fetched = match into {
            Some(into) => Fetched { blob: Some(self.write_blob(into, response)?), ..fetched },
            None => Fetched { body: Some(self.text(response)?), ..fetched },
        };
        serde_json::to_value(fetched).map_err(|e| RpcError::internal(e.to_string()))
    }

    /// Sends `sent` to `url`, and follows the redirects it's answered with as browsers do, each
    /// to where the plugin may go ([`refused`]) and in its turn: its last answer. Once one leads
    /// to another server, the headers its secrets were filled into, and those reqwest never sends
    /// on to another (its credentials), are left out from then on; a body with a secret in it is
    /// never sent there.
    fn follow(&self, mut url: Url, mut sent: Sent, cancel: &CancelToken) -> Result<Response, RpcError> {
        let external = |why: String| RpcError::failed("external", self.net.scrub(&why));
        let mut hops = 0;
        loop {
            self.wait_turn(&url, cancel)?;
            let mut request = self.client()?.request(sent.method.clone(), url.clone());
            request = request.headers(sent.plain.clone()).headers(sent.filled.clone());
            if let Some(body) = &sent.body {
                request = request.body(body.clone());
            }
            let response = request.send().map_err(|e| external(with_causes(&e)))?;
            let location = response.headers().get(LOCATION).and_then(|l| l.to_str().ok());
            let (Some(location), 301 | 302 | 303 | 307 | 308) = (location, response.status().as_u16()) else {
                return Ok(response);
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
            if next.origin() != url.origin() {
                if sent.secret {
                    return Err(external(format!("redirected to {next}, which its body's secret isn't for")));
                }
                sent.filled.clear();
                for name in [AUTHORIZATION, COOKIE, PROXY_AUTHORIZATION, WWW_AUTHENTICATE] {
                    sent.plain.remove(name);
                }
            }
            url = next;
        }
    }

    /// An answer's body as text, up to [`super::Options::max_body`].
    fn text(&self, response: Response) -> Result<String, RpcError> {
        let most = self.host.options.max_body;
        let mut body = Vec::new();
        response
            .take(most + 1)
            .read_to_end(&mut body)
            .map_err(|e| RpcError::failed("external", self.net.scrub(&with_causes(&e))))?;
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
    fn write_blob(&self, mut into: NewBlob, response: Response) -> Result<PathBuf, RpcError> {
        let room = into.room;
        let copied =
            std::io::copy(&mut response.take(room + 1), &mut into.out).and_then(|n| into.out.flush().map(|()| n));
        match copied {
            Ok(n) if n > room => Err(RpcError::failed(
                "too_big",
                format!("the answer is more than the scratch folder has room for ({})", op::bytes(room)),
            )),
            Ok(_) => {
                into.written = true;
                Ok(into.path.clone())
            }
            Err(e) => Err(RpcError::failed("external", self.net.scrub(&with_causes(&e)))),
        }
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
