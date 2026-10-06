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
//! * Records ([`repo`]): writing locallibrary's own record types ([`lexicon`]) with the session,
//!   which needs the [`PUBLISH_SCOPES`], and reading anyone's without one.
//!
//! One account is signed in at a time. Bluesky's servers end a public client's session two weeks
//! after sign-in however often it's refreshed ([`AccountInfo::sign_in_again_by`]).

mod callback;
pub mod lexicon;
mod net;
pub mod repo;
pub mod store;

use std::collections::HashMap;
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

/// What publishing asks for besides `atproto`: to create and replace locallibrary's own record
/// types (see [`lexicon`]), and nothing else in the account (not even deleting them, which
/// locallibrary doesn't do). Signing in only asks for them when the DJ publishes (`account login
/// --publish`, or a publish command that signs in again). The hosted client document has to
/// allow them before a release asks: sign-in checks, and refuses while it doesn't.
pub const PUBLISH_SCOPES: [&str; 3] = [
    "repo:computer.localnetwork.locallibrary.settings?action=create&action=update",
    "repo:computer.localnetwork.locallibrary.profile?action=create&action=update",
    "repo:computer.localnetwork.locallibrary.set?action=create&action=update",
];

/// `atproto` and the publish scopes: what a sign-in that allows publishing asks for.
pub fn publish_scopes() -> Vec<String> {
    DEFAULT_SCOPES.iter().chain(&PUBLISH_SCOPES).map(|s| s.to_string()).collect()
}

/// The scope a session needs to write (create and replace) records of `collection`, when the
/// account's granted scopes don't include it. `None`: it may write them.
pub fn missing_scope(info: &AccountInfo, collection: &str) -> Option<String> {
    let granted = Scopes::new(SmolStr::new(info.scopes.join(" "))).ok();
    let grants = |s: &Scope| granted.as_ref().is_some_and(|g| g.grants(s));
    let writes = match (Scope::repo_create(collection), Scope::repo_update(collection)) {
        (Ok(create), Ok(update)) => grants(&create) && grants(&update),
        _ => false,
    };
    // `transition:generic` is the old all-or-nothing scope: an app password's powers.
    (!writes && !grants(&Scope::transition_generic())).then(|| format!("repo:{collection}?action=create&action=update"))
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
        _ => Error::Account(one_line(&format!("signing in: {e}"))),
    }
}

