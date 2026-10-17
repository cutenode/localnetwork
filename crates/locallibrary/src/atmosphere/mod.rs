//! Signing in with an Atmosphere account: the AT Protocol account people use for Bluesky and
//! other atproto apps, hosted by Bluesky or any other PDS.
//!
//! The OAuth itself is the `jacquard-oauth` client's: finding the account's server from a handle,
//! DID or server URL, the pushed authorization request with PKCE, DPoP-bound tokens, the token
//! exchange and its checks (the `sub` must be the account that server answers for), refresh, and
//! revocation. This module adds what a desktop app and a CLI need around it:
//!
//! * A native public client. locallibrary's is the document at [`CLIENT_ID`] ([`client_metadata`]
//!   writes it; Local Network's web app serves it on that host), which servers name and
//!   remember. Until that document is online, signing in falls back to [`Client::Loopback`], the
//!   `http://localhost` development client, which servers show as a local app and ask about
//!   every time.
//! * The browser round trip: [`begin_login`] returns the URL to open; [`finish_login`] waits for
//!   the redirect on a 127.0.0.1 port, and [`finish_login_with_url`] takes the redirect address
//!   pasted by hand when the browser runs on another computer.
//! * Storage (see [`store`]): the DPoP key and refresh token in the OS keychain, or with
//!   [`Storage::File`] in a file only the user can read; the rest in `atmosphere.json`.
//! * A lock, so the CLI and a running sidecar never spend the same single-use refresh token.
//! * The network (`net`): servers found from a handle or DID are only reached at public https
//!   addresses, and answers are read up to a limit.
//! * Records ([`repo`]): writing the record types locallibrary publishes ([`lexicon`]) with the
//!   session, which needs the [`PUBLISH_SCOPES`] ([`PUBLISH_SET`] grants them), and reading
//!   anyone's without one.
//! * Bug reports ([`bug_report`]), posted from the account to userinput.app, which need
//!   [`REPORT_SCOPE`].
//!
//! One account is signed in at a time. Bluesky's servers end a public client's session two weeks
//! after sign-in however often it's refreshed ([`AccountInfo::sign_in_again_by`]).

pub mod bug_report;
mod callback;
pub mod lexicon;
mod net;
pub mod repo;
pub mod store;

use rustc_hash::FxHashMap;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex, OnceLock};
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use jacquard_api::com_atproto::server::get_session::GetSession;
use jacquard_common::deps::fluent_uri::Uri;
use jacquard_common::error::ClientErrorKind;
use jacquard_common::types::did::Did;
use jacquard_common::xrpc::{XrpcClient, XrpcError};
use jacquard_common::{SmolStr, ToSmolStr};
use jacquard_identity::JacquardResolver;
use jacquard_identity::resolver::ResolverOptions;
use jacquard_oauth::atproto::AtprotoClientMetadata;
use jacquard_oauth::client::{OAuthClient, OAuthSession};
use jacquard_oauth::error::OAuthError;
use jacquard_oauth::scopes::{Scope, Scopes};
use jacquard_oauth::session::ClientData;
use jacquard_oauth::types::{AuthorizeOptions, CallbackParams};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub use callback::{CALLBACK_PATH, CallbackQuery, span};
use net::Http;
pub(crate) use net::this_computer;
use store::{AccountFile, LocalStore};

use crate::error::{Action, Reason};
use crate::export::one_line;
use crate::progress::Ctx;
use crate::{Error, Result};

/// locallibrary's client metadata document: what servers fetch to learn who's asking.
pub const CLIENT_ID: &str = "https://locallibrary.localnetwork.computer/oauth-client-metadata.json";

/// What signing in asks for: to know who you are, nothing more.
pub const DEFAULT_SCOPES: [&str; 1] = ["atproto"];

/// The permission set publishing asks for (as `include:<nsid>`), a Lexicon in the repo's
/// `lexicons/`. Servers show its title and detail when the DJ approves and grant the
/// [`PUBLISH_SCOPES`] it lists in its place, so granted scopes never name it. They look it up at
/// sign-in and on every refresh, so it has to be published first. It sits directly under
/// `computer.localnetwork` because a set only grants records under its own NSID's group, and
/// locallibrary's are under both `computer.localnetwork.dj` and `computer.localnetwork.locallibrary`.
pub const PUBLISH_SET: &str = "computer.localnetwork.locallibraryPublishing";

