use super::*;

#[test]
fn only_https_addresses_and_files_that_exist_are_opened() {
    assert!(web_address("https://bsky.social/oauth/authorize?client_id=a&request_uri=b").is_ok());
    assert!(web_address("HTTPS://bsky.social/").is_ok());
    for refused in [
        "http://bsky.social/",
        "file:///etc/passwd",
        "ms-msdt:/id PCWDiagnostic",
        "search-ms:query=x",
        "-a Calculator",
        "https://",
        "https://a.example/ x",
        "https://a.example/\u{1b}]8;;x",
        "https://a.example/\"&calc",
    ] {
        assert!(web_address(refused).is_err(), "{refused}");
        assert!(open_url(refused).is_err(), "{refused}");
    }
    let dir = tempfile::tempdir().unwrap();
    let err = open_file(&dir.path().join("missing.html")).unwrap_err();
    assert_eq!(err.code(), "not_found", "{err}");
    assert!(open_file(dir.path()).is_err(), "a folder isn't a file");
}

/// Only an `https:` address with something after the scheme, and nothing in it that could end it
/// early (spaces, control characters, quotes), is handed to the browser.
#[hegel::test(test_cases = 300)]
fn only_plain_https_addresses_go_to_the_browser(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let scheme = tc.draw(gs::sampled_from(vec!["https://", "HTTPS://", "http://", "file://", "javascript:", ""]));
    let rest = tc.draw(gs::text().max_size(12));
    let url = format!("{scheme}{rest}");
    let https = scheme.eq_ignore_ascii_case("https://");
    let clean = !url.chars().any(|c| c.is_whitespace() || c.is_control() || c == '"');
    assert_eq!(web_address(&url).is_ok(), https && !rest.is_empty() && clean, "{url:?}");
}

/// Nothing that isn't a plain `https:` address reaches the browser, and nothing but an existing
/// file reaches its app: each is refused before anything is started. (Only refused targets are
/// drawn, so a passing run starts nothing.)
#[hegel::test(test_cases = 100)]
#[hegel::explicit_test_case(scheme = "https://", rest = "a.example/ x".to_string(), kind = 0u8)]
#[hegel::explicit_test_case(scheme = "file://", rest = "etc/passwd".to_string(), kind = 1u8)]
#[hegel::explicit_test_case(scheme = "", rest = "".to_string(), kind = 2u8)]
fn only_addresses_and_files_that_can_be_opened_are_handed_over(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let scheme = tc.draw(gs::sampled_from(vec!["https://", "http://", "file://", "javascript:", "-a ", ""]));
    let rest = tc.draw(gs::text().max_size(10));
    // A missing file, a folder, an empty path.
    let kind = tc.draw(gs::integers::<u8>().max_value(2));
    let url = match scheme {
        // Something that ends it early: refused, whatever comes before.
        "https://" => format!("{scheme}{rest}\"x"),
        _ => format!("{scheme}{rest}"),
    };
    let refused = open_url(&url).unwrap_err();
    assert_eq!(refused.code(), "invalid", "{url:?}");
    assert!(refused.to_string().contains("only https:// addresses"), "{refused}");

    let dir = tempfile::tempdir().unwrap();
    let target = match kind {
        0 => dir.path().join(format!("missing {rest}.html").replace('/', "_").replace('\0', "")),
        1 => dir.path().to_path_buf(),
        _ => std::path::PathBuf::new(),
    };
    let err = open_file(&target).unwrap_err();
    match kind {
        2 => assert!(matches!(err, Error::Io { .. }), "an empty path: {err}"),
        _ => assert_eq!(err.code(), "not_found", "{target:?}: {err}"),
    }
}
