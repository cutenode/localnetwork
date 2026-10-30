use super::*;

/// The issue form: `.github/ISSUE_TEMPLATE/locallibrary-bug.yml`.
fn form() -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.github/ISSUE_TEMPLATE/locallibrary-bug.yml");
    std::fs::read_to_string(path).unwrap()
}

/// The form's field ids, and the options of the field `install`.
fn ids_and_install_options(form: &str) -> (Vec<String>, Vec<String>) {
    let ids: Vec<String> =
        form.lines().filter_map(|l| l.trim().strip_prefix("id: ")).map(|id| id.trim().to_string()).collect();
    let install = form.split("id: install").nth(1).unwrap();
    let options = install.split("options:").nth(1).unwrap();
    let options =
        options.lines().skip(1).map_while(|l| l.trim().strip_prefix("- ")).map(|o| o.trim().to_string()).collect();
    (ids, options)
}

fn report(install: Option<&'static str>, title: Option<&str>, message: Option<&str>) -> Report {
    Report {
        title: title.map(String::from),
        message: message.map(String::from),
        version: "locallibrary 0.1.0".into(),
        os: "macOS 26.1.0, Apple silicon".into(),
        install,
    }
}

/// The query's values by name, decoded.
fn query(url: &str) -> Vec<(String, String)> {
    let query = url.split_once('?').unwrap().1;
    query
        .split('&')
        .map(|kv| {
            let (k, v) = kv.split_once('=').unwrap();
            (k.to_string(), percent_encoding::percent_decode_str(v).decode_utf8().unwrap().into_owned())
        })
        .collect()
}

#[test]
fn every_field_filled_in_is_on_the_form_and_every_install_answer_is_one_of_its_options() {
    let (ids, options) = ids_and_install_options(&form());
    assert!(ids.contains(&"what-happened".to_string()) && options.len() == 4, "{ids:?} {options:?}");
    let full = report(Some(HOMEBREW), Some("t"), Some("m"));
    let filled: Vec<&str> = full.fields().into_iter().map(|(id, _)| id).collect();
    assert_eq!(filled, ["version", "os", "install", "title", "what-happened"]);
    // `title` is the issue's own title, not a field of the form.
    for id in filled.iter().filter(|id| **id != "title") {
        assert!(ids.iter().any(|i| i == id), "{id} isn't a field of the form: {ids:?}");
    }
    for answer in [CARGO_INSTALL, INSTALLER, HOMEBREW] {
        assert!(options.iter().any(|o| o == answer), "{answer:?} isn't one of {options:?}");
    }
}

#[test]
fn the_form_is_filled_in_with_what_was_typed_and_this_computer() {
    let url = report(None, None, None).github_url();
    assert!(url.starts_with("https://github.com/cutenode/localnetwork/issues/new?template=locallibrary-bug.yml&"));
    assert_eq!(
        query(&url),
        [
            ("template".into(), "locallibrary-bug.yml".into()),
            ("version".into(), "locallibrary 0.1.0".into()),
            ("os".into(), "macOS 26.1.0, Apple silicon".into()),
        ]
    );
    // Text that means something in an address arrives as it was typed.
    let typed = "Sync & eject: 100% stuck #3 ?x=1+2 日本 🇬🇧";
    let url = report(Some(CARGO_INSTALL), Some(typed), Some("line one\nline two")).github_url();
    let q = query(&url);
    assert_eq!(q[3], ("install".into(), CARGO_INSTALL.into()));
    assert_eq!(q[4], ("title".into(), typed.into()));
    assert_eq!(q[5], ("what-happened".into(), "line one\nline two".into()));
    assert!(!url.contains(' ') && !url.contains('\n'), "{url}");
}

#[test]
fn a_long_report_is_cut_short_to_fit_an_address() {
    let long = "é".repeat(5000);
    let url = report(None, Some("short"), Some(&long)).github_url();
    assert!(url.len() <= MAX_URL && url.len() > MAX_URL - 6, "{}", url.len());
    let q = query(&url);
    assert_eq!(q[3], ("title".into(), "short".into()));
    // Cut between characters, never inside one.
    let said = &q[4].1;
    assert!(said.len() > 2000 && long.starts_with(said.as_str()), "{}", said.len());
}

/// However long the title and the message, the address fits in [`MAX_URL`]: each field it has is
/// the start of what was typed, in the form's order, and a field whose name doesn't fit is left
/// out.
#[hegel::test(test_cases = 200)]
#[hegel::explicit_test_case(title_len = 9000usize, message_len = 1usize, piece = "a")]
fn the_address_never_runs_past_its_limit(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let title_len = tc.draw(gs::integers::<usize>().max_value(9000));
    let message_len = tc.draw(gs::integers::<usize>().max_value(9000));
    let piece = tc.draw(gs::sampled_from(&["a", "é", "日", "🇬🇧", "& ="][..]));
    let (title, message) = (piece.repeat(title_len), piece.repeat(message_len));
    let url = report(Some(HOMEBREW), Some(&title), Some(&message)).github_url();
    assert!(url.len() <= MAX_URL, "{}", url.len());
    let q = query(&url);
    let ids: Vec<&str> = q.iter().map(|(k, _)| k.as_str()).collect();
    let all = ["template", "version", "os", "install", "title", "what-happened"];
    assert!(all.starts_with(&ids), "{ids:?}");
    for ((_, said), typed) in q.iter().skip(4).zip([&title, &message]) {
        assert!(typed.starts_with(said.as_str()), "{said:?}");
    }
}