/// What publishing needs besides `atproto`, and what [`PUBLISH_SET`] grants: to create and replace
/// the record types locallibrary writes (see [`lexicon`]), and nothing else in the account (not
/// even deleting them, which locallibrary doesn't do). The set lists each record type on its own
/// because servers write a permission for several as `repo?collection=…`, which jacquard can't
/// read. Signing in only asks for them when the DJ publishes (`account login --publish`, or a
/// publish command that signs in again).
pub const PUBLISH_SCOPES: [&str; 3] = [
    "repo:computer.localnetwork.locallibrary.settings?action=create&action=update",
    "repo:computer.localnetwork.dj.profile?action=create&action=update",
    "repo:computer.localnetwork.dj.set?action=create&action=update",
];

/// `atproto` and [`PUBLISH_SET`]: what a sign-in that allows publishing asks for. The hosted
/// client document has to allow them before a release asks: sign-in checks, and refuses while it
/// doesn't.
pub fn publish_scopes() -> Vec<String> {
    DEFAULT_SCOPES.iter().map(|s| s.to_string()).chain([format!("include:{PUBLISH_SET}")]).collect()
}

/// What filing a bug report needs besides `atproto`: to create the records a bug report is
/// ([`lexicon::DISCUSSION`]), not to change or delete them, and nothing else in the account. Only
/// `locallibrary bug` asks for it, signing in again when the session doesn't have it.
pub const REPORT_SCOPE: &str = "repo:app.userinput.discussion?action=create";

/// Whether the account's granted scopes (the server's, with [`PUBLISH_SET`] already replaced by
/// what it grants) include each of `needed`. `transition:generic`, the old all-or-nothing scope
/// (an app password's powers), includes them all.
fn grants<E>(info: &AccountInfo, needed: impl IntoIterator<Item = std::result::Result<Scope<SmolStr>, E>>) -> bool {
    let Ok(granted) = Scopes::new(SmolStr::new(info.scopes.join(" "))) else { return false };
    let mut needed = needed.into_iter();
    granted.grants(&Scope::transition_generic()) || needed.all(|s| s.is_ok_and(|s| granted.grants(&s)))
}

/// The scope a session needs to write (create and replace) records of `collection`, when the
/// account's granted scopes don't include it. `None`: it may write them.
pub fn missing_scope(info: &AccountInfo, collection: &str) -> Option<String> {
    let writes = grants(info, [Scope::repo_create(collection), Scope::repo_update(collection)]);
    (!writes).then(|| format!("repo:{collection}?action=create&action=update"))
}

/// Whether the session may create records of `collection` (not necessarily replace them).
pub fn may_create(info: &AccountInfo, collection: &str) -> bool {
    grants(info, [Scope::repo_create(collection)])
}

/// What signing `info`'s account in again asks for, so the session keeps what it may do now:
/// `atproto`, publishing ([`PUBLISH_SET`]) when it may publish, and [`REPORT_SCOPE`] when it may
/// file bug reports. Add what the new sign-in is for.
pub fn kept_scopes(info: &AccountInfo) -> Vec<String> {
    let publishes = lexicon::COLLECTIONS.iter().all(|c| missing_scope(info, c).is_none());
    let mut kept = if publishes { publish_scopes() } else { DEFAULT_SCOPES.map(String::from).to_vec() };
    if may_create(info, lexicon::DISCUSSION) {
        kept.push(REPORT_SCOPE.to_string());
    }
    kept
}

/// How long Bluesky's servers keep a public client's session, refreshed or not.
pub const SESSION_LIMIT_DAYS: i64 = 14;

/// How long to wait for the browser.
pub const LOGIN_TIMEOUT: Duration = Duration::from_secs(5 * 60);

/// The longest the browser is waited for, whatever's asked.
pub const LONGEST_LOGIN_WAIT: Duration = Duration::from_secs(60 * 60);

/// A sign-in nobody finishes is forgotten after this.
const PENDING_LIMIT: Duration = Duration::from_secs(15 * 60);

