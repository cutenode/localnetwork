//! Where a signed-in session lives.
//!
//! `atmosphere.json` in the store folder holds the account and the session without its secrets.
//! The secrets (the DPoP private key that every token is bound to, and the single-use refresh
//! token) go to the OS keychain, one item per store folder and account, or with
//! [`Storage::File`] to `atmosphere-session.json`, under the account's DID. Both files are
//! readable only by the user. Splitting keeps the keychain item small: Windows' Credential
//! Manager holds at most 2,560 bytes, and a whole session can be more.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use chrono::{DateTime, Utc};
use jacquard_common::SmolStr;
use jacquard_common::bos::BosStr;
use jacquard_common::session::{SessionKey, SessionStoreError};
use jacquard_common::types::did::Did;
use jacquard_oauth::authstore::ClientAuthStore;
use jacquard_oauth::session::{AuthRequestData, ClientSessionData};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use super::{AccountInfo, Client, SESSION_LIMIT_DAYS, Storage};
use crate::error::IoContext;
use crate::error::{Action, Reason};
use crate::{Error, Result};

pub const ACCOUNT_FILE: &str = "atmosphere.json";
pub const SECRETS_FILE: &str = "atmosphere-session.json";
const LOCK_FILE: &str = "atmosphere.lock";
pub(super) const KEYCHAIN_SERVICE: &str = "locallibrary";

/// Session fields that stay out of `atmosphere.json`.
const SECRET_FIELDS: [&str; 2] = ["dpop_key", "refresh_token"];

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct AccountFile {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account: Option<StoredAccount>,
    /// The handle last signed in with, offered next time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_handle: Option<String>,
    /// Secrets that couldn't be removed (the keychain didn't answer) when a sign-in replaced
    /// them or signing out removed them, by where they are and whose: signing out tries again.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub left_behind: Vec<(Storage, String)>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct StoredAccount {
    pub info: AccountInfo,
    pub session_id: String,
    /// The exact redirect URI signed in with: a loopback client's ID is built from it, and
    /// refreshing needs the same client ID.
    pub redirect_uri: String,
    /// The scopes asked for (a loopback client's ID is built from them too).
    #[serde(default)]
    pub requested_scopes: Vec<String>,
    /// The session without its secret fields (`dpop_key` and `refresh_token`, stored apart).
    pub session: Value,
}

impl AccountFile {
    pub fn load(root: &Path) -> Result<AccountFile> {
        Ok(crate::fsutil::read_json_opt(&root.join(ACCOUNT_FILE))?.unwrap_or_default())
    }

    pub fn save(&self, root: &Path) -> Result<()> {
        write_private(&root.join(ACCOUNT_FILE), self)
    }

    /// Remember `did`'s secrets in `storage`, which couldn't be removed, to try again.
    pub(super) fn leave_behind(&mut self, storage: Storage, did: &str) {
        let place = (storage, did.to_string());
        if !self.left_behind.contains(&place) {
            self.left_behind.push(place);
        }
    }
}

/// Write `value` as JSON to a file only its owner can read, atomically.
fn write_private(path: &Path, value: &impl Serialize) -> Result<()> {
    let mut bytes = serde_json::to_vec_pretty(value).map_err(|e| Error::invalid(e.to_string()))?;
    bytes.push(b'\n');
    crate::fsutil::write_private(path, &bytes)
}

/// Held while a session is read, refreshed and written back, so two processes (the CLI and a
/// running sidecar) don't both spend the single-use refresh token.
pub struct AccountLock {
    _lock: crate::fsutil::FileLock,
}

pub fn lock(root: &Path) -> Result<AccountLock> {
    std::fs::create_dir_all(root).at(root)?;
    Ok(AccountLock { _lock: crate::fsutil::FileLock::wait(&root.join(LOCK_FILE))? })
}

/// The keychain item for `did`'s secrets: one per store folder, so two stores signed in to the
/// same account don't share (and spend) one refresh token.
pub(super) fn keychain_account(root: &Path, did: &str) -> Result<String> {
    Ok(format!("atmosphere:{did}@{}", crate::store::Store::open(root)?.id()?))
}

