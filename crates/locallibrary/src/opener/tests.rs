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