/// Where the session's secrets are kept.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Storage {
    /// macOS Keychain, Windows Credential Manager, or the Secret Service on Linux.
    #[default]
    Keychain,
    /// `atmosphere-session.json` in the store folder, readable only by you: for computers without
    /// a keychain (a Linux server without a desktop session).
    File,
}

impl Storage {
    /// `LOCALLIBRARY_SESSION_STORE=file` chooses the file.
    pub fn from_env() -> Storage {
        match std::env::var("LOCALLIBRARY_SESSION_STORE").as_deref() {
            Ok("file") => Storage::File,
            _ => Storage::Keychain,
        }
    }
}

/// How locallibrary introduces itself to the account's server.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Client {
    /// The `http://localhost` development client: nothing to host, but servers show it as a
    /// local app and ask for approval at every sign-in.
    Loopback,
    /// A client metadata document at this HTTPS URL (see [`client_metadata`]).
    Hosted { client_id: String },
}

impl Client {
    /// `LOCALLIBRARY_OAUTH_CLIENT_ID`: a document's URL, or `loopback` for the development
    /// client. `None` when unset (locallibrary's own client).
    pub fn from_env() -> Option<Client> {
        match std::env::var("LOCALLIBRARY_OAUTH_CLIENT_ID").as_deref().map(str::trim) {
            Ok("loopback" | "localhost") => Some(Client::Loopback),
            Ok(id) if !id.is_empty() => Some(Client::Hosted { client_id: id.to_string() }),
            _ => None,
        }
    }

    /// locallibrary's own client.
    pub fn published() -> Client {
        Client::Hosted { client_id: CLIENT_ID.to_string() }
    }
}

/// The signed-in Atmosphere account.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct AccountInfo {
    pub did: String,
    /// As the account's server reports it (`None` when the handle doesn't check out).
    pub handle: Option<String>,
    /// The account's data server (PDS).
    pub pds: String,
    /// Where it signed in: the PDS, or an entryway such as `https://bsky.social`.
    pub auth_server: String,
    /// What was granted.
    pub scopes: Vec<String>,
    pub signed_in_at: DateTime<Utc>,
    /// The latest a session can last on Bluesky's servers; sign in again before then.
    pub sign_in_again_by: DateTime<Utc>,
    pub access_expires_at: Option<DateTime<Utc>>,
    pub client: Client,
    pub storage: Storage,
}

impl AccountInfo {
    /// `@handle`, or the DID.
    pub fn name(&self) -> String {
        self.handle.as_ref().map_or_else(|| self.did.clone(), |h| format!("@{h}"))
    }

    /// Whole days until [`AccountInfo::sign_in_again_by`] (negative once it's passed).
    pub fn days_left(&self) -> i64 {
        (self.sign_in_again_by - Utc::now()).num_days()
    }

    /// Whether the session has outlived what Bluesky's servers allow.
    pub fn expired(&self) -> bool {
        Utc::now() >= self.sign_in_again_by
    }
}

// ---------------------------------------------------------------------------------------------
// The OAuth client
// ---------------------------------------------------------------------------------------------

type Oauth = OAuthClient<JacquardResolver<Http>, LocalStore>;
type Session = OAuthSession<JacquardResolver<Http>, LocalStore>;

/// The async runtime the OAuth client runs on; everything outside this module stays synchronous.
fn runtime() -> &'static tokio::runtime::Runtime {
    static RUNTIME: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
    RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .thread_name("atmosphere")
            .enable_all()
            .build()
            .expect("start the network runtime")
    })
}

fn block_on<F: Future>(f: F) -> F::Output {
    runtime().block_on(f)
}

/// A redirect that isn't the sign-in's own.
fn other_attempt() -> Error {
    Error::Account("the browser came back from a different sign-in attempt; sign in again".into())
}

/// What went wrong, worded for people. It quotes what servers sent, so on one line, without
/// control characters, for the terminal it ends up in.
fn oauth_error(e: OAuthError) -> Error {
    use jacquard_oauth::error::CallbackError;
    match &e {
        OAuthError::Callback(CallbackError::MissingState) => other_attempt(),
        OAuthError::Callback(CallbackError::IssuerMismatch { .. }) => Error::Account(one_line(&format!(
            "the sign-in came back from a different server than it went to ({e}); sign in again"
        ))),
        _ => Error::Account(one_line(&format!("signing in: {}", net::with_causes(&e)))),
    }
}