fn keychain_entry(root: &Path, did: &str) -> Result<keyring::Entry> {
    keyring::Entry::new(KEYCHAIN_SERVICE, &keychain_account(root, did)?).map_err(keychain_error)
}

/// Remove `did`'s secrets from the one keychain item every store folder shared before each had
/// its own. Nothing reads that item any more.
fn delete_old_keychain_item(did: &str) -> Result<()> {
    delete_entry(keyring::Entry::new(KEYCHAIN_SERVICE, &format!("atmosphere:{did}")).map_err(keychain_error)?)
}

fn delete_entry(entry: keyring::Entry) -> Result<()> {
    match entry.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(keychain_error(e)),
    }
}

fn keychain_error(e: keyring::Error) -> Error {
    no_keychain(e)
}

/// The keychain can't keep the session: a file only you can read can.
pub(crate) fn no_keychain(e: impl std::fmt::Display) -> Error {
    Error::Account(format!(
        "the system keychain isn't available ({e}); the session can be kept in a file only you can read instead"
    ))
    .because(Reason::NoKeychain)
    .action(Action::retry(serde_json::json!({ "session_file": true }), "Keep the session in a file"))
}

/// A saved session that isn't a JSON object.
fn not_an_object() -> Error {
    Error::Account("the saved session isn't a JSON object; sign in again".into())
}

/// A saved session this version can't read. serde's own message can quote a value, which may be
/// a secret, so only what kind of problem it is and where are said.
fn unreadable(e: serde_json::Error) -> Error {
    use serde_json::error::Category;
    let what = match e.classify() {
        Category::Io => "it couldn't be read",
        Category::Syntax | Category::Eof => "it isn't JSON",
        Category::Data => "its fields aren't what this version expects",
    };
    let at = if e.line() > 0 { format!(", at line {} column {}", e.line(), e.column()) } else { String::new() };
    Error::Account(format!("the saved session can't be used ({what}{at}); sign in again"))
}

/// The session file's secrets, by account. A file from before it held more than one account
/// holds one account's secrets with its `did` among them.
fn secrets_file(root: &Path) -> Result<Map<String, Value>> {
    let path = root.join(SECRETS_FILE);
    let text = match std::fs::read_to_string(&path) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Map::new()),
        Err(e) => return Err(Error::Io { path, source: e }),
    };
    let Value::Object(mut all) = serde_json::from_str(&text).map_err(unreadable)? else {
        return Err(not_an_object());
    };
    if let Some(Value::String(did)) = all.remove("did") {
        return Ok(Map::from_iter([(did, Value::Object(all))]));
    }
    Ok(all)
}

/// Read the secrets for `did`, if there are any.
pub fn read_secrets(root: &Path, storage: Storage, did: &str) -> Result<Option<Map<String, Value>>> {
    let value = match storage {
        Storage::Keychain => match keychain_entry(root, did)?.get_password() {
            Ok(s) => serde_json::from_str(&s).map_err(unreadable)?,
            Err(keyring::Error::NoEntry) => return Ok(None),
            Err(e) => return Err(keychain_error(e)),
        },
        Storage::File => match secrets_file(root)?.remove(did) {
            Some(v) => v,
            None => return Ok(None),
        },
    };
    match value {
        Value::Object(m) => Ok(Some(m)),
        _ => Err(not_an_object()),
    }
}

pub fn write_secrets(root: &Path, storage: Storage, did: &str, secrets: &Map<String, Value>) -> Result<()> {
    match storage {
        Storage::Keychain => {
            let text = serde_json::to_string(secrets).map_err(|e| Error::invalid(e.to_string()))?;
            keychain_entry(root, did)?.set_password(&text).map_err(keychain_error)
        }
        Storage::File => {
            // A file that can't be read has nothing usable in it: signing in replaces it.
            let mut all = secrets_file(root).unwrap_or_default();
            all.insert(did.to_string(), Value::Object(secrets.clone()));
            write_secrets_file(root, &all)
        }
    }
}