fn scopes(list: &[String]) -> Result<Scopes<SmolStr>> {
    let joined = if list.is_empty() { DEFAULT_SCOPES.join(" ") } else { list.join(" ") };
    let scopes = Scopes::new(SmolStr::new(&joined)).map_err(|e| Error::invalid(format!("scopes {joined:?}: {e:?}")))?;
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

static PENDING: LazyLock<Mutex<HashMap<String, Pending>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

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
/// name and remember approvals. Its `scope` is the most any sign-in may ask for: `atproto`, the
/// [`PUBLISH_SCOPES`], and `extra`.
pub fn client_metadata(client_id: &str, extra: &[String]) -> Result<serde_json::Value> {
    let url = uri(client_id)?;
    if url.scheme().as_str() != "https" {
        return Err(Error::invalid(format!("a hosted client ID is an https:// URL, not {client_id:?}")));
    }
    let host = url.authority().map(|a| a.host().to_string()).unwrap_or_default();
    let mut all = publish_scopes();
    for s in extra {
        if !all.contains(s) {
            all.push(s.clone());
        }
    }
    let scope = self::scopes(&all)?.to_normalized_string();
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
pub(crate) mod tests {
    use jacquard_oauth::authstore::ClientAuthStore;

    use super::*;

    #[test]
    fn loopback_clients_carry_their_redirect_and_scopes() {
        let c = client_config(&Client::Loopback, "http://127.0.0.1:53111/callback", scopes(&[]).unwrap()).unwrap();
        let id = c.client_id.as_str();
        assert!(id.starts_with("http://localhost"), "{id}");
        assert!(id.contains("redirect_uri=http%3A%2F%2F127.0.0.1%3A53111%2Fcallback"), "{id}");
        assert!(id.contains("scope=atproto"), "{id}");
    }

    #[test]
    fn hosted_clients_need_https() {
        let hosted = |id: &str| Client::Hosted { client_id: id.into() };
        let c = client_config(
            &hosted("https://locallibrary.example/oauth-client-metadata.json"),
            "http://127.0.0.1:53111/callback",
            scopes(&[]).unwrap(),
        )
        .unwrap();
        assert_eq!(c.client_id.as_str(), "https://locallibrary.example/oauth-client-metadata.json");
        assert!(
            client_config(
                &hosted("http://insecure.example/c.json"),
                "http://127.0.0.1:1/callback",
                scopes(&[]).unwrap()
            )
            .is_err()
        );
    }

    #[test]
    fn the_published_document_passes_the_sign_in_check() {
        let scopes: Vec<String> = DEFAULT_SCOPES.iter().map(|s| s.to_string()).collect();
        let doc = client_metadata(CLIENT_ID, &scopes).unwrap();
        assert_eq!(check_document(&doc, CLIENT_ID, &scopes), Ok(()));
        assert_eq!(doc["client_uri"], "https://locallibrary.localnetwork.computer");
    }

    #[test]
    fn documents_are_checked_like_a_server_would() {
        let scopes = vec!["atproto".to_string()];
        let doc = client_metadata(CLIENT_ID, &scopes).unwrap();
        assert!(check_document(&doc, "https://other.example/c.json", &scopes).unwrap_err().contains("client_id"));
        let wants_more = vec!["atproto".to_string(), "repo:app.bsky.feed.post".to_string()];
        assert!(check_document(&doc, CLIENT_ID, &wants_more).unwrap_err().contains("doesn't allow"));
        let mut web = doc.clone();
        web["application_type"] = "web".into();
        assert!(check_document(&web, CLIENT_ID, &scopes).is_err());
        let mut no_loopback = doc.clone();
        no_loopback["redirect_uris"] = serde_json::json!(["https://example.com/callback"]);
        assert!(check_document(&no_loopback, CLIENT_ID, &scopes).unwrap_err().contains("127.0.0.1"));
    }

    #[test]
    fn scopes_must_include_atproto() {
        assert!(scopes(&["repo:app.example.thing".into()]).is_err());
        let s = scopes(&["atproto".into(), "repo:app.example.thing".into()]).unwrap();
        assert!(s.to_normalized_string().contains("repo:app.example.thing"));
    }

    #[test]
    fn hosted_metadata_is_a_native_public_client() {
        let m = client_metadata("https://locallibrary.example/oauth-client-metadata.json", &[]).unwrap();
        assert_eq!(m["application_type"], "native");
        assert_eq!(m["token_endpoint_auth_method"], "none");
        assert_eq!(m["redirect_uris"][0], "http://127.0.0.1/callback");
        assert_eq!(m["client_uri"], "https://locallibrary.example");
        assert_eq!(
            m["scope"],
            "atproto repo:computer.localnetwork.locallibrary.profile?action=create&action=update \
             repo:computer.localnetwork.locallibrary.set?action=create&action=update \
             repo:computer.localnetwork.locallibrary.settings?action=create&action=update"
        );
        assert!(client_metadata("http://locallibrary.example/c.json", &[]).is_err());
    }

    #[test]
    fn the_published_document_allows_publishing() {
        let doc = client_metadata(CLIENT_ID, &[]).unwrap();
        assert_eq!(check_document(&doc, CLIENT_ID, &publish_scopes()), Ok(()));
        let publish: Vec<String> =
            lexicon::COLLECTIONS.iter().map(|c| format!("repo:{c}?action=create&action=update")).collect();
        assert_eq!(PUBLISH_SCOPES.to_vec(), publish);
        // Each is a scope jacquard understands, written the way it writes them back.
        for s in PUBLISH_SCOPES {
            assert_eq!(scopes(&["atproto".into(), s.into()]).unwrap().to_normalized_string(), format!("atproto {s}"));
        }
        // Creating and replacing, and nothing more: not deleting.
        let granted = scopes(&publish_scopes()).unwrap();
        for c in lexicon::COLLECTIONS {
            assert!(granted.grants(&Scope::repo_create(c).unwrap()) && granted.grants(&Scope::repo_update(c).unwrap()));
            assert!(!granted.grants(&Scope::repo_delete(c).unwrap()), "{c}");
        }
    }

    fn signed_in_with(scopes: &[&str]) -> AccountInfo {
        AccountInfo {
            did: "did:plc:abc".into(),
            handle: Some("dj.example.com".into()),
            pds: "https://pds.example".into(),
            auth_server: "https://pds.example".into(),
            scopes: scopes.iter().map(|s| s.to_string()).collect(),
            signed_in_at: Utc::now(),
            sign_in_again_by: Utc::now() + chrono::Duration::days(SESSION_LIMIT_DAYS),
            access_expires_at: None,
            client: Client::Loopback,
            storage: Storage::File,
        }
    }

    #[test]
    fn writing_a_record_needs_its_scope() {
        let settings = lexicon::SETTINGS;
        let needed = Some(format!("repo:{settings}?action=create&action=update"));
        assert_eq!(missing_scope(&signed_in_with(&["atproto"]), settings), needed);
        let all = publish_scopes();
        let all: Vec<&str> = all.iter().map(String::as_str).collect();
        for c in lexicon::COLLECTIONS {
            assert_eq!(missing_scope(&signed_in_with(&all), c), None, "{c}");
        }
        // Creating isn't enough: publishing again replaces the record.
        let create_only = format!("repo:{settings}?action=create");
        assert_eq!(missing_scope(&signed_in_with(&["atproto", &create_only]), settings), needed);
        let both = format!("repo:{settings}?action=create&action=update");
        assert_eq!(missing_scope(&signed_in_with(&["atproto", &both]), settings), None);
        assert_eq!(missing_scope(&signed_in_with(&["atproto", "repo:*"]), settings), None);
        assert_eq!(missing_scope(&signed_in_with(&["atproto", "transition:generic"]), settings), None);
        assert_eq!(missing_scope(&signed_in_with(&["atproto", "repo:app.example.thing"]), settings), needed);
    }

    /// A session as jacquard would hand it over, for `did`, with servers that refuse connections.
    fn session(did: &str, refresh: &str) -> jacquard_oauth::session::ClientSessionData {
        let key = jacquard_oauth::utils::generate_key(&["ES256"]).unwrap();
        let server = "https://127.0.0.1:9";
        serde_json::from_value(serde_json::json!({
            "account_did": did,
            "session_id": "s1",
            "host_url": server,
            "authserver_url": server,
            "authserver_token_endpoint": format!("{server}/oauth/token"),
            "authserver_revocation_endpoint": format!("{server}/oauth/revoke"),
            "scopes": "atproto",
            "dpop_key": key,
            "dpop_authserver_nonce": "",
            "dpop_host_nonce": "",
            "iss": server,
            "sub": did,
            "aud": server,
            "scope": "atproto",
            "refresh_token": refresh,
            "access_token": "access-token",
            "token_type": "DPoP",
            "expires_at": "2099-01-01T00:00:00.000Z"
        }))
        .unwrap()
    }

    /// Sign `did` in on this computer (session file storage) with a session for the PDS `pds`
    /// that was granted `scopes`.
    pub(crate) fn save_session(root: &Path, did: &str, pds: &str, scopes: &str) {
        let mut s = serde_json::to_value(session(did, "refresh-1")).unwrap();
        s["host_url"] = pds.into();
        s["scopes"] = scopes.into();
        s["scope"] = scopes.into();
        let store =
            LocalStore::new(root, Storage::File, Client::Loopback, "http://127.0.0.1:1/callback".into(), vec![]);
        let session = serde_json::from_value(s).unwrap();
        block_on(jacquard_oauth::authstore::ClientAuthStore::upsert_session(&store, session)).unwrap();
    }

    #[test]
    fn a_saved_session_keeps_its_secrets_apart_and_signing_out_removes_it() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let store =
            LocalStore::new(root, Storage::File, Client::Loopback, "http://127.0.0.1:1/callback".into(), vec![]);
        block_on(jacquard_oauth::authstore::ClientAuthStore::upsert_session(
            &store,
            session("did:plc:abc", "refresh-1"),
        ))
        .unwrap();
        let account = std::fs::read_to_string(root.join(store::ACCOUNT_FILE)).unwrap();
        assert!(account.contains("access-token") && !account.contains("refresh-1") && !account.contains("\"d\""));
        let secrets = std::fs::read_to_string(root.join(store::SECRETS_FILE)).unwrap();
        assert!(secrets.contains("refresh-1") && secrets.contains("\"d\""));

        let did: Did = Did::new_owned("did:plc:abc").unwrap();
        let back =
            block_on(jacquard_oauth::authstore::ClientAuthStore::get_session(&store, &did, "s1")).unwrap().unwrap();
        assert_eq!(back.token_set.refresh_token.as_deref(), Some("refresh-1"));
        assert_eq!(back.token_set.access_token.as_str(), "access-token");
        let info = status(root).unwrap().unwrap();
        assert_eq!((info.did.as_str(), info.storage), ("did:plc:abc", Storage::File));
        assert_eq!(info.days_left(), SESSION_LIMIT_DAYS - 1);

        // Another account replaces the first, secrets and all, and keeps its own.
        block_on(jacquard_oauth::authstore::ClientAuthStore::upsert_session(
            &store,
            session("did:plc:other", "refresh-2"),
        ))
        .unwrap();
        assert_eq!(status(root).unwrap().unwrap().did, "did:plc:other");
        assert_eq!(store::read_secrets(root, Storage::File, "did:plc:abc").unwrap(), None);
        let theirs = store::read_secrets(root, Storage::File, "did:plc:other").unwrap().unwrap();
        assert_eq!(theirs["refresh_token"], "refresh-2");
        let other: Did = Did::new_owned("did:plc:other").unwrap();
        assert!(block_on(ClientAuthStore::get_session(&store, &other, "s1")).unwrap().is_some());

        // The server can't be reached: the session still leaves this computer.
        let r = logout(root).unwrap();
        assert_eq!(r.account.unwrap().did, "did:plc:other");
        assert!(!r.revoked && r.left_behind.is_none());
        assert!(status(root).unwrap().is_none());
        assert!(!root.join(store::SECRETS_FILE).exists());
    }

    /// Keychain items go to an in-memory keychain in this test binary, never the computer's.
    pub(crate) fn mock_keychain() {
        static MOCK: std::sync::Once = std::sync::Once::new();
        MOCK.call_once(|| {
            // keyring sets the system's keychain as the default the first time it's used.
            let _ = keyring::Entry::store_status();
            keyring_core::set_default_store(keyring_core::mock::Store::new().unwrap());
        });
    }

    /// The mock keychain's next answer for `did`'s item in the store at `root` is an error.
    fn keychain_fails(root: &Path, did: &str) {
        let entry = keyring::Entry::new(store::KEYCHAIN_SERVICE, &store::keychain_account(root, did).unwrap()).unwrap();
        let cred: &keyring_core::mock::Cred = entry.inner.as_any().downcast_ref().unwrap();
        cred.set_error(keyring::Error::NoStorageAccess(std::io::Error::other("locked").into()));
    }

    #[test]
    fn signing_in_where_the_secrets_go_elsewhere_removes_the_old_ones() {
        mock_keychain();
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        crate::store::Store::open_or_init(root).unwrap();
        let in_file =
            LocalStore::new(root, Storage::File, Client::Loopback, "http://127.0.0.1:1/callback".into(), vec![]);
        let in_keychain =
            LocalStore::new(root, Storage::Keychain, Client::Loopback, "http://127.0.0.1:1/callback".into(), vec![]);
        block_on(ClientAuthStore::upsert_session(&in_file, session("did:plc:abc", "refresh-1"))).unwrap();
        assert!(root.join(store::SECRETS_FILE).exists());
        // The same account signed in again, kept in the keychain this time: the file's copy goes.
        let mut again = session("did:plc:abc", "refresh-2");
        again.session_id = "s2".into();
        block_on(ClientAuthStore::upsert_session(&in_keychain, again)).unwrap();
        assert!(!root.join(store::SECRETS_FILE).exists(), "the old refresh token doesn't outlive the switch");
        let kept = store::read_secrets(root, Storage::Keychain, "did:plc:abc").unwrap().unwrap();
        assert_eq!(kept["refresh_token"], "refresh-2");

        // The keychain stops answering: signing in to the file still works, and signing out
        // still signs out, saying what stayed behind.
        keychain_fails(root, "did:plc:abc");
        block_on(ClientAuthStore::upsert_session(&in_file, session("did:plc:other", "refresh-3"))).unwrap();
        assert_eq!(status(root).unwrap().unwrap().did, "did:plc:other");
        assert_eq!(AccountFile::load(root).unwrap().left_behind, [(Storage::Keychain, "did:plc:abc".to_string())]);
        keychain_fails(root, "did:plc:abc");
        let r = logout(root).unwrap();
        assert_eq!(r.account.unwrap().did, "did:plc:other");
        assert!(r.left_behind.unwrap().contains("keychain isn't available"));
        assert!(status(root).unwrap().is_none() && !root.join(store::SECRETS_FILE).exists());
        // Signing out again tries again.
        let r = logout(root).unwrap();
        assert!(r.account.is_none() && r.left_behind.is_none());
        assert!(AccountFile::load(root).unwrap().left_behind.is_empty());
        assert_eq!(store::read_secrets(root, Storage::Keychain, "did:plc:abc").unwrap(), None);
    }

    #[test]
    fn the_keychain_item_every_store_shared_before_goes_too() {
        mock_keychain();
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        crate::store::Store::open_or_init(root).unwrap();
        let did = "did:plc:earlier";
        // Where an earlier version kept the secrets, whichever store folder signed in.
        let old = || keyring::Entry::new(store::KEYCHAIN_SERVICE, &format!("atmosphere:{did}")).unwrap();
        let old_kept = || !matches!(old().get_password(), Err(keyring::Error::NoEntry));
        let in_keychain =
            LocalStore::new(root, Storage::Keychain, Client::Loopback, "http://127.0.0.1:1/callback".into(), vec![]);

        // Signing in removes it.
        old().set_password(r#"{"refresh_token":"old"}"#).unwrap();
        block_on(ClientAuthStore::upsert_session(&in_keychain, session(did, "refresh-1"))).unwrap();
        assert!(!old_kept(), "signing in removes it");
        assert!(store::read_secrets(root, Storage::Keychain, did).unwrap().is_some());
        // So does signing out.
        old().set_password(r#"{"refresh_token":"old"}"#).unwrap();
        let r = logout(root).unwrap();
        assert!(r.account.is_some() && r.left_behind.is_none());
        assert!(!old_kept(), "signing out removes it");

        // The keychain won't let go of it: signed in all the same, and signing out tries again.
        old().set_password(r#"{"refresh_token":"old"}"#).unwrap();
        let entry = old();
        let cred: &keyring_core::mock::Cred = entry.inner.as_any().downcast_ref().unwrap();
        cred.set_error(keyring::Error::NoStorageAccess(std::io::Error::other("locked").into()));
        block_on(ClientAuthStore::upsert_session(&in_keychain, session(did, "refresh-2"))).unwrap();
        assert_eq!(status(root).unwrap().unwrap().did, did);
        assert_eq!(AccountFile::load(root).unwrap().left_behind, [(Storage::Keychain, did.to_string())]);
        assert!(old_kept());
        let r = logout(root).unwrap();
        assert!(r.left_behind.is_none() && AccountFile::load(root).unwrap().left_behind.is_empty());
        assert!(!old_kept(), "signing out tried again");
    }

    /// A sign-in waiting for the browser, with `state`.
    fn waiting(root: &Path, state: &str) -> String {
        let store =
            LocalStore::new(root, Storage::File, Client::Loopback, "http://127.0.0.1:1/callback".into(), vec![]);
        let request = jacquard_oauth::session::AuthRequestData {
            state: state.into(),
            authserver_url: "https://127.0.0.1:9".into(),
            account_did: None,
            scopes: scopes(&[]).unwrap(),
            request_uri: "urn:ietf:params:oauth:request_uri:x".into(),
            authserver_token_endpoint: "https://127.0.0.1:9/oauth/token".into(),
            authserver_revocation_endpoint: None,
            pkce_verifier: "verifier".into(),
            dpop_data: jacquard_oauth::session::DpopReqData {
                dpop_key: jacquard_oauth::utils::generate_key(&["ES256"]).unwrap(),
                dpop_authserver_nonce: None,
            },
        };
        block_on(ClientAuthStore::save_auth_req_info(&store, &request)).unwrap();
        let (listener, redirect) = callback::bind().unwrap();
        let config = client_config(&Client::Loopback, &redirect, scopes(&[]).unwrap()).unwrap();
        let pending = Pending {
            oauth: oauth(store, config, true).unwrap(),
            listener,
            root: root.to_path_buf(),
            input: "dj.example.com".into(),
            requested: vec![],
            started: Instant::now(),
        };
        let id = uuid::Uuid::new_v4().simple().to_string();
        PENDING.lock().unwrap().insert(id.clone(), pending);
        id
    }

    #[test]
    fn only_the_sign_ins_own_redirect_can_refuse_it() {
        let dir = tempfile::tempdir().unwrap();
        // Someone else's refusal, or one without a state, isn't this sign-in's.
        let id = waiting(dir.path(), "st4te");
        let err =
            finish_login_with_url(&id, "http://127.0.0.1:1/callback?error=access_denied&state=guess").unwrap_err();
        assert!(err.to_string().contains("different sign-in attempt"), "{err}");
        let id = waiting(dir.path(), "st4te");
        let err =
            finish_login_with_url(&id, "http://127.0.0.1:1/callback?error=x&error_description=gotcha").unwrap_err();
        assert!(err.to_string().contains("different sign-in attempt"), "{err}");
        // Its own is.
        let id = waiting(dir.path(), "st4te");
        let err =
            finish_login_with_url(&id, "http://127.0.0.1:1/callback?error=access_denied&state=st4te").unwrap_err();
        assert!(err.to_string().contains("declined"), "{err}");
    }

    #[test]
    fn signing_in_only_reaches_public_https_servers() {
        // A server on this computer, named by the account's handle or typed in: nothing is sent.
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let options = LoginOptions { client: Some(Client::Loopback), storage: Storage::File, scopes: vec![] };
        let server = format!("https://127.0.0.1:{}", listener.local_addr().unwrap().port());
        assert!(begin_login(dir.path(), &server, &options).is_err());
        assert!(listener.accept().is_err(), "no connection was made");
        // Nor for a client document.
        let options =
            LoginOptions { client: Some(Client::Hosted { client_id: format!("{server}/c.json") }), ..options };
        let err = begin_login(dir.path(), "dj.example.com", &options).unwrap_err();
        assert!(err.to_string().contains("on this computer or its local network"), "{err}");
        let options = LoginOptions {
            client: Some(Client::Hosted { client_id: "http://locallibrary.example/c.json".into() }),
            ..options
        };
        assert!(
            begin_login(dir.path(), "dj.example.com", &options).unwrap_err().to_string().contains("isn't an https")
        );
        assert!(listener.accept().is_err(), "no connection was made");
    }

    #[test]
    fn the_development_client_stands_in_only_while_the_document_is_offline() {
        let (client, note) = own_client(Ok(())).unwrap();
        assert_eq!((client, note), (Client::published(), None));
        let (client, note) = own_client(Err(Unusable::Offline("its server couldn't be reached".into()))).unwrap();
        assert_eq!(client, Client::Loopback);
        assert!(note.unwrap().contains("its server couldn't be reached"));
        // Online but not fit: said, with the way on.
        let err = own_client(Err(Unusable::Unfit("it doesn't allow the scope repo:x".into()))).unwrap_err();
        assert!(err.to_string().contains("doesn't allow the scope repo:x"), "{err}");
        assert_eq!(err.detail().actions[0].params["dev_client"], true);
    }

    #[test]
    fn nothing_signed_in_reads_as_none() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(status(dir.path()).unwrap(), None);
        assert_eq!(last_handle(dir.path()), None);
        let r = logout(dir.path()).unwrap();
        assert!(r.account.is_none() && !r.revoked);
        assert!(check(dir.path(), &Ctx::silent()).unwrap_err().to_string().contains("not signed in"));
        assert!(std::fs::read_dir(dir.path()).unwrap().next().is_none(), "nothing written when nobody is signed in");
        assert!(finish_login("nope", Duration::from_secs(1), &Ctx::silent()).is_err());
        assert!(begin_login(dir.path(), "  ", &LoginOptions::default()).is_err());
    }
}
