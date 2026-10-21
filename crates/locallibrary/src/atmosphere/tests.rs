use jacquard_oauth::authstore::ClientAuthStore;

use super::*;

#[path = "../../tests/support/mock_pds.rs"]
pub(crate) mod mock_pds;

/// jacquard's message can leave out why (a request's "storage error"): a refused sign-in
/// says its causes too, each once.
#[test]
fn sign_in_errors_say_their_causes() {
    use jacquard_common::session::SessionStoreError;
    use jacquard_oauth::request::{RequestError, RequestErrorKind};
    let full = || Some(Box::new(std::io::Error::other("disk full")) as _);
    let e = OAuthError::Request(RequestError::new(RequestErrorKind::Storage, full()));
    assert_eq!(oauth_error(e).to_string(), "signing in: storage error: disk full");
    let e = OAuthError::Storage(SessionStoreError::Io(std::io::Error::other("disk full")));
    assert_eq!(oauth_error(e).to_string(), "signing in: I/O error: disk full", "named once");
}

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
        client_config(&hosted("http://insecure.example/c.json"), "http://127.0.0.1:1/callback", scopes(&[]).unwrap())
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
        "atproto include:computer.localnetwork.locallibraryPublishing repo:app.userinput.discussion?action=create"
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
    // The server grants the permission set's scopes in its place: these, each one jacquard
    // understands, written the way it writes them back.
    let granted = mock_pds::granted(&publish_scopes());
    assert_eq!(granted, [&PUBLISH_SCOPES[..], &["atproto"]].concat());
    for s in PUBLISH_SCOPES {
        assert_eq!(scopes(&["atproto".into(), s.into()]).unwrap().to_normalized_string(), format!("atproto {s}"));
    }
    // Creating and replacing, and nothing more: not deleting.
    let granted = scopes(&granted).unwrap();
    for c in lexicon::COLLECTIONS {
        assert!(granted.grants(&Scope::repo_create(c).unwrap()) && granted.grants(&Scope::repo_update(c).unwrap()));
        assert!(!granted.grants(&Scope::repo_delete(c).unwrap()), "{c}");
    }
}

