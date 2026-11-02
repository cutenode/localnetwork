//! What a server refusing a request's token says why: the auth-params of its `WWW-Authenticate`
//! challenges (RFC 7235 2.1), such as RFC 6750's `error="insufficient_scope"` and the `scope` it
//! needs, and Google's `ACCESS_TOKEN_SCOPE_INSUFFICIENT` in a JSON body.

use serde_json::Value;

/// A challenge: its scheme, and its auth-params (names in lowercase), in order.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Challenge {
    pub scheme: String,
    pub params: Vec<(String, String)>,
}

impl Challenge {
    /// The value of its auth-param `name`.
    pub fn param(&self, name: &str) -> Option<&str> {
        self.params.iter().find(|(n, _)| n == name).map(|(_, v)| v.as_str())
    }
}

/// Whether `c` is a character of a token (RFC 7230 3.2.6).
fn is_tchar(c: char) -> bool {
    c.is_ascii_alphanumeric() || "!#$%&'*+-.^_`|~".contains(c)
}

/// The challenges a `WWW-Authenticate` header's `value` makes: each a scheme, then auth-params
/// (`name=token` or `name="quoted string"`) separated by commas. A token68 (`Basic dGVzdA==`)
/// isn't understood, and what can't be read is skipped.
pub(super) fn challenges(value: &str) -> Vec<Challenge> {
    let mut all: Vec<Challenge> = Vec::new();
    let mut rest = value;
    loop {
        rest = rest.trim_start_matches([' ', '\t', ',']);
        let end = rest.find(|c| !is_tchar(c)).unwrap_or(rest.len());
        let (token, after) = rest.split_at(end);
        if token.is_empty() {
            match after.chars().next() {
                Some(c) => rest = &after[c.len_utf8()..],
                None => return all,
            }
            continue;
        }
        let after = after.trim_start_matches([' ', '\t']);
        let Some(value) = after.strip_prefix('=') else {
            all.push(Challenge { scheme: token.into(), params: Vec::new() });
            rest = after;
            continue;
        };
        let value = value.trim_start_matches([' ', '\t']);
        let (said, left) = match value.strip_prefix('"') {
            Some(quoted) => unquoted(quoted),
            None => {
                let end = value.find(|c| !is_tchar(c)).unwrap_or(value.len());
                (value[..end].to_string(), &value[end..])
            }
        };
        if let Some(c) = all.last_mut() {
            c.params.push((token.to_ascii_lowercase(), said));
        }
        rest = left;
    }
}

/// A quoted string's text, `quoted` being what follows its opening quote, and what follows its
/// closing one (none when it isn't closed).
fn unquoted(quoted: &str) -> (String, &str) {
    let mut text = String::new();
    let mut chars = quoted.char_indices();
    while let Some((i, c)) = chars.next() {
        match c {
            '"' => return (text, &quoted[i + 1..]),
            '\\' => text.extend(chars.next().map(|(_, c)| c)),
            c => text.push(c),
        }
    }
    (text, "")
}

/// The scopes (as the service names them, maybe none) a server refusing a request with `status`
/// says its token lacks, when that's why: a 401 or 403 whose challenge says
/// `error="insufficient_scope"` (with them in `scope`), or a 403 whose JSON `body` says
/// `ACCESS_TOKEN_SCOPE_INSUFFICIENT` (Google's), not just `insufficientPermissions`.
pub(super) fn insufficient_scope(status: u16, challenges: &[Challenge], body: &[u8]) -> Option<Vec<String>> {
    let said = challenges.iter().find(|c| c.param("error") == Some("insufficient_scope"));
    if let Some(c) = said.filter(|_| matches!(status, 401 | 403)) {
        return Some(c.param("scope").unwrap_or_default().split_whitespace().map(String::from).collect());
    }
    let json: Value = serde_json::from_slice(body).ok().filter(|_| status == 403)?;
    let details = json.pointer("/error/details").and_then(Value::as_array)?;
    let google =
        details.iter().any(|d| d.get("reason").and_then(Value::as_str) == Some("ACCESS_TOKEN_SCOPE_INSUFFICIENT"));
    google.then(Vec::new)
}

#[cfg(test)]
mod tests;
