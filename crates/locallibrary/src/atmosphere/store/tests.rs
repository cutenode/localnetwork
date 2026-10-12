use super::*;
use crate::atmosphere::block_on;

#[test]
fn secrets_split_off_and_join_back() {
    let dir = tempfile::tempdir().unwrap();
    let mut secrets = Map::new();
    secrets.insert("dpop_key".into(), serde_json::json!({ "kty": "EC", "d": "secret" }));
    secrets.insert("refresh_token".into(), Value::String("ref-1".into()));
    write_secrets(dir.path(), Storage::File, "did:plc:abc", &secrets).unwrap();
    assert_eq!(read_secrets(dir.path(), Storage::File, "did:plc:abc").unwrap(), Some(secrets.clone()));
    // Another account's file isn't applied.
    assert_eq!(read_secrets(dir.path(), Storage::File, "did:plc:other").unwrap(), None);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(dir.path().join(SECRETS_FILE)).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }
    delete_secrets(dir.path(), Storage::File, "did:plc:abc").unwrap();
    assert_eq!(read_secrets(dir.path(), Storage::File, "did:plc:abc").unwrap(), None);
    delete_secrets(dir.path(), Storage::File, "did:plc:abc").unwrap();
}

#[test]
fn each_accounts_secrets_have_their_own_place_in_the_file() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let secrets = |refresh: &str| Map::from_iter([("refresh_token".to_string(), Value::from(refresh))]);
    write_secrets(root, Storage::File, "did:plc:a", &secrets("ref-a")).unwrap();
    write_secrets(root, Storage::File, "did:plc:b", &secrets("ref-b")).unwrap();
    delete_secrets(root, Storage::File, "did:plc:a").unwrap();
    assert_eq!(read_secrets(root, Storage::File, "did:plc:a").unwrap(), None);
    assert_eq!(read_secrets(root, Storage::File, "did:plc:b").unwrap(), Some(secrets("ref-b")));
    delete_secrets(root, Storage::File, "did:plc:b").unwrap();
    assert!(!root.join(SECRETS_FILE).exists(), "gone with the last account's");

    // A file from before: one account's secrets, naming it.
    let old = serde_json::json!({ "did": "did:plc:a", "refresh_token": "ref-a" });
    std::fs::write(root.join(SECRETS_FILE), old.to_string()).unwrap();
    assert_eq!(read_secrets(root, Storage::File, "did:plc:a").unwrap(), Some(secrets("ref-a")));
    assert_eq!(read_secrets(root, Storage::File, "did:plc:b").unwrap(), None);
    delete_secrets(root, Storage::File, "did:plc:b").unwrap();
    assert_eq!(read_secrets(root, Storage::File, "did:plc:a").unwrap(), Some(secrets("ref-a")));
    // A file that can't be read is replaced by the next sign-in.
    std::fs::write(root.join(SECRETS_FILE), "{ not json").unwrap();
    write_secrets(root, Storage::File, "did:plc:b", &secrets("ref-b")).unwrap();
    assert_eq!(read_secrets(root, Storage::File, "did:plc:b").unwrap(), Some(secrets("ref-b")));
}

#[test]
fn a_session_that_cant_be_read_is_said_without_quoting_it() {
    let dir = tempfile::tempdir().unwrap();
    // A secret of the wrong shape (a hand-edited file, or another version's).
    let mut secrets = Map::new();
    secrets.insert("dpop_key".into(), Value::from("super-secret-key"));
    secrets.insert("refresh_token".into(), Value::from("super-secret-token"));
    let public = serde_json::json!({ "account_did": "did:plc:abc" });
    let err = join(&public, secrets).unwrap_err().to_string();
    assert!(!err.contains("super-secret"), "{err}");
    assert!(err.contains("can't be used") && err.contains("sign in again"), "{err}");
    let text = r#"{"did:plc:abc": {"refresh_token": "super-secret-token", }"#;
    std::fs::write(dir.path().join(SECRETS_FILE), text).unwrap();
    let err = read_secrets(dir.path(), Storage::File, "did:plc:abc").unwrap_err().to_string();
    assert!(!err.contains("super-secret") && err.contains("it isn't JSON, at line 1"), "{err}");
    // Not an object: said the same way wherever it's found.
    let err = join(&Value::Null, Map::new()).unwrap_err().to_string();
    assert_eq!(err, "the saved session isn't a JSON object; sign in again");
}

/// Regression: the session files were written through a fixed `<name>.tmp`, which a write
/// cut short left behind and nothing tidied. Their temp files are the store's own now, and
/// one that wasn't renamed is removed.
#[test]
fn a_session_write_that_fails_leaves_no_temp_file() {
    let dir = tempfile::tempdir().unwrap();
    // A folder where the file goes: the rename fails.
    std::fs::create_dir_all(dir.path().join(ACCOUNT_FILE).join("in the way")).unwrap();
    assert!(AccountFile::default().save(dir.path()).is_err());
    let names: Vec<String> =
        std::fs::read_dir(dir.path()).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into()).collect();
    assert_eq!(names, [ACCOUNT_FILE]);
}