fn scopes(list: &[String]) -> Result<Scopes<SmolStr>> {
    let joined = if list.is_empty() { DEFAULT_SCOPES.join(" ") } else { list.join(" ") };
    // jacquard drops a repeated scope, but not a repeated `include:`.
    let unique: Vec<&str> = joined.split(' ').collect::<indexmap::IndexSet<_>>().into_iter().collect();
    let scopes =
        Scopes::new(SmolStr::new(unique.join(" "))).map_err(|e| Error::invalid(format!("scopes {joined:?}: {e:?}")))?;
    if !scopes.iter().any(|s| s.to_string() == "atproto") {
        return Err(Error::invalid("the scopes have to include `atproto`"));
    }
    Ok(scopes)
}

fn uri(s: &str) -> Result<Uri<String>> {
    Uri::parse(s.to_string()).map_err(|e| Error::invalid(format!("{s:?} isn't a URL: {e:?}")))
}

fn client_config(
    client: &Client,
    redirect_uri: &str,
    scopes: Scopes<SmolStr>,
) -> Result<AtprotoClientMetadata<SmolStr>> {
    let redirect = uri(redirect_uri)?;
    Ok(match client {
        Client::Loopback => AtprotoClientMetadata::new_localhost(Some(vec![redirect]), Some(scopes)),
        Client::Hosted { client_id } => {
            if !client_id.starts_with("https://") {
                return Err(Error::invalid(format!("a hosted client ID is an https:// URL, not {client_id:?}")));
            }
            AtprotoClientMetadata::new(vec![redirect], uri(client_id)?, Some(scopes))
                .with_client_name(SmolStr::new_static("locallibrary"))
        }
    })
}

/// Finding an account's servers and talking to them; with `public_only` (signing in, where the
/// servers are whatever the handle's DID document says), only at public https addresses.
fn resolver(public_only: bool) -> Result<JacquardResolver<Http>> {
    let _guard = runtime().enter();
    Ok(JacquardResolver::new(Http::new(public_only)?, ResolverOptions::default()).with_system_dns())
}

fn oauth(store: LocalStore, config: AtprotoClientMetadata<SmolStr>, public_only: bool) -> Result<Oauth> {
    Ok(OAuthClient::new_from_resolver(store, resolver(public_only)?, ClientData { keyset: None, config }))
}

/// The handle the account's server has for the session's account, checked by that server
/// (`None` when the handle doesn't check out). An error when the server can't be asked or
/// doesn't take the session.
fn server_handle(session: &Session) -> Result<Option<String>> {
    let ended = || {
        Error::Account(
            "the account's server didn't accept the session (it has ended or was revoked); sign in again".into(),
        )
        .because(Reason::SessionEnded)
        .action(sign_in())
    };
    let response = block_on(session.send(GetSession)).map_err(|e| match e.kind() {
        ClientErrorKind::Auth(_) => ended(),
        ClientErrorKind::Http { status } if status.as_u16() == 401 => ended(),
        _ => Error::Account(one_line(&format!("asking the account's server who's signed in: {e}"))),
    })?;
    let status = response.status();
    match response.into_output() {
        Ok(out) => Ok(Some(out.handle.as_str().to_string()).filter(|h| h != "handle.invalid")),
        Err(XrpcError::Auth(_)) => Err(ended()),
        Err(_) if status.as_u16() == 401 => Err(ended()),
        Err(e) => Err(Error::Account(one_line(&format!("the account's server couldn't say who's signed in ({e})")))),
    }
}

// ---------------------------------------------------------------------------------------------
// Signing in
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct LoginOptions {
    /// `None`: locallibrary's own client ([`CLIENT_ID`]), or the development client while that
    /// document isn't online.
    #[serde(default)]
    pub client: Option<Client>,
    #[serde(default)]
    pub storage: Storage,
    /// Scopes to ask for (default: `atproto`).
    #[serde(default)]
    pub scopes: Vec<String>,
}

/// A sign-in waiting for the browser.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct LoginStart {
    /// Pass to [`finish_login`] / [`finish_login_with_url`].
    pub id: String,
    /// Open this in the browser.
    pub url: String,
    /// The host the account signs in at, e.g. `bsky.social`.
    pub server: String,
    pub redirect_uri: String,
    /// The client signing in.
    pub client: Client,
    /// Why it isn't locallibrary's own client, when it fell back to the development one.
    pub note: Option<String>,
}