pub fn delete_secrets(root: &Path, storage: Storage, did: &str) -> Result<()> {
    match storage {
        Storage::Keychain => {
            delete_entry(keychain_entry(root, did)?)?;
            delete_old_keychain_item(did)
        }
        Storage::File => {
            let mut all = secrets_file(root).unwrap_or_default();
            all.remove(did);
            write_secrets_file(root, &all)
        }
    }
}

/// Write the session file, or remove it once it holds nobody's secrets.
fn write_secrets_file(root: &Path, all: &Map<String, Value>) -> Result<()> {
    let path = root.join(SECRETS_FILE);
    match all.is_empty() {
        false => write_private(&path, all),
        true => crate::fsutil::remove_if_exists(&path).map(|_| ()),
    }
}

/// A session as JSON, split into what can be written to `atmosphere.json` and the secrets.
pub fn split(session: &ClientSessionData) -> Result<(Value, Map<String, Value>)> {
    let Value::Object(mut all) = serde_json::to_value(session).map_err(|e| Error::invalid(e.to_string()))? else {
        return Err(Error::invalid("session didn't serialise to an object"));
    };
    let secrets: Map<String, Value> =
        SECRET_FIELDS.iter().filter_map(|k| all.remove(*k).map(|v| (k.to_string(), v))).collect();
    Ok((Value::Object(all), secrets))
}

/// The two halves back together.
pub fn join(public: &Value, secrets: Map<String, Value>) -> Result<ClientSessionData> {
    let Value::Object(mut all) = public.clone() else {
        return Err(not_an_object());
    };
    all.extend(secrets);
    serde_json::from_value(Value::Object(all)).map_err(unreadable)
}

fn store_error(e: Error) -> SessionStoreError {
    SessionStoreError::Other(Box::new(e))
}

/// The [`ClientAuthStore`] the OAuth client reads and writes sessions through.
pub struct LocalStore {
    pub root: PathBuf,
    pub storage: Storage,
    /// What a session created through this store was signed in with.
    pub client: Client,
    pub redirect_uri: String,
    pub requested_scopes: Vec<String>,
    /// Authorization requests in flight. They only have to outlive one sign-in, in this process.
    pending: Mutex<HashMap<String, AuthRequestData>>,
}

impl LocalStore {
    pub fn new(
        root: &Path,
        storage: Storage,
        client: Client,
        redirect_uri: String,
        requested_scopes: Vec<String>,
    ) -> LocalStore {
        LocalStore {
            root: root.to_path_buf(),
            storage,
            client,
            redirect_uri,
            requested_scopes,
            pending: Mutex::new(HashMap::new()),
        }
    }

    /// Whether `state` is a sign-in in flight here: the only redirect worth acting on.
    pub fn expects(&self, state: &str) -> bool {
        self.pending.lock().unwrap().contains_key(state)
    }

    fn get(&self, did: &str, session_id: &str) -> Result<Option<ClientSessionData>> {
        let file = AccountFile::load(&self.root)?;
        let Some(account) = file.account.filter(|a| a.info.did == did && a.session_id == session_id) else {
            return Ok(None);
        };
        let Some(secrets) = read_secrets(&self.root, account.info.storage, did)? else {
            return Ok(None);
        };
        join(&account.session, secrets).map(Some)
    }

