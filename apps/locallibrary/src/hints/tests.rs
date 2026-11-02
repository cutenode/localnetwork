use serde_json::json;

use super::*;

#[test]
fn actions_become_commands_and_flags() {
    let e = locallibrary::Error::NotFound("library \"Club\" in the store".into())
        .action(Action::call("pull", json!({ "drive": "/Volumes/My DNA" }), "Pull DNA into the store"))
        .action(Action::retry(json!({ "force": true, "library": null }), "Force it"))
        .action(Action::call(
            "sync.plan",
            json!({ "left": "/Volumes/A", "right": "/Volumes/B", "direction": "right_to_left" }),
            "Copy B over A",
        ))
        .action(Action::call("something.new", json!({}), "Something new"));
    assert_eq!(
        hints(&e),
        [
            "Pull DNA into the store: locallibrary pull \"/Volumes/My DNA\"",
            "Force it: add --force --library <LIBRARY>",
            "Copy B over A: locallibrary sync --from /Volumes/B /Volumes/A",
            "Something new",
        ]
    );
    let admin = locallibrary::Error::Permission("formatting /dev/disk4 needs administrator rights".into());
    assert!(hints(&admin)[0].contains("run it again"), "{:?}", hints(&admin));
}

#[test]
fn a_switch_is_a_flag_without_a_value() {
    let e = locallibrary::Error::Account("the system keychain isn't available".into())
        .action(Action::retry(json!({ "session_file": true }), "Keep the session in a file"));
    assert_eq!(hints(&e), ["Keep the session in a file: add --session-file"]);
}

/// Values become flags as typed: a word as it is, a number bare, an unset switch left out; and
/// a method's own spelling of a value becomes the command line's.
#[test]
fn values_are_spelled_as_the_command_line_takes_them() {
    let e = locallibrary::Error::Conflict("the drives differ".into())
        .action(Action::retry(json!({ "folder_name": "From B", "limit": 3, "dry_run": false }), "Retry"))
        .action(Action::call("sync.plan", json!({ "left": "A", "right": "B" }), "Sync"))
        .action(Action::call("drive.role", json!({ "role": "backup" }), "Make it a backup"))
        .action(Action::call("convert.plan", json!({ "drive": "A", "to": "device_library" }), "Rebuild"))
        .action(Action::call("convert.plan", json!({ "drive": "A", "to": "one_library" }), "Convert"))
        .action(Action::call("convert.plan", json!({ "drive": "A", "to": "one_library", "replace": true }), "Redo"));
    assert_eq!(
        hints(&e),
        [
            "Retry: add --folder \"From B\" --limit 3",
            "Sync: locallibrary sync A B",
            "Make it a backup: locallibrary drive role <DRIVE> backup",
            "Rebuild: locallibrary convert A --to device-library",
            "Convert: locallibrary convert A --to onelibrary",
            "Redo: locallibrary convert A --to onelibrary --replace",
        ]
    );
}