#[test]
fn without_a_keychain_the_way_on_is_the_session_file() {
    let err = no_keychain("locked");
    assert_eq!(err.reason(), Some(Reason::NoKeychain));
    // A switch, turned on: `null` would mean "the default", the keychain again.
    assert_eq!(err.detail().actions[0].params["session_file"], true);
}

#[test]
fn each_store_has_its_own_keychain_item() {
    let (one, two) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    crate::store::Store::open_or_init(one.path()).unwrap();
    crate::store::Store::open_or_init(two.path()).unwrap();
    let item = |root: &Path| keychain_account(root, "did:plc:abc").unwrap();
    assert!(item(one.path()).starts_with("atmosphere:did:plc:abc@"));
    assert_ne!(item(one.path()), item(two.path()));
    assert_eq!(item(one.path()), item(one.path()));
}

/// What jacquard asks the store for: only the signed-in account's own session, with its
/// secrets, and the sign-ins in flight in this process.
#[test]
fn jacquard_gets_only_the_signed_in_session() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let store = LocalStore::new(root, Storage::File, Client::Loopback, "http://127.0.0.1:1/callback".into(), vec![]);
    let did: Did = Did::new_owned("did:plc:abc").unwrap();
    let get = |session_id: &str| block_on(store.get_session(&did, session_id)).unwrap();
    assert!(block_on(store.list_session_keys()).unwrap().is_empty());
    block_on(store.upsert_session(crate::atmosphere::tests::session("did:plc:abc", "refresh-1"))).unwrap();
    let keys = block_on(store.list_session_keys()).unwrap();
    assert_eq!(keys.len(), 1);
    assert!(get("s1").is_some() && get("s2").is_none());

    // Another session's removal leaves this one; its own removes it, secrets and all.
    block_on(store.delete_session(&did, "s2")).unwrap();
    assert!(get("s1").is_some());
    block_on(store.delete_session(&did, "s1")).unwrap();
    assert!(AccountFile::load(root).unwrap().account.is_none() && !root.join(SECRETS_FILE).exists());

    // Without its secrets a session is no session.
    block_on(store.upsert_session(crate::atmosphere::tests::session("did:plc:abc", "refresh-1"))).unwrap();
    std::fs::remove_file(root.join(SECRETS_FILE)).unwrap();
    assert!(get("s1").is_none());
}

/// A session file that's been damaged (a folder in its place, a list, an account's secrets
/// that aren't an object) is said to be, not read as no session at all.
#[test]
fn a_damaged_session_file_is_an_error() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let read = || read_secrets(root, Storage::File, "did:plc:abc").unwrap_err().to_string();
    std::fs::write(root.join(SECRETS_FILE), "[]").unwrap();
    assert_eq!(read(), "the saved session isn't a JSON object; sign in again");
    std::fs::write(root.join(SECRETS_FILE), r#"{"did:plc:abc": "a token"}"#).unwrap();
    assert_eq!(read(), "the saved session isn't a JSON object; sign in again");
    std::fs::remove_file(root.join(SECRETS_FILE)).unwrap();
    std::fs::create_dir(root.join(SECRETS_FILE)).unwrap();
    assert!(read().contains(SECRETS_FILE));
    // Through jacquard, the same.
    let store = LocalStore::new(root, Storage::File, Client::Loopback, "http://127.0.0.1:1/callback".into(), vec![]);
    let session = crate::atmosphere::tests::session("did:plc:abc", "refresh-1");
    assert!(block_on(store.upsert_session(session)).is_err());
}

#[test]
fn the_lock_waits_for_the_holder() {
    let dir = tempfile::tempdir().unwrap();
    let held = lock(dir.path()).unwrap();
    let root = dir.path().to_path_buf();
    let (tx, rx) = std::sync::mpsc::channel();
    let t = std::thread::spawn(move || {
        let _l = lock(&root).unwrap();
        tx.send(()).unwrap();
    });
    assert!(rx.recv_timeout(std::time::Duration::from_millis(200)).is_err(), "second lock waits");
    drop(held);
    rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
    t.join().unwrap();
}

use hegel::TestCase;
use hegel::generators as gs;
use rustc_hash::FxHashSet;

const DIDS: [&str; 2] = ["did:plc:aaaaaaaaaaaaaaaaaaaaaaaa", "did:plc:bbbbbbbbbbbbbbbbbbbbbbbb"];
const SESSION_IDS: [&str; 2] = ["s1", "s2"];
const STATES: [&str; 2] = ["st4te", "other"];