pub(crate) fn signed_in_with(scopes: &[&str]) -> AccountInfo {
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
    let all = mock_pds::granted(&publish_scopes());
    let all: Vec<&str> = all.iter().map(String::as_str).collect();
    // The set's name grants nothing by itself: only what the server granted in its place.
    let asked = publish_scopes();
    let asked: Vec<&str> = asked.iter().map(String::as_str).collect();
    for c in lexicon::COLLECTIONS {
        assert_eq!(missing_scope(&signed_in_with(&all), c), None, "{c}");
        assert!(missing_scope(&signed_in_with(&asked), c).is_some(), "{c}");
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
pub(crate) fn session(did: &str, refresh: &str) -> jacquard_oauth::session::ClientSessionData {
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
    let store = LocalStore::new(root, Storage::File, Client::Loopback, "http://127.0.0.1:1/callback".into(), vec![]);
    let session = serde_json::from_value(s).unwrap();
    block_on(jacquard_oauth::authstore::ClientAuthStore::upsert_session(&store, session)).unwrap();
}

#[test]
fn a_saved_session_keeps_its_secrets_apart_and_signing_out_removes_it() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let store = LocalStore::new(root, Storage::File, Client::Loopback, "http://127.0.0.1:1/callback".into(), vec![]);
    block_on(jacquard_oauth::authstore::ClientAuthStore::upsert_session(&store, session("did:plc:abc", "refresh-1")))
        .unwrap();
    let account = std::fs::read_to_string(root.join(store::ACCOUNT_FILE)).unwrap();
    assert!(account.contains("access-token") && !account.contains("refresh-1") && !account.contains("\"d\""));
    let secrets = std::fs::read_to_string(root.join(store::SECRETS_FILE)).unwrap();
    assert!(secrets.contains("refresh-1") && secrets.contains("\"d\""));

    let did: Did = Did::new_owned("did:plc:abc").unwrap();
    let back = block_on(jacquard_oauth::authstore::ClientAuthStore::get_session(&store, &did, "s1")).unwrap().unwrap();
    assert_eq!(back.token_set.refresh_token.as_deref(), Some("refresh-1"));
    assert_eq!(back.token_set.access_token.as_str(), "access-token");
    let info = status(root).unwrap().unwrap();
    assert_eq!((info.did.as_str(), info.storage), ("did:plc:abc", Storage::File));
    assert_eq!(info.days_left(), SESSION_LIMIT_DAYS - 1);

    // Another account replaces the first, secrets and all, and keeps its own.
    block_on(jacquard_oauth::authstore::ClientAuthStore::upsert_session(&store, session("did:plc:other", "refresh-2")))
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

pub(crate) use crate::keychain::tests::mock_keychain;

/// The mock keychain's next answer for `did`'s item in the store at `root` is an error.
pub(crate) fn keychain_fails(root: &Path, did: &str) {
    let entry = store::keychain_entry(root, did).unwrap();
    let cred: &keyring_core::mock::Cred = entry.as_any().downcast_ref().unwrap();
    cred.set_error(keyring::Error::NoStorageAccess(std::io::Error::other("locked").into()));
}

#[test]
fn signing_in_where_the_secrets_go_elsewhere_removes_the_old_ones() {
    mock_keychain();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    crate::store::Store::open_or_init(root).unwrap();
    let in_file = LocalStore::new(root, Storage::File, Client::Loopback, "http://127.0.0.1:1/callback".into(), vec![]);
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

/// A sign-in waiting for the browser, with `state`.
fn waiting(root: &Path, state: &str) -> String {
    waiting_at(root, state, "https://127.0.0.1:9", true)
}

/// [`waiting`], at the authorization server `authserver`, reached only if it's at a public
/// address with `public_only`.
fn waiting_at(root: &Path, state: &str, authserver: &str, public_only: bool) -> String {
    let store = LocalStore::new(root, Storage::File, Client::Loopback, "http://127.0.0.1:1/callback".into(), vec![]);
    let request = jacquard_oauth::session::AuthRequestData {
        state: state.into(),
        authserver_url: authserver.into(),
        account_did: None,
        scopes: scopes(&[]).unwrap(),
        request_uri: "urn:ietf:params:oauth:request_uri:x".into(),
        authserver_token_endpoint: format!("{authserver}/oauth/token").into(),
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
        oauth: oauth(store, config, public_only).unwrap(),
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
    let err = finish_login_with_url(&id, "http://127.0.0.1:1/callback?error=access_denied&state=guess").unwrap_err();
    assert!(err.to_string().contains("different sign-in attempt"), "{err}");
    let id = waiting(dir.path(), "st4te");
    let err = finish_login_with_url(&id, "http://127.0.0.1:1/callback?error=x&error_description=gotcha").unwrap_err();
    assert!(err.to_string().contains("different sign-in attempt"), "{err}");
    // Its own is.
    let id = waiting(dir.path(), "st4te");
    let err = finish_login_with_url(&id, "http://127.0.0.1:1/callback?error=access_denied&state=st4te").unwrap_err();
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
    let options = LoginOptions { client: Some(Client::Hosted { client_id: format!("{server}/c.json") }), ..options };
    let err = begin_login(dir.path(), "dj.example.com", &options).unwrap_err();
    assert!(err.to_string().contains("on this computer or its local network"), "{err}");
    let options = LoginOptions {
        client: Some(Client::Hosted { client_id: "http://locallibrary.example/c.json".into() }),
        ..options
    };
    assert!(begin_login(dir.path(), "dj.example.com", &options).unwrap_err().to_string().contains("isn't an https"));
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

/// A server on 127.0.0.1 answering each request with `answer(its base URL, "METHOD /path")`:
/// a status, extra header lines (each ending in CRLF) and a JSON body. Returns its base URL.
pub(crate) fn serving(answer: impl Fn(&str, &str) -> (u16, String, serde_json::Value) + Send + 'static) -> String {
    use std::io::{BufRead, BufReader, Read, Write};
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let url = base.clone();
    std::thread::spawn(move || {
        for mut conn in listener.incoming().flatten() {
            let mut reader = BufReader::new(conn.try_clone().unwrap());
            let (mut line, mut header, mut length) = (String::new(), String::new(), 0);
            let _ = reader.read_line(&mut line);
            while reader.read_line(&mut header).is_ok_and(|n| n > 2) {
                if let Some(n) = header.to_ascii_lowercase().strip_prefix("content-length:") {
                    length = n.trim().parse().unwrap_or(0);
                }
                header.clear();
            }
            let _ = reader.read_exact(&mut vec![0; length]);
            let target: Vec<&str> = line.split_whitespace().take(2).collect();
            let (status, headers, body) = answer(&base, &target.join(" "));
            let body = if status == 204 { String::new() } else { body.to_string() };
            let _ = write!(
                conn,
                "HTTP/1.1 {status} X\r\n{headers}content-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            );
        }
    });
    url
}

/// The browser that comes back is shown how the sign-in went; anything else knocking on the
/// listener is turned away while it waits.
#[test]
fn the_browser_is_shown_how_the_sign_in_went() {
    use std::io::{Read, Write};
    let dir = tempfile::tempdir().unwrap();
    let id = waiting(dir.path(), "st4te");
    let port = PENDING.lock().unwrap()[&id].listener.local_addr().unwrap().port();
    let send = |query: &str| {
        let mut s = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
        write!(s, "GET /callback?{query} HTTP/1.1\r\nhost: 127.0.0.1\r\n\r\n").unwrap();
        s
    };
    let read = |mut s: std::net::TcpStream| {
        let mut text = String::new();
        s.read_to_string(&mut text).unwrap();
        text
    };
    let stranger = send("code=c&state=guess");
    let browser = send("error=access_denied&state=st4te");
    let err = finish_login(&id, Duration::from_secs(60), &Ctx::silent()).unwrap_err();
    assert!(err.to_string().contains("declined"), "{err}");
    assert!(read(stranger).starts_with("HTTP/1.1 400"));
    let page = read(browser);
    assert!(page.starts_with("HTTP/1.1 200") && page.contains("couldn't sign you in"), "{page}");
    assert!(page.contains("the sign-in was declined"), "{page}");
    assert!(!cancel_login(&id), "finished, so nothing to cancel");
}

#[test]
fn a_cancelled_sign_in_cant_be_finished() {
    let dir = tempfile::tempdir().unwrap();
    let id = waiting(dir.path(), "st4te");
    assert!(cancel_login(&id));
    let err = finish_login_with_url(&id, "http://127.0.0.1:1/callback?code=c&state=st4te").unwrap_err();
    assert!(matches!(err, Error::NotFound(_)), "{err}");
}

/// The server's metadata, as the mock PDS's (`tests/support/mock_pds.rs`) describes itself.
fn authorization_server(base: &str) -> serde_json::Value {
    serde_json::json!({
        "issuer": base,
        "authorization_endpoint": format!("{base}/oauth/authorize"),
        "token_endpoint": format!("{base}/oauth/token"),
        "pushed_authorization_request_endpoint": format!("{base}/oauth/par"),
        "revocation_endpoint": format!("{base}/oauth/revoke"),
        "scopes_supported": ["atproto"],
        "response_types_supported": ["code"],
        "grant_types_supported": ["authorization_code", "refresh_token"],
        "code_challenge_methods_supported": ["S256"],
        "token_endpoint_auth_methods_supported": ["none"],
        "dpop_signing_alg_values_supported": ["ES256"],
        "require_pushed_authorization_requests": true,
        "authorization_response_iss_parameter_supported": true,
        "client_id_metadata_document_supported": true,
    })
}

/// A redirect naming another issuer than the server the sign-in went to is a mix-up (RFC
/// 9207): refused before the code goes anywhere. The sign-in's own goes on to the token
/// exchange, and a server refusing the code says why.
#[test]
fn a_redirect_from_another_server_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let tokens_asked = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let asked = tokens_asked.clone();
    let auth = serving(move |base, target| match target {
        "GET /.well-known/oauth-authorization-server" => (200, String::new(), authorization_server(base)),
        "POST /oauth/token" => {
            asked.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let refusal = serde_json::json!({ "error": "invalid_grant", "error_description": "code expired" });
            (400, String::new(), refusal)
        }
        _ => (404, String::new(), serde_json::json!({})),
    });
    let id = waiting_at(dir.path(), "st4te", &auth, false);
    let url = "http://127.0.0.1:1/callback?code=c&state=st4te&iss=https%3A%2F%2Fevil.example";
    let err = finish_login_with_url(&id, url).unwrap_err();
    assert!(err.to_string().contains("came back from a different server"), "{err}");
    assert_eq!(tokens_asked.load(std::sync::atomic::Ordering::SeqCst), 0);

    let id = waiting_at(dir.path(), "st4te", &auth, false);
    let url = format!("http://127.0.0.1:1/callback?code=c&state=st4te&iss={auth}");
    let err = finish_login_with_url(&id, &url).unwrap_err();
    assert!(err.to_string().starts_with("signing in: ") && err.to_string().contains("code expired"), "{err}");
    assert_eq!(tokens_asked.load(std::sync::atomic::Ordering::SeqCst), 1);
    assert_eq!(status(dir.path()).unwrap(), None);
    // The state is spent either way: the same redirect again is another sign-in's.
    assert_eq!(
        oauth_error(OAuthError::Callback(jacquard_oauth::error::CallbackError::MissingState)).to_string(),
        other_attempt().to_string()
    );
}

/// Asking the server who's signed in: a server that can't be reached, or answers with an
/// error, says so and leaves the session as it is; a handle that doesn't check out isn't
/// taken.
#[test]
fn checking_says_what_the_server_said() {
    let unreachable = tempfile::tempdir().unwrap();
    save_session(unreachable.path(), "did:plc:abc", "http://127.0.0.1:9", "atproto");
    let err = check(unreachable.path(), &Ctx::silent()).unwrap_err();
    assert!(err.to_string().contains("asking the account's server who's signed in"), "{err}");
    assert_eq!(status(unreachable.path()).unwrap().unwrap().did, "did:plc:abc");

    let pds = serving(|_, target| match target {
        "GET /xrpc/com.atproto.server.getSession" => {
            (400, String::new(), serde_json::json!({ "error": "InvalidRequest", "message": "down for a bit" }))
        }
        _ => (404, String::new(), serde_json::json!({})),
    });
    let dir = tempfile::tempdir().unwrap();
    save_session(dir.path(), "did:plc:abc", &pds, "atproto");
    let err = check(dir.path(), &Ctx::silent()).unwrap_err();
    assert!(err.to_string().contains("couldn't say who's signed in"), "{err}");
    assert!(status(dir.path()).unwrap().is_some());

    let pds =
        serving(|_, _| (200, String::new(), serde_json::json!({ "did": "did:plc:abc", "handle": "handle.invalid" })));
    let dir = tempfile::tempdir().unwrap();
    save_session(dir.path(), "did:plc:abc", &pds, "atproto");
    let mut file = AccountFile::load(dir.path()).unwrap();
    file.account.as_mut().unwrap().info.handle = Some("dj.example.com".into());
    file.save(dir.path()).unwrap();
    assert_eq!(check(dir.path(), &Ctx::silent()).unwrap().handle.as_deref(), Some("dj.example.com"));
}

/// Sign `did:plc:abc` in with a session whose servers are all `server`, and whose access
/// token runs out at `expires_at`.
fn signed_in_at(root: &Path, server: &str, expires_at: &str) {
    let mut s = serde_json::to_value(session("did:plc:abc", "refresh-1")).unwrap();
    for field in ["host_url", "authserver_url", "iss", "aud"] {
        s[field] = server.into();
    }
    s["authserver_token_endpoint"] = format!("{server}/oauth/token").into();
    s["authserver_revocation_endpoint"] = format!("{server}/oauth/revoke").into();
    s["expires_at"] = expires_at.into();
    let store = LocalStore::new(root, Storage::File, Client::Loopback, "http://127.0.0.1:1/callback".into(), vec![]);
    block_on(ClientAuthStore::upsert_session(&store, serde_json::from_value(s).unwrap())).unwrap();
}

/// A server that offers no revocation still signs out here, unrevoked; one that can't
/// refresh a session for now (it's down) leaves it signed in, to try again.
#[test]
fn a_server_that_cant_help_leaves_what_it_can() {
    let server = serving(|base, target| match target {
        "GET /.well-known/oauth-authorization-server" => {
            let mut metadata = authorization_server(base);
            metadata.as_object_mut().unwrap().remove("revocation_endpoint");
            (200, String::new(), metadata)
        }
        _ => (503, String::new(), serde_json::json!({ "error": "unavailable" })),
    });
    let dir = tempfile::tempdir().unwrap();
    signed_in_at(dir.path(), &server, "2099-01-01T00:00:00.000Z");
    let r = logout(dir.path()).unwrap();
    assert!(r.account.is_some() && !r.revoked);
    assert_eq!(status(dir.path()).unwrap(), None);

    signed_in_at(dir.path(), &server, "2020-01-01T00:00:00.000Z");
    let err = check(dir.path(), &Ctx::silent()).unwrap_err();
    assert!(err.to_string().starts_with("signing in: "), "{err}");
    assert!(status(dir.path()).unwrap().is_some(), "still signed in");
}

/// Bluesky's servers answer a revocation with 204 (RFC 7009 says 200; the mock PDS's tests
/// cover that): both tokens are revoked either way.
#[test]
fn signing_out_where_revoking_answers_no_content_revokes() {
    let revoked = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let counted = revoked.clone();
    let server = serving(move |base, target| match target {
        "GET /.well-known/oauth-authorization-server" => (200, String::new(), authorization_server(base)),
        "POST /oauth/revoke" => {
            counted.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            (204, String::new(), serde_json::Value::Null)
        }
        _ => (404, String::new(), serde_json::json!({})),
    });
    let dir = tempfile::tempdir().unwrap();
    signed_in_at(dir.path(), &server, "2099-01-01T00:00:00.000Z");
    let r = logout(dir.path()).unwrap();
    assert!(r.revoked && r.account.is_some());
    assert_eq!(revoked.load(std::sync::atomic::Ordering::SeqCst), 2);
    assert_eq!(status(dir.path()).unwrap(), None);
}

#[test]
fn sessions_last_until_they_have_to_be_renewed() {
    let mut info = signed_in_with(&["atproto"]);
    assert!(!info.expired() && info.name() == "@dj.example.com");
    info.sign_in_again_by = Utc::now() - chrono::Duration::days(1);
    info.handle = None;
    assert!(info.expired() && info.days_left() < 0 && info.name() == "did:plc:abc");
    // Not a record type at all: nothing grants it.
    assert!(missing_scope(&signed_in_with(&["atproto", "transition:generic"]), "not an nsid").is_none());
    assert_eq!(missing_scope(&info, "not an nsid").as_deref(), Some("repo:not an nsid?action=create&action=update"));
    // Scopes the document lists anyway aren't listed twice.
    let doc = client_metadata(CLIENT_ID, &[publish_scopes(), vec![REPORT_SCOPE.into()]].concat()).unwrap();
    assert_eq!(doc["scope"], client_metadata(CLIENT_ID, &[]).unwrap()["scope"]);
    let doc = client_metadata(CLIENT_ID, &["repo:app.example.thing".into()]).unwrap();
    assert!(doc["scope"].as_str().unwrap().split(' ').any(|s| s == "repo:app.example.thing"));
}

use hegel::generators as gs;
use hegel::{Generator, TestCase};

fn alnum() -> impl hegel::PrintableGenerator<String> {
    gs::text().alphabet("abcdefghijklmnopqrstuvwxyz0123456789").min_size(1).max_size(10)
}

/// A client document is checked the way a server would, for the first thing wrong with it: a
/// client_id that isn't its address, an app that isn't native, no loopback redirect, or a
/// scope asked for that it doesn't allow.
#[hegel::test(test_cases = 300)]
fn documents_are_refused_for_the_first_thing_wrong(tc: TestCase) {
    let extra: Vec<String> = tc
        .draw(gs::vecs(gs::sampled_from(vec!["transition:generic", "repo:app.example.thing"])).max_size(2))
        .into_iter()
        .map(String::from)
        .collect();
    let mut doc = client_metadata(CLIENT_ID, &extra).unwrap();
    let other_id = tc.draw(gs::booleans());
    if other_id {
        doc["client_id"] = "https://other.example/oauth-client-metadata.json".into();
    }
    let kind =
        tc.draw(gs::sampled_from(vec![serde_json::json!("native"), serde_json::json!("web"), serde_json::Value::Null]));
    doc["application_type"] = kind.clone();
    let redirects = tc.draw(gs::sampled_from(vec![
        serde_json::json!(["http://127.0.0.1/callback"]),
        serde_json::json!(["http://127.0.0.1:8080/callback", "http://127.0.0.1/callback"]),
        serde_json::json!(["https://example.com/callback"]),
        serde_json::json!([]),
        serde_json::Value::Null,
    ]));
    doc["redirect_uris"] = redirects.clone();
    let mut allowed: Vec<String> = doc["scope"].as_str().unwrap().split(' ').map(String::from).collect();
    if let Some(i) = tc.draw(gs::optional(gs::integers::<usize>().max_value(allowed.len() - 1))) {
        allowed.remove(i);
        doc["scope"] = allowed.join(" ").into();
    }
    let include = format!("include:{PUBLISH_SET}").leak();
    let pool = vec!["atproto", "transition:generic", "repo:app.example.thing", include, PUBLISH_SCOPES[2]];
    let requested: Vec<String> =
        tc.draw(gs::vecs(gs::sampled_from(pool)).max_size(4)).into_iter().map(String::from).collect();

    let loopback = redirects.as_array().is_some_and(|r| r.iter().any(|u| u == "http://127.0.0.1/callback"));
    let missing = requested.iter().find(|s| !allowed.contains(s));
    let expected = if other_id {
        Some("its client_id is \"https://other.example/oauth-client-metadata.json\"".to_string())
    } else if kind != "native" {
        Some("it isn't a native app's".to_string())
    } else if !loopback {
        Some("it doesn't list http://127.0.0.1/callback".to_string())
    } else {
        missing.map(|m| format!("it doesn't allow the scope {m}"))
    };
    assert_eq!(check_document(&doc, CLIENT_ID, &requested).err(), expected, "{doc}");
}

/// Scopes to ask for are read when every one can be and `atproto` is among them (none at all
/// means `atproto`), and come back as asked, each once.
#[hegel::test(test_cases = 300)]
fn scopes_are_read_when_they_include_atproto(tc: TestCase) {
    let atoms = vec![
        ("atproto", true),
        ("transition:generic", true),
        ("repo:app.example.thing", true),
        (PUBLISH_SCOPES[0], true),
        (format!("include:{PUBLISH_SET}").leak(), true),
        ("foo:bar", false),
        ("repo:app.example.thing?action=publish", false),
    ];
    let list: Vec<(&str, bool)> = tc.draw(gs::vecs(gs::sampled_from(atoms)).max_size(4));
    let asked: Vec<String> = list.iter().map(|(s, _)| s.to_string()).collect();
    match scopes(&asked) {
        Ok(read) => {
            let mut want: Vec<String> = if asked.is_empty() { vec!["atproto".into()] } else { asked.clone() };
            want.sort();
            want.dedup();
            let mut got: Vec<String> = read.to_normalized_string().split(' ').map(String::from).collect();
            got.sort();
            assert_eq!(got, want);
        }
        Err(e) if list.iter().all(|(_, ok)| *ok) => {
            assert!(!asked.is_empty() && !asked.iter().any(|s| s == "atproto"), "{asked:?}: {e}");
            assert!(e.to_string().contains("have to include `atproto`"), "{e}");
        }
        Err(e) => assert!(e.to_string().contains(&format!("scopes {:?}", asked.join(" "))), "{e}"),
    }
}

/// A loopback client's ID carries its redirect, whatever the port, and the scopes it asks
/// for; a hosted client's is its document's https address, and nothing else will do.
#[hegel::test(test_cases = 200)]
fn clients_are_introduced_as_they_should_be(tc: TestCase) {
    let port = tc.draw(gs::integers::<u16>().min_value(1));
    let redirect = format!("http://127.0.0.1:{port}{CALLBACK_PATH}");
    let include: &str = format!("include:{PUBLISH_SET}").leak();
    let extra = tc.draw(gs::optional(gs::sampled_from(vec!["transition:generic", include])));
    let asked: Vec<String> = ["atproto"].into_iter().chain(extra).map(String::from).collect();
    let c = client_config(&Client::Loopback, &redirect, scopes(&asked).unwrap()).unwrap();
    let id = c.client_id.as_str();
    assert!(id.starts_with("http://localhost"), "{id}");
    assert!(id.contains(&format!("redirect_uri=http%3A%2F%2F127.0.0.1%3A{port}%2Fcallback")), "{id}");
    assert_eq!(c.redirect_uris.iter().map(|u| u.as_str()).collect::<Vec<_>>(), [redirect.as_str()]);
    assert_eq!(c.scopes.to_normalized_string(), scopes(&asked).unwrap().to_normalized_string());

    let host = tc.draw(gs::from_regex(r"[a-z][a-z0-9]{0,8}\.[a-z]{2,5}").fullmatch(true));
    let scheme = tc.draw(gs::sampled_from(vec!["https", "http", "ftp"]));
    let client_id = format!("{scheme}://{host}/oauth-client-metadata.json");
    let hosted = client_config(&Client::Hosted { client_id: client_id.clone() }, &redirect, scopes(&asked).unwrap());
    match scheme {
        "https" => {
            let c = hosted.unwrap();
            assert_eq!(c.client_id.as_str(), client_id);
            assert_eq!(c.client_name.as_deref(), Some("locallibrary"));
        }
        _ => assert!(hosted.unwrap_err().to_string().contains("is an https:// URL"), "{client_id}"),
    }
}

/// locallibrary's own client signs in when its document is fit; while it's offline the
/// development client stands in, saying why; a document that's online but doesn't fit
/// stops the sign-in, saying why and offering the development client.
#[hegel::test(test_cases = 100)]
fn the_own_client_falls_back_only_while_offline(tc: TestCase) {
    let why = tc.draw(gs::text().max_size(20));
    match tc.draw(gs::integers::<u8>().max_value(2)) {
        0 => assert_eq!(own_client(Ok(())).unwrap(), (Client::published(), None)),
        1 => {
            let (client, note) = own_client(Err(Unusable::Offline(why.clone()))).unwrap();
            assert_eq!(client, Client::Loopback);
            assert!(note.unwrap().contains(&format!("({why}), so this signs in as a local development app")));
        }
        _ => {
            let err = own_client(Err(Unusable::Unfit(why.clone()))).unwrap_err();
            assert!(err.to_string().contains(&format!("doesn't fit this sign-in ({why})")), "{err}");
            assert_eq!(err.detail().actions[0].params["dev_client"], true);
        }
    }
}

/// What went wrong signing in is said on one line, with its cause once, whatever the cause
/// says.
#[hegel::test(test_cases = 200)]
fn sign_in_errors_are_said_on_one_line(tc: TestCase) {
    use jacquard_oauth::request::{RequestError, RequestErrorKind};
    let cause = tc.draw(hegel::one_of!(
        gs::text().max_size(20),
        gs::sampled_from(vec!["storage".to_string(), "error".into(), "line\none\u{1b}[2J".into()])
    ));
    let e = OAuthError::Request(RequestError::new(
        RequestErrorKind::Storage,
        Some(Box::new(std::io::Error::other(cause.clone()))),
    ));
    let said = oauth_error(e).to_string();
    let expected = if "storage error".contains(&cause) {
        "signing in: storage error".to_string()
    } else {
        format!("signing in: storage error: {cause}")
    };
    assert_eq!(said, crate::export::one_line(&expected));
    assert!(!said.chars().any(char::is_control), "{said:?}");
}

/// The redirect back to a sign-in.
#[derive(Debug, Clone)]
struct Redirect {
    /// 0: no state, 1: the sign-in's own, 2: someone else's.
    state: u8,
    code: Option<String>,
    error: Option<String>,
    error_description: Option<String>,
}

#[hegel::test_helper]
fn redirect(tc: &TestCase) -> Redirect {
    Redirect {
        // Mostly the sign-in's own: the others all go the same way.
        state: tc.draw(gs::sampled_from(vec![0, 1, 1, 1, 2])),
        code: tc.draw(gs::optional(alnum())),
        error: tc.draw(gs::optional(gs::sampled_from(vec!["access_denied".to_string(), "server_error".into()]))),
        error_description: tc.draw(gs::optional(gs::text().max_size(12))),
    }
}

impl Redirect {
    fn query(&self, state: &str) -> String {
        use percent_encoding::{NON_ALPHANUMERIC, utf8_percent_encode};
        let state = match self.state {
            0 => None,
            1 => Some(state.to_string()),
            _ => Some(format!("{state}x")),
        };
        [
            ("state", &state),
            ("code", &self.code),
            ("error", &self.error),
            ("error_description", &self.error_description),
        ]
        .into_iter()
        .filter_map(|(k, v)| Some(format!("{k}={}", utf8_percent_encode(v.as_ref()?, NON_ALPHANUMERIC))))
        .collect::<Vec<_>>()
        .join("&")
    }

    /// How finishing with it goes: what the error says (it can't succeed without a server).
    fn expected(&self, state: &str) -> String {
        let q = CallbackQuery::parse(&self.query(state));
        if self.state != 1 {
            other_attempt().to_string()
        } else if let Some(refusal) = q.refusal() {
            refusal
        } else if self.code.is_none() {
            "the browser came back without a sign-in code".to_string()
        } else {
            "signing in: ".to_string()
        }
    }
}

/// Finishing with a pasted address goes the way a model says: one without a result leaves
/// the sign-in waiting; one from another attempt, or without a state, is turned away; the
/// sign-in's own refusal is said; its own code goes on to the token exchange (here, with a
/// server that isn't public, refused). Each but the first ends the sign-in, and nobody is
/// signed in.
#[hegel::test(test_cases = 4)]
fn finishing_with_a_pasted_address_agrees_with_a_model(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let state = tc.draw(alnum());
    let r = redirect(&tc);
    let id = waiting(dir.path(), &state);
    let url = format!("http://127.0.0.1:1/callback?{}", r.query(&state));
    let err = finish_login_with_url(&id, &url).unwrap_err().to_string();
    if r.code.is_none() && r.error.is_none() {
        assert!(err.contains("no sign-in result"), "{err}");
        assert!(cancel_login(&id), "still waiting");
    } else {
        let expected = r.expected(&state);
        assert!(
            if expected.ends_with(": ") { err.starts_with(&expected) } else { err == expected },
            "{expected:?}: {err}"
        );
        assert!(!cancel_login(&id), "over");
    }
    assert_eq!(status(dir.path()).unwrap(), None);
}

/// Finishing in the browser goes the same way (and without a code, says so), and the browser
/// is shown what went wrong, escaped.
#[hegel::test(test_cases = 3)]
fn finishing_in_the_browser_shows_how_it_went(tc: TestCase) {
    use std::io::{Read, Write};
    let dir = tempfile::tempdir().unwrap();
    let state = tc.draw(alnum());
    let mut r = redirect(&tc);
    // Only the sign-in's own redirect ends the wait.
    r.state = 1;
    let id = waiting(dir.path(), &state);
    let port = PENDING.lock().unwrap()[&id].listener.local_addr().unwrap().port();
    let mut browser = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
    write!(browser, "GET /callback?{} HTTP/1.1\r\nhost: 127.0.0.1\r\n\r\n", r.query(&state)).unwrap();
    let err = finish_login(&id, Duration::from_secs(30), &Ctx::silent()).unwrap_err().to_string();
    let expected = r.expected(&state);
    assert!(if expected.ends_with(": ") { err.starts_with(&expected) } else { err == expected }, "{expected:?}: {err}");
    let mut page = String::new();
    browser.read_to_string(&mut page).unwrap();
    assert!(page.contains("couldn't sign you in"), "{page}");
    let escaped = err.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;");
    assert!(page.contains(&escaped), "{page}");
    assert!(!cancel_login(&id));
}

/// How the account's server answers `getSession`.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Says {
    /// The handle it vouches for.
    Handle,
    /// `handle.invalid`: a handle that doesn't check out.
    NoHandle,
    /// 401: the session isn't accepted.
    Unauthorized,
    /// 401 saying the DPoP token is no good.
    TokenRefused,
    /// 400 `ExpiredToken`.
    Expired,
    /// 400 with another error.
    Refused,
    /// Nothing listens.
    Unreachable,
    /// The access token ran out, and the server is down, so it can't be refreshed for now.
    Down,
}

/// A session at a server on this computer that answers `getSession` as drawn and revokes tokens
/// with a drawn status (or offers no revocation): checking takes the handle the server vouches
/// for and keeps the one known otherwise, says the session has ended when the server won't take
/// it, and says what went wrong when it answers with another error, isn't there, or can't
/// refresh a session whose access token ran out, the session kept each time; signing out removes it whatever, revoked only when the server took both
/// tokens. Signed out, signing out again has nobody to sign out, and signing in needs an
/// account named.
#[hegel::test(test_cases = 40)]
fn checking_and_signing_out_say_what_the_server_did(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let says = tc.draw(gs::sampled_from(vec![
        Says::Handle,
        Says::NoHandle,
        Says::Unauthorized,
        Says::TokenRefused,
        Says::Expired,
        Says::Refused,
        Says::Unreachable,
        Says::Down,
    ]));
    let revocation: Option<u16> = tc.draw(gs::optional(gs::sampled_from(vec![200u16, 204, 500])));
    let known = tc.draw(gs::optional(gs::sampled_from(vec!["before.example.com".to_string()])));

    let server = match says {
        Says::Unreachable => "http://127.0.0.1:9".to_string(),
        _ => serving(move |base, target| match target {
            "GET /xrpc/com.atproto.server.getSession" => match says {
                Says::Handle => {
                    (200, String::new(), serde_json::json!({ "did": "did:plc:abc", "handle": "dj.example.com" }))
                }
                Says::NoHandle => {
                    (200, String::new(), serde_json::json!({ "did": "did:plc:abc", "handle": "handle.invalid" }))
                }
                Says::Unauthorized => (401, String::new(), serde_json::json!({ "error": "AuthenticationRequired" })),
                Says::TokenRefused => (
                    401,
                    "www-authenticate: DPoP error=\"invalid_token\"\r\n".to_string(),
                    serde_json::json!({ "error": "invalid_token" }),
                ),
                Says::Down => (503, String::new(), serde_json::json!({ "error": "unavailable" })),
                Says::Expired => {
                    (400, String::new(), serde_json::json!({ "error": "ExpiredToken", "message": "expired" }))
                }
                _ => {
                    (400, String::new(), serde_json::json!({ "error": "InvalidRequest", "message": "down for a bit" }))
                }
            },
            "GET /.well-known/oauth-authorization-server" => {
                let mut metadata = authorization_server(base);
                if revocation.is_none() {
                    metadata.as_object_mut().unwrap().remove("revocation_endpoint");
                }
                (200, String::new(), metadata)
            }
            "POST /oauth/revoke" => (revocation.unwrap_or(404), String::new(), serde_json::json!({})),
            "POST /oauth/token" => (503, String::new(), serde_json::json!({ "error": "unavailable" })),
            _ => (404, String::new(), serde_json::json!({})),
        }),
    };
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let expires = if says == Says::Down { "2020-01-01T00:00:00.000Z" } else { "2099-01-01T00:00:00.000Z" };
    signed_in_at(root, &server, expires);
    if let Some(h) = &known {
        let mut file = AccountFile::load(root).unwrap();
        file.account.as_mut().unwrap().info.handle = Some(h.clone());
        file.save(root).unwrap();
    }
    let checked = check(root, &Ctx::silent());
    match says {
        Says::Handle => assert_eq!(checked.unwrap().handle.as_deref(), Some("dj.example.com")),
        Says::NoHandle => assert_eq!(checked.unwrap().handle, known),
        Says::Unauthorized | Says::TokenRefused | Says::Expired => {
            assert_eq!(checked.unwrap_err().reason(), Some(Reason::SessionEnded))
        }
        Says::Refused => {
            let e = checked.unwrap_err().to_string();
            assert!(e.contains("couldn't say who's signed in") && e.contains("down for a bit"), "{e}");
        }
        Says::Unreachable => {
            let e = checked.unwrap_err().to_string();
            assert!(e.contains("asking the account's server who's signed in"), "{e}");
        }
        Says::Down => {
            let e = checked.unwrap_err().to_string();
            assert!(e.starts_with("signing in: "), "{e}");
        }
    }
    assert!(status(root).unwrap().is_some(), "still signed in");
    let r = logout(root).unwrap();
    let took = !matches!(says, Says::Unreachable | Says::Down) && matches!(revocation, Some(200 | 204));
    assert_eq!((r.account.map(|a| a.did), r.revoked), (Some("did:plc:abc".to_string()), took));
    assert_eq!(status(root).unwrap(), None);
    let again = logout(root).unwrap();
    assert!(again.account.is_none() && !again.revoked);
    let e = begin_login(root, " @ ", &LoginOptions::default()).unwrap_err();
    assert!(matches!(e, Error::Invalid(_)), "no account named: {e}");
}

/// A redirect back from the browser counts only when it carries this sign-in's own state:
/// anything else (no state, another's) is another sign-in attempt, whether it brings a code or
/// a refusal. Its own refusal says why, as the server worded it; its own code from another
/// issuer is a mix-up, refused; otherwise it goes on to the token exchange, which this server
/// refuses.
#[hegel::test(test_cases = 60)]
fn only_a_sign_ins_own_redirect_counts(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let state = tc.draw(gs::optional(gs::sampled_from(vec!["st4te", "guess"])));
    let answer = tc.draw(gs::sampled_from(vec![
        "error=access_denied",
        "error=x&error_description=gotcha",
        "code=c",
        "code=c&iss=https%3A%2F%2Fevil.example",
    ]));
    let dir = tempfile::tempdir().unwrap();
    let auth = serving(|base, target| match target {
        "GET /.well-known/oauth-authorization-server" => (200, String::new(), authorization_server(base)),
        _ => (404, String::new(), serde_json::json!({})),
    });
    let id = waiting_at(dir.path(), "st4te", &auth, false);
    let query = match state {
        Some(s) => format!("{answer}&state={s}"),
        None => answer.to_string(),
    };
    let err = finish_login_with_url(&id, &format!("http://127.0.0.1:1/callback?{query}")).unwrap_err().to_string();
    let said = match (state, answer) {
        (Some("st4te"), "error=access_denied") => "declined",
        (Some("st4te"), "code=c") => "signing in: ",
        (Some("st4te"), "code=c&iss=https%3A%2F%2Fevil.example") => "came back from a different server",
        (Some("st4te"), _) => "gotcha",
        _ => "the browser came back from a different sign-in attempt; sign in again",
    };
    assert!(err.contains(said), "{query}: {err}");
    assert_eq!(status(dir.path()).unwrap(), None, "nobody signed in");
    assert!(!cancel_login(&id), "finished either way");
}

/// A saved account whose DID or redirect address isn't one any more (the file was edited, or
/// damaged) can't be used, whatever it says instead: checking it says which is no good (the
/// address first), before anything is sent; signing out still removes it, revoking nothing.
#[hegel::test(test_cases = 30)]
fn a_saved_account_whose_did_or_redirect_isnt_one_cant_be_used(tc: TestCase) {
    // Anything not starting `did:` isn't a DID, nor is `did:` with no method or identifier.
    let did: Option<String> = tc.draw(gs::optional(hegel::one_of!(
        gs::sampled_from(vec!["", "did:", "did:plc:", "did:plc", "plc:abc", " did:plc:abc"]).map(String::from),
        gs::text().max_size(16).filter(|s: &String| !s.starts_with("did:"))
    )));
    let redirect: Option<&str> = tc.draw(gs::optional(gs::sampled_from(vec!["", "not a url", "http://[::1", "%%"])));
    tc.assume(did.is_some() || redirect.is_some());
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let server = format!("http://{}", listener.local_addr().unwrap());
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    crate::store::Store::open_or_init(root).unwrap();
    save_session(root, "did:plc:abc", &server, "atproto transition:generic");
    let mut file = AccountFile::load(root).unwrap();
    let account = file.account.as_mut().unwrap();
    if let Some(d) = &did {
        account.info.did = d.clone();
    }
    if let Some(r) = redirect {
        account.redirect_uri = r.to_string();
    }
    file.save(root).unwrap();

    let err = check(root, &Ctx::silent()).unwrap_err();
    let text = err.to_string();
    match redirect {
        Some(r) => {
            assert_eq!(err.code(), "invalid", "{text}");
            assert!(text.starts_with(&format!("{r:?} isn't a URL")), "{text}");
        }
        None => {
            assert_eq!(err.code(), "account", "{text}");
            assert!(text.starts_with("stored DID: "), "{did:?}: {text}");
        }
    }
    assert!(listener.accept().is_err(), "nothing was sent");
    let kept = did.clone().unwrap_or_else(|| "did:plc:abc".into());
    assert_eq!(status(root).unwrap().map(|i| i.did), Some(kept), "kept until signing out");
    let out = logout(root).unwrap();
    assert!(out.account.is_some() && !out.revoked);
    assert_eq!(status(root).unwrap(), None);
}

#[test]
fn signing_in_again_keeps_what_the_session_may_do() {
    let report = || REPORT_SCOPE.to_string();
    assert_eq!(kept_scopes(&signed_in_with(&["atproto"])), ["atproto"]);
    let publishing = mock_pds::granted(&publish_scopes());
    let publishing: Vec<&str> = publishing.iter().map(String::as_str).collect();
    assert_eq!(kept_scopes(&signed_in_with(&publishing)), publish_scopes());
    assert_eq!(kept_scopes(&signed_in_with(&["atproto", REPORT_SCOPE])), ["atproto".to_string(), report()]);
    let both = [&publishing[..], &[REPORT_SCOPE]].concat();
    assert_eq!(kept_scopes(&signed_in_with(&both)), [publish_scopes(), vec![report()]].concat());
    // locallibrary's document allows a sign-in that keeps them all.
    let doc = client_metadata(CLIENT_ID, &[]).unwrap();
    assert_eq!(check_document(&doc, CLIENT_ID, &kept_scopes(&signed_in_with(&both))), Ok(()));
    // Only a bug report asks for its scope: signing in and publishing don't.
    assert!(!publish_scopes().contains(&report()) && !DEFAULT_SCOPES.contains(&REPORT_SCOPE));
}
