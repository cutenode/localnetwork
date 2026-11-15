//! Restoring access: signing out of an account, or its session ending, removes every plugin's
//! access to it (see [`Book::remember`]), and the store remembers each on this computer (in
//! `accounts.json`, which no copy of the store has) until it's restored, chosen from or reset once
//! the account is signed in again. Automatically Restore Access, for one account or all, restores
//! it as the account is signed in, without asking.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::access::{Access, AccountId};
use super::kept::{self, Book};
use super::shared::not_kept;
use crate::error::Reason;
use crate::plugin::Plugins;
use crate::store::Store;
use crate::{Error, Result};

/// What signing in to an account does with the access plugins had to it before it was signed
/// out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Restoring {
    /// Every plugin gets its access back, as it was.
    Restore,
    /// No plugin does, and what was remembered is forgotten.
    Reset,
}

/// One plugin's access chosen to be restored, with the scopes (by name) it gets back.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Chosen {
    pub plugin: String,
    /// The account type's scopes, by name: those the plugin needs are given whatever's chosen.
    #[serde(default)]
    pub scopes: Vec<String>,
}

/// Automatically Restore Access, on or off: for one account, or (`account` none) for all those
/// without a setting of their own.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct AutoRestore {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account: Option<AccountId>,
    pub on: bool,
}

impl Book {
    /// Removes every plugin's access to `account`, remembering each in place of any remembered
    /// before for the same plugin. Its default for the type isn't moved to another account.
    pub(super) fn remember(&mut self, account: &AccountId) {
        let (gone, left): (Vec<Access>, Vec<Access>) =
            std::mem::take(&mut self.accesses).into_iter().partition(|a| a.account == *account);
        self.accesses = left;
        self.remembered.retain(|r| !gone.iter().any(|g| g.plugin == r.plugin && g.account == r.account));
        self.remembered.extend(gone);
    }

    /// Forgets what's remembered of the access plugins had to `account`, those `which` picks:
    /// what was.
    fn recall(&mut self, account: &AccountId, which: impl Fn(&Access) -> bool) -> Vec<Access> {
        let (theirs, left): (Vec<Access>, Vec<Access>) =
            std::mem::take(&mut self.remembered).into_iter().partition(|a| a.account == *account && which(a));
        self.remembered = left;
        theirs
    }

    /// Gives a plugin back the access `a` was, joined to the one it has to the account when it was
    /// given one since: its default for the type when it was, or when it has no access of the
    /// type, and has no default since.
    fn put_back(&mut self, a: Access) -> Access {
        if let Some(had) = self.accesses.iter_mut().find(|x| x.plugin == a.plugin && x.account == a.account) {
            had.more(&a.scopes, &a.declined);
            return had.clone();
        }
        let of_kind = || self.accesses.iter().filter(|x| x.plugin == a.plugin && x.kind == a.kind);
        let default = !of_kind().any(|x| x.default) && (a.default || of_kind().next().is_none());
        let id = match self.accesses.iter().any(|x| x.id == a.id) {
            true => super::access::new_id(&self.accesses),
            false => a.id.clone(),
        };
        self.accesses.push(Access { id, default, ..a });
        self.accesses.last().cloned().unwrap_or_else(|| unreachable!("just pushed"))
    }

    /// Restores every plugin's access to `account` that's remembered: what each is now.
    pub(super) fn restore(&mut self, account: &AccountId) -> Vec<Access> {
        self.recall(account, |_| true).into_iter().map(|a| self.put_back(a)).collect()
    }

    /// Restores `plugin`'s access to `account` when it's remembered, leaving the others'.
    pub(super) fn restore_one(&mut self, account: &AccountId, plugin: &str) -> Option<Access> {
        let a = self.recall(account, |a| a.plugin == plugin).into_iter().next()?;
        Some(self.put_back(a))
    }

    /// Why `account` can't be given to a plugin: it's signed out, when plugins had access to it,
    /// else it isn't kept.
    pub(super) fn unkept(&self, account: &AccountId) -> Error {
        let Some(a) = self.remembered.iter().find(|a| a.account == *account) else {
            return not_kept(&account.plugin, &account.subject);
        };
        let why = format!("the {} account {} is signed out: sign in to it first", a.kind, a.name);
        Error::Account(why).because(Reason::NotSignedIn)
    }

    /// Whether `account` is restored without asking: its own setting, else the one for all, else
    /// not.
    pub(super) fn restores_automatically(&self, account: &AccountId) -> bool {
        let setting = |of: Option<&AccountId>| self.restore_automatically.iter().find(|s| s.account.as_ref() == of);
        setting(Some(account)).or_else(|| setting(None)).is_some_and(|s| s.on)
    }

    /// What signing in to `account` does with what's remembered of it, as `restoring` says, else
    /// as Automatically Restore Access does, else nothing: the accesses restored.
    pub(super) fn signed_in_again(&mut self, account: &AccountId, restoring: Option<Restoring>) -> Vec<Access> {
        match restoring {
            Some(Restoring::Reset) => drop(self.recall(account, |_| true)),
            Some(Restoring::Restore) => return self.restore(account),
            None if self.restores_automatically(account) => return self.restore(account),
            None => {}
        }
        Vec::new()
    }
}

impl Plugins {
    /// Restores the access plugins had to `account` before it was signed out: every one, or with
    /// `chosen`, those it names, with the scopes it names, the rest forgotten (none chosen: all
    /// forgotten). One whose plugin isn't installed is forgotten. Only an account signed in gets
    /// any back; one signed out may only be reset. What's restored, as it is now.
    pub fn restore_access(&self, account: &AccountId, chosen: Option<&[Chosen]>, store: &Store) -> Result<Vec<Access>> {
        kept::update(store, |book| {
            let resetting = chosen.is_some_and(<[Chosen]>::is_empty);
            if !resetting && book.account(&account.plugin, &account.subject).is_none() {
                return Err(book.unkept(account));
            }
            let mut restored = Vec::new();
            for mut a in book.recall(account, |_| true) {
                let Ok(slot) = self.slot(&a.plugin) else { continue };
                if let Some(chosen) = chosen {
                    let Some(c) = chosen.iter().find(|c| c.plugin == a.plugin) else { continue };
                    a.scopes = slot.scopes_given(&a.kind, &c.scopes)?;
                    a.declined.retain(|d| !a.scopes.contains(d));
                }
                restored.push(book.put_back(a));
            }
            Ok(restored)
        })
    }

    /// Turns Automatically Restore Access on or off for `account`, or with none for every account
    /// without a setting of its own.
    pub fn restore_automatically(&self, account: Option<&AccountId>, on: bool, store: &Store) -> Result<AutoRestore> {
        kept::update(store, |book| {
            let setting = AutoRestore { account: account.cloned(), on };
            book.restore_automatically.retain(|s| s.account.as_ref() != account);
            book.restore_automatically.push(setting.clone());
            Ok(setting)
        })
    }
}
