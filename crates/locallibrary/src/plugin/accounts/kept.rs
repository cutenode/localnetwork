//! Where a store keeps its accounts: `accounts.json` in the store's folder, readable only by the
//! user and left out of its copies, has each account but its tokens, and the accesses plugins were
//! given to them, changed under `accounts.lock`. Its tokens are a secret (see [`Place`]): the keychain item
//! `account:<account plugin>:<subject>@<store id>` of `locallibrary`, or with
//! [`Storage::File`] in `secrets/accounts.json`. Each account's tokens are read and refreshed
//! under a lock of their own, in `accounts/`.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::Tokens;
use super::access::Access;
use crate::error::IoContext;
use crate::fsutil::{self, FileLock};
use crate::hash::Hash;
use crate::keychain::Storage;
use crate::plugin::secrets::{Place, set_secret};
use crate::store::Store;
use crate::{Error, Result};

const FILE: &str = "accounts.json";
const LOCK: &str = "accounts.lock";

/// What `accounts.json` keeps: the accounts, and the accesses plugins were given to them, which
/// outlast an account signed out.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub(super) struct Book {
    #[serde(default)]
    pub accounts: Vec<Kept>,
    #[serde(default)]
    pub accesses: Vec<Access>,
}

impl Book {
    /// The account `subject` that `plugin` signed in, if it's kept.
    pub fn account(&self, plugin: &str, subject: &str) -> Option<&Kept> {
        self.accounts.iter().find(|k| k.is(plugin, subject))
    }
}

/// An account as `accounts.json` keeps it: one for each account plugin and subject.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub(super) struct Kept {
    #[serde(rename = "type")]
    pub kind: String,
    /// The account plugin that signed it in, whose client alone refreshes its tokens.
    pub plugin: String,
    /// Its id at the service, as the account type's whoami says it (an integer as its digits).
    pub subject: String,
    pub name: String,
    /// What the service granted, as it names them.
    pub scopes: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<DateTime<Utc>>,
    /// It's refreshed without signing in again.
    pub refreshes: bool,
}

impl Kept {
    /// Whether it's the account `subject` that `plugin` signed in.
    pub fn is(&self, plugin: &str, subject: &str) -> bool {
        self.plugin == plugin && self.subject == subject
    }

    /// What `tokens` say of it: what the service granted, when they expire and whether they
    /// refresh.
    pub fn granted(&mut self, tokens: &Tokens) {
        self.scopes = tokens.scopes.clone();
        self.expires_at = tokens.expires_at;
        self.refreshes = tokens.refresh_token.is_some();
    }
}

/// The accounts `store` keeps, and the accesses to them.
pub(super) fn read(store: &Store) -> Result<Book> {
    Ok(fsutil::read_json_opt(&store.root().join(FILE))?.unwrap_or_default())
}

/// Changes the accounts `store` keeps and the accesses to them: `f` gets them all, and what it
/// leaves is written back (when it changed anything), read and written under `accounts.lock` as
/// [`Store::update_drives`] does, so `f` can keep their tokens too.
pub(super) fn update<T>(store: &Store, f: impl FnOnce(&mut Book) -> Result<T>) -> Result<T> {
    let _lock = FileLock::wait(&store.root().join(LOCK))?;
    let mut all = read(store)?;
    let before = all.clone();
    let out = f(&mut all)?;
    if all != before {
        let mut bytes = serde_json::to_vec_pretty(&all).map_err(|e| Error::invalid(e.to_string()))?;
        bytes.push(b'\n');
        fsutil::write_private(&store.root().join(FILE), &bytes)?;
    }
    Ok(out)
}

/// Where the tokens of the account `subject` that `plugin` signed in are kept.
pub(super) fn place<'a>(store: &'a Store, plugin: &str, subject: &str) -> Place<'a> {
    Place {
        store,
        service: "locallibrary".into(),
        file: store.root().join("secrets").join(FILE),
        key: format!("account:{plugin}:{subject}"),
        moved_from: None,
    }
}

/// Keeps `tokens` as those of the account `kept` (only the access and refresh tokens: the rest is
/// the account's), or with `None` forgets them.
pub(super) fn keep(storage: Storage, store: &Store, kept: &Kept, tokens: Option<&Tokens>) -> Result<()> {
    let json = tokens
        .map(|t| {
            let only = Tokens {
                access_token: t.access_token.clone(),
                refresh_token: t.refresh_token.clone(),
                expires_at: None,
                scopes: Vec::new(),
            };
            serde_json::to_string(&only).map_err(|e| Error::invalid(e.to_string()))
        })
        .transpose()?;
    set_secret(storage, &place(store, &kept.plugin, &kept.subject), json.as_deref())
}

/// Held while the tokens of the account `subject` that `plugin` signed in are read, refreshed
/// and kept, so two hosts on one store spend a refresh token once. Its file is named for a hash
/// of the subject, which may have anything in it.
pub(super) fn hold(store: &Store, plugin: &str, subject: &str) -> Result<FileLock> {
    let dir = store.root().join("accounts");
    std::fs::create_dir_all(&dir).at(&dir)?;
    FileLock::wait(&dir.join(format!("{plugin}-{}.lock", Hash::of(subject.as_bytes()).to_hex())))
}