struct Pending {
    oauth: Oauth,
    listener: TcpListener,
    root: PathBuf,
    input: String,
    requested: Vec<String>,
    started: Instant,
}

static PENDING: LazyLock<Mutex<FxHashMap<String, Pending>>> = LazyLock::new(|| Mutex::new(FxHashMap::default()));

fn take_pending(id: &str) -> Result<Pending> {
    PENDING.lock().unwrap().remove(id).ok_or_else(|| {
        Error::NotFound(format!(
            "sign-in {id:?} (it finished, was cancelled, or is over {} minutes old)",
            PENDING_LIMIT.as_secs() / 60
        ))
    })
}

/// Start signing in with `input`: a handle (`alice.bsky.social`), a DID, or the account's server
/// (`https://bsky.social`, which also lets people pick an account or make one). Returns the URL
/// to open; the sign-in finishes with [`finish_login`] or [`finish_login_with_url`].
pub fn begin_login(root: &Path, input: &str, options: &LoginOptions) -> Result<LoginStart> {
    let input = input.trim().trim_start_matches('@').to_string();
    if input.is_empty() {
        return Err(Error::invalid(
            "enter your Atmosphere handle (alice.bsky.social) or your account's server (https://bsky.social)",
        ));
    }
    if options.storage == Storage::Keychain
        && let Err(e) = keyring::Entry::store_status()
    {
        return Err(store::no_keychain(e));
    }
    let requested: Vec<String> = if options.scopes.is_empty() {
        DEFAULT_SCOPES.iter().map(|s| s.to_string()).collect()
    } else {
        options.scopes.clone()
    };
    let (client, note) = match &options.client {
        None => own_client(block_on(document_check(CLIENT_ID, &requested)))?,
        Some(Client::Hosted { client_id }) => {
            block_on(document_check(client_id, &requested))
                .map_err(|why| Error::Account(format!("the client document at {client_id} can't be used: {why}")))?;
            (Client::Hosted { client_id: client_id.clone() }, None)
        }
        Some(Client::Loopback) => (Client::Loopback, None),
    };
    let (listener, redirect_uri) = callback::bind()?;
    let config = client_config(&client, &redirect_uri, scopes(&requested)?)?;
    let store = LocalStore::new(root, options.storage, client.clone(), redirect_uri.clone(), requested.clone());
    let oauth = oauth(store, config, true)?;
    let url = block_on(oauth.start_auth(&input, AuthorizeOptions::default())).map_err(oauth_error)?;
    // The address comes from the server's metadata, and goes to the browser.
    net::public(&url).map_err(|why| {
        Error::Account(format!("the account's server sent a sign-in page that can't be opened: {why}"))
    })?;
    let server = url.split("://").nth(1).and_then(|r| r.split(['/', '?']).next()).unwrap_or_default().to_string();
    let id = uuid::Uuid::new_v4().simple().to_string();
    let mut pending = PENDING.lock().unwrap();
    pending.retain(|_, p| p.started.elapsed() < PENDING_LIMIT);
    pending.insert(
        id.clone(),
        Pending { oauth, listener, root: root.to_path_buf(), input, requested, started: Instant::now() },
    );
    Ok(LoginStart { id, url, server, redirect_uri, client, note })
}

/// Why a client document can't be used.
#[derive(Debug, PartialEq)]
enum Unusable {
    /// It isn't online: its server couldn't be reached, or doesn't serve it (yet).
    Offline(String),
    /// It can't be this sign-in's, wherever it is: it isn't at a public https address, or it's
    /// online but doesn't fit (the release asks for more than it allows, say).
    Unfit(String),
}

impl std::fmt::Display for Unusable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Unusable::Offline(why) | Unusable::Unfit(why) => f.write_str(why),
        }
    }
}

