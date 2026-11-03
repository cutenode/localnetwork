//! Account types signed in to with keys (`kind = "keys"`): the keys the user enters, which the
//! service gave them, and the `Authorization` header's value they make in a request, which names
//! them.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::AccountType;
use crate::ManifestError;
use crate::hooks::accounts::endpoints;
use crate::manifest::{NAME_RULE, invalid, is_name, shown};

/// `[contributes.account_types.keys.<name>]`: a key the user enters.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Key {
    /// What it's called where the user gets it (`S3 access key`).
    pub title: String,
}

/// Where an account goes in a request (the `Authorization` header, or a query parameter), and
/// what goes there, each key or the token it's made of named in braces: see
/// [`AccountType::place`].
#[derive(Debug, Clone, PartialEq)]
pub struct KeyPlace {
    pub header: Option<String>,
    pub query: Option<String>,
    pub value: String,
}

impl KeyPlace {
    /// Its value with each key it names as `key` gives it.
    pub fn filled<'a>(&self, key: impl Fn(&str) -> &'a str) -> String {
        let mut rest = self.value.as_str();
        let mut out = String::with_capacity(rest.len());
        while let Some((before, after)) = rest.split_once('{') {
            let (name, after) = after.split_once('}').unwrap_or((after, ""));
            out.push_str(before);
            out.push_str(key(name));
            rest = after;
        }
        out.push_str(rest);
        out
    }
}

/// The keys `value` names in braces, in its order: `None` when a brace names none (left open,
/// closed alone, or around what isn't a name).
fn named(value: &str) -> Option<Vec<&str>> {
    let mut parts = value.split('{');
    if parts.next().is_some_and(|first| first.contains('}')) {
        return None;
    }
    parts
        .map(|part| {
            let (name, after) = part.split_once('}')?;
            (is_name(name) && !after.contains('}')).then_some(name)
        })
        .collect()
}

/// `x` (at `at`) an account type signed in to with keys: none of the fields of one signed in to
/// in the browser; a key at least, each a name with a title; where the user gets them; and the
/// `Authorization` header's value they make, printable ASCII naming every key and nothing else in
/// braces.
pub(super) fn validate(at: &str, x: &AccountType) -> Result<(), ManifestError> {
    let browser = [
        ("authorize_url", x.authorize_url.is_some()),
        ("token_url", x.token_url.is_some()),
        ("token_exchange", x.token_exchange.is_some()),
        ("client_id", x.client_id.is_some()),
        ("revoke_url", x.revoke_url.is_some()),
        ("token", !x.token.is_bearer()),
        ("redirect_port", x.redirect_port.is_some()),
        ("always", !x.always.is_empty()),
        ("one_account", !x.one_account.is_empty()),
        ("another_account", !x.another_account.is_empty()),
        ("params", !x.params.is_empty()),
    ];
    if let Some((key, _)) = browser.iter().find(|(_, set)| *set) {
        return Err(invalid(
            format!("{at}.{key}"),
            "is for an account type signed in to in the browser, not with keys",
        ));
    }
    if x.keys.is_empty() {
        return Err(invalid(format!("{at}.keys"), "an account type signed in to with keys has a key at least"));
    }
    for (name, key) in &x.keys {
        if !is_name(name) {
            return Err(invalid(format!("{at}.keys.{name}"), format!("isn't a name: {NAME_RULE}")));
        }
        shown(format!("{at}.keys.{name}.title"), &key.title)?;
    }
    let keys_url = x.keys_url.as_deref().ok_or_else(|| invalid(format!("{at}.keys_url"), "is missing"))?;
    endpoints(at, [("keys_url", Some(keys_url))])?;
    let value = x.authorization.as_deref().ok_or_else(|| invalid(format!("{at}.authorization"), "is missing"))?;
    let wrong = |why: String| invalid(format!("{at}.authorization"), why);
    if !value.bytes().all(|b| (b' '..=b'~').contains(&b)) {
        return Err(wrong(format!("{value:?} isn't printable ASCII")));
    }
    let names = named(value).ok_or_else(|| wrong(format!("{value:?} has a {{ or }} that doesn't name a key")))?;
    if names.is_empty() {
        return Err(wrong(format!("{value:?} names none of its keys, like {{key}}")));
    }
    if let Some(name) = names.iter().find(|n| !x.keys.contains_key(**n)) {
        return Err(wrong(format!("{value:?} names {{{name}}}, which isn't one of its keys")));
    }
    match x.keys.keys().find(|k| !names.contains(&k.as_str())) {
        Some(name) => Err(invalid(format!("{at}.keys.{name}"), "goes nowhere: authorization doesn't name it")),
        None => Ok(()),
    }
}