/// `line`'s words as a shell reads them, expanding what it expands: what `printf` is handed.
#[cfg(unix)]
fn sh_words(line: &str) -> Vec<String> {
    let out = std::process::Command::new("sh").arg("-c").arg(format!("printf '%s\\n' {line}")).output().unwrap();
    assert!(out.status.success(), "{line}: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8(out.stdout).unwrap().lines().map(String::from).collect()
}

/// `line`'s words: split as a shell splits them, and on Unix read the same by a real one, so
/// nothing in them expands.
fn shell_words(line: &str) -> Vec<String> {
    let words = shlex::split(line).unwrap_or_else(|| panic!("{line} doesn't split"));
    #[cfg(unix)]
    if !line.contains('<') {
        assert_eq!(sh_words(line), words, "a shell reads {line} otherwise");
    }
    words
}

/// Drives, libraries and labels as people name them: with spaces, quotes, backslashes and
/// what a shell would expand.
const VALUES: &[&str] = &[
    "A",
    "/Volumes/My DNA",
    "Club \"Night\"",
    "back\\slash",
    "rekordbox:usb",
    "Ünïcödé",
    "a$b",
    "`x`",
    "DJ's USB",
    "Wow!",
];

/// Every hint that names a command is one the command line reads, with each value given
/// back as it was (quoted where a shell needs it); one that adds to the request names a flag
/// for each parameter it adds, and leaves out a switch that's off.
#[hegel::test(test_cases = 200)]
fn hints_are_command_lines_that_parse(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let methods = [
        "pull",
        "push.plan",
        "sync.plan",
        "merge.plan",
        "undo.plan",
        "undo.list",
        "drive.backups",
        "drive.role",
        "drive.retire",
        "drive.reidentify",
        "snapshot.label",
        "convert.plan",
        #[cfg(feature = "account")]
        "account.login.begin",
        "accounts.sign_in.begin",
        "accounts.access.set_default",
        "settings.schema",
        "settings.import.plan",
        "library.list",
    ];
    let value = |tc: &hegel::TestCase| tc.draw(gs::sampled_from(VALUES));
    let mut params = Map::new();
    for key in ["drive", "left", "right", "source", "target", "label", "type", "with", "for", "plugin"] {
        if tc.draw(gs::weighted_booleans(0.8)) {
            params.insert(key.into(), value(&tc).into());
        }
    }
    params.insert("account".into(), serde_json::json!({ "plugin": value(&tc), "subject": value(&tc) }));
    let scopes: Vec<&str> = tc.draw(gs::vecs(gs::sampled_from(VALUES)).max_size(2));
    params.insert("scopes".into(), scopes.clone().into());
    params.insert("role".into(), tc.draw(gs::sampled_from(&["backup", "primary", "archive"][..])).into());
    params.insert("to".into(), tc.draw(gs::sampled_from(&["one_library", "device_library"][..])).into());
    let direction = tc.draw(gs::sampled_from(&["left_to_right", "right_to_left", "either"][..]));
    params.insert("direction".into(), direction.into());
    for key in ["replace", "undo", "remove", "publish"] {
        params.insert(key.into(), tc.draw(gs::booleans()).into());
    }
    let method = tc.draw(gs::sampled_from(&methods[..]));
    // Signing in again names the account by its subject.
    let again = method == "accounts.sign_in.begin" && tc.draw(gs::booleans());
    if again {
        params.insert("account".into(), value(&tc).into());
    }
    let e =
        locallibrary::Error::Conflict("x".into()).action(Action::call(method, Value::Object(params.clone()), "Do it"));
    let hint = &hints(&e)[0];
    let Some(cmd) = hint.strip_prefix("Do it: ") else {
        assert_eq!(hint, "Do it", "no command, no colon");
        return;
    };
    let words = shell_words(cmd);
    assert_eq!(words[0], "locallibrary", "{cmd}");
    if !cmd.contains('<') {
        // Built once: it's the slow part.
        static COMMAND: std::sync::OnceLock<clap::Command> = std::sync::OnceLock::new();
        let parsed = COMMAND.get_or_init(crate::help::command).clone().try_get_matches_from(&words);
        assert!(parsed.is_ok(), "{cmd}: {}", parsed.err().map(|e| e.to_string()).unwrap_or_default());
    }
    // The values each command is made of.
    let used: &[&str] = match method {
        "sync.plan" => &["left", "right"],
        "merge.plan" => &["source", "target"],
        "snapshot.label" => &["label"],
        "accounts.sign_in.begin" if again => &["with", "for"],
        "accounts.sign_in.begin" => &["type", "with", "for"],
        "accounts.access.set_default" => &["plugin"],
        "undo.list" | "account.login.begin" | "settings.schema" | "settings.import.plan" | "library.list" => &[],
        _ => &["drive"],
    };
    let comes_back = |v: &str| assert!(words.iter().any(|w| w == v), "{v:?} doesn't come back from {cmd}: {words:?}");
    used.iter().filter_map(|key| params.get(*key).and_then(Value::as_str)).for_each(comes_back);
    if again {
        comes_back(&format!("{}:{}", params["type"].as_str().unwrap(), params["account"].as_str().unwrap()));
    }
    match method {
        "accounts.sign_in.begin" => scopes.iter().for_each(|s| comes_back(s)),
        "accounts.access.set_default" => comes_back(params["account"]["subject"].as_str().unwrap()),
        _ => {}
    }

    // The request that failed, again with more parameters.
    let mut extra = Map::new();
    for key in ["force", "dry_run", "library", "folder_name", "allow_low_confidence", "limit", "session_file"] {
        if tc.draw(gs::booleans()) {
            let v = match tc.draw(gs::integers::<u8>().max_value(3)) {
                0 => Value::Bool(true),
                1 => Value::Bool(false),
                2 => Value::Null,
                _ => value(&tc).into(),
            };
            extra.insert(key.into(), v);
        }
    }
    let e = locallibrary::Error::Conflict("x".into()).action(Action::retry(Value::Object(extra.clone()), "Again"));
    let hint = &hints(&e)[0];
    let on: Vec<(&String, &Value)> = extra.iter().filter(|(_, v)| **v != Value::Bool(false)).collect();
    if on.is_empty() {
        assert_eq!(hint, "Again");
        return;
    }
    let flags = hint.strip_prefix("Again: add ").unwrap_or_else(|| panic!("{hint}"));
    let words = shell_words(flags);
    for (key, v) in on {
        let flag = match key.as_str() {
            "allow_low_confidence" => "--allow-guess".to_string(),
            "folder_name" => "--folder".to_string(),
            k => format!("--{}", k.replace('_', "-")),
        };
        let at = words.iter().position(|w| *w == flag).unwrap_or_else(|| panic!("no {flag} in {hint}"));
        match v {
            Value::Bool(true) => {}
            Value::Null => assert_eq!(words[at + 1], format!("<{}>", key.to_uppercase()), "{hint}"),
            Value::String(s) => assert_eq!(&words[at + 1], s, "{hint}"),
            _ => unreachable!(),
        }
    }
    for (key, _) in extra.iter().filter(|(_, v)| **v == Value::Bool(false)) {
        assert!(!flags.contains(&key.replace('_', "-")), "{key} is off, yet in {hint}");
    }
}

/// An action that adds to the request that failed becomes `add` and a flag for each parameter
/// it adds, in order: a switch that's on bare and one that's off left out, a number as written,
/// a word as the shell reads it back, a value to give as its placeholder (a drive on its own,
/// with no flag); nothing to add is no command at all. One that needs administrator rights says
/// to run it again so, whatever else it carries.
#[hegel::test(test_cases = 200)]
fn retries_add_a_flag_for_each_parameter(tc: hegel::TestCase) {
    use hegel::generators as gs;
    let keys = ["force", "library", "folder_name", "allow_low_confidence", "limit", "drive", "dry_run"];
    let mut params = Map::new();
    for key in keys {
        let value = match tc.draw(gs::integers::<u8>().max_value(5)) {
            0 => continue,
            1 => Value::Bool(true),
            2 => Value::Bool(false),
            3 => Value::Null,
            4 => Value::from(tc.draw(gs::integers::<i64>().min_value(-5).max_value(500))),
            _ => Value::from(tc.draw(gs::sampled_from(VALUES))),
        };
        params.insert(key.into(), value);
    }
    // In the parameters' own order.
    let mut expected = Vec::new();
    for (key, value) in &params {
        let flag = match key.as_str() {
            "allow_low_confidence" => "--allow-guess".to_string(),
            "folder_name" => "--folder".to_string(),
            _ => format!("--{}", key.replace('_', "-")),
        };
        match value {
            Value::Bool(false) => {}
            Value::Bool(true) => expected.push(vec![flag]),
            Value::Null if key == "drive" => expected.push(vec!["<DRIVE>".to_string()]),
            Value::Null => expected.push(vec![flag, format!("<{}>", key.to_uppercase())]),
            Value::String(s) => expected.push(vec![flag, s.clone()]),
            other => expected.push(vec![flag, other.to_string()]),
        }
    }
    let elevate = tc.draw(gs::weighted_booleans(0.2));
    let a = Action { label: "Try again".into(), method: None, params, elevate };
    let hint = &hints(&locallibrary::Error::Conflict("x".into()).action(a))[0];
    if elevate {
        assert!(hint.starts_with("Try again: run it again "), "{hint}");
        return;
    }
    if expected.is_empty() {
        assert_eq!(hint, "Try again");
        return;
    }
    let cmd = hint.strip_prefix("Try again: add ").unwrap_or_else(|| panic!("{hint}"));
    // Placeholders aren't shell words: compare them apart from what the shell reads.
    let words: Vec<String> = match cmd.contains('<') {
        true => cmd.split(' ').map(String::from).collect(),
        false => shell_words(cmd),
    };
    if !cmd.contains('<') {
        assert_eq!(words, expected.concat(), "{cmd}");
    } else {
        assert_eq!(
            words.iter().filter(|w| w.starts_with("--")).count(),
            expected.iter().filter(|e| e[0].starts_with("--")).count(),
            "{cmd}"
        );
    }
}