#[cfg(feature = "account")]
#[test]
fn a_post_says_what_happened_then_the_details() {
    let r = report(Some(INSTALLER), Some("t"), Some("  It stopped.\n "));
    assert_eq!(
        r.body(),
        "It stopped.\n\nVersion: locallibrary 0.1.0\nOperating system: macOS 26.1.0, Apple silicon\nInstalled: Installer script"
    );
    assert_eq!(
        report(None, None, None).body(),
        "Version: locallibrary 0.1.0\nOperating system: macOS 26.1.0, Apple silicon"
    );
}

#[test]
fn how_it_was_installed_is_told_by_where_the_binary_is() {
    let dir = tempfile::tempdir().unwrap();
    let at = |rel: &str| dir.path().join(rel);
    assert_eq!(install_method(&at("opt/homebrew/Cellar/locallibrary/0.1.0/bin/locallibrary")), Some(HOMEBREW));
    // `cargo install` lists what it installed; the installer script doesn't.
    std::fs::create_dir_all(at("cargo/bin")).unwrap();
    let listing = json!({ "installs": { "locallibrary-cli 0.1.0 (path+file:///src)": { "bins": ["locallibrary"] } } });
    std::fs::write(at("cargo/.crates2.json"), listing.to_string()).unwrap();
    assert_eq!(install_method(&at("cargo/bin/locallibrary")), Some(CARGO_INSTALL));
    assert_eq!(install_method(&at("cargo/bin/locallibrary.exe")), Some(CARGO_INSTALL));
    assert_eq!(install_method(&at(".cargo/bin/locallibrary")), Some(INSTALLER));
    let others = json!({ "installs": { "ripgrep 14.1.1 (registry+https://github.com/rust-lang/crates.io-index)": { "bins": ["rg"] } } });
    std::fs::create_dir_all(at(".cargo/bin")).unwrap();
    std::fs::write(at(".cargo/.crates2.json"), others.to_string()).unwrap();
    assert_eq!(install_method(&at(".cargo/bin/locallibrary")), Some(INSTALLER));
    // Anywhere else (an unpacked release archive, a build): it can't tell.
    for elsewhere in [
        "Downloads/locallibrary-aarch64-apple-darwin/locallibrary",
        "src/target/debug/locallibrary",
        "usr/local/bin/locallibrary",
    ] {
        assert_eq!(install_method(&at(elsewhere)), None, "{elsewhere}");
    }
}

#[test]
fn the_system_is_named_with_its_version_and_processor() {
    let os = os();
    let (name, arch) = os.rsplit_once(", ").unwrap();
    assert!(!name.is_empty(), "{os}");
    if cfg!(target_os = "macos") {
        assert!(name.starts_with("macOS ") && ["Apple silicon", "Intel"].contains(&arch), "{os}");
    } else {
        assert_eq!(arch, std::env::consts::ARCH);
    }
}

#[test]
fn each_system_is_named_the_way_the_form_asks() {
    use os_info::{Type, Version};
    let semantic = Version::Semantic(26, 1, 0);
    assert_eq!(os_label(Type::Macos, &semantic, "macos", "aarch64"), "macOS 26.1.0, Apple silicon");
    assert_eq!(os_label(Type::Macos, &semantic, "macos", "x86_64"), "macOS 26.1.0, Intel");
    assert_eq!(os_label(Type::Windows, &Version::Custom("11".into()), "windows", "aarch64"), "Windows 11, aarch64");
    assert_eq!(os_label(Type::Linux, &Version::Unknown, "linux", "x86_64"), "Linux, x86_64");
}

#[cfg(feature = "account")]
#[test]
fn a_typed_title_is_one_line_and_nothing_typed_is_none() {
    assert_eq!(ask("", &mut "  The map is grey  \nmore\n".as_bytes()).unwrap().as_deref(), Some("The map is grey"));
    assert_eq!(ask("", &mut "   \n".as_bytes()).unwrap(), None);
    assert_eq!(ask("", &mut "".as_bytes()).unwrap(), None);
}

#[cfg(feature = "account")]
#[test]
fn a_description_runs_to_the_first_empty_line() {
    let typed = "I added a venue\nand the map stayed grey\n\nnot this\n";
    assert_eq!(
        ask_lines("", &mut typed.as_bytes()).unwrap().as_deref(),
        Some("I added a venue\nand the map stayed grey")
    );
    assert_eq!(ask_lines("", &mut "\nlater\n".as_bytes()).unwrap(), None);
    assert_eq!(ask_lines("", &mut "only line".as_bytes()).unwrap().as_deref(), Some("only line"));
}