#[derive(Debug, Clone)]
enum Op {
    SignIn {
        did: usize,
        session: usize,
        storage: Storage,
        refresh: u8,
    },
    Get {
        did: usize,
        session: usize,
    },
    Delete {
        did: usize,
        session: usize,
    },
    /// The keychain's next answer for this account's item is an error.
    KeychainStops(usize),
    /// A sign-in starts, waiting for the redirect with this state.
    Start(usize),
    /// It's over.
    Finish(usize),
    /// The account's secrets go missing behind the store's back.
    Lose,
}

#[hegel::test_helper]
fn op(tc: &TestCase) -> Op {
    let did = tc.draw(gs::integers::<usize>().max_value(1));
    let session = tc.draw(gs::integers::<usize>().max_value(1));
    match tc.draw(gs::integers::<u8>().max_value(8)) {
        0..=2 => Op::SignIn {
            did,
            session,
            storage: if tc.draw(gs::booleans()) { Storage::File } else { Storage::Keychain },
            refresh: tc.draw(gs::integers::<u8>().max_value(9)),
        },
        3 => Op::Get { did, session },
        4 => Op::Delete { did, session },
        5 => Op::KeychainStops(did),
        6 => Op::Start(session),
        7 => Op::Finish(session),
        _ => Op::Lose,
    }
}

#[derive(Debug, Clone, PartialEq)]
struct Signed {
    did: usize,
    session: usize,
    storage: Storage,
}

fn auth_request(state: &str) -> AuthRequestData {
    AuthRequestData {
        state: state.into(),
        authserver_url: "https://127.0.0.1:9".into(),
        account_did: None,
        scopes: jacquard_oauth::scopes::Scopes::new(SmolStr::new("atproto")).unwrap(),
        request_uri: "urn:ietf:params:oauth:request_uri:x".into(),
        authserver_token_endpoint: "https://127.0.0.1:9/oauth/token".into(),
        authserver_revocation_endpoint: None,
        pkce_verifier: "verifier".into(),
        dpop_data: jacquard_oauth::session::DpopReqData {
            dpop_key: jacquard_oauth::utils::generate_key(&["ES256"]).unwrap(),
            dpop_authserver_nonce: None,
        },
    }
}

