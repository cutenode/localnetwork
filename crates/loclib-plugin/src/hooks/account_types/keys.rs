//! Account types signed in to with keys (`kind = "keys"`): the keys the user enters, which the
//! service gave them, and where they go in a request, as each place's value names them.

use rustc_hash::FxHashSet;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::{AccountType, KEYS_HEADER};
use crate::ManifestError;
use crate::hooks::accounts::{QUERY_NAME_RULE, endpoints, is_query_name};
use crate::manifest::{NAME_RULE, invalid, is_name, shown};

/// `[contributes.account_types.keys.<name>]`: a key the user enters.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Key {
    /// What it's called where the user gets it (`S3 access key`).
    pub title: String,
}

/// `{ header = "Authorization", value = "LOW {access}:{secret}" }` or `{ query = "accesskey", value
/// = "{access}" }`: where an account goes in a request (the `Authorization` header, or a query
/// parameter), and what goes there, each of its keys named in braces.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct KeyPlace {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub header: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
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
/// in the browser; a key at least, each a name with a title; where the user gets them; and a place
/// or more, each the `Authorization` header or a query parameter, once, its value printable ASCII
/// naming some of the keys and nothing else in braces, every key named somewhere.
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
    if x.goes.is_empty() {
        let why = "an account goes in a request somewhere: the Authorization header or a query parameter";
        return Err(invalid(format!("{at}.goes"), why));
    }
    let mut used = FxHashSet::default();
    for (j, place) in x.goes.iter().enumerate() {
        let at = format!("{at}.goes[{j}]");
        match (&place.header, &place.query) {
            (Some(header), None) if header != KEYS_HEADER => {
                let why = format!("{header:?} isn't {KEYS_HEADER}, the only header keys go in");
                return Err(invalid(format!("{at}.header"), why));
            }
            (None, Some(query)) if !is_query_name(query) => {
                return Err(invalid(format!("{at}.query"), format!("{query:?} isn't {QUERY_NAME_RULE}")));
            }
            (Some(_), None) | (None, Some(_)) => {}
            _ => return Err(invalid(at, "goes in the Authorization header or a query parameter, one of them")),
        }
        if let Some(i) = x.goes[..j].iter().position(|p| (&p.header, &p.query) == (&place.header, &place.query)) {
            return Err(invalid(at, format!("goes where goes[{i}] does: a place has one value")));
        }
        let value = &place.value;
        if !value.bytes().all(|b| (b' '..=b'~').contains(&b)) {
            return Err(invalid(format!("{at}.value"), format!("{value:?} isn't printable ASCII")));
        }
        let names = named(value).ok_or_else(|| {
            invalid(format!("{at}.value"), format!("{value:?} has a {{ or }} that doesn't name a key"))
        })?;
        if names.is_empty() {
            return Err(invalid(format!("{at}.value"), format!("{value:?} names none of its keys, like {{key}}")));
        }
        if let Some(name) = names.iter().find(|n| !x.keys.contains_key(**n)) {
            let why = format!("{value:?} names {{{name}}}, which isn't one of its keys");
            return Err(invalid(format!("{at}.value"), why));
        }
        used.extend(names);
    }
    match x.keys.keys().find(|k| !used.contains(k.as_str())) {
        Some(name) => Err(invalid(format!("{at}.keys.{name}"), "goes nowhere: a place's value names it")),
        None => Ok(()),
    }
}
