use hegel::TestCase;
use hegel::generators as gs;

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

/// Text as typed: any characters (those that mean something in an address too), sometimes many
/// times over, so the form's address has to cut it short.
#[hegel::test_helper]
fn typed(tc: &TestCase) -> Option<String> {
    let text = tc.draw(gs::optional(gs::text().max_size(40)))?;
    Some(text.repeat(tc.draw(gs::sampled_from(vec![1, 1, 2, 400]))))
}

/// Any report fills in GitHub's form within [`MAX_URL`]: its fields in the form's order, each as
/// typed (decoded back) or, once there's no room left, its beginning, cut between characters.
/// Only the last field is cut: titles are a line long.
#[hegel::test(test_cases = 100)]
fn any_report_fills_in_the_form_within_an_address(tc: TestCase) {
    let install = tc.draw(gs::sampled_from(vec![None, Some(HOMEBREW), Some(CARGO_INSTALL), Some(INSTALLER)]));
    let title = tc.draw(gs::optional(gs::text().max_size(60)));
    let message = typed(&tc);
    let r = report(install, title.as_deref(), message.as_deref());
    let url = r.github_url();
    assert!(url.len() <= MAX_URL && url.starts_with(NEW_ISSUE), "{}", url.len());
    assert!(!url.contains([' ', '\n', '#']), "{url}");
    let q = query(&url);
    assert_eq!(q[0], ("template".into(), "locallibrary-bug.yml".into()));
    let fields = r.fields();
    assert_eq!(q.len(), fields.len() + 1, "{q:?}");
    let whole = NEW_ISSUE.len()
        + fields.iter().map(|(id, v)| id.len() + 2 + utf8_percent_encode(v, QUERY).to_string().len()).sum::<usize>();
    for ((id, value), (k, got)) in fields.iter().zip(&q[1..]) {
        assert_eq!(k, id);
        assert!(value.starts_with(got.as_str()), "{id}: {got:?} isn't the start of {value:?}");
    }
    // All of it when it fits; cut, the last field is as long as the room allowed.
    match whole <= MAX_URL {
        true => assert_eq!(url.len(), whole),
        false => assert!(url.len() > MAX_URL - 12 && q.last().unwrap().1.len() < fields.last().unwrap().1.len()),
    }
}

/// What's posted is what happened, trimmed, then a line for each detail: only the details when
/// nothing but space was typed.
#[cfg(feature = "account")]
#[hegel::test(test_cases = 100)]
fn any_post_says_what_happened_then_the_details(tc: TestCase) {
    let install = tc.draw(gs::sampled_from(vec![None, Some(HOMEBREW), Some(INSTALLER)]));
    let message = tc.draw(gs::optional(gs::text().max_size(30)));
    let pad = tc.draw(gs::sampled_from(vec!["", " ", "\n", " \t\n"]));
    let message = message.map(|m| format!("{pad}{m}{pad}"));
    let body = report(install, None, message.as_deref()).body();
    let details = "Version: locallibrary 0.1.0\nOperating system: macOS 26.1.0, Apple silicon".to_string();
    let details = match install {
        Some(i) => format!("{details}\nInstalled: {i}"),
        None => details,
    };
    match message.as_deref().map(str::trim).filter(|m| !m.is_empty()) {
        Some(m) => assert_eq!(body, format!("{m}\n\n{details}")),
        None => assert_eq!(body, details),
    }
}