/// What jacquard asks of the store, in any order, with secrets in the session file or the
/// keychain and a keychain that stops answering now and then, agrees with a model: one
/// account at a time, its secrets where it was signed in to keep them and nowhere else, its
/// session read back only by its own DID and session ID, the old account's secrets removed
/// when another signs in (or remembered, when the keychain won't let go), and the sign-ins
/// in flight kept by their state.
#[hegel::test(test_cases = 6)]
fn the_store_agrees_with_a_model_of_one_account(tc: TestCase) {
    crate::atmosphere::tests::mock_keychain();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    crate::store::Store::open_or_init(root).unwrap();
    let store =
        |storage| LocalStore::new(root, storage, Client::Loopback, "http://127.0.0.1:1/callback".into(), vec![]);
    let (in_file, in_keychain) = (store(Storage::File), store(Storage::Keychain));
    let mut account: Option<Signed> = None;
    // Keyed by where (`Storage as u8`) and whose.
    let mut secrets: FxHashMap<(u8, usize), String> = FxHashMap::default();
    let mut left_behind: Vec<(Storage, String)> = Vec::new();
    let mut armed: FxHashSet<usize> = FxHashSet::default();
    let mut pending: FxHashSet<usize> = FxHashSet::default();
    let did_of = |i: usize| Did::<SmolStr>::new_owned(DIDS[i]).unwrap();
    // The model's armed keychain item, if this is one: its next answer is the error.
    let fails =
        |armed: &mut FxHashSet<usize>, storage: Storage, did: usize| storage == Storage::Keychain && armed.remove(&did);
    let mut ops: Vec<Op> = (0..tc.draw(gs::integers::<usize>().max_value(7))).map(|_| op(&tc)).collect();
    // And then, so every run sees them: the keychain won't let go of the old account's
    // secrets when another signs in, nor answer when it's read or signed out of (and then
    // does), and the session file goes missing behind the store's back.
    let (a, b) = (0, 1);
    ops.extend([
        Op::SignIn { did: a, session: 0, storage: Storage::Keychain, refresh: 1 },
        Op::KeychainStops(a),
        Op::SignIn { did: b, session: 0, storage: Storage::File, refresh: 2 },
        Op::Lose,
        Op::Get { did: b, session: 0 },
        Op::Get { did: b, session: 1 },
        Op::SignIn { did: a, session: 1, storage: Storage::Keychain, refresh: 3 },
        Op::KeychainStops(a),
        Op::Get { did: a, session: 1 },
        Op::KeychainStops(a),
        Op::Delete { did: a, session: 1 },
        Op::Delete { did: a, session: 1 },
    ]);
    for op in ops {
        tc.note(&format!("{op:?}"));
        match op {
            Op::SignIn { did, session, storage, refresh } => {
                let mut s = crate::atmosphere::tests::session(DIDS[did], &format!("refresh-{refresh}"));
                s.session_id = SESSION_IDS[session].into();
                let to = if storage == Storage::File { &in_file } else { &in_keychain };
                let put = block_on(to.upsert_session(s));
                if fails(&mut armed, storage, did) {
                    assert!(put.is_err());
                    continue;
                }
                put.unwrap();
                secrets.insert((storage as u8, did), format!("refresh-{refresh}"));
                if let Some(old) = account.take().filter(|a| (a.storage, a.did) != (storage, did)) {
                    if fails(&mut armed, old.storage, old.did) {
                        let place = (old.storage, DIDS[old.did].to_string());
                        if !left_behind.contains(&place) {
                            left_behind.push(place);
                        }
                    } else {
                        secrets.remove(&(old.storage as u8, old.did));
                    }
                }
                account = Some(Signed { did, session, storage });
            }
            Op::Get { did, session } => {
                let got = block_on(in_file.get_session(&did_of(did), SESSION_IDS[session]));
                match account.as_ref().filter(|a| (a.did, a.session) == (did, session)) {
                    Some(a) if fails(&mut armed, a.storage, a.did) => assert!(got.is_err()),
                    Some(a) => {
                        let refresh = got.unwrap().and_then(|s| s.token_set.refresh_token.map(|r| r.to_string()));
                        assert_eq!(refresh.as_ref(), secrets.get(&(a.storage as u8, a.did)), "{a:?}");
                    }
                    None => assert!(got.unwrap().is_none()),
                }
            }
            Op::Delete { did, session } => {
                let deleted = block_on(in_file.delete_session(&did_of(did), SESSION_IDS[session]));
                match account.clone().filter(|a| (a.did, a.session) == (did, session)) {
                    Some(a) if fails(&mut armed, a.storage, a.did) => assert!(deleted.is_err()),
                    Some(a) => {
                        deleted.unwrap();
                        secrets.remove(&(a.storage as u8, a.did));
                        account = None;
                    }
                    None => deleted.unwrap(),
                }
            }
            Op::KeychainStops(did) => {
                crate::atmosphere::tests::keychain_fails(root, DIDS[did]);
                armed.insert(did);
            }
            Op::Start(state) => {
                block_on(in_file.save_auth_req_info(&auth_request(STATES[state]))).unwrap();
                pending.insert(state);
            }
            Op::Finish(state) => {
                block_on(in_file.delete_auth_req_info(STATES[state])).unwrap();
                pending.remove(&state);
            }
            Op::Lose => {
                if let Some(a) = &account {
                    let lost = delete_secrets(root, a.storage, DIDS[a.did]);
                    if fails(&mut armed, a.storage, a.did) {
                        assert!(lost.is_err());
                    } else {
                        lost.unwrap();
                        secrets.remove(&(a.storage as u8, a.did));
                    }
                }
            }
        }

        let file = AccountFile::load(root).unwrap();
        let stored = file.account.as_ref().map(|a| (a.info.did.as_str(), a.session_id.as_str(), a.info.storage));
        let modelled = account.as_ref().map(|a| (DIDS[a.did], SESSION_IDS[a.session], a.storage));
        assert_eq!(stored, modelled);
        assert_eq!(file.left_behind, left_behind);
        let keys = block_on(in_file.list_session_keys()).unwrap();
        let keys: Vec<(String, String)> =
            keys.iter().map(|k| (k.did.as_str().to_string(), k.session_id.to_string())).collect();
        assert_eq!(keys, modelled.iter().map(|(d, s, _)| (d.to_string(), s.to_string())).collect::<Vec<_>>());
        for storage in [Storage::File, Storage::Keychain] {
            for (did, name) in DIDS.iter().enumerate() {
                if storage == Storage::Keychain && armed.contains(&did) {
                    continue;
                }
                let kept = read_secrets(root, storage, name).unwrap();
                let refresh = kept.map(|s| s["refresh_token"].as_str().unwrap().to_string());
                assert_eq!(refresh.as_ref(), secrets.get(&(storage as u8, did)), "{storage:?} {name}");
            }
        }
        assert_eq!(root.join(SECRETS_FILE).exists(), secrets.keys().any(|(s, _)| *s == Storage::File as u8));
        for (i, state) in STATES.iter().enumerate() {
            assert_eq!(in_file.expects(state), pending.contains(&i));
            let info = block_on(in_file.get_auth_req_info(state)).unwrap();
            assert_eq!(info.map(|r| r.state.to_string()), pending.contains(&i).then(|| state.to_string()));
            assert!(!in_keychain.expects(state), "each store's own");
        }
    }
}