/// locallibrary's own client, given how the check of its document went: the development client
/// (and why) while the document is offline. A document that's online but doesn't fit is a
/// release ahead of its document: saying so beats signing in, without a word, as an app servers
/// show as "local".
fn own_client(check: std::result::Result<(), Unusable>) -> Result<(Client, Option<String>)> {
    match check {
        Ok(()) => Ok((Client::published(), None)),
        Err(Unusable::Offline(why)) => Ok((
            Client::Loopback,
            Some(format!(
                "locallibrary's client document can't be reached ({why}), so this signs in as a local development app"
            )),
        )),
        Err(Unusable::Unfit(why)) => Err(Error::Account(format!(
            "locallibrary's client document doesn't fit this sign-in ({why}); sign in as the development client meanwhile"
        ))
        .action(Action::retry(serde_json::json!({ "dev_client": true }), "Sign in as the development client"))),
    }
}

/// Whether the client document at `client_id` is online and fits a native app asking for
/// `requested`: what a server checks before it shows the sign-in page.
async fn document_check(client_id: &str, requested: &[String]) -> std::result::Result<(), Unusable> {
    // Checked before anything is fetched: a client ID is a public https address.
    net::public(client_id).map_err(Unusable::Unfit)?;
    let offline = Unusable::Offline;
    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|e| offline(e.to_string()))?;
    let response = http.get(client_id).send().await.map_err(|e| {
        offline(if e.is_timeout() {
            "it didn't answer within 5 seconds".to_string()
        } else if e.is_connect() {
            "its server couldn't be reached".to_string()
        } else {
            e.to_string()
        })
    })?;
    if !response.status().is_success() {
        return Err(offline(format!("it answered {}", response.status())));
    }
    let body = net::body(response).await.map_err(|e| offline(e.to_string()))?;
    // Something else served at its address (a web app's page) means it isn't online yet.
    let doc: serde_json::Value = serde_json::from_slice(&body).map_err(|_| offline("it isn't JSON".into()))?;
    check_document(&doc, client_id, requested).map_err(Unusable::Unfit)
}

fn check_document(doc: &serde_json::Value, client_id: &str, requested: &[String]) -> std::result::Result<(), String> {
    if doc["client_id"] != client_id {
        return Err(format!("its client_id is {}", doc["client_id"]));
    }
    if doc["application_type"] != "native" {
        return Err("it isn't a native app's".into());
    }
    let redirect = format!("http://127.0.0.1{CALLBACK_PATH}");
    if !doc["redirect_uris"].as_array().is_some_and(|r| r.iter().any(|u| u == redirect.as_str())) {
        return Err(format!("it doesn't list {redirect}"));
    }
    let allowed: Vec<&str> = doc["scope"].as_str().unwrap_or_default().split_whitespace().collect();
    if let Some(missing) = requested.iter().find(|s| !allowed.contains(&s.as_str())) {
        return Err(format!("it doesn't allow the scope {missing}"));
    }
    Ok(())
}

/// Wait for the browser to come back (up to `timeout`, at most an hour), finish signing in, and
/// show the browser how it went.
pub fn finish_login(id: &str, timeout: Duration, ctx: &Ctx<'_>) -> Result<AccountInfo> {
    let pending = take_pending(id)?;
    let store = pending.oauth.registry.store.clone();
    let request = callback::wait(&pending.listener, timeout, ctx, |state| store.expects(state))?;
    match complete(&pending, &request.query) {
        Ok(info) => {
            request.respond(true, &format!("Signed in with your Atmosphere account as {}.", info.name()));
            Ok(info)
        }
        Err(e) => {
            request.respond(false, &e.to_string());
            Err(e)
        }
    }
}

/// Finish signing in with the address the browser ended up on, pasted by hand (when the browser
/// runs on another computer, its redirect to 127.0.0.1 can't reach this one).
pub fn finish_login_with_url(id: &str, url: &str) -> Result<AccountInfo> {
    let query = CallbackQuery::from_url(url)?;
    complete(&take_pending(id)?, &query)
}

/// Forget a sign-in that won't be finished. False when there was none.
pub fn cancel_login(id: &str) -> bool {
    PENDING.lock().unwrap().remove(id).is_some()
}

