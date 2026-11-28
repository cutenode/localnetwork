//! The player's keys and the account: publishing `keybinds.json` as the account's keybinds record,
//! reading someone's (yours, on another computer) from Local Network's directory, and what to do
//! with yours right after signing in. Each makes network requests, so the player runs them on a
//! worker thread.

use std::path::Path;

use chrono::Utc;

use crate::atmosphere::directory::{self, Fetched};
use crate::atmosphere::repo::{self, Resolved};
use crate::atmosphere::{self, lexicon};
use crate::keybinds::{self, Keybinds};
use crate::publish::{self, Published};
use crate::{Error, Result};

/// Someone's published keys.
#[derive(Debug, Clone, PartialEq)]
pub struct KeybindsFetched {
    pub from: Resolved,
    pub uri: String,
    pub theirs: Keybinds,
    /// What the record holds that this version can't read, left out (see
    /// [`Keybinds::from_record`]).
    pub skipped: Vec<String>,
}

/// What [`keybinds_after_sign_in`] found, and did.
#[derive(Debug, Clone, PartialEq)]
pub enum AfterSignIn {
    /// The account has none published. `published_here`: there were keys set here, so they're
    /// published now.
    NonePublished { published_here: bool },
    /// No keys are set here, so the account's are: written to `keybinds.json`.
    Loaded(Keybinds),
    /// The account's are the ones set here.
    Same,
    /// Keys are set here and the account's differ: which to keep is the user's to choose.
    Differs(KeybindsFetched),
}

/// Publish `keybinds.json` as the signed-in account's keybinds record (written only when it
/// changes what's published, and only over the version Local Network's directory has).
pub fn keybinds_publish(store_root: &Path) -> Result<Published> {
    let record = keybinds::load(store_root)?.to_record(Utc::now());
    let plan = publish::plan(store_root, lexicon::KEYBINDS, "self", record)?;
    publish::apply(store_root, &plan)
}

/// The keybinds record of the account at `did`, as Local Network's directory has it.
fn published(did: &str) -> Result<Option<(Fetched, Keybinds, Vec<String>)>> {
    let Some(fetched) = directory::get_record(did, lexicon::KEYBINDS, "self")? else { return Ok(None) };
    let (theirs, skipped) = Keybinds::from_record(&fetched.value)?;
    Ok(Some((fetched, theirs, skipped)))
}

/// The keys `handle_or_did` published, read through Local Network's directory, never their data
/// server.
pub fn keybinds_fetch(handle_or_did: &str) -> Result<KeybindsFetched> {
    let from = repo::resolve(handle_or_did)?;
    let Some((fetched, theirs, skipped)) = published(&from.did)? else {
        return Err(Error::NotFound(format!(
            "keys published by {} (Local Network's directory doesn't have any)",
            from.name()
        )));
    };
    Ok(KeybindsFetched { from, uri: fetched.uri, theirs, skipped })
}

/// Right after signing in: have the directory read the account's earlier records, then compare
/// the account's published keys with the ones set here (see [`AfterSignIn`]).
pub fn keybinds_after_sign_in(store_root: &Path) -> Result<AfterSignIn> {
    let info = atmosphere::status(store_root)?.ok_or_else(atmosphere::not_signed_in)?;
    // Without it the directory may not have keys published before it kept them; a publish then
    // can't land over them, since it names the version it replaces.
    directory::request_index(store_root, &info)?;
    let here = keybinds::load(store_root)?;
    let Some((fetched, theirs, skipped)) = published(&info.did)? else {
        let published_here = !here.keys.is_empty();
        if published_here {
            keybinds_publish(store_root)?;
        }
        return Ok(AfterSignIn::NonePublished { published_here });
    };
    if here.readable() == theirs.readable() {
        return Ok(AfterSignIn::Same);
    }
    if here.keys.is_empty() {
        keybinds::save(store_root, &theirs)?;
        return Ok(AfterSignIn::Loaded(theirs));
    }
    let from = Resolved { did: info.did, handle: info.handle };
    Ok(AfterSignIn::Differs(KeybindsFetched { from, uri: fetched.uri, theirs, skipped }))
}
