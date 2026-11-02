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