fn complete(p: &Pending, q: &CallbackQuery) -> Result<AccountInfo> {
    // A refusal counts only from this sign-in's own redirect (RFC 6749 4.1.2.1), as a code does.
    if !q.state.as_deref().is_some_and(|s| p.oauth.registry.store.expects(s)) {
        return Err(other_attempt());
    }
    if let Some(refusal) = q.refusal() {
        return Err(Error::Account(refusal));
    }
    let code =
        q.code.as_deref().ok_or_else(|| Error::Account("the browser came back without a sign-in code".into()))?;
    let _lock = store::lock(&p.root)?;
    let params = CallbackParams {
        code: code.to_smolstr(),
        state: q.state.as_deref().map(ToSmolStr::to_smolstr),
        iss: q.iss.as_deref().map(ToSmolStr::to_smolstr),
    };
    let session = block_on(p.oauth.callback(params)).map_err(oauth_error)?;
    // Signed in either way: the handle is only to show.
    let handle = server_handle(&session).ok().flatten();
    let mut file = AccountFile::load(&p.root)?;
    let account = file.account.as_mut().ok_or_else(|| Error::Account("the session wasn't saved".into()))?;
    account.info.handle = handle.clone();
    account.requested_scopes = p.requested.clone();
    let info = account.info.clone();
    file.last_handle =
        handle.or_else(|| (!p.input.contains("://") && !p.input.starts_with("did:")).then(|| p.input.clone()));
    file.save(&p.root)?;
    Ok(info)
}

// ---------------------------------------------------------------------------------------------
// The signed-in account
// ---------------------------------------------------------------------------------------------

/// The signed-in account, as last saved (no network).
pub fn status(root: &Path) -> Result<Option<AccountInfo>> {
    Ok(AccountFile::load(root)?.account.map(|a| a.info))
}

/// The handle last signed in with, to offer next time.
pub fn last_handle(root: &Path) -> Option<String> {
    AccountFile::load(root).ok()?.last_handle
}

pub(crate) fn not_signed_in() -> Error {
    Error::Account("not signed in to an Atmosphere account".into()).because(Reason::NotSignedIn).action(sign_in())
}

/// Signing in, as an action.
pub(crate) fn sign_in() -> Action {
    Action::call("account.login.begin", serde_json::json!({}), "Sign in")
}

/// The stored session, refreshed if its access token ran out. A session the server no longer
/// honours is removed, so status stops reporting it.
fn restore(root: &Path) -> Result<Session> {
    let file = AccountFile::load(root)?;
    let account = file.account.ok_or_else(not_signed_in)?;
    let config = client_config(&account.info.client, &account.redirect_uri, scopes(&account.requested_scopes)?)?;
    let store = LocalStore::new(
        root,
        account.info.storage,
        account.info.client.clone(),
        account.redirect_uri.clone(),
        account.requested_scopes.clone(),
    );
    // The signed-in account's own servers, whatever their addresses.
    let oauth = oauth(store, config, false)?;
    let did: Did = Did::new_owned(account.info.did.clone()).map_err(|e| Error::Account(format!("stored DID: {e}")))?;
    match block_on(oauth.restore(&did, &account.session_id)) {
        Ok(session) => Ok(session),
        Err(OAuthError::Session(e)) if e.is_permanent() => {
            forget(root)?;
            Err(Error::Account(format!(
                "the session for {} has ended (sessions last up to {SESSION_LIMIT_DAYS} days); sign in again",
                account.info.name()
            ))
            .because(Reason::SessionEnded)
            .action(sign_in()))
        }
        Err(e) => Err(oauth_error(e)),
    }
}

/// Remove the session from this computer (no network), with any secrets an earlier sign-in
/// left behind. Returns why secrets stay when some can't be removed (the keychain doesn't
/// answer); they're remembered, to try again.
fn forget(root: &Path) -> Result<Option<String>> {
    let mut file = AccountFile::load(root)?;
    let account = file.account.take();
    if account.is_none() && file.left_behind.is_empty() {
        return Ok(None);
    }
    let mut why = None;
    let all = account.iter().map(|a| (a.info.storage, a.info.did.clone())).chain(std::mem::take(&mut file.left_behind));
    for (storage, did) in all.collect::<Vec<_>>() {
        if let Err(e) = store::delete_secrets(root, storage, &did) {
            why = Some(e.to_string());
            file.leave_behind(storage, &did);
        }
    }
    file.save(root)?;
    Ok(why)
}

