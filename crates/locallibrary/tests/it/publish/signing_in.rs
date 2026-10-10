//! Signing in with an Atmosphere account, as far as it goes without one: which account is asked
//! for, a server that isn't there, sign-ins that were never started, and the client metadata
//! document to host. The session is kept in files, never the keychain, and the only server tried
//! is a closed port on 127.0.0.1.

use locallibrary::Error;
use locallibrary::api::{self, AccountLoginRequest};
use locallibrary::progress::Ctx;

/// Without a handle, and none signed in with before, it asks which account.
#[test]
fn signing_in_asks_which_account() {
    let dir = tempfile::tempdir().unwrap();
    let req = AccountLoginRequest { handle: Some("  ".into()), session_file: Some(true), ..Default::default() };
    let err = api::account_login_begin(dir.path(), &req).unwrap_err();
    assert!(err.to_string().starts_with("which Atmosphere account?"), "{err}");
    let status = api::account_status(dir.path()).unwrap();
    assert!(status.account.is_none() && status.last_handle.is_none());
}

/// A server that doesn't answer fails the sign-in, and nothing is signed in.
#[test]
fn a_server_that_isnt_there_fails_the_sign_in() {
    let dir = tempfile::tempdir().unwrap();
    let req = AccountLoginRequest {
        handle: Some("http://127.0.0.1:1".into()),
        dev_client: true,
        session_file: Some(true),
        publish: true,
        scopes: vec!["atproto".into()],
        ..Default::default()
    };
    let err = api::account_login_begin(dir.path(), &req).unwrap_err();
    assert!(matches!(&err, Error::Account(m) if m.starts_with("signing in: ")), "{err}");
    assert!(api::account_status(dir.path()).unwrap().account.is_none());
}

/// A sign-in that was never started can't be finished, by waiting or by a pasted address, nor
/// cancelled.
#[test]
fn a_sign_in_never_started_cant_be_finished() {
    let never_started =
        |e: Error| assert!(matches!(&e, Error::NotFound(m) if m.starts_with("sign-in \"nope\"")), "{e}");
    let finish = api::AccountLoginFinish { id: "nope".into(), timeout_secs: Some(0) };
    never_started(api::account_login_finish(&finish, &Ctx::silent()).unwrap_err());
    let paste = api::AccountLoginPaste { id: "nope".into(), url: "http://127.0.0.1/callback?code=c&state=s".into() };
    never_started(api::account_login_paste(&paste).unwrap_err());
    assert!(!api::account_login_cancel(&api::AccountLoginCancel { id: "nope".into() }));
}

/// The client metadata document is for an https client ID, and allows the scopes asked for
/// besides the ones every document allows.
#[test]
fn the_client_metadata_document_is_for_an_https_client() {
    let req = |client_id: &str| api::ClientMetadataRequest {
        client_id: client_id.into(),
        scopes: vec!["transition:generic".into()],
    };
    let err = api::account_client_metadata(&req("http://example.com/client.json")).unwrap_err();
    assert!(err.to_string().contains("https://"), "{err}");
    let doc = api::account_client_metadata(&req("https://example.com/client.json")).unwrap();
    assert_eq!(doc["client_id"], "https://example.com/client.json");
    let scope = doc["scope"].as_str().unwrap();
    assert!(scope.contains("atproto") && scope.contains("transition:generic"), "{scope}");
    let default: api::ClientMetadataRequest = serde_json::from_value(serde_json::json!({})).unwrap();
    assert!(default.client_id.starts_with("https://"), "{}", default.client_id);
}