    fn put(&self, session: &ClientSessionData) -> Result<()> {
        let did = session.account_did.as_str().to_string();
        let (public, secrets) = split(session)?;
        write_secrets(&self.root, self.storage, &did, &secrets)?;
        let mut file = AccountFile::load(&self.root)?;
        let expires = session.token_set.expires_at.as_ref().and_then(|d| d.as_str().parse::<DateTime<Utc>>().ok());
        let scopes: Vec<String> = session.scopes.iter().map(|s| s.to_string()).collect();
        let previous = file.account.take();
        // Signing in replaces whoever was signed in, and wherever their secrets were kept: those
        // go too (writing the new ones already replaced them in the same place). The new session
        // stands even when the keychain won't let go of the old one; signing out tries again.
        if let Some(old) = previous.as_ref().filter(|a| (a.info.storage, a.info.did.as_str()) != (self.storage, &did))
            && delete_secrets(&self.root, old.info.storage, &old.info.did).is_err()
        {
            file.leave_behind(old.info.storage, &old.info.did);
        }
        let account = match previous.filter(|a| a.info.did == did && a.session_id == session.session_id.as_str()) {
            Some(mut a) => {
                a.info.access_expires_at = expires;
                a.info.scopes = scopes;
                a.session = public;
                a
            }
            None => {
                // A new session in the keychain: the item an earlier version kept this account's
                // secrets in goes too.
                if self.storage == Storage::Keychain && delete_old_keychain_item(&did).is_err() {
                    file.leave_behind(Storage::Keychain, &did);
                }
                let now = Utc::now();
                StoredAccount {
                    info: AccountInfo {
                        did: did.clone(),
                        handle: None,
                        pds: session.host_url.as_str().trim_end_matches('/').to_string(),
                        auth_server: session.authserver_url.as_str().trim_end_matches('/').to_string(),
                        scopes,
                        signed_in_at: now,
                        sign_in_again_by: now + chrono::Duration::days(SESSION_LIMIT_DAYS),
                        access_expires_at: expires,
                        client: self.client.clone(),
                        storage: self.storage,
                    },
                    session_id: session.session_id.as_str().to_string(),
                    redirect_uri: self.redirect_uri.clone(),
                    requested_scopes: self.requested_scopes.clone(),
                    session: public,
                }
            }
        };
        file.account = Some(account);
        file.save(&self.root)
    }

    fn remove(&self, did: &str, session_id: &str) -> Result<()> {
        let mut file = AccountFile::load(&self.root)?;
        if let Some(a) = file.account.as_ref().filter(|a| a.info.did == did && a.session_id == session_id) {
            delete_secrets(&self.root, a.info.storage, did)?;
            file.account = None;
            file.save(&self.root)?;
        }
        Ok(())
    }
}

impl ClientAuthStore for LocalStore {
    async fn get_session<D: BosStr + Send + Sync>(
        &self,
        did: &Did<D>,
        session_id: &str,
    ) -> std::result::Result<Option<ClientSessionData>, SessionStoreError> {
        self.get(did.as_ref(), session_id).map_err(store_error)
    }

    async fn upsert_session(&self, session: ClientSessionData) -> std::result::Result<(), SessionStoreError> {
        self.put(&session).map_err(store_error)
    }

    async fn delete_session<D: BosStr + Send + Sync>(
        &self,
        did: &Did<D>,
        session_id: &str,
    ) -> std::result::Result<(), SessionStoreError> {
        self.remove(did.as_ref(), session_id).map_err(store_error)
    }

    async fn get_auth_req_info(&self, state: &str) -> std::result::Result<Option<AuthRequestData>, SessionStoreError> {
        Ok(self.pending.lock().unwrap().get(state).cloned())
    }

    async fn save_auth_req_info(&self, info: &AuthRequestData) -> std::result::Result<(), SessionStoreError> {
        self.pending.lock().unwrap().insert(info.state.as_str().to_string(), info.clone());
        Ok(())
    }

    async fn delete_auth_req_info(&self, state: &str) -> std::result::Result<(), SessionStoreError> {
        self.pending.lock().unwrap().remove(state);
        Ok(())
    }

    async fn list_session_keys(&self) -> std::result::Result<Vec<SessionKey>, SessionStoreError> {
        let file = AccountFile::load(&self.root).map_err(store_error)?;
        Ok(file
            .account
            .and_then(|a| Some(SessionKey::new(Did::<SmolStr>::new_owned(a.info.did).ok()?, a.session_id)))
            .into_iter()
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
