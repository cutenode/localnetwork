use super::*;

#[test]
fn only_https_addresses_and_files_that_exist_are_opened() {
    let authorize = "https://bsky.social/oauth/authorize?client_id=a&request_uri=b";
    assert_eq!(web_address(authorize).unwrap(), authorize);
    // As parsed: the scheme and host in lower case, a path's other characters escaped.
    assert_eq!(web_address("HTTPS://Bsky.Social").unwrap(), "https://bsky.social/");
    assert_eq!(web_address("https://bsky.app/caf\u{e9}'s").unwrap(), "https://bsky.app/caf%C3%A9's");
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
        // Regressions: these began with `https://`, so they went to the browser.
        "https://@",
        "https://me@a.example/",
        "https://me:secret@a.example/",
        "https://:secret@a.example/",
        "https://a.example@localhost/",
        "https://localhost:8080/",
        "https://127.0.0.1/",
        "https://192.168.1.1/admin",
        "https://[::1]/",
        "https://a b/",
        "https://%zz/",
    ] {
        assert!(web_address(refused).is_err(), "{refused}");
        assert!(open_url(refused).is_err(), "{refused}");
    }
    let dir = tempfile::tempdir().unwrap();
    let err = open_file(&dir.path().join("missing.html")).unwrap_err();
    assert_eq!(err.code(), "not_found", "{err}");
    assert!(open_file(dir.path()).is_err(), "a folder isn't a file");
    let password = web_address("https://me:secret@a.example/").unwrap_err().to_string();
    assert!(password.contains("user name or password") && !password.contains("secret"), "{password}");
}

/// Only an `https:` address on the internet, without a user name or password and with nothing in
/// it that could end it early (spaces, control characters, quotes), is handed to the browser, as
/// the URL parser writes it out.
#[hegel::test(test_cases = 300)]
fn only_plain_https_addresses_go_to_the_browser(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let scheme = tc.draw(gs::sampled_from(vec!["https://", "HTTPS://", "http://", "file://", "javascript:", ""]));
    let user = tc.draw(gs::sampled_from(vec!["", "@", "me@", "me:pw@", ":pw@"]));
    let host =
        tc.draw(gs::sampled_from(vec!["bsky.app", "8.8.8.8", "localhost", "127.0.0.1", "10.0.0.1", "[fe80::1]"]));
    let rest = tc.draw(gs::text().max_size(12));
    let url = format!("{scheme}{user}{host}/{rest}");
    let https = scheme.eq_ignore_ascii_case("https://");
    let clean = !url.chars().any(|c| c.is_whitespace() || c.is_control() || c == '"');
    let public = matches!(host, "bsky.app" | "8.8.8.8");
    let given = web_address(&url);
    assert_eq!(given.is_ok(), https && clean && matches!(user, "" | "@") && public, "{url:?}: {given:?}");
    if let Ok(given) = given {
        assert_eq!(given, reqwest::Url::parse(&url).unwrap().as_str(), "{url:?}");
    }
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

#[cfg(unix)]
use crate::format::tests::fake_tools;

/// The system's opener gets exactly what was asked as its one argument (an address as parsed, a
/// file by its absolute path, whatever its name), and nothing for a file that isn't there; when
/// the opener fails, the error names it and how it ended.
#[cfg(unix)]
#[hegel::test(test_cases = 20)]
fn the_opener_gets_exactly_what_was_asked(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let mut fake = fake_tools::FakeTools::new();
    let program = if cfg!(target_os = "macos") { "open" } else { "xdg-open" };
    let name: String = tc.draw(gs::text().alphabet("ab -'\"$é").min_size(1).max_size(10));
    let kind = tc.draw(gs::sampled_from(vec!["address", "file", "missing file"]));
    let target = match kind {
        "address" => format!("https://bsky.app/{}", name.replace([' ', '"'], "_")),
        _ => fake.dir().join(&name).display().to_string(),
    };
    if kind == "file" {
        std::fs::write(&target, "").unwrap();
    }
    // An address goes as parsed, `é` escaped.
    let arg = match kind {
        "address" => reqwest::Url::parse(&target).unwrap().to_string(),
        _ => target.clone(),
    };
    let opens = tc.draw(gs::booleans());
    if opens {
        fake.answer(program, &[&arg], "");
    } else {
        fake.refuse(program, &[&arg], "no application knows how to open it");
    }

    let opened = if kind == "address" { open_url(&target) } else { open_file(Path::new(&target)) };
    let ran = fake.calls();
    if kind == "missing file" {
        assert!(matches!(opened, Err(Error::NotFound(_))) && ran.is_empty(), "{opened:?}");
        return;
    }
    assert_eq!(ran, [crate::format::Cmd::new(program, &[&arg])]);
    match opened {
        Ok(()) => assert!(opens),
        Err(Error::External { command, message }) => {
            assert!(!opens && command == program && message.contains("exit status: 1"))
        }
        Err(e) => panic!("{e}"),
    }
}