/// Every system is named as the form asks: macOS by that name, others by os_info's, then its
/// version when it's known, then its processor, which on a Mac is Apple silicon or Intel.
#[hegel::test(test_cases = 50)]
fn any_system_is_named_with_its_version_and_processor(tc: TestCase) {
    use os_info::{Type, Version};
    let kind = tc.draw(gs::sampled_from(vec![Type::Macos, Type::Windows, Type::Ubuntu, Type::Linux, Type::FreeBSD]));
    let version = match tc.draw(gs::integers::<u8>().max_value(2)) {
        0 => Version::Unknown,
        1 => Version::Semantic(tc.draw(gs::integers()), tc.draw(gs::integers()), tc.draw(gs::integers())),
        _ => Version::Custom(tc.draw(gs::sampled_from(vec!["11", "24.04 LTS", "rolling"])).into()),
    };
    let os = tc.draw(gs::sampled_from(vec!["macos", "linux", "windows"]));
    let arch = tc.draw(gs::sampled_from(vec!["aarch64", "x86_64", "riscv64"]));
    let label = os_label(kind, &version, os, arch);
    let (named, processor) = label.rsplit_once(", ").unwrap();
    let name = if kind == Type::Macos { "macOS".to_string() } else { kind.to_string() };
    match &version {
        Version::Unknown => assert_eq!(named, name),
        v => assert_eq!(named, format!("{name} {v}")),
    }
    let mac = match arch {
        "aarch64" => "Apple silicon",
        "x86_64" => "Intel",
        other => other,
    };
    assert_eq!(processor, if os == "macos" { mac } else { arch });
}

/// Wherever the binary is: in Homebrew's Cellar it's Homebrew's; in a `bin` folder whose
/// `.crates2.json` lists it (by its name, or without `.exe`) `cargo install`'s; in `.cargo/bin`
/// otherwise the installer script's; anywhere else, or beside a listing that isn't one, it can't
/// tell.
#[hegel::test(test_cases = 60)]
fn where_the_binary_is_tells_how_it_was_installed(tc: TestCase) {
    let dir = tempfile::tempdir().unwrap();
    let cellar = tc.draw(gs::weighted_booleans(0.2));
    let root_name = tc.draw(gs::sampled_from(vec![".cargo", "cargo", "opt"]));
    let bin_name = tc.draw(gs::sampled_from(vec!["bin", "bin", "debug"]));
    let exe_name = tc.draw(gs::sampled_from(vec!["locallibrary", "locallibrary.exe", "other"]));
    let listing = tc.draw(gs::sampled_from(vec!["none", "ours", "others", "broken"]));
    let root = match cellar {
        true => dir.path().join("homebrew/Cellar/locallibrary/0.1.0"),
        false => dir.path().join(root_name),
    };
    std::fs::create_dir_all(root.join(bin_name)).unwrap();
    let bins = match listing {
        "ours" => Some(json!(["locallibrary"]).to_string()),
        "others" => Some(json!(["rg"]).to_string()),
        "broken" => Some("[".to_string()),
        _ => None,
    };
    if let Some(bins) = bins {
        let listed = format!(r#"{{ "installs": {{ "some-crate 1.0.0": {{ "bins": {bins} }} }} }}"#);
        std::fs::write(root.join(".crates2.json"), listed).unwrap();
    }
    let exe = root.join(bin_name).join(exe_name);
    let listed = listing == "ours" && exe_name != "other";
    let wanted = match () {
        _ if cellar => Some(HOMEBREW),
        _ if bin_name != "bin" => None,
        _ if listed => Some(CARGO_INSTALL),
        _ if root_name == ".cargo" => Some(INSTALLER),
        // Unless the tests run with that folder as $CARGO_HOME, which a temporary one never is.
        _ => None,
    };
    assert_eq!(install_method(&exe), wanted, "{}", exe.display());
}

/// Typed in answer: a title is the first line, trimmed (none when it's empty); what happened is
/// the lines up to the first empty one (none when that's the first), as typed.
#[cfg(feature = "account")]
#[hegel::test(test_cases = 100)]
fn typed_answers_run_to_the_first_empty_line(tc: TestCase) {
    let line = gs::sampled_from(vec!["", "  ", "Sync stops", "  at 99% ", "日本 🇬🇧"]);
    let lines: Vec<&str> = tc.draw(gs::vecs(line).max_size(5));
    let typed = lines.iter().map(|l| format!("{l}\n")).collect::<String>();
    let title = lines.first().map(|l| l.trim()).filter(|l| !l.is_empty());
    assert_eq!(ask("", &mut typed.as_bytes()).unwrap().as_deref(), title);
    let told: Vec<&str> = lines.iter().copied().take_while(|l| !l.trim().is_empty()).collect();
    let told = Some(told.join("\n")).filter(|t| !t.is_empty());
    assert_eq!(ask_lines("", &mut typed.as_bytes()).unwrap(), told);
}