/// Ask the account's server who's signed in (refreshing the session if needed), and save what
/// it says.
pub fn check(root: &Path, ctx: &Ctx<'_>) -> Result<AccountInfo> {
    ctx.check()?;
    // Nothing to check: don't create the folder for a lock.
    status(root)?.ok_or_else(not_signed_in)?;
    let _lock = store::lock(root)?;
    let session = restore(root)?;
    let handle = server_handle(&session)?;
    let mut file = AccountFile::load(root)?;
    let account = file.account.as_mut().ok_or_else(not_signed_in)?;
    if handle.is_some() {
        account.info.handle = handle.clone();
        file.last_handle = handle;
    }
    let info = account.info.clone();
    file.save(root)?;
    Ok(info)
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct LogoutReport {
    /// The account that was signed in (`None`: nobody was).
    pub account: Option<AccountInfo>,
    /// Whether the server revoked the session's tokens. When it couldn't be reached (or offers
    /// no revocation), the session is still removed from this computer and ends on its own.
    pub revoked: bool,
    /// Why some of the session's secrets are still on this computer (the keychain didn't
    /// answer); signing out again tries again. `None`: they're gone.
    pub left_behind: Option<String>,
}

/// Revoke the session at the account's server and remove it from this computer.
pub fn logout(root: &Path) -> Result<LogoutReport> {
    let file = AccountFile::load(root)?;
    if file.account.is_none() && file.left_behind.is_empty() {
        return Ok(LogoutReport { account: None, revoked: false, left_behind: None });
    }
    let _lock = store::lock(root)?;
    // Read first: a session that has ended is removed while it's restored.
    let account = status(root)?;
    let revoked = account.is_some()
        && match restore(root) {
            Ok(session) => block_on(revoke(&session)),
            Err(_) => false,
        };
    let left_behind = forget(root)?;
    Ok(LogoutReport { account, revoked, left_behind })
}

/// Revoke the session's tokens at its authorization server, the refresh token first (a server
/// may keep it alive when only the access token is revoked, RFC 7009 2.1). True when the server
/// took each of them.
async fn revoke(session: &Session) -> bool {
    use jacquard_oauth::request::{OAuthMetadata, RequestErrorKind, revoke};
    let mut data = session.data.write().await;
    let Ok(meta) = OAuthMetadata::new(session.client.as_ref(), &session.registry.client_data, &data).await else {
        return false;
    };
    if meta.server_metadata.revocation_endpoint.is_none() {
        return false;
    }
    let tokens: Vec<SmolStr> =
        data.token_set.refresh_token.iter().chain([&data.token_set.access_token]).cloned().collect();
    let mut revoked = true;
    for token in tokens {
        revoked &= match revoke(session.client.as_ref(), &mut data.dpop_data, &token, &meta).await {
            Ok(()) => true,
            // jacquard expects Bluesky's 204; RFC 7009 says 200.
            Err(e) => matches!(e.kind(), RequestErrorKind::HttpStatus(s) if s.as_u16() == 200),
        };
    }
    revoked
}

/// The client metadata document to host at `client_id` (an HTTPS URL, conventionally ending in
/// `/oauth-client-metadata.json`) for [`Client::Hosted`]. Servers then show its host as the app's
/// name and remember approvals. Its `scope` is the most any sign-in may ask for: `atproto`,
/// [`PUBLISH_SET`], [`REPORT_SCOPE`], and `extra`.
pub fn client_metadata(client_id: &str, extra: &[String]) -> Result<serde_json::Value> {
    let url = uri(client_id)?;
    if url.scheme().as_str() != "https" {
        return Err(Error::invalid(format!("a hosted client ID is an https:// URL, not {client_id:?}")));
    }
    let host = url.authority().map(|a| a.host().to_string()).unwrap_or_default();
    let scope =
        self::scopes(&[publish_scopes(), vec![REPORT_SCOPE.into()], extra.to_vec()].concat())?.to_normalized_string();
    Ok(serde_json::json!({
        "client_id": client_id,
        "client_name": "locallibrary",
        "client_uri": format!("https://{host}"),
        "application_type": "native",
        "token_endpoint_auth_method": "none",
        "dpop_bound_access_tokens": true,
        "grant_types": ["authorization_code", "refresh_token"],
        "response_types": ["code"],
        "redirect_uris": [format!("http://127.0.0.1{CALLBACK_PATH}")],
        "scope": scope.as_str(),
    }))
}

#[cfg(test)]
pub(crate) mod tests;
