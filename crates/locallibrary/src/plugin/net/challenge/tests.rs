use hegel::TestCase;
use hegel::generators as gs;

use super::*;

/// A token: a scheme's or an auth-param's name, or a value that needs no quotes.
fn token() -> impl hegel::PrintableGenerator<String> {
    gs::from_regex("[A-Za-z0-9!#$%&'*+.^_`|~-]{1,8}").fullmatch(true)
}

/// For any challenges (each a scheme and auth-params, any value written as a token when it can be
/// or quoted, its quotes and backslashes escaped, with any spaces around `=` and between them),
/// what a `WWW-Authenticate` header says is read back as they were, names in lowercase.
#[hegel::test(test_cases = 200)]
fn a_challenge_reads_back_as_it_was_written(tc: TestCase) {
    let made: Vec<(String, Vec<(String, String)>)> = tc.draw(
        gs::vecs(gs::tuples!(
            token(),
            gs::vecs(gs::tuples!(token(), hegel::one_of!(token(), gs::text().max_size(12)))).max_size(3)
        ))
        .min_size(1)
        .max_size(3),
    );
    let space = || tc.draw(gs::sampled_from(vec!["", " ", "  ", "\t"]));
    let mut header = Vec::new();
    for (scheme, params) in &made {
        let params: Vec<String> = params
            .iter()
            .map(|(name, value)| {
                let quoted = value.is_empty() || !value.chars().all(is_tchar) || tc.draw(gs::booleans());
                let value = match quoted {
                    true => format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\"")),
                    false => value.clone(),
                };
                format!("{name}{}={}{value}", space(), space())
            })
            .collect();
        header.push(format!("{scheme} {}", params.join(&format!(",{}", space()))));
    }
    let header = header.join(", ");
    let read = challenges(&header);
    let expected: Vec<Challenge> = made
        .into_iter()
        .map(|(scheme, params)| Challenge {
            scheme,
            params: params.into_iter().map(|(n, v)| (n.to_ascii_lowercase(), v)).collect(),
        })
        .collect();
    assert_eq!(read, expected, "{header:?}");
}

/// A token's scopes are insufficient when a 401 or 403's challenge says so (naming them or not),
/// or a 403's JSON body says Google's `ACCESS_TOKEN_SCOPE_INSUFFICIENT`; not for another error, a
/// body saying only `insufficientPermissions`, or another status.
#[test]
fn insufficient_scope_is_read_from_the_challenge_or_googles_body() {
    let said = |status, header: &str, body: &str| insufficient_scope(status, &challenges(header), body.as_bytes());
    let scoped = r#"Bearer realm="x", error="insufficient_scope", scope="a b""#;
    assert_eq!(said(403, scoped, ""), Some(vec!["a".to_string(), "b".to_string()]));
    assert_eq!(said(401, r#"DPoP error="insufficient_scope""#, ""), Some(Vec::new()));
    assert_eq!(said(400, scoped, ""), None);
    assert_eq!(said(401, r#"Bearer error="invalid_token""#, ""), None);
    let google = r#"{"error":{"errors":[{"reason":"insufficientPermissions"}],"details":[{"reason":"ACCESS_TOKEN_SCOPE_INSUFFICIENT"}]}}"#;
    assert_eq!(said(403, "", google), Some(Vec::new()));
    assert_eq!(said(401, "", google), None);
    assert_eq!(said(403, "", r#"{"error":{"errors":[{"reason":"insufficientPermissions"}]}}"#), None);
    assert_eq!(said(403, "", "not json"), None);
}

/// For any header with what can't be read around a challenge (characters no token has, an
/// auth-param before any scheme, a quoted string left open), what can be is read: the rest
/// skipped, a param with no scheme dropped, and an open quote's text, its escapes taken, to the
/// end.
#[hegel::test(test_cases = 100)]
fn prop_what_cant_be_read_in_a_challenge_is_skipped(tc: TestCase) {
    let junk: String = tc.draw(gs::from_regex("[@()<>/?{}\\[\\];:]{1,3}").fullmatch(true));
    let (scheme, name) = (tc.draw(token()), tc.draw(token()));
    let value: String = tc.draw(gs::from_regex("[a-z \"\\\\]{0,8}").fullmatch(true)) + "\"";
    let escaped = value.replace('\\', "\\\\").replace('"', "\\\"");
    let header = format!("{junk} {name}=1, {junk} {scheme} {name}=\"{escaped}");
    let expected = Challenge { scheme, params: vec![(name.to_ascii_lowercase(), value)] };
    assert_eq!(challenges(&header), [expected], "{header:?}");
}

/// For any 403 whose JSON body gives details, the token's scopes are insufficient exactly when
/// one says Google's `ACCESS_TOKEN_SCOPE_INSUFFICIENT`, naming none; without details, they aren't.
#[hegel::test(test_cases = 50)]
fn prop_a_403_body_names_insufficient_scopes_by_its_reasons(tc: TestCase) {
    let all = vec!["ACCESS_TOKEN_SCOPE_INSUFFICIENT", "insufficientPermissions", "other"];
    let reasons: Vec<&str> = tc.draw(gs::vecs(gs::sampled_from(all)).max_size(3));
    let details: Vec<serde_json::Value> = reasons.iter().map(|r| serde_json::json!({ "reason": r })).collect();
    let body = serde_json::json!({ "error": { "details": details } }).to_string();
    let insufficient = reasons.contains(&"ACCESS_TOKEN_SCOPE_INSUFFICIENT");
    assert_eq!(insufficient_scope(403, &[], body.as_bytes()), insufficient.then(Vec::new));
    // Without details, it says nothing of scopes.
    let reason = reasons.first().copied().unwrap_or("other");
    let bare = serde_json::json!({ "error": { "errors": [{ "reason": reason }] } }).to_string();
    assert_eq!(insufficient_scope(403, &[], bare.as_bytes()), None);
}
